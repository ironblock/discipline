/**
 * The session's log, as the surface reads it: `diet/formats/log` v0 (#137,
 * `diet/src/formats/log.rs`), mirrored by hand -- and, beside it, what the
 * surface draws that v0 does not say yet, each tagged with the step of #117
 * that will make `diet` say it.
 *
 * HAND-MIRRORED, NOT GENERATED. #31 asks for bindings generated from the
 * format and checked in CI; the generator is track one's, queued after I1
 * (ruled on #117, 2026-09-28). Until it lands, `log.test.ts` folds every
 * valid fixture in `diet/formats/log/fixtures/valid/` and fails on anything
 * this file cannot read -- `diet`'s fixtures stay the one source. The PR that
 * adopts the generated types deletes this mirror.
 *
 * References are the `seq` of the line they name, as v0 has them: an
 * identifier the log issues, never one anybody invents. What v0 does not
 * have yet is marked AHEAD (a kind, or an optional field on a v0 kind); a v0
 * reader would refuse it, and a session run against `diet` today carries
 * none of it -- the surface draws what it can without it.
 */

/** A step of #117, and what it makes the drive able to say. One source. */
export const NEEDS = {
  R2: 'the interactive drive: an ask from a person, the trunk appended until a seam, settlement, cancel',
  R3: 'per-request timings and cache telemetry; streaming',
  R4: 'warm-tail interview forks in the idle gap, on declared slots',
  R5: 'patches and working memory, visible',
  R6: 'the human-declared seam: ratify, render, refill, pre-warm',
} as const;

export type Need = keyof typeof NEEDS;

/**
 * An OPEN set: its members come from `diet`, the regimen or the person and
 * grow without a surface release. The known members are named (for the
 * registries in `src/ui/sets.ts`, and for autocomplete); any other string is
 * carried through the fold and drawn neutrally under its own name. A CLOSED
 * set -- the surface's own, like a node's kind -- stays a plain union, so
 * adding a member breaks the build everywhere that must handle it.
 */
export type Open<Known extends string> = Known | (string & {});

// ------------------------------------------------------------------ v0's sets

/** What the session is doing (v0 `State`). */
export type State = 'awaiting' | 'turn' | 'capture' | 'ended';
/** A command a person sent (v0 `Command`). */
export type CommandKind = 'ask' | 'cancel' | 'declare-seam' | 'end';
/** Why a command was refused (v0 `Refusal`). */
export type RefusalReason = Open<'in-flight' | 'ended' | 'nothing-in-flight' | 'seam-not-built' | 'stale'>;
/** Who a head message is from (v0 `Role`). */
export type Role = 'system' | 'user' | 'assistant';
/** How a turn ended (v0 `SettleReason`). */
export type SettleReason = Open<'final' | 'cancelled' | 'max_steps' | 'timeout' | 'failed'>;
/** What ended an idle gap (v0 `GapEnd`). */
export type GapEnd = 'ask' | 'seam' | 'cancel' | 'end';
/**
 * Why a call ended without an answer (v0 `FailReason`: server, timeout,
 * transport, crashed). AHEAD: `context_overflow`, the predecessor's commonest
 * hard failure, which v0 folds into `server`.
 */
export type FailReason = Open<'server' | 'timeout' | 'transport' | 'crashed' | 'context_overflow'>;

/** The side lanes a fork runs in (AHEAD, R4). `extraction` is the predecessor's mechanical read of the trunk. */
export type ForkLane = Open<'interview' | 'ratify' | 'extraction'>;
/** Which lane a request was made on: v0 has only `trunk`; the rest are AHEAD (R4). */
export type Lane = 'trunk' | ForkLane;

/** llama.cpp's per-request `timings`, the fields the surface reads (AHEAD, R3). */
export interface Timings {
  /** Prompt tokens evaluated for this request: the new part of the prefix. */
  readonly prompt_n: number;
  /** Prompt tokens reused from the slot's cache. */
  readonly cache_n: number;
  readonly prompt_ms: number;
  /** Tokens generated, reasoning included. */
  readonly predicted_n: number;
  readonly predicted_ms: number;
}

/** The tools a model may call (AHEAD, R2's later bump). Only `bash` is drawn by name; any other as `name(args)`. */
export type Tool = Open<'bash'>;

/** How a fork ended: the one enum, ruled on #117 (2026-09-26, naming 6) for the record (AHEAD, R4). */
export type ForkOutcome = Open<'value' | 'decline' | 'mimicry' | 'unparseable' | 'thinking_exhausted' | 'rejected' | 'timeout' | 'truncated' | 'output_too_large'>;

/** A change to working memory: `diet`'s ops, ruled on #117 (naming 4) (AHEAD, R5). */
export type PatchOp = Open<'add' | 'supersede' | 'resolve' | 'retire' | 'park' | 'edit'>;

/** How an entry was known -- `authority`, ruled on #117 (naming 5) (AHEAD, R5). */
export type Authority = Open<'stated' | 'extracted' | 'observed' | 'arm'>;

export type SeamReason = Open<'operator' | 'phase' | 'cadence' | 'budget'>;

// ------------------------------------------------------------------ lines

interface At {
  /** Its position in the log: gapless, from 0. The primary key, and what references name. */
  readonly seq: number;
  /** Milliseconds since the session opened. */
  readonly t: number;
}

export interface HeadMessage {
  readonly role: Role;
  readonly content: string;
}

/** The session opened. The first line, and only the first. */
export interface SessionStart extends At {
  readonly kind: 'session.start';
  readonly version: number;
  /** When, in milliseconds since the Unix epoch: the stream's identity (Q11). */
  readonly opened: number;
  /** The model name requests are sent with -- a name, not an identity. */
  readonly model: string;
  /** The messages the trunk starts from; the system prompt is its `system` message. */
  readonly head: readonly HeadMessage[];
  /** AHEAD (R2's record fields): the arm this session runs. */
  readonly arm?: string;
  /** AHEAD (R4): the server's `-np`, how many requests it serves at once. */
  readonly slots?: number;
  /** AHEAD (R4): the slot the trunk is pinned to. */
  readonly trunk_slot?: number;
  /** AHEAD (R6): the phase the session opens in. */
  readonly phase?: string;
  /** AHEAD (R3): the system prompt's size in tokens. */
  readonly system_tokens?: number;
}

/** An ask was admitted, and a turn begins on it. */
export interface Ask extends At {
  readonly kind: 'ask';
  /** The turn it begins, from 1. */
  readonly turn: number;
  readonly text: string;
}

/** The session's state moved. */
export interface Settlement extends At {
  readonly kind: 'settlement';
  readonly from: State;
  readonly to: State;
}

/** A call was made to the model. Its `seq` is its identity: what its deltas, answer or failure name. */
export interface Request extends At {
  readonly kind: 'request';
  readonly turn: number;
  readonly lane: Lane;
  /** AHEAD (R4): the server slot it went to. */
  readonly slot?: number;
  /** AHEAD (R4): the `seq` of the `fork` this call serves, for a side lane. */
  readonly fork?: number;
}

/** A command was refused. */
export interface Refused extends At {
  readonly kind: 'refused';
  readonly command: CommandKind;
  readonly because: RefusalReason;
  readonly during: State;
}

/** A piece of a call's answer: exactly one of `text` and `reasoning`. */
export interface Delta extends At {
  readonly kind: 'delta';
  /** The `seq` of the `request` it answers. */
  readonly request: number;
  readonly text?: string;
  readonly reasoning?: string;
}

/** A stop was asked for a turn's call. */
export interface StopAsked extends At {
  readonly kind: 'stop.asked';
  readonly turn: number;
}

/** A call answered. */
export interface Response extends At {
  readonly kind: 'response';
  /** The `seq` of the `request` it answers. */
  readonly to_request: number;
  /** The whole answer. */
  readonly text: string;
  /** Why the server stopped, as it spelled it, if it said: llama.cpp's `stop`, `length`, `tool_calls`. */
  readonly finish_reason?: string;
  /** AHEAD (R3): llama.cpp's timings for the call. */
  readonly timings?: Timings;
  /**
   * AHEAD (R3, the surface's ask, ruled into R3's scope on #117): where its
   * tool calls began in what it wrote -- tokens and ms generated before the
   * first tool-call chunk. The rest of `timings`' generation is the calls'.
   * `predicted_n` alone absent when only the time was kept (a harness's
   * transcript that recorded when a call part began, not what came before).
   */
  readonly calls_from?: { readonly predicted_n?: number; readonly predicted_ms: number };
  /** AHEAD (R2/I5r): the whole reasoning, as the deltas streamed it. */
  readonly reasoning?: string;
}

/** A call was stopped. What arrived before is never an answer. */
export interface Cancelled extends At {
  readonly kind: 'cancelled';
  /** The `seq` of the `request` stopped. */
  readonly request: number;
  readonly partial: string;
}

/** A call ended without an answer: refused by the server, failed, or its thread crashed. */
export interface RequestFailed extends At {
  readonly kind: 'request.failed';
  /** The `seq` of the `request`. */
  readonly request: number;
  readonly reason: FailReason;
  /** What the server, the transport or the panic said. */
  readonly message: string;
  /** The HTTP status, when the server refused it. */
  readonly status?: number;
  /** What arrived before it ended, when anything did. */
  readonly partial?: string;
}

/** A turn is over. */
export interface TurnSettled extends At {
  readonly kind: 'turn.settled';
  readonly turn: number;
  readonly reason: SettleReason;
}

/**
 * A person's idle gap after a settled turn, as the surface measured it (Q4,
 * ruled on #117): integer ms on the surface's monotonic clock, durations
 * only. The five sum to the gap's wall clock -- from the settling to the
 * accepted command -- within the residual the fold reports.
 */
export interface IdleGap extends At {
  readonly kind: 'idle.gap';
  /** The `seq` of the `turn.settled` that opened the gap. */
  readonly opened_by: number;
  readonly notice: number;
  readonly read: number;
  readonly compose: number;
  readonly away: number;
  readonly blocked: number;
  readonly ended_by: GapEnd;
}

// ------------------------------------------------------------------ AHEAD kinds

/**
 * AHEAD (R3): where a request is, right now -- how much of its prompt is
 * read, and how many tokens it has generated. From llama.cpp's stream if it
 * carries prompt progress, else its `/slots`, polled once a second while
 * busy (ruled 2026-09-26; the surface's ask, R3's scope). Transient.
 */
export interface ProgressFrame extends At {
  readonly kind: 'progress';
  /** The `seq` of the `request`. */
  readonly request: number;
  readonly prompt: { readonly total: number; readonly cache: number; readonly processed: number };
  readonly decoded: number;
}

/** AHEAD (R2's later bump, DoD 2): a tool call began. */
export interface ToolBegin extends At {
  readonly kind: 'tool.begin';
  readonly turn: number;
  /** The `seq` of the `request` whose answer made the call. */
  readonly request: number;
  readonly tool: Tool;
  /** The call's arguments as the model gave them; `bash` takes `{ command }`. */
  readonly args: Readonly<Record<string, unknown>>;
}

/** AHEAD (R2's later bump, DoD 2): a tool call ended. */
export interface ToolEnd extends At {
  readonly kind: 'tool.end';
  /** The `seq` of its `tool.begin`. */
  readonly begin: number;
  readonly exit: number;
  readonly output: string;
  /** The harness cut the output before the model saw it. */
  readonly truncated?: boolean;
}

/** AHEAD (R4): a side call off the trunk's warm tail, on a slot of its own. */
export interface Fork extends At {
  readonly kind: 'fork';
  readonly lane: ForkLane;
  readonly slot: number;
  readonly of_turn: number;
  /** The `seq` of the trunk line it branches from: a `request` (its answer) or a `tool.begin`. */
  readonly at: number;
  /** What `diet` noticed that made it ask. */
  readonly why: string;
  readonly question: string;
  /** Prefix tokens shared with the trunk: the warm tail it forked from. */
  readonly prefix_tokens: number;
}

/** AHEAD (R4): how a fork ended. */
export interface ForkSettled extends At {
  readonly kind: 'fork.settled';
  /** The `seq` of the `fork`. */
  readonly fork: number;
  readonly outcome: ForkOutcome;
}

export interface Entry {
  /** Working memory's own id for the entry. */
  readonly id: string;
  readonly category?: string;
  readonly text: string;
}

/** AHEAD (R5): a change to working memory, from the fork that produced it. */
export interface Patch extends At {
  readonly kind: 'patch';
  /** The `seq` of the `fork` that produced it. */
  readonly fork: number;
  readonly op: PatchOp;
  readonly entry: Entry;
  /** For `supersede`: the entry this one replaces. */
  readonly supersedes?: string;
  readonly authority?: Authority;
}

/** AHEAD (R6): the one deliberate prefill event -- the trunk rebuilt from working memory. */
export interface Seam extends At {
  readonly kind: 'seam';
  readonly at_turn: number;
  readonly reason: SeamReason;
  readonly phase?: { readonly from: string; readonly to: string };
  readonly prefix_hash_before: string;
  readonly prefix_hash_after: string;
  /** The new system prompt: working memory rendered, with the phase's priming. */
  readonly render: { readonly version: number; readonly text: string; readonly tokens?: number };
  /** The pre-warm: the new prefix sent once so the next ask finds it cached. */
  readonly warm?: Timings;
}

export type LogLine =
  | SessionStart
  | Ask
  | Settlement
  | Request
  | Refused
  | Delta
  | StopAsked
  | Response
  | Cancelled
  | RequestFailed
  | TurnSettled
  | IdleGap
  | ProgressFrame
  | ToolBegin
  | ToolEnd
  | Fork
  | ForkSettled
  | Patch
  | Seam;

export type Kind = LogLine['kind'];

/** The line of one kind. */
export type LineOf<K extends Kind> = Extract<LogLine, { readonly kind: K }>;

/**
 * Which step of #117 each kind waits on: what the gaps overlay outlines.
 * v0's kinds wait on R2 (I1 landed the format; the drive that emits them is
 * I3 and I5); an AHEAD kind waits on the step that adds it.
 */
export const NEEDS_OF: { readonly [K in Kind]: readonly Need[] } = {
  'session.start': ['R2'],
  ask: ['R2'],
  settlement: ['R2'],
  request: ['R2'],
  refused: ['R2'],
  delta: ['R2'],
  'stop.asked': ['R2'],
  response: ['R2', 'R3'],
  cancelled: ['R2'],
  'request.failed': ['R2'],
  'turn.settled': ['R2'],
  'idle.gap': ['R2'],
  progress: ['R3'],
  'tool.begin': ['R2'],
  'tool.end': ['R2'],
  fork: ['R4'],
  'fork.settled': ['R4'],
  patch: ['R5'],
  seam: ['R6'],
};

/**
 * v0's keys per kind, required and optional, as `keys()` in `log.rs` has them
 * (every line also carries `seq`, `t` and `kind`): the mirror's one runtime
 * statement of v0, which `log.test.ts` holds `diet`'s fixtures to. A key or
 * a kind v0 gains fails that test until this file says it too.
 */
export const V0_KEYS: { readonly [K in string]: readonly [readonly string[], readonly string[]] } = {
  'session.start': [['version', 'opened', 'model', 'head'], []],
  ask: [['turn', 'text'], []],
  settlement: [['from', 'to'], []],
  request: [['turn', 'lane'], []],
  refused: [['command', 'because', 'during'], []],
  delta: [['request'], ['text', 'reasoning']],
  'stop.asked': [['turn'], []],
  response: [['to_request', 'text'], ['finish_reason']],
  cancelled: [['request', 'partial'], []],
  'request.failed': [['request', 'reason', 'message'], ['status', 'partial']],
  'turn.settled': [['turn', 'reason'], []],
  'idle.gap': [['opened_by', 'notice', 'read', 'compose', 'away', 'blocked', 'ended_by'], []],
};

/** v0's closed sets, as the mirror knows them: each fixture's values must be among them. */
export const V0_SETS = {
  state: ['awaiting', 'turn', 'capture', 'ended'],
  command: ['ask', 'cancel', 'declare-seam', 'end'],
  refusal: ['in-flight', 'ended', 'nothing-in-flight', 'seam-not-built', 'stale'],
  lane: ['trunk'],
  role: ['system', 'user', 'assistant'],
  fail: ['server', 'timeout', 'transport', 'crashed'],
  settle: ['final', 'cancelled', 'max_steps', 'timeout', 'failed'],
  gapEnd: ['ask', 'seam', 'cancel', 'end'],
} as const satisfies Record<string, readonly string[]>;

/** v0's kinds: the rest are AHEAD. */
export const V0_KINDS: ReadonlySet<string> = new Set(Object.keys(V0_KEYS));
