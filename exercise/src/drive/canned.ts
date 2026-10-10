/**
 * A `DriveTransport` that plays an authored script instead of a model.
 *
 * It exists so the surface can be designed before #117's loop does: the
 * script's beats fire on the same three commands a real session takes, the
 * trunk's answers stream, and a slow `cargo test` really does leave the trunk
 * idle for twelve seconds. The ask a person types replaces the script's ask;
 * everything the model says is scripted. `snapshot()` is the same expansion
 * without a clock, so a story can stop the session at any moment.
 */

import { frames } from './progress.ts';
import type { FileRef, LogLine } from './log.ts';
import { isPng } from './png.ts';
import { place as placeAll, Placer } from './place.ts';
import type { Response, ScriptedPrompt, Unplaced } from './script.ts';
import type { Beat, Trigger } from './specimen.ts';
import type { IdleGapBody } from '../session/gap.ts';
import type { Ack, Command, Decision, DriveTransport, Prompt, Uploaded } from './transport.ts';
import { sha256 } from './files.ts';
import type { FileAnswer } from './files.ts';

/** The gap a snapshot assumes between beats: a person reading, then typing. */
export const READING_GAP_MS = 20_000;

/** Where a snapshot stops: `beat` beats have fired, and `t` ms of the last. */
export interface Cursor {
  readonly beat: number;
  /** Milliseconds into beat `beat - 1`. Absent: all of it. */
  readonly t?: number;
}

/**
 * A beat's events at absolute time, with each generating response preceded by
 * the deltas it would have streamed: reasoning first, then text, spread from
 * the end of prefill to the response.
 */
export function expand(beat: Beat, start: number, ask?: string, scoping?: true, files?: readonly FileRef[]): Unplaced[] {
  const out: Unplaced[] = [];
  const requestAt = new Map<string, number>();
  for (const event of beat.events) {
    const t = start + event.t;
    if (event.kind === 'request') requestAt.set(event.id, t);
    if (event.kind === 'response') out.push(...deltas(event, requestAt.get(event.to_request) ?? t, t), ...frames(event, requestAt.get(event.to_request) ?? t, t));
    // A decision a script declares is on the beat's clock, as its time is.
    const decided = event.kind === 'tool.end' && event.approval?.decided_at !== undefined ? { approval: { ...event.approval, decided_at: start + event.approval.decided_at } } : {};
    out.push(event.kind === 'ask' && ask !== undefined ? { ...event, t, text: ask, ...(scoping ? { scoping } : {}), ...(files && files.length > 0 ? { files } : {}) } : { ...event, t, ...decided });
  }
  return out.sort((a, b) => a.t - b.t);
}

/** The deltas a response would have streamed: reasoning first, then text, spread from the end of prefill to the response. */
export function deltas(response: Omit<Response, 'seq'>, requested: number, done: number): Unplaced[] {
  const from = requested + response.timings.prompt_ms;
  const reasoning = chunks(response.reasoning ?? '');
  const text = chunks(response.text);
  const all = [...reasoning.map((r) => ({ reasoning: r })), ...text.map((x) => ({ text: x }))];
  const span = Math.max(0, done - from);
  return all.map((part, i) => ({
    kind: 'delta' as const,
    t: Math.floor(from + (span * i) / Math.max(1, all.length)),
    request: response.to_request,
    ...part,
  }));
}

/** Split prose into streamable pieces of a few words, whitespace kept. */
function chunks(s: string): string[] {
  const words = s.match(/\S+\s*/g) ?? [];
  const size = Math.max(1, Math.ceil(words.length / 40));
  const out: string[] = [];
  for (let i = 0; i < words.length; i += size) out.push(words.slice(i, i + size).join(''));
  return out;
}

/** The session at a cursor, with no clock -- beats start `READING_GAP_MS` after the last one ended -- as the script it plays. */
export function scriptAt(beats: readonly Beat[], cursor: Cursor): readonly Unplaced[] {
  const events: Unplaced[] = [];
  let start = 0;
  const upto = Math.min(cursor.beat, beats.length);
  for (let b = 0; b < upto; b += 1) {
    const beat = beats[b];
    if (!beat) break;
    const expanded = expand(beat, start);
    const last = b === upto - 1 && cursor.t !== undefined ? start + cursor.t : Number.POSITIVE_INFINITY;
    events.push(...expanded.filter((e) => e.t <= last));
    const end = expanded.at(-1)?.t ?? start;
    start = end + (beats[b + 1]?.trigger === 'send' ? READING_GAP_MS : 1500);
  }
  return events;
}

/** The session at a cursor, placed in the log. */
export function snapshot(beats: readonly Beat[], cursor: Cursor): readonly LogLine[] {
  return placeAll(scriptAt(beats, cursor)).log;
}

/** Each beat's length, so a story can ask for "the middle of the tool call". */
export function beatLength(beat: Beat): number {
  return beat.events.at(-1)?.t ?? 0;
}

export interface CannedOptions {
  /** 2 plays twice as fast. Time on events stays session time. */
  readonly speed?: number;
}

/** Which command ends a gap with which `ended_by`. An answer to a prompt ends none: a turn is in flight. */
const ENDS = { ask: 'ask', seam: 'seam', cancel: 'cancel', end: 'end', approve: undefined, 'open-tangent': undefined, 'close-tangent': undefined } as const satisfies Record<Command['kind'], IdleGapBody['ended_by'] | undefined>;

/** An event the script will play, and what follows it: what a timer holds, and what a prompt holds back. */
interface Scheduled {
  readonly event: Unplaced;
  readonly ahead: readonly Unplaced[];
}

/** A call waiting on the operator (#389): what was shown, and the rest of its beat, held from when it began. */
interface Waiting {
  readonly prompt: Prompt;
  readonly scripted: ScriptedPrompt;
  readonly t: number;
  readonly held: readonly Scheduled[];
}

export class CannedTransport implements DriveTransport {
  readonly #beats: readonly Beat[];
  readonly #speed: number;
  /** What the script has played, labels and all: what `cancel` reads to find what is still open. */
  readonly #played: Unplaced[] = [];
  readonly #placer = new Placer();
  readonly #log: LogLine[] = [];
  /** The latest `turn.settled` no gap has been logged against: the only gap a command may carry. */
  #openGap: number | undefined;
  /** An admitted command's gap, logged before the next line it pushes. */
  #gap: IdleGapBody | undefined;
  readonly #listeners = new Set<(line: LogLine) => void>();
  readonly #prompts = new Set<(prompt: Prompt | undefined) => void>();
  /** Each timer, and the event it will play: what a prompt holds back. */
  readonly #timers = new Map<ReturnType<typeof setTimeout>, Scheduled>();
  #waiting: Waiting | undefined;
  /** The operator's decision on each call, by its label: added to its `tool_call` line. */
  readonly #decided = new Map<string, NonNullable<Extract<Unplaced, { kind: 'tool.end' }>['approval']>>();
  readonly #opened = performance.now();
  #next = 0;

  /** What the operator uploaded, by digest: as `serve` keeps it, in the recording's `files/`. */
  readonly #uploads = new Map<string, Uint8Array>();

  /** The script's files and the operator's uploads, by digest: unchecked, as a server's are (#372). */
  readonly file = (sha256: string): Promise<FileAnswer> => {
    const found = this.#uploads.get(sha256) ?? this.#beats.flatMap((b) => b.events).flatMap((e) => (e.kind === 'tool.end' ? (e.files ?? []) : [])).find((f) => f.sha256 === sha256)?.bytes;
    return Promise.resolve(found ? { kind: 'bytes', bytes: found } : { kind: 'not-found' });
  };

  /** As `serve`'s `POST /files`: a PNG by its signature, kept by digest; anything else refused. */
  async upload(bytes: Uint8Array): Promise<Uploaded> {
    if (!isPng(bytes)) return { ok: false, refused: 'not-a-png' };
    const digest = await sha256(bytes);
    this.#uploads.set(digest, bytes);
    return { ok: true, sha256: digest, bytes: bytes.length };
  }

  constructor(beats: readonly Beat[], options: CannedOptions = {}) {
    this.#beats = beats;
    this.#speed = options.speed ?? 1;
    this.#fire('open');
  }

  /** What the script expects next, so a demo can say so. */
  get expects(): Trigger | undefined {
    return this.#beats[this.#next]?.trigger;
  }

  get busy(): boolean {
    return this.#timers.size > 0 || this.#waiting !== undefined;
  }

  watchPrompt(listener: (prompt: Prompt | undefined) => void): () => void {
    listener(this.#waiting?.prompt);
    this.#prompts.add(listener);
    return () => this.#prompts.delete(listener);
  }

  subscribe(listener: (line: LogLine) => void): () => void {
    for (const event of this.#log) listener(event);
    this.#listeners.add(listener);
    return () => this.#listeners.delete(listener);
  }

  dispatch(command: Command, extras?: { readonly idle_gap?: IdleGapBody }): Promise<Ack> {
    // As `diet` does (#146, ruling (b) on #117): a command's gap rides only if the command is admitted -- logged
    // just before the first line it pushes -- and must be the open gap, ended by this kind of command. `diet`
    // turns a bad one away with the command (400), and `HttpTransport` sends the command again without it; here
    // the two are one step: the command goes ahead, its gap unlogged.
    if (command.kind === 'approve') return Promise.resolve(this.#approve(command.call, command.scope));
    // The canned script plays no tangent (#608): the served drive's to take.
    if (command.kind === 'open-tangent' || command.kind === 'close-tangent') return Promise.resolve({ ok: false, refused: 'off-script' });
    const gap = extras?.idle_gap;
    const opened = this.#openGap;
    this.#gap = gap && gap.ended_by === ENDS[command.kind] && gap.opened_by === opened ? gap : undefined;
    const ack =
      command.kind === 'ask' ? this.#ask(command) : command.kind === 'seam' ? this.#seam(command.to) : command.kind === 'end' ? this.#end() : this.#cancel();
    // Refused, it is dropped. Admitted, the gap that was open closes, carried or not -- unless the command settled a
    // turn and opened the next one itself, as a cancel does (`diet` closes it at admission, before the command runs).
    this.#gap = undefined;
    if (ack.ok && this.#openGap === opened) this.#openGap = undefined;
    return Promise.resolve(ack);
  }

  /** An ask, its attachments by reference as `serve` logs them: each must have been uploaded first. */
  #ask(command: Extract<Command, { kind: 'ask' }>): Ack {
    const files: FileRef[] = [];
    for (const digest of command.files ?? []) {
      const bytes = this.#uploads.get(digest);
      if (!bytes) return { ok: false, refused: 'not-uploaded' };
      files.push({ path: `files/${digest}`, sha256: digest, media_type: 'image/png', bytes: bytes.length });
    }
    return this.#fire('send', command.text, command.scoping, files);
  }

  #seam(to: string | undefined): Ack {
    if (!this.#log.some((e) => e.kind === 'turn.settled')) return { ok: false, refused: 'nothing-to-seam' };
    const scripted = this.#beats[this.#next]?.events.find((e) => e.kind === 'seam');
    if (scripted?.kind === 'seam' && scripted.phase && scripted.phase.to !== to) return { ok: false, refused: 'off-script' };
    return this.#fire('seam');
  }

  /** As `diet`'s `Session::end`: taken only while awaiting -- refused while work is in flight, or once ended. */
  #end(): Ack {
    if (this.#log.some((l) => l.kind === 'settlement' && l.to === 'ended')) return { ok: false, refused: 'ended' };
    if (this.busy) return { ok: false, refused: 'in-flight' };
    this.#emit({ kind: 'session.end', t: this.#now() });
    return { ok: true };
  }

  /**
   * The operator's answer (`serve`'s `POST /approve`, ruled on #389): a scope plays the rest of the beat on from
   * now, the call carrying the decision; `decline` refuses the call and plays the script's way on instead.
   */
  #approve(call: string, scope: Decision): Ack {
    const waiting = this.#waiting;
    if (!waiting) return { ok: false, refused: 'nothing-waiting' };
    if (waiting.prompt.id !== call) return { ok: false, refused: 'stale' };
    this.#setWaiting(undefined);
    const now = this.#now();
    if (scope === 'decline') {
      this.#emit({ kind: 'tool.end', t: now, id: call, exit: 0, output: '', refused: 'declined' });
      this.#schedule(waiting.scripted.declined.map((e) => ({ ...e, t: now + e.t })), now);
      return { ok: true };
    }
    this.#decided.set(call, { scope, decided_at: now, why: waiting.prompt.reason });
    const shift = now - waiting.t;
    this.#schedule(
      waiting.held.map((h): Unplaced => ({ ...h.event, t: h.event.t + shift })),
      now,
    );
    return { ok: true };
  }

  #cancel(): Ack {
    if (!this.busy) return { ok: false, refused: 'nothing-to-cancel' };
    for (const timer of this.#timers.keys()) clearTimeout(timer);
    this.#timers.clear();
    // A call waiting on the operator is cut off like one running: its line says `cancelled` (#298 point 8).
    this.#setWaiting(undefined);
    const now = this.#now();
    const open = openWork(this.#played);
    for (const request of open.requests) {
      const streamed = this.#played.filter((e): e is Extract<Unplaced, { kind: 'delta' }> => e.kind === 'delta' && e.request === request);
      this.#emit({
        kind: 'response',
        t: now,
        id: `${request}#response`,
        to_request: request,
        reasoning: streamed.map((d) => d.reasoning ?? '').join(''),
        text: streamed.map((d) => d.text ?? '').join(''),
        stop: 'cancelled',
        timings: { prompt_n: 0, cache_n: 0, prompt_ms: 0, predicted_n: 0, predicted_ms: 0 },
      });
    }
    // Every call the model made leaves one line: one cut off mid-command, `cancelled` (#297, ruled 5973541934).
    // Placed calls, not played begins: a call's fragment streams with its response, before its `tool.begin` plays.
    for (const call of this.#placer.openCalls()) this.#emit({ kind: 'tool.end', t: now, id: call, exit: 0, output: '', cancelled: true });
    for (const fork of open.forks) this.#emit({ kind: 'fork.settled', t: now, id: fork, outcome: 'cancelled' });
    if (open.turn !== undefined) this.#emit({ kind: 'turn.settled', t: now, turn: open.turn, reason: 'cancelled' });
    return { ok: true };
  }

  /** Stop every timer. The log stays. */
  close(): void {
    for (const timer of this.#timers.keys()) clearTimeout(timer);
    this.#timers.clear();
  }

  #fire(trigger: Trigger, ask?: string, scoping?: true, files?: readonly FileRef[]): Ack {
    if (this.busy) return { ok: false, refused: 'busy' };
    const beat = this.#beats[this.#next];
    if (!beat) return { ok: false, refused: 'ended' };
    if (beat.trigger !== trigger) return { ok: false, refused: 'off-script' };
    this.#next += 1;
    const start = this.#now();
    this.#schedule(expand(beat, start, ask, scoping, files), start);
    return { ok: true };
  }

  /** Play EVENTS (session time) from START: now if due, else on a timer. A prompted call holds back whatever follows it. */
  #schedule(events: readonly Unplaced[], start: number): void {
    for (const [i, event] of events.entries()) {
      const ahead = events.slice(i + 1);
      const delay = (event.t - start) / this.#speed;
      if (delay <= 0) {
        if (this.#play(event, ahead)) return;
        continue;
      }
      const timer = setTimeout(() => {
        this.#timers.delete(timer);
        this.#play(event, ahead);
      }, delay);
      this.#timers.set(timer, { event, ahead });
    }
  }

  /** Play one event. True when it is a call that waits on the operator: everything scheduled after it is held. */
  #play(event: Unplaced, ahead: readonly Unplaced[]): boolean {
    this.#emit(event, ahead);
    if (event.kind !== 'tool.begin' || !event.prompt) return false;
    const held = [...this.#timers.values()].sort((a, b) => a.event.t - b.event.t);
    for (const timer of this.#timers.keys()) clearTimeout(timer);
    this.#timers.clear();
    // What is due now and not yet played is held too: `ahead` from this event on.
    const due = ahead.filter((e) => !held.some((h) => h.event === e));
    const request = this.#placer.seqOf(event.after) ?? -1;
    const command = typeof event.args['command'] === 'string' ? event.args['command'] : JSON.stringify(event.args);
    this.#setWaiting({
      prompt: { request, id: event.id, command, cwd: event.cwd ?? '', reason: event.prompt.reason, segments: event.prompt.segments },
      scripted: event.prompt,
      t: event.t,
      held: [...due.map((e, i) => ({ event: e, ahead: due.slice(i + 1) })), ...held].sort((a, b) => a.event.t - b.event.t),
    });
    return true;
  }

  #setWaiting(waiting: Waiting | undefined): void {
    this.#waiting = waiting;
    for (const listener of this.#prompts) listener(waiting?.prompt);
  }

  #now(): number {
    return Math.round((performance.now() - this.#opened) * this.#speed);
  }

  #emit(event: Unplaced, ahead: readonly Unplaced[] = []): void {
    // An admitted command's gap: placed, and logged, before the first line the command pushes.
    if (this.#gap) {
      const gap = this.#gap;
      this.#gap = undefined;
      this.#push(this.#placer.line({ kind: 'idle.gap', t: event.t, ...gap }));
    }
    this.#played.push(event);
    // A call that ran on the operator's decision carries it (log v4's `approval`, #388).
    const decided = event.kind === 'tool.end' ? this.#decided.get(event.id) : undefined;
    const placed = decided && event.kind === 'tool.end' && !event.refused && !event.approval ? { ...event, approval: decided } : event;
    for (const line of this.#placer.place(placed, ahead)) this.#push(line);
  }

  #push(line: LogLine): void {
    if (line.kind === 'turn.settled') this.#openGap = line.seq;
    this.#log.push(line);
    for (const listener of this.#listeners) listener(line);
  }
}

/** Requests with no response, forks not settled, and a turn not settled. */
function openWork(log: readonly Unplaced[]) {
  const requests = new Set<string>();
  const forks = new Set<string>();
  let turn: number | undefined;
  for (const e of log) {
    if (e.kind === 'request') requests.add(e.id);
    if (e.kind === 'response') requests.delete(e.to_request);
    if (e.kind === 'fork') forks.add(e.id);
    if (e.kind === 'fork.settled') forks.delete(e.id);
    if (e.kind === 'ask') turn = e.turn;
    if (e.kind === 'turn.settled') turn = undefined;
  }
  return { requests: [...requests], forks: [...forks], turn };
}
