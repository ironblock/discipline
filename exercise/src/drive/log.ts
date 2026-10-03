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

/** The tools a model may call (AHEAD, R2's later bump). Only `bash` is drawn by name; any other as `name(args)`. */
export type Tool = Open<'bash'>;

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
/** A piece of a call's answer: exactly one of `text` and `reasoning`. */
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

// ------------------------------------------------------------------ AHEAD kinds

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
