/**
 * The minimap's geometry: pure, so it is tested without a layout engine.
 * `Minimap.tsx` measures the page and hands the numbers in here.
 *
 * `onMap`, `lens` and `jump` are in fractions of the stage's height (0..1);
 * `picture` is in the map's own pixels, at one scale for both axes.
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
 * The map's picture of the stage, as VS Code's proportional minimap draws its editor (`minimap.ts`'s
 * `MinimapLayout.create`), in map pixels: one `scale` for both axes -- a trunk mark's width over the trunk's -- so
 * every block keeps its message's shape, never squashed; the `lens` over the band the page shows; and the `offset` the
 * map's content is scrolled by. A stage whose picture fits the map is drawn from its top, unscrolled. A taller one
 * scrolls in step with the page: the lens sits at the same relative place in the map as the view does in the stage --
 * VS Code's `sliderTop = scrollTop * computedSliderRatio` -- and the content scrolls so the lens covers exactly what
 * the page shows. `stageTop`, `viewTop` and `viewBottom` are as `lens` takes them; `ratio` is how far the lens moves
 * for each pixel the page scrolls, for `dragged`.
 */
export interface Picture {
  readonly scale: number;
  readonly offset: number;
  readonly lens: { readonly top: number; readonly height: number };
  readonly ratio: number;
}

export function picture(stageHeight: number, columnWidth: number, markWidth: number, mapHeight: number, stageTop: number, viewTop: number, viewBottom: number): Picture {
  const scale = columnWidth > 0 && markWidth > 0 ? markWidth / columnWidth : stageHeight > 0 ? mapHeight / stageHeight : 1;
  const from = clamp(viewTop - stageTop, 0, Math.max(0, stageHeight));
  const to = clamp(viewBottom - stageTop, 0, Math.max(0, stageHeight));
  const height = Math.max(0, to - from) * scale;
  const content = stageHeight * scale;
  if (content <= mapHeight) return { scale, offset: 0, lens: { top: from * scale, height }, ratio: scale };
  // The page can scroll the stage through `travel`; the lens moves through the map's height less its own.
  const travel = Math.max(1, stageHeight - (viewBottom - viewTop));
  const ratio = Math.max(0, mapHeight - height) / travel;
  const top = clamp(from * ratio, 0, Math.max(0, mapHeight - height));
  return { scale, offset: clamp(from * scale - top, 0, content - mapHeight), lens: { top, height }, ratio };
}

/** How far the page scrolls for a drag of the lens by `delta` map pixels: VS Code's `delta / computedSliderRatio`. */
export function dragged(at: Picture, delta: number): number {
  return at.ratio > 0 ? delta / at.ratio : 0;
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
