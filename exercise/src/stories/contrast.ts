/**
 * WCAG contrast of an element's text against what is actually behind it, in
 * the browser: every ancestor's background colour painted in order onto a
 * one-pixel canvas (so translucent fills, `color-mix()` and `oklab()` resolve
 * as the page resolves them), then the text colour over that. Background
 * images -- grain, glass, pools -- are ignored: this is the fill's contrast.
 */
export function contrast(el: Element): number {
  const paint = painter();
  const behind = over(paint, el);
  return ratio(paint(getComputedStyle(el).color), behind);
}

/**
 * WCAG contrast of an element's FILL against what is behind it: its own
 * background colour painted over its ancestors', against theirs alone.
 * For marks that carry no text -- a minimap's slivers. With `pseudo`, the
 * mark is that pseudo-element's fill, over the element's own.
 */
export function fillContrast(el: Element, pseudo?: '::before' | '::after'): number {
  const paint = painter();
  const behind = over(paint, pseudo ? el : el.parentElement);
  return ratio(paint(getComputedStyle(el, pseudo).backgroundColor), behind);
}

/** WCAG contrast of two colours, as the page resolves them. */
export function colourContrast(a: string, b: string): number {
  const paint = painter();
  const on = (colour: string) => (paint('#000'), paint(colour));
  return ratio(on(a), on(b));
}

type Rgb = readonly [number, number, number];

/** A one-pixel canvas: paint a colour over what is there, and read back what the pixel became. */
function painter(): (colour: string) => Rgb {
  const ctx = document.createElement('canvas').getContext('2d', { willReadFrequently: true });
  if (!ctx) throw new Error('no 2d canvas');
  return (colour) => {
    ctx.fillStyle = colour;
    ctx.fillRect(0, 0, 1, 1);
    const [r = 0, g = 0, b = 0] = ctx.getImageData(0, 0, 1, 1).data;
    return [r, g, b];
  };
}

/** Black, then every background from the root down to `from`, painted in order: what shows behind `from`'s content. */
function over(paint: (colour: string) => Rgb, from: Element | null): Rgb {
  const layers: string[] = [];
  for (let at = from; at; at = at.parentElement) layers.unshift(getComputedStyle(at).backgroundColor);
  let behind = paint('#000');
  for (const colour of layers) behind = paint(colour);
  return behind;
}

function ratio(a: Rgb, b: Rgb): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
  return (hi + 0.05) / (lo + 0.05);
}

/** sRGB to linear light, IEC 61966-2-1's threshold. */
const linear = (c: number) => {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
};

function luminance([r, g, b]: Rgb): number {
  return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
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

function painted(el: Element): Rgb {
  const paint = painter();
  over(paint, el.parentElement);
  return paint(getComputedStyle(el).color);
}

function oklab([r, g, b]: Rgb): Rgb {
  const [lr, lg, lb] = [linear(r), linear(g), linear(b)];
  const l = Math.cbrt(0.4122214708 * lr + 0.5363019296 * lg + 0.0514459929 * lb);
  const m = Math.cbrt(0.2119034982 * lr + 0.6806995451 * lg + 0.1073969566 * lb);
  const s = Math.cbrt(0.0883024619 * lr + 0.2817188376 * lg + 0.6299787005 * lb);
  return [0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s, 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s, 0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s];
}
