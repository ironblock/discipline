/**
 * The drive, served: a `DriveTransport` against `diet`'s session over HTTP
 * (`diet/src/drive/serve.rs`, #128; the binary is I5, `diet-drive serve`).
 *
 *   GET  /events?from=<seq>   the log from `seq`, then its tail, as server-sent
 *                             events: `id: <opened>-<seq>`, one v0 line per
 *                             `data:`. A resume (`Last-Event-ID`) naming
 *                             another process's log is refused 410.
 *   POST /commands            one command, JSON: `{"kind": "ask", "text"}`,
 *                             `{"kind": "cancel", "turn"}`, `{"kind":
 *                             "declare-seam"}`, `{"kind": "end"}`. A refusal is
 *                             409 `{"refused": <tag>}`, and logged.
 *
 * The page reaches it same-origin: in development Vite proxies both routes
 * (`vite.config.ts`, `DIET_DRIVE`), and `diet` is started with
 * `--allow-origin` naming the page's origin, whose host it then accepts as
 * the `Host` a proxy forwards.
 *
 * FINDING 17 (#117): an `EventSource` never exposes a status, so a closed
 * one cannot tell a restarted drive (410) from a refused page (403), a
 * missing credential (401) or a drive that is down. On close, one fetch of
 * the same resume learns the status: 410 rebuilds -- the log starts over
 * from 0, a new session -- and anything else is shown to the author, why
 * and all, never retried blind.
 */

import type { IdleGapBody } from '../session/gap.ts';
import type { LogLine } from './log.ts';
import type { Ack, Command, DriveTransport, Link } from './transport.ts';

/** What the transport needs from the browser: injectable, so a test can stand in for the server. */
export interface Web {
  readonly EventSource: new (url: string) => EventSourceLike;
  readonly fetch: (url: string, init?: RequestInit) => Promise<Response>;
}

/** The part of `EventSource` used here. */
export interface EventSourceLike {
  readonly readyState: number;
  onopen: ((event: Event) => void) | null;
  onmessage: ((event: MessageEvent<string>) => void) | null;
  onerror: ((event: Event) => void) | null;
  close(): void;
}

const CONNECTING = 0;
const CLOSED = 2;
/** Resumes after a probe answered 200, in a row, before the stream is called lost. */
const MAX_RETRIES = 3;

/** Why a closed stream is closed, from the status its resume was answered with. */
export function whyClosed(status: number | undefined): string {
  switch (status) {
    case undefined:
      return 'the drive cannot be reached';
    case 401:
      return 'the drive asks for credentials (401)';
    case 403:
      return "the drive refused this page: its origin or host is not allowed (start diet with --allow-origin naming this page's origin) (403)";
    case 404:
      return 'nothing serves a session here (404)';
    case 503:
      return 'the drive is at its connection limit, or down (503)';
    default:
      return `the drive answered ${status}`;
  }
}

export class HttpTransport implements DriveTransport {
  readonly #base: string;
  readonly #web: Web;

  readonly #log: LogLine[] = [];
  readonly #listeners = new Set<(line: LogLine) => void>();
  readonly #watchers = new Set<(link: Link, why?: string) => void>();
  #source: EventSourceLike | undefined;
  /** The last event id seen, `<opened>-<seq>`: what a resume names. */
  #lastId: string | undefined;
  /** The session this log is: its `opened`, from its first line. */
  #opened: number | undefined;
  /** Resumes in a row that received nothing: a stream the probe says is fine but will not stay open is lost, not retried forever. */
  #retries = 0;
  #link: Link = 'reconnecting';
  #why: string | undefined;
  /** Bumped by `close()`: a probe begun before it does nothing after. */
  #epoch = 0;

  constructor(base = '', web: Web = { EventSource: globalThis.EventSource, fetch: globalThis.fetch.bind(globalThis) }) {
    this.#base = base.replace(/\/$/, '');
    this.#web = web;
  }

  subscribe(listener: (line: LogLine) => void): () => void {
    for (const line of this.#log) listener(line);
    this.#listeners.add(listener);
    if (!this.#source) this.#connect();
    return () => this.#listeners.delete(listener);
  }

  watchLink(listener: (link: Link, why?: string) => void): () => void {
    listener(this.#link, this.#why);
    this.#watchers.add(listener);
    return () => this.#watchers.delete(listener);
  }

  /**
   * One command, and the idle gap it ends as `idle_gap` (#146): logged by
   * `diet` just before the command's outcome if the command is admitted,
   * dropped if it is refused. A gap `diet` cannot log turns the whole command
   * away (400, nothing logged) -- then the command goes again without it:
   * a measurement never costs a person their ask.
   */
  async dispatch(command: Command, extras?: { readonly idle_gap?: IdleGapBody }): Promise<Ack> {
    const plain = this.#body(command);
    if (typeof plain['refused'] === 'string') return { ok: false, refused: plain['refused'] };
    const gap = extras?.idle_gap;
    const first = await this.#post(gap ? { ...plain, idle_gap: gap } : plain);
    return gap && first.status === 400 ? (await this.#post(plain)).ack : first.ack;
  }

  /** One post, and what it came to: its status (0 when nothing answered), and the ack. */
  async #post(body: Readonly<Record<string, unknown>>): Promise<{ readonly status: number; readonly ack: Ack }> {
    let reply: Response;
    try {
      reply = await this.#web.fetch(`${this.#base}/commands`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    } catch {
      return { status: 0, ack: { ok: false, refused: 'unreachable' } };
    }
    if (reply.ok) return { status: reply.status, ack: { ok: true } };
    if (reply.status === 409) {
      const said = (await reply.json().catch(() => ({}))) as { readonly refused?: unknown };
      return { status: 409, ack: { ok: false, refused: typeof said.refused === 'string' ? said.refused : 'refused' } };
    }
    return { status: reply.status, ack: { ok: false, refused: `http-${reply.status}` } };
  }

  /** Stop listening. The log stays, and the next subscriber resumes from it (React mounts, unmounts and remounts). */
  close(): void {
    this.#epoch += 1;
    this.#source?.close();
    this.#source = undefined;
  }

  /** A command as `serve.rs` takes it; one the log cannot place yet is refused here. */
  #body(command: Command): Readonly<Record<string, unknown>> {
    switch (command.kind) {
      case 'ask':
        return { kind: 'ask', text: command.text };
      case 'cancel': {
        // A stop names the turn it is for: the latest asked.
        const turn = this.#log.findLast((l) => l.kind === 'ask');
        return turn?.kind === 'ask' ? { kind: 'cancel', turn: turn.turn } : { refused: 'nothing-in-flight' };
      }
      case 'seam':
        // v0's declare-seam takes no phase: the one it moves to is the drive's to say.
        return { kind: 'declare-seam' };
      case 'end':
        return { kind: 'end' };
    }
  }

  #connect(): void {
    const source = new this.#web.EventSource(`${this.#base}/events?from=${this.#log.length}`);
    this.#source = source;
    source.onopen = () => this.#setLink('live');
    // A source this transport has let go of -- closed, or replaced while a probe was out -- is not heard.
    source.onmessage = (event) => {
      if (source === this.#source) this.#receive(event);
    };
    source.onerror = () => {
      if (source !== this.#source) return;
      // Still trying: the browser retries a dropped stream itself, resuming from the last id.
      if (source.readyState === CONNECTING) return this.#setLink('reconnecting');
      if (source.readyState === CLOSED) void this.#probe();
    };
  }

  #receive(event: MessageEvent<string>): void {
    let line: LogLine;
    try {
      line = JSON.parse(event.data) as LogLine;
    } catch {
      return;
    }
    if (typeof line.seq !== 'number') return;
    // Every id names its session: one that is not this log's is another process's -- a drive restarted under a
    // resume that could not name the old one (a new EventSource sends no Last-Event-ID). Start over, from 0.
    const opened = event.lastEventId ? Number(event.lastEventId.split('-')[0]) : undefined;
    if (opened !== undefined && this.#opened !== undefined && opened !== this.#opened) return this.#restart();
    // Gapless, from 0: a line already held is a resume's overlap; one past the next is a gap the resume closes.
    if (line.seq !== this.#log.length) return;
    if (line.kind === 'session.start') this.#opened = line.opened;
    this.#lastId = event.lastEventId || this.#lastId;
    this.#retries = 0;
    this.#log.push(line);
    for (const listener of this.#listeners) listener(line);
  }

  /** Another session: drop this log and read the new one from its first line. */
  #restart(): void {
    this.#source?.close();
    this.#source = undefined;
    this.#log.length = 0;
    this.#lastId = undefined;
    this.#opened = undefined;
    this.#setLink('reconnecting', 'the drive restarted: a new session');
    this.#connect();
  }

  /** The stream closed for good: ask what it was answered with (finding 17). */
  async #probe(): Promise<void> {
    const source = this.#source;
    const epoch = this.#epoch;
    source?.close();
    this.#source = undefined;
    let status: number | undefined;
    const abort = new AbortController();
    try {
      const reply = await this.#web.fetch(`${this.#base}/events?from=${this.#log.length}`, {
        headers: this.#lastId ? { 'Last-Event-ID': this.#lastId } : {},
        signal: abort.signal,
      });
      status = reply.status;
    } catch {
      status = undefined;
    } finally {
      abort.abort();
    }
    // Closed since, or subscribed again while the probe was out (which connected): this probe's answer is stale.
    if (epoch !== this.#epoch || this.#source) return;
    // Another process's log: this session is gone. Start over, from the new one's first line.
    if (status === 410) return this.#restart();
    // Fine now: resume from the next line -- a few times, and then say so rather than loop.
    if (status === 200 && this.#retries < MAX_RETRIES) {
      this.#retries += 1;
      this.#setLink('reconnecting');
      this.#connect();
      return;
    }
    this.#setLink('lost', status === 200 ? 'the drive answers, but its stream keeps closing' : whyClosed(status));
  }

  #setLink(link: Link, why?: string): void {
    if (link === this.#link && why === this.#why) return;
    this.#link = link;
    this.#why = why;
    for (const watcher of this.#watchers) watcher(link, why);
  }
}
