/**
 * The authored shape of a session: what the specimen, the kitchen sink, the
 * recorded sessions and the canned transport are written in -- the surface's
 * provisional vocabulary from before `diet`'s log existed, kept as a script
 * language because it names things. Labels (`q/2`, `t/1`, `i/1`) stand where
 * the log has references by `seq`; `place.ts` turns a script into the log
 * (`log.ts`), and only the log is folded.
 *
 * Nothing reads a script but `place.ts`. A session run against `diet` never
 * passes through here.
 */

import type { Authority, FailReason, ForkLane, ForkOutcome, Lane, PatchOp, SeamReason, SettleReason, Timings, Tool } from './log.ts';

export type { Authority, FailReason, ForkLane, ForkOutcome, Lane, PatchOp, SeamReason, SettleReason, Timings, Tool } from './log.ts';
export { NEEDS, NEEDS_OF } from './log.ts';
export type { Need, Open } from './log.ts';
import type { Open } from './log.ts';

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

/**
 * Where a request is, right now: how much of its prompt is read, and how
 * many tokens it has generated. Transient, like `delta`: never recorded. From
 * llama.cpp's stream if it carries prompt progress, else its `/slots`, polled
 * once a second while busy (#117 R3, ruled 2026-09-26).
 */
export interface ProgressFrame extends At {
  readonly kind: 'progress';
  readonly request: string;
  /** The prompt: all of it, the part reused from the slot's cache, and how many of the rest -- the new tokens -- have been read. */
  readonly prompt: { readonly total: number; readonly cache: number; readonly processed: number };
  /** Tokens generated so far. */
  readonly decoded: number;
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
  /**
   * Where its tool calls began in what it wrote: tokens generated, and ms
   * spent generating, before the first call's first token -- llama.cpp's
   * per-token timings at the first tool-call chunk. The rest of `timings`'
   * generation is the calls'. Absent when the drive cannot tell: a protocol
   * that parses a call out of text only once it closes, or a record that
   * kept none; `predicted_n` alone absent when only the time was kept (a
   * harness's transcript: OpenCode records when a call part began, not how
   * many tokens came before it). The surface's ask (R3); the name is provisional.
   */
  readonly calls_from?: { readonly predicted_n?: number; readonly predicted_ms: number };
}

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
  | ProgressFrame
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
