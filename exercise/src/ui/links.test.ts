import { describe, expect, it } from 'vitest';

import { link } from './links.ts';

// A side call's bar at the left, working memory's panel at the right, in viewport pixels.
const from = { left: 100, right: 400, top: 200, bottom: 240 };
const panel = { left: 500, right: 800, top: 100, bottom: 700 };

describe('a line from a side call to the entry it wrote', () => {
  it('leaves the side call’s right edge at its bar and enters the entry’s left edge', () => {
    const got = link(from, { left: 520, right: 780, top: 300, bottom: 340 }, panel, 15);
    expect(got.start).toEqual({ x: 400, y: 215 });
    expect(got.end).toEqual({ x: 520, y: 315 });
    expect(got.clipped).toBe(false);
    // A horizontal S: out level, in level.
    expect(got.d).toBe('M400 215C460 215 460 315 520 315');
  });

  it('stops at the panel’s edge when the entry is scrolled out of it, and says so', () => {
    const above = link(from, { left: 520, right: 780, top: 20, bottom: 60 }, panel, 15);
    expect(above.end).toEqual({ x: 520, y: 100 });
    expect(above.clipped).toBe(true);
    const below = link(from, { left: 520, right: 780, top: 900, bottom: 940 }, panel, 15);
    expect(below.end.y).toBe(700);
    expect(below.clipped).toBe(true);
  });
});
