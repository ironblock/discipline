import { describe, expect, it } from 'vitest';

import type { LogLine } from '../drive/log.ts';
import { place } from '../drive/place.ts';
import type { Unplaced } from '../drive/script.ts';
import { RECORDINGS, recordedAt } from '../drive/recorded.ts';
import { fold } from './fold.ts';
import { receiptOf } from './receipt.ts';

/** A log from loose scripted rows, placed in order. */
const log = (rows: readonly Record<string, unknown>[]): readonly LogLine[] => place(rows as unknown as Unplaced[]).log;

describe('the receipt: six numbers a session is measured on (#31)', () => {
  it('counts side calls, patches and mimicry per ask', () => {
    const r = receiptOf(
      log([
        { kind: 'ask', t: 0, turn: 1, text: 'a' },
        { kind: 'ask', t: 50, turn: 2, text: 'b' },
        { kind: 'fork', t: 10, id: 'f1', lane: 'interview', slot: 1, of_turn: 1, at: 'x', why: '', question: '', prefix_tokens: 0 },
        { kind: 'fork', t: 20, id: 'f2', lane: 'interview', slot: 1, of_turn: 1, at: 'x', why: '', question: '', prefix_tokens: 0 },
        { kind: 'fork.settled', t: 30, id: 'f2', outcome: 'mimicry' },
        { kind: 'patch', t: 30, id: 'p1', from: 'f1', op: 'add', entry: { id: '1', text: '' } },
      ]),
    );
    expect(r).toMatchObject({ asks: 2, sideCalls: 2, patches: 1, mimicry: 1 });
  });

  it('measures the trunk idle before each refill: from its last node to the seam', () => {
    const r = receiptOf(
      log([
        { kind: 'ask', t: 0, turn: 1, text: 'a' },
        { kind: 'request', t: 1, id: 'q', lane: 'trunk', slot: 0, turn: 1 },
        { kind: 'response', t: 100, id: 'q#r', to_request: 'q', text: '', stop: 'stop', timings: {} },
        { kind: 'turn.settled', t: 101, turn: 1, reason: 'final' },
        { kind: 'seam', t: 400, id: 's', at_turn: 1 },
      ]),
    );
    expect(r.idleBeforeRefill).toEqual([300]);
  });

  it('measures how much side-call time fell inside a person’s gap: turn handed back, to their next act', () => {
    const r = receiptOf(
      log([
        { kind: 'ask', t: 0, turn: 1, text: 'a' },
        // A side call from 5 to 25; the turn is handed back at 10, and the person asks again at 30.
        { kind: 'fork', t: 5, id: 'f', lane: 'interview', slot: 1, of_turn: 1, at: 'x', why: '', question: '', prefix_tokens: 0 },
        { kind: 'request', t: 5, id: 'f/q', lane: 'interview', slot: 1, turn: 1, fork: 'f' },
        { kind: 'turn.settled', t: 10, turn: 1, reason: 'final' },
        { kind: 'fork.settled', t: 25, id: 'f', outcome: 'value' },
        { kind: 'ask', t: 30, turn: 2, text: 'b' },
      ]),
    );
    expect(r.sideCallMs).toBe(20);
    expect(r.inGapMs).toBe(15);
  });

  it('is the floor on the predecessor’s first drive', () => {
    const r = fold(recordedAt(RECORDINGS['first-drive'], Number.POSITIVE_INFINITY)).receipt;
    // One patch per entry, as ruled: the predecessor's 255 rows landed 264 entry ops.
    expect(r).toMatchObject({ asks: 5, sideCalls: 94, patches: 264, mimicry: 7, liveEntries: 202 });
    expect(r.idleBeforeRefill.map((ms) => Math.round(ms / 1000))).toEqual([442, 637]);
    // Every second of side-call time falls in a gap, because the predecessor
    // refused asks while capture ran: from the log alone, a person waiting on
    // capture and a person reading look the same. `idle.gap` will tell them apart.
    expect(r.inGapMs).toBe(r.sideCallMs);
  });
});

describe('the sixth number, once the surface measures the gaps (Q4)', () => {
  // A turn settles at 100; a side call runs 100..600; the person reads to 300, composes to 400, is blocked to 900.
  const rows = [
    { kind: 'ask', t: 0, turn: 1, text: 'a' },
    { kind: 'turn.settled', t: 100, turn: 1, reason: 'final' },
    { kind: 'fork', t: 100, id: 'f', lane: 'interview', slot: 1, of_turn: 1, at: 'x', why: '', question: '', prefix_tokens: 0 },
    { kind: 'request', t: 100, id: 'fq', lane: 'interview', slot: 1, turn: 1, fork: 'f' },
    { kind: 'fork.settled', t: 600, id: 'f', outcome: 'value' },
  ];
  const measuredGap = (opened: number) => ({ seq: 0, kind: 'idle.gap', t: 900, opened_by: opened, notice: 0, read: 200, compose: 100, away: 0, blocked: 500, ended_by: 'ask' }) as unknown as LogLine;

  it('counts side-call time inside the attended part only -- not time blocked -- and says it is exact', () => {
    const placed = log(rows);
    const settled = placed.find((l) => l.kind === 'turn.settled')!;
    const r = receiptOf([...placed, { ...measuredGap(settled.seq), seq: placed.length } as LogLine]);
    expect(r).toMatchObject({ sideCallMs: 500, inGapMs: 300, gapsMeasured: 1, gapsTotal: 1 });
  });

  it('counts an unmeasured gap whole, and says so', () => {
    const r = receiptOf(log(rows));
    expect(r).toMatchObject({ sideCallMs: 500, inGapMs: 500, gapsMeasured: 0, gapsTotal: 1 });
  });

  it('folds a measured gap with its residual against the log’s own stamps', () => {
    const placed = log(rows);
    const settled = placed.find((l) => l.kind === 'turn.settled')!;
    const start = { seq: 0, t: 0, kind: 'session.start', version: 0, opened: 1, model: 'm', head: [] } as unknown as LogLine;
    const lines = [start, ...placed.map((l) => ({ ...l, seq: l.seq + 1 }) as LogLine)];
    const gapLine = { ...measuredGap(settled.seq + 1), seq: lines.length } as LogLine;
    const [gapNode] = fold([...lines, gapLine]).gaps;
    // 800 measured against 900 - 100 = 800 on the log's stamps.
    expect(gapNode).toMatchObject({ openedBy: String(settled.seq + 1), residual: 0, endedBy: 'ask' });
  });
});
