/**
 * The fold: a session's log in, what the surface draws out.
 *
 * This is the only place a drive event becomes something a component can
 * render, and the type says so: every node is `Folded`, a brand declared
 * here and never exported, so a component cannot be handed a node that did
 * not come from the log -- a story included. The log is `diet`'s (`log.ts`):
 * a node's `id` is the `seq` of the line it began at, the identifier the log
 * issues, and every reference between nodes is one. Each node also carries `from`,
 * the log positions it was folded from, which is what the inspector shows
 * when you ask where a number came from, and `needs`, the steps of #117 the
 * node waits on, which is what the gaps overlay outlines.
 */

import type { Approval, Authority, FailReason, FileRef, ForkLane, ForkOutcome, IsolationWord, Lane, LineOf, LogLine, Need, NetworkWord, Open, PatchOp, SeamReason, SettleReason, Timings, Tool, ToolOutcome, ToolRefusal } from '../drive/log.ts';
import { needsOf } from '../drive/log.ts';
import { receiptOf } from './receipt.ts';
import type { Receipt } from './receipt.ts';

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
  readonly tokens?: number;
  /** Set when this system prompt is a render of working memory: the frame it was rendered with. */
  readonly render?: string;
  /**
   * Set when the render rode in a user message after the head, which stays as the session sent it (log v7's seam
   * `placement`, #597): `text` is that message.
   */
  readonly placement?: 'message';
}

export interface UserNode extends Provenance {
  readonly kind: 'user';
  readonly id: string;
  readonly turn: number;
  readonly text: string;
  /** What the ask cost: the next trunk request's prefill, new and reused. */
  readonly prefill?: { readonly fresh: number; readonly cached: number };
  /** Session time it finished: when it was asked. */
  readonly endedAt: number;
  /**
   * Not in what the model reads from here on (#289): its turn failed or timed out before any step finished, or was
   * cancelled before anything was said -- a failed or timed-out turn keeps its ask and every finished step on the trunk
   * and loses only its last request (#541); a cancelled one keeps its ask, its steps and what it had said (#575). The fact
   * is the log's `turn.settled`, and this is its word. A capped turn is `failed` (#290).
   */
  readonly outOfContext?: OffTrunk;
  /**
   * Forks' patches delivered after this ask (log v7's `delivered`, the fork delivery lever): the note the model was
   * sent at the tail of the turn's first request, and its framing. Folded, not yet drawn.
   */
  readonly delivered?: { readonly framing: string; readonly text: string };
  /**
   * Archived items recalled after this ask (log v7's `recalled`, #566): the note the model was sent after the ask,
   * and how it matched. Folded, not yet drawn.
   */
  readonly recalled?: { readonly recall: string; readonly text: string };
  /**
   * Ended background commands' notifications delivered after this ask (log v7's `notice`, #614): the note the model
   * was sent at the tail of the turn's first request. Folded, not yet drawn.
   */
  readonly noticed?: string;
  /**
   * The tool results the model pruned in this turn (log v7's `pruned`, #612): each call, the bytes a later seam
   * removes, and the reference line it carries instead. Folded, not yet drawn.
   */
  readonly pruned?: readonly { readonly call: string; readonly bytes: number; readonly text: string }[];
  /** Self-capture's reminder after this ask (log v7's `reminded`, #619): a note the model was sent, the harness's words. */
  readonly reminded?: string;
  /** The files the operator attached to the ask (log v5's `ask.files`, #372): read by digest, never by path. */
  readonly files?: readonly FileRef[];
  /** The operator marked it the scope answer (the `ask` line's `scoping`, log v5, #453): its turn warrants the interview fork. */
  readonly scoping?: true;
}

export type Progress = 'prefill' | 'streaming' | 'done' | 'cancelled' | 'failed';

/** The settle words that leave a turn off the trunk. */
export type OffTrunk = 'cancelled' | 'failed' | 'timeout' | 'rolled-back';

/** Why a generation stopped: the response's `finish_reason` as llama.cpp spells it, or `cancelled` for a stopped call. */
export type Stop = Open<'stop' | 'tool_calls' | 'length' | 'cancelled'>;

export interface Generation {
  readonly progress: Progress;
  readonly reasoning: string;
  readonly text: string;
  readonly stop?: Stop;
  /** Its response hit the output cap (the log's `capped`): what it wrote is not an answer (#290). */
  readonly capped?: true;
  readonly slot: number;
  readonly startedAt: number;
  /** Where its tool calls began in what it wrote (the response's `calls_from`): what came before is its text's. */
  readonly callsFrom?: { readonly predicted_n?: number; readonly predicted_ms: number };
  /** Session time it began writing: its first token. Absent while it reads. */
  readonly writingSince?: number;
  /** Session time of the last sign of life: the request, the latest delta, the response. */
  readonly lastActivityAt: number;
  /** Session time it finished -- its response, or its failure; absent while it runs. */
  readonly endedAt?: number;
  readonly timings?: Timings;
  /** Request to response, wall clock. */
  readonly wallMs?: number;
  /** Why no response will come: the request failed. */
  readonly failure?: {
    readonly reason: FailReason;
    readonly message: string;
    /** An overflow's sizes (log v7, #628): the prompt as sized, the window, and whether serve inferred it from the size. */
    readonly overflow?: { readonly promptTokens: number; readonly window: number; readonly inferred: boolean };
  };
  /** Where a running request is, from its progress frames; absent once it has answered. */
  readonly meter?: Meter;
}

/**
 * A running request's prefill, as the log's `progress` lines say it: its
 * prompt (all of it, the warm part, and how much is processed so far, the
 * warm part among it -- held at the most any frame said), and the rate the
 * new part is read at by the server's own clock, in tokens per second. The
 * frames stop at the first token and never count what is generated (#288).
 */
export interface Meter {
  /** Session time of the last frame. */
  readonly at: number;
  readonly total: number;
  readonly cache: number;
  /** Prompt tokens processed, the cache among them: `cache` at the start, `total` once the prompt is read. */
  readonly processed: number;
  readonly ppRate?: number;
}

export interface AssistantNode extends Provenance, Generation {
  readonly kind: 'assistant';
  /** Its request's `seq`: what its answer, its tool calls and its side calls name. */
  readonly id: string;
  readonly turn: number;
  /**
   * Not in what the model reads from here on (#289): its turn failed or timed out before any step finished, or was
   * cancelled before anything was said -- a failed or timed-out turn keeps its ask and every finished step on the trunk
   * and loses only its last request (#541); a cancelled one keeps its ask, its steps and what it had said (#575). The fact
   * is the log's `turn.settled`, and this is its word. A capped turn is `failed` (#290).
   */
  readonly outOfContext?: OffTrunk;
}

/**
 * A tool call: from its first streamed fragment (its `id` is that delta's
 * `seq`, so it is the same node from the moment the model writes it) to its
 * `tool_call` line, which says what became of it (v3, #297).
 */
export interface ToolNode extends Provenance {
  readonly kind: 'tool';
  readonly id: string;
  readonly turn: number;
  readonly tool: Tool;
  /** The arguments text as the model wrote it, as far as it has streamed. */
  readonly arguments: string;
  /** The same, read as the JSON object a tool takes; empty while it does not read as one. */
  readonly args: Readonly<Record<string, unknown>>;
  /** The assistant node that made the call (its request's `seq`): the model wrote it, as the end of that generation. */
  readonly after: string;
  /** The call as the drive names it: its request's `seq` and its id as the model streamed it. What a prompt names (#389). */
  readonly call: { readonly request: number; readonly id: string } | undefined;
  /** Session time the call began: its response, or the call before it ending, or its fragment if later. */
  readonly startedAt: number;
  readonly running: boolean;
  /** Not begun: a call before it on the same response is still running, and the drive runs them one at a time. */
  readonly waiting?: true;
  /** Still being written: its response has not arrived, so the drive cannot have begun it. */
  readonly writing?: true;
  /** What became of it; absent while it runs. */
  readonly outcome?: ToolOutcome;
  /** The command as the drive asked for it. */
  readonly argv?: readonly string[];
  /** The directory it ran in, or would have (log v4's `cwd`, #388). */
  readonly cwd?: string;
  /** The decision it ran under: the operator's on its prompt, or the pre-seeded set (log v4's `approval`, #388). */
  readonly approval?: Approval;
  /** The files its result is, by reference (log v4's `files`, #372): read by digest, never by path. */
  readonly files?: readonly FileRef[];
  /** What ran, under which mechanism and network: absent where the log does not say. */
  readonly confinement?: { readonly confined?: readonly string[]; readonly isolation?: IsolationWord; readonly network?: NetworkWord };
  readonly exit?: number;
  readonly output?: string;
  readonly stderr?: string;
  /** Why the drive refused it. */
  readonly refusal?: ToolRefusal;
  /** What a self-capture call did (log v7's `capture`, #619): its outcome, the entries it wrote, and why when it did not. */
  readonly capture?: { readonly outcome: string; readonly entries: readonly string[]; readonly why?: string };
  /** The background job it started (the call's `background`, #614), and how the job ended once it has (`background.ended`). */
  readonly background?: { readonly job: string; readonly status?: string; readonly exit?: number };
  /** The model pruned its output (log v7's `pruned`, #630): the bytes, and whether a seam has since replaced it by its pointer. */
  readonly pruned?: { readonly bytes: number; readonly replaced: boolean };
  /** A phase proposal the operator ruled on (#651): the choice, and the phase it proposed. */
  readonly ruled?: { readonly choice: string; readonly to: string };
  /** Running, it is near its timeout (log v7's `timeout.near`, #613): its timeout, and when the warning came. */
  readonly nearTimeout?: { readonly timeoutMs: number; readonly at: number };
  /** The policy it failed under: the Seatbelt profile's sha256. */
  readonly policy?: string;
  readonly ms?: number;
  /** Session time the call ended; absent while it runs. */
  readonly endedAt?: number;
}

/** The end of a turn that did not end on its own: the step limit, a timeout, a reason from a newer drive. */
export interface SettledNode extends Provenance {
  readonly kind: 'settled';
  readonly id: string;
  readonly turn: number;
  readonly reason: SettleReason;
  /** It settled `failed` because its answer hit the output cap: the pair is the record of a capped turn (#290, ruled 5969297103). */
  readonly capped?: true;
  readonly endedAt: number;
}

export type TrunkNode = Folded<UserNode> | Folded<AssistantNode> | Folded<ToolNode> | Folded<SettledNode>;

export interface PatchNode extends Provenance {
  readonly id: string;
  readonly op: PatchOp;
  readonly entryId: string;
  readonly category?: string;
  readonly text: string;
  readonly supersedes?: string;
  /** How the entry was known (#117 naming 5). */
  readonly authority?: Authority;
}

export interface BranchNode extends Provenance, Partial<Generation> {
  readonly kind: 'branch';
  readonly id: string;
  readonly lane: ForkLane;
  readonly slot: number;
  /** The trunk node it branched from: an assistant node, or a tool call. */
  readonly at: string;
  readonly why: string;
  readonly question: string;
  /** AHEAD (`slots`): prefix tokens shared with the trunk; `diet`'s fork line does not say. */
  readonly prefixTokens?: number;
  /** The offboard seat it ran on (#615): the registry's id and model, with its cold prefill and wall time. Absent when warm. */
  readonly seat?: { readonly substrate: string; readonly model: string; readonly promptTokens?: number; readonly wallMs?: number };
  /** What triggered it (#620): `turn_end`, or `call:<class>:<id>`. */
  readonly trigger?: string;
  readonly outcome?: ForkOutcome;
  /** Why it was never sent, when refused (#637): `pool` -- the slots had no room for it. */
  readonly refused?: string;
  /** A hazard it was sent knowing (#637): `may-displace-trunk-cache`. */
  readonly hazard?: string;
  /** A seam's audit (#646): of the entries live when it opened, those it kept, updated (superseded) and removed (retired). */
  readonly audit?: { readonly kept: readonly string[]; readonly updated: readonly string[]; readonly removed: readonly string[] };
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
  readonly prefixAfter?: number;
  readonly warm?: Timings;
  /** What the refill carried (log v6): working-memory entries, and turns of the old trunk. */
  readonly carried?: { readonly entries: number; readonly turns: number };
  /** An automatic seam's size (log v7, #633): the prompt it was fired at, and the window that fired it. */
  readonly size?: { readonly promptTokens: number; readonly window: number };
}

export interface Era {
  readonly index: number;
  /** The seam that opened this era; absent for the first. */
  readonly seam?: Folded<SeamNode>;
  readonly system: Folded<SystemNode>;
  readonly nodes: readonly TrunkNode[];
}

/** `parked`: set aside as its tangent's at the tangent's close (#608), out of the render and kept in the archive. */
export type EntryState = 'live' | 'superseded' | 'retired' | 'parked';

export interface MemoryEntry extends Provenance {
  readonly id: string;
  readonly category?: string;
  readonly text: string;
  readonly state: EntryState;
  /** The patch that last changed it. */
  readonly by: string;
  /** Its last op, when that was not one of the three that set `state`. */
  readonly op?: PatchOp;
  /** How it was known, as its last patch said. */
  readonly authority?: Authority;
  /** Log position of the patch that last changed it. */
  readonly landedAt: number;
  /** Landed since the last ask: what the operator has not seen yet. */
  readonly fresh: boolean;
  /** The trunk's lane that wrote it, when no fork did (a patch's `lane`, #627): `self-capture` today. */
  readonly lane?: string;
  /** The tangent it was born in (a patch's `tangent`, #608): what that tangent's close rules on. */
  readonly tangent?: string;
}

export type SessionState = 'connecting' | 'awaiting' | 'turn' | 'capture' | 'ratify' | 'ended';

/** Who holds a slot: the request, or the fork it serves, and its lane. */
export interface Holder {
  readonly id: string;
  readonly lane: Lane;
}

/**
 * An idle gap, as the surface measured it and `diet` logged it (Q4): its
 * phases, what ended it, and its residual -- how far the five miss the gap's
 * wall clock on the log's own stamps (the settling to this line). Reported,
 * never refused: the surface's clock and the log's are not the same clock.
 */
export interface GapNode {
  readonly id: string;
  /** The `turn.settled` that opened it. */
  readonly openedBy: string;
  readonly notice: number;
  readonly read: number;
  readonly compose: number;
  readonly away: number;
  readonly blocked: number;
  readonly endedBy: string;
  readonly residual?: number;
}

export interface Session {
  readonly state: SessionState;
  /** The `seq` of the latest `turn.settled`: the gap a person's next command ends opened there. */
  readonly lastSettled?: number;
  /** Every idle gap the log carries. */
  readonly gaps: readonly Folded<GapNode>[];
  /** When the session opened, ms since the Unix epoch: the stream's identity (Q11). 0 before it has. */
  readonly opened: number;
  readonly arm: string;
  readonly model: string;
  /** The lever states the session ran under, as its `session.start` declares them (#573); absent when undeclared. */
  readonly levers: Levers;
  readonly slots: number;
  readonly trunkSlot: number;
  readonly phase: string;
  /** The phases the logged graph allows a seam to move to from the current one (#563): none without a graph. */
  readonly phaseMoves: readonly string[];
  readonly eras: readonly Era[];
  /** Branches keyed by the trunk node they came from. */
  readonly branches: ReadonlyMap<string, readonly Folded<BranchNode>[]>;
  readonly memory: readonly Folded<MemoryEntry>[];
  /** The tangent open now (#608), and the live entries born in it: what its close must rule on, no more and no fewer. */
  readonly tangent?: { readonly id: string; readonly entries: readonly string[] };
  /** How many tangents the session has opened: the next one's id is `t/<this + 1>`. */
  readonly tangentsOpened: number;
  /** The model's phase proposal waiting on the operator's ruling (#124, #651): its call, the move, and its reason. */
  readonly proposal?: { readonly call: string; readonly from?: string; readonly to: string; readonly reason?: string };
  /** What each slot is serving right now, and for which lane; absent when idle. */
  readonly occupancy: readonly (Holder | undefined)[];
  /** Session time of the last event. */
  readonly now: number;
  readonly events: number;
  /** Events of a kind this surface does not know, by kind: kept and counted, never dropped silently. */
  readonly unknown: ReadonlyMap<string, number>;
  /** The six numbers #31 measures a session on. */
  readonly receipt: Receipt;
}

/** The lever states a session declares on its first line (#573): only what the log says, never a default of the surface's. */
export interface Levers {
  /** `off` (log v7's `approvals_off`, #544), or `gate`: from v7 an absent field is the gate deciding. Undeclared before v7. */
  readonly approvals?: 'off' | 'gate';
  /** How a fork's result reaches the trunk (`fork_delivery`): `seam`, `advisory`, `imperative`, or a newer drive's word. */
  readonly forkDelivery?: string;
  /** The reasoning state on the wire (`template_kwargs`): thinking on or off, and the effort, as sent. */
  readonly reasoning?: string;
  /**
   * Every lever's state, as `session.start`'s `levers` declares it (#623): the record's start row, read from the log.
   * Words, shown as given, `undeclared` among them. Absent from a log written before it.
   */
  readonly table?: Readonly<Record<string, string>>;
}

export function leversOf(start: LineOf<'session.start'>): Levers {
  const kwargs = start.template_kwargs;
  const reasoning = [
    kwargs?.enable_thinking === undefined ? undefined : `thinking ${kwargs.enable_thinking ? 'on' : 'off'}`,
    kwargs?.reasoning_effort === undefined ? undefined : `effort ${kwargs.reasoning_effort}`,
  ].filter((part): part is string => part !== undefined);
  return {
    ...(start.approvals_off === true ? { approvals: 'off' as const } : start.version >= 7 ? { approvals: 'gate' as const } : {}),
    ...(start.fork_delivery !== undefined ? { forkDelivery: start.fork_delivery } : {}),
    ...(reasoning.length > 0 ? { reasoning: reasoning.join(' · ') } : {}),
    ...(start.levers !== undefined ? { table: start.levers } : {}),
  };
}

function brand<T>(value: T): Folded<T> {
  return value as Folded<T>;
}

function needsOfAll(events: readonly LogLine[]): Need[] {
  const out = new Set<Need>();
  for (const e of events) for (const n of needsOf(e)) out.add(n);
  return [...out].sort();
}

function provenance(...events: readonly (LogLine | undefined)[]): Provenance {
  const present = events.filter((e): e is LogLine => e !== undefined);
  return { from: present.map((e) => e.seq), needs: needsOfAll(present) };
}

type Mutable<T> = { -readonly [K in keyof T]: T[K] };

interface GenerationBuilder {
  request: LineOf<'request'>;
  deltas: LineOf<'delta'>[];
  frames: LineOf<'progress'>[];
  response?: LineOf<'response'>;
  cancelled?: LineOf<'cancelled'>;
  failed?: LineOf<'request.failed'>;
}

/** G, drawn; a request that names no slot (v0's never do) is on UNNAMED: the trunk's, or its fork's side slot. */
function generation(g: GenerationBuilder, unnamed: number): Generation {
  const { request, response, cancelled, failed } = g;
  const streamed = (piece: 'text' | 'reasoning') => g.deltas.map((d) => d[piece] ?? '').join('');
  const reasoning = response?.reasoning ?? streamed('reasoning');
  const text = response ? response.text : cancelled ? cancelled.partial : streamed('text');
  const ended = response ?? cancelled ?? failed;
  const progress: Progress = response ? 'done' : cancelled ? 'cancelled' : failed ? 'failed' : g.deltas.length > 0 ? 'streaming' : 'prefill';
  return {
    progress,
    reasoning,
    text,
    slot: request.slot ?? unnamed,
    startedAt: request.t,
    ...(g.deltas[0] ? { writingSince: g.deltas[0].t } : {}),
    lastActivityAt: ended?.t ?? g.deltas.at(-1)?.t ?? request.t,
    ...(ended ? { endedAt: ended.t, wallMs: ended.t - request.t } : {}),
    ...(response?.finish_reason !== undefined ? { stop: response.finish_reason } : cancelled ? { stop: 'cancelled' } : {}),
    ...(response?.capped ? { capped: true as const } : {}),
    // A stopped call's timings are not a measurement, and v0 gives it none: what the frames said stands.
    ...(response?.timings ? { timings: response.timings } : {}),
    ...(response?.calls_from ? { callsFrom: response.calls_from } : {}),
    ...(failed && !response
      ? {
          failure: {
            reason: failed.reason,
            message: failed.message,
            ...(failed.prompt_tokens !== undefined && failed.window !== undefined && failed.inferred !== undefined
              ? { overflow: { promptTokens: failed.prompt_tokens, window: failed.window, inferred: failed.inferred } }
              : {}),
          },
        }
      : {}),
    ...(!response && !failed && g.frames.length > 0 ? { meter: meterOf(g.frames) } : {}),
  };
}

/** A call's arguments read as the object a tool takes; a text that does not read as one (yet) is no arguments. */
function objectOf(text: string): Readonly<Record<string, unknown>> {
  try {
    const value: unknown = JSON.parse(text);
    return value !== null && typeof value === 'object' && !Array.isArray(value) ? (value as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

function meterOf(frames: readonly LineOf<'progress'>[]): Meter {
  const last = frames.at(-1)!;
  const top = frames.reduce((best, f) => (f.processed > best.processed ? f : best), frames[0]!);
  // The new part read, over the prefill time the server measured for it.
  const ppRate = top.time_ms > 0 && top.processed > top.cache ? (1000 * (top.processed - top.cache)) / top.time_ms : undefined;
  return {
    at: last.t,
    total: last.total,
    cache: last.cache,
    processed: top.processed,
    ...(ppRate !== undefined ? { ppRate } : {}),
  };
}

function unended(g: Generation): Omit<Generation, 'endedAt'> {
  const { endedAt, ...rest } = g;
  void endedAt;
  return rest;
}

/** Fold a session's log. Pure; the same log always folds the same. */
export function fold(lines: readonly LogLine[]): Session {
  const start = lines.find((e): e is LineOf<'session.start'> => e.kind === 'session.start');
  if (!start) {
    return {
      state: 'connecting',
      opened: 0,
      arm: '',
      model: '',
      levers: {},
      slots: 0,
      trunkSlot: 0,
      phase: '',
      tangentsOpened: 0,
      phaseMoves: [],
      eras: [],
      gaps: [],
      branches: new Map(),
      memory: [],
      occupancy: [],
      now: 0,
      events: lines.length,
      unknown: new Map(),
      receipt: { ...receiptOf(lines), liveEntries: 0 },
    };
  }
  const trunkSlot = start.trunk_slot ?? 0;
  // Where a fork goes when neither it nor its request names a slot, as `diet`'s never do: the first one beside the trunk's.
  const sideSlot = trunkSlot === 0 ? 1 : 0;
  const id = (seq: number) => String(seq);

  // Builders, keyed by the `seq` later lines name.
  const generations = new Map<number, GenerationBuilder>();
  const asks = new Map<number, LineOf<'ask'>>();
  const deliveries = new Map<number, LineOf<'delivered'>>();
  const recalls = new Map<number, LineOf<'recalled'>>();
  const notices = new Map<number, LineOf<'notice'>>();
  // Background jobs' ends, by job (#614); calls warned near their timeout, by `<request>/<call id>` (#613).
  const backgroundEnds = new Map<string, LineOf<'background.ended'>>();
  const nearTimeouts = new Map<string, LineOf<'timeout.near'>>();
  const prunes = new Map<number, LineOf<'pruned'>[]>();
  // The calls whose output a seam replaced by its pointer (#630): every seam's `pruned`.
  const replacedBySeam = new Set<string>();
  const reminders = new Map<number, LineOf<'reminded'>>();
  // Self-capture's outcomes, by the call they belong to: `<request>/<call id>`.
  const captures = new Map<string, LineOf<'capture'>>();
  const firstRequestOfTurn = new Map<number, number>();
  // Each call, keyed by the `seq` of its first fragment (or of its line, where none streamed); found by its request and index.
  const calls = new Map<number, { request: number; t: number; first?: LineOf<'delta'>; id?: string; name?: Tool; args: string; line?: LineOf<'tool_call'> }>();
  const callAt = new Map<string, number>();
  const forks = new Map<number, { fork: LineOf<'fork'>; request?: number; settled?: LineOf<'fork.settled'>; patches: LineOf<'patch'>[]; audited?: string[] }>();
  const entries = new Map<string, Mutable<Omit<MemoryEntry, 'fresh' | 'landedAt'>> & { seq: number }>();
  // The tangent open now (#608), the turns asked inside each, and the turns a close rolled the trunk back over.
  let openTangent: string | undefined;
  let tangentsOpened = 0;
  const tangentTurns = new Map<string, number[]>();
  const rolledBack = new Set<number>();

  type Slot =
    | { kind: 'user'; turn: number }
    | { kind: 'assistant'; request: number }
    | { kind: 'tool'; call: number }
    | { kind: 'settled'; line: LineOf<'turn.settled'> };
  const system = start.head.find((m) => m.role === 'system');
  const eras: { seam?: LineOf<'seam'>; system: SystemNode; slots: Slot[] }[] = [
    {
      system: {
        kind: 'system',
        // Part of a line, not the whole of one: the line's own id is its session start, or its seam.
        id: `system/${start.seq}`,
        text: system?.content ?? '',
        ...(start.system_tokens !== undefined ? { tokens: start.system_tokens } : {}),
        ...provenance(start),
      },
      slots: [],
    },
  ];
  const era = () => eras[eras.length - 1]!;

  const unknown = new Map<string, number>();
  const settles = new Map<number, LineOf<'turn.settled'>>();
  const offTrunk = new Map<number, OffTrunk>();
  // Each turn's trunk requests, in order: a failed or timed-out turn keeps every step a later request followed (#541).
  const trunkRequestsOfTurn = new Map<number, number[]>();
  /** Turns whose latest trunk response hit the output cap. */
  const cappedTurns = new Set<number>();
  const gaps: Folded<GapNode>[] = [];
  let lastSettled: number | undefined;
  // The phase it opens in: the graph's opening phase (log v7, #563), or a placed recording's own `phase`.
  let phase = start.opening_phase ?? start.phase ?? '';
  let proposal: { call: string; from?: string; to: string; reason?: string } | undefined;
  const rulings = new Map<string, LineOf<'phase.ruled'>>();
  let openTurn: number | undefined;
  let lastAskSeq = -1;
  // The state as the log says it, when it says it (`diet` logs every move; a script logs only the end).
  let settledTo: LineOf<'settlement'>['to'] | undefined;

  for (const e of lines) {
    switch (e.kind) {
      case 'session.start':
      case 'refused':
      case 'stop.asked':
        break;
      case 'idle.gap': {
        const opened = settles.get(e.opened_by);
        const measured = e.notice + e.read + e.compose + e.away + e.blocked;
        gaps.push(
          brand<GapNode>({
            id: id(e.seq),
            openedBy: id(e.opened_by),
            notice: e.notice,
            read: e.read,
            compose: e.compose,
            away: e.away,
            blocked: e.blocked,
            endedBy: e.ended_by,
            ...(opened ? { residual: measured - (e.t - opened.t) } : {}),
            ...provenance(e),
          }),
        );
        break;
      }
      case 'settlement':
        settledTo = e.to;
        break;
      case 'ask':
        asks.set(e.turn, e);
        if (openTangent !== undefined) tangentTurns.get(openTangent)?.push(e.turn);
        openTurn = e.turn;
        lastAskSeq = e.seq;
        era().slots.push({ kind: 'user', turn: e.turn });
        break;
      case 'request':
        generations.set(e.seq, { request: e, deltas: [], frames: [] });
        if (e.lane === 'trunk') {
          if (!firstRequestOfTurn.has(e.turn)) firstRequestOfTurn.set(e.turn, e.seq);
          trunkRequestsOfTurn.set(e.turn, [...(trunkRequestsOfTurn.get(e.turn) ?? []), e.seq]);
          // A later step on the trunk: the cap that matters is the latest step's.
          cappedTurns.delete(e.turn);
          era().slots.push({ kind: 'assistant', request: e.seq });
        } else if (e.fork !== undefined) {
          const f = forks.get(e.fork);
          if (f) f.request = e.seq;
        }
        break;
      case 'delta': {
        const g = generations.get(e.request);
        if (!('tool_call' in e) || e.tool_call === undefined) {
          g?.deltas.push(e);
          break;
        }
        // A fragment placed after its response (an authored call, `place.ts`) is not the answer being written.
        if (g && !g.response) g.deltas.push(e);
        const piece = e.tool_call;
        const key = `${e.request}/${piece.index}`;
        const at = callAt.get(key);
        const call = at !== undefined ? calls.get(at) : undefined;
        if (call) {
          call.args += piece.arguments;
          if (call.id === undefined && piece.id !== undefined) call.id = piece.id;
          if (call.name === undefined && piece.name !== undefined) call.name = piece.name;
        } else {
          calls.set(e.seq, { request: e.request, t: e.t, first: e, ...(piece.id !== undefined ? { id: piece.id } : {}), ...(piece.name !== undefined ? { name: piece.name } : {}), args: piece.arguments });
          callAt.set(key, e.seq);
          era().slots.push({ kind: 'tool', call: e.seq });
        }
        break;
      }
      case 'progress':
        generations.get(e.request)?.frames.push(e);
        break;
      case 'response': {
        const g = generations.get(e.to_request);
        if (g) g.response = e;
        if (g && e.capped && g.request.lane === 'trunk' && g.request.turn !== undefined) cappedTurns.add(g.request.turn);
        break;
      }
      case 'cancelled': {
        const g = generations.get(e.request);
        if (g) g.cancelled = e;
        break;
      }
      case 'request.failed': {
        const g = generations.get(e.request);
        if (g) g.failed = e;
        break;
      }
      case 'tool_call': {
        const streamed = [...calls.values()].find((c) => c.request === e.request && c.id === e.id && !c.line);
        if (streamed) streamed.line = e;
        else {
          // A call whose fragments the log does not carry: drawn from its line alone.
          calls.set(e.seq, { request: e.request, t: e.t, id: e.id, name: e.name, args: e.arguments, line: e });
          era().slots.push({ kind: 'tool', call: e.seq });
        }
        break;
      }
      case 'turn.settled':
        settles.set(e.seq, e);
        if (e.reason === 'cancelled' || e.reason === 'failed' || e.reason === 'timeout') offTrunk.set(e.turn, e.reason as OffTrunk);
        lastSettled = e.seq;
        if (openTurn === e.turn) openTurn = undefined;
        // A turn that ended on its own, or was cancelled (the message says so), needs no mark.
        if (e.reason !== 'final' && e.reason !== 'cancelled') era().slots.push({ kind: 'settled', line: e });
        break;
      case 'fork':
        // A seam's audit (#646) rules on the working memory live when it opens: what it leaves alone, it kept.
        forks.set(e.seq, { fork: e, patches: [], ...(e.lane === 'audit' ? { audited: [...entries.values()].filter((x) => x.state === 'live').map((x) => x.id) } : {}) });
        break;
      case 'fork.settled': {
        const f = forks.get(e.fork);
        if (f) f.settled = e;
        break;
      }
      case 'patch': {
        // A fork's patch is drawn on its branch too; the trunk's own (a `lane`, #627) only in working memory.
        if (e.fork !== undefined) forks.get(e.fork)?.patches.push(e);
        const old = entries.get(e.entry.id);
        const base = {
          ...(e.entry.category !== undefined ? { category: e.entry.category } : {}),
          ...(e.authority !== undefined ? { authority: e.authority } : {}),
          text: e.entry.text,
          by: id(e.seq),
          seq: e.seq,
          ...provenance(e),
        };
        if (e.op === 'retire') {
          if (old) entries.set(e.entry.id, { ...old, state: 'retired', by: id(e.seq), seq: e.seq, from: [...old.from, e.seq] });
          break;
        }
        if (e.op === 'supersede' && e.supersedes) {
          const replaced = entries.get(e.supersedes);
          if (replaced) entries.set(e.supersedes, { ...replaced, state: 'superseded', by: id(e.seq), seq: e.seq, from: [...replaced.from, e.seq] });
        }
        if (e.op === 'add' || e.op === 'supersede' || !old) {
          entries.set(e.entry.id, { id: e.entry.id, state: 'live', ...base, ...(e.op !== 'add' && e.op !== 'supersede' ? { op: e.op } : {}), ...(e.tangent !== undefined ? { tangent: e.tangent } : {}), ...(e.lane !== undefined ? { lane: e.lane } : {}) });
          break;
        }
        // Any other op rewrites the entry, keeps its state, and is shown by name.
        entries.set(e.entry.id, { ...old, ...base, op: e.op, from: [...old.from, e.seq] });
        break;
      }
      case 'delivered':
        deliveries.set(e.turn, e);
        break;
      case 'recalled':
        recalls.set(e.turn, e);
        break;
      case 'notice':
        notices.set(e.turn, e);
        break;
      // A background job's end (#614): onto the call that started it; what it said reaches the model as the next ask's
      // notice.
      case 'background.ended':
        backgroundEnds.set(e.job, e);
        break;
      // A call near its timeout (#613): the surface's warning, which the model never sees -- onto the running call.
      case 'timeout.near':
        nearTimeouts.set(`${e.request}/${e.call}`, e);
        break;
      // A fork screened out of its gap (#611): no branch to draw; folded into no node yet.
      case 'fork.skipped':
        break;
      case 'pruned':
        prunes.set(e.turn, [...(prunes.get(e.turn) ?? []), e]);
        break;
      case 'reminded':
        reminders.set(e.turn, e);
        break;
      case 'capture':
        captures.set(`${e.request}/${e.call}`, e);
        // The model's phase proposal (#651): the latest one waits on the operator until a ruling names its call.
        if (e.tool === 'propose_phase_transition' && e.outcome === 'proposed' && e.to !== undefined) {
          const args = objectOf(calls.get([...calls.keys()].find((k) => calls.get(k)?.id === e.call && calls.get(k)?.request === e.request) ?? -1)?.args ?? '');
          proposal = { call: e.call, ...(e.from !== undefined ? { from: e.from } : {}), to: e.to, ...(typeof args['reason'] === 'string' ? { reason: args['reason'] } : {}) };
        }
        break;
      case 'phase.ruled':
        rulings.set(e.call, e);
        if (proposal?.call === e.call) proposal = undefined;
        // "continue" moves the phase with no seam; "seam" is followed by the seam line, which moves it.
        if (e.choice === 'continue') phase = e.to;
        break;
      case 'seam': {
        for (const call of e.pruned ?? []) replacedBySeam.add(call);
        if (e.phase) phase = e.phase.to;
        // What the model was sent after the seam -- `diet`'s `seam::render::refill`, whose output the record's head
        // check verifies: since #597 the head as it was and a user message carrying the render (and the tool outputs
        // the seam carried, #553); before, the head's system message, a blank line, then the render.
        const message = 'placement' in e && e.placement === 'message';
        const rendered: SystemNode = {
          kind: 'system',
          id: `system/${e.seq}`,
          text: message
            ? `<summary>\n${e.render}${'outputs' in e && e.outputs !== undefined ? e.outputs : ''}\n</summary>`
            : system
              ? `${system.content}\n\n${e.render}`
              : e.render,
          ...(message ? { placement: 'message' as const } : {}),
          ...(e.render_tokens !== undefined ? { tokens: e.render_tokens } : {}),
          render: e.render_version !== undefined ? `v${e.render_version}` : (e.frame ?? 'frame not recorded'),
          ...provenance(e),
        };
        eras.push({ seam: e, system: rendered, slots: [] });
        break;
      }
      case 'tangent.open':
        openTangent = e.id;
        tangentsOpened += 1;
        tangentTurns.set(e.id, []);
        break;
      case 'tangent.close': {
        // Dropped entries retire and parked ones are set aside -- the archive keeps both; the trunk returns to the open.
        const rule = (ids: readonly string[], state: EntryState) => {
          for (const entry of ids) {
            const old = entries.get(entry);
            if (old) entries.set(entry, { ...old, state, by: id(e.seq), seq: e.seq, from: [...old.from, e.seq] });
          }
        };
        rule(e.dropped, 'retired');
        rule(e.parked, 'parked');
        for (const turn of tangentTurns.get(e.id) ?? []) rolledBack.add(turn);
        if (openTangent === e.id) openTangent = undefined;
        break;
      }
      default: {
        // Every kind the log names is handled above -- `never` keeps the
        // compiler checking that. What reaches here at run time is a kind from a
        // newer drive: the log carries it before the surface draws it.
        const newer: never = e;
        const kind = (newer as { readonly kind: string }).kind;
        unknown.set(kind, (unknown.get(kind) ?? 0) + 1);
      }
    }
  }

  // When each call began running -- AN INFERENCE, as v3 has no start line (ruled on #300, ruling 3): its response, or
  // the call before it on that response ending, or its own fragment if later. A call whose predecessor has not ended
  // has not begun.
  const callStarts = new Map<number, number>();
  const notBegun = new Set<number>();
  const unanswered = new Set<number>();
  const lastEnded = new Map<number, number | undefined>();
  for (const [key, c] of calls) {
    const answered = generations.get(c.request)?.response?.t;
    if (answered === undefined) unanswered.add(key);
    const before = lastEnded.has(c.request) ? lastEnded.get(c.request) : c.t;
    if (before === undefined) notBegun.add(key);
    callStarts.set(key, Math.max(c.t, answered ?? c.t, before ?? c.t));
    lastEnded.set(c.request, c.line?.t);
  }

  // Trunk nodes, era by era.
  const trunkPrefixAt = (timings: Timings | undefined) => (timings ? timings.prompt_n + timings.cache_n + timings.predicted_n : undefined);
  let previousEraEnd: Timings | undefined;
  const builtEras: Era[] = eras.map((raw, index) => {
    const nodes: TrunkNode[] = raw.slots.map((slot): TrunkNode => {
      /**
       * Whether a turn's ask, or the answer of its request REQUEST, left the model's context (#289). A failed or
       * timed-out one keeps its ask and every step a later request followed, as `diet` does since #541 -- only its last
       * request, the failing step, leaves; one that failed on its first request keeps nothing. A cancelled one keeps its
       * ask, every step, and what it had said when the cancel came, as `diet` does since #575: only a generation the
       * cancel cut before it said anything leaves, and the ask with it when that was the turn's only request.
       */
      const outOf = (turn: number | undefined, request?: number): OffTrunk | undefined => {
        // A tangent's close rolled the trunk back over every turn asked inside it (#608): ask and answers alike.
        if (turn !== undefined && rolledBack.has(turn)) return 'rolled-back';
        const why = turn !== undefined ? offTrunk.get(turn) : undefined;
        if (why === undefined) return why;
        const steps = trunkRequestsOfTurn.get(turn!) ?? [];
        if (why === 'cancelled') {
          const silent = (seq: number | undefined) => seq !== undefined && generations.get(seq)?.cancelled?.partial === '';
          if (request === undefined) return steps.length <= 1 && silent(steps[0]) ? why : undefined;
          return request === steps.at(-1) && silent(request) ? why : undefined;
        }
        if (request === undefined) return steps.length > 1 ? undefined : why;
        return request === steps.at(-1) ? why : undefined;
      };
      switch (slot.kind) {
        case 'user': {
          const ask = asks.get(slot.turn)!;
          const firstSeq = firstRequestOfTurn.get(slot.turn);
          const first = firstSeq !== undefined ? generations.get(firstSeq) : undefined;
          const timings = first?.response?.timings;
          return brand<UserNode>({
            kind: 'user',
            id: id(ask.seq),
            turn: slot.turn,
            text: ask.text,
            endedAt: ask.t,
            ...(timings ? { prefill: { fresh: timings.prompt_n, cached: timings.cache_n } } : {}),
            ...(outOf(slot.turn) ? { outOfContext: outOf(slot.turn)! } : {}),
            ...(ask.files && ask.files.length > 0 ? { files: ask.files } : {}),
            ...(ask.scoping === true ? { scoping: true as const } : {}),
            ...(deliveries.has(slot.turn)
              ? { delivered: { framing: deliveries.get(slot.turn)!.framing, text: deliveries.get(slot.turn)!.text } }
              : {}),
            ...(recalls.has(slot.turn)
              ? { recalled: { recall: recalls.get(slot.turn)!.recall, text: recalls.get(slot.turn)!.text } }
              : {}),
            ...(notices.has(slot.turn) ? { noticed: notices.get(slot.turn)!.text } : {}),
            ...(prunes.has(slot.turn)
              ? { pruned: prunes.get(slot.turn)!.map(({ call, bytes, text }) => ({ call, bytes, text })) }
              : {}),
            ...(reminders.has(slot.turn) ? { reminded: reminders.get(slot.turn)!.text } : {}),
            ...provenance(ask, first?.response),
          });
        }
        case 'assistant': {
          const g = generations.get(slot.request)!;
          const node: AssistantNode = {
            kind: 'assistant',
            id: id(g.request.seq),
            turn: g.request.turn,
            ...generation(g, trunkSlot),
            ...(outOf(g.request.turn, g.request.seq) ? { outOfContext: outOf(g.request.turn, g.request.seq)! } : {}),
            ...provenance(g.request, ...g.deltas.slice(0, 1), g.response, g.cancelled, g.failed),
          };
          return brand(node);
        }
        case 'tool': {
          const c = calls.get(slot.call)!;
          const line = c.line;
          const startedAt = callStarts.get(slot.call)!;
          const text = line?.arguments ?? c.args;
          const confinement = line && (line.confined || line.isolation || line.network) ? { ...(line.confined ? { confined: line.confined } : {}), ...(line.isolation ? { isolation: line.isolation } : {}), ...(line.network ? { network: line.network } : {}) } : undefined;
          return brand<ToolNode>({
            kind: 'tool',
            id: id(slot.call),
            turn: line?.turn ?? generations.get(c.request)?.request.turn ?? 0,
            tool: line?.name ?? c.name ?? '',
            arguments: text,
            args: objectOf(text),
            after: id(c.request),
            call: c.id !== undefined ? { request: c.request, id: c.id } : undefined,
            startedAt,
            running: line === undefined && !notBegun.has(slot.call) && !unanswered.has(slot.call),
            ...(line === undefined && c.id !== undefined && nearTimeouts.has(`${c.request}/${c.id}`)
              ? { nearTimeout: { timeoutMs: nearTimeouts.get(`${c.request}/${c.id}`)!.timeout_ms, at: nearTimeouts.get(`${c.request}/${c.id}`)!.t } }
              : {}),
            ...(line === undefined && unanswered.has(slot.call) ? { writing: true as const } : line === undefined && notBegun.has(slot.call) ? { waiting: true as const } : {}),
            ...(line
              ? {
                  outcome: line.outcome,
                  ms: Math.max(0, line.t - startedAt),
                  endedAt: line.t,
                  ...(line.argv ? { argv: line.argv } : {}),
                  ...(line.cwd !== undefined ? { cwd: line.cwd } : {}),
                  ...(line.approval ? { approval: line.approval } : {}),
                  ...(line.files && line.files.length > 0 ? { files: line.files } : {}),
                  ...(confinement ? { confinement } : {}),
                  ...(line.exit !== undefined ? { exit: line.exit } : {}),
                  ...(line.stdout !== undefined ? { output: line.stdout } : {}),
                  ...(line.stderr ? { stderr: line.stderr } : {}),
                  ...(rulings.has(line.id) ? { ruled: { choice: rulings.get(line.id)!.choice, to: rulings.get(line.id)!.to } } : {}),
                  ...(line.reason !== undefined ? { refusal: line.reason } : {}),
                  ...(() => {
                    const cut = [...prunes.values()].flat().find((p) => p.call === line.id);
                    return cut ? { pruned: { bytes: cut.bytes, replaced: replacedBySeam.has(line.id) } } : {};
                  })(),
                  ...(line.background !== undefined
                    ? (() => {
                        const ended = backgroundEnds.get(line.background);
                        return { background: { job: line.background, ...(ended ? { status: ended.status } : {}), ...(ended?.exit !== undefined ? { exit: ended.exit } : {}) } };
                      })()
                    : {}),
                  ...(captures.has(`${c.request}/${line.id}`)
                    ? (() => {
                        const k = captures.get(`${c.request}/${line.id}`)!;
                        return { capture: { outcome: k.outcome, entries: k.entries, ...(k.why !== undefined ? { why: k.why } : {}) } };
                      })()
                    : {}),
                  ...(line.policy !== undefined ? { policy: line.policy } : {}),
                }
              : {}),
            ...provenance(c.first, line),
          });
        }
        case 'settled':
          return brand<SettledNode>({
            kind: 'settled',
            id: id(slot.line.seq),
            turn: slot.line.turn,
            reason: slot.line.reason,
            ...(slot.line.reason === 'failed' && cappedTurns.has(slot.line.turn) ? { capped: true as const } : {}),
            endedAt: slot.line.t,
            ...provenance(slot.line),
          });
      }
    });
    const seamLine = raw.seam;
    const seam = seamLine
      ? brand<SeamNode>({
          kind: 'seam',
          id: id(seamLine.seq),
          atTurn: seamLine.at_turn,
          reason: seamLine.reason,
          ...(seamLine.phase ? { phase: seamLine.phase } : {}),
          hashBefore: seamLine.prefix_hash_before,
          hashAfter: seamLine.prefix_hash_after,
          ...(previousEraEnd ? { prefixBefore: trunkPrefixAt(previousEraEnd)! } : {}),
          ...(seamLine.render_tokens !== undefined ? { prefixAfter: seamLine.render_tokens } : {}),
          ...(seamLine.carried_entries !== undefined && seamLine.carried_turns !== undefined
            ? { carried: { entries: seamLine.carried_entries, turns: seamLine.carried_turns } }
            : {}),
          ...(seamLine.warm ? { warm: seamLine.warm } : {}),
          ...(seamLine.prompt_tokens !== undefined && seamLine.window !== undefined ? { size: { promptTokens: seamLine.prompt_tokens, window: seamLine.window } } : {}),
          ...provenance(seamLine),
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
  const openForks: LineOf<'fork'>[] = [];
  for (const { fork, request, settled, patches, audited } of forks.values()) {
    const g = request !== undefined ? generations.get(request) : undefined;
    const at = id(fork.at);
    const slot = fork.slot ?? g?.request.slot ?? sideSlot;
    if (!settled) openForks.push(fork);
    const node = brand<BranchNode>({
      kind: 'branch',
      id: id(fork.seq),
      lane: fork.lane,
      slot,
      at,
      why: fork.why,
      question: fork.question,
      ...(fork.prefix_tokens !== undefined ? { prefixTokens: fork.prefix_tokens } : {}),
      ...(fork.trigger !== undefined ? { trigger: fork.trigger } : {}),
      ...(fork.substrate !== undefined && fork.model !== undefined
        ? {
            seat: {
              substrate: fork.substrate,
              model: fork.model,
              ...(settled?.prompt_tokens !== undefined ? { promptTokens: settled.prompt_tokens } : {}),
              ...(settled?.wall_ms !== undefined ? { wallMs: settled.wall_ms } : {}),
            },
          }
        : {}),
      // A side call has finished when it settles, after its patches -- not at its response.
      ...(g ? unended(generation(g, slot)) : {}),
      ...(settled ? { outcome: settled.outcome, endedAt: settled.t } : {}),
      ...(settled?.refused !== undefined ? { refused: settled.refused } : {}),
      ...(fork.hazard !== undefined ? { hazard: fork.hazard } : {}),
      ...(audited !== undefined && settled?.outcome === 'value'
        ? (() => {
            const updated = patches.filter((p) => p.op === 'supersede' && p.supersedes !== undefined).map((p) => p.supersedes!);
            const removed = patches.filter((p) => p.op === 'retire').map((p) => p.entry.id);
            return { audit: { kept: audited.filter((x) => !updated.includes(x) && !removed.includes(x)), updated, removed } };
          })()
        : {}),
      patches: patches.map((p) =>
        brand<PatchNode>({
          id: id(p.seq),
          op: p.op,
          entryId: p.entry.id,
          ...(p.entry.category !== undefined ? { category: p.entry.category } : {}),
          text: p.entry.text,
          ...(p.supersedes ? { supersedes: p.supersedes } : {}),
          ...(p.authority ? { authority: p.authority } : {}),
          ...provenance(p),
        }),
      ),
      ...provenance(fork, g?.request, g?.response, settled),
    });
    const list = branches.get(at) ?? [];
    list.push(node);
    branches.set(at, list);
  }

  // The server's slots: as many as the session declares, or as its branches use -- `diet` declares none.
  const slots = Math.max(start.slots ?? 1, trunkSlot + 1, ...[...branches.values()].flat().map((b) => b.slot + 1));
  // Slot occupancy: whatever request is generating, per slot. A fork's request that names none is on its branch's.
  const occupancy: (Holder | undefined)[] = Array.from({ length: slots }, () => undefined);
  const forkSlot = new Map([...branches.values()].flat().map((b) => [Number(b.id), b.slot]));
  for (const g of generations.values()) {
    const unnamed = g.request.fork !== undefined ? (forkSlot.get(g.request.fork) ?? sideSlot) : trunkSlot;
    if (!g.response && !g.cancelled && !g.failed) occupancy[g.request.slot ?? unnamed] = { id: id(g.request.fork ?? g.request.seq), lane: g.request.lane };
  }

  const ratifying = openForks.some((f) => f.lane === 'ratify');
  const state: SessionState =
    settledTo === 'ended'
      ? 'ended'
      : settledTo !== undefined
        ? settledTo === 'capture' && ratifying
          ? 'ratify'
          : settledTo
        : openTurn !== undefined
          ? 'turn'
          : ratifying
            ? 'ratify'
            : openForks.length > 0
              ? 'capture'
              : 'awaiting';

  const memory = [...entries.values()].map(({ seq, ...entry }) => brand<MemoryEntry>({ ...entry, landedAt: seq, fresh: seq > lastAskSeq }));

  return {
    state,
    ...(lastSettled !== undefined ? { lastSettled } : {}),
    gaps,
    opened: start.opened,
    arm: start.arm ?? '',
    levers: leversOf(start),
    model: start.model,
    slots,
    trunkSlot,
    phase,
    tangentsOpened,
    ...(proposal ? { proposal } : {}),
    ...(openTangent !== undefined
      ? { tangent: { id: openTangent, entries: [...entries.values()].filter((x) => x.tangent === openTangent && x.state === 'live').map((x) => x.id) } }
      : {}),
    phaseMoves: (start.phase_transitions ?? []).filter((move) => move.from === phase).map((move) => move.to),
    eras: builtEras,
    branches,
    memory,
    occupancy,
    now: lines.at(-1)?.t ?? 0,
    events: lines.length,
    unknown,
    receipt: { ...receiptOf(lines), liveEntries: memory.filter((m) => m.state === 'live').length },
  };
}
