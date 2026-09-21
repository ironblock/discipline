/**
 * The loader: `diet check-record`'s projection in, placed events out.
 *
 * NO SECOND PARSER. This module never reads a `.jsonl`. It takes what `diet`
 * returned for one -- `project()`'s value, the same bytes from the CLI today
 * and from `diet::wasm::check_record` when the Pages payload needs it (#104
 * proves them identical) -- and decodes `canonical`, which is the record
 * rendered back by `diet` itself: one event per line, sorted keys, the
 * value space's JSON subset. `JSON.parse` over `diet`'s own rendering is a
 * decode of a verdict already given, not a reading of the format; nothing
 * here decides whether a line is an event, and a line `diet` refused never
 * reaches this function because the CLI's envelope says `ok: false` first.
 *
 * Two things are lost at this boundary and are named rather than hidden.
 * `JSON.parse` folds an exact decimal to a double (`0.7000` reads as `0.7`)
 * and cannot hold a `Count` above 2^53; both are disclosed in
 * `pending.types.ts`, and `scripts/gen-fixtures.mjs` lists every fixture
 * line that does not survive the round trip byte for byte.
 *
 * The right fix is upstream and is requested, not built here: `project()`
 * could carry `events` as a structured value (`to_value()` in
 * `formats/record/mod.rs` already renders one and nothing exposes it), so
 * the browser would receive an array and split nothing.
 */

import type { Placed } from './bound.ts';
import type { CliVerdict, Event, Kind, Projection, Source } from './types.ts';

/** A record the loader accepted, with every event at its position. */
export interface Loaded {
  readonly regime: Projection['regime'];
  readonly source: Source;
  readonly kinds: readonly Kind[];
  readonly events: readonly Placed[];
}

/** Why a projection could not be loaded. */
export type LoadRefusal =
  /** `diet` refused the record; the reason is its own, verbatim. */
  | { readonly kind: 'refused-by-diet'; readonly error: string }
  /** `canonical` had a line `JSON.parse` could not read -- which would mean
   *  `diet`'s renderer and this decoder disagree, and is a defect here. */
  | { readonly kind: 'undecodable-line'; readonly line: number; readonly error: string };

export type LoadResult =
  | { readonly ok: true; readonly loaded: Loaded }
  | { readonly ok: false; readonly refusal: LoadRefusal };

type Decoded =
  | { readonly ok: true; readonly events: readonly Placed[] }
  | { readonly ok: false; readonly refusal: LoadRefusal };

/** Decode `canonical` into placed events. */
export function decodeCanonical(canonical: string): Decoded {
  const events: Placed[] = [];
  const lines = canonical.split('\n');
  for (let i = 0; i < lines.length; i += 1) {
    const line = lines[i];
    if (line === undefined || line === '') continue;
    try {
      // The cast is the trust boundary, and it is the only one in the SPA:
      // `diet` accepted this record and rendered this line, so it is an
      // `Event` by `event_value()`'s own construction.
      const event = JSON.parse(line) as Event;
      events.push({ index: events.length, event });
    } catch (err) {
      return {
        ok: false,
        refusal: { kind: 'undecodable-line', line: i + 1, error: err instanceof Error ? err.message : String(err) },
      };
    }
  }
  return { ok: true, events };
}

/** Load a projection `diet` returned. */
export function load(projection: Projection): LoadResult {
  const decoded = decodeCanonical(projection.canonical);
  if (!decoded.ok) return decoded;
  return {
    ok: true,
    loaded: {
      regime: projection.regime,
      source: projection.source,
      kinds: projection.kinds,
      events: decoded.events,
    },
  };
}

/** Load straight from the CLI's envelope, refusing what `diet` refused. */
export function loadVerdict(verdict: CliVerdict): LoadResult {
  if (!verdict.ok) return { ok: false, refusal: { kind: 'refused-by-diet', error: verdict.error } };
  return load(verdict.value);
}

/** The loaded record, or a thrown error naming why not. For fixtures the
 *  generator already proved `diet` accepts, where a refusal is a defect. */
export function mustLoad(projection: Projection): Loaded {
  const result = load(projection);
  if (!result.ok) throw new Error(`the loader refused a fixture diet accepted: ${JSON.stringify(result.refusal)}`);
  return result.loaded;
}
