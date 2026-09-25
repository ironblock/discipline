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

function luminance([r, g, b]: readonly [number, number, number]): number {
  const lin = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b);
}
