import { describe, expect, it } from 'vitest';

import { serialize } from '../replay/payload.ts';
import { kitchenSinkExample } from './kitchen-sink.ts';
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
    const first = kitchenSinkExample(today, { migration: [], log: run([2, 3]) }, []);
    const second = kitchenSinkExample(today, first as { migration: string[]; log: LogLine[] }, []);
    expect(serialize(second)).toBe(serialize(first));
    expect(latencies((first as { log: LogLine[] }).log)).toEqual([2, 3]);
    expect((first as { migration: string[] }).migration.some((line) => line.includes('latency_ms') && line.includes('carried'))).toBe(true);
  });
});
