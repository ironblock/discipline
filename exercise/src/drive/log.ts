/**
 * The session's log, as the surface reads it: `diet/formats/log`, whose
 * types are GENERATED from `diet/src/formats/log.rs` (#144) and imported
 * here -- and, on top of them, what the surface draws that `diet` does not
 * write yet, each marked AHEAD and named in `NEEDS` by what it is. The hand
 * mirror this file was is gone (ruled on #117, 2026-09-28): a line's shape
 * is `diet`'s, checked in its CI.
 *
 * References are the `seq` of the line they name, as v0 has them: an
 * identifier the log issues, never one anybody invents. A session run
 * against `diet` today carries nothing AHEAD -- the surface draws what it can
 * without it.
 */

import type * as V0 from '../../../diet/formats/log/log.ts';

/**
 * What the surface draws that `diet` does not write yet, by what it is: the "what diet can't emit yet" overlay
 * outlines a line carrying any of it (`needsOf`). Every kind is `diet`'s now (#503); what is left is fields. One source.
 */
export const NEEDS = {
  record: 'the arm a session runs, and its system prompt’s size in tokens, on its start',
  slots: 'the server’s slots, and the one that served each call',
  phases: 'phases: the one a session opens in, and the move a seam makes',
  lanes: 'the ratify and extraction side lanes',
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
/** Why a call ended without an answer: `diet`'s (`context_overflow` among them, #192), open to a newer drive's. */
export type FailReason = Open<V0.FailReason>;

/** The side lanes a fork runs in: `diet`'s `interview`, and AHEAD `ratify` and `extraction` (the predecessor's mechanical read of the trunk). */
export type ForkLane = Open<'interview' | 'ratify' | 'extraction'>;
/** Which lane a request was made on: `diet`'s, or an AHEAD side lane. */
export type Lane = V0.Lane | ForkLane;

/** llama.cpp's per-request `timings`, the fields the surface reads: `diet`'s `response` carries them (#192, #198). */
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

/** How a fork ended: the one enum, ruled on #117 (2026-09-26, naming 6) for the record. */
export type ForkOutcome = Open<'value' | 'decline' | 'mimicry' | 'unparseable' | 'thinking_exhausted' | 'rejected' | 'timeout' | 'truncated' | 'output_too_large'>;

/** A change to working memory: `diet`'s ops, ruled on #117 (naming 4). */
export type PatchOp = Open<'add' | 'supersede' | 'resolve' | 'retire' | 'park' | 'edit'>;

/** How an entry was known -- `authority`, ruled on #117 (naming 5). */
export type Authority = Open<'stated' | 'extracted' | 'observed' | 'arm'>;

/** Why a seam fired: the format's words (log v6, #493), open. A served session writes only `operator`. */
export type SeamReason = Open<V0.SeamReason>;

// ------------------------------------------------------------------ `diet`'s lines, with what is AHEAD on them

interface At {
  /** Its position in the log: gapless, from 0. The primary key, and what references name. */
  readonly seq: number;
  /** Milliseconds since the session opened. */
  readonly t: number;
}

/** The session opened. The first line, and only the first. */
export type SessionStart = V0.SessionStartLine & {
  /** AHEAD (`record`): the arm this session runs. */
  readonly arm?: string;
  /** AHEAD (`slots`): the server's `-np`, how many requests it serves at once. */
  readonly slots?: number;
  /** AHEAD (`slots`): the slot the trunk is pinned to. */
  readonly trunk_slot?: number;
  /** AHEAD (`phases`): the phase the session opens in. `diet` writes it as `opening_phase` (v7, #563). */
  readonly phase?: string;
  /** AHEAD (`record`): the system prompt's size in tokens. */
  readonly system_tokens?: number;
};

export type Ask = V0.AskLine;
export type Settlement = V0.SettlementLine;

/** A call was made to the model. Its `seq` is its identity: what its deltas, answer or failure name. */
export type Request = Omit<V0.RequestLine, 'lane'> & {
  /** `trunk` or `diet`'s `interview`, or an AHEAD side lane (`lanes`). */
  readonly lane: Lane;
  /** AHEAD (`slots`): the server slot it went to. */
  readonly slot?: number;
  /** The `seq` of the `fork` this call serves, for a side lane. */
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
  /** llama.cpp's timings for the call. */
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
  /** The whole reasoning, as the deltas streamed it. */
  readonly reasoning?: string;
};

export type Cancelled = V0.CancelledLine;
/** A call ended without an answer, and why. */
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

/**
 * A file a tool call's result is (log v4, #372 5983588781): where the call
 * left it, its sha256, its media type and its size. The page never follows
 * `path`: it asks for the bytes by digest and checks them (`files.ts`).
 */
export type FileRef = V0.RecordedFile;

// ------------------------------------------------------------------ forks, patches, the seam

/** A side call off the trunk's warm tail (`diet`'s from log v5), with what is AHEAD on it. */
export interface Fork extends At {
  readonly kind: 'fork';
  readonly lane: ForkLane;
  /** AHEAD (`slots`): `diet`'s fork line names no slot; the surface puts such a fork beside the trunk. */
  readonly slot?: number;
  readonly of_turn: number;
  /** The `seq` of the trunk line it branches from: a `request` (its answer), or a tool call's first fragment. */
  readonly at: number;
  /** What `diet` noticed that made it ask. */
  readonly why: string;
  readonly question: string;
  /** AHEAD (`slots`): prefix tokens shared with the trunk, the warm tail it forked from. `diet`'s fork line does not say. */
  readonly prefix_tokens?: number;
  /** An offboard seat's registry id, when the fork ran off the warm trunk (log v7, #615); with the model it served. */
  readonly substrate?: string;
  readonly model?: string;
}

/** How a fork ended. */
export interface ForkSettled extends At {
  readonly kind: 'fork.settled';
  /** The `seq` of the `fork`. */
  readonly fork: number;
  readonly outcome: ForkOutcome;
  /** An offboard fork's prompt as it read it cold (#615): its prefill, in tokens. */
  readonly prompt_tokens?: number;
  /** An offboard fork's wall time, from its request to its settle (#615). */
  readonly wall_ms?: number;
}

export interface Entry {
  /** Working memory's own id for the entry. */
  readonly id: string;
  readonly category?: string;
  readonly text: string;
}

/** A change to working memory, from the fork that produced it. */
export interface Patch extends At {
  readonly kind: 'patch';
  /** The `seq` of the `fork` that produced it; absent for the trunk's own change, which names its `lane` (#627). */
  readonly fork?: number;
  /** The trunk's lane that made it, when no fork did (log v7, #627): today always `self-capture`. */
  readonly lane?: string;
  readonly op: PatchOp;
  readonly entry: Entry;
  /** For `supersede`: the entry this one replaces. */
  readonly supersedes?: string;
  readonly authority?: Authority;
  /** The tangent open when it was made (log v7, #608): the entry is that tangent's, to be ruled on at its close. */
  readonly tangent?: string;
}

/** The operator opened a tangent (log v7, #608): the trunk as it stood is the point a close rolls back to. */
export type TangentOpen = V0.TangentOpenLine;
/** The operator closed it: its entries kept, dropped or parked, and the trunk rolled back to where it opened. */
export type TangentClose = V0.TangentCloseLine;

/**
 * The one deliberate prefill event -- the trunk refilled from working memory (log v6, #493): the head, the render
 * after it, and no turn of the old trunk. The format's line, with what is AHEAD on it. A seam placed in the surface's
 * own older shape (`render_version`, drawn before v6; its re-recording is #427) carries none of `frame`,
 * `carried_entries` and `carried_turns`.
 */
export type Seam = Omit<V0.SeamLine, 'reason' | 'frame' | 'carried_entries' | 'carried_turns'> &
  Partial<Pick<V0.SeamLine, 'frame' | 'carried_entries' | 'carried_turns'>> & {
    readonly reason: SeamReason;
    /** The phases it moved between (v7, #563). */
    readonly phase?: { readonly from: string; readonly to: string };
    /** The pre-warm (log v7, #504): the new prefix sent once so the next ask finds it cached. */
    readonly warm?: Timings;
    /** The surface's own older shape: the render's number, in a recording placed before v6. */
    readonly render_version?: number;
  };

/** Forks' patches delivered after an ask (v7, the fork delivery lever). */
export type Delivered = V0.DeliveredLine;
/** Self-capture's reminder, a note after an ask (v7, #619): the harness's words, not the operator's. */
export type Reminded = V0.RemindedLine;
/** What a self-capture call did (v7, #619), logged beside its `tool_call` line: its outcome and the entries it wrote. */
export type Capture = V0.CaptureLine;
/** Archived items recalled after an ask (v7, the archive recall lever, #566). */
export type Recalled = V0.RecalledLine;
/** A background command's end (v7, #614). Folded, not yet drawn. */
export type BackgroundEnded = V0.BackgroundEndedLine;
/** Ended background commands' notifications delivered after an ask (v7, #614). */
export type Notice = V0.NoticeLine;
/** A running call near its timeout (v7, #613): for the surface's warning, never the model's. */
export type TimeoutNear = V0.TimeoutNearLine;
/** A tool result the model pruned, replaced at a later seam (v7, `prune_output`, #612). */
export type Pruned = V0.PrunedLine;

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
  | Seam
  | Delivered
  | Recalled
  | Reminded
  | Capture
  | TangentOpen
  | TangentClose
  | Pruned
  | BackgroundEnded
  | Notice
  | TimeoutNear;

export type Kind = LogLine['kind'];

/** The line of one kind. */
export type LineOf<K extends Kind> = Extract<LogLine, { readonly kind: K }>;

/** The side lanes `diet` does not run yet. */
const AHEAD_LANES: ReadonlySet<string> = new Set(['ratify', 'extraction']);

/**
 * What LINE carries that `diet` does not write yet: what the gaps overlay outlines. A line as `diet` writes it
 * carries none (`log.test.ts` holds every line of `diet`'s own valid logs to that).
 */
export function needsOf(line: LogLine): Need[] {
  const out: Need[] = [];
  const has = (need: Need, ...fields: readonly unknown[]) => fields.some((f) => f !== undefined) && out.push(need);
  switch (line.kind) {
    case 'session.start':
      has('record', line.arm, line.system_tokens);
      has('slots', line.slots, line.trunk_slot);
      has('phases', line.phase);
      break;
    case 'request':
      has('slots', line.slot);
      if (AHEAD_LANES.has(line.lane)) out.push('lanes');
      break;
    case 'fork':
      has('slots', line.slot, line.prefix_tokens);
      if (AHEAD_LANES.has(line.lane)) out.push('lanes');
      break;
    case 'seam':
      // `phase` is the format's own since v7 (#563); `render_tokens`, the render's estimated size, since v7's render
      // budget (#565), written beside the budget that produced it; `warm`, the pre-warm's timings, since #504.
      break;
  }
  return out;
}
