/**
 * WCAG contrast of an element's text against what is actually behind it, in
 * the browser: every ancestor's background colour painted in order onto a
 * one-pixel canvas (so translucent fills, `color-mix()` and `oklab()` resolve
 * as the page resolves them), then the text colour over that. Background
 * images -- grain, glass, pools -- are ignored: this is the fill's contrast.
 */
export function contrast(el: Element): number {
  const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  if (!ctx) throw new Error('no 2d canvas');
  const paint = (colour: string): readonly [number, number, number] => {
    ctx.fillStyle = colour;
    ctx.fillRect(0, 0, 1, 1);
    const [r = 0, g = 0, b = 0] = ctx.getImageData(0, 0, 1, 1).data;
    return [r, g, b];
  };
  const layers: string[] = [];
  for (let at: Element | null = el; at; at = at.parentElement) layers.unshift(getComputedStyle(at).backgroundColor);
  paint('#000');
  let behind: readonly [number, number, number] = [0, 0, 0];
  for (const colour of layers) behind = paint(colour);
  const text = paint(getComputedStyle(el).color);
  const [hi, lo] = [luminance(text), luminance(behind)].sort((a, b) => b - a) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

/**
 * WCAG contrast of an element's FILL against what is behind it: its own
 * background colour painted over its ancestors', against theirs alone.
 * For marks that carry no text -- a minimap's slivers. With `pseudo`, the
 * mark is that pseudo-element's fill, over the element's own.
 */
export function fillContrast(el: Element, pseudo?: '::before' | '::after'): number {
  const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  if (!ctx) throw new Error('no 2d canvas');
  const paint = (colour: string): readonly [number, number, number] => {
    ctx.fillStyle = colour;
    ctx.fillRect(0, 0, 1, 1);
    const [r = 0, g = 0, b = 0] = ctx.getImageData(0, 0, 1, 1).data;
    return [r, g, b];
  };
  const layers: string[] = [];
  for (let at: Element | null = pseudo ? el : el.parentElement; at; at = at.parentElement) layers.unshift(getComputedStyle(at).backgroundColor);
  paint('#000');
  let behind: readonly [number, number, number] = [0, 0, 0];
  for (const colour of layers) behind = paint(colour);
  const mark = paint(getComputedStyle(el, pseudo).backgroundColor);
  const [hi, lo] = [luminance(mark), luminance(behind)].sort((a, b) => b - a) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

/** WCAG contrast of two colours, as the page resolves them. */
export function colourContrast(a: string, b: string): number {
  const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  if (!ctx) throw new Error('no 2d canvas');
  const paint = (colour: string): readonly [number, number, number] => {
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, 1, 1);
    ctx.fillStyle = colour;
    ctx.fillRect(0, 0, 1, 1);
    const [r = 0, g = 0, b = 0] = ctx.getImageData(0, 0, 1, 1).data;
    return [r, g, b];
  };
  const [hi, lo] = [luminance(paint(a)), luminance(paint(b))].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

function luminance([r, g, b]: readonly [number, number, number]): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}

/**
 * How far apart two marks' colours look (the distance between them in
 * OKLab, where 0.02 is about the least a person can see), each as the page
 * resolves it: its own colour -- the one its strokes take, `currentColor` --
 * painted over every ancestor's background. For lines that say what they
 * are by hue: two lanes' cables.
 */
export function apart(a: Element, b: Element): number {
  const [la, aa, ba] = oklab(painted(a));
  const [lb, ab, bb] = oklab(painted(b));
  return Math.hypot(la - lb, aa - ab, ba - bb);
}

function painted(el: Element): readonly [number, number, number] {
  const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  if (!ctx) throw new Error('no 2d canvas');
  const paint = (colour: string): readonly [number, number, number] => {
    ctx.fillStyle = colour;
    ctx.fillRect(0, 0, 1, 1);
    const [r = 0, g = 0, bl = 0] = ctx.getImageData(0, 0, 1, 1).data;
    return [r, g, bl];
  };
  const layers: string[] = [];
  for (let at: Element | null = el.parentElement; at; at = at.parentElement) layers.unshift(getComputedStyle(at).backgroundColor);
  paint('#000');
  for (const colour of layers) paint(colour);
  return paint(getComputedStyle(el).color);
}

function oklab([r, g, b]: readonly [number, number, number]): readonly [number, number, number] {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const [lr, lg, lb] = [lin(r), lin(g), lin(b)];
  const l = Math.cbrt(0.4122214708 * lr + 0.5363019296 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s];
}
