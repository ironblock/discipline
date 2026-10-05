/**
 * The authored shape of a session: what the specimen, the kitchen sink, the
 * recorded sessions and the canned transport are written in -- the surface's
 * provisional vocabulary from before `diet`'s log existed, kept as a script
 * language because it names things. Labels (`q/2`, `t/1`, `i/1`) stand where
 * the log has references by `seq`; `place.ts` turns a script into the log
 * (`log.ts`), and only the log is folded.
 *
 * What authors or replays a session reads it -- the canned transport, the
 * recordings, `compose.ts`, `progress.ts` -- and `place.ts` turns it into
 * the log. A session run against `diet` never passes through here.
 */

import type { Approval, Authority, FailReason, ForkLane, ForkOutcome, Lane, PatchOp, SeamReason, SettleReason, Timings, Tool, ToolRefusal } from './log.ts';
import type { Segment } from './transport.ts';

export type { Approval, Authority, FailReason, ForkLane, ForkOutcome, Lane, PatchOp, SeamReason, SettleReason, Timings, Tool, ToolRefusal } from './log.ts';
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
  /**
   * The log version it is placed as: 3 unless it says. A v4 log carries `cwd` with every `argv`, and a v3 log
   * refuses v4's keys (`cwd`, `approval`, `files`), so a session that uses them declares 4 and gives each bash
   * call its `cwd`.
   */
  readonly version?: 3 | 4;
  /** The trunk's first system prompt, as sent; its size in tokens when the record measured it. */
  readonly system: { readonly text: string; readonly tokens?: number };
}

/** A person's words: the ask, as a field rather than inside a wire body. */
export interface Ask extends At {
  readonly kind: 'ask';
  readonly turn: number;
  readonly text: string;
  /** The operator marked it the scope answer (#453): placed on its `ask` line, as serve logs it (log v5). */
  readonly scoping?: true;
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
 * Where a request's prefill is, right now: the log's `progress` line
 * (`log.ts`, the format's own), with its request named by label. `processed`
 * counts the cache in; no frame comes after the first delta, and none says
 * how much is generated.
 */
export interface ProgressFrame extends At {
  readonly kind: 'progress';
  readonly request: string;
  /** Prompt tokens in all. */
  readonly total: number;
  /** Prompt tokens reused from the slot's cache. */
  readonly cache: number;
  /** Prompt tokens processed so far, the cache among them: `cache` at the start, `total` once read. */
  readonly processed: number;
  /** Milliseconds of prefill so far, by the server's clock. */
  readonly time_ms: number;
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
  /** A bash call's working directory, where the script says one (log v4's `cwd`, #388): placed beside its `argv`. */
  readonly cwd?: string;
  /** The call waits on the operator before it runs (#389): the canned transport holds the rest of the beat on it. */
  readonly prompt?: ScriptedPrompt;
}

/**
 * What the gate said of a call it held for the operator -- `serve`'s waiting event, less what the call already
 * says (#389, ruled 5982826097) -- and the session's way on if the operator declines: the events after the
 * refusal, `t` relative to it. An approval plays the beat on as written.
 */
export interface ScriptedPrompt {
  readonly reason: string;
  readonly segments: readonly Segment[];
  readonly declined: readonly Unplaced[];
}

export interface ToolEnd extends At {
  readonly kind: 'tool.end';
  readonly id: string;
  readonly exit: number;
  readonly output: string;
  /** The harness cut the output before the model saw it. */
  readonly truncated?: boolean;
  /** The turn was cancelled before the call's outcome: its exit and output are not placed. */
  readonly cancelled?: true;
  /** The drive refused the call, and why: it did not run, and its exit and output are not placed. */
  readonly refused?: ToolRefusal;
  /** The decision it ran under (log v4, #388): a pre-seed a script declares, or the operator's, which the canned transport adds. */
  readonly approval?: Approval;
  /** The files its result is (#372): placed as references, their bytes served by digest. */
  readonly files?: readonly ScriptedFile[];
}

/** A file a call left: what the log references (`FileRef`), and the bytes a source answers its digest with. */
export interface ScriptedFile {
  readonly path: string;
  readonly media_type: string;
  readonly sha256: string;
  readonly bytes: Uint8Array;
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
