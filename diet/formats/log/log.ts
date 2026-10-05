// GENERATED from diet/src/formats/log.rs -- do not edit by hand.
// Regenerate: cargo test -p discipline-diet --lib formats::log::tests::write_the_bindings -- --ignored
// A count here is an integer in the log; the reader refuses one past
// i64, and a JavaScript number is exact only to 2^53. A `timings`
// millisecond may carry a fraction, written as the server wrote it.

export const VERSION = 5;
export const READS = [0, 1, 2, 3, 4, 5] as const;
export const PRESENCE_WINDOW_MS = 2000;

export type Kind =
  | "session.start"
  | "ask"
  | "settlement"
  | "request"
  | "refused"
  | "delta"
  | "stop.asked"
  | "response"
  | "cancelled"
  | "request.failed"
  | "turn.settled"
  | "idle.gap"
  | "progress"
  | "tool_call"
  | "fork"
  | "fork.settled"
  | "patch"
;

export type State =
  | "awaiting"
  | "turn"
  | "capture"
  | "ended"
;

export type Lane =
  | "trunk"
  | "interview"
;

export type Command =
  | "ask"
  | "cancel"
  | "declare-seam"
  | "end"
;

export type Refusal =
  | "in-flight"
  | "ended"
  | "nothing-in-flight"
  | "seam-not-built"
  | "stale"
;

export type FailReason =
  | "server"
  | "timeout"
  | "transport"
  | "crashed"
  | "context_overflow"
;

export type SettleReason =
  | "final"
  | "cancelled"
  | "max_steps"
  | "timeout"
  | "failed"
  | "capped"
;

export type GapEnd =
  | "ask"
  | "seam"
  | "cancel"
  | "end"
;

export type Role =
  | "system"
  | "user"
  | "assistant"
;

export type ToolOutcome =
  | "ran"
  | "refused"
  | "command_failed"
  | "cancelled"
;

export type Isolation =
  | "none"
  | "sandbox"
  | "vm"
  | "unrecorded"
;

export type Network =
  | "none"
  | "host"
  | "unrecorded"
;

export type ToolRefusal =
  | "not_allowed"
  | "max_steps"
  | "unparsable"
  | "unknown_tool"
  | "denylist"
  | "declined"
;

export type EngineIdentity =
  | "checked_commit"
  | "literal_matched"
;

export type Provenance =
  | "placed"
  | "constructed"
;

export type ApprovalScope =
  | "once"
  | "session"
  | "workspace"
  | "preseeded"
;

export type Warrant =
  | "read"
  | "scoping"
;

export type ForkOutcome =
  | "value"
  | "decline"
  | "unparseable"
  | "truncated"
  | "failed"
  | "cancelled"
;

export type PatchOp =
  | "add"
  | "supersede"
  | "resolve"
  | "retire"
  | "park"
;

export interface HeadMessage {
  role: Role;
  content: string;
}

export interface Timings {
  prompt_n?: number;
  cache_n?: number;
  prompt_ms?: number;
  predicted_n?: number;
  predicted_ms?: number;
  draft_n?: number;
  draft_n_accepted?: number;
}

export interface Usage {
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens?: number;
}

export interface Serving {
  dialect: string;
  concurrency?: number;
}

export interface ToolCallPiece {
  index: number;
  id?: string;
  name?: string;
  arguments: string;
}

export interface Approval {
  scope: ApprovalScope;
  decided_at?: number;
  why?: string;
}

export interface RecordedFile {
  path: string;
  sha256: string;
  media_type: string;
  bytes: number;
}

export interface PatchEntry {
  id: string;
  text: string;
  category?: string;
}

export type SessionStartLine = {
  seq: number;
  t: number;
  kind: "session.start";
  version: 0 | 1 | 2 | 3 | 4 | 5;
  opened: number;
  model: string;
  head: HeadMessage[];
  serving?: Serving;
  provenance?: Provenance;
  tools?: string[];
} & ({ substrate: string; registry_sha256: string; engine_build: string; engine_identity: EngineIdentity } | { substrate?: never; registry_sha256?: never; engine_build?: never; engine_identity?: never });

export type AskLine = {
  seq: number;
  t: number;
  kind: "ask";
  turn: number;
  text: string;
  scoping?: boolean;
  files?: RecordedFile[];
};

export type SettlementLine = {
  seq: number;
  t: number;
  kind: "settlement";
  from: State;
  to: State;
};

export type RequestLine = {
  seq: number;
  t: number;
  kind: "request";
  turn: number;
  lane: Lane;
  head_sha256?: string;
  fork?: number;
};

export type RefusedLine = {
  seq: number;
  t: number;
  kind: "refused";
  command: Command;
  because: Refusal;
  during: State;
};

export type DeltaLine = {
  seq: number;
  t: number;
  kind: "delta";
  request: number;
} & ({ text: string; reasoning?: never; tool_call?: never } | { reasoning: string; text?: never; tool_call?: never } | { tool_call: ToolCallPiece; text?: never; reasoning?: never });

export type StopAskedLine = {
  seq: number;
  t: number;
  kind: "stop.asked";
  turn: number;
};

export type ResponseLine = {
  seq: number;
  t: number;
  kind: "response";
  to_request: number;
  text: string;
  finish_reason?: string;
  reasoning?: string;
  capped?: boolean;
} & ({ timings?: Timings; usage?: never } | { usage?: Usage; timings?: never });

export type CancelledLine = {
  seq: number;
  t: number;
  kind: "cancelled";
  request: number;
  partial: string;
  reasoning?: string;
};

export type RequestFailedLine = {
  seq: number;
  t: number;
  kind: "request.failed";
  request: number;
  reason: FailReason;
  message: string;
  status?: number;
  partial?: string;
};

export type TurnSettledLine = {
  seq: number;
  t: number;
  kind: "turn.settled";
  turn: number;
  reason: SettleReason;
};

export type IdleGapLine = {
  seq: number;
  t: number;
  kind: "idle.gap";
  opened_by: number;
  notice: number;
  read: number;
  compose: number;
  away: number;
  blocked: number;
  ended_by: GapEnd;
};

export type ProgressLine = {
  seq: number;
  t: number;
  kind: "progress";
  request: number;
  total: number;
  cache: number;
  processed: number;
  time_ms: number;
};

export type ToolCallLine = {
  seq: number;
  t: number;
  kind: "tool_call";
  request: number;
  turn: number;
  id: string;
  name: string;
  arguments: string;
  outcome: ToolOutcome;
  argv?: string[];
  cwd?: string;
  confined?: string[];
  isolation?: Isolation;
  network?: Network;
  exit?: number;
  reason?: ToolRefusal;
  policy?: string;
  stdout?: string;
  stdout_bytes?: number;
  stderr?: string;
  stderr_bytes?: number;
  approval?: Approval;
  files?: RecordedFile[];
  shown?: string;
};

export type ForkLine = {
  seq: number;
  t: number;
  kind: "fork";
  lane: Lane;
  of_turn: number;
  at: number;
  why: Warrant;
  question: string;
};

export type ForkSettledLine = {
  seq: number;
  t: number;
  kind: "fork.settled";
  fork: number;
  outcome: ForkOutcome;
};

export type PatchLine = {
  seq: number;
  t: number;
  kind: "patch";
  fork: number;
  op: PatchOp;
  entry: PatchEntry;
  supersedes?: string;
};

export type LogLine =
  | SessionStartLine
  | AskLine
  | SettlementLine
  | RequestLine
  | RefusedLine
  | DeltaLine
  | StopAskedLine
  | ResponseLine
  | CancelledLine
  | RequestFailedLine
  | TurnSettledLine
  | IdleGapLine
  | ProgressLine
  | ToolCallLine
  | ForkLine
  | ForkSettledLine
  | PatchLine
;
