/**
 * Sessions recorded against a real model, migrated once into this
 * vocabulary -- the predecessor's (`scripts/migrate-recorded.py`), and one
 * OpenCode session (`scripts/migrate-opencode.py`) -- each file's `migration`
 * header saying what the migration decided rather than the record. Where the
 * specimen is authored intent, these are what actually happened -- slow
 * reads, side calls that answered as the agent, junk in working memory. The
 * OpenCode session ran no side calls: its are stitched on and say so.
 *
 * The recordings are read in `recordings.ts`. A recording carries no deltas (they were never recorded); `placed()`
 * synthesizes them the way the canned transport does, so a moment mid-answer
 * shows the answer mid-stream.
 */

import { deltas } from './canned.ts';
import { frames } from './progress.ts';
import type { LogLine } from './log.ts';
import { place } from './place.ts';
import type { EventOf, Unplaced } from './script.ts';
import type { Ack, DriveTransport } from './transport.ts';

export interface Recording {
  readonly title: string;
  /** What the migration decided, rather than the record. */
  readonly migration: readonly string[];
  /**
   * The events of a kind this vocabulary does not have, carried under their
   * own name, with how many of each: what a fold of the events must leave
   * unknown, no more and no less (#173). Written by the migration beside
   * `migration`, whose prose says the same.
   */
  readonly carried: Readonly<Record<string, number>>;
  readonly events: readonly Unplaced[];
}

/** Where a recording lives in the repository, by name: what a failure to read one names (#32, ruling 9). */
export const recordingPath = (name: string) => `exercise/src/drive/recorded/${name}.json`;

/** Where an authored example's committed text lives (#272), by name: what a failure to read one names. */
export const examplePath = (name: string) => `exercise/src/drive/examples/${name}.json`;

/**
 * A recording read from its text, or an error that names its file: a build
 * that fails on a recording says which one, not only an event's index. The
 * recordings themselves are in `recordings.ts`, so that a page can load one
 * without carrying the others (the replay page, `replay.tsx`). An authored
 * example is read the same way and names its own file (`examplePath`, #272).
 */
export function load(name: string, text: string, file = recordingPath(name)): Recording {
  const fail = (why: string) => new Error(`${file}: not a recording: ${why}`);
  let parsed: Partial<Recording>;
  try {
    parsed = JSON.parse(text) as Partial<Recording>;
  } catch (err) {
    throw fail(`not JSON (${(err as Error).message})`);
  }
  if (typeof parsed.title !== 'string' || !Array.isArray(parsed.migration) || !Array.isArray(parsed.events)) {
    throw fail('expected title, migration and events');
  }
  const carried = parsed.carried as unknown;
  if (
    typeof carried !== 'object' || carried === null || Array.isArray(carried) ||
    !Object.entries(carried).every(([kind, n]) => kind !== '' && Number.isInteger(n) && (n as number) > 0)
  ) {
    throw fail('expected carried: {kind: count}, each kind named and each count a whole number above 0');
  }
  for (const [i, e] of parsed.events.entries()) {
    if (typeof e?.kind !== 'string' || typeof e.t !== 'number') throw fail(`event ${i} has no kind or time`);
  }
  return parsed as Recording;
}

/** The whole log, with synthesized deltas, placed in order. */
export function placed(recording: Recording): readonly LogLine[] {
  return placing(recording).log;
}

/** Where each of the recording's labels landed in its placed log: the node id a story looks for. */
export function labelsOf(recording: Recording): ReadonlyMap<string, number> {
  return placing(recording).labels;
}

function placing(recording: Recording) {
  const requestAt = new Map<string, number>();
  const out: Unplaced[] = [];
  for (const event of recording.events) {
    if (event.kind === 'request') requestAt.set(event.id, event.t);
    if (event.kind === 'response') {
      const response = event as Omit<EventOf<'response'>, 'seq'>;
      const requested = requestAt.get(event.to_request) ?? event.t;
      // The record has no progress; frames are made from the response's timings, once a second as `/slots` is polled.
      out.push(...deltas(response, requested, event.t), ...frames(response, requested, event.t, 1000));
    }
    out.push(event);
  }
  const ordered = out
    .map((e, i) => [e, i] as const)
    .sort(([a, i], [b, j]) => a.t - b.t || i - j)
    .map(([e]) => e);
  return place(ordered);
}

/** The log up to session time `t`: the recording stopped at a moment. */
export function recordedAt(recording: Recording, t: number): readonly LogLine[] {
  const log = placed(recording);
  const upto = log.findIndex((e) => e.t > t);
  return upto === -1 ? log : log.slice(0, upto);
}

/**
 * A transport that plays a recording on a clock. It takes no commands: a
 * recording already happened. The clock starts at the first subscriber and,
 * after `close()`, resumes where it stopped when someone subscribes again --
 * so a mount, unmount and remount (React's StrictMode) loses nothing.
 */
export class ReplayTransport implements DriveTransport {
  readonly #log: readonly LogLine[];
  readonly #speed: number;
  readonly #emitted: LogLine[] = [];
  readonly #listeners = new Set<(line: LogLine) => void>();
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor(recording: Recording, { speed = 1 }: { readonly speed?: number } = {}) {
    this.#log = placed(recording);
    this.#speed = speed;
  }

  subscribe(listener: (line: LogLine) => void): () => void {
    for (const event of this.#emitted) listener(event);
    this.#listeners.add(listener);
    if (this.#timer === undefined) this.#play();
    return () => this.#listeners.delete(listener);
  }

  dispatch(): Promise<Ack> {
    return Promise.resolve({ ok: false, refused: 'recording' });
  }

  close(): void {
    clearTimeout(this.#timer);
    this.#timer = undefined;
  }

  /**
   * Play on from where the recording stopped: one timer at a time, which
   * delivers every line due by then and sets the next -- timed from when play
   * began, so the waits do not drift.
   */
  #play(): void {
    const from = this.#emitted.at(-1)?.t ?? 0;
    const began = Date.now();
    const due = (line: LogLine) => began + (line.t - from) / this.#speed;
    const next = () => {
      const upcoming = this.#log[this.#emitted.length];
      if (!upcoming) return void (this.#timer = undefined);
      this.#timer = setTimeout(() => {
        for (let line = this.#log[this.#emitted.length]; line && due(line) <= Date.now(); line = this.#log[this.#emitted.length]) {
          this.#emitted.push(line);
          for (const listener of this.#listeners) listener(line);
        }
        next();
      }, Math.max(0, due(upcoming) - Date.now()));
    };
    next();
  }
}
