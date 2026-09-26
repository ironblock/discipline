import { describe, expect, it } from 'vitest';

import { MIN_BAR, PATCH_TICK, ROW, barHeight, rowsOf } from './condensed.ts';

describe('a side call, condensed to a bar', () => {
  it('counts the rows its answer fills, a long line wrapping', () => {
    expect(rowsOf(undefined)).toBe(0);
    expect(rowsOf('')).toBe(0);
    expect(rowsOf('FACT: one line')).toBe(1);
    expect(rowsOf('FACT: one\n\nFACT: two')).toBe(3);
    expect(rowsOf('x'.repeat(130), 60)).toBe(3);
  });

  it('grows a row at a time as the answer is written, and a tick for each patch it landed', () => {
    const streaming = ['', 'FACT: a', 'FACT: a\nFACT: b', 'FACT: a\nFACT: b\nDECISION: c'].map((text) => barHeight(rowsOf(text), 0));
    for (let i = 1; i < streaming.length; i++) expect(streaming[i]).toBeGreaterThanOrEqual(streaming[i - 1] ?? 0);
    expect(barHeight(40, 0) - barHeight(39, 0)).toBe(ROW);
    expect(barHeight(40, 2) - barHeight(40, 0)).toBe(2 * PATCH_TICK);
  });

  it('is never shorter than the place its cable jacks in', () => {
    expect(barHeight(0, 0)).toBe(MIN_BAR);
    expect(barHeight(1, 0)).toBe(MIN_BAR);
  });
});
