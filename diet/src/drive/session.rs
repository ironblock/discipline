//! An interactive session: a person asks, the trunk answers streamed, and
//! the session says at every moment what it is doing (#117, R2).
//!
//! [`super::run`] is the scripted gym -- three turns from a TOML file, the
//! trunk rebuilt from the rendered object on every one. This is the other
//! thing #117 names: an ask that comes from a person, whenever they send it,
//! answered on a trunk that is APPENDED until a seam and never rebuilt. The
//! dogma's `Site::Fork` is "a disposable, single-turn fork off the warm tail
//! of the canonical session"; a warm tail only exists if the trunk's prefix
//! is the same bytes from one turn to the next, and that is the property
//! [`tests::the_trunk_is_appended_never_rebuilt`] holds.
//!
//! # The settlement is a state machine, and a command is checked against it
//!
//! [`Settlement`] is `awaiting | turn | capture | ended`. An ask is accepted
//! only while `awaiting`; one sent while a turn or a capture is in flight is
//! REFUSED, by name ([`Refusal::InFlight`]), and the refusal is an event in
//! the log -- a person who pressed send and saw nothing happen deserves to
//! be told why, and so does whoever reads the log afterwards. `capture` is
//! where the idle-gap interview runs (#374): after a turn settles `final`,
//! at most one fork, when the regimen's [`INTERVIEW_WARRANT`] warrants it
//! (see [`Interview`]); a gap with no warrant passes through `capture`
//! straight back to `awaiting`, and says so in the log rather than skipping
//! the state.
//!
//! # A cancel reaches the call
//!
//! [`Session::cancel`] asks the in-flight call to stop through
//! [`crate::client::stream::Cancel`], which WAKES a transport blocked
//! mid-answer rather than setting a flag nobody reads until the answer is
//! done. What arrived before the stop is kept as partial text on
//! [`Event::Cancelled`] and is never an answer: a cancelled turn, like a
//! failed one, does not go on the trunk. The trunk carries settled exchanges
//! only, so the next ask is sent on exactly the prefix the last settled turn
//! left.
//!
//! # The log is the source of truth
//!
//! Every change is an [`Event`] appended to one log, numbered by its position
//! in it: sequence numbers are gapless and increasing by construction, and
//! [`Session::events_from`] is how anything -- a test, R2c's SSE stream, a
//! person reconnecting -- learns what happened. The settlement and the trunk
//! are kept beside it for the commands to check, and every change to either
//! is logged in the same critical section, so the log never says less than
//! the state.
//!
//! **A seam** (#493): [`Session::declare_seam`] refills the trunk from
//! working memory and appends from there; the seam's audit and pre-warm are
//! not built yet, and its docs say so. The log is in memory. It is a format,
//! `diet/formats/log` (ruled on #117), and what each event carries is what
//! R2c's plan (`diet/drive/plans/r2c-proposal.md`, D4) drafts for it: a
//! header line, a turn counter, a `request` per call that the call's other
//! events cite by its sequence number, and a `turn.settled` with a reason.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::capture::mechanical::Facts;
use crate::capture::router::{self, AskKind, Class, Router, Routing};
use crate::client::CAPPED_FINISH_REASONS;
use crate::client::shape::{Concurrency, Message, RequestShape, Role, Serving, ToolCall};
use crate::client::stream::{
    Cancel, Ended as StreamEnded, Piece, Progress, Rejection, Streaming, Timings,
};
use crate::client::transport::TransportFailure;
use crate::client::vocabulary;
use crate::formats::log;
use crate::formats::record::Event as RecordEvent;
use crate::formats::record::json::Value;
use crate::formats::regimen::{self, Regimen};
use crate::object::{Patch, WorkingObject};

use super::shell_gate::{Outcome as GateOutcome, Scope};
use super::tool_loop::{
    self, BASH, Call, Calls, Counts, Decider, Decision, Entry, Judged, Prompt, Tools,
};

vocabulary! {
    /// What the session is doing.
    Settlement {
        /// Nothing in flight; an ask is welcome.
        Awaiting => "awaiting",
        /// The trunk is answering an ask.
        Turn => "turn",
        /// The idle-gap work after a settled turn (R4's interviews).
        Capture => "capture",
        /// The session is over; nothing more is accepted.
        Ended => "ended",
    }
}

vocabulary! {
    /// What a person can ask the session to do.
    CommandKind {
        /// Send an ask to the trunk.
        Ask => "ask",
        /// Stop the call in flight.
        Cancel => "cancel",
        /// Declare a seam (R6).
        DeclareSeam => "declare-seam",
        /// End the session.
        End => "end",
        /// Open a tangent (#22).
        OpenTangent => "open-tangent",
        /// Close the open tangent (#22).
        CloseTangent => "close-tangent",
        /// Move the running command to the background (#614).
        Background => "background",
    }
}

vocabulary! {
    /// Why a command was refused.
    Refusal {
        /// A turn or a capture is in flight.
        InFlight => "in-flight",
        /// The session has ended.
        Ended => "ended",
        /// There is no call in flight to stop.
        NothingInFlight => "nothing-in-flight",
        /// A seam has nothing to refill from: no turn has settled, or working
        /// memory holds no entry (#493).
        NothingToSeam => "nothing-to-seam",
        /// A seam named a phase to move to, and the session has no phase
        /// graph (#563).
        NoPhaseGraph => "no-phase-graph",
        /// A seam named a phase the graph does not declare.
        NotAPhase => "not-a-phase",
        /// A seam named the phase the session is already in.
        AlreadyInPhase => "already-in-phase",
        /// A seam named a move the graph does not allow from here.
        NoPhaseEdge => "no-phase-edge",
        /// A cancel named a turn older than the latest one: it arrived after
        /// that turn settled and must not stop the next (the admission
        /// counter ruled on #117).
        Stale => "stale",
        /// A seam, or a second tangent, while a tangent is open (#22).
        TangentOpen => "tangent-open",
        /// A close with no tangent open (#22).
        NoTangent => "no-tangent",
        /// A tangent asked of a session that keeps no working memory, or
        /// under an id it cannot take (#22).
        BadTangent => "bad-tangent",
        /// A close whose dispositions are not exactly the tangent's entries
        /// (#22).
        NotTheScope => "not-the-scope",
        /// A move to the background with no command running (#614).
        NothingRunning => "nothing-running",
    }
}

vocabulary! {
    /// How a turn ended, as `turn.settled` says it (ruled on #117).
    SettleReason {
        /// The trunk answered.
        Final => "final",
        /// A stop reached the call.
        Cancelled => "cancelled",
        /// The tool loop ran out of steps. Nothing emits it until the tool
        /// loop exists; it is here because the ruling names it.
        MaxSteps => "max_steps",
        /// The call ran out of time.
        Timeout => "timeout",
        /// The call failed, was refused by the server, or its thread crashed.
        Failed => "failed",
    }
}

vocabulary! {
    /// What ended an idle gap: the command that carried it.
    GapEnd {
        /// An ask.
        Ask => "ask",
        /// A seam.
        Seam => "seam",
        /// A stop.
        Cancel => "cancel",
        /// The end of the session.
        End => "end",
    }
}

/// An idle gap, as the surface measured it and sent it with the command that
/// ended it (#117, Q4 and D13 (c)): five durations in integer milliseconds
/// on the surface's own clock, what ended it, and the `turn.settled` that
/// opened it. Durations only, never content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdleGap {
    /// The sequence number of the [`Event::TurnSettled`] that opened it.
    pub opened_by: u64,
    /// From the settling to the first sign the person was present.
    pub notice: u64,
    /// From the end of `notice` to the first keystroke or seam click.
    pub read: u64,
    /// From the first keystroke to the accepted send or declare.
    pub compose: u64,
    /// Time the page was hidden, taken out of the phase it interrupted.
    pub away: u64,
    /// From the first send refused because work was in flight to the
    /// accepted send.
    pub blocked: u64,
    /// What ended it.
    pub ended_by: GapEnd,
}

/// Why a gap sent with a command was not logged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapError {
    /// It does not cite the latest settling, or that settling's gap was
    /// already logged: a gap is opened by the latest `turn.settled`, once.
    NotTheOpenGap {
        /// The settling it cited.
        opened_by: u64,
    },
    /// A duration too large for the log's integers to hold.
    NotACount,
    /// It says a different command ended it than the one carrying it.
    EndedByAnotherCommand {
        /// What it says.
        says: GapEnd,
        /// The command that carried it.
        carried_by: CommandKind,
    },
}

vocabulary! {
    /// Which lane a request is made on.
    Lane {
        /// The canonical session.
        Trunk => "trunk",
        /// A fork's single call off the trunk's warm tail, never appended to
        /// it (#374).
        Interview => "interview",
        /// A seam's audit (#504): a fork's call, before the refill.
        Audit => "audit",
    }
}

/// One thing that happened in a session.
// `Started` is the large one, and it is built once per session, at open: the
// size each other event carries for it is not worth a box per field.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The session opened. Always the first event, at sequence number zero.
    Started {
        /// When it opened: milliseconds since the Unix epoch, measured once.
        /// With the sequence number it is what tells one process's log from
        /// another's.
        opened: u64,
        /// The model name every request is sent with. A name, not an
        /// identity: nothing here claims which weights answer to it.
        model: String,
        /// The messages the trunk starts from.
        head: Vec<Message>,
        /// What serves the session, as its caller declared it: log v2's
        /// `serving` (#292). `None` when nothing was declared.
        serving: Option<Serving>,
        /// What it claims serves it: log v3's substrate claim (#292), when
        /// `serve` started it against a regimen whose engine check passed.
        claim: Option<log::SubstrateClaim>,
        /// The tools its requests declare, by name, in order (#472): empty
        /// when they declare none.
        tools: Vec<String>,
        /// The template variables every request carries (R1), as sent:
        /// empty when it sends none.
        template_kwargs: BTreeMap<String, Value>,
        /// What the regime declares and no request carries (R1).
        unsent: Option<log::Unsent>,
        /// Whether approvals were off: the approval lever's `none`.
        approvals_off: bool,
        /// The fork delivery lever's state, for a session that forks.
        fork_delivery: Option<log::ForkDelivery>,
        /// The ask set a session that forks asks in (#595).
        fork_asks: Option<&'static crate::dogma::asks::AskSet>,
        /// The template's default level, when thinking is on and none is
        /// sent.
        reasoning_effort_default: Option<String>,
        /// The instruction files the system prompt carries (#559), by path
        /// and digest: empty when none were injected.
        instruction_files: Vec<log::InstructionFile>,
        /// Each lever's state, as the record's start row names them (#573):
        /// `serve_levers`' one reading, when a regimen was read.
        levers: Option<BTreeMap<String, String>>,
        /// The cap tool outputs arrive under, for a session that runs tools
        /// (#554).
        tool_output: Option<super::output::OutputCap>,
        /// A `bash` call's default timeout, in milliseconds, 0 for none
        /// (#613), when the session runs commands.
        bash_timeout_ms: Option<u64>,
        /// The phase graph it runs under, as the log names it (#563): its
        /// phases and allowed moves, and the phase it opens in; `None` when
        /// the regimen declares none.
        phases: Option<(Vec<String>, Vec<log::PhaseMove>, String)>,
    },
    /// An ask was accepted, and a turn begins on it.
    Asked {
        /// The turn it begins: 1 for the first ask admitted, then counting.
        turn: u32,
        /// What the person asked.
        text: String,
        /// Whether the operator marked it a scoping question (#374, ruled
        /// 5985110649): the warrant's rule (b).
        scoping: bool,
        /// The files the operator attached to it, by reference, in the order
        /// they were attached (#372, ruled 5989411005): the copies in the
        /// recording's directory. Empty when nothing was attached, and when
        /// something was but no recording keeps a copy.
        files: Vec<log::RecordedFile>,
    },
    /// A call was made to the model. Every event the call produces names
    /// this event by its sequence number.
    Requested {
        /// The turn the call belongs to.
        turn: u32,
        /// The lane it was made on.
        lane: Lane,
        /// The sha256 of the request's frozen head, as `client::head` hashes
        /// it: what a live record's request names (#157).
        head_sha256: String,
        /// The sequence number of the [`Event::Forked`] it is the call of,
        /// on the [`Lane::Interview`] lane; `None` on the trunk (#374).
        fork: Option<u64>,
        /// The `max_tokens` it was sent with, after the clamp (#588).
        max_tokens: u32,
    },
    /// The settlement moved.
    Settled {
        /// Where it was.
        from: Settlement,
        /// Where it is now.
        to: Settlement,
    },
    /// A command was refused.
    Refused {
        /// Which command.
        command: CommandKind,
        /// Why.
        because: Refusal,
        /// What the session was doing when it refused.
        during: Settlement,
    },
    /// A piece of the trunk's reasoning arrived: what a thinking model
    /// streams before its answer. It joins the trunk with the answer (#117,
    /// Q10).
    Reasoning {
        /// The sequence number of the [`Event::Requested`] it belongs to.
        request: u64,
        /// The piece, as the server sent it.
        text: String,
    },
    /// The server's count of the call's prompt, prefilled so far (#117 R3):
    /// one per frame it streams, before the call's first piece.
    Progress {
        /// The sequence number of the [`Event::Requested`] it counts.
        request: u64,
        /// The frame, as the server sent it.
        progress: Progress,
    },
    /// A piece of the trunk's answer arrived.
    Delta {
        /// The sequence number of the [`Event::Requested`] it answers.
        request: u64,
        /// The piece, as the server sent it.
        text: String,
    },
    /// A stop was asked for the call in flight. What the call did about it
    /// is the next terminal event of the turn, which may be any of them:
    /// [`Event::Cancelled`] if it stopped, [`Event::Answered`] if the answer
    /// was already done, [`Event::Capped`] if the cap ended it first, or
    /// [`Event::Rejected`], [`Event::Failed`] or
    /// [`Event::Crashed`] if the call ended some other way first.
    StopAsked {
        /// The turn the stop was asked for.
        turn: u32,
    },
    /// The trunk answered. The ask and this answer are now on the trunk.
    Answered {
        /// The sequence number of the [`Event::Requested`] it answers.
        request: u64,
        /// The whole answer: every [`Event::Delta`] of this turn, in order.
        text: String,
        /// Why the server stopped, as it spelled it, if it said.
        finish_reason: Option<String>,
        /// The whole reasoning: every [`Event::Reasoning`] of this turn, in
        /// order, byte for byte -- the same string the trunk carries.
        reasoning: Option<String>,
        /// What the server measured of the call, as it reported it.
        timings: Option<Timings>,
        /// What a hosted API said beside the answer (#555).
        hosted: Hosted,
    },
    /// The call hit its output cap (a `finish_reason` in the client's
    /// [`crate::client::CAPPED_FINISH_REASONS`], `length` among them): what it
    /// streamed is not an answer, so neither its ask nor `text` is on the
    /// trunk, and the turn settles `failed`. Written as a `response` with
    /// `capped` (ruled on #290, comment 5969297103; log v3's `capped` settle
    /// word replaces `failed` here once it applies, #297).
    Capped {
        /// The sequence number of the [`Event::Requested`] it answers.
        request: u64,
        /// The text streamed before the cap, which may be none.
        text: String,
        /// Why the server stopped, as it spelled it.
        finish_reason: Option<String>,
        /// The reasoning streamed before the cap.
        reasoning: Option<String>,
        /// What the server measured of the call, as it reported it.
        timings: Option<Timings>,
        /// What a hosted API said beside it (#555).
        hosted: Hosted,
    },
    /// The turn was stopped. Neither its ask nor `partial` is on the trunk.
    Cancelled {
        /// The sequence number of the [`Event::Requested`] that was stopped.
        request: u64,
        /// What arrived before the stop. Never an answer.
        partial: String,
    },
    /// The server refused the turn rather than answering it. Neither its ask
    /// nor `partial` is on the trunk.
    Rejected {
        /// The sequence number of the [`Event::Requested`] refused.
        request: u64,
        /// The HTTP status, or `200` for an `error` event inside a stream.
        status: u16,
        /// What the server said.
        body: String,
        /// What kind of refusal its typed field says it is, if one this
        /// client names.
        class: Option<Rejection>,
        /// What arrived before the refusal.
        partial: String,
        /// Its prompt against the window, when it overflowed it (#616).
        overflow: Option<log::Overflow>,
    },
    /// The turn's own thread panicked, or could not be started, before it
    /// could settle. Neither its ask nor anything that arrived is on the
    /// trunk; what arrived is in the [`Event::Delta`]s before this.
    Crashed {
        /// The sequence number of the [`Event::Requested`] whose thread
        /// crashed.
        request: u64,
        /// What arrived before it crashed: every answer piece logged for this
        /// request. Never an answer.
        partial: String,
        /// Why: the panic's message, or the operating system's refusal.
        why: String,
    },
    /// The turn's call failed. Neither its ask nor `partial` is on the trunk.
    Failed {
        /// The sequence number of the [`Event::Requested`] that failed.
        request: u64,
        /// Its prompt against the window, when it overflowed it (#616):
        /// never for a timeout.
        overflow: Option<log::Overflow>,
        /// How it failed.
        failure: TransportFailure,
        /// What arrived before it failed.
        partial: String,
    },
    /// A turn is over, however it ended. Exactly one per admitted ask, after
    /// the call's terminal event and before the settlement leaves `turn`.
    TurnSettled {
        /// The turn.
        turn: u32,
        /// How it ended.
        reason: SettleReason,
    },
    /// A person's idle gap, logged immediately before the outcome of the
    /// command that ended it, admitted or refused.
    IdleGap(IdleGap),
    /// A fragment of a call the model is making, as the server streamed it:
    /// log v3's `delta` with a `tool_call` piece (#298 T11).
    ToolCallPiece {
        /// The sequence number of the [`Event::Requested`] it answers.
        request: u64,
        /// Which call of the response it belongs to.
        index: u64,
        /// The call's id, where the fragment carried one.
        id: Option<String>,
        /// The function's name, where the fragment carried one.
        name: Option<String>,
        /// This fragment of the arguments.
        arguments: String,
    },
    /// A step's response that made calls: the request is over and the loop
    /// goes on (#29 D11, a step is a request). Not the turn's answer, so
    /// nothing goes on the trunk here; written as a `response`.
    Called {
        /// The sequence number of the [`Event::Requested`] it answers.
        request: u64,
        /// The text streamed beside the calls, which may be none.
        text: String,
        /// Why the server stopped, as it spelled it.
        finish_reason: Option<String>,
        /// The reasoning streamed before the calls.
        reasoning: Option<String>,
        /// What the server measured of the call.
        timings: Option<Timings>,
        /// What a hosted API said beside the calls (#555).
        hosted: Hosted,
    },
    /// What became of one call the model made: its `tool_call` line, one per
    /// call (log v3, v4).
    ToolCalled(Box<ToolLine>),
    /// The gap's one fork (#374): a single call off the trunk's warm tail,
    /// asked after the turn settled `final`, under the regimen's warrant.
    /// Its call is the next [`Event::Requested`], on [`Lane::Interview`].
    Forked {
        /// The settled turn it follows.
        of_turn: u32,
        /// The sequence number of that turn's answered trunk request.
        at: u64,
        /// The rule that warranted it.
        why: log::Warrant,
        /// What it asks.
        question: String,
        /// What it saw of the trunk (#567).
        view: ForkView,
        /// What fired it under the interview cadence (#564): `turn_end`,
        /// or `call:<class>:<id>`.
        trigger: String,
        /// The role it asked in (#599).
        role: Role,
        /// The offboard seat its call ran on (#570); `None` is warm.
        seat: Option<log::ForkSeat>,
        /// Which ask of its set it sent (#595).
        ask: AskKind,
        /// Whether it is sent to the trunk's own server with a prompt that
        /// is not the trunk's prefix, and so may displace the trunk's cached
        /// prefix there (#406).
        displaces: bool,
    },
    /// A seam's audit (#504): a fork off the warm trunk on the `audit` lane,
    /// the dogma's pinned ask after it, before the refill.
    Audited {
        /// The settled turn it follows.
        of_turn: u32,
        /// The sequence number of that turn's answered trunk request.
        at: u64,
        /// The pinned ask it put.
        template: crate::dogma::Template,
        /// The ask, filled.
        question: String,
    },
    /// How the fork ended: once per fork, after its call's last event.
    ForkSettled {
        /// The sequence number of its [`Event::Forked`].
        fork: u64,
        /// How.
        outcome: log::ForkOutcome,
        /// An offboard fork's prompt tokens its server evaluated -- its
        /// `prompt_n`, the prefill -- as it reported them (#570).
        prompt_tokens: Option<u64>,
        /// An offboard fork's call, wall time, in milliseconds (#570).
        wall_ms: Option<u64>,
        /// Why a fork settled `refused` was never sent (#406): `pool`.
        refused: Option<String>,
    },
    /// A seam fired, and the trunk was refilled from working memory (#493):
    /// the head with the working object rendered after it, and no turn of
    /// the old trunk.
    Seamed {
        /// The latest turn, settled, which it follows.
        at_turn: u32,
        /// What fired it: the operator, or the regimen's budget or cadence.
        reason: crate::seam::Reason,
        /// The `head_sha256` a trunk request on the trunk before would carry.
        prefix_hash_before: String,
        /// The same, after.
        prefix_hash_after: String,
        /// The working object, rendered.
        render: String,
        /// How many entries the render carried.
        carried_entries: u64,
        /// The compaction depth the seam ran at (#552): estimated tokens of
        /// recent whole turns it could keep; 0, the total refill.
        tail_tokens: u64,
        /// How many whole turns of the old trunk it kept.
        carried_turns: u64,
        /// Their estimated tokens.
        carried_tokens: u64,
        /// The phases it moved between (#563), when the operator named a
        /// move the graph allowed.
        phase: Option<log::PhaseMove>,
        /// What it carried of the tool outputs it compacted away (#553).
        tool_outputs: log::SeamToolOutputs,
        /// The section after the render carrying them, when it carried any,
        /// and how many it carried.
        outputs: Option<(String, u64)>,
        /// The render's budget and what it did (#565), when declared.
        render_budget: Option<log::RenderBudget>,
        /// The prompt that fired it and the window, on a `window` seam (#617).
        fired: Option<log::SeamFired>,
        /// The calls whose pruned results it replaced (#612), when any.
        pruned: Option<Vec<String>>,
        /// The pre-warm's timings (#504), when the refilled trunk was sent
        /// once and the call finished.
        warm: Option<Timings>,
    },
    /// Forks' patches delivered at the tail of a trunk request, after its
    /// ask (the fork delivery lever): the note stays on the trunk.
    /// A tool result the model pruned (#612): logged when `prune_output`
    /// answered; a later seam carries `text` in its place.
    Pruned {
        /// The turn the prune was called in.
        turn: u32,
        /// The call whose result it pruned.
        call: String,
        /// The sha256 of the result's whole, as saved.
        sha256: String,
        /// The bytes of the result the trunk carried.
        bytes: u64,
        /// Its reference line (#596).
        text: String,
    },
    /// Archived items recalled after an ask (#566): one note, which stays
    /// on the trunk.
    /// A running `bash` call near its timeout (#613): for the surface's
    /// warning, never sent to the model.
    TimeoutNear {
        /// The request whose response carried the call.
        request: u64,
        /// The call's id.
        call: String,
        /// Its timeout, in milliseconds.
        timeout_ms: u64,
    },
    /// A background command ended (#614): its job, how, its exit status,
    /// and its output kept by digest.
    BackgroundEnded {
        /// The job's id.
        job: String,
        /// How it ended.
        status: log::BackgroundStatus,
        /// Its exit status, when it exited.
        exit: Option<u64>,
        /// Its output, kept in the recording by digest.
        files: Vec<log::RecordedFile>,
    },
    /// Ended background commands' notifications, delivered after an ask
    /// (#614): one note at the tail of its first request.
    Notified {
        /// The turn whose first request carried it.
        turn: u32,
        /// The note as sent.
        text: String,
    },
    Recalled {
        /// The turn whose first request carried it.
        turn: u32,
        /// How it matched.
        recall: log::RecallState,
        /// The note as sent.
        text: String,
        /// Each item, in rank order.
        items: Vec<log::RecalledItem>,
    },
    Delivered {
        /// The turn whose first request carried it.
        turn: u32,
        /// The framing every line used.
        framing: log::Framing,
        /// The note as sent: one line per patch.
        text: String,
        /// Each line's patch and template.
        lines: Vec<log::NoteLine>,
    },
    /// One entry the fork's answer patched into the session's working
    /// object, after the fork settled `value`.
    Patched {
        /// The sequence number of its [`Event::Forked`]; `None` for a patch
        /// the trunk's own self-capture call made (#609).
        fork: Option<u64>,
        /// The lane that made it, when no fork did: `self-capture`.
        lane: Option<String>,
        /// What it did.
        op: log::PatchOp,
        /// The entry.
        entry: log::PatchEntry,
        /// The id of the entry it replaced, exactly when `op` is `supersede`.
        supersedes: Option<String>,
        /// The tangent it was made under (#22), when one was open.
        tangent: Option<String>,
    },
    /// A self-capture call and what it came to (#609).
    Captured {
        /// The request whose answer made the call: the trunk's, or with
        /// `fork` the interview fork's (#610).
        request: u64,
        /// The call's id.
        call: String,
        /// The tool.
        tool: String,
        /// What it came to (the log's words).
        outcome: String,
        /// The entries it wrote or ruled on.
        entries: Vec<String>,
        /// Why, when dropped or refused.
        why: Option<String>,
        /// The interview fork that made the call, when one answered through
        /// the capture tools (#610).
        fork: Option<u64>,
    },
    /// A fork screened out of the gap (#611): the turn's own self-capture
    /// already recorded what it would ask.
    Skipped {
        /// The settled turn it would have followed.
        of_turn: u32,
        /// What would have fired it (#564).
        trigger: String,
        /// The ask it would have sent (#595).
        ask: AskKind,
        /// The self-capture call that recorded it.
        call: String,
        /// The field that call recorded under.
        field: String,
    },
    /// The self-capture reminder, as a note after turn `turn`'s ask (#609).
    Reminded {
        /// The turn.
        turn: u32,
        /// The note as sent.
        text: String,
    },
    /// A tangent opened (#22).
    TangentOpened {
        /// Its id.
        id: String,
        /// The turns settled when it opened.
        at_turn: u32,
        /// The trunk's messages at the fork point.
        trunk_messages: u64,
    },
    /// A tangent closed (#22): its entries disposed and the trunk rolled
    /// back to the fork point.
    TangentClosed {
        /// Its id.
        id: String,
        /// The turns settled when it closed.
        at_turn: u32,
        /// The entries kept, dropped and parked, by id.
        kept: Vec<String>,
        /// Dropped to the archive.
        dropped: Vec<String>,
        /// Parked as the tangent's.
        parked: Vec<String>,
        /// Whether working memory's trunk entries rendered as at the fork.
        prefix_intact: bool,
        /// Messages the rollback took off the trunk.
        rolled_back: u64,
    },
}

/// One call's `tool_call` line, in the log's own words (v4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolLine {
    /// The request whose response carried the call.
    pub request: u64,
    /// The turn.
    pub turn: u32,
    /// The call's id.
    pub id: String,
    /// The function's name.
    pub name: String,
    /// The arguments text, as streamed.
    pub arguments: String,
    /// What became of it.
    pub outcome: log::ToolOutcome,
    /// The command as the drive asked for it: the model's own.
    pub argv: Option<Vec<String>>,
    /// Its working directory, as the regimen's worktree is named.
    pub cwd: Option<String>,
    /// What actually ran, runner and all.
    pub confined: Option<Vec<String>>,
    /// The confinement.
    pub isolation: Option<log::Isolation>,
    /// The network.
    pub network: Option<log::Network>,
    /// Its exit status.
    pub exit: Option<u64>,
    /// Why it was refused.
    pub reason: Option<log::ToolRefusal>,
    /// The sha256 of the policy it failed under.
    pub policy: Option<String>,
    /// What it printed.
    pub stdout: Option<log::Output>,
    /// What it printed on standard error.
    pub stderr: Option<log::Output>,
    /// The decision it ran under.
    pub approval: Option<log::Approval>,
    /// Exactly what the model was given as the call's result, when it was
    /// given one (#472).
    pub shown: Option<String>,
    /// The whole output, kept in the recording by digest, when what the
    /// model was shown is capped (#554), or the image `read` returned
    /// (#557): the log's `files`.
    pub files: Vec<log::RecordedFile>,
    /// The images `read` returned (#557), each with the reference its bytes
    /// are checked against, for the result message: never logged, and
    /// taken off the line before it is.
    pub images: Vec<(log::RecordedFile, Vec<u8>)>,
    /// The text the call was recovered from, when the model wrote it as
    /// text rather than calling natively (#560).
    pub recovered_from: Option<String>,
    /// The background job it started, or was moved into (#614).
    pub background: Option<String>,
    /// The timeout that ended it, in milliseconds (#613).
    pub timeout_ms: Option<u64>,
}

impl ToolLine {
    /// A line for `call` of `request` with only its outcome set.
    fn of(request: u64, turn: u32, call: &Call, outcome: log::ToolOutcome) -> Self {
        Self {
            request,
            turn,
            id: call.id.clone(),
            name: call.name.clone(),
            arguments: call.arguments.clone(),
            outcome,
            argv: None,
            cwd: None,
            confined: None,
            isolation: None,
            network: None,
            exit: None,
            reason: None,
            policy: None,
            stdout: None,
            stderr: None,
            approval: None,
            shown: None,
            files: Vec::new(),
            images: Vec::new(),
            recovered_from: None,
            background: None,
            timeout_ms: None,
        }
    }
}

/// The regimen key naming the rules that warrant a fork in the capture gap
/// (#374, ruled 5985110649): a list of the log's [`log::Warrant`] words,
/// `read` and `scoping`. A drive-read key, like
/// [`tool_loop::APPROVAL_POLICY`]: the regimen format does not register it.
/// Absent or empty, no fork ever fires.
pub const INTERVIEW_WARRANT: &str = "interview_warrant";

/// The regimen key for the fork delivery lever.
pub const FORK_DELIVERY: &str = "fork_delivery";

/// The fork delivery lever's state the regimen declares: `seam` (the
/// default, today's behaviour), `advisory` or `imperative`.
///
/// # Errors
///
/// A value that is none of the three.
pub fn fork_delivery(regimen: &Regimen) -> Result<log::ForkDelivery, String> {
    match regimen.get(FORK_DELIVERY) {
        None => Ok(log::ForkDelivery::Seam),
        Some(crate::formats::regimen::Value::String(state)) => log::ForkDelivery::from_tag(state)
            .ok_or_else(|| {
                format!(
                    "`{FORK_DELIVERY}` is \"{state}\": it takes \"seam\", \"advisory\" or \
                     \"imperative\""
                )
            }),
        Some(_) => Err(format!("`{FORK_DELIVERY}` is not a string")),
    }
}

/// The rules `regimen` enables under [`INTERVIEW_WARRANT`], in the order it
/// lists them; empty when it lists none.
///
/// # Errors
///
/// The key is not a list, or an item is not one of [`log::Warrant`]'s words.
pub fn interview_warrant(regimen: &Regimen) -> Result<Vec<log::Warrant>, String> {
    let Some(value) = regimen.get(INTERVIEW_WARRANT) else {
        return Ok(Vec::new());
    };
    let regimen::Value::Array(items) = value else {
        return Err(format!("`{INTERVIEW_WARRANT}` is not a list"));
    };
    items
        .iter()
        .map(|item| match item {
            regimen::Value::String(word) => log::Warrant::from_tag(word)
                // A seam's audit is the seam's, never an interview's (#504).
                .filter(|rule| *rule != log::Warrant::Seam)
                .ok_or_else(|| {
                    format!(
                        "`{INTERVIEW_WARRANT}` names `{word}`, which is not a rule: {}",
                        log::Warrant::ALL
                            .iter()
                            .filter(|rule| **rule != log::Warrant::Seam)
                            .map(|rule| rule.tag())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                }),
            _ => Err(format!(
                "`{INTERVIEW_WARRANT}` holds an item that is not a word"
            )),
        })
        .collect()
}

/// What a served session declares at start beside its template: the
/// substrate claim, what its capture gap forks under, and what its log names
/// that no request carries -- a budget declared and not sent, and the
/// template's default level when none is sent.
pub type Declared = (
    Option<log::SubstrateClaim>,
    Option<Interview>,
    (
        Option<log::Unsent>,
        Option<String>,
        Vec<log::InstructionFile>,
        Option<BTreeMap<String, String>>,
    ),
);

/// The phase graph `interview` runs under, as `session.start` names it
/// (#563): its phases, its allowed moves and the phase it opens in, the
/// graph's first; `None` when it declares none.
#[allow(clippy::type_complexity)]
fn phases_of(interview: Option<&Interview>) -> Option<(Vec<String>, Vec<log::PhaseMove>, String)> {
    let graph = &interview?.phases;
    let first = graph.first()?.to_owned();
    Some((
        graph.phases().to_vec(),
        graph
            .transitions()
            .into_iter()
            .map(|(from, to)| log::PhaseMove { from, to })
            .collect(),
        first,
    ))
}

/// What the capture gap runs under (#374): the rules that warrant its fork,
/// and the working object the fork's patches are applied to.
#[derive(Debug, Clone)]
pub struct Interview {
    /// The rules enabled, as [`interview_warrant`] read them.
    pub rules: Vec<log::Warrant>,
    /// The session's working object.
    pub object: WorkingObject,
    /// When derived seams fire: the regimen's cadence and budget.
    pub seams: crate::seam::policy::Served,
    /// How a fork's patches reach the trunk: at the seam (`seam`, the
    /// default), or as a note at the tail of the next trunk request.
    pub delivery: log::ForkDelivery,
    /// The regimen's phase graph (#563), read as the scripted drive reads
    /// it; empty when it declares none.
    pub phases: crate::seam::phase::PhaseGraph,
    /// The role a fork's interview question is asked in (#599): `user`, the
    /// default, or `system` or `developer` where the served template
    /// renders it (serve checks at start).
    pub role: Role,
    /// The ask set its forks ask in (#595).
    pub asks: &'static crate::dogma::asks::AskSet,
    /// How archived items are recalled at an ask (#566): off, the default.
    pub recall: super::archive::Recall,
    /// What a fork sees of the trunk (#567): all of it, or its last whole
    /// turns after the head; `None` when the regimen does not say, which is
    /// the whole trunk warm and the last turn offboard (#570).
    pub view: Option<ForkView>,
    /// When a fork fires, and on what (#564): one per gap, the default.
    pub cadence: Cadence,
    /// The output size, in bytes, a read must reach to fork (#564); `None`,
    /// the default, gates nothing.
    pub threshold_bytes: Option<u64>,
    /// Whether a fork the turn's own self-capture already recorded is
    /// skipped (#611); off, the default.
    pub skip_self_recorded: bool,
    /// Self-capture (#609): the contract's tools offered from the first
    /// request, and the cadence of silent turns its reminder fires after;
    /// `None` when off.
    pub self_capture: Option<crate::capture::tools::Cadence>,
    /// How a fork answers (#610): in fields, the default, or through the
    /// self-capture tools.
    pub capture: crate::dogma::asks::Modality,
    /// The output cap a fork's call is clamped from (#406): the regimen's
    /// `fork_tail_tokens`, or `None`, the session's.
    pub fork_tail: Option<u32>,
    /// Whether the model is offered `prune_output`, and when its prunes
    /// are applied (#612); `None`, the default, offers nothing.
    pub prune: Option<super::prune::PruneSeam>,
}

/// The interview cadence (#564): when the capture gap's forks fire, and on
/// what. Every state queues its forks into the gap, in call order, one
/// after another; none fires mid-turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cadence {
    /// At most one fork per gap, under the warrant (#374): the judgment ask
    /// on an operator-marked turn, else the class ask on the turn's last
    /// read. Today's, the default.
    #[default]
    Gap,
    /// A fork on every call that ran, under `read`: the class's ask where the
    /// router routes the class to one, else its declared default (the
    /// generic ask); then the judgment ask on a marked turn, under `scoping`.
    PerCall,
    /// A fork on every call the router routes to a class ask, under `read`;
    /// then the judgment ask on a marked turn, under `scoping`.
    PerClass,
    /// The judgment ask at every turn's end, under `scoping`, marked or
    /// not: the router's turn boundary.
    TurnBoundary,
}

impl Cadence {
    /// The regimen's word for it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Gap => "gap",
            Self::PerCall => "per_call",
            Self::PerClass => "per_class",
            Self::TurnBoundary => "turn_boundary",
        }
    }
}

/// The regimen key for the interview cadence (#564).
pub const INTERVIEW_CADENCE: &str = "interview_cadence";

/// The regimen key for the read fork's output threshold, in bytes (#564).
pub const INTERVIEW_THRESHOLD_BYTES: &str = "interview_threshold_bytes";

/// The cadence the regimen declares, leniently: a state's word, or `gap`.
#[must_use]
pub fn interview_cadence(regimen: &Regimen) -> Cadence {
    let Some(crate::formats::regimen::Value::String(word)) = regimen.get(INTERVIEW_CADENCE) else {
        return Cadence::Gap;
    };
    [
        Cadence::Gap,
        Cadence::PerCall,
        Cadence::PerClass,
        Cadence::TurnBoundary,
    ]
    .into_iter()
    .find(|cadence| cadence.word() == word)
    .unwrap_or_default()
}

/// The regimen key for skipping a fork the turn's self-capture already
/// recorded (#611).
pub const INTERVIEW_SKIP_SELF_RECORDED: &str = "interview_skip_self_recorded";

/// Whether the regimen skips self-recorded forks, leniently: `true` or
/// `"on"`; anything else is off.
#[must_use]
pub fn interview_skip_self_recorded(regimen: &Regimen) -> bool {
    match regimen.get(INTERVIEW_SKIP_SELF_RECORDED) {
        Some(crate::formats::regimen::Value::Boolean(on)) => *on,
        Some(crate::formats::regimen::Value::String(word)) => word == "on",
        _ => false,
    }
}

/// The threshold the regimen declares, leniently: a positive whole number of
/// bytes, or none.
#[must_use]
pub fn interview_threshold_bytes(regimen: &Regimen) -> Option<u64> {
    match regimen.get(INTERVIEW_THRESHOLD_BYTES) {
        Some(crate::formats::regimen::Value::Integer(n)) if *n > 0 => Some(n.unsigned_abs()),
        _ => None,
    }
}

/// `shape`'s tools with self-capture's after them, when `interview` has it
/// on (#609): declared from the first request and never changed. With it off
/// `shape` is left exactly as it was.
///
/// # Panics
///
/// If the pinned contract does not read, which `dogma::capture`'s and
/// `capture::tools`' tests hold it always does.
pub fn declare_self_capture(shape: &mut RequestShape, interview: Option<&Interview>) {
    if interview.is_some_and(|interview| interview.self_capture.is_some()) {
        let contract = crate::capture::tools::contract()
            .expect("the pinned contract reads: `dogma::capture`'s tests hold it");
        shape
            .tools
            .extend(crate::capture::tools::definitions(&contract));
    }
}

/// The regimen key that turns self-capture on (#609).
pub const SELF_CAPTURE: &str = "self_capture";

/// The regimen key for the self-capture reminder's cadence (#609).
pub const SELF_CAPTURE_CADENCE: &str = "self_capture_cadence";

/// Self-capture as the regimen declares it, leniently (#609): on for
/// `self_capture = true` or `"on"`, with `self_capture_cadence` silent
/// turns between reminders when it is a positive whole number, else the
/// module's default; off otherwise.
#[must_use]
pub fn self_capture(regimen: &Regimen) -> Option<crate::capture::tools::Cadence> {
    use crate::capture::tools::Cadence;
    use crate::formats::regimen::Value as Word;
    let on = match regimen.get(SELF_CAPTURE) {
        Some(Word::Boolean(on)) => *on,
        Some(Word::String(word)) => word == "on",
        _ => false,
    };
    if !on {
        return None;
    }
    let every = match regimen.get(SELF_CAPTURE_CADENCE) {
        Some(Word::Integer(n)) if *n > 0 => u32::try_from(*n).ok(),
        _ => None,
    };
    Some(
        every
            .and_then(|n| Cadence::every(n).ok())
            .unwrap_or(Cadence::DEFAULT),
    )
}

/// A tool result the model pruned (#612), until a seam replaces it and
/// after, so a later seam still carries its line.
#[derive(Debug, Clone)]
struct Prune {
    /// The call whose result it is.
    call: String,
    /// What the trunk carried as that result.
    shown: String,
    /// Its reference line, which a seam carries in its place.
    text: String,
    /// Whether a seam has replaced it yet.
    applied: bool,
}

impl Prune {
    /// Whether `message` is the result it prunes, as the trunk carried it
    /// or as a seam already replaced it.
    fn is(&self, message: &Message) -> bool {
        message.role == Role::Tool
            && message.tool_call_id.as_deref() == Some(self.call.as_str())
            && (message.content == self.shown || message.content == self.text)
    }
}

/// What a fork sees of the trunk (#567).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForkView {
    /// The whole warm trunk: today's, the default.
    #[default]
    Trunk,
    /// The head -- everything before the first turn -- then the last this
    /// many whole turns.
    Last(u32),
}

impl ForkView {
    /// The regimen's word for it: `trunk`, `last_turn`, or `last:N`.
    #[must_use]
    pub fn word(self) -> String {
        match self {
            Self::Trunk => "trunk".to_owned(),
            Self::Last(1) => "last_turn".to_owned(),
            Self::Last(n) => format!("last:{n}"),
        }
    }
}

/// The regimen key for what a fork sees of the trunk (#567).
pub const FORK_VIEW: &str = "fork_view";

/// The fork view the regimen declares, leniently: `"last_turn"`, or
/// `"last:N"` with N a positive whole number; anything else is the trunk.
#[must_use]
pub fn fork_view(regimen: &Regimen) -> ForkView {
    let Some(crate::formats::regimen::Value::String(word)) = regimen.get(FORK_VIEW) else {
        return ForkView::Trunk;
    };
    if word == "last_turn" {
        return ForkView::Last(1);
    }
    word.strip_prefix("last:")
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| *n > 0)
        .map_or(ForkView::Trunk, ForkView::Last)
}

/// What a fork under `view` sees of `trunk`, whose first `head` messages
/// are the session's head (#567): all of it, or the head -- the system
/// message and everything before the first turn, so that much of the prefix
/// still meets the cache -- then the last whole turns.
fn viewed(trunk: &[Message], head: usize, view: ForkView) -> Vec<Message> {
    let ForkView::Last(n) = view else {
        return trunk.to_vec();
    };
    let head = head.min(trunk.len());
    let turns = crate::seam::render::last_turns(&trunk[head..], n as usize);
    trunk[..head].iter().chain(turns).cloned().collect()
}

/// The regimen key for the role the interview asks in (#599).
pub const INTERVIEW_ROLE: &str = "interview_role";

/// The interview role the regimen declares, leniently: `"system"` or
/// `"developer"`, or else `user`.
#[must_use]
pub fn interview_role(regimen: &Regimen) -> Role {
    match regimen.get(INTERVIEW_ROLE) {
        Some(crate::formats::regimen::Value::String(word)) if word == "system" => Role::System,
        Some(crate::formats::regimen::Value::String(word)) if word == "developer" => {
            Role::Developer
        }
        _ => Role::User,
    }
}

/// The regimen key for the fork ask set (#595).
pub const FORK_ASKS: &str = "fork_asks";

/// The regimen key for how a fork answers (#610).
pub const CAPTURE_MODALITY: &str = "capture_modality";

/// How the regimen has a fork answer (#610): `fields`, the default when it
/// says nothing, or `tools`.
///
/// # Errors
///
/// It names anything else.
pub fn capture_modality(regimen: &Regimen) -> Result<crate::dogma::asks::Modality, String> {
    use crate::dogma::asks::Modality;
    match regimen.get(CAPTURE_MODALITY) {
        None => Ok(Modality::Fields),
        Some(crate::formats::regimen::Value::String(word)) if word == "fields" => {
            Ok(Modality::Fields)
        }
        Some(crate::formats::regimen::Value::String(word)) if word == "tools" => {
            Ok(Modality::Tools)
        }
        Some(other) => Err(format!(
            "`{CAPTURE_MODALITY}` is `fields` or `tools`, not {other:?}"
        )),
    }
}

/// The ask set the regimen names, leniently: a set by its name, or else
/// [`crate::dogma::asks::DEFAULT`].
#[must_use]
pub fn fork_asks(regimen: &Regimen) -> &'static crate::dogma::asks::AskSet {
    match regimen.get(FORK_ASKS) {
        Some(crate::formats::regimen::Value::String(name)) => {
            crate::dogma::asks::set(name).unwrap_or(crate::dogma::asks::DEFAULT)
        }
        _ => crate::dogma::asks::DEFAULT,
    }
}

/// One fork the cadence fires in a gap (#564): the rule that warranted it,
/// what it asks, and what fired it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Firing {
    why: log::Warrant,
    /// Which ask of the session's set it sends (#595).
    ask: AskKind,
    question: String,
    /// `turn_end`, or `call:<class>:<id>`.
    trigger: String,
}

/// A fork's trigger at a turn's end.
const TURN_END: &str = "turn_end";

/// One call of the turn, as the router classed it.
struct Routed {
    id: String,
    class: Class,
    routing: Routing,
    /// `bash`'s command, quoted back by the ask; a standard tool has none.
    command: Option<String>,
    /// Its output's size: what the threshold reads.
    bytes: u64,
}

/// The forks to fire after `turn` settled `final`, in order, under the
/// interview's cadence and warrant (#374, #564).
///
/// Under `gap`, at most one, as the predicate ruled at 5985110649: (b)
/// `scoping`, the turn's ask carried the operator's mark, and the question
/// is the router's turn-boundary ask, [`AskKind::Judgment`]; else (a)
/// `read`, the turn's last call that `ran` and that the router's table
/// classes [`Class::DocumentRead`] or [`Class::SourceRead`], and the
/// question is the ask the router routes that class to. The other states
/// are [`Cadence`]'s. Every ask quotes back what the trunk last said it was
/// about to do, as the router does; a read under the threshold forks
/// nothing.
fn firings(interview: &Interview, log: &[Logged], turn: u32, answer: &str) -> Vec<Firing> {
    let rules = &interview.rules;
    let intent = router::stated_intent(answer);
    // The working record, for an ask in a set that shows it (#595).
    let record = crate::seam::render::entries(&interview.object);
    let ask = |kind: AskKind, last_command: Option<String>| {
        let text = router::Ask {
            kind,
            intent: intent.clone(),
        }
        .render_in(
            interview.asks,
            &Facts {
                cwd: None,
                last_edited: None,
                last_command,
            },
            Some(&record),
        );
        (kind, text)
    };
    let judgment = || {
        let (kind, question) = ask(AskKind::Judgment, None);
        Firing {
            why: log::Warrant::Scoping,
            ask: kind,
            question,
            trigger: TURN_END.to_owned(),
        }
    };
    let marked = rules.contains(&log::Warrant::Scoping)
        && log.iter().any(|logged| {
            matches!(&logged.event, Event::Asked { turn: asked, scoping: true, .. } if *asked == turn)
        });
    let calls = if rules.contains(&log::Warrant::Read) {
        routed(log, turn)
    } else {
        Vec::new()
    };
    let enough = |call: &&Routed| {
        !READS.contains(&call.class)
            || interview
                .threshold_bytes
                .is_none_or(|bytes| call.bytes >= bytes)
    };
    let fork = |call: &Routed, kind: AskKind| {
        let (kind, question) = ask(kind, call.command.clone());
        Firing {
            why: log::Warrant::Read,
            ask: kind,
            question,
            trigger: format!("call:{}:{}", call.class.tag(), call.id),
        }
    };
    let mut out: Vec<Firing> = match interview.cadence {
        Cadence::Gap => {
            if marked {
                return vec![judgment()];
            }
            return calls
                .iter()
                .rev()
                .filter(|call| READS.contains(&call.class))
                .filter(enough)
                .find_map(|call| match call.routing {
                    Routing::Fork(kind) => Some(fork(call, kind)),
                    Routing::Silent | Routing::Defer => None,
                })
                .into_iter()
                .collect();
        }
        Cadence::TurnBoundary => {
            return if rules.contains(&log::Warrant::Scoping) {
                vec![judgment()]
            } else {
                Vec::new()
            };
        }
        Cadence::PerClass => calls
            .iter()
            .filter(enough)
            .filter_map(|call| match call.routing {
                Routing::Fork(kind) => Some(fork(call, kind)),
                Routing::Silent | Routing::Defer => None,
            })
            .collect(),
        Cadence::PerCall => calls
            .iter()
            .filter(enough)
            .map(|call| match call.routing {
                Routing::Fork(kind) => fork(call, kind),
                // The router's declared default for a call it would not
                // interrupt: the generic ask.
                Routing::Silent | Routing::Defer => fork(call, AskKind::Generic),
            })
            .collect(),
    };
    if marked {
        out.push(judgment());
    }
    out
}

/// The forks the cadence chose, screened before any fires: where a routing
/// state skips one, it is dropped here, and a `fork.skipped` line says why.
///
/// #611's state, when on: a fork is skipped when the turn's own
/// self-capture already recorded what it would ask -- an `update_record`
/// the turn made, recorded, under a field the fork's ask asks for (its
/// `fields X, Y and Z`, read off the ask as sent), and, for a fork a call
/// fired, made after that call. No ask is changed and no text added.
fn screened(
    interview: &Interview,
    log: &[Logged],
    turn: u32,
    firings: Vec<Firing>,
) -> (Vec<Firing>, Vec<Event>) {
    if !interview.skip_self_recorded {
        return (firings, Vec::new());
    }
    let recorded = self_recorded(log, turn);
    let mut skips = Vec::new();
    let kept = firings
        .into_iter()
        .filter(|firing| {
            let after = firing
                .trigger
                .strip_prefix("call:")
                .and_then(|rest| rest.split_once(':'))
                .and_then(|(_, id)| {
                    log.iter().rposition(|logged| {
                        matches!(&logged.event, Event::ToolCalled(line)
                            if line.turn == turn && line.id == id)
                    })
                })
                .unwrap_or(0);
            let asked = asked_fields(&firing.question);
            let Some((call, field)) = recorded
                .iter()
                .find(|(at, _, field)| *at > after && asked.contains(field))
                .map(|(_, call, field)| (call.clone(), field.clone()))
            else {
                return true;
            };
            skips.push(Event::Skipped {
                of_turn: turn,
                trigger: firing.trigger.clone(),
                ask: firing.ask,
                call,
                field,
            });
            false
        })
        .collect();
    (kept, skips)
}

/// What `turn`'s own self-capture recorded (#609): each `update_record`
/// the trunk made that came to `recorded`, with where its `capture` line
/// sits in the log, its call's id, and its field.
fn self_recorded(log: &[Logged], turn: u32) -> Vec<(usize, String, String)> {
    let records = crate::capture::tools::CaptureTool::UpdateRecord.tag();
    log.iter()
        .enumerate()
        .filter_map(|(at, logged)| {
            let Event::Captured {
                request,
                call,
                tool,
                outcome,
                fork: None,
                ..
            } = &logged.event
            else {
                return None;
            };
            if tool != records || outcome != "recorded" {
                return None;
            }
            let field = log.iter().find_map(|logged| match &logged.event {
                Event::ToolCalled(line)
                    if line.turn == turn && line.request == *request && line.id == *call =>
                {
                    serde_json::from_str::<serde_json::Value>(&line.arguments)
                        .ok()?
                        .get("field")?
                        .as_str()
                        .map(str::to_owned)
                }
                _ => None,
            })?;
            Some((at, call.clone(), field))
        })
        .collect()
}

/// The fields an ask asks for, read off its words: the `fields X, Y and Z`
/// (or `field X`) every router ask names its answer with, lower-cased to
/// the contract's spelling. None when it names none.
fn asked_fields(question: &str) -> Vec<String> {
    let Some(at) = question
        .find("fields ")
        .map(|at| at + 7)
        .or_else(|| question.find("field ").map(|at| at + 6))
    else {
        return Vec::new();
    };
    let named = &question[at..];
    let end = named.find([';', '.', '\n']).unwrap_or(named.len());
    named[..end]
        .split([',', ' '])
        .filter(|word| !word.is_empty() && *word != "and")
        .take_while(|word| word.chars().all(|c| c.is_ascii_uppercase() || c == '_'))
        .map(str::to_ascii_lowercase)
        .collect()
}

/// The calls of `turn` that `ran`, in order, each classed by the router: a
/// `bash` call by its command, a standard tool's by its own string
/// arguments (#557), its `path` among them.
fn routed(log: &[Logged], turn: u32) -> Vec<Routed> {
    let Ok(mut table) = Router::new() else {
        return Vec::new();
    };
    log.iter()
        .filter_map(|logged| match &logged.event {
            Event::ToolCalled(line)
                if line.turn == turn && line.outcome == log::ToolOutcome::Ran =>
            {
                Some(line.as_ref())
            }
            _ => None,
        })
        .filter_map(|line| {
            let command = tool_loop::command_of(&line.arguments);
            let args: BTreeMap<String, Value> = match &command {
                Some(command) => {
                    BTreeMap::from([("command".to_owned(), Value::String(command.clone()))])
                }
                None => serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
                    &line.arguments,
                )
                .ok()?
                .into_iter()
                .filter_map(|(key, value)| match value {
                    serde_json::Value::String(text) => Some((key, Value::String(text))),
                    _ => None,
                })
                .collect(),
            };
            let decided = table.observe(&RecordEvent::ToolCall {
                id: line.id.clone(),
                at_turn: turn,
                tool: line.name.clone(),
                args: Some(args),
                exit: None,
                output: None,
                exec: None,
            });
            let class = decided.first()?.class;
            let bytes = match (&line.stdout, &line.stderr) {
                (Some(out), err) => out.bytes + err.as_ref().map_or(0, |err| err.bytes),
                (None, _) => line.shown.as_ref().map_or(0, |shown| shown.len() as u64),
            };
            Some(Routed {
                id: line.id.clone(),
                class,
                routing: class.routing(),
                command,
                bytes,
            })
        })
        .collect()
}

/// The router's classes that are a read under rule (a).
const READS: &[Class] = &[Class::DocumentRead, Class::SourceRead];

vocabulary! {
    /// Why `POST /approve` did not answer a prompt.
    ApproveRefusal {
        /// No prompt is waiting.
        NothingWaiting => "nothing-waiting",
        /// The prompt waiting is another call's, or already answered: the
        /// word #389 ruled (5982826097 point 4).
        Stale => "stale",
        /// A standing scope for a prompt only `once` can answer: a dynamic
        /// segment, an alias, or an `npm run` whose script cannot be named.
        NotStanding => "not-standing",
        /// `workspace` in a session that keeps no store.
        NoStore => "no-store",
    }
}

/// An ask the session admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Admitted {
    /// The sequence number of its [`Event::Asked`].
    pub seq: u64,
    /// The turn it began; name it to [`Session::cancel`].
    pub turn: u32,
}

/// Why a command did not do what it asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejected {
    /// The session refused it, and logged the refusal (and any gap it
    /// carried, before it).
    Refused(Refusal),
    /// A cancel named a turn that was never admitted. That is not a command
    /// the session can refuse, so nothing is logged.
    NoSuchTurn(u32),
    /// The gap it carried cannot be logged, so neither it nor the command
    /// is: the command was not carried out.
    BadGap(GapError),
}

/// An event and its place in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    /// Its position in the log: gapless, from zero.
    pub seq: u64,
    /// When it was logged: milliseconds since the session opened, from a
    /// monotonic clock read under the same lock that numbers it, so `t`
    /// never decreases in `seq` order.
    pub t: u64,
    /// What happened.
    pub event: Event,
}

/// The call in flight, and what its events must name.
struct Flight {
    turn: u32,
    request: u64,
    cancel: Cancel,
}

struct State {
    settlement: Settlement,
    trunk: Vec<Message>,
    log: Vec<Logged>,
    flight: Option<Flight>,
    /// Asks admitted so far: the latest turn's number.
    turns: u32,
    opened_at: Instant,
    /// The latest `turn.settled`, while no gap has been logged against it.
    gap_open: Option<u64>,
    /// The gap the command being handled carries, and that command, not yet
    /// checked: checked only if the command is admitted.
    carried: Option<(IdleGap, CommandKind)>,
    /// A checked gap waiting for its admitted command's outcome: the next
    /// event pushed, under the same lock.
    pending_gap: Option<IdleGap>,
    /// The phase the session is in, under a phase graph (#563): the graph's
    /// first at open, then where each seam's move took it.
    phase: Option<String>,
    /// The open tangent (#22), and the trunk at its fork point, which its
    /// close restores.
    tangent: Option<(crate::object::tangent::Tangent, Vec<Message>)>,
    /// The self-capture reminder (#609), when self-capture is on.
    reminder: Option<crate::capture::tools::Reminder>,
    /// Whether the turn in flight elected `update_record`.
    recorded_this_turn: bool,
    /// The self-capture calls the turn in flight has made: each one's
    /// position in the lane's emission for the turn.
    captures_this_turn: u32,
    /// The reminder due at the next ask, when the cadence fired.
    reminder_due: Option<String>,
    /// Patches waiting to be delivered at the next trunk request, under a
    /// mid-turn fork delivery: each op and the entry text its line names.
    undelivered: Vec<(log::PatchOp, String, String)>,
    /// What seams dropped from the trunk, for a recall to find (#566).
    archive: super::archive::Archive,
    /// The text each recovered call came from, by its id, until its line is
    /// logged (#560).
    recovered: BTreeMap<String, String>,
    /// Where each event is written as it is appended (see
    /// [`Session::write_through`]).
    sink: Option<Sink>,
    /// The standing approvals in force: pre-seeds, the store's, and every
    /// standing decision so far (#298 point 5).
    allowed: Vec<Entry>,
    /// The decisions made, by scope.
    counts: Counts,
    /// The prompt waiting on the operator, and their answer once given.
    waiting: Option<Waiting>,
    /// An `end` was admitted while a prompt waited: once the turn settles,
    /// the session ends.
    ending: bool,
    /// The latest prompt the operator decided, by its request and call id:
    /// what `serve`'s `answered` event names.
    answered: Option<(u64, String)>,
    /// What the capture gap runs under, when the regimen warrants forks.
    interview: Option<Interview>,
    /// The fork in flight in the capture gap, by its sequence number.
    forking: Option<u64>,
    /// The gap's forks not yet fired (#564), in order: each fires once the
    /// one before it settles; the turn and the trunk request they fork at.
    queued: std::collections::VecDeque<(u32, u64, Firing)>,
    /// `turns` at the latest seam, or 0: what a cadence counts from.
    turns_at_seam: u32,
    /// The trunk's tokens as the latest trunk call measured them, cleared
    /// by a seam: what a budget reads.
    trunk_tokens: Option<u64>,
    /// The estimated tokens of the latest request's messages, as it was
    /// sized (#588).
    sent_estimate: Option<u64>,
    /// The latest request's prompt as sized, its output cap, and the window
    /// it was sized against (#616): what tells an overflow from its size.
    last_sized: Option<(u64, u32, Option<u64>)>,
    /// The turn that last seamed and went again after an overflow (#617):
    /// once per turn.
    overflow_retried: Option<u32>,
    /// A seam queued for its audit or pre-warm (#504), until [`seam_work`]
    /// takes it.
    seam_pending: Option<PendingSeam>,
    /// The pre-warm in flight (#504), which an ask cancels.
    warming: Option<Cancel>,
    /// The latest measured prompt (prefilled plus cached tokens) and the
    /// estimate of the request it measured: what the next request's prompt
    /// is sized from (#588).
    measured_prompt: Option<(u64, u64)>,
    /// The latest tool-calling step's measurement, the trunk's only once
    /// that step's exchange joins it (a `max_steps` settling).
    step_tokens: Option<u64>,
    /// The turn in flight's exchange through its last completed step: its
    /// ask, then each step's call and results that the turn went on from.
    /// A turn that fails after a step keeps it on the trunk, as `max_steps`
    /// does, rather than losing the commands it ran.
    ran: Vec<Message>,
    /// The recording directory, when the session keeps one: where a seam's
    /// `reference` saves an output's whole (#553).
    recording: Option<std::path::PathBuf>,
    /// The session's background jobs (#614), by id.
    jobs: BTreeMap<String, super::background::Job>,
    /// Ended jobs' notifications, waiting for the next ask (#614).
    notices: Vec<String>,
    /// The running foreground `bash` call's promotion (#614): the job it
    /// becomes, once the operator asks.
    promotable: Option<Arc<Mutex<Option<super::background::Job>>>>,
    /// The results the model pruned (#612), in the order it pruned them.
    pruned: Vec<Prune>,
    /// Whether the trunk carries a seam's refill message after the head
    /// (#597): the turns start after it.
    refilled: bool,
}

impl State {
    /// A session's state as it opens: awaiting, on `trunk`, in `phase`.
    fn opening(
        trunk: Vec<Message>,
        phase: Option<String>,
        interview: Option<Interview>,
        tools: Option<&Tools>,
    ) -> Self {
        State {
            settlement: Settlement::Awaiting,
            trunk,
            log: Vec::new(),
            flight: None,
            turns: 0,
            turns_at_seam: 0,
            trunk_tokens: None,
            sent_estimate: None,
            last_sized: None,
            overflow_retried: None,
            seam_pending: None,
            warming: None,
            measured_prompt: None,
            step_tokens: None,
            ran: Vec::new(),
            opened_at: Instant::now(),
            gap_open: None,
            carried: None,
            pending_gap: None,
            phase,
            tangent: None,
            reminder: interview
                .as_ref()
                .and_then(|interview| interview.self_capture)
                .map(crate::capture::tools::Reminder::on),
            recorded_this_turn: false,
            captures_this_turn: 0,
            reminder_due: None,
            undelivered: Vec::new(),
            archive: super::archive::Archive::default(),
            recovered: BTreeMap::new(),
            sink: None,
            allowed: tools
                .as_ref()
                .map(|t| t.allowed.clone())
                .unwrap_or_default(),
            counts: Counts {
                preseeded: tools.map_or(0, |t| {
                    t.allowed
                        .iter()
                        .filter(|e| e.scope == Scope::Preseeded)
                        .count() as u64
                }),
                ..Counts::default()
            },
            waiting: None,
            ending: false,
            answered: None,
            interview,
            forking: None,
            queued: std::collections::VecDeque::new(),
            recording: tools.and_then(|t| t.recording.clone()),
            pruned: Vec::new(),
            refilled: false,
            jobs: BTreeMap::new(),
            notices: Vec::new(),
            promotable: None,
        }
    }
}

/// A prompt waiting on the operator, and the answer when one arrives.
struct Waiting {
    prompt: Prompt,
    /// The decision and when it was made, on the log's clock.
    answer: Option<(Decision, u64)>,
}

/// What is handed each logged event, on the appending thread, under the
/// session's lock (see [`Session::write_through`]).
pub type Sink = Box<dyn FnMut(&Logged) + Send>;

impl State {
    /// Size `shape`'s output cap to the room its prompt leaves in the
    /// context window (#588), from `ceiling`, the session's cap, and return
    /// it. The prompt is the latest measured one plus the estimate of what
    /// was added since; with none to go on, or a prompt that shrank (a seam),
    /// the whole estimate plus [`ESTIMATE_PAD`].
    fn sized(&mut self, shape: &mut RequestShape, ceiling: u32) -> u32 {
        let prompt = self.prompt_of(shape);
        self.sized_at(shape, ceiling, prompt)
    }

    /// [`State::sized`], for a request whose prompt is `prompt` (#406: a
    /// fork's, which is not always the trunk's).
    fn sized_at(&mut self, shape: &mut RequestShape, ceiling: u32, prompt: u64) -> u32 {
        let estimate = estimate_of(shape);
        self.sent_estimate = Some(estimate);
        let cap = clamped(ceiling, shape.limits.context_window, prompt);
        shape.limits.max_output_tokens = cap;
        self.last_sized = Some((prompt, cap, shape.limits.context_window));
        cap
    }

    /// The latest request's prompt against the window, when it overflowed it
    /// (#616): the server's typed refusal said so (`engine`), or -- the
    /// refusal naming no kind -- the prompt and its output cap reached into
    /// the window's margin ([`margin`]), which the sizing keeps clear. `None`
    /// when serve knows no window.
    fn overflow(&self, engine: bool) -> Option<log::Overflow> {
        let (prompt, cap, window) = self.last_sized?;
        let window = window?;
        let at_the_edge =
            i128::from(prompt) + i128::from(cap) > i128::from(window) - margin(i128::from(window));
        (engine || at_the_edge).then_some(log::Overflow {
            prompt_tokens: prompt,
            window,
            inferred: !engine,
        })
    }

    /// [`State::overflow`] for a call that failed in transport: never a
    /// timeout, which is the clock's and not the prompt's.
    fn failed_overflow(&self, failure: &TransportFailure) -> Option<log::Overflow> {
        match failure {
            TransportFailure::Timeout { .. } => None,
            _ => self.overflow(false),
        }
    }

    /// How a fork's call on `shape` fits (#406). Its prompt: on the trunk's
    /// server and the trunk's prefix, sized as the trunk's is, from the
    /// trunk's measurement; otherwise the trunk's measurement says nothing
    /// of it, and its estimate is all there is. Whether it may displace the
    /// trunk's cache: on the trunk's server, off its prefix. Whether it is
    /// refused unsent: its window leaves less than the clamp's floor (or
    /// `ceiling`, its tail, if that is less), the room the trunk's own clamp
    /// never goes below (#588).
    fn fork_fit(&self, shape: &RequestShape, offboard: bool, ceiling: u32) -> ForkFit {
        let before_the_ask = &shape.messages[..shape.messages.len() - 1];
        let on_the_trunks_prefix = self.trunk.starts_with(before_the_ask);
        let prompt = if !offboard && on_the_trunks_prefix {
            self.prompt_of(shape)
        } else {
            estimate_of(shape)
        };
        ForkFit {
            prompt,
            displaces: !offboard && !on_the_trunks_prefix,
            refused: shape.limits.context_window.is_some_and(|window| {
                window.saturating_sub(prompt) < u64::from(ceiling).min(CLAMP_FLOOR)
            }),
        }
    }

    /// The prompt a request on `shape` would send, as [`State::sized`]
    /// sizes it (#588): the latest measured prompt plus the estimate of what
    /// was added since; with none to go on, or a prompt that shrank (a
    /// seam), the whole estimate plus [`ESTIMATE_PAD`].
    fn prompt_of(&self, shape: &RequestShape) -> u64 {
        let estimate = estimate_of(shape);
        match self.measured_prompt {
            Some((measured, then)) if estimate >= then => measured + (estimate - then),
            _ => estimate + ESTIMATE_PAD,
        }
    }

    fn push(&mut self, event: Event) -> u64 {
        // An admitted command's gap is logged immediately BEFORE that
        // command's outcome, under the same lock (#117, D13 (c)).
        if let Some(gap) = self.pending_gap.take() {
            self.gap_open = None;
            self.append(Event::IdleGap(gap));
        }
        self.append(event)
    }

    fn append(&mut self, event: Event) -> u64 {
        // A call's measured prompt, against the estimate it was sized with
        // (#588).
        if let Event::Answered { timings, .. }
        | Event::Capped { timings, .. }
        | Event::Called { timings, .. } = &event
            && let (Some(timings), Some(sent)) = (timings, self.sent_estimate)
            && (timings.prompt_n.is_some() || timings.cache_n.is_some())
        {
            let prompt = timings.prompt_n.unwrap_or(0) + timings.cache_n.unwrap_or(0);
            self.measured_prompt = Some((prompt, sent));
        }
        let seq = u64::try_from(self.log.len()).expect("a log longer than u64 cannot be built");
        let t = u64::try_from(self.opened_at.elapsed().as_millis()).unwrap_or(u64::MAX);
        if matches!(event, Event::TurnSettled { .. }) {
            self.gap_open = Some(seq);
        }
        self.log.push(Logged { seq, t, event });
        if let (Some(sink), Some(logged)) = (self.sink.as_mut(), self.log.last()) {
            sink(logged);
        }
        seq
    }

    /// Hold the gap a command carries until the command is admitted or
    /// refused.
    fn carry(&mut self, gap: Option<IdleGap>, command: CommandKind) {
        self.carried = gap.map(|gap| (gap, command));
    }

    /// The note recalling what earlier seams archived that `asked` names
    /// (#566), logged as turn `turn`'s `recalled` line, or `None`: under
    /// `literal`, the archive's best items by the ask's anchors, in Qwen
    /// Code's reminder shape. Under `off`, nothing.
    fn recall(&mut self, turn: u32, asked: &str) -> Option<Message> {
        use super::archive::Recall;
        let recall = self.interview.as_ref()?.recall;
        if recall != Recall::Literal || self.archive.is_empty() {
            return None;
        }
        let found = self.archive.literal(asked);
        if found.is_empty() {
            return None;
        }
        let text = super::archive::note(&found);
        let items = found
            .iter()
            .map(|(item, score)| log::RecalledItem {
                key: item.key.clone(),
                sha256: item.sha256(),
                score: *score,
            })
            .collect();
        self.push(Event::Recalled {
            turn,
            recall: log::RecallState::Literal,
            text: text.clone(),
            items,
        });
        Some(Message::new(Role::User, text))
    }

    /// The note delivering every patch waiting since the last trunk
    /// request, logged as turn `turn`'s `delivered` line, or `None` when
    /// none waits. One line per patch, each the (b′) sentence of the
    /// session's framing, pinned in the dogma.
    fn notify(&mut self, turn: u32) -> Option<Message> {
        if self.notices.is_empty() {
            return None;
        }
        let text = std::mem::take(&mut self.notices).join("\n");
        self.push(Event::Notified {
            turn,
            text: text.clone(),
        });
        Some(Message::new(Role::User, text))
    }

    fn deliver(&mut self, turn: u32) -> Option<Message> {
        let delivery = self.interview.as_ref()?.delivery;
        let (framing, template) = match delivery {
            log::ForkDelivery::Seam => return None,
            log::ForkDelivery::Advisory => (
                log::Framing::Advisory,
                crate::dogma::Template::ForkNoteAdvisory,
            ),
            log::ForkDelivery::Imperative => (
                log::Framing::Imperative,
                crate::dogma::Template::ForkNoteImperative,
            ),
        };
        if self.undelivered.is_empty() {
            return None;
        }
        let mut written = Vec::new();
        let mut lines = Vec::new();
        for (op, id, text) in std::mem::take(&mut self.undelivered) {
            let Ok(line) = template.fill(&[(crate::dogma::Hole::Entry, text.as_str())]) else {
                continue;
            };
            written.push(line);
            lines.push(log::NoteLine {
                entry: id,
                op,
                template: template.name().to_owned(),
            });
        }
        if written.is_empty() {
            return None;
        }
        let text = written.join("\n");
        self.push(Event::Delivered {
            turn,
            framing,
            text: text.clone(),
            lines,
        });
        Some(Message::new(Role::User, text))
    }

    /// A command was admitted. The gap it carries is checked and held for its
    /// outcome; and whatever gap was open ended here, carried or not (a gap
    /// ends at the command that ends it -- left open, a later command could
    /// log it after the one that really ended it, or after the session
    /// ended; #146's first review).
    ///
    /// # Errors
    ///
    /// [`Rejected::BadGap`] when the carried gap cannot be logged: then the
    /// command is not carried out and nothing is logged.
    fn admit(&mut self) -> Result<(), Rejected> {
        if let Some((gap, command)) = self.carried.take() {
            let ends = match command {
                CommandKind::Ask => GapEnd::Ask,
                CommandKind::Cancel => GapEnd::Cancel,
                CommandKind::DeclareSeam => GapEnd::Seam,
                CommandKind::End => GapEnd::End,
                CommandKind::OpenTangent | CommandKind::CloseTangent | CommandKind::Background => {
                    unreachable!("a tangent or background command carries no idle gap")
                }
            };
            if gap.ended_by != ends {
                return Err(Rejected::BadGap(GapError::EndedByAnotherCommand {
                    says: gap.ended_by,
                    carried_by: command,
                }));
            }
            if self.gap_open != Some(gap.opened_by) {
                return Err(Rejected::BadGap(GapError::NotTheOpenGap {
                    opened_by: gap.opened_by,
                }));
            }
            let durations = [gap.notice, gap.read, gap.compose, gap.away, gap.blocked];
            if durations
                .iter()
                .any(|duration| i64::try_from(*duration).is_err())
            {
                return Err(Rejected::BadGap(GapError::NotACount));
            }
            self.pending_gap = Some(gap);
        }
        self.gap_open = None;
        Ok(())
    }

    /// Now, on the log's clock: milliseconds since the session opened.
    fn now(&self) -> u64 {
        u64::try_from(self.opened_at.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// The turn is over: back to `awaiting`, and on to `ended` when an
    /// `end` was admitted while it waited on a prompt.
    /// A turn that settled `failed` or `timeout` after at least one step
    /// keeps those steps on the trunk -- the ask, each call and its results
    /// -- as a `max_steps` settle does (#29): the commands it ran are not
    /// lost. A turn that failed on its first request keeps nothing.
    fn keep_ran_steps(&mut self) {
        let ran = std::mem::take(&mut self.ran);
        if ran.len() > 1 {
            self.trunk.extend(ran);
            self.trunk_tokens = self.step_tokens.take();
        }
    }

    fn after_the_turn(&mut self) {
        // The self-capture reminder (#609): a turn that recorded nothing
        // counts toward the cadence, and a cadence that comes round leaves a
        // note for the next ask.
        let recorded = std::mem::take(&mut self.recorded_this_turn);
        self.captures_this_turn = 0;
        let turn = self.turns;
        if let Some(ask) = self
            .reminder
            .as_mut()
            .and_then(|reminder| reminder.observe(turn, recorded))
        {
            self.reminder_due = Some(ask.text());
        }
        self.move_to(Settlement::Awaiting);
        if self.ending {
            self.ending = false;
            self.move_to(Settlement::Ended);
        }
    }

    fn move_to(&mut self, to: Settlement) {
        let from = self.settlement;
        self.settlement = to;
        self.push(Event::Settled { from, to });
    }

    fn refuse(&mut self, command: CommandKind, because: Refusal) -> Refusal {
        // A refused command's gap is neither logged nor closed (ruled on
        // #146, amending D13 (c)): the person has not been served yet, and the
        // admitted command that follows carries the gap -- its `blocked`
        // running from this refusal.
        self.carried = None;
        let during = self.settlement;
        // Once ended, nothing more is logged: `ended` is the log's last line,
        // so a reader that waits for it has the whole session (#291). The
        // caller still answers the refusal.
        if during == Settlement::Ended {
            return because;
        }
        self.push(Event::Refused {
            command,
            because,
            during,
        });
        because
    }
}

struct Shared<S> {
    transport: S,
    template: RequestShape,
    state: Mutex<State>,
    changed: Condvar,
    /// What a call runs under, when the session runs commands.
    tools: Option<Tools>,
    /// Where a started background job's child goes to be waited on (#614):
    /// the session's reaper, when background commands are on.
    reap: Mutex<Option<std::sync::mpsc::Sender<(String, std::process::Child)>>>,
    /// Where the interview fork runs when it is not the trunk's server
    /// (#570); `None` is warm.
    seat: Option<Offboard<S>>,
}

/// An offboard extraction seat (#570): the second server a fork's call is
/// made to, the registry id it was checked as, the model its request names,
/// and the context window it serves, when the registry declares one.
#[derive(Debug)]
pub struct Offboard<S> {
    /// The seat's server.
    pub transport: S,
    /// Its registry id.
    pub substrate: String,
    /// The model a fork's request names.
    pub model: String,
    /// The context window a fork's output cap is clamped to.
    pub context_window: Option<u64>,
}

impl<S> Shared<S> {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// An interactive session on one trunk.
///
/// Each turn's call runs on a thread of its own, DETACHED: nothing here
/// joins it. A join is a wait on an invariant ("awaiting means the last turn
/// is done"), and a wait on a broken invariant is a hang rather than a
/// failure -- the seeded fault that accepts an ask mid-turn found exactly
/// that, a test that never finished instead of one that went red. A turn's
/// thread holds the session's shared state by `Arc`, so outliving the
/// `Session` is safe, and dropping a session stops its call (see `Drop`).
pub struct Session<S: Streaming + 'static> {
    shared: Arc<Shared<S>>,
}

impl<S: Streaming + 'static> fmt::Debug for Session<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.shared.lock();
        f.debug_struct("Session")
            .field("transport", &self.shared.transport.describes())
            .field("settlement", &state.settlement)
            .field("trunk", &state.trunk.len())
            .field("log", &state.log.len())
            .finish_non_exhaustive()
    }
}

impl<S: Streaming + 'static> Session<S> {
    /// Open a session on `transport`.
    ///
    /// `template` is every request's shape; its `messages` are the head the
    /// trunk starts from, and its `limits.call` bounds each turn's call.
    ///
    /// # Panics
    ///
    /// On a head holding a [`Role::Tool`] message, here and in every other
    /// `open`: a tool result answers a call made in a turn, and a head comes
    /// before any.
    #[must_use]
    pub fn open(transport: S, template: RequestShape) -> Self {
        Self::opened_as(
            transport,
            template,
            None,
            None,
            (None, None, (None, None, Vec::new(), None)),
        )
    }

    /// [`Session::open`], declaring what serves it -- the dialect it speaks
    /// and, when the operator declared it, its concurrency -- which the log's
    /// `session.start` carries (#292).
    #[must_use]
    pub fn open_serving(transport: S, template: RequestShape, serving: Serving) -> Self {
        Self::opened_as(
            transport,
            template,
            Some(serving),
            None,
            (None, None, (None, None, Vec::new(), None)),
        )
    }

    /// A session that runs the model's calls (#298): `template` declares the
    /// tools, `tools` says what a call runs under and who answers a prompt.
    #[must_use]
    pub fn open_looping(
        transport: S,
        template: RequestShape,
        serving: Option<Serving>,
        tools: Tools,
    ) -> Self {
        Self::opened_as(
            transport,
            template,
            serving,
            Some(tools),
            (None, None, (None, None, Vec::new(), None)),
        )
    }

    /// [`Session::open_serving`] or [`Session::open_looping`], by whether it
    /// runs commands: `serve`'s one way in. `claim` is the substrate claim
    /// its `session.start` carries (#292), when it has one; `interview` is
    /// what its capture gap forks under (#374), when the regimen warrants
    /// forks.
    #[must_use]
    pub fn open_with(
        transport: S,
        template: RequestShape,
        serving: Option<Serving>,
        tools: Option<Tools>,
        claim: Option<log::SubstrateClaim>,
        interview: Option<Interview>,
    ) -> Self {
        Self::opened_as(
            transport,
            template,
            serving,
            tools,
            (claim, interview, (None, None, Vec::new(), None)),
        )
    }

    /// [`Self::open_with`], recording what the regime declares and no
    /// request carries (R1) on `session.start`.
    #[must_use]
    pub fn open_declaring(
        transport: S,
        template: RequestShape,
        serving: Option<Serving>,
        tools: Option<Tools>,
        (claim, interview, unsent): Declared,
    ) -> Self {
        Self::opened_as(
            transport,
            template,
            serving,
            tools,
            (claim, interview, unsent),
        )
    }

    fn opened_as(
        transport: S,
        template: RequestShape,
        serving: Option<Serving>,
        tools: Option<Tools>,
        (claim, interview, (unsent, reasoning_effort_default, instruction_files, levers)): Declared,
    ) -> Self {
        a_head(&template.messages);
        let trunk = template.messages.clone();
        let fork_delivery = interview.as_ref().map(|interview| interview.delivery);
        let phases = phases_of(interview.as_ref());
        let phase_at_open = phases.as_ref().map(|(_, _, first)| first.clone());
        let fork_asks = interview.as_ref().map(|interview| interview.asks);
        let opened = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        let mut state = State::opening(trunk, phase_at_open.clone(), interview, tools.as_ref());
        state.push(Event::Started {
            opened,
            model: template.model.clone(),
            head: template.messages.clone(),
            serving,
            claim,
            tools: template
                .tools
                .iter()
                .map(|tool| tool.name.clone())
                .collect(),
            template_kwargs: template.template_kwargs.clone(),
            unsent,
            approvals_off: tools.as_ref().is_some_and(|tools| tools.approvals_off),
            fork_delivery,
            fork_asks,
            reasoning_effort_default,
            instruction_files,
            levers,
            tool_output: tools.as_ref().map(|tools| tools.output_cap),
            bash_timeout_ms: bash_timeout_of(tools.as_ref(), &template),
            phases,
        });
        Self::sharing(Shared {
            transport,
            template,
            state: Mutex::new(state),
            changed: Condvar::new(),
            tools,
            reap: Mutex::new(None),
            seat: None,
        })
    }

    /// The session over `shared`, its reaper started when background
    /// commands are on (#614).
    fn sharing(shared: Shared<S>) -> Self {
        let shared = Arc::new(shared);
        start_reaper(&shared);
        Self { shared }
    }

    /// This session, its interview fork seated offboard (#570): every fork's
    /// call is made to `seat`, naming its model, and the fork's log line
    /// names both. The trunk is untouched.
    ///
    /// # Panics
    ///
    /// When the session is already shared: a seat is chosen before a
    /// session serves anything.
    #[must_use]
    pub fn seated(mut self, seat: Offboard<S>) -> Self {
        Arc::get_mut(&mut self.shared)
            .expect("a session is seated before it is shared")
            .seat = Some(seat);
        self
    }

    /// Send an ask to the trunk. Returns the sequence number of its
    /// [`Event::Asked`] and the turn it begins; the answer arrives in the log.
    ///
    /// `gap` is the idle gap this ask ended, if the surface measured one. If
    /// the ask is admitted, the gap is logged immediately before it; if it is
    /// refused, the gap is neither logged nor closed, and the next admitted
    /// command carries it.
    ///
    /// # Errors
    ///
    /// [`Refusal::InFlight`] while a turn or a capture is in flight, and
    /// [`Refusal::Ended`] once the session has ended. The first is logged; the
    /// second is not, so `ended` stays the log's last line (#291).
    /// [`Rejected::BadGap`] when `gap` cannot be logged, and then nothing is.
    pub fn ask(&self, text: &str, gap: Option<IdleGap>) -> Result<Admitted, Rejected> {
        self.ask_marked(text, gap, false)
    }

    /// [`Session::ask`], `scoping` when the operator marked the ask a
    /// scoping question: its `ask` line carries the mark, and the capture
    /// gap after its turn is warranted a fork under rule (b) (#374).
    ///
    /// # Errors
    ///
    /// As [`Session::ask`]: an ask sent while the gap's fork is in flight is
    /// refused [`Refusal::InFlight`], and waits, held by the surface, for the
    /// fork to settle.
    pub fn ask_marked(
        &self,
        text: &str,
        gap: Option<IdleGap>,
        scoping: bool,
    ) -> Result<Admitted, Rejected> {
        self.ask_attached(super::attach::Attached::plain(text), gap, scoping)
    }

    /// [`Session::ask_marked`], with what the operator attached (#372):
    /// `attached`'s message is the turn's user message -- the ask's words
    /// and its images -- sent and kept on the trunk, and its files are the
    /// `ask` line's `files`.
    ///
    /// # Errors
    ///
    /// As [`Session::ask_marked`].
    pub fn ask_attached(
        &self,
        attached: super::attach::Attached,
        gap: Option<IdleGap>,
        scoping: bool,
    ) -> Result<Admitted, Rejected> {
        let super::attach::Attached { message, files } = attached;
        let mut state = self.shared.lock();
        state.carry(gap, CommandKind::Ask);
        // A seam's pre-warm in flight is cancelled by an ask (#504): the
        // ask's own request warms the same prefix. The seam stands, logged
        // without the warm's timings, and the ask goes on once it is.
        while let Some(warm) = state.warming.as_ref() {
            warm.ask();
            state = self
                .shared
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        match state.settlement {
            Settlement::Awaiting => {}
            Settlement::Turn | Settlement::Capture => {
                let refused = state.refuse(CommandKind::Ask, Refusal::InFlight);
                self.shared.changed.notify_all();
                return Err(Rejected::Refused(refused));
            }
            Settlement::Ended => {
                let refused = state.refuse(CommandKind::Ask, Refusal::Ended);
                self.shared.changed.notify_all();
                return Err(Rejected::Refused(refused));
            }
        }
        state.admit()?;
        state.turns += 1;
        let turn = state.turns;
        let seq = state.push(Event::Asked {
            turn,
            text: message.content.clone(),
            scoping,
            files,
        });
        state.move_to(Settlement::Turn);
        let mut shape = self.shared.template.clone();
        shape.messages.clone_from(&state.trunk);
        shape.messages.push(message.clone());
        // The fork delivery lever: what forks patched since the last
        // request, as one note after the ask; it joins the trunk with it.
        let mut opening = vec![message];
        if let Some(note) = state.deliver(turn) {
            shape.messages.push(note.clone());
            opening.push(note);
        }
        // Archive recall (#566): what earlier seams dropped that this ask
        // names, as one note after it (and after any delivered note).
        if let Some(note) = state.recall(turn, &opening[0].content) {
            shape.messages.push(note.clone());
            opening.push(note);
        }
        // Background commands that ended since (#614): their notifications,
        // as one note after it, Qwen Code's `<task-notification>`s.
        if let Some(note) = state.notify(turn) {
            shape.messages.push(note.clone());
            opening.push(note);
        }
        // The self-capture reminder (#609), when its cadence came round: an
        // advisory note after the ask (and after any other note), never in
        // the system prompt.
        if let Some(text) = state.reminder_due.take() {
            state.push(Event::Reminded {
                turn,
                text: text.clone(),
            });
            let note = Message::new(Role::User, text);
            shape.messages.push(note.clone());
            opening.push(note);
        }
        // Pushed here, under the lock that admits the ask, and never on the
        // turn's thread: a thread that cannot start still settles with a
        // `request.failed` that cites a request that exists (#117, R2c
        // finding 21).
        // The automatic seam (#617), before the request is sized.
        window_seam(&self.shared.template, &mut state, &mut shape, false);
        let max_tokens = state.sized(&mut shape, self.shared.template.limits.max_output_tokens);
        let request = state.push(Event::Requested {
            turn,
            lane: Lane::Trunk,
            head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
            fork: None,
            max_tokens,
        });
        let cancel = Cancel::new();
        state.flight = Some(Flight {
            turn,
            request,
            cancel: cancel.clone(),
        });
        drop(state);
        self.shared.changed.notify_all();

        let shared = Arc::clone(&self.shared);
        let spawned = std::thread::Builder::new()
            .name("diet-turn".to_owned())
            .spawn(move || {
                // Caught rather than left to unwind: a panicking transport, or
                // an overflowing deadline, must still settle the turn -- and
                // say why -- or the session sits in `turn` for good, refusing
                // every ask (#120's first review).
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    call(&shared, &shape, &cancel, opening, turn, request);
                    // A seam the turn's end made due, queued for its audit
                    // or pre-warm (#504).
                    seam_work(&shared);
                }));
                if let Err(payload) = outcome {
                    crashed(&shared, panic_message(payload.as_ref()));
                }
            });
        if let Err(why) = spawned {
            // The same outcome by the other door: `std::thread::spawn` would
            // have panicked on the caller with the session already in `turn`
            // (#120's second review).
            crashed(
                &self.shared,
                format!("the turn's thread could not start: {why}"),
            );
        }
        Ok(Admitted { seq, turn })
    }

    /// Ask the call of `turn` to stop.
    ///
    /// The turn is named so that a stop sent for one turn cannot land on
    /// the next: over HTTP a cancel can arrive after its turn settled and a
    /// new ask was admitted, and it must not stop that one.
    ///
    /// # Errors
    ///
    /// [`Rejected::NoSuchTurn`] for a turn never admitted (not logged), and
    /// [`Rejected::BadGap`] when `gap` cannot be logged (nothing is).
    /// Otherwise refused: [`Refusal::Ended`] once the session has ended (not
    /// logged, #291), and, logged, [`Refusal::Stale`] for a turn older than the latest, and
    /// [`Refusal::NothingInFlight`] when the latest turn has no call in
    /// flight.
    pub fn cancel(&self, turn: u32, gap: Option<IdleGap>) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        let latest = state.turns;
        if turn == 0 || turn > latest {
            return Err(Rejected::NoSuchTurn(turn));
        }
        state.carry(gap, CommandKind::Cancel);
        let because = match (state.settlement, state.flight.as_ref()) {
            (Settlement::Ended, _) => Refusal::Ended,
            _ if turn < latest => Refusal::Stale,
            // The turn's call, or its gap's fork (#374): a stop reaches
            // either, and a fork it reaches settles `cancelled`.
            (Settlement::Turn | Settlement::Capture, Some(flight)) => {
                let cancel = flight.cancel.clone();
                state.admit()?;
                state.push(Event::StopAsked { turn });
                drop(state);
                self.shared.changed.notify_all();
                // Outside the lock: a stopper may do anything a transport
                // needs to wake its call, and none of it should be done while
                // holding the session.
                cancel.ask();
                return Ok(());
            }
            _ => Refusal::NothingInFlight,
        };
        let refused = state.refuse(CommandKind::Cancel, because);
        drop(state);
        self.shared.changed.notify_all();
        Err(Rejected::Refused(refused))
    }

    /// Declare a seam (#493): the trunk is refilled from working memory --
    /// the head, with the working object rendered after it
    /// ([`crate::seam::render::refill`]) -- and the next ask is sent on it.
    /// No turn of the old trunk is carried. Its [`Event::Seamed`] records the
    /// head's digest either side, the render, and what was carried.
    ///
    /// Not yet: the audit (the dogma's operator ask, whose answer grammar is
    /// unwritten), so nothing is ratified; and the pre-warm, so the next ask
    /// prefills the refilled trunk itself.
    ///
    /// # Errors
    ///
    /// [`Refusal::InFlight`] while a turn or a capture is in flight, and
    /// [`Refusal::NothingToSeam`] before any turn has settled, or when working
    /// memory holds no entry (or the session keeps none): both logged. [`Refusal::Ended`] once the
    /// session has ended, not logged (#291). A refused command's `gap` is
    /// neither logged nor closed. [`Rejected::BadGap`] when `gap` cannot be
    /// logged, and then nothing is.
    pub fn declare_seam(&self, gap: Option<IdleGap>) -> Result<(), Rejected> {
        self.declare_seam_to(gap, None)
    }

    /// [`Self::declare_seam`], moving to phase `to` when one is named
    /// (#563): the regimen's phase graph rules on the move, as it rules on
    /// the scripted drive's (`seam::phase::PhaseGraph::decide`), and a move
    /// it does not allow is refused with its reason, logged, as
    /// `nothing-to-seam` is. With no phase named, the session stays in its
    /// phase.
    ///
    /// # Errors
    ///
    /// As [`Self::declare_seam`], and [`Refusal::NoPhaseGraph`],
    /// [`Refusal::NotAPhase`], [`Refusal::AlreadyInPhase`] or
    /// [`Refusal::NoPhaseEdge`] for a move the graph refuses.
    pub fn declare_seam_to(&self, gap: Option<IdleGap>, to: Option<&str>) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        state.carry(gap, CommandKind::DeclareSeam);
        let because = match state.settlement {
            Settlement::Ended => Some(Refusal::Ended),
            Settlement::Turn | Settlement::Capture => Some(Refusal::InFlight),
            // Nothing to refill from: no turn yet, no working object, or an
            // empty one -- a refill from an empty object would drop every
            // turn and carry nothing in their place.
            // A seam while a tangent is open would replace the trunk its
            // close restores (#22).
            Settlement::Awaiting if state.tangent.is_some() => Some(Refusal::TangentOpen),
            Settlement::Awaiting
                if state.turns == 0
                    || state
                        .interview
                        .as_ref()
                        .is_none_or(|interview| interview.object.live().next().is_none()) =>
            {
                Some(Refusal::NothingToSeam)
            }
            Settlement::Awaiting => to.and_then(|to| {
                use crate::seam::phase::{Decision, Refusal as Graph};
                let graph = state
                    .interview
                    .as_ref()
                    .map(|interview| &interview.phases)
                    .filter(|graph| !graph.is_empty());
                let Some(graph) = graph else {
                    return Some(Refusal::NoPhaseGraph);
                };
                match graph.decide(state.phase.as_deref(), to) {
                    Decision::Ratified => None,
                    Decision::Refused(Graph::NoGraph) => Some(Refusal::NoPhaseGraph),
                    Decision::Refused(Graph::NotAPhase) => Some(Refusal::NotAPhase),
                    Decision::Refused(Graph::AlreadyThere) => Some(Refusal::AlreadyInPhase),
                    Decision::Refused(Graph::NoEdge) => Some(Refusal::NoPhaseEdge),
                }
            }),
        };
        if let Some(because) = because {
            let refused = state.refuse(CommandKind::DeclareSeam, because);
            drop(state);
            self.shared.changed.notify_all();
            return Err(Rejected::Refused(refused));
        }
        state.admit()?;
        let queued = seam(
            &self.shared.template,
            &mut state,
            crate::seam::Reason::Operator,
            to.map(str::to_owned),
        );
        drop(state);
        self.shared.changed.notify_all();
        if queued {
            // Its audit and pre-warm (#504), on a thread of their own.
            let shared = Arc::clone(&self.shared);
            let spawned = std::thread::Builder::new()
                .name("diet-seam".to_owned())
                .spawn(move || seam_work(&shared));
            if spawned.is_err() {
                seam_work(&self.shared);
            }
        }
        Ok(())
    }

    /// Open a tangent under `id` (#22): the trunk as it stands is the fork
    /// point, kept for the close to restore; every entry a fork patches in
    /// until the close carries `id` in its provenance.
    ///
    /// # Errors
    ///
    /// Refused, and logged: [`Refusal::Ended`], [`Refusal::InFlight`];
    /// [`Refusal::TangentOpen`] with one already open; [`Refusal::BadTangent`]
    /// when the session keeps no working memory, or `id` is blank or names a
    /// tangent the object already holds entries from.
    pub fn open_tangent(&self, id: &str) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        let because = match state.settlement {
            Settlement::Ended => Some(Refusal::Ended),
            Settlement::Turn | Settlement::Capture => Some(Refusal::InFlight),
            Settlement::Awaiting if state.tangent.is_some() => Some(Refusal::TangentOpen),
            Settlement::Awaiting => None,
        };
        let opened = match because {
            Some(because) => Err(because),
            None => state
                .interview
                .as_ref()
                .ok_or(Refusal::BadTangent)
                .and_then(|interview| {
                    crate::object::tangent::Tangent::open(&interview.object, id, state.turns)
                        .map_err(|_| Refusal::BadTangent)
                }),
        };
        let tangent = match opened {
            Ok(tangent) => tangent,
            Err(because) => {
                let refused = state.refuse(CommandKind::OpenTangent, because);
                drop(state);
                self.shared.changed.notify_all();
                return Err(Rejected::Refused(refused));
            }
        };
        let at_turn = state.turns;
        let trunk_messages = state.trunk.len() as u64;
        let fork_point = state.trunk.clone();
        state.tangent = Some((tangent, fork_point));
        state.push(Event::TangentOpened {
            id: id.to_owned(),
            at_turn,
            trunk_messages,
        });
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Close the open tangent (#22): `dispositions` rules on every entry it
    /// created -- kept live, dropped to the archive, or parked as the
    /// tangent's -- through `object::tangent::Tangent::close`, and the trunk
    /// is restored to the fork point.
    ///
    /// # Errors
    ///
    /// Refused, and logged: [`Refusal::Ended`], [`Refusal::InFlight`];
    /// [`Refusal::NoTangent`] with none open; [`Refusal::NotTheScope`] when
    /// the map names an entry the tangent did not create or leaves one it
    /// did unruled. A refused close changes nothing.
    pub fn close_tangent(
        &self,
        dispositions: &BTreeMap<String, crate::object::tangent::Disposition>,
    ) -> Result<(), Rejected> {
        use crate::object::tangent::Disposition;
        let mut state = self.shared.lock();
        let because = match state.settlement {
            Settlement::Ended => Some(Refusal::Ended),
            Settlement::Turn | Settlement::Capture => Some(Refusal::InFlight),
            Settlement::Awaiting if state.tangent.is_none() => Some(Refusal::NoTangent),
            Settlement::Awaiting => None,
        };
        let map: Option<BTreeMap<crate::object::EntryId, Disposition>> = dispositions
            .iter()
            .map(|(id, disposition)| {
                crate::object::EntryId::new(id)
                    .ok()
                    .map(|id| (id, *disposition))
            })
            .collect();
        let at_turn = state.turns;
        let closed = match (because, map) {
            (Some(because), _) => Err(because),
            (None, None) => Err(Refusal::NotTheScope),
            (None, Some(map)) => {
                let state = &mut *state;
                match (state.tangent.as_ref(), state.interview.as_mut()) {
                    (Some((tangent, _)), Some(interview)) => tangent
                        .close(&mut interview.object, at_turn, &map)
                        .map_err(|_| Refusal::NotTheScope),
                    _ => Err(Refusal::NoTangent),
                }
            }
        };
        let closed = match closed {
            Ok(closed) => closed,
            Err(because) => {
                let refused = state.refuse(CommandKind::CloseTangent, because);
                drop(state);
                self.shared.changed.notify_all();
                return Err(Rejected::Refused(refused));
            }
        };
        let Some((tangent, fork_point)) = state.tangent.take() else {
            unreachable!("a close that applied had a tangent open");
        };
        let rolled_back = state.trunk.len().saturating_sub(fork_point.len()) as u64;
        state.trunk = fork_point;
        // What was measured of the trunk the rollback removed no longer holds.
        state.trunk_tokens = None;
        let ruled = |wanted: Disposition| {
            dispositions
                .iter()
                .filter(|(_, disposition)| **disposition == wanted)
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        };
        state.push(Event::TangentClosed {
            id: tangent.id().to_owned(),
            at_turn,
            kept: ruled(Disposition::Keep),
            dropped: ruled(Disposition::Drop),
            parked: ruled(Disposition::Park),
            prefix_intact: closed.prefix_intact,
            rolled_back,
        });
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// End the session.
    ///
    /// # Errors
    ///
    /// [`Refusal::InFlight`] while a turn or a capture is in flight -- stop
    /// it first -- logged -- and [`Refusal::Ended`] if it already ended, not
    /// logged (#291); the refused `gap` is neither logged nor closed. Or [`Rejected::BadGap`]
    /// when an admitted end's `gap` cannot be logged: then it does not end,
    /// and nothing is logged.
    pub fn end(&self, gap: Option<IdleGap>) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        state.carry(gap, CommandKind::End);
        let waiting = state.waiting.as_ref().is_some_and(|w| w.answer.is_none());
        let mut stop = None;
        let outcome = match state.settlement {
            Settlement::Awaiting => state.admit().map(|()| state.move_to(Settlement::Ended)),
            // A prompt waits with no timeout, so `end` is how a session that
            // will not answer it ends: the call settles `cancelled`, then the
            // session ends (#298 point 8).
            Settlement::Turn if waiting => state.admit().map(|()| {
                state.ending = true;
                stop = state.flight.as_ref().map(|flight| flight.cancel.clone());
            }),
            // So does an `end` in the capture gap while its fork is in
            // flight: the fork settles `cancelled`, then the session ends
            // (#374).
            Settlement::Capture if state.forking.is_some() => state.admit().map(|()| {
                state.ending = true;
                stop = state.flight.as_ref().map(|flight| flight.cancel.clone());
            }),
            Settlement::Turn | Settlement::Capture => Err(Rejected::Refused(
                state.refuse(CommandKind::End, Refusal::InFlight),
            )),
            Settlement::Ended => Err(Rejected::Refused(
                state.refuse(CommandKind::End, Refusal::Ended),
            )),
        };
        drop(state);
        self.shared.changed.notify_all();
        if let Some(stop) = stop {
            stop.ask();
        }
        outcome
    }

    /// Answer the prompt waiting on call `call` (#298 point 8): `decline`
    /// refuses it `declined`; `once`, `session` or `workspace` runs it,
    /// the last two growing the allow set by its shapes.
    ///
    /// # Errors
    ///
    /// [`ApproveRefusal`]: nothing waits, another call's prompt does, a
    /// standing scope for a prompt only `once` can answer, or `workspace` in
    /// a session with no store. Nothing is logged for a refusal.
    pub fn approve(&self, call: &str, decision: Decision) -> Result<(), ApproveRefusal> {
        let mut state = self.shared.lock();
        let now = state.now();
        let store = self
            .shared
            .tools
            .as_ref()
            .is_some_and(|tools| tools.store.is_some());
        let Some(waiting) = state.waiting.as_mut() else {
            return Err(ApproveRefusal::NothingWaiting);
        };
        if waiting.prompt.id != call || waiting.answer.is_some() {
            return Err(ApproveRefusal::Stale);
        }
        let standing = matches!(decision, Decision::Session | Decision::Workspace);
        if standing && !waiting.prompt.standing {
            return Err(ApproveRefusal::NotStanding);
        }
        if decision == Decision::Workspace && !store {
            return Err(ApproveRefusal::NoStore);
        }
        waiting.answer = Some((decision, now));
        let decided = (waiting.prompt.request, waiting.prompt.id.clone());
        state.answered = Some(decided);
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// The prompt waiting on the operator, if one is.
    #[must_use]
    pub fn waiting(&self) -> Option<Prompt> {
        let state = self.shared.lock();
        state
            .waiting
            .as_ref()
            .filter(|waiting| waiting.answer.is_none())
            .map(|waiting| waiting.prompt.clone())
    }

    /// [`Session::wait_from`], also returning when the prompt waiting is no
    /// longer `shown`: the events from `first` on, the prompt waiting now,
    /// and the latest prompt decided (its request and call id).
    #[must_use]
    pub fn wait_for(
        &self,
        first: u64,
        patience: Duration,
        shown: Option<&Prompt>,
    ) -> (Vec<Logged>, Option<Prompt>, Option<(u64, String)>) {
        let until = Instant::now() + patience;
        let mut state = self.shared.lock();
        loop {
            let found = from(&state.log, first);
            let prompt = state
                .waiting
                .as_ref()
                .filter(|waiting| waiting.answer.is_none())
                .map(|waiting| waiting.prompt.clone());
            let now = Instant::now();
            if !found.is_empty() || prompt.as_ref() != shown || now >= until {
                return (found, prompt, state.answered.clone());
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, until - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// The session's product: its working object at this moment, dumped as
    /// the batch drive dumps its own ([`super::dump`]). Empty when the
    /// regimen warrants no fork, or no fork patched anything.
    #[must_use]
    pub fn product(&self) -> String {
        let state = self.shared.lock();
        state
            .interview
            .as_ref()
            .map(|interview| super::dump(&interview.object))
            .unwrap_or_default()
    }

    /// The session's receipt (#298 point 9), when it runs commands: the
    /// allow set it ends with, the decisions, and the rest of
    /// [`tool_loop::receipt`]. `reference_modified` is read now.
    #[must_use]
    pub fn receipt(&self) -> Option<Value> {
        let tools = self.shared.tools.as_ref()?;
        let (allowed, counts) = {
            let state = self.shared.lock();
            (state.allowed.clone(), state.counts)
        };
        let reference = tool_loop::reference_modified(&tools.policy);
        Some(tool_loop::receipt(
            &allowed,
            counts,
            tools.approval_policy.as_deref(),
            &tools.policy,
            &reference,
        ))
    }

    /// End what the session's commands left running: every process group a
    /// command led, under Seatbelt. Called once, at the end.
    pub fn end_commands(&self) {
        // Every running background job's process group (#614), as a stop.
        {
            let mut state = self.shared.lock();
            for job in state.jobs.values_mut() {
                if job.status == super::background::Status::Running {
                    job.stopping = true;
                    super::background::stop_group(job.pid);
                }
            }
        }
        if let Some(tools) = &self.shared.tools {
            tools.confinement.end_session();
        }
    }

    /// Move the running foreground `bash` call to the background (#614): it
    /// returns at once, Qwen Code's promotion answer, and the command keeps
    /// running under the session's confinement, its output written to a
    /// file. Returns the job's id.
    ///
    /// # Errors
    ///
    /// Refused, and logged: [`Refusal::Ended`]; [`Refusal::NothingRunning`]
    /// when no `bash` call is running, or one is already being moved.
    pub fn background(&self) -> Result<String, Rejected> {
        let mut state = self.shared.lock();
        let slot = match (state.settlement, &state.promotable) {
            (Settlement::Ended, _) => Err(Refusal::Ended),
            (_, Some(slot)) => Ok(Arc::clone(slot)),
            (_, None) => Err(Refusal::NothingRunning),
        };
        let asked = slot.and_then(|slot| {
            let mut job = slot.lock().unwrap_or_else(PoisonError::into_inner);
            if job.is_some() {
                return Err(Refusal::NothingRunning);
            }
            let id = super::background::new_id();
            let (output, status_file) =
                super::background::files_of(&background_dir(&self.shared, &state), &id);
            *job = Some(super::background::Job {
                id: id.clone(),
                command: String::new(),
                cwd: String::new(),
                pid: 0,
                output,
                status_file,
                status: super::background::Status::Running,
                exit: None,
                stopping: false,
            });
            Ok(id)
        });
        match asked {
            Ok(id) => Ok(id),
            Err(because) => {
                let refused = state.refuse(CommandKind::Background, because);
                drop(state);
                self.shared.changed.notify_all();
                Err(Rejected::Refused(refused))
            }
        }
    }

    /// When the session opened, as its first event says: milliseconds since
    /// the Unix epoch. What tells this session's log from another's.
    #[must_use]
    pub fn opened(&self) -> u64 {
        let state = self.shared.lock();
        let Some(Logged {
            event: Event::Started { opened, .. },
            ..
        }) = state.log.first()
        else {
            unreachable!("`open` pushes `Started` before anything else can be logged");
        };
        *opened
    }

    /// What the session is doing now.
    #[must_use]
    pub fn settlement(&self) -> Settlement {
        self.shared.lock().settlement
    }

    /// The trunk as it stands: the head, then every settled exchange.
    #[must_use]
    pub fn trunk(&self) -> Vec<Message> {
        self.shared.lock().trunk.clone()
    }

    /// Hand `sink` every event logged so far, then each one as it is
    /// appended (`--log`, #157). It runs on the appending thread, under the
    /// session's lock, so an append returns only after `sink` has returned
    /// (#230, ruled: the write is the appender's own, not a thread's that
    /// may lag it). A `sink` must not call back into the session.
    pub fn write_through(&self, mut sink: Sink) {
        let mut state = self.shared.lock();
        for logged in &state.log {
            sink(logged);
        }
        state.sink = Some(sink);
    }

    /// Every event from sequence number `first` on.
    #[must_use]
    pub fn events_from(&self, first: u64) -> Vec<Logged> {
        let state = self.shared.lock();
        from(&state.log, first)
    }

    /// Every event from `first` on, waiting up to `patience` for at least one
    /// to exist. Empty when none arrived in time.
    #[must_use]
    pub fn wait_from(&self, first: u64, patience: Duration) -> Vec<Logged> {
        let until = Instant::now() + patience;
        let mut state = self.shared.lock();
        loop {
            let found = from(&state.log, first);
            let now = Instant::now();
            if !found.is_empty() || now >= until {
                return found;
            }
            state = self
                .shared
                .changed
                .wait_timeout(state, until - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }
}

impl<S: Streaming + 'static> Drop for Session<S> {
    /// A session dropped mid-turn stops its call, rather than leaving it
    /// running to its cap for a log nobody will read.
    fn drop(&mut self) {
        let cancel = self
            .shared
            .lock()
            .flight
            .as_ref()
            .map(|flight| flight.cancel.clone());
        if let Some(cancel) = cancel {
            cancel.ask();
        }
    }
}

fn from(log: &[Logged], first: u64) -> Vec<Logged> {
    let start = usize::try_from(first).map_or(log.len(), |start| start.min(log.len()));
    log[start..].to_vec()
}

/// The template variables a session sends, in the log's words (R1): the
/// two it knows, `enable_thinking` and `reasoning_effort`. `None` when it
/// sends neither.
fn logged_kwargs(kwargs: &BTreeMap<String, Value>) -> Option<log::TemplateKwargs> {
    let logged = log::TemplateKwargs {
        enable_thinking: match kwargs.get("enable_thinking") {
            Some(Value::Boolean(thinking)) => Some(*thinking),
            _ => None,
        },
        reasoning_effort: match kwargs.get("reasoning_effort") {
            Some(Value::String(effort)) => Some(effort.clone()),
            _ => None,
        },
        preserve_thinking: match kwargs.get("preserve_thinking") {
            Some(Value::Boolean(preserve)) => Some(*preserve),
            _ => None,
        },
    };
    (logged != log::TemplateKwargs::default()).then_some(logged)
}

/// A logged event as a line of the session log format, `diet/formats/log`,
/// at its current [`log::VERSION`] (#117, R2c I3). One exhaustive match, so an event with no line fails
/// to compile, and every word goes through the format's own vocabulary.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn line_of(logged: &Logged) -> log::Line {
    let event = match &logged.event {
        Event::Started {
            opened,
            model,
            head,
            serving,
            claim,
            tools,
            template_kwargs,
            unsent,
            approvals_off,
            fork_delivery,
            fork_asks,
            reasoning_effort_default,
            tool_output,
            bash_timeout_ms,
            phases,
            instruction_files,
            levers,
        } => log::Event::SessionStart {
            bash_timeout_ms: *bash_timeout_ms,
            levers: levers.clone(),
            // #563: the graph, and the phase it opens in; nothing when none.
            phases: phases.as_ref().map(|(names, _, _)| names.clone()),
            phase_transitions: phases.as_ref().map(|(_, moves, _)| moves.clone()),
            opening_phase: phases.as_ref().map(|(_, _, first)| first.clone()),
            fork_asks: fork_asks.map(|set| log::ForkAsks {
                name: set.name.to_owned(),
                digest: Some(set.digest()),
            }),
            // #559: by path and digest, their text in `head`; nothing when none.
            instruction_files: Some(instruction_files.clone()).filter(|files| !files.is_empty()),
            fork_delivery: *fork_delivery,
            reasoning_effort_default: reasoning_effort_default.clone(),
            tool_output: tool_output.map(tool_output_of),
            // The approval lever's `none`: `true`, or nothing.
            approvals_off: approvals_off.then_some(true),
            unsent: unsent.clone(),
            version: log::VERSION,
            // R1: what reaches the template, as sent; nothing when nothing is.
            template_kwargs: logged_kwargs(template_kwargs),
            opened: *opened,
            model: model.clone(),
            head: head
                .iter()
                .map(|message| log::HeadMessage {
                    role: role_of(message.role),
                    content: message.content.clone(),
                })
                .collect(),
            // v2's declaration, written once the session carries one (#30's I2).
            serving: serving.as_ref().map(|serving| log::Serving {
                dialect: serving.dialect.name.clone(),
                concurrency: match serving.concurrency {
                    Concurrency::Declared(streams) => Some(u64::from(streams)),
                    Concurrency::Undeclared => None,
                },
            }),
            // v3's substrate claim, as `serve` announced it (#292).
            claim: claim.clone(),
            // A session writing as it runs carries no provenance word.
            provenance: None,
            // The declared tools, by name (#472): none written when there are none.
            tools: Some(tools.clone()).filter(|tools| !tools.is_empty()),
        },
        Event::Asked {
            turn,
            text,
            scoping,
            files,
        } => log::Event::Ask {
            turn: *turn,
            text: text.clone(),
            // Only the operator's mark is written (#374): `true`, or nothing.
            scoping: scoping.then_some(true),
            // By reference, never inlined (#372): nothing when nothing was
            // kept, as the format reads an empty list as no list.
            files: (!files.is_empty()).then(|| files.clone()),
        },
        Event::Requested {
            turn,
            lane,
            head_sha256,
            fork,
            max_tokens,
        } => log::Event::Request {
            turn: *turn,
            lane: match lane {
                Lane::Trunk => log::Lane::Trunk,
                Lane::Interview => log::Lane::Interview,
                Lane::Audit => log::Lane::Audit,
            },
            head_sha256: Some(head_sha256.clone()),
            fork: *fork,
            max_tokens: Some(u64::from(*max_tokens)),
        },
        Event::Settled { from, to } => log::Event::Settlement {
            from: state_of(*from),
            to: state_of(*to),
        },
        Event::Refused {
            command,
            because,
            during,
        } => log::Event::Refused {
            command: command_of(*command),
            because: refusal_of(*because),
            during: state_of(*during),
        },
        Event::Reasoning { request, text } => log::Event::Delta {
            request: *request,
            piece: log::Piece::Reasoning(text.clone()),
        },
        Event::Delta { request, text } => log::Event::Delta {
            request: *request,
            piece: log::Piece::Text(text.clone()),
        },
        Event::StopAsked { turn } => log::Event::StopAsked { turn: *turn },
        // A step's response that made calls is a `response` too: the
        // request is over, and its calls are the `tool_call` lines after it.
        Event::Answered {
            request,
            text,
            finish_reason,
            reasoning,
            timings,
            hosted,
        }
        | Event::Called {
            request,
            text,
            finish_reason,
            reasoning,
            timings,
            hosted,
        } => log::Event::Response {
            to_request: *request,
            text: text.clone(),
            finish_reason: finish_reason.clone(),
            reasoning: reasoning.clone(),
            timings: timings.as_ref().map(timings_line),
            // `usage` for a dialect with no timings: a hosted API's (#555);
            // `capped` is written only by a capped call, below.
            usage: hosted.usage.clone(),
            capped: None,
            reasoning_signature: hosted.signature.clone(),
            redacted: (!hosted.redacted.is_empty()).then(|| hosted.redacted.clone()),
        },
        Event::Capped {
            request,
            text,
            finish_reason,
            reasoning,
            timings,
            hosted,
        } => log::Event::Response {
            to_request: *request,
            text: text.clone(),
            finish_reason: finish_reason.clone(),
            reasoning: reasoning.clone(),
            timings: timings.as_ref().map(timings_line),
            usage: hosted.usage.clone(),
            capped: Some(true),
            reasoning_signature: hosted.signature.clone(),
            redacted: (!hosted.redacted.is_empty()).then(|| hosted.redacted.clone()),
        },
        Event::Progress { request, progress } => log::Event::Progress {
            request: *request,
            total: progress.total,
            cache: progress.cache,
            processed: progress.processed,
            time_ms: progress.time_ms,
        },
        Event::Cancelled { request, partial } => log::Event::Cancelled {
            request: *request,
            partial: partial.clone(),
            // v3's reasoning before the stop, written once the session
            // carries it (#291).
            reasoning: None,
        },
        Event::Rejected {
            request,
            status,
            body,
            class,
            partial,
            overflow,
        } => log::Event::RequestFailed {
            request: *request,
            reason: match (class, overflow) {
                (Some(Rejection::Refusal), _) => log::FailReason::Refusal,
                (Some(Rejection::ContextOverflow), _) | (None, Some(_)) => {
                    log::FailReason::ContextOverflow
                }
                (None, None) => log::FailReason::Server,
            },
            message: body.clone(),
            status: Some(*status),
            partial: arrived(partial),
            overflow: *overflow,
        },
        Event::Failed {
            request,
            failure,
            partial,
            overflow,
        } => failed_line(*request, failure, partial, *overflow),
        Event::Crashed {
            request,
            partial,
            why,
        } => log::Event::RequestFailed {
            request: *request,
            reason: log::FailReason::Crashed,
            message: why.clone(),
            status: None,
            partial: arrived(partial),
            overflow: None,
        },
        Event::IdleGap(gap) => gap_line(gap),
        Event::ToolCallPiece {
            request,
            index,
            id,
            name,
            arguments,
        } => log::Event::Delta {
            request: *request,
            piece: log::Piece::ToolCall {
                index: *index,
                id: id.clone(),
                name: name.clone(),
                arguments: arguments.clone(),
            },
        },
        Event::ToolCalled(line) => {
            let ToolLine {
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
                shown,
                files,
                images: _,
                recovered_from,
                background,
                timeout_ms,
            } = line.as_ref().clone();
            log::Event::ToolCall {
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
                files: (!files.is_empty()).then_some(files),
                shown,
                recovered_from,
                background,
                timeout_ms,
            }
        }
        Event::TimeoutNear {
            request,
            call,
            timeout_ms,
        } => log::Event::TimeoutNear {
            request: *request,
            call: call.clone(),
            timeout_ms: *timeout_ms,
        },
        Event::BackgroundEnded {
            job,
            status,
            exit,
            files,
        } => log::Event::BackgroundEnded {
            job: job.clone(),
            status: *status,
            exit: *exit,
            files: (!files.is_empty()).then(|| files.clone()),
        },
        Event::Notified { turn, text } => log::Event::Notice {
            turn: *turn,
            text: text.clone(),
        },
        Event::TurnSettled { turn, reason } => log::Event::TurnSettled {
            turn: *turn,
            reason: settle_reason_in_the_log(*reason),
        },
        Event::Forked {
            of_turn,
            at,
            why,
            question,
            view,
            trigger,
            role,
            seat,
            ask,
            displaces,
        } => log::Event::Fork {
            lane: log::Lane::Interview,
            of_turn: *of_turn,
            at: *at,
            why: *why,
            question: question.clone(),
            // Absent is the tail (#568): a tail fork's line is as before.
            // Absent is the whole trunk (#567).
            view: (*view != ForkView::Trunk).then(|| view.word()),
            trigger: Some(trigger.clone()),
            // Absent is `user` (#599): a user-role fork's line is as before.
            role: (*role != Role::User).then(|| role.tag().to_owned()),
            seat: seat.clone(),
            ask: Some(ask.tag().to_owned()),
            hazard: displaces.then(|| DISPLACES_TRUNK_CACHE.to_owned()),
        },
        Event::Audited {
            of_turn,
            at,
            template,
            question,
        } => log::Event::Fork {
            lane: log::Lane::Audit,
            of_turn: *of_turn,
            at: *at,
            why: log::Warrant::Seam,
            question: question.clone(),
            view: None,
            trigger: None,
            role: None,
            seat: None,
            // The pinned ask it put, by its dogma name, lower case.
            ask: Some(template.name().to_lowercase()),
            hazard: None,
        },
        Event::ForkSettled {
            fork,
            outcome,
            prompt_tokens,
            wall_ms,
            refused,
        } => log::Event::ForkSettled {
            fork: *fork,
            outcome: *outcome,
            prompt_tokens: *prompt_tokens,
            wall_ms: *wall_ms,
            refused: refused.clone(),
        },
        Event::Recalled {
            turn,
            recall,
            text,
            items,
        } => log::Event::Recalled {
            turn: *turn,
            recall: *recall,
            text: text.clone(),
            items: items.clone(),
        },
        Event::Delivered {
            turn,
            framing,
            text,
            lines,
        } => log::Event::Delivered {
            turn: *turn,
            framing: *framing,
            text: text.clone(),
            lines: lines.clone(),
        },
        Event::Patched {
            fork,
            lane,
            op,
            entry,
            supersedes,
            tangent,
        } => log::Event::Patch {
            fork: *fork,
            lane: lane.clone(),
            op: *op,
            entry: entry.clone(),
            supersedes: supersedes.clone(),
            tangent: tangent.clone(),
        },
        Event::Captured {
            request,
            call,
            tool,
            outcome,
            entries,
            why,
            fork,
        } => log::Event::Capture {
            request: *request,
            call: call.clone(),
            tool: tool.clone(),
            outcome: outcome.clone(),
            entries: entries.clone(),
            why: why.clone(),
            fork: *fork,
        },
        Event::Skipped {
            of_turn,
            trigger,
            ask,
            call,
            field,
        } => log::Event::ForkSkipped {
            of_turn: *of_turn,
            trigger: trigger.clone(),
            ask: ask.tag().to_owned(),
            call: call.clone(),
            field: field.clone(),
        },
        Event::Reminded { turn, text } => log::Event::Reminded {
            turn: *turn,
            text: text.clone(),
        },
        Event::TangentOpened {
            id,
            at_turn,
            trunk_messages,
        } => log::Event::TangentOpen {
            id: id.clone(),
            at_turn: *at_turn,
            trunk_messages: *trunk_messages,
        },
        Event::TangentClosed {
            id,
            at_turn,
            kept,
            dropped,
            parked,
            prefix_intact,
            rolled_back,
        } => log::Event::TangentClose {
            id: id.clone(),
            at_turn: *at_turn,
            kept: kept.clone(),
            dropped: dropped.clone(),
            parked: parked.clone(),
            prefix_intact: *prefix_intact,
            rolled_back: *rolled_back,
        },
        Event::Seamed {
            at_turn,
            reason,
            prefix_hash_before,
            prefix_hash_after,
            render,
            carried_entries,
            tail_tokens,
            carried_turns,
            carried_tokens,
            phase,
            tool_outputs,
            outputs,
            render_budget,
            fired,
            pruned,
            warm,
        } => log::Event::Seam {
            fired: *fired,
            warm: warm.as_ref().map(timings_line),
            phase: phase.clone(),
            at_turn: *at_turn,
            reason: match reason {
                crate::seam::Reason::Operator => log::SeamReason::Operator,
                crate::seam::Reason::Phase => log::SeamReason::Phase,
                crate::seam::Reason::Budget => log::SeamReason::Budget,
                crate::seam::Reason::Cadence => log::SeamReason::Cadence,
                crate::seam::Reason::Window => log::SeamReason::Window,
                crate::seam::Reason::Prune => log::SeamReason::Prune,
            },
            prefix_hash_before: prefix_hash_before.clone(),
            prefix_hash_after: prefix_hash_after.clone(),
            frame: crate::seam::render::FRAME_VERSION.to_owned(),
            render: render.clone(),
            carried_entries: *carried_entries,
            carried_turns: *carried_turns,
            // The depth, and what it kept, on a seam that could keep a tail
            // (#552); a total compaction writes neither.
            tail_tokens: (*tail_tokens > 0).then_some(*tail_tokens),
            carried_tokens: (*tail_tokens > 0).then_some(*carried_tokens),
            // The state always (#553); the section, its count and its
            // bytes when it carried any.
            tool_outputs: Some(*tool_outputs),
            outputs: outputs.as_ref().map(|(text, _)| text.clone()),
            carried_outputs: outputs.as_ref().map(|(_, n)| *n),
            carried_output_bytes: outputs.as_ref().map(|(text, _)| text.len() as u64),
            // A user message after the head (#597).
            placement: Some(log::RenderPlacement::Message),
            render_budget: render_budget.clone(),
            pruned: pruned.clone(),
        },
        Event::Pruned {
            turn,
            call,
            sha256,
            bytes,
            text,
        } => log::Event::Pruned {
            turn: *turn,
            call: call.clone(),
            sha256: sha256.clone(),
            bytes: *bytes,
            text: text.clone(),
        },
    };
    log::Line {
        seq: logged.seq,
        t: logged.t,
        event,
    }
}

/// A logged event, written as one line of the log format: what
/// `drive::serve` sends as each event's `data:`.
#[must_use]
pub fn render(logged: &Logged) -> String {
    log::render(&line_of(logged))
}

/// A call's timings as the log carries them: the same keys, the
/// milliseconds as the digits the server wrote.
fn timings_line(timings: &Timings) -> log::Timings {
    let millis = |ms: &Option<crate::client::stream::Millis>| {
        ms.as_ref().and_then(|ms| log::Millis::new(ms.as_str()))
    };
    log::Timings {
        prompt_n: timings.prompt_n,
        cache_n: timings.cache_n,
        prompt_ms: millis(&timings.prompt_ms),
        predicted_n: timings.predicted_n,
        predicted_ms: millis(&timings.predicted_ms),
        draft_n: timings.draft_n,
        draft_n_accepted: timings.draft_n_accepted,
    }
}

/// A failed call's line: a timeout is `timeout`, one whose prompt
/// overflowed the window `context_overflow` (#616), every other transport
/// failure `transport`, and the failure's own words are the message.
fn failed_line(
    request: u64,
    failure: &TransportFailure,
    partial: &str,
    overflow: Option<log::Overflow>,
) -> log::Event {
    log::Event::RequestFailed {
        request,
        reason: match (failure, overflow) {
            (TransportFailure::Timeout { .. }, _) => log::FailReason::Timeout,
            (_, Some(_)) => log::FailReason::ContextOverflow,
            (_, None) => log::FailReason::Transport,
        },
        message: failure.to_string(),
        status: None,
        partial: arrived(partial),
        overflow,
    }
}

/// `partial` as the log format carries it: "what arrived before it ended,
/// when anything did" -- absent when nothing did, never an empty string.
fn arrived(partial: &str) -> Option<String> {
    (!partial.is_empty()).then(|| partial.to_owned())
}

fn gap_line(gap: &IdleGap) -> log::Event {
    log::Event::IdleGap {
        opened_by: gap.opened_by,
        notice: gap.notice,
        read: gap.read,
        compose: gap.compose,
        away: gap.away,
        blocked: gap.blocked,
        ended_by: match gap.ended_by {
            GapEnd::Ask => log::GapEnd::Ask,
            GapEnd::Seam => log::GapEnd::Seam,
            GapEnd::Cancel => log::GapEnd::Cancel,
            GapEnd::End => log::GapEnd::End,
        },
    }
}

/// A head message's role. A head is the trunk's opening, before any turn,
/// and a tool message answers a call made in a turn, so none is in one: the
/// session opens only on a head its log can write.
fn role_of(role: Role) -> log::Role {
    match role {
        Role::Tool => unreachable!("a head holds no tool result: `open` asserts it"),
        Role::Developer => {
            unreachable!("a head holds no developer message, a fork's ask alone: `open` asserts it")
        }
        Role::System => log::Role::System,
        Role::User => log::Role::User,
        Role::Assistant => log::Role::Assistant,
    }
}

fn command_of(command: CommandKind) -> log::Command {
    match command {
        CommandKind::Ask => log::Command::Ask,
        CommandKind::Cancel => log::Command::Cancel,
        CommandKind::DeclareSeam => log::Command::DeclareSeam,
        CommandKind::End => log::Command::End,
        CommandKind::OpenTangent => log::Command::OpenTangent,
        CommandKind::CloseTangent => log::Command::CloseTangent,
        CommandKind::Background => log::Command::Background,
    }
}

fn refusal_of(refusal: Refusal) -> log::Refusal {
    match refusal {
        Refusal::InFlight => log::Refusal::InFlight,
        Refusal::Ended => log::Refusal::Ended,
        Refusal::NothingInFlight => log::Refusal::NothingInFlight,
        Refusal::NothingToSeam => log::Refusal::NothingToSeam,
        Refusal::NoPhaseGraph => log::Refusal::NoPhaseGraph,
        Refusal::NotAPhase => log::Refusal::NotAPhase,
        Refusal::AlreadyInPhase => log::Refusal::AlreadyInPhase,
        Refusal::NoPhaseEdge => log::Refusal::NoPhaseEdge,
        Refusal::Stale => log::Refusal::Stale,
        Refusal::TangentOpen => log::Refusal::TangentOpen,
        Refusal::NoTangent => log::Refusal::NoTangent,
        Refusal::BadTangent => log::Refusal::BadTangent,
        Refusal::NotTheScope => log::Refusal::NotTheScope,
        Refusal::NothingRunning => log::Refusal::NothingRunning,
    }
}

fn settle_reason_in_the_log(reason: SettleReason) -> log::SettleReason {
    match reason {
        SettleReason::Final => log::SettleReason::Final,
        SettleReason::Cancelled => log::SettleReason::Cancelled,
        SettleReason::MaxSteps => log::SettleReason::MaxSteps,
        SettleReason::Timeout => log::SettleReason::Timeout,
        SettleReason::Failed => log::SettleReason::Failed,
    }
}

fn state_of(settlement: Settlement) -> log::State {
    match settlement {
        Settlement::Awaiting => log::State::Awaiting,
        Settlement::Turn => log::State::Turn,
        Settlement::Capture => log::State::Capture,
        Settlement::Ended => log::State::Ended,
    }
}

/// One turn, on its own thread: a step per request, until the trunk answers
/// or the loop ends (#29 D11: a step is a request).
fn call<S: Streaming>(
    shared: &Shared<S>,
    shape: &RequestShape,
    cancel: &Cancel,
    opening: Vec<Message>,
    turn: u32,
    request: u64,
) {
    let mut shape = shape.clone();
    let mut request = request;
    let mut steps = 1;
    // The turn's exchange so far: its ask, then each step's calls and their
    // results. It joins the trunk when the turn settles `final` or
    // `max_steps` (Q12); settling `failed` or `timeout` after a step, the
    // steps that completed join it (`State::keep_ran_steps`); cancelled,
    // never (D13).
    let mut exchange = opening;
    shared.lock().ran.clear();
    while let Some(next) = step(
        shared,
        &mut shape,
        cancel,
        &mut exchange,
        (turn, request, steps),
    ) {
        request = next;
        steps += 1;
    }
}

/// Whether `finish_reason` is a cap's.
fn capped(finish_reason: Option<&str>) -> bool {
    finish_reason.is_some_and(|reason| CAPPED_FINISH_REASONS.contains(&reason))
}

/// One step's request: stream it into the log, then settle the turn, or run
/// the calls it made and return the next request's sequence number.
#[allow(clippy::too_many_lines)]
fn step<S: Streaming>(
    shared: &Shared<S>,
    shape: &mut RequestShape,
    cancel: &Cancel,
    exchange: &mut Vec<Message>,
    (turn, request, steps): (u32, u64, u32),
) -> Option<u64> {
    let deadline = Instant::now() + shared.template.limits.call;
    let mut partial = String::new();
    let mut reasoning = String::new();
    let mut calls = Calls::default();
    let mut hosted = Hosted::default();
    let result = shared
        .transport
        .stream(shape, deadline, cancel, &mut |piece: Piece<'_>| {
            if hosted.took(&piece) {
                return;
            }
            let event = match piece {
                Piece::Text(text) => {
                    partial.push_str(text);
                    Event::Delta {
                        request,
                        text: text.to_owned(),
                    }
                }
                Piece::Reasoning(text) => {
                    reasoning.push_str(text);
                    Event::Reasoning {
                        request,
                        text: text.to_owned(),
                    }
                }
                Piece::Progress(progress) => Event::Progress { request, progress },
                Piece::Signature(_) | Piece::Redacted(_) | Piece::Usage(_) => return,
                // Each fragment as the server sent it, and assembled beside
                // the log: never answer text (#298 T11).
                Piece::ToolCall {
                    index,
                    id,
                    name,
                    arguments,
                } => {
                    calls.piece(index, id, name, arguments);
                    Event::ToolCallPiece {
                        request,
                        index,
                        id: id.map(str::to_owned),
                        name: name.map(str::to_owned),
                        arguments: arguments.to_owned(),
                    }
                }
            };
            shared.lock().push(event);
            shared.changed.notify_all();
        });

    // #560: a turn that made no native call but wrote one in its answer, under
    // a regimen that turns the fallback on, has the call recovered from its
    // text, as Qwen Code does (`xml-tool-call-fallback.ts`, called at
    // `llm-chat.ts` only for a finished turn with no native call). The log
    // keeps the answer as the model wrote it; the trunk carries what is left
    // of it, with the recovered calls.
    let mut said_text = None;
    let mut recovered = BTreeMap::new();
    if let Ok(StreamEnded::Finished {
        finish_reason: Some(reason),
        ..
    }) = &result
        && calls.is_empty()
        && !capped(Some(reason.as_str()))
        && shared
            .tools
            .as_ref()
            .is_some_and(|tools| tools.text_fallback)
        && crate::client::xml_fallback::contains_xml_tool_calls(&partial)
        && let Some(recovery) = crate::client::xml_fallback::try_recover(&partial)
    {
        for (index, call) in recovery.calls.iter().enumerate() {
            let id = format!("recovered-{request}-{index}");
            let arguments = serde_json::Value::Object(call.arguments.clone()).to_string();
            calls.piece(index as u64, Some(&id), Some(&call.name), &arguments);
            recovered.insert(id, call.source.clone());
        }
        said_text = Some(recovery.remaining);
    }
    let mut state = shared.lock();
    state.flight = None;
    state.recovered.extend(recovered);
    let mut forked = None;
    match result {
        Ok(StreamEnded::Finished {
            finish_reason,
            timings,
        }) if calls.is_empty() || capped(finish_reason.as_deref()) => {
            if settle_finished(
                &mut state,
                std::mem::take(exchange),
                (request, turn),
                (partial, reasoning),
                (finish_reason, timings, hosted),
            ) {
                forked = gap(shared, &mut state, turn, request);
            }
        }
        Ok(StreamEnded::Finished {
            finish_reason,
            timings,
        }) => {
            // Still the turn's call: a cancel reaches it between steps and
            // while a prompt waits.
            state.flight = Some(Flight {
                turn,
                request,
                cancel: cancel.clone(),
            });
            let calls = calls.into_calls();
            let reasoning = Some(reasoning).filter(|thought| !thought.is_empty());
            let mut said = Message::new(
                Role::Assistant,
                said_text.unwrap_or_else(|| partial.clone()),
            );
            said.reasoning.clone_from(&reasoning);
            hosted.onto(&mut said);
            said.tool_calls = calls
                .iter()
                .map(|call| ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                })
                .collect();
            state.step_tokens = timings.as_ref().and_then(trunk_tokens_of);
            state.push(Event::Called {
                request,
                text: partial,
                finish_reason,
                reasoning,
                timings,
                hosted,
            });
            drop(state);
            shared.changed.notify_all();
            return run_calls(
                shared,
                (shape, exchange),
                cancel,
                said,
                &calls,
                (turn, request, steps),
            );
        }
        Ok(StreamEnded::Cancelled) => {
            // The steps it finished stay on the trunk, and so does what it
            // had said when the cancel came, as `OpenCode` and Qwen Code keep
            // it (#575): its text, unmarked. Its reasoning is not kept: the
            // log's `cancelled` line carries none to rebuild it from.
            if partial.is_empty() {
                state.keep_ran_steps();
            } else {
                let mut kept = std::mem::take(&mut state.ran);
                if kept.is_empty() {
                    kept.push(
                        exchange
                            .first()
                            .cloned()
                            .unwrap_or_else(|| Message::new(Role::User, String::new())),
                    );
                }
                kept.push(Message::new(Role::Assistant, partial.clone()));
                state.trunk.extend(kept);
                if let Some(measured) = state.step_tokens.take() {
                    state.trunk_tokens = Some(measured);
                }
            }
            state.push(Event::Cancelled { request, partial });
            state.push(Event::TurnSettled {
                turn,
                reason: SettleReason::Cancelled,
            });
            state.move_to(Settlement::Awaiting);
        }
        Ok(StreamEnded::Rejected {
            status,
            body,
            class,
        }) => {
            let overflow = state.overflow(class == Some(Rejection::ContextOverflow));
            state.push(Event::Rejected {
                request,
                status,
                body,
                overflow,
                class,
                partial,
            });
            // #617: an overflow seams once and goes again.
            if overflow.is_some()
                && let Some(next) = retried(&shared.template, &mut state, shape, turn)
            {
                drop(state);
                shared.changed.notify_all();
                return Some(next);
            }
            state.keep_ran_steps();
            state.push(Event::TurnSettled {
                turn,
                reason: SettleReason::Failed,
            });
            state.move_to(Settlement::Awaiting);
        }
        Err(failure) => {
            let reason = settle_reason_of(&failure);
            let overflow = state.failed_overflow(&failure);
            state.push(Event::Failed {
                request,
                overflow,
                failure,
                partial,
            });
            // #617: an overflow seams once and goes again.
            if overflow.is_some()
                && let Some(next) = retried(&shared.template, &mut state, shape, turn)
            {
                drop(state);
                shared.changed.notify_all();
                return Some(next);
            }
            state.keep_ran_steps();
            state.push(Event::TurnSettled { turn, reason });
            state.move_to(Settlement::Awaiting);
        }
    }
    drop(state);
    shared.changed.notify_all();
    if let Some(forked) = forked {
        interview(shared, forked);
    }
    None
}

/// What a cancelled turn tells the model of a call that was running when
/// the cancel came: after what the command printed, Qwen Code's own result
/// for a call the user cancelled (`qc:packages/core/src/core/coreToolScheduler.ts`
/// 467-468 and 1201-1209 at `c0c697c8`). The other harnesses answer it too,
/// in their own words (Pi "Command aborted", `OpenCode` "Tool execution
/// interrupted"); where the wording differs, Qwen Code's, the harness
/// closest to the model this drives (#575).
pub const CANCELLED_CALL: &str = "[Operation Cancelled] Reason: User intentionally cancelled \
     this tool call. Stop and await further instructions; do not retry or work around it.";

/// What a cancelled turn tells the model of a call that had not started --
/// or was waiting on the operator -- when the cancel came: Qwen Code's
/// repair text for a call with no recorded result
/// (`qc:packages/core/src/core/llm-chat.ts` 1817-1819 at `c0c697c8`), which
/// is what its model sees after an interrupt (#575).
pub const UNRECORDED_CALL: &str = "Tool execution result was not recorded — likely interrupted \
     by network failure, abort, or process exit. Treat as failure and retry if needed.";

/// A step's calls, each to its one line, in the order they were made; then
/// the turn goes on with their results, or settles: `cancelled` if a stop
/// reached it, `failed` if a call named a tool the session did not declare
/// (T13), `max_steps` if this was the last step (T8, Q12: the calls that ran
/// stay on the trunk).
fn run_calls<S: Streaming>(
    shared: &Shared<S>,
    (shape, exchange): (&mut RequestShape, &mut Vec<Message>),
    cancel: &Cancel,
    said: Message,
    calls: &[Call],
    (turn, request, steps): (u32, u64, u32),
) -> Option<u64> {
    let last = shared
        .tools
        .as_ref()
        .and_then(|tools| tools.max_steps)
        .is_some_and(|max| steps >= max);
    let mut results = Vec::new();
    let mut unknown = false;
    let mut stopped = false;
    for call in calls {
        let (mut line, shown) = if stopped || cancel.is_asked() {
            (cancelled_line(shared, (turn, request), call), None)
        } else {
            one_call(shared, cancel, (turn, request), call, last)
        };
        // A call a cancel reached before it ran is still answered, as every
        // harness answers it (#575).
        let shown = if line.outcome == log::ToolOutcome::Cancelled && shown.is_none() {
            Some(UNRECORDED_CALL.to_owned())
        } else {
            shown
        };
        // The one string the result message carries, logged as given.
        line.shown.clone_from(&shown);
        unknown |= line.reason == Some(log::ToolRefusal::UnknownTool);
        stopped |= line.outcome == log::ToolOutcome::Cancelled;
        let images = std::mem::take(&mut line.images);
        if let Some(shown) = shown {
            let mut result = Message::tool_result(call.id.clone(), shown);
            for (file, bytes) in &images {
                result = crate::client::attach(result, file, bytes)
                    .expect("the reference was taken from these bytes");
            }
            results.push(result);
        }
        let mut state = shared.lock();
        line.recovered_from = state.recovered.remove(&line.id);
        state.push(Event::ToolCalled(Box::new(line)));
        drop(state);
        shared.changed.notify_all();
    }
    let mut state = shared.lock();
    let settled = if stopped || cancel.is_asked() {
        Some(SettleReason::Cancelled)
    } else if unknown {
        Some(SettleReason::Failed)
    } else if last {
        Some(SettleReason::MaxSteps)
    } else {
        None
    };
    if let Some(reason) = settled {
        state.flight = None;
        let measured = state.step_tokens.take();
        if reason == SettleReason::MaxSteps {
            let ran = std::mem::take(exchange);
            state.trunk.extend(ran);
            state.trunk_tokens = measured;
            state.ran.clear();
        } else if reason == SettleReason::Failed {
            state.step_tokens = measured;
            state.keep_ran_steps();
        } else if reason == SettleReason::Cancelled {
            // Every call of this step answered, the cancelled one included:
            // the turn so far joins the trunk, as the other harnesses keep
            // it (#575).
            let mut ran = std::mem::take(exchange);
            ran.push(said);
            ran.extend(results);
            state.trunk.extend(ran);
            state.trunk_tokens = measured;
            state.ran.clear();
        }
        state.push(Event::TurnSettled { turn, reason });
        turn_over(&shared.template, &mut state);
        drop(state);
        shared.changed.notify_all();
        return None;
    }
    exchange.push(said.clone());
    exchange.extend(results.iter().cloned());
    state.ran.clone_from(exchange);
    shape.messages.push(said);
    shape.messages.extend(results);
    // The automatic seam (#617), between the turn's steps too.
    window_seam(&shared.template, &mut state, shape, false);
    let max_tokens = state.sized(shape, shared.template.limits.max_output_tokens);
    let next = state.push(Event::Requested {
        turn,
        lane: Lane::Trunk,
        head_sha256: crate::client::head::Head::of(shape).digest().to_owned(),
        fork: None,
        max_tokens,
    });
    if let Some(flight) = state.flight.as_mut() {
        flight.request = next;
    }
    drop(state);
    shared.changed.notify_all();
    Some(next)
}

/// The log's word for a confinement's mechanism.
fn isolation_word(isolation: crate::isolation::Isolation) -> log::Isolation {
    match isolation {
        crate::isolation::Isolation::None => log::Isolation::None,
        crate::isolation::Isolation::Sandbox => log::Isolation::Sandbox,
        crate::isolation::Isolation::Vm => log::Isolation::Vm,
    }
}

/// The log's word for a network.
fn network_word(network: crate::isolation::Network) -> log::Network {
    match network {
        crate::isolation::Network::None => log::Network::None,
        crate::isolation::Network::Host => log::Network::Host,
    }
}

/// A call left unanswered by a stop: `cancelled`, with its confinement and,
/// where its command parsed, its argv.
fn cancelled_line<S: Streaming>(
    shared: &Shared<S>,
    (turn, request): (u32, u64),
    call: &Call,
) -> ToolLine {
    let mut line = ToolLine::of(request, turn, call, log::ToolOutcome::Cancelled);
    if let Some(tools) = shared.tools.as_ref().filter(|_| call.name == BASH) {
        line.isolation = Some(isolation_word(tools.confinement.isolation()));
        line.network = Some(network_word(tools.policy.network));
        if let Some(command) = tool_loop::command_of(&call.arguments) {
            line.argv = Some(tool_loop::argv_of(&command));
            line.cwd = Some(tools.cwd.clone());
        }
    }
    line
}

/// Wait for the operator's answer to `prompt`, with no timeout: the decision
/// and when it was made, or `None` when a stop reached the turn first.
fn decided<S: Streaming>(
    shared: &Shared<S>,
    cancel: &Cancel,
    prompt: Prompt,
) -> Option<(Decision, u64)> {
    let mut state = shared.lock();
    state.waiting = Some(Waiting {
        prompt,
        answer: None,
    });
    shared.changed.notify_all();
    loop {
        if cancel.is_asked() {
            state.waiting = None;
            return None;
        }
        if let Some(answer) = state.waiting.as_mut().and_then(|w| w.answer.take()) {
            state.waiting = None;
            return Some(answer);
        }
        // Woken by an answer, a stop, or the session ending; the stop is
        // asked after its notify, so the flag is read again on a short beat.
        state = shared
            .changed
            .wait_timeout(state, Duration::from_millis(50))
            .unwrap_or_else(PoisonError::into_inner)
            .0;
    }
}

/// The approval a call that runs records: the decision made for it, or the
/// standing entry that covered it -- an operator's (`session`,
/// `workspace`) over a pre-seed whenever one covered any segment, since
/// that is the approval the call needed; the first segment's otherwise.
fn approval_of(judged: &Judged, allowed: &[Entry]) -> Option<log::Approval> {
    let covering: Vec<&Entry> = judged
        .covered_by
        .iter()
        .flatten()
        .filter_map(|at| allowed.get(*at))
        .collect();
    let entry = covering
        .iter()
        .find(|entry| entry.scope != Scope::Preseeded)
        .or_else(|| covering.first())?;
    Some(log::Approval {
        scope: tool_loop::scope_tag(entry.scope),
        decided_at: entry.decided_at,
        why: entry.why.clone(),
    })
}

/// The log's words for a cap (#554).
fn tool_output_of(cap: super::output::OutputCap) -> log::ToolOutput {
    use super::output::OutputCap;
    let count = |n: usize| u64::try_from(n).unwrap_or(u64::MAX);
    match cap {
        OutputCap::Capped {
            max_lines,
            max_bytes,
        } => log::ToolOutput {
            state: log::ToolOutputState::Capped,
            max_lines: Some(count(max_lines)),
            max_bytes: Some(count(max_bytes)),
        },
        OutputCap::Keep => log::ToolOutput {
            state: log::ToolOutputState::Keep,
            max_lines: None,
            max_bytes: None,
        },
    }
}

/// What the model is shown of `whole`, a call's output, under the session's
/// cap (#554): within it, `whole`; over it, Qwen Code's notice and the head
/// and tail, the whole kept in the recording by digest and named on `line`.
fn shown_capped(tools: &Tools, whole: &str, line: &mut ToolLine) -> String {
    use super::output::{self, Kept};
    if !output::over(whole, tools.output_cap) {
        return whole.to_owned();
    }
    let saved = tools.recording.as_ref().and_then(|dir| {
        super::attach::kept_whole(dir, whole.as_bytes(), "text/plain")
            .ok()
            .map(|file| (dir.join(&file.path).to_string_lossy().into_owned(), file))
    });
    let kept = saved.as_ref().map_or(Kept::Nowhere, |(path, _)| Kept::At {
        path,
        read_tool: tools.read_tool.as_deref(),
    });
    let shown = output::capped(whole, tools.output_cap, kept);
    if shown != whole
        && let Some((_, file)) = saved
    {
        line.files.push(file);
    }
    shown
}

/// A standard tool's call (#557), to its line and what the model is shown:
/// its result under the session's cap, or -- a stop having killed its
/// helper -- the cancel's result; the step limit refuses it as it refuses
/// `bash`.
/// One self-capture call (#609): its arguments through the contract and,
/// for `update_record`, the groundedness gate against what the model saw
/// (`capture::tools::apply_call`); its patches applied to working memory,
/// stamped with an open tangent; a `capture` line logged; and the result the
/// model is shown -- the outcome, and the entry id it can cite later.
fn capture_call<S>(
    shared: &Shared<S>,
    (turn, request): (u32, u64),
    call: &Call,
) -> (ToolLine, Option<String>) {
    use crate::capture::tools::CaptureTool;
    let line = ToolLine::of(request, turn, call, log::ToolOutcome::Ran);
    let mut state = shared.lock();
    // Each patch it applied, as the `patch` line a fork's would be: one
    // place for every working-memory change.
    let mut lines = Vec::new();
    let (outcome, entries, why) = match capture_patches(&mut state, (turn, request), call) {
        Err(why) => ("refused", Vec::new(), Some(why)),
        Ok(patches) => {
            let refused = state.interview.as_mut().map_or_else(
                || Some("the session keeps no working memory".to_owned()),
                |interview| {
                    let refused = patches
                        .iter()
                        .find_map(|patch| interview.object.apply(patch).err())
                        .map(|why| why.to_string());
                    if refused.is_none() {
                        lines = patches
                            .iter()
                            .map(|patch| patched(None, patch, &interview.object))
                            .collect();
                    }
                    refused
                },
            );
            capture_outcome(&call.name, refused, entries_of(&patches))
        }
    };
    if call.name == CaptureTool::UpdateRecord.tag() && outcome == "recorded" {
        state.recorded_this_turn = true;
    }
    let shown = match (&why, entries.first()) {
        (Some(why), _) => format!("{outcome}: {why}"),
        (None, Some(entry)) => format!("{outcome}: {entry}"),
        (None, None) => outcome.to_owned(),
    };
    state.push(Event::Captured {
        request,
        call: call.id.clone(),
        tool: call.name.clone(),
        outcome: outcome.to_owned(),
        entries,
        why,
        fork: None,
    });
    for patch in lines {
        state.push(patch);
    }
    drop(state);
    shared.changed.notify_all();
    (line, Some(shown))
}

/// A self-capture call's patches (#609): its arguments through the contract
/// and, for `update_record`, the groundedness gate against what the model
/// saw by turn `turn` (`capture::tools::apply_call`), each stamped with an
/// open tangent; or why the contract refused it. Nothing is applied.
fn capture_patches(
    state: &mut State,
    (turn, request): (u32, u64),
    call: &Call,
) -> Result<Vec<Patch>, String> {
    let index = state.captures_this_turn;
    state.captures_this_turn += 1;
    let seen = seen_by(&state.log, turn);
    // A served call's id is the model's, and the model may reuse one across
    // turns; the request's sequence number makes the entry's id the record's.
    let id = format!("r{request}/{}", call.id);
    let effect = crate::formats::record::json::line(&call.arguments)
        .map_err(|why| format!("the arguments are not a JSON object: {why}"))
        .and_then(|args| {
            crate::capture::tools::apply_call(
                &id,
                turn,
                &call.name,
                &args,
                index,
                seen.contract_input(),
            )
            .map_err(|why| why.to_string())
        })?;
    let tangent = state
        .tangent
        .as_ref()
        .map(|(tangent, _)| tangent.id().to_owned());
    let mut patches = effect.patches;
    for patch in &mut patches {
        provenance_of(patch).tangent.clone_from(&tangent);
    }
    Ok(patches)
}

/// A patch's provenance, to stamp.
fn provenance_of(patch: &mut Patch) -> &mut crate::object::Provenance {
    match patch {
        Patch::Add { provenance, .. }
        | Patch::Supersede { provenance, .. }
        | Patch::Resolve { provenance, .. }
        | Patch::Retire { provenance, .. }
        | Patch::Park { provenance, .. } => provenance,
    }
}

/// The entries `patches` write or rule on, by id.
fn entries_of(patches: &[Patch]) -> Vec<String> {
    patches
        .iter()
        .map(|patch| match patch {
            Patch::Add { id, .. } | Patch::Supersede { id, .. } => id.as_str().to_owned(),
            Patch::Resolve { target, .. }
            | Patch::Retire { target, .. }
            | Patch::Park { target, .. } => target.as_str().to_owned(),
        })
        .collect()
}

/// What a capture call to `tool` came to, in the log's words, given why its
/// patches were refused, if they were, and the entries they wrote.
fn capture_outcome(
    tool: &str,
    refused: Option<String>,
    entries: Vec<String>,
) -> (&'static str, Vec<String>, Option<String>) {
    use crate::capture::tools::CaptureTool;
    match (refused, CaptureTool::from_tag(tool)) {
        (Some(why), _) => ("refused", Vec::new(), Some(why)),
        (None, Some(CaptureTool::UpdateRecord)) if entries.is_empty() => (
            "dropped",
            Vec::new(),
            Some("the groundedness gate kept nothing of it".to_owned()),
        ),
        (None, Some(CaptureTool::UpdateRecord)) => ("recorded", entries, None),
        (None, Some(CaptureTool::ResolveEntry)) if entries.is_empty() => {
            ("judged", Vec::new(), None)
        }
        (None, Some(CaptureTool::ResolveEntry)) => ("resolved", entries, None),
        (None, _) => ("proposed", Vec::new(), None),
    }
}

/// What the model saw by turn `turn`, as the groundedness gate reads it
/// (#609): that turn's trunk prose and non-capture tool output as the
/// source, every earlier turn's as the session prefix -- from the log, as
/// `capture::tools::Seen::of_turn` reads a record.
fn seen_by(log: &[Logged], turn: u32) -> crate::capture::tools::Seen {
    use crate::capture::tools::CaptureTool;
    let mut trunk_turn = BTreeMap::new();
    let (mut source, mut prefix) = (String::new(), String::new());
    let mut file = |at: u32, text: &str| {
        let bucket = match at.cmp(&turn) {
            std::cmp::Ordering::Equal => &mut source,
            std::cmp::Ordering::Less => &mut prefix,
            std::cmp::Ordering::Greater => return,
        };
        bucket.push_str(text);
        bucket.push('\n');
    };
    for logged in log {
        match &logged.event {
            Event::Requested {
                turn: at,
                lane: Lane::Trunk,
                ..
            } => {
                trunk_turn.insert(logged.seq, *at);
            }
            Event::Answered { request, text, .. } | Event::Called { request, text, .. } => {
                if let Some(at) = trunk_turn.get(request) {
                    file(*at, text);
                }
            }
            Event::ToolCalled(line) if CaptureTool::from_tag(&line.name).is_none() => {
                if let Some(shown) = &line.shown {
                    file(line.turn, shown);
                }
            }
            _ => {}
        }
    }
    crate::capture::tools::Seen::from_parts(source, prefix)
}

fn standard_call(
    tools: &Tools,
    cancel: &Cancel,
    (turn, request): (u32, u64),
    call: &Call,
    last: bool,
) -> (ToolLine, Option<String>) {
    if last {
        let mut line = ToolLine::of(request, turn, call, log::ToolOutcome::Refused);
        line.reason = Some(log::ToolRefusal::MaxSteps);
        return (line, None);
    }
    match super::standard::run(&call.name, &call.arguments, tools, &|| cancel.is_asked()) {
        super::standard::Done::Shown(result) => {
            let mut line = ToolLine::of(request, turn, call, log::ToolOutcome::Ran);
            let shown = shown_capped(tools, &result, &mut line);
            (line, Some(shown))
        }
        super::standard::Done::Image { media_type, bytes } => {
            let mut line = ToolLine::of(request, turn, call, log::ToolOutcome::Ran);
            // Kept by digest, as an attachment is, so the projection can
            // rebuild the result; unrecorded, it is still sent.
            let file = tools.recording.as_ref().map_or_else(
                || super::attach::referenced(&bytes, media_type),
                |dir| {
                    super::attach::kept_whole(dir, &bytes, media_type).map_or_else(
                        |_| super::attach::referenced(&bytes, media_type),
                        |file| {
                            line.files.push(file.clone());
                            file
                        },
                    )
                },
            );
            line.images.push((file, bytes));
            // Qwen Code's form, since Pi's and `OpenCode` 2's words differ:
            // the image alone, no text beside it.
            (line, Some(String::new()))
        }
        super::standard::Done::Cancelled(_) => {
            let line = ToolLine::of(request, turn, call, log::ToolOutcome::Cancelled);
            (line, Some(CANCELLED_CALL.to_owned()))
        }
    }
}

/// `prune_output` (#612): the most recent earlier call whose string
/// argument is the one named, pruned -- its whole saved by digest, its
/// reference line kept for the seam that replaces it -- and the answer
/// saying when. A name no call matches, a result already pruned, or one a
/// seam already compacted away is answered with what is wrong, as a
/// standard tool answers an error.
fn prune_call<S>(
    shared: &Shared<S>,
    (turn, request): (u32, u64),
    call: &Call,
) -> (ToolLine, Option<String>) {
    let line = ToolLine::of(request, turn, call, log::ToolOutcome::Ran);
    let Some(named) = super::prune::target_of(&call.arguments) else {
        return (line, Some(super::prune::unmatched("")));
    };
    let mut state = shared.lock();
    let target = state
        .log
        .iter()
        .rev()
        .find_map(|logged| match &logged.event {
            Event::ToolCalled(target)
                if target.name != super::prune::PRUNE_OUTPUT
                    && target.outcome == log::ToolOutcome::Ran
                    && super::prune::names(&target.arguments, &named) =>
            {
                Some((target.as_ref().clone(), target.shown.clone()?))
            }
            _ => None,
        });
    let Some((target, shown)) = target else {
        return (line, Some(super::prune::unmatched(&named)));
    };
    if state
        .pruned
        .iter()
        .any(|prune| prune.call == target.id && prune.shown == shown)
    {
        return (line, Some(super::prune::ALREADY_PRUNED.to_owned()));
    }
    // On the trunk: a step of this turn's, or a result a seam has not
    // compacted away.
    let on_trunk = target.request == request
        || state.trunk.iter().chain(&state.ran).any(|message| {
            message.role == Role::Tool
                && message.tool_call_id.as_deref() == Some(target.id.as_str())
                && message.content == shown
        });
    if !on_trunk {
        return (line, Some(super::prune::ALREADY_COMPACTED.to_owned()));
    }
    let saved = saved_whole(
        &shown,
        target.files.first().cloned(),
        state.recording.as_deref(),
    );
    let text = crate::seam::outputs::reference_line(&crate::seam::outputs::Output {
        turn: target.turn,
        name: target.name.clone(),
        arguments: target.arguments.clone(),
        shown: shown.clone(),
        images: Vec::new(),
        saved: Some(saved.clone()),
        excerpts: Vec::new(),
        pruned: None,
    });
    state.push(Event::Pruned {
        turn,
        call: target.id.clone(),
        sha256: saved.sha256,
        bytes: shown.len() as u64,
        text: text.clone(),
    });
    state.pruned.push(Prune {
        call: target.id,
        shown,
        text,
        applied: false,
    });
    let seam = state
        .interview
        .as_ref()
        .and_then(|interview| interview.prune)
        .unwrap_or_default();
    drop(state);
    shared.changed.notify_all();
    (line, Some(seam.answer().to_owned()))
}

/// One call, to its line and what the model is shown of it (`None` when
/// nothing goes back: an undeclared tool, the step limit, a stop).
#[allow(clippy::too_many_lines)]
fn one_call<S: Streaming>(
    shared: &Shared<S>,
    cancel: &Cancel,
    (turn, request): (u32, u64),
    call: &Call,
    last: bool,
) -> (ToolLine, Option<String>) {
    let refused = |reason: log::ToolRefusal| {
        let mut line = ToolLine::of(request, turn, call, log::ToolOutcome::Refused);
        line.reason = Some(reason);
        line
    };
    let declared = shared
        .template
        .tools
        .iter()
        .any(|tool| tool.name == call.name);
    // `prune_output` (#612): answered from the session's own log, run
    // under no confinement, since it runs nothing.
    if declared && call.name == super::prune::PRUNE_OUTPUT {
        if last {
            return (refused(log::ToolRefusal::MaxSteps), None);
        }
        return prune_call(shared, (turn, request), call);
    }
    // Self-capture (#609): the contract's tools write working memory and
    // nothing else, so no gate, no confinement and no approval decide them.
    if declared && crate::capture::tools::CaptureTool::from_tag(&call.name).is_some() {
        return capture_call(shared, (turn, request), call);
    }
    // The standard surface's tools (#557), each run under the session's
    // confinement; no gate decides them.
    if declared
        && super::standard::is_standard(&call.name)
        && let Some(tools) = shared.tools.as_ref()
    {
        return standard_call(tools, cancel, (turn, request), call, last);
    }
    // `task_stop` (#614): no gate decides it; it stops a job the session
    // started.
    if declared
        && call.name == super::background::TASK_STOP
        && shared.tools.as_ref().is_some_and(|tools| tools.background)
    {
        if last {
            let mut line = refused(log::ToolRefusal::MaxSteps);
            line.reason = Some(log::ToolRefusal::MaxSteps);
            return (line, None);
        }
        return task_stop_call(shared, (turn, request), call);
    }
    let Some(tools) = shared
        .tools
        .as_ref()
        .filter(|_| declared && call.name == BASH)
    else {
        return (refused(log::ToolRefusal::UnknownTool), None);
    };
    let Some(command) = tool_loop::command_of(&call.arguments) else {
        return (
            refused(log::ToolRefusal::Unparsable),
            Some(tool_loop::refusal_text(log::ToolRefusal::Unparsable, "")),
        );
    };
    // In the background (#614): one bare trailing `&` comes off, as Qwen
    // Code takes it off, before the gate judges the command.
    // Its timeout (#613): its own, or the session's default; refused in
    // Qwen Code's words when it is not one.
    let timeout = match tool_loop::timeout_of(&call.arguments) {
        Ok(own) => own.or(tools.timeout_ms),
        Err(why) => return (refused(log::ToolRefusal::Unparsable), Some(why.to_owned())),
    };
    let background = tools.background && tool_loop::background_of(&call.arguments);
    let command = if background {
        super::background::without_trailing_amp(&command)
    } else {
        command
    };
    let parsed = |mut line: ToolLine| {
        line.argv = Some(tool_loop::argv_of(&command));
        line.cwd = Some(tools.cwd.clone());
        line
    };
    if last {
        return (parsed(refused(log::ToolRefusal::MaxSteps)), None);
    }
    // Approvals off (the approval lever's `none`): no gate decision and no
    // prompt; the command runs as the model sent it, confined as ever.
    let (run, approval) = 'gated: {
        if tools.approvals_off {
            break 'gated (
                tool_loop::argv_of(&command),
                Some(log::Approval {
                    scope: log::ApprovalScope::Off,
                    decided_at: None,
                    why: None,
                }),
            );
        }
        let mut allowed = shared.lock().allowed.clone();
        let mut judged = tools.gate.judge(&command, &allowed);
        let mut approval = None;
        match judged.outcome() {
            GateOutcome::Refused => {
                let entry = judged.judgement.refused_by().unwrap_or_default().to_owned();
                return (
                    parsed(refused(log::ToolRefusal::Denylist)),
                    Some(tool_loop::refusal_text(log::ToolRefusal::Denylist, &entry)),
                );
            }
            GateOutcome::Prompt => {
                let why = judged.why().unwrap_or("not_approved").to_owned();
                let answer = match tools.decider {
                    Decider::Decline => Some((Decision::Decline, 0)),
                    Decider::Operator => decided(
                        shared,
                        cancel,
                        tool_loop::prompt_of(&judged, request, turn, &call.id, &tools.cwd),
                    ),
                };
                let Some((decision, at)) = answer else {
                    return (cancelled_line(shared, (turn, request), call), None);
                };
                let mut state = shared.lock();
                let scope = match decision {
                    Decision::Decline => {
                        state.counts.declined += 1;
                        return (
                            parsed(refused(log::ToolRefusal::Declined)),
                            Some(tool_loop::refusal_text(log::ToolRefusal::Declined, "")),
                        );
                    }
                    Decision::Once => {
                        state.counts.once += 1;
                        judged.approve_once();
                        Scope::Once
                    }
                    Decision::Session | Decision::Workspace => {
                        let mut scope = if decision == Decision::Workspace {
                            Scope::Workspace
                        } else {
                            Scope::Session
                        };
                        let mut granted = judged.grants(scope, at);
                        if scope == Scope::Workspace
                            && let Some(store) = &tools.store
                        {
                            let wall = SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(0));
                            for entry in &mut granted {
                                entry.approved_unix_ms = Some(wall);
                            }
                            let mut kept: Vec<Entry> = state
                                .allowed
                                .iter()
                                .filter(|e| e.scope == Scope::Workspace)
                                .cloned()
                                .collect();
                            kept.extend(granted.iter().cloned());
                            // An approval the store cannot keep is not a
                            // workspace one, and is not recorded as one.
                            if store.write(&kept, wall).is_err() {
                                scope = Scope::Session;
                                for entry in &mut granted {
                                    entry.scope = Scope::Session;
                                    entry.approved_unix_ms = None;
                                }
                            }
                        }
                        if scope == Scope::Workspace {
                            state.counts.workspace += 1;
                        } else {
                            state.counts.session += 1;
                        }
                        state.allowed.extend(granted);
                        allowed.clone_from(&state.allowed);
                        judged = tools.gate.judge(&command, &allowed);
                        scope
                    }
                };
                drop(state);
                if judged.outcome() != GateOutcome::Run {
                    // The operator approved this call: whatever no entry covers,
                    // the decision does.
                    judged.approve_once();
                }
                approval = Some(log::Approval {
                    scope: tool_loop::scope_tag(scope),
                    decided_at: Some(at),
                    why: Some(why),
                });
            }
            GateOutcome::Run => {}
        }
        let approval = approval.or_else(|| approval_of(&judged, &allowed));
        (judged.run, approval)
    };
    let mut line = parsed(ToolLine::of(request, turn, call, log::ToolOutcome::Ran));
    line.approval = approval;
    let profiled = tools.confinement.isolation() != crate::isolation::Isolation::None;
    if background {
        return background_call(shared, tools, line, (&run, &command));
    }
    // A running call the operator can move to the background (#614), when
    // background commands are on.
    let slot = tools.background.then(|| {
        let slot = Arc::new(Mutex::new(None));
        shared.lock().promotable = Some(Arc::clone(&slot));
        slot
    });
    // Near its timeout, a line for the surface's warning (#613).
    let warn = || {
        if let Some(ms) = timeout {
            shared.lock().push(Event::TimeoutNear {
                request,
                call: call.id.clone(),
                timeout_ms: ms,
            });
            shared.changed.notify_all();
        }
    };
    // A stop reaches a running call: its whole process group is killed, and
    // the turn settles `cancelled` (#551). Its timeout ends it alone (#613).
    let finished = tools.confinement.run_promotable(
        &tools.policy,
        &tools.worktree,
        &run,
        &|| cancel.is_asked(),
        (
            &|| {
                slot.as_ref().and_then(|slot| {
                    slot.lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .as_ref()
                        .and_then(|job: &super::background::Job| {
                            std::fs::File::create(&job.output).ok()
                        })
                })
            },
            timeout.map(|ms| crate::isolation::Limit {
                after: Duration::from_millis(ms),
                warn: &warn,
            }),
        ),
    );
    if slot.is_some() {
        shared.lock().promotable = None;
    }
    let finished = match finished {
        Ok(crate::isolation::Finished::Promoted(ran, child)) => {
            let job =
                slot.and_then(|slot| slot.lock().unwrap_or_else(PoisonError::into_inner).take());
            return promoted_call(shared, tools, line, (ran, child), (job, &command));
        }
        Ok(crate::isolation::Finished::Ran(ran)) => Ok(ran),
        Err(not_run) => Err(not_run),
    };
    match finished {
        Ok(ran) if ran.timed_out && !ran.cancelled => {
            timed_out(tools, line, &ran, timeout.unwrap_or_default())
        }
        Ok(ran) if ran.cancelled => {
            // A cancelled line carries no streams (the log format's rule);
            // what the command printed before the cancel reaches the log as
            // the result the model was shown.
            line.outcome = log::ToolOutcome::Cancelled;
            line.confined = Some(ran.confined.clone());
            line.isolation = Some(isolation_word(ran.isolation));
            line.network = Some(network_word(ran.network));
            // What it printed before the cancel, then the cancel's result
            // (#575).
            let printed = ran.as_the_model_sees_it();
            let shown = if printed.is_empty() {
                CANCELLED_CALL.to_owned()
            } else {
                format!("{printed}\n{CANCELLED_CALL}")
            };
            (line, Some(shown))
        }
        Ok(ran) => {
            let denied = ran.denials().iter().any(|d| d.kind.is_unambiguous());
            if profiled && ran.exit != Some(0) && denied {
                line.outcome = log::ToolOutcome::CommandFailed;
                line.policy.clone_from(&ran.policy);
            }
            line.confined = Some(ran.confined.clone());
            line.isolation = Some(isolation_word(ran.isolation));
            line.network = Some(network_word(ran.network));
            line.exit = ran.exit.and_then(|code| u64::try_from(code).ok());
            line.stdout = Some(log::Output {
                text: ran.stdout.clone(),
                bytes: ran.stdout_bytes,
            });
            line.stderr = Some(log::Output {
                text: ran.stderr.clone(),
                bytes: ran.stderr_bytes,
            });
            let shown = shown_capped(tools, &ran.as_the_model_sees_it(), &mut line);
            (line, Some(shown))
        }
        Err(not_run) => never_ran(tools, line, &run, &not_run),
    }
}

/// Where the session's background jobs write (#614): the recording's
/// `background/`, or, with no recording, a directory of the session's own
/// under the system's temporary one.
fn background_dir<S>(shared: &Shared<S>, state: &State) -> std::path::PathBuf {
    let dir = shared
        .tools
        .as_ref()
        .and_then(|tools| tools.recording.as_ref())
        .map_or_else(
            || {
                let opened = match state.log.first() {
                    Some(Logged {
                        event: Event::Started { opened, .. },
                        ..
                    }) => *opened,
                    _ => 0,
                };
                std::env::temp_dir()
                    .join(format!("diet-background-{opened}-{}", std::process::id()))
            },
            |recording| recording.join("background"),
        );
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// A started job, kept and handed to the reaper (#614).
fn register<S>(shared: &Shared<S>, job: super::background::Job, child: std::process::Child) {
    job.write_status();
    let id = job.id.clone();
    shared.lock().jobs.insert(id.clone(), job);
    if let Some(reap) = shared
        .reap
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = reap.send((id, child));
    }
}

/// A `bash` call that asked for the background (#614): started under the
/// session's confinement, its output written to a file, and answered at
/// once with Qwen Code's start text.
fn background_call<S>(
    shared: &Shared<S>,
    tools: &Tools,
    mut line: ToolLine,
    (run, command): (&[String], &str),
) -> (ToolLine, Option<String>) {
    let dir = background_dir(shared, &shared.lock());
    let id = super::background::new_id();
    let (output, status_file) = super::background::files_of(&dir, &id);
    let spawned = std::fs::File::create(&output)
        .map_err(|why| crate::isolation::NotRun::Runner {
            said: format!(
                "the output file {} could not be made: {why}",
                output.display()
            ),
        })
        .and_then(|file| {
            tools
                .confinement
                .spawn_detached(&tools.policy, &tools.worktree, run, &file)
        });
    match spawned {
        Ok((ran, child)) => {
            let job = super::background::Job {
                id: id.clone(),
                command: command.to_owned(),
                cwd: tools.cwd.clone(),
                pid: child.id(),
                output,
                status_file,
                status: super::background::Status::Running,
                exit: None,
                stopping: false,
            };
            let shown = super::background::started(&job);
            line.confined = Some(ran.confined.clone());
            line.isolation = Some(isolation_word(ran.isolation));
            line.network = Some(network_word(ran.network));
            line.stdout = Some(log::Output {
                text: String::new(),
                bytes: 0,
            });
            line.stderr = Some(log::Output {
                text: String::new(),
                bytes: 0,
            });
            line.background = Some(id);
            register(shared, job, child);
            (line, Some(shown))
        }
        Err(not_run) => never_ran(tools, line, run, &not_run),
    }
}

/// A running call the operator moved to the background (#614): what it
/// printed until then on its line, Qwen Code's promotion text for the
/// model, and the command left running as a job.
fn promoted_call<S>(
    shared: &Shared<S>,
    tools: &Tools,
    mut line: ToolLine,
    (ran, child): (crate::isolation::Ran, std::process::Child),
    (job, command): (Option<super::background::Job>, &str),
) -> (ToolLine, Option<String>) {
    let Some(mut job) = job else {
        unreachable!("a call is promoted only once its job is set");
    };
    command.clone_into(&mut job.command);
    job.cwd.clone_from(&tools.cwd);
    job.pid = child.id();
    let shown = super::background::promoted(&job);
    line.confined = Some(ran.confined.clone());
    line.isolation = Some(isolation_word(ran.isolation));
    line.network = Some(network_word(ran.network));
    line.stdout = Some(log::Output {
        text: ran.stdout.clone(),
        bytes: ran.stdout_bytes,
    });
    line.stderr = Some(log::Output {
        text: ran.stderr.clone(),
        bytes: ran.stderr_bytes,
    });
    line.background = Some(job.id.clone());
    register(shared, job, child);
    (line, Some(shown))
}

/// The default timeout `session.start` names (#613), 0 for none: where the
/// declared `bash` takes `timeout`, what the projection rebuilds its
/// definition from.
fn bash_timeout_of(tools: Option<&Tools>, template: &RequestShape) -> Option<u64> {
    tools
        .filter(|_| declares_timeout(template))
        .map(|tools| tools.timeout_ms.unwrap_or(0))
}

/// Whether `template`'s `bash` takes `timeout` (#613).
fn declares_timeout(template: &RequestShape) -> bool {
    template.tools.iter().any(|tool| {
        tool.name == BASH
            && matches!(&tool.schema, Value::Object(schema)
                if matches!(schema.get("properties"), Some(Value::Object(properties))
                    if properties.contains_key("timeout")))
    })
}

/// A call its timeout ended (#613): what it printed on its line, and Qwen
/// Code's answer (`tools/shell.ts:2973-2989`) -- the timeout, then what it
/// printed before it, or that it printed nothing -- under the cap.
fn timed_out(
    tools: &Tools,
    mut line: ToolLine,
    ran: &crate::isolation::Ran,
    ms: u64,
) -> (ToolLine, Option<String>) {
    line.confined = Some(ran.confined.clone());
    line.isolation = Some(isolation_word(ran.isolation));
    line.network = Some(network_word(ran.network));
    line.exit = ran.exit.and_then(|code| u64::try_from(code).ok());
    line.stdout = Some(log::Output {
        text: ran.stdout.clone(),
        bytes: ran.stdout_bytes,
    });
    line.stderr = Some(log::Output {
        text: ran.stderr.clone(),
        bytes: ran.stderr_bytes,
    });
    line.timeout_ms = Some(ms);
    let printed = ran.as_the_model_sees_it();
    let said = if printed.is_empty() {
        format!(
            "Command timed out after {ms}ms before it could complete. There was no output before \
             it timed out."
        )
    } else {
        format!(
            "Command timed out after {ms}ms before it could complete. Below is the output before \
             it timed out:\n{printed}"
        )
    };
    let shown = shown_capped(tools, &said, &mut line);
    (line, Some(shown))
}

/// A command that never ran: what would have run, and why it did not, as
/// the command's own failure under its confinement.
fn never_ran(
    tools: &Tools,
    mut line: ToolLine,
    run: &[String],
    not_run: &crate::isolation::NotRun,
) -> (ToolLine, Option<String>) {
    let said = not_run.to_string();
    let confined = tools
        .confinement
        .compose(&tools.policy, &tools.worktree, run);
    line.outcome = log::ToolOutcome::CommandFailed;
    line.policy = tools.confinement.policy_of(&confined);
    line.confined = Some(confined);
    line.isolation = Some(isolation_word(tools.confinement.isolation()));
    line.network = Some(network_word(tools.policy.network));
    line.stdout = Some(log::Output {
        text: String::new(),
        bytes: 0,
    });
    line.stderr = Some(log::Output {
        text: said.clone(),
        bytes: said.len() as u64,
    });
    (line, Some(said))
}

/// `task_stop` (#614): a running job's whole process group signalled, and
/// Qwen Code's answers -- its cancellation requested, an id that names no
/// job, or one no longer running.
fn task_stop_call<S>(
    shared: &Shared<S>,
    (turn, request): (u32, u64),
    call: &Call,
) -> (ToolLine, Option<String>) {
    use super::background::{self, Status};
    let id = serde_json::from_str::<serde_json::Value>(&call.arguments)
        .ok()
        .and_then(|value| value.get("task_id")?.as_str().map(str::to_owned))
        .unwrap_or_default();
    let mut state = shared.lock();
    let shown = match state.jobs.get_mut(&id) {
        None => background::not_found(&id),
        Some(job) if job.status != Status::Running => background::not_running(job),
        Some(job) => {
            job.stopping = true;
            background::stop_group(job.pid);
            background::stopping(job)
        }
    };
    drop(state);
    (
        ToolLine::of(request, turn, call, log::ToolOutcome::Ran),
        Some(shown),
    )
}

/// Background commands (#614), when they are on: one thread waits on every
/// job, holding the session only weakly, so a dropped session ends it.
fn start_reaper<S: Streaming + 'static>(shared: &Arc<Shared<S>>) {
    if !shared.tools.as_ref().is_some_and(|tools| tools.background) {
        return;
    }
    let (send, receive) = std::sync::mpsc::channel();
    *shared.reap.lock().unwrap_or_else(PoisonError::into_inner) = Some(send);
    let weak = Arc::downgrade(shared);
    let _ = std::thread::Builder::new()
        .name("diet-reaper".to_owned())
        .spawn(move || reaper(&weak, &receive));
}

/// The session's reaper (#614): it waits on every background job's child,
/// and logs each as it ends; it stops once the session is gone.
fn reaper<S: Streaming + 'static>(
    shared: &std::sync::Weak<Shared<S>>,
    receive: &std::sync::mpsc::Receiver<(String, std::process::Child)>,
) {
    use std::sync::mpsc::RecvTimeoutError;
    let mut running: Vec<(String, std::process::Child)> = Vec::new();
    loop {
        match receive.recv_timeout(Duration::from_millis(50)) {
            Ok(job) => running.push(job),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => {
                if running.is_empty() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        running.extend(receive.try_iter());
        let mut at = 0;
        while at < running.len() {
            match running[at].1.try_wait() {
                Ok(None) => at += 1,
                done => {
                    let (id, _) = running.swap_remove(at);
                    let Some(shared) = shared.upgrade() else {
                        return;
                    };
                    ended(
                        &shared,
                        &id,
                        done.ok().flatten().and_then(|status| status.code()),
                    );
                }
            }
        }
    }
}

/// Job `id` ended with `code` (#614): its status file says so, its output
/// is kept whole by digest, its end is logged, and its notification waits
/// for the next ask.
fn ended<S>(shared: &Shared<S>, id: &str, code: Option<i32>) {
    use super::background::Status;
    let recording = shared
        .tools
        .as_ref()
        .and_then(|tools| tools.recording.clone());
    let mut state = shared.lock();
    let Some(job) = state.jobs.get_mut(id) else {
        return;
    };
    job.exit = code;
    job.status = if job.stopping {
        Status::Cancelled
    } else if code == Some(0) {
        Status::Completed
    } else {
        Status::Failed
    };
    job.write_status();
    let output = std::fs::read(&job.output).unwrap_or_default();
    let note = super::background::notification(job, &output);
    let status = match job.status {
        Status::Completed => log::BackgroundStatus::Completed,
        Status::Failed => log::BackgroundStatus::Failed,
        Status::Running | Status::Cancelled => log::BackgroundStatus::Cancelled,
    };
    let files: Vec<log::RecordedFile> = recording
        .and_then(|dir| super::attach::kept_whole(&dir, &output, "text/plain").ok())
        .into_iter()
        .collect();
    state.notices.push(note);
    if state.settlement != Settlement::Ended {
        state.push(Event::BackgroundEnded {
            job: id.to_owned(),
            status,
            exit: code.and_then(|code| u64::try_from(code).ok()),
            files,
        });
    }
    drop(state);
    shared.changed.notify_all();
}

/// Refill the trunk from working memory for a seam `reason` fired (#493):
/// the head with the working object rendered after it, and no turn of the
/// old trunk. The caller has checked the session is awaiting and working
/// memory holds an entry. A cadence then counts from here, and a budget
/// waits for the next trunk call to measure the refilled trunk.
fn refill_trunk(
    template: &RequestShape,
    state: &mut State,
    seam: (crate::seam::Reason, Option<log::SeamFired>),
    to: Option<String>,
) {
    let seamed = refilled(template, state, seam, to);
    state.push(seamed);
}

/// [`refill_trunk`]'s refill, its line returned rather than logged, so a
/// pre-warm's timings can join it (#504).
fn refilled(
    template: &RequestShape,
    state: &mut State,
    (reason, fired): (crate::seam::Reason, Option<log::SeamFired>),
    to: Option<String>,
) -> Event {
    // The move the graph allowed, made before the render, so the refill
    // names the phase the session is now in (#563).
    let phase = to.map(|to| log::PhaseMove {
        from: state.phase.clone().unwrap_or_default(),
        to,
    });
    if let Some(moved) = &phase {
        state.phase = Some(moved.to.clone());
    }
    let Some(interview) = state.interview.as_ref() else {
        unreachable!("a seam is refused or not due when the session keeps no working memory");
    };
    // The render, under the regimen's budget when it declares one (#565).
    let budget = interview.seams.render_budget;
    let rendered = crate::seam::render::rendered(&interview.object, state.phase.as_deref(), budget);
    let render_budget = budget.map(|budget| log::RenderBudget {
        tokens: budget.tokens,
        over: match budget.over {
            crate::seam::render::OverBudget::Tier => "tier",
            crate::seam::render::OverBudget::Elide => "elide",
        }
        .to_owned(),
        rendered: rendered.tokens,
        reduced: rendered.reduced,
    });
    let render = rendered.text;
    let carried_entries = interview.object.live().count() as u64;
    // The head a trunk request on `messages` carries: `Head::of` leaves
    // out a request's last message, its ask, so one stands in for it.
    let digest = |messages: &[Message]| {
        let mut shape = template.clone();
        shape.messages = messages.to_vec();
        shape.messages.push(Message::new(Role::User, String::new()));
        crate::client::head::Head::of(&shape).digest().to_owned()
    };
    let prefix_hash_before = digest(&state.trunk);
    // The compaction depth (#552): the most recent whole turns, within the
    // regimen's budget, kept after the refill as they sat on the trunk.
    let tail_tokens = interview.seams.tail_tokens;
    let turns = state
        .trunk
        .get(template.messages.len() + usize::from(state.refilled)..)
        .unwrap_or_default();
    let kept = crate::seam::render::tail(turns, tail_tokens).to_vec();
    let carried_turns = kept
        .iter()
        .filter(|message| message.role == Role::User)
        .count() as u64;
    let carried_tokens = kept
        .iter()
        .map(crate::seam::render::estimated_tokens)
        .sum::<u64>();
    // What the refill carries of the outputs it compacts away (#553): those
    // before the kept tail, which stays as it sat.
    let tool_outputs = interview.seams.outputs;
    let compacted = &turns[..turns.len() - kept.len()];
    let mut kept = kept;
    replaced_in_tail(&mut kept, &state.pruned);
    // The archive (#566): what this seam drops from the trunk, and every
    // working-memory entry no longer live, for a recall to find later.
    let archived = archived_at_seam(compacted, &interview.object, state.turns);
    let outputs = crate::seam::outputs::section(
        tool_outputs,
        &compacted_outputs(
            &state.log,
            compacted,
            tool_outputs,
            (state.recording.as_deref(), &state.pruned),
        ),
    );
    let sent = outputs
        .as_ref()
        .map_or_else(|| render.clone(), |(text, _)| format!("{render}{text}"));
    let mut refilled = crate::seam::render::refill(&template.messages, &sent);
    state.refilled = true;
    refilled.extend(kept);
    let prefix_hash_after = digest(&refilled);
    state.trunk = refilled;
    for item in archived {
        state.archive.push(item);
    }
    state.turns_at_seam = state.turns;
    state.trunk_tokens = None;
    let pruned = prunes_applied(state);
    let at_turn = state.turns;
    Event::Seamed {
        at_turn,
        reason,
        prefix_hash_before,
        prefix_hash_after,
        render,
        carried_entries,
        tail_tokens,
        carried_turns,
        carried_tokens,
        phase,
        tool_outputs,
        outputs,
        render_budget,
        fired,
        pruned,
        warm: None,
    }
}

/// A seam `reason` fired (#504): refilled now; or, when the regimen asks
/// for its audit (and the dogma pins one for `reason`) or its pre-warm,
/// queued for [`seam_work`], which makes those calls on a thread that can
/// wait for them, the session in `capture` until it is done. `true` when it
/// was queued.
fn seam(
    template: &RequestShape,
    state: &mut State,
    reason: crate::seam::Reason,
    to: Option<String>,
) -> bool {
    let (audit, warm) = state
        .interview
        .as_ref()
        .map_or((false, false), |interview| {
            (
                interview.seams.audit && crate::seam::pinned_ask(reason).is_some(),
                interview.seams.warm && reason != crate::seam::Reason::Window,
            )
        });
    if !audit && !warm {
        refill_trunk(template, state, (reason, None), to);
        return false;
    }
    state.seam_pending = Some(PendingSeam { reason, to });
    state.move_to(Settlement::Capture);
    true
}

/// A seam queued by [`seam`] (#504), done: its audit, when the regimen asks
/// for one and there is a settled `final` turn to cut it from -- a side
/// call off the warm trunk, the dogma's pinned ask after it, its answer read
/// by `diet::formats::audit` and folded into working memory, logged as a
/// fork on the `audit` lane; then the refill; then its pre-warm, when asked
/// for -- the refilled trunk sent once with an output cap of one, which an
/// ask cancels, its own request warming the same prefix; then the seam's
/// line, with the warm's timings when it finished; then back to `awaiting`.
/// Nothing when nothing is queued.
fn seam_work<S: Streaming>(shared: &Shared<S>) {
    let mut state = shared.lock();
    let Some(PendingSeam { reason, to }) = state.seam_pending.take() else {
        return;
    };
    if let Some(fired) = audit_fired(&shared.template, &mut state, reason) {
        drop(state);
        shared.changed.notify_all();
        audited(shared, fired);
        state = shared.lock();
    }
    let mut seamed = refilled(&shared.template, &mut state, (reason, None), to);
    let warm = state
        .interview
        .as_ref()
        .is_some_and(|interview| interview.seams.warm)
        && reason != crate::seam::Reason::Window;
    if warm {
        let mut shape = shared.template.clone();
        shape.messages.clone_from(&state.trunk);
        shape.limits.max_output_tokens = 1;
        let cancel = Cancel::new();
        state.warming = Some(cancel.clone());
        drop(state);
        shared.changed.notify_all();
        let deadline = Instant::now() + shared.template.limits.call;
        let ended = shared
            .transport
            .stream(&shape, deadline, &cancel, &mut |_: Piece<'_>| {});
        state = shared.lock();
        state.warming = None;
        if let (Ok(StreamEnded::Finished { timings, .. }), Event::Seamed { warm, .. }) =
            (ended, &mut seamed)
        {
            *warm = timings;
        }
    }
    // Back to `awaiting`, then the seam's line, under one lock: a seam is
    // logged while the session awaits, and nothing comes between.
    state.move_to(Settlement::Awaiting);
    state.push(seamed);
    if state.ending {
        state.ending = false;
        state.move_to(Settlement::Ended);
    }
    drop(state);
    shared.changed.notify_all();
}

/// A seam's audit, fired (#504): its fork line and its request, when the
/// regimen asks for one, the dogma pins an ask for `reason`, working memory
/// holds an entry, and the latest turn settled `final` -- the audit is cut
/// from that turn's warm trunk, as a fork is.
fn audit_fired(
    template: &RequestShape,
    state: &mut State,
    reason: crate::seam::Reason,
) -> Option<AuditFired> {
    let interview = state
        .interview
        .as_ref()
        .filter(|interview| interview.seams.audit)?;
    let items: Vec<(crate::object::EntryId, String)> = interview
        .object
        .live()
        .map(|entry| (entry.id.clone(), entry.content.clone()))
        .collect();
    let ask = crate::seam::audit_ask(reason, &items)?;
    let turn = state.turns;
    let finished = state
        .log
        .iter()
        .rev()
        .find_map(|logged| match &logged.event {
            Event::TurnSettled {
                turn: settled,
                reason,
            } if *settled == turn => Some(*reason),
            _ => None,
        });
    if finished != Some(SettleReason::Final) {
        return None;
    }
    let at = state
        .log
        .iter()
        .rev()
        .find_map(|logged| match &logged.event {
            Event::Requested {
                turn: asked,
                lane: Lane::Trunk,
                ..
            } if *asked == turn => Some(logged.seq),
            _ => None,
        })?;
    let mut shape = template.clone();
    shape.messages.clone_from(&state.trunk);
    shape
        .messages
        .push(Message::new(Role::User, ask.text.clone()));
    let fork = state.push(Event::Audited {
        of_turn: turn,
        at,
        template: ask.template,
        question: ask.text.clone(),
    });
    let max_tokens = state.sized(&mut shape, template.limits.max_output_tokens);
    let request = state.push(Event::Requested {
        turn,
        lane: Lane::Audit,
        head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
        fork: Some(fork),
        max_tokens,
    });
    let cancel = Cancel::new();
    state.flight = Some(Flight {
        turn,
        request,
        cancel: cancel.clone(),
    });
    Some(AuditFired {
        fork,
        request,
        shape,
        cancel,
        items: ask.items,
    })
}

/// A side call's stream (#504): its text, reasoning and progress logged
/// as they arrive, naming its request; a tool call it makes is neither run
/// nor logged, only noted.
fn aside<S: Streaming>(
    shared: &Shared<S>,
    shape: &RequestShape,
    cancel: &Cancel,
    request: u64,
) -> Aside {
    let deadline = Instant::now() + shared.template.limits.call;
    let mut partial = String::new();
    let mut reasoning = String::new();
    let mut called = false;
    let mut hosted = Hosted::default();
    let result = shared
        .transport
        .stream(shape, deadline, cancel, &mut |piece: Piece<'_>| {
            if hosted.took(&piece) {
                return;
            }
            let event = match piece {
                Piece::Text(piece) => {
                    partial.push_str(piece);
                    Event::Delta {
                        request,
                        text: piece.to_owned(),
                    }
                }
                Piece::Reasoning(piece) => {
                    reasoning.push_str(piece);
                    Event::Reasoning {
                        request,
                        text: piece.to_owned(),
                    }
                }
                Piece::Progress(frame) => Event::Progress {
                    request,
                    progress: frame,
                },
                Piece::ToolCall { .. } => {
                    called = true;
                    return;
                }
                Piece::Signature(_) | Piece::Redacted(_) | Piece::Usage(_) => return,
            };
            shared.lock().push(event);
            shared.changed.notify_all();
        });
    Aside {
        result,
        partial,
        reasoning,
        called,
        hosted,
    }
}

/// What [`aside`] streamed.
struct Aside {
    result: Result<StreamEnded, TransportFailure>,
    partial: String,
    reasoning: String,
    called: bool,
    /// What a hosted API said beside the answer (#555).
    hosted: Hosted,
}

/// A seam's audit call, made and settled (#504): its pieces streamed into
/// the log on the `audit` lane; on an answer, read by `diet::formats::audit`
/// against the ask's numbering -- one line per note, 1..n -- and folded:
/// `UPDATE` supersedes the note, `REMOVE` and a dup retire it, `KEEP`
/// leaves it. `value` when it changed something, `decline` when it kept
/// everything, `unparseable` when the answer is not the ask's audit (or the
/// call made a tool call); a cut, stopped or failed call folds nothing.
fn audited<S: Streaming>(shared: &Shared<S>, fired: AuditFired) {
    let AuditFired {
        fork,
        request,
        shape,
        cancel,
        items,
    } = fired;
    let Aside {
        result,
        partial,
        reasoning,
        called,
        hosted,
    } = aside(shared, &shape, &cancel, request);
    let mut state = shared.lock();
    state.flight = None;
    let reasoning = Some(reasoning).filter(|thought| !thought.is_empty());
    let mut lines = Vec::new();
    let outcome = match result {
        Ok(StreamEnded::Finished {
            finish_reason,
            timings,
        }) => {
            let text = partial.clone();
            let cut = capped(finish_reason.as_deref());
            state.push(Event::Answered {
                request,
                text: partial,
                finish_reason,
                reasoning,
                timings,
                hosted,
            });
            if cancel.is_asked() {
                log::ForkOutcome::Cancelled
            } else if cut {
                log::ForkOutcome::Truncated
            } else if called {
                log::ForkOutcome::Unparseable
            } else {
                let (outcome, patched) = audit_folded(&mut state, &text, &items, fork);
                lines = patched;
                outcome
            }
        }
        Ok(StreamEnded::Cancelled) => {
            state.push(Event::Cancelled { request, partial });
            log::ForkOutcome::Cancelled
        }
        Ok(StreamEnded::Rejected {
            status,
            body,
            class,
        }) => {
            let overflow = state.overflow(class == Some(Rejection::ContextOverflow));
            state.push(Event::Rejected {
                request,
                status,
                body,
                overflow,
                class,
                partial,
            });
            log::ForkOutcome::Failed
        }
        Err(failure) => {
            let overflow = state.failed_overflow(&failure);
            state.push(Event::Failed {
                request,
                overflow,
                failure,
                partial,
            });
            log::ForkOutcome::Failed
        }
    };
    state.push(Event::ForkSettled {
        fork,
        outcome,
        prompt_tokens: None,
        wall_ms: None,
        refused: None,
    });
    for line in lines {
        state.push(line);
    }
    drop(state);
    shared.changed.notify_all();
}

/// An audit's answer `text`, folded (#504): its lines, which must number
/// every one of `items` once; each `UPDATE` a supersession of its note,
/// each `REMOVE` (a dup among them) a retirement, applied as a fork's
/// patches are, the audit's fork `f/<fork>` their provenance.
fn audit_folded(
    state: &mut State,
    text: &str,
    items: &[crate::object::EntryId],
    fork: u64,
) -> (log::ForkOutcome, Vec<Event>) {
    use crate::formats::audit::Judgment;
    let Ok(lines) = crate::formats::audit::parse(text) else {
        return (log::ForkOutcome::Unparseable, Vec::new());
    };
    if lines.len() != items.len() {
        return (log::ForkOutcome::Unparseable, Vec::new());
    }
    let turn = state.turns;
    let provenance = |index: u32| crate::object::Provenance {
        turn,
        lane: log::Lane::Audit.tag().to_owned(),
        fork: Some(format!("f/{fork}")),
        tangent: None,
        index,
    };
    let patches: Vec<Patch> = lines
        .iter()
        .filter_map(|line| {
            let target = items.get(usize::try_from(line.number).ok()? - 1)?.clone();
            match &line.judgment {
                Judgment::Keep => None,
                Judgment::Update(note) => Some(Patch::Supersede {
                    id: crate::object::EntryId::new(&format!("audit-{fork}-{}", line.number))
                        .ok()?,
                    content: note.clone(),
                    voids: target,
                    provenance: provenance(line.number),
                }),
                Judgment::Remove(_) | Judgment::DupOf(_) => Some(Patch::Retire {
                    target,
                    provenance: provenance(line.number),
                }),
            }
        })
        .collect();
    if patches.is_empty() {
        return (log::ForkOutcome::Decline, Vec::new());
    }
    applied(state, &patches, fork)
}

/// The automatic seam (#617), checked before every trunk request on
/// `shape`: when serve knows the window, the regimen has not turned it off
/// ([`crate::seam::policy::SEAM_WINDOW`]), no tangent is open, working
/// memory holds an entry and the trunk holds turns the refill would drop,
/// and the prompt as sized (#588) would leave less than
/// [`crate::seam::policy::WINDOW_RESERVE`] -- or the output cap, if more --
/// of the window: the trunk is refilled, and the turn's own messages so far
/// ride after the refill as they rode after the trunk. Otherwise the request
/// goes as it is, as Pi, `OpenCode` 2 and Qwen Code all send it when there is
/// nothing to compact.
fn window_seam(
    template: &RequestShape,
    state: &mut State,
    shape: &mut RequestShape,
    forced: bool,
) -> bool {
    let Some(window) = shape.limits.context_window else {
        return false;
    };
    let Some(interview) = state.interview.as_ref() else {
        return false;
    };
    if interview.seams.window_off
        || state.tangent.is_some()
        || interview.object.live().next().is_none()
        || !shape.messages.starts_with(&state.trunk)
    {
        return false;
    }
    let turns = state
        .trunk
        .get(template.messages.len() + usize::from(state.refilled)..)
        .unwrap_or_default();
    if crate::seam::render::tail(turns, interview.seams.tail_tokens).len() == turns.len() {
        return false;
    }
    let prompt = state.prompt_of(shape);
    let reserve =
        u64::from(template.limits.max_output_tokens).max(crate::seam::policy::WINDOW_RESERVE);
    if !forced && prompt <= window.saturating_sub(reserve) {
        return false;
    }
    let own = shape.messages.split_off(state.trunk.len());
    refill_trunk(
        template,
        state,
        (
            crate::seam::Reason::Window,
            Some(log::SeamFired {
                prompt_tokens: prompt,
                window,
            }),
        ),
        None,
    );
    shape.messages.clone_from(&state.trunk);
    shape.messages.extend(own);
    true
}

/// After a trunk request that overflowed the window (#616), the automatic
/// seam once more and the request again (#617), as Pi, `OpenCode` 2 and Qwen
/// Code each compact and retry once after an overflow: the seam forced,
/// whatever the sizing said, and the same messages sent on the refilled
/// trunk. Once per turn; and not when the seam has nothing to refill from
/// or nothing to drop, when the turn fails as it would have. The next
/// request's sequence number when it was sent.
fn retried(
    template: &RequestShape,
    state: &mut State,
    shape: &mut RequestShape,
    turn: u32,
) -> Option<u64> {
    if state.overflow_retried == Some(turn) || !window_seam(template, state, shape, true) {
        return None;
    }
    state.overflow_retried = Some(turn);
    let max_tokens = state.sized(shape, template.limits.max_output_tokens);
    let next = state.push(Event::Requested {
        turn,
        lane: Lane::Trunk,
        head_sha256: crate::client::head::Head::of(shape).digest().to_owned(),
        fork: None,
        max_tokens,
    });
    if let Some(flight) = state.flight.as_mut() {
        flight.request = next;
    }
    Some(next)
}

/// Each pruned result in a seam's kept tail replaced in place by its line
/// (#612): the tool message stays, answering its call.
fn replaced_in_tail(kept: &mut [Message], pruned: &[Prune]) {
    for message in kept {
        if let Some(prune) = pruned.iter().find(|prune| prune.is(message)) {
            message.content.clone_from(&prune.text);
            message.images.clear();
        }
    }
}

/// The prunes a seam applies -- every one since the last (#612) -- marked
/// applied, so none is due any more; `None` when there were none.
fn prunes_applied(state: &mut State) -> Option<Vec<String>> {
    let applied: Vec<String> = state
        .pruned
        .iter_mut()
        .filter(|prune| !prune.applied)
        .map(|prune| {
            prune.applied = true;
            prune.call.clone()
        })
        .collect();
    (!applied.is_empty()).then_some(applied)
}

/// What a seam at `at_turn` archives (#566): each message of `compacted`,
/// keyed by the seam and its position, and each entry of `object` that is
/// no longer live, keyed by its id.
fn archived_at_seam(
    compacted: &[Message],
    object: &WorkingObject,
    at_turn: u32,
) -> Vec<super::archive::Item> {
    use super::archive::Item;
    let messages = compacted.iter().enumerate().map(|(at, message)| {
        let calls: Vec<String> = message
            .tool_calls
            .iter()
            .map(|call| format!("{} {}", call.name, call.arguments))
            .collect();
        let title = match message.role {
            Role::User => "an ask".to_owned(),
            Role::Tool => "a tool result".to_owned(),
            _ if !calls.is_empty() => "a step that called tools".to_owned(),
            _ => "an answer".to_owned(),
        };
        let text = if calls.is_empty() {
            message.content.clone()
        } else {
            format!("{}\n{}", message.content, calls.join("\n"))
        };
        Item {
            key: format!("seam-{at_turn}/message-{}", at + 1),
            title,
            text,
        }
    });
    let entries = object
        .entries()
        .filter(|entry| !entry.state.is_live())
        .map(|entry| Item {
            key: format!("entry/{}", entry.id.as_str()),
            title: format!("a {} entry", entry.state.name()),
            text: entry.content.clone(),
        });
    messages.chain(entries).collect()
}

/// The outputs in `compacted` -- the trunk's turns a seam compacts away --
/// in call order, each with its call, its turn and what `state` needs of
/// it (#553): its whole saved by digest for `reference`, the read fork's
/// excerpts quoted from it for `salient`. Each result is matched to its
/// `tool_call` line in log order, by call id and what it was shown.
fn compacted_outputs(
    log: &[Logged],
    compacted: &[Message],
    state: log::SeamToolOutputs,
    (recording, pruned): (Option<&std::path::Path>, &[Prune]),
) -> Vec<crate::seam::outputs::Output> {
    use crate::seam::outputs::Output;
    let lines: Vec<&ToolLine> = log
        .iter()
        .filter_map(|logged| match &logged.event {
            Event::ToolCalled(line) => Some(line.as_ref()),
            _ => None,
        })
        .collect();
    let mut cursor = 0;
    let mut calls: &[crate::client::shape::ToolCall] = &[];
    let mut outputs = Vec::new();
    let mut files = Vec::new();
    for message in compacted {
        if message.role == Role::Assistant {
            calls = &message.tool_calls;
            continue;
        }
        if message.role != Role::Tool {
            continue;
        }
        let Some(call) = calls
            .iter()
            .find(|call| Some(&call.id) == message.tool_call_id.as_ref())
        else {
            continue;
        };
        // A pruned result is its line, whatever was shown (#612): one an
        // earlier seam already replaced in the tail has no line to match.
        if let Some(prune) = pruned.iter().find(|prune| prune.is(message)) {
            if let Some(at) = lines.iter().skip(cursor).position(|line| {
                line.id == call.id && line.shown.as_deref() == Some(message.content.as_str())
            }) {
                cursor += at + 1;
            }
            files.push(None);
            outputs.push(Output {
                turn: 0,
                name: call.name.clone(),
                arguments: call.arguments.clone(),
                shown: String::new(),
                images: Vec::new(),
                saved: None,
                excerpts: Vec::new(),
                pruned: Some(prune.text.clone()),
            });
            continue;
        }
        let Some(at) = lines.iter().skip(cursor).position(|line| {
            line.id == call.id && line.shown.as_deref() == Some(message.content.as_str())
        }) else {
            continue;
        };
        let line = lines[cursor + at];
        cursor += at + 1;
        files.push(line.files.first().cloned());
        outputs.push(Output {
            turn: line.turn,
            name: call.name.clone(),
            arguments: call.arguments.clone(),
            shown: message.content.clone(),
            images: message
                .images
                .iter()
                .map(|image| image.media_type().to_owned())
                .collect(),
            saved: None,
            excerpts: Vec::new(),
            pruned: None,
        });
    }
    if state == log::SeamToolOutputs::Reference {
        for (output, file) in outputs
            .iter_mut()
            .zip(files)
            .filter(|(output, _)| output.pruned.is_none())
        {
            output.saved = Some(saved_whole(&output.shown, file, recording));
        }
    }
    if state == log::SeamToolOutputs::Salient {
        for (turn, excerpt) in read_excerpts(log) {
            let quoted = excerpt.trim();
            if let Some(output) = outputs.iter_mut().find(|output| {
                output.pruned.is_none()
                    && output.turn == turn
                    && !quoted.is_empty()
                    && output.shown.contains(quoted)
            }) {
                output.excerpts.push(quoted.to_owned());
            }
        }
    }
    outputs
}

/// An output's whole, by digest: the copy its line already names (a
/// capped output's whole, #554, or `read`'s image, #557), else `shown`
/// saved now; unsaved, its size and digest alone, with no recording.
fn saved_whole(
    shown: &str,
    file: Option<log::RecordedFile>,
    recording: Option<&std::path::Path>,
) -> crate::seam::outputs::Saved {
    let file = file.or_else(|| {
        recording
            .and_then(|dir| super::attach::kept_whole(dir, shown.as_bytes(), "text/plain").ok())
    });
    match (file, recording) {
        (Some(file), Some(dir)) => crate::seam::outputs::Saved {
            bytes: file.bytes,
            sha256: file.sha256,
            path: Some(dir.join(&file.path).to_string_lossy().into_owned()),
        },
        _ => crate::seam::outputs::Saved {
            bytes: shown.len() as u64,
            sha256: crate::digest::sha256_hex(shown.as_bytes()),
            path: None,
        },
    }
}

/// Each excerpt a `read` fork quoted (the dogma's `EXCERPT`, folded as an
/// `evidence` entry), with the turn the fork followed, in log order.
fn read_excerpts(log: &[Logged]) -> Vec<(u32, String)> {
    let prefix = format!(
        "{}: ",
        crate::formats::interview::FieldKind::Evidence.canonical_tag()
    );
    let reads: BTreeMap<u64, u32> = log
        .iter()
        .filter_map(|logged| match &logged.event {
            Event::Forked {
                of_turn,
                why: log::Warrant::Read,
                ..
            } => Some((logged.seq, *of_turn)),
            _ => None,
        })
        .collect();
    log.iter()
        .filter_map(|logged| match &logged.event {
            Event::Patched {
                fork,
                op: log::PatchOp::Add,
                entry,
                ..
            } => {
                let turn = reads.get(fork.as_ref()?)?;
                Some((*turn, entry.text.strip_prefix(&prefix)?.to_owned()))
            }
            _ => None,
        })
        .collect()
}

/// The turn is over: back to awaiting, or on to ended; then, while
/// awaiting, the seam the regimen's cadence or budget makes due, if working
/// memory holds an entry to refill from. Not due with nothing to refill
/// from: it stays due, and fires after the next turn that leaves an entry.
fn turn_over(template: &RequestShape, state: &mut State) {
    state.after_the_turn();
    if state.settlement != Settlement::Awaiting {
        return;
    }
    // No derived seam while a tangent is open (#22): its close restores the
    // fork point's trunk, which a refill would have replaced.
    let due = state
        .interview
        .as_ref()
        .filter(|_| state.tangent.is_none())
        .and_then(|interview| {
            interview.object.live().next()?;
            interview
                .seams
                .due(state.turns - state.turns_at_seam, state.trunk_tokens)
                // A prune applied as its turn settles (#612): deferred, as
                // any derived seam is, while it cannot fire.
                .or_else(|| {
                    (state.pruned.iter().any(|prune| !prune.applied)
                        && interview.prune == Some(super::prune::PruneSeam::TurnEnd))
                    .then_some(crate::seam::Reason::Prune)
                })
        });
    if let Some(reason) = due {
        seam(template, state, reason, None);
    }
}

/// The least output the clamp leaves a request when its window has it
/// (#588): Qwen Code's 4,000.
pub const CLAMP_FLOOR: u64 = 4_000;

/// The regimen key for a fork's tail (#406): the output cap a fork's call
/// is clamped from, in place of the session's.
pub const FORK_TAIL_TOKENS: &str = "fork_tail_tokens";

/// The fork tail the regimen declares (#406), leniently: a positive whole
/// number, or else none, the session's output cap.
#[must_use]
pub fn fork_tail(regimen: &Regimen) -> Option<u32> {
    match regimen.get(FORK_TAIL_TOKENS) {
        Some(crate::formats::regimen::Value::Integer(n)) if *n > 0 => u32::try_from(*n).ok(),
        _ => None,
    }
}

/// A seam queued for [`seam_work`] (#504): what fired it, and the phase it
/// moves to.
struct PendingSeam {
    reason: crate::seam::Reason,
    to: Option<String>,
}

/// A seam's audit, fired ([`audit_fired`], #504).
struct AuditFired {
    /// The sequence number of its fork line.
    fork: u64,
    /// The sequence number of its call's request.
    request: u64,
    shape: RequestShape,
    cancel: Cancel,
    /// The notes the ask numbered, in its order.
    items: Vec<crate::object::EntryId>,
}

/// How a fork's call fits its server ([`State::fork_fit`], #406).
struct ForkFit {
    /// Its prompt, as sized.
    prompt: u64,
    /// Whether it may displace the trunk's cached prefix.
    displaces: bool,
    /// Whether it is refused unsent, `pool`.
    refused: bool,
}

/// Why a fork that would not fit its window was never sent (#406).
const REFUSED_POOL: &str = "pool";

/// The hazard a fork line names when its call, on the trunk's own server,
/// does not share the trunk's prefix (#406).
const DISPLACES_TRUNK_CACHE: &str = "may-displace-trunk-cache";

/// The share of a `window` the output cap's sizing keeps clear of a
/// prompt's estimate (#588): 5% of it, or 10,000 tokens if that is more.
fn margin(window: i128) -> i128 {
    (window * 5 / 100).max(10_000)
}

/// That `messages` can open a session: a head is the trunk before any
/// turn; a tool result answers a call made in one, and the log's head has
/// no word for it or for a developer message (`role_of`).
///
/// # Panics
///
/// When it holds either.
fn a_head(messages: &[Message]) {
    assert!(
        messages
            .iter()
            .all(|m| m.role != Role::Tool && m.role != Role::Developer),
        "a session's head holds no tool result and no developer message"
    );
}

/// The estimated tokens of `shape`'s messages (#588).
fn estimate_of(shape: &RequestShape) -> u64 {
    shape
        .messages
        .iter()
        .map(crate::seam::render::estimated_tokens)
        .sum()
}

/// The pad added to a prompt that is wholly estimated (#588): Qwen Code's,
/// for the system prompt and tool definitions an estimate misses
/// (`qc:packages/core/src/core/llm-chat.ts:729`, `ESTIMATE_CLAMP_OVERHEAD_PAD`).
pub const ESTIMATE_PAD: u64 = 20_000;

/// The output cap `ceiling`, clamped to the room a `prompt` of that many
/// tokens leaves in a `window` (#588), by Qwen Code's arithmetic, which
/// breaks Pi's and its tie on the margin and the floor
/// (`qc:packages/core/src/core/tokenLimits.ts:50,59-61,83-106`): the room is
/// the window less the prompt less a margin of max(10,000, 5% of the window),
/// floored at min(4,000, window - prompt), and at least 1. No window, no
/// clamp.
#[must_use]
pub fn clamped(ceiling: u32, window: Option<u64>, prompt: u64) -> u32 {
    let Some(window) = window else {
        return ceiling;
    };
    let window = i128::from(window);
    let prompt = i128::from(prompt);
    let room = window - prompt - margin(window);
    let floor = (window - prompt).clamp(1, i128::from(CLAMP_FLOOR));
    let cap = i128::from(ceiling).min(room.max(floor));
    u32::try_from(cap).unwrap_or(ceiling)
}

/// The trunk's tokens after a trunk call: what it prefilled, what it reused
/// from the cache, and what it generated. `None` when the server reported
/// none of them.
fn trunk_tokens_of(timings: &Timings) -> Option<u64> {
    if timings.prompt_n.is_none() && timings.cache_n.is_none() && timings.predicted_n.is_none() {
        return None;
    }
    Some(
        timings.prompt_n.unwrap_or(0)
            + timings.cache_n.unwrap_or(0)
            + timings.predicted_n.unwrap_or(0),
    )
}

/// A call that finished. One its output cap ended is not an answer: off the
/// trunk, settled `failed` (#290, ruled 5969297103), as a failed or
/// cancelled call is (D13). On the floor a loop on a literal `</think>`
/// followed truncated reasoning re-sent as history; whether that was the
/// cause, the template, or both is unmeasured (#94). Any other is the
/// turn's answer, and the session moves to `capture`: whether it settled
/// `final`, and so whether its gap is the caller's to run.
fn settle_finished(
    state: &mut State,
    exchange: Vec<Message>,
    (request, turn): (u64, u32),
    (partial, reasoning): (String, String),
    (finish_reason, timings, hosted): (Option<String>, Option<Timings>, Hosted),
) -> bool {
    if finish_reason
        .as_deref()
        .is_some_and(|reason| CAPPED_FINISH_REASONS.contains(&reason))
    {
        state.push(Event::Capped {
            request,
            text: partial,
            finish_reason,
            reasoning: (!reasoning.is_empty()).then_some(reasoning),
            timings,
            hosted,
        });
        state.keep_ran_steps();
        state.push(Event::TurnSettled {
            turn,
            reason: SettleReason::Failed,
        });
        state.move_to(Settlement::Awaiting);
        return false;
    }
    state.ran.clear();
    state.trunk.extend(exchange);
    // The reasoning goes back with the answer, byte for byte and
    // untrimmed: measured on e7051ef (#117, Q10), dropping it
    // diverges the next prompt at this turn, and a stray newline
    // diverges it inside this turn. ONE binding feeds the trunk and
    // the response line, so the two cannot differ (R3.4).
    let reasoning = (!reasoning.is_empty()).then_some(reasoning);
    let mut answer = Message::new(Role::Assistant, partial.clone());
    answer.reasoning.clone_from(&reasoning);
    hosted.onto(&mut answer);
    state.trunk.push(answer);
    state.trunk_tokens = timings.as_ref().and_then(trunk_tokens_of);
    state.push(Event::Answered {
        request,
        text: partial,
        finish_reason,
        reasoning,
        timings,
        hosted,
    });
    state.push(Event::TurnSettled {
        turn,
        reason: SettleReason::Final,
    });
    state.move_to(Settlement::Capture);
    true
}

/// What a hosted API says beside an answer (#555): the signature on its
/// thinking, the thinking it redacted, and its token counts with the
/// cache's reads and writes. Empty from a server we run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hosted {
    /// The thinking's signature.
    pub signature: Option<String>,
    /// Each redacted block's data.
    pub redacted: Vec<String>,
    /// The counts, as the log's `usage` writes them.
    pub usage: Option<log::Usage>,
}

impl Hosted {
    /// Whether `piece` is one of these, taken.
    fn took(&mut self, piece: &Piece<'_>) -> bool {
        match piece {
            Piece::Signature(signature) => {
                self.signature
                    .get_or_insert_with(String::new)
                    .push_str(signature);
            }
            Piece::Redacted(data) => self.redacted.push((*data).to_owned()),
            Piece::Usage(usage) => {
                self.usage = usage.prompt_tokens().map(|prompt_tokens| log::Usage {
                    prompt_tokens,
                    completion_tokens: usage.output_tokens.unwrap_or(0),
                    cached_tokens: usage.cache_read_input_tokens,
                    cache_creation_tokens: usage.cache_creation_input_tokens,
                    cache_creation_5m_tokens: usage.ephemeral_5m_input_tokens,
                    cache_creation_1h_tokens: usage.ephemeral_1h_input_tokens,
                });
            }
            _ => return false,
        }
        true
    }

    /// The signature and redacted thinking onto the message that goes back.
    fn onto(&self, message: &mut Message) {
        message.reasoning_signature.clone_from(&self.signature);
        message.redacted.clone_from(&self.redacted);
    }
}

/// A fork fired in the capture gap, and what its call needs.
struct Fired {
    turn: u32,
    /// The sequence number of its [`Event::Forked`].
    fork: u64,
    /// The sequence number of its call's [`Event::Requested`].
    request: u64,
    shape: RequestShape,
    cancel: Cancel,
}

/// The capture gap after `turn` settled `final` at its trunk request `at`
/// (#374): at most one fork, fired when the regimen's warrant holds, born
/// off the warm trunk -- its messages, then the question -- and never
/// appended to it. One call site, after one `final` settling, so one gap
/// cannot fire two. With no warrant, or an `end` admitted meanwhile, the gap
/// passes through to `awaiting`.
fn gap<S>(shared: &Shared<S>, state: &mut State, turn: u32, at: u64) -> Option<Fired> {
    let answer = state
        .log
        .iter()
        .rev()
        .find_map(|logged| match &logged.event {
            Event::Answered { request, text, .. } if *request == at => Some(text.as_str()),
            _ => None,
        })
        .unwrap_or_default();
    let mut skipped = Vec::new();
    let mut fired: std::collections::VecDeque<Firing> = state
        .interview
        .as_ref()
        .filter(|_| !state.ending)
        .map(|interview| {
            let (kept, skips) = screened(
                interview,
                &state.log,
                turn,
                firings(interview, &state.log, turn, answer),
            );
            (kept, skips)
        })
        .map(|(kept, skips)| {
            skipped.extend(skips);
            kept
        })
        .unwrap_or_default()
        .into();
    // Each fork screened out (#611), before the gap fires any.
    for skip in std::mem::take(&mut skipped) {
        state.push(skip);
    }
    let Some(first) = fired.pop_front() else {
        turn_over(&shared.template, state);
        return None;
    };
    fired.push_front(first);
    state.queued = fired.into_iter().map(|firing| (turn, at, firing)).collect();
    let next = fire_next(shared, state);
    if next.is_none() {
        turn_over(&shared.template, state);
    }
    next
}

/// The gap's next queued fork that is sent (#564, #406): a fork refused
/// unsent for the pool settles at once, and the one after it fires. `None`
/// when none is left.
fn fire_next<S>(shared: &Shared<S>, state: &mut State) -> Option<Fired> {
    while let Some((turn, at, firing)) = state.queued.pop_front() {
        if let Some(fired) = fire(shared, state, turn, at, firing) {
            return Some(fired);
        }
    }
    None
}

/// One of the gap's forks, fired: born off the warm trunk -- what the view
/// shows of it, then the question -- and never appended to it.
fn fire<S>(
    shared: &Shared<S>,
    state: &mut State,
    turn: u32,
    at: u64,
    firing: Firing,
) -> Option<Fired> {
    let Firing {
        why,
        ask,
        question,
        trigger,
    } = firing;
    let role = state
        .interview
        .as_ref()
        .map_or(Role::User, |interview| interview.role);
    let mut shape = shared.template.clone();
    let declared = state
        .interview
        .as_ref()
        .and_then(|interview| interview.view);
    // An offboard seat holds none of the trunk warm, so it is shown the last
    // turn unless the regimen says otherwise (#570).
    let view = declared.unwrap_or(if shared.seat.is_some() {
        ForkView::Last(1)
    } else {
        ForkView::Trunk
    });
    shape.messages = viewed(&state.trunk, shared.template.messages.len(), view);
    shape.messages.push(Message::new(role, question.clone()));
    let seat = shared.seat.as_ref().map(|seat| {
        shape.model.clone_from(&seat.model);
        shape.limits.context_window = seat.context_window;
        log::ForkSeat {
            substrate: seat.substrate.clone(),
            model: seat.model.clone(),
        }
    });
    // Its tail: the output cap, or the regimen's, clamped as the trunk's is
    // (#588); refused unsent when its window leaves less than the floor.
    let ceiling = state
        .interview
        .as_ref()
        .and_then(|interview| interview.fork_tail)
        .unwrap_or(shared.template.limits.max_output_tokens);
    let ForkFit {
        prompt,
        displaces,
        refused,
    } = state.fork_fit(&shape, seat.is_some(), ceiling);
    let fork = state.push(Event::Forked {
        of_turn: turn,
        at,
        why,
        question,
        view,
        trigger,
        ask,
        role,
        seat,
        displaces,
    });
    if refused {
        state.push(Event::ForkSettled {
            fork,
            outcome: log::ForkOutcome::Refused,
            prompt_tokens: None,
            wall_ms: None,
            refused: Some(REFUSED_POOL.to_owned()),
        });
        return None;
    }
    let max_tokens = state.sized_at(&mut shape, ceiling, prompt);
    let request = state.push(Event::Requested {
        turn,
        lane: Lane::Interview,
        head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
        fork: Some(fork),
        max_tokens,
    });
    let cancel = Cancel::new();
    state.flight = Some(Flight {
        turn,
        request,
        cancel: cancel.clone(),
    });
    state.forking = Some(fork);
    Some(Fired {
        turn,
        fork,
        request,
        shape,
        cancel,
    })
}

/// The gap's fork, on the turn's own thread: its one call streamed into the
/// log on the interview lane and never retried; then how it ended, and on
/// `value` its patches, applied to the session's working object; then back
/// to `awaiting`, or on to `ended` when an `end` arrived meanwhile.
///
/// A stop that reaches it settles it `cancelled`, even when the answer was
/// already done. A call that ended without an answer is `failed`; one its
/// output cap ended is `truncated`. A fork runs nothing (the router's
/// imperative: answer from this turn alone), so a call it makes is neither
/// run nor logged as a piece, and its answer is `unparseable` -- unless the
/// fork answers through the capture tools (#610), when each call is logged
/// as a piece and its capture calls are what it answered ([`captured`]).
fn interview<S: Streaming>(shared: &Shared<S>, forked: Fired) {
    let mut next = Some(forked);
    while let Some(forked) = next {
        next = one_fork(shared, forked);
    }
}

/// One of the gap's forks, run to its settling: then the next queued fork,
/// fired, or -- none left, a stop, or an `end` -- back to `awaiting`.
#[allow(clippy::too_many_lines)]
fn one_fork<S: Streaming>(shared: &Shared<S>, forked: Fired) -> Option<Fired> {
    let Fired {
        turn,
        fork,
        request,
        shape,
        cancel,
    } = forked;
    let deadline = Instant::now() + shared.template.limits.call;
    let mut partial = String::new();
    let mut reasoning = String::new();
    let mut called = false;
    let tools = shared
        .lock()
        .interview
        .as_ref()
        .map(|interview| interview.capture)
        == Some(crate::dogma::asks::Modality::Tools);
    let mut calls = Calls::default();
    let transport = shared
        .seat
        .as_ref()
        .map_or(&shared.transport, |seat| &seat.transport);
    let began = Instant::now();
    let mut hosted = Hosted::default();
    let result = transport.stream(&shape, deadline, &cancel, &mut |piece: Piece<'_>| {
        if hosted.took(&piece) {
            return;
        }
        // The fork's own pieces, each naming its call's request.
        let event = match piece {
            Piece::Text(piece) => {
                partial.push_str(piece);
                let text = piece.to_owned();
                Event::Delta { request, text }
            }
            Piece::Reasoning(piece) => {
                reasoning.push_str(piece);
                let text = piece.to_owned();
                Event::Reasoning { request, text }
            }
            Piece::Progress(frame) => Event::Progress {
                request,
                progress: frame,
            },
            Piece::ToolCall {
                index,
                id,
                name,
                arguments,
            } if tools => {
                calls.piece(index, id, name, arguments);
                Event::ToolCallPiece {
                    request,
                    index,
                    id: id.map(str::to_owned),
                    name: name.map(str::to_owned),
                    arguments: arguments.to_owned(),
                }
            }
            Piece::ToolCall { .. } => {
                called = true;
                return;
            }
            Piece::Signature(_) | Piece::Redacted(_) | Piece::Usage(_) => return,
        };
        shared.lock().push(event);
        shared.changed.notify_all();
    });
    let mut state = shared.lock();
    state.flight = None;
    state.forking = None;
    let reasoning = Some(reasoning).filter(|thought| !thought.is_empty());
    let mut patches = Vec::new();
    let mut prompt_tokens = None;
    let outcome = match result {
        Ok(StreamEnded::Finished {
            finish_reason,
            timings,
        }) => {
            prompt_tokens = timings.as_ref().and_then(|timings| timings.prompt_n);
            let text = partial.clone();
            let cut = capped(finish_reason.as_deref());
            state.push(if cut {
                Event::Capped {
                    request,
                    text: partial,
                    finish_reason,
                    reasoning,
                    timings,
                    hosted,
                }
            } else {
                Event::Answered {
                    request,
                    text: partial,
                    finish_reason,
                    reasoning,
                    timings,
                    hosted,
                }
            });
            if cancel.is_asked() {
                log::ForkOutcome::Cancelled
            } else if cut {
                log::ForkOutcome::Truncated
            } else if tools {
                let (outcome, lines) =
                    captured(&mut state, &calls.into_calls(), (turn, request), fork);
                patches = lines;
                outcome
            } else if called {
                log::ForkOutcome::Unparseable
            } else {
                let (outcome, lines) = folded(&mut state, &text, turn, fork);
                patches = lines;
                outcome
            }
        }
        Ok(StreamEnded::Cancelled) => {
            // A stop reached the fork's call: what arrived is no answer.
            state.push(Event::Cancelled { request, partial });
            log::ForkOutcome::Cancelled
        }
        Ok(StreamEnded::Rejected {
            status,
            body,
            class,
        }) => {
            // The server refused the fork's call: `failed`, never retried.
            let overflow = state.overflow(class == Some(Rejection::ContextOverflow));
            state.push(Event::Rejected {
                request,
                status,
                body,
                overflow,
                class,
                partial,
            });
            log::ForkOutcome::Failed
        }
        Err(failure) => {
            let overflow = state.failed_overflow(&failure);
            state.push(Event::Failed {
                request,
                overflow,
                failure,
                partial,
            });
            log::ForkOutcome::Failed
        }
    };
    // An offboard fork's cost, for the seat's prefill rate (#570).
    let (prompt_tokens, wall_ms) = if shared.seat.is_some() {
        (
            prompt_tokens,
            Some(u64::try_from(began.elapsed().as_millis()).unwrap_or(u64::MAX)),
        )
    } else {
        (None, None)
    };
    state.push(Event::ForkSettled {
        fork,
        outcome,
        prompt_tokens,
        wall_ms,
        refused: None,
    });
    for patch in patches {
        state.push(patch);
    }
    // The gap's next fork (#564), unless a stop or an `end` came meanwhile.
    if outcome == log::ForkOutcome::Cancelled || state.ending {
        state.queued.clear();
    }
    let next = fire_next(shared, &mut state);
    if next.is_none() {
        turn_over(&shared.template, &mut state);
    }
    drop(state);
    shared.changed.notify_all();
    next
}

/// A fork's answer through the capture tools (#610): each capture call run
/// as the trunk's is -- the contract, and for `update_record` the
/// groundedness gate against what the trunk saw by turn `turn`, never the
/// fork's own words -- its patches the fork's (lane `interview`, fork
/// `f/<fork>`) and applied as a fork's are, one `capture` line each naming
/// the fork, then its patches' lines. `value` when a call wrote or ruled on
/// something; `decline` when the capture calls kept nothing, or it made
/// none; `unparseable` when every call was to some other tool, which a fork
/// never runs.
fn captured(
    state: &mut State,
    calls: &[Call],
    (turn, request): (u32, u64),
    fork: u64,
) -> (log::ForkOutcome, Vec<Event>) {
    use crate::capture::tools::CaptureTool;
    let mut lines = Vec::new();
    let mut wrote = false;
    let capture_calls: Vec<&Call> = calls
        .iter()
        .filter(|call| CaptureTool::from_tag(&call.name).is_some())
        .collect();
    if capture_calls.is_empty() {
        let outcome = if calls.is_empty() {
            log::ForkOutcome::Decline
        } else {
            log::ForkOutcome::Unparseable
        };
        return (outcome, lines);
    }
    for call in capture_calls {
        let (refused, entries, patched) = match capture_patches(state, (turn, request), call) {
            Err(why) => (Some(why), Vec::new(), Vec::new()),
            Ok(mut patches) => {
                for patch in &mut patches {
                    let at = provenance_of(patch);
                    super::INTERVIEW.clone_into(&mut at.lane);
                    at.fork = Some(format!("f/{fork}"));
                }
                let entries = entries_of(&patches);
                if patches.is_empty() {
                    (None, entries, Vec::new())
                } else {
                    match applied(state, &patches, fork) {
                        (log::ForkOutcome::Value, lines) => (None, entries, lines),
                        _ => (
                            Some("the working object refused it".to_owned()),
                            Vec::new(),
                            Vec::new(),
                        ),
                    }
                }
            }
        };
        let (outcome, entries, why) = capture_outcome(&call.name, refused, entries);
        wrote |= matches!(outcome, "recorded" | "resolved");
        lines.push(Event::Captured {
            request,
            call: call.id.clone(),
            tool: call.name.clone(),
            outcome: outcome.to_owned(),
            entries,
            why,
            fork: Some(fork),
        });
        lines.extend(patched);
    }
    let outcome = if wrote {
        log::ForkOutcome::Value
    } else {
        log::ForkOutcome::Decline
    };
    (outcome, lines)
}

/// The fork's answer, read by the interview grammar and folded as the
/// scripted gym folds a fork's answer ([`super::fold`]), its patches applied
/// to the working object: `value` and one [`Event::Patched`] per patch;
/// `decline` when it recorded nothing; `unparseable` when the grammar, or
/// the object, refused it.
fn folded(state: &mut State, text: &str, turn: u32, fork: u64) -> (log::ForkOutcome, Vec<Event>) {
    let Ok((patches, _census)) =
        super::fold(text, turn, super::INTERVIEW, Some(&format!("f/{fork}")))
    else {
        return (log::ForkOutcome::Unparseable, Vec::new());
    };
    if patches.is_empty() {
        return (log::ForkOutcome::Decline, Vec::new());
    }
    let Some(interview) = state.interview.as_ref() else {
        return (log::ForkOutcome::Unparseable, Vec::new());
    };
    let patches = cited(patches, &interview.object);
    // Made under an open tangent (#22): stamped with it from birth, so its
    // close finds them by provenance and never by recency.
    let patches: Vec<Patch> = match state.tangent.as_ref() {
        None => patches,
        Some((tangent, _)) => patches
            .into_iter()
            .map(|mut patch| {
                let at = match &mut patch {
                    Patch::Add { provenance, .. }
                    | Patch::Supersede { provenance, .. }
                    | Patch::Resolve { provenance, .. }
                    | Patch::Retire { provenance, .. }
                    | Patch::Park { provenance, .. } => provenance,
                };
                at.tangent = Some(tangent.id().to_owned());
                patch
            })
            .collect(),
    };
    applied(state, &patches, fork)
}

/// A fork's `patches`, applied to the working object (every op the format
/// has, #562): one [`Event::Patched`] each, and each non-`add` op's entry
/// held for the next ask's note when delivery is mid-turn (#550).
fn applied(state: &mut State, patches: &[Patch], fork: u64) -> (log::ForkOutcome, Vec<Event>) {
    let Some(interview) = state.interview.as_mut() else {
        return (log::ForkOutcome::Unparseable, Vec::new());
    };
    // The entry each delivered line names, read before the patches apply:
    // a supersede's voided entry, a verdict's target. An `add` names none
    // and is carried at the seam only (no measured sentence fits it).
    let delivering = interview.delivery != log::ForkDelivery::Seam;
    let named: Vec<(log::PatchOp, String, String)> = patches
        .iter()
        .filter(|_| delivering)
        .filter_map(|patch| {
            let (op, id) = match patch {
                Patch::Add { .. } => return None,
                Patch::Supersede { voids, .. } => (log::PatchOp::Supersede, voids),
                Patch::Resolve { target, .. } => (log::PatchOp::Resolve, target),
                Patch::Retire { target, .. } => (log::PatchOp::Retire, target),
                Patch::Park { target, .. } => (log::PatchOp::Park, target),
            };
            let text = interview.object.entry(id)?.content.clone();
            Some((op, id.as_str().to_owned(), text))
        })
        .collect();
    if interview.object.apply_turn(patches).is_err() {
        return (log::ForkOutcome::Unparseable, Vec::new());
    }
    let lines = patches
        .iter()
        .map(|patch| patched(Some(fork), patch, &interview.object))
        .collect();
    state.undelivered.extend(named);
    (log::ForkOutcome::Value, lines)
}

/// The prefix [`super::fold`] gives a `SUPERSEDE` field's entry: its kind's
/// canonical tag.
const SUPERSEDE_FIELD: &str = "supersede: ";

/// `patches` with each `SUPERSEDE` field that cites a live entry of
/// `object` made the supersession the format defines (#562): `SUPERSEDE:
/// <id> <the entry that should stand>` voids `<id>`, linked, and records the
/// rest under the field's own id. The seam's render shows each entry as
/// `<id>\t<content>`, so the id is what a model that saw it can cite; quotes,
/// backticks and brackets around it, and a colon or dash after it, are read
/// past. A field that cites nothing live, or names no entry to stand, stays
/// the addition [`super::fold`] made of it.
///
/// One field may carry several pairs, `;` between them, and `→` or `->` may
/// stand between a cited id and what replaces it (seen on a v4 fork, #562).
/// A pair whose replacement is itself a live entry's id retires the cited
/// entry in favour of that one, writing no new text: the entry that should
/// stand already does. Each pair after the first records under the field's
/// id with its position after it, and a pair that cites nothing live stays
/// an addition of its own text.
fn cited(patches: Vec<Patch>, object: &WorkingObject) -> Vec<Patch> {
    let fields = patches.len();
    let mut read: Vec<Patch> = patches
        .into_iter()
        .flat_map(|patch| {
            let Patch::Add {
                id,
                content,
                provenance,
            } = &patch
            else {
                return vec![patch];
            };
            let Some(field) = content.strip_prefix(SUPERSEDE_FIELD) else {
                return vec![patch];
            };
            let pairs: Vec<&str> = field
                .split(';')
                .map(str::trim)
                .filter(|pair| !pair.is_empty())
                .collect();
            if pairs.len() < 2 {
                return vec![
                    superseding(field.trim(), object, id.clone(), provenance).unwrap_or(patch),
                ];
            }
            // Each pair on its own: one that cites nothing live stays the
            // addition the field would have been, alone.
            pairs
                .iter()
                .enumerate()
                .filter_map(|(at, pair)| {
                    let id = if at == 0 {
                        id.clone()
                    } else {
                        crate::object::EntryId::new(&format!("{}-{at}", id.as_str())).ok()?
                    };
                    Some(
                        superseding(pair, object, id.clone(), provenance).unwrap_or_else(|| {
                            Patch::Add {
                                id,
                                content: format!("{SUPERSEDE_FIELD}{pair}"),
                                provenance: provenance.clone(),
                            }
                        }),
                    )
                })
                .collect()
        })
        .collect();
    // A field read as several pairs is several patches: each takes its own
    // position in the fork's emission, in the order they were written, so
    // `(lane, fork, index)` stays a total order.
    if read.len() > fields {
        for (index, patch) in read.iter_mut().enumerate() {
            provenance_of(patch).index = u32::try_from(index).unwrap_or(u32::MAX);
        }
    }
    read
}

/// One `SUPERSEDE` pair (#562): the cited id, then -- after whitespace, a
/// colon, a dash, `→` or `->` -- what replaces it: a live entry's id, which
/// retires the cited entry in favour of it, or the text that should stand,
/// recorded under `id`. `None` when it cites nothing live or names nothing
/// to stand.
fn superseding(
    pair: &str,
    object: &WorkingObject,
    id: crate::object::EntryId,
    provenance: &crate::object::Provenance,
) -> Option<Patch> {
    let quoted = |c: char| "`'\"[]()<>*:,—-".contains(c);
    let (cited, rest) = match pair.split_once('→').or_else(|| pair.split_once("->")) {
        Some((cited, rest)) => (cited.trim(), rest),
        None => pair.split_once(char::is_whitespace).unwrap_or((pair, "")),
    };
    let cited = cited.trim_matches(quoted);
    let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || ":—-".contains(c));
    let rest = rest.trim();
    let live = |id: &crate::object::EntryId| object.live().any(|entry| entry.id == *id);
    let voids = crate::object::EntryId::new(cited)
        .ok()
        .filter(|voids| !rest.is_empty() && live(voids))?;
    let standing = crate::object::EntryId::new(rest.trim_matches(quoted))
        .ok()
        .filter(|standing| *standing != voids && live(standing));
    Some(match standing {
        Some(_) => Patch::Retire {
            target: voids,
            provenance: provenance.clone(),
        },
        None => Patch::Supersede {
            id,
            content: rest.to_owned(),
            voids,
            provenance: provenance.clone(),
        },
    })
}

/// One applied patch as its log line: its op, its entry -- the patch's own
/// content, or for a verdict on an entry that entry's -- and what it voided.
fn patched(fork: Option<u64>, patch: &Patch, object: &WorkingObject) -> Event {
    let held = |id: &crate::object::EntryId| {
        object
            .entry(id)
            .map(|entry| entry.content.clone())
            .unwrap_or_default()
    };
    let (op, id, text, supersedes) = match patch {
        Patch::Add { id, content, .. } => (log::PatchOp::Add, id, content.clone(), None),
        Patch::Supersede {
            id, content, voids, ..
        } => (
            log::PatchOp::Supersede,
            id,
            content.clone(),
            Some(voids.as_str().to_owned()),
        ),
        Patch::Resolve { target, .. } => (log::PatchOp::Resolve, target, held(target), None),
        Patch::Retire { target, .. } => (log::PatchOp::Retire, target, held(target), None),
        Patch::Park { target, .. } => (log::PatchOp::Park, target, held(target), None),
    };
    Event::Patched {
        fork,
        // A fork's patch is named by its fork; the trunk's own by its lane.
        lane: fork.is_none().then(|| patch.provenance().lane.clone()),
        op,
        entry: log::PatchEntry {
            id: id.as_str().to_owned(),
            text,
            category: None,
        },
        supersedes,
        tangent: patch.provenance().tangent.clone(),
    }
}

/// How a failed call settles its turn: a call that ran out of time is a
/// `timeout`, and every other failure is `failed`.
fn settle_reason_of(failure: &TransportFailure) -> SettleReason {
    match failure {
        TransportFailure::Timeout { .. } => SettleReason::Timeout,
        _ => SettleReason::Failed,
    }
}

/// Settle a turn that ended without [`call`] settling it -- or its gap's
/// fork, `failed` (#374).
fn crashed<S>(shared: &Shared<S>, why: String) {
    let mut state = shared.lock();
    if state.settlement == Settlement::Capture {
        crashed_fork(&shared.template, &mut state, why);
        drop(state);
        shared.changed.notify_all();
        return;
    }
    if state.settlement == Settlement::Turn {
        if let Some(flight) = state.flight.take() {
            // The call's own `partial` died with its thread; what it had
            // delivered is in the log, piece by piece.
            let partial: String = state
                .log
                .iter()
                .filter_map(|logged| match &logged.event {
                    Event::Delta { request, text } if *request == flight.request => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect();
            state.push(Event::Crashed {
                request: flight.request,
                partial,
                why,
            });
            state.keep_ran_steps();
            state.push(Event::TurnSettled {
                turn: flight.turn,
                reason: SettleReason::Failed,
            });
        }
        state.move_to(Settlement::Awaiting);
    }
    drop(state);
    shared.changed.notify_all();
}

/// The gap's fork, when its thread died: its call `crashed`, with what it
/// had delivered, and the fork settled `failed` (#374); then the gap is
/// over.
fn crashed_fork(template: &RequestShape, state: &mut State, why: String) {
    if let (Some(flight), Some(fork)) = (state.flight.take(), state.forking.take()) {
        let mut partial = String::new();
        for logged in &state.log {
            if let Event::Delta { request, text } = &logged.event
                && *request == flight.request
            {
                partial.push_str(text);
            }
        }
        state.push(Event::Crashed {
            request: flight.request,
            partial,
            why,
        });
        state.push(Event::ForkSettled {
            fork,
            outcome: log::ForkOutcome::Failed,
            prompt_tokens: None,
            wall_ms: None,
            refused: None,
        });
    }
    turn_over(template, state);
}

/// What a panic said, when it said it as text.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "a panic with no message".to_owned())
}

#[cfg(test)]
pub(in crate::drive) mod tests {
    use super::*;
    use crate::client::shape::{Limits, SamplerCard};
    use crate::client::stream::{Canned, Gate, Step};

    const HEAD: &str = "you are the trunk";

    /// The head digest of the first request a session on [`template`]
    /// sends for `asked`: the template's messages and the ask, through
    /// `client::head`, computed here apart from the session.
    fn first_head(asked: &str) -> String {
        let mut shape = template();
        shape.messages.push(Message::new(Role::User, asked));
        crate::client::head::Head::of(&shape).digest().to_owned()
    }

    pub(in crate::drive) fn template() -> RequestShape {
        RequestShape {
            model: "a-model".to_owned(),
            messages: vec![Message::new(Role::System, HEAD)],
            sampler: SamplerCard::empty(),
            limits: Limits {
                attempt: Duration::from_secs(5),
                call: Duration::from_secs(5),
                max_output_tokens: 64,
                retries: 0,
                context_window: None,
            },
            grammar: None,
            template_kwargs: std::collections::BTreeMap::new(),
            tools: Vec::new(),
        }
    }

    pub(in crate::drive) fn deltas(pieces: &[&str]) -> Vec<Step> {
        pieces
            .iter()
            .map(|piece| Step::Delta((*piece).to_owned()))
            .collect()
    }

    /// Wait until `done` holds of the whole log, or fail -- never hang. A
    /// cancel that does not reach its call would otherwise show up as a test
    /// that never finishes rather than one that fails.
    pub(in crate::drive) fn wait_until<S: Streaming + 'static>(
        session: &Session<S>,
        what: &str,
        done: impl Fn(&[Logged]) -> bool,
    ) -> Vec<Logged> {
        let give_up = Instant::now() + Duration::from_secs(10);
        loop {
            let log = session.events_from(0);
            if done(&log) {
                return log;
            }
            assert!(
                Instant::now() < give_up,
                "gave up waiting for {what}; the log is {log:#?}"
            );
            let _ = session.wait_from(log.len() as u64, Duration::from_millis(200));
        }
    }

    pub(in crate::drive) fn settled(log: &[Logged]) -> bool {
        matches!(
            log.last(),
            Some(Logged {
                event: Event::Settled {
                    to: Settlement::Awaiting,
                    ..
                },
                ..
            })
        )
    }

    fn events(log: &[Logged]) -> Vec<Event> {
        log.iter().map(|logged| logged.event.clone()).collect()
    }

    fn user(text: &str) -> Message {
        Message::new(Role::User, text)
    }

    fn assistant(text: &str) -> Message {
        Message::new(Role::Assistant, text)
    }

    #[test]
    fn an_ask_is_answered_streamed_and_the_answer_is_every_piece_in_order() {
        let session = Session::open(Canned::new([deltas(&["Hel", "lo", "!"])]), template());
        assert_eq!(
            session.ask("say hello", None),
            Ok(Admitted { seq: 1, turn: 1 })
        );
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(
            events(&log[1..]),
            [
                Event::Asked {
                    turn: 1,
                    text: "say hello".to_owned(),
                    scoping: false,
                    files: Vec::new(),
                },
                Event::Settled {
                    from: Settlement::Awaiting,
                    to: Settlement::Turn
                },
                Event::Requested {
                    turn: 1,
                    lane: Lane::Trunk,
                    head_sha256: first_head("say hello"),
                    fork: None,
                    max_tokens: 64,
                },
                Event::Delta {
                    request: 3,
                    text: "Hel".to_owned()
                },
                Event::Delta {
                    request: 3,
                    text: "lo".to_owned()
                },
                Event::Delta {
                    request: 3,
                    text: "!".to_owned()
                },
                Event::Answered {
                    hosted: Hosted::default(),
                    request: 3,
                    text: "Hello!".to_owned(),
                    finish_reason: Some("stop".to_owned()),
                    reasoning: None,
                    timings: None,
                },
                Event::TurnSettled {
                    turn: 1,
                    reason: SettleReason::Final
                },
                Event::Settled {
                    from: Settlement::Turn,
                    to: Settlement::Capture
                },
                Event::Settled {
                    from: Settlement::Capture,
                    to: Settlement::Awaiting
                },
            ]
        );
        assert_eq!(
            session.trunk(),
            [
                Message::new(Role::System, HEAD),
                user("say hello"),
                assistant("Hello!")
            ]
        );
    }

    #[test]
    fn the_trunk_is_appended_never_rebuilt() {
        let canned = Canned::new([deltas(&["one"]), deltas(&["two"]), deltas(&["three"])]);
        let session = Session::open(canned, template());
        for (index, ask) in ["first", "second", "third"].into_iter().enumerate() {
            session
                .ask(ask, None)
                .expect("an ask while awaiting is accepted");
            wait_until(&session, "the turn to settle", |log| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::Answered { .. }))
                    .count()
                    == index + 1
                    && settled(log)
            });
        }
        let sent = session.shared.transport.sent();
        assert_eq!(sent.len(), 3);
        // Every request's messages are the previous request's messages, then
        // the previous answer, then the new ask -- byte for byte, so the
        // prefix a server cached for one turn is the prefix of the next.
        for pair in sent.windows(2) {
            let (before, after) = (&pair[0].messages, &pair[1].messages);
            assert_eq!(
                after[..before.len()],
                before[..],
                "the trunk was rebuilt: a later request does not begin with the earlier one"
            );
            assert_eq!(after.len(), before.len() + 2);
        }
        assert_eq!(
            sent[2].messages,
            [
                Message::new(Role::System, HEAD),
                user("first"),
                assistant("one"),
                user("second"),
                assistant("two"),
                user("third"),
            ]
        );
    }

    #[test]
    fn an_ask_sent_while_a_turn_is_in_flight_is_refused_by_name() {
        let gate = Gate::new();
        let canned = Canned::new([vec![
            Step::Delta("Hel".to_owned()),
            Step::Hold(gate.clone()),
            Step::Delta("lo".to_owned()),
        ]]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        wait_until(&session, "the first piece", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::Delta { .. }))
        });

        assert_eq!(
            session.ask("second", None),
            Err(Rejected::Refused(Refusal::InFlight))
        );
        assert_eq!(session.end(None), Err(Rejected::Refused(Refusal::InFlight)));
        let log = session.events_from(0);
        assert!(
            log.iter().any(|logged| logged.event
                == Event::Refused {
                    command: CommandKind::Ask,
                    because: Refusal::InFlight,
                    during: Settlement::Turn
                }),
            "the refusal is not in the log: {log:#?}"
        );

        gate.open();
        wait_until(&session, "the turn to settle", settled);
        assert_eq!(
            session.trunk(),
            [
                Message::new(Role::System, HEAD),
                user("first"),
                assistant("Hello")
            ],
            "a refused ask reached the trunk"
        );
        assert_eq!(session.shared.transport.sent().len(), 1);
    }

    #[test]
    fn a_cancel_reaches_a_call_blocked_mid_answer_and_keeps_what_it_had_said() {
        // Held, and never opened by this test: only the cancel can wake it.
        let gate = Gate::new();
        let canned = Canned::new([
            vec![
                Step::Delta("Hel".to_owned()),
                Step::Hold(gate.clone()),
                Step::Delta("lo".to_owned()),
            ],
            deltas(&["again"]),
        ]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        // The cancel goes in once the call is BLOCKED, never in the window
        // between its first piece and the gate: there, the call's own flag
        // check would stop it and this test would pass with the stopper
        // broken, on whichever runs happened to land in the window.
        assert!(
            gate.wait_for_a_waiter(Duration::from_secs(10)),
            "the call never reached the gate"
        );

        assert_eq!(session.cancel(1, None), Ok(()));
        let log = wait_until(&session, "the stopped turn to settle", settled);
        let tail = events(&log[log.len() - 4..]);
        assert_eq!(
            tail,
            [
                Event::StopAsked { turn: 1 },
                Event::Cancelled {
                    request: 3,
                    partial: "Hel".to_owned()
                },
                Event::TurnSettled {
                    turn: 1,
                    reason: SettleReason::Cancelled
                },
                Event::Settled {
                    from: Settlement::Turn,
                    to: Settlement::Awaiting
                },
            ]
        );
        // What it had said stays, unmarked, as OpenCode and Qwen Code keep it
        // (#575).
        let kept = [
            Message::new(Role::System, HEAD),
            user("first"),
            Message::new(Role::Assistant, "Hel"),
        ];
        assert_eq!(session.trunk(), kept);

        // The next ask goes out on that prefix.
        session
            .ask("second", None)
            .expect("accepted after a cancel");
        wait_until(&session, "the second turn", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::Answered { .. }))
                && settled(log)
        });
        let mut next = kept.to_vec();
        next.push(user("second"));
        assert_eq!(session.shared.transport.sent()[1].messages, next);
    }

    #[test]
    fn the_output_cap_is_clamped_to_the_room_the_prompt_leaves_by_qwen_codes_arithmetic() {
        // No window, no clamp.
        assert_eq!(clamped(64_000, None, 1_000_000), 64_000);
        // The room: window - prompt - max(10,000, 5% of the window).
        assert_eq!(
            clamped(64_000, Some(32_768), 1_000),
            32_768 - 1_000 - 10_000
        );
        assert_eq!(
            clamped(500_000, Some(400_000), 1_000),
            400_000 - 1_000 - 20_000
        );
        // A ceiling below the room stands.
        assert_eq!(clamped(8_192, Some(160_000), 1_000), 8_192);
        // Floored at min(4,000, window - prompt), and at least 1.
        assert_eq!(clamped(64_000, Some(32_768), 30_000), 2_768);
        assert_eq!(clamped(64_000, Some(32_768), 20_000), 4_000);
        assert_eq!(clamped(64_000, Some(32_768), 40_000), 1);
    }

    #[test]
    fn a_request_is_sized_from_the_pad_until_a_prompt_is_measured_then_from_the_measure() {
        let measured = Timings {
            prompt_n: Some(18),
            cache_n: Some(0),
            predicted_n: Some(2),
            ..Timings::default()
        };
        let canned = Canned::new([
            vec![
                Step::Delta("one".to_owned()),
                Step::Timings(measured.clone()),
            ],
            vec![Step::Delta("two".to_owned()), Step::Timings(measured)],
        ]);
        let mut shape = template();
        shape.limits.max_output_tokens = 50_000;
        shape.limits.context_window = Some(40_000);
        let session = Session::open(canned, shape);
        for (turn, ask) in [(1, "first"), (2, "second")] {
            session.ask(ask, None).expect("accepted");
            wait_until(&session, "the turn to settle", |log| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    >= turn
            });
        }
        let caps: Vec<u32> = session
            .events_from(0)
            .iter()
            .filter_map(|logged| match logged.event {
                Event::Requested { max_tokens, .. } => Some(max_tokens),
                _ => None,
            })
            .collect();
        let sent: Vec<u32> = session
            .shared
            .transport
            .sent()
            .iter()
            .map(|shape| shape.limits.max_output_tokens)
            .collect();
        assert_eq!(
            caps, sent,
            "the log names the cap each request was sent with"
        );
        // The first is wholly estimated: a few tokens plus the 20,000 pad.
        assert!(
            u64::from(caps[0]) < 40_000 - ESTIMATE_PAD - 10_000,
            "{caps:?}"
        );
        // The second starts from the 18 tokens measured, plus what was added.
        assert!(caps[1] > 40_000 - 100 - 10_000, "{caps:?}");
    }

    #[test]
    fn a_failed_call_is_typed_and_leaves_the_trunk_alone() {
        let failure = TransportFailure::Timeout {
            after: Duration::from_secs(5),
        };
        let canned = Canned::new([vec![
            Step::Delta("par".to_owned()),
            Step::Fail(failure.clone()),
        ]]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the failed turn to settle", settled);
        assert!(log.iter().any(|logged| logged.event
            == Event::Failed {
                request: 3,
                failure: failure.clone(),
                partial: "par".to_owned(),
                overflow: None,
            }));
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::Answered { .. })),
            "a failed call was logged as an answer"
        );
        assert_eq!(session.trunk(), [Message::new(Role::System, HEAD)]);
    }

    #[test]
    fn a_turn_the_server_refused_is_logged_as_refused_and_leaves_the_trunk_alone() {
        let canned = Canned::new([vec![
            Step::Delta("par".to_owned()),
            Step::Reject(503, "busy".to_owned()),
        ]]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the refused turn to settle", settled);
        assert!(log.iter().any(|logged| logged.event
            == Event::Rejected {
                request: 3,
                status: 503,
                body: "busy".to_owned(),
                class: None,
                partial: "par".to_owned(),
                overflow: None,
            }));
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::Answered { .. })),
            "a refusal was logged as an answer"
        );
        assert_eq!(session.trunk(), [Message::new(Role::System, HEAD)]);
    }

    /// The session's whole log so far, as the log format reads it.
    fn whole_log<S: Streaming>(session: &Session<S>) -> Vec<log::Line> {
        let document: String = session
            .events_from(0)
            .iter()
            .map(|logged| render(logged) + "\n")
            .collect();
        log::parse(&document).expect("the session's log is a log the format reads")
    }

    #[test]
    fn a_finished_turns_response_carries_the_servers_timings() {
        let measured = Timings {
            prompt_n: Some(18),
            cache_n: Some(160),
            prompt_ms: crate::client::stream::Millis::new("225.217"),
            predicted_n: Some(66),
            predicted_ms: crate::client::stream::Millis::new("557.106"),
            draft_n: Some(72),
            draft_n_accepted: Some(44),
        };
        let canned = Canned::new([vec![
            Step::Delta("ok".to_owned()),
            Step::Timings(measured.clone()),
        ]]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        assert!(
            log.iter().any(|logged| matches!(
                &logged.event,
                Event::Answered { timings: Some(timings), .. } if *timings == measured
            )),
            "the response dropped the server's timings"
        );
        let response = whole_log(&session)
            .into_iter()
            .find_map(|line| match line.event {
                log::Event::Response { timings, .. } => Some(timings),
                _ => None,
            })
            .expect("a response line");
        assert_eq!(
            response,
            Some(log::Timings {
                prompt_n: Some(18),
                cache_n: Some(160),
                prompt_ms: log::Millis::new("225.217"),
                predicted_n: Some(66),
                predicted_ms: log::Millis::new("557.106"),
                draft_n: Some(72),
                draft_n_accepted: Some(44),
            }),
            "every key, as the server sent it"
        );
    }

    #[test]
    fn the_responses_reasoning_is_its_deltas_byte_for_byte() {
        let canned = Canned::new([vec![
            Step::Reasoning("weighing\n".to_owned()),
            Step::Reasoning(" it up \n".to_owned()),
            Step::Delta("ok".to_owned()),
        ]]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let whole = "weighing\n it up \n".to_owned();
        assert!(
            log.iter().any(|logged| matches!(
                &logged.event,
                Event::Answered { reasoning: Some(reasoning), .. } if *reasoning == whole
            )),
            "the response's reasoning is not its deltas, untrimmed"
        );
        assert_eq!(
            session.trunk().last().and_then(|m| m.reasoning.clone()),
            Some(whole),
            "and it is the trunk's"
        );
    }

    #[test]
    fn progress_is_logged_before_the_first_delta_and_never_after_the_end() {
        let frame = |processed| Progress {
            total: 9276,
            cache: 0,
            processed,
            time_ms: processed / 2,
        };
        let canned = Canned::new([vec![
            Step::Progress(frame(1066)),
            Step::Progress(frame(9276)),
            Step::Delta("ok".to_owned()),
        ]]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        let lines = whole_log(&session);
        let at = |kind: fn(&log::Event) -> bool| -> Vec<usize> {
            lines
                .iter()
                .enumerate()
                .filter(|(_, line)| kind(&line.event))
                .map(|(index, _)| index)
                .collect()
        };
        let progress = at(|e| matches!(e, log::Event::Progress { .. }));
        let first_delta = at(|e| matches!(e, log::Event::Delta { .. }))[0];
        let response = at(|e| matches!(e, log::Event::Response { .. }))[0];
        assert_eq!(progress.len(), 2, "one line per frame: {lines:?}");
        assert!(
            progress
                .iter()
                .all(|index| *index < first_delta && *index < response),
            "a progress line after the answer began: {lines:?}"
        );
    }

    #[test]
    fn an_overflow_is_request_failed_context_overflow_and_the_turn_failed() {
        let overflow = r#"{"error":{"code":400,"message":"request (262149 tokens) exceeds the available context size (262144 tokens), try increasing it","type":"exceed_context_size_error","n_prompt_tokens":262149,"n_ctx":262144}}"#;
        let canned = Canned::new([vec![Step::Reject(400, overflow.to_owned())]]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        let lines = whole_log(&session);
        assert!(
            lines.iter().any(|line| matches!(
                line.event,
                log::Event::RequestFailed {
                    reason: log::FailReason::ContextOverflow,
                    status: Some(400),
                    ..
                }
            )),
            "the overflow was not `context_overflow`: {lines:?}"
        );
        assert!(lines.iter().any(|line| matches!(
            line.event,
            log::Event::TurnSettled {
                reason: log::SettleReason::Failed,
                ..
            }
        )));
        assert_eq!(session.trunk(), [Message::new(Role::System, HEAD)]);
    }

    /// The `request.failed` line of a session over a `window`-token
    /// context whose only call ends as `step`.
    fn failed_over(window: u64, step: Step) -> log::Event {
        let mut shape = template();
        shape.limits.context_window = Some(window);
        let session = Session::open(Canned::new([vec![step]]), shape);
        session.ask("first", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        whole_log(&session)
            .into_iter()
            .find_map(|line| {
                matches!(line.event, log::Event::RequestFailed { .. }).then_some(line.event)
            })
            .expect("a failed call")
    }

    /// #616: a refusal that names no kind, or a dropped connection, whose
    /// prompt and output cap reached into the window's margin is a context
    /// overflow told from the sizes -- `inferred`, with the prompt as sized
    /// and the window; the same failure with room to spare stays `server`
    /// or `transport`, and a timeout is the clock's whatever the size. A
    /// first call's prompt is wholly estimated: the estimate plus
    /// [`ESTIMATE_PAD`], so over 30,000 tokens it is at the edge, and over
    /// 1,000,000 it is not.
    #[test]
    fn a_failure_at_the_windows_edge_is_an_overflow_told_from_its_size() {
        let aborted = || Step::Reject(500, "Chat completion aborted.".to_owned());
        let log::Event::RequestFailed {
            reason, overflow, ..
        } = failed_over(30_000, aborted())
        else {
            unreachable!()
        };
        assert_eq!(reason, log::FailReason::ContextOverflow);
        let overflow = overflow.expect("its sizes");
        assert!(overflow.inferred);
        assert_eq!(overflow.window, 30_000);
        assert!(overflow.prompt_tokens > ESTIMATE_PAD, "{overflow:?}");
        let log::Event::RequestFailed {
            reason, overflow, ..
        } = failed_over(1_000_000, aborted())
        else {
            unreachable!()
        };
        assert_eq!((reason, overflow), (log::FailReason::Server, None));
        let dropped = || Step::Fail(TransportFailure::Connect("reset".to_owned()));
        assert!(matches!(
            failed_over(30_000, dropped()),
            log::Event::RequestFailed {
                reason: log::FailReason::ContextOverflow,
                overflow: Some(log::Overflow { inferred: true, .. }),
                ..
            }
        ));
        assert!(matches!(
            failed_over(1_000_000, dropped()),
            log::Event::RequestFailed {
                reason: log::FailReason::Transport,
                overflow: None,
                ..
            }
        ));
        assert!(matches!(
            failed_over(
                30_000,
                Step::Fail(TransportFailure::Timeout {
                    after: Duration::from_secs(1)
                })
            ),
            log::Event::RequestFailed {
                reason: log::FailReason::Timeout,
                overflow: None,
                ..
            }
        ));
    }

    /// #616: the engine's typed refusal is an overflow whatever the size,
    /// and with the window known it carries the sizes, not `inferred`.
    #[test]
    fn an_engines_typed_overflow_carries_the_sizes_it_was_sent_at() {
        let typed = r#"{"error":{"code":400,"type":"exceed_context_size_error"}}"#;
        let log::Event::RequestFailed {
            reason, overflow, ..
        } = failed_over(1_000_000, Step::Reject(400, typed.to_owned()))
        else {
            unreachable!()
        };
        assert_eq!(reason, log::FailReason::ContextOverflow);
        let overflow = overflow.expect("its sizes");
        assert!(!overflow.inferred);
        assert_eq!(overflow.window, 1_000_000);
    }

    /// #157: two scoping forks with a turn between them, under `view`;
    /// the prefix rows the record writes at the forks' requests.
    fn fork_prefix_rows(view: Option<ForkView>) -> Vec<crate::formats::record::Event> {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.view = view;
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&["a turn between them"]),
                deltas(&[SCOPED]),
                deltas(&["DECISION: NONE\nPLAN: NONE\n"]),
            ]),
            template(),
            None,
            None,
            None,
            Some(interview),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "fork one", |log| {
            settled(log) && fork_outcomes(log).len() == 1
        });
        session.ask("go on", None).expect("accepted");
        wait_until(&session, "turn two", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        session
            .ask_marked("and who is it for?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "fork two", |log| {
            settled(log) && fork_outcomes(log).len() == 2
        });
        let forks: Vec<String> = log
            .iter()
            .filter(|logged| {
                matches!(
                    logged.event,
                    Event::Requested {
                        lane: Lane::Interview,
                        ..
                    }
                )
            })
            .map(|logged| format!("q/{}", logged.seq))
            .collect();
        assert_eq!(forks.len(), 2);
        let lines: Vec<log::Line> = session.events_from(0).iter().map(line_of).collect();
        super::super::projection::project(&lines, &regime(), None)
            .expect("the log projects")
            .events
            .into_iter()
            .filter(|event| matches!(
                event,
                crate::formats::record::Event::PrefixChanged { at_request, .. } if forks.contains(at_request)
            ))
            .collect()
    }

    /// #157, from the seam smoke run (q/2142): a whole-trunk fork's head is
    /// the trunk's, so the second fork's request -- its head moved by the
    /// turns between -- is verified against the rebuilt trunk and writes no
    /// prefix row; a `last_turn` fork's head is not rebuilt, and its move is
    /// still named unattributed.
    #[test]
    fn a_whole_trunk_forks_head_is_verified_against_the_trunk_and_writes_no_row() {
        assert_eq!(fork_prefix_rows(None), []);
        let rows = fork_prefix_rows(Some(ForkView::Last(1)));
        assert!(
            matches!(
                &rows[..],
                [crate::formats::record::Event::PrefixChanged {
                    reason: crate::formats::record::PrefixReason::Unattributed,
                    ..
                }]
            ),
            "{rows:?}"
        );
    /// An object holding the live entries `ids`, each `<id>: text`.
    fn object_of(ids: &[&str]) -> WorkingObject {
        let mut object = WorkingObject::open(regime());
        let provenance = crate::object::Provenance {
            turn: 1,
            lane: "interview".to_owned(),
            fork: None,
            tangent: None,
            index: 0,
        };
        object
            .apply_turn(
                &ids.iter()
                    .zip(0..)
                    .map(|(id, index)| Patch::Add {
                        id: crate::object::EntryId::new(id).expect("an id"),
                        content: format!("{id}: text"),
                        provenance: crate::object::Provenance {
                            index,
                            ..provenance.clone()
                        },
                    })
                    .collect::<Vec<_>>(),
            )
            .expect("applied");
        object
    }

    /// A fork's `SUPERSEDE` field as the fold makes it, under `id`.
    fn supersede_field(id: &str, text: &str) -> Patch {
        Patch::Add {
            id: crate::object::EntryId::new(id).expect("an id"),
            content: format!("{SUPERSEDE_FIELD}{text}"),
            provenance: crate::object::Provenance {
                turn: 2,
                lane: "interview".to_owned(),
                fork: Some("f/9".to_owned()),
                tangent: None,
                index: 0,
            },
        }
    }

    /// #562: `;` separates pairs, and each is its own supersession: the
    /// second pair is not lost into the first's text.
    #[test]
    fn a_supersede_field_of_two_pairs_supersedes_twice() {
        let object = object_of(&["interview-t2-0", "interview-t2-1"]);
        let patches = cited(
            vec![supersede_field(
                "interview-t3-0",
                "interview-t2-0 a tracker for two teams; interview-t2-1 no login after all",
            )],
            &object,
        );
        let read: Vec<(String, String)> = patches
            .iter()
            .map(|patch| match patch {
                Patch::Supersede { content, voids, .. } => {
                    (voids.as_str().to_owned(), content.clone())
                }
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            read,
            [
                (
                    "interview-t2-0".to_owned(),
                    "a tracker for two teams".to_owned()
                ),
                ("interview-t2-1".to_owned(), "no login after all".to_owned()),
            ]
        );
        // Each its own position, so the two apply together.
        let mut object = object;
        object.apply_turn(&patches).expect("both apply");
    }

    /// #562: `→` or `->` between the cited id and what replaces it.
    #[test]
    fn a_supersede_pair_may_point_with_an_arrow() {
        let object = object_of(&["interview-t2-0"]);
        for arrow in ["→", "->"] {
            let patches = cited(
                vec![supersede_field(
                    "interview-t3-0",
                    &format!("interview-t2-0 {arrow} a tracker for two teams"),
                )],
                &object,
            );
            assert!(
                matches!(
                    &patches[..],
                    [Patch::Supersede { content, voids, .. }]
                        if content == "a tracker for two teams" && voids.as_str() == "interview-t2-0"
                ),
                "{arrow}: {patches:?}"
            );
        }
    }

    /// #562, from the seam smoke run: a pair whose replacement is itself a
    /// live entry's id retires the cited entry in favour of it, writing no
    /// new text -- the id never becomes an entry's text.
    #[test]
    fn a_supersede_pair_naming_a_live_entry_retires_the_cited_one() {
        let object = object_of(&[
            "interview-t2-0",
            "interview-t2-1",
            "r1516/call-a/decision",
            "r1516/call-b/followup",
        ]);
        let patches = cited(
            vec![supersede_field(
                "interview-t3-0",
                "interview-t2-0 → r1516/call-a/decision; interview-t2-1 → r1516/call-b/followup",
            )],
            &object,
        );
        let retired: Vec<&str> = patches
            .iter()
            .map(|patch| match patch {
                Patch::Retire { target, .. } => target.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(retired, ["interview-t2-0", "interview-t2-1"]);
    }

    /// A transport that panics mid-call: the one thing a turn's thread can
    /// do that no `Ended` or `TransportFailure` describes.
    struct Panics;

    impl Streaming for Panics {
        fn stream(
            &self,
            _shape: &RequestShape,
            _deadline: Instant,
            _cancel: &Cancel,
            on_delta: &mut dyn FnMut(Piece<'_>),
        ) -> Result<StreamEnded, TransportFailure> {
            on_delta(Piece::Text("half"));
            panic!("a transport that panics mid-answer (seeded by this test)");
        }

        fn describes(&self) -> String {
            "a transport that panics".to_owned()
        }
    }

    #[test]
    fn a_turn_whose_thread_panics_still_settles_and_the_session_goes_on() {
        let session = Session::open(Panics, template());
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the crashed turn to settle", settled);
        let tail = events(&log[log.len() - 3..]);
        assert_eq!(
            tail,
            [
                Event::Crashed {
                    request: 3,
                    partial: "half".to_owned(),
                    why: "a transport that panics mid-answer (seeded by this test)".to_owned()
                },
                Event::TurnSettled {
                    turn: 1,
                    reason: SettleReason::Failed
                },
                Event::Settled {
                    from: Settlement::Turn,
                    to: Settlement::Awaiting
                },
            ],
            "the crash was not settled, or its reason was lost"
        );
        assert_eq!(session.trunk(), [Message::new(Role::System, HEAD)]);
        assert!(
            session.ask("again", None).is_ok(),
            "a crashed turn left the session refusing asks"
        );
    }

    /// #555: an answer over Anthropic's Messages API goes onto the trunk
    /// with its thinking's signature, to go back verbatim, and its response
    /// line carries the signature and the usage: the prompt in all, the
    /// cache's reads, and its writes by lifetime. The log reads back.
    #[test]
    fn a_hosted_answer_keeps_its_signature_and_logs_its_cache_counts() {
        use crate::client::anthropic::tests::{a_signed_answer, streamed};
        use crate::client::stream::HttpStream;
        use crate::client::stub::Stub;
        use crate::client::transport::Endpoint;

        let stub = Stub::serving(vec![streamed(&a_signed_answer())]).expect("loopback");
        let transport = HttpStream::new(Endpoint::parse(&stub.url()).expect("an endpoint"))
            .with_anthropic(crate::client::anthropic::Options::default());
        let session = Session::open(transport, template());
        session.ask("finish it", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        let trunk = session.trunk();
        let answer = trunk.last().expect("the answer");
        assert_eq!(
            (
                answer.content.as_str(),
                answer.reasoning.as_deref(),
                answer.reasoning_signature.as_deref()
            ),
            ("Done.", Some("brief"), Some("sig-xyz"))
        );
        let lines = whole_log(&session);
        let response = lines
            .iter()
            .find_map(|line| match &line.event {
                log::Event::Response {
                    usage,
                    reasoning_signature,
                    timings,
                    ..
                } => Some((usage.clone(), reasoning_signature.clone(), timings.clone())),
                _ => None,
            })
            .expect("a response line");
        assert_eq!(
            response,
            (
                Some(log::Usage {
                    prompt_tokens: 9 + 2048 + 64,
                    completion_tokens: 30,
                    cached_tokens: Some(2048),
                    cache_creation_tokens: Some(64),
                    cache_creation_5m_tokens: Some(64),
                    cache_creation_1h_tokens: Some(0),
                }),
                Some("sig-xyz".to_owned()),
                None
            )
        );
        reads_whole(&session);
    }

    #[test]
    fn a_session_over_http_cancels_and_the_server_sees_the_caller_leave() {
        use crate::client::stream::HttpStream;
        use crate::client::stub::{Act, Held, Stub};
        use crate::client::transport::Endpoint;

        let piece =
            r#"data: {"choices":[{"index":0,"delta":{"content":"Hel"},"finish_reason":null}]}"#;
        let stub = Stub::serving(vec![Act::StreamThenHold(vec![format!("{piece}\n\n")])])
            .expect("loopback");
        let transport =
            HttpStream::new(Endpoint::parse(&stub.url()).expect("the stub's URL is an endpoint"));
        // A call limit far past every bound below, so nothing but the stopper
        // can end the read in time: at 5 s the deadline could, and the test
        // passed with the stopper broken whenever the clock overshot by less
        // than the stub's head start (#120's second review).
        let mut template = template();
        template.limits.call = Duration::from_secs(30);
        template.limits.attempt = Duration::from_secs(30);
        let session = Session::open(transport, template);
        session.ask("first", None).expect("accepted");
        wait_until(&session, "the first piece", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::Delta { .. }))
        });
        // The server is holding the connection open and sending nothing: the
        // turn's thread is blocked in a read that only the stopper can end.
        assert_eq!(session.cancel(1, None), Ok(()));
        let log = wait_until(&session, "the stopped turn to settle", settled);
        assert!(
            log.iter().any(|logged| logged.event
                == Event::Cancelled {
                    request: 3,
                    partial: "Hel".to_owned()
                }),
            "{log:#?}"
        );
        let give_up = Instant::now() + Duration::from_secs(10);
        let held = loop {
            if let Some(held) = stub.hangups().first().copied() {
                break held;
            }
            assert!(Instant::now() < give_up, "the stub recorded nothing");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            matches!(held, Held::HungUp(after) if after < Duration::from_secs(5)),
            "{held:?}"
        );
    }

    #[test]
    fn a_thinking_turns_reasoning_joins_the_trunk_and_goes_back_byte_identical() {
        use crate::client::stream::HttpStream;
        use crate::client::stub::{Act, Stub};
        use crate::client::transport::Endpoint;

        // The drive endpoint's own reply to a thinking turn (#117 Q10,
        // measured by track four): re-sent, its reasoning kept 400 of 420
        // prompt tokens warm; dropped, the prompt diverged at this turn.
        //
        // This pins the CLIENT's half of that prefix: what streamed goes back
        // byte for byte. The server's half -- that those bytes re-render to
        // the same 400 tokens -- is a measurement, not a test, and has no
        // fault here:
        // unseedable: the server-side 400-token prefix identity needs the model and its tokenizer; track four measured it on capture b91695d8 (#117 comment 5864110723)
        let capture =
            include_bytes!("../../client/fixtures/llama-server-e7051ef-reasoning-stream.http");
        let stub = Stub::serving(vec![Act::Raw(capture.to_vec()), Act::Raw(capture.to_vec())])
            .expect("loopback");
        let transport =
            HttpStream::new(Endpoint::parse(&stub.url()).expect("the stub's URL is an endpoint"));
        let session = Session::open(transport, template());
        for (turn, ask) in [(1_usize, "how long?"), (2, "and back?")] {
            session.ask(ask, None).expect("accepted");
            wait_until(&session, "the turn to settle", |log| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == turn
                    && settled(log)
            });
        }
        let log = session.events_from(0);
        let first_request = 3;
        let streamed = |reasoning: bool| -> String {
            log.iter()
                .filter_map(|logged| match &logged.event {
                    Event::Reasoning { request, text }
                        if reasoning && *request == first_request =>
                    {
                        Some(text.as_str())
                    }
                    Event::Delta { request, text } if !reasoning && *request == first_request => {
                        Some(text.as_str())
                    }
                    _ => None,
                })
                .collect()
        };
        drop(session);

        let bodies: Vec<serde_json::Value> = stub
            .received()
            .iter()
            .map(|body| serde_json::from_str(body).expect("a request body is JSON"))
            .collect();
        assert_eq!(bodies.len(), 2);
        let first = bodies[0]["messages"].as_array().expect("messages");
        let second = bodies[1]["messages"].as_array().expect("messages");
        // Appended, never rebuilt: the second request begins with the first.
        assert_eq!(second[..first.len()], first[..]);
        let answered = &second[first.len()];
        assert_eq!(answered["role"], serde_json::json!("assistant"));
        assert_eq!(
            answered["reasoning_content"],
            serde_json::json!(streamed(true)),
            "the reasoning went back changed, or not at all"
        );
        assert_eq!(answered["content"], serde_json::json!(streamed(false)));
        assert!(streamed(true).ends_with('\n') && !streamed(false).is_empty());
    }

    #[test]
    fn a_command_with_nothing_to_act_on_is_refused_and_logged() {
        let session = Session::open(Canned::new([deltas(&["done"])]), template());
        // A cancel for a turn that was never admitted is not a command the
        // session can refuse: it says so, and logs nothing.
        assert_eq!(session.cancel(1, None), Err(Rejected::NoSuchTurn(1)));
        session.ask("one", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        assert_eq!(
            session.cancel(1, None),
            Err(Rejected::Refused(Refusal::NothingInFlight))
        );
        assert_eq!(session.cancel(0, None), Err(Rejected::NoSuchTurn(0)));
        assert_eq!(session.cancel(2, None), Err(Rejected::NoSuchTurn(2)));
        assert_eq!(
            session.declare_seam(None),
            Err(Rejected::Refused(Refusal::NothingToSeam))
        );
        assert_eq!(session.end(None), Ok(()));
        assert_eq!(session.settlement(), Settlement::Ended);
        assert_eq!(
            session.ask("too late", None),
            Err(Rejected::Refused(Refusal::Ended))
        );
        assert_eq!(
            session.cancel(1, None),
            Err(Rejected::Refused(Refusal::Ended))
        );
        assert_eq!(
            session.declare_seam(None),
            Err(Rejected::Refused(Refusal::Ended))
        );
        assert_eq!(session.end(None), Err(Rejected::Refused(Refusal::Ended)));
        let refusals: Vec<(CommandKind, Refusal, Settlement)> = session
            .events_from(0)
            .into_iter()
            .filter_map(|logged| match logged.event {
                Event::Refused {
                    command,
                    because,
                    during,
                } => Some((command, because, during)),
                _ => None,
            })
            .collect();
        assert_eq!(
            refusals,
            [
                (
                    CommandKind::Cancel,
                    Refusal::NothingInFlight,
                    Settlement::Awaiting
                ),
                (
                    CommandKind::DeclareSeam,
                    Refusal::NothingToSeam,
                    Settlement::Awaiting
                ),
            ]
        );
        // Every command after `end` was refused, and none was logged:
        // `ended` is the last line (#291).
        assert!(
            matches!(
                session.events_from(0).last().map(|logged| &logged.event),
                Some(Event::Settled {
                    to: Settlement::Ended,
                    ..
                })
            ),
            "{:?}",
            session.events_from(0).last()
        );
        assert_eq!(
            session.shared.transport.sent().len(),
            1,
            "a refused ask reached the transport"
        );
    }

    #[test]
    fn the_log_is_numbered_by_position_and_a_reader_can_resume_from_any_point() {
        let session = Session::open(Canned::new([deltas(&["a", "b"])]), template());
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        for (index, logged) in log.iter().enumerate() {
            assert_eq!(logged.seq, index as u64);
        }
        assert_eq!(session.events_from(3), log[3..]);
        assert!(session.events_from(log.len() as u64).is_empty());
        assert!(session.events_from(u64::MAX).is_empty());
    }

    #[test]
    fn a_cancel_naming_a_turn_that_already_settled_does_not_stop_the_next_one() {
        let gate = Gate::new();
        let canned = Canned::new([
            deltas(&["one"]),
            vec![
                Step::Delta("Hel".to_owned()),
                Step::Hold(gate.clone()),
                Step::Delta("lo".to_owned()),
            ],
        ]);
        let session = Session::open(canned, template());
        session.ask("first", None).expect("accepted");
        wait_until(&session, "the first turn to settle", settled);
        assert_eq!(
            session.ask("second", None).map(|admitted| admitted.turn),
            Ok(2)
        );
        assert!(
            gate.wait_for_a_waiter(Duration::from_secs(10)),
            "the second call never reached the gate"
        );

        // The stop meant for turn 1, arriving late, while turn 2 is in flight.
        assert_eq!(
            session.cancel(1, None),
            Err(Rejected::Refused(Refusal::Stale))
        );
        gate.open();
        let log = wait_until(&session, "the second turn to settle", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
                && settled(log)
        });
        assert!(
            log.iter().any(|logged| logged.event
                == Event::Refused {
                    command: CommandKind::Cancel,
                    because: Refusal::Stale,
                    during: Settlement::Turn
                }),
            "the stale cancel is not in the log: {log:#?}"
        );
        assert!(
            log.iter().any(|logged| logged.event
                == Event::TurnSettled {
                    turn: 2,
                    reason: SettleReason::Final
                }),
            "a stale cancel stopped the next turn: {log:#?}"
        );
        assert_eq!(session.trunk().last(), Some(&assistant("Hello")));
    }

    #[test]
    fn every_way_a_turn_ends_settles_it_once_with_its_reason() {
        let gate = Gate::new();
        let canned = Canned::new([
            deltas(&["done"]),
            vec![Step::Delta("par".to_owned()), Step::Hold(gate.clone())],
            vec![Step::Fail(TransportFailure::Timeout {
                after: Duration::from_secs(5),
            })],
            vec![Step::Fail(TransportFailure::Connect("refused".to_owned()))],
            vec![Step::Reject(503, "busy".to_owned())],
        ]);
        let session = Session::open(canned, template());
        let expected = [
            SettleReason::Final,
            SettleReason::Cancelled,
            SettleReason::Timeout,
            SettleReason::Failed,
            SettleReason::Failed,
        ];
        for turn in 1..=5u32 {
            let admitted = session.ask("go", None).expect("accepted while awaiting");
            assert_eq!(admitted.turn, turn);
            if turn == 2 {
                assert!(
                    gate.wait_for_a_waiter(Duration::from_secs(10)),
                    "the call never reached the gate"
                );
                assert_eq!(session.cancel(2, None), Ok(()));
            }
            wait_until(&session, "the turn to settle", |log| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == turn as usize
                    && settled(log)
            });
        }
        let log = session.events_from(0);

        // A crashed thread settles its turn too, through the other door.
        let crashing = Session::open(Panics, template());
        crashing.ask("go", None).expect("accepted");
        let crashed_log = wait_until(&crashing, "the crashed turn to settle", settled);

        for (turn, reason) in (1..=5u32).zip(expected) {
            assert_settled_once(&log, turn, reason);
        }
        assert_settled_once(&crashed_log, 1, SettleReason::Failed);
    }

    /// Exactly one `TurnSettled` for `turn`, with `reason`, straight after
    /// the call's terminal event and straight before the settlement leaves
    /// `turn`.
    fn assert_settled_once(log: &[Logged], turn: u32, reason: SettleReason) {
        let at: Vec<usize> = log
            .iter()
            .enumerate()
            .filter(|(_, logged)| {
                matches!(logged.event, Event::TurnSettled { turn: settled, .. } if settled == turn)
            })
            .map(|(index, _)| index)
            .collect();
        assert_eq!(
            at.len(),
            1,
            "turn {turn} settled {} times: {log:#?}",
            at.len()
        );
        let at = at[0];
        assert_eq!(
            log[at].event,
            Event::TurnSettled { turn, reason },
            "turn {turn} settled for the wrong reason"
        );
        assert!(
            matches!(
                log[at - 1].event,
                Event::Answered { .. }
                    | Event::Cancelled { .. }
                    | Event::Rejected { .. }
                    | Event::Failed { .. }
                    | Event::Crashed { .. }
            ),
            "turn {turn}'s settling does not follow its terminal event: {log:#?}"
        );
        assert!(
            matches!(
                log[at + 1].event,
                Event::Settled {
                    from: Settlement::Turn,
                    ..
                }
            ),
            "turn {turn}'s settling is not followed by the settlement leaving `turn`"
        );
    }

    #[test]
    fn every_call_is_a_request_and_what_it_produced_names_it() {
        let session = Session::open(Canned::new([deltas(&["a", "b"])]), template());
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let requests: Vec<u64> = log
            .iter()
            .filter(|logged| {
                matches!(
                    logged.event,
                    Event::Requested {
                        turn: 1,
                        lane: Lane::Trunk,
                        ..
                    }
                )
            })
            .map(|logged| logged.seq)
            .collect();
        assert_eq!(requests.len(), 1, "one call, one request: {log:#?}");
        let request = requests[0];
        let cited: Vec<u64> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::Delta { request, .. } | Event::Answered { request, .. } => Some(*request),
                _ => None,
            })
            .collect();
        assert_eq!(cited, [request, request, request], "{log:#?}");
    }

    #[test]
    fn the_log_begins_with_the_session_it_describes() {
        let unix_ms = || {
            u64::try_from(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .expect("after the epoch")
                    .as_millis(),
            )
            .expect("fits")
        };
        let before = unix_ms();
        let session = Session::open(Canned::new([]), template());
        let after = unix_ms();
        let log = session.events_from(0);
        let Event::Started {
            opened,
            model,
            head,
            serving: None,
            claim: None,
            tools: _,
            template_kwargs: _,
            unsent: None,
            approvals_off: false,
            fork_delivery: None,
            reasoning_effort_default: None,
            tool_output: None,
            phases: _,
            instruction_files: _,
            levers: None,
            ..
        } = &log[0].event
        else {
            panic!("the log does not begin with the session: {log:#?}");
        };
        assert_eq!(log[0].seq, 0);
        assert!(
            (before..=after).contains(opened),
            "opened {opened} is not between {before} and {after}"
        );
        assert_eq!(model, "a-model");
        assert_eq!(head, &[Message::new(Role::System, HEAD)]);
    }

    #[test]
    fn each_event_is_stamped_when_it_was_logged() {
        let gate = Gate::new();
        let canned = Canned::new([vec![
            Step::Delta("a".to_owned()),
            Step::Hold(gate.clone()),
            Step::Delta("b".to_owned()),
        ]]);
        let session = Session::open(canned, template());
        session.ask("go", None).expect("accepted");
        assert!(
            gate.wait_for_a_waiter(Duration::from_secs(10)),
            "the call never reached the gate"
        );
        // Held for at least 50 ms. The assertion below is a lower bound
        // only, so a slow machine cannot make it flake.
        std::thread::sleep(Duration::from_millis(60));
        gate.open();
        let log = wait_until(&session, "the turn to settle", settled);
        for pair in log.windows(2) {
            assert!(pair[0].t <= pair[1].t, "t went backwards: {pair:#?}");
        }
        let stamp = |text: &str| {
            log.iter()
                .find(|logged| matches!(&logged.event, Event::Delta { text: piece, .. } if piece == text))
                .map(|logged| logged.t)
                .expect("the piece is in the log")
        };
        assert!(
            stamp("b") - stamp("a") >= 50,
            "a 60 ms hold shows as {} ms: {log:#?}",
            stamp("b") - stamp("a")
        );
    }

    /// A substrate claim, as `serve` passes one in (#292).
    fn claimed() -> log::SubstrateClaim {
        log::SubstrateClaim {
            substrate: "a-substrate".to_owned(),
            registry_sha256: "ab".repeat(32),
            engine: log::ClaimedEngine::Served(vec![log::ServedField {
                field: "engine_commit".to_owned(),
                value: "0123abc".repeat(5) + "01234",
                provenance: log::FieldProvenance::Corroborated,
                reported: Some("b1-0123abc".to_owned()),
            }]),
        }
    }

    /// A PNG the operator attached, as the `ask` line names it (#372).
    fn attached_file() -> log::RecordedFile {
        log::RecordedFile {
            path: format!("files/{}", "0".repeat(64)),
            sha256: "0".repeat(64),
            media_type: "image/png".to_owned(),
            bytes: 8,
        }
    }

    /// One of every event, and a match with no wildcard that names each
    /// variant: an event added to [`Event`] fails to compile here until it
    /// has a sample, and so a line. One entry per variant is its length.
    #[allow(clippy::too_many_lines)]
    fn one_of_every_event() -> Vec<Logged> {
        let events = vec![
            Event::Started {
                bash_timeout_ms: None,
                opened: 1_790_000_000_000,
                model: "a-model".to_owned(),
                head: vec![Message::new(Role::System, HEAD)],
                serving: Some(Serving {
                    concurrency: Concurrency::Declared(2),
                    dialect: crate::client::shape::Dialect::llama_cpp(),
                }),
                claim: Some(claimed()),
                tools: Vec::new(),
                template_kwargs: BTreeMap::from([
                    ("enable_thinking".to_owned(), Value::Boolean(true)),
                    (
                        "reasoning_effort".to_owned(),
                        Value::String("medium".to_owned()),
                    ),
                ]),
                unsent: Some(log::Unsent { budget_tokens: 512 }),
                approvals_off: true,
                fork_delivery: Some(log::ForkDelivery::Advisory),
                reasoning_effort_default: Some("xhigh".to_owned()),
                tool_output: Some(crate::drive::output::OutputCap::DEFAULT),
                phases: None,
                instruction_files: vec![log::InstructionFile {
                    path: "AGENTS.md".to_owned(),
                    sha256: "e".repeat(64),
                }],
                levers: Some(BTreeMap::from([(
                    "fork-asks".to_owned(),
                    "v3".to_owned(),
                )])),
                fork_asks: Some(&crate::dogma::asks::V4),
            },
            Event::Asked {
                turn: 1,
                text: "say \"hi\"\n".to_owned(),
                scoping: true,
                files: vec![attached_file()],
            },
            Event::Settled {
                from: Settlement::Awaiting,
                to: Settlement::Turn,
            },
            Event::Requested {
                turn: 1,
                lane: Lane::Trunk,
                head_sha256: "a".repeat(64),
                fork: None,
                max_tokens: 8192,
            },
            Event::Refused {
                command: CommandKind::Cancel,
                because: Refusal::Stale,
                during: Settlement::Turn,
            },
            Event::Progress {
                request: 3,
                progress: Progress {
                    total: 9276,
                    cache: 0,
                    processed: 1066,
                    time_ms: 721,
                },
            },
            Event::Reasoning {
                request: 3,
                text: "thinking\n".to_owned(),
            },
            Event::Delta {
                request: 3,
                text: "Hel".to_owned(),
            },
            Event::StopAsked { turn: 1 },
            Event::Answered {
                hosted: Hosted::default(),
                request: 3,
                text: "Hello".to_owned(),
                finish_reason: Some("stop".to_owned()),
                reasoning: Some("thinking\n".to_owned()),
                timings: Some(Timings {
                    prompt_n: Some(89),
                    cache_n: Some(0),
                    prompt_ms: crate::client::stream::Millis::new("297.198"),
                    predicted_n: Some(312),
                    predicted_ms: crate::client::stream::Millis::new("2591"),
                    draft_n: Some(312),
                    draft_n_accepted: Some(207),
                }),
            },
            Event::Cancelled {
                request: 3,
                partial: "Hel".to_owned(),
            },
            Event::Capped {
                hosted: Hosted::default(),
                request: 3,
                text: String::new(),
                finish_reason: Some("length".to_owned()),
                reasoning: Some("thinking, cut".to_owned()),
                timings: None,
            },
            Event::Rejected {
                request: 3,
                status: 503,
                body: "busy".to_owned(),
                class: None,
                partial: "Hel".to_owned(),
                overflow: None,
            },
            Event::Rejected {
                request: 3,
                status: 400,
                body: "too long".to_owned(),
                class: Some(Rejection::ContextOverflow),
                partial: String::new(),
                overflow: None,
            },
            Event::Crashed {
                request: 3,
                partial: String::new(),
                why: "a panic".to_owned(),
            },
            Event::Failed {
                request: 3,
                failure: TransportFailure::Timeout {
                    after: Duration::from_secs(5),
                },
                partial: "Hel".to_owned(),
                overflow: None,
            },
            Event::TurnSettled {
                turn: 1,
                reason: SettleReason::MaxSteps,
            },
            Event::IdleGap(IdleGap {
                opened_by: 13,
                notice: 120,
                read: 2_400,
                compose: 3_100,
                away: 0,
                blocked: 50,
                ended_by: GapEnd::Ask,
            }),
            Event::ToolCallPiece {
                request: 3,
                index: 0,
                id: Some("call-a".to_owned()),
                name: Some("bash".to_owned()),
                arguments: "{\"command\":".to_owned(),
            },
            Event::Called {
                hosted: Hosted::default(),
                request: 3,
                text: String::new(),
                finish_reason: Some("tool_calls".to_owned()),
                reasoning: None,
                timings: None,
            },
            Event::ToolCalled(Box::new(ToolLine {
                request: 3,
                turn: 1,
                id: "call-a".to_owned(),
                name: "bash".to_owned(),
                arguments: "{\"command\":\"ls\"}".to_owned(),
                outcome: log::ToolOutcome::Ran,
                argv: Some(tool_loop::argv_of("ls")),
                cwd: Some("~/git/x".to_owned()),
                confined: Some(tool_loop::argv_of("ls")),
                isolation: Some(log::Isolation::None),
                network: Some(log::Network::Host),
                exit: Some(0),
                reason: None,
                policy: None,
                stdout: Some(log::Output {
                    text: "a\n".to_owned(),
                    bytes: 2,
                }),
                stderr: Some(log::Output {
                    text: String::new(),
                    bytes: 0,
                }),
                approval: Some(log::Approval {
                    scope: log::ApprovalScope::Session,
                    decided_at: Some(4),
                    why: Some("not_approved".to_owned()),
                }),
                shown: None,
                files: Vec::new(),
                images: Vec::new(),
                recovered_from: None,
                background: None,
                timeout_ms: None,
            })),
            Event::Forked {
                of_turn: 1,
                at: 3,
                why: log::Warrant::Scoping,
                question: "what did you decide?".to_owned(),
                view: ForkView::Last(2),
                trigger: "turn_end".to_owned(),
                role: Role::Developer,
                seat: Some(log::ForkSeat {
                    substrate: "cpu-seat".to_owned(),
                    model: "small".to_owned(),
                }),
                ask: AskKind::Judgment,
                displaces: false,
            },
            Event::Requested {
                turn: 1,
                lane: Lane::Interview,
                head_sha256: "b".repeat(64),
                fork: Some(20),
                max_tokens: 8192,
            },
            Event::ForkSettled {
                fork: 20,
                outcome: log::ForkOutcome::Value,
                prompt_tokens: Some(281),
                wall_ms: Some(480),
                refused: None,
            },
            Event::Patched {
                fork: Some(20),
                lane: None,
                op: log::PatchOp::Add,
                entry: log::PatchEntry {
                    id: "interview-t1-0".to_owned(),
                    text: "decision: keep it".to_owned(),
                    category: None,
                },
                supersedes: None,
                tangent: Some("t/1".to_owned()),
            },
            Event::Seamed {
                at_turn: 1,
                reason: crate::seam::Reason::Operator,
                prefix_hash_before: "c".repeat(64),
                prefix_hash_after: "d".repeat(64),
                render: "# regime\n".to_owned(),
                carried_entries: 1,
                tail_tokens: 0,
                carried_turns: 0,
                carried_tokens: 0,
                phase: None,
                tool_outputs: log::SeamToolOutputs::Evict,
                outputs: None,
                render_budget: None,
                fired: None,
                pruned: None,
                warm: None,
            },
            Event::Recalled {
                turn: 2,
                recall: log::RecallState::Literal,
                text: "<system-reminder>\n## Relevant memory\n</system-reminder>".to_owned(),
                items: vec![log::RecalledItem {
                    key: "seam-1/message-2".to_owned(),
                    sha256: "f".repeat(64),
                    score: 2,
                }],
            },
            Event::Pruned {
                turn: 2,
                call: "call-1".to_owned(),
                sha256: "e".repeat(64),
                bytes: 300_000,
                text: "- turn 1: bash args={} : 300000 bytes".to_owned(),
            },
            Event::Delivered {
                turn: 2,
                framing: log::Framing::Advisory,
                text: "Working record note: the entry \"decision: keep it\" may be affected by this step. If it no longer holds, say so; otherwise carry on.".to_owned(),
                lines: vec![log::NoteLine {
                    entry: "interview-t1-0".to_owned(),
                    op: log::PatchOp::Retire,
                    template: "FORK_NOTE_ADVISORY".to_owned(),
                }],
            },
            Event::TangentOpened {
                id: "t/1".to_owned(),
                at_turn: 1,
                trunk_messages: 3,
            },
            Event::TangentClosed {
                id: "t/1".to_owned(),
                at_turn: 2,
                kept: vec!["interview-t2-0".to_owned()],
                dropped: Vec::new(),
                parked: vec!["interview-t2-1".to_owned()],
                prefix_intact: true,
                rolled_back: 2,
            },
            Event::BackgroundEnded {
                job: "bg_0123abcd".to_owned(),
                status: log::BackgroundStatus::Completed,
                exit: Some(0),
                files: Vec::new(),
            },
            Event::Notified {
                turn: 2,
                text: "<task-notification>\n</task-notification>".to_owned(),
            },
            Event::TimeoutNear {
                request: 3,
                call: "call-a".to_owned(),
                timeout_ms: 120_000,
            },
            Event::Skipped {
                of_turn: 1,
                trigger: "turn_end".to_owned(),
                ask: AskKind::Judgment,
                call: "call-c".to_owned(),
                field: "decision".to_owned(),
            },
            Event::Captured {
                request: 3,
                call: "call-c".to_owned(),
                tool: "update_record".to_owned(),
                outcome: "dropped".to_owned(),
                entries: Vec::new(),
                why: Some("the groundedness gate kept nothing of it".to_owned()),
                fork: None,
            },
            Event::Reminded {
                turn: 2,
                text: "Anything you meant to record?".to_owned(),
            },
            Event::Audited {
                of_turn: 1,
                at: 3,
                template: crate::dogma::Template::AuditQHuman,
                question: "audit the notes".to_owned(),
            },
        ];
        let mut kinds = std::collections::BTreeSet::new();
        for event in &events {
            kinds.insert(match event {
                Event::Started { .. } => 0,
                Event::Asked { .. } => 1,
                Event::Requested { .. } => 2,
                Event::Settled { .. } => 3,
                Event::Refused { .. } => 4,
                Event::Reasoning { .. } => 5,
                Event::Delta { .. } => 6,
                Event::StopAsked { .. } => 7,
                Event::Answered { .. } => 8,
                Event::Cancelled { .. } => 9,
                Event::Rejected { .. } => 10,
                Event::Crashed { .. } => 11,
                Event::Failed { .. } => 12,
                Event::TurnSettled { .. } => 13,
                Event::IdleGap(_) => 14,
                Event::Progress { .. } => 15,
                Event::Capped { .. } => 16,
                Event::ToolCallPiece { .. } => 17,
                Event::Called { .. } => 18,
                Event::ToolCalled(_) => 19,
                Event::Forked { .. } => 20,
                Event::ForkSettled { .. } => 21,
                Event::Patched { .. } => 22,
                Event::Seamed { .. } => 23,
                Event::Delivered { .. } => 24,
                Event::Recalled { .. } => 25,
                Event::TangentOpened { .. } => 26,
                Event::TangentClosed { .. } => 27,
                Event::Pruned { .. } => 28,
                Event::Captured { .. } => 29,
                Event::Reminded { .. } => 30,
                Event::BackgroundEnded { .. } => 31,
                Event::Notified { .. } => 32,
                Event::TimeoutNear { .. } => 33,
                Event::Skipped { .. } => 34,
                Event::Audited { .. } => 35,
            });
        }
        assert_eq!(kinds.len(), 36, "a variant has no sample");
        events
            .into_iter()
            .enumerate()
            .map(|(seq, event)| Logged {
                seq: seq as u64,
                t: seq as u64 * 7,
                event,
            })
            .collect()
    }

    /// What each of `one_of_every_event`'s samples must become, in order.
    /// A table, one entry per variant, so its length is its content.
    #[allow(clippy::too_many_lines)]
    fn the_lines_of_every_event() -> Vec<log::Event> {
        vec![
            log::Event::SessionStart {
                bash_timeout_ms: None,
                version: log::VERSION,
                opened: 1_790_000_000_000,
                model: "a-model".to_owned(),
                serving: Some(log::Serving {
                    dialect: "llama.cpp".to_owned(),
                    concurrency: Some(2),
                }),
                head: vec![log::HeadMessage {
                    role: log::Role::System,
                    content: HEAD.to_owned(),
                }],
                claim: Some(claimed()),
                provenance: None,
                tools: None,
                template_kwargs: Some(log::TemplateKwargs {
                    enable_thinking: Some(true),
                    reasoning_effort: Some("medium".to_owned()),
                    preserve_thinking: None,
                }),
                unsent: Some(log::Unsent { budget_tokens: 512 }),
                approvals_off: Some(true),
                fork_delivery: Some(log::ForkDelivery::Advisory),
                reasoning_effort_default: Some("xhigh".to_owned()),
                instruction_files: Some(vec![log::InstructionFile {
                    path: "AGENTS.md".to_owned(),
                    sha256: "e".repeat(64),
                }]),
                levers: Some(BTreeMap::from([(
                    "fork-asks".to_owned(),
                    "v3".to_owned(),
                )])),
                tool_output: Some(log::ToolOutput {
                    state: log::ToolOutputState::Capped,
                    max_lines: Some(2000),
                    max_bytes: Some(51_200),
                }),
                phases: None,
                phase_transitions: None,
                opening_phase: None,
                fork_asks: Some(log::ForkAsks {
                    name: "v4".to_owned(),
                    digest: Some(crate::dogma::asks::V4.digest()),
                }),
            },
            log::Event::Ask {
                turn: 1,
                text: "say \"hi\"\n".to_owned(),
                scoping: Some(true),
                files: Some(vec![attached_file()]),
            },
            log::Event::Settlement {
                from: log::State::Awaiting,
                to: log::State::Turn,
            },
            log::Event::Request {
                turn: 1,
                lane: log::Lane::Trunk,
                head_sha256: Some("a".repeat(64)),
                fork: None,
                max_tokens: Some(8192),
            },
            log::Event::Refused {
                command: log::Command::Cancel,
                because: log::Refusal::Stale,
                during: log::State::Turn,
            },
            log::Event::Progress {
                request: 3,
                total: 9276,
                cache: 0,
                processed: 1066,
                time_ms: 721,
            },
            log::Event::Delta {
                request: 3,
                piece: log::Piece::Reasoning("thinking\n".to_owned()),
            },
            log::Event::Delta {
                request: 3,
                piece: log::Piece::Text("Hel".to_owned()),
            },
            log::Event::StopAsked { turn: 1 },
            log::Event::Response {
                reasoning_signature: None,
                redacted: None,
                to_request: 3,
                text: "Hello".to_owned(),
                finish_reason: Some("stop".to_owned()),
                reasoning: Some("thinking\n".to_owned()),
                usage: None,
                capped: None,
                timings: Some(log::Timings {
                    prompt_n: Some(89),
                    cache_n: Some(0),
                    prompt_ms: log::Millis::new("297.198"),
                    predicted_n: Some(312),
                    predicted_ms: log::Millis::new("2591"),
                    draft_n: Some(312),
                    draft_n_accepted: Some(207),
                }),
            },
            log::Event::Cancelled {
                request: 3,
                partial: "Hel".to_owned(),
                reasoning: None,
            },
            log::Event::Response {
                reasoning_signature: None,
                redacted: None,
                to_request: 3,
                text: String::new(),
                finish_reason: Some("length".to_owned()),
                reasoning: Some("thinking, cut".to_owned()),
                timings: None,
                usage: None,
                capped: Some(true),
            },
            log::Event::RequestFailed {
                request: 3,
                reason: log::FailReason::Server,
                message: "busy".to_owned(),
                status: Some(503),
                partial: Some("Hel".to_owned()),
                overflow: None,
            },
            log::Event::RequestFailed {
                request: 3,
                reason: log::FailReason::ContextOverflow,
                message: "too long".to_owned(),
                status: Some(400),
                partial: None,
                overflow: None,
            },
            // Nothing arrived before this crash: no `partial` at all.
            log::Event::RequestFailed {
                request: 3,
                reason: log::FailReason::Crashed,
                message: "a panic".to_owned(),
                status: None,
                partial: None,
                overflow: None,
            },
            log::Event::RequestFailed {
                request: 3,
                reason: log::FailReason::Timeout,
                message: TransportFailure::Timeout {
                    after: Duration::from_secs(5),
                }
                .to_string(),
                status: None,
                partial: Some("Hel".to_owned()),
                overflow: None,
            },
            log::Event::TurnSettled {
                turn: 1,
                reason: log::SettleReason::MaxSteps,
            },
            log::Event::IdleGap {
                opened_by: 13,
                notice: 120,
                read: 2_400,
                compose: 3_100,
                away: 0,
                blocked: 50,
                ended_by: log::GapEnd::Ask,
            },
            log::Event::Delta {
                request: 3,
                piece: log::Piece::ToolCall {
                    index: 0,
                    id: Some("call-a".to_owned()),
                    name: Some("bash".to_owned()),
                    arguments: "{\"command\":".to_owned(),
                },
            },
            log::Event::Response {
                reasoning_signature: None,
                redacted: None,
                to_request: 3,
                text: String::new(),
                finish_reason: Some("tool_calls".to_owned()),
                reasoning: None,
                timings: None,
                usage: None,
                capped: None,
            },
            log::Event::ToolCall {
                request: 3,
                turn: 1,
                id: "call-a".to_owned(),
                name: "bash".to_owned(),
                arguments: "{\"command\":\"ls\"}".to_owned(),
                outcome: log::ToolOutcome::Ran,
                argv: Some(tool_loop::argv_of("ls")),
                cwd: Some("~/git/x".to_owned()),
                confined: Some(tool_loop::argv_of("ls")),
                isolation: Some(log::Isolation::None),
                network: Some(log::Network::Host),
                exit: Some(0),
                reason: None,
                policy: None,
                stdout: Some(log::Output {
                    text: "a\n".to_owned(),
                    bytes: 2,
                }),
                stderr: Some(log::Output {
                    text: String::new(),
                    bytes: 0,
                }),
                approval: Some(log::Approval {
                    scope: log::ApprovalScope::Session,
                    decided_at: Some(4),
                    why: Some("not_approved".to_owned()),
                }),
                files: None,
                shown: None,
                recovered_from: None,
                background: None,
                timeout_ms: None,
            },
            log::Event::Fork {
                lane: log::Lane::Interview,
                of_turn: 1,
                at: 3,
                why: log::Warrant::Scoping,
                question: "what did you decide?".to_owned(),
                view: Some("last:2".to_owned()),
                trigger: Some("turn_end".to_owned()),
                role: Some("developer".to_owned()),
                seat: Some(log::ForkSeat {
                    substrate: "cpu-seat".to_owned(),
                    model: "small".to_owned(),
                }),
                ask: Some("judgment".to_owned()),
                hazard: None,
            },
            log::Event::Request {
                turn: 1,
                lane: log::Lane::Interview,
                head_sha256: Some("b".repeat(64)),
                fork: Some(20),
                max_tokens: Some(8192),
            },
            log::Event::ForkSettled {
                fork: 20,
                outcome: log::ForkOutcome::Value,
                prompt_tokens: Some(281),
                wall_ms: Some(480),
                refused: None,
            },
            log::Event::Patch {
                fork: Some(20),
                lane: None,
                op: log::PatchOp::Add,
                entry: log::PatchEntry {
                    id: "interview-t1-0".to_owned(),
                    text: "decision: keep it".to_owned(),
                    category: None,
                },
                supersedes: None,
                tangent: Some("t/1".to_owned()),
            },
            log::Event::Seam {
                at_turn: 1,
                reason: log::SeamReason::Operator,
                prefix_hash_before: "c".repeat(64),
                prefix_hash_after: "d".repeat(64),
                frame: crate::seam::render::FRAME_VERSION.to_owned(),
                render: "# regime\n".to_owned(),
                carried_entries: 1,
                carried_turns: 0,
                tail_tokens: None,
                carried_tokens: None,
                phase: None,
                tool_outputs: Some(log::SeamToolOutputs::Evict),
                outputs: None,
                carried_outputs: None,
                carried_output_bytes: None,
                placement: Some(log::RenderPlacement::Message),
                render_budget: None,
                fired: None,
                pruned: None,
                warm: None,
            },
            log::Event::Recalled {
                turn: 2,
                recall: log::RecallState::Literal,
                text: "<system-reminder>\n## Relevant memory\n</system-reminder>".to_owned(),
                items: vec![log::RecalledItem {
                    key: "seam-1/message-2".to_owned(),
                    sha256: "f".repeat(64),
                    score: 2,
                }],
            },
            log::Event::Pruned {
                turn: 2,
                call: "call-1".to_owned(),
                sha256: "e".repeat(64),
                bytes: 300_000,
                text: "- turn 1: bash args={} : 300000 bytes".to_owned(),
            },
            log::Event::Delivered {
                turn: 2,
                framing: log::Framing::Advisory,
                text: "Working record note: the entry \"decision: keep it\" may be affected by this step. If it no longer holds, say so; otherwise carry on.".to_owned(),
                lines: vec![log::NoteLine {
                    entry: "interview-t1-0".to_owned(),
                    op: log::PatchOp::Retire,
                    template: "FORK_NOTE_ADVISORY".to_owned(),
                }],
            },
            log::Event::TangentOpen {
                id: "t/1".to_owned(),
                at_turn: 1,
                trunk_messages: 3,
            },
            log::Event::TangentClose {
                id: "t/1".to_owned(),
                at_turn: 2,
                kept: vec!["interview-t2-0".to_owned()],
                dropped: Vec::new(),
                parked: vec!["interview-t2-1".to_owned()],
                prefix_intact: true,
                rolled_back: 2,
            },
            log::Event::BackgroundEnded {
                job: "bg_0123abcd".to_owned(),
                status: log::BackgroundStatus::Completed,
                exit: Some(0),
                files: None,
            },
            log::Event::Notice {
                turn: 2,
                text: "<task-notification>\n</task-notification>".to_owned(),
            },
            log::Event::TimeoutNear {
                request: 3,
                call: "call-a".to_owned(),
                timeout_ms: 120_000,
            },
            log::Event::ForkSkipped {
                of_turn: 1,
                trigger: "turn_end".to_owned(),
                ask: "judgment".to_owned(),
                call: "call-c".to_owned(),
                field: "decision".to_owned(),
            },
            log::Event::Capture {
                request: 3,
                call: "call-c".to_owned(),
                tool: "update_record".to_owned(),
                outcome: "dropped".to_owned(),
                entries: Vec::new(),
                why: Some("the groundedness gate kept nothing of it".to_owned()),
                fork: None,
            },
            log::Event::Reminded {
                turn: 2,
                text: "Anything you meant to record?".to_owned(),
            },
            log::Event::Fork {
                lane: log::Lane::Audit,
                of_turn: 1,
                at: 3,
                why: log::Warrant::Seam,
                question: "audit the notes".to_owned(),
                view: None,
                trigger: None,
                role: None,
                seat: None,
                ask: Some("audit_q_human".to_owned()),
                hazard: None,
            },
        ]
    }

    #[test]
    fn every_capped_finish_reason_the_client_knows_caps_a_turn() {
        // One list, the client's, so the session and the scripted drive
        // cannot disagree about what a cap is (#290).
        for reason in crate::client::CAPPED_FINISH_REASONS {
            let session = Session::open(
                Canned::new([vec![
                    Step::Delta("cut".to_owned()),
                    Step::FinishReason((*reason).to_owned()),
                ]]),
                template(),
            );
            session.ask("go", None).expect("accepted");
            let log = wait_until(&session, "the turn to settle", settled);
            assert!(
                log.iter()
                    .any(|logged| matches!(logged.event, Event::Capped { .. })),
                "{reason}: {log:?}"
            );
        }
    }

    #[test]
    fn a_capped_call_is_written_capped_settles_failed_and_stays_off_the_trunk() {
        // #290, ruled (5969297103): a turn its output cap ended is not an
        // answer. The rehearsal's 3 of 10 turns were all reasoning.
        let session = Session::open(
            Canned::new([
                vec![
                    Step::Reasoning("thinking, and thinking".to_owned()),
                    Step::FinishReason("length".to_owned()),
                ],
                deltas(&["two"]),
            ]),
            template(),
        );
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the capped turn to settle", settled);
        let lines: Vec<log::Event> = log.iter().map(|logged| line_of(logged).event).collect();
        assert!(
            lines.iter().any(|line| matches!(
                line,
                log::Event::Response {
                    capped: Some(true),
                    finish_reason: Some(reason),
                    ..
                } if reason == "length"
            )),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| matches!(
                line,
                log::Event::TurnSettled {
                    turn: 1,
                    reason: log::SettleReason::Failed
                }
            )),
            "{lines:?}"
        );
        session.ask("second", None).expect("accepted");
        let _ = wait_until(&session, "the second turn to settle", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
                && settled(log)
        });
        // Neither the capped ask nor its reasoning went back as history.
        assert_eq!(
            session.shared.transport.sent()[1].messages,
            template()
                .messages
                .into_iter()
                .chain([user("second")])
                .collect::<Vec<_>>()
        );
    }

    /// A whole session with one capped turn, as the session logs it, with
    /// `opened` and each `t` pinned so the text is the same on every run.
    fn a_capped_session_log() -> String {
        let session = Session::open(
            Canned::new([vec![
                Step::Reasoning("Let me think about this carefully".to_owned()),
                Step::Delta("The answer".to_owned()),
                Step::FinishReason("length".to_owned()),
            ]]),
            template(),
        );
        session.ask("what is the answer?", None).expect("accepted");
        let _ = wait_until(&session, "the capped turn to settle", settled);
        assert_eq!(session.end(None), Ok(()));
        session
            .events_from(0)
            .into_iter()
            .map(|mut logged| {
                logged.t = logged.seq * 5;
                if let Event::Started { opened, .. } = &mut logged.event {
                    *opened = 1_790_000_000_000;
                }
                render(&logged) + "\n"
            })
            .collect()
    }

    #[test]
    fn the_capped_turn_fixture_is_what_a_capped_session_logs() {
        // The fixture the surface replays for its capped badge (#290, ruled
        // 5969297103): `response` with `capped`, then `turn.settled` failed.
        let fixture = include_str!("../../drive/fixtures/a-capped-turn.jsonl");
        assert_eq!(a_capped_session_log(), fixture);
        let read = log::parse(fixture).expect("the fixture is a log the format reads");
        assert!(read.iter().any(|line| matches!(
            line.event,
            log::Event::Response {
                capped: Some(true),
                ..
            }
        )));
    }

    /// The writer's half of #249's rule: a server that reports `timings`
    /// gets no `usage` on its response line -- on llama.cpp the two are equal
    /// (measured on #157), and the reader refuses a line carrying both.
    #[test]
    fn a_response_with_timings_is_written_without_usage() {
        let session = Session::open(
            Canned::new([vec![
                Step::Delta("Hello".to_owned()),
                Step::Timings(crate::client::stream::Timings {
                    prompt_n: Some(18),
                    cache_n: Some(160),
                    predicted_n: Some(66),
                    ..crate::client::stream::Timings::default()
                }),
            ]]),
            template(),
        );
        session.ask("say hello", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let responses: Vec<log::Line> = log
            .iter()
            .map(line_of)
            .filter(|line| matches!(line.event, log::Event::Response { .. }))
            .collect();
        assert!(
            matches!(
                responses.as_slice(),
                [log::Line {
                    event: log::Event::Response {
                        timings: Some(_),
                        usage: None,
                        ..
                    },
                    ..
                }]
            ),
            "{responses:?}"
        );
    }

    #[test]
    fn every_event_the_session_logs_is_a_line_the_log_format_reads() {
        for logged in one_of_every_event() {
            let rendered = render(&logged);
            assert!(!rendered.contains('\n'), "a line broke: {rendered}");
            assert_eq!(
                log::line(&rendered),
                Ok(line_of(&logged)),
                "{rendered} does not read back as the line it was written from"
            );
        }

        // A round trip cannot see a conversion that loses or mistranslates
        // something on the way in -- it reads back whatever was written -- so
        // what EVERY sample becomes is written out here, one line each, in
        // `one_of_every_event`'s order (#140's first review).
        let expected = the_lines_of_every_event();
        let written: Vec<log::Event> = one_of_every_event()
            .iter()
            .map(|logged| line_of(logged).event)
            .collect();
        assert_eq!(written, expected);
        // A connection that failed is `transport`, not `timeout`.
        assert_eq!(
            failed_line(
                3,
                &TransportFailure::Connect("refused".to_owned()),
                "",
                None
            ),
            log::Event::RequestFailed {
                request: 3,
                reason: log::FailReason::Transport,
                message: TransportFailure::Connect("refused".to_owned()).to_string(),
                status: None,
                partial: None,
                overflow: None,
            }
        );
    }

    #[test]
    fn a_real_sessions_whole_log_is_a_log_the_format_reads() {
        // Every rule that spans lines included: an answer, a cancel, a
        // failure, a refusal by the server.
        let gate = Gate::new();
        let canned = Canned::new([
            vec![
                Step::Reasoning("hmm\n".to_owned()),
                Step::Delta("one".to_owned()),
            ],
            vec![Step::Delta("par".to_owned()), Step::Hold(gate.clone())],
            vec![Step::Fail(TransportFailure::Connect("refused".to_owned()))],
            vec![Step::Reject(503, "busy".to_owned())],
            vec![
                Step::Reasoning("all of it".to_owned()),
                Step::FinishReason("length".to_owned()),
            ],
        ]);
        let session = Session::open(canned, template());
        for turn in 1..=5_u32 {
            session.ask("go", None).expect("accepted");
            if turn == 2 {
                assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
                assert_eq!(session.cancel(2, None), Ok(()));
            }
            wait_until(&session, "the turn to settle", |log| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == turn as usize
                    && settled(log)
            });
        }
        let document: String = session
            .events_from(0)
            .iter()
            .map(|logged| render(logged) + "\n")
            .collect();
        let read = log::parse(&document).expect("the session's log is a log the format reads");
        assert_eq!(read.len(), session.events_from(0).len());
    }

    fn gap(opened_by: u64, ended_by: GapEnd) -> IdleGap {
        IdleGap {
            opened_by,
            notice: 120,
            read: 2_400,
            compose: 3_100,
            away: 0,
            blocked: 0,
            ended_by,
        }
    }

    fn settling_seq(log: &[Logged], turn: u32) -> u64 {
        log.iter()
            .find(|logged| matches!(logged.event, Event::TurnSettled { turn: settled, .. } if settled == turn))
            .map(|logged| logged.seq)
            .expect("the turn settled")
    }

    #[test]
    fn a_refused_commands_gap_is_neither_logged_nor_closed_and_the_admitted_one_carries_it() {
        let session = Session::open(
            Canned::new([deltas(&["one"]), deltas(&["two"])]),
            template(),
        );
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the first turn to settle", settled);
        let first = settling_seq(&log, 1);

        // Refused: this session keeps no working memory to refill from. The
        // gap it carried is not logged, and it stays open (ruled on #146,
        // amending D13 (c)).
        assert_eq!(
            session.declare_seam(Some(gap(first, GapEnd::Seam))),
            Err(Rejected::Refused(Refusal::NothingToSeam))
        );
        assert!(
            !session
                .events_from(0)
                .iter()
                .any(|logged| matches!(logged.event, Event::IdleGap(_))),
            "a refused command's gap was logged"
        );

        // Admitted: the ask that follows carries the gap, its `blocked`
        // running from the refusal, and it is the one gap logged -- right
        // before the ask.
        let mut carried = gap(first, GapEnd::Ask);
        carried.blocked = 850;
        session.ask("second", Some(carried)).expect("accepted");
        let log = wait_until(&session, "the second turn to settle", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
                && settled(log)
        });
        let gaps: Vec<usize> = log
            .iter()
            .enumerate()
            .filter(|(_, logged)| matches!(logged.event, Event::IdleGap(_)))
            .map(|(at, _)| at)
            .collect();
        assert_eq!(gaps.len(), 1, "{log:#?}");
        assert_eq!(log[gaps[0]].event, Event::IdleGap(carried));
        assert!(matches!(
            log[gaps[0] + 1].event,
            Event::Asked { turn: 2, .. }
        ));
        let document: String = log.iter().map(|logged| render(logged) + "\n").collect();
        log::parse(&document).expect("a log carrying a gap is a log the format reads");
    }

    #[test]
    fn a_gap_closes_at_the_command_that_ends_it_even_when_that_command_carries_none() {
        let gate = Gate::new();
        let session = Session::open(
            Canned::new([
                deltas(&["one"]),
                vec![Step::Delta("par".to_owned()), Step::Hold(gate.clone())],
            ]),
            template(),
        );
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the first turn to settle", settled);
        let first = settling_seq(&log, 1);

        // The second ask ends turn 1's gap without carrying it.
        session.ask("second", None).expect("accepted");
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        assert_eq!(
            session.cancel(2, Some(gap(first, GapEnd::Cancel))),
            Err(Rejected::BadGap(GapError::NotTheOpenGap {
                opened_by: first
            })),
            "a gap was logged after the ask that ended it"
        );
        assert_eq!(session.cancel(2, None), Ok(()));
        let log = wait_until(&session, "the second turn to settle", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
                && settled(log)
        });
        let second = settling_seq(&log, 2);

        // An end ends the gap too; nothing logs one after the session ended.
        assert_eq!(session.end(None), Ok(()));
        assert_eq!(
            session.declare_seam(Some(gap(second, GapEnd::Seam))),
            Err(Rejected::Refused(Refusal::Ended))
        );
        assert!(
            !session
                .events_from(0)
                .iter()
                .any(|logged| matches!(logged.event, Event::IdleGap(_))),
            "a gap was logged"
        );
    }

    #[test]
    fn an_admitted_end_logs_the_gap_it_carries_before_it_ends() {
        let session = Session::open(Canned::new([deltas(&["one"])]), template());
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let settling = settling_seq(&log, 1);
        assert_eq!(session.end(Some(gap(settling, GapEnd::End))), Ok(()));
        let log = session.events_from(0);
        let at = log.len() - 1;
        assert_eq!(
            log[at].event,
            Event::Settled {
                from: Settlement::Awaiting,
                to: Settlement::Ended
            }
        );
        assert_eq!(
            log[at - 1].event,
            Event::IdleGap(gap(settling, GapEnd::End))
        );

        // And a duration the log cannot write is refused, not capped.
        let fresh = Session::open(Canned::new([deltas(&["one"])]), template());
        fresh.ask("first", None).expect("accepted");
        let log = wait_until(&fresh, "the turn to settle", settled);
        let mut huge = gap(settling_seq(&log, 1), GapEnd::End);
        huge.away = u64::MAX;
        assert_eq!(
            fresh.end(Some(huge)),
            Err(Rejected::BadGap(GapError::NotACount))
        );
    }

    #[test]
    fn a_gap_the_session_cannot_log_is_rejected_and_nothing_is_logged() {
        let session = Session::open(Canned::new([deltas(&["one"])]), template());
        // No settling yet: there is no gap to close.
        assert_eq!(
            session.ask("first", Some(gap(0, GapEnd::Ask))),
            Err(Rejected::BadGap(GapError::NotTheOpenGap { opened_by: 0 }))
        );
        session.ask("first", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let settling = settling_seq(&log, 1);
        let before = log.len();

        // A gap that cites anything but the latest settling.
        assert_eq!(
            session.end(Some(gap(settling - 1, GapEnd::End))),
            Err(Rejected::BadGap(GapError::NotTheOpenGap {
                opened_by: settling - 1
            }))
        );
        // A gap that says another command ended it.
        assert_eq!(
            session.end(Some(gap(settling, GapEnd::Ask))),
            Err(Rejected::BadGap(GapError::EndedByAnotherCommand {
                says: GapEnd::Ask,
                carried_by: CommandKind::End
            }))
        );
        assert_eq!(
            session.events_from(0).len(),
            before,
            "a gap that could not be logged left something in the log"
        );
        assert_eq!(
            session.settlement(),
            Settlement::Awaiting,
            "a rejected end ended"
        );

        // And the gap is still open for the command that does end it.
        assert_eq!(session.end(Some(gap(settling, GapEnd::End))), Ok(()));
    }

    #[test]
    fn the_vocabularies_are_the_words_the_log_carries() {
        let tags = |all: &[&str]| all.join(" ");
        assert_eq!(
            tags(
                &Settlement::ALL
                    .iter()
                    .map(|it| it.tag())
                    .collect::<Vec<_>>()
            ),
            "awaiting turn capture ended"
        );
        assert_eq!(
            tags(
                &CommandKind::ALL
                    .iter()
                    .map(|it| it.tag())
                    .collect::<Vec<_>>()
            ),
            "ask cancel declare-seam end open-tangent close-tangent background"
        );
        assert_eq!(
            tags(&Refusal::ALL.iter().map(|it| it.tag()).collect::<Vec<_>>()),
            "in-flight ended nothing-in-flight nothing-to-seam no-phase-graph not-a-phase \
             already-in-phase no-phase-edge stale tangent-open no-tangent bad-tangent not-the-scope nothing-running"
        );
        assert_eq!(
            tags(
                &SettleReason::ALL
                    .iter()
                    .map(|it| it.tag())
                    .collect::<Vec<_>>()
            ),
            "final cancelled max_steps timeout failed"
        );
        assert_eq!(
            tags(&Lane::ALL.iter().map(|it| it.tag()).collect::<Vec<_>>()),
            "trunk interview audit"
        );
    }

    // -----------------------------------------------------------------------
    // the tool loop (#298)
    // -----------------------------------------------------------------------

    use crate::isolation::bwrap::Bubblewrap;
    use crate::isolation::{Backend, Confinement, Policy as IsolationPolicy};
    use std::path::{Path, PathBuf};

    use super::super::tool_loop::tests::{no_aliases, scratch};

    /// A runner that drops everything up to `--` and runs `then`: the
    /// sandbox's ARM, so what a line records of a confined run is checked
    /// against a run that happened, on a host with no sandbox. It confines
    /// nothing, and nothing here says it does.
    pub(in crate::drive) fn stand_in(dir: &Path, then: &str) -> Confinement {
        use std::os::unix::fs::PermissionsExt as _;
        let at = dir.join("stand-in-runner");
        std::fs::write(
            &at,
            format!("#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\n{then}\n"),
        )
        .expect("a stand-in runner");
        std::fs::set_permissions(&at, std::fs::Permissions::from_mode(0o755))
            .expect("it is executable");
        Confinement::Sandbox(Backend::Bubblewrap(Bubblewrap::at(at)))
    }

    /// The loop's parts over `worktree`, its allow set pre-seeded with
    /// `preseed`.
    pub(in crate::drive) fn tools(
        confinement: Confinement,
        worktree: &Path,
        preseed: &[&str],
        max_steps: Option<u32>,
        decider: Decider,
    ) -> Tools {
        let gate = tool_loop::Gate {
            aliases: no_aliases,
            ..tool_loop::Gate::standard(worktree)
        };
        let commands: Vec<String> = preseed.iter().map(|c| (*c).to_owned()).collect();
        let allowed = tool_loop::preseeded(&commands, &gate).expect("a pre-seed");
        Tools {
            confinement,
            policy: IsolationPolicy::unconfined(),
            worktree: worktree.to_path_buf(),
            cwd: "~/git/a-worktree".to_owned(),
            max_steps,
            decider,
            gate,
            allowed,
            store: None,
            approval_policy: None,
            approvals_off: false,
            text_fallback: false,
            output_cap: crate::drive::output::OutputCap::DEFAULT,
            recording: None,
            read_tool: None,
            surface: tool_loop::ToolSurface::Bash,
            background: false,
            timeout_ms: None,
        }
    }

    pub(in crate::drive) fn looping() -> RequestShape {
        RequestShape {
            tools: vec![tool_loop::bash_tool()],
            ..template()
        }
    }

    pub(in crate::drive) fn bash(id: &str, command: &str) -> Step {
        Step::call(
            0,
            id,
            "bash",
            &format!("{{\"command\":{}}}", serde_json::Value::from(command)),
        )
    }

    /// A session whose calls time out after `default` ms (#613), approvals
    /// off, its `bash` declared with `timeout`, playing `replies`.
    fn timing(tree: &Path, default: Option<u64>, replies: Vec<Vec<Step>>) -> Session<Canned> {
        let mut tools = tools(
            Confinement::Unconfined,
            tree,
            &["sleep", "echo", "ls"],
            None,
            Decider::Decline,
        );
        tools.approvals_off = true;
        tools.timeout_ms = default;
        let mut shape = looping();
        shape.tools = tool_loop::ToolSurface::Bash.tools_timed(false, default);
        Session::open_looping(Canned::new(replies), shape, None, tools)
    }

    /// A call its default timeout ends (#613): its group killed, the call
    /// answered with Qwen Code's words and what it printed, and the turn
    /// goes on to its answer; every head rebuilds from the log.
    #[test]
    fn a_call_at_its_timeout_is_ended_answered_and_the_turn_goes_on() {
        let tree = scratch("timeout-default");
        let session = timing(
            &tree,
            Some(500),
            vec![
                vec![bash("call-1", "echo early; sleep 30")],
                deltas(&["it hung"]),
            ],
        );
        let asked = Instant::now();
        session.ask("install it", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert!(
            asked.elapsed() < Duration::from_secs(5),
            "{:?}",
            asked.elapsed()
        );
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        let line = lines(&log).remove(0);
        assert_eq!(line.outcome, log::ToolOutcome::Ran);
        assert_eq!(line.timeout_ms, Some(500));
        assert_eq!(
            line.shown.as_deref(),
            Some(
                "Command timed out after 500ms before it could complete. Below is the output \
                 before it timed out:\nearly\n"
            )
        );
        let Event::Started {
            bash_timeout_ms, ..
        } = &log[0].event
        else {
            panic!("the log opens with the session");
        };
        assert_eq!(*bash_timeout_ms, Some(500));
        reads_whole(&session);
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// A call's own `timeout` (#613) wins over a session with none, and one
    /// out of range is refused in Qwen Code's words.
    #[test]
    fn a_calls_own_timeout_ends_it_and_a_bad_one_is_refused() {
        let tree = scratch("timeout-own");
        let session = timing(
            &tree,
            None,
            vec![
                vec![Step::call(
                    0,
                    "call-1",
                    "bash",
                    r#"{"command":"sleep 30","timeout":300}"#,
                )],
                vec![Step::call(
                    0,
                    "call-2",
                    "bash",
                    r#"{"command":"ls","timeout":700000}"#,
                )],
                deltas(&["done"]),
            ],
        );
        session.ask("try", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        let written = lines(&log);
        assert_eq!(written[0].timeout_ms, Some(300));
        assert_eq!(
            written[0].shown.as_deref(),
            Some(
                "Command timed out after 300ms before it could complete. There was no output \
                 before it timed out."
            )
        );
        assert_eq!(written[1].outcome, log::ToolOutcome::Refused);
        assert_eq!(
            written[1].shown.as_deref(),
            Some("Timeout cannot exceed 600000ms (10 minutes).")
        );
        let Event::Started {
            bash_timeout_ms, ..
        } = &log[0].event
        else {
            panic!("the log opens with the session");
        };
        assert_eq!(*bash_timeout_ms, Some(0), "none, declared");
        reads_whole(&session);
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// Fifteen seconds before its timeout (#613), a running call's
    /// `timeout.near` line is logged for the surface, never the model.
    #[test]
    fn a_call_near_its_timeout_is_logged_for_the_surface() {
        let tree = scratch("timeout-near");
        let session = timing(
            &tree,
            Some(15_200),
            vec![vec![bash("call-1", "sleep 30")], deltas(&["stopped"])],
        );
        session.ask("wait", None).expect("accepted");
        let log = wait_until(&session, "the warning", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::TimeoutNear { .. }))
        });
        let near = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::TimeoutNear {
                    call, timeout_ms, ..
                } => Some((call.clone(), *timeout_ms)),
                _ => None,
            })
            .expect("the warning");
        assert_eq!(near, ("call-1".to_owned(), 15_200));
        session.cancel(1, None).expect("cancelled");
        wait_until(&session, "the turn", settled);
        let sent = session.shared.transport.sent();
        assert!(
            sent.iter()
                .flat_map(|shape| &shape.messages)
                .all(|message| !message.content.contains("time out")),
            "the warning never reaches the model"
        );
        reads_whole(&session);
        tidy(&[&tree]);
    }

    /// A session whose `bash` runs in the background on request (#614),
    /// approvals off, playing `replies`.
    fn backgrounding(
        tree: &Path,
        recording: Option<&Path>,
        replies: Vec<Vec<Step>>,
    ) -> Session<Canned> {
        let mut tools = tools(
            Confinement::Unconfined,
            tree,
            &["sleep", "echo"],
            None,
            Decider::Decline,
        );
        tools.background = true;
        tools.approvals_off = true;
        tools.recording = recording.map(Path::to_path_buf);
        let mut shape = looping();
        shape.tools = tool_loop::ToolSurface::Bash.tools_with(true);
        Session::open_looping(Canned::new(replies), shape, None, tools)
    }

    /// A `bash` call asking for the background.
    fn in_the_background(id: &str, command: &str) -> Step {
        Step::call(
            0,
            id,
            "bash",
            &serde_json::json!({"command": command, "is_background": true}).to_string(),
        )
    }

    /// Each ended job: its id, how, and its exit status (#614).
    fn ended_jobs(log: &[Logged]) -> Vec<(String, log::BackgroundStatus, Option<u64>)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::BackgroundEnded {
                    job, status, exit, ..
                } => Some((job.clone(), *status, *exit)),
                _ => None,
            })
            .collect()
    }

    /// How many turns have settled.
    fn turns_settled(log: &[Logged]) -> usize {
        log.iter()
            .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
            .count()
    }

    /// A background call (#614) returns at once with Qwen Code's start
    /// text, its bare trailing `&` taken off; the command runs on, its
    /// status file says so, and when it ends its output is kept by digest,
    /// its end is logged, and its notification follows the next ask --
    /// every head rebuilding.
    #[test]
    fn a_background_call_returns_at_once_and_its_end_is_noticed_at_the_next_ask() {
        let tree = scratch("bg-start");
        let recording = scratch("bg-start-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let session = backgrounding(
            &tree,
            Some(&recording),
            vec![
                vec![in_the_background("call-1", "sleep 2; echo ready &")],
                deltas(&["started"]),
                deltas(&["it is ready"]),
            ],
        );
        let asked = Instant::now();
        session.ask("start it", None).expect("accepted");
        let log = wait_until(&session, "the turn", |log| turns_settled(log) == 1);
        assert!(
            asked.elapsed() < Duration::from_millis(1500),
            "{:?}",
            asked.elapsed()
        );
        let line = lines(&log).remove(0);
        let id = line.background.clone().expect("a job");
        assert_eq!(line.outcome, log::ToolOutcome::Ran);
        assert_eq!(line.argv, Some(tool_loop::argv_of("sleep 2; echo ready")));
        assert!(
            line.shown.as_deref().is_some_and(
                |shown| shown.starts_with(&format!("Background shell started.\nid: {id}\n"))
            ),
            "{:?}",
            line.shown
        );
        let (output, status) =
            super::super::background::files_of(&recording.join("background"), &id);
        assert!(
            std::fs::read_to_string(&status)
                .expect("a status file")
                .contains("\"status\":\"running\"")
        );
        let log = wait_until(&session, "the job's end", |log| ended_jobs(log).len() == 1);
        assert_eq!(
            ended_jobs(&log),
            [(id.clone(), log::BackgroundStatus::Completed, Some(0))]
        );
        assert_eq!(
            std::fs::read_to_string(&output).expect("its output"),
            "ready\n"
        );
        assert!(
            std::fs::read_to_string(&status)
                .expect("a status file")
                .contains("\"status\":\"completed\"")
        );
        session.ask("is it ready?", None).expect("accepted");
        let log = wait_until(&session, "the second turn", |log| turns_settled(log) == 2);
        let sent = session.shared.transport.sent();
        let last = &sent.last().expect("a request").messages;
        let note = &last[last.len() - 1];
        assert_eq!(last[last.len() - 2].content, "is it ready?");
        assert_eq!(note.role, Role::User);
        assert!(
            note.content
                .starts_with(&format!("<task-notification>\n<task-id>{id}</task-id>"))
        );
        assert!(note.content.contains("<status>completed</status>"));
        assert!(
            note.content
                .contains("<output-tail truncated=\"false\">ready\n</output-tail>")
        );
        reads_whole(&session);
        every_head_rebuilds(&log);
        session.end_commands();
        tidy(&[&tree, &recording]);
    }

    /// `task_stop` (#614): a running job's group stopped and the job ended
    /// `cancelled`; an id that names no job, and one no longer running,
    /// answered in Qwen Code's words. A turn's end leaves a job running.
    #[test]
    fn task_stop_cancels_a_running_job_and_answers_what_it_cannot_stop() {
        let tree = scratch("bg-stop");
        let session = backgrounding(
            &tree,
            None,
            vec![
                vec![in_the_background("call-1", "sleep 30")],
                deltas(&["started"]),
            ],
        );
        session.ask("start it", None).expect("accepted");
        let log = wait_until(&session, "the turn", |log| turns_settled(log) == 1);
        let id = lines(&log)[0].background.clone().expect("a job");
        assert!(
            ended_jobs(&log).is_empty(),
            "the turn's end leaves it running"
        );
        let stop = |call: &str, id: &str| {
            Step::call(
                0,
                call,
                super::super::background::TASK_STOP,
                &serde_json::json!({ "task_id": id }).to_string(),
            )
        };
        let transport = &session.shared.transport;
        transport.append(vec![stop("call-2", &id)]);
        transport.append(vec![stop("call-3", "bg_nothere")]);
        transport.append(deltas(&["stopped"]));
        session.ask("stop it", None).expect("accepted");
        let log = wait_until(&session, "the stop and the job's end", |log| {
            turns_settled(log) == 2 && ended_jobs(log).len() == 1
        });
        let written = lines(&log);
        assert!(
            written[1]
                .shown
                .as_deref()
                .is_some_and(|shown| shown.starts_with(&format!(
                    "Cancellation requested for background shell \"{id}\""
                ))),
            "{:?}",
            written[1].shown
        );
        assert_eq!(
            written[2].shown.as_deref(),
            Some("Error: No background task found with ID \"bg_nothere\".")
        );
        assert_eq!(ended_jobs(&log)[0].1, log::BackgroundStatus::Cancelled);
        transport.append(vec![stop("call-4", &id)]);
        transport.append(deltas(&["done"]));
        session.ask("stop it again", None).expect("accepted");
        let log = wait_until(&session, "the third turn", |log| turns_settled(log) == 3);
        assert_eq!(
            lines(&log)[3].shown.as_deref(),
            Some(
                format!("Error: Background shell \"{id}\" is not running (status: cancelled).")
                    .as_str()
            )
        );
        reads_whole(&session);
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// The operator moves a running call to the background (#614): it
    /// answers at once with Qwen Code's promotion text and what it printed
    /// so far, the command runs on as the job the command channel named,
    /// and with nothing running the move is refused `nothing-running`.
    #[test]
    fn a_running_call_moved_to_the_background_returns_and_runs_on() {
        let tree = scratch("bg-promote");
        let session = backgrounding(
            &tree,
            None,
            vec![
                vec![bash("call-1", "echo early; sleep 2; echo late")],
                deltas(&["moved on"]),
            ],
        );
        assert!(matches!(
            session.background(),
            Err(Rejected::Refused(Refusal::NothingRunning))
        ));
        session.ask("run it", None).expect("accepted");
        let started = Instant::now();
        let job = loop {
            if let Ok(job) = session.background() {
                break job;
            }
            assert!(started.elapsed() < Duration::from_secs(5), "never running");
            std::thread::sleep(Duration::from_millis(20));
        };
        let log = wait_until(&session, "the turn", |log| turns_settled(log) == 1);
        let line = lines(&log).remove(0);
        assert_eq!(line.background.as_deref(), Some(job.as_str()));
        assert!(
            line.shown.as_deref().is_some_and(|shown| shown.starts_with(&format!(
                "Foreground command \"echo early; sleep 2; echo late\" promoted to background as {job}."
            ))),
            "{:?}",
            line.shown
        );
        let log = wait_until(&session, "the job's end", |log| ended_jobs(log).len() == 1);
        assert_eq!(ended_jobs(&log)[0].1, log::BackgroundStatus::Completed);
        let output = std::fs::read_to_string(
            super::super::background::files_of(
                &background_dir(&session.shared, &session.shared.lock()),
                &job,
            )
            .0,
        )
        .expect("its output");
        assert!(output.ends_with("late\n"), "{output:?}");
        reads_whole(&session);
        tidy(&[&tree]);
    }

    pub(in crate::drive) fn lines(log: &[Logged]) -> Vec<ToolLine> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::ToolCalled(line) => Some(line.as_ref().clone()),
                _ => None,
            })
            .collect()
    }

    fn requested(log: &[Logged]) -> usize {
        log.iter()
            .filter(|logged| matches!(logged.event, Event::Requested { turn: 1, .. }))
            .count()
    }

    fn settled_as(log: &[Logged]) -> Option<SettleReason> {
        log.iter().rev().find_map(|logged| match logged.event {
            Event::TurnSettled { reason, .. } => Some(reason),
            _ => None,
        })
    }

    fn call_message(id: &str, command: &str) -> Message {
        let mut said = assistant("");
        said.tool_calls = vec![ToolCall {
            id: id.to_owned(),
            name: "bash".to_owned(),
            arguments: format!("{{\"command\":{}}}", serde_json::Value::from(command)),
        }];
        said
    }

    /// The log a session wrote, read back whole by the format's own reader.
    fn reads_whole<S: Streaming>(session: &Session<S>) {
        let text: String = session
            .events_from(0)
            .iter()
            .map(|logged| render(logged) + "\n")
            .collect();
        if let Err(why) = log::parse(&text) {
            panic!("the log does not read: {why}\n{text}");
        }
    }

    fn tidy(dirs: &[&PathBuf]) {
        for dir in dirs {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// T6: a listed command runs through the confinement, its line says what
    /// ran it, and the call and its result go back to the model and onto the
    /// trunk.
    /// A session that runs commands, projected (#472): its requests carry
    /// the bash tool and a tool turn's whole exchange stays on the trunk, and
    /// the projection rebuilds both -- every trunk head of a two-step tool
    /// turn and of the plain turn after it is verified, none named.
    #[test]
    fn a_tool_turns_heads_and_the_trunk_after_it_are_rebuilt_and_verified() {
        let tree = scratch("t472-tree");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch a")],
                vec![bash("call-2", "touch b")],
                deltas(&["done"]),
                deltas(&["ok"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("make two markers", None).expect("accepted");
        wait_until(&session, "the tool turn to settle", settled);
        session.ask("and now?", None).expect("accepted");
        let log = wait_until(&session, "the second turn to settle", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        let lines: Vec<log::Line> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        let named: Vec<String> = projected
            .unspellable
            .iter()
            .filter(|u| u.kind == "request")
            .map(|u| u.why.clone())
            .collect();
        assert_eq!(named, Vec::<String>::new());
        let trunk_requests = lines
            .iter()
            .filter(|line| {
                matches!(
                    line.event,
                    log::Event::Request {
                        lane: log::Lane::Trunk,
                        ..
                    }
                )
            })
            .count();
        assert_eq!(trunk_requests, 4, "three steps of turn 1, one of turn 2");
        let _ = std::fs::remove_dir_all(&tree);
    }

    #[test]
    fn a_listed_call_runs_confined_goes_back_as_openais_shape_and_settles_final() {
        for confined in [true, false] {
            let tree = scratch(&format!("t6-tree-{confined}"));
            let runner = scratch(&format!("t6-runner-{confined}"));
            let confinement = if confined {
                stand_in(&runner, "exec \"$@\"")
            } else {
                Confinement::Unconfined
            };
            let session = Session::open_looping(
                Canned::new([vec![bash("call-1", "touch marker")], deltas(&["done"])]),
                looping(),
                None,
                tools(
                    confinement.clone(),
                    &tree,
                    &["touch"],
                    None,
                    Decider::Decline,
                ),
            );
            session.ask("make a marker", None).expect("accepted");
            let log = wait_until(&session, "the turn to settle", settled);
            reads_whole(&session);
            let argv = tool_loop::argv_of("touch marker");
            let [line] = lines(&log).try_into().expect("one call, one line");
            assert_eq!(line.outcome, log::ToolOutcome::Ran);
            assert_eq!(line.argv, Some(argv.clone()));
            assert_eq!(line.cwd.as_deref(), Some("~/git/a-worktree"));
            let composed = confinement.compose(&IsolationPolicy::unconfined(), &tree, &argv);
            assert_eq!(line.confined, Some(composed.clone()));
            if confined {
                assert_ne!(composed, argv, "the runner is in what ran");
                assert_eq!(line.isolation, Some(log::Isolation::Sandbox));
            } else {
                assert_eq!(composed, argv, "unconfined, what ran is the argv");
                assert_eq!(line.isolation, Some(log::Isolation::None));
            }
            assert_eq!(
                line.approval,
                Some(log::Approval {
                    scope: log::ApprovalScope::Preseeded,
                    decided_at: None,
                    why: None,
                })
            );
            assert!(tree.join("marker").exists(), "the command ran");
            assert_eq!(requested(&log), 2, "a step is a request");
            assert_eq!(settled_as(&log), Some(SettleReason::Final));

            let sent = session.shared.transport.sent();
            assert_eq!(sent.len(), 2);
            let result = Message::tool_result("call-1", "");
            assert_eq!(
                sent[1].messages,
                [
                    Message::new(Role::System, HEAD),
                    user("make a marker"),
                    call_message("call-1", "touch marker"),
                    result.clone(),
                ]
            );
            assert_eq!(
                session.trunk()[1..],
                [
                    user("make a marker"),
                    call_message("call-1", "touch marker"),
                    result,
                    assistant("done"),
                ]
            );
            tidy(&[&tree, &runner]);
        }
    }

    /// T7, restated by the amendment: a denylisted segment in a chain is
    /// refused, nothing of the line runs, the model is told, and the turn
    /// goes on.
    #[test]
    fn a_denylisted_segment_refuses_the_line_and_the_model_is_told_and_the_turn_goes_on() {
        let tree = scratch("t7");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch marker; sudo id")],
                deltas(&["understood"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let [line] = lines(&log).try_into().expect("one line");
        assert_eq!(line.outcome, log::ToolOutcome::Refused);
        assert_eq!(line.reason, Some(log::ToolRefusal::Denylist));
        assert_eq!(line.argv, Some(tool_loop::argv_of("touch marker; sudo id")));
        assert!(
            !tree.join("marker").exists(),
            "no segment of a refused line ran"
        );
        let sent = session.shared.transport.sent();
        let shown = &sent[1].messages.last().expect("a result").content;
        assert_eq!(
            shown,
            &tool_loop::refusal_text(log::ToolRefusal::Denylist, "sudo"),
            "the model sees the refusal"
        );
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        tidy(&[&tree]);
    }

    /// T8 and Q12: the `max_steps`-th request's call is received and not
    /// run; the turn settles `max_steps`, and the calls that ran stay on the
    /// trunk.
    #[test]
    fn the_last_steps_call_is_refused_max_steps_and_the_ran_calls_stay_on_the_trunk() {
        let tree = scratch("t8");
        let max = 3;
        let replies: Vec<Vec<Step>> = (0..=max)
            .map(|n| vec![bash(&format!("call-{n}"), &format!("touch m{n}"))])
            .collect();
        let session = Session::open_looping(
            Canned::new(replies),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                Some(max),
                Decider::Decline,
            ),
        );
        session.ask("loop", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        assert_eq!(session.shared.transport.sent().len(), max as usize);
        assert_eq!(settled_as(&log), Some(SettleReason::MaxSteps));
        let written = lines(&log);
        assert_eq!(written.len(), max as usize);
        assert_eq!(written[2].reason, Some(log::ToolRefusal::MaxSteps));
        assert!(
            written[..2]
                .iter()
                .all(|l| l.outcome == log::ToolOutcome::Ran)
        );
        assert!(!tree.join("m2").exists(), "the last call was not run");
        let trunk = session.trunk();
        assert_eq!(trunk.len(), 1 + 1 + 2 * 2, "{trunk:#?}");
        assert_eq!(trunk[1], user("loop"));
        assert_eq!(trunk[4], call_message("call-1", "touch m1"));
        tidy(&[&tree]);
    }

    /// A turn that fails after its tool steps keeps them on the trunk, as a
    /// `max_steps` turn does (#29): two calls ran, then the third request's
    /// answer hit the output cap and the turn settled `failed`. The ask and
    /// both exchanges stay on the trunk, the next ask's request carries them,
    /// and the record's projection rebuilds every head from the log.
    #[test]
    fn a_turn_that_fails_after_its_tool_steps_keeps_them_on_the_trunk() {
        let tree = scratch("failed-keeps-steps");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch a")],
                vec![bash("call-2", "touch b")],
                vec![
                    Step::Delta("cut off".to_owned()),
                    Step::FinishReason("length".to_owned()),
                ],
                deltas(&["done"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("work", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Failed));
        assert!(tree.join("a").exists() && tree.join("b").exists());
        let trunk = session.trunk();
        assert_eq!(trunk.len(), 1 + 1 + 2 * 2, "{trunk:#?}");
        assert_eq!(trunk[1], user("work"));
        assert_eq!(trunk[2], call_message("call-1", "touch a"));
        assert_eq!(trunk[4], call_message("call-2", "touch b"));

        session.ask("next", None).expect("accepted");
        let log = wait_until(&session, "the second turn to settle", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        reads_whole(&session);
        let sent = session.shared.transport.sent();
        assert_eq!(sent.len(), 4);
        assert_eq!(
            sent[3].messages[..trunk.len()],
            trunk[..],
            "the next ask's request carries the failed turn's steps"
        );
        let lines: Vec<_> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        let unrebuilt: Vec<&str> = projected
            .unspellable
            .iter()
            .filter(|named| named.why.contains("could not be rebuilt"))
            .map(|named| named.why.as_str())
            .collect();
        assert!(unrebuilt.is_empty(), "{unrebuilt:#?}");
        tidy(&[&tree]);
    }

    /// A call to an undeclared tool fails the turn after a step that ran:
    /// the step that ran stays on the trunk, the failing step does not.
    #[test]
    fn a_turn_failed_by_an_undeclared_tool_keeps_the_step_before_it() {
        let tree = scratch("failed-unknown-keeps-step");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch a")],
                vec![Step::call(0, "call-2", "python", "{}")],
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("work", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Failed));
        let trunk = session.trunk();
        assert_eq!(trunk.len(), 1 + 1 + 2, "{trunk:#?}");
        assert_eq!(trunk[2], call_message("call-1", "touch a"));
        tidy(&[&tree]);
    }

    /// A cancel reaches a call that is running (#551): its process group is
    /// killed, the call is logged `cancelled`, and the turn settles
    /// `cancelled` within seconds.
    #[test]
    fn a_cancel_stops_a_running_call_and_settles_the_turn_cancelled() {
        let tree = scratch("cancel-running-call");
        let session = Session::open_looping(
            Canned::new([vec![bash("call-1", "sleep 600")], deltas(&["never"])]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["sleep"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("wait", None).expect("accepted");
        wait_until(&session, "the call to be made", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::Called { .. }))
        });
        std::thread::sleep(Duration::from_millis(300));
        let asked = Instant::now();
        session.cancel(1, None).expect("the turn is in flight");
        let log = wait_until(&session, "the turn to settle", settled);
        assert!(
            asked.elapsed() < Duration::from_secs(5),
            "{:?}",
            asked.elapsed()
        );
        assert_eq!(settled_as(&log), Some(SettleReason::Cancelled));
        // The log reads: a cancelled line carries no streams.
        reads_whole(&session);
        let written = lines(&log);
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].outcome, log::ToolOutcome::Cancelled);
        tidy(&[&tree]);
    }

    /// A call that leaves a server running in the background returns, and a
    /// later call in the same session still runs (#551): the server's held
    /// pipes do not poison the next call.
    #[test]
    fn a_call_after_a_backgrounded_server_still_runs() {
        let tree = scratch("after-background");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "sleep 30 & echo started")],
                vec![bash("call-2", "echo second")],
                deltas(&["done"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["sleep", "echo"],
                None,
                Decider::Decline,
            ),
        );
        let asked = Instant::now();
        session.ask("serve", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        assert!(
            asked.elapsed() < Duration::from_secs(10),
            "{:?}",
            asked.elapsed()
        );
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        let written = lines(&log);
        assert_eq!(written.len(), 2, "{written:?}");
        assert!(
            written.iter().all(|l| l.outcome == log::ToolOutcome::Ran),
            "{written:?}"
        );
        let said = |line: &ToolLine| line.stdout.as_ref().map(|out| out.text.clone());
        assert_eq!(said(&written[0]).as_deref(), Some("started\n"));
        assert_eq!(said(&written[1]).as_deref(), Some("second\n"));
        tidy(&[&tree]);
    }

    /// `bash` carries its description on the wire (#558), and a log
    /// written before it -- `bash` as I0 captured it -- still rebuilds every
    /// head; a definition that is neither leaves the heads unverified.
    #[test]
    fn bash_heads_rebuild_with_its_description_and_without_it_before_558() {
        let run = |bash_tool: crate::client::shape::ToolDefinition| {
            let tree = scratch("bash-description");
            let session = Session::open_looping(
                Canned::new([vec![bash("call-1", "echo hi")], deltas(&["done"])]),
                RequestShape {
                    tools: vec![bash_tool],
                    ..template()
                },
                None,
                tools(
                    Confinement::Unconfined,
                    &tree,
                    &["echo"],
                    None,
                    Decider::Decline,
                ),
            );
            session.ask("say hi", None).expect("accepted");
            let log = wait_until(&session, "the turn to settle", settled);
            assert_eq!(settled_as(&log), Some(SettleReason::Final));
            let sent = session.shared.transport.sent();
            tidy(&[&tree]);
            (log, sent)
        };
        let (log, sent) = run(tool_loop::bash_tool());
        assert_eq!(
            sent[0].tools[0].description.as_deref(),
            Some(tool_loop::BASH_DESCRIPTION)
        );
        every_head_rebuilds(&log);
        let (log, _) = run(tool_loop::bash_tool_before_its_description());
        every_head_rebuilds(&log);
        let mut neither = tool_loop::bash_tool();
        neither.description = Some("something else".to_owned());
        let (log, _) = run(neither);
        let lines: Vec<_> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        assert!(
            projected
                .unspellable
                .iter()
                .any(|named| named.why.contains("could not be rebuilt")),
            "a definition the log never sent verifies nothing"
        );
    }

    /// Every head the log's projection rebuilds is verified: none is named
    /// as one it could not rebuild.
    fn every_head_rebuilds(log: &[Logged]) {
        let lines: Vec<_> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        let unrebuilt: Vec<&str> = projected
            .unspellable
            .iter()
            .filter(|named| named.why.contains("could not be rebuilt"))
            .map(|named| named.why.as_str())
            .collect();
        assert!(unrebuilt.is_empty(), "{unrebuilt:#?}");
    }

    /// A turn cancelled after a finished call, while its next call runs
    /// (#575): the finished step stays on the trunk, the running call is
    /// killed and answered with what it printed and Qwen Code's cancel
    /// result, the next ask carries all of it, and the projection rebuilds
    /// every head.
    #[test]
    fn a_cancelled_turn_keeps_its_finished_steps_and_answers_the_running_call() {
        let tree = scratch("cancel-keeps-steps");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch a")],
                vec![bash("call-2", "echo partway; sleep 600")],
                deltas(&["next"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch", "echo", "sleep"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("work", None).expect("accepted");
        wait_until(&session, "the second call to be made", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::Called { .. }))
                .count()
                == 2
        });
        std::thread::sleep(Duration::from_millis(300));
        session.cancel(1, None).expect("the turn is in flight");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Cancelled));
        let trunk = session.trunk();
        assert_eq!(trunk.len(), 1 + 1 + 2 * 2, "{trunk:#?}");
        assert_eq!(trunk[1], user("work"));
        assert_eq!(trunk[2], call_message("call-1", "touch a"));
        assert_eq!(trunk[4], call_message("call-2", "echo partway; sleep 600"));
        assert_eq!(
            trunk[5].content,
            format!("partway\n\n{CANCELLED_CALL}"),
            "what it printed, then the cancel"
        );
        let written = lines(&log);
        assert_eq!(written[1].outcome, log::ToolOutcome::Cancelled);
        assert_eq!(written[1].shown.as_deref(), Some(trunk[5].content.as_str()));

        session.ask("next", None).expect("accepted after a cancel");
        let log = wait_until(&session, "the second turn to settle", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        reads_whole(&session);
        let sent = session.shared.transport.sent();
        assert_eq!(
            sent[2].messages[..trunk.len()],
            trunk[..],
            "the next ask carries the cancelled turn's steps"
        );
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// A call the cancel reached before it started -- the second of one
    /// message -- never runs, and is answered with Qwen Code's repair text
    /// (#575), so every call of the kept step has its result.
    #[test]
    fn a_call_a_cancel_reached_before_it_started_is_answered_as_unrecorded() {
        let tree = scratch("cancel-unstarted");
        let session = Session::open_looping(
            Canned::new([vec![
                Step::call(0, "call-1", "bash", r#"{"command":"sleep 600"}"#),
                Step::call(1, "call-2", "bash", r#"{"command":"touch never"}"#),
            ]]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch", "sleep"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("work", None).expect("accepted");
        wait_until(&session, "the calls to be made", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::Called { .. }))
        });
        std::thread::sleep(Duration::from_millis(300));
        session.cancel(1, None).expect("the turn is in flight");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Cancelled));
        assert!(!tree.join("never").exists(), "the unstarted call ran");
        let trunk = session.trunk();
        assert_eq!(trunk.len(), 1 + 1 + 3, "{trunk:#?}");
        assert_eq!(
            trunk[3].content, CANCELLED_CALL,
            "the running call printed nothing"
        );
        assert_eq!(trunk[4].content, UNRECORDED_CALL);
        assert_eq!(trunk[4].tool_call_id.as_deref(), Some("call-2"));
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// Cancelled while generating after a finished step: the step stays, and
    /// so does the text said so far (#575).
    #[test]
    fn a_turn_cancelled_while_generating_keeps_its_step_and_its_text() {
        let tree = scratch("cancel-generating");
        let gate = Gate::new();
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch a")],
                vec![Step::Delta("so far".to_owned()), Step::Hold(gate.clone())],
                deltas(&["next"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("work", None).expect("accepted");
        assert!(
            gate.wait_for_a_waiter(Duration::from_secs(10)),
            "never held"
        );
        session.cancel(1, None).expect("the turn is in flight");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Cancelled));
        let trunk = session.trunk();
        assert_eq!(trunk.len(), 1 + 1 + 2 + 1, "{trunk:#?}");
        assert_eq!(trunk[2], call_message("call-1", "touch a"));
        assert_eq!(trunk[4], Message::new(Role::Assistant, "so far"));
        session.ask("next", None).expect("accepted after a cancel");
        let log = wait_until(&session, "the second turn to settle", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// A session whose one turn runs `commands`, one call each, then
    /// answers; its tools capped as `cap`, keeping capped outputs in
    /// `recording`.
    fn capping_session(
        tree: &Path,
        commands: &[&str],
        cap: crate::drive::output::OutputCap,
        recording: Option<&Path>,
    ) -> Session<Canned> {
        let mut replies: Vec<Vec<Step>> = commands
            .iter()
            .enumerate()
            .map(|(n, command)| vec![bash(&format!("call-{n}"), command)])
            .collect();
        replies.push(deltas(&["done"]));
        let mut tools = tools(
            Confinement::Unconfined,
            tree,
            &["seq", "sed", "echo"],
            None,
            Decider::Decline,
        );
        tools.output_cap = cap;
        tools.recording = recording.map(Path::to_path_buf);
        // The gate is not under test: a read of the recording runs.
        tools.approvals_off = true;
        Session::open_looping(Canned::new(replies), looping(), None, tools)
    }

    /// #554: an output over the cap reaches the model capped -- Qwen Code's
    /// notice naming the kept file, the head, the separator, the tail -- and
    /// the whole of it is kept in the recording by digest, named on the
    /// line's `files`; a later call reads a slice of it through `bash`. The
    /// session's start names the cap, the log reads, and the projection
    /// rebuilds every head and carries the cap onto the record's start.
    #[test]
    fn an_output_over_the_cap_is_capped_kept_whole_by_digest_and_readable_in_slices() {
        use crate::drive::output::{OutputCap, SEPARATOR};
        let tree = scratch("cap-over");
        let recording = scratch("cap-over-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let whole: String = (1..=5000)
            .flat_map(|n| [n.to_string(), "\n".to_owned()])
            .collect();
        let sha256 = crate::digest::sha256_hex(whole.as_bytes());
        let kept = recording.join("files").join(&sha256);
        let slice = format!("sed -n '2500,2502p' \"{}\"", kept.display());
        let session = capping_session(
            &tree,
            &["seq 1 5000", &slice],
            OutputCap::DEFAULT,
            Some(&recording),
        );
        session.ask("count", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        reads_whole(&session);
        let written = lines(&log);
        let shown = written[0].shown.clone().expect("shown");
        assert!(
            shown.starts_with("Tool output was too large and has been truncated.\n"),
            "{shown}"
        );
        assert!(
            shown.contains(&format!(
                "The full output has been saved to: {}\n",
                kept.display()
            )),
            "{shown}"
        );
        let (head, tail) = shown
            .split_once("Truncated part of the output:\n")
            .expect("the notice")
            .1
            .split_once(SEPARATOR)
            .expect("the separator");
        assert!(
            head.starts_with("1\n2\n") && tail.ends_with("4999\n5000\n"),
            "{shown}"
        );
        assert_eq!(head.lines().count(), 400);
        assert_eq!(tail.lines().count(), 1600);
        assert_eq!(
            written[0].files,
            [log::RecordedFile {
                path: format!("files/{sha256}"),
                sha256: sha256.clone(),
                media_type: "text/plain".to_owned(),
                bytes: whole.len() as u64,
            }]
        );
        assert_eq!(std::fs::read_to_string(&kept).expect("kept whole"), whole);
        // The log keeps the whole stream; the model was shown the cap.
        assert_eq!(
            written[0].stdout.as_ref().map(|out| out.text.as_str()),
            Some(whole.as_str())
        );
        assert_eq!(written[1].shown.as_deref(), Some("2500\n2501\n2502\n"));
        let Event::Started { tool_output, .. } = &log[0].event else {
            panic!("the log opens with the session");
        };
        assert_eq!(*tool_output, Some(OutputCap::DEFAULT));
        every_head_rebuilds(&log);
        let lines: Vec<_> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        assert!(
            matches!(
                projected.events.first(),
                Some(crate::formats::record::Event::Start {
                    tool_output: Some(log::ToolOutput {
                        state: log::ToolOutputState::Capped,
                        ..
                    }),
                    ..
                })
            ),
            "the record's start names the cap"
        );
        // And the record reads back with it.
        let record = crate::formats::record::Record {
            events: projected.events.clone(),
        };
        let read = crate::formats::record::parse(&crate::formats::record::render(&record))
            .expect("the record reads");
        assert_eq!(
            read.tool_output().map(|cap| (cap.max_lines, cap.max_bytes)),
            Some((Some(2000), Some(51_200)))
        );
        tidy(&[&tree, &recording]);
    }

    /// Within the cap the output is shown whole and nothing is kept; with
    /// no recording, an output over it is capped and the notice says it was
    /// not saved; with the cap off, it is shown whole, and the start says
    /// `keep`.
    #[test]
    fn under_the_cap_without_a_recording_or_with_the_cap_off_nothing_is_kept() {
        use crate::drive::output::OutputCap;
        let tree = scratch("cap-under");
        let recording = scratch("cap-under-recording");
        let session = capping_session(&tree, &["echo short"], OutputCap::DEFAULT, Some(&recording));
        session.ask("say", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let written = lines(&log);
        assert_eq!(written[0].shown.as_deref(), Some("short\n"));
        assert!(written[0].files.is_empty());
        assert!(!recording.join("files").exists(), "something was kept");

        let session = capping_session(&tree, &["seq 1 5000"], OutputCap::DEFAULT, None);
        session.ask("count", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let written = lines(&log);
        let shown = written[0].shown.clone().expect("shown");
        assert!(
            shown.contains("The full output was not saved.\n"),
            "{shown}"
        );
        assert!(written[0].files.is_empty());

        let session = capping_session(&tree, &["seq 1 5000"], OutputCap::Keep, Some(&recording));
        session.ask("count", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let written = lines(&log);
        assert_eq!(
            written[0].shown.as_deref().map(str::len),
            written[0].stdout.as_ref().map(|out| out.text.len())
        );
        assert!(!recording.join("files").exists(), "something was kept");
        let Event::Started { tool_output, .. } = &log[0].event else {
            panic!("the log opens with the session");
        };
        assert_eq!(*tool_output, Some(OutputCap::Keep));
        tidy(&[&tree, &recording]);
    }

    /// The standard surface (#557): the request declares `bash` and the
    /// standard tools with their descriptions; the model writes, reads and
    /// edits a file in the worktree; each call is a line under its tool's
    /// name with the result it was shown; the log reads whole and the
    /// projection rebuilds every head from the declared names.
    #[test]
    fn the_standard_surface_writes_reads_and_edits_and_every_head_rebuilds() {
        let tree = scratch("standard-surface");
        let write = r#"{"path":"notes.txt","content":"alpha\nbeta\n"}"#;
        let read = r#"{"path":"notes.txt"}"#;
        let edit = r#"{"file_path":"notes.txt","old_string":"beta","new_string":"gamma"}"#;
        let mut shape = looping();
        shape.tools = tool_loop::ToolSurface::Standard.tools();
        let mut tools = tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline);
        tools.surface = tool_loop::ToolSurface::Standard;
        tools.read_tool = tool_loop::ToolSurface::Standard.read_tool();
        let session = Session::open_looping(
            Canned::new([
                vec![Step::call(0, "call-1", "write", write)],
                vec![Step::call(0, "call-2", "read", read)],
                vec![Step::call(0, "call-3", "edit", edit)],
                deltas(&["done"]),
            ]),
            shape,
            None,
            tools,
        );
        session.ask("keep notes", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        reads_whole(&session);
        let written = lines(&log);
        let names: Vec<&str> = written.iter().map(|line| line.name.as_str()).collect();
        assert_eq!(names, ["write", "read", "edit"]);
        assert!(
            written
                .iter()
                .all(|line| line.outcome == log::ToolOutcome::Ran)
        );
        assert_eq!(
            written[0].shown.as_deref(),
            Some("Successfully created and wrote to new file: notes.txt.")
        );
        assert_eq!(written[1].shown.as_deref(), Some("alpha\nbeta\n"));
        assert!(
            written[2]
                .shown
                .as_deref()
                .is_some_and(|shown| shown.starts_with("The file: notes.txt has been updated."))
        );
        assert_eq!(
            std::fs::read_to_string(tree.join("notes.txt")).expect("the file"),
            "alpha\ngamma\n"
        );
        let sent = session.shared.transport.sent();
        let declared: Vec<(&str, bool)> = sent[0]
            .tools
            .iter()
            .map(|tool| (tool.name.as_str(), tool.description.is_some()))
            .collect();
        assert_eq!(
            declared,
            [
                ("bash", true),
                ("read", true),
                ("write", true),
                ("edit", true),
                ("grep", true),
                ("glob", true)
            ]
        );
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// `read` of an image (#557): the result goes back as an image part
    /// inside the tool message, with no words beside it, the image kept in
    /// the recording by digest and named on the line's `files`; every head
    /// rebuilds from the recording, and without one the head that carries
    /// the image is named unrebuilt.
    #[test]
    fn a_read_image_rides_in_the_tool_message_and_every_head_rebuilds() {
        let tree = scratch("standard-image");
        let recording = scratch("standard-image-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let png = b"\x89PNG\r\n\x1a\nsome pixels".to_vec();
        std::fs::write(tree.join("shot.png"), &png).expect("written");
        let sha256 = crate::digest::sha256_hex(&png);
        let mut shape = looping();
        shape.tools = tool_loop::ToolSurface::Standard.tools();
        let mut tools = tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline);
        tools.surface = tool_loop::ToolSurface::Standard;
        tools.read_tool = tool_loop::ToolSurface::Standard.read_tool();
        tools.recording = Some(recording.clone());
        let session = Session::open_looping(
            Canned::new([
                vec![Step::call(0, "call-1", "read", r#"{"path":"shot.png"}"#)],
                deltas(&["red"]),
            ]),
            shape,
            None,
            tools,
        );
        session.ask("what colour?", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        reads_whole(&session);
        let written = lines(&log);
        assert_eq!(written[0].outcome, log::ToolOutcome::Ran);
        assert_eq!(written[0].shown.as_deref(), Some(""));
        let file = log::RecordedFile {
            path: format!("files/{sha256}"),
            sha256: sha256.clone(),
            media_type: "image/png".to_owned(),
            bytes: png.len() as u64,
        };
        assert_eq!(written[0].files, std::slice::from_ref(&file));
        assert!(written[0].images.is_empty(), "the bytes are never logged");
        assert_eq!(
            std::fs::read(recording.join(&file.path)).expect("kept"),
            png
        );
        let sent = session.shared.transport.sent();
        let result = sent[1].messages.last().expect("the result");
        assert_eq!(result.role, Role::Tool);
        assert_eq!(result.tool_call_id.as_deref(), Some("call-1"));
        assert_eq!(result.content, "");
        assert_eq!(
            result.images,
            crate::client::attach(Message::new(Role::Tool, ""), &file, &png)
                .expect("the file's bytes")
                .images
        );
        let lines: Vec<_> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project_in(&lines, &regime(), None, Some(&recording))
                .expect("projected");
        let unrebuilt = |projected: &crate::drive::projection::Projection| -> Vec<String> {
            projected
                .unspellable
                .iter()
                .filter(|named| named.why.contains("could not be rebuilt"))
                .map(|named| named.why.clone())
                .collect()
        };
        assert!(
            unrebuilt(&projected).is_empty(),
            "{:#?}",
            unrebuilt(&projected)
        );
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        let named = unrebuilt(&projected);
        assert!(
            named.iter().any(|why| why.contains("call call-1's image")),
            "{named:#?}"
        );
        tidy(&[&tree, &recording]);
    }

    /// A turn that fails on its first request ran nothing, and keeps nothing:
    /// the trunk is as it was (#289's rule, unchanged for it).
    #[test]
    fn a_turn_that_fails_before_any_step_keeps_nothing() {
        let tree = scratch("failed-keeps-nothing");
        let session = Session::open_looping(
            Canned::new([vec![Step::Reject(500, "broken".to_owned())]]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("work", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(settled_as(&log), Some(SettleReason::Failed));
        assert_eq!(session.trunk().len(), 1, "only the head");
        tidy(&[&tree]);
    }

    /// T13: a call to a tool the session never declared runs nothing, is not
    /// answered, and fails the turn naming the call.
    #[test]
    fn an_undeclared_call_runs_nothing_and_fails_the_turn() {
        // With no loop at all, and with one whose request declares no tool:
        // a `touch` the pre-seed would run, had the call been declared.
        for looped in [false, true] {
            let tree = scratch(&format!("t13-{looped}"));
            let canned = Canned::new([vec![bash("call-1", "touch marker")], deltas(&["never"])]);
            let session = if looped {
                Session::open_looping(
                    canned,
                    template(),
                    None,
                    tools(
                        Confinement::Unconfined,
                        &tree,
                        &["touch"],
                        None,
                        Decider::Decline,
                    ),
                )
            } else {
                Session::open(canned, template())
            };
            session.ask("go", None).expect("accepted");
            let log = wait_until(&session, "the turn to settle", settled);
            reads_whole(&session);
            assert_eq!(session.shared.transport.sent().len(), 1);
            assert_eq!(settled_as(&log), Some(SettleReason::Failed));
            let [line] = lines(&log).try_into().expect("one line");
            assert_eq!(line.reason, Some(log::ToolRefusal::UnknownTool));
            assert_eq!(line.name, "bash", "the line names the undeclared call");
            assert_eq!(line.argv, None, "an undeclared call was never parsed");
            assert!(!tree.join("marker").exists(), "an undeclared call ran");
            assert_eq!(session.trunk().len(), 1, "nothing joined the trunk");
            tidy(&[&tree]);
        }
    }

    #[test]
    fn arguments_that_are_not_the_tools_json_are_refused_unparsable_and_the_model_is_told() {
        let tree = scratch("unparsable");
        let session = Session::open_looping(
            Canned::new([
                vec![Step::call(0, "call-1", "bash", "{\"cmd\":\"ls\"}")],
                deltas(&["ok"]),
            ]),
            looping(),
            None,
            tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline),
        );
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let [line] = lines(&log).try_into().expect("one line");
        assert_eq!(line.reason, Some(log::ToolRefusal::Unparsable));
        assert_eq!(line.argv, None);
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        tidy(&[&tree]);
    }

    /// The gym declines every prompt: a command no pre-seed covers is
    /// refused `declined`, and the turn goes on.
    #[test]
    fn the_gym_declines_every_prompt_and_the_turn_goes_on() {
        let tree = scratch("gym");
        let session = Session::open_looping(
            Canned::new([vec![bash("call-1", "touch marker")], deltas(&["ok"])]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["ls"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let [line] = lines(&log).try_into().expect("one line");
        assert_eq!(line.reason, Some(log::ToolRefusal::Declined));
        assert!(line.approval.is_none());
        assert!(!tree.join("marker").exists());
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        tidy(&[&tree]);
    }

    /// (e): a free read runs as git with its configured programs off, and
    /// its line keeps the model's own argv.
    #[test]
    fn a_free_git_read_runs_with_its_overrides_and_records_the_models_argv() {
        let tree = scratch("free-read");
        let init = std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(&tree)
            .status()
            .expect("git runs");
        assert!(init.success());
        let session = Session::open_looping(
            Canned::new([vec![bash("call-1", "git status")], deltas(&["clean"])]),
            looping(),
            None,
            tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline),
        );
        session.ask("status?", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let [line] = lines(&log).try_into().expect("one line");
        assert_eq!(line.outcome, log::ToolOutcome::Ran, "{line:?}");
        assert_eq!(line.argv, Some(tool_loop::argv_of("git status")));
        assert_eq!(
            line.confined,
            Some(super::super::tool_loop::tests::free_read_of(&[
                "status",
                "--ignore-submodules=all"
            ]))
        );
        assert_eq!(line.approval, None, "a free read ran under no decision");
        tidy(&[&tree]);
    }

    /// A step's request the server refuses with a 500 is the existing
    /// `request.failed` shape, reason `server`, and it is not sent again
    /// (#298's line from #406).
    #[test]
    fn a_steps_500_is_a_typed_failure_and_never_a_silent_retry() {
        let tree = scratch("five-hundred");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch marker")],
                vec![Step::Reject(
                    500,
                    "{\"error\":{\"message\":\"busy\"}}".to_owned(),
                )],
                deltas(&["never sent"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        assert_eq!(session.shared.transport.sent().len(), 2, "no retry");
        assert!(log.iter().any(|logged| matches!(
            line_of(logged).event,
            log::Event::RequestFailed {
                reason: log::FailReason::Server,
                status: Some(500),
                ..
            }
        )));
        assert_eq!(settled_as(&log), Some(SettleReason::Failed));
        tidy(&[&tree]);
    }

    fn waiting_on<S: Streaming>(session: &Session<S>) -> Prompt {
        let give_up = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(prompt) = session.waiting() {
                return prompt;
            }
            assert!(Instant::now() < give_up, "no prompt came to wait");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// Point 8: the loop waits on the operator; a session approval runs the
    /// call and covers the next of its shape with no prompt; the line says
    /// when it was decided and why it prompted.
    #[test]
    fn an_operators_session_approval_runs_the_call_and_covers_the_next_of_its_shape() {
        let tree = scratch("approve");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch a")],
                vec![bash("call-2", "touch b")],
                deltas(&["done"]),
            ]),
            looping(),
            None,
            tools(Confinement::Unconfined, &tree, &[], None, Decider::Operator),
        );
        session.ask("go", None).expect("accepted");
        let prompt = waiting_on(&session);
        assert_eq!(prompt.id, "call-1");
        assert_eq!(prompt.command, "touch a");
        assert_eq!(prompt.cwd, "~/git/a-worktree");
        assert_eq!(prompt.reason, "not_approved");
        assert!(prompt.standing);
        assert_eq!(
            session.approve("call-9", Decision::Session),
            Err(ApproveRefusal::Stale)
        );
        assert_eq!(
            session.approve("call-1", Decision::Workspace),
            Err(ApproveRefusal::NoStore)
        );
        assert_eq!(session.approve("call-1", Decision::Session), Ok(()));
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let written = lines(&log);
        assert_eq!(written.len(), 2);
        assert!(written.iter().all(|l| l.outcome == log::ToolOutcome::Ran));
        let first = written[0].approval.clone().expect("an approval");
        assert_eq!(first.scope, log::ApprovalScope::Session);
        assert_eq!(first.why.as_deref(), Some("not_approved"));
        assert_eq!(
            written[1].approval,
            Some(first),
            "the second ran under the first decision, unprompted"
        );
        assert!(tree.join("a").exists() && tree.join("b").exists());
        assert_eq!(
            session.approve("call-1", Decision::Once),
            Err(ApproveRefusal::NothingWaiting)
        );
        tidy(&[&tree]);
    }

    /// The approval lever's `none`: a call that would prompt runs with no
    /// gate decision and nothing waiting on the operator, its line says
    /// approvals were off, and so does `session.start`.
    #[test]
    fn with_approvals_off_a_call_that_would_prompt_runs_and_says_so() {
        let tree = scratch("approvals-off");
        let mut off = tools(Confinement::Unconfined, &tree, &[], None, Decider::Operator);
        off.approvals_off = true;
        let session = Session::open_looping(
            Canned::new([vec![bash("call-1", "touch a")], deltas(&["done"])]),
            looping(),
            None,
            off,
        );
        session.ask("go", None).expect("accepted");
        // Settles with no `approve`: an operator decider would wait forever.
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        assert!(session.waiting().is_none());
        let written = lines(&log);
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].outcome, log::ToolOutcome::Ran);
        assert_eq!(
            written[0].approval,
            Some(log::Approval {
                scope: log::ApprovalScope::Off,
                decided_at: None,
                why: None,
            })
        );
        assert!(tree.join("a").exists());
        assert!(matches!(
            line_of(&log[0]).event,
            log::Event::SessionStart {
                approvals_off: Some(true),
                ..
            }
        ));
        tidy(&[&tree]);
    }

    /// The served template's own format, as the model would write it in its
    /// answer rather than as a native call (#560).
    const WRITTEN_CALL: &str = "Running it.\n<tool_call>\n<function=bash>\n\
         <parameter=command>\ntouch made\n</parameter>\n</function>\n</tool_call>";

    fn falling_back(tree: &Path, on: bool, answer: &str) -> Session<Canned> {
        let mut loop_tools = tools(
            Confinement::Unconfined,
            tree,
            &["touch"],
            None,
            Decider::Decline,
        );
        loop_tools.text_fallback = on;
        Session::open_looping(
            Canned::new([deltas(&[answer]), deltas(&["done"])]),
            looping(),
            None,
            loop_tools,
        )
    }

    /// #560, fallback on: a turn that wrote its call as text runs it, the
    /// line says what text it came from, the trunk carries the answer with
    /// the block taken out and the call as a call, and the projection
    /// rebuilds that head.
    #[test]
    fn with_the_text_fallback_on_a_call_written_as_text_runs() {
        let tree = scratch("text-fallback-on");
        let session = falling_back(&tree, true, WRITTEN_CALL);
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let written = lines(&log);
        assert_eq!(written.len(), 1, "{log:#?}");
        assert_eq!(written[0].outcome, log::ToolOutcome::Ran);
        assert_eq!(written[0].name, "bash");
        assert_eq!(written[0].arguments, r#"{"command":"touch made"}"#);
        assert!(written[0].id.starts_with("recovered-"), "{}", written[0].id);
        let source = written[0].recovered_from.as_deref().expect("recovered");
        assert!(
            source.starts_with("<function=bash>") && source.ends_with("</function>"),
            "{source}"
        );
        assert!(tree.join("made").exists());
        // The answer as the model wrote it stays in the log.
        assert!(
            log.iter()
                .any(|l| matches!(&l.event, Event::Called { text, .. } if text == WRITTEN_CALL))
        );
        // The next request carries what was left, and the call as a call.
        let sent = session.shared.transport.sent();
        assert_eq!(sent.len(), 2);
        let said = sent[1]
            .messages
            .iter()
            .find(|message| message.role == Role::Assistant)
            .expect("the step's message");
        assert_eq!(said.content, "Running it.");
        assert_eq!(said.tool_calls.len(), 1);
        let lines_of: Vec<log::Line> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines_of, &regime(), None).expect("projected");
        assert!(
            !projected
                .unspellable
                .iter()
                .any(|item| item.why.contains("rebuilt")),
            "{:?}",
            projected.unspellable
        );
        tidy(&[&tree]);
    }

    /// #560, fallback off (the default, as Pi and `OpenCode` 2 have it): the
    /// same answer is an answer, and nothing runs.
    #[test]
    fn with_the_text_fallback_off_a_call_written_as_text_is_an_answer() {
        let tree = scratch("text-fallback-off");
        let session = falling_back(&tree, false, WRITTEN_CALL);
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        assert!(lines(&log).is_empty());
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        assert_eq!(session.shared.transport.sent().len(), 1);
        assert!(!tree.join("made").exists());
        tidy(&[&tree]);
    }

    /// #560: a malformed block -- its function never closes -- is not
    /// dispatched, as Qwen Code leaves it inert: the turn is an answer.
    #[test]
    fn a_malformed_written_call_is_left_as_an_answer() {
        let tree = scratch("text-fallback-malformed");
        let session = falling_back(
            &tree,
            true,
            "<function=bash><parameter=command>touch made</parameter>",
        );
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        assert!(lines(&log).is_empty());
        assert_eq!(settled_as(&log), Some(SettleReason::Final));
        assert!(!tree.join("made").exists());
        tidy(&[&tree]);
    }

    /// A call whose segments are covered partly by a pre-seed and partly by
    /// the operator's session approval records the operator's approval, the
    /// one it needed: `ls | wc -l` with `ls` pre-seeded and `wc` approved
    /// for the session records `session` on its repeat, never `preseeded`
    /// (the approval trace is a measurement).
    #[test]
    fn a_repeat_covered_by_a_session_approval_records_session_not_the_preseed() {
        let tree = scratch("approve-mixed");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "ls | wc -l")],
                vec![bash("call-2", "ls | wc -l")],
                deltas(&["done"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["ls"],
                None,
                Decider::Operator,
            ),
        );
        session.ask("go", None).expect("accepted");
        let prompt = waiting_on(&session);
        assert_eq!(prompt.command, "ls | wc -l");
        assert_eq!(session.approve("call-1", Decision::Session), Ok(()));
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let written = lines(&log);
        assert_eq!(written.len(), 2);
        assert!(written.iter().all(|l| l.outcome == log::ToolOutcome::Ran));
        let first = written[0].approval.clone().expect("an approval");
        assert_eq!(first.scope, log::ApprovalScope::Session);
        assert_eq!(
            written[1].approval,
            Some(first),
            "the repeat ran under the operator's session approval"
        );
        tidy(&[&tree]);
    }

    #[test]
    fn a_declined_prompt_is_refused_declined_and_a_once_runs_once() {
        let tree = scratch("decline");
        let session = Session::open_looping(
            Canned::new([
                vec![bash("call-1", "touch a")],
                vec![bash("call-2", "touch b")],
                vec![bash("call-3", "touch c")],
                deltas(&["done"]),
            ]),
            looping(),
            None,
            tools(Confinement::Unconfined, &tree, &[], None, Decider::Operator),
        );
        session.ask("go", None).expect("accepted");
        assert_eq!(waiting_on(&session).id, "call-1");
        session
            .approve("call-1", Decision::Decline)
            .expect("answered");
        assert_eq!(waiting_on(&session).id, "call-2");
        session.approve("call-2", Decision::Once).expect("answered");
        assert_eq!(
            waiting_on(&session).id,
            "call-3",
            "once covered nothing after"
        );
        session
            .approve("call-3", Decision::Decline)
            .expect("answered");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let written = lines(&log);
        assert_eq!(written[0].reason, Some(log::ToolRefusal::Declined));
        assert_eq!(
            written[1].approval.as_ref().map(|a| a.scope),
            Some(log::ApprovalScope::Once)
        );
        assert!(!tree.join("a").exists() && tree.join("b").exists());
        tidy(&[&tree]);
    }

    /// A prompt unanswered at a cancel, or at the session's end, is
    /// `cancelled`; there is no timeout.
    #[test]
    fn a_prompt_unanswered_at_cancel_or_end_settles_cancelled() {
        for end in [false, true] {
            let tree = scratch(&format!("unanswered-{end}"));
            let session = Session::open_looping(
                Canned::new([vec![bash("call-1", "touch a")], deltas(&["never"])]),
                looping(),
                None,
                tools(Confinement::Unconfined, &tree, &[], None, Decider::Operator),
            );
            let turn = session.ask("go", None).expect("accepted").turn;
            waiting_on(&session);
            if end {
                session
                    .end(None)
                    .expect("an end is admitted while a prompt waits");
            } else {
                session
                    .cancel(turn, None)
                    .expect("a cancel reaches the prompt");
            }
            let log = wait_until(&session, "the turn to settle", |log| {
                log.iter()
                    .any(|l| matches!(l.event, Event::TurnSettled { .. }))
                    && matches!(
                        log.last().map(|l| &l.event),
                        Some(Event::Settled {
                            to: Settlement::Awaiting | Settlement::Ended,
                            ..
                        })
                    )
            });
            reads_whole(&session);
            let [line] = lines(&log).try_into().expect("one line");
            assert_eq!(line.outcome, log::ToolOutcome::Cancelled);
            assert_eq!(line.isolation, Some(log::Isolation::None));
            assert_eq!(settled_as(&log), Some(SettleReason::Cancelled));
            assert!(!tree.join("a").exists());
            let expected = if end {
                Settlement::Ended
            } else {
                Settlement::Awaiting
            };
            assert_eq!(session.settlement(), expected);
            assert_eq!(session.shared.transport.sent().len(), 1);
            tidy(&[&tree]);
        }
    }

    /// Point 5: a workspace approval is kept under the state directory and
    /// nothing is written into the worktree.
    #[test]
    fn a_workspace_approval_is_kept_outside_the_worktree_and_counted() {
        let tree = scratch("workspace-tree");
        let state = scratch("workspace-state");
        let mut held = tools(Confinement::Unconfined, &tree, &[], None, Decider::Operator);
        let (store, _) =
            tool_loop::Store::open(&state, &tree, &IsolationPolicy::unconfined()).expect("opens");
        let path = store.path().to_path_buf();
        held.store = Some(store);
        let session = Session::open_looping(
            Canned::new([vec![bash("call-1", "date")], deltas(&["ok"])]),
            looping(),
            None,
            held,
        );
        session.ask("go", None).expect("accepted");
        waiting_on(&session);
        session
            .approve("call-1", Decision::Workspace)
            .expect("answered");
        let log = wait_until(&session, "the turn to settle", settled);
        let [line] = lines(&log).try_into().expect("one line");
        assert_eq!(
            line.approval.map(|a| a.scope),
            Some(log::ApprovalScope::Workspace)
        );
        assert!(path.starts_with(&state) && path.exists());
        assert_eq!(std::fs::read_dir(&tree).expect("tree").count(), 0);
        let receipt = session.receipt().expect("a receipt");
        let mut text = String::new();
        crate::formats::record::json::render(&receipt, &mut text);
        assert!(text.contains("\"workspace\":1"), "{text}");
        assert!(text.contains("\"shape\":\"date\""), "{text}");
        assert!(
            text.contains("\"lifecycle_scripts\":\"unguarded\""),
            "{text}"
        );
        tidy(&[&tree, &state]);
    }

    /// I0's turn 2, `openai` shape (#29 I0).
    const I0: &str = "../../../substrates/measurements/2026-10-02-i0-tool-call-captures";

    /// T12: after the round trip over HTTP, the second request is I0's
    /// `openai`-shape turn 2, byte for byte. The wire is under test, so the
    /// `bash` tool is declared as I0 captured it, before its description
    /// (#558).
    #[test]
    fn the_second_request_is_i0s_openai_shape_byte_for_byte() {
        use crate::client::shape::{Pin, SamplerSetting};
        use crate::client::stream::HttpStream;
        use crate::client::stub::{Act, Stub};
        use crate::client::transport::Endpoint;
        const TURN1: &[u8] = include_bytes!(
            "../../../substrates/measurements/2026-10-02-i0-tool-call-captures/turn1.http"
        );
        const TURN2: &[u8] = include_bytes!(
            "../../../substrates/measurements/2026-10-02-i0-tool-call-captures/turn2-openai.http"
        );
        const SENT: &str = include_str!(
            "../../../substrates/measurements/2026-10-02-i0-tool-call-captures/turn2-openai.request.json"
        );
        let output = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/drive")
            .join(I0)
            .join("tool-output.txt");
        let tree = scratch("t12-tree");
        let runner = scratch("t12-runner");
        // The command's output is I0's, whatever this host's `wc` pads.
        let confinement = stand_in(&runner, &format!("cat '{}'", output.display()));
        let stub = Stub::serving(vec![Act::Raw(TURN1.to_vec()), Act::Raw(TURN2.to_vec())])
            .expect("loopback");
        let transport = HttpStream::new(Endpoint::parse(&stub.url()).expect("the stub's endpoint"));
        let shape = RequestShape {
            model: "qwen3.8-flash-next".to_owned(),
            tools: vec![tool_loop::bash_tool_before_its_description()],
            messages: vec![Message::new(
                Role::System,
                "You are working in a git repository. Use the bash tool to run commands.",
            )],
            sampler: SamplerCard::empty()
                .with_decimal(SamplerSetting::Temperature, "0.6")
                .expect("a decimal")
                .with_decimal(SamplerSetting::TopP, "0.95")
                .expect("a decimal")
                .with(SamplerSetting::Seed, Pin::Integer(7)),
            limits: Limits {
                attempt: Duration::from_secs(10),
                call: Duration::from_secs(10),
                max_output_tokens: 512,
                retries: 0,
                context_window: None,
            },
            grammar: None,
            template_kwargs: std::collections::BTreeMap::from([(
                "enable_thinking".to_owned(),
                Value::Boolean(false),
            )]),
        };
        let session = Session::open_looping(
            transport,
            shape,
            None,
            tools(confinement, &tree, &["ls", "wc -l"], None, Decider::Decline),
        );
        session
            .ask(
                "[be135a0c73102528] How many files are in the current directory? Run `ls | wc -l` \
                 with the bash tool and tell me the number.",
                None,
            )
            .expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        assert_eq!(settled_as(&log), Some(SettleReason::Final), "{log:#?}");
        drop(session);
        let received = stub.received();
        assert_eq!(received.len(), 2);
        assert_eq!(received[1], SENT, "the second request is not I0's");
        tidy(&[&tree, &runner]);
    }

    // -----------------------------------------------------------------------
    // the capture gap's fork (#374)
    // -----------------------------------------------------------------------

    /// A regime for the working object: the dev-loop regimen's.
    pub(in crate::drive) fn regime() -> crate::formats::record::Regime {
        let regimen =
            crate::formats::regimen::parse(super::super::canned::DEV_LOOP).expect("a regimen");
        super::super::regimen::regime_of(&regimen, false).expect("a regime")
    }

    /// What the capture gap forks under: `rules`, and an empty object.
    pub(in crate::drive) fn interviewing(rules: &[log::Warrant]) -> Interview {
        Interview {
            rules: rules.to_vec(),
            object: WorkingObject::open(regime()),
            seams: crate::seam::policy::Served::default(),
            delivery: log::ForkDelivery::Seam,
            phases: crate::seam::phase::PhaseGraph::none(),
            recall: super::super::archive::Recall::Off,
            role: Role::User,
            view: None,
            cadence: Cadence::Gap,
            threshold_bytes: None,
            skip_self_recorded: false,
            prune: None,
            self_capture: None,
            asks: &crate::dogma::asks::V3,
            capture: crate::dogma::asks::Modality::Fields,
            fork_tail: None,
        }
    }

    /// A session whose scoping fork leaves three decisions in working memory
    /// and whose seams `shape_it` configures, playing `rest` after the turn
    /// and its fork (#504); the log once the fork settled.
    fn three_decisions_then(
        shape_it: impl FnOnce(&mut crate::seam::policy::Served),
        rest: Vec<Vec<Step>>,
    ) -> Session<Canned> {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        shape_it(&mut interview.seams);
        let mut acts = vec![deltas(&[SCOPED]), deltas(&[DECIDED])];
        acts.extend(rest);
        let session = Session::open_with(
            Canned::new(acts),
            template(),
            None,
            None,
            None,
            Some(interview),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        session
    }

    fn seamed(log: &[Logged]) -> bool {
        log.iter()
            .any(|logged| matches!(logged.event, Event::Seamed { .. }))
            && log.last().is_some_and(|logged| {
                !matches!(
                    logged.event,
                    Event::Settled {
                        to: Settlement::Capture,
                        ..
                    }
                )
            })
    }

    /// #504: with the audit on, an operator's seam puts the dogma's pinned
    /// ask over the three notes on the `audit` lane, off the warm trunk;
    /// its answer -- one line per note -- folds before the refill: `KEEP`
    /// leaves a note, `UPDATE` supersedes it, `REMOVE` retires it. The audit
    /// is a fork line, a request, an answer, a settling and its patches, the
    /// seam's line after them, and the log reads back and projects.
    #[test]
    fn an_audited_seam_folds_its_answer_before_the_refill() {
        let session = three_decisions_then(
            |seams| seams.audit = true,
            vec![deltas(&[
                "1. KEEP\n2. UPDATE: for the one team that files bugs\n3. REMOVE — they asked for logins after all\n",
            ])],
        );
        session.declare_seam(None).expect("admitted");
        let log = wait_until(&session, "the seam", seamed);
        let audited = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Audited {
                    template, question, ..
                } => Some((logged.seq, *template, question.clone())),
                _ => None,
            })
            .expect("an audit");
        assert_eq!(audited.1, crate::dogma::Template::AuditQHuman);
        assert!(
            audited.2.contains("1. decision: a tracker"),
            "{}",
            audited.2
        );
        assert!(log.iter().any(|logged| matches!(
            logged.event,
            Event::Requested { lane: Lane::Audit, fork: Some(fork), .. } if fork == audited.0
        )));
        assert!(log.iter().any(|logged| matches!(
            logged.event,
            Event::ForkSettled { fork, outcome: log::ForkOutcome::Value, .. } if fork == audited.0
        )));
        let seam_at = log
            .iter()
            .find(|logged| matches!(logged.event, Event::Seamed { .. }))
            .expect("the seam")
            .seq;
        assert!(audited.0 < seam_at);
        let live: std::collections::BTreeSet<String> = session
            .shared
            .lock()
            .interview
            .as_ref()
            .expect("memory")
            .object
            .live()
            .map(|entry| entry.content.clone())
            .collect();
        assert_eq!(
            live,
            ["decision: a tracker", "for the one team that files bugs"]
                .map(str::to_owned)
                .into(),
            "kept, updated, and the removed one gone"
        );
        // The refill renders what the audit left.
        let trunk = session.trunk();
        let refill = &trunk[template().messages.len()].content;
        assert!(
            refill.contains("for the one team that files bugs"),
            "{refill}"
        );
        assert!(!refill.contains("no login"), "{refill}");
        assert_eq!(session.settlement(), Settlement::Awaiting);
        reads_whole(&session);
        let lines: Vec<log::Line> = session.events_from(0).iter().map(line_of).collect();
        super::super::projection::project(&lines, &regime(), None).expect("the log projects");
    }

    /// #504: an answer that is not the ask's audit -- prose, or a line short
    /// -- folds nothing and settles `unparseable`; the seam goes on.
    #[test]
    fn an_audit_answer_not_in_the_grammar_folds_nothing_and_the_seam_goes_on() {
        for answer in ["All three look fine to me.", "1. KEEP\n2. KEEP\n"] {
            let session = three_decisions_then(|seams| seams.audit = true, vec![deltas(&[answer])]);
            session.declare_seam(None).expect("admitted");
            let log = wait_until(&session, "the seam", seamed);
            assert!(
                log.iter().any(|logged| matches!(
                    logged.event,
                    Event::ForkSettled {
                        outcome: log::ForkOutcome::Unparseable,
                        ..
                    }
                )),
                "{answer}"
            );
            let held = session.shared.lock();
            assert_eq!(
                held.interview
                    .as_ref()
                    .expect("memory")
                    .object
                    .live()
                    .count(),
                3
            );
            drop(held);
            reads_whole(&session);
        }
    }

    /// #504: with the pre-warm on, a seam sends its refilled trunk once with
    /// an output cap of one, and its line carries the warm's timings.
    #[test]
    fn a_warmed_seam_sends_the_refilled_trunk_once_and_logs_its_timings() {
        let session = three_decisions_then(
            |seams| seams.warm = true,
            vec![vec![
                Step::Delta("x".to_owned()),
                Step::Timings(Timings {
                    prompt_n: Some(412),
                    ..Timings::default()
                }),
            ]],
        );
        session.declare_seam(None).expect("admitted");
        let log = wait_until(&session, "the seam", seamed);
        let warm = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Seamed { warm, .. } => Some(warm.clone()),
                _ => None,
            })
            .expect("the seam");
        assert_eq!(warm.and_then(|timings| timings.prompt_n), Some(412));
        let sent = session.shared.transport.sent();
        let warmed = sent.last().expect("the warm");
        assert_eq!(warmed.limits.max_output_tokens, 1);
        assert_eq!(warmed.messages, session.trunk());
        reads_whole(&session);
    }

    /// #504: an ask while the warm is in flight cancels it: the seam stands,
    /// logged without the warm's timings, and the ask is admitted.
    #[test]
    fn an_ask_cancels_the_warm_and_the_seam_stands() {
        let gate = Gate::new();
        let session = three_decisions_then(
            |seams| seams.warm = true,
            vec![vec![Step::Hold(gate.clone())], deltas(&["next answer"])],
        );
        session.declare_seam(None).expect("admitted");
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        session
            .ask("next", None)
            .expect("admitted once the warm was cancelled");
        let log = wait_until(&session, "turn two", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        assert!(
            log.iter()
                .any(|logged| matches!(logged.event, Event::Seamed { warm: None, .. }))
        );
        reads_whole(&session);
    }

    /// The scope-boundary turn's answer: it states what it will do next.
    pub(in crate::drive) const SCOPED: &str =
        "A tracker for one team, no login. Next, I will sketch the schema.";

    /// The interview's answer in T1's shape: three decisions, no plan.
    pub(in crate::drive) const DECIDED: &str = "DECISION: a tracker\n\
         DECISION: for one team\n\
         DECISION: no login\n\
         PLAN: NONE\n";

    /// The router's turn-boundary ask, quoting `answer`'s stated intent: the
    /// question a scoping fork asks.
    pub(in crate::drive) fn judgment_after(answer: &str) -> String {
        router::Ask {
            kind: AskKind::Judgment,
            intent: router::stated_intent(answer),
        }
        .render(&Facts::default())
    }

    fn forks(log: &[Logged]) -> Vec<(u64, u32, u64, log::Warrant, String)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Forked {
                    of_turn,
                    at,
                    why,
                    question,
                    ..
                } => Some((logged.seq, *of_turn, *at, *why, question.clone())),
                _ => None,
            })
            .collect()
    }

    /// Each fork's warrant and trigger, in order (#564).
    fn triggers(log: &[Logged]) -> Vec<(log::Warrant, String)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Forked { why, trigger, .. } => Some((*why, trigger.clone())),
                _ => None,
            })
            .collect()
    }

    /// A session whose one turn runs `commands` in order, then answers, and
    /// whose gap forks under `cadence` and `threshold` with both warrants:
    /// each fork answers with a learned line (#564).
    fn cadenced(
        tree: &Path,
        commands: &[&str],
        (cadence, threshold_bytes): (Cadence, Option<u64>),
        forks: usize,
    ) -> Session<Canned> {
        std::fs::write(tree.join("notes.md"), "a note\n").expect("a note");
        std::fs::write(tree.join("lib.rs"), "pub fn f() {}\n").expect("a source");
        let mut replies: Vec<Vec<Step>> = commands
            .iter()
            .enumerate()
            .map(|(n, command)| vec![bash(&format!("call-{}", n + 1), command)])
            .collect();
        replies.push(deltas(&["Done. Next, I will write the schema."]));
        replies.extend((0..forks).map(|_| deltas(&["LEARNED: something\n"])));
        Session::open_with(
            Canned::new(replies),
            looping(),
            None,
            Some(tools(
                Confinement::Unconfined,
                tree,
                &["cat", "echo"],
                None,
                Decider::Decline,
            )),
            None,
            Some(Interview {
                cadence,
                threshold_bytes,
                ..interviewing(&[log::Warrant::Scoping, log::Warrant::Read])
            }),
        )
    }

    /// Runs `cadenced`'s one turn to the end of its gap: every fork settled.
    fn through_the_gap(session: &Session<Canned>, forks: usize) -> Vec<Logged> {
        session.ask("look around", None).expect("accepted");
        let log = wait_until(session, "the gap's forks", |log| {
            settled(log) && fork_outcomes(log).len() == forks
        });
        reads_whole(session);
        log
    }

    /// `per_class` (#564): a fork on every call the router routes to a class
    /// ask, in call order, each naming its call and class; a call it would
    /// not interrupt forks nothing. The gap's forks run one after another,
    /// and the log reads.
    #[test]
    fn per_class_forks_on_each_routed_call_in_call_order() {
        let tree = scratch("cadence-per-class");
        let session = cadenced(
            &tree,
            &["cat notes.md", "echo hi", "cat lib.rs"],
            (Cadence::PerClass, None),
            2,
        );
        let log = through_the_gap(&session, 2);
        assert_eq!(
            triggers(&log),
            [
                (log::Warrant::Read, "call:document-read:call-1".to_owned()),
                (log::Warrant::Read, "call:source-read:call-3".to_owned()),
            ]
        );
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Value; 2]);
        let questions: Vec<String> = forks(&log).into_iter().map(|fork| fork.4).collect();
        assert!(
            questions[0].contains("You just read a document"),
            "{questions:?}"
        );
        assert!(
            questions[1].contains("You last ran: cat lib.rs"),
            "{questions:?}"
        );
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// `per_call` (#564): a fork on every call that ran -- the class's ask,
    /// or the router's declared default, the generic ask, for one it would
    /// not interrupt.
    #[test]
    fn per_call_forks_on_every_call_with_the_generic_ask_as_its_default() {
        let tree = scratch("cadence-per-call");
        let session = cadenced(
            &tree,
            &["cat notes.md", "echo hi"],
            (Cadence::PerCall, None),
            2,
        );
        let log = through_the_gap(&session, 2);
        let found = triggers(&log);
        assert_eq!(found[0].1, "call:document-read:call-1");
        assert!(found[1].1.ends_with(":call-2"), "{found:?}");
        let generic = router::Ask {
            kind: AskKind::Generic,
            intent: router::stated_intent("Done. Next, I will write the schema."),
        }
        .render(&Facts {
            last_command: Some("echo hi".to_owned()),
            ..Facts::default()
        });
        assert_eq!(forks(&log)[1].4, generic);
        tidy(&[&tree]);
    }

    /// `turn_boundary` (#564): the judgment ask at the turn's end under
    /// `scoping`, though the ask carried no mark; `gap` fires nothing for
    /// the same turn with no read in it.
    #[test]
    fn turn_boundary_asks_judgment_at_every_turns_end() {
        let tree = scratch("cadence-turn-boundary");
        let session = cadenced(&tree, &["echo hi"], (Cadence::TurnBoundary, None), 1);
        let log = through_the_gap(&session, 1);
        assert_eq!(
            triggers(&log),
            [(log::Warrant::Scoping, "turn_end".to_owned())]
        );
        let session = cadenced(&tree, &["echo hi"], (Cadence::Gap, None), 0);
        session.ask("look around", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert!(triggers(&log).is_empty());
        tidy(&[&tree]);
    }

    /// The threshold (#564) gates a read: under it, the read forks nothing;
    /// at it, the read forks as before, its trigger naming the call.
    #[test]
    fn a_read_under_the_threshold_forks_nothing() {
        let tree = scratch("cadence-threshold");
        // `notes.md` is seven bytes.
        let session = cadenced(&tree, &["cat notes.md"], (Cadence::Gap, Some(8)), 0);
        session.ask("look around", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert!(triggers(&log).is_empty(), "{:?}", triggers(&log));
        let session = cadenced(&tree, &["cat notes.md"], (Cadence::Gap, Some(7)), 1);
        let log = through_the_gap(&session, 1);
        assert_eq!(
            triggers(&log),
            [(log::Warrant::Read, "call:document-read:call-1".to_owned())]
        );
        tidy(&[&tree]);
    }

    /// The cadence and the threshold read leniently (#564).
    #[test]
    fn the_cadence_and_threshold_read_leniently() {
        let read = |text: &str| {
            let regimen = regimen::parse(text).expect("a regimen");
            (
                interview_cadence(&regimen),
                interview_threshold_bytes(&regimen),
            )
        };
        assert_eq!(read(""), (Cadence::Gap, None));
        assert_eq!(
            read("interview_cadence = \"per_class\"\ninterview_threshold_bytes = 3000\n"),
            (Cadence::PerClass, Some(3000))
        );
        assert_eq!(
            read("interview_cadence = \"often\"\ninterview_threshold_bytes = 0\n"),
            (Cadence::Gap, None)
        );
        assert_eq!(
            read("interview_cadence = \"turn_boundary\"\n").0,
            Cadence::TurnBoundary
        );
        assert_eq!(
            read("interview_cadence = \"per_call\"\n").0,
            Cadence::PerCall
        );
    }

    fn fork_outcomes(log: &[Logged]) -> Vec<log::ForkOutcome> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::ForkSettled { outcome, .. } => Some(*outcome),
                _ => None,
            })
            .collect()
    }

    fn patches(log: &[Logged]) -> Vec<log::PatchEntry> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Patched { entry, .. } => Some(entry.clone()),
                _ => None,
            })
            .collect()
    }

    /// The trunk request a turn's answer answered.
    fn answered_at(log: &[Logged]) -> u64 {
        log.iter()
            .find_map(|logged| match &logged.event {
                Event::Answered { request, .. } => Some(*request),
                _ => None,
            })
            .expect("an answer")
    }

    /// Qwen's convention (the reasoning ruling): a tool step's reasoning
    /// goes back in the next step's request, on the assistant message that
    /// made the call, unchanged -- what `preserve_thinking` renders.
    #[test]
    fn a_tool_steps_reasoning_goes_back_unchanged_in_the_next_step() {
        let tree = scratch("reasoning-back");
        let session = Session::open_looping(
            Canned::new([
                vec![
                    Step::Reasoning("next, B\n".to_owned()),
                    bash("call-1", "touch b"),
                ],
                deltas(&["done"]),
            ]),
            looping(),
            None,
            tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                None,
                Decider::Decline,
            ),
        );
        session.ask("go", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        let sent = session.shared.transport.sent();
        assert_eq!(sent.len(), 2);
        let said = sent[1]
            .messages
            .iter()
            .find(|message| message.role == Role::Assistant)
            .expect("the step's assistant message");
        assert_eq!(said.reasoning.as_deref(), Some("next, B\n"));
        tidy(&[&tree]);
    }

    /// The fork delivery lever: patches waiting at the next ask are one
    /// note after it, each line the (b′) sentence of the session's framing,
    /// logged as `delivered`; the note stays on the trunk for the turns
    /// after. Under `seam`, nothing is delivered.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_delivered_note_follows_the_ask_and_stays_on_the_trunk() {
        for (delivery, template_name) in [
            (log::ForkDelivery::Advisory, "FORK_NOTE_ADVISORY"),
            (log::ForkDelivery::Imperative, "FORK_NOTE_IMPERATIVE"),
            (log::ForkDelivery::Seam, ""),
        ] {
            let mut interview = interviewing(&[]);
            interview.delivery = delivery;
            let session = Session::open_with(
                Canned::new([deltas(&["one"]), deltas(&["two"])]),
                template(),
                None,
                None,
                None,
                Some(interview),
            );
            session.shared.lock().undelivered = vec![
                (
                    log::PatchOp::Supersede,
                    "e-1".to_owned(),
                    "the schema is per-team".to_owned(),
                ),
                (
                    log::PatchOp::Retire,
                    "e-2".to_owned(),
                    "no login".to_owned(),
                ),
            ];
            session.ask("go", None).expect("accepted");
            wait_until(&session, "turn one", settled);
            session.ask("again", None).expect("accepted");
            let log = wait_until(&session, "turn two", |log| {
                settled(log)
                    && log
                        .iter()
                        .filter(|l| matches!(l.event, Event::Answered { .. }))
                        .count()
                        == 2
            });
            reads_whole(&session);
            let sent = session.shared.transport.sent();
            let delivered: Vec<&Event> = log
                .iter()
                .map(|logged| &logged.event)
                .filter(|event| matches!(event, Event::Delivered { .. }))
                .collect();
            if delivery == log::ForkDelivery::Seam {
                assert!(delivered.is_empty(), "{delivered:?}");
                assert_eq!(sent[0].messages.last(), Some(&user("go")));
                continue;
            }
            let template = if delivery == log::ForkDelivery::Advisory {
                crate::dogma::Template::ForkNoteAdvisory
            } else {
                crate::dogma::Template::ForkNoteImperative
            };
            let line = |entry: &str| {
                template
                    .fill(&[(crate::dogma::Hole::Entry, entry)])
                    .expect("one hole")
            };
            let note = format!("{}\n{}", line("the schema is per-team"), line("no login"));
            // The first request: the ask, then the note at the tail.
            let first = &sent[0].messages;
            assert_eq!(&first[first.len() - 2..], &[user("go"), user(&note)]);
            // The second: the note stayed on the trunk after its ask.
            let second = &sent[1].messages;
            assert!(
                second
                    .windows(2)
                    .any(|pair| pair == [user("go"), user(&note)]),
                "{second:#?}"
            );
            assert_eq!(second.last(), Some(&user("again")));
            // The projection rebuilds both heads with the note, from the log.
            let lines: Vec<log::Line> = log.iter().map(line_of).collect();
            let projected =
                crate::drive::projection::project(&lines, &regime(), None).expect("projected");
            // And the record names the state.
            assert!(matches!(
                projected.events.first(),
                Some(crate::formats::record::Event::Start { fork_delivery: Some(named), .. })
                    if *named == delivery
            ));
            assert!(
                !projected
                    .unspellable
                    .iter()
                    .any(|item| item.why.contains("rebuilt")),
                "{:?}",
                projected.unspellable
            );
            let [
                Event::Delivered {
                    turn, text, lines, ..
                },
            ] = delivered.as_slice()
            else {
                panic!("one delivery: {delivered:?}");
            };
            assert_eq!((*turn, text.as_str()), (1, note.as_str()));
            assert_eq!(
                lines
                    .iter()
                    .map(|l| (l.entry.as_str(), l.op, l.template.as_str()))
                    .collect::<Vec<_>>(),
                [
                    ("e-1", log::PatchOp::Supersede, template_name),
                    ("e-2", log::PatchOp::Retire, template_name),
                ]
            );
        }
    }

    /// An `add` names no existing entry, and no measured sentence fits it:
    /// under a mid-turn delivery it is still carried at the seam only.
    fn from_fork(index: u32) -> crate::object::Provenance {
        crate::object::Provenance {
            turn: 1,
            lane: super::super::INTERVIEW.to_owned(),
            fork: Some("f/9".to_owned()),
            tangent: None,
            index,
        }
    }

    fn entry_id(id: &str) -> crate::object::EntryId {
        crate::object::EntryId::new(id).expect("an id")
    }

    /// #562: a `SUPERSEDE` field citing a live entry's id is the format's
    /// supersession; one citing nothing live, or naming nothing to stand,
    /// stays the addition the fold made.
    #[test]
    fn a_supersede_field_citing_a_live_entry_voids_it_and_any_other_stays_an_add() {
        let mut object = WorkingObject::open(regime());
        object
            .apply(&Patch::Add {
                id: entry_id("interview-t1-0"),
                content: "decision: a tracker".to_owned(),
                provenance: from_fork(0),
            })
            .expect("applied");
        for (answer, voids) in [
            (
                "SUPERSEDE: interview-t1-0 decision: a tracker for two teams\n",
                Some("interview-t1-0"),
            ),
            (
                "SUPERSEDE: `interview-t1-0`: decision: a tracker for two teams\n",
                Some("interview-t1-0"),
            ),
            ("SUPERSEDE: interview-t9-9 decision: something else\n", None),
            ("SUPERSEDE: interview-t1-0\n", None),
        ] {
            let (patches, _) = super::super::fold(answer, 2, super::super::INTERVIEW, Some("f/9"))
                .expect("folded");
            let patches = cited(patches, &object);
            match (&patches[..], voids) {
                (
                    [
                        Patch::Supersede {
                            voids: got,
                            content,
                            ..
                        },
                    ],
                    Some(want),
                ) => {
                    assert_eq!(got.as_str(), want, "{answer}");
                    assert_eq!(content, "decision: a tracker for two teams", "{answer}");
                }
                ([Patch::Add { .. }], None) => {}
                (other, _) => panic!("{answer}: {other:?}"),
            }
        }
    }

    /// #562: every op the format has, applied through serve's fork path:
    /// the entry's state is the format's, a `patch` line is logged with the
    /// op, and under mid-turn delivery each op but `add` is held for the
    /// next ask's note.
    #[test]
    fn each_patch_op_a_fork_emits_applies_as_the_format_says_and_all_but_add_are_delivered() {
        use crate::object::EntryState;
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.delivery = log::ForkDelivery::Advisory;
        let session = Session::open_with(
            Canned::new(Vec::<Vec<Step>>::new()),
            template(),
            None,
            None,
            None,
            Some(interview),
        );
        let mut state = session.shared.lock();
        let seeds: Vec<Patch> = ["a", "b", "c", "d"]
            .iter()
            .enumerate()
            .map(|(index, name)| Patch::Add {
                id: entry_id(&format!("seed-{name}")),
                content: format!("fact {name}"),
                provenance: from_fork(u32::try_from(index).expect("small")),
            })
            .collect();
        let (outcome, _) = applied(&mut state, &seeds, 1);
        assert_eq!(outcome, log::ForkOutcome::Value);
        assert!(state.undelivered.is_empty(), "an add waits for the seam");
        let ops = vec![
            Patch::Supersede {
                id: entry_id("next-a"),
                content: "fact a, corrected".to_owned(),
                voids: entry_id("seed-a"),
                provenance: from_fork(0),
            },
            Patch::Resolve {
                target: entry_id("seed-b"),
                provenance: from_fork(1),
            },
            Patch::Retire {
                target: entry_id("seed-c"),
                provenance: from_fork(2),
            },
            // A park is a tangent's ruling on its own fact (object.rs): the
            // format refuses one with no tangent, below.
            Patch::Park {
                target: entry_id("seed-d"),
                provenance: crate::object::Provenance {
                    tangent: Some("t/1".to_owned()),
                    ..from_fork(3)
                },
            },
        ];
        let untangled = Patch::Park {
            target: entry_id("seed-d"),
            provenance: from_fork(3),
        };
        assert_eq!(
            applied(&mut state, std::slice::from_ref(&untangled), 3).0,
            log::ForkOutcome::Unparseable,
            "a park outside a tangent is refused, as the format says"
        );
        let (outcome, lines) = applied(&mut state, &ops, 2);
        assert_eq!(outcome, log::ForkOutcome::Value);
        let logged: Vec<log::PatchOp> = lines
            .iter()
            .filter_map(|event| match event {
                Event::Patched { op, .. } => Some(*op),
                _ => None,
            })
            .collect();
        assert_eq!(
            logged,
            [
                log::PatchOp::Supersede,
                log::PatchOp::Resolve,
                log::PatchOp::Retire,
                log::PatchOp::Park
            ]
        );
        let object = &state.interview.as_ref().expect("interviewing").object;
        let state_of = |id: &str| object.entry(&entry_id(id)).expect("kept").state.clone();
        assert!(matches!(state_of("seed-a"), EntryState::Voided { .. }));
        assert_eq!(state_of("next-a"), EntryState::Live);
        assert_eq!(state_of("seed-b"), EntryState::Resolved);
        assert_eq!(state_of("seed-c"), EntryState::Retired);
        assert_eq!(state_of("seed-d"), EntryState::Parked);
        let held: Vec<&str> = state
            .undelivered
            .iter()
            .map(|(_, id, _)| id.as_str())
            .collect();
        assert_eq!(held, ["seed-a", "seed-b", "seed-c", "seed-d"]);
    }

    /// #562's acceptance: a fork's `SUPERSEDE` citing an entry an earlier
    /// fork recorded voids it, and under advisory delivery the next ask
    /// carries the measured note naming that entry.
    #[test]
    fn a_forks_supersede_of_an_existing_entry_is_a_mid_turn_note_under_advisory_delivery() {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.delivery = log::ForkDelivery::Advisory;
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&[SCOPED]),
                deltas(&["SUPERSEDE: interview-t1-0 decision: a tracker for two teams\n"]),
                deltas(&["next"]),
            ]),
            template(),
            None,
            None,
            None,
            Some(interview),
        );
        for (turn, ask) in [(1, "what are we building?"), (2, "and who is it for?")] {
            session.ask_marked(ask, None, true).expect("accepted");
            wait_until(&session, "the fork to settle", |log| {
                settled(log) && fork_outcomes(log).len() >= turn
            });
        }
        session.ask("go on", None).expect("accepted");
        let log = wait_until(&session, "turn three", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|l| matches!(l.event, Event::Asked { .. }))
                    .count()
                    == 3
        });
        reads_whole(&session);
        assert!(log.iter().any(|l| matches!(
            l.event,
            Event::Patched {
                op: log::PatchOp::Supersede,
                ..
            }
        )));
        let delivered: Vec<&log::NoteLine> = log
            .iter()
            .filter_map(|l| match &l.event {
                Event::Delivered { turn: 3, lines, .. } => Some(lines),
                _ => None,
            })
            .flatten()
            .collect();
        assert_eq!(delivered.len(), 1, "{delivered:?}");
        assert_eq!(delivered[0].op, log::PatchOp::Supersede);
        assert_eq!(delivered[0].entry, "interview-t1-0");
        assert_eq!(delivered[0].template, "FORK_NOTE_ADVISORY");
    }

    /// #567: a narrow view keeps the head whole and the last whole turns,
    /// each from its user message; the trunk view is the trunk.
    #[test]
    fn a_forks_view_keeps_the_head_and_cuts_at_whole_turns() {
        let call = {
            let mut said = Message::new(Role::Assistant, "");
            said.tool_calls = vec![crate::client::shape::ToolCall {
                id: "c".to_owned(),
                name: "bash".to_owned(),
                arguments: "{}".to_owned(),
            }];
            said
        };
        let head = template().messages;
        let mut trunk = head.clone();
        trunk.extend([user("one"), Message::new(Role::Assistant, "a")]);
        trunk.extend([
            user("two"),
            call,
            Message::tool_result("c".to_owned(), "out".to_owned()),
            Message::new(Role::Assistant, "b"),
        ]);
        trunk.extend([user("three"), Message::new(Role::Assistant, "c")]);
        assert_eq!(viewed(&trunk, head.len(), ForkView::Trunk), trunk);
        let last = viewed(&trunk, head.len(), ForkView::Last(1));
        assert_eq!(last[..head.len()], head[..], "the head is unchanged");
        assert_eq!(last[head.len()..], trunk[trunk.len() - 2..]);
        let two = viewed(&trunk, head.len(), ForkView::Last(2));
        assert_eq!(
            two[head.len()],
            user("two"),
            "a turn starts at its user message"
        );
        assert_eq!(
            two.len(),
            head.len() + 6,
            "the call and its result stay together"
        );
        assert_eq!(viewed(&trunk, head.len(), ForkView::Last(9)), trunk);
    }

    /// #567: the regimen's words, read leniently.
    #[test]
    fn the_fork_view_is_read_leniently() {
        let read = |text: &str| fork_view(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read(""), ForkView::Trunk);
        assert_eq!(read("fork_view = \"trunk\"\n"), ForkView::Trunk);
        assert_eq!(read("fork_view = \"last_turn\"\n"), ForkView::Last(1));
        assert_eq!(read("fork_view = \"last:3\"\n"), ForkView::Last(3));
        for unread in ["last:0", "last:x", "everything"] {
            assert_eq!(
                read(&format!("fork_view = \"{unread}\"\n")),
                ForkView::Trunk
            );
        }
        assert_eq!(ForkView::Last(1).word(), "last_turn");
        assert_eq!(ForkView::Last(3).word(), "last:3");
    }

    /// #567: a served fork under `last_turn` sees the head and the turn it
    /// follows, not the one before; under the trunk it sees the whole trunk,
    /// as before; its `fork` line names a narrow view.
    #[test]
    fn a_served_fork_sees_the_view_its_regimen_declares() {
        for view in [ForkView::Trunk, ForkView::Last(1)] {
            let mut interview = interviewing(&[log::Warrant::Scoping]);
            interview.view = Some(view);
            let session = Session::open_with(
                Canned::new([
                    deltas(&["first answer"]),
                    deltas(&[SCOPED]),
                    deltas(&[DECIDED]),
                ]),
                template(),
                None,
                None,
                None,
                Some(interview),
            );
            session.ask("an earlier turn", None).expect("accepted");
            wait_until(&session, "turn one", settled);
            let trunk_before = session.trunk();
            session
                .ask_marked("what are we building?", None, true)
                .expect("accepted");
            wait_until(&session, "the fork to settle", |log| {
                settled(log) && !fork_outcomes(log).is_empty()
            });
            let sent = session.shared.transport.sent();
            let fork = &sent.last().expect("the fork's request").messages;
            let ask = fork.last().expect("its ask");
            let seen = &fork[..fork.len() - 1];
            let trunk = session.trunk();
            match view {
                ForkView::Trunk => assert_eq!(seen, &trunk[..], "the whole trunk, as before"),
                ForkView::Last(_) => {
                    let head = template().messages.len();
                    assert_eq!(seen[..head], trunk[..head]);
                    assert_eq!(seen[head..], trunk[trunk_before.len()..]);
                    assert!(!seen.iter().any(|m| m.content == "an earlier turn"));
                }
            }
            assert_eq!(ask.role, Role::User);
            let lines = whole_log(&session);
            let named = lines.iter().find_map(|line| match &line.event {
                log::Event::Fork { view, .. } => Some(view.clone()),
                _ => None,
            });
            assert_eq!(
                named.expect("a fork line"),
                (view != ForkView::Trunk).then(|| view.word())
            );
            // A warm fork's settling line is as before #570.
            assert!(lines.iter().any(|line| matches!(
                line.event,
                log::Event::ForkSettled {
                    prompt_tokens: None,
                    wall_ms: None,
                    ..
                }
            )));
        }
    }

    /// #570: a seated session's fork is made to the seat, naming its model,
    /// shown the last turn when the regimen does not say, and its line names
    /// the seat; the trunk's server sees only the trunk.
    /// A scoping session over a `window`-token context, its interview
    /// shaped by `shape_it`, playing an earlier turn, the scoped turn and
    /// its fork's answer; the log once the fork settled (#406).
    fn forked_over(
        window: u64,
        shape_it: impl FnOnce(&mut Interview),
    ) -> (Session<Canned>, Vec<Logged>) {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        shape_it(&mut interview);
        let mut shape = template();
        shape.limits.context_window = Some(window);
        let session = Session::open_with(
            Canned::new([
                deltas(&["first answer"]),
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
            ]),
            shape,
            None,
            None,
            None,
            Some(interview),
        );
        session.ask("an earlier turn", None).expect("accepted");
        wait_until(&session, "turn one", settled);
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        (session, log)
    }

    /// The fork's request's output cap, when it was sent.
    fn fork_cap(log: &[Logged]) -> Option<u32> {
        log.iter().find_map(|logged| match &logged.event {
            Event::Requested {
                lane: Lane::Interview,
                max_tokens,
                ..
            } => Some(*max_tokens),
            _ => None,
        })
    }

    /// #406: a fork whose prompt leaves its window less than the clamp's
    /// floor is never sent -- `refused`, `pool` -- and the session goes on;
    /// one with room is sent with the output cap, clamped as the trunk's is.
    #[test]
    fn a_fork_that_would_not_fit_its_window_is_refused_unsent() {
        let (session, log) = forked_over(1_000, |_| {});
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Refused]);
        assert_eq!(fork_cap(&log), None, "never sent");
        assert_eq!(
            session.shared.transport.sent().len(),
            2,
            "the trunk's two turns only"
        );
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::ForkSettled { refused: Some(why), .. } if why == "pool"
        )));
        assert_eq!(session.settlement(), Settlement::Awaiting);
        reads_whole(&session);
        let (_, log) = forked_over(1_000_000, |_| {});
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Value]);
        assert_eq!(fork_cap(&log), Some(template().limits.max_output_tokens));
    }

    /// #406: `fork_tail_tokens` is the cap a fork is clamped from.
    #[test]
    fn a_forks_tail_is_the_regimens_when_it_declares_one() {
        let (_, log) = forked_over(1_000_000, |interview| interview.fork_tail = Some(48));
        assert_eq!(fork_cap(&log), Some(48));
    }

    /// #406: a fork on the trunk's own server whose view is not the trunk's
    /// prefix says it may displace the trunk's cache; one on the whole
    /// trunk does not.
    #[test]
    fn a_fork_off_the_trunks_prefix_on_its_server_names_the_hazard() {
        let hazard = |log: &[Logged]| {
            log.iter().find_map(|logged| match &logged.event {
                Event::Forked { displaces, .. } => Some(*displaces),
                _ => None,
            })
        };
        let (session, log) = forked_over(1_000_000, |interview| {
            interview.view = Some(ForkView::Last(1));
        });
        assert_eq!(hazard(&log), Some(true));
        let lines = whole_log(&session);
        assert!(lines.iter().any(|line| matches!(
            &line.event,
            log::Event::Fork { hazard: Some(h), .. } if h == "may-displace-trunk-cache"
        )));
        let (_, log) = forked_over(1_000_000, |_| {});
        assert_eq!(hazard(&log), Some(false));
    }

    #[test]
    fn a_seated_fork_runs_on_its_seat_and_says_so() {
        let session = Session::open_with(
            Canned::new([deltas(&["first answer"]), deltas(&[SCOPED])]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        )
        .seated(Offboard {
            transport: Canned::new([deltas(&[DECIDED])]),
            substrate: "cpu-seat".to_owned(),
            model: "small".to_owned(),
            context_window: Some(8192),
        });
        session.ask("an earlier turn", None).expect("accepted");
        wait_until(&session, "turn one", settled);
        let trunk_before = session.trunk();
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Value]);

        let trunk_sent = session.shared.transport.sent();
        assert_eq!(
            trunk_sent.len(),
            2,
            "the trunk's server saw the two turns only"
        );
        assert!(trunk_sent.iter().all(|shape| shape.model == "a-model"));
        let seat = &session.shared.seat.as_ref().expect("seated").transport;
        let [fork] = seat
            .sent()
            .try_into()
            .expect("one fork request, to the seat");
        assert_eq!(fork.model, "small");
        assert_eq!(fork.limits.context_window, Some(8192));
        // #406: sized from its own estimate, not the trunk's padded one, so
        // an 8,192-token seat leaves it the session's whole output cap.
        assert_eq!(
            fork.limits.max_output_tokens,
            template().limits.max_output_tokens
        );
        // Undeclared, offboard: the head and the last turn, then the ask.
        let trunk = session.trunk();
        let head = template().messages.len();
        let seen = &fork.messages[..fork.messages.len() - 1];
        assert_eq!(seen[..head], trunk[..head]);
        assert_eq!(seen[head..], trunk[trunk_before.len()..]);

        let lines = whole_log(&session);
        let named = lines.iter().find_map(|line| match &line.event {
            log::Event::Fork { view, seat, .. } => Some((view.clone(), seat.clone())),
            _ => None,
        });
        assert_eq!(
            named.expect("a fork line"),
            (
                Some("last_turn".to_owned()),
                Some(log::ForkSeat {
                    substrate: "cpu-seat".to_owned(),
                    model: "small".to_owned(),
                })
            )
        );
        // Its cost: the wall time always, the prompt as its server reports
        // it, which this one does not.
        let cost = lines.iter().find_map(|line| match &line.event {
            log::Event::ForkSettled {
                prompt_tokens,
                wall_ms,
                ..
            } => Some((*prompt_tokens, *wall_ms)),
            _ => None,
        });
        assert!(
            matches!(cost, Some((None, Some(_)))),
            "an offboard fork's cost: {cost:?}"
        );
    }

    /// #595: under ask set v4 the scoping fork's judgment ask shows the
    /// working record as `<id>\t<entry>` lines and asks for SUPERSEDE; a
    /// fork citing an entry voids it, and its `patch` line carries the cited
    /// id and the entry that replaces it, so a blind judge can grade it from
    /// the log. `session.start` names the set and its digest, and each fork
    /// line names the ask it sent.
    #[test]
    fn under_ask_set_v4_a_fork_sees_the_record_and_its_supersede_is_gradable_from_the_log() {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.asks = &crate::dogma::asks::V4;
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&[SCOPED]),
                deltas(&[
                    "DECISION: NONE\nPLAN: NONE\nSUPERSEDE: interview-t1-0 decision: a tracker for two teams\n",
                ]),
            ]),
            template(),
            None,
            None,
            None,
            Some(interview),
        );
        for (turn, ask) in [(1, "what are we building?"), (2, "and who is it for?")] {
            session.ask_marked(ask, None, true).expect("accepted");
            wait_until(&session, "the fork to settle", |log| {
                settled(log) && fork_outcomes(log).len() >= turn
            });
        }
        let lines = whole_log(&session);
        let Some(log::Event::SessionStart { fork_asks, .. }) = lines.first().map(|l| &l.event)
        else {
            panic!("the log opens with session.start");
        };
        assert_eq!(
            fork_asks.as_ref(),
            Some(&log::ForkAsks {
                name: "v4".to_owned(),
                digest: Some(crate::dogma::asks::V4.digest()),
            })
        );
        let forks: Vec<(String, Option<String>)> = lines
            .iter()
            .filter_map(|l| match &l.event {
                log::Event::Fork { question, ask, .. } => Some((question.clone(), ask.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(forks.len(), 2);
        assert!(
            forks
                .iter()
                .all(|(_, ask)| ask.as_deref() == Some("judgment"))
        );
        // The first fork had no record to show; the second shows the first's.
        assert!(
            !forks[0].0.contains("The working record so far"),
            "{}",
            forks[0].0
        );
        assert!(forks[1].0.contains("SUPERSEDE"), "{}", forks[1].0);
        assert!(
            forks[1]
                .0
                .contains("\ninterview-t1-0\tdecision: a tracker\n"),
            "the record, as the render writes it: {}",
            forks[1].0
        );
        let superseded = lines.iter().find_map(|l| match &l.event {
            log::Event::Patch {
                op: log::PatchOp::Supersede,
                entry,
                supersedes,
                ..
            } => Some((entry.text.clone(), supersedes.clone())),
            _ => None,
        });
        assert_eq!(
            superseded,
            Some((
                "decision: a tracker for two teams".to_owned(),
                Some("interview-t1-0".to_owned())
            )),
            "the patch line names the cited id and the entry that replaces it"
        );
    }

    /// #595: the set is read from the regimen leniently, v3 by default.
    #[test]
    fn the_fork_ask_set_is_read_leniently_and_defaults_to_v3() {
        let read = |text: &str| fork_asks(&regimen::parse(text).expect("a regimen")).name;
        assert_eq!(read(""), "v3");
        assert_eq!(read("fork_asks = \"v3\"\n"), "v3");
        assert_eq!(read("fork_asks = \"v4\"\n"), "v4");
        assert_eq!(read("fork_asks = \"no-such-set\"\n"), "v3");
        assert_eq!(read("fork_asks = 3\n"), "v3");
    }

    /// #599: a fork's ask goes in the interview's role, and its `fork` line
    /// names a role but `user`; under `user` the line is as before.
    #[test]
    fn a_forks_ask_goes_in_the_interview_role_and_its_line_names_it() {
        for role in [Role::User, Role::Developer] {
            let mut interview = interviewing(&[log::Warrant::Scoping]);
            interview.role = role;
            let session = Session::open_with(
                Canned::new([deltas(&[SCOPED]), deltas(&[DECIDED])]),
                template(),
                None,
                None,
                None,
                Some(interview),
            );
            session
                .ask_marked("what are we building?", None, true)
                .expect("accepted");
            wait_until(&session, "the fork to settle", |log| {
                settled(log) && !fork_outcomes(log).is_empty()
            });
            let sent = session.shared.transport.sent();
            let ask = sent
                .last()
                .and_then(|fork| fork.messages.last())
                .expect("the fork's ask");
            assert_eq!(ask.role, role);
            let lines = whole_log(&session);
            let named = lines.iter().find_map(|line| match &line.event {
                log::Event::Fork { role, .. } => Some(role.clone()),
                _ => None,
            });
            assert_eq!(
                named.expect("a fork line"),
                (role != Role::User).then(|| role.tag().to_owned())
            );
        }
    }

    /// #599: the role is read leniently, `user` by default.
    #[test]
    fn the_interview_role_is_read_leniently() {
        let read = |text: &str| interview_role(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read(""), Role::User);
        assert_eq!(read("interview_role = \"system\"\n"), Role::System);
        assert_eq!(read("interview_role = \"developer\"\n"), Role::Developer);
        assert_eq!(read("interview_role = \"assistant\"\n"), Role::User);
    }

    /// A session whose first scoping turn records three trunk decisions,
    /// then opens tangent `t/1`, whose scoping turn's fork records four.
    fn a_tangent_with_four_entries() -> (Session<Canned>, Vec<Message>) {
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&[SCOPED]),
                deltas(&["DECISION: one\nDECISION: two\nDECISION: three\nDECISION: four\n"]),
                deltas(&["back on the trunk"]),
            ]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the first fork", |log| {
            settled(log) && fork_outcomes(log).len() == 1
        });
        let fork_point = session.trunk();
        session.open_tangent("t/1").expect("opened");
        session
            .ask_marked("what if we tried it another way?", None, true)
            .expect("accepted");
        wait_until(&session, "the tangent's fork", |log| {
            settled(log) && fork_outcomes(log).len() == 2
        });
        (session, fork_point)
    }

    /// #22: closing four tangent entries KEEP, DROP, PARK, KEEP leaves the
    /// two kept in the render, keeps all four in the object, and restores
    /// the trunk to the fork point byte for byte; the close is logged.
    #[test]
    fn closing_a_tangent_disposes_its_entries_and_rolls_the_trunk_back() {
        use crate::object::tangent::Disposition;
        let (session, fork_point) = a_tangent_with_four_entries();
        assert_ne!(
            session.trunk(),
            fork_point,
            "the tangent's turn is on the trunk"
        );
        let ruled = BTreeMap::from([
            ("interview-t2-0".to_owned(), Disposition::Keep),
            ("interview-t2-1".to_owned(), Disposition::Drop),
            ("interview-t2-2".to_owned(), Disposition::Park),
            ("interview-t2-3".to_owned(), Disposition::Keep),
        ]);
        session.close_tangent(&ruled).expect("closed");
        assert_eq!(session.trunk(), fork_point, "rolled back to the fork point");
        let held = session.shared.lock();
        let object = &held.interview.as_ref().expect("interviewing").object;
        let render = crate::seam::render::render(object, None);
        for kept in [
            "interview-t2-0\tdecision: one",
            "interview-t2-3\tdecision: four",
        ] {
            assert!(render.contains(kept), "{render}");
        }
        for gone in ["decision: two", "decision: three"] {
            assert!(!render.contains(gone), "{render}");
        }
        assert_eq!(
            object
                .entries()
                .filter(|e| e.id.as_str().starts_with("interview-t2-"))
                .count(),
            4,
            "the archive keeps everything"
        );
        drop(held);
        let lines = whole_log(&session);
        let closed = lines.iter().find_map(|line| match &line.event {
            log::Event::TangentClose {
                kept,
                dropped,
                parked,
                rolled_back,
                ..
            } => Some((kept.clone(), dropped.clone(), parked.clone(), *rolled_back)),
            _ => None,
        });
        let (kept, dropped, parked, rolled_back) = closed.expect("a tangent.close line");
        assert_eq!(kept, ["interview-t2-0", "interview-t2-3"]);
        assert_eq!((dropped.len(), parked.len()), (1, 1));
        assert_eq!(rolled_back, 2, "the tangent's ask and answer");
        assert!(lines.iter().any(|line| matches!(
            &line.event,
            log::Event::Patch { tangent: Some(t), .. } if t == "t/1"
        )));
    }

    /// #22: the record's projection rolls its rebuilt trunk back at the
    /// close as the session does, so the next trunk request's head is
    /// rebuilt from the log and verified.
    #[test]
    fn the_trunk_request_after_a_close_is_rebuilt_from_the_log() {
        use crate::object::tangent::Disposition;
        let (session, _) = a_tangent_with_four_entries();
        let ruled: BTreeMap<String, Disposition> = (0..4)
            .map(|i| (format!("interview-t2-{i}"), Disposition::Keep))
            .collect();
        session.close_tangent(&ruled).expect("closed");
        session.ask("go on", None).expect("accepted");
        let log = wait_until(&session, "the turn after the close", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 3
        });
        let lines: Vec<log::Line> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        let named: Vec<String> = projected
            .unspellable
            .iter()
            .filter(|u| u.kind == "request")
            .map(|u| u.why.clone())
            .collect();
        assert_eq!(named, Vec::<String>::new());
    }

    /// #22: scope is provenance, never recency -- a close naming an entry
    /// born outside the tangent is refused, and changes nothing.
    #[test]
    fn a_close_naming_an_entry_born_outside_the_tangent_is_refused() {
        use crate::object::tangent::Disposition;
        let (session, _) = a_tangent_with_four_entries();
        let mut ruled: BTreeMap<String, Disposition> = (0..4)
            .map(|i| (format!("interview-t2-{i}"), Disposition::Keep))
            .collect();
        ruled.insert("interview-t1-0".to_owned(), Disposition::Drop);
        let trunk = session.trunk();
        let refused = session.close_tangent(&ruled).expect_err("refused");
        assert!(
            matches!(refused, Rejected::Refused(Refusal::NotTheScope)),
            "{refused:?}"
        );
        assert_eq!(session.trunk(), trunk, "nothing rolled back");
        let missing: BTreeMap<String, Disposition> =
            BTreeMap::from([("interview-t2-0".to_owned(), Disposition::Keep)]);
        assert!(matches!(
            session.close_tangent(&missing),
            Err(Rejected::Refused(Refusal::NotTheScope))
        ));
    }

    /// #22: a seam while a tangent is open is refused `tangent-open`, as is
    /// a second tangent.
    #[test]
    fn a_seam_or_a_second_tangent_while_one_is_open_is_refused() {
        let (session, _) = a_tangent_with_four_entries();
        assert!(matches!(
            session.declare_seam(None),
            Err(Rejected::Refused(Refusal::TangentOpen))
        ));
        assert!(matches!(
            session.open_tangent("t/2"),
            Err(Rejected::Refused(Refusal::TangentOpen))
        ));
    }

    /// A session with self-capture on, every `every` silent turns, and the
    /// contract's tools declared, playing `acts`.
    fn self_capturing(acts: Vec<Vec<Step>>, every: u32) -> Session<Canned> {
        let mut interview = interviewing(&[]);
        interview.self_capture =
            Some(crate::capture::tools::Cadence::every(every).expect("a cadence"));
        let mut shape = template();
        declare_self_capture(&mut shape, Some(&interview));
        Session::open_with(Canned::new(acts), shape, None, None, None, Some(interview))
    }

    /// A self-capturing session over a `window`-token context, its seam
    /// policy `seams`, playing `acts` (#617).
    fn windowed(
        acts: Vec<Vec<Step>>,
        window: u64,
        seams: crate::seam::policy::Served,
    ) -> Session<Canned> {
        let mut interview = interviewing(&[]);
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        interview.seams = seams;
        let mut shape = template();
        shape.limits.context_window = Some(window);
        declare_self_capture(&mut shape, Some(&interview));
        Session::open_with(Canned::new(acts), shape, None, None, None, Some(interview))
    }

    /// The acts of #617's two turns: turn one records a fact the trunk said
    /// and answers; turn two takes a step the server measures at
    /// `measured` prompt tokens, then answers.
    fn two_turns_measuring(measured: u64) -> Vec<Vec<Step>> {
        const SAID: &str = "The parser drops blank lines before it tokenizes.";
        let timings = |prompt: u64| {
            Step::Timings(Timings {
                prompt_n: Some(prompt),
                cache_n: Some(0),
                ..Timings::default()
            })
        };
        vec![
            vec![
                Step::Delta(SAID.to_owned()),
                record("call-1", "fact", SAID),
                timings(1_000),
            ],
            vec![Step::Delta("done".to_owned()), timings(1_100)],
            vec![
                Step::Delta("Reading it now.".to_owned()),
                record("call-2", "fact", "Reading it now."),
                timings(measured),
            ],
            deltas(&["done again"]),
        ]
    }

    fn window_seams(log: &[Logged]) -> Vec<(u64, u32, Option<log::SeamFired>)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Seamed {
                    reason: crate::seam::Reason::Window,
                    at_turn,
                    fired,
                    ..
                } => Some((logged.seq, *at_turn, *fired)),
                _ => None,
            })
            .collect()
    }

    /// #617: before a step's request whose prompt would leave less than the
    /// reserve of the window, the trunk is refilled from working memory --
    /// under the turn, between its steps -- and the turn goes on: the next
    /// request carries the refill, then the turn's own ask and step. The
    /// seam names the prompt that fired it and the window, the log reads
    /// back, and the record projects it.
    #[test]
    fn a_step_that_would_overflow_the_window_seams_first_and_the_turn_goes_on() {
        let session = windowed(
            two_turns_measuring(90_000),
            100_000,
            crate::seam::policy::Served::default(),
        );
        session
            .ask("how are blank lines treated?", None)
            .expect("accepted");
        wait_until(&session, "turn one", settled);
        session.ask("second", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        let seams = window_seams(&log);
        assert_eq!(seams.len(), 1, "{log:#?}");
        let (at, at_turn, fired) = seams[0];
        assert_eq!(at_turn, 2);
        let fired = fired.expect("its sizes");
        assert_eq!(fired.window, 100_000);
        assert!(fired.prompt_tokens > 80_000, "{fired:?}");
        // Under the turn: after its first request, before its next.
        let turn_two_requests: Vec<u64> = log
            .iter()
            .filter(|logged| {
                matches!(
                    logged.event,
                    Event::Requested {
                        turn: 2,
                        lane: Lane::Trunk,
                        ..
                    }
                )
            })
            .map(|logged| logged.seq)
            .collect();
        assert_eq!(turn_two_requests.len(), 2);
        assert!(turn_two_requests[0] < at && at < turn_two_requests[1]);
        // The request after it: the head, the refill, then turn two's ask
        // and its step -- turn one is gone from it.
        let sent = session.shared.transport.sent();
        let after = &sent[3].messages;
        assert!(
            !after
                .iter()
                .any(|message| message.content == "how are blank lines treated?"),
            "{after:#?}"
        );
        assert!(after.iter().any(|message| message.content == "second"));
        reads_whole(&session);
        let lines: Vec<log::Line> = session.events_from(0).iter().map(line_of).collect();
        let projected = super::super::projection::project(
            &lines,
            &regime(),
            Some(super::super::projection::Engine::Commit("e7051ef")),
        )
        .expect("the log projects");
        let rendered = crate::formats::record::render(&crate::formats::record::Record {
            events: projected.events.clone(),
        });
        crate::formats::record::parse(&rendered)
            .unwrap_or_else(|why| panic!("the record does not read: {why:?}\n{rendered}"));
        // The record's seam row names the prompt that fired it and the
        // window.
        assert!(
            projected.events.iter().any(|event| matches!(
                event,
                crate::formats::record::Event::Seam {
                    at_turn: 2,
                    prompt_tokens: Some(prompt),
                    window: Some(100_000),
                    ..
                } if *prompt == fired.prompt_tokens
            )),
            "{:#?}",
            projected.events
        );
    }

    /// #617: a request the engine refuses as an overflow is seamed once
    /// and sent again on the refilled trunk -- the refusal logged, then the
    /// seam, then the same turn's request again -- and the turn goes on. A
    /// second overflow fails the turn: once per turn. With nothing in
    /// working memory there is nothing to seam, and it fails as before.
    #[test]
    fn an_overflow_seams_once_and_goes_again() {
        const TYPED: &str = r#"{"error":{"code":400,"type":"exceed_context_size_error"}}"#;
        let refused = || vec![Step::Reject(400, TYPED.to_owned())];
        let turn_two = |session: &Session<Canned>| {
            session
                .ask("how are blank lines treated?", None)
                .expect("accepted");
            wait_until(session, "turn one", settled);
            session.ask("second", None).expect("accepted");
            let log = wait_until(session, "turn two", |log| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
            });
            reads_whole(session);
            log
        };
        let settled_as = |log: &[Logged]| {
            log.iter()
                .rev()
                .find_map(|logged| match &logged.event {
                    Event::TurnSettled { turn: 2, reason } => Some(*reason),
                    _ => None,
                })
                .expect("turn two settled")
        };
        let mut acts = two_turns_measuring(1_000);
        acts.truncate(2);
        acts.push(refused());
        acts.push(deltas(&["after the seam"]));
        let once = windowed(acts, 1_000_000, crate::seam::policy::Served::default());
        let log = turn_two(&once);
        assert_eq!(settled_as(&log), SettleReason::Final);
        let seams = window_seams(&log);
        assert_eq!(seams.len(), 1, "{log:#?}");
        let (at, _, fired) = seams[0];
        assert_eq!(fired.map(|fired| fired.window), Some(1_000_000));
        let refused_at = log
            .iter()
            .find(|logged| matches!(logged.event, Event::Rejected { .. }))
            .expect("the refusal")
            .seq;
        let again = log
            .iter()
            .filter(|logged| matches!(logged.event, Event::Requested { turn: 2, .. }))
            .map(|logged| logged.seq)
            .max()
            .expect("a request");
        assert!(refused_at < at && at < again, "{log:#?}");
        let lines: Vec<log::Line> = once.events_from(0).iter().map(line_of).collect();
        super::super::projection::project(&lines, &regime(), None).expect("the log projects");

        let mut acts = two_turns_measuring(1_000);
        acts.truncate(2);
        acts.push(refused());
        acts.push(refused());
        let twice = windowed(acts, 1_000_000, crate::seam::policy::Served::default());
        let log = turn_two(&twice);
        assert_eq!(settled_as(&log), SettleReason::Failed);
        assert_eq!(window_seams(&log).len(), 1);

        let mut acts = two_turns_measuring(1_000);
        acts.truncate(2);
        acts[0][1] = record(
            "call-1",
            "fact",
            "The cache is flushed every ninety seconds.",
        );
        acts.push(refused());
        let empty = windowed(acts, 1_000_000, crate::seam::policy::Served::default());
        let log = turn_two(&empty);
        assert_eq!(settled_as(&log), SettleReason::Failed);
        assert!(window_seams(&log).is_empty());
    }

    /// #617: with room in the window no seam fires; turned off by the
    /// regimen none fires however full it is; and with nothing in working
    /// memory the request goes as it is.
    #[test]
    fn no_window_seam_with_room_when_turned_off_or_with_nothing_to_refill_from() {
        let run = |session: Session<Canned>| {
            session
                .ask("how are blank lines treated?", None)
                .expect("accepted");
            wait_until(&session, "turn one", settled);
            session.ask("second", None).expect("accepted");
            let log = wait_until(&session, "turn two", |log| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
            });
            reads_whole(&session);
            window_seams(&log).len()
        };
        let roomy = windowed(
            two_turns_measuring(10_000),
            100_000,
            crate::seam::policy::Served::default(),
        );
        assert_eq!(run(roomy), 0);
        let off = windowed(
            two_turns_measuring(90_000),
            100_000,
            crate::seam::policy::Served {
                window_off: true,
                ..crate::seam::policy::Served::default()
            },
        );
        assert_eq!(run(off), 0);
        let mut acts = two_turns_measuring(90_000);
        acts[0] = vec![
            Step::Delta("Nothing worth keeping.".to_owned()),
            record(
                "call-1",
                "fact",
                "The cache is flushed every ninety seconds.",
            ),
        ];
        acts[2][1] = record(
            "call-2",
            "fact",
            "The cache is flushed every ninety seconds.",
        );
        let empty = windowed(acts, 100_000, crate::seam::policy::Served::default());
        assert_eq!(run(empty), 0);
    }

    /// Each fork screened out (#611): its trigger, ask, the capture call
    /// that recorded it, and its field.
    fn skips(log: &[Logged]) -> Vec<(String, AskKind, String, String)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Skipped {
                    trigger,
                    ask,
                    call,
                    field,
                    ..
                } => Some((trigger.clone(), *ask, call.clone(), field.clone())),
                _ => None,
            })
            .collect()
    }

    /// A session with both warrants, self-capture on, and #611's skip as
    /// `skip`, its `bash` running `cat`, playing `acts`.
    fn skipping(tree: &Path, skip: bool, acts: Vec<Vec<Step>>) -> Session<Canned> {
        std::fs::write(tree.join("notes.md"), "a note\n").expect("a note");
        let mut interview = interviewing(&[log::Warrant::Scoping, log::Warrant::Read]);
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        interview.skip_self_recorded = skip;
        let mut shape = looping();
        declare_self_capture(&mut shape, Some(&interview));
        let mut tools = tools(
            Confinement::Unconfined,
            tree,
            &["cat"],
            None,
            Decider::Decline,
        );
        tools.approvals_off = true;
        Session::open_with(
            Canned::new(acts),
            shape,
            None,
            Some(tools),
            None,
            Some(interview),
        )
    }

    /// The fields an ask asks for (#611), read off the ask set's words.
    #[test]
    fn an_asks_fields_are_read_off_its_words() {
        let ask = |kind: AskKind| {
            router::Ask { kind, intent: None }.render_in(
                &crate::dogma::asks::V3,
                &Facts::default(),
                None,
            )
        };
        assert_eq!(asked_fields(&ask(AskKind::Judgment)), ["decision", "plan"]);
        assert_eq!(
            asked_fields(&ask(AskKind::Finding)),
            ["learned", "evidence", "stuck"]
        );
        assert_eq!(
            asked_fields(&ask(AskKind::ApiSurface)),
            ["api_surface", "learned", "stuck"]
        );
        assert!(asked_fields("say anything").is_empty());
    }

    /// A marked turn whose model recorded a decision itself (#611): its
    /// judgment fork is skipped and the log names the capture call; with
    /// the skip off, the fork fires.
    #[test]
    fn a_judgment_fork_the_turn_recorded_itself_is_skipped() {
        let tree = scratch("skip-judgment");
        let acts = |forks: usize| {
            let mut acts = vec![
                vec![
                    Step::Delta(SCOPED.to_owned()),
                    record("call-1", "decision", "a tracker for one team"),
                ],
                deltas(&["Next, I will sketch the schema."]),
            ];
            acts.extend((0..forks).map(|_| deltas(&[DECIDED])));
            acts
        };
        let session = skipping(&tree, true, acts(0));
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert_eq!(
            skips(&log),
            [(
                "turn_end".to_owned(),
                AskKind::Judgment,
                "call-1".to_owned(),
                "decision".to_owned()
            )]
        );
        assert!(forks(&log).is_empty());
        reads_whole(&session);
        let session = skipping(&tree, false, acts(1));
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        assert!(skips(&log).is_empty());
        assert_eq!(forks(&log).len(), 1);
        tidy(&[&tree]);
    }

    /// A read fork (#611) is skipped only by what the turn recorded after
    /// the read: a `learned` recorded before the read leaves its fork to
    /// fire.
    #[test]
    fn a_read_fork_is_skipped_only_by_a_record_made_after_the_read() {
        let tree = scratch("skip-read");
        let session = skipping(
            &tree,
            true,
            vec![
                vec![bash("call-1", "cat notes.md")],
                vec![record("call-2", "learned", "the note says a note")],
                deltas(&["It says a note."]),
            ],
        );
        session.ask("read the notes", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert_eq!(
            skips(&log),
            [(
                "call:document-read:call-1".to_owned(),
                AskKind::Finding,
                "call-2".to_owned(),
                "learned".to_owned()
            )]
        );
        assert!(forks(&log).is_empty());
        reads_whole(&session);
        let session = skipping(
            &tree,
            true,
            vec![
                vec![record("call-1", "learned", "notes exist")],
                vec![bash("call-2", "cat notes.md")],
                deltas(&["It says a note."]),
                deltas(&["LEARNED: the note says a note\n"]),
            ],
        );
        session.ask("read the notes", None).expect("accepted");
        let log = wait_until(&session, "the fork", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        assert!(skips(&log).is_empty());
        assert_eq!(forks(&log).len(), 1);
        tidy(&[&tree]);
    }

    fn record(id: &str, field: &str, content: &str) -> Step {
        Step::call(
            0,
            id,
            "update_record",
            &serde_json::json!({ "field": field, "content": content }).to_string(),
        )
    }

    fn captures(log: &[Logged]) -> Vec<(String, Vec<String>)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Captured {
                    outcome, entries, ..
                } => Some((outcome.clone(), entries.clone())),
                _ => None,
            })
            .collect()
    }

    /// #609: with self-capture off the request's tools are exactly as they
    /// were; on, the contract's three follow them, from the first request.
    #[test]
    fn self_capture_off_leaves_the_tools_byte_identical_and_on_adds_the_contracts() {
        let before = looping();
        let mut off = before.clone();
        declare_self_capture(&mut off, Some(&interviewing(&[log::Warrant::Scoping])));
        declare_self_capture(&mut off, None);
        assert_eq!(off.tools, before.tools);
        let mut on_interview = interviewing(&[]);
        on_interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        let mut on = before.clone();
        declare_self_capture(&mut on, Some(&on_interview));
        let names: Vec<&str> = on.tools.iter().map(|tool| tool.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "bash",
                "update_record",
                "resolve_entry",
                "propose_phase_transition"
            ]
        );
        assert_eq!(on.tools[0], before.tools[0]);
    }

    /// #609: a fact the model said this turn, recorded through
    /// `update_record`, enters working memory as the self-capture lane, is
    /// logged as a `capture` line, and the model is shown the entry's id.
    #[test]
    fn a_self_capture_call_records_a_grounded_fact_and_logs_it() {
        let session = self_capturing(
            vec![
                vec![
                    Step::Delta("The parser drops blank lines before it tokenizes.".to_owned()),
                    record(
                        "call-1",
                        "fact",
                        "The parser drops blank lines before it tokenizes.",
                    ),
                ],
                deltas(&["done"]),
            ],
            3,
        );
        session
            .ask("how does the parser treat blank lines?", None)
            .expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        let recorded = captures(&log);
        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0].0, "recorded");
        let entry = recorded[0].1[0].clone();
        assert!(
            entry.starts_with('r') && entry.ends_with("/call-1/fact"),
            "{entry}"
        );
        let held = session.shared.lock();
        let object = &held.interview.as_ref().expect("working memory").object;
        let kept = object
            .entry(&crate::object::EntryId::new(&entry).expect("an id"))
            .expect("recorded");
        assert_eq!(kept.provenances[0].lane, crate::capture::tools::LANE);
        drop(held);
        // One place for every working-memory change: a `patch` line, named
        // by its lane rather than a fork, carrying the entry's text.
        let patch_lines: Vec<(Option<u64>, Option<String>, log::PatchEntry)> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::Patched {
                    fork, lane, entry, ..
                } => Some((*fork, lane.clone(), entry.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            patch_lines,
            [(
                None,
                Some(crate::capture::tools::LANE.to_owned()),
                log::PatchEntry {
                    id: entry.clone(),
                    text: "The parser drops blank lines before it tokenizes.".to_owned(),
                    category: None,
                }
            )]
        );
        let shown = lines(&log)[0].shown.clone();
        assert_eq!(shown, Some(format!("recorded: {entry}")));
        reads_whole(&session);
    }

    /// #609: content the model never saw is dropped by the groundedness
    /// gate, nothing is written, and the drop is logged with why.
    #[test]
    fn an_ungrounded_self_capture_is_dropped_and_says_so() {
        let session = self_capturing(
            vec![
                vec![
                    Step::Delta("Looking at it now.".to_owned()),
                    record(
                        "call-1",
                        "fact",
                        "The cache is flushed every ninety seconds.",
                    ),
                ],
                deltas(&["done"]),
            ],
            3,
        );
        session
            .ask("what about the cache?", None)
            .expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert_eq!(captures(&log), [("dropped".to_owned(), Vec::new())]);
        let held = session.shared.lock();
        assert_eq!(
            held.interview
                .as_ref()
                .expect("memory")
                .object
                .live()
                .count(),
            0
        );
    }

    /// A session whose scoping forks answer through the capture tools
    /// (#610), self-capture on, playing `acts`.
    fn forks_through_tools(acts: Vec<Vec<Step>>) -> Session<Canned> {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        interview.capture = crate::dogma::asks::Modality::Tools;
        let mut shape = template();
        declare_self_capture(&mut shape, Some(&interview));
        Session::open_with(Canned::new(acts), shape, None, None, None, Some(interview))
    }

    /// One `update_record` call at `index` of a streamed answer.
    fn record_at(index: u64, id: &str, field: &str, content: &str) -> Step {
        Step::call(
            index,
            id,
            "update_record",
            &serde_json::json!({ "field": field, "content": content }).to_string(),
        )
    }

    /// #610: a fork asked to answer through the capture tools records what
    /// the trunk said as its own -- lane `interview`, fork `f/<n>` -- through
    /// the groundedness gate the trunk's calls go through: a fact only the
    /// fork wrote is dropped, its own words grounding nothing. Each call is
    /// a `capture` line naming the fork, the kept one a `patch` line too,
    /// and the log reads back and projects.
    #[test]
    fn a_fork_answering_through_the_capture_tools_records_only_what_the_trunk_said() {
        const SAID: &str = "The parser drops blank lines before it tokenizes.";
        const UNSAID: &str = "The cache is flushed every ninety seconds.";
        let session = forks_through_tools(vec![
            deltas(&[SAID]),
            vec![
                Step::Delta(UNSAID.to_owned()),
                record_at(0, "f-1", "fact", SAID),
                record_at(1, "f-2", "fact", UNSAID),
            ],
        ]);
        session
            .ask_marked("how does the parser treat blank lines?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        reads_whole(&session);
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Value]);
        let fork = log
            .iter()
            .find(|logged| matches!(logged.event, Event::Forked { .. }))
            .expect("a fork")
            .seq;
        let lines: Vec<(String, Vec<String>, Option<u64>)> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::Captured {
                    outcome,
                    entries,
                    fork,
                    ..
                } => Some((outcome.clone(), entries.clone(), *fork)),
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!((lines[0].0.as_str(), lines[0].2), ("recorded", Some(fork)));
        assert_eq!(
            (lines[1].0.as_str(), lines[1].1.clone(), lines[1].2),
            ("dropped", Vec::new(), Some(fork))
        );
        let entry = lines[0].1[0].clone();
        assert_eq!(
            patches(&log)
                .into_iter()
                .map(|patched| patched.id)
                .collect::<Vec<_>>(),
            std::slice::from_ref(&entry)
        );
        let held = session.shared.lock();
        let object = &held.interview.as_ref().expect("working memory").object;
        assert_eq!(object.live().count(), 1);
        let kept = object
            .entry(&crate::object::EntryId::new(&entry).expect("an id"))
            .expect("recorded");
        assert_eq!(kept.provenances[0].lane, super::super::INTERVIEW);
        assert_eq!(kept.provenances[0].fork.clone(), Some(format!("f/{fork}")));
        drop(held);
        let projected: Vec<log::Line> = session.events_from(0).iter().map(line_of).collect();
        super::super::projection::project(&projected, &regime(), None).expect("the log projects");
    }

    /// #610: under the tools modality a fork that only writes prose
    /// declines -- its fields are not parsed -- and one that only calls a
    /// tool a fork never runs is `unparseable`.
    #[test]
    fn a_tools_fork_that_calls_no_capture_tool_declines_or_is_unparseable() {
        let prose =
            forks_through_tools(vec![deltas(&["Scoped."]), deltas(&["DECISION: a tracker"])]);
        prose.ask_marked("scope it", None, true).expect("accepted");
        let log = wait_until(&prose, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Decline]);
        assert!(patches(&log).is_empty());
        let bash = forks_through_tools(vec![
            deltas(&["Scoped."]),
            vec![Step::call(0, "f-1", "bash", r#"{"command":"ls"}"#)],
        ]);
        bash.ask_marked("scope it", None, true).expect("accepted");
        let log = wait_until(&bash, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Unparseable]);
        reads_whole(&bash);
    }

    /// #610: the modality is `fields` unless the regimen says `tools`, and
    /// any other word is refused.
    #[test]
    fn the_capture_modality_is_fields_unless_the_regimen_says_tools() {
        use crate::dogma::asks::Modality;
        let read = |extra: &str| {
            crate::formats::regimen::parse(&format!("{extra}\n{}", super::super::canned::DEV_LOOP))
                .expect("a regimen")
        };
        assert_eq!(capture_modality(&read("")), Ok(Modality::Fields));
        assert_eq!(
            capture_modality(&read(r#"capture_modality = "fields""#)),
            Ok(Modality::Fields)
        );
        assert_eq!(
            capture_modality(&read(r#"capture_modality = "tools""#)),
            Ok(Modality::Tools)
        );
        assert!(capture_modality(&read(r#"capture_modality = "both""#)).is_err());
    }

    /// #609: after the cadence of silent turns the next ask carries the
    /// reminder as a note after it, logged; the system prompt is untouched.
    #[test]
    fn the_self_capture_reminder_is_a_note_after_the_next_ask() {
        let session = self_capturing(vec![deltas(&["one"]), deltas(&["two"])], 1);
        session.ask("first", None).expect("accepted");
        wait_until(&session, "turn one", settled);
        session.ask("second", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        let reminder = crate::capture::tools::AskKind::Reminder.opening();
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::Reminded { turn: 2, text } if text == reminder
        )));
        let sent = session.shared.transport.sent();
        let second = &sent[1].messages;
        assert_eq!(second.last().map(|m| m.content.as_str()), Some(reminder));
        assert_eq!(second[second.len() - 2].content, "second");
        assert_eq!(
            second[0], sent[0].messages[0],
            "the system prompt is unchanged"
        );
        reads_whole(&session);
    }

    /// #609: the regimen's words, read leniently.
    #[test]
    fn self_capture_is_read_leniently() {
        let read = |text: &str| self_capture(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read(""), None);
        assert_eq!(read("self_capture = false\n"), None);
        assert_eq!(
            read("self_capture = true\n"),
            Some(crate::capture::tools::Cadence::DEFAULT)
        );
        assert_eq!(
            read("self_capture = \"on\"\nself_capture_cadence = 5\n")
                .map(crate::capture::tools::Cadence::interval),
            Some(5)
        );
        assert_eq!(
            read("self_capture = true\nself_capture_cadence = 0\n"),
            Some(crate::capture::tools::Cadence::DEFAULT)
        );
    }

    #[test]
    fn a_forks_add_patches_are_not_delivered_mid_turn() {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.delivery = log::ForkDelivery::Imperative;
        let session = Session::open_with(
            Canned::new([deltas(&[SCOPED]), deltas(&[DECIDED]), deltas(&["next"])]),
            template(),
            None,
            None,
            None,
            Some(interview),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        session.ask("go on", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|l| matches!(l.event, Event::Asked { .. }))
                    .count()
                    == 2
        });
        reads_whole(&session);
        assert!(log.iter().any(|l| matches!(
            l.event,
            Event::Patched {
                op: log::PatchOp::Add,
                ..
            }
        )));
        assert!(
            !log.iter()
                .any(|l| matches!(l.event, Event::Delivered { .. }))
        );
    }

    /// The definition of done's first line: the scope-boundary gap (an ask the operator
    /// marked `scoping`) fires one fork, off the warm trunk and never
    /// appended to it, whose patch carries the three decisions -- three
    /// `patch` lines, applied to the session's working object.
    #[test]
    fn a_scoping_gap_fires_one_fork_whose_patches_carry_its_decisions() {
        let session = Session::open_with(
            Canned::new([deltas(&[SCOPED]), deltas(&[DECIDED])]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        reads_whole(&session);
        let at = answered_at(&log);
        let question = judgment_after(SCOPED);
        assert_eq!(
            forks(&log),
            // After the settling, and the move to `capture`.
            [(
                settling_seq(&log, 1) + 2,
                1,
                at,
                log::Warrant::Scoping,
                question.clone()
            )],
            "{log:#?}"
        );
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Value]);
        let texts: Vec<String> = patches(&log).into_iter().map(|entry| entry.text).collect();
        assert_eq!(
            texts,
            [
                "decision: a tracker",
                "decision: for one team",
                "decision: no login"
            ]
        );
        let held = session.shared.lock();
        let object = &held.interview.as_ref().expect("interviewing").object;
        assert_eq!(object.live().count(), 3);
        drop(held);
        // Born off the warm trunk: its messages, then the question; and
        // never appended to it.
        let sent = session.shared.transport.sent();
        assert_eq!(sent.len(), 2, "one turn, one fork");
        let trunk = session.trunk();
        assert_eq!(
            trunk[1..],
            [user("what are we building?"), assistant(SCOPED)]
        );
        let mut born = trunk;
        born.push(user(&question));
        assert_eq!(sent[1].messages, born);
        assert_eq!(session.settlement(), Settlement::Awaiting);
    }

    // -----------------------------------------------------------------------
    // derived seams: the regimen's cadence and budget
    // -----------------------------------------------------------------------

    /// An interview whose regimen declares `seams`.
    fn seaming(seams: crate::seam::policy::Served) -> Interview {
        Interview {
            seams,
            ..interviewing(&[log::Warrant::Scoping])
        }
    }

    /// An interview under a three-phase graph: plan to build to review.
    fn phased() -> Interview {
        let mut graph = crate::seam::phase::PhaseGraph::of(&[
            "plan".to_owned(),
            "build".to_owned(),
            "review".to_owned(),
        ])
        .expect("three phases");
        graph.allow("plan", "build").expect("an edge");
        graph.allow("build", "review").expect("an edge");
        Interview {
            phases: graph,
            ..interviewing(&[log::Warrant::Scoping])
        }
    }

    /// #563: a served session opens in its graph's first phase and logs the
    /// graph; a seam that names a move the graph allows moves, the refill
    /// renders the new phase, and the seam line says from and to; a move it
    /// refuses is refused with the graph's reason, logged; with no phase
    /// named the session stays where it is; and the projection rebuilds the
    /// refilled head.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_served_seam_moves_between_the_phases_its_graph_allows() {
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&["started on the schema"]),
            ]),
            template(),
            None,
            None,
            None,
            Some(phased()),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        for (to, refused) in [
            ("review", Refusal::NoPhaseEdge),
            ("plan", Refusal::AlreadyInPhase),
            ("ship", Refusal::NotAPhase),
        ] {
            assert_eq!(
                session.declare_seam_to(None, Some(to)),
                Err(Rejected::Refused(refused)),
                "{to}"
            );
        }
        session
            .declare_seam_to(None, Some("build"))
            .expect("plan to build is allowed");
        let render = |phase: &str| {
            let held = session.shared.lock();
            let object = &held.interview.as_ref().expect("interviewing").object;
            crate::seam::render::render(object, Some(phase))
        };
        assert_eq!(
            session.trunk(),
            crate::seam::render::refill(&template().messages, &render("build"))
        );
        // No phase named: a seam that stays in `build`.
        session.declare_seam(None).expect("a seam that stays");
        session.ask("go on", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|l| matches!(l.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        let lines: Vec<log::Line> = log.iter().map(line_of).collect();
        let log::Event::SessionStart {
            phases,
            phase_transitions,
            opening_phase,
            ..
        } = &lines[0].event
        else {
            panic!("the session's start");
        };
        assert_eq!(
            phases.as_deref(),
            Some(&["plan".to_owned(), "build".to_owned(), "review".to_owned()][..])
        );
        assert_eq!(
            phase_transitions.as_ref().map(Vec::len),
            Some(2),
            "{phase_transitions:?}"
        );
        assert_eq!(opening_phase.as_deref(), Some("plan"));
        let moves: Vec<Option<log::PhaseMove>> = lines
            .iter()
            .filter_map(|line| match &line.event {
                log::Event::Seam { phase, .. } => Some(phase.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            moves,
            [
                Some(log::PhaseMove {
                    from: "plan".to_owned(),
                    to: "build".to_owned()
                }),
                None
            ]
        );
        let refusals: Vec<log::Refusal> = lines
            .iter()
            .filter_map(|line| match &line.event {
                log::Event::Refused { because, .. } => Some(*because),
                _ => None,
            })
            .collect();
        assert_eq!(
            refusals,
            [
                log::Refusal::NoPhaseEdge,
                log::Refusal::AlreadyInPhase,
                log::Refusal::NotAPhase
            ]
        );
        // The projection rebuilds the head the refill made, phase and all.
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        assert!(
            !projected
                .unspellable
                .iter()
                .any(|item| item.why.contains("rebuilt")),
            "{:?}",
            projected.unspellable
        );
        reads_whole(&session);
    }

    /// #566, `literal`: a seam archives what it drops; a later ask that
    /// names it gets one note after it, in Qwen Code's reminder shape, logged
    /// `recalled` with each item's key, digest and score; the note stays on
    /// the trunk and the projection rebuilds the head. Under `off`, nothing.
    #[test]
    fn a_literal_recall_brings_back_what_a_seam_archived() {
        for recall in [
            super::super::archive::Recall::Literal,
            super::super::archive::Recall::Off,
        ] {
            let session = Session::open_with(
                Canned::new([
                    deltas(&[SCOPED]),
                    deltas(&[DECIDED]),
                    deltas(&["a table per team"]),
                ]),
                template(),
                None,
                None,
                None,
                Some(Interview {
                    recall,
                    ..interviewing(&[log::Warrant::Scoping])
                }),
            );
            session
                .ask_marked("what are we building?", None, true)
                .expect("accepted");
            wait_until(&session, "the fork to settle", |log| {
                settled(log) && !fork_outcomes(log).is_empty()
            });
            session.declare_seam(None).expect("a seam");
            session
                .ask("how should the `schema` look?", None)
                .expect("accepted");
            let log = wait_until(&session, "turn two", |log| {
                settled(log)
                    && log
                        .iter()
                        .filter(|l| matches!(l.event, Event::TurnSettled { .. }))
                        .count()
                        == 2
            });
            reads_whole(&session);
            let lines: Vec<log::Line> = log.iter().map(line_of).collect();
            let recalled: Vec<&log::Event> = lines
                .iter()
                .map(|line| &line.event)
                .filter(|event| matches!(event, log::Event::Recalled { .. }))
                .collect();
            let sent = session.shared.transport.sent();
            let asked = sent.last().expect("turn two's request");
            if recall == super::super::archive::Recall::Off {
                assert!(recalled.is_empty(), "{recalled:?}");
                assert_eq!(
                    asked.messages.last().map(|m| m.content.as_str()),
                    Some("how should the `schema` look?")
                );
                continue;
            }
            let [
                log::Event::Recalled {
                    turn, text, items, ..
                },
            ] = recalled.as_slice()
            else {
                panic!("one recall: {recalled:?}");
            };
            assert_eq!(*turn, 2);
            // The scoping turn's answer, archived by the seam, names the schema.
            assert_eq!(items[0].key, "seam-1/message-2");
            assert_eq!(
                items[0].sha256,
                crate::digest::sha256_hex(SCOPED.as_bytes())
            );
            assert_eq!(items[0].score, 1);
            assert!(
                text.starts_with("<system-reminder>\n## Relevant memory"),
                "{text}"
            );
            assert!(text.contains(SCOPED), "{text}");
            // The note follows the ask, on the request and on the trunk.
            let n = asked.messages.len();
            assert_eq!(
                asked.messages[n - 2].content,
                "how should the `schema` look?"
            );
            assert_eq!(&asked.messages[n - 1].content, text);
            assert!(session.trunk().iter().any(|m| &m.content == text));
            let projected =
                crate::drive::projection::project(&lines, &regime(), None).expect("projected");
            assert!(
                !projected
                    .unspellable
                    .iter()
                    .any(|item| item.why.contains("rebuilt")),
                "{:?}",
                projected.unspellable
            );
        }
    }

    /// #563: naming a phase with no graph is refused, logged.
    #[test]
    fn a_phase_named_with_no_graph_is_refused() {
        let session = Session::open_with(
            Canned::new([deltas(&[SCOPED]), deltas(&[DECIDED])]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        assert_eq!(
            session.declare_seam_to(None, Some("build")),
            Err(Rejected::Refused(Refusal::NoPhaseGraph))
        );
        reads_whole(&session);
    }

    /// A `prune_output` call naming `call`.
    fn prune_step(id: &str, call: &str) -> Step {
        Step::call(
            0,
            id,
            super::super::prune::PRUNE_OUTPUT,
            &serde_json::json!({ "call": call }).to_string(),
        )
    }

    /// A session that runs `cat` and `echo`, offers `prune_output` applied
    /// under `prune`, keeps a tail of `tail_tokens`, and plays `acts`; its
    /// tree holds a sizeable `notes.md`.
    fn pruning(
        tree: &Path,
        prune: super::super::prune::PruneSeam,
        tail_tokens: u64,
        acts: Vec<Vec<Step>>,
    ) -> Session<Canned> {
        let notes: String = (0..80)
            .map(|n| format!("note line {n}\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(tree.join("notes.md"), notes).expect("the notes");
        let mut shape = looping();
        shape.tools.push(super::super::prune::definition());
        Session::open_with(
            Canned::new(acts),
            shape,
            None,
            Some(tools(
                Confinement::Unconfined,
                tree,
                &["cat", "echo"],
                None,
                Decider::Decline,
            )),
            None,
            Some(Interview {
                seams: crate::seam::policy::Served {
                    tail_tokens,
                    ..crate::seam::policy::Served::default()
                },
                prune: Some(prune),
                ..interviewing(&[log::Warrant::Scoping])
            }),
        )
    }

    /// What each call was shown, by its id, in call order.
    fn shown_by_id(log: &[Logged]) -> Vec<(String, String)> {
        lines(log)
            .into_iter()
            .map(|line| (line.id, line.shown.unwrap_or_default()))
            .collect()
    }

    /// The `pruned` lines: each call and its text.
    fn prunes_in(log: &[Logged]) -> Vec<(String, u64, String)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Pruned {
                    call, bytes, text, ..
                } => Some((call.clone(), *bytes, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// The seam lines' `pruned`.
    fn seam_prunes(log: &[Logged]) -> Vec<Option<Vec<String>>> {
        log.iter()
            .filter_map(|logged| match line_of(logged).event {
                log::Event::Seam { pruned, .. } => Some(pruned),
                _ => None,
            })
            .collect()
    }

    /// #612, `turn_end`: the tool answers at once and the trunk keeps the
    /// result through the turn -- the prefix rule -- then a seam derived as
    /// the turn settles replaces it, in the kept tail, by its reference
    /// line; the next request carries the line, and the projection rebuilds
    /// every head.
    #[test]
    fn a_pruned_result_is_replaced_by_its_line_at_the_seam_its_turn_end_derives() {
        let tree = scratch("t612-turn-end");
        let session = pruning(
            &tree,
            super::super::prune::PruneSeam::TurnEnd,
            100_000,
            vec![
                vec![bash("call-1", "cat notes.md")],
                vec![prune_step("call-2", "cat notes.md")],
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&["after"]),
            ],
        );
        session
            .ask_marked("read the notes", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the prune's seam", |log| {
            !seams_in(log).is_empty()
        });
        let notes = std::fs::read_to_string(tree.join("notes.md")).expect("the notes");
        let shown = shown_by_id(&log);
        assert_eq!(shown[0], ("call-1".to_owned(), notes.clone()));
        assert_eq!(
            shown[1],
            (
                "call-2".to_owned(),
                "pruned; replaced when this turn ends".to_owned()
            )
        );
        let prunes = prunes_in(&log);
        let [(call, bytes, text)] = prunes.as_slice() else {
            panic!("one prune: {prunes:?}");
        };
        assert_eq!((call.as_str(), *bytes), ("call-1", notes.len() as u64));
        assert!(
            text.starts_with("- turn 1: bash args={\"command\":\"cat notes.md\"}: ")
                && text.contains(&crate::digest::sha256_hex(notes.as_bytes())),
            "{text}"
        );
        // The prefix rule: the step after the prune still carried it whole.
        let sent = session.shared.transport.sent();
        assert!(
            sent[2].messages.iter().any(|m| m.content == notes),
            "the trunk was rewritten mid-turn"
        );
        assert_eq!(seams_in(&log), [(1, log::SeamReason::Prune)]);
        assert_eq!(seam_prunes(&log), [Some(vec!["call-1".to_owned()])]);
        // In the kept tail, in place: the tool message answers its call.
        let trunk = session.trunk();
        let result = trunk
            .iter()
            .find(|m| m.tool_call_id.as_deref() == Some("call-1"))
            .expect("call-1's result stays, replaced");
        assert_eq!(&result.content, text);
        assert!(!trunk.iter().any(|m| m.content == notes));

        session.ask("and now?", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        let sent = session.shared.transport.sent();
        let last = &sent.last().expect("turn two's request").messages;
        assert!(last.iter().any(|m| &m.content == text));
        assert!(!last.iter().any(|m| m.content == notes));
        let lines: Vec<log::Line> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        let named: Vec<String> = projected
            .unspellable
            .iter()
            .filter(|u| u.kind == "request")
            .map(|u| u.why.clone())
            .collect();
        assert_eq!(named, Vec::<String>::new());
        reads_whole(&session);
        tidy(&[&tree]);
    }

    /// #612, `next` (the default): no seam is derived for a prune; the
    /// operator's seam applies it, a total refill carrying its line in the
    /// section under `evict`, as reference. The tool answers a name no call
    /// matches, a result already pruned, and one a seam compacted away with
    /// what is wrong.
    #[test]
    fn a_pruned_result_waits_for_the_next_seam_and_the_tool_says_what_it_cannot_prune() {
        let tree = scratch("t612-next");
        let session = pruning(
            &tree,
            super::super::prune::PruneSeam::Next,
            0,
            vec![
                vec![bash("call-1", "cat notes.md")],
                vec![bash("call-5", "echo hi")],
                vec![prune_step("call-2", "cat notes.md")],
                vec![prune_step("call-3", "nope")],
                vec![prune_step("call-6", "cat notes.md")],
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                vec![prune_step("call-4", "echo hi")],
                deltas(&["ok"]),
            ],
        );
        session
            .ask_marked("read the notes", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        assert!(seams_in(&log).is_empty(), "no seam is derived under `next`");
        session.declare_seam(None).expect("a seam");
        let log = wait_until(&session, "the seam", |log| !seams_in(log).is_empty());
        assert_eq!(seam_prunes(&log), [Some(vec!["call-1".to_owned()])]);
        let (state, section, ..) = seam_outputs_of(&log);
        assert_eq!(state, Some(log::SeamToolOutputs::Evict));
        let section = section.expect("a section carrying the pruned result");
        let [(_, _, text)] = prunes_in(&log).try_into().expect("one prune");
        assert!(
            section.contains("listed as reference only") && section.ends_with(&text),
            "{section}"
        );
        assert_eq!(
            section.matches("- turn").count(),
            1,
            "only the pruned one: {section}"
        );

        session.ask("tidy", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        let shown: Vec<String> = shown_by_id(&log)
            .into_iter()
            .filter(|(id, _)| ["call-2", "call-3", "call-6", "call-4"].contains(&id.as_str()))
            .map(|(_, shown)| shown)
            .collect();
        assert_eq!(
            shown,
            [
                "pruned; replaced at the next context compaction",
                "prune_output: no earlier call matches `nope`",
                super::super::prune::ALREADY_PRUNED,
                super::super::prune::ALREADY_COMPACTED,
            ]
        );
        assert_eq!(prunes_in(&log).len(), 1, "a refused prune logs none");
        reads_whole(&session);
        tidy(&[&tree]);
    }

    /// A session whose first turn reads `notes.md` and draws a read fork
    /// quoting `line two`, whose second runs `echo tail`, and which seams
    /// every `every` turns at `tail_tokens` under `state` (#553).
    fn seaming_outputs(
        tree: &Path,
        state: log::SeamToolOutputs,
        (every, tail_tokens): (u32, u64),
        recording: Option<&Path>,
    ) -> Session<Canned> {
        let notes: String = std::iter::once("line one\nline two\n".to_owned())
            .chain((0..60).map(|n| format!("filler line {n}\n")))
            .collect();
        std::fs::write(tree.join("notes.md"), notes).expect("the notes");
        let mut tools = tools(
            Confinement::Unconfined,
            tree,
            &["cat", "echo"],
            None,
            Decider::Decline,
        );
        tools.recording = recording.map(Path::to_path_buf);
        Session::open_with(
            Canned::new([
                vec![bash("call-1", "cat notes.md")],
                deltas(&["It says line two."]),
                deltas(&["EXCERPT: line two\n"]),
                vec![bash("call-2", "echo tail")],
                deltas(&["ok"]),
                deltas(&["next"]),
                deltas(&["after"]),
            ]),
            looping(),
            None,
            Some(tools),
            None,
            Some(Interview {
                seams: crate::seam::policy::Served {
                    every_turns: Some(every),
                    at_trunk_tokens: None,
                    tail_tokens,
                    outputs: state,
                    render_budget: None,
                    window_off: false,
                    warm: false,
                    audit: false,
                },
                ..interviewing(&[log::Warrant::Read])
            }),
        )
    }

    /// The seam line's tool-output fields: its state, its section, how many
    /// outputs and how many bytes it carried.
    fn seam_outputs_of(
        log: &[Logged],
    ) -> (
        Option<log::SeamToolOutputs>,
        Option<String>,
        Option<u64>,
        Option<u64>,
    ) {
        log.iter()
            .find_map(|logged| match line_of(logged).event {
                log::Event::Seam {
                    tool_outputs,
                    outputs,
                    carried_outputs,
                    carried_output_bytes,
                    ..
                } => Some((tool_outputs, outputs, carried_outputs, carried_output_bytes)),
                _ => None,
            })
            .expect("a seam line")
    }

    /// Runs `seaming_outputs`' turns through the seam and one ask after it:
    /// the log, and the request that ask sent.
    fn through_the_seam(session: &Session<Canned>, turns: u32) -> (Vec<Logged>, RequestShape) {
        session.ask("read the notes", None).expect("accepted");
        // Settled, or -- at a cadence of one -- seamed after the fork.
        wait_until(session, "the first turn and its fork", |log| {
            !fork_outcomes(log).is_empty() && (settled(log) || !seams_in(log).is_empty())
        });
        if turns == 2 {
            session.ask("echo something", None).expect("accepted");
        }
        wait_until(session, "the seam", |log| !seams_in(log).is_empty());
        session.ask("go on", None).expect("accepted");
        let log = wait_until(session, "the turn after the seam", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == turns as usize + 1
        });
        reads_whole(session);
        let sent = session.shared.transport.sent();
        (log, sent.last().expect("a request").clone())
    }

    /// `evict`, the default: the refill carries nothing of the outputs it
    /// compacts away, and the seam line names the state alone (#553).
    #[test]
    fn evict_carries_no_tool_output_across_the_seam() {
        let tree = scratch("seam-evict");
        let session = seaming_outputs(&tree, log::SeamToolOutputs::Evict, (1, 0), None);
        let (log, sent) = through_the_seam(&session, 1);
        assert_eq!(
            seam_outputs_of(&log),
            (Some(log::SeamToolOutputs::Evict), None, None, None)
        );
        let head = template().messages.len();
        assert!(
            !sent.messages[head]
                .content
                .contains(crate::seam::outputs::HEADER)
        );
        // Nothing of turn 1 after the refill: the next ask follows it.
        assert_eq!(sent.messages[head + 1].content, "go on");
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// After a seam the system message is the session's first, byte for
    /// byte, and the render rides in a user message after it (#597). A log
    /// written before #597 -- its seam with no `placement`, its heads hashed
    /// over the render in the system message -- still rebuilds every head,
    /// and the digest still decides: the wrong placement verifies nothing.
    #[test]
    fn the_system_message_survives_a_seam_and_a_log_from_before_597_still_rebuilds() {
        let tree = scratch("seam-placement");
        let session = seaming_outputs(&tree, log::SeamToolOutputs::Evict, (1, 0), None);
        let (log, _) = through_the_seam(&session, 1);
        let head = template().messages.len();
        let sent = session.shared.transport.sent();
        let after: Vec<&RequestShape> = sent
            .iter()
            .filter(|shape| {
                shape.messages.len() > head
                    && shape.messages[head].content.starts_with("<summary>\n")
            })
            .collect();
        assert!(!after.is_empty(), "a request after the seam");
        for shape in &after {
            assert_eq!(
                shape.messages[..head],
                template().messages[..],
                "never mutated"
            );
        }
        every_head_rebuilds(&log);

        // The same session as a build before #597 sent it: the render
        // appended to the system message, no refill message.
        let digest =
            |shape: &RequestShape| crate::client::head::Head::of(shape).digest().to_owned();
        let old: BTreeMap<String, String> = after
            .iter()
            .map(|shape| {
                let mut before = (*shape).clone();
                let refill = before.messages.remove(head);
                let render = refill
                    .content
                    .strip_prefix("<summary>\n")
                    .and_then(|text| text.strip_suffix("\n</summary>"))
                    .expect("the refill's wrapper");
                before.messages[0].content.push_str("\n\n");
                before.messages[0].content.push_str(render);
                (digest(shape), digest(&before))
            })
            .collect();
        let rewritten = |placement: Option<log::RenderPlacement>| -> Vec<log::Line> {
            log.iter()
                .map(line_of)
                .map(|mut line| {
                    match &mut line.event {
                        log::Event::Seam { placement: at, .. } => *at = placement,
                        log::Event::Request {
                            head_sha256: Some(sha),
                            ..
                        } => {
                            if let Some(before) = old.get(sha.as_str()) {
                                sha.clone_from(before);
                            }
                        }
                        _ => {}
                    }
                    line
                })
                .collect()
        };
        let unrebuilt = |lines: &[log::Line]| -> usize {
            crate::drive::projection::project(lines, &regime(), None)
                .expect("projected")
                .unspellable
                .iter()
                .filter(|named| named.why.contains("could not be rebuilt"))
                .count()
        };
        assert_eq!(
            unrebuilt(&rewritten(None)),
            0,
            "a log from before #597 rebuilds"
        );
        assert!(
            unrebuilt(&rewritten(Some(log::RenderPlacement::Message))) > 0,
            "the wrong placement verifies nothing"
        );
        tidy(&[&tree]);
    }

    /// `keep`: each compacted output as the trunk had it, after the render,
    /// in call order; the kept tail's outputs are untouched -- not in the
    /// section, and still on the trunk as they sat (#553).
    #[test]
    fn keep_carries_the_compacted_outputs_and_leaves_the_kept_tail_alone() {
        let tree = scratch("seam-keep");
        let session = seaming_outputs(&tree, log::SeamToolOutputs::Keep, (2, 100), None);
        let (log, sent) = through_the_seam(&session, 2);
        let (state, section, carried, bytes) = seam_outputs_of(&log);
        let section = section.expect("a section");
        assert_eq!(state, Some(log::SeamToolOutputs::Keep));
        assert_eq!(carried, Some(1));
        assert_eq!(bytes, Some(section.len() as u64));
        let notes = std::fs::read_to_string(tree.join("notes.md")).expect("the notes");
        assert_eq!(
            section,
            format!(
                "\n# tool outputs\nThe following tool results were produced before context was \
                 compacted, in call order:\n\n- turn 1: bash args={{\"command\":\"cat notes.md\"}}\n\
                 [Tool result]: {notes}"
            )
        );
        // The system message as the session sent it first (#597), the
        // refill after it.
        let head = template().messages.len();
        assert_eq!(sent.messages[..head], template().messages[..]);
        assert!(
            sent.messages[head]
                .content
                .ends_with(&format!("{section}\n</summary>"))
        );
        assert!(
            !section.contains("echo tail"),
            "the kept tail is not carried"
        );
        let tail: Vec<&Message> = sent.messages[head + 1..].iter().collect();
        assert_eq!(tail[0].content, "echo something");
        assert_eq!(tail[2].role, Role::Tool);
        assert_eq!(
            tail[2].content, "tail\n",
            "the kept tail's output as it sat"
        );
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// `reference`: one line per compacted output naming its tool,
    /// arguments, size and sha256, its whole saved by digest in the
    /// recording at the path the line gives (#553).
    #[test]
    fn reference_names_each_output_and_saves_its_whole_by_digest() {
        let tree = scratch("seam-reference");
        let recording = scratch("seam-reference-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let session = seaming_outputs(
            &tree,
            log::SeamToolOutputs::Reference,
            (1, 0),
            Some(&recording),
        );
        let (log, sent) = through_the_seam(&session, 1);
        let notes = std::fs::read_to_string(tree.join("notes.md")).expect("the notes");
        let sha256 = crate::digest::sha256_hex(notes.as_bytes());
        let saved = recording.join("files").join(&sha256);
        let (state, section, carried, _) = seam_outputs_of(&log);
        let section = section.expect("a section");
        assert_eq!(state, Some(log::SeamToolOutputs::Reference));
        assert_eq!(carried, Some(1));
        assert!(
            section.ends_with(&format!(
                "- turn 1: bash args={{\"command\":\"cat notes.md\"}}: {} bytes, sha256 {sha256}, \
                 saved at {}\n",
                notes.len(),
                saved.display()
            )),
            "{section}"
        );
        assert_eq!(std::fs::read_to_string(&saved).expect("saved whole"), notes);
        let head = template().messages.len();
        assert!(
            sent.messages[head]
                .content
                .ends_with(&format!("{section}\n</summary>"))
        );
        every_head_rebuilds(&log);
        tidy(&[&tree, &recording]);
    }

    /// `salient`: the excerpt the read fork quoted from the output, and
    /// nothing for an output no fork quoted (#553).
    #[test]
    fn salient_carries_the_read_forks_excerpts_and_nothing_else() {
        let tree = scratch("seam-salient");
        let session = seaming_outputs(&tree, log::SeamToolOutputs::Salient, (2, 0), None);
        let (log, sent) = through_the_seam(&session, 2);
        let (state, section, carried, _) = seam_outputs_of(&log);
        let section = section.expect("a section");
        assert_eq!(state, Some(log::SeamToolOutputs::Salient));
        assert_eq!(carried, Some(1));
        assert_eq!(
            section,
            "\n# tool outputs\nThe following excerpts were quoted from tool results produced \
             before context was compacted, in call order:\n\n- turn 1: bash \
             args={\"command\":\"cat notes.md\"}\n[Tool result excerpt]: line two\n"
        );
        let head = template().messages.len();
        assert!(
            sent.messages[head]
                .content
                .ends_with(&format!("{section}\n</summary>"))
        );
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// The seam lines in `log`, as the log writes them: turn and reason.
    fn seams_in(log: &[Logged]) -> Vec<(u32, log::SeamReason)> {
        log.iter()
            .filter_map(|logged| match line_of(logged).event {
                log::Event::Seam {
                    at_turn, reason, ..
                } => Some((at_turn, reason)),
                _ => None,
            })
            .collect()
    }

    fn a_reply_with_timings(
        text: &str,
        prompt_n: u64,
        cache_n: u64,
        predicted_n: u64,
    ) -> Vec<Step> {
        vec![
            Step::Delta(text.to_owned()),
            Step::Timings(Timings {
                prompt_n: Some(prompt_n),
                cache_n: Some(cache_n),
                predicted_n: Some(predicted_n),
                ..Timings::default()
            }),
        ]
    }

    /// A cadence of one operator turn: the scoping turn's fork fills working
    /// memory, and the seam fires on its own once the gap is over, logged
    /// `cadence`; the next ask runs on the refilled trunk, and the turn after
    /// it fires the next. Nobody declared either.
    #[test]
    fn a_cadence_fires_a_seam_on_its_own_after_each_counted_turn() {
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&["started on the schema"]),
            ]),
            template(),
            None,
            None,
            None,
            Some(seaming(crate::seam::policy::Served {
                every_turns: Some(1),
                at_trunk_tokens: None,
                tail_tokens: 0,
                outputs: log::SeamToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the cadence's seam", |log| {
            !seams_in(log).is_empty()
        });
        assert_eq!(seams_in(&log), vec![(1, log::SeamReason::Cadence)]);
        assert!(
            !session.trunk().iter().any(|m| m.content == SCOPED),
            "the seam carried a turn of the old trunk"
        );
        let refilled = session.trunk();

        session.ask("build it", None).expect("accepted");
        let log = wait_until(&session, "the second seam", |log| seams_in(log).len() == 2);
        assert_eq!(
            seams_in(&log),
            vec![(1, log::SeamReason::Cadence), (2, log::SeamReason::Cadence)]
        );
        let sent = session.shared.transport.sent();
        let mut expected = refilled;
        expected.push(user("build it"));
        assert_eq!(
            sent[2].messages, expected,
            "the ask after the seam runs on the refill"
        );
        assert_eq!(session.settlement(), Settlement::Awaiting);
        reads_whole(&session);
    }

    /// The compaction depth (#552): a seam with a tail budget keeps the
    /// old trunk's most recent whole turn after the refill, as it sat on the
    /// trunk; the seam line names the depth and what it kept; the next ask
    /// runs on head, render and tail; the projection rebuilds every head. A
    /// budget no whole turn fits keeps none.
    #[test]
    fn a_seam_with_a_tail_budget_keeps_the_most_recent_whole_turns() {
        let tailed = |tail_tokens: u64| {
            Session::open_with(
                Canned::new([
                    deltas(&[SCOPED]),
                    deltas(&[DECIDED]),
                    deltas(&["started on the schema"]),
                ]),
                template(),
                None,
                None,
                None,
                Some(seaming(crate::seam::policy::Served {
                    every_turns: Some(1),
                    at_trunk_tokens: None,
                    tail_tokens,
                    outputs: log::SeamToolOutputs::Evict,
                    render_budget: None,
                    window_off: false,
                    warm: false,
                    audit: false,
                })),
            )
        };
        let session = tailed(10_000);
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the seam", |log| !seams_in(log).is_empty());
        let trunk = session.trunk();
        let kept = [
            user("what are we building?"),
            Message::new(Role::Assistant, SCOPED),
        ];
        assert_eq!(trunk[trunk.len() - 2..], kept[..], "{trunk:#?}");
        // The head, the refill message (#597), then the tail.
        assert_eq!(trunk.len(), template().messages.len() + 3);
        let seam = log
            .iter()
            .find_map(|logged| match line_of(logged).event {
                log::Event::Seam {
                    tail_tokens,
                    carried_turns,
                    carried_tokens,
                    tool_outputs: Some(log::SeamToolOutputs::Evict),
                    outputs: None,
                    ..
                } => Some((tail_tokens, carried_turns, carried_tokens)),
                _ => None,
            })
            .expect("a seam line");
        let estimated: u64 = kept.iter().map(crate::seam::render::estimated_tokens).sum();
        assert_eq!(seam, (Some(10_000), 1, Some(estimated)));

        session.ask("build it", None).expect("accepted");
        let log = wait_until(&session, "the second seam", |log| seams_in(log).len() == 2);
        reads_whole(&session);
        let mut expected = trunk.clone();
        expected.push(user("build it"));
        assert_eq!(session.shared.transport.sent()[2].messages, expected);
        every_head_rebuilds(&log);

        // A budget no whole turn fits: the total refill.
        let session = tailed(1);
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the seam", |log| !seams_in(log).is_empty());
        assert_eq!(session.trunk().len(), template().messages.len() + 1);
    }

    /// A budget: the trunk call reports 18 prefilled, 160 reused and 66
    /// generated, 244 tokens, against a limit of 200, so the seam fires
    /// `budget` though the cadence (every 5 turns) has not come round. The
    /// refilled trunk is unmeasured until its next call.
    #[test]
    fn a_budget_fires_a_seam_once_the_trunk_call_reports_the_limit_reached() {
        let session = Session::open_with(
            Canned::new([
                a_reply_with_timings(SCOPED, 18, 160, 66),
                deltas(&[DECIDED]),
            ]),
            template(),
            None,
            None,
            None,
            Some(seaming(crate::seam::policy::Served {
                every_turns: Some(5),
                at_trunk_tokens: Some(200),
                tail_tokens: 0,
                outputs: log::SeamToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the budget's seam", |log| {
            !seams_in(log).is_empty()
        });
        assert_eq!(seams_in(&log), vec![(1, log::SeamReason::Budget)]);
        assert_eq!(session.shared.lock().trunk_tokens, None);
        reads_whole(&session);
    }

    /// Under the limit, no seam: 18 + 160 + 66 is 244, and the limit is 245.
    #[test]
    fn a_trunk_under_the_budget_fires_no_seam() {
        let session = Session::open_with(
            Canned::new([
                a_reply_with_timings(SCOPED, 18, 160, 66),
                deltas(&[DECIDED]),
            ]),
            template(),
            None,
            None,
            None,
            Some(seaming(crate::seam::policy::Served {
                every_turns: None,
                at_trunk_tokens: Some(245),
                tail_tokens: 0,
                outputs: log::SeamToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        assert!(seams_in(&log).is_empty(), "{:#?}", seams_in(&log));
        assert_eq!(session.shared.lock().trunk_tokens, Some(244));
    }

    /// A step that calls a tool measures a trunk that only holds if the
    /// step's exchange joins it. Here the call names a tool the session never
    /// declared, the turn settles `failed`, and nothing joins the trunk
    /// (T13): its 500 tokens are not the trunk's, and the budget of 100 does
    /// not fire on them. The first turn measured 10.
    #[test]
    fn a_step_whose_exchange_never_joins_the_trunk_does_not_count_against_the_budget() {
        let mut call = vec![bash("call-1", "touch marker")];
        call.push(Step::Timings(Timings {
            prompt_n: Some(400),
            cache_n: Some(0),
            predicted_n: Some(100),
            ..Timings::default()
        }));
        let session = Session::open_with(
            Canned::new([
                a_reply_with_timings(SCOPED, 4, 0, 6),
                deltas(&[DECIDED]),
                call,
            ]),
            template(),
            None,
            None,
            None,
            Some(seaming(crate::seam::policy::Served {
                every_turns: None,
                at_trunk_tokens: Some(100),
                tail_tokens: 0,
                outputs: log::SeamToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        session.ask("go", None).expect("accepted");
        // The settling and any seam it makes due are pushed under one lock.
        let log = wait_until(&session, "the second turn", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        assert!(log.iter().any(|logged| matches!(
            logged.event,
            Event::TurnSettled {
                turn: 2,
                reason: SettleReason::Failed
            }
        )));
        assert!(seams_in(&log).is_empty(), "{:#?}", seams_in(&log));
        assert_eq!(session.shared.lock().trunk_tokens, Some(10));
    }

    /// A cadence that comes round with working memory empty fires nothing
    /// (a refill would drop every turn and carry nothing) and stays due: the
    /// first turn that leaves an entry fires it, at that turn.
    #[test]
    fn a_due_seam_with_nothing_to_refill_from_waits_for_an_entry() {
        let session = Session::open_with(
            Canned::new([deltas(&["hello"]), deltas(&[SCOPED]), deltas(&[DECIDED])]),
            template(),
            None,
            None,
            None,
            Some(seaming(crate::seam::policy::Served {
                every_turns: Some(1),
                at_trunk_tokens: None,
                tail_tokens: 0,
                outputs: log::SeamToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session.ask("hi", None).expect("accepted");
        let log = wait_until(&session, "the first turn", settled);
        assert!(seams_in(&log).is_empty());
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the seam", |log| !seams_in(log).is_empty());
        assert_eq!(seams_in(&log), vec![(2, log::SeamReason::Cadence)]);
    }

    /// An operator's seam resets the cadence: with a cadence of two, a
    /// declared seam after turn 1 means turn 2 does not fire one.
    #[test]
    fn an_operator_seam_restarts_the_cadence() {
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&["started on the schema"]),
            ]),
            template(),
            None,
            None,
            None,
            Some(seaming(crate::seam::policy::Served {
                every_turns: Some(2),
                at_trunk_tokens: None,
                tail_tokens: 0,
                outputs: log::SeamToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        session.declare_seam(None).expect("admitted");
        session.ask("build it", None).expect("accepted");
        let log = wait_until(&session, "the second turn", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        assert_eq!(seams_in(&log), vec![(1, log::SeamReason::Operator)]);
    }

    // -----------------------------------------------------------------------
    // the operator's seam (#493)
    // -----------------------------------------------------------------------

    /// #493's test: one ask, a declared seam, one ask after it. The seam
    /// refills the trunk from working memory -- the head, the render after
    /// it, and no turn of the old trunk -- the next ask is sent on exactly
    /// that, and the log says so: the seam line carries the render, what was
    /// carried, and the head's digest either side, which the projection's
    /// rebuild of the next request's head checks.
    /// #565: a seam under the regimen's render budget refills from the
    /// budgeted render, and its `seam` line names the budget, the tokens the
    /// render ran to, and how many entries it reduced.
    #[test]
    fn a_seam_under_a_render_budget_records_the_budget_and_what_it_reduced() {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.seams.render_budget = Some(crate::seam::render::Budget {
            tokens: 1,
            over: crate::seam::render::OverBudget::Elide,
        });
        let session = Session::open_with(
            Canned::new([deltas(&[SCOPED]), deltas(&[DECIDED])]),
            template(),
            None,
            None,
            None,
            Some(interview),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        let opened_by = settling_seq(&log, 1);
        session
            .declare_seam(Some(gap(opened_by, GapEnd::Seam)))
            .expect("admitted");
        let lines = whole_log(&session);
        let seam = lines
            .iter()
            .find_map(|line| match &line.event {
                log::Event::Seam {
                    render,
                    render_budget,
                    ..
                } => Some((render.clone(), render_budget.clone())),
                _ => None,
            })
            .expect("a seam line");
        let (render, Some(budget)) = seam.clone() else {
            panic!("the seam names its budget: {seam:?}");
        };
        assert_eq!((budget.tokens, budget.over.as_str()), (1, "elide"));
        assert_eq!(budget.reduced, 3, "the three decisions, elided: {render}");
        assert!(render.ends_with("3 older entries elided\n"), "{render}");
        assert_eq!(budget.rendered, (render.chars().count() as u64).div_ceil(4));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_declared_seam_refills_the_trunk_from_working_memory_and_the_next_ask_runs_on_it() {
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&["started on the schema"]),
            ]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        let opened_by = settling_seq(&log, 1);
        let before = session.trunk();

        session
            .declare_seam(Some(gap(opened_by, GapEnd::Seam)))
            .expect("a seam after a settled turn is admitted");

        let held = session.shared.lock();
        let object = &held.interview.as_ref().expect("interviewing").object;
        let render = crate::seam::render::render(object, None);
        drop(held);
        let refilled = crate::seam::render::refill(&template().messages, &render);
        assert_eq!(session.trunk(), refilled);
        assert!(
            render.contains("decision: for one team"),
            "the render carries working memory: {render}"
        );
        assert!(
            !session
                .trunk()
                .iter()
                .any(|m| m.content.contains("what are we building?") || m.content == SCOPED),
            "a turn of the old trunk was carried: {:#?}",
            session.trunk()
        );

        // The head a request on the old trunk would have carried: the trunk,
        // then an ask, as `ask` builds it.
        let mut on_before = template();
        on_before.messages.clone_from(&before);
        on_before.messages.push(user("an ask"));
        let log = session.events_from(0);
        let seam = log
            .iter()
            .position(|logged| matches!(logged.event, Event::Seamed { .. }))
            .expect("a seam line");
        assert!(
            matches!(&log[seam - 1].event, Event::IdleGap(gap) if gap.ended_by == GapEnd::Seam),
            "the gap the seam ended is logged just before it"
        );
        let log::Event::Seam {
            prefix_hash_before,
            prefix_hash_after,
            ..
        } = line_of(&log[seam]).event
        else {
            panic!("the seam's line is a seam");
        };
        assert_eq!(
            prefix_hash_before,
            crate::client::head::Head::of(&on_before).digest()
        );
        assert_eq!(
            line_of(&log[seam]).event,
            log::Event::Seam {
                at_turn: 1,
                reason: log::SeamReason::Operator,
                prefix_hash_before: prefix_hash_before.clone(),
                prefix_hash_after: prefix_hash_after.clone(),
                frame: crate::seam::render::FRAME_VERSION.to_owned(),
                render: render.clone(),
                carried_entries: 3,
                carried_turns: 0,
                tail_tokens: None,
                carried_tokens: None,
                phase: None,
                tool_outputs: Some(log::SeamToolOutputs::Evict),
                outputs: None,
                carried_outputs: None,
                carried_output_bytes: None,
                placement: Some(log::RenderPlacement::Message),
                render_budget: None,
                fired: None,
                pruned: None,
                warm: None,
            }
        );

        session.ask("build it", None).expect("accepted");
        let log = wait_until(&session, "the second turn to settle", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        reads_whole(&session);
        let sent = session.shared.transport.sent();
        assert_eq!(
            sent.len(),
            3,
            "a turn, its fork, and the turn after the seam"
        );
        let mut expected = refilled.clone();
        expected.push(user("build it"));
        assert_eq!(
            sent[2].messages, expected,
            "the next ask runs on the refill"
        );
        // The seam's "after" is the head the next trunk request carried, as
        // the log itself says.
        let next_head = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Requested {
                    turn: 2,
                    lane: Lane::Trunk,
                    head_sha256,
                    ..
                } => Some(head_sha256.clone()),
                _ => None,
            })
            .expect("turn 2's trunk request");
        assert_eq!(prefix_hash_after, next_head);

        let lines: Vec<log::Line> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        // The seam's record row needs its turn's row, which needs timings
        // this transport does not send: the projection's own test has it.
        let named: Vec<String> = projected
            .unspellable
            .iter()
            .filter(|u| u.kind == "request")
            .map(|u| u.why.clone())
            .collect();
        assert_eq!(
            named,
            Vec::<String>::new(),
            "every head after the seam verifies"
        );
    }

    /// A seam needs something to refill from and a session at rest: before
    /// any turn, or over working memory with no entry, it is refused
    /// `nothing-to-seam`; mid-turn, `in-flight`. Each refusal is logged and
    /// changes nothing.
    #[test]
    fn a_seam_with_nothing_to_refill_from_or_under_a_turn_is_refused() {
        let gate = Gate::new();
        let session = Session::open_with(
            Canned::new([vec![
                Step::Delta("Hel".to_owned()),
                Step::Hold(gate.clone()),
            ]]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[])),
        );
        assert_eq!(
            session.declare_seam(None),
            Err(Rejected::Refused(Refusal::NothingToSeam))
        );
        session.ask("one", None).expect("accepted");
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        assert_eq!(
            session.declare_seam(None),
            Err(Rejected::Refused(Refusal::InFlight))
        );
        gate.open();
        wait_until(&session, "the turn to settle", settled);
        // A settled turn, and a working object with nothing in it: a refill
        // would drop the turn and carry nothing.
        assert_eq!(
            session.declare_seam(None),
            Err(Rejected::Refused(Refusal::NothingToSeam))
        );
        assert_eq!(session.trunk()[1..], [user("one"), assistant("Hel")]);
        assert!(
            !session
                .events_from(0)
                .iter()
                .any(|logged| matches!(logged.event, Event::Seamed { .. }))
        );
        reads_whole(&session);
    }

    /// The definition of done's second line: a gap after a plain chat turn fires none -- and
    /// a session whose regimen warrants nothing fires none after a scoping
    /// ask either.
    #[test]
    fn a_plain_gap_fires_no_fork_and_no_warrant_fires_none() {
        let session = Session::open_with(
            Canned::new([deltas(&[SCOPED]), deltas(&["never sent"])]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping, log::Warrant::Read])),
        );
        session.ask("hello", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        reads_whole(&session);
        assert!(forks(&log).is_empty(), "{log:#?}");
        assert_eq!(session.shared.transport.sent().len(), 1);

        let unwarranted = Session::open(
            Canned::new([deltas(&[SCOPED]), deltas(&["never sent"])]),
            template(),
        );
        unwarranted
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&unwarranted, "the turn to settle", settled);
        assert!(forks(&log).is_empty(), "{log:#?}");
        assert_eq!(unwarranted.shared.transport.sent().len(), 1);
    }

    /// A fork's call that the server refuses is a typed outcome, `failed`,
    /// never retried; nothing is patched, and the next ask is welcome.
    #[test]
    fn a_forks_500_settles_it_failed_and_is_never_retried() {
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                vec![Step::Reject(
                    500,
                    "{\"error\":{\"message\":\"busy\"}}".to_owned(),
                )],
                deltas(&["the next turn"]),
            ]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        reads_whole(&session);
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Failed]);
        assert!(patches(&log).is_empty());
        assert_eq!(session.shared.transport.sent().len(), 2, "no retry");
        session.ask("go on", None).expect("the next ask is welcome");
        let log = wait_until(&session, "turn 2 to settle", |log| {
            settled(log)
                && log
                    .iter()
                    .any(|logged| matches!(logged.event, Event::TurnSettled { turn: 2, .. }))
        });
        assert_eq!(forks(&log).len(), 1, "a plain turn 2 forks nothing");
    }

    /// An ask while the fork is in flight is refused `in-flight` and waits
    /// (the surface holds it); a cancel reaches the fork and settles it
    /// `cancelled`; an `end` does too, then the session ends.
    #[test]
    fn a_cancel_or_end_during_the_fork_settles_it_cancelled() {
        for ending in [false, true] {
            let gate = Gate::new();
            let session = Session::open_with(
                Canned::new([
                    deltas(&[SCOPED]),
                    vec![
                        Step::Delta("DECISION: ha".to_owned()),
                        Step::Hold(gate.clone()),
                    ],
                ]),
                template(),
                None,
                None,
                None,
                Some(interviewing(&[log::Warrant::Scoping])),
            );
            session
                .ask_marked("what are we building?", None, true)
                .expect("accepted");
            assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
            assert_eq!(session.settlement(), Settlement::Capture);
            assert_eq!(
                session.ask("next", None),
                Err(Rejected::Refused(Refusal::InFlight))
            );
            if ending {
                assert_eq!(session.end(None), Ok(()));
            } else {
                assert_eq!(session.cancel(1, None), Ok(()));
            }
            let log = wait_until(&session, "the fork to settle", |log| {
                !fork_outcomes(log).is_empty()
                    && matches!(
                        log.last(),
                        Some(Logged {
                            event: Event::Settled { .. },
                            ..
                        })
                    )
                    && session.settlement() != Settlement::Capture
            });
            reads_whole(&session);
            assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Cancelled]);
            assert!(patches(&log).is_empty());
            let want = if ending {
                Settlement::Ended
            } else {
                Settlement::Awaiting
            };
            assert_eq!(session.settlement(), want, "{log:#?}");
        }
    }

    /// Rule (a): a turn whose call ran and read a document, by the router's
    /// table, warrants a fork asking the router's ask for that class,
    /// quoting the command back.
    #[test]
    fn a_turn_that_read_a_document_fires_a_read_fork() {
        let tree = scratch("read-fork");
        std::fs::write(tree.join("notes.md"), "a note\n").expect("a note");
        let session = Session::open_with(
            Canned::new([
                vec![bash("call-1", "cat notes.md")],
                deltas(&["It says a note."]),
                deltas(&["LEARNED: the note says a note\n"]),
            ]),
            looping(),
            None,
            Some(tools(
                Confinement::Unconfined,
                &tree,
                &["cat"],
                None,
                Decider::Decline,
            )),
            None,
            Some(interviewing(&[log::Warrant::Read])),
        );
        session.ask("read the notes", None).expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        reads_whole(&session);
        let question = router::Ask {
            kind: AskKind::Finding,
            intent: None,
        }
        .render(&Facts {
            last_command: Some("cat notes.md".to_owned()),
            ..Facts::default()
        });
        let found = forks(&log);
        assert_eq!(found.len(), 1, "{log:#?}");
        assert_eq!(found[0].3, log::Warrant::Read);
        assert_eq!(found[0].4, question);
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Value]);
        assert_eq!(patches(&log).len(), 1);
        tidy(&[&tree]);
    }

    /// On the standard surface (#557) a `read` of a document fires the
    /// read fork as `cat` does: the router classes it by its `path`, and the
    /// ask, with no command to quote, drops its `last_command` line.
    #[test]
    fn a_standard_read_of_a_document_fires_a_read_fork() {
        let tree = scratch("standard-read-fork");
        std::fs::write(tree.join("notes.md"), "a note\n").expect("a note");
        let mut shape = looping();
        shape.tools = tool_loop::ToolSurface::Standard.tools();
        let mut tools = tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline);
        tools.surface = tool_loop::ToolSurface::Standard;
        tools.read_tool = tool_loop::ToolSurface::Standard.read_tool();
        let session = Session::open_with(
            Canned::new([
                vec![Step::call(0, "call-1", "read", r#"{"path":"notes.md"}"#)],
                deltas(&["It says a note."]),
                deltas(&["LEARNED: the note says a note\n"]),
            ]),
            shape,
            None,
            Some(tools),
            None,
            Some(interviewing(&[log::Warrant::Read])),
        );
        session.ask("read the notes", None).expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        reads_whole(&session);
        let question = router::Ask {
            kind: AskKind::Finding,
            intent: None,
        }
        .render(&Facts::default());
        let found = forks(&log);
        assert_eq!(found.len(), 1, "{log:#?}");
        assert_eq!(found[0].3, log::Warrant::Read);
        assert_eq!(found[0].4, question);
        assert!(!question.contains("You last ran"));
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Value]);
        tidy(&[&tree]);
    }

    #[test]
    fn the_warrant_key_is_a_list_of_the_logs_rules() {
        let read = |text: &str| interview_warrant(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read("arm = \"x\"\n"), Ok(Vec::new()));
        assert_eq!(
            read("interview_warrant = [\"scoping\", \"read\"]\n"),
            Ok(vec![log::Warrant::Scoping, log::Warrant::Read])
        );
        assert!(read("interview_warrant = \"scoping\"\n").is_err());
        assert!(read("interview_warrant = [\"always\"]\n").is_err());
    }
}
