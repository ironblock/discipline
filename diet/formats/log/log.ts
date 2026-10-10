// GENERATED from diet/src/formats/log.rs -- do not edit by hand.
// Regenerate: cargo test -p discipline-diet --lib formats::log::tests::write_the_bindings -- --ignored
// A count here is an integer in the log; the reader refuses one past
// i64, and a JavaScript number is exact only to 2^53. A `timings`
// millisecond may carry a fraction, written as the server wrote it.

export const VERSION = 7;
export const READS = [0, 1, 2, 3, 4, 5, 6, 7] as const;
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
  | "seam"
  | "delivered"
  | "recalled"
  | "pruned"
  | "phase.ruled"
  | "tangent.open"
  | "tangent.close"
  | "capture"
  | "reminded"
  | "background.ended"
  | "notice"
  | "timeout.near"
  | "fork.skipped"
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
  | "audit"
;

export type Command =
  | "ask"
  | "cancel"
  | "declare-seam"
  | "end"
  | "open-tangent"
  | "close-tangent"
  | "background"
  | "ratify-phase"
;

export type Refusal =
  | "in-flight"
  | "ended"
  | "nothing-in-flight"
  | "seam-not-built"
  | "nothing-to-seam"
  | "no-phase-graph"
  | "not-a-phase"
  | "already-in-phase"
  | "no-phase-edge"
  | "stale"
  | "tangent-open"
  | "no-tangent"
  | "bad-tangent"
  | "not-the-scope"
  | "nothing-running"
  | "no-proposal"
;

export type FailReason =
  | "server"
  | "timeout"
  | "transport"
  | "crashed"
  | "context_overflow"
  | "refusal"
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

export type FieldProvenance =
  | "declared"
  | "corroborated"
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
  | "off"
;

export type Warrant =
  | "read"
  | "scoping"
  | "seam"
;

export type ForkOutcome =
  | "value"
  | "decline"
  | "unparseable"
  | "truncated"
  | "failed"
  | "cancelled"
  | "refused"
;

export type PatchOp =
  | "add"
  | "supersede"
  | "resolve"
  | "retire"
  | "park"
;

export type SeamReason =
  | "operator"
  | "phase"
  | "budget"
  | "cadence"
  | "window"
  | "prune"
;

export type Framing =
  | "advisory"
  | "imperative"
;

export type RecallState =
  | "literal"
;

export type ForkDelivery =
  | "seam"
  | "advisory"
  | "imperative"
;

export type ToolOutputState =
  | "capped"
  | "keep"
;

export type SeamToolOutputs =
  | "evict"
  | "reference"
  | "salient"
  | "keep"
;

export type RenderPlacement =
  | "system"
  | "message"
;

export type BackgroundStatus =
  | "completed"
  | "failed"
  | "cancelled"
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
  cache_creation_tokens?: number;
  cache_creation_5m_tokens?: number;
  cache_creation_1h_tokens?: number;
}

export interface Serving {
  dialect: string;
  concurrency?: number;
}

export interface TemplateKwargs {
  enable_thinking?: boolean;
  reasoning_effort?: string;
  preserve_thinking?: boolean;
}

export interface Unsent {
  budget_tokens: number;
}

export interface PhaseMove {
  from: string;
  to: string;
}

export interface InstructionFile {
  path: string;
  sha256: string;
}

export interface NoteLine {
  entry: string;
  op: PatchOp;
  template: string;
}

export interface RecalledItem {
  key: string;
  sha256: string;
  score: number;
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

export interface ServedField {
  field: string;
  value: string;
  provenance: FieldProvenance;
  reported?: string;
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
  version: 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7;
  opened: number;
  model: string;
  head: HeadMessage[];
  serving?: Serving;
  engine_build?: string;
  engine_identity?: EngineIdentity;
  served?: ServedField[];
  provenance?: Provenance;
  tools?: string[];
  template_kwargs?: TemplateKwargs;
  unsent?: Unsent;
  approvals_off?: boolean;
  fork_delivery?: ForkDelivery;
  levers?: Record<string, string>;
  fork_asks?: string;
  fork_asks_digest?: string;
  reasoning_effort_default?: string;
  phases?: string[];
  phase_transitions?: PhaseMove[];
  opening_phase?: string;
  instruction_files?: InstructionFile[];
  tool_output?: ToolOutputState;
  tool_output_max_lines?: number;
  tool_output_max_bytes?: number;
  bash_timeout_ms?: number;
} & ({ substrate: string; registry_sha256: string } | { substrate?: never; registry_sha256?: never });

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
  max_tokens?: number;
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
  timings?: Timings;
  usage?: Usage;
  capped?: boolean;
  reasoning_signature?: string;
  redacted?: string[];
};

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
} & ({ prompt_tokens: number; window: number; inferred: boolean } | { prompt_tokens?: never; window?: never; inferred?: never });

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
  recovered_from?: string;
  background?: string;
  timeout_ms?: number;
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
  view?: string;
  trigger?: string;
  role?: string;
  ask?: string;
  hazard?: string;
} & ({ substrate: string; model: string } | { substrate?: never; model?: never });

export type ForkSettledLine = {
  seq: number;
  t: number;
  kind: "fork.settled";
  fork: number;
  outcome: ForkOutcome;
  prompt_tokens?: number;
  wall_ms?: number;
  refused?: string;
};

export type PatchLine = {
  seq: number;
  t: number;
  kind: "patch";
  op: PatchOp;
  entry: PatchEntry;
  supersedes?: string;
  tangent?: string;
} & ({ fork: number; lane?: never } | { lane: string; fork?: never });

export type SeamLine = {
  seq: number;
  t: number;
  kind: "seam";
  at_turn: number;
  reason: SeamReason;
  prefix_hash_before: string;
  prefix_hash_after: string;
  frame: string;
  render: string;
  carried_entries: number;
  carried_turns: number;
  tail_tokens?: number;
  carried_tokens?: number;
  phase?: PhaseMove;
  tool_outputs?: SeamToolOutputs;
  outputs?: string;
  carried_outputs?: number;
  carried_output_bytes?: number;
  placement?: RenderPlacement;
  render_budget_tokens?: number;
  render_over_budget?: string;
  render_tokens?: number;
  render_reduced?: number;
  prompt_tokens?: number;
  window?: number;
  pruned?: string[];
  warm?: Timings;
};

export type DeliveredLine = {
  seq: number;
  t: number;
  kind: "delivered";
  turn: number;
  framing: Framing;
  text: string;
  lines: NoteLine[];
};

export type RecalledLine = {
  seq: number;
  t: number;
  kind: "recalled";
  turn: number;
  recall: RecallState;
  text: string;
  items: RecalledItem[];
};

export type PrunedLine = {
  seq: number;
  t: number;
  kind: "pruned";
  turn: number;
  call: string;
  sha256: string;
  bytes: number;
  text: string;
};

export type PhaseRuledLine = {
  seq: number;
  t: number;
  kind: "phase.ruled";
  call: string;
  choice: string;
  from?: string;
  to: string;
};

export type TangentOpenLine = {
  seq: number;
  t: number;
  kind: "tangent.open";
  id: string;
  at_turn: number;
  trunk_messages: number;
};

export type TangentCloseLine = {
  seq: number;
  t: number;
  kind: "tangent.close";
  id: string;
  at_turn: number;
  kept: string[];
  dropped: string[];
  parked: string[];
  prefix_intact: boolean;
  rolled_back: number;
};

export type CaptureLine = {
  seq: number;
  t: number;
  kind: "capture";
  request: number;
  call: string;
  tool: string;
  outcome: string;
  entries: string[];
  why?: string;
  fork?: number;
  from?: string;
  to?: string;
};

export type RemindedLine = {
  seq: number;
  t: number;
  kind: "reminded";
  turn: number;
  text: string;
};

export type BackgroundEndedLine = {
  seq: number;
  t: number;
  kind: "background.ended";
  job: string;
  status: BackgroundStatus;
  exit?: number;
  files?: RecordedFile[];
};

export type NoticeLine = {
  seq: number;
  t: number;
  kind: "notice";
  turn: number;
  text: string;
};

export type TimeoutNearLine = {
  seq: number;
  t: number;
  kind: "timeout.near";
  request: number;
  call: string;
  timeout_ms: number;
};

export type ForkSkippedLine = {
  seq: number;
  t: number;
  kind: "fork.skipped";
  of_turn: number;
  trigger: string;
  ask: string;
  call: string;
  field: string;
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
  | SeamLine
  | DeliveredLine
  | RecalledLine
  | PrunedLine
  | PhaseRuledLine
  | TangentOpenLine
  | TangentCloseLine
  | CaptureLine
  | RemindedLine
  | BackgroundEndedLine
  | NoticeLine
  | TimeoutNearLine
  | ForkSkippedLine
;
