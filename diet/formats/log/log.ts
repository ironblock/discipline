// GENERATED from diet/src/formats/log.rs -- do not edit by hand.
// Regenerate: cargo test -p discipline-diet --lib formats::log::tests::write_the_bindings -- --ignored
// A number here is an integer in the log; the reader refuses one past
// i64, and a JavaScript number is exact only to 2^53.

export const VERSION = 0;

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

export type SessionStartLine = {
  seq: number;
  t: number;
  kind: "session.start";
  version: 0;
  opened: number;
  model: string;
  head: HeadMessage[];
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
};

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
;
