import { describe, expect, it } from 'vitest';

import { serialize } from '../replay/payload.ts';
import { fails, kitchenSinkExample, sinkVerdict } from './kitchen-sink.ts';
import type { LogLine } from './log.ts';
import { clockCarried } from './scenario-log.ts';

/**
 * A gate's `latency_ms` is wall time on the machine that ran (#862), as `t` is: carried from the example a write
 * replaces, matched by lane and place as `scenario-log.ts` compares lines, so a rewrite changes only what the session
 * changed, and says so.
 */
const START = { seq: 0, t: 0, kind: 'session.start', version: 8, opened: 1, model: 'm', head: [{ role: 'system', content: 's' }] };
const gated = (seq: number, request: number, latency: number) => ({
  seq,
  t: seq,
  kind: 'warrant.gated',
  request,
  outcome: 'decided',
  probe: 'gotcha',
  latency_ms: latency,
});
const run = (latencies: readonly number[], first = 1) =>
  [
    START,
    { seq: first, t: first, kind: 'ask', turn: 1, text: 'go' },
    { seq: first + 1, t: first + 1, kind: 'request', lane: 'trunk', turn: 1 },
    gated(first + 2, first + 1, latencies[0]!),
    { seq: first + 3, t: first + 3, kind: 'request', lane: 'trunk', turn: 1 },
    gated(first + 4, first + 3, latencies[1]!),
  ] as unknown as LogLine[];
const latencies = (log: readonly LogLine[]) =>
  (log as unknown as readonly { kind: string; latency_ms?: number }[]).filter((l) => l.kind === 'warrant.gated').map((l) => l.latency_ms);

describe("a rewrite's wall-clock fields", () => {
  it('are carried from the example it replaces, by lane and place, whatever the seqs', () => {
    const { log, carried, own } = clockCarried(run([2, 3]), run([7, 9], 40));
    expect(latencies(log)).toEqual([2, 3]);
    expect([carried, own]).toEqual([2, 0]);
  });

  it('are the run\'s own where the example it replaces has no line there', () => {
    const { log, carried, own } = clockCarried(run([2, 3]).slice(0, 4), run([7, 9]));
    expect(latencies(log)).toEqual([2, 9]);
    expect([carried, own]).toEqual([1, 1]);
  });

  it('give byte-identical examples when the same log is written twice', () => {
    const today = run([7, 9]);
    const first = kitchenSinkExample(today, { migration: [], log: run([2, 3]) }, [], 'c'.repeat(64));
    const second = kitchenSinkExample(today, first as { migration: string[]; log: LogLine[] }, [], 'c'.repeat(64));
    expect(serialize(second)).toBe(serialize(first));
    expect(latencies((first as { log: LogLine[] }).log)).toEqual([2, 3]);
    expect((first as { migration: string[] }).migration.some((line) => line.includes('latency_ms') && line.includes('carried'))).toBe(true);
  });
});

/**
 * The kitchen sink lags (the maintainer's ruling, 2026-11-11): a change to what the scenario writes merges without
 * regenerating the example, and one regeneration on develop catches it up. `check` fails only when the scenario file
 * itself changed since the example was written; lane drift alone is stale -- shown, not failed -- unless the check is
 * asked for the current example (`--current`, which the regeneration runs).
 */
describe('the kitchen sink check', () => {
  const SCENARIO = 'a'.repeat(64);
  const published = (log: readonly LogLine[], scenario?: string) => ({ log, ...(scenario !== undefined ? { scenario_sha256: scenario } : {}) });

  it('records the digest of the scenario file it was written from', () => {
    const example = kitchenSinkExample(run([2, 3]), undefined, [], SCENARIO) as { scenario_sha256?: string; migration: string[] };
    expect(example.scenario_sha256).toBe(SCENARIO);
    expect(example.migration.some((line) => line.includes(SCENARIO))).toBe(true);
  });

  it('is current when each lane is what the scenario writes', () => {
    expect(sinkVerdict(published(run([2, 3]), SCENARIO), run([7, 9]), SCENARIO)).toEqual({ verdict: 'current' });
  });

  it('is stale, not failed, when the code changed what the scenario writes', () => {
    const today = [...run([2, 3]), { seq: 6, t: 6, kind: 'turn.settled', turn: 1, reason: 'final' }] as unknown as LogLine[];
    const verdict = sinkVerdict(published(run([2, 3]), SCENARIO), today, SCENARIO);
    expect(verdict.verdict).toBe('stale');
    expect(verdict.verdict === 'stale' && verdict.lanes.length).toBe(1);
    expect(fails(verdict, { current: false })).toBe(false);
    expect(fails(verdict, { current: true })).toBe(true);
  });

  it('fails when the scenario file changed and the example was not written from it', () => {
    const verdict = sinkVerdict(published(run([2, 3]), SCENARIO), run([2, 3]), 'b'.repeat(64));
    expect(verdict.verdict).toBe('unwritten');
    expect(fails(verdict, { current: false })).toBe(true);
    expect(sinkVerdict(published(run([2, 3])), run([2, 3]), SCENARIO).verdict).toBe('unwritten');
  });
});
