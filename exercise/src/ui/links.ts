/**
 * The line from a side call to a working-memory entry it wrote: pure, so it
 * is tested without a layout engine. `Links.tsx` measures the page and hands
 * the rectangles in here, in viewport pixels.
 */

export interface Box {
  readonly left: number;
  readonly right: number;
  readonly top: number;
  readonly bottom: number;
}

export interface Line {
  readonly d: string;
  readonly start: { readonly x: number; readonly y: number };
  readonly end: { readonly x: number; readonly y: number };
  /** The entry is scrolled out of memory's panel: the line stops at the panel's edge. */
  readonly clipped: boolean;
}

/**
 * Out of the side call's right edge `at` below its top (its bar), into the
 * entry's left edge as far below its own top, as a horizontal S. An entry
 * outside the panel's visible band is reached at the band's edge instead.
 */
export function link(from: Box, to: Box, panel: Box, at: number): Line {
  const start = { x: from.right, y: from.top + at };
  const want = to.top + at;
  const y = Math.min(panel.bottom, Math.max(panel.top, want));
  const end = { x: to.left, y };
  const mid = (start.x + end.x) / 2;
  return {
    d: `M${n(start.x)} ${n(start.y)}C${n(mid)} ${n(start.y)} ${n(mid)} ${n(end.y)} ${n(end.x)} ${n(end.y)}`,
    start,
    end,
    clipped: y !== want,
  };
}

function n(x: number): string {
  return String(Math.round(x * 10) / 10);
}
