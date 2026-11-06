/**
 * The minimap's geometry: pure, so it is tested without a layout engine.
 * `Minimap.tsx` measures the page and hands the numbers in here.
 *
 * Everything is in fractions of the stage's height (0..1), so nothing crosses
 * between the page's pixels and the map's: the map multiplies by its own
 * height when it draws.
 */

/** A measured span of the stage, in page pixels from the stage's top. */
export interface Span {
  readonly top: number;
  readonly height: number;
}

/** Where a span sits on the map, as fractions of the map's height. Never thinner than `min`. */
export function onMap(span: Span, stageHeight: number, min = 0): { readonly top: number; readonly height: number } {
  if (stageHeight <= 0) return { top: 0, height: min };
  return { top: clamp(span.top / stageHeight, 0, 1), height: Math.max(min, span.height / stageHeight) };
}

/**
 * The height the map's picture spans, in stage pixels: what `onMap` divides by. The map draws the column at one scale
 * for both axes -- a trunk mark's width over the trunk's -- so each block keeps its message's shape, and a short
 * column fills only the top of the map. A column whose picture at that scale is taller than the map is fitted to the
 * map's height instead, as the map always drew it: still one scale for every block, each squashed alike.
 */
export function pictured(stageHeight: number, columnWidth: number, markWidth: number, mapHeight: number): number {
  if (columnWidth <= 0 || markWidth <= 0 || mapHeight <= 0) return stageHeight;
  return Math.max(stageHeight, (mapHeight * columnWidth) / markWidth);
}

/**
 * The lens: the part of the stage in view. `stageTop` is the stage's top
 * edge relative to the viewport's (negative once scrolled past it);
 * `viewTop`/`viewBottom` are the band of the viewport the stage can be seen
 * in -- below the sticky header, above the composer.
 */
export function lens(stageTop: number, stageHeight: number, viewTop: number, viewBottom: number): { readonly top: number; readonly height: number } {
  if (stageHeight <= 0) return { top: 0, height: 1 };
  const from = clamp(viewTop - stageTop, 0, stageHeight);
  const to = clamp(viewBottom - stageTop, 0, stageHeight);
  return { top: from / stageHeight, height: Math.max(0, to - from) / stageHeight };
}

/**
 * Where to scroll so the point at fraction `at` of the stage sits in the
 * middle of the visible band. `scrollY` is the page's scroll now.
 */
export function jump(at: number, scrollY: number, stageTop: number, stageHeight: number, viewTop: number, viewBottom: number): number {
  const point = stageTop + clamp(at, 0, 1) * stageHeight;
  return Math.max(0, scrollY + point - (viewTop + viewBottom) / 2);
}

function clamp(n: number, lo: number, hi: number): number {
  return Math.min(hi, Math.max(lo, n));
}
