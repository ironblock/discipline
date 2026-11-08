import { describe, expect, it } from 'vitest';

import { dragged, jump, lens, onMap, picture } from './minimap.ts';

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

  // VS Code's proportional minimap (#1084's one scale, scrolling): a stage 50,000 tall, its trunk 760 wide drawn 200
  // wide, on a map 800 tall; the page shows a band 800 tall.
  const long = (from: number) => picture(50_000, 760, 200, 800, -from, 0, 800);
  const scale = 200 / 760;

  it('keeps one scale for both axes on a long column: no squash', () => {
    for (const from of [0, 20_000, 49_200]) {
      const at = long(from);
      expect(at.scale).toBeCloseTo(scale, 9);
      expect(at.lens.height).toBeCloseTo(800 * scale, 6);
    }
  });

  it('scrolls the map in step with the page, the lens over what the page shows', () => {
    const top = long(0);
    expect(top.offset).toBe(0);
    expect(top.lens.top).toBe(0);
    const end = long(49_200);
    // At the end the lens is at the map's foot and the map shows its last 800 pixels.
    expect(end.lens.top + end.lens.height).toBeCloseTo(800, 6);
    expect(end.offset).toBeCloseTo(50_000 * scale - 800, 6);
    for (const from of [0, 7_000, 20_000, 33_333, 49_200]) {
      const at = long(from);
      // The lens sits at the same relative place the page does...
      expect(at.lens.top / (800 - at.lens.height)).toBeCloseTo(from / (50_000 - 800), 9);
      // ...over the content the page shows: the stage's `from` drawn at the lens's top.
      expect(from * at.scale - at.offset).toBeCloseTo(at.lens.top, 6);
    }
  });

  it('does not scroll a column that fits, and a drag moves the page by the lens’s ratio', () => {
    const short = picture(1_000, 760, 200, 800, 0, 0, 800);
    expect(short.offset).toBe(0);
    expect(short.lens).toEqual({ top: 0, height: 800 * scale });
    // A drag of the lens by d map pixels moves the page by d over the lens's ratio, as VS Code's slider does.
    const at = long(20_000);
    expect(dragged(at, 10)).toBeCloseTo((10 * (50_000 - 800)) / (800 - at.lens.height), 6);
    expect(dragged(short, 10)).toBeCloseTo(10 / scale, 6);
  });
});
