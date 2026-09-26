import { describe, expect, it } from 'vitest';

import type { DriveEvent } from '../drive/events.ts';
import { RECORDINGS, recordedAt } from '../drive/recorded.ts';
import { fold } from './fold.ts';
import { receiptOf } from './receipt.ts';

/** A log from loose rows, numbered in order. */
const log = (rows: readonly Record<string, unknown>[]): DriveEvent[] => rows.map((r, seq) => ({ ...r, seq }) as unknown as DriveEvent);

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
