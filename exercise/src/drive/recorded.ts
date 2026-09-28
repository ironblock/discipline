/**
 * Sessions recorded against a real model, migrated once into this
 * vocabulary -- the predecessor's (`scripts/migrate-recorded.py`), and one
 * OpenCode session (`scripts/migrate-opencode.py`) -- each file's `migration`
 * header saying what the migration decided rather than the record. Where the
 * specimen is authored intent, these are what actually happened -- slow
 * reads, side calls that answered as the agent, junk in working memory. The
 * OpenCode session ran no side calls: its are stitched on and say so.
 *
 * A recording carries no deltas (they were never recorded); `placed()`
 * synthesizes them the way the canned transport does, so a moment mid-answer
 * shows the answer mid-stream.
 */

import { deltas } from './canned.ts';
import { frames } from './progress.ts';
import type { LogLine } from './log.ts';
import { place } from './place.ts';
import type { EventOf, Unplaced } from './script.ts';
import type { Ack, DriveTransport } from './transport.ts';
import firstDrive from './recorded/first-drive.json?raw';
import cancelledCapture from './recorded/cancelled-capture.json?raw';
import stepLimit from './recorded/step-limit.json?raw';
import voxelStress from './recorded/voxel-stress.json?raw';

export interface Recording {
  readonly title: string;
  /** What the migration decided, rather than the record. */
  readonly migration: readonly string[];
  readonly events: readonly Unplaced[];
}

function load(text: string): Recording {
  const parsed = JSON.parse(text) as Partial<Recording>;
  if (typeof parsed.title !== 'string' || !Array.isArray(parsed.migration) || !Array.isArray(parsed.events)) {
    throw new Error('not a recording: expected title, migration and events');
  }
  for (const [i, e] of parsed.events.entries()) {
    if (typeof e?.kind !== 'string' || typeof e.t !== 'number') throw new Error(`not a recording: event ${i} has no kind or time`);
  }
  return parsed as Recording;
}

export const RECORDINGS = {
  'first-drive': load(firstDrive),
  /** A capture round the person cancelled: `capture.cancelled`, which the vocabulary does not have yet. */
  'cancelled-capture': load(cancelledCapture),
  /** A turn that ran into the step limit (30 steps). */
  'step-limit': load(stepLimit),
  /** OpenCode, native tool calls, several per step, six tools; its side calls authored (stitch-sides.py). */
  'voxel-stress': load(voxelStress),
} as const;

export type RecordingName = keyof typeof RECORDINGS;

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
  readonly #timers = new Set<ReturnType<typeof setTimeout>>();

  constructor(recording: Recording, { speed = 1 }: { readonly speed?: number } = {}) {
    this.#log = placed(recording);
    this.#speed = speed;
  }

  subscribe(listener: (line: LogLine) => void): () => void {
    for (const event of this.#emitted) listener(event);
    this.#listeners.add(listener);
    if (this.#timers.size === 0) this.#play();
    return () => this.#listeners.delete(listener);
  }

  dispatch(): Promise<Ack> {
    return Promise.resolve({ ok: false, refused: 'recording' });
  }

  close(): void {
    for (const timer of this.#timers) clearTimeout(timer);
    this.#timers.clear();
  }

  /** Schedule everything not yet emitted, from where the recording stopped. */
  #play(): void {
    const from = this.#emitted.at(-1)?.t ?? 0;
    for (const event of this.#log.slice(this.#emitted.length)) {
      const timer = setTimeout(() => {
        this.#timers.delete(timer);
        this.#emitted.push(event);
        for (const listener of this.#listeners) listener(event);
      }, (event.t - from) / this.#speed);
      this.#timers.add(timer);
    }
  }
}
