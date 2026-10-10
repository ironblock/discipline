//! The session event log, v4 (#388), which reads v3, v2, v1 and v0.
//!
//! `diet/formats/log/grammar.pest` says what a log document is: one event
//! per line, in the record's value space. This module is its one reader and
//! its one writer. What each `kind` carries, and the rules that span lines,
//! are checked here under a name, as the record checks its own.
//!
//! Every line carries `seq` (its position, the primary key), `t`
//! (milliseconds since the session opened) and `kind`. The vocabulary is
//! CLOSED: an unknown kind is not a line of any version, and a later kind is
//! a versioned bump, because a line nobody can conformance-test is a line two
//! readers can disagree about.
//!
//! # Versions
//!
//! v1 (#117 R3, `diet/drive/plans/r3-proposal.md` D7) adds a `response`'s
//! `timings` and `reasoning`, the `progress` kind, and the `request.failed`
//! reason `context_overflow`. The writer writes [`VERSION`]; the reader
//! reads every version in [`READS`]. What arrived in v1 is scoped by the
//! version `session.start` declares: a whole log that declares 0 and carries
//! a v1 kind, key or tag is refused ([`parse`]). [`line`] reads a line alone
//! -- a reader resuming mid-log never sees `session.start` -- so it reads the
//! union, and the scoping is the whole-log reader's.
//!
//! v2 (#157 and #30's I0, applied from the #117 courier) adds a `response`'s
//! `usage` (for a server that reports no `timings`; never both,
//! [`at_most_one`]) and `capped`, a `session.start`'s `serving`, and a
//! `request`'s `head_sha256`. A log that declares 0 or 1 and carries one is
//! refused the same way.
//!
//! v3 (#297, from the #117 courier) adds the `tool_call` kind, one line per
//! call the model made, written at its outcome, whose keys must fit that
//! outcome ([`outcome_keys`]) and its name (only `bash` carries the [`EXEC`]
//! keys, `policy` among them, and only `bash` must carry its streams; a
//! `policy` only under an `isolation` that names a profile, `sandbox` or
//! `vm`), whose
//! streams' byte counts are `0` exactly when their text is empty and are not
//! otherwise compared with it (output that is not UTF-8 is written as lossy
//! text), whose `isolation` and
//! `network` each admit `unrecorded` on its own, only where no substrate
//! claim was made (a placed replay), and which names its request's turn and is
//! written at most once per call; a `delta`'s `id` and `name` only on a
//! call's first fragment, and a line for every streamed call, are the
//! writer's obligations, not read here; a `session.start`'s substrate claim, its four
//! keys together or not at all ([`all_or_none`]); a `session.start`'s
//! `provenance`, `placed` or `constructed`, absent when a session wrote the
//! log as it ran; a `delta`'s third piece,
//! `tool_call`, a fragment of a streamed call; `turn.settled`'s reason
//! `capped`, which requires the turn's trunk `response` to carry `capped:
//! true`; and a `cancelled`'s `reasoning`. A log that declares 0, 1 or 2 and
//! carries one is refused the same way. `lane` on a `tool_call` is reserved
//! until #80's Q1 and refused as any undeclared key is.
//!
//! v4 (#388, ruled on #298 point 10 and at 5981578575, 5981588394 and
//! 5982826236) adds a `tool_call`'s `approval`, the decision it ran under: an
//! object of its `scope` ([`ApprovalScope`]: `once`, `session`, `workspace`
//! or `preseeded`), `decided_at`, a count of milliseconds on the line's own
//! clock, never greater than the line's `t`, and `why`, the prompting
//! segment's reason, one word of an open set whose first words are the gate
//! module's `unparsable`, `dynamic` and `not_approved` (#402; #298
//! 5983544366). `decided_at` and `why` are
//! required of `once`, `session` and `workspace`, where the call prompted and
//! an operator decided it, and refused on `preseeded`, which no prompt decided
//! ([`decided_as_its_scope_says`]). `approval` is optional under `ran`, `command_failed` and
//! `cancelled`, on any name -- a free read or an unconfined scripted call
//! carries none -- and refused under `refused` ([`outcome_keys`]). v4 also
//! adds the [`ToolRefusal`]s `denylist` and `declined`; each is refused after
//! the call parsed, so a refused `bash` call carries its `argv`
//! ([`argv_if_it_parsed`], ratified at 5982002587). And v4 adds a `bash`
//! call's `cwd`, the command's working directory as the runner passed it,
//! absolute or a tilde form never expanded to a user ([`is_a_working_directory`]):
//! it comes with `argv` -- a `cwd` without an `argv` is refused on any line,
//! and in a log that declares v4 an `argv` without a `cwd` is refused
//! ([`cwd_with_argv`]), so `cwd` is required, optional and forbidden exactly
//! where `argv` is. `not_allowed` stays readable and no v4 writer
//! writes it. An unanswered prompt is the `cancelled` outcome, unchanged;
//! `lane` stays reserved, and there is no `operator` kind. And v4 adds a
//! `tool_call`'s `files` (ruled at 5983588924): the files a call left, each
//! by reference -- its `path` relative to the recording's directory, its
//! `sha256`, `media_type` and `bytes` -- and never its content
//! ([`RECORDED_FILE`]), optional under `ran` and `command_failed` on any name
//! and refused under `refused` and `cancelled` ([`outcome_keys`]). A log that
//! declares 0 to 3 and carries `approval`, `cwd`, `files`, `denylist` or
//! `declined` is refused the same way. v4 is closed with `files`.
//!
//! v6 (#493) adds the `seam` kind: the operator declared a seam, and the
//! trunk was refilled from working memory -- the session's head with the
//! working object rendered after it, and no turn of the old trunk carried
//! (a total compaction, the maintainer's intent on #493). It carries the turn
//! it follows (`at_turn`), why it fired (`reason`, a [`SeamReason`]), the
//! digests of the trunk's head before and after (`prefix_hash_before`,
//! `prefix_hash_after`), the frame the render was built with and the render
//! itself (`frame`, `render`), and what the refill carried (`carried_entries`,
//! `carried_turns`), so a later strategy that carries a tail is told apart
//! from this one. There is no ratification in v6: the audit's answer grammar
//! is unwritten. Whole-log rules: a seam follows the latest turn,
//! settled, while the state is `awaiting` and no fork is unsettled; a fork of
//! a turn at or before a seam is refused, since its warm tail is gone.
//! `seam-not-built` stays readable, and no v6 writer writes it; v6 adds the
//! refusal `nothing-to-seam`, for a seam declared before any turn settled or
//! over empty working memory. A log that
//! declares 0 to 5 and carries a `seam` line is refused the same way.
//!
//! v7 (#509, the duty-of-care ruling of 2026-10-07) replaces the substrate
//! claim's `engine_build` and `engine_identity` with `served`: each field of
//! the served configuration the registry declares, by its registry key, with
//! its `value`, its `provenance` ([`FieldProvenance`]: `declared` when the
//! engine reports nothing on it, `corroborated` when it reports it and the
//! report agreed), and under `corroborated` what the engine `reported`. A
//! claim carries `served` or the two older keys, never both; a log that
//! declares 0 to 6 and carries `served` is refused the same way, and one that
//! declares 7 and carries `engine_build` or `engine_identity` is refused. A
//! contradiction is never logged: `serve` refuses to start on one. v7 also
//! adds a `session.start`'s `template_kwargs` (R1): the template variables
//! every request of the session sends, as sent -- `enable_thinking` and
//! `reasoning_effort` -- an object never empty -- and its `unsent`, what the
//! regime declares and no request carries (`budget_tokens`), recorded rather
//! than refused. And v7 adds the [`ApprovalScope`] `off` and a
//! `session.start`'s `approvals_off`, `true` or absent: the approval lever's
//! `none`, under which no gate decided a call and nothing prompted. A v7
//! `seam` may carry `tail_tokens`, the compaction depth it ran at, and
//! `carried_tokens`, the estimated tokens of the whole turns it kept after
//! the refill (#552); absent, the total refill, and `carried_turns` 0. A
//! v7 `seam` may carry `tool_outputs`, the seam's tool-output state (#553),
//! and, when it carried any, `outputs` -- the section of the refill after
//! the render, as sent -- with `carried_outputs`, the outputs it carried,
//! and `carried_output_bytes`, the section's bytes; absent, `evict`.
//!
//! # A torn final line
//!
//! A writer killed mid-write leaves the start of an event with no line break
//! after it. [`read`] sets that one torn FINAL line aside and counts it
//! (`torn: 1`); the log's truth ends at the last complete event (#230). A
//! torn line anywhere else is refused, as any line no version reads is.
//!
//! The draft is `diet/drive/plans/r2c-proposal.md`, D4, as ruled on #117:
//! names from a ruling first, then the record, then the drive's own tags. A
//! cancelled call is `cancelled`, never a `response`; the drive's rejected,
//! failed and crashed calls are one `request.failed` with a reason, because
//! the record's `rejected` means something else. References are the `seq` of
//! the `request` they answer -- an identifier the log issues, not one anybody
//! invents.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use pest::Parser;
use pest_derive::Parser;

use super::record::json::{self, Value};
use super::record::vocabulary;

#[derive(Parser)]
#[grammar = "../formats/record/grammar.pest"]
#[grammar = "../formats/log/grammar.pest"]
#[grammar = "../formats/number.pest"]
struct LogParser;

/// The version this module writes, as `session.start` states it.
pub const VERSION: i64 = 7;

/// Every version this module reads.
pub const READS: &[i64] = &[0, 1, 2, 3, 4, 5, 6, 7];

/// How recent an input event must be, at the moment a turn settles, for the
/// person to count as already present: `notice` is then zero (Q4 (a), ruled
/// on #117, comment 5883557987). Declared here so the fold can name it and a
/// later session can change it as data rather than as a reading.
pub const PRESENCE_WINDOW_MS: u64 = 2000;

vocabulary! {
    /// What each line is.
    Kind {
        /// The session opened. The first line, and only the first.
        SessionStart => "session.start",
        /// An ask was admitted, and a turn begins on it.
        Ask => "ask",
        /// The session's state moved.
        Settlement => "settlement",
        /// A call was made to the model.
        Request => "request",
        /// A command was refused.
        Refused => "refused",
        /// A piece of a call's answer arrived.
        Delta => "delta",
        /// A stop was asked for a turn's call.
        StopAsked => "stop.asked",
        /// A call answered.
        Response => "response",
        /// A call was stopped.
        Cancelled => "cancelled",
        /// A call ended without an answer: refused by the server, failed, or
        /// its thread crashed.
        RequestFailed => "request.failed",
        /// A turn is over.
        TurnSettled => "turn.settled",
        /// A person's idle gap after a settled turn, as the surface measured
        /// it (Q4, ruled on #117 2026-09-27).
        IdleGap => "idle.gap",
        /// The server's count of a call's prompt prefilled so far (v1).
        Progress => "progress",
        /// A call the model made, and what became of it (v3).
        ToolCall => "tool_call",
        /// A side call off the trunk's warm tail, in the gap after a settled
        /// turn, under the regimen's warrant (v5, #374).
        Fork => "fork",
        /// How a fork ended (v5, #374).
        ForkSettled => "fork.settled",
        /// One entry a fork's answer patched into working memory (v5, #374).
        Patch => "patch",
        /// The trunk refilled from working memory (v6, #493).
        Seam => "seam",
        /// Forks' patches delivered at the tail of a trunk request (v7, the
        /// fork delivery lever).
        Delivered => "delivered",
    }
}

vocabulary! {
    /// The tool output disposition lever's arrival state (v7, #554): whether
    /// what the model is shown of a tool's output is capped as it arrives.
    ToolOutputState {
        /// Capped at a line and a byte limit, the whole kept by digest.
        Capped => "capped",
        /// Kept whole: the cap turned off.
        Keep => "keep",
    }
}

/// The cap a session's tool outputs arrived under (v7, #554): on the wire,
/// `tool_output` and, when capped, `tool_output_max_lines` and
/// `tool_output_max_bytes`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolOutput {
    /// Capped or kept.
    pub state: ToolOutputState,
    /// The line limit, when capped.
    pub max_lines: Option<u64>,
    /// The byte limit, when capped.
    pub max_bytes: Option<u64>,
}

/// A `session.start`'s cap on tool output, from its three flat keys: the
/// limits present exactly when it is `capped`.
fn tool_output(fields: &Fields<'_>) -> Result<Option<ToolOutput>, String> {
    let state = fields.optional_tag("tool_output", ToolOutputState::from_tag)?;
    let max_lines = fields.optional_count("tool_output_max_lines")?;
    let max_bytes = fields.optional_count("tool_output_max_bytes")?;
    let limited = max_lines.is_some() || max_bytes.is_some();
    match state {
        None if limited => Err(
            "`tool_output_max_lines` or `tool_output_max_bytes` without `tool_output`".to_owned(),
        ),
        None => Ok(None),
        Some(ToolOutputState::Capped) if max_lines.is_none() || max_bytes.is_none() => Err(
            "`tool_output` is `capped` without both `tool_output_max_lines` and \
             `tool_output_max_bytes`"
                .to_owned(),
        ),
        Some(ToolOutputState::Keep) if limited => {
            Err("`tool_output` is `keep` and carries a limit".to_owned())
        }
        Some(state) => Ok(Some(ToolOutput {
            state,
            max_lines,
            max_bytes,
        })),
    }
}

vocabulary! {
    /// The fork delivery lever's state (v7): how a fork's patches reach the
    /// trunk.
    ForkDelivery {
        /// At the next seam's render only: today's behaviour, the default.
        Seam => "seam",
        /// As a note at the tail of the next trunk request, framed as advice.
        Advisory => "advisory",
        /// The same, framed as an instruction.
        Imperative => "imperative",
    }
}

vocabulary! {
    /// What a seam's refill carries of the tool outputs it compacts away
    /// (v7, #553): those in the turns before the kept tail.
    SeamToolOutputs {
        /// Nothing: today's behaviour, the default.
        Evict => "evict",
        /// A line per output naming its tool, arguments, size and sha256,
        /// the whole saved by digest in the recording.
        Reference => "reference",
        /// The verbatim excerpts the read fork quoted from each output.
        Salient => "salient",
        /// Each output as the trunk had it, after the cap.
        Keep => "keep",
    }
}

impl Default for SeamToolOutputs {
    /// `evict`: today's behaviour.
    fn default() -> Self {
        Self::Evict
    }
}

vocabulary! {
    /// How a fork's patches reach the trunk before a seam (v7): the fork
    /// delivery lever's two mid-turn states, each a (b′) framing.
    Framing {
        /// "may be affected ... If it no longer holds, say so; otherwise
        /// carry on."
        Advisory => "advisory",
        /// "is superseded ... Update it now".
        Imperative => "imperative",
    }
}

vocabulary! {
    /// Why a seam fired (v6, #493): `seam::Reason`'s words. `serve` writes
    /// `operator`, and `cadence` and `budget` when the regimen declares them
    /// (#520); `phase` is read so that a session that fires it needs no new
    /// version.
    SeamReason {
        /// The operator declared it.
        Operator => "operator",
        /// The phase graph ratified a transition.
        Phase => "phase",
        /// The declared budget was reached: the working set's byte count, or
        /// a share of the context window (#520).
        Budget => "budget",
        /// The declared cadence came round.
        Cadence => "cadence",
    }
}

vocabulary! {
    /// What ended an idle gap.
    GapEnd {
        /// An ask was sent.
        Ask => "ask",
        /// A seam was declared.
        Seam => "seam",
        /// A stop was sent.
        Cancel => "cancel",
        /// The session ended.
        End => "end",
    }
}

vocabulary! {
    /// What the session is doing.
    State {
        /// Nothing in flight; an ask is welcome.
        Awaiting => "awaiting",
        /// The trunk is answering.
        Turn => "turn",
        /// The idle-gap work after a settled turn.
        Capture => "capture",
        /// Over.
        Ended => "ended",
    }
}

vocabulary! {
    /// A command a person sent.
    Command {
        /// An ask.
        Ask => "ask",
        /// A stop.
        Cancel => "cancel",
        /// A seam.
        DeclareSeam => "declare-seam",
        /// The end.
        End => "end",
    }
}

vocabulary! {
    /// Why a command was refused.
    Refusal {
        /// A turn or a capture is in flight.
        InFlight => "in-flight",
        /// The session has ended.
        Ended => "ended",
        /// The turn named has no call in flight.
        NothingInFlight => "nothing-in-flight",
        /// Seams are not built yet. Readable; no v6 writer writes it.
        SeamNotBuilt => "seam-not-built",
        /// Nothing to refill from: no turn has settled, or working memory
        /// holds no entry (v6).
        NothingToSeam => "nothing-to-seam",
        /// A seam asked to move to a phase under no phase graph (v7, #563).
        NoPhaseGraph => "no-phase-graph",
        /// A seam asked to move to a phase the graph does not declare (v7).
        NotAPhase => "not-a-phase",
        /// A seam asked to move to the phase the session is in (v7).
        AlreadyInPhase => "already-in-phase",
        /// A seam asked for a move the graph does not allow from the current
        /// phase (v7).
        NoPhaseEdge => "no-phase-edge",
        /// A stop named a turn older than the latest.
        Stale => "stale",
    }
}

vocabulary! {
    /// Which lane a request was made on.
    Lane {
        /// The canonical session.
        Trunk => "trunk",
        /// A fork's single call off the trunk's warm tail, never appended
        /// to it (v5, #374).
        Interview => "interview",
    }
}

vocabulary! {
    /// What warranted a fork: the regimen's rule, never the model's choice
    /// (v5, #374, ruled at 5985110649).
    Warrant {
        /// The settled turn read a file.
        Read => "read",
        /// The settled turn's ask was marked a scoping question.
        Scoping => "scoping",
    }
}

vocabulary! {
    /// How a fork ended (v5, #374).
    ForkOutcome {
        /// It answered, and its answer was folded into patches.
        Value => "value",
        /// It answered that it had nothing to record.
        Decline => "decline",
        /// It answered in a shape the fold could not read.
        Unparseable => "unparseable",
        /// Its call stopped at its output cap.
        Truncated => "truncated",
        /// Its call ended without an answer.
        Failed => "failed",
        /// It was stopped.
        Cancelled => "cancelled",
    }
}

vocabulary! {
    /// What a patch does to working memory: `object::Patch`'s variants
    /// (v5, #374).
    PatchOp {
        /// A new entry.
        Add => "add",
        /// An entry that replaces an earlier one.
        Supersede => "supersede",
        /// An entry resolved.
        Resolve => "resolve",
        /// An entry retired.
        Retire => "retire",
        /// An entry parked.
        Park => "park",
    }
}

vocabulary! {
    /// Who a head message is from.
    Role {
        /// The standing instruction.
        System => "system",
        /// The person.
        User => "user",
        /// The model.
        Assistant => "assistant",
    }
}

vocabulary! {
    /// Why a call ended without an answer.
    FailReason {
        /// The server refused it.
        Server => "server",
        /// It ran out of time.
        Timeout => "timeout",
        /// The connection failed.
        Transport => "transport",
        /// The thread making it crashed.
        Crashed => "crashed",
        /// The server refused it before any prefill, because the prompt is
        /// longer than its context -- read from the refusal's typed field,
        /// never its message (v1; R3.0's C3).
        ContextOverflow => "context_overflow",
    }
}

vocabulary! {
    /// How a turn ended.
    SettleReason {
        /// Answered.
        Final => "final",
        /// Stopped.
        Cancelled => "cancelled",
        /// The tool loop ran out of steps.
        MaxSteps => "max_steps",
        /// Out of time.
        Timeout => "timeout",
        /// Anything else that ended it without an answer.
        Failed => "failed",
        /// Its trunk call stopped at its output cap (v3, #290).
        Capped => "capped",
    }
}

vocabulary! {
    /// What became of a call the model made (v3).
    ToolOutcome {
        /// The command ran and exited, or a signal ended it.
        Ran => "ran",
        /// The drive did not run it.
        Refused => "refused",
        /// The command failed under its confinement's policy (#29's I5).
        CommandFailed => "command_failed",
        /// Its outcome never arrived: the turn was cancelled, or the session
        /// ended, mid-command.
        Cancelled => "cancelled",
    }
}

vocabulary! {
    /// How confined a call's command was: `diet/src/isolation`'s
    /// `Isolation::tag`, word for word (v3).
    Isolation {
        /// Unconfined, declared.
        None => "none",
        /// The sandbox.
        Sandbox => "sandbox",
        /// A virtual machine.
        Vm => "vm",
        /// Not recorded: a placed replay's call, whose confinement the
        /// recording does not say (#297). Not one of the isolation module's
        /// words; a served session may not write it.
        Unrecorded => "unrecorded",
    }
}

vocabulary! {
    /// What a call's command could reach on the network:
    /// `diet/src/isolation`'s `Network::tag`, word for word (v3).
    Network {
        /// Nothing.
        None => "none",
        /// The host's network.
        Host => "host",
        /// Not recorded, as [`Isolation::Unrecorded`] (#297).
        Unrecorded => "unrecorded",
    }
}

vocabulary! {
    /// Why the drive did not run a call (v3, ruled on #297; v4, #388).
    ToolRefusal {
        /// The allowlist does not admit it. Readable; no v4 writer writes
        /// it (#298 point 10).
        NotAllowed => "not_allowed",
        /// The tool loop's step limit was reached.
        MaxSteps => "max_steps",
        /// Its arguments are not what the tool takes.
        Unparsable => "unparsable",
        /// It names a tool the session did not declare.
        UnknownTool => "unknown_tool",
        /// The destructive denylist refuses it (v4).
        Denylist => "denylist",
        /// The operator was asked and said no (v4).
        Declined => "declined",
    }
}

vocabulary! {
    /// How far an approval a call ran under reaches (v4, #388): the
    /// operator's choice at the prompt, or a pre-seeded session set.
    ApprovalScope {
        /// This call only.
        Once => "once",
        /// Every call of the same shape for the rest of the session.
        Session => "session",
        /// Every call of the same shape in the workspace, persisted.
        Workspace => "workspace",
        /// The session started with it allowed: no prompt decided it.
        Preseeded => "preseeded",
        /// Approvals were off (v7): no gate decided it and nothing prompted.
        Off => "off",
    }
}

vocabulary! {
    /// How the engine's identity was established at the start-time check
    /// (v3, ruled on #297 Q4).
    EngineIdentity {
        /// The server's `build_info` named a commit, and it was checked.
        CheckedCommit => "checked_commit",
        /// The server names no commit; its literal `build_info` matched.
        LiteralMatched => "literal_matched",
    }
}

vocabulary! {
    /// Where a served field's value comes from (v7, #509, the duty-of-care
    /// ruling of 2026-10-07).
    FieldProvenance {
        /// Whoever ran the test declared it; the engine did not report it.
        Declared => "declared",
        /// The engine reported it, and the report agreed with the
        /// declaration.
        Corroborated => "corroborated",
    }
}

vocabulary! {
    /// Who wrote a log that a session did not write as it ran (v3, ruled on
    /// #297 at 5976392264). Absent, a session wrote it as it ran.
    Provenance {
        /// A recording placed into the log, replayed rather than served.
        Placed => "placed",
        /// Written by hand, as a fixture is: no session ran it.
        Constructed => "constructed",
    }
}

/// One message of the head the trunk starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadMessage {
    /// Who it is from.
    pub role: Role,
    /// What it says.
    pub content: String,
}

/// What one delta carries: exactly one of the three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Answer text.
    Text(String),
    /// Reasoning text, from a model that thinks.
    Reasoning(String),
    /// A fragment of a streamed tool call, as the server sent it (v3). The
    /// stream's `type` is not kept: it is always `"function"`.
    ToolCall {
        /// Which call of the response it belongs to.
        index: u64,
        /// The call's id, on its first fragment only.
        id: Option<String>,
        /// The function's name, on its first fragment only.
        name: Option<String>,
        /// This fragment of the arguments text.
        arguments: String,
    },
}

/// What a `session.start` claims serves it (v3, #292): the regimen's
/// substrate, the registry it was read from, and the engine the start-time
/// check passed. Its four keys come together or not at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubstrateClaim {
    /// The substrate's registry id.
    pub substrate: String,
    /// The sha256 of the registry the id was read from.
    pub registry_sha256: String,
    /// What the start-time check established about the engine.
    pub engine: ClaimedEngine,
}

/// What a substrate claim says of the engine: in v3 to v6, the build the
/// check passed and how; from v7 (#509), each served field and where its
/// value comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimedEngine {
    /// `engine_build` and `engine_identity` (v3 to v6).
    Checked {
        /// The `build_info` the engine check passed.
        build: String,
        /// How the engine's identity was established.
        identity: EngineIdentity,
    },
    /// `served` (v7): never empty.
    Served(Vec<ServedField>),
}

/// The template variables a session sends on every request (v7, R1): what
/// reaches the chat template, as sent. At least one is carried.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateKwargs {
    /// `enable_thinking`: whether thinking was requested.
    pub enable_thinking: Option<bool>,
    /// `reasoning_effort`: the level, in the template's own words.
    pub reasoning_effort: Option<String>,
    /// `preserve_thinking`: whether earlier turns' reasoning renders in
    /// history, as the substrate's model convention needs.
    pub preserve_thinking: Option<bool>,
}

/// What a session's regime declares and its requests cannot carry (v7, R1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsent {
    /// `[reasoning]`'s token budget: no chat template variable carries one.
    pub budget_tokens: u64,
}

/// One line of a delivered note (v7): the patch it delivers, by its entry
/// and op, and the dogma template it was written with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteLine {
    /// The entry the line names: the voided one for a `supersede`, the
    /// target for a `resolve`, `retire` or `park`.
    pub entry: String,
    /// The patch's op.
    pub op: PatchOp,
    /// The template's dogma name.
    pub template: String,
}

/// A move between two phases of a phase graph (v7, #563): a seam's move, or
/// one of the graph's allowed transitions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhaseMove {
    /// The phase moved from.
    pub from: String,
    /// The phase moved to.
    pub to: String,
}

/// One instruction file a session's system prompt carries (v7, #559).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstructionFile {
    /// Its path relative to the worktree: never absolute, never a home's.
    pub path: String,
    /// The sha256 of its bytes.
    pub sha256: String,
}

/// One field of the served configuration (v7, #509).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedField {
    /// The field, by its registry key.
    pub field: String,
    /// The declared value.
    pub value: String,
    /// Where the value comes from.
    pub provenance: FieldProvenance,
    /// What the engine reported, under `corroborated` only.
    pub reported: Option<String>,
}

/// One output stream of a call's command: its text, whole, and its length
/// in bytes beside it, so a reader can size a line before parsing it (v3,
/// ruled on #297 Q3). The count is what the command produced and the text
/// its decoding, lossy where the output is not UTF-8, so the two may
/// differ and the reader does not compare them; it refuses only a count
/// that is `0` beside a text that is not empty, or not `0` beside one that
/// is (ruled at 5975957135).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// What it printed.
    pub text: String,
    /// How many bytes that is.
    pub bytes: u64,
}

/// The decision a call ran under (v4, #388): its scope, and when an
/// operator decided it, on the line's own clock. A `preseeded` approval was
/// decided by no prompt and carries no `decided_at`; every other scope
/// carries one, never after the line's `t`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Approval {
    /// How far it reaches.
    pub scope: ApprovalScope,
    /// When it was decided: milliseconds since the session opened.
    pub decided_at: Option<u64>,
    /// Why the call prompted: the prompting segment's reason, one word of an
    /// open set (ruled at 5982826236).
    pub why: Option<String>,
}

/// A file a call left, recorded by reference and never inlined (v4, ruled
/// at 5983588924).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedFile {
    /// Where it is, relative to the recording's directory.
    pub path: String,
    /// The sha256 of its bytes.
    pub sha256: String,
    /// Its media type, `type/subtype`.
    pub media_type: String,
    /// How many bytes it is.
    pub bytes: u64,
}

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The session opened.
    SessionStart {
        /// The format version the log declares: one of [`READS`]. A writer
        /// writes [`VERSION`].
        version: i64,
        /// When: milliseconds since the Unix epoch.
        opened: u64,
        /// The model name requests are sent with -- a name, not an identity.
        model: String,
        /// The messages the trunk starts from.
        head: Vec<HeadMessage>,
        /// What serves the session, as the client declares it (v2, #30
        /// D7/N10): its dialect, and its concurrency when declared.
        serving: Option<Serving>,
        /// What the session claims serves it (v3, #292), when it was started
        /// against a regimen.
        claim: Option<SubstrateClaim>,
        /// Who wrote the log, when a session did not write it as it ran (v3).
        provenance: Option<Provenance>,
        /// The tools the session declared to the model, by name, in the
        /// order its requests carry them (v5, #472): what a head is rebuilt
        /// with.
        tools: Option<Vec<String>>,
        /// The `chat_template_kwargs` every request of the session carries
        /// (v7, R1): the reasoning state as it was requested on the wire.
        template_kwargs: Option<TemplateKwargs>,
        /// What the regime declares and no request carries (v7, R1): best
        /// effort in the duty-of-care sense, recorded rather than refused.
        unsent: Option<Unsent>,
        /// Approvals were off for the session (v7, the approval lever's
        /// `none`): `true`, or absent.
        approvals_off: Option<bool>,
        /// The fork delivery lever's state (v7), for a session that forks.
        fork_delivery: Option<ForkDelivery>,
        /// With thinking on and no `reasoning_effort` sent, the level the
        /// chat template renders by default, as the registry declares it
        /// (v7): what the model was asked for, named.
        reasoning_effort_default: Option<String>,
        /// The phase graph a served session runs under (v7, #563): its
        /// phases, in declared order, and the moves it allows; absent when it
        /// declares none.
        phases: Option<Vec<String>>,
        /// The graph's allowed moves.
        phase_transitions: Option<Vec<PhaseMove>>,
        /// The phase the session opens in: the graph's first. Named so
        /// because recordings placed in older versions carry a `phase` of
        /// their own on this line.
        opening_phase: Option<String>,
        /// The instruction files the system prompt carries (v7, #559): each
        /// by its path relative to the worktree and its digest. Their text
        /// is in `head`.
        instruction_files: Option<Vec<InstructionFile>>,
        /// The cap tool outputs arrived under (v7, #554), when the session
        /// runs tools.
        tool_output: Option<ToolOutput>,
    },
    /// An ask was admitted.
    Ask {
        /// The turn it begins, from 1.
        turn: u32,
        /// What was asked.
        text: String,
        /// Whether the operator marked it a scoping question (v5, #374).
        scoping: Option<bool>,
        /// The files the operator attached to it, by reference: sent to the
        /// model as image parts of this message, never inlined here (v5,
        /// #372, ruled at 5989411005).
        files: Option<Vec<RecordedFile>>,
    },
    /// The state moved.
    Settlement {
        /// Where it was.
        from: State,
        /// Where it is now.
        to: State,
    },
    /// A call was made.
    Request {
        /// The turn it belongs to.
        turn: u32,
        /// The lane it was made on.
        lane: Lane,
        /// The sha256 of the head of the request as sent: the bytes its
        /// body starts with, as the client hashes them (v2, ruled on #157,
        /// so a live record's request names the prefix it sent).
        head_sha256: Option<String>,
        /// The `seq` of the fork it is the call of, on the `interview` lane
        /// (v5, #374).
        fork: Option<u64>,
        /// The `max_tokens` it was sent with, after the output cap was
        /// clamped to the room left in the context window (v7, #588).
        max_tokens: Option<u64>,
    },
    /// A command was refused.
    Refused {
        /// Which command.
        command: Command,
        /// Why.
        because: Refusal,
        /// What the session was doing.
        during: State,
    },
    /// A piece of an answer.
    Delta {
        /// The `seq` of the `request` it answers.
        request: u64,
        /// What arrived.
        piece: Piece,
    },
    /// A stop was asked.
    StopAsked {
        /// The turn it was asked for.
        turn: u32,
    },
    /// A call answered.
    Response {
        /// The `seq` of the `request` it answers.
        to_request: u64,
        /// The whole answer.
        text: String,
        /// Why the server stopped, as it spelled it, if it said.
        finish_reason: Option<String>,
        /// The whole reasoning, as its deltas streamed it, byte for byte;
        /// absent when none arrived (v1, D4).
        reasoning: Option<String>,
        /// What the server measured of the call, as it reported it (v1, D1).
        timings: Option<Timings>,
        /// The server's token counts, as it reported them, for a dialect
        /// whose server reports no `timings` (v2, ruled on #157).
        usage: Option<Usage>,
        /// Whether the answer stopped at its output cap (v2, #30 D3/N10);
        /// absent when the writer did not say.
        capped: Option<bool>,
    },
    /// A call was stopped.
    Cancelled {
        /// The `seq` of the `request` stopped.
        request: u64,
        /// What arrived before the stop. Never an answer.
        partial: String,
        /// The reasoning that streamed before the stop, beside `partial`
        /// (v3, #291).
        reasoning: Option<String>,
    },
    /// A call ended without an answer.
    RequestFailed {
        /// The `seq` of the `request`.
        request: u64,
        /// Why.
        reason: FailReason,
        /// What the server, the transport or the panic said.
        message: String,
        /// The HTTP status, when the server refused it.
        status: Option<u16>,
        /// What arrived before it ended, when anything did.
        partial: Option<String>,
    },
    /// A turn is over.
    TurnSettled {
        /// The turn.
        turn: u32,
        /// How it ended.
        reason: SettleReason,
    },
    /// An idle gap, emitted by the surface once, at the command that is
    /// finally admitted. Integer milliseconds on the surface's monotonic
    /// clock; durations only, never content (Q4, as ruled on #117 in
    /// comments 5883557987 and 5885438738).
    ///
    /// A refused command ends no gap: the person is still in it, about to
    /// retry. The gap continues, the refusal's span is added to `blocked`,
    /// and the gap is logged once, at the command finally admitted. The gap
    /// measures the person, not the server. The surface's *expectation* of
    /// admission decides only whether an attempt is made; it never decides
    /// whether a gap ends.
    IdleGap {
        /// The `seq` of the `turn.settled` that opened the gap.
        opened_by: u64,
        /// From the settling to the first sign the person is present: an
        /// input event, the settled block entering the viewport after the gap
        /// opened, or a scroll by hand (wheel, touch, pointer, keys -- never
        /// the page's own follow-to-bottom). Zero when an input event came
        /// within [`PRESENCE_WINDOW_MS`] before the settling.
        notice: u64,
        /// From the end of `notice` to the first keystroke or seam click.
        read: u64,
        /// From the first keystroke to the accepted send or declare.
        compose: u64,
        /// The time the page was hidden, taken out of the phase it
        /// interrupted.
        away: u64,
        /// From the first held or refused attempt to the accepted send. A
        /// held attempt emits nothing itself.
        blocked: u64,
        /// What ended it.
        ended_by: GapEnd,
    },
    /// The server's count of a call's prompt, prefilled so far: one line per
    /// progress frame the server streams, before the call's first delta (v1,
    /// D2; its keys are the frame's own, R3.0's C1).
    Progress {
        /// The `seq` of the `request` it counts.
        request: u64,
        /// Prompt tokens in all.
        total: u64,
        /// Prompt tokens reused from the slot's cache.
        cache: u64,
        /// Prompt tokens processed so far.
        processed: u64,
        /// Milliseconds of prefill so far, by the server's clock.
        time_ms: u64,
    },
    /// A call the model made, written once its outcome is known: every call
    /// leaves exactly one (v3, ruled on #297). Which keys it carries is
    /// [`outcome_keys`]'s.
    ToolCall {
        /// The `seq` of the `request` whose response carried the call.
        request: u64,
        /// The turn.
        turn: u32,
        /// The call's id, as streamed.
        id: String,
        /// The function's name.
        name: String,
        /// The arguments text assembled from the streamed fragments, byte for
        /// byte, never parsed.
        arguments: String,
        /// What became of it.
        outcome: ToolOutcome,
        /// The command as the drive asked for it.
        argv: Option<Vec<String>>,
        /// Its working directory, as the runner passed it (v4).
        cwd: Option<String>,
        /// What actually ran, runner and all.
        confined: Option<Vec<String>>,
        /// The confinement it ran under.
        isolation: Option<Isolation>,
        /// The network it had.
        network: Option<Network>,
        /// Its exit status; absent when a signal ended it.
        exit: Option<u64>,
        /// Why it was refused.
        reason: Option<ToolRefusal>,
        /// The sha256 of the confinement policy it failed under.
        policy: Option<String>,
        /// What it printed.
        stdout: Option<Output>,
        /// What it printed on standard error.
        stderr: Option<Output>,
        /// The decision it ran under, when it ran under one (v4).
        approval: Option<Approval>,
        /// The files it left, by reference (v4).
        files: Option<Vec<RecordedFile>>,
        /// Exactly what the model was given as this call's result, when it
        /// was given one (v5, #472): what the next step's head is rebuilt
        /// with.
        shown: Option<String>,
        /// The text this call was recovered from (v7, #560): the block the
        /// model wrote in its answer, when the call was not a native one.
        /// Absent for a streamed call.
        recovered_from: Option<String>,
    },
    /// A side call off the trunk's warm tail (v5, #374).
    Fork {
        /// The lane its call is made on: `interview`.
        lane: Lane,
        /// The settled turn it follows.
        of_turn: u32,
        /// The `seq` of that turn's trunk `request`, whose answer it forks
        /// from.
        at: u64,
        /// What warranted it.
        why: Warrant,
        /// What it asks.
        question: String,
    },
    /// How a fork ended (v5, #374).
    ForkSettled {
        /// The `seq` of the fork.
        fork: u64,
        /// How.
        outcome: ForkOutcome,
    },
    /// One entry a fork's answer patched into working memory (v5, #374).
    Patch {
        /// The `seq` of the fork.
        fork: u64,
        /// What it does.
        op: PatchOp,
        /// The entry.
        entry: PatchEntry,
        /// The id of the entry it replaces, exactly when `op` is
        /// `supersede`.
        supersedes: Option<String>,
    },
    /// Forks' patches delivered to the trunk (v7, the fork delivery lever):
    /// one note at the tail of turn `turn`'s first request, after its ask,
    /// which stays on the trunk.
    Delivered {
        /// The turn whose request carried it.
        turn: u32,
        /// The framing every line used.
        framing: Framing,
        /// The note as sent: one line per patch.
        text: String,
        /// Each line, in order: the patch it delivers and the template it
        /// was written with.
        lines: Vec<NoteLine>,
    },
    /// The trunk refilled from working memory (v6, #493).
    Seam {
        /// The latest turn, settled, which the seam follows.
        at_turn: u32,
        /// Why it fired.
        reason: SeamReason,
        /// The `head_sha256` a trunk request on the trunk before the refill
        /// would carry; `prefix_hash_after`'s is the next trunk request's.
        prefix_hash_before: String,
        /// The same, after.
        prefix_hash_after: String,
        /// The frame the render was built with (`seam::render::FRAME_VERSION`).
        frame: String,
        /// The working object, rendered: what the refilled head carries after
        /// the session's own.
        render: String,
        /// How many working-memory entries the render carried.
        carried_entries: u64,
        /// How many turns of the old trunk the refill carried.
        carried_turns: u64,
        /// The compaction depth the seam ran at (v7, #552): the estimated
        /// tokens of recent whole turns it could keep. Absent is 0, the
        /// total refill.
        tail_tokens: Option<u64>,
        /// The estimated tokens of the turns it kept (v7, #552), beside
        /// `tail_tokens`.
        carried_tokens: Option<u64>,
        /// The phases it moved between (v7, #563), when the operator named
        /// a move the graph allowed; absent when the session stayed in its
        /// phase.
        phase: Option<PhaseMove>,
        /// What it carried of the tool outputs it compacted away (v7,
        /// #553). Absent is `evict`.
        tool_outputs: Option<SeamToolOutputs>,
        /// The section of the refill after the render carrying them, as
        /// sent (v7, #553): present exactly when it carried any.
        outputs: Option<String>,
        /// How many outputs the section carried (v7, #553).
        carried_outputs: Option<u64>,
        /// The section's bytes (v7, #553).
        carried_output_bytes: Option<u64>,
    },
}

/// A patch's entry (v5, #374): its id, its text, and its category when the
/// fold names one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatchEntry {
    /// The entry's id.
    pub id: String,
    /// What it records.
    pub text: String,
    /// Its category, when named.
    pub category: Option<String>,
}

/// A call's token counts as the server reported them in its `usage`
/// object, keys as the server names them (v2). Carried only for a dialect
/// whose server reports no `timings`: on llama.cpp they equal `timings`,
/// measured on #157, and carrying both would let them disagree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Usage {
    /// Prompt tokens, cached or not.
    pub prompt_tokens: u64,
    /// Tokens generated.
    pub completion_tokens: u64,
    /// Prompt tokens reused from a cache, where the server reports them.
    pub cached_tokens: Option<u64>,
}

/// What serves a session, as the client declares it (v2, #30 D7/N10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Serving {
    /// The client's dialect, by its name.
    pub dialect: String,
    /// How many requests the server serves at once, when declared. Absent
    /// is undeclared, never one.
    pub concurrency: Option<u64>,
}

/// A call's timings as the server reported them: llama.cpp's `timings`
/// object, keys as the server names them (v1, D1). Each is absent when the
/// server did not send it -- never zero -- and nothing here is computed from
/// anything else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timings {
    /// Prompt tokens prefilled.
    pub prompt_n: Option<u64>,
    /// Prompt tokens reused from the slot's cache.
    pub cache_n: Option<u64>,
    /// How long the prefill took.
    pub prompt_ms: Option<Millis>,
    /// Tokens generated.
    pub predicted_n: Option<u64>,
    /// How long generating them took.
    pub predicted_ms: Option<Millis>,
    /// Tokens a speculative decoder drafted: in-band evidence of that
    /// regime (Q3, ruled). A missing key says nothing about speculation.
    pub draft_n: Option<u64>,
    /// How many of them were accepted.
    pub draft_n_accepted: Option<u64>,
}

/// A duration in milliseconds as the digits the server wrote: a
/// non-negative integer or exact decimal of the record's value space, never
/// a float (v1; #162, disclosure 9 ruled).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Millis(String);

impl Millis {
    /// The number written as `text`, or nothing when it is not a
    /// non-negative integer or exact decimal the record's grammar reads.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        let number = json::line(&format!("{{\"n\":{text}}}")).ok()?;
        match number.get("n")? {
            Value::Integer(n) if *n >= 0 => Some(Self(text.to_owned())),
            Value::Decimal(d) if !d.as_str().starts_with('-') && d.as_str() == text => {
                Some(Self(text.to_owned()))
            }
            _ => None,
        }
    }

    /// The digits, as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// As the record's value space holds it.
    fn to_value(&self) -> Value {
        match json::line(&format!("{{\"n\":{}}}", self.0))
            .ok()
            .and_then(|mut object| object.remove("n"))
        {
            Some(value) => value,
            None => unreachable!("a Millis is only ever built from text the grammar reads"),
        }
    }
}

/// One line of a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Its position in the log: gapless, from 0.
    pub seq: u64,
    /// Milliseconds since the session opened.
    pub t: u64,
    /// What happened.
    pub event: Event,
}

/// Why a log was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogError {
    /// The 1-based line it is about; 0 when it is about the whole text.
    pub line: usize,
    /// What is wrong.
    pub why: String,
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.why)
        } else {
            write!(f, "line {}: {}", self.line, self.why)
        }
    }
}

impl std::error::Error for LogError {}

/// Read one line: what a reader that resumes mid-log reads. The rules that
/// span lines are [`parse`]'s.
///
/// # Errors
///
/// When the text is not one object line of the record's value space, or the
/// object is not a line of any version this reader reads.
pub fn line(text: &str) -> Result<Line, String> {
    let object = json::line(text).map_err(|err| err.to_string())?;
    from_object(&object)
}

/// A whole log as read: its complete events, and how many torn lines were
/// set aside after them -- 0, or 1 when the final line was cut short.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    /// Every complete event, in file order.
    pub lines: Vec<Line>,
    /// Torn final lines set aside: 0 or 1.
    pub torn: usize,
}

/// How a scan of a value-space object stopped short of finishing it.
enum Stop {
    /// The text ran out with every byte so far a valid start.
    Ended,
    /// A byte no object of the value space has there.
    Invalid,
}

/// A scan of the record's value space (`diet/formats/record/grammar.pest`)
/// that reports WHERE it stopped, which a parse that fails cannot: pest names
/// the start of the failing token, not whether the text merely ran out.
struct Prefix<'a> {
    text: &'a str,
    at: usize,
}

impl Prefix<'_> {
    fn peek(&self) -> Result<u8, Stop> {
        self.text
            .as_bytes()
            .get(self.at)
            .copied()
            .ok_or(Stop::Ended)
    }

    fn eat(&mut self, byte: u8) -> Result<(), Stop> {
        if self.peek()? == byte {
            self.at += 1;
            Ok(())
        } else {
            Err(Stop::Invalid)
        }
    }

    fn space(&mut self) {
        while matches!(self.text.as_bytes().get(self.at), Some(b' ' | b'\t')) {
            self.at += 1;
        }
    }

    fn object(&mut self) -> Result<(), Stop> {
        self.eat(b'{')?;
        self.space();
        if self.peek()? == b'}' {
            self.at += 1;
            return Ok(());
        }
        loop {
            self.string()?;
            self.space();
            self.eat(b':')?;
            self.space();
            self.value()?;
            self.space();
            match self.peek()? {
                b',' => {
                    self.at += 1;
                    self.space();
                }
                b'}' => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err(Stop::Invalid),
            }
        }
    }

    fn array(&mut self) -> Result<(), Stop> {
        self.eat(b'[')?;
        self.space();
        if self.peek()? == b']' {
            self.at += 1;
            return Ok(());
        }
        loop {
            self.value()?;
            self.space();
            match self.peek()? {
                b',' => {
                    self.at += 1;
                    self.space();
                }
                b']' => {
                    self.at += 1;
                    return Ok(());
                }
                _ => return Err(Stop::Invalid),
            }
        }
    }

    fn value(&mut self) -> Result<(), Stop> {
        match self.peek()? {
            b'{' => self.object(),
            b'[' => self.array(),
            b'"' => self.string(),
            b't' => self.word(b"true"),
            b'f' => self.word(b"false"),
            b'-' | b'0'..=b'9' => self.number(),
            _ => Err(Stop::Invalid),
        }
    }

    fn word(&mut self, word: &[u8]) -> Result<(), Stop> {
        word.iter().try_for_each(|byte| self.eat(*byte))
    }

    fn string(&mut self) -> Result<(), Stop> {
        self.eat(b'"')?;
        loop {
            match self.peek()? {
                b'"' => {
                    self.at += 1;
                    return Ok(());
                }
                b'\\' => {
                    self.at += 1;
                    match self.peek()? {
                        b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't' => self.at += 1,
                        b'u' => {
                            self.at += 1;
                            for _ in 0..4 {
                                if !self.peek()?.is_ascii_hexdigit() {
                                    return Err(Stop::Invalid);
                                }
                                self.at += 1;
                            }
                        }
                        _ => return Err(Stop::Invalid),
                    }
                }
                0x00..=0x1f => return Err(Stop::Invalid),
                _ => self.at += 1,
            }
        }
    }

    /// A number is read whole, then judged: complete, by the grammar's own
    /// `integer` and `decimal`; cut off by the end of the text, by whether
    /// some number of the value space starts that way.
    fn number(&mut self) -> Result<(), Stop> {
        let start = self.at;
        while matches!(
            self.text.as_bytes().get(self.at),
            Some(b'-' | b'0'..=b'9' | b'.')
        ) {
            self.at += 1;
        }
        let token = &self.text[start..self.at];
        if self.at == self.text.len() {
            return Err(if starts_a_number(token) {
                Stop::Ended
            } else {
                Stop::Invalid
            });
        }
        let whole = |rule| {
            LogParser::parse(rule, token)
                .ok()
                .and_then(|mut pairs| pairs.next())
                .is_some_and(|pair| pair.as_span().end() == token.len())
        };
        if whole(Rule::integer) || whole(Rule::decimal) {
            Ok(())
        } else {
            Err(Stop::Invalid)
        }
    }
}

/// Whether some number of the value space starts with `token`: a sign, then
/// `0` or a nonzero digit and digits, then a point and digits.
fn starts_a_number(token: &str) -> bool {
    let unsigned = token.strip_prefix('-').unwrap_or(token);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let whole_ok = whole.is_empty() && !unsigned.contains('.')
        || whole == "0"
        || (whole.starts_with(|c: char| c.is_ascii_digit() && c != '0')
            && whole.bytes().all(|b| b.is_ascii_digit()));
    whole_ok && fraction.bytes().all(|b| b.is_ascii_digit())
}

/// Whether `text` is a STRICT PREFIX of an object the record's GRAMMAR
/// accepts: every byte a valid continuation, and the text ending before the
/// object does. Syntax only: a prefix the grammar could finish but the value
/// reader would then refuse (a duplicate key, a lone surrogate, nesting past
/// the depth limit) is still torn here, and none is what a writer of valid
/// events leaves. That is what a writer killed mid-write leaves, and nothing
/// else is: a complete object, an object with anything after it, and a byte
/// no object has there are not torn writes.
fn is_a_torn_write(text: &str) -> bool {
    text.starts_with('{') && matches!(Prefix { text, at: 0 }.object(), Err(Stop::Ended))
}

/// A log's text from its bytes. A writer killed mid-write can cut a
/// character as well as an event, so bytes that end inside a UTF-8 sequence
/// are read up to the cut -- but only when what is left of the final line is
/// a torn write ([`is_a_torn_write`]) that the cut character itself
/// continues -- that is, the cut is inside a string or a key; any other
/// invalid UTF-8 is refused as it is in every format.
///
/// # Errors
///
/// When the bytes are not UTF-8 and are not a log cut inside a character.
pub fn decode(bytes: &[u8]) -> Result<&str, std::str::Utf8Error> {
    let err = match std::str::from_utf8(bytes) {
        Ok(text) => return Ok(text),
        Err(err) => err,
    };
    if err.error_len().is_none()
        && let Ok(text) = std::str::from_utf8(&bytes[..err.valid_up_to()])
        // The cut character itself must continue the tail: a non-ASCII byte
        // stands only inside a string, so the tail with a whole character
        // where the cut one was must still be a torn write. Without this, a
        // partial byte after a number, a brace or a closing quote read as a
        // kill (#259's second review).
        && is_a_torn_write(&format!(
            "{}\u{e9}",
            &text[text.rfind('\n').map_or(0, |at| at + 1)..]
        ))
    {
        return Ok(text);
    }
    Err(err)
}

/// A TORN FINAL LINE (#230, amending ruling 4 for the final line only): the
/// tail after the last line break, when it is a strict prefix of an object
/// ([`is_a_torn_write`]). The file's truth ends at the last complete event
/// before it. Any other final line is read and checked like every line --
/// a complete event is an event, anything else is refused -- and a torn
/// line anywhere but last is followed by a line break and is refused where
/// it is.
fn set_aside_a_torn_tail(text: &str) -> (&str, usize) {
    let tail = &text[text.rfind('\n').map_or(0, |at| at + 1)..];
    if is_a_torn_write(tail) {
        (&text[..text.len() - tail.len()], 1)
    } else {
        (text, 0)
    }
}

/// Read a whole log, including the rules that span lines, setting aside a
/// torn final line and counting it.
///
/// # Errors
///
/// [`LogError`] naming the first line no version reads, or the first rule a
/// line breaks -- and when the only line is torn, since then no event is
/// complete.
pub fn read(text: &str) -> Result<Read, LogError> {
    let (complete, torn) = set_aside_a_torn_tail(text);
    if complete.is_empty() && torn == 1 {
        return Err(LogError {
            line: 1,
            why: "the only line is torn: no event in the log is complete".to_owned(),
        });
    }
    Ok(Read {
        lines: parse_complete(complete)?,
        torn,
    })
}

/// Read a whole log, including the rules that span lines: [`read`]'s
/// complete events, a torn final line set aside.
///
/// # Errors
///
/// As [`read`].
pub fn parse(text: &str) -> Result<Vec<Line>, LogError> {
    read(text).map(|read| read.lines)
}

fn parse_complete(text: &str) -> Result<Vec<Line>, LogError> {
    let document = LogParser::parse(Rule::log_document, text).map_err(|err| {
        let line = match err.line_col {
            pest::error::LineColLocation::Pos((line, _))
            | pest::error::LineColLocation::Span((line, _), _) => line,
        };
        let written = text.split('\n').nth(line.saturating_sub(1)).unwrap_or("");
        let why = if written.is_empty() {
            "a blank line: one event per line and nothing between them"
        } else if written.starts_with([' ', '\t']) {
            "an indented line: each event starts its line"
        } else {
            "not one event per line"
        };
        LogError {
            line,
            why: why.to_owned(),
        }
    })?;
    let mut lines = Vec::new();
    let log_lines = document
        .flatten()
        .filter(|pair| pair.as_rule() == Rule::log_line);
    for (index, pair) in log_lines.enumerate() {
        let object = pair
            .into_inner()
            .find(|inner| inner.as_rule() == Rule::object)
            .map_or("", |object| object.as_str());
        lines.push(line(object).map_err(|why| LogError {
            line: index + 1,
            why,
        })?);
    }
    check(&lines)?;
    Ok(lines)
}

/// Write one line: sorted keys, no raw line break, so one event is one line.
#[must_use]
pub fn render(line: &Line) -> String {
    let mut out = String::new();
    json::render(&to_value(line), &mut out);
    out
}

/// Parse `source` and project it: every line, as it reads back. What
/// `diet check-log` prints and the conformance corpus pins.
///
/// # Errors
///
/// When `source` is not a log this reader reads.
pub fn project(source: &str) -> Result<Value, String> {
    read(source)
        .map(|read| {
            Value::Object(BTreeMap::from([
                (
                    "events".to_owned(),
                    Value::Array(read.lines.iter().map(to_value).collect()),
                ),
                (
                    "torn".to_owned(),
                    Value::Integer(i64::try_from(read.torn).unwrap_or(i64::MAX)),
                ),
            ]))
        })
        .map_err(|err| err.to_string())
}

// ---------------------------------------------------------------------------
// the rules that span lines
// ---------------------------------------------------------------------------

/// The first line is `session.start`: it carries the version, so a reader
/// knows what it is reading before it reads anything else. The version it
/// declares.
fn begins_with_the_session(lines: &[Line]) -> Result<i64, LogError> {
    if let Some(Event::SessionStart { version, .. }) = lines.first().map(|line| &line.event) {
        Ok(*version)
    } else {
        Err(LogError {
            line: 1,
            why: "the first line is not `session.start`".to_owned(),
        })
    }
}

/// Q4: an `idle.gap` is emitted once per gap, and references the
/// `turn.settled` that opened it -- the latest one.
fn gap_once(
    opened_by: u64,
    settlings: &BTreeSet<u64>,
    latest: Option<u64>,
    gapped: &mut BTreeSet<u64>,
) -> Result<(), String> {
    if !settlings.contains(&opened_by) {
        return Err(format!("seq {opened_by} is not an earlier `turn.settled`"));
    }
    if latest != Some(opened_by) {
        return Err(format!("seq {opened_by} is not the latest `turn.settled`"));
    }
    if !gapped.insert(opened_by) {
        return Err(format!("a second `idle.gap` opened by seq {opened_by}"));
    }
    Ok(())
}

/// A `seam` (v6, #493): after the latest turn, settled, while the session
/// awaits and no fork is unsettled.
fn seam_at(
    at_turn: u32,
    turns: u32,
    settled: &BTreeSet<u32>,
    state: State,
    forks: &Forks,
) -> Result<(), String> {
    if at_turn != turns {
        return Err(format!(
            "a seam at turn {at_turn} where the latest is {turns}: a seam follows the latest turn"
        ));
    }
    if turns > 0 && !settled.contains(&turns) {
        return Err(format!(
            "a seam at turn {turns}, which has not settled: a seam never refills under a turn"
        ));
    }
    if state != State::Awaiting {
        return Err(format!(
            "a seam while the state is `{}`: a seam is declared while the session awaits",
            state.tag()
        ));
    }
    if let Some(fork) = forks
        .of_turn
        .keys()
        .find(|fork| !forks.settled.contains_key(fork))
    {
        return Err(format!(
            "a seam while fork {fork} is unsettled: the fork is cut from the trunk the seam replaces"
        ));
    }
    Ok(())
}

/// What the whole-log rules of #374's forks (v5) carry from line to line.
#[derive(Default)]
struct Forks {
    /// The turns settled `final`.
    finals: BTreeSet<u32>,
    /// The trunk requests that have a `response`.
    answered: BTreeSet<u64>,
    /// Each fork's `seq`, to the turn it follows.
    of_turn: BTreeMap<u64, u32>,
    /// Each settled fork's `seq`, to how it ended.
    settled: BTreeMap<u64, ForkOutcome>,
    /// The turn the latest seam followed (v6, #493): a fork of it, or of any
    /// turn before it, forks a warm tail the seam replaced.
    seamed: Option<u32>,
}

impl Forks {
    /// A `fork` (#374): on the `interview` lane, after the latest turn
    /// settled `final` and before the next `ask`, at that turn's answered
    /// trunk request, and the only fork in its gap.
    fn fork(
        &mut self,
        seq: u64,
        (lane, of_turn, at): (Lane, u32, u64),
        turns: u32,
        trunk: &BTreeMap<u32, u64>,
    ) -> Result<(), String> {
        if lane != Lane::Interview {
            return Err(format!(
                "a fork on the `{}` lane: a fork's call is made on `interview`",
                lane.tag()
            ));
        }
        if of_turn != turns || !self.finals.contains(&of_turn) {
            return Err(format!(
                "a fork of turn {of_turn} outside its gap: a fork follows the latest turn \
                 (here {turns}) once it settled `final`, before the next `ask`"
            ));
        }
        if trunk.get(&of_turn) != Some(&at) || !self.answered.contains(&at) {
            return Err(format!(
                "a fork at seq {at}, which is not turn {of_turn}'s answered trunk `request`"
            ));
        }
        if self.seamed.is_some_and(|seamed| of_turn <= seamed) {
            return Err(format!(
                "a fork of turn {of_turn} after a seam that followed it: the seam replaced the \
                 trunk the fork would be cut from"
            ));
        }
        if self.of_turn.values().any(|turn| *turn == of_turn) {
            return Err(format!(
                "a second fork in the gap after turn {of_turn}: at most one fork per gap"
            ));
        }
        self.of_turn.insert(seq, of_turn);
        Ok(())
    }

    /// A `request`'s `fork` (#374): an `interview` request names an earlier
    /// fork not yet settled; a trunk request names none.
    fn request(&self, lane: Lane, fork: Option<u64>) -> Result<(), String> {
        let Some(fork) = fork else {
            if lane == Lane::Interview {
                return Err(
                    "an `interview` request carries no `fork`: it names the fork it is the \
                     call of"
                        .to_owned(),
                );
            }
            return Ok(());
        };
        if lane == Lane::Trunk {
            return Err(format!(
                "a trunk request carries `fork` {fork}: only an `interview` request is a fork's"
            ));
        }
        if !self.of_turn.contains_key(&fork) {
            return Err(format!("`fork` {fork} is not an earlier `fork`"));
        }
        if self.settled.contains_key(&fork) {
            return Err(format!("`fork` {fork} is already settled"));
        }
        Ok(())
    }

    /// A `fork.settled` (#374): once per fork, after it.
    fn settle(&mut self, fork: u64, outcome: ForkOutcome) -> Result<(), String> {
        if !self.of_turn.contains_key(&fork) {
            return Err(format!("`fork` {fork} is not an earlier `fork`"));
        }
        if self.settled.insert(fork, outcome).is_some() {
            return Err(format!("fork {fork} settled twice"));
        }
        Ok(())
    }

    /// A `patch` (#374): it cites a fork already settled `value`.
    fn patch(&self, fork: u64) -> Result<(), String> {
        match self.settled.get(&fork) {
            Some(outcome) if *outcome != ForkOutcome::Value => Err(format!(
                "a patch of fork {fork}, which settled `{}`: only a fork settled `value` patches",
                outcome.tag()
            )),
            Some(_) => Ok(()),
            None => Err(format!(
                "a patch of fork {fork}, which is not an earlier settled `fork`"
            )),
        }
    }
}

/// A request ends once: a `response`, a `cancelled` or a `request.failed`
/// (the module's own rule, "a cancelled call is `cancelled`, never a
/// `response`"; found by #137's review).
fn ends_once(event: &Event, request: u64, ended: &mut BTreeSet<u64>) -> Result<(), String> {
    if ended.insert(request) {
        return Ok(());
    }
    let kind = match event {
        Event::Response { .. } => Kind::Response,
        Event::Cancelled { .. } => Kind::Cancelled,
        _ => Kind::RequestFailed,
    };
    Err(format!(
        "a `{}` for request {request}, which has already ended",
        kind.tag()
    ))
}

/// A line about a call cites an earlier `request`. A `delta` or a
/// `progress` cites one still in flight; a `response`, `cancelled` or
/// `request.failed` ends it, once.
fn cites(event: &Event, requests: &BTreeSet<u64>, ended: &mut BTreeSet<u64>) -> Result<(), String> {
    let request = match event {
        Event::Delta { request, .. }
        | Event::Progress { request, .. }
        | Event::Cancelled { request, .. }
        | Event::RequestFailed { request, .. } => *request,
        Event::Response { to_request, .. } => *to_request,
        _ => return Ok(()),
    };
    if !requests.contains(&request) {
        return Err(format!("seq {request} is not an earlier `request`"));
    }
    match event {
        Event::Delta { .. } | Event::Progress { .. } => {
            if ended.contains(&request) {
                let kind = if matches!(event, Event::Delta { .. }) {
                    Kind::Delta
                } else {
                    Kind::Progress
                };
                return Err(format!(
                    "a `{}` for request {request}, which has already ended",
                    kind.tag()
                ));
            }
            Ok(())
        }
        _ => ends_once(event, request, ended),
    }
}

/// A `tool_call` cites an earlier `request`, as [`cites`] has it, but does
/// not end it: the call's line is written at its outcome, which can come
/// after its request's `response` (v3).
fn called_from(cited: u64, requests: &BTreeSet<u64>) -> Result<(), String> {
    if !requests.contains(&cited) {
        return Err(format!(
            "a `tool_call` cites seq {cited}, which is not an earlier `request`"
        ));
    }
    Ok(())
}

/// A `tool_call` names its request's turn, and a call -- its request and
/// its `id` -- leaves at most one line (v3, #297 Q1: every call the model
/// made leaves exactly one line; that each streamed call leaves one is the
/// writer's obligation, not read here).
fn called_once(
    request: u64,
    turn: u32,
    id: &str,
    request_turns: &BTreeMap<u64, u32>,
    calls: &mut BTreeSet<(u64, String)>,
) -> Result<(), String> {
    if let Some(&asked) = request_turns.get(&request)
        && asked != turn
    {
        return Err(format!(
            "a `tool_call` names turn {turn}, and its request {request} is turn {asked}"
        ));
    }
    if !calls.insert((request, id.to_owned())) {
        return Err(format!(
            "a second `tool_call` for call `{id}` of request {request}: a call leaves one line"
        ));
    }
    Ok(())
}

/// A served session -- its `session.start` carries the substrate claim --
/// records each call's confinement: `unrecorded` is a placed replay's word,
/// allowed only where the claim is absent, and `isolation` and `network` are
/// each checked on their own (#297, ruled at 5975651100).
fn confinement_recorded(
    claimed: bool,
    isolation: Option<Isolation>,
    network: Option<Network>,
) -> Result<(), String> {
    if claimed && isolation == Some(Isolation::Unrecorded) {
        return Err(
            "a `tool_call` whose `isolation` is `unrecorded`, in a session whose \
             `session.start` carries the substrate claim"
                .to_owned(),
        );
    }
    if claimed && network == Some(Network::Unrecorded) {
        return Err(
            "a `tool_call` whose `network` is `unrecorded`, in a session whose \
             `session.start` carries the substrate claim"
                .to_owned(),
        );
    }
    Ok(())
}

/// A turn settled `capped` stopped at its output cap: its trunk call's
/// `response` -- the turn's latest trunk `request`'s -- says so with
/// `capped: true` (v3, #290).
fn settled_capped(
    turn: u32,
    trunk: &BTreeMap<u32, u64>,
    capped: &BTreeSet<u64>,
) -> Result<(), String> {
    let response_capped = trunk
        .get(&turn)
        .is_some_and(|request| capped.contains(request));
    if !response_capped {
        return Err(format!(
            "turn {turn} settled `capped`, and its trunk `response` does not carry \
             `capped: true`"
        ));
    }
    Ok(())
}

/// `seq` counts from 0 without a gap, and `t` never goes back.
fn in_order(line: &Line, index: usize, last_t: u64) -> Result<(), String> {
    if line.seq != index as u64 {
        return Err(format!(
            "`seq` is {}, where the line's position is {index}",
            line.seq
        ));
    }
    if line.t < last_t {
        return Err(format!("`t` went back from {last_t} to {}", line.t));
    }
    Ok(())
}

/// What in `line` arrived after the version the log declares, if anything:
/// its kind, a key it carries, or a tag it holds -- each read off
/// [`introduced`], [`schema`]'s `since` and [`tag_introduced`], the one
/// declaration of what arrived when.
fn beyond(line: &Line, declared: i64) -> Option<String> {
    let Value::Object(object) = to_value(line) else {
        return None;
    };
    let kind = match object.get("kind") {
        Some(Value::String(tag)) => Kind::from_tag(tag)?,
        _ => return None,
    };
    let arrived = |since: i64, what: String| {
        (since > declared).then(|| {
            format!("{what}, which arrived in v{since}, and this log declares v{declared}")
        })
    };
    if let Some(why) = arrived(introduced(kind), format!("a `{}` line", kind.tag())) {
        return Some(why);
    }
    for field in schema(kind) {
        let Some(value) = object.get(field.key) else {
            continue;
        };
        if let Some(why) = arrived(
            field.since,
            format!("`{}` carries `{}`", kind.tag(), field.key),
        ) {
            return Some(why);
        }
        if let (Holds::Tag(tags), Value::String(tag)) = (field.holds, value)
            && let Some(why) = arrived(
                tag_introduced(tags, tag),
                format!("`{}`'s `{}` is `{tag}`", kind.tag(), field.key),
            )
        {
            return Some(why);
        }
        // One object down: a tag that arrived after its object did (the
        // approval scope `off`, v7).
        if let (Some(inner_fields), Value::Object(inner)) = (object_fields(field.holds), value) {
            for nested in inner_fields {
                if let (Holds::Tag(tags), Some(Value::String(tag))) =
                    (nested.holds, inner.get(nested.key))
                    && let Some(why) = arrived(
                        tag_introduced(tags, tag),
                        format!(
                            "`{}`'s `{}.{}` is `{tag}`",
                            kind.tag(),
                            field.key,
                            nested.key
                        ),
                    )
                {
                    return Some(why);
                }
            }
        }
    }
    None
}

/// Every rule that no single line can break alone.
#[allow(clippy::too_many_lines)]
fn check(lines: &[Line]) -> Result<(), LogError> {
    let at = |index: usize, why: String| LogError {
        line: index + 1,
        why,
    };
    let declared = begins_with_the_session(lines)?;
    let mut state = State::Awaiting;
    let mut turns = 0_u32;
    let mut settled = BTreeSet::new();
    let mut requests = BTreeSet::new();
    let mut settlings = BTreeSet::new();
    let mut ended = BTreeSet::new();
    let mut latest_settling = None;
    let mut gapped = BTreeSet::new();
    let mut trunk = BTreeMap::new();
    let mut capped = BTreeSet::new();
    let mut request_turns = BTreeMap::new();
    let mut calls = BTreeSet::new();
    let mut forks = Forks::default();
    let mut claimed = false;
    let mut last_t = 0_u64;
    for (index, line) in lines.iter().enumerate() {
        in_order(line, index, last_t).map_err(|why| at(index, why))?;
        if let Some(why) = beyond(line, declared) {
            return Err(at(index, why));
        }
        last_t = line.t;
        match &line.event {
            Event::SessionStart { .. } if index > 0 => {
                return Err(at(index, "a second `session.start`".to_owned()));
            }
            Event::SessionStart { claim, .. } => {
                // v7 replaced the claim's two older keys with `served` (#509).
                if declared >= 7
                    && let Some(SubstrateClaim {
                        engine: ClaimedEngine::Checked { .. },
                        ..
                    }) = claim
                {
                    return Err(at(
                        index,
                        format!(
                            "`session.start` carries `engine_build` and `engine_identity`, \
                             which v7 replaced with `served`, and this log declares v{declared}"
                        ),
                    ));
                }
                claimed = claim.is_some();
            }
            Event::Settlement { from, to } => {
                if *from != state {
                    return Err(at(
                        index,
                        format!(
                            "a settlement from `{}` where the state is `{}`",
                            from.tag(),
                            state.tag()
                        ),
                    ));
                }
                state = *to;
            }
            Event::Ask { turn, .. } => {
                if *turn != turns + 1 {
                    return Err(at(index, format!("turn {turn} follows turn {turns}")));
                }
                turns = *turn;
            }
            Event::Request {
                turn, lane, fork, ..
            } => {
                if *turn == 0 || *turn != turns {
                    return Err(at(
                        index,
                        format!("a request for turn {turn} where the latest is {turns}"),
                    ));
                }
                forks.request(*lane, *fork).map_err(|why| at(index, why))?;
                requests.insert(line.seq);
                request_turns.insert(line.seq, *turn);
                if *lane == Lane::Trunk {
                    trunk.insert(*turn, line.seq);
                }
            }
            Event::StopAsked { turn } if *turn == 0 || *turn > turns => {
                return Err(at(index, format!("a stop for turn {turn}, never asked")));
            }
            Event::TurnSettled { turn, reason } => {
                if *turn == 0 || *turn > turns {
                    return Err(at(index, format!("turn {turn} settled, never asked")));
                }
                if !settled.insert(*turn) {
                    return Err(at(index, format!("turn {turn} settled twice")));
                }
                if *reason == SettleReason::Capped {
                    settled_capped(*turn, &trunk, &capped).map_err(|why| at(index, why))?;
                }
                if *reason == SettleReason::Final {
                    forks.finals.insert(*turn);
                }
                settlings.insert(line.seq);
                latest_settling = Some(line.seq);
            }
            Event::IdleGap { opened_by, .. } => {
                gap_once(*opened_by, &settlings, latest_settling, &mut gapped)
                    .map_err(|why| at(index, why))?;
            }
            event @ (Event::Delta { .. }
            | Event::Progress { .. }
            | Event::Response { .. }
            | Event::Cancelled { .. }
            | Event::RequestFailed { .. }) => {
                cites(event, &requests, &mut ended).map_err(|why| at(index, why))?;
                if let Event::Response {
                    to_request,
                    capped: Some(true),
                    ..
                } = event
                {
                    capped.insert(*to_request);
                }
                if let Event::Response { to_request, .. } = event
                    && trunk.values().any(|request| request == to_request)
                {
                    forks.answered.insert(*to_request);
                }
            }
            Event::Fork {
                lane,
                of_turn,
                at: forked_at,
                ..
            } => forks
                .fork(line.seq, (*lane, *of_turn, *forked_at), turns, &trunk)
                .map_err(|why| at(index, why))?,
            Event::ForkSettled { fork, outcome } => forks
                .settle(*fork, *outcome)
                .map_err(|why| at(index, why))?,
            Event::Patch { fork, .. } => forks.patch(*fork).map_err(|why| at(index, why))?,
            Event::Seam { at_turn, .. } => {
                seam_at(*at_turn, turns, &settled, state, &forks).map_err(|why| at(index, why))?;
                forks.seamed = Some(*at_turn);
            }
            Event::ToolCall {
                request,
                turn,
                id,
                isolation,
                network,
                argv,
                cwd,
                ..
            } => {
                confinement_recorded(claimed, *isolation, *network)
                    .map_err(|why| at(index, why))?;
                cwd_with_argv(declared, argv.is_some(), cwd.is_some())
                    .map_err(|why| at(index, why))?;
                called_from(*request, &requests).map_err(|why| at(index, why))?;
                called_once(*request, *turn, id, &request_turns, &mut calls)
                    .map_err(|why| at(index, why))?;
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// one line, both ways
// ---------------------------------------------------------------------------

/// The keys every line carries.
const COMMON: &[&str] = &["seq", "t", "kind"];

/// A line from its object: every key it must have, none it may not.
///
/// One arm per kind, the mirror of [`to_value`]'s.
#[allow(clippy::too_many_lines)]
fn from_object(object: &BTreeMap<String, Value>) -> Result<Line, String> {
    let fields = Fields(object);
    let kind_tag = fields.string("kind")?;
    let kind = Kind::from_tag(&kind_tag).ok_or_else(|| format!("unknown kind `{kind_tag}`"))?;
    let (required, optional) = keys(kind);
    for key in object.keys() {
        let known = COMMON.contains(&key.as_str())
            || required.contains(&key.as_str())
            || optional.contains(&key.as_str());
        if !known {
            return Err(format!("`{kind_tag}` carries no `{key}`"));
        }
    }
    let together = all_or_none(kind);
    let carried: Vec<&str> = together
        .iter()
        .copied()
        .filter(|key| object.contains_key(*key))
        .collect();
    if !carried.is_empty() && carried.len() < together.len() {
        let missing: Vec<String> = together
            .iter()
            .filter(|key| !object.contains_key(**key))
            .map(|key| format!("`{key}`"))
            .collect();
        return Err(format!(
            "`{kind_tag}` carries `{}` without {}: its keys {} come together or not at all",
            carried[0],
            missing.join(" and "),
            together.join(", ")
        ));
    }
    let event = match kind {
        Kind::SessionStart => {
            let version = fields.integer("version")?;
            if !READS.contains(&version) {
                return Err(format!(
                    "version {version}, where this reader reads {READS:?}"
                ));
            }
            Event::SessionStart {
                version,
                opened: fields.count("opened")?,
                model: fields.string("model")?,
                head: fields.head("head")?,
                serving: match object.get("serving") {
                    None => None,
                    Some(_) => Some(fields.serving("serving")?),
                },
                // Built only when its two keys are carried ([`all_or_none`]),
                // with exactly one form of what it says of the engine.
                claim: fields
                    .claimed_engine(together.iter().all(|key| object.contains_key(*key)))?
                    .map(|engine| -> Result<SubstrateClaim, String> {
                        Ok(SubstrateClaim {
                            substrate: fields.string("substrate")?,
                            registry_sha256: fields.digest("registry_sha256")?,
                            engine,
                        })
                    })
                    .transpose()?,
                provenance: fields.optional_tag("provenance", Provenance::from_tag)?,
                tools: match fields.optional_strings("tools")? {
                    Some(tools) if tools.is_empty() => {
                        return Err(
                            "`tools` is empty: a session that declared no tool carries no \
                                    `tools`"
                                .to_owned(),
                        );
                    }
                    tools => tools,
                },
                template_kwargs: match object.get("template_kwargs") {
                    None => None,
                    Some(_) => Some(fields.template_kwargs("template_kwargs")?),
                },
                unsent: if object.contains_key("unsent") {
                    Some(fields.unsent("unsent")?)
                } else {
                    None
                },
                approvals_off: match fields.optional_flag("approvals_off")? {
                    Some(false) => {
                        return Err(
                            "`approvals_off` is `false`: only `true` is written, and absent is \
                             the gate"
                                .to_owned(),
                        );
                    }
                    off => off,
                },
                fork_delivery: fields.optional_tag("fork_delivery", ForkDelivery::from_tag)?,
                reasoning_effort_default: match object.get("reasoning_effort_default") {
                    None => None,
                    Some(_) => Some(fields.string("reasoning_effort_default")?),
                },
                instruction_files: fields.instruction_files("instruction_files")?,
                tool_output: tool_output(&fields)?,
                phases: fields.optional_strings("phases")?,
                phase_transitions: match object.get("phase_transitions") {
                    None => None,
                    Some(Value::Array(moves)) => Some(
                        moves
                            .iter()
                            .enumerate()
                            .map(|(index, entry)| match entry {
                                Value::Object(entry) => Fields(entry)
                                    .phase_move_of()
                                    .map_err(|why| format!("`phase_transitions[{index}]`: {why}")),
                                _ => Err(format!("`phase_transitions[{index}]` is not an object")),
                            })
                            .collect::<Result<Vec<_>, _>>()?,
                    ),
                    Some(_) => return Err("`phase_transitions` is not a list".to_owned()),
                },
                opening_phase: match object.get("opening_phase") {
                    None => None,
                    Some(_) => Some(fields.string("opening_phase")?),
                },
            }
        }
        Kind::Ask => Event::Ask {
            turn: fields.turn("turn")?,
            text: fields.string("text")?,
            scoping: match fields.optional_flag("scoping")? {
                Some(false) => {
                    return Err(
                        "`scoping` is `false`: only the operator's mark, `true`, is \
                                written (#374)"
                            .to_owned(),
                    );
                }
                scoping => scoping,
            },
            files: fields.optional_files("files")?,
        },
        Kind::Settlement => Event::Settlement {
            from: fields.tag("from", State::from_tag)?,
            to: fields.tag("to", State::from_tag)?,
        },
        Kind::Request => Event::Request {
            turn: fields.turn("turn")?,
            lane: fields.tag("lane", Lane::from_tag)?,
            head_sha256: fields.optional_digest("head_sha256")?,
            fork: fields.optional_count("fork")?,
            max_tokens: fields.optional_count("max_tokens")?,
        },
        Kind::Refused => Event::Refused {
            command: fields.tag("command", Command::from_tag)?,
            because: fields.tag("because", Refusal::from_tag)?,
            during: fields.tag("during", State::from_tag)?,
        },
        Kind::Delta => Event::Delta {
            request: fields.count("request")?,
            piece: {
                let pieces: Vec<&str> = exactly_one(kind)
                    .iter()
                    .copied()
                    .filter(|key| object.contains_key(*key))
                    .collect();
                match (
                    object.contains_key("text"),
                    object.contains_key("reasoning"),
                    object.contains_key("tool_call"),
                ) {
                    (true, false, false) => Piece::Text(fields.string("text")?),
                    (false, true, false) => Piece::Reasoning(fields.string("reasoning")?),
                    (false, false, true) => fields.tool_call_piece("tool_call")?,
                    (false, false, false) => {
                        return Err(
                            "a delta carries none of `text`, `reasoning` and `tool_call`"
                                .to_owned(),
                        );
                    }
                    _ => {
                        let named: Vec<String> =
                            pieces.iter().map(|key| format!("`{key}`")).collect();
                        return Err(format!(
                            "a delta carries {}: exactly one of `text`, `reasoning` and \
                             `tool_call`",
                            named.join(" and ")
                        ));
                    }
                }
            },
        },
        Kind::StopAsked => Event::StopAsked {
            turn: fields.turn("turn")?,
        },
        Kind::Response => Event::Response {
            to_request: fields.count("to_request")?,
            text: fields.string("text")?,
            finish_reason: fields.optional_string("finish_reason")?,
            reasoning: fields.optional_string("reasoning")?,
            timings: match object.get("timings") {
                None => None,
                Some(_) => Some(fields.timings("timings")?),
            },
            usage: match (object.get("usage"), object.get("timings")) {
                (None, _) => None,
                (Some(_), None) => Some(fields.usage("usage")?),
                (Some(_), Some(_)) => {
                    return Err("a response carries both `timings` and `usage`: `usage` is \
                                carried only for a server that reports no timings (#157)"
                        .to_owned());
                }
            },
            capped: fields.optional_flag("capped")?,
        },
        Kind::Cancelled => Event::Cancelled {
            request: fields.count("request")?,
            partial: fields.string("partial")?,
            reasoning: fields.optional_string("reasoning")?,
        },
        Kind::RequestFailed => Event::RequestFailed {
            request: fields.count("request")?,
            reason: fields.tag("reason", FailReason::from_tag)?,
            message: fields.string("message")?,
            status: match object.get("status") {
                None => None,
                Some(_) => Some(
                    u16::try_from(fields.count("status")?)
                        .map_err(|_| "`status` is not an HTTP status".to_owned())?,
                ),
            },
            partial: fields.optional_string("partial")?,
        },
        Kind::TurnSettled => Event::TurnSettled {
            turn: fields.turn("turn")?,
            reason: fields.tag("reason", SettleReason::from_tag)?,
        },
        Kind::IdleGap => Event::IdleGap {
            opened_by: fields.count("opened_by")?,
            notice: fields.count("notice")?,
            read: fields.count("read")?,
            compose: fields.count("compose")?,
            away: fields.count("away")?,
            blocked: fields.count("blocked")?,
            ended_by: fields.tag("ended_by", GapEnd::from_tag)?,
        },
        Kind::Progress => Event::Progress {
            request: fields.count("request")?,
            total: fields.count("total")?,
            cache: fields.count("cache")?,
            processed: fields.count("processed")?,
            time_ms: fields.count("time_ms")?,
        },
        Kind::ToolCall => {
            let outcome = fields.tag("outcome", ToolOutcome::from_tag)?;
            let isolation = fields.optional_tag("isolation", Isolation::from_tag)?;
            let network = fields.optional_tag("network", Network::from_tag)?;
            let reason = fields.optional_tag("reason", ToolRefusal::from_tag)?;
            let unrecorded = isolation == Some(Isolation::Unrecorded);
            let profiled = matches!(isolation, Some(Isolation::Sandbox | Isolation::Vm));
            let name = fields.string("name")?;
            fits_its_outcome(object, &name, outcome, unrecorded, profiled)?;
            if name == "bash" {
                argv_if_it_parsed(object, reason)?;
            }
            let approval = fields.approval("approval")?;
            if let Some(approval) = &approval {
                decided_as_its_scope_says(approval, fields.count("t")?)?;
            }
            Event::ToolCall {
                request: fields.count("request")?,
                turn: fields.turn("turn")?,
                id: fields.string("id")?,
                name,
                arguments: fields.string("arguments")?,
                outcome,
                argv: fields.optional_strings("argv")?,
                cwd: fields.optional_working_directory("cwd")?,
                confined: fields.optional_strings("confined")?,
                isolation,
                network,
                exit: fields.optional_count("exit")?,
                reason,
                policy: fields.optional_digest("policy")?,
                stdout: fields.output("stdout", "stdout_bytes")?,
                stderr: fields.output("stderr", "stderr_bytes")?,
                approval,
                files: fields.optional_files("files")?,
                shown: fields.optional_string("shown")?,
                recovered_from: fields.optional_string("recovered_from")?,
            }
        }
        Kind::Fork => Event::Fork {
            lane: fields.tag("lane", Lane::from_tag)?,
            of_turn: fields.turn("of_turn")?,
            at: fields.count("at")?,
            why: fields.tag("why", Warrant::from_tag)?,
            question: fields.string("question")?,
        },
        Kind::ForkSettled => Event::ForkSettled {
            fork: fields.count("fork")?,
            outcome: fields.tag("outcome", ForkOutcome::from_tag)?,
        },
        Kind::Patch => {
            let op = fields.tag("op", PatchOp::from_tag)?;
            let supersedes = fields.optional_string("supersedes")?;
            if (op == PatchOp::Supersede) != supersedes.is_some() {
                return Err(format!(
                    "a patch whose `op` is `{}` {} `supersedes`: a patch names the entry it \
                     replaces exactly when its `op` is `supersede`",
                    op.tag(),
                    if supersedes.is_some() {
                        "carries"
                    } else {
                        "lacks"
                    }
                ));
            }
            Event::Patch {
                fork: fields.count("fork")?,
                op,
                entry: fields.entry("entry")?,
                supersedes,
            }
        }
        Kind::Delivered => Event::Delivered {
            turn: fields.turn("turn")?,
            framing: fields.tag("framing", Framing::from_tag)?,
            text: fields.string("text")?,
            lines: fields.delivered_lines("lines")?,
        },
        Kind::Seam => Event::Seam {
            at_turn: fields.turn("at_turn")?,
            reason: fields.tag("reason", SeamReason::from_tag)?,
            prefix_hash_before: fields.digest("prefix_hash_before")?,
            prefix_hash_after: fields.digest("prefix_hash_after")?,
            frame: fields.string("frame")?,
            render: fields.string("render")?,
            carried_entries: fields.count("carried_entries")?,
            carried_turns: fields.count("carried_turns")?,
            tail_tokens: fields.optional_count("tail_tokens")?,
            carried_tokens: fields.optional_count("carried_tokens")?,
            phase: match object.get("phase") {
                None => None,
                Some(_) => Some(fields.phase_move("phase")?),
            },
            tool_outputs: fields.optional_tag("tool_outputs", SeamToolOutputs::from_tag)?,
            outputs: fields.optional_string("outputs")?,
            carried_outputs: fields.optional_count("carried_outputs")?,
            carried_output_bytes: fields.optional_count("carried_output_bytes")?,
        },
    };
    Ok(Line {
        seq: fields.count("seq")?,
        t: fields.count("t")?,
        event,
    })
}

/// Each output stream's text and the key that carries its length in bytes.
pub const STREAMS: &[(&str, &str)] = &[("stdout", "stdout_bytes"), ("stderr", "stderr_bytes")];

/// The keys that say what a command ran as and under what: only a `bash`
/// call carries them (#297, ruled from #300's build). Any other tool --
/// `read`, `edit` -- ran no command, and an argv for it would be invented.
/// `policy` is one of them (ruled at 5975957135): nothing but `bash` ran
/// under a profile, so nothing else has a profile to name.
pub const EXEC: &[&str] = &["argv", "confined", "isolation", "network", "policy"];

/// The keys a `tool_call` must carry, then the keys it may not, for each
/// outcome (v3, ruled on #297): what ran says what ran it and what it
/// printed; what failed under a policy names the policy; what was refused
/// says why and ran nothing; what was cancelled never finished. `exit` is
/// never required: a signal leaves none. The [`EXEC`] keys required here
/// (`policy` among them, under `command_failed`) are required of a `bash`
/// call only, and forbidden on any other (ruled at 5975957135); `policy` is
/// required, and allowed, only under an `isolation` that names a profile,
/// `sandbox` or `vm` (ruled at #299, 5976386318 point 6); the
/// [`STREAMS`] required here are required of a `bash` call only, and on any
/// other neither required nor refused (ruled at 5975651100): a
/// cancelled `bash` call always says its `isolation` and `network`, and its
/// `confined` only if the command had started. A refused `bash` call's
/// `argv` follows its reason ([`argv_if_it_parsed`]). `approval` (v4) is
/// optional under `ran`, `command_failed` and `cancelled`, on any name, and
/// forbidden under `refused`: a refused call ran under no decision (ruled at
/// 5981588394 (c)). `files` (v4) is optional under `ran` and
/// `command_failed`, on any name, and forbidden under `refused` and
/// `cancelled`: a call that never finished left nothing to record (ruled at
/// 5983588924).
#[must_use]
pub fn outcome_keys(outcome: ToolOutcome) -> (&'static [&'static str], &'static [&'static str]) {
    match outcome {
        ToolOutcome::Ran => (
            &[
                "argv",
                "isolation",
                "network",
                "confined",
                "stdout",
                "stdout_bytes",
                "stderr",
                "stderr_bytes",
            ],
            &["reason", "policy"],
        ),
        ToolOutcome::CommandFailed => (
            &[
                "argv",
                "isolation",
                "network",
                "confined",
                "stdout",
                "stdout_bytes",
                "stderr",
                "stderr_bytes",
                "policy",
            ],
            &["reason"],
        ),
        ToolOutcome::Refused => (
            &["reason"],
            &[
                "confined",
                "isolation",
                "network",
                "exit",
                "policy",
                "stdout",
                "stdout_bytes",
                "stderr",
                "stderr_bytes",
                "approval",
                "files",
            ],
        ),
        ToolOutcome::Cancelled => (
            &["isolation", "network"],
            &[
                "exit",
                "reason",
                "policy",
                "stdout",
                "stdout_bytes",
                "stderr",
                "stderr_bytes",
                "files",
            ],
        ),
    }
}

/// A refused `bash` call carries `argv` exactly when it parsed (#297): one
/// refused `not_allowed` or `max_steps` was read into an argv before it was
/// refused; one refused `unparsable` or `unknown_tool` never was, and an argv
/// for it would be invented. One refused `denylist` or `declined` (v4) was
/// read into an argv too: the denylist matches a parsed command, and the
/// operator was shown one (ratified on #388 at 5982002587).
fn argv_if_it_parsed(
    object: &BTreeMap<String, Value>,
    reason: Option<ToolRefusal>,
) -> Result<(), String> {
    let Some(reason) = reason else {
        return Ok(());
    };
    let parsed = matches!(
        reason,
        ToolRefusal::NotAllowed
            | ToolRefusal::MaxSteps
            | ToolRefusal::Denylist
            | ToolRefusal::Declined
    );
    let has_argv = object.contains_key("argv");
    if parsed && !has_argv {
        return Err(format!(
            "a `bash` call refused `{}` carries no `argv`: it parsed",
            reason.tag()
        ));
    }
    if !parsed && has_argv {
        return Err(format!(
            "a `bash` call refused `{}` carries `argv`: it never parsed",
            reason.tag()
        ));
    }
    Ok(())
}

/// An approval's `decided_at` and `why` fit its scope and its line (v4,
/// ruled at 5981588394 (b) and (d), and at 5982826236): a `once`, `session`
/// or `workspace` approval prompted and an operator decided it, so it says
/// why and when; no prompt decided a `preseeded` one, so it says neither;
/// and a decision comes no later than the line written at the call's
/// outcome.
fn decided_as_its_scope_says(approval: &Approval, t: u64) -> Result<(), String> {
    let scope = approval.scope.tag();
    // Neither a pre-seed nor approvals off was decided by a prompt.
    let preseeded = matches!(
        approval.scope,
        ApprovalScope::Preseeded | ApprovalScope::Off
    );
    let prompted = !preseeded;
    if prompted && approval.why.is_none() {
        return Err(format!(
            "a `{scope}` approval carries no `why`: the call prompted"
        ));
    }
    if !prompted && approval.why.is_some() {
        return Err(format!(
            "a `{scope}` approval carries `why`: no prompt asked"
        ));
    }
    let Some(decided_at) = approval.decided_at else {
        if !preseeded {
            return Err(format!(
                "a `{scope}` approval carries no `decided_at`: an operator decided it"
            ));
        }
        return Ok(());
    };
    if preseeded {
        return Err(format!(
            "a `{scope}` approval carries `decided_at`: no prompt decided it"
        ));
    }
    if decided_at > t {
        return Err(format!(
            "an approval's `decided_at` is {decided_at}, after its line's `t` {t}: a call is \
             decided before its outcome is written"
        ));
    }
    Ok(())
}

/// The version a `tool_call`'s `cwd` arrived in, read off [`schema`].
fn cwd_since() -> i64 {
    schema(Kind::ToolCall)
        .iter()
        .find(|field| field.key == "cwd")
        .map_or(i64::MAX, |field| field.since)
}

/// In a log that declares the version `cwd` arrived in, a `tool_call` that
/// carries `argv` carries `cwd` (v4, ruled at 5982826236). The other
/// direction, no `cwd` without an `argv`, is every line's
/// ([`fits_its_outcome`]); this one is the whole log's, because a v3 line
/// carries an `argv` and could not carry a `cwd`.
fn cwd_with_argv(declared: i64, has_argv: bool, has_cwd: bool) -> Result<(), String> {
    if declared >= cwd_since() && has_argv && !has_cwd {
        return Err(format!(
            "a `tool_call` carries `argv` without `cwd`, in a log that declares v{declared}: \
             a command's argv and its working directory come together"
        ));
    }
    Ok(())
}

/// Whether `text` is a working directory as the runner passes one (v4,
/// ruled at 5982826236): absolute, or the home as a tilde, `~` or `~/...`,
/// never expanded and never another user's (`~user/...`).
fn is_a_working_directory(text: &str) -> bool {
    text.starts_with('/') || text == "~" || text.starts_with("~/")
}

/// Whether `text` is a recorded file's path as 5983588924 has it, relative
/// to the recording's directory: one or more non-empty components, none
/// `.` or `..`, joined by single `/` -- so not empty, not absolute, no
/// trailing `/` and no `//` -- with no `\\` anywhere; and a tilde never
/// expanded to a user, which this reader takes as no leading `~` at all, a
/// home not being the recording's directory (the spelling is this
/// reader's judgement call).
pub(crate) fn is_a_recorded_path(text: &str) -> bool {
    !text.starts_with('~')
        && !text.contains('\\')
        && text
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// Whether `text` is a media type as this reader spells one: `type/subtype`,
/// each side non-empty, one `/`, no whitespace.
pub(crate) fn is_a_media_type(text: &str) -> bool {
    text.split_once('/').is_some_and(|(kind, subtype)| {
        !kind.is_empty() && !subtype.is_empty() && !subtype.contains('/')
    }) && !text.chars().any(char::is_whitespace)
}

/// Whether `text` is one word of an approval's `why`: a lowercase letter,
/// then lowercase letters, digits and `_` (5982826236; the spelling is
/// this reader's choice).
fn is_a_reason_word(text: &str) -> bool {
    let mut chars = text.chars();
    chars.next().is_some_and(|first| first.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// A `tool_call`'s keys fit its outcome ([`outcome_keys`]), and each stream's
/// text and byte count come together or not at all ([`STREAMS`]). Only a
/// call named `bash` carries the [`EXEC`] keys, and only a `bash` call must
/// carry its streams. A call whose `isolation` is `unrecorded` carries no
/// `confined`, whatever its outcome requires (5974732908). `confined` is
/// keyed to `isolation` alone: `network` alone `unrecorded` leaves
/// `confined` as its outcome says (ruled at 5975827372). A `bash` call
/// carries `policy` only when `profiled`, its `isolation` `sandbox` or `vm`,
/// and a `command_failed` one must then (ruled at #299, 5976386318 point 6).
fn fits_its_outcome(
    object: &BTreeMap<String, Value>,
    name: &str,
    outcome: ToolOutcome,
    unrecorded: bool,
    profiled: bool,
) -> Result<(), String> {
    let bash = name == "bash";
    for (text_key, bytes_key) in STREAMS {
        let (has_text, has_bytes) = (
            object.contains_key(*text_key),
            object.contains_key(*bytes_key),
        );
        if has_text != has_bytes {
            let (carried, missing) = if has_text {
                (text_key, bytes_key)
            } else {
                (bytes_key, text_key)
            };
            return Err(format!(
                "a `tool_call` carries `{carried}` without `{missing}`: a stream's text and \
                 its byte count come together"
            ));
        }
    }
    if unrecorded && object.contains_key("confined") {
        return Err(
            "a `tool_call` whose `isolation` is `unrecorded` carries `confined`".to_owned(),
        );
    }
    let (required, forbidden) = outcome_keys(outcome);
    for needed in required {
        if unrecorded && *needed == "confined" {
            continue;
        }
        // `policy` names the profile that confined a command, so a call no
        // profile confined -- `isolation` `none` or `unrecorded` -- has none
        // to name (ruled at #299, 5976386318 point 6).
        if !profiled && *needed == "policy" {
            continue;
        }
        if !bash && EXEC.contains(needed) {
            continue;
        }
        if !bash
            && STREAMS
                .iter()
                .any(|(text, bytes)| needed == text || needed == bytes)
        {
            continue;
        }
        if !object.contains_key(*needed) {
            return Err(format!(
                "a `tool_call` whose outcome is `{}` carries no `{needed}`",
                outcome.tag()
            ));
        }
    }
    for barred in forbidden {
        if object.contains_key(*barred) {
            return Err(format!(
                "a `tool_call` whose outcome is `{}` carries `{barred}`",
                outcome.tag()
            ));
        }
    }
    if bash && !profiled && object.contains_key("policy") {
        return Err(format!(
            "a `bash` call whose outcome is `{}` carries `policy` under no profile: only \
             `isolation` `sandbox` or `vm` confines a command under one",
            outcome.tag()
        ));
    }
    for exec in EXEC {
        if !bash && object.contains_key(*exec) {
            return Err(format!(
                "a `tool_call` named `{name}` carries `{exec}`: only a `bash` call ran a command"
            ));
        }
    }
    // `cwd` follows `argv` (v4, ruled at 5982826236): forbidden wherever
    // `argv` is absent, so wherever `argv` is forbidden.
    if object.contains_key("cwd") && !object.contains_key("argv") {
        return Err(
            "a `tool_call` carries `cwd` without `argv`: a working directory is a command's"
                .to_owned(),
        );
    }
    Ok(())
}

/// The keys a kind must carry, then the keys it may: read off [`schema`],
/// the one place a kind's keys are declared.
fn keys(kind: Kind) -> (Vec<&'static str>, Vec<&'static str>) {
    let fields = schema(kind);
    (
        fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.key)
            .collect(),
        fields
            .iter()
            .filter(|f| !f.required)
            .map(|f| f.key)
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// the schema, as data
// ---------------------------------------------------------------------------

/// A closed vocabulary a key holds one tag of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tags {
    /// [`State`].
    State,
    /// [`Lane`].
    Lane,
    /// [`Command`].
    Command,
    /// [`Refusal`].
    Refusal,
    /// [`FailReason`].
    FailReason,
    /// [`SettleReason`].
    SettleReason,
    /// [`GapEnd`].
    GapEnd,
    /// [`Role`].
    Role,
    /// [`ToolOutcome`] (v3).
    ToolOutcome,
    /// [`Isolation`] (v3).
    Isolation,
    /// [`Network`] (v3).
    Network,
    /// [`ToolRefusal`] (v3).
    ToolRefusal,
    /// [`EngineIdentity`] (v3).
    EngineIdentity,
    /// [`FieldProvenance`] (v7).
    FieldProvenance,
    /// [`Provenance`] (v3).
    Provenance,
    /// [`ApprovalScope`] (v4).
    ApprovalScope,
    /// [`Warrant`] (v5).
    Warrant,
    /// [`ForkOutcome`] (v5).
    ForkOutcome,
    /// [`PatchOp`] (v5).
    PatchOp,
    /// [`SeamReason`] (v6).
    SeamReason,
    /// [`Framing`] (v7).
    Framing,
    /// [`ForkDelivery`] (v7).
    ForkDelivery,
    /// [`ToolOutputState`] (v7).
    ToolOutputState,
    /// [`SeamToolOutputs`] (v7).
    SeamToolOutputs,
}

impl Tags {
    /// Every vocabulary a key can hold.
    pub const ALL: &'static [Self] = &[
        Self::State,
        Self::Lane,
        Self::Command,
        Self::Refusal,
        Self::FailReason,
        Self::SettleReason,
        Self::GapEnd,
        Self::Role,
        Self::ToolOutcome,
        Self::Isolation,
        Self::Network,
        Self::ToolRefusal,
        Self::EngineIdentity,
        Self::FieldProvenance,
        Self::Provenance,
        Self::ApprovalScope,
        Self::Warrant,
        Self::ForkOutcome,
        Self::PatchOp,
        Self::SeamReason,
        Self::Framing,
        Self::ForkDelivery,
        Self::ToolOutputState,
        Self::SeamToolOutputs,
    ];

    /// The Rust type's name, which the bindings name the union after.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::State => "State",
            Self::Lane => "Lane",
            Self::Command => "Command",
            Self::Refusal => "Refusal",
            Self::FailReason => "FailReason",
            Self::SettleReason => "SettleReason",
            Self::GapEnd => "GapEnd",
            Self::Role => "Role",
            Self::ToolOutcome => "ToolOutcome",
            Self::Isolation => "Isolation",
            Self::Network => "Network",
            Self::ToolRefusal => "ToolRefusal",
            Self::EngineIdentity => "EngineIdentity",
            Self::FieldProvenance => "FieldProvenance",
            Self::Provenance => "Provenance",
            Self::ApprovalScope => "ApprovalScope",
            Self::Warrant => "Warrant",
            Self::ForkOutcome => "ForkOutcome",
            Self::PatchOp => "PatchOp",
            Self::SeamReason => "SeamReason",
            Self::Framing => "Framing",
            Self::ForkDelivery => "ForkDelivery",
            Self::ToolOutputState => "ToolOutputState",
            Self::SeamToolOutputs => "SeamToolOutputs",
        }
    }

    /// Its tags, read off the vocabulary itself.
    #[must_use]
    pub fn tags(self) -> Vec<&'static str> {
        fn of<T: Copy>(all: &[T], tag: fn(T) -> &'static str) -> Vec<&'static str> {
            all.iter().map(|v| tag(*v)).collect()
        }
        match self {
            Self::State => of(State::ALL, State::tag),
            Self::Lane => of(Lane::ALL, Lane::tag),
            Self::Command => of(Command::ALL, Command::tag),
            Self::Refusal => of(Refusal::ALL, Refusal::tag),
            Self::FailReason => of(FailReason::ALL, FailReason::tag),
            Self::SettleReason => of(SettleReason::ALL, SettleReason::tag),
            Self::GapEnd => of(GapEnd::ALL, GapEnd::tag),
            Self::Role => of(Role::ALL, Role::tag),
            Self::ToolOutcome => of(ToolOutcome::ALL, ToolOutcome::tag),
            Self::Isolation => of(Isolation::ALL, Isolation::tag),
            Self::Network => of(Network::ALL, Network::tag),
            Self::ToolRefusal => of(ToolRefusal::ALL, ToolRefusal::tag),
            Self::EngineIdentity => of(EngineIdentity::ALL, EngineIdentity::tag),
            Self::FieldProvenance => of(FieldProvenance::ALL, FieldProvenance::tag),
            Self::Provenance => of(Provenance::ALL, Provenance::tag),
            Self::ApprovalScope => of(ApprovalScope::ALL, ApprovalScope::tag),
            Self::Warrant => of(Warrant::ALL, Warrant::tag),
            Self::ForkOutcome => of(ForkOutcome::ALL, ForkOutcome::tag),
            Self::PatchOp => of(PatchOp::ALL, PatchOp::tag),
            Self::SeamReason => of(SeamReason::ALL, SeamReason::tag),
            Self::Framing => of(Framing::ALL, Framing::tag),
            Self::ForkDelivery => of(ForkDelivery::ALL, ForkDelivery::tag),
            Self::ToolOutputState => of(ToolOutputState::ALL, ToolOutputState::tag),
            Self::SeamToolOutputs => of(SeamToolOutputs::ALL, SeamToolOutputs::tag),
        }
    }
}

/// What a key holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holds {
    /// A non-negative integer.
    Count,
    /// Text.
    Text,
    /// The format's version, which is [`VERSION`].
    Version,
    /// The session's head: a list of `{role, content}`.
    Head,
    /// One tag of a closed vocabulary.
    Tag(Tags),
    /// A non-negative number of milliseconds: an integer, or an exact
    /// decimal as written ([`Millis`]).
    Millis,
    /// A `response`'s [`Timings`]: an object of the keys [`TIMINGS`]
    /// declares, every one optional.
    Timings,
    /// A JSON boolean (v2).
    Flag,
    /// A sha256: 64 lowercase hex digits (v2).
    Digest,
    /// A `response`'s [`Usage`]: an object of the keys [`USAGE`] declares,
    /// its two counts required and its cache count optional (v2).
    Usage,
    /// A `session.start`'s [`TemplateKwargs`] (v7): an object of the keys
    /// [`TEMPLATE_KWARGS`] declares.
    TemplateKwargs,
    /// A `delivered` line's `lines` (v7): a non-empty list of objects of
    /// the keys [`DELIVERED_LINE`] declares.
    DeliveredLines,
    /// A seam's `phase` (v7): an object of [`PHASE_MOVE`]'s keys.
    PhaseMove,
    /// A `session.start`'s `phase_transitions` (v7): a list of such objects.
    PhaseMoves,
    /// A `session.start`'s `instruction_files` (v7): a non-empty list of
    /// objects of the keys [`INSTRUCTION_FILE`] declares.
    InstructionFiles,
    /// A `session.start`'s [`Unsent`] (v7): an object of the keys [`UNSENT`]
    /// declares.
    Unsent,
    /// A substrate claim's `served` (v7): a non-empty list of objects of
    /// the keys [`SERVED_FIELD`] declares.
    Served,
    /// A `session.start`'s [`Serving`]: an object of the keys [`SERVING`]
    /// declares (v2).
    Serving,
    /// A list of text (v3).
    Strings,
    /// A `delta`'s tool-call fragment: an object of the keys
    /// [`TOOL_CALL_PIECE`] declares (v3).
    ToolCallPiece,
    /// A `tool_call`'s [`Approval`]: an object of the keys [`APPROVAL`]
    /// declares (v4).
    Approval,
    /// A working directory as [`is_a_working_directory`] has it: absolute,
    /// or the home as `~` or `~/...` (v4).
    WorkingDirectory,
    /// A `tool_call`'s files: a non-empty list of objects of the keys
    /// [`RECORDED_FILE`] declares (v4).
    Files,
    /// A `patch`'s [`PatchEntry`]: an object of the keys [`PATCH_ENTRY`]
    /// declares (v5).
    Entry,
}

/// One key a kind carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// The key.
    pub key: &'static str,
    /// What it holds.
    pub holds: Holds,
    /// Whether every line of the kind carries it.
    pub required: bool,
    /// The version it arrived in.
    pub since: i64,
}

const fn must(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: true,
        since: 0,
    }
}

const fn may(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: false,
        since: 0,
    }
}

/// An optional key that arrived in v1.
const fn may_v1(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: false,
        since: 1,
    }
}

/// An optional key that arrived in v2.
const fn may_v2(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: false,
        since: 2,
    }
}

/// A key that arrived in v2 and is required wherever its object is written.
const fn must_v2(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: true,
        since: 2,
    }
}

/// An optional key that arrived in v3.
const fn may_v3(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: false,
        since: 3,
    }
}

/// A key that arrived in v3 and is required wherever its object is written.
const fn must_v3(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: true,
        since: 3,
    }
}

/// An optional key that arrived in v4.
const fn may_v4(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: false,
        since: 4,
    }
}

/// A key that arrived in v4 and is required wherever its object is written.
const fn must_v4(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: true,
        since: 4,
    }
}

/// The keys a `tool_call`'s `approval` carries: its scope always, and when
/// it was decided and why it prompted, which [`decided_as_its_scope_says`]
/// requires of every scope but `preseeded` and refuses there. Arrived in v4.
pub const APPROVAL: &[Field] = &[
    must_v4("scope", Holds::Tag(Tags::ApprovalScope)),
    may_v4("decided_at", Holds::Count),
    may_v4("why", Holds::Text),
];

/// The keys of each entry of a `tool_call`'s `files`: all four, always, and
/// nothing else -- a file is recorded by reference, never inlined (ruled at
/// 5983588924). Arrived in v4.
pub const RECORDED_FILE: &[Field] = &[
    must_v4("path", Holds::Text),
    must_v4("sha256", Holds::Digest),
    must_v4("media_type", Holds::Text),
    must_v4("bytes", Holds::Count),
];

/// The keys a `session.start`'s `template_kwargs` may carry, each as sent.
/// Arrived in v7.
pub const TEMPLATE_KWARGS: &[Field] = &[
    may_v7("enable_thinking", Holds::Flag),
    may_v7("reasoning_effort", Holds::Text),
    may_v7("preserve_thinking", Holds::Flag),
];

/// The keys of each of a `delivered` line's `lines`. Arrived in v7.
pub const DELIVERED_LINE: &[Field] = &[
    must_v7("entry", Holds::Text),
    must_v7("op", Holds::Tag(Tags::PatchOp)),
    must_v7("template", Holds::Text),
];

/// The keys of a phase move (v7, #563): both, always.
pub const PHASE_MOVE: &[Field] = &[must_v7("from", Holds::Text), must_v7("to", Holds::Text)];
/// The keys of each of a `session.start`'s `instruction_files`. Arrived in
/// v7.
pub const INSTRUCTION_FILE: &[Field] = &[
    must_v7("path", Holds::Text),
    must_v7("sha256", Holds::Digest),
];

/// The keys of a `session.start`'s `unsent`: what the regime declares and
/// no request carries. Arrived in v7.
pub const UNSENT: &[Field] = &[must_v7("budget_tokens", Holds::Count)];

/// The keys of each entry of a substrate claim's `served`: the field, its
/// declared value and its provenance always, and what the engine reported
/// under `corroborated` only. Arrived in v7.
pub const SERVED_FIELD: &[Field] = &[
    must_v7("field", Holds::Text),
    must_v7("value", Holds::Text),
    must_v7("provenance", Holds::Tag(Tags::FieldProvenance)),
    may_v7("reported", Holds::Text),
];

/// The keys of a `patch`'s entry: its id and text always, its category when
/// the fold names one. Arrived in v5.
pub const PATCH_ENTRY: &[Field] = &[
    must_v5("id", Holds::Text),
    must_v5("text", Holds::Text),
    may_v5("category", Holds::Text),
];

/// An optional key that arrived in v5.
const fn may_v5(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: false,
        since: 5,
    }
}

/// A key that arrived in v5 and is required wherever its object is written.
const fn must_v5(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: true,
        since: 5,
    }
}

/// A key that arrived in v6 and is required wherever its object is written.
const fn may_v7(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: false,
        since: 7,
    }
}

/// A key that arrived in v7 and is required wherever its object is written.
const fn must_v7(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: true,
        since: 7,
    }
}

/// A key that arrived in v6 and is required wherever its object is written.
const fn must_v6(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: true,
        since: 6,
    }
}

/// The keys a `delta`'s `tool_call` carries: the call's index always, its
/// id and name on its first fragment only, as the server sent them, and the
/// fragment of its arguments. Arrived in v3.
pub const TOOL_CALL_PIECE: &[Field] = &[
    must_v3("index", Holds::Count),
    may_v3("id", Holds::Text),
    may_v3("name", Holds::Text),
    must_v3("arguments", Holds::Text),
];

/// The keys a `response`'s `usage` carries, as the server names them
/// (`prompt_tokens_details.cached_tokens` flattened to `cached_tokens`).
/// A server that sends `usage` sends both counts; only the cache count is
/// optional. Arrived in v2.
pub const USAGE: &[Field] = &[
    must_v2("prompt_tokens", Holds::Count),
    must_v2("completion_tokens", Holds::Count),
    may_v2("cached_tokens", Holds::Count),
];

/// The keys a `session.start`'s `serving` carries: its dialect, always, and
/// its concurrency when declared. Arrived in v2.
pub const SERVING: &[Field] = &[
    must_v2("dialect", Holds::Text),
    may_v2("concurrency", Holds::Count),
];

/// Whether `text` is a sha256 as this format writes one.
fn is_a_digest(text: &str) -> bool {
    text.len() == 64
        && text
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// The keys of the object a key holds, for the holders that are objects, or
/// of each entry, for [`Holds::Files`], a list of them.
#[must_use]
pub fn object_fields(holds: Holds) -> Option<&'static [Field]> {
    match holds {
        Holds::Timings => Some(TIMINGS),
        Holds::Usage => Some(USAGE),
        Holds::Serving => Some(SERVING),
        Holds::TemplateKwargs => Some(TEMPLATE_KWARGS),
        Holds::Unsent => Some(UNSENT),
        Holds::PhaseMove | Holds::PhaseMoves => Some(PHASE_MOVE),
        Holds::InstructionFiles => Some(INSTRUCTION_FILE),
        Holds::DeliveredLines => Some(DELIVERED_LINE),
        Holds::ToolCallPiece => Some(TOOL_CALL_PIECE),
        Holds::Approval => Some(APPROVAL),
        Holds::Files => Some(RECORDED_FILE),
        Holds::Served => Some(SERVED_FIELD),
        Holds::Entry => Some(PATCH_ENTRY),
        _ => None,
    }
}

/// The version a kind arrived in.
#[must_use]
pub fn introduced(kind: Kind) -> i64 {
    match kind {
        Kind::Progress => 1,
        Kind::ToolCall => 3,
        Kind::Fork | Kind::ForkSettled | Kind::Patch => 5,
        Kind::Seam => 6,
        Kind::Delivered => 7,
        _ => 0,
    }
}

/// The version a tag of `tags` arrived in. A vocabulary that arrived with
/// its key is scoped by the key's `since`.
#[must_use]
pub fn tag_introduced(tags: Tags, tag: &str) -> i64 {
    let refused_in_v4 = tags == Tags::ToolRefusal
        && ToolRefusal::from_tag(tag)
            .is_some_and(|reason| [ToolRefusal::Denylist, ToolRefusal::Declined].contains(&reason));
    if refused_in_v4 {
        return 4;
    }
    if tags == Tags::Lane && Lane::from_tag(tag) == Some(Lane::Interview) {
        return 5;
    }
    if tags == Tags::Refusal && Refusal::from_tag(tag) == Some(Refusal::NothingToSeam) {
        return 6;
    }
    let phase_refusal = tags == Tags::Refusal
        && Refusal::from_tag(tag).is_some_and(|refusal| {
            matches!(
                refusal,
                Refusal::NoPhaseGraph
                    | Refusal::NotAPhase
                    | Refusal::AlreadyInPhase
                    | Refusal::NoPhaseEdge
            )
        });
    if phase_refusal {
        return 7;
    }
    if tags == Tags::ApprovalScope && ApprovalScope::from_tag(tag) == Some(ApprovalScope::Off) {
        return 7;
    }
    let capped =
        tags == Tags::SettleReason && SettleReason::from_tag(tag) == Some(SettleReason::Capped);
    if capped {
        return 3;
    }
    let context_overflow =
        tags == Tags::FailReason && FailReason::from_tag(tag) == Some(FailReason::ContextOverflow);
    i64::from(context_overflow)
}

/// The keys a `response`'s `timings` may carry, as the server names them
/// (D1, and Q3 for the draft pair). Every one is optional and arrived in v1.
pub const TIMINGS: &[Field] = &[
    may_v1("prompt_n", Holds::Count),
    may_v1("cache_n", Holds::Count),
    may_v1("prompt_ms", Holds::Millis),
    may_v1("predicted_n", Holds::Count),
    may_v1("predicted_ms", Holds::Millis),
    may_v1("draft_n", Holds::Count),
    may_v1("draft_n_accepted", Holds::Count),
];

/// Every key a kind carries beyond `seq`, `t` and `kind`, and what each
/// holds. THE ONE DECLARATION: the reader's key check ([`keys`]), the
/// TypeScript bindings ([`typescript`]) and the test that pins this table
/// against what [`render`] actually writes all read it.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn schema(kind: Kind) -> &'static [Field] {
    use Holds::{Count, Head, Tag, Text, Timings, Version};
    match kind {
        Kind::SessionStart => {
            const F: &[Field] = &[
                must("version", Version),
                must("opened", Count),
                must("model", Text),
                must("head", Head),
                may_v2("serving", Holds::Serving),
                may_v3("substrate", Text),
                may_v3("registry_sha256", Holds::Digest),
                may_v3("engine_build", Text),
                may_v3("engine_identity", Tag(Tags::EngineIdentity)),
                may_v7("served", Holds::Served),
                may_v3("provenance", Tag(Tags::Provenance)),
                may_v5("tools", Holds::Strings),
                may_v7("template_kwargs", Holds::TemplateKwargs),
                may_v7("unsent", Holds::Unsent),
                may_v7("approvals_off", Holds::Flag),
                may_v7("fork_delivery", Tag(Tags::ForkDelivery)),
                may_v7("reasoning_effort_default", Text),
                may_v7("phases", Holds::Strings),
                may_v7("phase_transitions", Holds::PhaseMoves),
                may_v7("opening_phase", Text),
                may_v7("instruction_files", Holds::InstructionFiles),
                may_v7("tool_output", Tag(Tags::ToolOutputState)),
                may_v7("tool_output_max_lines", Holds::Count),
                may_v7("tool_output_max_bytes", Holds::Count),
            ];
            F
        }
        Kind::Ask => {
            const F: &[Field] = &[
                must("turn", Count),
                must("text", Text),
                may_v5("scoping", Holds::Flag),
                may_v5("files", Holds::Files),
            ];
            F
        }
        Kind::Settlement => {
            const F: &[Field] = &[must("from", Tag(Tags::State)), must("to", Tag(Tags::State))];
            F
        }
        Kind::Request => {
            const F: &[Field] = &[
                must("turn", Count),
                must("lane", Tag(Tags::Lane)),
                may_v2("head_sha256", Holds::Digest),
                may_v5("fork", Count),
                may_v7("max_tokens", Count),
            ];
            F
        }
        Kind::Refused => {
            const F: &[Field] = &[
                must("command", Tag(Tags::Command)),
                must("because", Tag(Tags::Refusal)),
                must("during", Tag(Tags::State)),
            ];
            F
        }
        Kind::Delta => {
            const F: &[Field] = &[
                must("request", Count),
                may("text", Text),
                may("reasoning", Text),
                may_v3("tool_call", Holds::ToolCallPiece),
            ];
            F
        }
        Kind::StopAsked => {
            const F: &[Field] = &[must("turn", Count)];
            F
        }
        Kind::Response => {
            const F: &[Field] = &[
                must("to_request", Count),
                must("text", Text),
                may("finish_reason", Text),
                may_v1("reasoning", Text),
                may_v1("timings", Timings),
                may_v2("usage", Holds::Usage),
                may_v2("capped", Holds::Flag),
            ];
            F
        }
        Kind::Cancelled => {
            const F: &[Field] = &[
                must("request", Count),
                must("partial", Text),
                may_v3("reasoning", Text),
            ];
            F
        }
        Kind::RequestFailed => {
            const F: &[Field] = &[
                must("request", Count),
                must("reason", Tag(Tags::FailReason)),
                must("message", Text),
                may("status", Count),
                may("partial", Text),
            ];
            F
        }
        Kind::TurnSettled => {
            const F: &[Field] = &[must("turn", Count), must("reason", Tag(Tags::SettleReason))];
            F
        }
        Kind::IdleGap => {
            const F: &[Field] = &[
                must("opened_by", Count),
                must("notice", Count),
                must("read", Count),
                must("compose", Count),
                must("away", Count),
                must("blocked", Count),
                must("ended_by", Tag(Tags::GapEnd)),
            ];
            F
        }
        Kind::Progress => {
            const F: &[Field] = &[
                must("request", Count),
                must("total", Count),
                must("cache", Count),
                must("processed", Count),
                must("time_ms", Count),
            ];
            F
        }
        Kind::ToolCall => {
            const F: &[Field] = &[
                must_v3("request", Count),
                must_v3("turn", Count),
                must_v3("id", Text),
                must_v3("name", Text),
                must_v3("arguments", Text),
                must_v3("outcome", Tag(Tags::ToolOutcome)),
                may_v3("argv", Holds::Strings),
                may_v4("cwd", Holds::WorkingDirectory),
                may_v3("confined", Holds::Strings),
                may_v3("isolation", Tag(Tags::Isolation)),
                may_v3("network", Tag(Tags::Network)),
                may_v3("exit", Count),
                may_v3("reason", Tag(Tags::ToolRefusal)),
                may_v3("policy", Holds::Digest),
                may_v3("stdout", Text),
                may_v3("stdout_bytes", Count),
                may_v3("stderr", Text),
                may_v3("stderr_bytes", Count),
                may_v4("approval", Holds::Approval),
                may_v4("files", Holds::Files),
                may_v5("shown", Text),
                may_v7("recovered_from", Text),
            ];
            F
        }
        Kind::Fork => {
            const F: &[Field] = &[
                must_v5("lane", Tag(Tags::Lane)),
                must_v5("of_turn", Count),
                must_v5("at", Count),
                must_v5("why", Tag(Tags::Warrant)),
                must_v5("question", Text),
            ];
            F
        }
        Kind::ForkSettled => {
            const F: &[Field] = &[
                must_v5("fork", Count),
                must_v5("outcome", Tag(Tags::ForkOutcome)),
            ];
            F
        }
        Kind::Patch => {
            const F: &[Field] = &[
                must_v5("fork", Count),
                must_v5("op", Tag(Tags::PatchOp)),
                must_v5("entry", Holds::Entry),
                may_v5("supersedes", Text),
            ];
            F
        }
        Kind::Seam => {
            const F: &[Field] = &[
                must_v6("at_turn", Count),
                must_v6("reason", Tag(Tags::SeamReason)),
                must_v6("prefix_hash_before", Holds::Digest),
                must_v6("prefix_hash_after", Holds::Digest),
                must_v6("frame", Text),
                must_v6("render", Text),
                must_v6("carried_entries", Count),
                must_v6("carried_turns", Count),
                may_v7("tail_tokens", Count),
                may_v7("carried_tokens", Count),
                may_v7("phase", Holds::PhaseMove),
                may_v7("tool_outputs", Tag(Tags::SeamToolOutputs)),
                may_v7("outputs", Text),
                may_v7("carried_outputs", Count),
                may_v7("carried_output_bytes", Count),
            ];
            F
        }
        Kind::Delivered => {
            const F: &[Field] = &[
                must_v7("turn", Count),
                must_v7("framing", Tag(Tags::Framing)),
                must_v7("text", Text),
                must_v7("lines", Holds::DeliveredLines),
            ];
            F
        }
    }
}

/// Optional keys of which a line of `kind` carries exactly one.
#[must_use]
pub fn exactly_one(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Delta => &["text", "reasoning", "tool_call"],
        _ => &[],
    }
}

/// Optional keys a line of `kind` carries all of or none of: a
/// `session.start`'s substrate claim (v3, #292). A session started without
/// a regimen carries none of them.
#[must_use]
pub fn all_or_none(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::SessionStart => &["substrate", "registry_sha256"],
        _ => &[],
    }
}

/// Optional keys of which a line of `kind` carries at most one. A
/// `response`'s `usage` is carried only for a server that reports no
/// `timings` (#157): on llama.cpp the two are equal, and a line carrying
/// both would let them disagree.
#[must_use]
pub fn at_most_one(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Response => &["timings", "usage"],
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// TypeScript bindings (#31: the SPA reads a log through types generated from
// this file, never through a hand-kept mirror of it)
// ---------------------------------------------------------------------------

/// The checked-in bindings' path, relative to the crate root.
pub const BINDINGS: &str = "formats/log/log.ts";

fn ts_holds(holds: Holds) -> String {
    match holds {
        Holds::Count | Holds::Millis => "number".to_owned(),
        Holds::Text | Holds::Digest | Holds::WorkingDirectory => "string".to_owned(),
        Holds::Version => READS
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | "),
        Holds::Timings => "Timings".to_owned(),
        Holds::Flag => "boolean".to_owned(),
        Holds::Usage => "Usage".to_owned(),
        Holds::Serving => "Serving".to_owned(),
        Holds::TemplateKwargs => "TemplateKwargs".to_owned(),
        Holds::Unsent => "Unsent".to_owned(),
        Holds::PhaseMove => "PhaseMove".to_owned(),
        Holds::PhaseMoves => "PhaseMove[]".to_owned(),
        Holds::InstructionFiles => "InstructionFile[]".to_owned(),
        Holds::DeliveredLines => "NoteLine[]".to_owned(),
        Holds::Strings => "string[]".to_owned(),
        Holds::ToolCallPiece => "ToolCallPiece".to_owned(),
        Holds::Approval => "Approval".to_owned(),
        Holds::Entry => "PatchEntry".to_owned(),
        Holds::Files => "RecordedFile[]".to_owned(),
        Holds::Served => "ServedField[]".to_owned(),
        Holds::Head => "HeadMessage[]".to_owned(),
        Holds::Tag(tags) => tags.name().to_owned(),
    }
}

fn ts_name(kind: Kind) -> String {
    kind.tag()
        .split(['.', '_'])
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .chain(std::iter::once("Line".to_owned()))
        .collect()
}

/// The bindings' half of [`all_or_none`]: the keys together, or none of them.
fn ts_all_or_none(out: &mut String, kind: Kind, all: &[&str]) {
    use std::fmt::Write as _;
    if all.is_empty() {
        return;
    }
    let typed: Vec<String> = all
        .iter()
        .map(|key| {
            let holds = schema(kind)
                .iter()
                .find(|f| f.key == *key)
                .map(|f| f.holds)
                .expect("`all_or_none` names only keys the schema declares");
            format!("{key}: {}", ts_holds(holds))
        })
        .collect();
    let never: Vec<String> = all.iter().map(|key| format!("{key}?: never")).collect();
    let _ = write!(
        out,
        " & ({{ {} }} | {{ {} }})",
        typed.join("; "),
        never.join("; ")
    );
}

/// The TypeScript bindings for this format, generated from [`schema`] and
/// the vocabularies. Deterministic, and independent of where it is run from:
/// it reads nothing but this module.
///
/// # Panics
///
/// If [`exactly_one`], [`at_most_one`] or [`all_or_none`] names a key
/// [`schema`] does not declare for the same kind -- a table defect, and one
/// the schema's own test refuses first.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn typescript() -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str(
        "// GENERATED from diet/src/formats/log.rs -- do not edit by hand.\n\
         // Regenerate: cargo test -p discipline-diet --lib \
         formats::log::tests::write_the_bindings -- --ignored\n\
         // A count here is an integer in the log; the reader refuses one past\n\
         // i64, and a JavaScript number is exact only to 2^53. A `timings`\n\
         // millisecond may carry a fraction, written as the server wrote it.\n\n",
    );
    let _ = writeln!(out, "export const VERSION = {VERSION};");
    let reads: Vec<String> = READS.iter().map(ToString::to_string).collect();
    let _ = writeln!(out, "export const READS = [{}] as const;", reads.join(", "));
    let _ = write!(
        out,
        "export const PRESENCE_WINDOW_MS = {PRESENCE_WINDOW_MS};\n\n"
    );
    out.push_str("export type Kind =\n");
    for kind in Kind::ALL {
        let _ = writeln!(out, "  | \"{}\"", kind.tag());
    }
    out.push_str(";\n\n");
    for tags in Tags::ALL {
        let _ = writeln!(out, "export type {} =", tags.name());
        for tag in tags.tags() {
            let _ = writeln!(out, "  | \"{tag}\"");
        }
        out.push_str(";\n\n");
    }
    out.push_str("export interface HeadMessage {\n  role: Role;\n  content: string;\n}\n\n");
    for (name, fields) in [
        ("Timings", TIMINGS),
        ("Usage", USAGE),
        ("Serving", SERVING),
        ("TemplateKwargs", TEMPLATE_KWARGS),
        ("Unsent", UNSENT),
        ("PhaseMove", PHASE_MOVE),
        ("InstructionFile", INSTRUCTION_FILE),
        ("NoteLine", DELIVERED_LINE),
        ("ToolCallPiece", TOOL_CALL_PIECE),
        ("Approval", APPROVAL),
        ("RecordedFile", RECORDED_FILE),
        ("ServedField", SERVED_FIELD),
        ("PatchEntry", PATCH_ENTRY),
    ] {
        let _ = writeln!(out, "export interface {name} {{");
        for field in fields {
            let optional = if field.required { "" } else { "?" };
            let _ = writeln!(out, "  {}{optional}: {};", field.key, ts_holds(field.holds));
        }
        out.push_str("}\n\n");
    }
    for kind in Kind::ALL {
        let name = ts_name(*kind);
        let one = exactly_one(*kind);
        let some = at_most_one(*kind);
        let all = all_or_none(*kind);
        let _ = writeln!(out, "export type {name} = {{");
        out.push_str("  seq: number;\n  t: number;\n");
        let _ = writeln!(out, "  kind: \"{}\";", kind.tag());
        for field in schema(*kind) {
            if one.contains(&field.key) || some.contains(&field.key) || all.contains(&field.key) {
                continue;
            }
            let optional = if field.required { "" } else { "?" };
            let _ = writeln!(out, "  {}{optional}: {};", field.key, ts_holds(field.holds));
        }
        out.push('}');
        if !one.is_empty() {
            out.push_str(" & (");
            for (index, key) in one.iter().enumerate() {
                if index > 0 {
                    out.push_str(" | ");
                }
                let holds = schema(*kind)
                    .iter()
                    .find(|f| f.key == *key)
                    .map(|f| f.holds)
                    .expect("`exactly_one` names only keys the schema declares");
                let _ = write!(out, "{{ {key}: {}", ts_holds(holds));
                for other in one.iter().filter(|o| *o != key) {
                    let _ = write!(out, "; {other}?: never");
                }
                out.push_str(" }");
            }
            out.push(')');
        }
        if !some.is_empty() {
            out.push_str(" & (");
            for (index, key) in some.iter().enumerate() {
                if index > 0 {
                    out.push_str(" | ");
                }
                let holds = schema(*kind)
                    .iter()
                    .find(|f| f.key == *key)
                    .map(|f| f.holds)
                    .expect("`at_most_one` names only keys the schema declares");
                let _ = write!(out, "{{ {key}?: {}", ts_holds(holds));
                for other in some.iter().filter(|o| *o != key) {
                    let _ = write!(out, "; {other}?: never");
                }
                out.push_str(" }");
            }
            out.push(')');
        }
        ts_all_or_none(&mut out, *kind, all);
        out.push_str(";\n\n");
    }
    out.push_str("export type LogLine =\n");
    for kind in Kind::ALL {
        let _ = writeln!(out, "  | {}", ts_name(*kind));
    }
    out.push_str(";\n");
    out
}

/// A line as the record's value space holds it.
///
/// One arm per kind, each the mirror of its arm in [`from_object`]: split
/// apart, the two directions of a kind would sit in different places.
#[allow(clippy::too_many_lines)]
fn to_value(line: &Line) -> Value {
    let mut object = BTreeMap::new();
    let mut put = |key: &str, value: Value| {
        object.insert(key.to_owned(), value);
    };
    put("seq", count(line.seq));
    put("t", count(line.t));
    let kind = match &line.event {
        Event::SessionStart {
            version,
            opened,
            model,
            head,
            serving,
            claim,
            provenance,
            tools,
            template_kwargs,
            unsent,
            approvals_off,
            fork_delivery,
            reasoning_effort_default,
            tool_output,
            phases,
            phase_transitions,
            opening_phase,
            instruction_files,
        } => {
            put("version", Value::Integer(*version));
            if let Some(phases) = phases {
                put(
                    "phases",
                    Value::Array(phases.iter().map(|name| text(name)).collect()),
                );
            }
            if let Some(moves) = phase_transitions {
                put(
                    "phase_transitions",
                    Value::Array(moves.iter().map(phase_move_value).collect()),
                );
            }
            if let Some(phase) = opening_phase {
                put("opening_phase", text(phase));
            }
            if let Some(effort) = reasoning_effort_default {
                put("reasoning_effort_default", text(effort));
            }
            if let Some(files) = instruction_files {
                put(
                    "instruction_files",
                    Value::Array(
                        files
                            .iter()
                            .map(|file| {
                                Value::Object(BTreeMap::from([
                                    ("path".to_owned(), text(&file.path)),
                                    ("sha256".to_owned(), text(&file.sha256)),
                                ]))
                            })
                            .collect(),
                    ),
                );
            }
            if let Some(delivery) = fork_delivery {
                put("fork_delivery", text(delivery.tag()));
            }
            if let Some(cap) = tool_output {
                put("tool_output", text(cap.state.tag()));
                if let Some(lines) = cap.max_lines {
                    put("tool_output_max_lines", count(lines));
                }
                if let Some(bytes) = cap.max_bytes {
                    put("tool_output_max_bytes", count(bytes));
                }
            }
            if let Some(off) = approvals_off {
                put("approvals_off", Value::Boolean(*off));
            }
            put("opened", count(*opened));
            put("model", text(model));
            put(
                "head",
                Value::Array(
                    head.iter()
                        .map(|message| {
                            Value::Object(BTreeMap::from([
                                ("role".to_owned(), text(message.role.tag())),
                                ("content".to_owned(), text(&message.content)),
                            ]))
                        })
                        .collect(),
                ),
            );
            if let Some(serving) = serving {
                let mut object = BTreeMap::from([("dialect".to_owned(), text(&serving.dialect))]);
                if let Some(concurrency) = serving.concurrency {
                    object.insert("concurrency".to_owned(), count(concurrency));
                }
                put("serving", Value::Object(object));
            }
            if let Some(claim) = claim {
                put("substrate", text(&claim.substrate));
                put("registry_sha256", text(&claim.registry_sha256));
                match &claim.engine {
                    ClaimedEngine::Checked { build, identity } => {
                        put("engine_build", text(build));
                        put("engine_identity", text(identity.tag()));
                    }
                    ClaimedEngine::Served(served) => put("served", served_value(served)),
                }
            }
            if let Some(provenance) = provenance {
                put("provenance", text(provenance.tag()));
            }
            if let Some(tools) = tools {
                put(
                    "tools",
                    Value::Array(tools.iter().map(|tool| text(tool)).collect()),
                );
            }
            if let Some(kwargs) = template_kwargs {
                let mut object = BTreeMap::new();
                if let Some(thinking) = kwargs.enable_thinking {
                    object.insert("enable_thinking".to_owned(), Value::Boolean(thinking));
                }
                if let Some(effort) = &kwargs.reasoning_effort {
                    object.insert("reasoning_effort".to_owned(), text(effort));
                }
                if let Some(preserve) = kwargs.preserve_thinking {
                    object.insert("preserve_thinking".to_owned(), Value::Boolean(preserve));
                }
                put("template_kwargs", Value::Object(object));
            }
            if let Some(unsent) = unsent {
                put(
                    "unsent",
                    Value::Object(BTreeMap::from([(
                        "budget_tokens".to_owned(),
                        count(unsent.budget_tokens),
                    )])),
                );
            }
            Kind::SessionStart
        }
        Event::Ask {
            turn,
            text: asked,
            scoping,
            files,
        } => {
            put("turn", count(u64::from(*turn)));
            put("text", text(asked));
            if let Some(scoping) = scoping {
                put("scoping", Value::Boolean(*scoping));
            }
            if let Some(files) = files {
                put("files", files_value(files));
            }
            Kind::Ask
        }
        Event::Settlement { from, to } => {
            put("from", text(from.tag()));
            put("to", text(to.tag()));
            Kind::Settlement
        }
        Event::Request {
            turn,
            lane,
            head_sha256,
            fork,
            max_tokens,
        } => {
            put("turn", count(u64::from(*turn)));
            put("lane", text(lane.tag()));
            if let Some(digest) = head_sha256 {
                put("head_sha256", text(digest));
            }
            if let Some(fork) = fork {
                put("fork", count(*fork));
            }
            if let Some(max_tokens) = max_tokens {
                put("max_tokens", count(*max_tokens));
            }
            Kind::Request
        }
        Event::Refused {
            command,
            because,
            during,
        } => {
            put("command", text(command.tag()));
            put("because", text(because.tag()));
            put("during", text(during.tag()));
            Kind::Refused
        }
        Event::Delta { request, piece } => {
            put("request", count(*request));
            match piece {
                Piece::Text(piece) => put("text", text(piece)),
                Piece::Reasoning(piece) => put("reasoning", text(piece)),
                Piece::ToolCall {
                    index,
                    id,
                    name,
                    arguments,
                } => {
                    let mut object = BTreeMap::from([
                        ("index".to_owned(), count(*index)),
                        ("arguments".to_owned(), text(arguments)),
                    ]);
                    for (key, value) in [("id", id), ("name", name)] {
                        if let Some(value) = value {
                            object.insert(key.to_owned(), text(value));
                        }
                    }
                    put("tool_call", Value::Object(object));
                }
            }
            Kind::Delta
        }
        Event::StopAsked { turn } => {
            put("turn", count(u64::from(*turn)));
            Kind::StopAsked
        }
        Event::Response {
            to_request,
            text: answer,
            finish_reason,
            reasoning,
            timings,
            usage,
            capped,
        } => {
            put("to_request", count(*to_request));
            put("text", text(answer));
            if let Some(reason) = finish_reason {
                put("finish_reason", text(reason));
            }
            if let Some(reasoning) = reasoning {
                put("reasoning", text(reasoning));
            }
            if let Some(timings) = timings {
                put("timings", timings_value(timings));
            }
            if let Some(usage) = usage {
                let object = [
                    ("prompt_tokens", Some(usage.prompt_tokens)),
                    ("completion_tokens", Some(usage.completion_tokens)),
                    ("cached_tokens", usage.cached_tokens),
                ]
                .into_iter()
                .filter_map(|(key, value)| value.map(|value| (key.to_owned(), count(value))))
                .collect();
                put("usage", Value::Object(object));
            }
            if let Some(capped) = capped {
                put("capped", Value::Boolean(*capped));
            }
            Kind::Response
        }
        Event::Cancelled {
            request,
            partial,
            reasoning,
        } => {
            put("request", count(*request));
            put("partial", text(partial));
            if let Some(reasoning) = reasoning {
                put("reasoning", text(reasoning));
            }
            Kind::Cancelled
        }
        Event::RequestFailed {
            request,
            reason,
            message,
            status,
            partial,
        } => {
            put("request", count(*request));
            put("reason", text(reason.tag()));
            put("message", text(message));
            if let Some(status) = status {
                put("status", count(u64::from(*status)));
            }
            if let Some(partial) = partial {
                put("partial", text(partial));
            }
            Kind::RequestFailed
        }
        Event::TurnSettled { turn, reason } => {
            put("turn", count(u64::from(*turn)));
            put("reason", text(reason.tag()));
            Kind::TurnSettled
        }
        Event::IdleGap {
            opened_by,
            notice,
            read,
            compose,
            away,
            blocked,
            ended_by,
        } => {
            put("opened_by", count(*opened_by));
            put("notice", count(*notice));
            put("read", count(*read));
            put("compose", count(*compose));
            put("away", count(*away));
            put("blocked", count(*blocked));
            put("ended_by", text(ended_by.tag()));
            Kind::IdleGap
        }
        Event::Progress {
            request,
            total,
            cache,
            processed,
            time_ms,
        } => {
            put("request", count(*request));
            put("total", count(*total));
            put("cache", count(*cache));
            put("processed", count(*processed));
            put("time_ms", count(*time_ms));
            Kind::Progress
        }
        Event::ToolCall {
            request,
            turn,
            id,
            name,
            arguments,
            outcome,
            argv,
            cwd,
            confined,
            isolation,
            network,
            exit,
            reason,
            policy,
            stdout,
            stderr,
            approval,
            files,
            shown,
            recovered_from,
        } => {
            put("request", count(*request));
            put("turn", count(u64::from(*turn)));
            put("id", text(id));
            put("name", text(name));
            put("arguments", text(arguments));
            put("outcome", text(outcome.tag()));
            for (key, list) in [("argv", argv), ("confined", confined)] {
                if let Some(list) = list {
                    put(
                        key,
                        Value::Array(list.iter().map(|item| text(item)).collect()),
                    );
                }
            }
            if let Some(cwd) = cwd {
                put("cwd", text(cwd));
            }
            if let Some(isolation) = isolation {
                put("isolation", text(isolation.tag()));
            }
            if let Some(network) = network {
                put("network", text(network.tag()));
            }
            if let Some(exit) = exit {
                put("exit", count(*exit));
            }
            if let Some(reason) = reason {
                put("reason", text(reason.tag()));
            }
            if let Some(policy) = policy {
                put("policy", text(policy));
            }
            for ((text_key, bytes_key), output) in STREAMS.iter().zip([stdout, stderr]) {
                if let Some(output) = output {
                    put(text_key, text(&output.text));
                    put(bytes_key, count(output.bytes));
                }
            }
            if let Some(approval) = approval {
                let mut object = BTreeMap::from([("scope".to_owned(), text(approval.scope.tag()))]);
                if let Some(decided_at) = approval.decided_at {
                    object.insert("decided_at".to_owned(), count(decided_at));
                }
                if let Some(why) = &approval.why {
                    object.insert("why".to_owned(), text(why));
                }
                put("approval", Value::Object(object));
            }
            if let Some(files) = files {
                put("files", files_value(files));
            }
            if let Some(shown) = shown {
                put("shown", text(shown));
            }
            if let Some(source) = recovered_from {
                put("recovered_from", text(source));
            }
            Kind::ToolCall
        }
        Event::Fork {
            lane,
            of_turn,
            at,
            why,
            question,
        } => {
            put("lane", text(lane.tag()));
            put("of_turn", count(u64::from(*of_turn)));
            put("at", count(*at));
            put("why", text(why.tag()));
            put("question", text(question));
            Kind::Fork
        }
        Event::ForkSettled { fork, outcome } => {
            put("fork", count(*fork));
            put("outcome", text(outcome.tag()));
            Kind::ForkSettled
        }
        Event::Patch {
            fork,
            op,
            entry,
            supersedes,
        } => {
            put("fork", count(*fork));
            put("op", text(op.tag()));
            let mut object = BTreeMap::from([
                ("id".to_owned(), text(&entry.id)),
                ("text".to_owned(), text(&entry.text)),
            ]);
            if let Some(category) = &entry.category {
                object.insert("category".to_owned(), text(category));
            }
            put("entry", Value::Object(object));
            if let Some(replaced) = supersedes {
                put("supersedes", text(replaced));
            }
            Kind::Patch
        }
        Event::Seam {
            at_turn,
            reason,
            prefix_hash_before,
            prefix_hash_after,
            frame,
            render,
            carried_entries,
            carried_turns,
            tail_tokens,
            carried_tokens,
            phase,
            tool_outputs,
            outputs,
            carried_outputs,
            carried_output_bytes,
        } => {
            put("at_turn", count(u64::from(*at_turn)));
            if let Some(moved) = phase {
                put("phase", phase_move_value(moved));
            }
            put("reason", text(reason.tag()));
            put("prefix_hash_before", text(prefix_hash_before));
            put("prefix_hash_after", text(prefix_hash_after));
            put("frame", text(frame));
            put("render", text(render));
            put("carried_entries", count(*carried_entries));
            put("carried_turns", count(*carried_turns));
            if let Some(tokens) = tail_tokens {
                put("tail_tokens", count(*tokens));
            }
            if let Some(tokens) = carried_tokens {
                put("carried_tokens", count(*tokens));
            }
            if let Some(state) = tool_outputs {
                put("tool_outputs", text(state.tag()));
            }
            if let Some(section) = outputs {
                put("outputs", text(section));
            }
            if let Some(n) = carried_outputs {
                put("carried_outputs", count(*n));
            }
            if let Some(n) = carried_output_bytes {
                put("carried_output_bytes", count(*n));
            }
            Kind::Seam
        }
        Event::Delivered {
            turn,
            framing,
            text: note,
            lines,
        } => {
            put("turn", count(u64::from(*turn)));
            put("framing", text(framing.tag()));
            put("text", text(note));
            put(
                "lines",
                Value::Array(
                    lines
                        .iter()
                        .map(|line| {
                            Value::Object(BTreeMap::from([
                                ("entry".to_owned(), text(&line.entry)),
                                ("op".to_owned(), text(line.op.tag())),
                                ("template".to_owned(), text(&line.template)),
                            ]))
                        })
                        .collect(),
                ),
            );
            Kind::Delivered
        }
    };
    put("kind", text(kind.tag()));
    Value::Object(object)
}

/// A phase move as the log writes it.
fn phase_move_value(moved: &PhaseMove) -> Value {
    Value::Object(BTreeMap::from([
        ("from".to_owned(), Value::String(moved.from.clone())),
        ("to".to_owned(), Value::String(moved.to.clone())),
    ]))
}

/// A claim's `served` as the log writes it: each [`SERVED_FIELD`] key, and
/// `reported` only where the engine reported.
fn served_value(served: &[ServedField]) -> Value {
    Value::Array(
        served
            .iter()
            .map(|field| {
                let mut entry = BTreeMap::from([
                    ("field".to_owned(), Value::String(field.field.clone())),
                    ("value".to_owned(), Value::String(field.value.clone())),
                    (
                        "provenance".to_owned(),
                        Value::String(field.provenance.tag().to_owned()),
                    ),
                ]);
                if let Some(reported) = &field.reported {
                    entry.insert("reported".to_owned(), Value::String(reported.clone()));
                }
                Value::Object(entry)
            })
            .collect(),
    )
}

/// A line's `files` as the log writes them: each [`RECORDED_FILE`] key, and
/// nothing inlined.
fn files_value(files: &[RecordedFile]) -> Value {
    Value::Array(
        files
            .iter()
            .map(|file| {
                Value::Object(BTreeMap::from([
                    ("path".to_owned(), Value::String(file.path.clone())),
                    ("sha256".to_owned(), Value::String(file.sha256.clone())),
                    (
                        "media_type".to_owned(),
                        Value::String(file.media_type.clone()),
                    ),
                    ("bytes".to_owned(), count(file.bytes)),
                ]))
            })
            .collect(),
    )
}

/// A [`Timings`] as the record's value space holds it: only the keys the
/// server sent.
fn timings_value(timings: &Timings) -> Value {
    let mut object = BTreeMap::new();
    let counts = [
        ("prompt_n", timings.prompt_n),
        ("cache_n", timings.cache_n),
        ("predicted_n", timings.predicted_n),
        ("draft_n", timings.draft_n),
        ("draft_n_accepted", timings.draft_n_accepted),
    ];
    for (key, value) in counts {
        if let Some(value) = value {
            object.insert(key.to_owned(), count(value));
        }
    }
    for (key, value) in [
        ("prompt_ms", &timings.prompt_ms),
        ("predicted_ms", &timings.predicted_ms),
    ] {
        if let Some(value) = value {
            object.insert(key.to_owned(), value.to_value());
        }
    }
    Value::Object(object)
}

fn count(number: u64) -> Value {
    Value::Integer(i64::try_from(number).unwrap_or(i64::MAX))
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// A line's object, read field by field with the field's name in every
/// error.
struct Fields<'a>(&'a BTreeMap<String, Value>);

impl Fields<'_> {
    fn get(&self, key: &str) -> Result<&Value, String> {
        self.0.get(key).ok_or_else(|| format!("no `{key}`"))
    }

    fn string(&self, key: &str) -> Result<String, String> {
        match self.get(key)? {
            Value::String(text) => Ok(text.clone()),
            _ => Err(format!("`{key}` is not a string")),
        }
    }

    fn optional_string(&self, key: &str) -> Result<Option<String>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(_) => self.string(key).map(Some),
        }
    }

    fn integer(&self, key: &str) -> Result<i64, String> {
        match self.get(key)? {
            Value::Integer(number) => Ok(*number),
            _ => Err(format!("`{key}` is not an integer")),
        }
    }

    fn count(&self, key: &str) -> Result<u64, String> {
        u64::try_from(self.integer(key)?).map_err(|_| format!("`{key}` is negative"))
    }

    fn turn(&self, key: &str) -> Result<u32, String> {
        u32::try_from(self.count(key)?)
            .ok()
            .filter(|turn| *turn > 0)
            .ok_or_else(|| format!("`{key}` is not a turn (1 or more)"))
    }

    fn tag<T>(&self, key: &str, from_tag: fn(&str) -> Option<T>) -> Result<T, String> {
        let tag = self.string(key)?;
        from_tag(&tag).ok_or_else(|| format!("`{key}` is `{tag}`, which no version names"))
    }

    /// A `response`'s `timings`: an object of [`TIMINGS`]' keys, each a
    /// count or a non-negative number of milliseconds.
    fn timings(&self, key: &str) -> Result<Timings, String> {
        let Value::Object(object) = self.get(key)? else {
            return Err(format!("`{key}` is not an object"));
        };
        if let Some(unknown) = object
            .keys()
            .find(|field| !TIMINGS.iter().any(|f| f.key == field.as_str()))
        {
            return Err(format!("`{key}` carries no `{unknown}`"));
        }
        let count = |field: &str| match object.get(field) {
            None => Ok(None),
            Some(Value::Integer(n)) => u64::try_from(*n)
                .map(Some)
                .map_err(|_| format!("`{key}.{field}` is negative")),
            Some(_) => Err(format!("`{key}.{field}` is not a count")),
        };
        let millis = |field: &str| match object.get(field) {
            None => Ok(None),
            Some(Value::Integer(n)) if *n < 0 => {
                Err(format!("`{key}.{field}` is a negative whole number"))
            }
            Some(Value::Decimal(d)) if d.as_str().starts_with('-') => {
                Err(format!("`{key}.{field}` is negative"))
            }
            Some(Value::Integer(n)) => Ok(Millis::new(&n.to_string())),
            Some(Value::Decimal(d)) => Ok(Millis::new(d.as_str())),
            Some(_) => Err(format!("`{key}.{field}` is not a number")),
        };
        Ok(Timings {
            prompt_n: count("prompt_n")?,
            cache_n: count("cache_n")?,
            prompt_ms: millis("prompt_ms")?,
            predicted_n: count("predicted_n")?,
            predicted_ms: millis("predicted_ms")?,
            draft_n: count("draft_n")?,
            draft_n_accepted: count("draft_n_accepted")?,
        })
    }

    fn optional_digest(&self, key: &str) -> Result<Option<String>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Value::String(digest)) if is_a_digest(digest) => Ok(Some(digest.clone())),
            Some(_) => Err(format!(
                "`{key}` is not a sha256 of 64 lowercase hex digits"
            )),
        }
    }

    fn digest(&self, key: &str) -> Result<String, String> {
        self.optional_digest(key)?
            .ok_or_else(|| format!("no `{key}`"))
    }

    fn optional_count(&self, key: &str) -> Result<Option<u64>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(_) => self.count(key).map(Some),
        }
    }

    fn optional_tag<T>(
        &self,
        key: &str,
        from_tag: fn(&str) -> Option<T>,
    ) -> Result<Option<T>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(_) => self.tag(key, from_tag).map(Some),
        }
    }

    /// A list of text, when carried (v3).
    fn optional_strings(&self, key: &str) -> Result<Option<Vec<String>>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| match item {
                    Value::String(text) => Ok(text.clone()),
                    _ => Err(format!("`{key}` holds an item that is not a string")),
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(format!("`{key}` is not a list")),
        }
    }

    /// An output stream: its text and its byte count, when both are carried.
    /// That they come together is [`fits_its_outcome`]'s rule, not this
    /// reader's. The count is `0` exactly when the text is empty, in both
    /// directions, and is not compared with the text's length otherwise:
    /// output that is not UTF-8 is written as lossy text ([`Output`]).
    fn output(&self, text_key: &str, bytes_key: &str) -> Result<Option<Output>, String> {
        match (
            self.optional_string(text_key)?,
            self.optional_count(bytes_key)?,
        ) {
            (Some(text), Some(bytes)) => {
                if text.is_empty() && bytes != 0 {
                    return Err(format!(
                        "`{text_key}` is empty and `{bytes_key}` is {bytes}: no text decodes \
                         from a non-zero count"
                    ));
                }
                if !text.is_empty() && bytes == 0 {
                    return Err(format!(
                        "`{text_key}` carries text and `{bytes_key}` is 0: zero bytes decode to \
                         nothing"
                    ));
                }
                Ok(Some(Output { text, bytes }))
            }
            _ => Ok(None),
        }
    }

    /// A `delta`'s `tool_call`: an object of [`TOOL_CALL_PIECE`]'s keys.
    fn tool_call_piece(&self, key: &str) -> Result<Piece, String> {
        let inner = Fields(self.object(key, TOOL_CALL_PIECE)?);
        let within = |why: String| format!("`{key}`: {why}");
        Ok(Piece::ToolCall {
            index: inner.count("index").map_err(within)?,
            id: inner.optional_string("id").map_err(within)?,
            name: inner.optional_string("name").map_err(within)?,
            arguments: inner.string("arguments").map_err(within)?,
        })
    }

    /// A `tool_call`'s `approval`, when carried: an object of [`APPROVAL`]'s
    /// keys (v4). That its `decided_at` fits its scope and its line is
    /// [`decided_as_its_scope_says`]'s rule, not this reader's.
    fn approval(&self, key: &str) -> Result<Option<Approval>, String> {
        if !self.0.contains_key(key) {
            return Ok(None);
        }
        let inner = Fields(self.object(key, APPROVAL)?);
        let within = |why: String| format!("`{key}`: {why}");
        Ok(Some(Approval {
            scope: inner
                .tag("scope", ApprovalScope::from_tag)
                .map_err(within)?,
            decided_at: inner.optional_count("decided_at").map_err(within)?,
            why: match inner.optional_string("why").map_err(within)? {
                Some(why) if !is_a_reason_word(&why) => {
                    return Err(format!(
                        "`{key}`: `why` is `{why}`, which is not one word: a lowercase letter, \
                         then lowercase letters, digits and `_`"
                    ));
                }
                why => why,
            },
        }))
    }

    /// What a `session.start`'s substrate claim says of the engine, when it
    /// is `claimed`: `served` (v7), or `engine_build` with `engine_identity`
    /// (v3 to v6), exactly one form. Unclaimed, it carries none of them.
    fn claimed_engine(&self, claimed: bool) -> Result<Option<ClaimedEngine>, String> {
        let carries = |key: &str| self.0.contains_key(key);
        let older = carries("engine_build") || carries("engine_identity");
        if !claimed {
            return match ["engine_build", "engine_identity", "served"]
                .into_iter()
                .find(|key| carries(key))
            {
                Some(key) => Err(format!(
                    "`session.start` carries `{key}` without `substrate` and `registry_sha256`: \
                     it is part of the substrate claim"
                )),
                None => Ok(None),
            };
        }
        match (carries("served"), older) {
            (true, true) => Err(
                "`session.start`'s substrate claim carries `served` beside `engine_build` or \
                 `engine_identity`: v7's `served` replaces them"
                    .to_owned(),
            ),
            (true, false) => Ok(Some(ClaimedEngine::Served(self.served("served")?))),
            (false, true) => Ok(Some(ClaimedEngine::Checked {
                build: self.string("engine_build")?,
                identity: self.tag("engine_identity", EngineIdentity::from_tag)?,
            })),
            (false, false) => Err(
                "`session.start`'s substrate claim says nothing of the engine: it carries \
                 `served`, or `engine_build` and `engine_identity`"
                    .to_owned(),
            ),
        }
    }

    /// A `delivered` line's `lines` (v7): a non-empty list, each entry the
    /// keys of [`DELIVERED_LINE`] and nothing else.
    fn delivered_lines(&self, key: &str) -> Result<Vec<NoteLine>, String> {
        let Value::Array(entries) = self.get(key)? else {
            return Err(format!("`{key}` is not a list"));
        };
        if entries.is_empty() {
            return Err(format!(
                "`{key}` is empty: a note delivers at least one line"
            ));
        }
        let mut lines = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let at = |why: String| format!("`{key}[{index}]`: {why}");
            let Value::Object(entry) = entry else {
                return Err(at("is not an object".to_owned()));
            };
            if let Some(extra) = entry
                .keys()
                .find(|field| !DELIVERED_LINE.iter().any(|f| f.key == field.as_str()))
            {
                return Err(at(format!("carries `{extra}`")));
            }
            let inner = Fields(entry);
            lines.push(NoteLine {
                entry: inner.string("entry").map_err(at)?,
                op: inner.tag("op", PatchOp::from_tag).map_err(at)?,
                template: inner.string("template").map_err(at)?,
            });
        }
        Ok(lines)
    }

    /// A claim's `served` (v7): a non-empty list, each entry the keys of
    /// [`SERVED_FIELD`] and nothing else, `reported` under `corroborated`
    /// only, and no field named twice.
    fn served(&self, key: &str) -> Result<Vec<ServedField>, String> {
        let Value::Array(entries) = self.get(key)? else {
            return Err(format!("`{key}` is not a list"));
        };
        if entries.is_empty() {
            return Err(format!(
                "`{key}` is empty: a claim names at least one field"
            ));
        }
        let mut served: Vec<ServedField> = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let at = |why: String| format!("`{key}[{index}]`: {why}");
            let Value::Object(entry) = entry else {
                return Err(at("is not an object".to_owned()));
            };
            if let Some(extra) = entry
                .keys()
                .find(|field| !SERVED_FIELD.iter().any(|f| f.key == field.as_str()))
            {
                return Err(at(format!("carries `{extra}`")));
            }
            let inner = Fields(entry);
            let field = inner.string("field").map_err(at)?;
            if served.iter().any(|seen| seen.field == field) {
                return Err(at(format!("names `{field}` a second time")));
            }
            let provenance = inner
                .tag("provenance", FieldProvenance::from_tag)
                .map_err(at)?;
            let reported = match entry.get("reported") {
                None => None,
                Some(_) => Some(inner.string("reported").map_err(at)?),
            };
            match (provenance, &reported) {
                (FieldProvenance::Corroborated, None) => {
                    return Err(at(
                        "is `corroborated` and carries no `reported`: what the engine said"
                            .to_owned(),
                    ));
                }
                (FieldProvenance::Declared, Some(_)) => {
                    return Err(at(
                        "is `declared` and carries `reported`: a declared field is one the \
                         engine did not report"
                            .to_owned(),
                    ));
                }
                _ => {}
            }
            served.push(ServedField {
                field,
                value: inner.string("value").map_err(at)?,
                provenance,
                reported,
            });
        }
        Ok(served)
    }

    /// A `tool_call`'s `files`, when carried (v4, ruled at 5983588924): a
    /// non-empty list, each entry all four of [`RECORDED_FILE`]'s keys and
    /// nothing else -- no `content`, `data` or any other inlining of the file.
    fn optional_files(&self, key: &str) -> Result<Option<Vec<RecordedFile>>, String> {
        let Some(value) = self.0.get(key) else {
            return Ok(None);
        };
        let Value::Array(entries) = value else {
            return Err(format!("`{key}` is not a list"));
        };
        if entries.is_empty() {
            return Err(format!(
                "`{key}` is empty: a call that left no file carries no `{key}`"
            ));
        }
        let mut files = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let at = |why: String| format!("`{key}[{index}]`: {why}");
            let Value::Object(entry) = entry else {
                return Err(at("is not an object".to_owned()));
            };
            if let Some(extra) = entry
                .keys()
                .find(|field| !RECORDED_FILE.iter().any(|f| f.key == field.as_str()))
            {
                return Err(at(format!(
                    "carries `{extra}`: a file is recorded by reference, never inlined"
                )));
            }
            let inner = Fields(entry);
            let path = inner.string("path").map_err(at)?;
            if !is_a_recorded_path(&path) {
                return Err(at(format!(
                    "`path` is `{path}`, which is not relative to the recording's directory: \
                     one or more components joined by single `/`, none empty, `.` or `..`, no \
                     `\\` and no leading `~`"
                )));
            }
            let media_type = inner.string("media_type").map_err(at)?;
            if !is_a_media_type(&media_type) {
                return Err(at(format!(
                    "`media_type` is `{media_type}`, which is not a `type/subtype`"
                )));
            }
            files.push(RecordedFile {
                path,
                sha256: inner.digest("sha256").map_err(at)?,
                media_type,
                bytes: inner.count("bytes").map_err(at)?,
            });
        }
        Ok(Some(files))
    }

    /// A `tool_call`'s `cwd`, when carried: a working directory as
    /// [`is_a_working_directory`] has it (v4).
    fn optional_working_directory(&self, key: &str) -> Result<Option<String>, String> {
        match self.optional_string(key)? {
            Some(cwd) if !is_a_working_directory(&cwd) => Err(format!(
                "`{key}` is `{cwd}`, which is not a working directory: absolute (`/...`), or \
                 the home as `~` or `~/...`, never relative and never another user's"
            )),
            cwd => Ok(cwd),
        }
    }

    /// A `patch`'s `entry`: an object of [`PATCH_ENTRY`]'s keys (v5).
    fn entry(&self, key: &str) -> Result<PatchEntry, String> {
        let inner = Fields(self.object(key, PATCH_ENTRY)?);
        let within = |why: String| format!("`{key}`: {why}");
        Ok(PatchEntry {
            id: inner.string("id").map_err(within)?,
            text: inner.string("text").map_err(within)?,
            category: inner.optional_string("category").map_err(within)?,
        })
    }

    fn optional_flag(&self, key: &str) -> Result<Option<bool>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(Value::Boolean(flag)) => Ok(Some(*flag)),
            Some(_) => Err(format!("`{key}` is not a boolean")),
        }
    }

    /// The object at `key`, refusing a key `declared` does not name.
    fn object(&self, key: &str, declared: &[Field]) -> Result<&BTreeMap<String, Value>, String> {
        let Value::Object(object) = self.get(key)? else {
            return Err(format!("`{key}` is not an object"));
        };
        if let Some(unknown) = object
            .keys()
            .find(|field| !declared.iter().any(|f| f.key == field.as_str()))
        {
            return Err(format!("`{key}` carries no `{unknown}`"));
        }
        Ok(object)
    }

    /// A `response`'s `usage`: an object of [`USAGE`]' keys, each a count.
    fn usage(&self, key: &str) -> Result<Usage, String> {
        let inner = Fields(self.object(key, USAGE)?);
        let count = |field: &str| inner.count(field).map_err(|why| format!("`{key}`: {why}"));
        Ok(Usage {
            prompt_tokens: count("prompt_tokens")?,
            completion_tokens: count("completion_tokens")?,
            cached_tokens: match inner.0.get("cached_tokens") {
                None => None,
                Some(_) => Some(count("cached_tokens")?),
            },
        })
    }

    /// A `session.start`'s `template_kwargs` (v7, R1): an object of
    /// [`TEMPLATE_KWARGS`]'s keys, at least one.
    fn template_kwargs(&self, key: &str) -> Result<TemplateKwargs, String> {
        let inner = Fields(self.object(key, TEMPLATE_KWARGS)?);
        let at = |why: String| format!("`{key}`: {why}");
        let kwargs = TemplateKwargs {
            enable_thinking: inner.optional_flag("enable_thinking").map_err(at)?,
            reasoning_effort: match inner.0.get("reasoning_effort") {
                None => None,
                Some(_) => Some(inner.string("reasoning_effort").map_err(at)?),
            },
            preserve_thinking: inner.optional_flag("preserve_thinking").map_err(at)?,
        };
        if kwargs == TemplateKwargs::default() {
            return Err(format!(
                "`{key}` is empty: a session that sends none carries no `{key}`"
            ));
        }
        Ok(kwargs)
    }

    /// A seam's `phase` (v7, #563).
    fn phase_move(&self, key: &str) -> Result<PhaseMove, String> {
        Fields(self.object(key, PHASE_MOVE)?)
            .phase_move_of()
            .map_err(|why| format!("`{key}`: {why}"))
    }

    /// These fields as a phase move: `from` and `to`, nothing else.
    fn phase_move_of(&self) -> Result<PhaseMove, String> {
        if let Some(extra) = self
            .0
            .keys()
            .find(|key| !PHASE_MOVE.iter().any(|f| f.key == key.as_str()))
        {
            return Err(format!("carries `{extra}`"));
        }
        Ok(PhaseMove {
            from: self.string("from")?,
            to: self.string("to")?,
        })
    }

    /// A `session.start`'s `instruction_files` (v7, #559), when carried: a
    /// non-empty list of `{path, sha256}`, each path relative -- never
    /// absolute, never a home's.
    fn instruction_files(&self, key: &str) -> Result<Option<Vec<InstructionFile>>, String> {
        let Some(value) = self.0.get(key) else {
            return Ok(None);
        };
        let Value::Array(entries) = value else {
            return Err(format!("`{key}` is not a list"));
        };
        if entries.is_empty() {
            return Err(format!(
                "`{key}` is empty: a session that injected none carries no `{key}`"
            ));
        }
        let mut files = Vec::with_capacity(entries.len());
        for (index, entry) in entries.iter().enumerate() {
            let at = |why: String| format!("`{key}[{index}]`: {why}");
            let Value::Object(entry) = entry else {
                return Err(at("is not an object".to_owned()));
            };
            if let Some(extra) = entry
                .keys()
                .find(|field| !INSTRUCTION_FILE.iter().any(|f| f.key == field.as_str()))
            {
                return Err(at(format!("carries `{extra}`")));
            }
            let inner = Fields(entry);
            let path = inner.string("path").map_err(at)?;
            if path.starts_with('/') || path.starts_with('~') || path.contains('\\') {
                return Err(at(format!(
                    "`path` is `{path}`: relative to the worktree, never absolute or a home's"
                )));
            }
            files.push(InstructionFile {
                path,
                sha256: inner.digest("sha256").map_err(at)?,
            });
        }
        Ok(Some(files))
    }

    /// A `session.start`'s `unsent` (v7, R1): an object of [`UNSENT`]'s keys.
    fn unsent(&self, key: &str) -> Result<Unsent, String> {
        let inner = Fields(self.object(key, UNSENT)?);
        Ok(Unsent {
            budget_tokens: inner
                .count("budget_tokens")
                .map_err(|why| format!("`{key}`: {why}"))?,
        })
    }

    /// A `session.start`'s `serving`: its dialect, and its concurrency when
    /// declared.
    fn serving(&self, key: &str) -> Result<Serving, String> {
        let inner = Fields(self.object(key, SERVING)?);
        Ok(Serving {
            dialect: inner
                .string("dialect")
                .map_err(|why| format!("`{key}`: {why}"))?,
            concurrency: match inner.0.get("concurrency") {
                None => None,
                Some(_) => Some(
                    inner
                        .count("concurrency")
                        .map_err(|why| format!("`{key}`: {why}"))?,
                ),
            },
        })
    }

    fn head(&self, key: &str) -> Result<Vec<HeadMessage>, String> {
        let Value::Array(messages) = self.get(key)? else {
            return Err(format!("`{key}` is not a list"));
        };
        messages
            .iter()
            .map(|message| {
                let Value::Object(message) = message else {
                    return Err(format!("a `{key}` message is not an object"));
                };
                if message
                    .keys()
                    .any(|field| field != "role" && field != "content")
                {
                    return Err(format!(
                        "a `{key}` message carries more than `role` and `content`"
                    ));
                }
                let fields = Fields(message);
                Ok(HeadMessage {
                    role: fields.tag("role", Role::from_tag)?,
                    content: fields.string("content")?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> Line {
        Line {
            seq: 0,
            t: 0,
            event: Event::SessionStart {
                version: VERSION,
                opened: 1_790_000_000_000,
                model: "a-model".to_owned(),
                serving: None,
                claim: None,
                provenance: None,
                head: vec![HeadMessage {
                    role: Role::System,
                    content: "you are the trunk".to_owned(),
                }],
                tools: None,
                template_kwargs: None,
                unsent: None,
                approvals_off: None,
                fork_delivery: None,
                reasoning_effort_default: None,
                instruction_files: None,
                tool_output: Some(ToolOutput {
                    state: ToolOutputState::Capped,
                    max_lines: Some(2000),
                    max_bytes: Some(51_200),
                }),
                phases: None,
                phase_transitions: None,
                opening_phase: None,
            },
        }
    }

    /// One of every event, in an order the rules that span lines accept.
    #[allow(clippy::too_many_lines)]
    fn every_event() -> Vec<Line> {
        let mut events = vec![
            start().event,
            Event::Ask {
                turn: 1,
                text: "say hi".to_owned(),
                scoping: None,
                files: None,
            },
            Event::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Event::Request {
                turn: 1,
                lane: Lane::Trunk,
                head_sha256: None,
                fork: None,
                max_tokens: None,
            },
            Event::Delta {
                request: 3,
                piece: Piece::Reasoning("thinking\nabout it".to_owned()),
            },
            Event::Delta {
                request: 3,
                piece: Piece::Text("Hi".to_owned()),
            },
            Event::Refused {
                command: Command::Ask,
                because: Refusal::InFlight,
                during: State::Turn,
            },
            Event::StopAsked { turn: 1 },
            Event::Cancelled {
                request: 3,
                partial: "Hi".to_owned(),
                reasoning: None,
            },
            Event::TurnSettled {
                turn: 1,
                reason: SettleReason::Cancelled,
            },
            Event::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
            Event::Ask {
                turn: 2,
                text: "again".to_owned(),
                scoping: None,
                files: None,
            },
            Event::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Event::Request {
                turn: 2,
                lane: Lane::Trunk,
                head_sha256: None,
                fork: None,
                max_tokens: None,
            },
            Event::RequestFailed {
                request: 13,
                reason: FailReason::Server,
                message: "busy".to_owned(),
                status: Some(503),
                partial: Some(String::new()),
            },
            Event::TurnSettled {
                turn: 2,
                reason: SettleReason::Failed,
            },
            Event::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
            Event::IdleGap {
                opened_by: 15,
                notice: 120,
                read: 2_400,
                compose: 3_100,
                away: 0,
                blocked: 0,
                ended_by: GapEnd::Ask,
            },
            Event::Ask {
                turn: 3,
                text: "once more".to_owned(),
                scoping: None,
                files: None,
            },
            Event::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Event::Request {
                turn: 3,
                lane: Lane::Trunk,
                head_sha256: None,
                fork: None,
                max_tokens: None,
            },
            Event::Progress {
                request: 20,
                total: 9276,
                cache: 0,
                processed: 0,
                time_ms: 0,
            },
            Event::Progress {
                request: 20,
                total: 9276,
                cache: 0,
                processed: 9276,
                time_ms: 6086,
            },
            Event::Delta {
                request: 20,
                piece: Piece::Reasoning("weighing it\n".to_owned()),
            },
            Event::Response {
                to_request: 20,
                text: "Done.".to_owned(),
                finish_reason: Some("stop".to_owned()),
                reasoning: Some("weighing it\n".to_owned()),
                usage: None,
                capped: None,
                timings: Some(Timings {
                    prompt_n: Some(89),
                    cache_n: Some(0),
                    prompt_ms: Millis::new("297.198"),
                    predicted_n: Some(312),
                    predicted_ms: Millis::new("2591"),
                    draft_n: Some(312),
                    draft_n_accepted: Some(207),
                }),
            },
            Event::TurnSettled {
                turn: 3,
                reason: SettleReason::Final,
            },
            Event::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
            Event::Ask {
                turn: 4,
                text: "a very long one".to_owned(),
                scoping: None,
                files: None,
            },
            Event::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Event::Request {
                turn: 4,
                lane: Lane::Trunk,
                head_sha256: None,
                fork: None,
                max_tokens: None,
            },
            Event::RequestFailed {
                request: 29,
                reason: FailReason::ContextOverflow,
                message: "request (262149 tokens) exceeds the available context size".to_owned(),
                status: Some(400),
                partial: None,
            },
            Event::TurnSettled {
                turn: 4,
                reason: SettleReason::Failed,
            },
            Event::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
            // v3: a call whose outcome never arrived, written after its
            // request ended -- a `tool_call` cites a request, never ends one.
            // v4: it ran under a session-scoped approval, decided before the
            // line (its `t` is 165).
            Event::ToolCall {
                request: 29,
                turn: 4,
                id: "call-0".to_owned(),
                name: "bash".to_owned(),
                arguments: "{\"command\":\"ls\"}".to_owned(),
                outcome: ToolOutcome::Cancelled,
                argv: None,
                cwd: None,
                confined: None,
                isolation: Some(Isolation::Sandbox),
                network: Some(Network::None),
                exit: None,
                reason: None,
                policy: None,
                stdout: None,
                stderr: None,
                approval: Some(Approval {
                    scope: ApprovalScope::Session,
                    decided_at: Some(160),
                    why: Some("not_approved".to_owned()),
                }),
                files: None,
                shown: None,
                recovered_from: None,
            },
        ];
        // v5 (#374): a scoping turn answered and settled `final`, then the
        // one fork of its gap, its interview call, its settling `value`, and
        // the two patches it folded into.
        let asked = events.len() as u64;
        let (request, fork) = (asked + 2, asked + 6);
        events.extend([
            Event::Ask {
                turn: 5,
                text: "what are we building".to_owned(),
                scoping: Some(true),
                // #372: the operator attached a screenshot to the ask.
                files: Some(vec![RecordedFile {
                    path: "files/sketch.png".to_owned(),
                    sha256: "3f1a8e0c5b2d4f6a7e9c1b3d5f7a9c2e4b6d8f0a1c3e5b7d9f1a3c5e7b9d2f4a"
                        .to_owned(),
                    media_type: "image/png".to_owned(),
                    bytes: 48_213,
                }]),
            },
            Event::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Event::Request {
                turn: 5,
                lane: Lane::Trunk,
                head_sha256: None,
                fork: None,
                max_tokens: None,
            },
            Event::Response {
                to_request: request,
                text: "a tracker".to_owned(),
                finish_reason: None,
                reasoning: None,
                timings: None,
                usage: None,
                capped: None,
            },
            Event::TurnSettled {
                turn: 5,
                reason: SettleReason::Final,
            },
            Event::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
            Event::Fork {
                lane: Lane::Interview,
                of_turn: 5,
                at: request,
                why: Warrant::Scoping,
                question: "what did the operator decide".to_owned(),
            },
            Event::Request {
                turn: 5,
                lane: Lane::Interview,
                head_sha256: None,
                fork: Some(fork),
                max_tokens: Some(4000),
            },
            Event::Response {
                to_request: fork + 1,
                text: "{\"decisions\":[]}".to_owned(),
                finish_reason: None,
                reasoning: None,
                timings: None,
                usage: None,
                capped: None,
            },
            Event::ForkSettled {
                fork,
                outcome: ForkOutcome::Value,
            },
            Event::Patch {
                fork,
                op: PatchOp::Add,
                entry: PatchEntry {
                    id: "d1".to_owned(),
                    text: "a tracker".to_owned(),
                    category: None,
                },
                supersedes: None,
            },
            Event::Patch {
                fork,
                op: PatchOp::Supersede,
                entry: PatchEntry {
                    id: "d2".to_owned(),
                    text: "a tracker for one team".to_owned(),
                    category: Some("scope".to_owned()),
                },
                supersedes: Some("d1".to_owned()),
            },
        ]);
        events
            .into_iter()
            .enumerate()
            .map(|(seq, event)| Line {
                seq: seq as u64,
                t: seq as u64 * 5,
                event,
            })
            .collect()
    }

    /// Whether `value` is what `holds` says a key holds.
    fn written_as(holds: Holds, value: &Value) -> bool {
        match (holds, value) {
            (Holds::Count | Holds::Millis, Value::Integer(n)) => *n >= 0,
            (Holds::Version, Value::Integer(n)) => READS.contains(n),
            (Holds::Millis, Value::Decimal(d)) => !d.as_str().starts_with('-'),
            (
                Holds::Timings
                | Holds::Usage
                | Holds::Serving
                | Holds::TemplateKwargs
                | Holds::Unsent
                | Holds::PhaseMove
                | Holds::ToolCallPiece
                | Holds::Approval
                | Holds::Entry,
                Value::Object(object),
            ) => {
                let declared = object_fields(holds).expect("an object holder");
                object.iter().all(|(key, value)| {
                    declared
                        .iter()
                        .find(|f| f.key == key)
                        .is_some_and(|f| written_as(f.holds, value))
                }) && declared
                    .iter()
                    .all(|f| !f.required || object.contains_key(f.key))
            }
            (Holds::Text, Value::String(_)) | (Holds::Flag, Value::Boolean(_)) => true,
            (Holds::Digest, Value::String(digest)) => is_a_digest(digest),
            (Holds::WorkingDirectory, Value::String(cwd)) => is_a_working_directory(cwd),
            (
                Holds::Files
                | Holds::Served
                | Holds::DeliveredLines
                | Holds::PhaseMoves
                | Holds::InstructionFiles,
                Value::Array(entries),
            ) => {
                let declared = object_fields(holds).expect("a list of objects");
                !entries.is_empty()
                    && entries.iter().all(|entry| match entry {
                        Value::Object(entry) => {
                            entry.iter().all(|(key, value)| {
                                declared
                                    .iter()
                                    .find(|f| f.key == key)
                                    .is_some_and(|f| written_as(f.holds, value))
                            }) && declared
                                .iter()
                                .all(|f| !f.required || entry.contains_key(f.key))
                        }
                        _ => false,
                    })
            }
            (Holds::Strings, Value::Array(items)) => {
                items.iter().all(|item| matches!(item, Value::String(_)))
            }
            (Holds::Tag(tags), Value::String(tag)) => tags.tags().contains(&tag.as_str()),
            (Holds::Head, Value::Array(messages)) => messages.iter().all(|m| match m {
                Value::Object(message) => {
                    message.len() == 2
                        && matches!(message.get("content"), Some(Value::String(_)))
                        && message
                            .get("role")
                            .is_some_and(|r| written_as(Holds::Tag(Tags::Role), r))
                }
                _ => false,
            }),
            _ => false,
        }
    }

    /// Every line the schema is pinned against: [`every_event`], and every
    /// valid fixture -- which is where the optional keys go missing.
    fn corpus() -> Vec<Line> {
        let mut lines = every_event();
        let valid =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("formats/log/fixtures/valid");
        let mut files: Vec<_> = std::fs::read_dir(&valid)
            .expect("the valid fixtures")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "jsonl"))
            .collect();
        files.sort();
        assert!(
            !files.is_empty(),
            "no valid fixture to pin the schema against"
        );
        for file in files {
            let text = std::fs::read_to_string(&file).expect("a fixture reads");
            lines.extend(parse(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display())));
        }
        lines
    }

    /// THE SCHEMA IS WHAT THE WRITER WRITES, BOTH WAYS. [`schema`] is read by
    /// the reader's key check and by the TypeScript bindings, so a wrong row is
    /// a reader that accepts a key nothing writes, or an SPA typed against a
    /// shape no log has. Pinned against [`to_value`] over [`corpus`]:
    ///
    /// * the keys written for a kind are exactly the keys declared for it --
    ///   a declared key nothing writes is a key the reader would accept;
    /// * a required key is always written, and an optional one is left out
    ///   somewhere -- or the bindings mark it optional for nothing;
    /// * each value is the type declared, and a tag key's vocabulary is the
    ///   one the READER reads it with: every tag of the declared vocabulary
    ///   parses in its place, and a tag only another vocabulary has does not
    ///   (vocabularies share tags, so membership alone cannot tell them apart;
    ///   found by #144's review).
    #[test]
    fn the_schema_is_what_every_kind_writes() {
        let mut written: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        let mut omitted: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        let (mut nested_written, mut nested_omitted) = (BTreeSet::new(), BTreeSet::new());
        for line in corpus() {
            let Value::Object(object) = to_value(&line) else {
                panic!("a line is an object");
            };
            let kind = Kind::from_tag(match object.get("kind") {
                Some(Value::String(k)) => k,
                _ => panic!("a line names its kind"),
            })
            .expect("a known kind");
            let fields = schema(kind);
            for (key, value) in &object {
                if COMMON.contains(&key.as_str()) {
                    continue;
                }
                let field = fields.iter().find(|f| f.key == key).unwrap_or_else(|| {
                    panic!(
                        "`{}` writes `{key}`, which its schema does not declare",
                        kind.tag()
                    )
                });
                assert!(
                    written_as(field.holds, value),
                    "`{}`'s `{key}` is written as {value:?}, which is not {:?}",
                    kind.tag(),
                    field.holds
                );
                if let Holds::Tag(tags) = field.holds {
                    the_reader_reads_it_as(&object, key, tags);
                }
                // Into a nested object: its keys, one level down, get the
                // same written-somewhere, omitted-somewhere pin.
                // A list of objects (`files`, v4) gets the same pin, entry by
                // entry.
                let inners = objects_in(value);
                if let Some(inner_fields) = object_fields(field.holds) {
                    for inner in inners {
                        for nested in inner_fields {
                            if inner.contains_key(nested.key) {
                                nested_written.insert((field.key, nested.key));
                            } else {
                                nested_omitted.insert((field.key, nested.key));
                            }
                        }
                    }
                }
                written.entry(kind.tag()).or_default().insert(key.clone());
            }
            for field in fields {
                if !object.contains_key(field.key) {
                    assert!(
                        !field.required,
                        "`{}` declares `{}` required and did not write it",
                        kind.tag(),
                        field.key
                    );
                    omitted.entry(kind.tag()).or_default().insert(field.key);
                }
            }
        }
        for kind in Kind::ALL {
            let declared: BTreeSet<String> =
                schema(*kind).iter().map(|f| f.key.to_owned()).collect();
            assert_eq!(
                written.get(kind.tag()).cloned().unwrap_or_default(),
                declared,
                "`{}`: the keys written are not the keys declared",
                kind.tag()
            );
            for field in schema(*kind).iter().filter(|f| !f.required) {
                assert!(
                    omitted
                        .get(kind.tag())
                        .is_some_and(|o| o.contains(field.key)),
                    "`{}` declares `{}` optional and every line writes it",
                    kind.tag(),
                    field.key
                );
            }
            for key in exactly_one(*kind) {
                assert!(
                    schema(*kind).iter().any(|f| f.key == *key && !f.required),
                    "`{}`: `exactly_one` names `{key}`, which is not an optional key",
                    kind.tag()
                );
            }
            for key in at_most_one(*kind) {
                assert!(
                    schema(*kind).iter().any(|f| f.key == *key && !f.required),
                    "`{}`: `at_most_one` names `{key}`, which is not an optional key",
                    kind.tag()
                );
            }
            for key in all_or_none(*kind) {
                assert!(
                    schema(*kind).iter().any(|f| f.key == *key && !f.required),
                    "`{}`: `all_or_none` names `{key}`, which is not an optional key",
                    kind.tag()
                );
            }
        }
        every_nested_key_is_written_and_omitted_as_declared(&nested_written, &nested_omitted);
    }

    /// The objects a value holds: itself, if it is one; each entry, if it is
    /// a list of them (`files`, v4); none otherwise.
    fn objects_in(value: &Value) -> Vec<&BTreeMap<String, Value>> {
        match value {
            Value::Object(inner) => vec![inner],
            Value::Array(items) => items
                .iter()
                .filter_map(|item| match item {
                    Value::Object(inner) => Some(inner),
                    _ => None,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The nested half of [`the_schema_is_what_every_kind_writes`]: every key
    /// of every object holder is written somewhere, and is left out
    /// somewhere exactly when it is optional.
    fn every_nested_key_is_written_and_omitted_as_declared(
        nested_written: &BTreeSet<(&str, &str)>,
        nested_omitted: &BTreeSet<(&str, &str)>,
    ) {
        for (outer, inner_fields) in [
            ("timings", TIMINGS),
            ("usage", USAGE),
            ("serving", SERVING),
            ("template_kwargs", TEMPLATE_KWARGS),
            ("unsent", UNSENT),
            ("phase", PHASE_MOVE),
            ("phase_transitions", PHASE_MOVE),
            ("instruction_files", INSTRUCTION_FILE),
            ("lines", DELIVERED_LINE),
            ("tool_call", TOOL_CALL_PIECE),
            ("approval", APPROVAL),
            ("files", RECORDED_FILE),
            ("served", SERVED_FIELD),
        ] {
            for inner in inner_fields {
                assert!(
                    nested_written.contains(&(outer, inner.key)),
                    "`{outer}.{}` is declared and no line writes it",
                    inner.key
                );
                assert_eq!(
                    nested_omitted.contains(&(outer, inner.key)),
                    !inner.required,
                    "`{outer}.{}`: declared {}, and {}",
                    inner.key,
                    if inner.required {
                        "required"
                    } else {
                        "optional"
                    },
                    if inner.required {
                        "a line leaves it out"
                    } else {
                        "every line writes it"
                    }
                );
            }
        }
    }

    /// A LOG CUT INSIDE A CHARACTER (#230): the bytes of a torn write that
    /// ends mid-sequence read up to the cut, and the tail is set aside; the
    /// same partial byte after a COMPLETE event is not a torn write and stays
    /// a refusal, as does invalid UTF-8 anywhere else.
    #[test]
    fn a_log_cut_inside_a_character_is_a_torn_write_and_nothing_else_is() {
        let head = "{\"head\":[{\"content\":\"x\",\"role\":\"system\"}],\"kind\":\"session.start\",\"model\":\"m\",\"opened\":1,\"seq\":0,\"t\":0,\"version\":0}\n";
        let mut cut = head.as_bytes().to_vec();
        cut.extend_from_slice(b"{\"kind\":\"ask\",\"seq\":1,\"t\":5,\"text\":\"caf\xc3");
        let text = decode(&cut).expect("a write cut inside a character decodes to the cut");
        let read = read(text).expect("and reads through its last complete event");
        assert_eq!((read.lines.len(), read.torn), (1, 1));

        let mut after_an_event = head.as_bytes().to_vec();
        after_an_event.push(0xc3);
        assert!(
            decode(&after_an_event).is_err(),
            "a partial byte after a complete event"
        );

        // A partial byte where no text can stand -- after a number, a brace,
        // a closing quote, an escape's backslash, inside `\u`, or inside
        // `true` -- is no torn write, whatever the bytes before it.
        for after in [
            &b"{\"kind\":\"ask\",\"seq\":9,\"t\":4"[..],
            b"{\"kind\":\"ask\",\"seq\":9,\"t\":",
            b"{",
            b"{\"kind\":\"ask\",\"x\":tr",
            b"{\"kind\":\"ask\",\"text\":\"caf\"",
            b"{\"kind\":\"ask\",\"text\":\"\\",
            b"{\"kind\":\"ask\",\"text\":\"\\u00",
        ] {
            let mut bytes = head.as_bytes().to_vec();
            bytes.extend_from_slice(after);
            bytes.push(0xc3);
            assert!(
                decode(&bytes).is_err(),
                "a cut character after {:?}",
                String::from_utf8_lossy(after)
            );
        }
        // And inside a key, or two bytes of three, it is one.
        for inside in [&b"{\"te\xc3"[..], b"{\"kind\":\"ask\",\"text\":\"\xe6\x97"] {
            let mut bytes = head.as_bytes().to_vec();
            bytes.extend_from_slice(inside);
            assert!(
                decode(&bytes).is_ok(),
                "a cut inside {:?}",
                String::from_utf8_lossy(inside)
            );
        }

        let mut in_the_middle = head.as_bytes().to_vec();
        in_the_middle.extend_from_slice(b"{\"kind\":\"ask\",\"text\":\"\xc3\n");
        assert!(
            decode(&in_the_middle).is_err(),
            "invalid UTF-8 before the last line break"
        );
    }

    /// `exactly_one` IS WHAT THE READER ENFORCES: for every line that writes
    /// one optional text key of its kind, adding another is refused exactly
    /// when the two are declared exclusive. An exclusivity the bindings drop
    /// is a union that admits a line the reader refuses.
    #[test]
    fn exclusive_keys_are_the_ones_the_reader_refuses_together() {
        let mut probed = 0;
        for line in corpus() {
            let Value::Object(object) = to_value(&line) else {
                panic!("a line is an object");
            };
            let Some(Value::String(tag)) = object.get("kind") else {
                panic!("a line names its kind");
            };
            let kind = Kind::from_tag(tag).expect("a known kind");
            let optional_text: Vec<&str> = schema(kind)
                .iter()
                .filter(|f| !f.required && f.holds == Holds::Text)
                .map(|f| f.key)
                .collect();
            for present in optional_text.iter().filter(|k| object.contains_key(**k)) {
                for absent in optional_text.iter().filter(|k| !object.contains_key(**k)) {
                    // A claim's two forms are the reader's other refusal
                    // ([`Fields::claimed_engine`], v7): `served` or the older
                    // keys, never both.
                    // Without a claim, `engine_build` alone is part of a claim
                    // that is not there; beside `served` it is the other form.
                    if ["engine_build"].contains(absent) {
                        continue;
                    }
                    // A key of an all-or-none set, added alone, is the
                    // reader's `all_or_none` refusal, not an exclusivity.
                    if all_or_none(kind).contains(absent) {
                        continue;
                    }
                    let mut both = object.clone();
                    both.insert((*absent).to_owned(), Value::String("x".to_owned()));
                    let mut rendered = String::new();
                    json::render(&Value::Object(both), &mut rendered);
                    let refused = self::line(&rendered).is_err();
                    let exclusive =
                        exactly_one(kind).contains(present) && exactly_one(kind).contains(absent);
                    assert_eq!(
                        refused,
                        exclusive,
                        "`{}` with both `{present}` and `{absent}`: the reader {} it, and \
                         `exactly_one` says they are{} exclusive",
                        kind.tag(),
                        if refused { "refuses" } else { "accepts" },
                        if exclusive { "" } else { " not" }
                    );
                    probed += 1;
                }
            }
        }
        assert!(probed > 0, "no pair of optional keys was probed");
    }

    /// `key` in `object` is read with `tags`: each of its tags parses there,
    /// and a tag only another vocabulary carries does not.
    ///
    /// A `tool_call`'s `outcome` decides which other keys the line carries
    /// ([`outcome_keys`]), so no one line accepts all four outcomes: there, a
    /// tag counts as read when the refusal is the outcome's keys rule, which
    /// runs only once the tag has parsed as a [`ToolOutcome`] (v3). Likewise
    /// `unrecorded` in `isolation` beside a `confined` is refused by the
    /// confinement rule, which runs only once the tag has parsed; so is a
    /// `policy` beside an `isolation` that names no profile, or none beside
    /// one that does (ruled at #299, 5976386318 point 6); and a
    /// `bash` refusal's `reason` decides its `argv`
    /// ([`argv_if_it_parsed`]), which runs only once the reason has parsed.
    fn the_reader_reads_it_as(object: &BTreeMap<String, Value>, key: &str, tags: Tags) {
        let with = |tag: &str| {
            let mut changed = object.clone();
            changed.insert(key.to_owned(), Value::String(tag.to_owned()));
            let mut rendered = String::new();
            json::render(&Value::Object(changed), &mut rendered);
            line(&rendered).map_or_else(
                |why| {
                    (tags == Tags::ToolOutcome && why.contains("whose outcome is"))
                        || (tags == Tags::Isolation
                            && (why.contains("`isolation` is `unrecorded` carries `confined`")
                                || why.contains("carries no `confined`")
                                || why.contains("carries `policy` under no profile")
                                || why.contains("carries no `policy`")))
                        || (tags == Tags::ToolRefusal && why.contains("a `bash` call refused"))
                        || (tags == Tags::PatchOp && why.contains("`supersedes`"))
                        || (tags == Tags::ToolOutputState
                            && why.contains("`tool_output` is `keep` and carries a limit"))
                },
                |_| true,
            )
        };
        let own = tags.tags();
        for tag in &own {
            assert!(
                with(tag),
                "`{key}` declared {} and `{tag}` is refused there",
                tags.name()
            );
        }
        for other in Tags::ALL.iter().filter(|t| **t != tags) {
            for tag in other.tags().into_iter().filter(|t| !own.contains(t)) {
                assert!(
                    !with(tag),
                    "`{key}` declared {} and accepts `{tag}`, which only {} has",
                    tags.name(),
                    other.name()
                );
            }
        }
    }

    /// `request.failed.message` IS PROSE (ruled on #140): the typed `reason`
    /// is the vocabulary, the message is diagnostic text, and nothing here
    /// reads it as anything else. Two fixtures identical but for the message
    /// -- empty, and a paragraph -- parse to the same lines and project to the
    /// same value, `message` aside.
    #[test]
    fn a_failures_message_is_read_as_nothing_but_prose() {
        let valid =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("formats/log/fixtures/valid");
        let read = |name: &str| {
            std::fs::read_to_string(valid.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        let empty = read("a-failure-whose-message-is-empty.jsonl");
        let paragraph = read("a-failure-whose-message-is-a-paragraph.jsonl");

        let without_message = |text: &str| -> Vec<Line> {
            parse(text)
                .expect("a valid fixture")
                .into_iter()
                .map(|mut line| {
                    if let Event::RequestFailed { message, .. } = &mut line.event {
                        message.clear();
                    }
                    line
                })
                .collect()
        };
        let (a, b) = (without_message(&empty), without_message(&paragraph));
        assert!(
            a.iter()
                .any(|l| matches!(l.event, Event::RequestFailed { .. })),
            "the fixtures carry a `request.failed`, or this proves nothing"
        );
        assert_eq!(a, b, "the two logs differ in more than a failure's message");
        // The MESSAGES differ, one of them empty -- not merely the files: two
        // files differing in whitespace would let a projection that reads the
        // message pass unseen (#147's review).
        let messages = |text: &str| -> Vec<String> {
            parse(text)
                .expect("a valid fixture")
                .into_iter()
                .filter_map(|line| match line.event {
                    Event::RequestFailed { message, .. } => Some(message),
                    _ => None,
                })
                .collect()
        };
        let (quiet, spoken) = (messages(&empty), messages(&paragraph));
        assert!(
            quiet.iter().all(String::is_empty) && spoken.iter().all(|m| !m.is_empty()),
            "one fixture's messages are empty and the other's are prose: {quiet:?} / {spoken:?}"
        );

        let projected_without_message = |text: &str| -> Value {
            let Value::Object(mut read) = project(text).expect("a valid fixture projects") else {
                panic!("a log projects to an object");
            };
            let Some(Value::Array(lines)) = read.remove("events") else {
                panic!("a log's projection carries its events");
            };
            Value::Array(
                lines
                    .into_iter()
                    .map(|line| match line {
                        Value::Object(mut object) => {
                            if matches!(object.get("kind"), Some(Value::String(k)) if k == "request.failed") {
                                object.remove("message");
                            }
                            Value::Object(object)
                        }
                        other => other,
                    })
                    .collect(),
            )
        };
        assert_eq!(
            projected_without_message(&empty),
            projected_without_message(&paragraph),
            "the projection reads a failure's message as more than prose"
        );
    }

    fn bindings_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(BINDINGS)
    }

    /// THE CHECKED-IN BINDINGS ARE WHAT THIS FILE GENERATES. The SPA reads
    /// `formats/log/log.ts`; a change here that is not regenerated there is a
    /// SPA reading a format that no longer exists, found only when it misreads
    /// a live log. Located from `CARGO_MANIFEST_DIR`, not the working
    /// directory, so the answer does not depend on where cargo was started.
    #[test]
    fn the_checked_in_bindings_are_current() {
        let checked_in = std::fs::read_to_string(bindings_path())
            .unwrap_or_else(|e| panic!("{BINDINGS} could not be read: {e}"));
        assert!(
            checked_in == typescript(),
            "{BINDINGS} is stale against log.rs; regenerate it: cargo test -p \
             discipline-diet --lib formats::log::tests::write_the_bindings -- --ignored"
        );
    }

    /// Writes the bindings. Ignored: a test run never writes the tree.
    #[test]
    #[ignore = "writes formats/log/log.ts; run it to regenerate the bindings"]
    fn write_the_bindings() {
        std::fs::write(bindings_path(), typescript()).expect("the bindings are written");
    }

    #[test]
    fn every_event_renders_to_one_line_that_reads_back_as_itself() {
        let lines = every_event();
        let mut document = String::new();
        for line in &lines {
            let rendered = render(line);
            assert!(
                !rendered.contains('\n'),
                "a rendered line broke: {rendered}"
            );
            assert_eq!(super::line(&rendered).as_ref(), Ok(line), "{rendered}");
            document.push_str(&rendered);
            document.push('\n');
        }
        assert_eq!(parse(&document), Ok(lines));
    }

    /// A `session.start`'s cap (#554): `capped` with both limits, `keep`
    /// with neither; anything else refused.
    #[test]
    fn a_tool_output_cap_carries_its_limits_exactly_when_capped() {
        let start = |extra: &str| {
            format!(
                r#"{{"head":[],"kind":"session.start","model":"m","opened":1,"seq":0,"t":0,"version":7{extra}}}"#
            )
        };
        assert!(
            line(&start(
                r#","tool_output":"capped","tool_output_max_bytes":10,"tool_output_max_lines":2"#
            ))
            .is_ok()
        );
        assert!(line(&start(r#","tool_output":"keep""#)).is_ok());
        for (extra, says) in [
            (
                r#","tool_output":"capped","tool_output_max_lines":2"#,
                "without both",
            ),
            (
                r#","tool_output":"keep","tool_output_max_lines":2"#,
                "carries a limit",
            ),
            (r#","tool_output_max_bytes":10"#, "without `tool_output`"),
        ] {
            let refused = line(&start(extra)).expect_err(extra);
            assert!(refused.contains(says), "{extra}: {refused}");
        }
    }

    #[test]
    fn a_v0_log_carrying_what_arrived_in_v1_is_refused_and_line_reads_it() {
        // The whole-log reader scopes by the version `session.start` states;
        // the per-line reader, which a resuming reader uses, reads the union.
        let mut lines = every_event();
        let Event::SessionStart {
            version,
            tool_output,
            ..
        } = &mut lines[0].event
        else {
            panic!("the first line opens the session");
        };
        *version = 0;
        // A v7 key on the first line would be the one named; the check is of
        // what arrived in v1, further down.
        *tool_output = None;
        let document: String = lines.iter().map(|line| render(line) + "\n").collect();
        let refused = parse(&document).expect_err("v1 content was read as v0");
        assert!(refused.why.contains("arrived in v1"), "{refused}");
        for line in &lines {
            assert_eq!(super::line(&render(line)).as_ref(), Ok(line));
        }
    }

    /// WHAT ARRIVED IN v4 IS REFUSED IN A LOG THAT DECLARES v3 (#388, ruled
    /// at 5981588394 (a), 5982826236 and 5983588924): an `approval`, a `cwd`,
    /// `files`, and each of
    /// the two refusals v4 adds, `denylist` and `declined`, each named in the
    /// refusal. As v4, each line reads once its `argv` carries its `cwd`.
    #[test]
    fn a_v3_log_carrying_what_arrived_in_v4_is_refused() {
        let call = |rest: &str| {
            format!(
                concat!(
                    r#"{{"head":[{{"content":"you are the trunk","role":"system"}}],"kind":"session.start","model":"a-model","opened":1,"seq":0,"t":0,"version":3}}"#,
                    "\n",
                    r#"{{"kind":"ask","seq":1,"t":5,"text":"x","turn":1}}"#,
                    "\n",
                    r#"{{"from":"awaiting","kind":"settlement","seq":2,"t":10,"to":"turn"}}"#,
                    "\n",
                    r#"{{"kind":"request","lane":"trunk","seq":3,"t":15,"turn":1}}"#,
                    "\n",
                    r#"{{{rest}"arguments":"{{}}","id":"c","kind":"tool_call","name":"bash","request":3,"seq":4,"t":20,"turn":1}}"#,
                    "\n"
                ),
                rest = rest
            )
        };
        for (what, rest) in [
            (
                "`tool_call` carries `approval`",
                r#""approval":{"scope":"preseeded"},"argv":["true"],"isolation":"none","network":"host","outcome":"cancelled","#,
            ),
            (
                "`tool_call` carries `files`",
                r#""argv":["true"],"confined":["true"],"exit":0,"files":[{"bytes":1,"media_type":"text/plain","path":"a.txt","sha256":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}],"isolation":"none","network":"none","outcome":"ran","stderr":"","stderr_bytes":0,"stdout":"","stdout_bytes":0,"#,
            ),
            (
                "`tool_call` carries `cwd`",
                r#""argv":["true"],"cwd":"/work/tree","isolation":"none","network":"host","outcome":"cancelled","#,
            ),
            (
                "`tool_call`'s `reason` is `denylist`",
                r#""argv":["true"],"outcome":"refused","reason":"denylist","#,
            ),
            (
                "`tool_call`'s `reason` is `declined`",
                r#""argv":["true"],"outcome":"refused","reason":"declined","#,
            ),
        ] {
            let document = call(rest);
            let refused = parse(&document).expect_err(&format!("{what} was read as v3"));
            assert!(
                refused.why.contains(&format!(
                    "{what}, which arrived in v4, and this log declares v3"
                )),
                "{refused}"
            );
            let mut as_v4 = document.replace(r#""version":3"#, r#""version":4"#);
            if !as_v4.contains(r#""cwd""#) {
                as_v4 = as_v4.replace(
                    r#""argv":["true"],"#,
                    r#""argv":["true"],"cwd":"/work/tree","#,
                );
            }
            parse(&as_v4).unwrap_or_else(|why| panic!("{what}, as v4: {why}"));
        }
    }

    /// AN APPROVAL'S `decided_at` AND `why` FIT ITS SCOPE AND ITS LINE (#388,
    /// ruled at 5981588394 (b) and (d), and at 5982826236): each required of
    /// `once`, `session` and `workspace` and refused on `preseeded`;
    /// `decided_at` never after the line's `t` -- equal to it reads, one past
    /// it does not; `why` one word as [`is_a_reason_word`] spells it -- under
    /// every outcome that admits an approval: `ran`, `command_failed` and
    /// `cancelled`.
    #[test]
    fn an_approvals_decided_at_fits_its_scope_and_its_line() {
        for (outcome, rest) in [
            ("ran", ""),
            ("command_failed", r#""exit":1,"#),
            ("cancelled", ""),
        ] {
            the_scope_rule_holds_under(outcome, rest);
        }
    }

    fn the_scope_rule_holds_under(outcome: &str, rest: &str) {
        let call = |approval: &str| {
            format!(
                concat!(
                    r#"{{"approval":{approval},"arguments":"{{}}",{rest}"id":"c","kind":"tool_call","#,
                    r#""name":"read","outcome":"{outcome}","request":3,"seq":9,"t":45,"turn":1}}"#
                ),
                approval = approval,
                rest = rest,
                outcome = outcome
            )
        };
        let refused_naming = |approval: String, needle: &str, what: &str| {
            let why = line(&call(&approval)).expect_err(&format!("{what} under {outcome}"));
            assert!(why.contains(needle), "{what} under {outcome}: {why}");
        };
        for scope in ApprovalScope::ALL {
            let tag = scope.tag();
            if matches!(scope, ApprovalScope::Preseeded | ApprovalScope::Off) {
                line(&call(&format!(r#"{{"scope":"{tag}"}}"#))).unwrap_or_else(|why| {
                    panic!("{tag} under {outcome}, without `decided_at` and `why`: {why}")
                });
                for decided_at in [0, 45, 46] {
                    refused_naming(
                        format!(r#"{{"decided_at":{decided_at},"scope":"{tag}"}}"#),
                        &format!("a `{tag}` approval carries `decided_at`"),
                        "a preseeded approval with `decided_at`",
                    );
                }
                refused_naming(
                    format!(r#"{{"scope":"{tag}","why":"not_approved"}}"#),
                    &format!("a `{tag}` approval carries `why`"),
                    "a preseeded approval with `why`",
                );
                continue;
            }
            let prompted =
                |decided_at: &str, why: &str| format!(r#"{{{decided_at}"scope":"{tag}"{why}}}"#);
            let why_held = r#","why":"not_approved""#;
            refused_naming(
                prompted("", why_held),
                &format!("a `{tag}` approval carries no `decided_at`"),
                &format!("{tag} without `decided_at`"),
            );
            refused_naming(
                prompted(r#""decided_at":45,"#, ""),
                &format!("a `{tag}` approval carries no `why`"),
                &format!("{tag} without `why`"),
            );
            for decided_at in [0, 45] {
                line(&call(&prompted(
                    &format!(r#""decided_at":{decided_at},"#),
                    why_held,
                )))
                .unwrap_or_else(|why| {
                    panic!("{tag} under {outcome}, decided at {decided_at}: {why}")
                });
            }
            refused_naming(
                prompted(r#""decided_at":46,"#, why_held),
                "`decided_at` is 46, after its line's `t` 45",
                &format!("{tag} decided after its line"),
            );
            // `why` is one word of an open set, spelled as this reader
            // declares (5982826236; the spelling is a judgement call).
            for word in ["a", "network", "writes_outside_2"] {
                line(&call(&prompted(
                    r#""decided_at":45,"#,
                    &format!(r#","why":"{word}""#),
                )))
                .unwrap_or_else(|why| panic!("{tag} under {outcome}, why `{word}`: {why}"));
            }
            for word in ["", "Network", "netWork", "two words", "1st", "_x", "a-b"] {
                refused_naming(
                    prompted(r#""decided_at":45,"#, &format!(r#","why":"{word}""#)),
                    &format!("`why` is `{word}`, which is not one word"),
                    &format!("{tag} why `{word}`"),
                );
            }
        }
    }

    /// A BASH CALL IN A v4 LOG SAYS WHERE IT RAN (#388, ruled at 5982826236):
    /// a `tool_call` that carries `argv` carries `cwd` in a log that declares
    /// v4 -- the whole log's rule, since a v3 line carries `argv` and no
    /// `cwd` -- and every line refuses a `cwd` without an `argv`. A `cwd` is
    /// absolute or a tilde form never expanded to a user; a relative path,
    /// an empty one and another user's home are refused.
    #[test]
    fn a_bash_call_in_a_v4_log_says_where_it_ran() {
        let log = |version: i64, call: &str| {
            format!(
                concat!(
                    r#"{{"head":[{{"content":"you are the trunk","role":"system"}}],"kind":"session.start","model":"a-model","opened":1,"seq":0,"t":0,"version":{version}}}"#,
                    "\n",
                    r#"{{"kind":"ask","seq":1,"t":5,"text":"x","turn":1}}"#,
                    "\n",
                    r#"{{"from":"awaiting","kind":"settlement","seq":2,"t":10,"to":"turn"}}"#,
                    "\n",
                    r#"{{"kind":"request","lane":"trunk","seq":3,"t":15,"turn":1}}"#,
                    "\n",
                    r#"{{{call}"arguments":"{{}}","id":"c","kind":"tool_call","request":3,"seq":4,"t":20,"turn":1}}"#,
                    "\n"
                ),
                version = version,
                call = call
            )
        };
        let ran = |cwd: &str| {
            format!(
                r#""argv":["true"],{cwd}"confined":["true"],"exit":0,"isolation":"sandbox","name":"bash","network":"none","outcome":"ran","stderr":"","stderr_bytes":0,"stdout":"","stdout_bytes":0,"#
            )
        };
        // Every outcome under which a bash call carries `argv`: in v4 its
        // `cwd` comes with it; in v3 it never could. `denylist` and
        // `declined` are themselves v4's, so a v3 log refuses them by the
        // version gate, never by this rule.
        let failed = r#""argv":["true"],"confined":["true"],"exit":1,"isolation":"none","name":"bash","network":"none","outcome":"command_failed","stderr":"","stderr_bytes":0,"stdout":"","stdout_bytes":0,"#;
        let cancelled = r#""argv":["true"],"isolation":"sandbox","name":"bash","network":"none","outcome":"cancelled","#;
        let refused_for = |reason: &str| {
            format!(r#""argv":["true"],"name":"bash","outcome":"refused","reason":"{reason}","#)
        };
        for (what, call, in_v3) in [
            ("ran", ran(""), true),
            ("command_failed", failed.to_owned(), true),
            ("cancelled", cancelled.to_owned(), true),
            ("refused not_allowed", refused_for("not_allowed"), true),
            ("refused max_steps", refused_for("max_steps"), true),
            ("refused denylist", refused_for("denylist"), false),
            ("refused declined", refused_for("declined"), false),
        ] {
            let refused = parse(&log(4, &call))
                .expect_err(&format!("{what}: a v4 argv without its cwd was read"));
            assert!(
                refused.why.contains(
                    "a `tool_call` carries `argv` without `cwd`, in a log that declares v4"
                ),
                "{what}: {refused}"
            );
            let as_v3 = parse(&log(3, &call));
            if in_v3 {
                as_v3.unwrap_or_else(|why| panic!("{what}: a v3 argv was refused: {why}"));
            } else {
                let why = as_v3.expect_err(&format!("{what}: read in a v3 log"));
                assert!(why.why.contains("which arrived in v4"), "{what}: {why}");
            }
        }
        for cwd in ["/", "/work/tree", "~", "~/src/repo"] {
            parse(&log(4, &ran(&format!(r#""cwd":"{cwd}","#))))
                .unwrap_or_else(|why| panic!("cwd `{cwd}` was refused: {why}"));
        }
        for cwd in ["", "work/tree", "./tree", "~user/tree", "~user", "~~"] {
            let refused = parse(&log(4, &ran(&format!(r#""cwd":"{cwd}","#))))
                .expect_err(&format!("cwd `{cwd}` was read"));
            assert!(
                refused.why.contains(&format!(
                    "`cwd` is `{cwd}`, which is not a working directory"
                )),
                "{refused}"
            );
        }
        for call in [
            r#""cwd":"/work/tree","name":"read","outcome":"ran","#,
            r#""cwd":"/work/tree","name":"bash","outcome":"refused","reason":"unparsable","#,
        ] {
            let refused = parse(&log(4, call)).expect_err("a cwd without an argv was read");
            assert!(
                refused
                    .why
                    .contains("a `tool_call` carries `cwd` without `argv`"),
                "{refused}"
            );
        }
    }

    /// A CALL'S FILES ARE REFERENCES, UNDER AN OUTCOME THAT FINISHED (#388,
    /// ruled at 5983588924): `files` reads under `ran` and `command_failed`
    /// on any name, and is refused under `refused` and `cancelled`; it is a
    /// non-empty list; each entry carries `path`, `sha256`, `media_type` and
    /// `bytes` and nothing else, so no content is inlined; a `path` is
    /// relative to the recording's directory -- not empty, not absolute, no
    /// `..`, no `~` -- and a `media_type` is a `type/subtype`.
    #[test]
    #[allow(clippy::too_many_lines)] // one list of cases, written out
    fn a_tool_calls_files_are_references_under_an_outcome_that_finished() {
        const SHA: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        const STREAMS_HELD: &str = r#""stderr":"","stderr_bytes":0,"stdout":"","stdout_bytes":0,"#;
        let entry = |path: &str, more: &str| {
            format!(
                r#"{{"bytes":48213,{more}"media_type":"image/png","path":"{path}","sha256":"{SHA}"}}"#
            )
        };
        let call = |name: &str, outcome: &str, rest: &str, files: &str| {
            format!(
                r#"{{{rest}"arguments":"{{}}","files":{files},"id":"c","kind":"tool_call","name":"{name}","outcome":"{outcome}","request":3,"seq":9,"t":45,"turn":1}}"#
            )
        };
        let one = format!("[{}]", entry("shots/turn-1.png", ""));
        let bash_ran = format!(
            r#""argv":["true"],"confined":["true"],"cwd":"/work/tree","exit":0,"isolation":"none","network":"none",{STREAMS_HELD}"#
        );
        let bash_failed = format!(
            r#""argv":["true"],"confined":["true"],"cwd":"/work/tree","exit":1,"isolation":"none","network":"none",{STREAMS_HELD}"#
        );
        for (what, text) in [
            ("bash ran", call("bash", "ran", &bash_ran, &one)),
            (
                "bash command_failed",
                call("bash", "command_failed", &bash_failed, &one),
            ),
            ("read ran", call("read", "ran", r#""exit":0,"#, &one)),
            (
                "edit command_failed",
                call("edit", "command_failed", r#""exit":1,"#, &one),
            ),
            (
                "two files",
                call(
                    "read",
                    "ran",
                    "",
                    &format!("[{},{}]", entry("a.png", ""), entry("b/c.png", "")),
                ),
            ),
        ] {
            line(&text).unwrap_or_else(|why| panic!("{what}: files were refused: {why}"));
        }
        let refused = |what: &str, text: String, needle: &str| {
            let why = line(&text).expect_err(&format!("{what} was read"));
            assert!(why.contains(needle), "{what}: {why}");
        };
        refused(
            "files under refused",
            call(
                "bash",
                "refused",
                r#""argv":["rm"],"cwd":"/w","reason":"denylist","#,
                &one,
            ),
            "a `tool_call` whose outcome is `refused` carries `files`",
        );
        refused(
            "files under cancelled",
            call(
                "bash",
                "cancelled",
                r#""isolation":"sandbox","network":"none","#,
                &one,
            ),
            "a `tool_call` whose outcome is `cancelled` carries `files`",
        );
        refused(
            "an empty files",
            call("read", "ran", "", "[]"),
            "`files` is empty",
        );
        refused(
            "files that are not a list",
            call("read", "ran", "", &entry("a.png", "")),
            "`files` is not a list",
        );
        for path in [
            "",
            "/etc/passwd",
            "../up.png",
            "shots/../../up.png",
            "shots/..",
            "~",
            "~/a.png",
            "~user/a.png",
            "a//b.png",
            ".",
            "./a.png",
            "a/./b.png",
            "shots/",
        ] {
            refused(
                &format!("path `{path}`"),
                call("read", "ran", "", &format!("[{}]", entry(path, ""))),
                &format!("`path` is `{path}`, which is not relative to the recording's directory"),
            );
        }
        // A backslash anywhere, written escaped in the line.
        for (written, read) in [(r"a\\b.png", r"a\b.png"), (r"\\a.png", r"\a.png")] {
            refused(
                &format!("path `{read}`"),
                call("read", "ran", "", &format!("[{}]", entry(written, ""))),
                &format!("`path` is `{read}`, which is not relative to the recording's directory"),
            );
        }
        // The accepted form: one or more non-empty components, none `.` or
        // `..`, joined by single `/`.
        for path in [
            "a.png",
            "shots/turn-1.png",
            "a/b/c.d/.hidden",
            "..a/b..",
            "a~/b",
        ] {
            line(&call("read", "ran", "", &format!("[{}]", entry(path, ""))))
                .unwrap_or_else(|why| panic!("path `{path}` was refused: {why}"));
        }
        for inlined in [
            r#""content":"iVBOR","#,
            r#""data":"iVBOR","#,
            r#""base64":"iVBOR","#,
        ] {
            let key = inlined.split('"').nth(1).expect("a key");
            refused(
                &format!("an entry with `{key}`"),
                call("read", "ran", "", &format!("[{}]", entry("a.png", inlined))),
                &format!(
                    "`files[0]`: carries `{key}`: a file is recorded by reference, never inlined"
                ),
            );
        }
        for missing in ["path", "sha256", "media_type", "bytes"] {
            let Value::Object(mut object) = json::line(&entry("a.png", ""))
                .map(Value::Object)
                .expect("an entry")
            else {
                panic!("an object");
            };
            object.remove(missing);
            let mut rendered = String::new();
            json::render(&Value::Object(object), &mut rendered);
            refused(
                &format!("an entry without `{missing}`"),
                call("read", "ran", "", &format!("[{rendered}]")),
                &format!("`files[0]`: no `{missing}`"),
            );
        }
        refused(
            "a sha256 that is not a digest",
            call(
                "read",
                "ran",
                "",
                &format!("[{}]", entry("a.png", "").replace(SHA, "f1e2569d")),
            ),
            "`files[0]`: `sha256` is not a sha256",
        );
        for media_type in ["png", "image/", "/png", "image/png/x", "image /png"] {
            refused(
                &format!("media_type `{media_type}`"),
                call(
                    "read",
                    "ran",
                    "",
                    &format!("[{}]", entry("a.png", "").replace("image/png", media_type)),
                ),
                &format!("`media_type` is `{media_type}`, which is not a `type/subtype`"),
            );
        }
        refused(
            "a negative byte count",
            call(
                "read",
                "ran",
                "",
                &format!("[{}]", entry("a.png", "").replace("48213", "-1")),
            ),
            "`files[0]`: `bytes` is negative",
        );
    }

    #[test]
    fn a_gap_in_seq_is_refused() {
        let mut lines = every_event();
        lines.remove(4);
        let document: String = lines.iter().map(|line| render(line) + "\n").collect();
        let refused = parse(&document).expect_err("a gap was read as a log");
        assert!(refused.why.contains("`seq`"), "{refused}");
    }

    #[test]
    fn a_reference_to_anything_but_an_earlier_request_is_refused() {
        let mut lines = every_event();
        lines[5].event = Event::Delta {
            request: 1,
            piece: Piece::Text("Hi".to_owned()),
        };
        let document: String = lines.iter().map(|line| render(line) + "\n").collect();
        let refused = parse(&document).expect_err("a reference to an ask was read");
        assert!(
            refused.why.contains("not an earlier `request`"),
            "{refused}"
        );
    }

    #[test]
    fn a_settlement_that_does_not_leave_the_state_it_is_in_is_refused() {
        let mut lines = every_event();
        lines[10].event = Event::Settlement {
            from: State::Capture,
            to: State::Awaiting,
        };
        let document: String = lines.iter().map(|line| render(line) + "\n").collect();
        let refused = parse(&document).expect_err("a broken chain was read");
        assert!(refused.why.contains("a settlement from"), "{refused}");
    }

    #[test]
    fn the_vocabularies_are_the_words_the_ruling_names() {
        let tags = |all: Vec<&str>| all.join(" ");
        assert_eq!(
            tags(Kind::ALL.iter().map(|it| it.tag()).collect()),
            "session.start ask settlement request refused delta stop.asked response \
             cancelled request.failed turn.settled idle.gap progress tool_call fork \
             fork.settled patch seam delivered"
        );
        assert_eq!(
            tags(FailReason::ALL.iter().map(|it| it.tag()).collect()),
            "server timeout transport crashed context_overflow"
        );
        assert_eq!(
            tags(Refusal::ALL.iter().map(|it| it.tag()).collect()),
            "in-flight ended nothing-in-flight seam-not-built nothing-to-seam no-phase-graph not-a-phase \
             already-in-phase no-phase-edge stale"
        );
        assert_eq!(
            tags(SettleReason::ALL.iter().map(|it| it.tag()).collect()),
            "final cancelled max_steps timeout failed capped"
        );
        assert_eq!(
            tags(ToolOutcome::ALL.iter().map(|it| it.tag()).collect()),
            "ran refused command_failed cancelled"
        );
        assert_eq!(
            tags(ToolRefusal::ALL.iter().map(|it| it.tag()).collect()),
            "not_allowed max_steps unparsable unknown_tool denylist declined"
        );
        assert_eq!(
            tags(ApprovalScope::ALL.iter().map(|it| it.tag()).collect()),
            "once session workspace preseeded off"
        );
        assert_eq!(
            tags(EngineIdentity::ALL.iter().map(|it| it.tag()).collect()),
            "checked_commit literal_matched"
        );
        assert_eq!(
            tags(Lane::ALL.iter().map(|it| it.tag()).collect()),
            "trunk interview"
        );
        assert_eq!(
            tags(Warrant::ALL.iter().map(|it| it.tag()).collect()),
            "read scoping"
        );
        assert_eq!(
            tags(ForkOutcome::ALL.iter().map(|it| it.tag()).collect()),
            "value decline unparseable truncated failed cancelled"
        );
        assert_eq!(
            tags(PatchOp::ALL.iter().map(|it| it.tag()).collect()),
            "add supersede resolve retire park"
        );
        assert_eq!(
            tags(SeamReason::ALL.iter().map(|it| it.tag()).collect()),
            "operator phase budget cadence"
        );
    }

    /// A `tool_call`'s `isolation` and `network` are the drive's own words:
    /// `diet/src/isolation`'s `Isolation::tag` and `Network::tag`, every one,
    /// in order, plus `unrecorded` and nothing else (v3; `unrecorded` ruled
    /// on #297 for a placed replay, which the drive never writes). Two
    /// spellings of one policy are two readers that can disagree.
    #[test]
    fn the_policy_words_are_the_isolation_modules() {
        use crate::isolation;
        let plus_unrecorded = |mut words: Vec<&'static str>| {
            words.push("unrecorded");
            words
        };
        assert_eq!(
            Isolation::ALL.iter().map(|it| it.tag()).collect::<Vec<_>>(),
            plus_unrecorded(
                isolation::Isolation::ALL
                    .iter()
                    .map(|it| it.tag())
                    .collect::<Vec<_>>()
            )
        );
        assert_eq!(
            Network::ALL.iter().map(|it| it.tag()).collect::<Vec<_>>(),
            plus_unrecorded(
                isolation::Network::ALL
                    .iter()
                    .map(|it| it.tag())
                    .collect::<Vec<_>>()
            )
        );
    }

    /// A STREAM'S TEXT AND ITS BYTE COUNT COME TOGETHER (v3, ruled on #297
    /// Q3), refused by that rule itself: each half of each stream left out
    /// of a `ran` line is refused naming the pair, not only by the outcome's
    /// required keys, which would refuse it too and say less.
    #[test]
    fn a_streams_text_and_its_byte_count_come_together() {
        let ran = concat!(
            r#"{"argv":["true"],"arguments":"{}","confined":["true"],"exit":0,"#,
            r#""id":"c","isolation":"none","kind":"tool_call","name":"bash","network":"host","#,
            r#""outcome":"ran","request":3,"seq":9,"stderr":"","stderr_bytes":0,"#,
            r#""stdout":"","stdout_bytes":0,"t":45,"turn":1}"#
        );
        let object = json::line(ran).expect("an object");
        line(ran).expect("the whole line reads");
        for (text_key, bytes_key) in STREAMS {
            for (dropped, kept) in [(text_key, bytes_key), (bytes_key, text_key)] {
                let mut without = object.clone();
                without.remove(*dropped);
                let mut rendered = String::new();
                json::render(&Value::Object(without), &mut rendered);
                let refused = line(&rendered).expect_err("half a stream was read");
                assert!(
                    refused.contains(&format!("carries `{kept}` without `{dropped}`")),
                    "{refused}"
                );
            }
        }
    }

    /// A STREAM'S BYTE COUNT IS `0` EXACTLY WHEN ITS TEXT IS EMPTY, AND IS
    /// NOT OTHERWISE COMPARED WITH IT (v3, ruled on #297 at 5975957135):
    /// each stream's empty text beside a non-zero count is refused, and its
    /// text beside a zero count is refused, each naming both keys; a text
    /// whose length differs from its count -- a lossy decoding of output that
    /// is not UTF-8 -- still reads.
    #[test]
    fn a_streams_byte_count_is_zero_exactly_when_its_text_is_empty() {
        let ran = |stream: &str, text: &str, bytes: u64| {
            let (other, _) = STREAMS
                .iter()
                .find(|(key, _)| *key != stream)
                .expect("two streams");
            format!(
                concat!(
                    r#"{{"argv":["true"],"arguments":"{{}}","confined":["true"],"exit":0,"#,
                    r#""id":"c","isolation":"none","kind":"tool_call","name":"bash","network":"host","#,
                    r#""outcome":"ran","request":3,"seq":9,"{stream}":"{text}","{stream}_bytes":{bytes},"#,
                    r#""{other}":"","{other}_bytes":0,"t":45,"turn":1}}"#
                ),
                stream = stream,
                text = text,
                bytes = bytes,
                other = other,
            )
        };
        for (stream, bytes_key) in STREAMS {
            let refused = line(&ran(stream, "", 1)).expect_err("empty text with a count was read");
            assert!(
                refused.contains(&format!("`{stream}` is empty and `{bytes_key}` is 1")),
                "{refused}"
            );
            let refused = line(&ran(stream, "a", 0)).expect_err("text with no bytes was read");
            assert!(
                refused.contains(&format!("`{stream}` carries text and `{bytes_key}` is 0")),
                "{refused}"
            );
            line(&ran(stream, "\u{fffd}", 1))
                .unwrap_or_else(|why| panic!("a lossy `{stream}` was refused: {why}"));
            line(&ran(stream, "a", 1))
                .unwrap_or_else(|why| panic!("a whole `{stream}` was refused: {why}"));
        }
    }

    /// EVERY KEY A `tool_call` MUST CARRY, AND EVERY KEY IT MAY NOT, FOR EACH
    /// OUTCOME AND NAME (v3, ruled on #297: the design, Q1, 5974151057,
    /// 5974672735, 5974732908, 5974810417, 5975651100, 5975827372,
    /// 5975957135). The table below is
    /// the rule as ruled, written out here rather than read from
    /// [`outcome_keys`], so an entry gone from either list there is a case
    /// here the reader no longer refuses. From a line that reads, each
    /// required key dropped is refused naming it, each forbidden key added is
    /// refused naming it, and each optional key dropped (where the line
    /// carries it) or added (where it does not) still reads.
    ///
    /// Two rows for each `unrecorded` alone, without a claim: `isolation` and
    /// `network` admit it each on its own (5975651100). A non-`bash` call's
    /// streams are optional under `ran` and `command_failed`, written both
    /// with and without them (5975651100); a `bash` call's are required.
    /// `policy` is required of a `bash` `command_failed` and forbidden on
    /// every other name (5975957135). A refused `bash` call carries `argv`
    /// for each reason that parsed (`not_allowed`, `max_steps`) and for none
    /// that did not (`unparsable`, `unknown_tool`), and a refused `read`
    /// carries none whatever its reason (5974810417, 5974672735).
    ///
    /// A stream's text and its byte count come together
    /// ([`a_streams_text_and_its_byte_count_come_together`]), so a text key
    /// is dropped or added with its byte count, and the refusal must name the
    /// text key. A byte count alone is refused by the pairing whatever the
    /// outcome lists say, so its entry in them changes nothing a line can
    /// show.
    ///
    /// `approval` (v4, ruled at 5981588394 (c)) is optional under `ran`,
    /// `command_failed` and `cancelled`, on every name, and forbidden under
    /// `refused`; a refused `bash` call carries `argv` for `denylist` and
    /// `declined` too (ratified at 5982002587), and a refused `read` carries
    /// none (#388).
    ///
    /// `cwd` (v4, ruled at 5982826236) is written beside every `argv`, and
    /// is dropped and added with it, as a stream's byte count is with its
    /// text. To this line reader it is optional wherever `argv` is carried,
    /// and forbidden wherever `argv` is: a `cwd` without an `argv` is every
    /// line's refusal. That an `argv` carries its `cwd` is the whole log's
    /// rule in a log that declares v4
    /// ([`a_bash_call_in_a_v4_log_says_where_it_ran`]).
    ///
    /// `files` (v4, ruled at 5983588924) is optional under `ran` and
    /// `command_failed`, on every name, and forbidden under `refused` and
    /// `cancelled`.
    ///
    /// The names are `bash` and two others, `read` and `edit`, so a reader
    /// that singled out one other name rather than `bash` goes red too.
    #[test]
    #[allow(clippy::too_many_lines)] // one table, written out
    fn a_tool_calls_keys_fit_its_outcome_and_name() {
        // (what, the line, required, forbidden, optional)
        type Row = (
            &'static str,
            String,
            Vec<&'static str>,
            Vec<&'static str>,
            Vec<&'static str>,
        );
        const POLICY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        const STREAMS_HELD: &str = r#""stderr":"","stderr_bytes":0,"stdout":"","stdout_bytes":0,"#;
        const BASH_STREAMS: &[&str] = &["stdout", "stdout_bytes", "stderr", "stderr_bytes"];
        let exec = |isolation: &str, network: &str| {
            format!(
                r#""argv":["true"],"cwd":"/work/tree","confined":["true"],"isolation":"{isolation}","network":"{network}","#
            )
        };
        let line_of = |name: &str, outcome: &str, rest: &str| {
            format!(
                r#"{{{rest}"arguments":"{{}}","id":"c","kind":"tool_call","name":"{name}","outcome":"{outcome}","request":3,"seq":9,"t":45,"turn":1}}"#
            )
        };
        let policy = format!(r#""policy":"{POLICY}","#);
        let ran_bash = format!("{}\"exit\":0,{STREAMS_HELD}", exec("sandbox", "none"));
        let ran_unrecorded = format!(
            r#""argv":["true"],"cwd":"/work/tree","exit":0,"isolation":"unrecorded","network":"unrecorded",{STREAMS_HELD}"#
        );
        let ran_isolation_unrecorded = format!(
            r#""argv":["true"],"cwd":"/work/tree","exit":0,"isolation":"unrecorded","network":"none",{STREAMS_HELD}"#
        );
        let ran_network_unrecorded =
            format!("{}\"exit\":0,{STREAMS_HELD}", exec("sandbox", "unrecorded"));
        let failed_bash = format!(
            "{}\"exit\":1,{policy}{STREAMS_HELD}",
            exec("sandbox", "none")
        );
        let failed_vm = format!("{}\"exit\":1,{policy}{STREAMS_HELD}", exec("vm", "none"));
        let failed_unprofiled = format!("{}\"exit\":1,{STREAMS_HELD}", exec("none", "none"));
        let failed_unrecorded = format!(
            r#""argv":["true"],"cwd":"/work/tree","exit":1,"isolation":"unrecorded","network":"unrecorded",{STREAMS_HELD}"#
        );
        let cancelled_bash = exec("sandbox", "none");
        let cancelled_unrecorded =
            r#""argv":["true"],"cwd":"/work/tree","isolation":"unrecorded","network":"unrecorded","#.to_owned();
        let read_ran = format!("\"exit\":0,{STREAMS_HELD}");
        let read_failed = format!("\"exit\":1,{STREAMS_HELD}");
        let read_failed_bare = "\"exit\":1,".to_owned();

        let streams_and = |more: &[&'static str]| -> Vec<&'static str> {
            BASH_STREAMS
                .iter()
                .copied()
                .chain(more.iter().copied())
                .collect()
        };
        let not_run = |more: &[&'static str]| -> Vec<&'static str> {
            ["exit", "policy", "files"]
                .into_iter()
                .chain(BASH_STREAMS.iter().copied())
                .chain(more.iter().copied())
                .collect()
        };
        // A refused call ran under no decision: `approval` is barred (v4).
        let refused_bars = |more: &[&'static str]| -> Vec<&'static str> {
            not_run(more).into_iter().chain(["approval"]).collect()
        };
        let streams_or_approval = || vec!["stdout", "stderr", "approval", "files"];
        let table: Vec<Row> = vec![
            (
                "bash ran",
                line_of("bash", "ran", &ran_bash),
                streams_and(&["argv", "confined", "isolation", "network"]),
                vec!["reason", "policy"],
                vec!["exit", "approval", "cwd", "files"],
            ),
            (
                "bash ran, unrecorded",
                line_of("bash", "ran", &ran_unrecorded),
                streams_and(&["argv", "isolation", "network"]),
                vec!["reason", "policy", "confined"],
                vec!["exit", "approval", "cwd", "files"],
            ),
            (
                "bash ran, only isolation unrecorded",
                line_of("bash", "ran", &ran_isolation_unrecorded),
                streams_and(&["argv", "isolation", "network"]),
                vec!["reason", "policy", "confined"],
                vec!["approval", "cwd", "files"],
            ),
            (
                "bash ran, only network unrecorded",
                line_of("bash", "ran", &ran_network_unrecorded),
                streams_and(&["argv", "confined", "isolation", "network"]),
                vec!["reason", "policy"],
                vec!["approval", "cwd", "files"],
            ),
            (
                "bash command_failed",
                line_of("bash", "command_failed", &failed_bash),
                streams_and(&["argv", "confined", "isolation", "network", "policy"]),
                vec!["reason"],
                vec!["exit", "approval", "cwd", "files"],
            ),
            (
                "bash command_failed under vm",
                line_of("bash", "command_failed", &failed_vm),
                streams_and(&["argv", "confined", "isolation", "network", "policy"]),
                vec!["reason"],
                vec!["approval", "cwd", "files"],
            ),
            (
                "bash command_failed under no profile",
                line_of("bash", "command_failed", &failed_unprofiled),
                streams_and(&["argv", "confined", "isolation", "network"]),
                vec!["reason", "policy"],
                vec!["approval", "cwd", "files"],
            ),
            (
                "bash command_failed, unrecorded",
                line_of("bash", "command_failed", &failed_unrecorded),
                streams_and(&["argv", "isolation", "network"]),
                vec!["reason", "policy", "confined"],
                vec!["approval", "cwd", "files"],
            ),
            (
                "bash refused not_allowed",
                line_of(
                    "bash",
                    "refused",
                    r#""argv":["rm"],"cwd":"/work/tree","reason":"not_allowed","#,
                ),
                vec!["reason", "argv"],
                refused_bars(&["confined", "isolation", "network"]),
                vec!["cwd"],
            ),
            (
                "bash refused max_steps",
                line_of(
                    "bash",
                    "refused",
                    r#""argv":["true"],"cwd":"/work/tree","reason":"max_steps","#,
                ),
                vec!["reason", "argv"],
                refused_bars(&["confined", "isolation", "network"]),
                vec!["cwd"],
            ),
            (
                "bash refused unparsable",
                line_of("bash", "refused", r#""reason":"unparsable","#),
                vec!["reason"],
                refused_bars(&["argv", "cwd", "confined", "isolation", "network"]),
                vec![],
            ),
            (
                "bash refused unknown_tool",
                line_of("bash", "refused", r#""reason":"unknown_tool","#),
                vec!["reason"],
                refused_bars(&["argv", "cwd", "confined", "isolation", "network"]),
                vec![],
            ),
            (
                "bash refused denylist",
                line_of(
                    "bash",
                    "refused",
                    r#""argv":["rm","-rf","/"],"cwd":"/work/tree","reason":"denylist","#,
                ),
                vec!["reason", "argv"],
                refused_bars(&["confined", "isolation", "network"]),
                vec!["cwd"],
            ),
            (
                "bash refused declined",
                line_of(
                    "bash",
                    "refused",
                    r#""argv":["true"],"cwd":"/work/tree","reason":"declined","#,
                ),
                vec!["reason", "argv"],
                refused_bars(&["confined", "isolation", "network"]),
                vec!["cwd"],
            ),
            (
                "bash cancelled",
                line_of("bash", "cancelled", &cancelled_bash),
                vec!["isolation", "network"],
                not_run(&["reason"]),
                vec!["confined", "approval", "cwd"],
            ),
            (
                "bash cancelled, unrecorded",
                line_of("bash", "cancelled", &cancelled_unrecorded),
                vec!["isolation", "network"],
                not_run(&["reason", "confined"]),
                vec!["approval", "cwd"],
            ),
            (
                "read ran",
                line_of("read", "ran", &read_ran),
                vec![],
                vec![
                    "reason",
                    "policy",
                    "argv",
                    "cwd",
                    "confined",
                    "isolation",
                    "network",
                ],
                streams_or_approval(),
            ),
            (
                "read ran, no streams",
                line_of("read", "ran", ""),
                vec![],
                vec![
                    "reason",
                    "policy",
                    "argv",
                    "cwd",
                    "confined",
                    "isolation",
                    "network",
                ],
                streams_or_approval(),
            ),
            (
                "read command_failed",
                line_of("read", "command_failed", &read_failed),
                vec![],
                vec![
                    "reason",
                    "policy",
                    "argv",
                    "cwd",
                    "confined",
                    "isolation",
                    "network",
                ],
                streams_or_approval(),
            ),
            (
                "read command_failed, no streams",
                line_of("read", "command_failed", &read_failed_bare),
                vec![],
                vec![
                    "reason",
                    "policy",
                    "argv",
                    "cwd",
                    "confined",
                    "isolation",
                    "network",
                ],
                streams_or_approval(),
            ),
            (
                "read refused",
                line_of("read", "refused", r#""reason":"unknown_tool","#),
                vec!["reason"],
                refused_bars(&["argv", "cwd", "confined", "isolation", "network"]),
                vec![],
            ),
            (
                "read refused not_allowed",
                line_of("read", "refused", r#""reason":"not_allowed","#),
                vec!["reason"],
                refused_bars(&["argv", "cwd", "confined", "isolation", "network"]),
                vec![],
            ),
            (
                "read refused declined",
                line_of("read", "refused", r#""reason":"declined","#),
                vec!["reason"],
                refused_bars(&["argv", "cwd", "confined", "isolation", "network"]),
                vec![],
            ),
            (
                "read cancelled",
                line_of("read", "cancelled", ""),
                vec![],
                not_run(&["reason", "argv", "cwd", "confined", "isolation", "network"]),
                vec!["approval"],
            ),
        ];
        // Every name but `bash` is one rule, so each `read` row is also an
        // `edit` row: a reader that took `read` for the only other name
        // would accept the `edit` line's exec keys.
        let table: Vec<_> = table
            .into_iter()
            .flat_map(|(what, text, required, forbidden, optional)| {
                let edit = what.strip_prefix("read ").map(|rest| {
                    (
                        format!("edit {rest}"),
                        text.replace(r#""name":"read""#, r#""name":"edit""#),
                        required.clone(),
                        forbidden.clone(),
                        optional.clone(),
                    )
                });
                std::iter::once((what.to_owned(), text, required, forbidden, optional)).chain(edit)
            })
            .collect();
        let value_of = |key: &str| -> Value {
            if key == "argv" || key == "confined" {
                Value::Array(vec![Value::String("true".to_owned())])
            } else if key == "isolation" {
                Value::String("sandbox".to_owned())
            } else if key == "network" {
                Value::String("none".to_owned())
            } else if key == "reason" {
                Value::String("not_allowed".to_owned())
            } else if key == "policy" {
                Value::String(POLICY.to_owned())
            } else if key == "stdout" || key == "stderr" {
                Value::String(String::new())
            } else if key == "approval" {
                Value::Object(BTreeMap::from([
                    ("decided_at".to_owned(), Value::Integer(40)),
                    ("scope".to_owned(), Value::String("once".to_owned())),
                    ("why".to_owned(), Value::String("not_approved".to_owned())),
                ]))
            } else if key == "cwd" {
                Value::String("/work/tree".to_owned())
            } else if key == "files" {
                Value::Array(vec![Value::Object(BTreeMap::from([
                    ("bytes".to_owned(), Value::Integer(1)),
                    (
                        "media_type".to_owned(),
                        Value::String("text/plain".to_owned()),
                    ),
                    ("path".to_owned(), Value::String("out/a.txt".to_owned())),
                    ("sha256".to_owned(), Value::String(POLICY.to_owned())),
                ]))])
            } else {
                Value::Integer(0)
            }
        };
        // A key's partner goes and comes with it: a stream's byte count with
        // its text, and `cwd` with `argv` (v4), so a probe of `argv` tests
        // `argv`'s own rule rather than the pairing's.
        let partner = |key: &str| {
            if key == "argv" {
                return Some("cwd");
            }
            STREAMS
                .iter()
                .find(|(text, _)| *text == key)
                .map(|(_, bytes)| *bytes)
        };
        let read_back = |object: &BTreeMap<String, Value>| {
            let mut rendered = String::new();
            json::render(&Value::Object(object.clone()), &mut rendered);
            line(&rendered)
        };
        let mut probed = 0;
        for (what, text, required, forbidden, optional) in &table {
            line(text).unwrap_or_else(|why| panic!("{what}: the line does not read: {why}"));
            let object = json::line(text).expect("an object");
            for key in required {
                let mut without = object.clone();
                assert!(without.remove(*key).is_some(), "{what}: carries no `{key}`");
                if let Some(bytes) = partner(key) {
                    without.remove(bytes);
                }
                let refused = read_back(&without)
                    .expect_err(&format!("{what}: a line without `{key}` was read"));
                assert!(
                    refused.contains(&format!("`{key}`")),
                    "{what}, without `{key}`: {refused}"
                );
                probed += 1;
            }
            for key in forbidden {
                let mut with = object.clone();
                assert!(
                    with.insert((*key).to_owned(), value_of(key)).is_none(),
                    "{what}: already carries `{key}`"
                );
                if let Some(bytes) = partner(key) {
                    with.insert(bytes.to_owned(), value_of(bytes));
                }
                let refused = read_back(&with)
                    .expect_err(&format!("{what}: a line carrying `{key}` was read"));
                assert!(
                    refused.contains(&format!("`{key}`")),
                    "{what}, with `{key}`: {refused}"
                );
                probed += 1;
            }
            for key in optional {
                let mut changed = object.clone();
                let carried = changed.remove(*key).is_some();
                if carried {
                    if let Some(bytes) = partner(key) {
                        changed.remove(bytes);
                    }
                } else {
                    changed.insert((*key).to_owned(), value_of(key));
                    if let Some(bytes) = partner(key) {
                        changed.insert(bytes.to_owned(), value_of(bytes));
                    }
                }
                read_back(&changed).unwrap_or_else(|why| {
                    panic!(
                        "{what}, {} `{key}`: refused: {why}",
                        if carried { "without" } else { "with" }
                    )
                });
                probed += 1;
            }
        }
        assert!(probed > 0, "nothing was probed");
    }
}
