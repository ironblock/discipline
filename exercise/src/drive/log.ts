/**
 * The session's log, as the surface reads it: `diet/formats/log` v0, whose
 * types are GENERATED from `diet/src/formats/log.rs` (#144) and imported
 * here -- and, on top of them, what the surface draws that v0 does not say
 * yet, each marked AHEAD and tagged with the step of #117 that will make
 * `diet` say it. The hand mirror this file was is gone (ruled on #117,
 * 2026-09-28): a v0 line's shape is `diet`'s, checked in its CI.
 *
 * References are the `seq` of the line they name, as v0 has them: an
 * identifier the log issues, never one anybody invents. A session run
 * against `diet` today carries nothing AHEAD -- the surface draws what it can
 * without it.
 */

import type * as V0 from '../../../diet/formats/log/log.ts';

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

// ------------------------------------------------------------------ v0's sets, from `diet`

export type { GapEnd, HeadMessage, Role, State } from '../../../diet/formats/log/log.ts';
export type CommandKind = V0.Command;
export type RefusalReason = V0.Refusal;
/** How a turn ended: v0's, open to a newer drive's for the registries that draw them. */
export type SettleReason = Open<V0.SettleReason>;
/**
 * Why a call ended without an answer: v0's (server, timeout, transport,
 * crashed). AHEAD: `context_overflow`, the predecessor's commonest hard
 * failure, which v0 folds into `server`.
 */
export type FailReason = Open<V0.FailReason | 'context_overflow'>;

/** The side lanes a fork runs in (AHEAD, R4). `extraction` is the predecessor's mechanical read of the trunk. */
export type ForkLane = Open<'interview' | 'ratify' | 'extraction'>;
/** Which lane a request was made on: v0 has only `trunk`; the rest are AHEAD (R4). */
export type Lane = V0.Lane | ForkLane;

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

/** The tools a model may call: the function name as the server spelled it. Only `bash` is drawn by name; any other as `name(args)`. */
export type Tool = Open<'bash'>;

/**
 * What became of a call (v3, #297, ruled 5973541934): it ran; the drive
 * refused it before running it; it ran and failed under its policy (a
 * refused syscall on macOS, #29 I5: never `denied`); or the turn or session
 * ended before its outcome arrived.
 */
export type ToolOutcome = Open<V0.ToolOutcome>;

/**
 * Why the drive refused a call (v3, #297 Q2, ruled 5973541934; v4, #388): among them `denylist`, the command
 * matched the destructive denylist, and `declined`, the operator declined its prompt.
 */
export type ToolRefusal = Open<V0.ToolRefusal>;

/**
 * Under what a call ran (log v4, #388): the operator's decision on its prompt, or the pre-seeded session set.
 * `once` covers the exact line once; `session` and `workspace` its shape from then on (#298 5981578399 point 5).
 */
export type ApprovalScope = Open<V0.ApprovalScope>;

/**
 * A call's approval, on its `tool_call` line (log v4, #388): present when it ran under a decision or a pre-seed.
 * `decided_at` is session time (the line's clock), absent for `preseeded`; `why` is the reason its prompting
 * segment prompted (an open set of words), absent for `preseeded`. The format's, with its scope open.
 */
export type Approval = Omit<V0.Approval, 'scope'> & { readonly scope: ApprovalScope };

/**
 * The mechanism a command ran under (`Isolation::tag`, diet/src/isolation/policy.rs), or `unrecorded`: a replayed
 * session that never said, allowed only where `session.start` carries no substrate claim (ruled on #300).
 */
export type IsolationWord = Open<V0.Isolation>;
/** The network a command had (`Network::tag`), or `unrecorded`, as for isolation. */
export type NetworkWord = Open<V0.Network>;

/** How a fork ended: the one enum, ruled on #117 (2026-09-26, naming 6) for the record (AHEAD, R4). */
export type ForkOutcome = Open<'value' | 'decline' | 'mimicry' | 'unparseable' | 'thinking_exhausted' | 'rejected' | 'timeout' | 'truncated' | 'output_too_large'>;

/** A change to working memory: `diet`'s ops, ruled on #117 (naming 4) (AHEAD, R5). */
export type PatchOp = Open<'add' | 'supersede' | 'resolve' | 'retire' | 'park' | 'edit'>;

/** How an entry was known -- `authority`, ruled on #117 (naming 5) (AHEAD, R5). */
export type Authority = Open<'stated' | 'extracted' | 'observed' | 'arm'>;

export type SeamReason = Open<'operator' | 'phase' | 'cadence' | 'budget'>;

// ------------------------------------------------------------------ v0's lines, with what is AHEAD on them

interface At {
  /** Its position in the log: gapless, from 0. The primary key, and what references name. */
  readonly seq: number;
  /** Milliseconds since the session opened. */
  readonly t: number;
}

/** The session opened. The first line, and only the first. */
export type SessionStart = V0.SessionStartLine & {
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
};

export type Ask = V0.AskLine;
export type Settlement = V0.SettlementLine;

/** A call was made to the model. Its `seq` is its identity: what its deltas, answer or failure name. */
export type Request = Omit<V0.RequestLine, 'lane'> & {
  /** v0's `trunk`, or AHEAD (R4) a side lane. */
  readonly lane: Lane;
  /** AHEAD (R4): the server slot it went to. */
  readonly slot?: number;
  /** AHEAD (R4): the `seq` of the `fork` this call serves, for a side lane. */
  readonly fork?: number;
};

export type Refused = V0.RefusedLine;

/**
 * One fragment of a tool call as the server streamed it (v3, #297 item 3):
 * `id` and `name` on the call's first fragment only, `arguments` a piece of
 * the arguments text, `index` which of the response's calls it belongs to.
 */
export type ToolCallPiece = V0.ToolCallPiece;

/** A piece of a call's answer: exactly one of `text`, `reasoning` and, from v3, `tool_call`. */
export type Delta = V0.DeltaLine;
export type StopAsked = V0.StopAskedLine;

/** A call answered. */
export type Response = V0.ResponseLine & {
  /** AHEAD (R3): llama.cpp's timings for the call. */
  readonly timings?: Timings;
  /**
   * The surface's ask, refused in R3 on #117 -- a client-derived split, not
   * a number the server reports
   * (https://github.com/ironblock/discipline/issues/117#issuecomment-5883589448):
   * where its tool calls began in what it wrote -- tokens and ms generated
   * before the first tool-call chunk. Only recordings carry it; `predicted_n`
   * alone absent when only the time was kept (a harness's transcript that
   * recorded when a call part began).
   */
  readonly calls_from?: { readonly predicted_n?: number; readonly predicted_ms: number };
  /** AHEAD (R2/I5r): the whole reasoning, as the deltas streamed it. */
  readonly reasoning?: string;
};

export type Cancelled = V0.CancelledLine;
/** A call ended without an answer; its reason v0's, or AHEAD `context_overflow`. */
export type RequestFailed = Omit<V0.RequestFailedLine, 'reason'> & { readonly reason: FailReason };
/** A turn is over. */
export type TurnSettled = Omit<V0.TurnSettledLine, 'reason'> & { readonly reason: SettleReason };
/** A person's idle gap after a settled turn, as the surface measured it (Q4). */
export type IdleGap = V0.IdleGapLine;

/**
 * Where a request's prefill is, right now: the format's own `progress` line
 * (diet/formats/log, v1 D2), one per frame the server streams before the
 * call's first delta. Its keys are llama.cpp's: `total` prompt tokens, the
 * `cache` reused, `processed` so far COUNTING THE CACHE IN (a warm turn's
 * first frame reads `processed == cache`, its last `processed == total`;
 * measured on #288), and `time_ms` of prefill by the server's clock. It says
 * nothing of generation. The surface read an older, nested shape of its own
 * until #288, and went blank on the first live frame.
 */
export type ProgressFrame = V0.ProgressLine;

// ------------------------------------------------------------------ v3's tool call

/**
 * A call the model made, and what became of it: one line per call, written
 * when its outcome is known (v3, #297). The format's own line, generated;
 * which keys fit which outcome is its reader's rule.
 */
export type ToolCall = Omit<V0.ToolCallLine, 'reason' | 'approval'> & {
  readonly reason?: ToolRefusal;
  /** The decision it ran under (log v4, #388), its scope open as the surface's sets are. */
  readonly approval?: Approval;
};

// ------------------------------------------------------------------ AHEAD kinds

/** AHEAD (R4): a side call off the trunk's warm tail, on a slot of its own. */
export interface Fork extends At {
  readonly kind: 'fork';
  readonly lane: ForkLane;
  readonly slot: number;
  readonly of_turn: number;
  /** The `seq` of the trunk line it branches from: a `request` (its answer), or a tool call's first fragment. */
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
  | ToolCall
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
  tool_call: ['R2'],
  fork: ['R4'],
  'fork.settled': ['R4'],
  patch: ['R5'],
  seam: ['R6'],
};
