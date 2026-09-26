/**
 * What the surface needs the drive to say, as events -- PROVISIONAL, and the
 * surface's half of #117.
 *
 * `diet` has no interactive loop yet, so nothing emits these. This file is the
 * request: every kind below is tagged with the step of #117 that would make
 * `diet` emit it (`NEEDS`), and the surface draws nothing it cannot fold from
 * these. The log is a format, `diet/formats/log` (ruled on #117, 2026-09-26):
 * when it lands, its generated types replace this file and every difference
 * is a compile error in `src/session/fold.ts`.
 *
 * Names follow the record where the record has the concept (`request`,
 * `response`, `fork`, `seam`, `to_request`, `of_turn`, `at_turn`) and the
 * rulings on #31 where it does not: the canonical session is the `trunk`,
 * a `slot` is a server slot and nothing else, the seam-time audit is `ratify`.
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

/** The side lanes a fork runs in. `extraction` is the predecessor's mechanical read of the trunk. */
export type ForkLane = Open<'interview' | 'ratify' | 'extraction'>;

export type Lane = 'trunk' | ForkLane;

/** llama.cpp's own per-request `timings`, the fields the surface reads. */
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

interface At {
  /** Position in the session's log, from 0. Assigned by the transport. */
  readonly seq: number;
  /** Milliseconds since `session.start`. */
  readonly t: number;
}

export interface SessionStart extends At {
  readonly kind: 'session.start';
  readonly arm: string;
  readonly model: string;
  /** The server's `-np`: how many requests it serves at once. */
  readonly slots: number;
  /** The slot the trunk is pinned to. */
  readonly trunk_slot: number;
  readonly phase: string;
  /** The trunk's first system prompt, as sent; its size in tokens when the record measured it. */
  readonly system: { readonly text: string; readonly tokens?: number };
}

/** A person's words: the ask, as a field rather than inside a wire body. */
export interface Ask extends At {
  readonly kind: 'ask';
  readonly turn: number;
  readonly text: string;
}

export interface Request extends At {
  readonly kind: 'request';
  readonly id: string;
  readonly lane: Lane;
  readonly slot: number;
  readonly turn: number;
  /** The fork this request belongs to, for a non-trunk lane. */
  readonly fork?: string;
}

/** Streamed text for a request still generating. Transient: never recorded. */
export interface Delta extends At {
  readonly kind: 'delta';
  readonly request: string;
  readonly reasoning?: string;
  readonly text?: string;
}

export type Stop = Open<'stop' | 'tool' | 'length' | 'cancelled'>;

export interface Response extends At {
  readonly kind: 'response';
  readonly id: string;
  readonly to_request: string;
  readonly reasoning?: string;
  readonly text: string;
  readonly stop: Stop;
  readonly timings: Timings;
}

/** The tools a model may call. Only `bash` exists today; every other tool is drawn as `name(args)`. */
export type Tool = Open<'bash'>;

/**
 * Why a request produced no response: the server refused or failed it, the
 * prompt no longer fit, the drive gave up waiting. Open.
 */
export type FailReason = Open<'server' | 'context_overflow' | 'timeout' | 'disconnected'>;

/** A request that will never have a response. Its slot is free again. */
export interface RequestFailed extends At {
  readonly kind: 'request.failed';
  readonly request: string;
  readonly reason: FailReason;
  /** What the server or the drive said, verbatim. */
  readonly message: string;
}

export interface ToolBegin extends At {
  readonly kind: 'tool.begin';
  readonly id: string;
  readonly turn: number;
  /** The response whose tool call this is. */
  readonly after: string;
  readonly tool: Tool;
  /** The call's arguments as the model gave them; `bash` takes `{ command }`. */
  readonly args: Readonly<Record<string, unknown>>;
}

export interface ToolEnd extends At {
  readonly kind: 'tool.end';
  readonly id: string;
  readonly exit: number;
  readonly output: string;
  /** The harness cut the output before the model saw it. */
  readonly truncated?: boolean;
}

export type SettleReason = Open<'final' | 'cancelled' | 'max_steps' | 'timeout' | 'failed'>;

export interface TurnSettled extends At {
  readonly kind: 'turn.settled';
  readonly turn: number;
  readonly reason: SettleReason;
}

/** A side call off the trunk's warm tail, on a slot of its own. */
export interface Fork extends At {
  readonly kind: 'fork';
  readonly id: string;
  readonly lane: ForkLane;
  readonly slot: number;
  readonly of_turn: number;
  /** The trunk node it branches from: a response id or a tool call id. */
  readonly at: string;
  /** What `diet` noticed that made it ask. */
  readonly why: string;
  readonly question: string;
  /** Prefix tokens shared with the trunk: the warm tail it forked from. */
  readonly prefix_tokens: number;
}

/**
 * How a fork ended: the one enum, ruled on #117 (2026-09-26, naming 6), which
 * lives on the fork event in `formats/record`. `value` is an answer the
 * capture could use; the drive's own outcomes fold into these.
 */
export type ForkOutcome = Open<
  | 'value'
  | 'decline'
  | 'mimicry'
  | 'unparseable'
  | 'thinking_exhausted'
  | 'rejected'
  | 'timeout'
  | 'truncated'
  | 'output_too_large'
>;

export interface ForkSettled extends At {
  readonly kind: 'fork.settled';
  readonly id: string;
  readonly outcome: ForkOutcome;
}

export interface Entry {
  readonly id: string;
  /** Absent in the predecessor's record, which kept one flat list. */
  readonly category?: string;
  readonly text: string;
}

/**
 * A change to working memory: `diet`'s ops, ruled on #117 (naming 4). `add`,
 * `supersede` and `retire` change an entry's state; `resolve` and `park`
 * rewrite it and are shown by name. `edit` is reserved for a person editing
 * working memory, with authority `stated`. `void` is a state, never an op.
 */
export type PatchOp = Open<'add' | 'supersede' | 'resolve' | 'retire' | 'park' | 'edit'>;

/**
 * How an entry was known -- `authority`, ruled on #117 (naming 5), where
 * `provenance` is its position (turn, lane, fork). Authority follows it: an
 * entry the arm declared is never rewritten.
 */
export type Authority = Open<'stated' | 'extracted' | 'observed' | 'arm'>;

/** A change to working memory, from the fork that produced it. */
export interface Patch extends At {
  readonly kind: 'patch';
  readonly id: string;
  readonly from: string;
  readonly op: PatchOp;
  readonly entry: Entry;
  /** For `supersede`: the entry this one replaces. */
  readonly supersedes?: string;
  /** How the entry was known. */
  readonly authority?: Authority;
}

export type SeamReason = Open<'operator' | 'phase' | 'cadence' | 'budget'>;

/** The one deliberate prefill event: the trunk rebuilt from working memory. */
export interface Seam extends At {
  readonly kind: 'seam';
  readonly id: string;
  readonly at_turn: number;
  readonly reason: SeamReason;
  /** Absent when the record did not say (the predecessor's seams). */
  readonly phase?: { readonly from: string; readonly to: string };
  readonly prefix_hash_before: string;
  readonly prefix_hash_after: string;
  /** The new system prompt: working memory rendered, with the phase's priming. */
  readonly render: { readonly version: number; readonly text: string; readonly tokens?: number };
  /** The pre-warm: the new prefix sent once so the next ask finds it cached. Absent: none was recorded. */
  readonly warm?: Timings;
}

export interface SessionEnd extends At {
  readonly kind: 'session.end';
}

export type DriveEvent =
  | SessionStart
  | Ask
  | Request
  | Delta
  | Response
  | RequestFailed
  | ToolBegin
  | ToolEnd
  | TurnSettled
  | Fork
  | ForkSettled
  | Patch
  | Seam
  | SessionEnd;

export type Kind = DriveEvent['kind'];

/** The event of one kind. */
export type EventOf<K extends Kind> = Extract<DriveEvent, { readonly kind: K }>;

/** An event before the transport has placed it in the log. */
export type Unplaced<E extends DriveEvent = DriveEvent> = E extends DriveEvent ? Omit<E, 'seq'> : never;

/** Which step of #117 each kind waits on. Nothing in `diet` emits any of them yet. */
export const NEEDS_OF: { readonly [K in Kind]: readonly Need[] } = {
  'session.start': ['R2', 'R4'],
  ask: ['R2'],
  request: ['R2', 'R4'],
  delta: ['R3'],
  response: ['R2', 'R3'],
  'request.failed': ['R2'],
  'tool.begin': ['R2'],
  'tool.end': ['R2'],
  'turn.settled': ['R2'],
  fork: ['R4'],
  'fork.settled': ['R4'],
  patch: ['R5'],
  seam: ['R6'],
  'session.end': ['R2'],
};
