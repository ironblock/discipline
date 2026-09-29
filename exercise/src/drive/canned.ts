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
import type { LogLine } from './log.ts';
import { place as placeAll, Placer } from './place.ts';
import type { Response, Unplaced } from './script.ts';
import type { Beat, Trigger } from './specimen.ts';
import type { IdleGapBody } from '../session/gap.ts';
import type { Ack, Command, DriveTransport } from './transport.ts';

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
export function expand(beat: Beat, start: number, ask?: string): Unplaced[] {
  const out: Unplaced[] = [];
  const requestAt = new Map<string, number>();
  for (const event of beat.events) {
    const t = start + event.t;
    if (event.kind === 'request') requestAt.set(event.id, t);
    if (event.kind === 'response') out.push(...deltas(event, requestAt.get(event.to_request) ?? t, t), ...frames(event, requestAt.get(event.to_request) ?? t, t));
    out.push(event.kind === 'ask' && ask !== undefined ? { ...event, t, text: ask } : { ...event, t });
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

/** Which command ends a gap with which `ended_by`. */
const ENDS = { ask: 'ask', seam: 'seam', cancel: 'cancel' } as const satisfies Record<Command['kind'], IdleGapBody['ended_by']>;

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
  readonly #timers = new Set<ReturnType<typeof setTimeout>>();
  readonly #opened = performance.now();
  #next = 0;

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
    return this.#timers.size > 0;
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
    const gap = extras?.idle_gap;
    const opened = this.#openGap;
    this.#gap = gap && gap.ended_by === ENDS[command.kind] && gap.opened_by === opened ? gap : undefined;
    const ack = command.kind === 'ask' ? this.#fire('send', command.text) : command.kind === 'seam' ? this.#seam(command.to) : this.#cancel();
    // Refused, it is dropped. Admitted, the gap that was open closes, carried or not -- unless the command settled a
    // turn and opened the next one itself, as a cancel does (`diet` closes it at admission, before the command runs).
    this.#gap = undefined;
    if (ack.ok && this.#openGap === opened) this.#openGap = undefined;
    return Promise.resolve(ack);
  }

  #seam(to: string): Ack {
    if (!this.#log.some((e) => e.kind === 'turn.settled')) return { ok: false, refused: 'nothing-to-seam' };
    const scripted = this.#beats[this.#next]?.events.find((e) => e.kind === 'seam');
    if (scripted?.kind === 'seam' && scripted.phase && scripted.phase.to !== to) return { ok: false, refused: 'off-script' };
    return this.#fire('seam');
  }

  #cancel(): Ack {
    if (!this.busy) return { ok: false, refused: 'nothing-to-cancel' };
    for (const timer of this.#timers) clearTimeout(timer);
    this.#timers.clear();
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
    for (const fork of open.forks) this.#emit({ kind: 'fork.settled', t: now, id: fork, outcome: 'cancelled' });
    if (open.turn !== undefined) this.#emit({ kind: 'turn.settled', t: now, turn: open.turn, reason: 'cancelled' });
    return { ok: true };
  }

  /** Stop every timer. The log stays. */
  close(): void {
    for (const timer of this.#timers) clearTimeout(timer);
    this.#timers.clear();
  }

  #fire(trigger: Trigger, ask?: string): Ack {
    if (this.busy) return { ok: false, refused: 'busy' };
    const beat = this.#beats[this.#next];
    if (!beat) return { ok: false, refused: 'ended' };
    if (beat.trigger !== trigger) return { ok: false, refused: 'off-script' };
    this.#next += 1;
    const start = this.#now();
    for (const event of expand(beat, start, ask)) {
      const delay = (event.t - start) / this.#speed;
      if (delay <= 0) {
        this.#emit(event);
        continue;
      }
      const timer = setTimeout(() => {
        this.#timers.delete(timer);
        this.#emit(event);
      }, delay);
      this.#timers.add(timer);
    }
    return { ok: true };
  }

  #now(): number {
    return Math.round((performance.now() - this.#opened) * this.#speed);
  }

  #emit(event: Unplaced): void {
    // An admitted command's gap: placed, and logged, before the first line the command pushes.
    if (this.#gap) {
      const gap = this.#gap;
      this.#gap = undefined;
      this.#push(this.#placer.line({ kind: 'idle.gap', t: event.t, ...gap }));
    }
    this.#played.push(event);
    for (const line of this.#placer.place(event)) this.#push(line);
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
