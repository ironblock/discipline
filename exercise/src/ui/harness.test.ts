import { describe, expect, it } from 'vitest';

import { HARNESS, draw, route } from './harness.ts';
import type { Net } from './harness.ts';

// Side calls at the left, a 40px gutter, working memory's panel at the right, in viewport pixels.
const gutter = { left: 400, right: 440 };
const panel = { left: 450, right: 800, top: 100, bottom: 700 };
const AT = 15;
const bar = (top: number) => ({ left: 100, right: 380, top, bottom: top + 40 });
const entry = (top: number) => ({ left: 460, right: 790, top, bottom: top + 40 });
const net = (key: string, top: number, ...entries: [string, number][]): Net => ({ key, from: bar(top), to: entries.map(([e, t]) => ({ entry: e, box: entry(t) })) });

// Tracks from the memory side leftwards, `spacing` apart.
const [T0, T1, T2] = [434, 426, 418];

describe('lines into memory, routed as a harness', () => {
  const a = net('a', 300, ['e1', 150]); // runs 165..315
  const b = net('b', 400, ['e2', 200]); // runs 215..415: at once with a
  const c = net('c', 600, ['e3', 550]); // runs 565..615: after a has ended

  it('gives lines that run at once tracks of their own, parallel in the gutter', () => {
    const [ra, rb] = route([a, b], gutter, panel, AT);
    expect([ra?.x, rb?.x]).toEqual([T0, T1]);
  });

  it('reuses a track once the line on it has ended', () => {
    const got = route([a, b, c], gutter, panel, AT);
    expect(got.map((r) => r.x)).toEqual([T0, T1, T0]);
  });

  it('takes the free track that crosses least, not the first free one', () => {
    const p = net('p', 100, ['e0', 110]); // a short run on T0
    const m = net('m', 105, ['m1', 285], ['m2', 305], ['m3', 325]); // at once with p: T1
    const nn = net('n', 200, ['n1', 500]); // at once with m, not p: T0 is free, but m's three stubs would cross it there
    const got = route([p, m, nn], gutter, panel, AT);
    expect(got.map((r) => r.x)).toEqual([T0, T1, T2]);
    expect(draw(got).some((d) => d.d.includes('A2.5'))).toBe(false);
  });

  it('hops a horizontal over a vertical it crosses, or breaks at it', () => {
    const routed = route([a, b], gutter, panel, AT);
    // b's stub into e2, at 215, crosses a's track; a's run out, at 315, crosses b's.
    const [da, db] = draw(routed);
    expect(db?.d).toContain(`L${T0 - 2.5} 215A2.5 2.5 0 0 1 ${T0 + 2.5} 215`);
    expect(da?.d).toContain(`L${T1 - 2.5} 315A2.5 2.5 0 0 1 ${T1 + 2.5} 315`);
    const [, gapped] = draw(routed, { ...HARNESS, crossing: 'gap' });
    expect(gapped?.d).toContain(`L${T0 - 2.5} 215M${T0 + 2.5} 215`);
  });

  it('bends round, chamfered or square', () => {
    const routed = route([a], gutter, panel, AT);
    const bent = (bend: 'round' | 'chamfer' | 'square') => draw(routed, { ...HARNESS, bend })[0]?.d;
    // Out right, up the track, right into the entry: an anticlockwise turn then a clockwise one.
    expect(bent('round')).toBe(`M380 315L429 315A5 5 0 0 0 434 310L434 170A5 5 0 0 1 439 165L460 165`);
    expect(bent('chamfer')).toBe(`M380 315L429 315L434 310L434 170L439 165L460 165`);
    expect(bent('square')).toBe(`M380 315L434 315L434 165L460 165`);
  });

  it('fans out from the track to each entry a side call wrote, a dot where each leaves it', () => {
    const [drawn] = draw(route([net('d', 500, ['e1', 150], ['e2', 250], ['e3', 350])], gutter, panel, AT));
    expect(drawn?.dots).toEqual([
      { x: T0, y: 265 },
      { x: T0, y: 365 },
    ]);
    expect(drawn?.wires.map((w) => w.entry)).toEqual(['e1', 'e2', 'e3']);
  });

  it('carries lines toward entries scrolled out of the panel up its track to the edge, one per edge, dashed past it', () => {
    const [routed] = route([net('d', 300, ['e1', 20], ['e2', 30], ['e3', 400], ['e4', 900])], gutter, panel, AT);
    expect(routed?.pins.map((p) => [p.x, p.y, p.clipped, p.entries])).toEqual([
      [T0, 100, true, ['e1', 'e2']],
      [460, 415, false, ['e3']],
      [T0, 700, true, ['e4']],
    ]);
    // Nothing runs along the panel's edge: no two such lines can cross there.
    const [drawn] = draw(routed ? [routed] : []);
    expect(drawn?.clipped).toBe(`M${T0} 100L${T0} 90M${T0} 700L${T0} 710`);
  });

  it('draws no tail where the track runs on past the edge to its side call anyway', () => {
    // The side call is below the panel; so are the entries it wrote.
    const [drawn] = draw(route([net('d', 800, ['e1', 900])], gutter, panel, AT));
    expect(drawn?.clipped).toBe('');
  });

  it('runs two lines into one entry side by side, not on top of each other', () => {
    const got = route([net('a', 300, ['e1', 150]), net('b', 400, ['e1', 150])], gutter, panel, AT);
    expect(got.map((r) => r.pins[0]?.y)).toEqual([165, 165 + HARNESS.pinStep]);
  });
});
