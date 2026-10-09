import type * as Format from '../../../diet/formats/log/log.ts';
import type { LogLine } from './log.ts';

/**
 * What of a placed log `diet check-log` is asked to read: THE PROJECTION
 * (ruled on #300, 5974717646). A placed log carries what the surface draws
 * ahead of the format -- side calls, working memory, seams, and keys on the
 * format's own lines -- which the format's reader refuses today, and that is
 * not a defect in the log. So each placed log is checked with exactly what
 * this list names taken out, and what is left must pass: the turn, its
 * requests and answers, its tool calls.
 *
 * THE LIST ONLY SHRINKS. When a step of #117 brings a kind or a key into the
 * format, an entry here that the format now reads is a finding, and it goes:
 * the projection may never hide what the format can check.
 */
export const STRIPPED = {
  /** Kinds the format does not have yet. */
  kinds: [],
  /** Keys the format does not have yet, by the kind they ride on. */
  keys: {
    'session.start': ['arm', 'slots', 'trunk_slot', 'phase', 'system_tokens'],
    request: ['slot'],
    response: ['calls_from'],
  },
  /** Lanes the format does not have yet (`lanes`): a request on one goes, with every line that names it. */
  lanes: ['ratify', 'extraction'],
  /**
   * The surface's own older shapes of a kind the format now has: a line of the kind carrying any of these keys was
   * placed in the shape the surface drew before the format said it (#374), and goes whole, with every line that
   * names it -- not trimmed to the format's keys, which would pass off the old fork as a v5 one. Its re-recording
   * in v5's form is #427.
   */
  shapes: {
    fork: ['slot', 'prefix_tokens'],
    patch: ['authority'],
    seam: ['render_version'],
  },
} as const satisfies {
  readonly kinds: readonly string[];
  readonly keys: Readonly<Record<string, readonly string[]>>;
  readonly lanes: readonly string[];
  readonly shapes: Readonly<Record<string, readonly string[]>>;
};

/**
 * The list only shrinks, held by the compiler: an entry the format's
 * generated types now carry -- a kind, a lane, or a key on its kind's line,
 * for every kind the list names keys on -- fails the typecheck here, and the
 * entry goes. A recording's `carried` kinds are its own declaration, not this
 * list's, and are not held here.
 */
type Lacks<Name extends string, In> = [Extract<Name, In>] extends [never] ? true : false;
type Keyed = typeof STRIPPED.keys;
type Shaped = typeof STRIPPED.shapes;
const STRIP_ONLY_WHAT_THE_FORMAT_LACKS: {
  readonly kinds: Lacks<(typeof STRIPPED.kinds)[number], Format.Kind>;
  readonly lanes: Lacks<(typeof STRIPPED.lanes)[number], Format.Lane>;
  readonly keys: { readonly [K in keyof Keyed]: Lacks<Keyed[K][number], keyof Extract<Format.LogLine, { kind: K }>> };
  readonly shapes: { readonly [K in keyof Shaped]: Lacks<Shaped[K][number], keyof Extract<Format.LogLine, { kind: K }>> };
} = { kinds: true, lanes: true, keys: { 'session.start': true, request: true, response: true }, shapes: { fork: true, patch: true, seam: true } };
void STRIP_ONLY_WHAT_THE_FORMAT_LACKS;

/** The keys that name another line by its `seq`, by kind: renumbered with the lines they name. */
const REFERENCES: Readonly<Record<string, readonly string[]>> = {
  delta: ['request'],
  progress: ['request'],
  response: ['to_request'],
  cancelled: ['request'],
  'request.failed': ['request'],
  tool_call: ['request'],
  'idle.gap': ['opened_by'],
  request: ['fork'],
  fork: ['at'],
  'fork.settled': ['fork'],
  patch: ['fork'],
};

/**
 * LOG with what `STRIPPED` names taken out, and its `seq`s renumbered from 0
 * with every reference following the line it names. CARRIED are the kinds a
 * recording declares it carries under their own name (`Recording.carried`):
 * the recording says so in its header, and they go too.
 */
export function projection(log: readonly LogLine[], carried: readonly string[] = []): Record<string, unknown>[] {
  const kinds = new Set<string>([...STRIPPED.kinds, ...carried]);
  const lanes = new Set<string>(STRIPPED.lanes);
  const shapes = STRIPPED.shapes as Readonly<Record<string, readonly string[]>>;
  // What goes, in log order: a stripped kind, a request on a stripped lane, a line in an old shape -- and every
  // line that names one that went (a reference names an earlier line, so one pass sees it gone first).
  const gone = new Set<number>();
  for (const l of log) {
    const line = l as unknown as Record<string, unknown>;
    const named = (REFERENCES[l.kind] ?? []).map((key) => line[key]);
    if (
      kinds.has(l.kind) ||
      (l.kind === 'request' && lanes.has(l.lane)) ||
      (shapes[l.kind] ?? []).some((key) => key in line) ||
      named.some((seq) => typeof seq === 'number' && gone.has(seq))
    )
      gone.add(l.seq);
  }
  const kept = log.filter((l) => !gone.has(l.seq));
  const renumbered = new Map(kept.map((l, i) => [l.seq, i] as const));
  return kept.map((l, i) => {
    const line: Record<string, unknown> = { ...l, seq: i };
    for (const key of (STRIPPED.keys as Readonly<Record<string, readonly string[]>>)[l.kind] ?? []) delete line[key];
    for (const key of REFERENCES[l.kind] ?? []) {
      const seq = line[key];
      if (typeof seq === 'number') line[key] = renumbered.get(seq) ?? -1;
    }
    return line;
  });
}
