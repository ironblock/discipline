import { EXAMPLE_LABEL } from '../replay/published.ts';
import type { LogLine } from './log.ts';
import { retime } from './retime.ts';
import { clockCarried, declarationsKept, laneDiff } from './scenario-log.ts';

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
export function kitchenSinkExample(log: readonly LogLine[], previous: Previous | undefined, images: readonly string[], scenarioSha256: string) {
  const { log: clocked, carried, own } = clockCarried(previous?.log ?? [], log);
  return {
    title: 'The kitchen sink: every lever, written by a scenario',
    migration: [
      `Authored: ${EXAMPLE_LABEL}`,
      `written by diet-drive scenario ${KITCHEN_SINK_SCENARIO} (sha256 ${scenarioSha256}) on the canned scenario substrate: every line is diet's own, the model's replies are the scenario's`,
      "the run's temporary and home paths are written /work/..., and its background jobs bg_1, bg_2, ... (src/drive/scenario-log.ts)",
      "retimed: every t is placed on assumed rates (src/drive/retime.ts), not measured, and reads nothing of the run's clock: a tool's run is its timeout_ms when it timed out, else 150 ms",
      `stitched: each wall-clock field (a gate's latency_ms, a trunk wait's wait_ms; never compared) is carried from the example this one replaced, by lane and place (src/drive/scenario-log.ts): ${carried} carried, ${own} this run's own`,
      "concurrent: a stopped background job's background.ended is placed by the reaper when it sees the job end, concurrent with the step that stopped it, so a regeneration may move it (#896)",
      ...declarationsKept(previous?.migration ?? [], images),
    ],
    carried: {},
    timing: 'retimed',
    /** The scenario file it was written from: `check` fails when today's differs, and only then (the lagging sink). */
    scenario_sha256: scenarioSha256,
    log: retime(clocked),
  };
}

/**
 * The published kitchen sink against what the scenario writes today (the maintainer's ruling, 2026-11-11: the sink
 * lags). `current`: each lane is what it writes. `stale`: the scenario file is the one the example was written from,
 * and the code writes something else -- a behaviour change merged ahead of the regeneration, its lane diff the
 * reviewer's view of it. `unwritten`: the scenario file changed since (or the example records none), so the PR that
 * changed it must write the example too.
 */
export type SinkVerdict =
  | { readonly verdict: 'current' }
  | { readonly verdict: 'stale'; readonly lanes: readonly string[] }
  | { readonly verdict: 'unwritten'; readonly recorded?: string; readonly today: string };

export function sinkVerdict(
  published: { readonly log?: readonly LogLine[]; readonly scenario_sha256?: unknown },
  written: readonly LogLine[],
  scenarioSha256: string,
): SinkVerdict {
  const recorded = typeof published.scenario_sha256 === 'string' ? published.scenario_sha256 : undefined;
  if (recorded !== scenarioSha256) return { verdict: 'unwritten', ...(recorded !== undefined ? { recorded } : {}), today: scenarioSha256 };
  const lanes = laneDiff(published.log ?? [], written);
  return lanes.length === 0 ? { verdict: 'current' } : { verdict: 'stale', lanes };
}

/** Whether `check` fails on `verdict`: an unwritten scenario always; a stale example only when asked for the current one. */
export function fails(verdict: SinkVerdict, { current }: { readonly current: boolean }): boolean {
  return verdict.verdict === 'unwritten' || (verdict.verdict === 'stale' && current);
}
