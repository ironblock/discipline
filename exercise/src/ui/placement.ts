/**
 * Where a side call is drawn: pure, so the rule is tested without a layout
 * engine. `SessionView` measures the page and hands the numbers in here.
 *
 * The page's vertical axis is time. A side call starts below the bottom of
 * everything that had finished when its request started -- trunk nodes and
 * other side calls alike -- so it sits level with whatever ran beside it,
 * and never above its own trunk node or an earlier side call in its slot.
 * One still waiting for its slot (a fork with no request yet) is not
 * pinned: it collects below everything finished so far, and follows the
 * work down until it starts. Its cable runs back up to the trunk node it
 * came from, however far.
 */

/** Where the cable enters a side call: the middle of its thin bar, from its top. */
export const ENTER = 15;
/** Where the cable leaves a trunk node it has run past: its footer, from its bottom. */
export const LEAVE = 17;

/** A drawn trunk node, in page pixels from the stage's top. */
export interface Span {
  readonly id: string;
  readonly top: number;
  readonly bottom: number;
  /** Session time it finished; absent while it runs. */
  readonly endedAt?: number;
}

/** A side call to place, in trunk order. */
export interface Side {
  readonly id: string;
  /** The trunk node it came from. */
  readonly at: string;
  readonly slot: number;
  readonly height: number;
  /** Session time its request started; absent while it waits for its slot. */
  readonly startedAt?: number;
  readonly endedAt?: number;
}

export interface Placed {
  readonly top: number;
  /** Where its cable leaves its trunk node. */
  readonly leave: number;
  readonly pending: boolean;
}

export function place(sides: readonly Side[], trunk: readonly Span[], now: number, gap: number): Map<string, Placed> {
  const anchors = new Map(trunk.map((s) => [s.id, s] as const));
  // In the order they started; the ones still waiting last, in trunk order.
  const order = sides.map((s, i) => ({ s, i })).sort((a, b) => (a.s.startedAt ?? Infinity) - (b.s.startedAt ?? Infinity) || a.i - b.i);
  const floors = new Map<number, number>();
  const done: { readonly bottom: number; readonly endedAt: number }[] = trunk.flatMap((s) => (s.endedAt !== undefined ? [{ bottom: s.bottom, endedAt: s.endedAt }] : []));
  const placed = new Map<string, Placed>();
  for (const { s } of order) {
    const anchor = anchors.get(s.at);
    if (!anchor) continue;
    const start = s.startedAt ?? now;
    let below = Number.NEGATIVE_INFINITY;
    for (const d of done) if (d.endedAt <= start) below = Math.max(below, d.bottom + gap);
    const top = Math.max(anchor.top, floors.get(s.slot) ?? 0, below);
    const leave = Math.max(anchor.top + ENTER, Math.min(top + ENTER, anchor.bottom - LEAVE));
    placed.set(s.id, { top, leave, pending: s.startedAt === undefined });
    floors.set(s.slot, top + s.height + gap);
    if (s.endedAt !== undefined) done.push({ bottom: top + s.height, endedAt: s.endedAt });
  }
  return placed;
}
