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
 *   GET  /files/<sha256>      a file the log names, its bytes; 404 naming the
 *                             digest when it has none (#372, 5976915436). The
 *                             page checks the bytes (`files.ts`).
 *   POST /files               the operator's PNG, raw, `Content-Type: image/png`:
 *                             200 `{"sha256", "bytes"}`, 400 when it is not a
 *                             PNG, 413 over the cap, 415 for another type. An
 *                             ask names it after by digest, `"files": [<sha>]`;
 *                             one it cannot attach is 400 `{"unattachable":
 *                             {path, check, reason}}`, nothing logged.
 *   POST /approve             the operator's answer to the call waiting on them,
 *                             `{"call": <id>, "scope": "once" | "session" |
 *                             "workspace" | "decline"}`: 204, or 409
 *                             `{"refused": "nothing-waiting" | "stale"}`.
 *
 * Beside the log's lines `/events` carries two named events, never logged and
 * with no `id:` (ruled on #389, 5982826097): `waiting`, the call that waits on
 * the operator (`Prompt`), sent again after the history on every connect while
 * it waits; and `answered` `{request, id}`, when it is decided. The prompt is
 * also over on this page's own `/approve` ack, and on the call's `tool_call` line.
 *
 * The page reaches it same-origin: in development Vite proxies every route here
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
import type { Ack, Command, DriveTransport, Link, Prompt, Uploaded } from './transport.ts';
import type { FileAnswer } from './files.ts';

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
  /** A named event's listener: `waiting` and `answered` (#389). */
  addEventListener(type: string, listener: (event: MessageEvent<string>) => void): void;
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
      return 'the drive asks for credentials (401): start the surface with DIET_DRIVE_AUTH_FILE naming serve’s --auth-file';
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
  readonly #prompts = new Set<(prompt: Prompt | undefined) => void>();
  #prompt: Prompt | undefined;
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

  /** A tool call's file by its digest: unchecked bytes, or why there are none. */
  readonly file = async (sha256: string): Promise<FileAnswer> => {
    if (!/^[0-9a-f]{64}$/.test(sha256)) return { kind: 'not-found' };
    let reply: Response;
    try {
      reply = await this.#web.fetch(`${this.#base}/files/${sha256}`);
    } catch {
      return { kind: 'unreachable', why: whyClosed(undefined) };
    }
    if (reply.status === 404) return { kind: 'not-found' };
    if (!reply.ok) return { kind: 'unreachable', why: whyClosed(reply.status) };
    return { kind: 'bytes', bytes: new Uint8Array(await reply.arrayBuffer()) };
  };

  /** The operator's PNG, sent ahead of the ask that names it (`POST /files`). */
  async upload(bytes: Uint8Array): Promise<Uploaded> {
    let reply: Response;
    try {
      reply = await this.#web.fetch(`${this.#base}/files`, { method: 'POST', headers: { 'Content-Type': 'image/png' }, body: bytes as Uint8Array<ArrayBuffer> });
    } catch {
      return { ok: false, refused: 'unreachable' };
    }
    if (reply.status === 400 || reply.status === 415) return { ok: false, refused: 'not-a-png' };
    if (reply.status === 413) return { ok: false, refused: 'too-large' };
    if (!reply.ok) return { ok: false, refused: `http-${reply.status}` };
    const said = (await reply.json().catch(() => ({}))) as { readonly sha256?: unknown; readonly bytes?: unknown };
    return typeof said.sha256 === 'string' && typeof said.bytes === 'number' ? { ok: true, sha256: said.sha256, bytes: said.bytes } : { ok: false, refused: 'http-200' };
  }

  watchPrompt(listener: (prompt: Prompt | undefined) => void): () => void {
    listener(this.#prompt);
    this.#prompts.add(listener);
    return () => this.#prompts.delete(listener);
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
    if (command.kind === 'approve') {
      const { ack } = await this.#post({ call: command.call, scope: command.scope }, '/approve');
      if (ack.ok && this.#prompt?.id === command.call) this.#setPrompt(undefined);
      return ack;
    }
    const plain = this.#body(command);
    if (typeof plain['refused'] === 'string') return { ok: false, refused: plain['refused'] };
    const gap = extras?.idle_gap;
    const first = await this.#post(gap ? { ...plain, idle_gap: gap } : plain);
    // An ask whose attachment the drive refused is refused for that, gap or none: it does not go again.
    return gap && first.status === 400 && !first.unattachable ? (await this.#post(plain)).ack : first.ack;
  }

  /** One post, and what it came to: its status (0 when nothing answered), and the ack. */
  async #post(body: Readonly<Record<string, unknown>>, route = '/commands'): Promise<{ readonly status: number; readonly ack: Ack; readonly unattachable?: true }> {
    let reply: Response;
    try {
      reply = await this.#web.fetch(`${this.#base}${route}`, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) });
    } catch {
      return { status: 0, ack: { ok: false, refused: 'unreachable' } };
    }
    if (reply.ok) return { status: reply.status, ack: { ok: true } };
    if (reply.status === 409) {
      const said = (await reply.json().catch(() => ({}))) as { readonly refused?: unknown };
      return { status: 409, ack: { ok: false, refused: typeof said.refused === 'string' ? said.refused : 'refused' } };
    }
    if (reply.status === 400) {
      // An attachment the drive would not take (#372): an upload it never had, or a named path it refused -- by its check.
      const said = (await reply.json().catch(() => ({}))) as { readonly unattachable?: { readonly check?: unknown } };
      const check = said.unattachable?.check;
      if (typeof check === 'string') return { status: 400, ack: { ok: false, refused: check }, unattachable: true };
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
  #body(command: Exclude<Command, { kind: 'approve' }>): Readonly<Record<string, unknown>> {
    switch (command.kind) {
      case 'ask':
        // The operator's mark rides on the ask it marks, and on no other (#453): serve refuses it anywhere else.
        return { kind: 'ask', text: command.text, ...(command.scoping ? { scoping: true } : {}), ...(command.files && command.files.length > 0 ? { files: command.files } : {}) };
      case 'cancel': {
        // A stop names the turn it is for: the latest asked.
        const turn = this.#log.findLast((l) => l.kind === 'ask');
        return turn?.kind === 'ask' ? { kind: 'cancel', turn: turn.turn } : { refused: 'nothing-in-flight' };
      }
      case 'seam':
        // v0's declare-seam takes no phase, and the composer offers none under `?drive`.
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
    source.addEventListener('waiting', (event) => {
      if (source === this.#source) this.#waiting(event);
    });
    source.addEventListener('answered', (event) => {
      if (source !== this.#source) return;
      const said = parse(event.data);
      if (said && said['id'] === this.#prompt?.id && said['request'] === this.#prompt?.request) this.#setPrompt(undefined);
    });
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
    // The call's outcome is known: whatever it waited on is decided.
    if (line.kind === 'tool_call' && line.id === this.#prompt?.id && line.request === this.#prompt?.request) this.#setPrompt(undefined);
    this.#log.push(line);
    for (const listener of this.#listeners) listener(line);
  }

  /** A call waits on the operator: the page shows it until it is decided. One that does not read as a prompt is not shown. */
  #waiting(event: MessageEvent<string>): void {
    const said = parse(event.data);
    if (!said || typeof said['request'] !== 'number' || typeof said['id'] !== 'string' || typeof said['command'] !== 'string' || !Array.isArray(said['segments'])) return;
    const prompt: Prompt = {
      request: said['request'],
      id: said['id'],
      command: said['command'],
      cwd: typeof said['cwd'] === 'string' ? said['cwd'] : '',
      reason: typeof said['reason'] === 'string' ? said['reason'] : '',
      segments: said['segments'] as Prompt['segments'],
    };
    this.#setPrompt(prompt);
  }

  #setPrompt(prompt: Prompt | undefined): void {
    if (prompt === this.#prompt) return;
    this.#prompt = prompt;
    for (const listener of this.#prompts) listener(prompt);
  }

  /** Another session: drop this log and read the new one from its first line. */
  #restart(): void {
    this.#source?.close();
    this.#source = undefined;
    this.#setPrompt(undefined);
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

/** A named event's data as an object, or undefined. */
function parse(data: string): Readonly<Record<string, unknown>> | undefined {
  try {
    const value: unknown = JSON.parse(data);
    return typeof value === 'object' && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : undefined;
  } catch {
    return undefined;
  }
}
