import { EXAMPLE_LABEL } from '../replay/published.ts';
import type { LogLine } from './log.ts';
import { retime } from './retime.ts';
import { clockCarried, declarationsKept } from './scenario-log.ts';

/** The scenario the kitchen sink's example is written from (#798). */
export const KITCHEN_SINK_SCENARIO = 'diet/scenarios/kitchen-sink.jsonl';

/** The example `previous` was, as far as a rewrite reads it: its migration's declarations and its log's clocks. */
type Previous = { readonly migration?: readonly string[]; readonly log?: readonly LogLine[] };

/**
 * The kitchen sink's published example (#798), as `scripts/kitchen-sink.mjs write` composes it from `log` -- the
 * scenario's log as written today, scrubbed -- and the example it replaces, `previous`: the images it names (`images`)
 * with the Declared-clean lines `previous` kept for them, and each gate's `latency_ms` carried from `previous` by lane
 * and place (`clockCarried`), the migration saying how many were carried and how many are this run's own. A pure
 * function of its inputs: the same log written over its own example gives the same bytes.
 */
export function kitchenSinkExample(log: readonly LogLine[], previous: Previous | undefined, images: readonly string[]) {
  const { log: clocked, carried, own } = clockCarried(previous?.log ?? [], log);
  return {
    title: 'The kitchen sink: every lever, written by a scenario',
    migration: [
      `Authored: ${EXAMPLE_LABEL}`,
      `written by diet-drive scenario ${KITCHEN_SINK_SCENARIO} on the canned scenario substrate: every line is diet's own, the model's replies are the scenario's`,
      "the run's temporary and home paths are written /work/..., and its background jobs bg_1, bg_2, ... (src/drive/scenario-log.ts)",
      'retimed: every t is placed on assumed rates (src/drive/retime.ts), not measured',
      `stitched: each gate's latency_ms (wall time, never compared) is carried from the example this one replaced, by lane and place (src/drive/scenario-log.ts): ${carried} carried, ${own} this run's own`,
      ...declarationsKept(previous?.migration ?? [], images),
    ],
    carried: {},
    timing: 'retimed',
    log: retime(clocked),
  };
}
