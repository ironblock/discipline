import type { Box } from './links.ts';

/**
 * Lines into working memory routed as a wiring harness, not as curves: pure,
 * so it is tested without a layout engine (`Links.tsx` measures, this routes).
 *
 * Channel routing, as on a circuit board: each side call's lines share one
 * vertical TRACK in the gutter before memory, and fan out from it to the
 * entries they wrote. Tracks are assigned greedily in the order the side
 * calls sit on the page (the left-edge algorithm: a track is reused once
 * the run on it has ended), each taking the free track that crosses the
 * fewest lines already placed. So lines run parallel and never share a
 * track; where one must cross another, the horizontal hops the vertical
 * (or breaks, as a gap). Corners are round, chamfered or square.
 */

export interface Pt {
  readonly x: number;
  readonly y: number;
}

/** A side call and the entries it wrote. */
export interface Net {
  readonly key: string;
  /** Its bar: lines leave its right edge `at` below its top. */
  readonly from: Box;
  readonly to: readonly { readonly entry: string; readonly box: Box }[];
}

export type Crossing = 'hop' | 'gap';
export type Bend = 'round' | 'chamfer' | 'square';

export interface Options {
  /** Space between tracks. */
  readonly spacing: number;
  /** Space between two lines into one entry. */
  readonly pinStep: number;
  /** Corner radius (or chamfer length). */
  readonly radius: number;
  /** A hop's height, and half its width over a single line. */
  readonly hop: number;
  readonly crossing: Crossing;
  readonly bend: Bend;
}

export const HARNESS: Options = { spacing: 8, pinStep: 4, radius: 5, hop: 2.5, crossing: 'hop', bend: 'round' };

/**
 * Where a net enters memory: an entry on screen, or -- for those scrolled
 * out of the panel -- its own track, carried on to the panel's edge.
 */
export interface Pin extends Pt {
  readonly entries: readonly string[];
  readonly clipped: boolean;
  /** Which way the entries out of the panel lie: up (-1) or down (1). */
  readonly toward?: -1 | 1;
}

export interface Routed {
  readonly key: string;
  /** Its track. */
  readonly x: number;
  readonly source: Pt;
  readonly pins: readonly Pin[];
  /** Its vertical run on the track, top to bottom. */
  readonly lo: number;
  readonly hi: number;
}

export interface Drawn {
  readonly key: string;
  /** The whole net, each stretch once. */
  readonly d: string;
  /** Its tails past the panel's edge, toward entries scrolled out of it. */
  readonly clipped: string;
  /** Where it forks: a line leaves the track mid-run. */
  readonly dots: readonly Pt[];
  /** Each entry's own line, source to pin, for lighting one. */
  readonly wires: readonly { readonly entry: string; readonly d: string; readonly clipped: boolean }[];
}

const EDGE = 6;
/** How far a line toward entries out of the panel runs on past its edge, dashed. */
const TAIL = 10;

/** Assign each net a track in `gutter` and a pin on each entry it wrote. */
export function route(nets: readonly Net[], gutter: { readonly left: number; readonly right: number }, panel: Box, at: number, o: Options = HARNESS): Routed[] {
  const order = [...nets].sort((a, b) => a.from.top - b.from.top);

  // Pins: one per entry on screen; one per edge for those scrolled out.
  const pinned = order.map((net) => {
    const byPlace = new Map<string, { y: number; x: number; clipped: boolean; toward?: -1 | 1; entries: string[] }>();
    for (const { entry, box } of net.to) {
      const want = box.top + at;
      const clipped = want < panel.top ? 'top' : want > panel.bottom ? 'bottom' : undefined;
      const place = clipped ?? `entry:${entry}`;
      // An edge pin's x is its track's, known once the track is.
      const pin = byPlace.get(place) ??
        (clipped
          ? { y: clipped === 'top' ? panel.top : panel.bottom, x: box.left, clipped: true, toward: clipped === 'top' ? -1 : 1, entries: [] }
          : { y: want, x: box.left, clipped: false, entries: [] });
      if (!pin.entries.includes(entry)) pin.entries.push(entry);
      byPlace.set(place, pin);
    }
    return [...byPlace.entries()];
  });
  // Two lines into one entry run parallel, `pinStep` apart, in page order.
  const writers = new Map<string, number>();
  const pins = pinned.map((placed) =>
    placed.map(([place, pin]) => {
      if (pin.clipped) return pin satisfies Pin;
      const k = writers.get(place) ?? 0;
      writers.set(place, k + 1);
      return { ...pin, y: pin.y + k * o.pinStep } satisfies Pin;
    }),
  );

  const count = Math.max(1, Math.floor((gutter.right - gutter.left - 2 * EDGE) / o.spacing) + 1);
  const xOf = (t: number) => gutter.right - EDGE - t * o.spacing;
  const placed: Routed[] = [];
  const onTrack: [number, number][][] = Array.from({ length: count }, () => []);
  const clear = o.pinStep;

  order.forEach((net, i) => {
    const onto = (x: number) => (pins[i] ?? []).map((p) => (p.clipped ? { ...p, x } : p));
    const netPins = onto(0);
    const source = { x: net.from.right, y: net.from.top + at };
    const ys = [source.y, ...netPins.map((p) => p.y)];
    const lo = Math.min(...ys);
    const hi = Math.max(...ys);
    let best = { t: 0, overlap: Number.POSITIVE_INFINITY, cost: Number.POSITIVE_INFINITY };
    for (let t = 0; t < count; t++) {
      const overlap = (onTrack[t] ?? []).reduce((sum, [a, b]) => sum + Math.max(0, Math.min(b, hi + clear) - Math.max(a, lo - clear)), 0);
      const cost = crossings({ key: net.key, x: xOf(t), source, pins: onto(xOf(t)), lo, hi }, placed);
      if (overlap < best.overlap || (overlap === best.overlap && cost < best.cost)) best = { t, overlap, cost };
    }
    onTrack[best.t]?.push([lo, hi]);
    placed.push({ key: net.key, x: xOf(best.t), source, pins: onto(xOf(best.t)), lo, hi });
  });
  return placed;
}

/** The horizontal stretches of a net: out of its side call, and into each pin. */
function horizontals(n: Routed): { y: number; a: number; b: number }[] {
  return [{ y: n.source.y, a: n.source.x, b: n.x }, ...n.pins.map((p) => ({ y: p.y, a: n.x, b: p.x }))];
}

function between(v: number, a: number, b: number, margin = 0.5): boolean {
  return v > Math.min(a, b) + margin && v < Math.max(a, b) - margin;
}

/** How many times `n` would cross the nets already placed. */
function crossings(n: Routed, placed: readonly Routed[]): number {
  let count = 0;
  for (const m of placed) {
    for (const h of horizontals(n)) if (between(m.x, h.a, h.b) && between(h.y, m.lo, m.hi)) count++;
    for (const h of horizontals(m)) if (between(n.x, h.a, h.b) && between(h.y, n.lo, n.hi)) count++;
  }
  return count;
}

/** Draw routed nets: corners as `bend`, a horizontal over another's vertical as `crossing`. */
export function draw(routed: readonly Routed[], o: Options = HARNESS): Drawn[] {
  return routed.map((n) => {
    const others = routed.filter((m) => m !== n);
    const over = (y: number, a: number, b: number) => others.filter((m) => between(m.x, a, b, o.hop) && between(y, m.lo, m.hi)).map((m) => m.x);
    const path = (points: readonly Pt[]) => polyline(points, over, o);

    const up = n.pins.filter((p) => p.y < n.source.y - 0.5);
    const down = n.pins.filter((p) => p.y > n.source.y + 0.5);
    const level = n.pins.filter((p) => !up.includes(p) && !down.includes(p));
    const first = n.pins.find((p) => p.y === n.lo);
    const last = n.pins.find((p) => p.y === n.hi);
    const solid: string[] = [];
    const clipped: string[] = [];
    const dots: Pt[] = [];
    const stub = (p: Pin) => solid.push(path([{ x: n.x, y: p.y }, p]));
    // Past the edge, dashed -- unless the track already runs on that way, to its side call.
    for (const p of n.pins) if (p.toward && (p.toward < 0 ? p.y <= n.lo : p.y >= n.hi)) clipped.push(path([p, { x: p.x, y: p.y + p.toward * TAIL }]));
    const turn = (p: Pin) => [{ x: n.x, y: p.y }, p];
    const fork = (p: Pin) => {
      stub(p);
      dots.push({ x: n.x, y: p.y });
    };

    if (up.length > 0 && down.length > 0) {
      // Pins both ways: the track runs end to end, the side call joins it mid-run.
      if (first && last) solid.push(path([first, { x: n.x, y: first.y }, { x: n.x, y: last.y }, last]));
      solid.push(path([n.source, { x: n.x, y: n.source.y }]));
      dots.push({ x: n.x, y: n.source.y });
      for (const p of n.pins) if (p !== first && p !== last) fork(p);
    } else {
      // One way: out, along the track, and into the farthest pin; the rest fork off it.
      const far = up.length > 0 ? first : last;
      solid.push(path([n.source, { x: n.x, y: n.source.y }, ...(far ? turn(far) : [])]));
      for (const p of [...up, ...down, ...level]) if (p !== far) fork(p);
    }

    return {
      key: n.key,
      d: solid.join(''),
      clipped: clipped.join(''),
      dots,
      wires: n.pins.flatMap((p) => {
        const d = path([n.source, { x: n.x, y: n.source.y }, { x: n.x, y: p.y }, p]);
        return p.entries.map((entry) => ({ entry, d, clipped: p.clipped }));
      }),
    };
  });
}

/**
 * A path through `points` (each stretch horizontal or vertical), its corners
 * bent as `o.bend`, and each horizontal stretch hopping (or breaking at)
 * the verticals `over` reports. Hops closer than a hop's width bridge as one.
 */
function polyline(raw: readonly Pt[], over: (y: number, a: number, b: number) => number[], o: Options): string {
  const points = raw.filter((p, i) => i === 0 || Math.abs(p.x - (raw[i - 1]?.x ?? 0)) + Math.abs(p.y - (raw[i - 1]?.y ?? 0)) > 0.5);
  const [start] = points;
  if (!start || points.length < 2) return '';
  const len = (a: Pt, b: Pt) => Math.hypot(b.x - a.x, b.y - a.y);
  const dir = (a: Pt, b: Pt) => ({ x: Math.sign(b.x - a.x), y: Math.sign(b.y - a.y) });
  // How far each corner is cut back along the stretches either side of it.
  const cut = points.map((p, i) => {
    const prev = points[i - 1];
    const next = points[i + 1];
    if (!prev || !next || o.bend === 'square') return 0;
    // No wider than leaves room to hop the next track over, beside the corner.
    return Math.min(o.radius, o.spacing - o.hop - 0.5, len(prev, p) / 2, len(p, next) / 2);
  });

  let d = `M${n(start.x)} ${n(start.y)}`;
  for (let i = 0; i < points.length - 1; i++) {
    const a = points[i] as Pt;
    const b = points[i + 1] as Pt;
    const u = dir(a, b);
    const from = { x: a.x + u.x * (cut[i] ?? 0), y: a.y + u.y * (cut[i] ?? 0) };
    const to = { x: b.x - u.x * (cut[i + 1] ?? 0), y: b.y - u.y * (cut[i + 1] ?? 0) };
    if (u.y === 0 && u.x !== 0) d += across(from, to, u.x, over(a.y, from.x, to.x), o);
    d += `L${n(to.x)} ${n(to.y)}`;
    const next = points[i + 2];
    const r = cut[i + 1] ?? 0;
    if (next && r > 0) {
      const v = dir(b, next);
      const out = { x: b.x + v.x * r, y: b.y + v.y * r };
      // Turning clockwise on screen (y down) is SVG's positive sweep.
      const sweep = u.x * v.y - u.y * v.x > 0 ? 1 : 0;
      d += o.bend === 'round' ? `A${n(r)} ${n(r)} 0 0 ${sweep} ${n(out.x)} ${n(out.y)}` : `L${n(out.x)} ${n(out.y)}`;
    }
  }
  return d;
}

/** Along a horizontal stretch, over each crossing: a hop, or a break. */
function across(from: Pt, to: Pt, way: number, xs: readonly number[], o: Options): string {
  const sorted = [...xs].sort((a, b) => (a - b) * way);
  // Crossings closer than a hop's width share one bridge.
  const spans: [number, number][] = [];
  for (const x of sorted) {
    const last = spans.at(-1);
    if (last && Math.abs(x - last[1]) < 2 * o.hop + 1) last[1] = x;
    else spans.push([x, x]);
  }
  let d = '';
  for (const [first, last] of spans) {
    const a = first - way * o.hop;
    const b = last + way * o.hop;
    if ((a - from.x) * way < 0 || (to.x - b) * way < 0) continue;
    d += `L${n(a)} ${n(from.y)}`;
    // Over the top: moving right, a hop turns clockwise on screen.
    d += o.crossing === 'hop' ? `A${n(Math.abs(b - a) / 2)} ${n(o.hop)} 0 0 ${way > 0 ? 1 : 0} ${n(b)} ${n(from.y)}` : `M${n(b)} ${n(from.y)}`;
  }
  return d;
}

function n(x: number): string {
  return String(Math.round(x * 10) / 10);
}
