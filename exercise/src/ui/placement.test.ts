import { describe, expect, it } from 'vitest';

import { ENTER, LEAVE, place } from './placement.ts';
import type { Side, Span } from './placement.ts';

// A trunk of four 100-px nodes, 20 apart, finishing at t = 10, 20, 30, 40.
const trunk: Span[] = [0, 1, 2, 3].map((i) => ({ id: `n${i}`, top: i * 120, bottom: i * 120 + 100, endedAt: (i + 1) * 10 }));
const side = (id: string, at: string, over: Partial<Side> = {}): Side => ({ id, at, slot: 1, height: 40, ...over });
const GAP = 14;

describe('where a side call is drawn', () => {
  it('starts below the bottom of the last thing that finished before it started', () => {
    const got = place([side('a', 'n0', { startedAt: 25 })], trunk, 100, GAP);
    // n0 and n1 had finished; n2 was running beside it.
    expect(got.get('a')?.top).toBe(220 + GAP);
  });

  it('is cabled from its own trunk node, however far above, leaving from that node’s foot', () => {
    const got = place([side('a', 'n0', { startedAt: 35 })], trunk, 100, GAP);
    expect(got.get('a')?.top).toBe(340 + GAP);
    expect(got.get('a')?.leave).toBe(100 - LEAVE);
  });

  it('sits level with a trunk node still running when it started -- an interview in a tool call’s idle gap', () => {
    const got = place([side('a', 'n2', { startedAt: 25 })], trunk, 100, GAP);
    expect(got.get('a')?.top).toBe(240);
    expect(got.get('a')?.leave).toBe(240 + ENTER);
  });

  it('waits unpinned while pending: below everything finished so far, following the work down', () => {
    const pending = [side('p', 'n0')];
    expect(place(pending, trunk, 15, GAP).get('p')?.top).toBe(100 + GAP);
    expect(place(pending, trunk, 45, GAP).get('p')?.top).toBe(460 + GAP);
    expect(place(pending, trunk, 45, GAP).get('p')?.pending).toBe(true);
  });

  it('pins once its request starts, and stays pinned as the trunk grows', () => {
    const started = [side('a', 'n0', { startedAt: 15 })];
    expect(place(started, trunk, 15, GAP).get('a')?.top).toBe(place(started, trunk, 45, GAP).get('a')?.top);
  });

  it('stacks below an earlier side call still in its slot', () => {
    const got = place([side('a', 'n0', { startedAt: 12, height: 300 }), side('b', 'n0', { startedAt: 14 })], trunk, 100, GAP);
    expect(got.get('b')?.top).toBe((got.get('a')?.top ?? 0) + 300 + GAP);
  });

  it('keeps time running down across slots: below a side call in another slot that had finished', () => {
    const got = place(
      [side('a', 'n0', { startedAt: 12, height: 200, endedAt: 18 }), side('b', 'n0', { slot: 2, startedAt: 19 })],
      trunk,
      100,
      GAP,
    );
    expect(got.get('b')?.top).toBe((got.get('a')?.top ?? 0) + 200 + GAP);
  });
});
