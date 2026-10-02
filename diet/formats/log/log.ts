// GENERATED from diet/src/formats/log.rs -- do not edit by hand.
// Regenerate: cargo test -p discipline-diet --lib formats::log::tests::write_the_bindings -- --ignored
// A count here is an integer in the log; the reader refuses one past
// i64, and a JavaScript number is exact only to 2^53. A `timings`
// millisecond may carry a fraction, written as the server wrote it.

export const VERSION = 2;
export const READS = [0, 1, 2] as const;
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
;

export type State =
  | "awaiting"
  | "turn"
  | "capture"
  | "ended"
;

export type Lane =
  | "trunk"
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

export type SessionStartLine = {
  seq: number;
  t: number;
  kind: "session.start";
  version: 0 | 1 | 2;
  opened: number;
  model: string;
  head: HeadMessage[];
  serving?: Serving;
};

export type AskLine = {
  seq: number;
  t: number;
  kind: "ask";
  turn: number;
  text: string;
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
} & ({ text: string; reasoning?: never } | { reasoning: string; text?: never });

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
;
