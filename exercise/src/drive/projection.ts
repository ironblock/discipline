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
  /** Kinds the format does not have yet (R4-R6). */
  kinds: ['fork', 'fork.settled', 'patch', 'seam'],
  /** Keys the format does not have yet, by the kind they ride on. */
  keys: {
    'session.start': ['arm', 'slots', 'trunk_slot', 'phase', 'system_tokens'],
    request: ['slot', 'fork'],
    response: ['calls_from'],
  },
  /** Lanes the format does not have yet (R4): a request on one goes, with every line that names it. */
  lanes: ['interview', 'ratify', 'extraction'],
} as const satisfies {
  readonly kinds: readonly string[];
  readonly keys: Readonly<Record<string, readonly string[]>>;
  readonly lanes: readonly string[];
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
const STRIP_ONLY_WHAT_THE_FORMAT_LACKS: {
  readonly kinds: Lacks<(typeof STRIPPED.kinds)[number], Format.Kind>;
  readonly lanes: Lacks<(typeof STRIPPED.lanes)[number], Format.Lane>;
  readonly keys: { readonly [K in keyof Keyed]: Lacks<Keyed[K][number], keyof Extract<Format.LogLine, { kind: K }>> };
} = { kinds: true, lanes: true, keys: { 'session.start': true, request: true, response: true } };
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
  const sideRequests = new Set(log.filter((l) => l.kind === 'request' && lanes.has(l.lane)).map((l) => l.seq));
  const kept = log.filter((l) => {
    if (kinds.has(l.kind) || sideRequests.has(l.seq)) return false;
    const named = (REFERENCES[l.kind] ?? []).map((key) => (l as unknown as Record<string, unknown>)[key]);
    return !named.some((seq) => typeof seq === 'number' && sideRequests.has(seq));
  });
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
