import type { Box } from './links.ts';

/**
 * Lines routed as a wiring harness, not as curves -- into working memory
 * (`route`, measured by `Links.tsx`) and from the trunk to its side calls
 * (`cabling`, measured by `SessionView`): pure, so tested without a layout
 * engine.
 *
 * Channel routing, as on a circuit board: each net's lines share one
 * vertical TRACK in a gutter and fan out from it to where they go. Tracks
 * are assigned greedily in the order the runs start down the page (the
 * left-edge algorithm: a track is reused once the run on it has ended),
 * each taking the free track that crosses the fewest lines already placed;
 * when more run at once than the gutter has room for, the tracks close up.
 * Lines into memory move as the page scrolls past memory's panel, so each
 * keeps the track it had while that stays free, and costs no more than a
 * couple of crossings over routing afresh. So lines run parallel and never share a track; where one must cross
 * another, the horizontal hops the vertical (or breaks, as a gap). Corners
 * are round, chamfered or square.
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
  /** Its track, counted from the gutter's right; and where that is. */
  readonly track: number;
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
/** How many more crossings kept tracks may cost than routing afresh would, before they are let go. */
const SLACK = 2;
/** How far a line toward entries out of the panel runs on past its edge, dashed. */
const TAIL = 10;

/** Assign each net a track in `gutter` and a pin on each entry it wrote. */
export function route(
  nets: readonly Net[],
  gutter: { readonly left: number; readonly right: number },
  panel: Box,
  at: number,
  o: Options = HARNESS,
  /** The track each net had last time round (by key): kept while free. */
  keep?: ReadonlyMap<string, number>,
): Routed[] {
  const order = [...nets].sort((a, b) => a.from.top - b.from.top);

  // Pins: one per entry on screen; one per edge for those scrolled out.
  const pinned = order.map((net) => {
    const byPlace = new Map<string, { y: number; x: number; clipped: boolean; toward?: -1 | 1; entries: string[] }>();
    for (const { entry, box } of net.to) {
      const want = box.top + at;
      const clipped = want < panel.top ? 'top' : want > panel.bottom ? 'bottom' : undefined;
      const place = clipped ?? `entry:${entry}`;
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

  return lay(
    order.map((net, i) => ({ key: net.key, source: { x: net.from.right, y: net.from.top + at }, pins: pins[i] ?? [] })),
    gutter,
    o,
    keep,
  );
}

/**
 * A side call off the trunk, placed (placement.ts): its cable leaves the
 * trunk's edge at `leave` and enters the side call at `enter`.
 */
export interface Tap {
  readonly id: string;
  /** The trunk node it came from. */
  readonly anchor: string;
  readonly slot: number;
  readonly leave: number;
  readonly enter: Pt;
  /** Waiting for its slot: its cable is laid apart from those that ran. */
  readonly pending: boolean;
}

/** A net of the trunk's cables, routed; `pending` when its side calls wait for their slot. */
export interface TrunkNet extends Routed {
  readonly pending: boolean;
}

/**
 * The cables from the trunk as a harness: in the gutter before each slot's
 * column, one net per trunk node, forking to each side call off it in that
 * slot. A net leaves the trunk where its highest cable would have. Side calls
 * waiting for their slot make a net of their own, and it says so.
 */
export function cabling(
  taps: readonly Tap[],
  trunkRight: number,
  gutters: ReadonlyMap<number, { readonly left: number; readonly right: number }>,
  o: Options = HARNESS,
): TrunkNet[] {
  const nets = new Map<string, { slot: number; pending: boolean; taps: Tap[] }>();
  for (const tap of taps) {
    // Its own key, so that a waiting net and one that ran off the same node are two; nothing reads it back.
    const key = `${tap.slot}>${tap.anchor}${tap.pending ? '>pending' : ''}`;
    const net = nets.get(key) ?? { slot: tap.slot, pending: tap.pending, taps: [] };
    net.taps.push(tap);
    nets.set(key, net);
  }
  const laid = [...gutters].flatMap(([slot, gutter]) =>
    lay(
      [...nets].flatMap(([key, net]) =>
        net.slot === slot
          ? [
              {
                key,
                source: { x: trunkRight, y: Math.min(...net.taps.map((t) => t.leave)) },
                pins: net.taps.map((t) => ({ ...t.enter, entries: [t.id], clipped: false })),
              },
            ]
          : [],
      ),
      gutter,
      o,
    ),
  );
  return laid.map((net) => ({ ...net, pending: nets.get(net.key)?.pending ?? false }));
}

/**
 * Give each net a track in `gutter`, left-edge style: in the order their
 * runs start down the page, each takes the free track (one whose run has
 * ended) that crosses fewest of the nets already placed.
 */
function lay(
  nets: readonly { readonly key: string; readonly source: Pt; readonly pins: readonly Pin[] }[],
  gutter: { readonly left: number; readonly right: number },
  o: Options,
  keep?: ReadonlyMap<string, number>,
): Routed[] {
  // Afresh, there is always a free track: as many are made as ever run at once.
  const fresh = assign(nets, gutter, o) ?? [];
  if (!keep) return fresh;
  // Kept tracks hold still as the page scrolls, until they cost more than SLACK crossings over a fresh routing.
  const kept = assign(nets, gutter, o, keep);
  return kept && tangle(kept) <= tangle(fresh) + SLACK ? kept : fresh;
}

/** How many times the nets of one routing cross each other. */
function tangle(routed: readonly Routed[]): number {
  return routed.reduce((sum, n, i) => sum + crossings(n, routed.slice(0, i)), 0);
}

/** One routing: afresh, or keeping the tracks in `keep` while they are free (undefined if that leaves a net nowhere free). */
function assign(
  nets: readonly { readonly key: string; readonly source: Pt; readonly pins: readonly Pin[] }[],
  gutter: { readonly left: number; readonly right: number },
  o: Options,
  keep?: ReadonlyMap<string, number>,
): Routed[] | undefined {
  const clear = o.pinStep;
  const spans = nets
    .map((net) => {
      const ys = [net.source.y, ...net.pins.map((p) => p.y)];
      return { net, lo: Math.min(...ys), hi: Math.max(...ys) };
    })
    // In order of where each run starts: then first-fit needs no more tracks than run at once.
    .sort((a, b) => a.lo - b.lo);
  // As many tracks as ever run at once; closer than `spacing` if the gutter is short of room, never shared.
  let most = 0;
  for (const { lo } of spans) most = Math.max(most, spans.filter((s) => s.lo <= lo && lo < s.hi + clear).length);
  const room = Math.max(0, gutter.right - gutter.left - 2 * EDGE);
  const fits = Math.floor(room / o.spacing) + 1;
  const spacing = most > fits ? room / Math.max(1, most - 1) : o.spacing;
  const count = Math.max(fits, most);
  const xOf = (t: number) => gutter.right - EDGE - t * spacing;
  const placed: Routed[] = [];
  const onTrack: [number, number][][] = Array.from({ length: count }, () => []);
  const overlap = (t: number, lo: number, hi: number) =>
    (onTrack[t] ?? []).reduce((sum, [a, b]) => sum + Math.max(0, Math.min(b, hi + clear) - Math.max(a, lo - clear)), 0);
  const put = ({ net, lo, hi }: (typeof spans)[number], t: number) => {
    // An edge pin's x is its track's.
    const pins = net.pins.map((p) => (p.clipped ? { ...p, x: xOf(t) } : p));
    onTrack[t]?.push([lo, hi]);
    placed.push({ key: net.key, track: t, x: xOf(t), source: net.source, pins, lo, hi });
  };

  // First, each net that had a track keeps it, while it is free.
  const rest = spans.filter((span) => {
    const t = keep?.get(span.net.key);
    if (t === undefined || t >= count || overlap(t, span.lo, span.hi) > 0) return true;
    put(span, t);
    return false;
  });
  // Then the rest, each on the free track that crosses least.
  for (const span of rest) {
    const { net, lo, hi } = span;
    let best = { t: 0, overlap: Number.POSITIVE_INFINITY, cost: Number.POSITIVE_INFINITY };
    for (let t = 0; t < count; t++) {
      const pins = net.pins.map((p) => (p.clipped ? { ...p, x: xOf(t) } : p));
      const cost = crossings({ key: net.key, track: t, x: xOf(t), source: net.source, pins, lo, hi }, placed);
      const shared = overlap(t, lo, hi);
      if (shared < best.overlap || (shared === best.overlap && cost < best.cost)) best = { t, overlap: shared, cost };
    }
    // Kept tracks can leave no free one where a fresh routing would: then they are let go, never shared.
    if (best.overlap > 0 && keep) return undefined;
    put(span, best.t);
  }
  // In page order, as they were given.
  return spans.flatMap(({ net }) => placed.filter((r) => r.key === net.key));
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
    return Math.max(0, Math.min(o.radius, o.spacing - o.hop - 0.5, len(prev, p) / 2, len(p, next) / 2));
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
