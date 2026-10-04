import type { LineOf, LogLine } from '../drive/log.ts';

/**
 * A session's receipt: the six numbers #31 measures a driven session on,
 * printed beside its record -- side calls per ask, patches per ask, live
 * entries at the end, forks typed `mimicry`, trunk idle before each refill,
 * and the share of side-call time that fell inside a person's gap. The
 * predecessor's first drive is the floor they are read against.
 *
 * Counts and durations, not ratios: the surface divides, so a receipt of a
 * session with no ask yet is still a receipt.
 */
export interface Receipt {
  readonly asks: number;
  readonly sideCalls: number;
  readonly patches: number;
  readonly liveEntries: number;
  readonly mimicry: number;
  /** For each refill, ms from the trunk's last node finishing to the seam. */
  readonly idleBeforeRefill: readonly number[];
  /** Side-call time, ms: each side call from its request to its settling. */
  readonly sideCallMs: number;
  /**
   * The part of it inside a person's gap, ms. A gap the surface measured
   * (`idle.gap`, Q4) counts only its attended time -- notice, read and
   * compose, laid from the settling in that order -- not time blocked on
   * work in flight, nor away. A gap it did not measure runs from the
   * settling to the person's next ask or refill, whole: an upper bound.
   */
  readonly inGapMs: number;
  /** How many gaps were measured, of how many: the sixth number is exact only when all were. */
  readonly gapsMeasured: number;
  readonly gapsTotal: number;
}

type Span = readonly [number, number];

/** Everything but live entries, which come from the fold's working memory. */
export function receiptOf(lines: readonly LogLine[]): Omit<Receipt, 'liveEntries'> {
  const of = <K extends LogLine['kind']>(kind: K) => lines.filter((e): e is LineOf<K> => e.kind === kind);
  const now = lines.at(-1)?.t ?? 0;
  const asks = of('ask');
  const forks = of('fork');
  const settled = new Map(of('fork.settled').map((e) => [e.fork, e] as const));

  // What the trunk did, and when it finished doing it.
  const trunkRequests = new Set(of('request').filter((e) => e.lane === 'trunk').map((e) => e.seq));
  const trunkDone = [
    ...asks.map((e) => e.t),
    ...of('response').filter((e) => trunkRequests.has(e.to_request)).map((e) => e.t),
    ...of('tool_call').map((e) => e.t),
  ];
  const idleBeforeRefill = of('seam').map((seam) => {
    const last = Math.max(...trunkDone.filter((t) => t <= seam.t));
    return Number.isFinite(last) ? seam.t - last : 0;
  });

  // Each side call from its request to its settling (or now, still running).
  const started = new Map(of('request').flatMap((e) => (e.fork !== undefined ? [[e.fork, e.t] as const] : [])));
  const sideCalls: Span[] = forks.flatMap((f) => {
    const from = started.get(f.seq);
    return from === undefined ? [] : [[from, settled.get(f.seq)?.t ?? now] as const];
  });

  // Each person's gap: the turn handed back, to their next ask or refill -- or, measured, its attended part.
  const acts = [...asks, ...of('seam')].map((e) => e.t).sort((a, b) => a - b);
  const measured = new Map(of('idle.gap').map((g) => [g.opened_by, g] as const));
  const settles = of('turn.settled');
  const gaps: Span[] = settles.map((s) => {
    const m = measured.get(s.seq);
    return m ? ([s.t, s.t + m.notice + m.read + m.compose] as const) : ([s.t, acts.find((t) => t > s.t) ?? now] as const);
  });

  const overlap = (a: Span, b: Span) => Math.max(0, Math.min(a[1], b[1]) - Math.max(a[0], b[0]));
  return {
    asks: asks.length,
    sideCalls: forks.length,
    patches: of('patch').length,
    mimicry: [...settled.values()].filter((s) => s.outcome === 'mimicry').length,
    idleBeforeRefill,
    sideCallMs: sideCalls.reduce((sum, [a, b]) => sum + (b - a), 0),
    inGapMs: sideCalls.reduce((sum, call) => sum + gaps.reduce((g, gap) => g + overlap(call, gap), 0), 0),
    gapsMeasured: settles.filter((s) => measured.has(s.seq)).length,
    gapsTotal: settles.length,
  };
}
