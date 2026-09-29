import { describe, expect, it } from 'vitest';

import { jump, lens, onMap } from './minimap.ts';

describe('the minimap geometry', () => {
  it('places a span by its share of the stage, never thinner than the minimum', () => {
    expect(onMap({ top: 500, height: 100 }, 1000)).toEqual({ top: 0.5, height: 0.1 });
    expect(onMap({ top: 990, height: 1 }, 10_000, 0.002)).toEqual({ top: 0.099, height: 0.002 });
    expect(onMap({ top: 20, height: 10 }, 0, 0.002)).toEqual({ top: 0, height: 0.002 });
  });

  it('shows the lens where the visible band crosses the stage', () => {
    // Stage 4,000 tall, scrolled so its top is 1,000 above the viewport; visible band 60..860.
    expect(lens(-1000, 4000, 60, 860)).toEqual({ top: 0.265, height: 0.2 });
    // Not scrolled, stage starts below the header: the lens starts at the stage's top.
    expect(lens(100, 4000, 60, 860)).toEqual({ top: 0, height: 0.19 });
    // Scrolled past the end: nothing of the stage is in view.
    expect(lens(-5000, 4000, 60, 860).height).toBe(0);
  });

  it('jumps so the chosen point is centred in the visible band', () => {
    // Stage top at -1,000 while scrollY is 1,100; click halfway: the point is 1,000 below the viewport's top.
    expect(jump(0.5, 1100, -1000, 4000, 60, 860)).toBe(1100 + 1000 - 460);
    expect(jump(0, 0, 100, 4000, 60, 860)).toBe(0);
  });
});
