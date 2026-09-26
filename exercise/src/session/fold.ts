/**
 * The fold: a session's log in, what the surface draws out.
 *
 * This is the only place a drive event becomes something a component can
 * render, and the type says so: every node is `Folded`, a brand declared
 * here and never exported, so a component cannot be handed a node that did
 * not come from the log -- a story included. Each node also carries `from`,
 * the log positions it was folded from, which is what the inspector shows
 * when you ask where a number came from, and `needs`, the steps of #117 the
 * node waits on, which is what the gaps overlay outlines.
 */

import type { DriveEvent, EventOf, ForkLane, ForkOutcome, Lane, Need, PatchOp, SeamReason, Stop, Timings, Tool } from '../drive/events.ts';
import { NEEDS_OF } from '../drive/events.ts';

declare const folded: unique symbol;

/** A node the fold made. Only this module can make one. */
export type Folded<T> = T & { readonly [folded]: true };

interface Provenance {
  /** Log positions this node was folded from. */
  readonly from: readonly number[];
  /** Steps of #117 the node waits on: what `diet` cannot emit yet. */
  readonly needs: readonly Need[];
}

export interface SystemNode extends Provenance {
  readonly kind: 'system';
  readonly id: string;
  readonly text: string;
  readonly tokens: number;
  /** Set when this system prompt is a render of working memory. */
  readonly render?: number;
}

export interface UserNode extends Provenance {
  readonly kind: 'user';
  readonly id: string;
  readonly turn: number;
  readonly text: string;
  /** What the ask cost: the next trunk request's prefill, new and reused. */
  readonly prefill?: { readonly fresh: number; readonly cached: number };
}

export type Progress = 'prefill' | 'streaming' | 'done' | 'cancelled';

export interface Generation {
  readonly progress: Progress;
  readonly reasoning: string;
  readonly text: string;
  readonly stop?: Stop;
  readonly slot: number;
  readonly startedAt: number;
  /** Session time of the last sign of life: the request, the latest delta, the response. */
  readonly lastActivityAt: number;
  readonly timings?: Timings;
  /** Request to response, wall clock. */
  readonly wallMs?: number;
}

export interface AssistantNode extends Provenance, Generation {
  readonly kind: 'assistant';
  /** The trunk request's id; `fork.at` may name its response instead. */
  readonly id: string;
  readonly turn: number;
}

export interface ToolNode extends Provenance {
  readonly kind: 'tool';
  readonly id: string;
  readonly turn: number;
  readonly tool: Tool;
  readonly args: Readonly<Record<string, unknown>>;
  /** Session time the call began. */
  readonly startedAt: number;
  readonly running: boolean;
  readonly exit?: number;
  readonly output?: string;
  readonly truncated?: boolean;
  readonly ms?: number;
}

export type TrunkNode = Folded<UserNode> | Folded<AssistantNode> | Folded<ToolNode>;

export interface PatchNode extends Provenance {
  readonly id: string;
  readonly op: PatchOp;
  readonly entryId: string;
  readonly category?: string;
  readonly text: string;
  readonly supersedes?: string;
  readonly provenance?: string;
}

export interface BranchNode extends Provenance, Partial<Generation> {
  readonly kind: 'branch';
  readonly id: string;
  readonly lane: ForkLane;
  readonly slot: number;
  /** The trunk node it branched from. */
  readonly at: string;
  readonly why: string;
  readonly question: string;
  readonly prefixTokens: number;
  readonly outcome?: ForkOutcome;
  readonly patches: readonly Folded<PatchNode>[];
}

export interface SeamNode extends Provenance {
  readonly kind: 'seam';
  readonly id: string;
  readonly atTurn: number;
  readonly reason: SeamReason;
  readonly phase?: { readonly from: string; readonly to: string };
  readonly hashBefore: string;
  readonly hashAfter: string;
  /** The trunk's prefix just before the seam: what the refill replaced. */
  readonly prefixBefore?: number;
  readonly prefixAfter: number;
  readonly warm: Timings;
}

export interface Era {
  readonly index: number;
  /** The seam that opened this era; absent for the first. */
  readonly seam?: Folded<SeamNode>;
  readonly system: Folded<SystemNode>;
  readonly nodes: readonly TrunkNode[];
}

export type EntryState = 'live' | 'superseded' | 'retired';

export interface MemoryEntry extends Provenance {
  readonly id: string;
  readonly category?: string;
  readonly text: string;
  readonly state: EntryState;
  /** The patch that last changed it. */
  readonly by: string;
  /** Its last op, when that was not one of the three that set `state`. */
  readonly op?: PatchOp;
  readonly provenance?: string;
  /** Log position of the patch that last changed it. */
  readonly landedAt: number;
  /** Landed since the last ask: what the operator has not seen yet. */
  readonly fresh: boolean;
}

export type SessionState = 'connecting' | 'awaiting' | 'turn' | 'capture' | 'ratify' | 'ended';

/** Who holds a slot: the request, or the fork it serves, and its lane. */
export interface Holder {
  readonly id: string;
  readonly lane: Lane;
}

export interface Session {
  readonly state: SessionState;
  readonly arm: string;
  readonly model: string;
  readonly slots: number;
  readonly trunkSlot: number;
  readonly phase: string;
  readonly eras: readonly Era[];
  /** Branches keyed by the trunk node they came from. */
  readonly branches: ReadonlyMap<string, readonly Folded<BranchNode>[]>;
  readonly memory: readonly Folded<MemoryEntry>[];
  /** What each slot is serving right now, and for which lane; absent when idle. */
  readonly occupancy: readonly (Holder | undefined)[];
  /** Session time of the last event. */
  readonly now: number;
  readonly events: number;
  /** Events of a kind this surface does not know, by kind: kept and counted, never dropped silently. */
  readonly unknown: ReadonlyMap<string, number>;
}

function brand<T>(value: T): Folded<T> {
  return value as Folded<T>;
}

function needsOf(events: readonly DriveEvent[]): Need[] {
  const out = new Set<Need>();
  for (const e of events) for (const n of NEEDS_OF[e.kind]) out.add(n);
  return [...out].sort();
}

function provenance(...events: readonly (DriveEvent | undefined)[]): Provenance {
  const present = events.filter((e): e is DriveEvent => e !== undefined);
  return { from: present.map((e) => e.seq), needs: needsOf(present) };
}

type Mutable<T> = { -readonly [K in keyof T]: T[K] };

interface GenerationBuilder {
  request: EventOf<'request'>;
  deltas: EventOf<'delta'>[];
  response?: EventOf<'response'>;
}

function generation(g: GenerationBuilder): Generation {
  const { request, response } = g;
  const reasoning = response ? (response.reasoning ?? '') : g.deltas.map((d) => d.reasoning ?? '').join('');
  const text = response ? response.text : g.deltas.map((d) => d.text ?? '').join('');
  const progress: Progress = response ? (response.stop === 'cancelled' ? 'cancelled' : 'done') : g.deltas.length > 0 ? 'streaming' : 'prefill';
  return {
    progress,
    reasoning,
    text,
    slot: request.slot,
    startedAt: request.t,
    lastActivityAt: response?.t ?? g.deltas.at(-1)?.t ?? request.t,
    ...(response ? { stop: response.stop, timings: response.timings, wallMs: response.t - request.t } : {}),
  };
}

/** Fold a session's log. Pure; the same log always folds the same. */
export function fold(events: readonly DriveEvent[]): Session {
  const start = events.find((e): e is EventOf<'session.start'> => e.kind === 'session.start');
  if (!start) {
    return {
      state: 'connecting',
      arm: '',
      model: '',
      slots: 0,
      trunkSlot: 0,
      phase: '',
      eras: [],
      branches: new Map(),
      memory: [],
      occupancy: [],
      now: 0,
      events: events.length,
      unknown: new Map(),
    };
  }

  // Builders, keyed by the ids later events name.
  const generations = new Map<string, GenerationBuilder>();
  const responseToRequest = new Map<string, string>();
  const asks = new Map<number, EventOf<'ask'>>();
  const firstRequestOfTurn = new Map<number, string>();
  const tools = new Map<string, { begin: EventOf<'tool.begin'>; end?: EventOf<'tool.end'> }>();
  const forks = new Map<string, { fork: EventOf<'fork'>; request?: string; settled?: EventOf<'fork.settled'>; patches: EventOf<'patch'>[] }>();
  const entries = new Map<string, Mutable<Omit<MemoryEntry, 'fresh' | 'landedAt'>> & { seq: number }>();

  type Slot = { kind: 'user'; turn: number } | { kind: 'assistant'; request: string } | { kind: 'tool'; id: string };
  const eras: { seam?: EventOf<'seam'>; system: SystemNode; slots: Slot[] }[] = [
    {
      system: { kind: 'system', id: 'system/0', text: start.system.text, tokens: start.system.tokens, ...provenance(start) },
      slots: [],
    },
  ];
  const era = () => eras[eras.length - 1]!;

  const unknown = new Map<string, number>();
  let phase = start.phase;
  let openTurn: number | undefined;
  let lastAskSeq = -1;
  let ended = false;

  for (const e of events) {
    switch (e.kind) {
      case 'session.start':
        break;
      case 'ask':
        asks.set(e.turn, e);
        openTurn = e.turn;
        lastAskSeq = e.seq;
        era().slots.push({ kind: 'user', turn: e.turn });
        break;
      case 'request':
        generations.set(e.id, { request: e, deltas: [] });
        if (e.lane === 'trunk') {
          if (!firstRequestOfTurn.has(e.turn)) firstRequestOfTurn.set(e.turn, e.id);
          era().slots.push({ kind: 'assistant', request: e.id });
        } else if (e.fork) {
          const f = forks.get(e.fork);
          if (f) f.request = e.id;
        }
        break;
      case 'delta':
        generations.get(e.request)?.deltas.push(e);
        break;
      case 'response': {
        const g = generations.get(e.to_request);
        if (g) g.response = e;
        responseToRequest.set(e.id, e.to_request);
        break;
      }
      case 'tool.begin':
        tools.set(e.id, { begin: e });
        era().slots.push({ kind: 'tool', id: e.id });
        break;
      case 'tool.end': {
        const t = tools.get(e.id);
        if (t) t.end = e;
        break;
      }
      case 'turn.settled':
        if (openTurn === e.turn) openTurn = undefined;
        break;
      case 'fork':
        forks.set(e.id, { fork: e, patches: [] });
        break;
      case 'fork.settled': {
        const f = forks.get(e.id);
        if (f) f.settled = e;
        break;
      }
      case 'patch': {
        forks.get(e.from)?.patches.push(e);
        const old = entries.get(e.entry.id);
        const base = {
          ...(e.entry.category !== undefined ? { category: e.entry.category } : {}),
          ...(e.provenance !== undefined ? { provenance: e.provenance } : {}),
          text: e.entry.text,
          by: e.id,
          seq: e.seq,
          ...provenance(e),
        };
        if (e.op === 'retire') {
          if (old) entries.set(e.entry.id, { ...old, state: 'retired', by: e.id, seq: e.seq, from: [...old.from, e.seq] });
          break;
        }
        if (e.op === 'supersede' && e.supersedes) {
          const replaced = entries.get(e.supersedes);
          if (replaced) entries.set(e.supersedes, { ...replaced, state: 'superseded', by: e.id, seq: e.seq, from: [...replaced.from, e.seq] });
        }
        if (e.op === 'add' || e.op === 'supersede' || !old) {
          entries.set(e.entry.id, { id: e.entry.id, state: 'live', ...base, ...(e.op !== 'add' && e.op !== 'supersede' ? { op: e.op } : {}) });
          break;
        }
        // Any other op rewrites the entry, keeps its state, and is shown by name.
        entries.set(e.entry.id, { ...old, ...base, op: e.op, from: [...old.from, e.seq] });
        break;
      }
      case 'seam': {
        if (e.phase) phase = e.phase.to;
        const system: SystemNode = {
          kind: 'system',
          id: `system/${e.id}`,
          text: e.render.text,
          tokens: e.render.tokens,
          render: e.render.version,
          ...provenance(e),
        };
        eras.push({ seam: e, system, slots: [] });
        break;
      }
      case 'session.end':
        ended = true;
        break;
      default: {
        // A kind from a newer drive: the log may carry it before the surface draws it.
        const kind = (e as { readonly kind: string }).kind;
        unknown.set(kind, (unknown.get(kind) ?? 0) + 1);
      }
    }
  }

  // Trunk nodes, era by era.
  const trunkPrefixAt = (timings: Timings | undefined) =>
    timings ? timings.prompt_n + timings.cache_n + timings.predicted_n : undefined;
  let previousEraEnd: Timings | undefined;
  const builtEras: Era[] = eras.map((raw, index) => {
    const nodes: TrunkNode[] = raw.slots.map((slot): TrunkNode => {
      switch (slot.kind) {
        case 'user': {
          const ask = asks.get(slot.turn)!;
          const firstId = firstRequestOfTurn.get(slot.turn);
          const first = firstId ? generations.get(firstId) : undefined;
          const timings = first?.response?.timings;
          return brand<UserNode>({
            kind: 'user',
            id: `ask/${slot.turn}`,
            turn: slot.turn,
            text: ask.text,
            ...(timings && first?.response?.stop !== 'cancelled' ? { prefill: { fresh: timings.prompt_n, cached: timings.cache_n } } : {}),
            ...provenance(ask, first?.response),
          });
        }
        case 'assistant': {
          const g = generations.get(slot.request)!;
          const node: AssistantNode = {
            kind: 'assistant',
            id: g.request.id,
            turn: g.request.turn,
            ...generation(g),
            ...provenance(g.request, ...g.deltas.slice(0, 1), g.response),
          };
          return brand(node);
        }
        case 'tool': {
          const { begin, end } = tools.get(slot.id)!;
          return brand<ToolNode>({
            kind: 'tool',
            id: begin.id,
            turn: begin.turn,
            tool: begin.tool,
            args: begin.args,
            startedAt: begin.t,
            running: end === undefined,
            ...(end ? { exit: end.exit, output: end.output, ms: end.t - begin.t, ...(end.truncated ? { truncated: true } : {}) } : {}),
            ...provenance(begin, end),
          });
        }
      }
    });
    const seamEvent = raw.seam;
    const seam = seamEvent
      ? brand<SeamNode>({
          kind: 'seam',
          id: seamEvent.id,
          atTurn: seamEvent.at_turn,
          reason: seamEvent.reason,
          ...(seamEvent.phase ? { phase: seamEvent.phase } : {}),
          hashBefore: seamEvent.prefix_hash_before,
          hashAfter: seamEvent.prefix_hash_after,
          ...(previousEraEnd ? { prefixBefore: trunkPrefixAt(previousEraEnd)! } : {}),
          prefixAfter: seamEvent.render.tokens,
          warm: seamEvent.warm,
          ...provenance(seamEvent),
        })
      : undefined;
    // The last trunk timings in this era, for the next seam's "before".
    for (const slot of raw.slots) {
      if (slot.kind === 'assistant') {
        const t = generations.get(slot.request)?.response?.timings;
        if (t && t.predicted_n > 0) previousEraEnd = t;
      }
    }
    return { index, system: brand(raw.system), nodes, ...(seam ? { seam } : {}) };
  });

  // Branches, keyed by the trunk node they came from.
  const branches = new Map<string, Folded<BranchNode>[]>();
  const openForks: EventOf<'fork'>[] = [];
  for (const { fork, request, settled, patches } of forks.values()) {
    const g = request ? generations.get(request) : undefined;
    const at = responseToRequest.get(fork.at) ?? fork.at;
    if (!settled) openForks.push(fork);
    const node = brand<BranchNode>({
      kind: 'branch',
      id: fork.id,
      lane: fork.lane,
      slot: fork.slot,
      at,
      why: fork.why,
      question: fork.question,
      prefixTokens: fork.prefix_tokens,
      ...(g ? generation(g) : {}),
      ...(settled ? { outcome: settled.outcome } : {}),
      patches: patches.map((p) =>
        brand<PatchNode>({
          id: p.id,
          op: p.op,
          entryId: p.entry.id,
          ...(p.entry.category !== undefined ? { category: p.entry.category } : {}),
          text: p.entry.text,
          ...(p.supersedes ? { supersedes: p.supersedes } : {}),
          ...(p.provenance ? { provenance: p.provenance } : {}),
          ...provenance(p),
        }),
      ),
      ...provenance(fork, g?.request, g?.response, settled),
    });
    const list = branches.get(at) ?? [];
    list.push(node);
    branches.set(at, list);
  }

  // Slot occupancy: whatever request is generating, per slot.
  const occupancy: (Holder | undefined)[] = Array.from({ length: start.slots }, () => undefined);
  for (const g of generations.values()) {
    if (!g.response) occupancy[g.request.slot] = { id: g.request.fork ?? g.request.id, lane: g.request.lane };
  }

  const state: SessionState = ended
    ? 'ended'
    : openTurn !== undefined
      ? 'turn'
      : openForks.some((f) => f.lane === 'ratify')
        ? 'ratify'
        : openForks.length > 0
          ? 'capture'
          : 'awaiting';

  const memory = [...entries.values()].map(({ seq, ...entry }) => brand<MemoryEntry>({ ...entry, landedAt: seq, fresh: seq > lastAskSeq }));

  return {
    state,
    arm: start.arm,
    model: start.model,
    slots: start.slots,
    trunkSlot: start.trunk_slot,
    phase,
    eras: builtEras,
    branches,
    memory,
    occupancy,
    now: events.at(-1)?.t ?? 0,
    events: events.length,
    unknown,
  };
}
