/**
 * A side call condensed to a bar: it keeps its place beside the trunk and
 * says how far along it is by its length, not its text. Pure, so the growth
 * is tested without a layout engine.
 */

/** The shortest bar: room for the cable to jack in (placement.ts, ENTER). */
export const MIN_BAR = 24;
/** The tallest bar: past this a side call has written a lot, and the lane says so without stretching. */
export const MAX_BAR = 120;
/** Pixels per row of answer written. */
export const ROW = 3;
/** Pixels per patch landed in working memory, drawn as a tick in the op's colour. */
export const PATCH_TICK = 5;
const BASE = 6;

/** Rows the answer fills: each line, wrapped at `columns`; an empty line is a row too. */
export function rowsOf(text: string | undefined, columns = 60): number {
  if (!text) return 0;
  return text.split('\n').reduce((rows, line) => rows + Math.max(1, Math.ceil(line.length / columns)), 0);
}

/** The bar's height: it grows a row at a time as the answer streams, and a tick per patch, up to a ceiling. */
export function barHeight(rows: number, patches: number): number {
  return Math.min(MAX_BAR, Math.max(MIN_BAR, BASE + rows * ROW + patches * PATCH_TICK));
}
