import { describe, expect, it } from 'vitest';

import { KITCHEN_SINK, KITCHEN_SINK_SCRIPT } from './kitchen-sink.ts';
import { compose } from './compose.ts';
import { placed } from './recorded.ts';
import { fold } from '../session/fold.ts';

type Row = Record<string, unknown> & { kind: string; t: number };
const events = compose(KITCHEN_SINK_SCRIPT) as unknown as Row[];
const of = (kind: string) => events.filter((e) => e.kind === kind);
const at = (kind: string, id: string, key = 'id') => events.find((e) => e.kind === kind && e[key] === id)?.t ?? Number.NaN;

describe('a composed session', () => {
  it('opens the session first, and keeps time in order', () => {
    expect(events[0]?.kind).toBe('session.start');
    expect(events.every((e, i) => i === 0 || e.t >= (events[i - 1]?.t ?? 0))).toBe(true);
  });

  it('answers every request after it was made, and ends every tool after it began', () => {
    for (const r of of('response')) expect(r.t).toBeGreaterThan(at('request', String(r['to_request'])));
    for (const e of of('tool.end')) expect(e.t).toBeGreaterThan(at('tool.begin', String(e['id'])));
  });

  it('runs one side call at a time on a slot, and lands its patches after it settles', () => {
    const bySlot = new Map<unknown, [number, number][]>();
    for (const f of of('fork')) bySlot.set(f['slot'], [...(bySlot.get(f['slot']) ?? []), [f.t, at('fork.settled', String(f['id']))]]);
    for (const spans of bySlot.values()) for (let i = 1; i < spans.length; i++) expect(spans[i]?.[0]).toBeGreaterThanOrEqual(spans[i - 1]?.[1] ?? 0);
    for (const p of of('patch')) expect(p.t).toBeGreaterThan(at('fork.settled', String(p['from'])));
  });

  it('makes a side call wait for its slot when another is still on it', () => {
    const ask = (question: string) => ({ lane: 'interview', slot: 1, when: 'settled' as const, why: '', question, answer: 'FACT: something long enough to take a while', changes: [] });
    const two = compose({
      model: 'm',
      slots: 2,
      phase: 'p',
      system: 's',
      parts: [{ kind: 'turn', after: 0, ask: 'a', steps: [{ say: 'done' }], sides: [ask('first?'), ask('second?')] }],
    }) as unknown as Row[];
    const [first, second] = two.filter((e) => e.kind === 'fork');
    const settled = two.find((e) => e.kind === 'fork.settled' && e['id'] === first?.['id']);
    expect(second?.t).toBeGreaterThanOrEqual(settled?.t ?? Number.POSITIVE_INFINITY);
  });

  it('says where a trunk response’s tool call began, as a drive calling tools natively can', () => {
    const called = of('response').filter((r) => r['stop'] === 'tool');
    expect(called.length).toBeGreaterThan(0);
    for (const r of called) {
      const from = r['calls_from'] as { predicted_n: number } | undefined;
      expect(from?.predicted_n).toBeLessThan((r['timings'] as { predicted_n: number }).predicted_n);
    }
  });

  it('gives every event of a kind its own id', () => {
    for (const kind of ['request', 'response', 'tool.begin', 'fork', 'patch', 'seam']) {
      const ids = of(kind).map((e) => e['id']);
      expect(new Set(ids).size).toBe(ids.length);
    }
  });
});

describe('the kitchen sink: the happy path the surface expects', () => {
  const session = fold(placed(KITCHEN_SINK));
  const r = session.receipt;

  it('has six asks over three phases and two refills, and nothing unknown', () => {
    expect(r.asks).toBe(6);
    expect(session.eras).toHaveLength(3);
    expect(session.unknown.size).toBe(0);
  });

  it('asks about two side calls and lands a few entries per ask, and none answers as the agent', () => {
    expect(r.sideCalls / r.asks).toBeGreaterThanOrEqual(1.5);
    expect(r.sideCalls / r.asks).toBeLessThanOrEqual(3);
    expect(r.patches / r.asks).toBeGreaterThanOrEqual(2);
    expect(r.patches / r.asks).toBeLessThanOrEqual(6);
    expect(r.mimicry).toBe(0);
  });

  it('refills soon after the trunk goes idle, and runs most side-call time while the trunk works or the person reads', () => {
    expect(r.idleBeforeRefill.every((ms) => ms < 30_000)).toBe(true);
    expect(r.liveEntries).toBeGreaterThanOrEqual(8);
  });
});
