//! An interactive session: a person asks, the trunk answers streamed, and
//! the session says at every moment what it is doing (#117, R2).
//!
//! [`super::run`] is the scripted gym -- three turns from a TOML file, the
//! trunk rebuilt from the rendered object on every one. This is the other
//! thing #117 names: an ask that comes from a person, whenever they send it,
//! answered on a trunk that is APPENDED until a meditation and never rebuilt. The
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
//! **A meditation** (#493): [`Session::declare_meditation`] refills the trunk from
//! working memory and appends from there; the meditation's audit and pre-warm are
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
        /// Declare a meditation (R6).
        DeclareMeditation => "declare-meditation",
        /// End the session.
        End => "end",
        /// Open a tangent (#22).
        OpenTangent => "open-tangent",
        /// Close the open tangent (#22).
        CloseTangent => "close-tangent",
        /// Move the running command to the background (#614).
        Background => "background",
        /// Rule on the model's pending phase proposal (#124).
        RatifyPhase => "ratify-phase",
        /// Correct a working-memory entry (#150).
        EditEntry => "edit-entry",
        /// Flag a working-memory entry (#150).
        FlagEntry => "flag-entry",
        /// Refill a held meditation anyway (#150's gate).
        RefillAnyway => "refill-anyway",
        /// Choose the starting phase, before the first ask (#788).
        StartPhase => "start-phase",
        /// Propose a phase move to the model (#788).
        ProposePhase => "propose-phase",
        /// Move the phase alone (#788), under `operator_unilateral`.
        MovePhase => "move-phase",
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
        /// A meditation has nothing to refill from: no turn has settled, or working
        /// memory holds no entry (#493).
        NothingToMeditate => "nothing-to-meditate",
        /// A meditation named a phase to move to, and the session has no phase
        /// graph (#563).
        NoPhaseGraph => "no-phase-graph",
        /// A meditation named a phase the graph does not declare.
        NotAPhase => "not-a-phase",
        /// A meditation named the phase the session is already in.
        AlreadyInPhase => "already-in-phase",
        /// A meditation named a move the graph does not allow from here.
        NoPhaseEdge => "no-phase-edge",
        /// A cancel named a turn older than the latest one: it arrived after
        /// that turn settled and must not stop the next (the admission
        /// counter ruled on #117).
        Stale => "stale",
        /// A meditation, or a second tangent, while a tangent is open (#22).
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
        /// A phase ruling named no pending proposal (#124): none is pending,
        /// or the call it names is not the one that is.
        NoProposal => "no-proposal",
        /// An edit or flag naming no entry working memory holds (#150).
        UnknownEntry => "unknown-entry",
        /// An edit or flag naming an entry no longer live (#150).
        NotLive => "not-live",
        /// An edit with no content, or a flag with no note (#150).
        Empty => "empty",
        /// An ask while a meditation is held for the operator (#150's gate): the
        /// trunk was not refilled, and an ask would answer on it.
        MeditationHeld => "meditation-held",
        /// A refill-anyway with no meditation held (#150's gate).
        NothingHeld => "nothing-held",
        /// An edit whose text restates the entry it corrects, or another
        /// live entry (v8, #729): working memory has it already.
        Restates => "restates",
        /// The operator's move alone where `operator_unilateral` is off
        /// (#788): propose it to the model instead.
        NotUnilateral => "not-unilateral",
        /// A starting phase chosen after the first ask (#788).
        AlreadyAsked => "already-asked",
        /// The operator's proposal while the model's waits on a ruling
        /// (#788): rule on the model's first.
        ProposalPending => "proposal-pending",
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
        /// A meditation.
        Meditation => "meditation",
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
    /// From the end of `notice` to the first keystroke or meditation click.
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
        /// A meditation's audit (#504): a fork's call, before the refill.
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
        /// Whether a turn's notes ride in its ask's user message (#609):
        /// always, from this build on.
        notes_in_the_ask: bool,
        /// The build serving the session (#757): its commit and whether its
        /// tracked files differed; `None` where serve did not say.
        build: Option<(String, bool)>,
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
        /// The opening phase's document (#787), under a phase library.
        phase_document: Option<log::PhaseDocument>,
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
        /// How many forks were in flight beside a trunk request, when the
        /// session's forks run during the turn (#782); `None` otherwise.
        beside: Option<u32>,
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
        /// The pinned dogma template it asked instead (#783), named on its
        /// line as its `ask`.
        pinned: Option<crate::dogma::Template>,
        /// Whether it is sent to the trunk's own server with a prompt that
        /// is not the trunk's prefix, and so may displace the trunk's cached
        /// prefix there (#406).
        displaces: bool,
        /// The `seq` of the step's last `tool_call` it was cut at, when it
        /// was cut during the turn (#782).
        cut: Option<u64>,
        /// When it ran (#782).
        timing: Timing,
        /// The view the regimen declared that a cut during the turn
        /// overrode: such a fork sees the whole trunk at the cut (#782).
        view_overridden: Option<ForkView>,
        /// A cut fork's reservation of the pool, and whether its prefix
        /// was counted as shared with the trunk's (#797).
        reserved: Option<(u64, bool)>,
    },
    /// A meditation's audit (#504): a fork off the warm trunk on the `audit` lane,
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
        /// The meditation whose refilled trunk it is cut from, by `seq`: a meditation
        /// declared again with no turn since the last, as the operator does
        /// to reconcile changes made after that refill.
        after_meditation: Option<u64>,
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
        /// What its answer wrote after its last field, past a blank line:
        /// recorded as no entry (levers run A).
        trailer: Option<String>,
        /// What a fork refused for the pool did not fit beside (#782).
        pool: Option<log::PoolRoom>,
        /// The calls the fork made that ran nothing (#806).
        not_run: Option<u64>,
    },
    /// A meditation fired, and the trunk was refilled from working memory (#493):
    /// the head with the working object rendered after it, and no turn of
    /// the old trunk.
    Meditated {
        /// The latest turn, settled, which it follows.
        at_turn: u32,
        /// What fired it: the operator, or the regimen's budget or cadence.
        reason: crate::meditation::Reason,
        /// The `head_sha256` a trunk request on the trunk before would carry.
        prefix_hash_before: String,
        /// The same, after.
        prefix_hash_after: String,
        /// The working object, rendered.
        render: String,
        /// How many entries the render carried.
        carried_entries: u64,
        /// The compaction depth the meditation ran at (#552): estimated tokens of
        /// recent whole turns it could keep; 0, the total refill.
        tail_tokens: u64,
        /// How many whole turns of the old trunk it kept.
        carried_turns: u64,
        /// Their estimated tokens.
        carried_tokens: u64,
        /// The phases it moved between (#563), when the operator named a
        /// move the graph allowed.
        phase: Option<log::PhaseMove>,
        /// The document of the phase it left the session in (#787), whose
        /// ground rules follow the head's system prompt on the refill.
        phase_document: Option<log::PhaseDocument>,
        /// What it carried of the tool outputs it compacted away (#553).
        tool_outputs: log::MeditationToolOutputs,
        /// The section after the render carrying them, when it carried any,
        /// and how many it carried.
        outputs: Option<(String, u64)>,
        /// The list of each background command still running (#614), as
        /// sent after the summary.
        background: Option<String>,
        /// The render's budget and what it did (#565), when declared.
        render_budget: Option<log::RenderBudget>,
        /// The prompt that fired it and the window, on a `window` meditation (#617).
        fired: Option<log::MeditationFired>,
        /// The calls whose pruned results it replaced (#612), when any.
        pruned: Option<Vec<String>>,
        /// The operator's edits and flags it carried (#150), when any.
        operator_changes: Option<Vec<log::OperatorChange>>,
        /// Of those, the entries no answer addressed (#150): carried on.
        unaddressed: Option<Vec<String>>,
        /// The newest turn's estimated tokens when the tail was asked for
        /// and kept no turn (#693).
        newest_turn_tokens: Option<u64>,
        /// The operator refilled it over the changes it was held for
        /// (#150's gate): `Some(true)`, or `None`.
        refilled_anyway: Option<bool>,
        /// The pre-warm's timings (#504), when the refilled trunk was sent
        /// once and the call finished.
        warm: Option<Timings>,
    },
    /// Forks' patches delivered at the tail of a trunk request, after its
    /// ask (the fork delivery lever): the note stays on the trunk.
    /// A tool result the model pruned (#612): logged when `prune_output`
    /// answered; a later meditation carries `text` in its place.
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
    /// The operator flagged an entry (#150): a note on it, the entry
    /// unchanged, kept until a meditation addresses it.
    /// A meditation held for the operator (#150's gate): an operator change no
    /// answer addressed under `reconcile_gate = "refuse"`, after the
    /// reconciling audit and its one re-ask; the trunk is not refilled.
    MeditationHeld {
        /// The latest turn, settled, which the meditation follows.
        at_turn: u32,
        /// What fired it.
        reason: crate::meditation::Reason,
        /// The entries whose changes stay unaddressed.
        unaddressed: Vec<String>,
    },
    Flagged {
        /// The latest turn.
        turn: u32,
        /// The entry.
        entry: String,
        /// The operator's note.
        note: String,
    },
    /// A model's patch refused (#150): it targeted an entry the operator
    /// wrote. The fold went on.
    PatchRefused {
        /// The fork whose answer made it, when one did.
        fork: Option<u64>,
        /// The lane that made it, when no fork did.
        lane: Option<String>,
        /// What it would have done.
        op: log::PatchOp,
        /// The entry it targeted.
        entry: String,
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
        /// Why the entry it voided or retired went (#812), when the fork
        /// said so: a `SUPERSEDE: <id> — <reason>` pair's reason.
        reason: Option<String>,
    },
    /// A self-capture call and what it came to (#609).
    /// The operator's ruling on the model's pending phase proposal (#124).
    PhaseRuled {
        /// The proposal's call; none on the operator's move alone (#788).
        call: Option<String>,
        /// Who moved (#788): the operator alone, or the model accepting the
        /// operator's proposal; `None`, the operator ruling on the model's.
        by: Option<Key>,
        /// What was ruled.
        choice: PhaseChoice,
        /// The phase the session was in.
        from: Option<String>,
        /// The phase proposed.
        to: String,
        /// The document of the phase the session is in after it (#787), on
        /// a `meditation` or `continue` under a phase library.
        phase_document: Option<log::PhaseDocument>,
    },
    /// A phase move proposed (#788), by the model through its call `call`
    /// or by the operator, with the evidence the other key rules on.
    PhaseProposed {
        /// Which key proposed it.
        by: Key,
        /// The model's call; none on the operator's proposal.
        call: Option<String>,
        /// The phase the session is in.
        from: Option<String>,
        /// The phase proposed.
        to: String,
        /// Why, in the model's words.
        reason: Option<String>,
        /// Whether the operator's proposal moves through a meditation.
        meditate: Option<bool>,
        /// The evidence.
        evidence: log::PhaseEvidence,
    },
    /// The model's answer to the operator's proposal (#788), through its
    /// call `call`.
    PhaseAnswered {
        /// The model's call.
        call: String,
        /// Whether it accepted.
        accepted: bool,
        /// Why, in its words.
        reason: Option<String>,
    },
    /// The starting phase the operator chose before the first ask (#788).
    PhaseStarted {
        /// The phase.
        phase: String,
        /// Its document, under a phase library.
        phase_document: Option<log::PhaseDocument>,
    },
    /// A phase's ground rules, as a note on turn `turn`'s ask (#787): the
    /// session entered the phase with no meditation to carry them.
    PhaseNoted {
        /// The turn whose request carried it.
        turn: u32,
        /// The note as sent.
        text: String,
    },
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
        /// A phase proposal's move (#124): the phase the session was in.
        from: Option<String>,
        /// The phase a `propose_phase_transition` call proposed.
        to: Option<String>,
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

/// The fork delivery lever's state the regimen declares: `meditation` (the
/// default, today's behaviour), `advisory` or `imperative`.
///
/// # Errors
///
/// A value that is none of the three.
pub fn fork_delivery(regimen: &Regimen) -> Result<log::ForkDelivery, String> {
    match regimen.get(FORK_DELIVERY) {
        None => Ok(log::ForkDelivery::Meditation),
        Some(crate::formats::regimen::Value::String(state)) => log::ForkDelivery::from_tag(state)
            .ok_or_else(|| {
                format!(
                    "`{FORK_DELIVERY}` is \"{state}\": it takes \"meditation\", \"advisory\" or \
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
                // A meditation's audit is the meditation's, never an interview's (#504).
                .filter(|rule| *rule != log::Warrant::Meditation)
                .ok_or_else(|| {
                    format!(
                        "`{INTERVIEW_WARRANT}` names `{word}`, which is not a rule: {}",
                        log::Warrant::ALL
                            .iter()
                            .filter(|rule| **rule != log::Warrant::Meditation)
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
        // The build serving it (#757): its commit and whether it was dirty.
        Option<(String, bool)>,
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
    /// When derived meditations fire: the regimen's cadence and budget.
    pub meditations: crate::meditation::policy::Served,
    /// How a fork's patches reach the trunk: at the meditation (`meditation`, the
    /// default), or as a note at the tail of the next trunk request.
    pub delivery: log::ForkDelivery,
    /// The regimen's phase graph (#563), read as the scripted drive reads
    /// it; empty when it declares none.
    pub phases: crate::meditation::phase::PhaseGraph,
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
    /// Whether each settled tool call's touches are patched into working
    /// memory by the mechanical lane (#784); off, the default. On, it opens
    /// working memory by itself, warrant or none: a refill holding only
    /// `files:` entries beats a session dead of its window -- the model can
    /// re-read what they list, and archive recall covers the rest.
    pub mechanical_capture: bool,
    /// Self-capture (#609): the contract's tools offered from the first
    /// request, and the cadence of silent turns its reminder fires after;
    /// `None` when off.
    pub self_capture: Option<crate::capture::tools::Cadence>,
    /// The contract set self-capture offers (#609): the regimen's
    /// `self_capture_contract`, else `c1`.
    pub capture_contract: &'static crate::dogma::capture::CaptureSet,
    /// How a fork answers (#610): in fields, the default, or through the
    /// self-capture tools.
    pub capture: crate::dogma::asks::Modality,
    /// The output cap a fork's call is clamped from (#406): the regimen's
    /// `fork_tail_tokens`, or `None`, the session's.
    pub fork_tail: Option<u32>,
    /// Whether the model is offered `prune_output`, and when its prunes
    /// are applied (#612); `None`, the default, offers nothing.
    pub prune: Option<super::prune::PruneMeditation>,
    /// When a fork a step warrants runs (#782): in the gap, the default, or
    /// during the turn.
    pub timing: Timing,
    /// The forks in flight at once during the turn (#782): the trunk's
    /// server's slots less the trunk's own, or an offboard seat's; 0 under
    /// the gap. [`fork_slots`] reads it at start.
    pub fork_slots: u32,
    /// Whether the trunk's server shares a prefix's pages between
    /// concurrent requests (#797): its entry's `served_prefix_shared`.
    /// [`crate::drive::registry::served_prefix_shared`] reads it at start.
    pub prefix_pages: PrefixPages,
    /// The warrants that look at what a step produced, and their numbers
    /// (#783).
    pub step: crate::drive::warrants::StepWarrants,
    /// Whether the operator may move the phase alone (#788), the
    /// regimen's `operator_unilateral`: on unless it says otherwise.
    pub operator_unilateral: bool,
}

/// What a concurrent request on the trunk's prefix costs the trunk's
/// server's pool (#797).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrefixPages {
    /// Its own copy: every concurrent prompt counts in full (#782), the
    /// default until a measurement says otherwise.
    #[default]
    Counted,
    /// The trunk's pages, shared: a fork cut from the trunk reserves only
    /// its tokens beyond the prefix the trunk holds.
    Shared,
}

impl PrefixPages {
    /// From the registry's `served_prefix_shared`.
    #[must_use]
    pub fn of(shared: bool) -> Self {
        if shared { Self::Shared } else { Self::Counted }
    }
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

/// When a fork a step warrants runs (#782).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Timing {
    /// In the capture gap after the turn settles: today's, the default.
    #[default]
    Gap,
    /// Cut from the trunk as it stood after the step's tool results and
    /// sent at once, in a spare slot, while the trunk goes on.
    DuringTurn,
}

impl Timing {
    /// The regimen's word for it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Gap => "gap",
            Self::DuringTurn => "during_turn",
        }
    }
}

/// The regimen key for the interview timing (#782).
pub const INTERVIEW_TIMING: &str = "interview_timing";

/// The regimen key for the shown output, in characters, an `evidence` fork
/// needs (#783).
pub const EVIDENCE_MIN_CHARS: &str = "evidence_min_chars";
/// The regimen key for the `evidence` forks one step fires at most (#783).
pub const EVIDENCE_PER_STEP: &str = "evidence_per_step";
/// The regimen key for the steps the surprise path waits after the trio
/// (#783).
pub const TROUBLE_COOLDOWN_STEPS: &str = "trouble_cooldown_steps";
/// The regimen key for the steps between `open` forks (#783).
pub const OPEN_EVERY_STEPS: &str = "open_every_steps";

/// The step warrants among `rules`, with the numbers the regimen declares,
/// each the predecessor's default when it declares none (#783).
///
/// # Errors
///
/// A number that is not a positive whole number, naming its key.
pub fn step_warrants(
    regimen: &Regimen,
    rules: &[log::Warrant],
) -> Result<crate::drive::warrants::StepWarrants, String> {
    let defaults = crate::drive::warrants::StepWarrants::DEFAULTS;
    let count = |key: &str, default: u64| -> Result<u64, String> {
        match regimen.get(key) {
            None => Ok(default),
            Some(crate::formats::regimen::Value::Integer(n)) if *n > 0 => {
                u64::try_from(*n).map_err(|_| format!("`{key}` is too large"))
            }
            Some(_) => Err(format!("`{key}` is not a positive whole number")),
        }
    };
    let small = |key: &str, default: u32| -> Result<u32, String> {
        count(key, u64::from(default))
            .and_then(|n| u32::try_from(n).map_err(|_| format!("`{key}` is too large")))
    };
    Ok(crate::drive::warrants::StepWarrants {
        evidence: rules.contains(&log::Warrant::Evidence),
        trouble: rules.contains(&log::Warrant::Trouble),
        open: rules.contains(&log::Warrant::Open),
        evidence_min_chars: count(EVIDENCE_MIN_CHARS, defaults.evidence_min_chars)?,
        evidence_per_step: usize::try_from(small(
            EVIDENCE_PER_STEP,
            u32::try_from(defaults.evidence_per_step).unwrap_or(u32::MAX),
        )?)
        .unwrap_or(usize::MAX),
        trouble_cooldown_steps: small(TROUBLE_COOLDOWN_STEPS, defaults.trouble_cooldown_steps)?,
        open_every_steps: small(OPEN_EVERY_STEPS, defaults.open_every_steps)?,
    })
}

/// The forks a session runs at once during the turn (#782), from the
/// registry's `serving_slots`: the trunk's server's slots less the trunk's
/// own, or, when forks run on an offboard seat (`seat` is `Some`), the
/// seat's -- which holds no trunk, so one is enough. 0 under the gap.
///
/// # Errors
///
/// During the turn: a server that declares no `serving_slots`, or a trunk's
/// server with fewer than 2, which leaves no slot for a fork beside the
/// trunk.
pub fn fork_slots(
    timing: Timing,
    trunk_slots: Option<u32>,
    seat: Option<Option<u32>>,
) -> Result<u32, String> {
    if timing == Timing::Gap {
        return Ok(0);
    }
    match seat {
        Some(Some(slots)) if slots >= 1 => Ok(slots),
        Some(_) => Err(format!(
            "`{INTERVIEW_TIMING} = \"during_turn\"` needs the offboard seat's `serving_slots` \
             in the registry, and it declares none"
        )),
        None => match trunk_slots {
            Some(slots) if slots >= 2 => Ok(slots - 1),
            Some(slots) => Err(format!(
                "`{INTERVIEW_TIMING} = \"during_turn\"` runs a fork beside the trunk, and the \
                 trunk's server declares {slots} `serving_slots`: it needs 2 or more"
            )),
            None => Err(format!(
                "`{INTERVIEW_TIMING} = \"during_turn\"` needs the trunk's server's \
                 `serving_slots` in the registry, and it declares none"
            )),
        },
    }
}

/// The timing `regimen` declares and the forks it runs at once (#782), its
/// trunk on substrate `trunk` and any offboard seat the regimen names, both
/// read from `registry`.
///
/// # Errors
///
/// A timing word it does not know; `during_turn` with no per-step source --
/// the `read` warrant at the `per_class` or `per_call` cadence -- which
/// would run nothing during the turn; or [`fork_slots`]' refusals.
pub fn timing_and_slots(
    regimen: &Regimen,
    (registry, trunk): (&str, &str),
) -> Result<(Timing, u32), String> {
    let timing = interview_timing(regimen)?;
    if timing == Timing::DuringTurn {
        let rules = interview_warrant(regimen).unwrap_or_default();
        let per_call = rules.contains(&log::Warrant::Read)
            && matches!(
                interview_cadence(regimen),
                Cadence::PerClass | Cadence::PerCall
            );
        // Or a warrant that looks at what a step produced (#783).
        let content = step_warrants(regimen, &rules).is_ok_and(|warrants| warrants.any());
        if !per_call && !content {
            return Err(format!(
                "`{INTERVIEW_TIMING} = \"during_turn\"` cuts the forks a step warrants, and \
                 nothing here warrants one at a step: it needs the `read` warrant and \
                 `{INTERVIEW_CADENCE} = \"per_class\"` or `\"per_call\"`, or a step warrant: \
                 `evidence`, `trouble` or `open`"
            ));
        }
    }
    let seat = match crate::drive::regimen::Seat::of(regimen) {
        crate::drive::regimen::Seat::Warm => None,
        crate::drive::regimen::Seat::Offboard(id) => {
            Some(crate::drive::registry::serving_slots(registry, &id))
        }
    };
    let slots = fork_slots(
        timing,
        crate::drive::registry::serving_slots(registry, trunk),
        seat,
    )?;
    Ok((timing, slots))
}

/// The interview timing the regimen declares: `gap` when it declares none.
///
/// # Errors
///
/// A value that is not one of the two words, naming it: a lever silently
/// read as its default would run a session the regimen did not ask for.
pub fn interview_timing(regimen: &Regimen) -> Result<Timing, String> {
    match regimen.get(INTERVIEW_TIMING) {
        None => Ok(Timing::Gap),
        Some(crate::formats::regimen::Value::String(word)) => [Timing::Gap, Timing::DuringTurn]
            .into_iter()
            .find(|timing| timing.word() == word)
            .ok_or_else(|| {
                format!("`{INTERVIEW_TIMING} = \"{word}\"` is not `gap` or `during_turn`")
            }),
        Some(_) => Err(format!(
            "`{INTERVIEW_TIMING}` is not a string: `gap` or `during_turn`"
        )),
    }
}

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

/// The regimen key for the mechanical lane in the live loop (#784).
pub const MECHANICAL_CAPTURE: &str = "mechanical_capture";

/// Whether the regimen turns mechanical capture on, leniently: `true` or
/// `"on"`; anything else is off.
#[must_use]
pub fn mechanical_capture(regimen: &Regimen) -> bool {
    match regimen.get(MECHANICAL_CAPTURE) {
        Some(crate::formats::regimen::Value::Boolean(on)) => *on,
        Some(crate::formats::regimen::Value::String(word)) => word == "on",
        _ => false,
    }
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
    if let Some(interview) = interview.filter(|interview| interview.self_capture.is_some()) {
        let contract = crate::capture::tools::contract_of(interview.capture_contract)
            .expect("the pinned contract reads: `dogma::capture`'s tests hold it");
        shape
            .tools
            .extend(crate::capture::tools::definitions(&contract));
    }
    // The phase tool, whenever phases are active (#788), self-capture or not.
    declare_phase_tool(shape, interview);
}

/// The regimen key for whether the operator may move the phase alone
/// (#788).
pub const OPERATOR_UNILATERAL: &str = "operator_unilateral";

/// Whether the regimen lets the operator move the phase alone (#788): true
/// unless it says `operator_unilateral = false`.
///
/// # Errors
///
/// A value that is not true or false.
pub fn operator_unilateral(regimen: &Regimen) -> Result<bool, String> {
    match regimen.get(OPERATOR_UNILATERAL) {
        None => Ok(true),
        Some(crate::formats::regimen::Value::Boolean(on)) => Ok(*on),
        Some(_) => Err(format!("`{OPERATOR_UNILATERAL}` is not true or false")),
    }
}

/// The regimen key that turns self-capture on (#609).
pub const SELF_CAPTURE: &str = "self_capture";

/// The regimen key for the self-capture reminder's cadence (#609).
pub const SELF_CAPTURE_CADENCE: &str = "self_capture_cadence";

/// The regimen key for the self-capture contract set (#609's c2).
pub const SELF_CAPTURE_CONTRACT: &str = "self_capture_contract";

/// The self-capture contract set the regimen names, leniently: a set by
/// its name, or else [`crate::dogma::capture::DEFAULT`], `c1`.
#[must_use]
pub fn self_capture_contract(regimen: &Regimen) -> &'static crate::dogma::capture::CaptureSet {
    match regimen.get(SELF_CAPTURE_CONTRACT) {
        Some(crate::formats::regimen::Value::String(name)) => {
            crate::dogma::capture::set(name).unwrap_or(crate::dogma::capture::DEFAULT)
        }
        _ => crate::dogma::capture::DEFAULT,
    }
}

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

/// A tool result the model pruned (#612), until a meditation replaces it and
/// after, so a later meditation still carries its line.
#[derive(Debug, Clone)]
struct Prune {
    /// The call whose result it is.
    call: String,
    /// What the trunk carried as that result.
    shown: String,
    /// Its reference line, which a meditation carries in its place.
    text: String,
    /// Whether a meditation has replaced it yet.
    applied: bool,
}

impl Prune {
    /// Whether `message` is the result it prunes, as the trunk carried it
    /// or as a meditation already replaced it.
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
    let turns = crate::meditation::render::last_turns(&trunk[head..], n as usize);
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

/// The priming paragraph a session under `regimen` opens with (#598): its
/// fork ask set's, when the regimen warrants forks and the set carries one.
/// It joins the system prompt once, at the session's start, and is never
/// changed after.
#[must_use]
pub fn priming(regimen: &Regimen) -> Option<&'static str> {
    let forks = interview_warrant(regimen).is_ok_and(|rules| !rules.is_empty());
    forks.then(|| fork_asks(regimen).priming()).flatten()
}

/// `head`, the system prompt, with `priming` after it and a blank line
/// (#598); `head` as it is with none.
#[must_use]
pub fn primed(head: &str, priming: Option<&str>) -> String {
    match priming {
        Some(priming) => format!("{}\n\n{}", head.trim_end(), priming.trim()),
        None => head.to_owned(),
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
    /// The pinned dogma template it asks instead (#783), for a warrant
    /// that looks at what a step produced.
    pinned: Option<crate::dogma::Template>,
    question: String,
    /// `turn_end`, `call:<class>:<id>`, or the step warrant and what fired
    /// it (#783).
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
    // During the turn (#782), the calls' forks were cut at their steps.
    let calls = if interview.rules.contains(&log::Warrant::Read) && interview.timing == Timing::Gap
    {
        routed(log, turn)
    } else {
        Vec::new()
    };
    firings_over(interview, log, (turn, answer), &calls, true)
}

/// The forks a step of `turn` warrants during the turn (#782): the per-call
/// forks of `per_class` and `per_call` over the step's calls `ids`, each
/// asking about what the step's text `said` it was about to do. None under
/// a cadence that forks on no call.
fn step_firings(
    interview: &Interview,
    log: &[Logged],
    (turn, said): (u32, &str),
    ids: &[String],
) -> Vec<Firing> {
    if !interview.rules.contains(&log::Warrant::Read)
        || !matches!(interview.cadence, Cadence::PerClass | Cadence::PerCall)
    {
        return Vec::new();
    }
    let calls: Vec<Routed> = routed(log, turn)
        .into_iter()
        .filter(|call| ids.contains(&call.id))
        .collect();
    firings_over(interview, log, (turn, said), &calls, false)
}

/// [`firings`] over `calls`, the judgment ask on a marked turn only when
/// `judge`.
fn firings_over(
    interview: &Interview,
    log: &[Logged],
    (turn, answer): (u32, &str),
    calls: &[Routed],
    judge: bool,
) -> Vec<Firing> {
    let rules = &interview.rules;
    let intent = router::stated_intent(answer);
    // The working record, for an ask in a set that shows it (#595).
    let record = crate::meditation::render::entries(&interview.object);
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
            pinned: None,
            question,
            trigger: TURN_END.to_owned(),
        }
    };
    let marked = judge
        && rules.contains(&log::Warrant::Scoping)
        && log.iter().any(|logged| {
            matches!(&logged.event, Event::Asked { turn: asked, scoping: true, .. } if *asked == turn)
        });
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
            pinned: None,
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
            return if judge && rules.contains(&log::Warrant::Scoping) {
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

/// The fields an ask asks for, read off its own instruction (#730): the
/// fields route's `with the fields X, Y and Z` (or `with the field X`),
/// upper case, or the tools route's `under the field x, y or z`, lower case
/// -- the last such, after the intent it quotes -- lower-cased to the
/// contract's spelling. None when it names none.
fn asked_fields(question: &str) -> Vec<String> {
    let marked = [
        ("with the fields ", true),
        ("with the field ", true),
        ("under the field ", false),
    ]
    .iter()
    .filter_map(|(marker, upper)| question.rfind(marker).map(|at| (at + marker.len(), *upper)))
    .max_by_key(|(at, _)| *at);
    let Some((at, upper)) = marked else {
        return Vec::new();
    };
    let named = &question[at..];
    let end = named.find([';', '.', '\n']).unwrap_or(named.len());
    named[..end]
        .split([',', ' '])
        .filter(|word| !word.is_empty() && *word != "and" && *word != "or")
        .take_while(|word| {
            word.chars().all(|c| {
                c == '_'
                    || if upper {
                        c.is_ascii_uppercase()
                    } else {
                        c.is_ascii_lowercase()
                    }
            })
        })
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
                lane: None,
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
    /// first at open, then where each meditation's move took it.
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
    /// A phase's ground rules due as a note at the next ask (#787): the
    /// session opened in the phase, or moved to it with no meditation.
    phase_note_due: Option<String>,
    /// Patches waiting to be delivered at the next trunk request, under a
    /// mid-turn fork delivery: each op and the entry text its line names.
    undelivered: Vec<(log::PatchOp, String, String)>,
    /// What meditations dropped from the trunk, for a recall to find (#566).
    archive: super::archive::Archive,
    /// The text each recovered call came from, by its id, until its line is
    /// logged (#560).
    recovered: BTreeMap<String, String>,
    /// The mechanical lane, once a call under `mechanical_capture` has
    /// settled (#784): the shell state it reads each next call against.
    mechanical: Option<crate::capture::mechanical::Lane>,
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
    /// `turns` at the latest meditation, or 0: what a cadence counts from.
    turns_at_meditation: u32,
    /// The trunk's tokens as the latest trunk call measured them, cleared
    /// by a meditation: what a budget reads.
    trunk_tokens: Option<u64>,
    /// The estimated tokens of the latest request's messages, as it was
    /// sized (#588).
    sent_estimate: Option<u64>,
    /// The latest request's prompt as sized, its output cap, and the window
    /// it was sized against (#616): what tells an overflow from its size.
    last_sized: Option<(u64, u32, Option<u64>)>,
    /// The turn that last meditated and went again after an overflow (#617):
    /// once per turn.
    overflow_retried: Option<u32>,
    /// A meditation queued for its audit or pre-warm (#504), until [`meditation_work`]
    /// takes it.
    meditation_pending: Option<PendingMeditation>,
    /// What fired the meditation [`meditation_work`] has taken and not yet
    /// logged the outcome of (#756): what `GET /state` calls `capturing`.
    meditating: Option<crate::meditation::Reason>,
    /// A meditation held for the operator (#150's gate): `reconcile_gate =
    /// "refuse"` and an operator change no answer addressed.
    held: Option<PendingMeditation>,
    /// The pre-warm in flight (#504), which an ask cancels.
    warming: Option<Cancel>,
    /// The model's phase proposal awaiting the operator's ruling (#124).
    proposal: Option<PendingProposal>,
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
    /// The recording directory, when the session keeps one: where a meditation's
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
    /// The operator's edits and flags since a meditation last addressed them
    /// (#150), in order.
    operator_pending: Vec<Pending>,
    /// The pending changes this meditation's audit addressed (#150): the ones
    /// its answer acted on ([`acted_on`], #748). Cleared as the meditation ends.
    operator_addressed: Vec<String>,
    /// Whether the trunk carries a meditation's refill message after the head
    /// (#597): the turns start after it.
    refilled: bool,
    /// The forks cut during the turn that are in flight (#782), each its
    /// fork line's `seq`, its prompt and its stop.
    pool: Vec<PoolFork>,
    /// The forks cut during the turn waiting for a slot (#782), in step
    /// order.
    pool_queue: std::collections::VecDeque<Cut>,
    /// What waits for the last fork cut during the turn (#782): the turn's
    /// end, or a move to `awaiting`.
    deferred: Option<Deferred>,
    /// What the step warrants carry from step to step (#783).
    step_counters: crate::drive::warrants::Counters,
    /// The forks the step warrants fired this turn that run in its gap
    /// (#783): every step's under the gap timing, the final answer's under
    /// either.
    gap_firings: Vec<Firing>,
    /// The operator's phase proposal the model has not yet answered
    /// (#788): the phase it proposes, and whether an accepted move
    /// meditates.
    operator_proposal: Option<(String, bool)>,
    /// The phase an accepted move meditates into once the turn ends
    /// (#788): a meditation never refills under a running turn.
    move_after_turn: Option<String>,
    /// The note that puts the operator's proposal to the model, due at the
    /// next ask (#788).
    operator_note_due: Option<String>,
    /// This step's own phase proposal, which ends the turn (#788), logged
    /// with its evidence once the step has joined the trunk.
    proposal_ends_turn: Option<OwnProposal>,
    /// The evidence's marks (#788): the log's length when the session
    /// entered its phase, the trunk's tokens then, the phase's growth before
    /// the latest meditation, and the trunk's tokens at that meditation.
    phase_entered: u64,
    phase_floor: u64,
    phase_grown: u64,
    meditation_floor: u64,
}

impl State {
    /// Whether nothing is running or in flight by the state's own fields
    /// (#756): awaiting or ended, no call, no fork in flight or queued, no
    /// pre-warm, and no meditation queued or under way. `GET /state`'s
    /// `idle` adds no prompt waiting and no fork line unsettled.
    fn idle(&self) -> bool {
        matches!(self.settlement, Settlement::Awaiting | Settlement::Ended)
            && self.flight.is_none()
            && self.forking.is_none()
            && self.queued.is_empty()
            && self.warming.is_none()
            && self.meditation_pending.is_none()
            && self.meditating.is_none()
            && !self.pool_busy()
    }

    /// How many cut forks are in flight beside a trunk request (#782), in a
    /// session whose forks run during the turn; `None` otherwise.
    fn trunk_beside(&self) -> Option<u32> {
        self.interview
            .as_ref()
            .filter(|interview| interview.timing == Timing::DuringTurn)
            .map(|_| u32::try_from(self.pool.len()).unwrap_or(u32::MAX))
    }

    /// Whether a fork cut during the turn is in flight or queued (#782).
    fn pool_busy(&self) -> bool {
        !self.pool.is_empty() || !self.pool_queue.is_empty()
    }

    /// `f`, with the trunk's sizing as it was before (#782): a fork in
    /// flight beside the trunk is sized, and answers, without moving the
    /// measurement the trunk's next request is sized from.
    fn aside_sizing<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> T {
        let kept = (self.sent_estimate, self.last_sized, self.measured_prompt);
        let out = f(self);
        (self.sent_estimate, self.last_sized, self.measured_prompt) = kept;
        out
    }

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
            turns_at_meditation: 0,
            trunk_tokens: None,
            sent_estimate: None,
            last_sized: None,
            overflow_retried: None,
            meditation_pending: None,
            meditating: None,
            held: None,
            warming: None,
            proposal: None,
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
            phase_note_due: None,
            undelivered: Vec::new(),
            archive: super::archive::Archive::default(),
            recovered: BTreeMap::new(),
            mechanical: None,
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
            operator_pending: Vec::new(),
            operator_addressed: Vec::new(),
            refilled: false,
            pool: Vec::new(),
            pool_queue: std::collections::VecDeque::new(),
            deferred: None,
            step_counters: crate::drive::warrants::Counters::default(),
            gap_firings: Vec::new(),
            operator_proposal: None,
            move_after_turn: None,
            operator_note_due: None,
            proposal_ends_turn: None,
            phase_entered: 0,
            phase_floor: 0,
            phase_grown: 0,
            meditation_floor: 0,
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
    /// was added since; with none to go on, or a prompt that shrank (a meditation),
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
    /// meditation), the whole estimate plus [`ESTIMATE_PAD`].
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
        if let Event::Answered {
            timings, hosted, ..
        }
        | Event::Capped {
            timings, hosted, ..
        }
        | Event::Called {
            timings, hosted, ..
        } = &event
            && let Some(sent) = self.sent_estimate
        {
            // llama.cpp's timings, or a hosted API's usage (#555).
            let measured = timings
                .as_ref()
                .filter(|timings| timings.prompt_n.is_some() || timings.cache_n.is_some())
                .map(|timings| timings.prompt_n.unwrap_or(0) + timings.cache_n.unwrap_or(0))
                .or_else(|| hosted.usage.as_ref().map(|usage| usage.prompt_tokens));
            if let Some(prompt) = measured {
                self.measured_prompt = Some((prompt, sent));
            }
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

    /// The note recalling what earlier meditations archived that `asked` names
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
            .map(|found| log::RecalledItem {
                key: found.item.key.clone(),
                sha256: found.item.sha256(),
                score: found.score,
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
            log::ForkDelivery::Meditation => return None,
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
                CommandKind::DeclareMeditation => GapEnd::Meditation,
                CommandKind::End => GapEnd::End,
                CommandKind::OpenTangent
                | CommandKind::CloseTangent
                | CommandKind::Background
                | CommandKind::RatifyPhase
                | CommandKind::EditEntry
                | CommandKind::FlagEntry
                | CommandKind::RefillAnyway
                | CommandKind::StartPhase
                | CommandKind::ProposePhase
                | CommandKind::MovePhase => {
                    unreachable!("a tangent, background or phase ruling carries no idle gap")
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

    /// A command that carries no idle gap and ends none, admitted (#729):
    /// the operator's edit, flag, phase ruling or refill-anyway is a step
    /// within the gap, so the gap stays open for the ask that ends it.
    fn admit_within_the_gap(&mut self) {
        self.carried = None;
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
        // While a fork cut during the turn runs, the session is not
        // awaiting (#782): the move waits for the last of them.
        let to = if to == Settlement::Awaiting && self.pool_busy() {
            self.deferred.get_or_insert(Deferred::Move);
            if self.settlement == Settlement::Capture {
                return;
            }
            Settlement::Capture
        } else {
            to
        };
        if to == Settlement::Ended {
            self.stop_jobs();
        }
        let from = self.settlement;
        self.settlement = to;
        self.push(Event::Settled { from, to });
    }

    /// Stop every running background job as the session ends (#614), and
    /// log each one's end: `ended` is the log's last line (#291), so a job
    /// the session's end kills is logged `cancelled` before it, with the
    /// output it wrote so far. Its reaper then logs nothing more. An
    /// admitted `end`'s gap stays immediately before its outcome.
    fn stop_jobs(&mut self) {
        use super::background::Status;
        let gap = self.pending_gap.take();
        let mut ends = Vec::new();
        for job in self.jobs.values_mut() {
            if job.status != Status::Running {
                continue;
            }
            job.stopping = true;
            job.status = Status::Cancelled;
            super::background::stop_group(job.pid);
            job.write_status();
            let output = std::fs::read(&job.output).unwrap_or_default();
            let files: Vec<log::RecordedFile> = self
                .recording
                .as_deref()
                .and_then(|dir| super::attach::kept_whole(dir, &output, "text/plain").ok())
                .into_iter()
                .collect();
            ends.push(Event::BackgroundEnded {
                job: job.id.clone(),
                status: log::BackgroundStatus::Cancelled,
                exit: None,
                files,
            });
        }
        for end in ends {
            self.push(end);
        }
        self.pending_gap = gap;
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
    /// A fork's own transport on the trunk's server, when it asks the cache
    /// differently (#556): a hosted API's, whose fork never writes its tail
    /// for an hour.
    forks: Option<S>,
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
            (None, None, (None, None, Vec::new(), None, None)),
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
            (None, None, (None, None, Vec::new(), None, None)),
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
            (None, None, (None, None, Vec::new(), None, None)),
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
            (claim, interview, (None, None, Vec::new(), None, None)),
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
        (claim, interview, (unsent, reasoning_effort_default, instruction_files, levers, build)): Declared,
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
        let tools = tools.map(|tools| readable_outputs(tools, opened));
        let mut state = State::opening(trunk, phase_at_open.clone(), interview, tools.as_ref());
        // The evidence counts the trunk's growth from its head (#788).
        state.phase_floor = estimate_of(&template);
        state.meditation_floor = state.phase_floor;
        // The opening phase's rules (#787): the head is the session's own,
        // so they ride as a note on the first ask.
        let phase_document = state.phase_document();
        state.phase_note_due = state.phase_note();
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
            build,
            notes_in_the_ask: true,
            tool_output: tools.as_ref().map(|tools| tools.output_cap),
            bash_timeout_ms: bash_timeout_of(tools.as_ref(), &template),
            phases,
            phase_document,
        });
        Self::sharing(Shared {
            transport,
            template,
            state: Mutex::new(state),
            changed: Condvar::new(),
            tools,
            reap: Mutex::new(None),
            seat: None,
            forks: None,
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

    /// This session, its forks on the trunk's server made through `forks`
    /// (#556): the same server, asked to cache a fork's tail differently.
    /// An offboard seat, when there is one, still takes them.
    ///
    /// # Panics
    ///
    /// When the session is already shared.
    #[must_use]
    pub fn forking_through(mut self, forks: S) -> Self {
        Arc::get_mut(&mut self.shared)
            .expect("a session's forks are set before it is shared")
            .forks = Some(forks);
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
    #[allow(clippy::too_many_lines)]
    pub fn ask_attached(
        &self,
        attached: super::attach::Attached,
        gap: Option<IdleGap>,
        scoping: bool,
    ) -> Result<Admitted, Rejected> {
        let super::attach::Attached { message, files } = attached;
        let mut state = self.shared.lock();
        state.carry(gap, CommandKind::Ask);
        // A meditation's pre-warm in flight is cancelled by an ask (#504): the
        // ask's own request warms the same prefix. The meditation stands, logged
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
            // The trunk a held meditation did not refill (#150's gate).
            Settlement::Awaiting if state.held.is_some() => {
                let refused = state.refuse(CommandKind::Ask, Refusal::MeditationHeld);
                self.shared.changed.notify_all();
                return Err(Rejected::Refused(refused));
            }
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
        // A new turn's outputs are its own to cover, and its gap's forks
        // its own (#783).
        state.step_counters.covered.clear();
        state.gap_firings.clear();
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
        // The harness's notes ride in the ask's own user message, BEFORE the
        // operator's words, which the model answers last (#609, run 3: a
        // reminder sent as the last message was answered and the ask never
        // was) -- Qwen Code's order, its system reminders ahead of the user's
        // prompt in one user turn. In the order they are logged: the fork
        // delivery lever's patches; archive recall (#566), what earlier
        // meditations dropped that this ask names; background commands that ended
        // since (#614), Qwen Code's `<task-notification>`s; and the
        // self-capture reminder (#609), when its cadence came round, never
        // in the system prompt.
        let mut message = message;
        if let Some(text) = state.noted(turn) {
            message.notes.push(text);
        }
        // The operator's phase proposal, put to the model (#788).
        if let Some(text) = state.operator_note_due.take() {
            state.push(Event::Notified {
                turn,
                text: text.clone(),
            });
            message.notes.push(text);
        }
        if let Some(note) = state.deliver(turn) {
            message.notes.push(note.content);
        }
        if let Some(note) = state.recall(turn, &message.content) {
            message.notes.push(note.content);
        }
        if let Some(note) = state.notify(turn) {
            message.notes.push(note.content);
        }
        if let Some(text) = state.reminder_due.take() {
            state.push(Event::Reminded {
                turn,
                text: text.clone(),
            });
            message.notes.push(text);
        }
        shape.messages.push(message.clone());
        let opening = vec![message];
        // Pushed here, under the lock that admits the ask, and never on the
        // turn's thread: a thread that cannot start still settles with a
        // `request.failed` that cites a request that exists (#117, R2c
        // finding 21).
        // The automatic meditation (#617), before the request is sized.
        window_meditation(&self.shared.template, &mut state, &mut shape, false);
        let max_tokens = state.sized(&mut shape, self.shared.template.limits.max_output_tokens);
        let beside = state.trunk_beside();
        let request = state.push(Event::Requested {
            turn,
            lane: Lane::Trunk,
            head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
            fork: None,
            max_tokens,
            beside,
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
                    // A meditation the turn's end made due, queued for its audit
                    // or pre-warm (#504).
                    meditation_work(&shared);
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

    /// Declare a meditation (#493): the trunk is refilled from working memory --
    /// the head, with the working object rendered after it
    /// ([`crate::meditation::render::refill`]) -- and the next ask is sent on it.
    /// No turn of the old trunk is carried. Its [`Event::Meditated`] records the
    /// head's digest either side, the render, and what was carried.
    ///
    /// Not yet: the audit (the dogma's operator ask, whose answer grammar is
    /// unwritten), so nothing is ratified; and the pre-warm, so the next ask
    /// prefills the refilled trunk itself.
    ///
    /// # Errors
    ///
    /// [`Refusal::InFlight`] while a turn or a capture is in flight, and
    /// [`Refusal::NothingToMeditate`] before any turn has settled, or when working
    /// memory holds no entry (or the session keeps none): both logged. [`Refusal::Ended`] once the
    /// session has ended, not logged (#291). A refused command's `gap` is
    /// neither logged nor closed. [`Rejected::BadGap`] when `gap` cannot be
    /// logged, and then nothing is.
    pub fn declare_meditation(&self, gap: Option<IdleGap>) -> Result<(), Rejected> {
        self.declare_meditation_to(gap, None)
    }

    /// [`Self::declare_meditation`], moving to phase `to` when one is named
    /// (#563): the regimen's phase graph rules on the move, as it rules on
    /// the scripted drive's (`meditation::phase::PhaseGraph::decide`), and a move
    /// it does not allow is refused with its reason, logged, as
    /// `nothing-to-meditate` is. With no phase named, the session stays in its
    /// phase.
    ///
    /// # Errors
    ///
    /// As [`Self::declare_meditation`], and [`Refusal::NoPhaseGraph`],
    /// [`Refusal::NotAPhase`], [`Refusal::AlreadyInPhase`] or
    /// [`Refusal::NoPhaseEdge`] for a move the graph refuses.
    pub fn declare_meditation_to(
        &self,
        gap: Option<IdleGap>,
        to: Option<&str>,
    ) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        state.carry(gap, CommandKind::DeclareMeditation);
        let because = match state.settlement {
            Settlement::Ended => Some(Refusal::Ended),
            Settlement::Turn | Settlement::Capture => Some(Refusal::InFlight),
            // Nothing to refill from: no turn yet, no working object, or an
            // empty one -- a refill from an empty object would drop every
            // turn and carry nothing in their place.
            // A meditation while a tangent is open would replace the trunk its
            // close restores (#22).
            Settlement::Awaiting if state.tangent.is_some() => Some(Refusal::TangentOpen),
            Settlement::Awaiting
                if state.turns == 0
                    || state
                        .interview
                        .as_ref()
                        .is_none_or(|interview| interview.object.live().next().is_none()) =>
            {
                Some(Refusal::NothingToMeditate)
            }
            Settlement::Awaiting => to.and_then(|to| graph_refusal(&state, to)),
        };
        if let Some(because) = because {
            let refused = state.refuse(CommandKind::DeclareMeditation, because);
            drop(state);
            self.shared.changed.notify_all();
            return Err(Rejected::Refused(refused));
        }
        state.admit()?;
        let queued = meditation(
            &self.shared.template,
            &mut state,
            crate::meditation::Reason::Operator,
            to.map(str::to_owned),
        );
        drop(state);
        self.shared.changed.notify_all();
        if queued {
            // Its audit and pre-warm (#504), on a thread of their own.
            let shared = Arc::clone(&self.shared);
            let spawned = std::thread::Builder::new()
                .name("diet-meditation".to_owned())
                .spawn(move || meditation_work(&shared));
            if spawned.is_err() {
                meditation_work(&self.shared);
            }
        }
        Ok(())
    }

    /// Rule on the model's pending phase proposal (#124), the one its call
    /// `call` made: `meditation` declares the meditation that moves to the proposed
    /// phase, as [`Session::declare_meditation_to`] does (and, with the regimen's
    /// audit or pre-warm, queues them); `continue` moves the phase and keeps
    /// the trunk, so the next meditation's render names it; `stay` declines. A
    /// `phase.ruled` line records the ruling, and the proposal is spent.
    ///
    /// # Errors
    ///
    /// Refused, and logged: [`Refusal::Ended`], [`Refusal::InFlight`];
    /// [`Refusal::NoProposal`] when none is pending or `call` names another;
    /// for `meditation` and `continue`, the graph's refusal of the move from the
    /// phase the session is in now, and for `meditation`
    /// [`Refusal::NothingToMeditate`] as a declared meditation would be.
    pub fn ratify_phase(&self, call: &str, choice: PhaseChoice) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        let pending = state
            .proposal
            .as_ref()
            .filter(|proposal| proposal.call == call)
            .map(|proposal| proposal.to.clone());
        let because = match (state.settlement, pending.as_deref()) {
            (Settlement::Ended, _) => Some(Refusal::Ended),
            (Settlement::Turn | Settlement::Capture, _) => Some(Refusal::InFlight),
            (Settlement::Awaiting, None) => Some(Refusal::NoProposal),
            (Settlement::Awaiting, Some(_)) if choice == PhaseChoice::Stay => None,
            // A meditation's refill under an open tangent is one its close
            // would undo (#729), as a declared one is refused.
            (Settlement::Awaiting, Some(_))
                if choice == PhaseChoice::Meditation && state.tangent.is_some() =>
            {
                Some(Refusal::TangentOpen)
            }
            (Settlement::Awaiting, Some(to)) => graph_refusal(&state, to).or_else(|| {
                (choice == PhaseChoice::Meditation
                    && (state.turns == 0
                        || state
                            .interview
                            .as_ref()
                            .is_none_or(|interview| interview.object.live().next().is_none())))
                .then_some(Refusal::NothingToMeditate)
            }),
        };
        if let Some(because) = because {
            let refused = state.refuse(CommandKind::RatifyPhase, because);
            drop(state);
            self.shared.changed.notify_all();
            return Err(Rejected::Refused(refused));
        }
        state.admit_within_the_gap();
        let Some(PendingProposal { call, from, to }) = state.proposal.take() else {
            unreachable!("a pending proposal was checked above");
        };
        let phase_document = (choice != PhaseChoice::Stay)
            .then(|| state.document_of(&to))
            .flatten();
        state.push(Event::PhaseRuled {
            call: Some(call),
            by: None,
            choice,
            from,
            to: to.clone(),
            phase_document,
        });
        let queued = moved(&self.shared.template, &mut state, &to, choice);
        drop(state);
        self.shared.changed.notify_all();
        if queued {
            self.meditate_aside();
        }
        Ok(())
    }

    /// Choose the starting phase (#788), before the first ask: the session
    /// enters it, its rules a note on the first ask, and a `phase.started`
    /// line records it.
    ///
    /// # Errors
    ///
    /// Refused, and logged: [`Refusal::Ended`]; [`Refusal::AlreadyAsked`]
    /// after the first ask; [`Refusal::NoPhaseGraph`] or
    /// [`Refusal::NotAPhase`].
    pub fn start_phase(&self, phase: &str) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        let known = state
            .interview
            .as_ref()
            .map(|interview| &interview.phases)
            .filter(|graph| !graph.is_empty())
            .map(|graph| graph.phases().iter().any(|name| name == phase));
        let because = match (state.settlement, known) {
            (Settlement::Ended, _) => Some(Refusal::Ended),
            _ if state.turns > 0 => Some(Refusal::AlreadyAsked),
            (_, None) => Some(Refusal::NoPhaseGraph),
            (_, Some(false)) => Some(Refusal::NotAPhase),
            (_, Some(true)) => None,
        };
        if let Some(because) = because {
            return Err(self.refused(state, CommandKind::StartPhase, because));
        }
        state.admit_within_the_gap();
        state.phase = Some(phase.to_owned());
        state.phase_note_due = state.phase_note();
        let phase_document = state.document_of(phase);
        state.push(Event::PhaseStarted {
            phase: phase.to_owned(),
            phase_document,
        });
        entered(&self.shared.template, &mut state);
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Propose a phase move to the model (#788), through a meditation when
    /// it is accepted if `meditate`, else kept: logged with its evidence,
    /// and put to the model in a note on its next ask, which it answers
    /// through the phase tool.
    ///
    /// # Errors
    ///
    /// Refused, and logged: [`Refusal::Ended`], [`Refusal::InFlight`];
    /// [`Refusal::ProposalPending`] while the model's own waits on a ruling;
    /// the graph's refusal of the move.
    pub fn propose_phase(&self, to: &str, meditate: bool) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        let because = match state.settlement {
            Settlement::Ended => Some(Refusal::Ended),
            Settlement::Turn | Settlement::Capture => Some(Refusal::InFlight),
            Settlement::Awaiting if state.proposal.is_some() => Some(Refusal::ProposalPending),
            Settlement::Awaiting => graph_refusal(&state, to),
        };
        if let Some(because) = because {
            return Err(self.refused(state, CommandKind::ProposePhase, because));
        }
        state.admit_within_the_gap();
        let from = state.phase.clone();
        let evidence = evidence(&state, &self.shared.template);
        state.operator_proposal = Some((to.to_owned(), meditate));
        state.operator_note_due = Some(operator_proposal_note(from.as_deref(), to));
        state.push(Event::PhaseProposed {
            by: Key::Operator,
            call: None,
            from,
            to: to.to_owned(),
            reason: None,
            meditate: Some(meditate),
            evidence,
        });
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Move the phase alone (#788), where `operator_unilateral` lets the
    /// operator: through a meditation that moves to `to` when `meditate`,
    /// else kept, the trunk as it stands. Any proposal waiting is spent.
    ///
    /// # Errors
    ///
    /// Refused, and logged: [`Refusal::Ended`], [`Refusal::InFlight`],
    /// [`Refusal::NotUnilateral`]; the graph's refusal of the move; and for
    /// a meditation, [`Refusal::TangentOpen`] or
    /// [`Refusal::NothingToMeditate`] as a declared one is refused.
    pub fn move_phase(&self, to: &str, meditate: bool) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        let unilateral = state
            .interview
            .as_ref()
            .is_some_and(|interview| interview.operator_unilateral);
        let because = match state.settlement {
            Settlement::Ended => Some(Refusal::Ended),
            Settlement::Turn | Settlement::Capture => Some(Refusal::InFlight),
            Settlement::Awaiting if !unilateral => Some(Refusal::NotUnilateral),
            Settlement::Awaiting => graph_refusal(&state, to).or_else(|| {
                if !meditate {
                    None
                } else if state.tangent.is_some() {
                    Some(Refusal::TangentOpen)
                } else {
                    (state.turns == 0
                        || state
                            .interview
                            .as_ref()
                            .is_none_or(|interview| interview.object.live().next().is_none()))
                    .then_some(Refusal::NothingToMeditate)
                }
            }),
        };
        if let Some(because) = because {
            return Err(self.refused(state, CommandKind::MovePhase, because));
        }
        state.admit_within_the_gap();
        state.proposal = None;
        state.operator_proposal = None;
        state.operator_note_due = None;
        let choice = if meditate {
            PhaseChoice::Meditation
        } else {
            PhaseChoice::Keep
        };
        let from = state.phase.clone();
        let phase_document = state.document_of(to);
        state.push(Event::PhaseRuled {
            call: None,
            by: Some(Key::Operator),
            choice,
            from,
            to: to.to_owned(),
            phase_document,
        });
        let queued = moved(&self.shared.template, &mut state, to, choice);
        drop(state);
        self.shared.changed.notify_all();
        if queued {
            self.meditate_aside();
        }
        Ok(())
    }

    /// `command` refused `because`, logged: the rejection to return.
    fn refused(
        &self,
        mut state: MutexGuard<'_, State>,
        command: CommandKind,
        because: Refusal,
    ) -> Rejected {
        let refused = state.refuse(command, because);
        drop(state);
        self.shared.changed.notify_all();
        Rejected::Refused(refused)
    }

    /// The meditation a phase move queued, on a thread of its own: its audit
    /// or pre-warm, then its line.
    fn meditate_aside(&self) {
        let shared = Arc::clone(&self.shared);
        let spawned = std::thread::Builder::new()
            .name("diet-meditation".to_owned())
            .spawn(move || meditation_work(&shared));
        if spawned.is_err() {
            meditation_work(&self.shared);
        }
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

    /// Correct working-memory entry `id` with `content` (#150): the
    /// operator's edit, written as the object writes every correction -- a
    /// new entry superseding `id`, on the operator's lane -- and logged as
    /// its `patch` line. It is carried to the next meditation as a change for the
    /// model to address.
    ///
    /// # Errors
    ///
    /// [`Refusal::Ended`]; [`Refusal::InFlight`] while a turn or a capture is
    /// in flight; [`Refusal::Empty`] for no content; [`Refusal::UnknownEntry`]
    /// for an id working memory does not hold, or none kept;
    /// [`Refusal::NotLive`] for one no longer live.
    pub fn edit_entry(&self, id: &str, content: &str) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        let because =
            operator_refusal(&state, id, content).or_else(|| restates(&state, id, content));
        if let Some(because) = because {
            let refused = state.refuse(CommandKind::EditEntry, because);
            drop(state);
            self.shared.changed.notify_all();
            return Err(Rejected::Refused(refused));
        }
        state.admit_within_the_gap();
        let seq = state.log.len();
        let turn = state.turns;
        let tangent = state
            .tangent
            .as_ref()
            .map(|(tangent, _)| tangent.id().to_owned());
        let (Ok(voids), Ok(new)) = (
            crate::object::EntryId::new(id),
            crate::object::EntryId::new(&format!("operator-{seq}")),
        ) else {
            unreachable!("an id the object holds, and one this session coins");
        };
        let patch = Patch::Supersede {
            id: new.clone(),
            content: content.to_owned(),
            voids,
            provenance: crate::object::Provenance {
                turn,
                lane: crate::object::OPERATOR.to_owned(),
                fork: None,
                tangent,
                index: 0,
            },
        };
        let Some(interview) = state.interview.as_mut() else {
            unreachable!("a live entry was checked above");
        };
        if interview.object.apply(&patch).is_err() {
            unreachable!("a live entry superseded by new content");
        }
        let line = patched(None, &patch, &interview.object);
        state.push(line);
        state.operator_pending.push(Pending {
            entry: new.as_str().to_owned(),
            kind: "edit",
            note: None,
        });
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Flag working-memory entry `id` with the operator's `note` (#150): an
    /// annotation, the entry unchanged, logged as a `flag` line and carried
    /// to the next meditation for the model to address.
    ///
    /// # Errors
    ///
    /// As [`Self::edit_entry`], `note` standing for its content.
    pub fn flag_entry(&self, id: &str, note: &str) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        if let Some(because) = operator_refusal(&state, id, note) {
            let refused = state.refuse(CommandKind::FlagEntry, because);
            drop(state);
            self.shared.changed.notify_all();
            return Err(Rejected::Refused(refused));
        }
        state.admit_within_the_gap();
        // By the object's own id (#729): a flag on an alias marks the entry
        // the audit lists.
        let id = state
            .interview
            .as_ref()
            .and_then(|interview| {
                crate::object::EntryId::new(id)
                    .ok()
                    .and_then(|named| interview.object.entry(&named))
                    .map(|entry| entry.id.as_str().to_owned())
            })
            .unwrap_or_else(|| id.to_owned());
        let id = id.as_str();
        let turn = state.turns;
        state.push(Event::Flagged {
            turn,
            entry: id.to_owned(),
            note: note.to_owned(),
        });
        state.operator_pending.push(Pending {
            entry: id.to_owned(),
            kind: "flag",
            note: Some(note.to_owned()),
        });
        drop(state);
        self.shared.changed.notify_all();
        Ok(())
    }

    /// Refill the meditation held for the operator anyway (#150's gate): the
    /// refill the meditation would have made, with no audit and no gate, its pre-
    /// warm when the regimen asks for one, and its meditation line marked
    /// `refilled_anyway` beside the changes it carried unaddressed.
    ///
    /// # Errors
    ///
    /// Refused, and logged: [`Refusal::Ended`], [`Refusal::InFlight`], and
    /// [`Refusal::NothingHeld`] when no meditation is held.
    pub fn refill_anyway(&self) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        let because = match state.settlement {
            Settlement::Ended => Some(Refusal::Ended),
            Settlement::Turn | Settlement::Capture => Some(Refusal::InFlight),
            Settlement::Awaiting if state.held.is_none() => Some(Refusal::NothingHeld),
            // As a declared meditation is (#729): its close would undo it.
            Settlement::Awaiting if state.tangent.is_some() => Some(Refusal::TangentOpen),
            Settlement::Awaiting => None,
        };
        if let Some(because) = because {
            let refused = state.refuse(CommandKind::RefillAnyway, because);
            drop(state);
            self.shared.changed.notify_all();
            return Err(Rejected::Refused(refused));
        }
        state.admit_within_the_gap();
        let Some(PendingMeditation { reason, to, .. }) = state.held.take() else {
            unreachable!("a held meditation was checked above");
        };
        // The phase it was to move to, ruled again from where the session
        // is now (#729): a `continue` while it held may have moved it, and a
        // move the graph no longer allows from here is not made -- the
        // refill is in place.
        let to = to.filter(|to| graph_refusal(&state, to).is_none());
        state.meditation_pending = Some(PendingMeditation {
            reason,
            to,
            anyway: true,
        });
        state.move_to(Settlement::Capture);
        drop(state);
        self.shared.changed.notify_all();
        let shared = Arc::clone(&self.shared);
        let spawned = std::thread::Builder::new()
            .name("diet-meditation".to_owned())
            .spawn(move || meditation_work(&shared));
        if spawned.is_err() {
            meditation_work(&self.shared);
        }
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
        let mut stops: Vec<Cancel> = Vec::new();
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
            // (#374). Every fork cut during the turn is stopped too, and
            // none queued is sent (#782).
            Settlement::Capture if state.forking.is_some() || state.pool_busy() => {
                state.admit().map(|()| {
                    state.ending = true;
                    stop = state.flight.as_ref().map(|flight| flight.cancel.clone());
                    stops.extend(state.pool.iter().map(|pool| pool.cancel.clone()));
                    state.pool_queue.clear();
                    let _ = pool_settled(&self.shared.template, &mut state);
                })
            }
            Settlement::Turn | Settlement::Capture => Err(Rejected::Refused(
                state.refuse(CommandKind::End, Refusal::InFlight),
            )),
            Settlement::Ended => Err(Rejected::Refused(
                state.refuse(CommandKind::End, Refusal::Ended),
            )),
        };
        drop(state);
        self.shared.changed.notify_all();
        for stop in stop.into_iter().chain(stops) {
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
                listed: None,
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

    /// What the session is doing now, as `GET /state` answers it (#756),
    /// read under one lock: whether it is running or ended; its settlement;
    /// the latest turn -- `running`, `awaiting-approval`, or `settled` with
    /// its reason; the forks and audits in flight, by their lines' `seq`,
    /// the forks queued behind them, and whether a pre-warm is in flight;
    /// the meditation -- `none`, `capturing`, or `held` with what it left
    /// `unaddressed`; the prompt waiting on the operator, in `waiting`'s
    /// shape; the last refusal, its `seq`, `command` and `because`; the
    /// last `seq`; and whether it is `idle`: no turn running, nothing in
    /// flight, nothing queued and no prompt waiting. `turn`, `approval` and
    /// `refusal` are absent when there is none, as the log leaves a key out.
    #[must_use]
    pub fn status(&self) -> Value {
        let state = self.shared.lock();
        let (number, text, object) = (status_number, status_text, status_object);
        let LogRead {
            open,
            settled,
            refusal,
            unaddressed,
        } = LogRead::of(&state);
        let refusal = refusal.map(|(seq, command, because)| {
            object(vec![
                ("seq", number(seq)),
                ("command", text(command.tag())),
                ("because", text(because.tag())),
            ])
        });
        let prompt = state
            .waiting
            .as_ref()
            .filter(|waiting| waiting.answer.is_none())
            .map(|waiting| waiting.prompt.value());
        let turn = match (state.turns, settled) {
            (0, _) => None,
            (n, Some(reason)) => Some(object(vec![
                ("number", number(u64::from(n))),
                ("state", text("settled")),
                ("reason", text(reason.tag())),
            ])),
            (n, None) => Some(object(vec![
                ("number", number(u64::from(n))),
                (
                    "state",
                    text(if prompt.is_some() {
                        "awaiting-approval"
                    } else {
                        "running"
                    }),
                ),
            ])),
        };
        let meditation = match (&state.held, unaddressed) {
            (Some(_), Some(left)) => object(vec![
                ("state", text("held")),
                (
                    "unaddressed",
                    Value::Array(left.iter().map(|entry| text(entry)).collect()),
                ),
            ]),
            _ if state.meditation_pending.is_some() || state.meditating.is_some() => {
                object(vec![("state", text("capturing"))])
            }
            _ => object(vec![("state", text("none"))]),
        };
        let idle = state.idle() && prompt.is_none() && open.is_empty();
        let seqs = |audits: bool| {
            Value::Array(
                open.iter()
                    .filter(|(_, audit)| *audit == audits)
                    .map(|(seq, _)| number(*seq))
                    .collect(),
            )
        };
        let mut status = vec![
            (
                "session",
                text(if state.settlement == Settlement::Ended {
                    "ended"
                } else {
                    "running"
                }),
            ),
            ("settlement", text(state.settlement.tag())),
            (
                "in_flight",
                object(vec![
                    ("forks", seqs(false)),
                    ("audits", seqs(true)),
                    ("queued", number(state.queued.len() as u64)),
                    ("warming", Value::Boolean(state.warming.is_some())),
                ]),
            ),
            ("meditation", meditation),
            (
                "last_seq",
                number(state.log.last().map_or(0, |logged| logged.seq)),
            ),
            ("idle", Value::Boolean(idle)),
        ];
        for (key, value) in [("turn", turn), ("approval", prompt), ("refusal", refusal)] {
            if let Some(value) = value {
                status.push((key, value));
            }
        }
        object(status)
    }

    /// Working memory as the session holds it, as `GET /memory` answers it
    /// (#756): `entries`, each as the object's dump writes it (`id`,
    /// `content`, `state`, `provenances`, and its links), the live ones
    /// only unless `all`; on each, the operator's changes no meditation has
    /// addressed yet as `operator`, each a `kind` -- `edit` or `flag` --
    /// and a flag's `note`; and `last_seq`, the log's position it was read
    /// at. A session with no working memory answers no entries.
    #[must_use]
    pub fn memory(&self, all: bool) -> Value {
        let state = self.shared.lock();
        let entries = state.interview.as_ref().map_or_else(Vec::new, |interview| {
            interview
                .object
                .dump_values()
                .into_iter()
                .filter(|(id, _)| {
                    all || interview
                        .object
                        .entry(id)
                        .is_some_and(|entry| entry.state.is_live())
                })
                .map(|(id, mut value)| {
                    let changes: Vec<Value> = state
                        .operator_pending
                        .iter()
                        .filter(|change| change.entry == id.as_str())
                        .map(|change| {
                            let mut members = BTreeMap::from([(
                                "kind".to_owned(),
                                Value::String(change.kind.to_owned()),
                            )]);
                            if let Some(note) = &change.note {
                                members.insert("note".to_owned(), Value::String(note.clone()));
                            }
                            Value::Object(members)
                        })
                        .collect();
                    if let Value::Object(members) = &mut value
                        && !changes.is_empty()
                    {
                        members.insert("operator".to_owned(), Value::Array(changes));
                    }
                    value
                })
                .collect()
        });
        let last = state.log.last().map_or(0, |logged| logged.seq);
        Value::Object(BTreeMap::from([
            ("entries".to_owned(), Value::Array(entries)),
            (
                "last_seq".to_owned(),
                Value::Integer(i64::try_from(last).unwrap_or(i64::MAX)),
            ),
        ]))
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
            phase_document,
            instruction_files,
            levers,
            notes_in_the_ask,
            build,
        } => log::Event::SessionStart {
            phase_document: phase_document.clone(),
            build_commit: build.as_ref().map(|(commit, _)| commit.clone()),
            build_dirty: build.as_ref().map(|(_, dirty)| *dirty),
            bash_timeout_ms: *bash_timeout_ms,
            levers: levers.clone(),
            notes_in_the_ask: notes_in_the_ask.then_some(true),
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
            beside,
        } => log::Event::Request {
            beside: beside.map(u64::from),
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
            pinned,
            displaces,
            cut,
            timing,
            view_overridden,
            reserved,
        } => log::Event::Fork {
            after_meditation: None,
            reserved: reserved.map(|(tokens, _)| tokens),
            prefix_shared: reserved.map(|(_, shared)| shared),
            cut: *cut,
            // Absent is the gap (#782): a gap fork's line is as before.
            timing: (*timing != Timing::Gap).then(|| timing.word().to_owned()),
            view_overridden: view_overridden.map(ForkView::word),
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
            // A pinned template by its dogma name, lower case, as an
            // audit's is (#783).
            ask: Some(pinned.map_or_else(|| ask.tag().to_owned(), |t| t.name().to_lowercase())),
            hazard: displaces.then(|| DISPLACES_TRUNK_CACHE.to_owned()),
        },
        Event::Audited {
            of_turn,
            at,
            template,
            question,
            after_meditation,
        } => log::Event::Fork {
            after_meditation: *after_meditation,
            lane: log::Lane::Audit,
            of_turn: *of_turn,
            at: *at,
            why: log::Warrant::Meditation,
            question: question.clone(),
            view: None,
            trigger: None,
            role: None,
            seat: None,
            // The pinned ask it put, by its dogma name, lower case.
            ask: Some(template.name().to_lowercase()),
            hazard: None,
            cut: None,
            timing: None,
            view_overridden: None,
            reserved: None,
            prefix_shared: None,
        },
        Event::ForkSettled {
            fork,
            outcome,
            prompt_tokens,
            wall_ms,
            refused,
            trailer,
            pool,
            not_run,
        } => log::Event::ForkSettled {
            pool: *pool,
            not_run: *not_run,
            fork: *fork,
            outcome: *outcome,
            prompt_tokens: *prompt_tokens,
            wall_ms: *wall_ms,
            refused: refused.clone(),
            trailer: trailer.clone(),
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
            reason,
        } => log::Event::Patch {
            fork: *fork,
            lane: lane.clone(),
            op: *op,
            entry: entry.clone(),
            supersedes: supersedes.clone(),
            tangent: tangent.clone(),
            reason: reason.clone(),
        },
        Event::PhaseRuled {
            call,
            by,
            choice,
            from,
            to,
            phase_document,
        } => log::Event::PhaseRuled {
            call: call.clone(),
            by: by.map(|key| key.tag().to_owned()),
            choice: choice.tag().to_owned(),
            from: from.clone(),
            to: to.clone(),
            phase_document: phase_document.clone(),
        },
        Event::PhaseProposed {
            by,
            call,
            from,
            to,
            reason,
            meditate,
            evidence,
        } => log::Event::PhaseProposed {
            by: by.tag().to_owned(),
            call: call.clone(),
            from: from.clone(),
            to: to.clone(),
            reason: reason.clone(),
            meditate: *meditate,
            evidence: *evidence,
        },
        Event::PhaseAnswered {
            call,
            accepted,
            reason,
        } => log::Event::PhaseAnswered {
            call: call.clone(),
            accepted: *accepted,
            reason: reason.clone(),
        },
        Event::PhaseStarted {
            phase,
            phase_document,
        } => log::Event::PhaseStarted {
            phase: phase.clone(),
            phase_document: phase_document.clone(),
        },
        Event::PhaseNoted { turn, text } => log::Event::PhaseNoted {
            turn: *turn,
            text: text.clone(),
        },
        Event::Captured {
            request,
            call,
            tool,
            outcome,
            entries,
            why,
            fork,
            from,
            to,
        } => log::Event::Capture {
            request: *request,
            call: call.clone(),
            tool: tool.clone(),
            outcome: outcome.clone(),
            entries: entries.clone(),
            why: why.clone(),
            fork: *fork,
            from: from.clone(),
            to: to.clone(),
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
        Event::Meditated {
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
            phase_document,
            tool_outputs,
            outputs,
            background,
            render_budget,
            fired,
            pruned,
            operator_changes,
            unaddressed,
            newest_turn_tokens,
            refilled_anyway,
            warm,
        } => log::Event::Meditation {
            fired: *fired,
            warm: warm.as_ref().map(timings_line),
            phase: phase.clone(),
            phase_document: phase_document.clone(),
            at_turn: *at_turn,
            reason: meditation_reason_of(*reason),
            prefix_hash_before: prefix_hash_before.clone(),
            prefix_hash_after: prefix_hash_after.clone(),
            frame: crate::meditation::render::FRAME_VERSION.to_owned(),
            render: render.clone(),
            carried_entries: *carried_entries,
            carried_turns: *carried_turns,
            // The depth, and what it kept, on a meditation that could keep a tail
            // (#552); a total compaction writes neither.
            tail_tokens: (*tail_tokens > 0).then_some(*tail_tokens),
            carried_tokens: (*tail_tokens > 0).then_some(*carried_tokens),
            // The state always (#553); the section, its count and its
            // bytes when it carried any.
            tool_outputs: Some(*tool_outputs),
            outputs: outputs.as_ref().map(|(text, _)| text.clone()),
            carried_outputs: outputs.as_ref().map(|(_, n)| *n),
            carried_output_bytes: outputs.as_ref().map(|(text, _)| text.len() as u64),
            background: background.clone(),
            // A user message after the head (#597).
            placement: Some(log::RenderPlacement::Message),
            render_budget: render_budget.clone(),
            pruned: pruned.clone(),
            operator_changes: operator_changes.clone(),
            unaddressed: unaddressed.clone(),
            newest_turn_tokens: *newest_turn_tokens,
            refilled_anyway: *refilled_anyway,
        },
        Event::MeditationHeld {
            at_turn,
            reason,
            unaddressed,
        } => log::Event::MeditationHeld {
            at_turn: *at_turn,
            reason: meditation_reason_of(*reason),
            unaddressed: unaddressed.clone(),
        },
        Event::Flagged { turn, entry, note } => log::Event::Flag {
            turn: *turn,
            entry: entry.clone(),
            note: note.clone(),
        },
        Event::PatchRefused {
            fork,
            lane,
            op,
            entry,
        } => log::Event::PatchRefused {
            fork: *fork,
            lane: lane.clone(),
            op: *op,
            entry: entry.clone(),
            reason: OPERATOR_ENTRY.to_owned(),
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

/// A logged event as its log line, read back as an object (#756): what
/// `GET /wait` names as its `event`, and whose `kind` it matches.
#[must_use]
pub fn line_value(logged: &Logged) -> BTreeMap<String, Value> {
    crate::formats::record::json::line(&render(logged)).unwrap_or_default()
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
            GapEnd::Meditation => log::GapEnd::Meditation,
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

/// A meditation's reason in the log's words.
fn meditation_reason_of(reason: crate::meditation::Reason) -> log::MeditationReason {
    match reason {
        crate::meditation::Reason::Operator => log::MeditationReason::Operator,
        crate::meditation::Reason::Phase => log::MeditationReason::Phase,
        crate::meditation::Reason::Budget => log::MeditationReason::Budget,
        crate::meditation::Reason::Cadence => log::MeditationReason::Cadence,
        crate::meditation::Reason::Window => log::MeditationReason::Window,
        crate::meditation::Reason::Prune => log::MeditationReason::Prune,
    }
}

fn command_of(command: CommandKind) -> log::Command {
    match command {
        CommandKind::Ask => log::Command::Ask,
        CommandKind::Cancel => log::Command::Cancel,
        CommandKind::DeclareMeditation => log::Command::DeclareMeditation,
        CommandKind::End => log::Command::End,
        CommandKind::OpenTangent => log::Command::OpenTangent,
        CommandKind::CloseTangent => log::Command::CloseTangent,
        CommandKind::Background => log::Command::Background,
        CommandKind::RatifyPhase => log::Command::RatifyPhase,
        CommandKind::EditEntry => log::Command::EditEntry,
        CommandKind::FlagEntry => log::Command::FlagEntry,
        CommandKind::RefillAnyway => log::Command::RefillAnyway,
        CommandKind::StartPhase => log::Command::StartPhase,
        CommandKind::ProposePhase => log::Command::ProposePhase,
        CommandKind::MovePhase => log::Command::MovePhase,
    }
}

fn refusal_of(refusal: Refusal) -> log::Refusal {
    match refusal {
        Refusal::InFlight => log::Refusal::InFlight,
        Refusal::Ended => log::Refusal::Ended,
        Refusal::NothingInFlight => log::Refusal::NothingInFlight,
        Refusal::NothingToMeditate => log::Refusal::NothingToMeditate,
        Refusal::NoPhaseGraph => log::Refusal::NoPhaseGraph,
        Refusal::NotAPhase => log::Refusal::NotAPhase,
        Refusal::AlreadyInPhase => log::Refusal::AlreadyInPhase,
        Refusal::NoPhaseEdge => log::Refusal::NoPhaseEdge,
        Refusal::Stale => log::Refusal::Stale,
        Refusal::TangentOpen => log::Refusal::TangentOpen,
        Refusal::NoTangent => log::Refusal::NoTangent,
        Refusal::NoProposal => log::Refusal::NoProposal,
        Refusal::BadTangent => log::Refusal::BadTangent,
        Refusal::NotTheScope => log::Refusal::NotTheScope,
        Refusal::NothingRunning => log::Refusal::NothingRunning,
        Refusal::UnknownEntry => log::Refusal::UnknownEntry,
        Refusal::NotLive => log::Refusal::NotLive,
        Refusal::Empty => log::Refusal::Empty,
        Refusal::MeditationHeld => log::Refusal::MeditationHeld,
        Refusal::NothingHeld => log::Refusal::NothingHeld,
        Refusal::Restates => log::Refusal::Restates,
        Refusal::NotUnilateral => log::Refusal::NotUnilateral,
        Refusal::AlreadyAsked => log::Refusal::AlreadyAsked,
        Refusal::ProposalPending => log::Refusal::ProposalPending,
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
fn call<S: Streaming + 'static>(
    shared: &Arc<Shared<S>>,
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
    while let Some((next, a_step)) = step(
        shared,
        &mut shape,
        cancel,
        &mut exchange,
        (turn, request, steps),
    ) {
        request = next;
        if a_step {
            steps += 1;
        }
    }
}

/// Whether `finish_reason` is a cap's.
fn capped(finish_reason: Option<&str>) -> bool {
    finish_reason.is_some_and(|reason| CAPPED_FINISH_REASONS.contains(&reason))
}

/// One step's request: stream it into the log, then settle the turn, or run
/// the calls it made and return the next request's sequence number.
#[allow(clippy::too_many_lines)]
fn step<S: Streaming + 'static>(
    shared: &Arc<Shared<S>>,
    shape: &mut RequestShape,
    cancel: &Cancel,
    exchange: &mut Vec<Message>,
    (turn, request, steps): (u32, u64, u32),
) -> Option<(u64, bool)> {
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
                // The final answer is a step too (#783): its gates count, and
                // its forks run in the gap, cut at the answer.
                let answered = state
                    .log
                    .iter()
                    .rev()
                    .find_map(|logged| match &logged.event {
                        Event::Answered {
                            request: to,
                            text,
                            reasoning,
                            ..
                        } if *to == request => Some((text.clone(), reasoning.clone())),
                        _ => None,
                    });
                if let Some((text, reasoning)) = answered {
                    let mut said = Message::new(Role::Assistant, text);
                    said.reasoning = reasoning;
                    let content = content_firings(&mut state, (turn, request), &said, &[]);
                    state.gap_firings.extend(content);
                }
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
            state.step_tokens = timings
                .as_ref()
                .and_then(trunk_tokens_of)
                .or_else(|| hosted.trunk_tokens());
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
            )
            .map(|next| (next, true));
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
            // #617: an overflow meditates once and goes again.
            if overflow.is_some()
                && let Some(next) = retried(&shared.template, &mut state, shape, (turn, cancel))
            {
                drop(state);
                shared.changed.notify_all();
                // The same step sent again (#728): no step of its own.
                return Some((next, false));
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
            // #617: an overflow meditates once and goes again.
            if overflow.is_some()
                && let Some(next) = retried(&shared.template, &mut state, shape, (turn, cancel))
            {
                drop(state);
                shared.changed.notify_all();
                // The same step sent again (#728): no step of its own.
                return Some((next, false));
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
#[allow(clippy::too_many_lines)]
fn run_calls<S: Streaming + 'static>(
    shared: &Arc<Shared<S>>,
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
    // Where a fork the step warrants is cut (#782): its last `tool_call`.
    let mut last_line = request;
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
        // Its touches, patched in as it settles (#784), logged after it.
        let mechanical = mechanically_captured(shared, &mut state, &line, (turn, request, steps));
        last_line = state.push(Event::ToolCalled(Box::new(line)));
        for patch in mechanical {
            state.push(patch);
        }
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
    // The forks this step warrants (#782).
    cut_step(
        shared,
        &mut state,
        (turn, request, last_line),
        (shape, &said, &results, calls),
        settled,
    );
    // A proposal of the model's own ends its turn after this step (#788):
    // the step joins the trunk, the result saying so the last thing on it.
    let proposed = state.proposal_ends_turn.take();
    if let (Some(OwnProposal { reason }), None) = (proposed, settled) {
        state.flight = None;
        let mut ran = std::mem::take(exchange);
        ran.push(said);
        ran.extend(results);
        state.trunk.extend(ran);
        state.trunk_tokens = state.step_tokens.take();
        state.ran.clear();
        // The proposal, with the evidence of the trunk it ends the turn on.
        if let Some(PendingProposal { call, from, to }) = state.proposal.clone() {
            let evidence = evidence(&state, &shared.template);
            state.push(Event::PhaseProposed {
                by: Key::Model,
                call: Some(call),
                from,
                to,
                reason,
                meditate: None,
                evidence,
            });
        }
        state.push(Event::TurnSettled {
            turn,
            reason: SettleReason::Final,
        });
        turn_over(&shared.template, &mut state);
        drop(state);
        shared.changed.notify_all();
        return None;
    }
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
    // The automatic meditation (#617), between the turn's steps too.
    window_meditation(&shared.template, &mut state, shape, false);
    let max_tokens = state.sized(shape, shared.template.limits.max_output_tokens);
    let beside = state.trunk_beside();
    let next = state.push(Event::Requested {
        turn,
        lane: Lane::Trunk,
        head_sha256: crate::client::head::Head::of(shape).digest().to_owned(),
        fork: None,
        max_tokens,
        beside,
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

/// What the model is shown of a command's output, `before` it (#734): as
/// [`shown_capped`] when the call held its output whole; spilled, the
/// whole kept in the recording from its files, never in memory at once,
/// and the held head and tail capped -- under the session's cap, or with
/// the cap off the default, since the whole was never held to show.
fn shown_ran(
    tools: &Tools,
    before: &str,
    ran: &crate::isolation::Ran,
    line: &mut ToolLine,
) -> String {
    use super::attach::Part;
    use super::output::{self, Kept, OutputCap};
    let seen = format!("{before}{}", ran.as_the_model_sees_it());
    if !ran.spilled() {
        return shown_capped(tools, &seen, line);
    }
    let newline = |held: &[u8]| (!held.is_empty() && !held.ends_with(b"\n")).then_some(b"\n");
    let mut parts = vec![Part::Bytes(before.as_bytes())];
    parts.push(match &ran.stdout_spill {
        Some(spill) => Part::File(&spill.path),
        None => Part::Bytes(&ran.raw_stdout),
    });
    if let Some(nl) = newline(&ran.raw_stdout) {
        parts.push(Part::Bytes(nl));
    }
    parts.push(match &ran.stderr_spill {
        Some(spill) => Part::File(&spill.path),
        None => Part::Bytes(ran.stderr.as_bytes()),
    });
    if let Some(nl) = newline(ran.stderr.as_bytes()) {
        parts.push(Part::Bytes(nl));
    }
    let saved = tools.recording.as_ref().and_then(|dir| {
        super::attach::kept_whole_from(dir, &parts, "text/plain")
            .ok()
            .map(|file| (dir.join(&file.path).to_string_lossy().into_owned(), file))
    });
    let kept = saved.as_ref().map_or(Kept::Nowhere, |(path, _)| Kept::At {
        path,
        read_tool: tools.read_tool.as_deref(),
    });
    let cap = match tools.output_cap {
        OutputCap::Keep => OutputCap::DEFAULT,
        capped @ OutputCap::Capped { .. } => capped,
    };
    let shown = output::capped(&seen, cap, kept);
    if let Some((_, file)) = saved {
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
            let (refused, written) = state.interview.as_mut().map_or_else(
                || {
                    (
                        Some("the session keeps no working memory".to_owned()),
                        Vec::new(),
                    )
                },
                |interview| {
                    // An entry the operator wrote is theirs (#150).
                    let (kept, refusals) = guarded(&patches, &interview.object, None);
                    let refused = kept
                        .iter()
                        .find_map(|patch| interview.object.apply(patch).err())
                        .map(|why| why.to_string())
                        // Nothing of it kept: refused over the operator's
                        // entry, and it says whose (#689).
                        .or_else(|| {
                            kept.is_empty()
                                .then(|| operator_refused(&refusals))
                                .flatten()
                        });
                    if refused.is_none() {
                        lines = refusals
                            .into_iter()
                            .chain(
                                kept.iter()
                                    .map(|patch| patched(None, patch, &interview.object)),
                            )
                            .collect();
                    } else {
                        lines = refusals;
                    }
                    (refused, entries_of(&kept))
                },
            );
            capture_outcome(&call.name, refused, written)
        }
    };
    if call.name == CaptureTool::UpdateRecord.tag() && outcome == "recorded" {
        state.recorded_this_turn = true;
    }
    // A phase proposal (#124), ruled by the graph at once: refused in the
    // graph's word, or held as the one proposal pending the operator.
    let (mut outcome, mut why) = (outcome, why);
    let mut moved = None;
    if call.name == CaptureTool::ProposePhaseTransition.tag() && outcome == "proposed" {
        let to = serde_json::from_str::<serde_json::Value>(&call.arguments)
            .ok()
            .and_then(|args| args.get("to")?.as_str().map(str::to_owned))
            .unwrap_or_default();
        let from = state.phase.clone();
        match graph_refusal(&state, &to) {
            Some(refusal) => {
                outcome = "refused";
                why = Some(refusal.tag().to_owned());
            }
            None => {
                state.proposal = Some(PendingProposal {
                    call: call.id.clone(),
                    from: from.clone(),
                    to: to.clone(),
                });
            }
        }
        moved = Some((from, to));
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
        from: moved.as_ref().and_then(|(from, _)| from.clone()),
        to: moved.map(|(_, to)| to),
    });
    for patch in lines {
        state.push(patch);
    }
    drop(state);
    shared.changed.notify_all();
    (line, Some(shown))
}

/// The id prefix of a mechanical entry (#784): `mechanical/<path>`.
const MECHANICAL_ID: &str = "mechanical/";

/// What tool call `line` touched, patched into working memory as it settles
/// (#784), when the regimen turns `mechanical_capture` on: no model call and
/// no fork. The mechanical lane reads the call as the shell would; each file
/// read, written or edited becomes one entry per path -- `files: <path>`,
/// id `mechanical/<path>` -- a later touch appending its provenance (lane
/// `mechanical`, the turn, the step as its index), and a delete retiring it;
/// a listed directory is `files: listed <dir>`. A path under the worktree is
/// written relative to it. The operator's entries are theirs (#150): the
/// guard refuses a patch on one. A path deleted and touched again stays
/// retired: the object refuses to restate a retired entry. The `patch`
/// lines to log, after the call's own.
fn mechanically_captured<S>(
    shared: &Shared<S>,
    state: &mut State,
    line: &ToolLine,
    (turn, request, step): (u32, u64, u32),
) -> Vec<Event> {
    use crate::capture::mechanical::{LANE, Lane, TouchKind};
    let on = state
        .interview
        .as_ref()
        .is_some_and(|interview| interview.mechanical_capture);
    let Some(tools) = shared.tools.as_ref().filter(|_| on) else {
        return Vec::new();
    };
    if line.outcome != log::ToolOutcome::Ran {
        return Vec::new();
    }
    let worktree = tools.worktree.clone();
    let reader = state
        .mechanical
        .get_or_insert_with(|| Lane::starting_in(worktree.clone()));
    let event = format!("r{request}/{}", line.id);
    reader.observe(&crate::formats::record::Event::ToolCall {
        id: event.clone(),
        at_turn: turn,
        tool: line.name.clone(),
        args: crate::formats::record::json::line(&line.arguments).ok(),
        exit: line.exit.and_then(|code| i64::try_from(code).ok()),
        output: line.stdout.as_ref().map(|out| out.text.clone()),
        exec: None,
        lane: None,
    });
    let mut touched = reader.touched_by(&event);
    touched.dedup();
    let tangent = state
        .tangent
        .as_ref()
        .map(|(tangent, _)| tangent.id().to_owned());
    let Some(interview) = state.interview.as_mut() else {
        return Vec::new();
    };
    let provenance = crate::object::Provenance {
        turn,
        lane: LANE.to_owned(),
        fork: None,
        tangent,
        index: step,
    };
    let mut patches = Vec::new();
    for (path, kind) in touched {
        let shown = match path.strip_prefix(&worktree) {
            Ok(under) if under.as_os_str().is_empty() => ".".to_owned(),
            Ok(under) => under.display().to_string(),
            Err(_) => path.display().to_string(),
        };
        let (id, content) = match kind {
            TouchKind::Listed => (
                format!("{MECHANICAL_ID}listed:{shown}"),
                format!("files: listed {shown}"),
            ),
            TouchKind::Read | TouchKind::Written | TouchKind::Deleted => {
                (format!("{MECHANICAL_ID}{shown}"), format!("files: {shown}"))
            }
        };
        // The path's live entry, under whichever id it was born: `<id>`,
        // or `<id>#2`, `#3` for a file deleted and touched again.
        let object = &interview.object;
        let held = object.live().find(|entry| entry.content == content);
        match (kind, held) {
            (TouchKind::Deleted, Some(entry)) => patches.push(Patch::Retire {
                target: entry.id.clone(),
                provenance: provenance.clone(),
            }),
            (TouchKind::Deleted, None) => {}
            (_, Some(entry)) => patches.push(Patch::Add {
                id: entry.id.clone(),
                content,
                provenance: provenance.clone(),
            }),
            (_, None) => {
                let fresh = (1..=u32::MAX)
                    .map(|n| {
                        if n == 1 {
                            id.clone()
                        } else {
                            format!("{id}#{n}")
                        }
                    })
                    .filter_map(|id| crate::object::EntryId::new(&id).ok())
                    .find(|id| object.entry(id).is_none());
                if let Some(id) = fresh {
                    patches.push(Patch::Add {
                        id,
                        content,
                        provenance: provenance.clone(),
                    });
                }
            }
        }
    }
    let (kept, mut lines) = guarded(&patches, &interview.object, None);
    for patch in &kept {
        if interview.object.apply(patch).is_ok() {
            lines.push(patched(None, patch, &interview.object));
        }
    }
    lines
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
/// (#609): that turn's ask, trunk prose and non-capture tool output as the
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
            // The operator's ask: the model saw it (#609, run 3).
            Event::Asked { turn: at, text, .. } => file(*at, text),
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
/// reference line kept for the meditation that replaces it -- and the answer
/// saying when. A name no call matches, a result already pruned, or one a
/// meditation already compacted away is answered with what is wrong, as a
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
    // On the trunk: a step of this turn's, or a result a meditation has not
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
    let text = crate::meditation::outputs::reference_line(&crate::meditation::outputs::Output {
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
    let meditation = state
        .interview
        .as_ref()
        .and_then(|interview| interview.prune)
        .unwrap_or_default();
    drop(state);
    shared.changed.notify_all();
    (line, Some(meditation.answer().to_owned()))
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
    // The phase tool (#788), whenever the session's phases are active.
    if declared
        && call.name == PHASE_TOOL
        && shared
            .lock()
            .interview
            .as_ref()
            .is_some_and(|interview| !interview.phases.is_empty())
    {
        return phase_call(shared, (turn, request), call);
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
            let shown = shown_ran(tools, "", &ran, &mut line);
            (line, Some(shown))
        }
        Err(not_run) => never_ran(tools, line, &run, &not_run),
    }
}

/// Where the session's background jobs write (#614): the recording's
/// `background/`, or, with no recording, a directory of the session's own
/// under the system's temporary one.
fn background_dir<S>(shared: &Shared<S>, state: &State) -> std::path::PathBuf {
    let opened = match state.log.first() {
        Some(Logged {
            event: Event::Started { opened, .. },
            ..
        }) => *opened,
        _ => 0,
    };
    // The recording's `background`, when it is one serve made (#735): a
    // symlink planted there would steer the job's files; the session's own
    // temporary directory otherwise.
    shared
        .tools
        .as_ref()
        .and_then(|tools| tools.recording.as_ref())
        .and_then(|recording| super::attach::own_dir(recording, "background").ok())
        .unwrap_or_else(|| {
            let dir = temporary_background(opened);
            let _ = std::fs::create_dir_all(&dir);
            dir
        })
}

/// The session's own directory for background jobs' files when it keeps no
/// recording (#614), under the system's temporary directory.
fn temporary_background(opened: u64) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("diet-background-{opened}-{}", std::process::id()))
}

/// `tools` with the directories serve points the model to readable under
/// its sandbox (#735): the recording's, where a capped output is kept whole
/// and background jobs write; or, with background commands on and no
/// recording, the session's temporary one, where they write. A sandbox
/// listing what it reads otherwise refuses the very `sed` and `cat` the
/// model is told to run.
fn readable_outputs(mut tools: Tools, opened: u64) -> Tools {
    if matches!(tools.confinement, crate::isolation::Confinement::Unconfined) {
        return tools;
    }
    let temporary = (tools.background && tools.recording.is_none()).then(|| {
        let dir = temporary_background(opened);
        let _ = std::fs::create_dir_all(&dir);
        dir
    });
    let dirs = tools.recording.iter().cloned().chain(temporary);
    for dir in dirs {
        let dir = dir.to_string_lossy().into_owned();
        if !tools.policy.readable.contains(&dir) {
            tools.policy.readable.push(dir);
        }
    }
    tools
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
    // Made new (#735): a name planted there is refused, not followed.
    let spawned = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output)
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
                listed: None,
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
    let shown = if ran.as_the_model_sees_it().is_empty() {
        shown_capped(
            tools,
            &format!(
                "Command timed out after {ms}ms before it could complete. There was no output \
                 before it timed out."
            ),
            &mut line,
        )
    } else {
        shown_ran(
            tools,
            &format!(
                "Command timed out after {ms}ms before it could complete. Below is the output \
                 before it timed out:\n"
            ),
            ran,
            &mut line,
        )
    };
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

/// Refill the trunk from working memory for a meditation `reason` fired (#493):
/// the head with the working object rendered after it, and no turn of the
/// old trunk. The caller has checked the session is awaiting and working
/// memory holds an entry. A cadence then counts from here, and a budget
/// waits for the next trunk call to measure the refilled trunk.
fn refill_trunk(
    template: &RequestShape,
    state: &mut State,
    meditation: (crate::meditation::Reason, Option<log::MeditationFired>),
    to: Option<String>,
) {
    let meditated = refilled(template, state, meditation, to);
    state.push(meditated);
}

/// [`refill_trunk`]'s refill, its line returned rather than logged, so a
/// pre-warm's timings can join it (#504).
#[allow(clippy::too_many_lines)]
fn refilled(
    template: &RequestShape,
    state: &mut State,
    (reason, fired): (crate::meditation::Reason, Option<log::MeditationFired>),
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
    // The phase's growth before this meditation replaces the trunk (#788).
    let before = trunk_now(state, template);
    state.phase_grown += before.saturating_sub(state.phase_floor);
    let Some(interview) = state.interview.as_ref() else {
        unreachable!("a meditation is refused or not due when the session keeps no working memory");
    };
    // The render, under the regimen's budget when it declares one (#565).
    let budget = interview.meditations.render_budget;
    let rendered = marked_render(state, interview, budget);
    let render_budget = budget.map(|budget| log::RenderBudget {
        tokens: budget.tokens,
        over: match budget.over {
            crate::meditation::render::OverBudget::Tier => "tier",
            crate::meditation::render::OverBudget::Elide => "elide",
        }
        .to_owned(),
        rendered: rendered.tokens,
        reduced: rendered.reduced,
        fits: Some(rendered.fits),
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
    let tail_tokens = interview.meditations.tail_tokens;
    let turns = state
        .trunk
        .get(template.messages.len() + usize::from(state.refilled)..)
        .unwrap_or_default();
    let on_trunk = prunes_on(&state.pruned, turns);
    let kept = crate::meditation::render::tail(turns, tail_tokens).to_vec();
    let (carried_turns, newest_turn_tokens) = tail_kept(&kept, turns, tail_tokens);
    let carried_tokens = kept
        .iter()
        .map(crate::meditation::render::estimated_tokens)
        .sum::<u64>();
    // What the refill carries of the outputs it compacts away (#553): those
    // before the kept tail, which stays as it sat.
    let tool_outputs = interview.meditations.outputs;
    let compacted = &turns[..turns.len() - kept.len()];
    let mut kept = kept;
    replaced_in_tail(&mut kept, &state.pruned);
    // The archive (#566): what this meditation drops from the trunk, and every
    // working-memory entry no longer live, for a recall to find later.
    let earlier = meditations_at(&state.log, state.turns);
    let archived = archived_at_meditation(compacted, &interview.object, (state.turns, earlier));
    let outputs = crate::meditation::outputs::section(
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
    // Each background command still running (#614), after the summary in
    // the refill's message, as Qwen Code places its list: the refill
    // compacts away the call that started it.
    let background = super::background::running_at_meditation(state.jobs.values_mut());
    let (head, phase_document) = state.phase_head(&template.messages);
    let mut refilled = crate::meditation::render::refill_with(&head, &sent, background.as_deref());
    state.refilled = true;
    refilled.extend(kept);
    let prefix_hash_after = digest(&refilled);
    state.trunk = refilled;
    // The refill is where the trunk's growth counts from again (#788), and
    // a move made by this meditation enters its phase here.
    state.trunk_tokens = None;
    let after = trunk_now(state, template);
    state.meditation_floor = after;
    state.phase_floor = after;
    if phase.is_some() {
        state.phase_entered = state.log.len() as u64;
        state.phase_grown = 0;
    }
    for item in archived {
        state.archive.push(item);
    }
    state.turns_at_meditation = state.turns;
    state.trunk_tokens = None;
    let pruned = prunes_applied(state, &on_trunk);
    let (operator_changes, unaddressed) = operator_carried(state);
    let at_turn = state.turns;
    Event::Meditated {
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
        phase_document,
        tool_outputs,
        outputs,
        background,
        render_budget,
        fired,
        pruned,
        operator_changes,
        unaddressed,
        newest_turn_tokens,
        refilled_anyway: None,
        warm: None,
    }
}

/// How many whole turns the tail `kept` of `turns` holds; and, when a tail
/// was asked for (`tail_tokens` above 0) and kept none (#693), the newest
/// turn's estimated tokens, for the operator's warning -- "you got what you
/// asked for".
fn tail_kept(kept: &[Message], turns: &[Message], tail_tokens: u64) -> (u64, Option<u64>) {
    let carried_turns = kept
        .iter()
        .filter(|message| message.role == Role::User)
        .count() as u64;
    let unkept = (tail_tokens > 0 && carried_turns == 0)
        .then(|| crate::meditation::render::newest_turn_tokens(turns))
        .flatten();
    (carried_turns, unkept)
}

impl Event {
    /// A fork's settle with nothing beside its outcome: no cost, no
    /// refusal, no trailer.
    fn settled(fork: u64, outcome: log::ForkOutcome) -> Self {
        Self::ForkSettled {
            fork,
            outcome,
            prompt_tokens: None,
            wall_ms: None,
            refused: None,
            trailer: None,
            pool: None,
            not_run: None,
        }
    }
}

/// [`Refusal::Restates`] when the operator's edit of `id` to `content`
/// is one working memory refuses as no correction (#729): `content` is the
/// entry's own text, or another live entry's. Tried on a copy, so nothing is
/// applied.
fn restates(state: &State, id: &str, content: &str) -> Option<Refusal> {
    let interview = state.interview.as_ref()?;
    let voids = crate::object::EntryId::new(id).ok()?;
    let new = crate::object::EntryId::new(&format!("operator-{}", state.log.len())).ok()?;
    let mut trial = interview.object.clone();
    trial
        .apply(&Patch::Supersede {
            id: new,
            content: content.to_owned(),
            voids,
            provenance: crate::object::Provenance {
                turn: state.turns,
                lane: crate::object::OPERATOR.to_owned(),
                fork: None,
                tangent: None,
                index: 0,
            },
        })
        .is_err()
        .then_some(Refusal::Restates)
}

/// Why an operator edit or flag of `id` with `text` is refused, if it is.
fn operator_refusal(state: &State, id: &str, text: &str) -> Option<Refusal> {
    if state.settlement == Settlement::Ended {
        return Some(Refusal::Ended);
    }
    if matches!(state.settlement, Settlement::Turn | Settlement::Capture) {
        return Some(Refusal::InFlight);
    }
    if text.trim().is_empty() {
        return Some(Refusal::Empty);
    }
    let entry = state.interview.as_ref().and_then(|interview| {
        crate::object::EntryId::new(id).ok().and_then(|id| {
            interview
                .object
                .entry(&id)
                .map(|entry| entry.state.is_live())
        })
    });
    match entry {
        None => Some(Refusal::UnknownEntry),
        Some(false) => Some(Refusal::NotLive),
        Some(true) => None,
    }
}

/// The entries `lines`' `patch` lines wrote or ruled on, by id.
fn written_by(lines: &[Event]) -> Vec<String> {
    lines
        .iter()
        .filter_map(|line| match line {
            Event::Patched { entry, .. } => Some(entry.id.clone()),
            _ => None,
        })
        .collect()
}

/// Why a capture none of whose patches was kept was refused (#689): the
/// first `patch.refused` line's reason and the entry it would have changed,
/// `operator-entry: <id>`.
fn operator_refused(refusals: &[Event]) -> Option<String> {
    refusals.iter().find_map(|line| match line {
        Event::PatchRefused { entry, .. } => Some(format!("{OPERATOR_ENTRY}: {entry}")),
        _ => None,
    })
}

/// Why a model's patch is refused (#150): its target is the operator's.
const OPERATOR_ENTRY: &str = "operator-entry";

/// `interview`'s working memory rendered for a refill under `budget`, the
/// operator's changes the meditation carries marked (#150).
fn marked_render(
    state: &State,
    interview: &Interview,
    budget: Option<crate::meditation::render::Budget>,
) -> crate::meditation::render::Rendered {
    crate::meditation::render::rendered_marked(
        &interview.object,
        state.phase.as_deref(),
        budget,
        &operator_marks(&state.operator_pending),
    )
}

/// An operator change a meditation has yet to address (#150).
#[derive(Debug, Clone)]
struct Pending {
    /// The entry it changed: the edit's new entry, or the flagged one.
    entry: String,
    /// `edit` or `flag`.
    kind: &'static str,
    /// A flag's note.
    note: Option<String>,
}

/// The markers the operator's pending changes put on their notes (#150):
/// [`crate::meditation::render::EDITED`] on an edit's entry, and
/// [`crate::meditation::render::flagged`] for each flag, in order.
fn operator_marks(pending: &[Pending]) -> crate::meditation::render::Marks {
    let mut marks = crate::meditation::render::Marks::new();
    for change in pending {
        let Ok(id) = crate::object::EntryId::new(&change.entry) else {
            continue;
        };
        let marker = change.note.as_deref().map_or_else(
            || crate::meditation::render::EDITED.to_owned(),
            crate::meditation::render::flagged,
        );
        marks.entry(id).or_default().push_str(&marker);
    }
    marks
}

/// The operator's changes a meditation carries (#150) -- each still live, or
/// addressed by this meditation's audit -- and the ones no answer addressed,
/// which stay pending for the next meditation. A change is addressed when the
/// meditation's audit acted on it ([`acted_on`], #748).
fn operator_carried(state: &mut State) -> (Option<Vec<log::OperatorChange>>, Option<Vec<String>>) {
    let live = |id: &str| {
        state.interview.as_ref().is_some_and(|interview| {
            crate::object::EntryId::new(id)
                .ok()
                .and_then(|id| interview.object.entry(&id))
                .is_some_and(|entry| entry.state.is_live())
        })
    };
    let addressed = std::mem::take(&mut state.operator_addressed);
    let carried: Vec<Pending> = state
        .operator_pending
        .iter()
        .filter(|change| addressed.contains(&change.entry) || live(&change.entry))
        .cloned()
        .collect();
    let unaddressed: Vec<Pending> = carried
        .iter()
        .filter(|change| !addressed.contains(&change.entry))
        .cloned()
        .collect();
    state.operator_pending.clone_from(&unaddressed);
    if carried.is_empty() {
        return (None, None);
    }
    let changes = carried
        .iter()
        .map(|change| log::OperatorChange {
            entry: change.entry.clone(),
            kind: change.kind.to_owned(),
        })
        .collect();
    let mut left: Vec<String> = Vec::new();
    for change in unaddressed {
        if !left.contains(&change.entry) {
            left.push(change.entry);
        }
    }
    (Some(changes), Some(left))
}

/// `patches`, less each one a model lane made that targets an entry the
/// operator wrote (#150), with the refusal each of those is logged as.
fn guarded(
    patches: &[Patch],
    object: &WorkingObject,
    fork: Option<u64>,
) -> (Vec<Patch>, Vec<Event>) {
    let mut kept = Vec::new();
    let mut refused = Vec::new();
    for patch in patches {
        let (op, target) = match patch {
            Patch::Add { .. } => (log::PatchOp::Add, None),
            Patch::Supersede { voids, .. } => (log::PatchOp::Supersede, Some(voids)),
            Patch::Resolve { target, .. } => (log::PatchOp::Resolve, Some(target)),
            Patch::Retire { target, .. } => (log::PatchOp::Retire, Some(target)),
            Patch::Park { target, .. } => (log::PatchOp::Park, Some(target)),
        };
        let theirs = patch.provenance().lane != crate::object::OPERATOR;
        match target {
            Some(target) if theirs && object.by_operator(target) => {
                refused.push(Event::PatchRefused {
                    fork,
                    lane: fork.is_none().then(|| patch.provenance().lane.clone()),
                    op,
                    entry: target.as_str().to_owned(),
                });
            }
            _ => kept.push(patch.clone()),
        }
    }
    (kept, refused)
}

/// A meditation `reason` fired (#504): refilled now; or, when the regimen asks
/// for its audit (and the dogma pins one for `reason`) or its pre-warm,
/// queued for [`meditation_work`], which makes those calls on a thread that can
/// wait for them, the session in `capture` until it is done. `true` when it
/// was queued.
fn meditation(
    template: &RequestShape,
    state: &mut State,
    reason: crate::meditation::Reason,
    to: Option<String>,
) -> bool {
    let (audit, warm) = state
        .interview
        .as_ref()
        .map_or((false, false), |interview| {
            (
                interview.meditations.audit && crate::meditation::pinned_ask(reason).is_some(),
                interview.meditations.warm && reason != crate::meditation::Reason::Window,
            )
        });
    if !audit && !warm {
        refill_trunk(template, state, (reason, None), to);
        return false;
    }
    // A meditation declared again over a held one replaces it (#150's gate).
    state.held = None;
    state.meditation_pending = Some(PendingMeditation {
        reason,
        to,
        anyway: false,
    });
    state.move_to(Settlement::Capture);
    true
}

/// A meditation queued by [`meditation`] (#504), done: its audit, when the regimen asks
/// for one and there is a settled `final` turn to cut it from -- a side
/// call off the warm trunk, the dogma's pinned ask after it, its answer read
/// by `diet::formats::audit` and folded into working memory, logged as a
/// fork on the `audit` lane; then the refill; then its pre-warm, when asked
/// for -- the refilled trunk sent once with an output cap of one, which an
/// ask cancels, its own request warming the same prefix; then the meditation's
/// line, with the warm's timings when it finished; then back to `awaiting`.
/// Nothing when nothing is queued.
fn meditation_work<S: Streaming>(shared: &Shared<S>) {
    let mut state = shared.lock();
    let Some(PendingMeditation { reason, to, anyway }) = state.meditation_pending.take() else {
        return;
    };
    state.meditating = Some(reason);
    if !anyway {
        state = audit_and_gate(shared, state, reason);
        if let Some(left) = left_unaddressed(&state)
            .filter(|_| refusing(&state) && crate::meditation::pinned_ask(reason).is_some())
        {
            // Held for the operator (#150's gate): no refill, and the line
            // naming what is left, under the lock that moves to `awaiting`.
            state.held = Some(PendingMeditation {
                reason,
                to,
                anyway: false,
            });
            state.meditating = None;
            state.move_to(Settlement::Awaiting);
            let at_turn = state.turns;
            state.push(Event::MeditationHeld {
                at_turn,
                reason,
                unaddressed: left,
            });
            if state.ending {
                state.ending = false;
                state.move_to(Settlement::Ended);
            }
            drop(state);
            shared.changed.notify_all();
            return;
        }
    }
    let mut meditated = refilled(&shared.template, &mut state, (reason, None), to);
    if let Event::Meditated {
        refilled_anyway, ..
    } = &mut meditated
    {
        *refilled_anyway = anyway.then_some(true);
    }
    let warm = state
        .interview
        .as_ref()
        .is_some_and(|interview| interview.meditations.warm)
        && reason != crate::meditation::Reason::Window;
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
        if let (Ok(StreamEnded::Finished { timings, .. }), Event::Meditated { warm, .. }) =
            (ended, &mut meditated)
        {
            *warm = timings;
        }
    }
    // Back to `awaiting`, then the meditation's line, under one lock: a meditation is
    // logged while the session awaits, and nothing comes between.
    state.meditating = None;
    state.move_to(Settlement::Awaiting);
    state.push(meditated);
    if state.ending {
        state.ending = false;
        state.move_to(Settlement::Ended);
    }
    drop(state);
    shared.changed.notify_all();
}

/// A meditation's audit, made and folded (#504); then, under `reconcile_gate =
/// "refuse"` with an operator change still unaddressed (#150), the same
/// reconciling ask once more over only the notes whose changes are left.
fn audit_and_gate<'a, S: Streaming>(
    shared: &'a Shared<S>,
    mut state: std::sync::MutexGuard<'a, State>,
    reason: crate::meditation::Reason,
) -> std::sync::MutexGuard<'a, State> {
    if let Some(fired) = audit_fired(&shared.template, &mut state, reason, None) {
        drop(state);
        shared.changed.notify_all();
        audited(shared, fired);
        state = shared.lock();
    }
    let Some(left) = left_unaddressed(&state).filter(|_| refusing(&state)) else {
        return state;
    };
    if let Some(fired) = audit_fired(&shared.template, &mut state, reason, Some(&left)) {
        drop(state);
        shared.changed.notify_all();
        audited(shared, fired);
        state = shared.lock();
    }
    state
}

/// Whether the session's regimen refuses to refill over an unaddressed
/// operator change (#150's gate).
fn refusing(state: &State) -> bool {
    state.interview.as_ref().is_some_and(|interview| {
        interview.meditations.reconcile_gate == crate::meditation::policy::ReconcileGate::Refuse
    })
}

/// The entries of the operator's pending changes, still live, that no
/// audit at this meditation addressed (#150), in order; `None` when there are none.
fn left_unaddressed(state: &State) -> Option<Vec<String>> {
    let live = |id: &str| {
        state.interview.as_ref().is_some_and(|interview| {
            crate::object::EntryId::new(id)
                .ok()
                .and_then(|id| interview.object.entry(&id))
                .is_some_and(|entry| entry.state.is_live())
        })
    };
    let mut left: Vec<String> = Vec::new();
    for change in &state.operator_pending {
        if live(&change.entry)
            && !state.operator_addressed.contains(&change.entry)
            && !left.contains(&change.entry)
        {
            left.push(change.entry.clone());
        }
    }
    (!left.is_empty()).then_some(left)
}

/// A meditation's audit, fired (#504): its fork line and its request, when the
/// regimen asks for one, the dogma pins an ask for `reason`, working memory
/// holds an entry, and the latest turn settled `final` -- the audit is cut
/// from that turn's warm trunk, as a fork is.
fn audit_fired(
    template: &RequestShape,
    state: &mut State,
    reason: crate::meditation::Reason,
    only: Option<&[String]>,
) -> Option<AuditFired> {
    let interview = state
        .interview
        .as_ref()
        .filter(|interview| interview.meditations.audit)?;
    // The gate's re-ask (#150) puts the same ask over only the notes whose
    // changes are left: `only`.
    let items: Vec<(crate::object::EntryId, String)> = interview
        .object
        .live()
        .filter(|entry| only.is_none_or(|only| only.iter().any(|id| id == entry.id.as_str())))
        .map(|entry| (entry.id.clone(), entry.content.clone()))
        .collect();
    // #610: a session whose forks answer through the record tools audits
    // through them too, in the tools-route set. That set's words carry no
    // markers yet, so it marks nothing (#150), though its patches address
    // (#748); the lines route marks the operator's pending changes and, with any, asks
    // the reconciling ask.
    let tools = interview.capture == crate::dogma::asks::Modality::Tools;
    let marks = if tools {
        crate::meditation::render::Marks::new()
    } else {
        operator_marks(&state.operator_pending)
    };
    let ask = if tools {
        crate::meditation::audit_ask_tools(reason, &items, crate::dogma::audits::DEFAULT)?
    } else {
        crate::meditation::audit_ask_marked(reason, &items, &marks)?
    };
    // The notes with a pending change, on either route (#748): the tools
    // route's words carry no markers, but its patches can still act on them.
    let marked = ask
        .items
        .iter()
        .filter(|id| {
            state
                .operator_pending
                .iter()
                .any(|change| change.entry == id.as_str())
        })
        .map(|id| id.as_str().to_owned())
        .collect();
    // The latest turn that settled `final` (#729): one that failed since
    // has no answered request to cut from, and under `refuse` a meditation
    // that could ask nothing would hold having asked nothing.
    let turn = state
        .log
        .iter()
        .rev()
        .find_map(|logged| match &logged.event {
            Event::TurnSettled {
                turn: settled,
                reason: SettleReason::Final,
            } => Some(*settled),
            _ => None,
        })?;
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
    // A meditation since that request (#698's run): the audit is cut from its
    // refill, the trunk the session holds now, and says so.
    let after_meditation = state
        .log
        .iter()
        .rev()
        .take_while(|logged| logged.seq > at)
        .find_map(|logged| matches!(logged.event, Event::Meditated { .. }).then_some(logged.seq));
    let fork = state.push(Event::Audited {
        of_turn: turn,
        at,
        template: ask.template,
        question: ask.text.clone(),
        after_meditation,
    });
    let max_tokens = state.sized(&mut shape, template.limits.max_output_tokens);
    let request = state.push(Event::Requested {
        turn,
        lane: Lane::Audit,
        head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
        fork: Some(fork),
        max_tokens,
        beside: None,
    });
    let cancel = Cancel::new();
    state.flight = Some(Flight {
        turn,
        request,
        cancel: cancel.clone(),
    });
    Some(AuditFired {
        turn,
        fork,
        request,
        shape,
        cancel,
        items: ask.items,
        marked,
        tools,
    })
}

/// A side call's stream (#504): its text, reasoning and progress logged
/// as they arrive, naming its request; a tool call it makes is only noted,
/// unless `tools`, when its pieces are logged and gathered for the record
/// tools (#610).
fn aside<S: Streaming>(
    shared: &Shared<S>,
    shape: &RequestShape,
    cancel: &Cancel,
    (request, tools): (u64, bool),
) -> Aside {
    let deadline = Instant::now() + shared.template.limits.call;
    let mut partial = String::new();
    let mut reasoning = String::new();
    let mut called = false;
    let mut calls = Calls::default();
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
    Aside {
        result,
        partial,
        reasoning,
        called,
        calls: calls.into_calls(),
        hosted,
    }
}

/// What [`aside`] streamed.
struct Aside {
    result: Result<StreamEnded, TransportFailure>,
    partial: String,
    reasoning: String,
    called: bool,
    /// The record tools' calls, when it was asked to answer through them.
    calls: Vec<Call>,
    /// What a hosted API said beside the answer (#555).
    hosted: Hosted,
}

/// A meditation's audit call, made and settled (#504): its pieces streamed into
/// the log on the `audit` lane; on an answer, read by `diet::formats::audit`
/// against the ask's numbering -- one line per note, 1..n -- and folded:
/// `UPDATE` supersedes the note, `REMOVE` and a dup retire it, `KEEP`
/// leaves it. `value` when it changed something, `decline` when it kept
/// everything, `unparseable` when the answer is not the ask's audit (or the
/// call made a tool call); a cut, stopped or failed call folds nothing.
fn audited<S: Streaming>(shared: &Shared<S>, fired: AuditFired) {
    let AuditFired {
        turn,
        fork,
        request,
        shape,
        cancel,
        items,
        marked,
        tools,
    } = fired;
    let Aside {
        result,
        partial,
        reasoning,
        called,
        calls,
        hosted,
    } = aside(shared, &shape, &cancel, (request, tools));
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
            } else if tools {
                // #610: each record-tool call run as a fork's is, its
                // patches the audit's (lane `audit`).
                let (outcome, patched) =
                    captured_in(&mut state, &calls, (turn, request), (fork, Lane::Audit));
                if outcome != log::ForkOutcome::Unparseable {
                    acted_on(&mut state, &marked, &patched);
                }
                lines = patched;
                outcome
            } else if called {
                log::ForkOutcome::Unparseable
            } else {
                let (outcome, patched) = audit_folded(&mut state, &text, &items, fork);
                // Parsed: every marked note had its line (#150), and the
                // answer's patches say which it acted on (#748).
                if outcome != log::ForkOutcome::Unparseable {
                    acted_on(&mut state, &marked, &patched);
                }
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
    state.push(Event::settled(fork, outcome));
    for line in lines {
        state.push(line);
    }
    drop(state);
    shared.changed.notify_all();
}

/// The marked entries an audit acted on (#748, log v8), added to the ones
/// this meditation addressed: a flag is addressed when one of the audit's
/// patches `lines` superseded, retired, resolved or parked its entry -- on the
/// lines route an `UPDATE` or a `REMOVE`, never a `KEEP` -- and an edit
/// unless a patch over it was refused -- on the lines route its `KEEP`.
/// An entry is addressed when each of its changes is.
fn acted_on(state: &mut State, marked: &[String], lines: &[Event]) {
    let settled = |entry: &str| settles(lines, entry);
    let refused = |entry: &str| {
        lines
            .iter()
            .any(|line| matches!(line, Event::PatchRefused { entry: over, .. } if over == entry))
    };
    for entry in marked {
        let addressed = state
            .operator_pending
            .iter()
            .filter(|change| change.entry == *entry)
            .all(|change| {
                if change.kind == "flag" {
                    settled(entry)
                } else {
                    !refused(entry)
                }
            });
        if addressed && !state.operator_addressed.contains(entry) {
            state.operator_addressed.push(entry.clone());
        }
    }
}

/// Whether one of `lines` took `entry` out of live memory (#748): a patch
/// that superseded, retired, resolved or parked it.
fn settles(lines: &[Event], entry: &str) -> bool {
    lines.iter().any(|line| match line {
        Event::Patched {
            op: log::PatchOp::Supersede,
            supersedes: Some(voids),
            ..
        } => voids == entry,
        Event::Patched {
            op: log::PatchOp::Retire | log::PatchOp::Resolve | log::PatchOp::Park,
            entry: patched,
            ..
        } => patched.id == entry,
        _ => false,
    })
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

/// The automatic meditation (#617), checked before every trunk request on
/// `shape`: when serve knows the window, the regimen has not turned it off
/// ([`crate::meditation::policy::MEDITATION_WINDOW`]), no tangent is open, working
/// memory holds an entry and the trunk holds turns the refill would drop,
/// and the prompt as sized (#588) would leave less than
/// [`crate::meditation::policy::WINDOW_RESERVE`] -- or the output cap, if more --
/// of the window: the trunk is refilled, and the turn's own messages so far
/// ride after the refill as they rode after the trunk. Otherwise the request
/// goes as it is, as Pi, `OpenCode` 2 and Qwen Code all send it when there is
/// nothing to compact.
fn window_meditation(
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
    if interview.meditations.window_off
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
    if crate::meditation::render::tail(turns, interview.meditations.tail_tokens).len()
        == turns.len()
    {
        return false;
    }
    let prompt = state.prompt_of(shape);
    let reserve =
        u64::from(template.limits.max_output_tokens).max(crate::meditation::policy::WINDOW_RESERVE);
    if !forced && prompt <= window.saturating_sub(reserve) {
        return false;
    }
    let own = shape.messages.split_off(state.trunk.len());
    refill_trunk(
        template,
        state,
        (
            crate::meditation::Reason::Window,
            Some(log::MeditationFired {
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
/// meditation once more and the request again (#617), as Pi, `OpenCode` 2 and Qwen
/// Code each compact and retry once after an overflow: the meditation forced,
/// whatever the sizing said, and the same messages sent on the refilled
/// trunk. Once per turn; and not when the meditation has nothing to refill from
/// or nothing to drop, when the turn fails as it would have. The next
/// request's sequence number when it was sent.
fn retried(
    template: &RequestShape,
    state: &mut State,
    shape: &mut RequestShape,
    (turn, cancel): (u32, &Cancel),
) -> Option<u64> {
    if state.overflow_retried == Some(turn) || !window_meditation(template, state, shape, true) {
        return None;
    }
    state.overflow_retried = Some(turn);
    let max_tokens = state.sized(shape, template.limits.max_output_tokens);
    let beside = state.trunk_beside();
    let next = state.push(Event::Requested {
        turn,
        lane: Lane::Trunk,
        head_sha256: crate::client::head::Head::of(shape).digest().to_owned(),
        fork: None,
        max_tokens,
        beside,
    });
    // In flight again (#728): the failed request cleared it, and a cancel,
    // an end or a drop must reach the retry as it reaches any request.
    state.flight = Some(Flight {
        turn,
        request: next,
        cancel: cancel.clone(),
    });
    Some(next)
}

/// Each pruned result in a meditation's kept tail replaced in place by its line
/// (#612): the tool message stays, answering its call.
fn replaced_in_tail(kept: &mut [Message], pruned: &[Prune]) {
    for message in kept {
        if let Some(prune) = pruned.iter().find(|prune| prune.is(message)) {
            message.content.clone_from(&prune.text);
            message.images.clear();
        }
    }
}

/// The calls of `pruned` whose result is in `turns`, the trunk a meditation
/// replaces (#728): one made in a turn a window meditation interrupts is in
/// that turn's own messages, which ride after the refill unreplaced, and
/// stays due.
fn prunes_on(pruned: &[Prune], turns: &[Message]) -> Vec<String> {
    pruned
        .iter()
        .filter(|prune| turns.iter().any(|message| prune.is(message)))
        .map(|prune| prune.call.clone())
        .collect()
}

/// How many meditations `log` holds at turn `turn` (#728): a window one
/// under the turn, then the operator's after it, archive under keys of
/// their own.
fn meditations_at(log: &[Logged], turn: u32) -> usize {
    log.iter()
        .filter(
            |logged| matches!(&logged.event, Event::Meditated { at_turn, .. } if *at_turn == turn),
        )
        .count()
}

/// The prunes a meditation applies -- every one since the last (#612) whose
/// result was on the trunk it replaced, `on_trunk` (#728) -- marked applied,
/// so none is due any more; `None` when there were none.
fn prunes_applied(state: &mut State, on_trunk: &[String]) -> Option<Vec<String>> {
    let applied: Vec<String> = state
        .pruned
        .iter_mut()
        .filter(|prune| !prune.applied && on_trunk.contains(&prune.call))
        .map(|prune| {
            prune.applied = true;
            prune.call.clone()
        })
        .collect();
    (!applied.is_empty()).then_some(applied)
}

/// What a meditation at `at_turn` archives (#566): each message of `compacted`,
/// keyed by the meditation and its position, and each entry of `object` that is
/// no longer live, keyed by its id. A meditation after `earlier` others at the
/// same turn keys its messages `compaction-<turn>.<earlier>` (#728): the
/// archive keeps one item per key, and the second's would otherwise collide.
fn archived_at_meditation(
    compacted: &[Message],
    object: &WorkingObject,
    (at_turn, earlier): (u32, usize),
) -> Vec<super::archive::Item> {
    use super::archive::Item;
    let meditation = if earlier == 0 {
        at_turn.to_string()
    } else {
        format!("{at_turn}.{earlier}")
    };
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
            // The model reads it: `compaction`, the word it was trained on,
            // the maintainer's choice (#583). Older logs keep `seam-`.
            key: format!("compaction-{meditation}/message-{}", at + 1),
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

/// The outputs in `compacted` -- the trunk's turns a meditation compacts away --
/// in call order, each with its call, its turn and what `state` needs of
/// it (#553): its whole saved by digest for `reference`, the read fork's
/// excerpts quoted from it for `salient`. Each result is matched to its
/// `tool_call` line in log order, by call id and what it was shown.
fn compacted_outputs(
    log: &[Logged],
    compacted: &[Message],
    state: log::MeditationToolOutputs,
    (recording, pruned): (Option<&std::path::Path>, &[Prune]),
) -> Vec<crate::meditation::outputs::Output> {
    use crate::meditation::outputs::Output;
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
        // earlier meditation already replaced in the tail has no line to match.
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
    if state == log::MeditationToolOutputs::Reference {
        for (output, file) in outputs
            .iter_mut()
            .zip(files)
            .filter(|(output, _)| output.pruned.is_none())
        {
            output.saved = Some(saved_whole(&output.shown, file, recording));
        }
    }
    if state == log::MeditationToolOutputs::Salient {
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
) -> crate::meditation::outputs::Saved {
    let file = file.or_else(|| {
        recording
            .and_then(|dir| super::attach::kept_whole(dir, shown.as_bytes(), "text/plain").ok())
    });
    match (file, recording) {
        (Some(file), Some(dir)) => crate::meditation::outputs::Saved {
            bytes: file.bytes,
            sha256: file.sha256,
            path: Some(dir.join(&file.path).to_string_lossy().into_owned()),
        },
        _ => crate::meditation::outputs::Saved {
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
/// awaiting, the meditation the regimen's cadence or budget makes due, if working
/// memory holds an entry to refill from. Not due with nothing to refill
/// from: it stays due, and fires after the next turn that leaves an entry.
fn turn_over(template: &RequestShape, state: &mut State) {
    // The turn ends once the last fork cut during it settles (#782).
    if state.pool_busy() {
        state.deferred = Some(Deferred::TurnOver);
        if state.settlement == Settlement::Turn {
            state.move_to(Settlement::Capture);
        }
        return;
    }
    state.after_the_turn();
    if state.settlement != Settlement::Awaiting {
        return;
    }
    // A move the model accepted to meditate into (#788), now the turn is
    // over; with nothing to refill from, the move is kept.
    if let Some(to) = state.move_after_turn.take() {
        if !meditation(
            template,
            state,
            crate::meditation::Reason::Phase,
            Some(to.clone()),
        ) {
            moved(template, state, &to, PhaseChoice::Keep);
        }
        return;
    }
    // No derived meditation while a tangent is open (#22): its close restores the
    // fork point's trunk, which a refill would have replaced.
    let due = state
        .interview
        .as_ref()
        .filter(|_| state.tangent.is_none())
        .and_then(|interview| {
            interview.object.live().next()?;
            interview
                .meditations
                .due(state.turns - state.turns_at_meditation, state.trunk_tokens)
                // A prune applied as its turn settles (#612): deferred, as
                // any derived meditation is, while it cannot fire.
                .or_else(|| {
                    (state.pruned.iter().any(|prune| !prune.applied)
                        && interview.prune == Some(super::prune::PruneMeditation::TurnEnd))
                    .then_some(crate::meditation::Reason::Prune)
                })
        });
    if let Some(reason) = due {
        meditation(template, state, reason, None);
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

/// A count as `GET /state` writes it (#756).
fn status_number(n: u64) -> Value {
    Value::Integer(i64::try_from(n).unwrap_or(i64::MAX))
}

/// Text as `GET /state` writes it (#756).
fn status_text(s: &str) -> Value {
    Value::String(s.to_owned())
}

/// An object of `pairs`, as `GET /state` writes it (#756).
fn status_object(pairs: Vec<(&str, Value)>) -> Value {
    Value::Object(
        pairs
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

/// What `GET /state` reads off the log (#756).
struct LogRead<'a> {
    /// The forks and audits with no `fork.settled` yet: each line's `seq`,
    /// and whether it is an audit.
    open: Vec<(u64, bool)>,
    /// How the latest turn settled, once it has.
    settled: Option<SettleReason>,
    /// The last refusal: its `seq`, command and reason.
    refusal: Option<(u64, CommandKind, Refusal)>,
    /// What the latest hold left unaddressed.
    unaddressed: Option<&'a Vec<String>>,
}

impl<'a> LogRead<'a> {
    fn of(state: &'a State) -> Self {
        let mut read = LogRead {
            open: Vec::new(),
            settled: None,
            refusal: None,
            unaddressed: None,
        };
        for logged in &state.log {
            match &logged.event {
                Event::Forked { .. } => read.open.push((logged.seq, false)),
                Event::Audited { .. } => read.open.push((logged.seq, true)),
                Event::ForkSettled { fork, .. } => read.open.retain(|(seq, _)| seq != fork),
                Event::TurnSettled { turn, reason } if *turn == state.turns => {
                    read.settled = Some(*reason);
                }
                Event::Refused {
                    command, because, ..
                } => read.refusal = Some((logged.seq, *command, *because)),
                Event::MeditationHeld { unaddressed, .. } => read.unaddressed = Some(unaddressed),
                _ => {}
            }
        }
        read
    }
}

/// A fork cut during the turn, in flight (#782).
struct PoolFork {
    /// Its fork line's `seq`.
    fork: u64,
    /// Its reservation, in tokens: what it holds of the pool (#797).
    prompt: u64,
    /// Its call's stop.
    cancel: Cancel,
}

/// A fork a step warranted, cut and waiting for a slot (#782).
struct Cut {
    /// The turn.
    turn: u32,
    /// The trunk request whose step warranted it.
    at: u64,
    /// The `seq` of the step's last `tool_call`.
    line: u64,
    firing: Firing,
    /// The trunk as it stood after the step's results.
    messages: Vec<Message>,
}

/// What waits for the last fork cut during the turn (#782).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Deferred {
    /// The turn's end ([`turn_over`]).
    TurnOver,
    /// A move to `awaiting`.
    Move,
}

/// A meditation queued for [`meditation_work`] (#504): what fired it, and the phase it
/// moves to.
struct PendingMeditation {
    reason: crate::meditation::Reason,
    to: Option<String>,
    /// The operator's refill-anyway (#150's gate): no audit, no gate.
    anyway: bool,
}

/// A meditation's audit, fired ([`audit_fired`], #504).
struct AuditFired {
    /// The turn it audits after.
    turn: u32,
    /// The sequence number of its fork line.
    fork: u64,
    /// The sequence number of its call's request.
    request: u64,
    shape: RequestShape,
    cancel: Cancel,
    /// The notes the ask numbered, in its order.
    items: Vec<crate::object::EntryId>,
    /// The notes among them with an operator's pending change (#150, #748).
    marked: Vec<String>,
    /// Whether it is answered through the record tools (#610), not in lines.
    tools: bool,
}

vocabulary! {
    /// What the operator ruled on the model's phase proposal (#124).
    PhaseChoice {
        /// Start the new phase from the summary: a meditation that moves to it.
        Meditation => "meditation",
        /// Move to the new phase and keep the trunk (#788; v7's `continue`).
        Keep => "keep",
        /// Stay: the proposal is declined.
        Stay => "stay",
    }
}

/// A phase move as `choice` makes it (#124, #788): through a meditation that
/// moves to `to`, queued; or kept, the trunk as it stands and the phase's
/// ground rules a note on the next ask (#787); or none. Whether a meditation
/// was queued.
fn moved(template: &RequestShape, state: &mut State, to: &str, choice: PhaseChoice) -> bool {
    match choice {
        PhaseChoice::Meditation => meditation(
            template,
            state,
            crate::meditation::Reason::Phase,
            Some(to.to_owned()),
        ),
        PhaseChoice::Keep => {
            state.phase = Some(to.to_owned());
            // No meditation to carry its rules (#787): a note does.
            state.phase_note_due = state.phase_note();
            entered(template, state);
            false
        }
        PhaseChoice::Stay => false,
    }
}

/// The session entered its phase now (#788): its steps and tokens count
/// from here.
fn entered(template: &RequestShape, state: &mut State) {
    state.phase_entered = state.log.len() as u64;
    state.phase_grown = 0;
    state.phase_floor = trunk_now(state, template);
}

/// The trunk's size now, in tokens (#788): as its latest call measured it,
/// else as estimated.
fn trunk_now(state: &State, template: &RequestShape) -> u64 {
    state.trunk_tokens.unwrap_or_else(|| {
        let mut shape = template.clone();
        shape.messages.clone_from(&state.trunk);
        estimate_of(&shape)
    })
}

/// The evidence a phase move is proposed with (#788): the trunk steps since
/// the session entered its phase and the tokens the trunk grew by in it,
/// across any meditation; the tokens it grew by since the latest meditation;
/// and whether that meditation was the operator's with no turn since.
fn evidence(state: &State, template: &RequestShape) -> log::PhaseEvidence {
    let now = trunk_now(state, template);
    let phase_steps = state
        .log
        .iter()
        .filter(|logged| {
            logged.seq >= state.phase_entered
                && matches!(
                    logged.event,
                    Event::Requested {
                        lane: Lane::Trunk,
                        ..
                    }
                )
        })
        .count() as u64;
    let meditated_by_hand = state
        .log
        .iter()
        .rev()
        .take_while(|logged| !matches!(logged.event, Event::Asked { .. }))
        .find_map(|logged| match &logged.event {
            Event::Meditated { reason, .. } => Some(*reason == crate::meditation::Reason::Operator),
            _ => None,
        })
        .unwrap_or(false);
    log::PhaseEvidence {
        phase_steps,
        phase_tokens: state.phase_grown + now.saturating_sub(state.phase_floor),
        tokens_since_meditation: now.saturating_sub(state.meditation_floor),
        meditated_by_hand,
    }
}

/// The phase tool's name (#124, #788).
pub const PHASE_TOOL: &str = "propose_phase_transition";

/// What the model is shown when its own proposal is pending (#788): the
/// pinned `PHASE_PROPOSED`, its turn ending.
///
/// # Panics
///
/// Never: the template's one hole is `to`, which a test holds.
#[must_use]
pub fn proposal_result(to: &str) -> String {
    crate::dogma::Template::PhaseProposed
        .fill(&[(crate::dogma::Hole::To, to)])
        .expect("PHASE_PROPOSED's one hole is `to`")
}

/// The note that puts the operator's proposal to the model at its next ask
/// (#788): the pinned `PHASE_OPERATOR_PROPOSAL`.
///
/// # Panics
///
/// Never: the template's holes are `from` and `to`, which a test holds.
#[must_use]
pub fn operator_proposal_note(from: Option<&str>, to: &str) -> String {
    crate::dogma::Template::PhaseOperatorProposal
        .fill(&[
            (crate::dogma::Hole::From, from.unwrap_or("none")),
            (crate::dogma::Hole::To, to),
        ])
        .expect("PHASE_OPERATOR_PROPOSAL's holes are `from` and `to`")
}

/// The phase tool's definition (#788): `to` and `reason`, and `accept`, a
/// boolean, when answering the operator.
#[must_use]
pub fn phase_tool() -> crate::client::shape::ToolDefinition {
    use crate::formats::record::json::Value;
    let text = |s: &str| Value::String(s.to_owned());
    let typed = |kind: &str| Value::Object(BTreeMap::from([("type".to_owned(), text(kind))]));
    crate::client::shape::ToolDefinition {
        name: PHASE_TOOL.to_owned(),
        description: Some(crate::dogma::Template::PhaseTool.text().to_owned()),
        schema: Value::Object(BTreeMap::from([
            (
                "properties".to_owned(),
                Value::Object(BTreeMap::from([
                    ("to".to_owned(), typed("string")),
                    ("reason".to_owned(), typed("string")),
                    ("accept".to_owned(), typed("boolean")),
                ])),
            ),
            (
                "required".to_owned(),
                Value::Array(vec![text("to"), text("reason")]),
            ),
            ("type".to_owned(), text("object")),
        ])),
    }
}

/// The phase tool on `shape`, offered whenever the session's phases are
/// active (#788), in place of any self-capture set's own.
pub fn declare_phase_tool(shape: &mut RequestShape, interview: Option<&Interview>) {
    if interview.is_some_and(|interview| !interview.phases.is_empty()) {
        shape.tools.retain(|tool| tool.name != PHASE_TOOL);
        shape.tools.push(phase_tool());
    }
}

/// The phase tool's call (#788): an answer to the operator's pending
/// proposal when it carries `accept` -- logged, and accepted, the move
/// kept -- else the model's own proposal, refused in the graph's word or
/// held for the operator's ruling with its evidence, its turn ending after
/// this step.
fn phase_call<S>(
    shared: &Shared<S>,
    (turn, request): (u32, u64),
    call: &Call,
) -> (ToolLine, Option<String>) {
    let line = ToolLine::of(request, turn, call, log::ToolOutcome::Ran);
    let args: serde_json::Value = serde_json::from_str(&call.arguments).unwrap_or_default();
    let to = args["to"].as_str().unwrap_or_default().to_owned();
    let reason = args["reason"]
        .as_str()
        .filter(|why| !why.is_empty())
        .map(str::to_owned);
    let mut state = shared.lock();
    let shown = if let Some(accept) = args["accept"].as_bool() {
        let pending = state
            .operator_proposal
            .as_ref()
            .filter(|(proposed, _)| *proposed == to)
            .map(|(_, meditate)| *meditate);
        if let Some(meditate) = pending {
            state.operator_proposal = None;
            state.push(Event::PhaseAnswered {
                call: call.id.clone(),
                accepted: accept,
                reason,
            });
            if accept {
                // The operator's choice (#788): through a meditation once
                // this turn ends, or kept now.
                let choice = if meditate {
                    PhaseChoice::Meditation
                } else {
                    PhaseChoice::Keep
                };
                let from = state.phase.clone();
                let phase_document = state.document_of(&to);
                state.push(Event::PhaseRuled {
                    call: Some(call.id.clone()),
                    by: Some(Key::Model),
                    choice,
                    from,
                    to: to.clone(),
                    phase_document,
                });
                if meditate {
                    state.move_after_turn = Some(to.clone());
                } else {
                    moved(&shared.template, &mut state, &to, PhaseChoice::Keep);
                }
                "accepted".to_owned()
            } else {
                "declined".to_owned()
            }
        } else {
            format!("refused: {}", Refusal::NoProposal.tag())
        }
    } else if state.operator_proposal.is_some() {
        format!("refused: {}", Refusal::ProposalPending.tag())
    } else if let Some(refusal) = graph_refusal(&state, &to) {
        format!("refused: {}", refusal.tag())
    } else {
        let from = state.phase.clone();
        state.proposal = Some(PendingProposal {
            call: call.id.clone(),
            from,
            to: to.clone(),
        });
        state.proposal_ends_turn = Some(OwnProposal { reason });
        proposal_result(&to)
    };
    drop(state);
    shared.changed.notify_all();
    (line, Some(shown))
}

vocabulary! {
    /// Which key turns a phase move (#788).
    Key {
        /// The model.
        Model => "model",
        /// The operator.
        Operator => "operator",
    }
}

/// A step's own phase proposal, held until the step settles (#788): why,
/// in the model's words.
struct OwnProposal {
    reason: Option<String>,
}

/// The model's phase proposal the operator has not yet ruled on (#124).
#[derive(Debug, Clone)]
struct PendingProposal {
    /// Its `propose_phase_transition` call.
    call: String,
    /// The phase the session was in when it was made.
    from: Option<String>,
    /// The phase proposed.
    to: String,
}

impl State {
    /// The document of the phase `name` (#787), under a phase library.
    fn document_of(&self, name: &str) -> Option<log::PhaseDocument> {
        self.interview
            .as_ref()?
            .phases
            .document(name)
            .map(crate::meditation::library::Document::logged)
    }

    /// The document of the phase the session is in (#787).
    fn phase_document(&self) -> Option<log::PhaseDocument> {
        self.document_of(self.phase.as_deref()?)
    }

    /// The head a meditation refills under (#787, refining #597): `head`
    /// with the current phase's ground rules after its system prompt, and
    /// that phase's document. The meditation carries them, so a note due
    /// is spent -- unless the head has no system prompt to carry them.
    fn phase_head(&mut self, head: &[Message]) -> (Vec<Message>, Option<log::PhaseDocument>) {
        let document = self.phase_document();
        if document.is_some() && crate::meditation::render::carries_ground_rules(head) {
            self.phase_note_due = None;
        }
        (
            crate::meditation::render::with_ground_rules(head, document.as_ref()),
            document,
        )
    }

    /// The phase note due at turn `turn`'s ask (#787), logged: first among
    /// its notes, the frame the rest is read in.
    fn noted(&mut self, turn: u32) -> Option<String> {
        let text = self.phase_note_due.take()?;
        self.push(Event::PhaseNoted {
            turn,
            text: text.clone(),
        });
        Some(text)
    }

    /// The note carrying the current phase's ground rules (#787).
    fn phase_note(&self) -> Option<String> {
        self.phase_document().map(|document| {
            crate::meditation::render::phase_note(&document.name, &document.ground_rules)
        })
    }
}

/// The phase graph's ruling on a move from `from` to `to` (#563, #124),
/// as a refusal when it refuses.
fn graph_refusal(state: &State, to: &str) -> Option<Refusal> {
    use crate::meditation::phase::{Decision, Refusal as Graph};
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
        .map(crate::meditation::render::estimated_tokens)
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
    state.trunk_tokens = timings
        .as_ref()
        .and_then(trunk_tokens_of)
        .or_else(|| hosted.trunk_tokens());
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

    /// The trunk's tokens after this answer, from a hosted API's usage: the
    /// prompt in all and what it generated (#555), as llama.cpp's timings
    /// give them.
    fn trunk_tokens(&self) -> Option<u64> {
        self.usage
            .as_ref()
            .map(|usage| usage.prompt_tokens + usage.completion_tokens)
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
    // Then what the turn's steps warranted by their output (#783).
    let content = std::mem::take(&mut state.gap_firings);
    if !state.ending {
        fired.extend(content);
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
        pinned,
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
        pinned,
        role,
        seat,
        displaces,
        cut: None,
        timing: Timing::Gap,
        view_overridden: None,
        reserved: None,
    });
    // Beside the forks cut during the turn still in flight (#782), each
    // prompt holds its own part of the pool.
    let (room, crowded) = pool_room(state, shape.limits.context_window, (0, prompt), ceiling);
    let room = room.filter(|room| room.beside > 0);
    let crowded = crowded && room.is_some();
    if refused || crowded {
        state.push(Event::ForkSettled {
            trailer: None,
            fork,
            outcome: log::ForkOutcome::Refused,
            prompt_tokens: None,
            wall_ms: None,
            refused: Some(REFUSED_POOL.to_owned()),
            pool: room,
            not_run: None,
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
        beside: None,
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
        next = one_fork(shared, &forked);
    }
}

/// A step's forks, when they run during the turn (#782): cut as the trunk
/// stands after the step's results -- `shape`'s messages, what it `said`,
/// then its `results` -- and sent beside the trunk's next request.
fn cut_step<S: Streaming + 'static>(
    shared: &Arc<Shared<S>>,
    state: &mut State,
    at: (u32, u64, u64),
    (shape, said, results, calls): (&RequestShape, &Message, &[Message], &[Call]),
    settled: Option<SettleReason>,
) {
    // A cancelled step warrants none.
    if settled == Some(SettleReason::Cancelled) {
        return;
    }
    let ids: Vec<String> = calls.iter().map(|call| call.id.clone()).collect();
    // What the step produced (#783): every step's gates count, either timing.
    let content = content_firings(state, (at.0, at.1), said, &ids);
    if !state
        .interview
        .as_ref()
        .is_some_and(|interview| interview.timing == Timing::DuringTurn)
    {
        state.gap_firings.extend(content);
        return;
    }
    let mut cut = shape.messages.clone();
    cut.push(said.clone());
    cut.extend(results.iter().cloned());
    cut_forks(shared, state, at, &cut, ((&said.content, &ids), content));
}

/// The forks a trunk step's own output warrants (#783): the step's gates,
/// read off what it said -- its reasoning, a line break, then its text --
/// and its calls' lines of request `request`, each fired as its pinned
/// template asks it. `ids` are the step's calls; none for a final answer.
fn content_firings(
    state: &mut State,
    (turn, request): (u32, u64),
    said: &Message,
    ids: &[String],
) -> Vec<Firing> {
    use crate::drive::warrants::{Step, StepCall, gates, ident};
    let Some(warrants) = state
        .interview
        .as_ref()
        .map(|interview| interview.step)
        .filter(crate::drive::warrants::StepWarrants::any)
    else {
        return Vec::new();
    };
    let listings: Vec<String> = routed(&state.log, turn)
        .into_iter()
        .filter(|call| call.class == Class::DirectoryListing)
        .map(|call| call.id)
        .collect();
    let calls = state
        .log
        .iter()
        .filter_map(|logged| match &logged.event {
            Event::ToolCalled(line) if line.request == request && ids.contains(&line.id) => {
                Some(line.as_ref())
            }
            _ => None,
        })
        .map(|line| StepCall {
            id: line.id.clone(),
            tool: line.name.clone(),
            ident: ident(&line.name, &line.arguments),
            shown: line.shown.clone().unwrap_or_default(),
            errored: line.outcome == log::ToolOutcome::Ran
                && (line.exit.is_some_and(|exit| exit != 0) || line.timeout_ms.is_some()),
            excluded: matches!(
                line.outcome,
                log::ToolOutcome::Refused | log::ToolOutcome::Cancelled
            ),
            listing: listings.contains(&line.id),
        })
        .collect();
    let step = Step {
        said: format!(
            "{}\n{}",
            said.reasoning.as_deref().unwrap_or_default(),
            said.content
        ),
        calls,
    };
    let mut counters = std::mem::take(&mut state.step_counters);
    let fired = gates(&warrants, &mut counters, &step);
    state.step_counters = counters;
    pinned_firings(fired, request)
}

/// The forks the step gates `fired` for the trunk step of request
/// `request` (#783), each asking its pinned template.
fn pinned_firings(fired: Vec<crate::drive::warrants::Gate>, request: u64) -> Vec<Firing> {
    use crate::dogma::{Hole, Template};
    use crate::drive::warrants::Gate;
    let pinned =
        |why: log::Warrant, template: Template, question: String, trigger: String| Firing {
            why,
            ask: AskKind::Generic,
            pinned: Some(template),
            question,
            trigger,
        };
    let mut out = Vec::new();
    for gate in fired {
        match gate {
            Gate::Evidence { id, source, size } => {
                let size = size.to_string();
                if let Ok(question) =
                    Template::Evidence.fill(&[(Hole::Source, &source), (Hole::Size, &size)])
                {
                    out.push(pinned(
                        log::Warrant::Evidence,
                        Template::Evidence,
                        question,
                        format!("evidence:{id}"),
                    ));
                }
            }
            Gate::Trouble => {
                for template in [
                    Template::Gotchas,
                    Template::Decisions,
                    Template::Constraints,
                ] {
                    if let Ok(question) = template.fill(&[]) {
                        out.push(pinned(
                            log::Warrant::Trouble,
                            template,
                            question,
                            format!("trouble:{request}"),
                        ));
                    }
                }
            }
            Gate::Open => {
                if let Ok(question) = Template::OpenNext.fill(&[]) {
                    out.push(pinned(
                        log::Warrant::Open,
                        Template::OpenNext,
                        question,
                        format!("open:{request}"),
                    ));
                }
            }
        }
    }
    out
}

/// The forks a step warranted during the turn (#782), cut as the trunk
/// stood after the step's results -- `messages` -- and queued in step order
/// behind the slots, then sent while a slot is free. `at` is the trunk
/// request whose step it is, `cut` the `seq` of the step's last
/// `tool_call`, `said` the step's own text, `ids` its calls.
fn cut_forks<S: Streaming + 'static>(
    shared: &Arc<Shared<S>>,
    state: &mut State,
    (turn, at, cut): (u32, u64, u64),
    messages: &[Message],
    ((said, ids), content): ((&str, &[String]), Vec<Firing>),
) {
    let Some(interview) = state.interview.as_ref().filter(|_| !state.ending) else {
        return;
    };
    let (kept, skips) = screened(
        interview,
        &state.log,
        turn,
        step_firings(interview, &state.log, (turn, said), ids),
    );
    for skip in skips {
        state.push(skip);
    }
    // The calls' forks, then what the step's output warrants (#783).
    for firing in kept.into_iter().chain(content) {
        state.pool_queue.push_back(Cut {
            turn,
            at,
            line: cut,
            firing,
            messages: messages.to_vec(),
        });
    }
    launch_pool(shared, state);
}

/// Send queued cut forks while a slot is free (#782), each on a thread of
/// its own; one refused for the pool settles at once and the next is tried.
fn launch_pool<S: Streaming + 'static>(shared: &Arc<Shared<S>>, state: &mut State) {
    let slots = state
        .interview
        .as_ref()
        .map_or(0, |interview| interview.fork_slots);
    while state.pool.len() < usize::try_from(slots).unwrap_or(usize::MAX) {
        let Some(cut) = state.pool_queue.pop_front() else {
            return;
        };
        let Some(fired) = fire_cut(shared, state, cut) else {
            continue;
        };
        let (fork, request) = (fired.fork, fired.request);
        let shared = Arc::clone(shared);
        let spawned = std::thread::Builder::new()
            .name("diet-fork".to_owned())
            .spawn(move || pool_fork(&shared, &fired));
        if let Err(why) = spawned {
            // No thread, no call: the fork is failed, said so, and its slot
            // freed.
            state.pool.retain(|pool| pool.fork != fork);
            state.push(Event::Failed {
                request,
                overflow: None,
                failure: TransportFailure::Connect(format!(
                    "the fork's thread could not start: {why}"
                )),
                partial: String::new(),
            });
            state.push(Event::settled(fork, log::ForkOutcome::Failed));
        }
    }
}

/// The pool a fork of `prompt` would share with the trunk's `trunk` and the
/// forks' in flight, in a `window` (#782): the four numbers, and whether
/// they leave less than the clamp's floor (or `ceiling`, if less).
fn pool_room(
    state: &State,
    window: Option<u64>,
    (trunk, prompt): (u64, u64),
    ceiling: u32,
) -> (Option<log::PoolRoom>, bool) {
    let beside: u64 = state.pool.iter().map(|pool| pool.prompt).sum();
    let room = window.map(|window| log::PoolRoom {
        window,
        trunk,
        beside,
        prompt,
    });
    let short = room.is_some_and(|room| {
        room.window
            .saturating_sub(room.trunk + room.beside + room.prompt)
            < u64::from(ceiling).min(CLAMP_FLOOR)
    });
    (room, short)
}

/// What a fork of `prompt` reserves of the pool (#797), and whether its
/// prefix was counted as shared: all of it, unless the trunk's server
/// shares a prefix's pages (`shared`), the fork sees the trunk (`view`),
/// and the trunk holds a prompt -- then the part of its `prefix` the trunk
/// holds, up to the trunk's resident prompt, is the trunk's and not the
/// fork's. A fork that sees anything else counts in full.
fn reservation(
    prompt: u64,
    (prefix, trunk): (u64, u64),
    view: ForkView,
    shared: bool,
) -> (u64, bool) {
    if shared && view == ForkView::Trunk && trunk > 0 {
        (prompt.saturating_sub(prefix.min(trunk)), true)
    } else {
        (prompt, false)
    }
}

/// The prompt the trunk holds of the pool while a cut fork is sized
/// (#782): its latest sized request while the turn runs, else none.
fn trunk_resident(state: &State) -> u64 {
    if state.settlement == Settlement::Turn {
        state.last_sized.map_or(0, |(prompt, _, _)| prompt)
    } else {
        0
    }
}

/// What a cut fork is asked and sized under (#782, #797): the interview's
/// role, the view the regimen declared, the fork's tail, and whether the
/// trunk's server shares a prefix's pages.
fn cut_terms(state: &State, template: &RequestShape) -> (Role, Option<ForkView>, u32, bool) {
    state.interview.as_ref().map_or(
        (Role::User, None, template.limits.max_output_tokens, false),
        |interview| {
            (
                interview.role,
                interview.view,
                interview
                    .fork_tail
                    .unwrap_or(template.limits.max_output_tokens),
                interview.prefix_pages == PrefixPages::Shared,
            )
        },
    )
}

/// One cut fork, fired (#782): the trunk as it stood after its step's
/// results, then the question, whatever `fork_view` says -- its warm
/// prefix -- the view it overrode named on its line. Refused unsent when the
/// trunk's prompt, the forks' in flight and its own [`reservation`] leave
/// the pool less than the clamp's floor, the numbers on its `fork.settled`.
fn fire_cut<S>(shared: &Shared<S>, state: &mut State, cut: Cut) -> Option<Fired> {
    let Cut {
        turn,
        at,
        line,
        firing,
        messages,
    } = cut;
    let Firing {
        why,
        ask,
        pinned,
        question,
        trigger,
    } = firing;
    let (role, declared, ceiling, sharing) = cut_terms(state, &shared.template);
    let mut shape = shared.template.clone();
    shape.messages = messages;
    // The cut, sized as the fork's prompt is: the prefix it holds in
    // common with the trunk's (#797).
    let prefix = state.prompt_of(&shape);
    shape.messages.push(Message::new(role, question.clone()));
    let seat = shared.seat.as_ref().map(|seat| {
        shape.model.clone_from(&seat.model);
        shape.limits.context_window = seat.context_window;
        log::ForkSeat {
            substrate: seat.substrate.clone(),
            model: seat.model.clone(),
        }
    });
    // On the trunk's server its prefix is the trunk's, sized as the trunk's
    // is; offboard, its estimate is all there is.
    let prompt = if seat.is_some() {
        estimate_of(&shape)
    } else {
        state.prompt_of(&shape)
    };
    let trunk = if seat.is_some() {
        0
    } else {
        trunk_resident(state)
    };
    let (reserved, counted_shared) = reservation(prompt, (prefix, trunk), ForkView::Trunk, sharing);
    let (room, refused) = pool_room(
        state,
        shape.limits.context_window,
        (trunk, reserved),
        ceiling,
    );
    let fork = state.push(Event::Forked {
        of_turn: turn,
        at,
        why,
        question,
        view: ForkView::Trunk,
        trigger,
        ask,
        pinned,
        role,
        seat,
        displaces: false,
        cut: Some(line),
        timing: Timing::DuringTurn,
        view_overridden: declared.filter(|view| *view != ForkView::Trunk),
        reserved: Some((reserved, counted_shared)),
    });
    if refused {
        state.push(Event::ForkSettled {
            trailer: None,
            fork,
            outcome: log::ForkOutcome::Refused,
            prompt_tokens: None,
            wall_ms: None,
            refused: Some(REFUSED_POOL.to_owned()),
            pool: room,
            not_run: None,
        });
        return None;
    }
    let max_tokens = state.aside_sizing(|state| state.sized_at(&mut shape, ceiling, prompt));
    let request = state.push(Event::Requested {
        turn,
        lane: Lane::Interview,
        head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
        fork: Some(fork),
        max_tokens,
        beside: None,
    });
    let cancel = Cancel::new();
    state.pool.push(PoolFork {
        fork,
        prompt: reserved,
        cancel: cancel.clone(),
    });
    Some(Fired {
        turn,
        fork,
        request,
        shape,
        cancel,
    })
}

/// A cut fork, on its own thread (#782): its call streamed beside the
/// trunk's, then settled under one hold of the session's lock -- its patches
/// folded into working memory, the trunk's sizing untouched -- its slot
/// given to the next queued; and, the last one out, whatever waited for it.
fn pool_fork<S: Streaming + 'static>(shared: &Arc<Shared<S>>, forked: &Fired) {
    let run = stream_fork(shared, forked);
    let mut state = shared.lock();
    state.pool.retain(|pool| pool.fork != forked.fork);
    state.aside_sizing(|state| settle_fork(shared, state, forked, run));
    if state.ending {
        state.pool_queue.clear();
    }
    launch_pool(shared, &mut state);
    let due = pool_settled(&shared.template, &mut state);
    drop(state);
    shared.changed.notify_all();
    // A meditation the turn's end made due runs here, its turn thread
    // long gone (#504).
    if due {
        meditation_work(shared);
    }
}

/// Once no cut fork is in flight or queued and the gap's own forks are
/// done (#782): the turn's end, or the move to `awaiting`, that waited for
/// them. Whether a meditation was queued for its audit or pre-warm.
fn pool_settled(template: &RequestShape, state: &mut State) -> bool {
    if state.pool_busy() || state.forking.is_some() || !state.queued.is_empty() {
        return false;
    }
    match state.deferred.take() {
        Some(Deferred::TurnOver) => turn_over(template, state),
        Some(Deferred::Move) => {
            state.move_to(Settlement::Awaiting);
            if state.ending {
                state.ending = false;
                state.move_to(Settlement::Ended);
            }
        }
        None => {}
    }
    state.meditation_pending.is_some()
}

/// One of the gap's forks, run to its settling: then the next queued fork,
/// fired, or -- none left, a stop, or an `end` -- back to `awaiting`.
fn one_fork<S: Streaming>(shared: &Shared<S>, forked: &Fired) -> Option<Fired> {
    let run = stream_fork(shared, forked);
    let mut state = shared.lock();
    state.flight = None;
    state.forking = None;
    let outcome = settle_fork(shared, &mut state, forked, run);
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

/// A fork's call, streamed: what arrived, and how it ended.
struct ForkRun {
    result: Result<StreamEnded, TransportFailure>,
    partial: String,
    reasoning: String,
    /// Every call it made, on either route (#610, #806): none of them runs.
    calls: Calls,
    tools: bool,
    hosted: Hosted,
    began: Instant,
}

/// A fork's one call, streamed into the log on the interview lane and never
/// retried, holding the session's lock only to log each piece.
fn stream_fork<S: Streaming>(shared: &Shared<S>, forked: &Fired) -> ForkRun {
    let Fired {
        request,
        shape,
        cancel,
        ..
    } = forked;
    let request = *request;
    let deadline = Instant::now() + shared.template.limits.call;
    let mut partial = String::new();
    let mut reasoning = String::new();
    let tools = shared
        .lock()
        .interview
        .as_ref()
        .map(|interview| interview.capture)
        == Some(crate::dogma::asks::Modality::Tools);
    let mut calls = Calls::default();
    let transport = shared.seat.as_ref().map_or_else(
        || shared.forks.as_ref().unwrap_or(&shared.transport),
        |seat| &seat.transport,
    );
    let began = Instant::now();
    let mut hosted = Hosted::default();
    let result = transport.stream(shape, deadline, cancel, &mut |piece: Piece<'_>| {
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
            // Every call is logged on either route (#806): a fork cut from
            // the trunk carries the trunk's tools, and its answer may be a
            // call. None runs.
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
            Piece::Signature(_) | Piece::Redacted(_) | Piece::Usage(_) => return,
        };
        shared.lock().push(event);
        shared.changed.notify_all();
    });
    ForkRun {
        result,
        partial,
        reasoning,
        calls,
        tools,
        hosted,
        began,
    }
}

/// A fork's call, ended: how, and on `value` its patches, applied to the
/// session's working object; its `fork.settled` line, then its patches'.
#[allow(clippy::too_many_lines)]
fn settle_fork<S>(
    shared: &Shared<S>,
    state: &mut State,
    forked: &Fired,
    run: ForkRun,
) -> log::ForkOutcome {
    let (turn, fork, request) = (forked.turn, forked.fork, forked.request);
    let ForkRun {
        result,
        partial,
        reasoning,
        calls,
        tools,
        hosted,
        began,
    } = run;
    let calls = calls.into_calls();
    // What a fork calls runs nothing (#806); a record tool's call is read
    // as its answer, and every other is counted on its settled line.
    let not_run = u64::try_from(calls.iter().filter(|call| !record_tool(call)).count())
        .ok()
        .filter(|calls| *calls > 0);
    let reasoning = Some(reasoning).filter(|thought| !thought.is_empty());
    let mut patches = Vec::new();
    let mut trailer = None;
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
            if forked.cancel.is_asked() {
                log::ForkOutcome::Cancelled
            } else if cut {
                log::ForkOutcome::Truncated
            } else if tools {
                let (outcome, lines) = captured(state, &calls, (turn, request), fork);
                patches = lines;
                outcome
            } else {
                // The fields route reads the prose and folds any record
                // tool's call beside it (#806).
                let recorded = calls.iter().any(record_tool);
                let prose = if text.trim().is_empty() && (recorded || not_run.is_some()) {
                    None
                } else {
                    // What trails the last field is no field (levers run A).
                    let (fields, after) = trailed(&text);
                    trailer = after;
                    let (outcome, lines) = folded(state, &fields, turn, fork);
                    patches = lines;
                    Some(outcome)
                };
                let called = recorded.then(|| {
                    let (outcome, lines) = captured(state, &calls, (turn, request), fork);
                    patches.extend(lines);
                    outcome
                });
                match (prose, called) {
                    (Some(log::ForkOutcome::Value), _) | (_, Some(log::ForkOutcome::Value)) => {
                        log::ForkOutcome::Value
                    }
                    (Some(outcome), _) | (None, Some(outcome)) => outcome,
                    (None, None) => log::ForkOutcome::Unparseable,
                }
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
            // A timeout too: logged as the call's failure, never silent.
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
        trailer,
        fork,
        outcome,
        prompt_tokens,
        wall_ms,
        refused: None,
        pool: None,
        not_run,
    });
    for patch in patches {
        state.push(patch);
    }
    outcome
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
    captured_in(state, calls, (turn, request), (fork, Lane::Interview))
}

/// Whether `call` is a record tool's (`update_record`, `resolve_entry`): the
/// calls a fork's or an audit's answer is read from (#610, #806). The phase
/// tool is not one: a fork proposes nothing.
fn record_tool(call: &Call) -> bool {
    use crate::capture::tools::CaptureTool;
    CaptureTool::from_tag(&call.name).is_some_and(|tool| CaptureTool::OFFERED.contains(&tool))
}

/// [`captured`], its patches on `lane`: `interview` for a fork's, `audit`
/// for a meditation's audit's (#610).
fn captured_in(
    state: &mut State,
    calls: &[Call],
    (turn, request): (u32, u64),
    (fork, lane): (u64, Lane),
) -> (log::ForkOutcome, Vec<Event>) {
    let mut lines = Vec::new();
    let mut wrote = false;
    let capture_calls: Vec<&Call> = calls.iter().filter(|call| record_tool(call)).collect();
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
                    lane.tag().clone_into(&mut at.lane);
                    at.fork = Some(format!("f/{fork}"));
                }
                if patches.is_empty() {
                    (None, Vec::new(), Vec::new())
                } else {
                    match applied(state, &patches, fork) {
                        // The entries it wrote are its `patch` lines', not
                        // a refused one's (#689).
                        (log::ForkOutcome::Value, lines) => (None, written_by(&lines), lines),
                        // Every patch refused over the operator's entry: it
                        // says whose, and its refusals are logged (#689).
                        (log::ForkOutcome::Decline, lines) if !lines.is_empty() => {
                            (operator_refused(&lines), Vec::new(), lines)
                        }
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
            from: None,
            to: None,
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

/// A fork's answer `text` cut at its last field's end (levers run A): what
/// that field wrote after a blank line is no part of it -- a sign-off an
/// instruction file asked of every answer, a remark -- and comes back on
/// its own, trimmed, to be logged and recorded as no entry. A field before
/// the last keeps its lines across a blank one, as the grammar reads them,
/// since the next field's tag ends it; and the last keeps every line up to
/// its first blank one. `None` when nothing trails.
fn trailed(text: &str) -> (std::borrow::Cow<'_, str>, Option<String>) {
    let unchanged = (std::borrow::Cow::Borrowed(text), None);
    let Ok(answer) = crate::formats::interview::parse(text) else {
        return unchanged;
    };
    let Some(last) = answer.fields.iter().rev().find(|field| field.tag.is_some()) else {
        return unchanged;
    };
    let mut lines = last.raw.split('\n');
    let _first = lines.next();
    let mut rest = lines.skip_while(|line| !line.trim().is_empty());
    if rest.next().is_none() {
        return unchanged;
    }
    let after: Vec<&str> = rest.collect();
    let trailer = after.join("\n").trim().to_owned();
    if trailer.is_empty() {
        return unchanged;
    }
    let first = after
        .iter()
        .find(|line| !line.trim().is_empty())
        .copied()
        .unwrap_or_default();
    let Some(at) = text.rfind(first.trim_end()) else {
        return unchanged;
    };
    // An answer the grammar read inside a fence keeps the fence's close.
    let closing = text[at..].trim_end().strip_suffix("```").is_some();
    let mut body = text[..at].trim_end().to_owned();
    if closing {
        body.push_str("\n```");
    }
    (std::borrow::Cow::Owned(body), Some(trailer))
}

/// `patches` with each addition whose id `object` already holds -- an
/// earlier fork in the same gap folded under the same `interview-t<turn>-
/// <index>` (#730) -- renamed `interview-t<turn>-f<fork>-<index>`, so the
/// second fork's facts fold beside the first's instead of being refused as an
/// id reused. A gap's first fork keeps the ids it always had.
fn unclashed(patches: Vec<Patch>, object: &WorkingObject, (turn, fork): (u32, u64)) -> Vec<Patch> {
    patches
        .into_iter()
        .map(|patch| match patch {
            Patch::Add {
                id,
                content,
                provenance,
            } if object.entry(&id).is_some() => {
                let renamed = crate::object::EntryId::new(&format!(
                    "{}-t{turn}-f{fork}-{}",
                    super::INTERVIEW,
                    provenance.index
                ))
                .unwrap_or(id);
                Patch::Add {
                    id: renamed,
                    content,
                    provenance,
                }
            }
            other => other,
        })
        .collect()
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
    let patches = unclashed(patches, &interview.object, (turn, fork));
    let Cited { patches, reasons } = cited(patches, &interview.object);
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
    let (outcome, mut lines) = applied(state, &patches, fork);
    // Each reasoned pair's reason, on the line that voided or retired the
    // entry it cited (#812).
    for line in &mut lines {
        if let Event::Patched {
            op,
            entry,
            supersedes,
            reason,
            ..
        } = line
        {
            let cited = match op {
                log::PatchOp::Supersede => supersedes.as_deref(),
                log::PatchOp::Retire => Some(entry.id.as_str()),
                _ => None,
            };
            *reason = cited.and_then(|cited| reasons.get(cited)).cloned();
        }
    }
    (outcome, lines)
}

/// A fork's `patches`, applied to the working object (every op the format
/// has, #562): one [`Event::Patched`] each, and each non-`add` op's entry
/// held for the next ask's note when delivery is mid-turn (#550).
fn applied(state: &mut State, patches: &[Patch], fork: u64) -> (log::ForkOutcome, Vec<Event>) {
    let Some(interview) = state.interview.as_mut() else {
        return (log::ForkOutcome::Unparseable, Vec::new());
    };
    // A patch on an entry the operator wrote is refused (#150), the rest
    // folded.
    let (kept, refused) = guarded(patches, &interview.object, Some(fork));
    if kept.is_empty() {
        return (log::ForkOutcome::Decline, refused);
    }
    let patches = kept.as_slice();
    // The entry each delivered line names, read before the patches apply:
    // a supersede's voided entry, a verdict's target. An `add` names none
    // and is carried at the meditation only (no measured sentence fits it).
    let delivering = interview.delivery != log::ForkDelivery::Meditation;
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
    let lines = refused
        .into_iter()
        .chain(
            patches
                .iter()
                .map(|patch| patched(Some(fork), patch, &interview.object)),
        )
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
/// rest under the field's own id. The meditation's render shows each entry as
/// `<id>\t<content>`, so the id is what a model that saw it can cite; quotes,
/// backticks and brackets around it, and a colon or hyphen after it, are
/// read past. An em dash after it is not: it opens the pair's reason (#812). A field that cites nothing live, or names no entry to stand, stays
/// the addition [`super::fold`] made of it.
///
/// One field may carry several pairs, `;` between them, and `→` or `->` may
/// stand between a cited id and what replaces it (seen on a v4 fork, #562).
/// A pair whose replacement is itself a live entry's id retires the cited
/// entry in favour of that one, writing no new text: the entry that should
/// stand already does. Each pair after the first records under the field's
/// id with its position after it, and a pair that cites nothing live stays
/// an addition of its own text.
///
/// An em dash after the cited id makes the rest that pair's reason, not an
/// entry (#812): `<id> — <reason>`, the audit grammar's `REMOVE — <reason>`
/// shape, which a model answering the v4 ask writes. The answer's own
/// DECISION then stands for the cited entry, superseding it, or with none
/// the cited entry retires; either way the reason is recorded, by the
/// voided entry's id, and never becomes an entry's text.
fn cited(patches: Vec<Patch>, object: &WorkingObject) -> Cited {
    let fields = patches.len();
    let mut reasons = BTreeMap::new();
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
            let pairs = pairs_of(field, object);
            let pairs: Vec<&str> = pairs.iter().map(String::as_str).collect();
            if pairs.len() < 2 {
                return vec![
                    superseding(field.trim(), object, id.clone(), provenance)
                        .map_or(patch, |pair| pair.patch(&mut reasons)),
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
                    Some(match superseding(pair, object, id.clone(), provenance) {
                        Some(read) => read.patch(&mut reasons),
                        None => Patch::Add {
                            id,
                            content: format!("{SUPERSEDE_FIELD}{pair}"),
                            provenance: provenance.clone(),
                        },
                    })
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
    // A reasoned pair's entry stands in the answer's own DECISION, which
    // supersedes the first entry cited so; any other is retired.
    let mut decided = read.iter().position(
        |patch| matches!(patch, Patch::Add { content, .. } if content.starts_with(DECISION_FIELD)),
    );
    for at in 0..read.len() {
        let Patch::Retire { target, .. } = &read[at] else {
            continue;
        };
        if !reasons.contains_key(target.as_str()) {
            continue;
        }
        let Some(decision) = decided.take() else {
            break;
        };
        let voids = target.clone();
        if let Patch::Add {
            id,
            content,
            provenance,
        } = read[decision].clone()
        {
            read[decision] = Patch::Supersede {
                id,
                content,
                voids,
                provenance,
            };
            read.remove(at);
        }
        break;
    }
    Cited {
        patches: read,
        reasons,
    }
}

/// [`cited`]'s reading: the patches, and the reason each `<id> — <reason>`
/// pair gave, by the id it voided (#812).
#[derive(Debug)]
pub(crate) struct Cited {
    /// The patches.
    pub patches: Vec<Patch>,
    /// Each reason, by the voided entry's id.
    pub reasons: BTreeMap<String, String>,
}

/// The prefix [`super::fold`] gives a `DECISION` field's entry.
const DECISION_FIELD: &str = "decision: ";

/// One `SUPERSEDE` pair read (#562, #812): the patch it makes, or -- an em
/// dash after the cited id -- the entry it voids and why.
enum Pair {
    /// A supersession, or a retirement in favour of a live entry.
    Patch(Patch),
    /// `<id> — <reason>`.
    Reasoned {
        /// The entry it voids.
        voids: crate::object::EntryId,
        /// Why.
        reason: String,
        /// Where it came from.
        provenance: crate::object::Provenance,
    },
}

impl Pair {
    /// The patch: a reasoned pair's a retirement, its reason recorded in
    /// `reasons` -- what [`cited`] turns into the DECISION's supersession.
    fn patch(self, reasons: &mut BTreeMap<String, String>) -> Patch {
        match self {
            Self::Patch(patch) => patch,
            Self::Reasoned {
                voids,
                reason,
                provenance,
            } => {
                reasons.insert(voids.as_str().to_owned(), reason);
                Patch::Retire {
                    target: voids,
                    provenance,
                }
            }
        }
    }
}

/// A `SUPERSEDE` field's pairs (#562): `;` starts a new pair only where a
/// live entry's id follows it -- the id a pair cites first. Elsewhere it is
/// the prose's own, and stays in the pair it is part of (levers run A: a
/// one-line supersede whose reason held two `;` was read as three pairs, two
/// of them additions of its own fragments).
fn pairs_of(field: &str, object: &WorkingObject) -> Vec<String> {
    let quoted = |c: char| "`'\"[]()<>*:,—-".contains(c);
    let cites = |piece: &str| {
        let first = piece.split(char::is_whitespace).next().unwrap_or_default();
        let first = first.split("→").next().unwrap_or_default();
        let first = first.split("->").next().unwrap_or_default();
        crate::object::EntryId::new(first.trim_matches(quoted))
            .ok()
            .is_some_and(|id| object.live().any(|entry| entry.id == id))
    };
    let mut pairs: Vec<String> = Vec::new();
    for piece in field.split(';') {
        let trimmed = piece.trim();
        match pairs.last_mut() {
            Some(pair) if !cites(trimmed) => {
                pair.push(';');
                pair.push_str(piece.trim_end());
            }
            _ if trimmed.is_empty() => {}
            _ => pairs.push(trimmed.to_owned()),
        }
    }
    for pair in &mut pairs {
        *pair = pair.trim().to_owned();
    }
    pairs
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
) -> Option<Pair> {
    let quoted = |c: char| "`'\"[]()<>*:,—-".contains(c);
    // An arrow points from the cited id only when it follows the id itself:
    // one inside the text that stands is that text's (#730).
    let arrowed = pair
        .split_once('→')
        .or_else(|| pair.split_once("->"))
        .filter(|(cited, _)| !cited.trim().contains(char::is_whitespace));
    let (cited, rest) = match arrowed {
        Some((cited, rest)) => (cited.trim(), rest),
        None => pair.split_once(char::is_whitespace).unwrap_or((pair, "")),
    };
    // An em dash straight after the id, unarrowed, opens a reason (#812).
    let reasoned = arrowed.is_none() && rest.trim_start().starts_with('—');
    let cited = cited.trim_matches(quoted);
    let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || ":—-".contains(c));
    let rest = rest.trim();
    let live = |id: &crate::object::EntryId| object.live().any(|entry| entry.id == *id);
    let voids = crate::object::EntryId::new(cited)
        .ok()
        .filter(|voids| !rest.is_empty() && live(voids))?;
    if reasoned {
        return Some(Pair::Reasoned {
            voids,
            reason: rest.to_owned(),
            provenance: provenance.clone(),
        });
    }
    let standing = crate::object::EntryId::new(rest.trim_matches(quoted))
        .ok()
        .filter(|standing| *standing != voids && live(standing));
    Some(Pair::Patch(match standing {
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
    }))
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
        reason: None,
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
        state.push(Event::settled(fork, log::ForkOutcome::Failed));
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
                idle: None,
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

    /// A user message carrying the harness's `notes` before its words
    /// (#609).
    fn noted(text: &str, notes: &[&str]) -> Message {
        let mut message = Message::new(Role::User, text);
        message.notes = notes.iter().map(|note| (*note).to_owned()).collect();
        message
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
                    beside: None,
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
        )
        .patches;
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

    /// Levers run A's turn-2 fork answer, verbatim: its fields, then the
    /// sign-off the worktree's AGENTS.md asks of every answer.
    const RUN_A_T2: &str = "\n\nDECISION: Add `#anchor` validation to `linkcheck.py` for local links — GitHub-style slugs via `slugify()`/`anchors_of()` (duplicates numbered `a`, `a-1`), same-file `#anchor` supported, output format and exit codes unchanged; samples updated to show valid anchor, broken file, and broken anchor.\nPLAN: No open work; optional follow-ups only if asked — verify `[x](b.md#b \"B\")` title-with-anchor handling and decide whether headings inside fenced code blocks should be ignored.\nSUPERSEDE: NONE\n\n-- checked against AGENTS.md";

    /// Levers run A's turn-7 fork answer, verbatim: a one-line SUPERSEDE
    /// whose reason holds two `;`, then the sign-off.
    const RUN_A_T7: &str = "\n\nDECISION: Dropped `--quiet` entirely in `linkcheck.py` — `main()` no longer filters argv for it; it takes the directory from `argv[1]` (default `.`) and always prints each broken link as `path:lineno: target`, exiting 1 if any were found. Verified: `python3 linkcheck.py docs` prints `docs/c.md:3: missing.md` and exits 1; a clean fixture dir prints nothing and exits 0. (Side effect worth noting: a stray `--quiet` argument would now be treated as a directory name.)\n\nPLAN: NONE — the change is made and verified; no further work this turn, per your note that you'll review the record yourself.\n\nSUPERSEDE: r3421/call_22baf3929b944b80b538212e/decision — its `--quiet` clause (\"main() accepts --quiet anywhere in argv (silences output…)\", \"--quiet prints nothing and exits 1 on broken / 0 on clean\") is replaced by this turn's decision: `--quiet` is gone; broken links always print. Also r4621/call_66c57752248e464cbd189810/decision — its \"–quiet kept\" clause is superseded by the same change, and its claim that anchor validation (HEADING_RE, FENCE_RE, slugify, anchors_of) was removed contradicts the file I read this turn, which still contains all of it; the entry that should stand on that point is the file's actual state.\n\n-- checked against AGENTS.md";

    /// Levers run A: what trails the last field after a blank line -- the
    /// sign-off -- is cut off and comes back on its own; `SUPERSEDE: NONE`
    /// is then a decline and no entry, as the grammar reads it. Lines before
    /// a blank one stay their field's.
    #[test]
    fn a_sign_off_after_the_last_field_is_no_field() {
        let (fields, trailer) = trailed(RUN_A_T2);
        assert_eq!(trailer.as_deref(), Some("-- checked against AGENTS.md"));
        assert!(fields.trim_end().ends_with("SUPERSEDE: NONE"), "{fields}");
        let (patches, _) =
            super::super::fold(&fields, 2, super::super::INTERVIEW, Some("f/9")).expect("read");
        let contents: Vec<String> = patches
            .iter()
            .map(|patch| match patch {
                Patch::Add { content, .. } => content.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(contents.len(), 2, "{contents:?}");
        assert!(contents[0].starts_with("decision: Add `#anchor` validation"));
        assert!(contents[1].starts_with("plan: No open work"));
        // A last field's continuation line, with no blank line, is its own.
        let (kept, none) = trailed("DECISION: first\nsecond\n");
        assert_eq!((kept.as_ref(), none), ("DECISION: first\nsecond\n", None));
    }

    /// Levers run A, served: the fork's sign-off is on its `fork.settled`
    /// line and in no entry; the log reads back.
    #[test]
    fn a_forks_sign_off_is_logged_on_its_settle_and_recorded_nowhere() {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.asks = &crate::dogma::asks::V4;
        let session = Session::open_with(
            Canned::new([deltas(&[SCOPED]), deltas(&[RUN_A_T2])]),
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
        let held = live_entries(&session);
        assert_eq!(held.len(), 2, "{held:?}");
        assert!(
            !held
                .iter()
                .any(|(_, content)| content.contains("AGENTS.md")),
            "{held:?}"
        );
        let trailers: Vec<Option<String>> = whole_log(&session)
            .iter()
            .filter_map(|line| match &line.event {
                log::Event::ForkSettled { trailer, .. } => Some(trailer.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(trailers, [Some("-- checked against AGENTS.md".to_owned())]);
        reads_whole(&session);
    }

    /// Levers run A, the hard case (#812): a `;` in a SUPERSEDE's prose is
    /// the prose's -- one pair, never an addition of its fragments -- and
    /// the em dash after the cited id makes the rest that pair's reason. The
    /// answer's own DECISION supersedes the cited entry; the reason is
    /// recorded, never an entry's text; and the second id the reason names
    /// ("Also r4621/…") is prose, not a citation.
    #[test]
    fn a_semicolon_in_a_supersedes_prose_is_no_new_pair() {
        let object = object_of(&[
            "r3421/call_22baf3929b944b80b538212e/decision",
            "r4621/call_66c57752248e464cbd189810/decision",
        ]);
        let (fields, trailer) = trailed(RUN_A_T7);
        assert_eq!(trailer.as_deref(), Some("-- checked against AGENTS.md"));
        let (patches, _) =
            super::super::fold(&fields, 7, super::super::INTERVIEW, Some("f/9")).expect("read");
        let Cited { patches, reasons } = cited(patches, &object);
        let superseding: Vec<(&str, &str)> = patches
            .iter()
            .filter_map(|patch| match patch {
                Patch::Supersede { voids, content, .. } => Some((voids.as_str(), content.as_str())),
                _ => None,
            })
            .collect();
        let [(voids, content)] = superseding.as_slice() else {
            panic!("{patches:?}");
        };
        assert_eq!(*voids, "r3421/call_22baf3929b944b80b538212e/decision");
        assert!(
            content.starts_with("decision: Dropped `--quiet` entirely"),
            "{content}"
        );
        let reason = &reasons["r3421/call_22baf3929b944b80b538212e/decision"];
        assert!(reason.starts_with("its `--quiet` clause"), "{reason}");
        assert!(
            reason
                .ends_with("the entry that should stand on that point is the file's actual state."),
            "{reason}"
        );
        assert!(
            !patches.iter().any(|patch| matches!(
                patch,
                Patch::Add { content, .. } | Patch::Supersede { content, .. }
                    if content.starts_with("supersede: ") || content.starts_with("its `--quiet`")
            )),
            "{patches:?}"
        );
        assert!(
            !patches.iter().any(|patch| matches!(
                patch,
                Patch::Retire { target, .. } if target.as_str().starts_with("r4621/")
            )),
            "an id inside a reason is not cited: {patches:?}"
        );
    }

    /// #812, the levers run C re-fire (#811): `SUPERSEDE: <id> — <reason>`,
    /// the audit grammar's `REMOVE — <reason>` shape, voids the cited entry
    /// in favour of the answer's own DECISION, the reason recorded and never
    /// an entry's text; with no DECISION to stand, the cited entry retires.
    #[test]
    fn a_supersede_pair_with_an_em_dash_reason_never_makes_the_reason_an_entry() {
        const REFIRE: &str = "DECISION: Broken links are now reported as JSON Lines\n\
                              PLAN: Nothing further unless the user requests changes.\n\
                              SUPERSEDE: interview-t1-f1366-0 — replaced by this turn's DECISION \
                              (output format is JSON Lines, not `file:line: target`).\n";
        let object = object_of(&["interview-t1-f1366-0"]);
        let (patches, _) =
            super::super::fold(REFIRE, 2, super::super::INTERVIEW, Some("f/2053")).expect("read");
        let Cited { patches, reasons } = cited(patches, &object);
        assert!(
            matches!(
                patches.as_slice(),
                [Patch::Supersede { content, voids, .. }, Patch::Add { content: plan, .. }]
                    if content == "decision: Broken links are now reported as JSON Lines"
                        && voids.as_str() == "interview-t1-f1366-0"
                        && plan.starts_with("plan: ")
            ),
            "{patches:#?}"
        );
        assert_eq!(
            reasons,
            BTreeMap::from([(
                "interview-t1-f1366-0".to_owned(),
                "replaced by this turn's DECISION (output format is JSON Lines, not \
                 `file:line: target`)."
                    .to_owned()
            )])
        );
        // No DECISION to stand: the cited entry retires, for the same reason.
        let (patches, _) = super::super::fold(
            "SUPERSEDE: interview-t1-f1366-0 — the format changed\n",
            2,
            super::super::INTERVIEW,
            Some("f/2053"),
        )
        .expect("read");
        let Cited { patches, reasons } = cited(patches, &object);
        assert!(
            matches!(
                patches.as_slice(),
                [Patch::Retire { target, .. }] if target.as_str() == "interview-t1-f1366-0"
            ),
            "{patches:#?}"
        );
        assert_eq!(reasons["interview-t1-f1366-0"], "the format changed");
        // A plain hyphen or a colon still introduces the entry that stands.
        for written in [
            "interview-t1-f1366-0 - decision: JSON Lines",
            "interview-t1-f1366-0: decision: JSON Lines",
        ] {
            let Cited { patches, reasons } =
                cited(vec![supersede_field("interview-t2-0", written)], &object);
            assert!(
                matches!(
                    patches.as_slice(),
                    [Patch::Supersede { content, .. }] if content == "decision: JSON Lines"
                ),
                "{written}: {patches:#?}"
            );
            assert!(reasons.is_empty(), "{written}");
        }
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
            )
            .patches;
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

    /// #812: a fork's `<id> — <reason>` reaches the log as the reason on
    /// the line that voided the cited entry -- its DECISION's `supersede`
    /// -- and no line's entry is the reason.
    #[test]
    fn a_forks_supersede_reason_is_logged_on_the_line_that_voids() {
        let session = Session::open_with(
            Canned::new(Vec::<Vec<Step>>::new()),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        );
        let mut state = session.shared.lock();
        let seed = Patch::Add {
            id: entry_id("interview-t1-f1366-0"),
            content: "decision: report broken links as `file:line: target`".to_owned(),
            provenance: from_fork(0),
        };
        assert_eq!(applied(&mut state, &[seed], 1).0, log::ForkOutcome::Value);
        let (outcome, lines) = folded(
            &mut state,
            "DECISION: report broken links as JSON Lines\n\
             PLAN: NONE\n\
             SUPERSEDE: interview-t1-f1366-0 — replaced by this turn's DECISION\n",
            2,
            2053,
        );
        assert_eq!(outcome, log::ForkOutcome::Value);
        let logged: Vec<log::Event> = lines
            .iter()
            .map(|event| {
                line_of(&Logged {
                    seq: 0,
                    t: 0,
                    event: event.clone(),
                })
                .event
            })
            .collect();
        assert!(
            logged.iter().any(|line| matches!(
                line,
                log::Event::Patch {
                    op: log::PatchOp::Supersede,
                    supersedes: Some(voids),
                    reason: Some(reason),
                    entry,
                    ..
                } if voids == "interview-t1-f1366-0"
                    && reason == "replaced by this turn's DECISION"
                    && entry.text == "decision: report broken links as JSON Lines"
            )),
            "{logged:#?}"
        );
        assert!(
            !logged.iter().any(|line| matches!(
                line,
                log::Event::Patch { entry, .. } if entry.text.contains("replaced by this turn")
            )),
            "{logged:#?}"
        );
    }

    /// #562, from the meditation smoke run: a pair whose replacement is itself a
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
        )
        .patches;
        let retired: Vec<&str> = patches
            .iter()
            .map(|patch| match patch {
                Patch::Retire { target, .. } => target.as_str(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(retired, ["interview-t2-0", "interview-t2-1"]);
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
        record_read_back(&session)
            .into_iter()
            .filter(|event| matches!(
                event,
                crate::formats::record::Event::PrefixChanged { at_request, .. } if forks.contains(at_request)
            ))
            .collect()
    }

    /// The record `session`'s log projects to, rendered and read back as
    /// serve does before it writes one (#654's run 2 lost its record to a
    /// row the reader refused).
    fn record_read_back<S: Streaming>(session: &Session<S>) -> Vec<crate::formats::record::Event> {
        use crate::formats::record::{self, Record};
        let lines: Vec<log::Line> = session.events_from(0).iter().map(line_of).collect();
        let events = super::super::projection::project(&lines, &regime(), None)
            .expect("the log projects")
            .events;
        let rendered = record::render(&Record {
            events: events.clone(),
        });
        record::parse(&rendered)
            .unwrap_or_else(|why| panic!("the projected record does not read back: {why:?}"));
        events
    }

    /// #157, from the meditation smoke runs: a whole-trunk fork's head is the
    /// trunk's, so the second fork's request -- its head moved by the turns
    /// between -- is verified against the rebuilt trunk and its move named
    /// as the trunk's are, attributed (run 1's q/2142 was named
    /// `unattributed`; run 2's audit lane named nothing, and the record was
    /// refused); a `last_turn` fork's head is not rebuilt, and its move is
    /// named unattributed. Either way the record reads back.
    #[test]
    fn a_whole_trunk_forks_head_move_is_attributed_and_the_record_reads_back() {
        let rows = fork_prefix_rows(None);
        assert!(
            matches!(
                &rows[..],
                [crate::formats::record::Event::PrefixChanged { reason, .. }]
                    if *reason != crate::formats::record::PrefixReason::Unattributed
            ),
            "{rows:?}"
        );
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

    /// #556: under `cache_ttl = "1h"` the trunk's request writes every
    /// breakpoint for an hour; its fork, made through the transport's fork
    /// form, reads the trunk's system at an hour and writes its own tail for
    /// five minutes only.
    #[test]
    fn a_hosted_fork_never_writes_its_tail_for_an_hour() {
        use crate::client::anthropic::tests::{answering, streamed};
        use crate::client::anthropic::{Options, Ttl, Ttls};
        use crate::client::stream::HttpStream;
        use crate::client::stub::Stub;
        use crate::client::transport::Endpoint;

        let stub = Stub::serving(vec![
            streamed(&answering(SCOPED, (40, 0, 900))),
            streamed(&answering(DECIDED, (30, 900, 20))),
        ])
        .expect("loopback");
        let transport = HttpStream::new(Endpoint::parse(&stub.url()).expect("an endpoint"))
            .with_anthropic(Options {
                ttl: Ttls::all(Ttl::Hour),
                ..Options::default()
            });
        let forks = transport.for_forks().expect("a fork asks differently");
        let session = Session::open_with(
            transport,
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        )
        .forking_through(forks);
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        let bodies: Vec<serde_json::Value> = stub
            .received()
            .iter()
            .map(|body| serde_json::from_str(body).expect("a JSON body"))
            .collect();
        let [trunk, fork] = bodies.as_slice() else {
            panic!("a trunk request and a fork request: {bodies:#?}");
        };
        let hour = serde_json::json!({"type": "ephemeral", "ttl": "1h"});
        let tail = |body: &serde_json::Value| {
            let messages = body["messages"].as_array().expect("messages");
            let content = messages.last().expect("a last message")["content"]
                .as_array()
                .expect("blocks")
                .clone();
            content.last().expect("a last block")["cache_control"].clone()
        };
        assert_eq!(
            (&trunk["system"][0]["cache_control"], tail(trunk)),
            (&hour, hour.clone())
        );
        assert_eq!(
            (&fork["system"][0]["cache_control"], tail(fork)),
            (&hour, serde_json::json!({"type": "ephemeral"}))
        );
    }

    /// #555: on a hosted API a fork reuses the trunk's prefix through the
    /// prompt cache: each response row carries its cache reads and writes,
    /// and the fork row the share of its prompt read from the cache.
    #[test]
    fn a_hosted_forks_row_names_its_cache_read_share() {
        use crate::client::anthropic::tests::{answering, streamed};
        use crate::client::stream::HttpStream;
        use crate::client::stub::Stub;
        use crate::client::transport::Endpoint;
        use crate::formats::record::Event as Row;

        let stub = Stub::serving(vec![
            streamed(&answering(SCOPED, (40, 0, 900))),
            streamed(&answering(DECIDED, (30, 900, 20))),
        ])
        .expect("loopback");
        let transport = HttpStream::new(Endpoint::parse(&stub.url()).expect("an endpoint"))
            .with_anthropic(crate::client::anthropic::Options::default());
        let session = Session::open_with(
            transport,
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
        let lines: Vec<log::Line> = log.iter().map(line_of).collect();
        let projected =
            crate::drive::projection::project(&lines, &regime(), None).expect("projected");
        let cache: Vec<_> = projected
            .events
            .iter()
            .filter_map(|row| match row {
                Row::Response {
                    cache_read,
                    cache_creation_5m,
                    ..
                } => Some((*cache_read, *cache_creation_5m)),
                _ => None,
            })
            .collect();
        assert_eq!(
            cache,
            [(Some(0), Some(900)), (Some(900), Some(20))],
            "{:#?}",
            projected.unspellable
        );
        let share = projected.events.iter().find_map(|row| match row {
            Row::Fork {
                cache_read_share, ..
            } => Some(cache_read_share.clone()),
            _ => None,
        });
        // 900 read of 30 + 900 + 20 = 950.
        assert_eq!(
            share
                .unwrap_or_else(|| panic!("no fork row: {:#?}", projected.unspellable))
                .as_ref()
                .map(|share| share.as_str().to_owned()),
            Some("0.9474".to_owned())
        );
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
            session.declare_meditation(None),
            Err(Rejected::Refused(Refusal::NothingToMeditate))
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
            session.declare_meditation(None),
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
                    CommandKind::DeclareMeditation,
                    Refusal::NothingToMeditate,
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

    /// A phase document, as a log line carries it (#787).
    fn a_phase_document() -> log::PhaseDocument {
        log::PhaseDocument {
            name: "build".to_owned(),
            digest: "b".repeat(64),
            transitions: vec![log::PhaseOption {
                to: "test".to_owned(),
                kind: "suggested".to_owned(),
            }],
            ground_rules: "Make the change.".to_owned(),
        }
    }

    /// One of every event, and a match with no wildcard that names each
    /// variant: an event added to [`Event`] fails to compile here until it
    /// has a sample, and so a line. One entry per variant is its length.
    #[allow(clippy::too_many_lines)]
    fn one_of_every_event() -> Vec<Logged> {
        let events = vec![
            Event::Started {
                phase_document: None,
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
                notes_in_the_ask: true,
                build: Some(("a".repeat(40), false)),
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
                beside: None,
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
                pinned: None,
                displaces: false,
                cut: None,
                timing: Timing::Gap,
                view_overridden: None,
                reserved: None,
            },
            Event::Requested {
                turn: 1,
                lane: Lane::Interview,
                head_sha256: "b".repeat(64),
                fork: Some(20),
                max_tokens: 8192,
                beside: None,
            },
            Event::ForkSettled {
                trailer: None,
                fork: 20,
                outcome: log::ForkOutcome::Value,
                prompt_tokens: Some(281),
                wall_ms: Some(480),
                refused: None,
                pool: None,
                not_run: None,
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
                reason: None,
            },
            Event::Meditated {
                phase_document: None,
                newest_turn_tokens: None,
                refilled_anyway: None,
                operator_changes: None,
                unaddressed: None,
                at_turn: 1,
                reason: crate::meditation::Reason::Operator,
                prefix_hash_before: "c".repeat(64),
                prefix_hash_after: "d".repeat(64),
                render: "# regime\n".to_owned(),
                carried_entries: 1,
                tail_tokens: 0,
                carried_turns: 0,
                carried_tokens: 0,
                phase: None,
                tool_outputs: log::MeditationToolOutputs::Evict,
                outputs: None,
                background: None,
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
                    key: "compaction-1/message-2".to_owned(),
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
                to: None,
                from: None,
            },
            Event::Reminded {
                turn: 2,
                text: "Anything you meant to record?".to_owned(),
            },
            Event::Audited {
                after_meditation: None,
                of_turn: 1,
                at: 3,
                template: crate::dogma::Template::AuditQHuman,
                question: "audit the notes".to_owned(),
            },
            Event::Flagged {
                turn: 2,
                entry: "interview-t1-0".to_owned(),
                note: "check this".to_owned(),
            },
            Event::PatchRefused {
                fork: Some(20),
                lane: None,
                op: log::PatchOp::Supersede,
                entry: "operator-9".to_owned(),
            },
            Event::PhaseRuled {
                call: Some("call-p".to_owned()),
                choice: PhaseChoice::Keep,
                from: Some("plan".to_owned()),
                to: "build".to_owned(),
                phase_document: Some(a_phase_document()),
                by: None,
            },
            Event::MeditationHeld {
                at_turn: 2,
                reason: crate::meditation::Reason::Operator,
                unaddressed: vec!["interview-t1-0".to_owned()],
            },
            Event::PhaseNoted {
                turn: 3,
                text: "The session is now in the build phase.".to_owned(),
            },
            // #788's three.
            Event::PhaseProposed {
                by: Key::Model,
                call: Some("call-p".to_owned()),
                from: Some("plan".to_owned()),
                to: "build".to_owned(),
                reason: Some("the plan is settled".to_owned()),
                meditate: None,
                evidence: log::PhaseEvidence {
                    phase_steps: 3,
                    phase_tokens: 900,
                    tokens_since_meditation: 900,
                    meditated_by_hand: false,
                },
            },
            Event::PhaseAnswered {
                call: "call-a".to_owned(),
                accepted: true,
                reason: None,
            },
            Event::PhaseStarted {
                phase: "plan".to_owned(),
                phase_document: None,
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
                Event::Meditated { .. } => 23,
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
                Event::PhaseRuled { .. } => 36,
                Event::Flagged { .. } => 37,
                Event::PatchRefused { .. } => 38,
                Event::MeditationHeld { .. } => 39,
                Event::PhaseNoted { .. } => 40,
                Event::PhaseProposed { .. } => 41,
                Event::PhaseAnswered { .. } => 42,
                Event::PhaseStarted { .. } => 43,
            });
        }
        assert_eq!(kinds.len(), 44, "a variant has no sample");
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
                phase_document: None,
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
                notes_in_the_ask: Some(true),
                build_commit: Some("a".repeat(40)),
                build_dirty: Some(false),
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
                beside: None,
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
                after_meditation: None,
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
                cut: None,
                timing: None,
                view_overridden: None,
                prefix_shared: None,
                reserved: None,
            },
            log::Event::Request {
                turn: 1,
                lane: log::Lane::Interview,
                head_sha256: Some("b".repeat(64)),
                fork: Some(20),
                max_tokens: Some(8192),
                beside: None,
            },
            log::Event::ForkSettled {
                trailer: None,
                fork: 20,
                outcome: log::ForkOutcome::Value,
                prompt_tokens: Some(281),
                wall_ms: Some(480),
                refused: None,
                pool: None,
                not_run: None,
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
                reason: None,
            },
            log::Event::Meditation {
                phase_document: None,
                newest_turn_tokens: None,
                refilled_anyway: None,
                operator_changes: None,
                unaddressed: None,
                at_turn: 1,
                reason: log::MeditationReason::Operator,
                prefix_hash_before: "c".repeat(64),
                prefix_hash_after: "d".repeat(64),
                frame: crate::meditation::render::FRAME_VERSION.to_owned(),
                render: "# regime\n".to_owned(),
                carried_entries: 1,
                carried_turns: 0,
                tail_tokens: None,
                carried_tokens: None,
                phase: None,
                tool_outputs: Some(log::MeditationToolOutputs::Evict),
                outputs: None,
                carried_outputs: None,
                carried_output_bytes: None,
                background: None,
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
                    key: "compaction-1/message-2".to_owned(),
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
                to: None,
                from: None,
            },
            log::Event::Reminded {
                turn: 2,
                text: "Anything you meant to record?".to_owned(),
            },
            log::Event::Fork {
                after_meditation: None,
                lane: log::Lane::Audit,
                of_turn: 1,
                at: 3,
                why: log::Warrant::Meditation,
                question: "audit the notes".to_owned(),
                view: None,
                trigger: None,
                role: None,
                seat: None,
                ask: Some("audit_q_human".to_owned()),
                hazard: None,
                cut: None,
                timing: None,
                view_overridden: None,
                prefix_shared: None,
                reserved: None,
            },
            log::Event::Flag {
                turn: 2,
                entry: "interview-t1-0".to_owned(),
                note: "check this".to_owned(),
            },
            log::Event::PatchRefused {
                fork: Some(20),
                lane: None,
                op: log::PatchOp::Supersede,
                entry: "operator-9".to_owned(),
                reason: "operator-entry".to_owned(),
            },
            log::Event::PhaseRuled {
                call: Some("call-p".to_owned()),
                choice: "keep".to_owned(),
                from: Some("plan".to_owned()),
                to: "build".to_owned(),
                phase_document: Some(a_phase_document()),
                by: None,
            },
            log::Event::MeditationHeld {
                at_turn: 2,
                reason: log::MeditationReason::Operator,
                unaddressed: vec!["interview-t1-0".to_owned()],
            },
            log::Event::PhaseNoted {
                turn: 3,
                text: "The session is now in the build phase.".to_owned(),
            },
            log::Event::PhaseProposed {
                by: "model".to_owned(),
                call: Some("call-p".to_owned()),
                from: Some("plan".to_owned()),
                to: "build".to_owned(),
                reason: Some("the plan is settled".to_owned()),
                meditate: None,
                evidence: log::PhaseEvidence {
                    phase_steps: 3,
                    phase_tokens: 900,
                    tokens_since_meditation: 900,
                    meditated_by_hand: false,
                },
            },
            log::Event::PhaseAnswered {
                call: "call-a".to_owned(),
                accepted: true,
                reason: None,
            },
            log::Event::PhaseStarted {
                phase: "plan".to_owned(),
                phase_document: None,
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
            session.declare_meditation(Some(gap(first, GapEnd::Meditation))),
            Err(Rejected::Refused(Refusal::NothingToMeditate))
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
            session.declare_meditation(Some(gap(second, GapEnd::Meditation))),
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
            "ask cancel declare-meditation end open-tangent close-tangent background ratify-phase \
             edit-entry flag-entry refill-anyway start-phase propose-phase move-phase"
        );
        assert_eq!(
            tags(&Refusal::ALL.iter().map(|it| it.tag()).collect::<Vec<_>>()),
            "in-flight ended nothing-in-flight nothing-to-meditate no-phase-graph not-a-phase \
             already-in-phase no-phase-edge stale tangent-open no-tangent bad-tangent not-the-scope nothing-running no-proposal unknown-entry not-live empty meditation-held nothing-held restates not-unilateral already-asked proposal-pending"
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
        // The notice rides in the ask's own message, before its words (#609).
        let asked = &last[last.len() - 1];
        assert_eq!(asked.content, "is it ready?");
        assert_eq!(asked.role, Role::User);
        let [note] = asked.notes.as_slice() else {
            panic!("one notice: {asked:?}");
        };
        assert!(note.starts_with(&format!("<task-notification>\n<task-id>{id}</task-id>")));
        assert!(note.contains("<status>completed</status>"));
        assert!(note.contains("<output-tail truncated=\"false\">ready\n</output-tail>"));
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

    /// A job still running when the session ends (#614) is stopped, and its
    /// end is logged `cancelled` before the session's `ended`, which stays
    /// the log's last line (#291): a reader that waits for `ended` sees no
    /// job left running. Its reaper logs nothing after.
    #[test]
    fn a_job_the_sessions_end_stops_is_logged_ended_before_ended() {
        let tree = scratch("bg-end");
        let recording = scratch("bg-end-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let session = backgrounding(
            &tree,
            Some(&recording),
            vec![
                vec![in_the_background("call-1", "echo serving; sleep 30")],
                deltas(&["started"]),
            ],
        );
        session.ask("start it", None).expect("accepted");
        let log = wait_until(&session, "the turn", |log| turns_settled(log) == 1);
        let id = lines(&log)[0].background.clone().expect("a job");
        let pid = session.shared.lock().jobs[&id].pid;
        assert!(ended_jobs(&log).is_empty());
        session.end(None).expect("ended");
        let log = wait_until(&session, "the end", |log| {
            log.last().is_some_and(|logged| {
                matches!(
                    logged.event,
                    Event::Settled {
                        to: Settlement::Ended,
                        ..
                    }
                )
            })
        });
        assert_eq!(
            ended_jobs(&log),
            [(id.clone(), log::BackgroundStatus::Cancelled, None)]
        );
        let at = log
            .iter()
            .position(|logged| matches!(logged.event, Event::BackgroundEnded { .. }))
            .expect("the job's end");
        assert_eq!(at, log.len() - 2, "the job's end, then `ended`");
        let (_, status) = super::super::background::files_of(&recording.join("background"), &id);
        assert!(
            std::fs::read_to_string(&status)
                .expect("a status file")
                .contains("\"status\":\"cancelled\"")
        );
        // The group is gone, and its reaper logged nothing after `ended`.
        let until = Instant::now() + Duration::from_secs(5);
        while std::process::Command::new("/bin/kill")
            .args(["-0", &pid.to_string()])
            .output()
            .is_ok_and(|out| out.status.success())
        {
            assert!(Instant::now() < until, "job {pid} still runs");
            std::thread::sleep(Duration::from_millis(50));
        }
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(session.shared.lock().log.len(), log.len());
        reads_whole(&session);
        session.end_commands();
        tidy(&[&tree, &recording]);
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

    /// #734: an output past what a call holds in memory -- 3 MB from `seq` --
    /// is counted whole on its line, its whole kept in the recording from
    /// its spill (the same digest a held output would get), and the model
    /// shown its head and tail capped, the cap off or not.
    #[test]
    fn a_spilled_output_is_kept_whole_from_its_file_and_shown_capped() {
        use crate::drive::output::OutputCap;
        let tree = scratch("cap-spill");
        let recording = scratch("cap-spill-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let whole: String = (1..=400_000)
            .flat_map(|n| [n.to_string(), "\n".to_owned()])
            .collect();
        assert!(whole.len() > 2 * 1024 * 1024);
        let sha256 = crate::digest::sha256_hex(whole.as_bytes());
        for cap in [OutputCap::DEFAULT, OutputCap::Keep] {
            let session = capping_session(&tree, &["seq 1 400000"], cap, Some(&recording));
            session.ask("count", None).expect("accepted");
            let log = wait_until(&session, "the turn to settle", settled);
            let written = lines(&log);
            let line = &written[0];
            assert_eq!(
                line.stdout.as_ref().map(|out| out.bytes),
                Some(whole.len() as u64)
            );
            assert_eq!(line.files.len(), 1);
            assert_eq!(line.files[0].sha256, sha256);
            let kept = recording.join(&line.files[0].path);
            assert_eq!(std::fs::read_to_string(&kept).expect("kept"), whole);
            let shown = line.shown.clone().expect("shown");
            assert!(
                shown.starts_with("Tool output was too large and has been truncated.\n"),
                "{}",
                &shown[..200]
            );
            assert!(shown.len() < 60_000, "{}", shown.len());
            assert!(shown.contains("\n1\n2\n") || shown.contains("1\n2\n3\n"));
            assert!(shown.ends_with("399999\n400000\n"));
            reads_whole(&session);
        }
        tidy(&[&tree, &recording]);
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
                idle: None,
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
            meditations: crate::meditation::policy::Served::default(),
            delivery: log::ForkDelivery::Meditation,
            phases: crate::meditation::phase::PhaseGraph::none(),
            recall: super::super::archive::Recall::Off,
            role: Role::User,
            view: None,
            cadence: Cadence::Gap,
            threshold_bytes: None,
            skip_self_recorded: false,
            mechanical_capture: false,
            prune: None,
            self_capture: None,
            capture_contract: crate::dogma::capture::DEFAULT,
            asks: &crate::dogma::asks::V3,
            capture: crate::dogma::asks::Modality::Fields,
            fork_tail: None,
            fork_slots: 0,
            prefix_pages: PrefixPages::Counted,
            timing: Timing::Gap,
            step: crate::drive::warrants::StepWarrants::DEFAULTS,
            operator_unilateral: true,
        }
    }

    /// A session whose scoping fork leaves three decisions in working memory
    /// and whose meditations `shape_it` configures, playing `rest` after the turn
    /// and its fork (#504); the log once the fork settled.
    fn three_decisions_then(
        shape_it: impl FnOnce(&mut crate::meditation::policy::Served),
        rest: Vec<Vec<Step>>,
    ) -> Session<Canned> {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        shape_it(&mut interview.meditations);
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

    fn meditated(log: &[Logged]) -> bool {
        log.iter()
            .any(|logged| matches!(logged.event, Event::Meditated { .. }))
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

    /// #150's definition of done, item 2, under `reconcile_gate = "refuse"`:
    /// the operator flags an entry and the model's audit answer ignores it
    /// -- two lines for three notes, so no note is addressed. The meditation
    /// re-asks the same reconciling ask once, over the flagged note alone;
    /// that answer ignores it too, so the meditation holds, names the flag, and
    /// refills nothing. An ask is refused while it holds. The operator's
    /// refill anyway refills, carries the flag unaddressed, and is logged on
    /// the meditation line; the log reads back and the record reads back.
    #[test]
    fn under_the_refuse_gate_an_ignored_flag_is_re_asked_once_then_held_until_refilled_anyway() {
        let session = three_decisions_then(
            |meditations| {
                meditations.audit = true;
                meditations.reconcile_gate = crate::meditation::policy::ReconcileGate::Refuse;
            },
            vec![deltas(&["1. KEEP\n2. KEEP\n"]), deltas(&["nothing to say"])],
        );
        let flagged = live_entries(&session)[0].0.clone();
        session.flag_entry(&flagged, "still so?").expect("a flag");
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the hold", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::MeditationHeld { .. }))
        });
        // Asked twice, the second time over the flagged note alone, in the
        // approved reconciling words.
        let asked: Vec<(crate::dogma::Template, String)> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::Audited {
                    template, question, ..
                } => Some((*template, question.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert!(
            asked
                .iter()
                .all(|(template, _)| *template == crate::dogma::Template::AuditQHumanReconcile)
        );
        assert!(asked[1].1.contains("Output 1 lines"), "{}", asked[1].1);
        assert!(
            asked[1].1.ends_with(" [flagged: still so?]"),
            "{}",
            asked[1].1
        );
        assert!(
            log.iter().any(|logged| matches!(
                &logged.event,
                Event::MeditationHeld { unaddressed, .. } if *unaddressed == [flagged.clone()]
            )),
            "held, naming the flag"
        );
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::Meditated { .. }))
        );
        assert_eq!(session.settlement(), Settlement::Awaiting);
        assert_eq!(
            session.ask("go on", None),
            Err(Rejected::Refused(Refusal::MeditationHeld))
        );
        session.refill_anyway().expect("refilled anyway");
        let log = wait_until(&session, "the refill", meditated);
        assert_eq!(
            every_carried(&log),
            [(
                Some(vec![log::OperatorChange {
                    entry: flagged.clone(),
                    kind: "flag".to_owned(),
                }]),
                Some(vec![flagged.clone()])
            )]
        );
        assert!(log.iter().any(|logged| matches!(
            logged.event,
            Event::Meditated {
                refilled_anyway: Some(true),
                ..
            }
        )));
        assert_eq!(
            session.refill_anyway(),
            Err(Rejected::Refused(Refusal::NothingHeld))
        );
        reads_whole(&session);
        record_read_back(&session);
    }

    /// The same session under `reconcile_gate = "carry"`, the default: one
    /// audit, the seam refills, and the flag is recorded `unaddressed`.
    #[test]
    fn under_the_carry_gate_an_ignored_flag_refills_and_is_recorded_unaddressed() {
        let session = three_decisions_then(
            |meditations| meditations.audit = true,
            vec![deltas(&["1. KEEP\n2. KEEP\n"])],
        );
        let flagged = live_entries(&session)[0].0.clone();
        session.flag_entry(&flagged, "still so?").expect("a flag");
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the meditation", meditated);
        assert_eq!(audited_templates(&log).len(), 1);
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::MeditationHeld { .. }))
        );
        assert_eq!(
            every_carried(&log),
            [(
                Some(vec![log::OperatorChange {
                    entry: flagged.clone(),
                    kind: "flag".to_owned(),
                }]),
                Some(vec![flagged])
            )]
        );
        reads_whole(&session);
    }

    /// Under `refuse`, a re-ask that addresses the change -- an `UPDATE` of
    /// the flagged note (#748) -- refills: the gate holds only what stays
    /// unaddressed.
    #[test]
    fn under_the_refuse_gate_a_re_ask_that_addresses_the_change_refills() {
        let session = three_decisions_then(
            |meditations| {
                meditations.audit = true;
                meditations.reconcile_gate = crate::meditation::policy::ReconcileGate::Refuse;
            },
            vec![
                deltas(&["1. KEEP\n2. KEEP\n"]),
                deltas(&["1. UPDATE: still so\n"]),
            ],
        );
        let flagged = live_entries(&session)[0].0.clone();
        session.flag_entry(&flagged, "still so?").expect("a flag");
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the meditation", meditated);
        assert_eq!(audited_templates(&log).len(), 2);
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::MeditationHeld { .. }))
        );
        assert_eq!(every_carried(&log)[0].1, Some(Vec::new()));
        record_read_back(&session);
    }

    /// A background command still running at a meditation (#614) is listed after
    /// the refill's outputs -- its id, its command and its output file --
    /// with whether it printed anything since it started, then since the
    /// last meditation that listed it; a job no longer running is not listed, and
    /// every head rebuilds from the meditation's `background`.
    #[test]
    fn a_meditation_lists_each_background_command_still_running() {
        let dir = scratch("bg-meditation");
        let session = three_decisions_then(|_| {}, Vec::new());
        let output = dir.join("shell-bg_0123abcd.output");
        std::fs::write(&output, "").expect("the output file");
        let job = |id: &str, status| super::super::background::Job {
            id: id.to_owned(),
            command: "python3 -m http.server 8765\n  --directory docs".to_owned(),
            cwd: ".".to_owned(),
            pid: 0,
            output: output.clone(),
            status_file: dir.join(format!("shell-{id}.status")),
            status,
            exit: None,
            stopping: false,
            listed: None,
        };
        {
            let mut state = session.shared.lock();
            let running = job("bg_0123abcd", super::super::background::Status::Running);
            let ended = job("bg_ffffffff", super::super::background::Status::Completed);
            state.jobs.insert(running.id.clone(), running);
            state.jobs.insert(ended.id.clone(), ended);
        }
        let backgrounds = |log: &[Logged]| -> Vec<Option<String>> {
            log.iter()
                .filter_map(|logged| match &logged.event {
                    Event::Meditated { background, .. } => Some(background.clone()),
                    _ => None,
                })
                .collect()
        };
        let row = |since: &str| {
            format!(
                "\n\n<background-tasks>\nThe following background commands were running at \
                 compaction. The summary above does not include their state. Use `task_stop` to \
                 stop one; do not assume they finished.\n- [running, {since}] bg_0123abcd: \
                 python3 -m http.server 8765 --directory docs\n  output file: {}\n\
                 </background-tasks>",
                output.display()
            )
        };
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the first meditation", |log| {
            backgrounds(log).len() == 1
        });
        assert_eq!(backgrounds(&log), [Some(row("no output since it started"))]);
        let head = template().messages.len();
        let refill = session.shared.lock().trunk[head].content.clone();
        assert!(
            refill.ends_with(&row("no output since it started")),
            "{refill}"
        );
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the second meditation", |log| {
            backgrounds(log).len() == 2
        });
        assert_eq!(
            backgrounds(&log)[1],
            Some(row("no new output since the last compaction"))
        );
        std::fs::write(&output, "Serving HTTP on :: port 8765\n").expect("its output");
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the third meditation", |log| {
            backgrounds(log).len() == 3
        });
        assert_eq!(
            backgrounds(&log)[2],
            Some(row("new output since the last compaction"))
        );
        session.shared.lock().jobs.clear();
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the fourth meditation", |log| {
            backgrounds(log).len() == 4
        });
        assert_eq!(
            backgrounds(&log)[3],
            None,
            "nothing running, nothing listed"
        );
        reads_whole(&session);
        every_head_rebuilds(&log);
        tidy(&[&dir]);
    }

    /// #504: with the audit on, an operator's meditation puts the dogma's pinned
    /// ask over the three notes on the `audit` lane, off the warm trunk;
    /// its answer -- one line per note -- folds before the refill: `KEEP`
    /// leaves a note, `UPDATE` supersedes it, `REMOVE` retires it. The audit
    /// is a fork line, a request, an answer, a settling and its patches, the
    /// meditation's line after them, and the log reads back and projects.
    #[test]
    fn an_audited_meditation_folds_its_answer_before_the_refill() {
        let session = three_decisions_then(
            |meditations| meditations.audit = true,
            vec![deltas(&[
                "1. KEEP\n2. UPDATE: for the one team that files bugs\n3. REMOVE — they asked for logins after all\n",
            ])],
        );
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the meditation", meditated);
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
        let meditation_at = log
            .iter()
            .find(|logged| matches!(logged.event, Event::Meditated { .. }))
            .expect("the meditation")
            .seq;
        assert!(audited.0 < meditation_at);
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

    /// #610: a session whose forks answer through the record tools audits
    /// through them too: the tools-route set's ask lists each note as
    /// `<id>\t<note>` and names the tools; a `resolve_entry` call settles
    /// its note, on the `audit` lane, before the refill; and the log reads
    /// back and projects.
    #[test]
    fn a_tools_route_audit_lists_ids_and_settles_through_the_record_tools() {
        // The fork's first decision's id, as the fold names it.
        let settled_id = "interview-t1-0".to_owned();
        let session = three_decisions_then(
            |meditations| meditations.audit = true,
            vec![vec![
                Step::Delta("Settling one.".to_owned()),
                Step::call(
                    0,
                    "call-a",
                    "resolve_entry",
                    &serde_json::json!({ "entry": settled_id, "verdict": "done" }).to_string(),
                ),
            ]],
        );
        let entries = live_entries(&session);
        assert!(
            entries.iter().any(|(id, _)| *id == settled_id),
            "{entries:?}"
        );
        {
            let mut state = session.shared.lock();
            let interview = state.interview.as_mut().expect("memory");
            interview.capture = crate::dogma::asks::Modality::Tools;
            interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        }
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the meditation", meditated);
        let (fork, question) = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Audited { question, .. } => Some((logged.seq, question.clone())),
                _ => None,
            })
            .expect("an audit");
        let listing = entries
            .iter()
            .map(|(id, content)| format!("{id}\t{content}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(
            question,
            crate::dogma::audits::T1
                .fill(
                    crate::dogma::Template::AuditQHuman,
                    crate::meditation::AUDIT_CATEGORY,
                    &listing
                )
                .expect("the ask")
        );
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::Captured { tool, outcome, fork: Some(at), .. }
                if tool == "resolve_entry" && outcome == "resolved" && *at == fork
        )));
        assert!(log.iter().any(|logged| matches!(
            logged.event,
            Event::ForkSettled { fork: at, outcome: log::ForkOutcome::Value, .. } if at == fork
        )));
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::Patched { fork: Some(at), op: log::PatchOp::Resolve, .. } if *at == fork
        )));
        let live: Vec<String> = live_entries(&session)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        assert!(!live.contains(&settled_id), "{live:?}");
        assert_eq!(live.len(), 2);
        reads_whole(&session);
        let lines: Vec<log::Line> = session.events_from(0).iter().map(line_of).collect();
        super::super::projection::project(&lines, &regime(), None).expect("the log projects");
    }

    /// Each live entry's id and content.
    /// A standard-surface session whose one turn reads `notes.md`, writes
    /// then edits `out.txt`, and runs `ls` and `cat notes.md` through
    /// `bash`, a step each, under `mechanical_capture` as `on` says (#784).
    fn touching(tree: &Path, on: bool) -> Session<Canned> {
        std::fs::write(tree.join("notes.md"), "line one\n").expect("the notes");
        let mut shape = looping();
        shape.tools = tool_loop::ToolSurface::Standard.tools_timed(false, None);
        let mut tools = tools(
            Confinement::Unconfined,
            tree,
            &["ls", "cat"],
            None,
            Decider::Decline,
        );
        tools.surface = tool_loop::ToolSurface::Standard;
        tools.read_tool = tool_loop::ToolSurface::Standard.read_tool();
        tools.approvals_off = true;
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.mechanical_capture = on;
        let step = |id: &str, name: &str, args: serde_json::Value| {
            vec![Step::call(0, id, name, &args.to_string())]
        };
        Session::open_with(
            Canned::new([
                step("c1", "read", serde_json::json!({"path": "notes.md"})),
                step(
                    "c2",
                    "write",
                    serde_json::json!({"path": "out.txt", "content": "one\n"}),
                ),
                step(
                    "c3",
                    "edit",
                    serde_json::json!({"file_path": "out.txt", "old_string": "one", "new_string": "two"}),
                ),
                step("c4", "bash", serde_json::json!({"command": "ls"})),
                step("c5", "bash", serde_json::json!({"command": "cat notes.md"})),
                deltas(&["done"]),
            ]),
            shape,
            None,
            Some(tools),
            None,
            Some(interview),
        )
    }

    /// Each mechanical `patch` line: its op, entry id and text.
    fn mechanical_patches(log: &[Logged]) -> Vec<(log::PatchOp, String, String)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Patched {
                    fork: None,
                    lane: Some(lane),
                    op,
                    entry,
                    ..
                } if lane == crate::capture::mechanical::LANE => {
                    Some((*op, entry.id.clone(), entry.text.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// #784: under `mechanical_capture`, each settled call's touches go into
    /// working memory with no model call -- a `read`, a `write`, an `edit`,
    /// an `ls` and a `cat` through `bash` -- one entry per path, relative to
    /// the worktree, a later touch appending its provenance (lane
    /// `mechanical`, the turn, the step); each logged as a `patch` line; and
    /// the log reads back and every head rebuilds.
    #[test]
    fn mechanical_capture_patches_each_calls_touches_one_entry_per_path() {
        let tree = scratch("mechanical-on");
        let session = touching(&tree, true);
        session.ask("look around", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        let mut live = live_entries(&session);
        live.sort();
        assert_eq!(
            live,
            [
                (
                    "mechanical/listed:.".to_owned(),
                    "files: listed .".to_owned()
                ),
                (
                    "mechanical/notes.md".to_owned(),
                    "files: notes.md".to_owned()
                ),
                ("mechanical/out.txt".to_owned(), "files: out.txt".to_owned()),
            ]
        );
        let provenances = |id: &str| -> Vec<(String, u32, u32)> {
            let state = session.shared.lock();
            let object = &state.interview.as_ref().expect("memory").object;
            object
                .entry(&crate::object::EntryId::new(id).expect("an id"))
                .expect("held")
                .provenances
                .iter()
                .map(|at| (at.lane.clone(), at.turn, at.index))
                .collect()
        };
        assert_eq!(
            provenances("mechanical/notes.md"),
            [
                ("mechanical".to_owned(), 1, 1),
                ("mechanical".to_owned(), 1, 5)
            ]
        );
        assert_eq!(
            provenances("mechanical/out.txt"),
            [
                ("mechanical".to_owned(), 1, 2),
                ("mechanical".to_owned(), 1, 3)
            ]
        );
        let patches = mechanical_patches(&log);
        assert_eq!(patches.len(), 5, "{patches:?}");
        assert!(
            patches.iter().all(|(op, _, _)| *op == log::PatchOp::Add),
            "{patches:?}"
        );
        reads_whole(&session);
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// #784: off, the default, nothing is written.
    #[test]
    fn mechanical_capture_off_writes_nothing() {
        let tree = scratch("mechanical-off");
        let session = touching(&tree, false);
        session.ask("look around", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert!(live_entries(&session).is_empty());
        assert!(mechanical_patches(&log).is_empty());
        tidy(&[&tree]);
    }

    /// #784: a delete retires the path's entry.
    #[test]
    fn mechanical_capture_retires_a_deleted_files_entry() {
        let tree = scratch("mechanical-rm");
        std::fs::write(tree.join("gone.txt"), "x\n").expect("a file");
        let mut tools = tools(
            Confinement::Unconfined,
            &tree,
            &["cat", "rm"],
            None,
            Decider::Decline,
        );
        tools.approvals_off = true;
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.mechanical_capture = true;
        let session = Session::open_with(
            Canned::new([
                vec![bash("c1", "cat gone.txt")],
                vec![bash("c2", "rm gone.txt")],
                deltas(&["done"]),
            ]),
            looping(),
            None,
            Some(tools),
            None,
            Some(interview),
        );
        session.ask("tidy up", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert!(
            live_entries(&session).is_empty(),
            "{:?}",
            live_entries(&session)
        );
        let ops: Vec<log::PatchOp> = mechanical_patches(&log)
            .iter()
            .map(|(op, _, _)| *op)
            .collect();
        assert_eq!(ops, [log::PatchOp::Add, log::PatchOp::Retire]);
        reads_whole(&session);
        tidy(&[&tree]);
    }

    /// #784: a file deleted and then written again -- the model regenerating
    /// it -- is back in working memory: a fresh entry under a fresh id, the
    /// retired one left as it was.
    #[test]
    fn mechanical_capture_brings_back_a_file_deleted_and_written_again() {
        let tree = scratch("mechanical-reborn");
        std::fs::write(tree.join("gen.txt"), "x\n").expect("a file");
        let mut tools = tools(
            Confinement::Unconfined,
            &tree,
            &["cat", "rm", "echo"],
            None,
            Decider::Decline,
        );
        tools.approvals_off = true;
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.mechanical_capture = true;
        let session = Session::open_with(
            Canned::new([
                vec![bash("c1", "cat gen.txt")],
                vec![bash("c2", "rm gen.txt")],
                vec![bash("c3", "echo y > gen.txt")],
                vec![bash("c4", "cat gen.txt")],
                vec![bash("c5", "rm gen.txt")],
                vec![bash("c6", "echo z > gen.txt")],
                deltas(&["done"]),
            ]),
            looping(),
            None,
            Some(tools),
            None,
            Some(interview),
        );
        session.ask("regenerate it", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert_eq!(
            live_entries(&session),
            [(
                "mechanical/gen.txt#3".to_owned(),
                "files: gen.txt".to_owned()
            )]
        );
        let patches: Vec<(log::PatchOp, String)> = mechanical_patches(&log)
            .into_iter()
            .map(|(op, id, _)| (op, id))
            .collect();
        assert_eq!(
            patches,
            [
                (log::PatchOp::Add, "mechanical/gen.txt".to_owned()),
                (log::PatchOp::Retire, "mechanical/gen.txt".to_owned()),
                (log::PatchOp::Add, "mechanical/gen.txt#2".to_owned()),
                (log::PatchOp::Add, "mechanical/gen.txt#2".to_owned()),
                (log::PatchOp::Retire, "mechanical/gen.txt#2".to_owned()),
                (log::PatchOp::Add, "mechanical/gen.txt#3".to_owned()),
            ]
        );
        reads_whole(&session);
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    fn live_entries(session: &Session<Canned>) -> Vec<(String, String)> {
        session
            .shared
            .lock()
            .interview
            .as_ref()
            .expect("memory")
            .object
            .live()
            .map(|entry| (entry.id.as_str().to_owned(), entry.content.clone()))
            .collect()
    }

    /// #150's audit: the reconciling ask, the operator's entry third and
    /// marked, the flagged one first, marked with the flag's note.
    fn the_reconciling_ask_marks_them(log: &[Logged]) {
        let (asked, question) = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Audited {
                    template, question, ..
                } => Some((*template, question.clone())),
                _ => None,
            })
            .expect("an audit");
        assert_eq!(asked, crate::dogma::Template::AuditQHumanReconcile);
        assert!(
            question.ends_with(
                "\n3. decision: for the one team that files bugs [edited by the operator]"
            ),
            "{question}"
        );
        assert!(
            question.contains(" [flagged: is this still true?]\n2. "),
            "{question}"
        );
    }

    /// #150's refill: both changes the meditation carried, marked.
    fn the_refill_marks_them(session: &Session<Canned>, operator: &str) {
        // The refill marks the changes the meditation carried.
        let refill = refill_of(session);
        let edited_line = format!(
            "{operator}\tdecision: for the one team that files bugs [edited by the operator]\n"
        );
        assert!(refill.contains(&edited_line), "{refill}");
        assert!(
            refill.contains(" [flagged: is this still true?]\n"),
            "{refill}"
        );
    }

    /// A meditation's operator changes and the entries it left unaddressed.
    type Carried = (Option<Vec<log::OperatorChange>>, Option<Vec<String>>);

    /// Each meditation's operator changes and the entries it left unaddressed.
    fn every_carried(log: &[Logged]) -> Vec<Carried> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Meditated {
                    operator_changes,
                    unaddressed,
                    ..
                } => Some((operator_changes.clone(), unaddressed.clone())),
                _ => None,
            })
            .collect()
    }

    /// Each audit's template, in order.
    fn audited_templates(log: &[Logged]) -> Vec<crate::dogma::Template> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Audited { template, .. } => Some(*template),
                _ => None,
            })
            .collect()
    }

    /// The trunk's refill message, after the head.
    fn refill_of(session: &Session<Canned>) -> String {
        session.trunk()[template().messages.len()].content.clone()
    }

    /// A wait for `n` meditations.
    fn meditations_logged(n: usize) -> impl Fn(&[Logged]) -> bool {
        move |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::Meditated { .. }))
                .count()
                == n
        }
    }

    /// The meditation's operator changes and the entries it left unaddressed.
    fn carried(log: &[Logged]) -> Carried {
        log.iter()
            .find_map(|logged| match &logged.event {
                Event::Meditated {
                    operator_changes,
                    unaddressed,
                    ..
                } => Some((operator_changes.clone(), unaddressed.clone())),
                _ => None,
            })
            .expect("the meditation")
    }

    /// #748: a session reconciling under `gate` whose audits answer
    /// `answers` in turn -- in lines, or through the record tools when
    /// `tools` -- after `change` makes the operator's changes on the three
    /// live entries; the log once the meditation refilled or held.
    fn reconciled(
        gate: crate::meditation::policy::ReconcileGate,
        answers: Vec<Vec<Step>>,
        tools: bool,
        change: impl FnOnce(&Session<Canned>, &[String]),
    ) -> (Session<Canned>, Vec<Logged>) {
        let session = three_decisions_then(
            |meditations| {
                meditations.audit = true;
                meditations.reconcile_gate = gate;
            },
            answers,
        );
        let ids: Vec<String> = live_entries(&session)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        change(&session, &ids);
        if tools {
            let mut state = session.shared.lock();
            let interview = state.interview.as_mut().expect("memory");
            interview.capture = crate::dogma::asks::Modality::Tools;
            interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        }
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the refill or the hold", |log| {
            meditated(log)
                || log
                    .iter()
                    .any(|logged| matches!(logged.event, Event::MeditationHeld { .. }))
                    && session_settled_awaiting(log)
        });
        (session, log)
    }

    /// The operator's entry an edit wrote: the live one with `content`.
    fn operators(session: &Session<Canned>, content: &str) -> String {
        live_entries(session)
            .into_iter()
            .find(|(_, held)| held == content)
            .expect("the operator's entry")
            .0
    }

    /// The entries the meditation held on, or `None` when it did not hold.
    fn held_on(log: &[Logged]) -> Option<Vec<String>> {
        log.iter().find_map(|logged| match &logged.event {
            Event::MeditationHeld { unaddressed, .. } => Some(unaddressed.clone()),
            _ => None,
        })
    }

    /// One `resolve_entry` call on `entry`, as the audit's whole answer.
    fn resolving(entry: &str) -> Vec<Step> {
        vec![
            Step::Delta("Settling it.".to_owned()),
            Step::call(
                0,
                "call-r",
                "resolve_entry",
                &serde_json::json!({ "entry": entry, "verdict": "done" }).to_string(),
            ),
        ]
    }

    /// #748, Dispatch's ruling: a park takes the entry out of live memory
    /// as a retire does, so it settles a flag; an add names no entry and
    /// does not.
    #[test]
    fn v8_a_park_settles_a_flag_as_a_retire_does() {
        let line = |op, id: &str| Event::Patched {
            fork: Some(1),
            lane: None,
            op,
            entry: log::PatchEntry {
                id: id.to_owned(),
                text: "a note".to_owned(),
                category: None,
            },
            supersedes: None,
            tangent: None,
            reason: None,
        };
        for op in [
            log::PatchOp::Park,
            log::PatchOp::Retire,
            log::PatchOp::Resolve,
        ] {
            assert!(settles(&[line(op, "e-1")], "e-1"), "{op:?}");
            assert!(!settles(&[line(op, "e-2")], "e-1"), "{op:?}");
        }
        assert!(!settles(&[line(log::PatchOp::Add, "e-1")], "e-1"));
    }

    const CARRY: crate::meditation::policy::ReconcileGate =
        crate::meditation::policy::ReconcileGate::Carry;
    const REFUSE: crate::meditation::policy::ReconcileGate =
        crate::meditation::policy::ReconcileGate::Refuse;

    /// #748, the fields route, under `carry`: a flag the audit keeps is not
    /// addressed; one it updates or removes is.
    #[test]
    fn v8_under_carry_a_kept_flag_is_unaddressed_and_an_updated_or_removed_one_addressed() {
        let (session, log) = reconciled(
            CARRY,
            vec![deltas(&["1. KEEP\n2. REMOVE — gone\n3. UPDATE: now so\n"])],
            false,
            |session, ids| {
                for id in ids {
                    session.flag_entry(id, "still so?").expect("a flag");
                }
            },
        );
        let ids: Vec<String> = every_carried(&log)[0]
            .0
            .clone()
            .expect("carried")
            .into_iter()
            .map(|change| change.entry)
            .collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(every_carried(&log)[0].1, Some(vec![ids[0].clone()]));
        reads_whole(&session);
        record_read_back(&session);
    }

    /// #748, the fields route, under `refuse`: a flag kept, then given an `UPDATE`
    /// on the re-ask, is addressed, and the meditation refills.
    #[test]
    fn v8_under_refuse_a_kept_flag_is_re_asked_and_an_update_addresses_it() {
        let (session, log) = reconciled(
            REFUSE,
            vec![
                deltas(&["1. KEEP\n2. KEEP\n3. KEEP\n"]),
                deltas(&["1. UPDATE: now so\n"]),
            ],
            false,
            |session, ids| session.flag_entry(&ids[0], "still so?").expect("a flag"),
        );
        assert_eq!(audited_templates(&log).len(), 2);
        assert_eq!(held_on(&log), None);
        assert_eq!(every_carried(&log)[0].1, Some(Vec::new()));
        record_read_back(&session);
    }

    /// #748's named case: under `refuse`, a flag the audit keeps and the
    /// re-ask keeps again holds, naming that entry.
    #[test]
    fn v8_under_refuse_a_flag_kept_twice_holds() {
        let (session, log) = reconciled(
            REFUSE,
            vec![
                deltas(&["1. KEEP\n2. KEEP\n3. KEEP\n"]),
                deltas(&["1. KEEP\n"]),
            ],
            false,
            |session, ids| session.flag_entry(&ids[0], "still so?").expect("a flag"),
        );
        let flagged = live_entries(&session)[0].0.clone();
        assert_eq!(audited_templates(&log).len(), 2);
        assert_eq!(held_on(&log), Some(vec![flagged]));
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::Meditated { .. }))
        );
        reads_whole(&session);
    }

    /// #748, the fields route: an edit the audit keeps is addressed, under
    /// either gate.
    #[test]
    fn v8_a_kept_edit_is_addressed_under_either_gate() {
        for gate in [CARRY, REFUSE] {
            let (session, log) = reconciled(
                gate,
                vec![deltas(&["1. KEEP\n2. KEEP\n3. KEEP\n"])],
                false,
                |session, ids| {
                    session
                        .edit_entry(&ids[1], "decision: the operator's version")
                        .expect("an edit");
                },
            );
            let operator = operators(&session, "decision: the operator's version");
            assert_eq!(held_on(&log), None, "{gate:?}");
            assert_eq!(
                every_carried(&log),
                [(
                    Some(vec![log::OperatorChange {
                        entry: operator,
                        kind: "edit".to_owned(),
                    }]),
                    Some(Vec::new())
                )],
                "{gate:?}"
            );
            record_read_back(&session);
        }
    }

    /// #748, the fields route: an UPDATE over the operator's edit is
    /// refused by the guard and leaves it unaddressed -- carried under
    /// `carry`, re-asked under `refuse`, where a KEEP addresses it.
    #[test]
    fn v8_a_refused_update_over_an_edit_leaves_it_unaddressed_under_either_gate() {
        let edit = |session: &Session<Canned>, ids: &[String]| {
            session
                .edit_entry(&ids[1], "decision: the operator's version")
                .expect("an edit");
        };
        let (session, log) = reconciled(
            CARRY,
            vec![deltas(&[
                "1. KEEP\n2. KEEP\n3. UPDATE: the model's rewrite\n",
            ])],
            false,
            edit,
        );
        let operator = operators(&session, "decision: the operator's version");
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::PatchRefused { entry, .. } if *entry == operator
        )));
        assert_eq!(every_carried(&log)[0].1, Some(vec![operator]));
        let (session, log) = reconciled(
            REFUSE,
            vec![
                deltas(&["1. KEEP\n2. KEEP\n3. UPDATE: the model's rewrite\n"]),
                deltas(&["1. KEEP\n"]),
            ],
            false,
            edit,
        );
        assert_eq!(audited_templates(&log).len(), 2);
        assert_eq!(held_on(&log), None);
        assert_eq!(every_carried(&log)[0].1, Some(Vec::new()));
        record_read_back(&session);
    }

    /// #748, the tools route: a flag the audit resolves is addressed; one
    /// it leaves alone is not -- carried under `carry`, held under
    /// `refuse` once the re-ask leaves it alone too.
    #[test]
    fn v8_tools_route_a_flag_is_addressed_when_a_patch_settles_it() {
        let flagged = "interview-t1-0";
        let flag = |session: &Session<Canned>, _: &[String]| {
            session.flag_entry(flagged, "still so?").expect("a flag");
        };
        for gate in [CARRY, REFUSE] {
            let (session, log) = reconciled(gate, vec![resolving(flagged)], true, flag);
            assert_eq!(held_on(&log), None, "{gate:?}");
            assert_eq!(every_carried(&log)[0].1, Some(Vec::new()), "{gate:?}");
            reads_whole(&session);
        }
        let (_, log) = reconciled(CARRY, vec![deltas(&["Nothing to change."])], true, flag);
        assert_eq!(every_carried(&log)[0].1, Some(vec![flagged.to_owned()]));
        let (_, log) = reconciled(
            REFUSE,
            vec![deltas(&["Nothing to change."]), deltas(&["Still nothing."])],
            true,
            flag,
        );
        assert_eq!(held_on(&log), Some(vec![flagged.to_owned()]));
    }

    /// #748, the tools route: an edit is addressed unless a write over it
    /// was refused -- carried under `carry`, re-asked under `refuse`.
    #[test]
    fn v8_tools_route_an_edit_is_addressed_unless_a_write_over_it_is_refused() {
        let edit = |session: &Session<Canned>, ids: &[String]| {
            session
                .edit_entry(&ids[1], "decision: the operator's version")
                .expect("an edit");
        };
        for gate in [CARRY, REFUSE] {
            let (session, log) =
                reconciled(gate, vec![deltas(&["Nothing to change."])], true, edit);
            assert_eq!(held_on(&log), None, "{gate:?}");
            assert_eq!(every_carried(&log)[0].1, Some(Vec::new()), "{gate:?}");
            reads_whole(&session);
        }
        // The operator's entry's id is the edit's; resolving it is refused.
        let operator = {
            let (session, _) = reconciled(CARRY, vec![deltas(&["x"])], true, edit);
            operators(&session, "decision: the operator's version")
        };
        let (_, log) = reconciled(CARRY, vec![resolving(&operator)], true, edit);
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::PatchRefused { entry, .. } if *entry == operator
        )));
        assert_eq!(every_carried(&log)[0].1, Some(vec![operator.clone()]));
        let (_, log) = reconciled(
            REFUSE,
            vec![resolving(&operator), deltas(&["Nothing to change."])],
            true,
            edit,
        );
        assert_eq!(audited_templates(&log).len(), 2);
        assert_eq!(held_on(&log), None);
        assert_eq!(every_carried(&log)[0].1, Some(Vec::new()));
    }

    /// #150: the operator edits one decision -- a new entry, on the
    /// operator's lane, superseding it -- and flags another; edits and flags
    /// of an entry gone or unknown, or with nothing to say, are refused. The
    /// audit's answer that would rewrite the operator's entry is refused per
    /// patch and the rest of the fold goes on. The audit is the reconciling
    /// ask, each change marked on its note. Its answer keeps the flagged note
    /// and its UPDATE of the edit is refused, so it acted on neither (#748):
    /// the meditation records both changes as carried and unaddressed, and the
    /// refill marks them too.
    #[test]
    fn the_operators_edits_and_flags_are_theirs_and_a_meditation_carries_them() {
        let session = three_decisions_then(
            |meditations| meditations.audit = true,
            vec![deltas(&[
                "1. KEEP\n2. UPDATE: the model's own take\n3. UPDATE: a model rewrite of the operator's note\n",
            ])],
        );
        let before = live_entries(&session);
        let (edited, flagged) = (before[1].0.clone(), before[0].0.clone());
        assert_eq!(
            session.edit_entry("no-such-entry", "x"),
            Err(Rejected::Refused(Refusal::UnknownEntry))
        );
        assert_eq!(
            session.flag_entry(&flagged, "   "),
            Err(Rejected::Refused(Refusal::Empty))
        );
        session
            .edit_entry(&edited, "decision: for the one team that files bugs")
            .expect("an edit");
        assert_eq!(
            session.edit_entry(&edited, "again"),
            Err(Rejected::Refused(Refusal::NotLive)),
            "the edited entry is superseded"
        );
        session
            .flag_entry(&flagged, "is this still true?")
            .expect("a flag");
        let after = live_entries(&session);
        let operator = after
            .iter()
            .find(|(_, content)| content == "decision: for the one team that files bugs")
            .expect("the operator's entry")
            .0
            .clone();
        assert!(
            session
                .shared
                .lock()
                .interview
                .as_ref()
                .expect("memory")
                .object
                .by_operator(&crate::object::EntryId::new(&operator).expect("an id"))
        );

        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the meditation", meditated);
        // The audit listed the operator's entry third, marked, and the
        // flagged one first, marked with the flag's note, in the reconciling
        // ask; its UPDATE of the operator's entry is refused, and the UPDATE
        // of the model's own second note folded.
        the_reconciling_ask_marks_them(&log);
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::PatchRefused { op: log::PatchOp::Supersede, entry, .. } if *entry == operator
        )));
        let held: Vec<String> = live_entries(&session)
            .into_iter()
            .map(|(_, content)| content)
            .collect();
        assert!(held.contains(&"decision: for the one team that files bugs".to_owned()));
        assert!(held.contains(&"the model's own take".to_owned()));
        assert!(
            !held
                .iter()
                .any(|content| content.contains("a model rewrite"))
        );
        // The edit's patch line names the operator's lane; the flag its own.
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::Patched { lane: Some(lane), supersedes: Some(voids), .. }
                if lane == crate::object::OPERATOR && *voids == edited
        )));
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::Flagged { entry, note, .. } if *entry == flagged && note == "is this still true?"
        )));
        assert_eq!(
            carried(&log),
            (
                Some(vec![
                    log::OperatorChange {
                        entry: operator.clone(),
                        kind: "edit".to_owned()
                    },
                    log::OperatorChange {
                        entry: flagged.clone(),
                        kind: "flag".to_owned()
                    },
                ]),
                Some(vec![operator.clone(), flagged.clone()])
            )
        );
        the_refill_marks_them(&session, &operator);
        reads_whole(&session);
        let lines: Vec<log::Line> = session.events_from(0).iter().map(line_of).collect();
        super::super::projection::project(&lines, &regime(), None).expect("the log projects");
    }

    /// #150: an audit whose answer does not parse addresses nothing -- both
    /// changes carried and left open, still marked -- and the next meditation's
    /// reconciling audit, which keeps the edit and updates the flagged note,
    /// addresses them (#748); the meditation after that carries none and asks
    /// the trigger's own ask again.
    #[test]
    fn an_unparsed_audit_leaves_the_changes_open_for_the_next_meditation() {
        let session = three_decisions_then(
            |meditations| meditations.audit = true,
            vec![
                deltas(&["not an audit"]),
                deltas(&["the next answer"]),
                deltas(&["1. UPDATE: still so\n2. KEEP\n3. KEEP\n"]),
                deltas(&["the answer after"]),
                deltas(&["1. KEEP\n2. KEEP\n3. KEEP\n"]),
            ],
        );
        let before = live_entries(&session);
        let (edited, flagged) = (before[1].0.clone(), before[0].0.clone());
        session
            .edit_entry(&edited, "decision: the operator's version")
            .expect("an edit");
        session.flag_entry(&flagged, "still so?").expect("a flag");
        let operator = live_entries(&session)
            .into_iter()
            .find(|(_, content)| content == "decision: the operator's version")
            .expect("the operator's entry")
            .0;
        let changes = Some(vec![
            log::OperatorChange {
                entry: operator.clone(),
                kind: "edit".to_owned(),
            },
            log::OperatorChange {
                entry: flagged.clone(),
                kind: "flag".to_owned(),
            },
        ]);
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "meditation one", meditations_logged(1));
        assert_eq!(
            every_carried(&log),
            [(
                changes.clone(),
                Some(vec![operator.clone(), flagged.clone()])
            )]
        );
        let refill = refill_of(&session);
        assert!(refill.contains(" [edited by the operator]\n"), "{refill}");
        session.ask("go on", None).expect("accepted");
        wait_until(&session, "turn two", |log: &[Logged]| {
            turns_settled(log) == 2
        });
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "meditation two", meditations_logged(2));
        assert_eq!(every_carried(&log)[1], (changes, Some(Vec::new())));
        session.ask("and on", None).expect("accepted");
        wait_until(&session, "turn three", |log: &[Logged]| {
            turns_settled(log) == 3
        });
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "meditation three", meditations_logged(3));
        assert_eq!(every_carried(&log)[2], (None, None));
        assert_eq!(
            audited_templates(&log),
            [
                crate::dogma::Template::AuditQHumanReconcile,
                crate::dogma::Template::AuditQHumanReconcile,
                crate::dogma::Template::AuditQHuman,
            ]
        );
        let refill = refill_of(&session);
        assert!(!refill.contains(" [edited by the operator]"), "{refill}");
        reads_whole(&session);
        // #688: the record reads back; the meditation that addressed both carries
        // them and no `unaddressed`, the one before both.
        let rows: Vec<(Option<usize>, Option<usize>)> = record_read_back(&session)
            .into_iter()
            .filter_map(|row| match row {
                crate::formats::record::Event::Meditation {
                    operator_changes,
                    unaddressed,
                    ..
                } => Some((
                    operator_changes.map(|changes| changes.len()),
                    unaddressed.map(|left| left.len()),
                )),
                _ => None,
            })
            .collect();
        assert_eq!(rows, [(Some(2), Some(2)), (Some(2), None), (None, None)]);
    }

    /// Track 5's gate-and-tail run: the operator flags an entry after a
    /// meditation's refill and declares the meditation again with no turn between, to
    /// reconcile it. The second meditation's audit is cut from the first's refill
    /// -- the trunk the session holds -- and its fork line says so,
    /// `after_meditation` naming that meditation; the log reads back (the run's log was
    /// refused: "a fork of turn 1 after a meditation that followed it") and so does
    /// the record.
    #[test]
    fn a_meditation_right_after_a_meditation_audits_the_refill_and_says_so() {
        let session = three_decisions_then(
            |meditations| meditations.audit = true,
            vec![
                deltas(&["1. KEEP\n2. KEEP\n3. KEEP\n"]),
                deltas(&["1. UPDATE: still so\n2. KEEP\n3. KEEP\n"]),
            ],
        );
        session.declare_meditation(None).expect("admitted");
        wait_until(&session, "meditation one", meditations_logged(1));
        let flagged = live_entries(&session)[0].0.clone();
        session.flag_entry(&flagged, "still so?").expect("a flag");
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "meditation two", meditations_logged(2));
        let first_meditation = log
            .iter()
            .find(|logged| matches!(logged.event, Event::Meditated { .. }))
            .expect("meditation one")
            .seq;
        let audits: Vec<Option<u64>> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::Audited {
                    after_meditation, ..
                } => Some(*after_meditation),
                _ => None,
            })
            .collect();
        assert_eq!(audits, [None, Some(first_meditation)]);
        assert_eq!(
            every_carried(&log)[1].1,
            Some(Vec::new()),
            "the flag addressed"
        );
        reads_whole(&session);
        record_read_back(&session);
    }

    /// #654's meditation smoke run 2: a session with the audit on and two meditations --
    /// two requests on the `audit` lane, the trunk moved between them --
    /// writes a record that reads back, the audit lane's move named.
    #[test]
    fn two_audited_meditations_write_a_record_that_reads_back() {
        let session = three_decisions_then(
            |meditations| meditations.audit = true,
            vec![
                deltas(&["1. KEEP\n2. KEEP\n3. KEEP\n"]),
                deltas(&["the next answer"]),
                deltas(&["1. KEEP\n2. KEEP\n3. KEEP\n"]),
            ],
        );
        session.declare_meditation(None).expect("admitted");
        wait_until(&session, "meditation one", meditated);
        session.ask("go on", None).expect("accepted");
        wait_until(&session, "turn two", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "meditation two", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::Meditated { .. }))
                .count()
                == 2
                && meditated(log)
        });
        let audits: Vec<String> = log
            .iter()
            .filter(|logged| {
                matches!(
                    logged.event,
                    Event::Requested {
                        lane: Lane::Audit,
                        ..
                    }
                )
            })
            .map(|logged| format!("q/{}", logged.seq))
            .collect();
        assert_eq!(audits.len(), 2);
        let rows = record_read_back(&session);
        assert!(
            rows.iter().any(|event| matches!(
                event,
                crate::formats::record::Event::PrefixChanged { at_request, .. } if *at_request == audits[1]
            )),
            "the audit lane's move is named"
        );
    }

    /// #504: an answer that is not the ask's audit -- prose, or a line short
    /// -- folds nothing and settles `unparseable`; the meditation goes on.
    #[test]
    fn an_audit_answer_not_in_the_grammar_folds_nothing_and_the_meditation_goes_on() {
        for answer in ["All three look fine to me.", "1. KEEP\n2. KEEP\n"] {
            let session = three_decisions_then(
                |meditations| meditations.audit = true,
                vec![deltas(&[answer])],
            );
            session.declare_meditation(None).expect("admitted");
            let log = wait_until(&session, "the meditation", meditated);
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

    /// #504: with the pre-warm on, a meditation sends its refilled trunk once with
    /// an output cap of one, and its line carries the warm's timings.
    #[test]
    fn a_warmed_meditation_sends_the_refilled_trunk_once_and_logs_its_timings() {
        let session = three_decisions_then(
            |meditations| meditations.warm = true,
            vec![vec![
                Step::Delta("x".to_owned()),
                Step::Timings(Timings {
                    prompt_n: Some(412),
                    ..Timings::default()
                }),
            ]],
        );
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the meditation", meditated);
        let warm = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Meditated { warm, .. } => Some(warm.clone()),
                _ => None,
            })
            .expect("the meditation");
        assert_eq!(warm.and_then(|timings| timings.prompt_n), Some(412));
        let sent = session.shared.transport.sent();
        let warmed = sent.last().expect("the warm");
        assert_eq!(warmed.limits.max_output_tokens, 1);
        assert_eq!(warmed.messages, session.trunk());
        reads_whole(&session);
    }

    /// #504: an ask while the warm is in flight cancels it: the meditation stands,
    /// logged without the warm's timings, and the ask is admitted.
    #[test]
    fn an_ask_cancels_the_warm_and_the_meditation_stands() {
        let gate = Gate::new();
        let session = three_decisions_then(
            |meditations| meditations.warm = true,
            vec![vec![Step::Hold(gate.clone())], deltas(&["next answer"])],
        );
        session.declare_meditation(None).expect("admitted");
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
                .any(|logged| matches!(logged.event, Event::Meditated { warm: None, .. }))
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

    // -----------------------------------------------------------------------
    // warrants by what a step produced (#783)
    // -----------------------------------------------------------------------

    /// #783 in the gap: a step that read 3000 characters warrants the
    /// EVIDENCE ask, filled with what it read and how much; a step whose
    /// command failed warrants the trio, GOTCHAS, DECISIONS and CONSTRAINTS;
    /// each fork names its warrant, its pinned ask and the step that fired
    /// it, they run after the turn in step order, and the log reads.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_large_read_warrants_evidence_and_a_failed_command_the_trio() {
        let tree = scratch("step-warrants");
        std::fs::write(tree.join("big.txt"), "x".repeat(3000)).expect("a large file");
        let replies = vec![
            vec![bash("call-1", "cat big.txt")],
            vec![bash("call-2", "cat missing.txt")],
            deltas(&["Done."]),
            deltas(&["EXCERPT: xxx\n"]),
            deltas(&["GOTCHA: missing.txt is absent — IMPLICATION: create it first\n"]),
            deltas(&["NONE"]),
            deltas(&["NONE"]),
        ];
        let session = Session::open_with(
            Canned::new(replies),
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
            Some(Interview {
                step: crate::drive::warrants::StepWarrants {
                    evidence: true,
                    trouble: true,
                    ..crate::drive::warrants::StepWarrants::DEFAULTS
                },
                ..interviewing(&[log::Warrant::Evidence, log::Warrant::Trouble])
            }),
        );
        session.ask("look", None).expect("accepted");
        let log = wait_until(&session, "the gap's forks", |log| {
            settled(log) && fork_outcomes(log).len() == 4
        });
        let forked: Vec<(log::Warrant, Option<String>, String)> = session
            .events_from(0)
            .iter()
            .filter_map(|logged| match line_of(logged).event {
                log::Event::Fork {
                    why, ask, trigger, ..
                } => Some((why, ask, trigger.unwrap_or_default())),
                _ => None,
            })
            .collect();
        let request_of = |id: &str| {
            log.iter()
                .find_map(|logged| match &logged.event {
                    Event::ToolCalled(line) if line.id == id => Some(line.request),
                    _ => None,
                })
                .expect("the call")
        };
        let trouble = format!("trouble:{}", request_of("call-2"));
        assert_eq!(
            forked,
            [
                (
                    log::Warrant::Evidence,
                    Some("evidence".to_owned()),
                    "evidence:call-1".to_owned()
                ),
                (
                    log::Warrant::Trouble,
                    Some("gotchas".to_owned()),
                    trouble.clone()
                ),
                (
                    log::Warrant::Trouble,
                    Some("decisions".to_owned()),
                    trouble.clone()
                ),
                (
                    log::Warrant::Trouble,
                    Some("constraints".to_owned()),
                    trouble
                ),
            ]
        );
        let questions: Vec<String> = forks(&log).into_iter().map(|fork| fork.4).collect();
        // Its size is what the model was shown, counted in characters.
        let shown = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::ToolCalled(line) if line.id == "call-1" => line.shown.clone(),
                _ => None,
            })
            .expect("what call-1 showed");
        assert!(
            questions[0].contains(&format!(
                "(bash of cat big.txt, roughly {} characters)",
                shown.chars().count()
            )),
            "{}",
            questions[0]
        );
        assert_eq!(
            questions[1],
            crate::dogma::Template::Gotchas.fill(&[]).expect("the ask")
        );
        assert!(
            live_entries(&session)
                .iter()
                .any(|(_, content)| content.contains("missing.txt is absent")),
            "{:?}",
            live_entries(&session)
        );
        reads_whole(&session);
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    // -----------------------------------------------------------------------
    // interviews during the turn (#782)
    // -----------------------------------------------------------------------

    /// A session whose trunk plays `trunk` and whose forks, on their own
    /// transport, play `forks`; per-class under both warrants, its forks
    /// cut during the turn with `slots` in flight at most.
    fn during_turn(
        tree: &Path,
        trunk: Vec<Vec<Step>>,
        forks: Vec<Vec<Step>>,
        slots: u32,
    ) -> Session<Canned> {
        during_turn_in(tree, (trunk, forks), slots, (None, false))
    }

    /// [`during_turn`], its pool `window` tokens when given, on a server
    /// that shares a prefix's pages when `shared` (#797).
    fn during_turn_in(
        tree: &Path,
        (trunk, forks): (Vec<Vec<Step>>, Vec<Vec<Step>>),
        slots: u32,
        (window, shared): (Option<u64>, bool),
    ) -> Session<Canned> {
        std::fs::write(tree.join("notes.md"), "a note\n").expect("a note");
        std::fs::write(tree.join("lib.rs"), "pub fn f() {}\n").expect("a source");
        let mut shape = looping();
        shape.limits.context_window = window;
        Session::open_with(
            Canned::new(trunk),
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
                cadence: Cadence::PerClass,
                timing: Timing::DuringTurn,
                fork_slots: slots,
                prefix_pages: PrefixPages::of(shared),
                ..interviewing(&[log::Warrant::Scoping, log::Warrant::Read])
            }),
        )
        .forking_through(Canned::new(forks))
    }

    /// Each fork's line: its seq, its `cut` and its `timing`.
    fn cuts(log: &[Logged]) -> Vec<(u64, Option<u64>, Timing)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Forked { cut, timing, .. } => Some((logged.seq, *cut, *timing)),
                _ => None,
            })
            .collect()
    }

    /// The seq of the `tool_call` line of the call `id`.
    fn call_line(log: &[Logged], id: &str) -> u64 {
        log.iter()
            .find_map(|logged| match &logged.event {
                Event::ToolCalled(line) if line.id == id => Some(logged.seq),
                _ => None,
            })
            .expect("the call's line")
    }

    fn turn_settled_at(log: &[Logged]) -> Option<u64> {
        log.iter().find_map(|logged| {
            matches!(logged.event, Event::TurnSettled { .. }).then_some(logged.seq)
        })
    }

    /// #782: a fork a step warrants is cut as the trunk stood after that
    /// step's result and sent at once, beside the trunk's next request,
    /// which names it; it settles, its patches folded into working memory,
    /// while the trunk is still answering; the gap fires it no second time.
    #[test]
    fn a_fork_cut_at_a_step_settles_before_the_turn_does() {
        let tree = scratch("during-turn-settles");
        let gate = Gate::new();
        let session = during_turn(
            &tree,
            vec![
                vec![bash("call-1", "cat notes.md")],
                vec![
                    Step::Hold(gate.clone()),
                    Step::Delta("Done. Next, I will write the schema.".to_owned()),
                ],
            ],
            vec![deltas(&["LEARNED: notes.md holds a note\n"])],
            1,
        );
        session.ask("look around", None).expect("accepted");
        let log = wait_until(&session, "the fork to settle mid-turn", |log| {
            !fork_outcomes(log).is_empty()
        });
        assert_eq!(turn_settled_at(&log), None, "the trunk is still answering");
        assert!(
            live_entries(&session)
                .iter()
                .any(|(_, content)| content.contains("notes.md holds a note")),
            "{:?}",
            live_entries(&session)
        );
        let [(fork, cut, timing)] = cuts(&log)[..] else {
            panic!("one fork: {:?}", cuts(&log));
        };
        assert_eq!(cut, Some(call_line(&log, "call-1")));
        assert_eq!(timing, Timing::DuringTurn);
        assert!(fork > call_line(&log, "call-1"));
        // The trunk's request after the step names the fork beside it.
        let besides: Vec<Option<u32>> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::Requested {
                    lane: Lane::Trunk,
                    beside,
                    ..
                } => Some(*beside),
                _ => None,
            })
            .collect();
        assert_eq!(besides, [Some(0), Some(1)]);
        gate.open();
        let log = wait_until(&session, "the turn", settled);
        assert_eq!(cuts(&log).len(), 1, "the gap fires no second fork");
        assert!(turn_settled_at(&log) > Some(fork));
        reads_whole(&session);
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// #782: at most `slots` forks in flight; the rest queue in step order,
    /// unlogged until sent, and one still queued when the turn settles runs
    /// in the gap -- the session not idle until it settles.
    #[test]
    fn a_fork_past_the_slots_queues_and_runs_in_the_gap() {
        let tree = scratch("during-turn-queues");
        let gate = Gate::new();
        let session = during_turn(
            &tree,
            vec![
                vec![
                    bash("call-1", "cat notes.md"),
                    Step::call(1, "call-2", "bash", r#"{"command":"cat lib.rs"}"#),
                ],
                deltas(&["Done. Next, I will write the schema."]),
            ],
            vec![
                vec![
                    Step::Hold(gate.clone()),
                    Step::Delta("LEARNED: the first\n".to_owned()),
                ],
                deltas(&["LEARNED: the second\n"]),
            ],
            1,
        );
        session.ask("look around", None).expect("accepted");
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        let log = wait_until(&session, "the turn to settle", |log| {
            turn_settled_at(log).is_some()
        });
        assert_eq!(cuts(&log).len(), 1, "the second waits for the slot");
        assert_ne!(session.settlement(), Settlement::Awaiting);
        gate.open();
        let log = wait_until(&session, "both forks", |log| {
            settled(log) && fork_outcomes(log).len() == 2
        });
        let cut = cuts(&log);
        assert_eq!(cut[1].1, Some(call_line(&log, "call-2")));
        assert!(Some(cut[1].0) > turn_settled_at(&log), "it ran in the gap");
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Value; 2]);
        reads_whole(&session);
        tidy(&[&tree]);
    }

    /// #782: the session's end cancels a fork in flight, logged
    /// `cancelled`, and `ended` stays the log's last line.
    #[test]
    fn end_cancels_a_fork_in_flight() {
        let tree = scratch("during-turn-end");
        let gate = Gate::new();
        let session = during_turn(
            &tree,
            vec![
                vec![bash("call-1", "cat notes.md")],
                deltas(&["Done. Next, I will write the schema."]),
            ],
            vec![vec![
                Step::Hold(gate.clone()),
                Step::Delta("LEARNED: never\n".to_owned()),
            ]],
            1,
        );
        session.ask("look around", None).expect("accepted");
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        wait_until(&session, "the turn to settle", |log| {
            turn_settled_at(log).is_some()
        });
        session.end(None).expect("an end while a fork runs");
        let log = wait_until(&session, "the end", |log| {
            matches!(
                log.last(),
                Some(Logged {
                    event: Event::Settled {
                        to: Settlement::Ended,
                        ..
                    },
                    ..
                })
            )
        });
        assert_eq!(fork_outcomes(&log), [log::ForkOutcome::Cancelled]);
        reads_whole(&session);
        tidy(&[&tree]);
    }

    /// #782: a fork that would not fit the pool beside the trunk's prompt
    /// and the forks in flight is refused unsent, the numbers logged.
    #[test]
    fn a_fork_that_will_not_fit_beside_the_trunk_is_refused_with_its_numbers() {
        let tree = scratch("during-turn-pool");
        // A pool the trunk and one fork cannot share.
        let window = 4_500;
        let session = during_turn_in(
            &tree,
            (
                vec![
                    vec![bash("call-1", "cat notes.md")],
                    deltas(&["Done. Next, I will write the schema."]),
                ],
                Vec::new(),
            ),
            1,
            (Some(window), false),
        );
        session.ask("look around", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        let refused: Vec<(Option<String>, Option<log::PoolRoom>)> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::ForkSettled { refused, pool, .. } => Some((refused.clone(), *pool)),
                _ => None,
            })
            .collect();
        let [(Some(why), Some(room))] = &refused[..] else {
            panic!("one fork, refused for the pool: {refused:?}");
        };
        assert_eq!(why, "pool");
        assert_eq!(room.window, window);
        assert!(room.trunk > 0 && room.prompt > 0, "{room:?}");
        assert!(
            room.window
                .saturating_sub(room.trunk + room.beside + room.prompt)
                < CLAMP_FLOOR,
            "{room:?}"
        );
        reads_whole(&session);
        tidy(&[&tree]);
    }

    /// The floor's pool (#797): one paged KV pool of 163,840 tokens.
    const FLOOR_POOL: u64 = 163_840;

    /// #797: a session asked ~100k tokens, whose trunk then reads a file and
    /// ends its turn: its one read cut during the turn beside the trunk's
    /// ~100k-token prompt, in the floor's pool, on a server that shares a
    /// prefix's pages when `shared`. Its log and its forks' outcomes, once
    /// the turn and the fork have settled.
    fn beside_a_100k_trunk(name: &str, shared: bool) -> (Vec<Logged>, Vec<log::ForkOutcome>) {
        let tree = scratch(name);
        let session = during_turn_in(
            &tree,
            (
                vec![vec![bash("call-1", "cat notes.md")], deltas(&["Done."])],
                vec![deltas(&["LEARNED: a long note\n"])],
            ),
            1,
            (Some(FLOOR_POOL), shared),
        );
        // ~100k tokens at the estimate's four characters to one.
        session
            .ask(&"look around, ".repeat(31_000), None)
            .expect("accepted");
        let log = wait_until(&session, "the turn and its fork", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        let outcomes = fork_outcomes(&log);
        reads_whole(&session);
        tidy(&[&tree]);
        (log, outcomes)
    }

    /// A cut fork's reservation and whether its prefix was counted shared.
    fn reservations(log: &[Logged]) -> Vec<Option<(u64, bool)>> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Forked { reserved, .. } => Some(*reserved),
                _ => None,
            })
            .collect()
    }

    /// #797: on a server that shares a prefix's pages, a fork cut beside a
    /// ~100k-token trunk fits the floor's pool: it reserves only its tokens
    /// beyond the trunk's prefix, and its line says so.
    #[test]
    fn a_fork_beside_a_100k_trunk_fits_the_floors_pool_when_its_prefix_is_shared() {
        let (log, outcomes) = beside_a_100k_trunk("during-turn-shared", true);
        assert_eq!(outcomes, [log::ForkOutcome::Value], "{log:#?}");
        let [Some((reserved, true))] = reservations(&log)[..] else {
            panic!("one cut fork, its prefix shared: {:?}", reservations(&log));
        };
        assert!(
            reserved < 1_000,
            "only the question is the fork's own: {reserved}"
        );
    }

    /// #797: without the declaration, the same fork is refused for the
    /// pool, as #782 counts it: a second full copy of the trunk.
    #[test]
    fn without_the_declaration_the_same_fork_is_refused_for_the_pool() {
        let (log, outcomes) = beside_a_100k_trunk("during-turn-unshared", false);
        assert_eq!(outcomes, [log::ForkOutcome::Refused], "{log:#?}");
        let [Some((reserved, false))] = reservations(&log)[..] else {
            panic!("one cut fork, counted in full: {:?}", reservations(&log));
        };
        assert!(reserved > 100_000, "the whole prompt: {reserved}");
        let rooms: Vec<log::PoolRoom> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::ForkSettled { pool, .. } => *pool,
                _ => None,
            })
            .collect();
        let [room] = rooms[..] else {
            panic!("one refusal's numbers: {rooms:?}");
        };
        assert!(room.trunk > 100_000, "the trunk holds ~100k: {room:?}");
        assert_eq!(room.prompt, reserved, "{room:?}");
        assert!(room.trunk + room.prompt > FLOOR_POOL, "{room:?}");
    }

    /// #797: a fork that does not see the trunk shares no prefix with it,
    /// and counts in full whatever the server declares; nor does a trunk
    /// that holds nothing lend its prefix.
    #[test]
    fn a_fork_that_does_not_see_the_trunk_counts_in_full() {
        let at = (100_000, 100_500);
        assert_eq!(reservation(100_200, at, ForkView::Trunk, true), (200, true));
        assert_eq!(
            reservation(100_200, at, ForkView::Trunk, false),
            (100_200, false)
        );
        for view in [ForkView::Last(1), ForkView::Last(2)] {
            assert_eq!(
                reservation(100_200, at, view, true),
                (100_200, false),
                "{view:?}"
            );
        }
        assert_eq!(
            reservation(100_200, (100_000, 0), ForkView::Trunk, true),
            (100_200, false)
        );
        // The trunk holds only part of the cut: the rest is the fork's.
        assert_eq!(
            reservation(100_200, (100_000, 60_000), ForkView::Trunk, true),
            (40_200, true)
        );
    }

    /// #782: serve's start reads the timing against the registry: refused
    /// with no per-step source, or on a server whose slots are too few.
    #[test]
    fn during_turn_is_refused_at_start_without_a_per_step_source_or_slots() {
        use crate::drive::registry::REGISTRY;
        let read = |text: &str, trunk: &str| {
            timing_and_slots(&regimen::parse(text).expect("a regimen"), (REGISTRY, trunk))
        };
        let per_class = "interview_timing = \"during_turn\"\ninterview_warrant = [\"read\"]\n\
                         interview_cadence = \"per_class\"\n";
        assert_eq!(
            read(per_class, "accel24-beellama-qwen27b-q4kxl"),
            Ok((Timing::DuringTurn, 1))
        );
        assert!(read(per_class, "canned").is_err_and(|why| why.contains("serving_slots")));
        assert!(
            read(
                "interview_timing = \"during_turn\"\ninterview_warrant = [\"read\"]\n",
                "accel24-beellama-qwen27b-q4kxl"
            )
            .is_err_and(|why| why.contains("per_class")),
            "the gap cadence warrants nothing at a step"
        );
        assert_eq!(read("", "canned"), Ok((Timing::Gap, 0)));
    }

    /// #782: the forks in flight at once -- `slots − 1` on the trunk's
    /// server, the seat's own count offboard -- and every start refused.
    #[test]
    fn the_fork_slots_are_the_servers_less_the_trunks_and_too_few_are_refused() {
        assert_eq!(fork_slots(Timing::Gap, None, None), Ok(0));
        assert_eq!(fork_slots(Timing::DuringTurn, Some(2), None), Ok(1));
        assert_eq!(fork_slots(Timing::DuringTurn, Some(4), None), Ok(3));
        assert!(fork_slots(Timing::DuringTurn, Some(1), None).is_err_and(|why| why.contains('1')));
        assert!(
            fork_slots(Timing::DuringTurn, None, None)
                .is_err_and(|why| why.contains("serving_slots"))
        );
        // Offboard (#570): the seat's slots, and one is enough.
        assert_eq!(
            fork_slots(Timing::DuringTurn, Some(1), Some(Some(1))),
            Ok(1)
        );
        assert_eq!(fork_slots(Timing::DuringTurn, None, Some(Some(4))), Ok(4));
        assert!(fork_slots(Timing::DuringTurn, Some(2), Some(None)).is_err());
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

    /// #782: the interview timing reads strictly: `gap`, the default, or
    /// `during_turn`; any other word is refused, naming it.
    #[test]
    fn the_interview_timing_reads_strictly() {
        let read = |text: &str| interview_timing(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read(""), Ok(Timing::Gap));
        assert_eq!(read("interview_timing = \"gap\"\n"), Ok(Timing::Gap));
        assert_eq!(
            read("interview_timing = \"during_turn\"\n"),
            Ok(Timing::DuringTurn)
        );
        assert!(
            read("interview_timing = \"soon\"\n").is_err_and(|why| why.contains("soon")),
            "an unknown word is refused"
        );
        assert!(read("interview_timing = 2\n").is_err());
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
    /// after. Under `meditation`, nothing is delivered.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_delivered_note_follows_the_ask_and_stays_on_the_trunk() {
        for (delivery, template_name) in [
            (log::ForkDelivery::Advisory, "FORK_NOTE_ADVISORY"),
            (log::ForkDelivery::Imperative, "FORK_NOTE_IMPERATIVE"),
            (log::ForkDelivery::Meditation, ""),
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
            if delivery == log::ForkDelivery::Meditation {
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
            // The first request: the ask's own message carries the note,
            // before its words (#609).
            let first = &sent[0].messages;
            assert_eq!(first.last(), Some(&noted("go", &[&note])));
            // The second: the note stayed on the trunk in its ask.
            let second = &sent[1].messages;
            assert!(second.contains(&noted("go", &[&note])), "{second:#?}");
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
    /// under a mid-turn delivery it is still carried at the meditation only.
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
            let patches = cited(patches, &object).patches;
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
        assert!(
            state.undelivered.is_empty(),
            "an add waits for the meditation"
        );
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

    /// #598: a session primes only when it forks under a set that primes --
    /// t2 -- and the priming follows the system prompt after a blank line,
    /// once; v3, v4 and t1 never prime.
    #[test]
    fn a_session_is_primed_only_when_it_forks_under_t2() {
        let read = |text: &str| priming(&regimen::parse(text).expect("a regimen"));
        let forks = "interview_warrant = [\"scoping\"]\n";
        assert_eq!(
            read(&format!("fork_asks = \"t2\"\n{forks}")),
            crate::dogma::asks::T2.priming()
        );
        assert!(crate::dogma::asks::T2.priming().is_some());
        assert_eq!(read("fork_asks = \"t2\"\n"), None, "no fork, no priming");
        for set in ["v3", "v4", "t1"] {
            assert_eq!(
                read(&format!("fork_asks = \"{set}\"\n{forks}")),
                None,
                "{set}"
            );
        }
        assert_eq!(read(forks), None, "the default set does not prime");
        assert_eq!(
            primed("the head\n", Some("the priming\n")),
            "the head\n\nthe priming"
        );
        assert_eq!(primed("the head", None), "the head");
    }

    /// #598: a primed session's system prompt is the same bytes on every
    /// request it sends, the trunk's and its forks', from the first to the
    /// last: the priming is never added, moved or changed mid-session.
    #[test]
    fn a_primed_sessions_system_prompt_is_unchanged_across_the_session() {
        let system = primed(HEAD, crate::dogma::asks::T2.priming());
        let mut shape = template();
        shape.messages = vec![Message::new(Role::System, system.clone())];
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        interview.capture = crate::dogma::asks::Modality::Tools;
        interview.asks = &crate::dogma::asks::T2;
        declare_self_capture(&mut shape, Some(&interview));
        let session = Session::open_with(
            Canned::new([
                deltas(&[SCOPED]),
                deltas(&["nothing to record"]),
                deltas(&["second answer"]),
                deltas(&["nothing again"]),
            ]),
            shape,
            None,
            None,
            None,
            Some(interview),
        );
        for ask in ["what are we building?", "and then?"] {
            session.ask_marked(ask, None, true).expect("accepted");
            wait_until(&session, "the fork to settle", |log| {
                settled(log) && !fork_outcomes(log).is_empty()
            });
        }
        let sent = session.shared.transport.sent();
        assert!(sent.len() >= 3, "{} requests", sent.len());
        for request in &sent {
            assert_eq!(request.messages[0].role, Role::System);
            assert_eq!(request.messages[0].content, system);
        }
        assert!(system.ends_with("it is not a new request from the operator."));
        let fork_ask = sent
            .iter()
            .filter_map(|request| request.messages.last())
            .find(|message| message.content.starts_with("<interview>\n"))
            .expect("a fork's ask in the tags");
        assert!(fork_ask.content.trim_end().ends_with("</interview>"));
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
        let render = crate::meditation::render::render(object, None);
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

    /// #22: a meditation while a tangent is open is refused `tangent-open`, as is
    /// a second tangent.
    #[test]
    fn a_meditation_or_a_second_tangent_while_one_is_open_is_refused() {
        let (session, _) = a_tangent_with_four_entries();
        assert!(matches!(
            session.declare_meditation(None),
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

    /// A self-capturing session over a `window`-token context, its meditation
    /// policy `meditations`, playing `acts` (#617).
    fn windowed(
        acts: Vec<Vec<Step>>,
        window: u64,
        meditations: crate::meditation::policy::Served,
    ) -> Session<Canned> {
        let mut interview = interviewing(&[]);
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        interview.meditations = meditations;
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

    fn window_meditations(log: &[Logged]) -> Vec<(u64, u32, Option<log::MeditationFired>)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::Meditated {
                    reason: crate::meditation::Reason::Window,
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
    /// meditation names the prompt that fired it and the window, the log reads
    /// back, and the record projects it.
    #[test]
    fn a_step_that_would_overflow_the_window_meditates_first_and_the_turn_goes_on() {
        let session = windowed(
            two_turns_measuring(90_000),
            100_000,
            crate::meditation::policy::Served::default(),
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
        let meditations = window_meditations(&log);
        assert_eq!(meditations.len(), 1, "{log:#?}");
        let (at, at_turn, fired) = meditations[0];
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
        // The record's meditation row names the prompt that fired it and the
        // window.
        assert!(
            projected.events.iter().any(|event| matches!(
                event,
                crate::formats::record::Event::Meditation {
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

    /// #617: a request the engine refuses as an overflow is meditated once
    /// and sent again on the refilled trunk -- the refusal logged, then the
    /// meditation, then the same turn's request again -- and the turn goes on. A
    /// second overflow fails the turn: once per turn. With nothing in
    /// working memory there is nothing to meditation, and it fails as before.
    #[test]
    fn an_overflow_meditates_once_and_goes_again() {
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
        acts.push(deltas(&["after the meditation"]));
        let once = windowed(
            acts,
            1_000_000,
            crate::meditation::policy::Served::default(),
        );
        let log = turn_two(&once);
        assert_eq!(settled_as(&log), SettleReason::Final);
        let meditations = window_meditations(&log);
        assert_eq!(meditations.len(), 1, "{log:#?}");
        let (at, _, fired) = meditations[0];
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
        let twice = windowed(
            acts,
            1_000_000,
            crate::meditation::policy::Served::default(),
        );
        let log = turn_two(&twice);
        assert_eq!(settled_as(&log), SettleReason::Failed);
        assert_eq!(window_meditations(&log).len(), 1);

        let mut acts = two_turns_measuring(1_000);
        acts.truncate(2);
        acts[0][1] = record(
            "call-1",
            "fact",
            "The cache is flushed every ninety seconds.",
        );
        acts.push(refused());
        let empty = windowed(
            acts,
            1_000_000,
            crate::meditation::policy::Served::default(),
        );
        let log = turn_two(&empty);
        assert_eq!(settled_as(&log), SettleReason::Failed);
        assert!(window_meditations(&log).is_empty());
    }

    /// #617: with room in the window no meditation fires; turned off by the
    /// regimen none fires however full it is; and with nothing in working
    /// memory the request goes as it is.
    #[test]
    fn no_window_meditation_with_room_when_turned_off_or_with_nothing_to_refill_from() {
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
            window_meditations(&log).len()
        };
        let roomy = windowed(
            two_turns_measuring(10_000),
            100_000,
            crate::meditation::policy::Served::default(),
        );
        assert_eq!(run(roomy), 0);
        let off = windowed(
            two_turns_measuring(90_000),
            100_000,
            crate::meditation::policy::Served {
                window_off: true,
                ..crate::meditation::policy::Served::default()
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
        let empty = windowed(acts, 100_000, crate::meditation::policy::Served::default());
        assert_eq!(run(empty), 0);
    }

    /// REVIEW: the request an overflow retries (#617) is the turn's call in
    /// flight, and a cancel reaches it as it reaches any other.
    #[test]
    fn review_a_request_retried_after_an_overflow_can_be_cancelled() {
        const TYPED: &str = r#"{"error":{"code":400,"type":"exceed_context_size_error"}}"#;
        let gate = Gate::new();
        let mut acts = two_turns_measuring(1_000);
        acts.truncate(2);
        acts.push(vec![Step::Reject(400, TYPED.to_owned())]);
        acts.push(vec![
            Step::Delta("Hel".to_owned()),
            Step::Hold(gate.clone()),
            Step::Delta("lo".to_owned()),
        ]);
        let session = windowed(
            acts,
            1_000_000,
            crate::meditation::policy::Served::default(),
        );
        session
            .ask("how are blank lines treated?", None)
            .expect("accepted");
        wait_until(&session, "turn one", settled);
        session.ask("second", None).expect("accepted");
        assert!(
            gate.wait_for_a_waiter(Duration::from_secs(10)),
            "the retried call never reached the gate"
        );
        assert_eq!(window_meditations(&session.events_from(0)).len(), 1);
        let cancelled = session.cancel(2, None);
        gate.open();
        assert_eq!(cancelled, Ok(()), "{:#?}", session.events_from(0));
    }

    /// REVIEW: a meditation's dropped messages are archived (#566) even
    /// when an earlier meditation at the same turn -- the window's, under
    /// the turn (#617) -- already archived messages under the same keys.
    #[test]
    fn review_a_second_meditation_at_the_same_turn_still_archives_what_it_drops() {
        let session = windowed(
            two_turns_measuring(90_000),
            100_000,
            crate::meditation::policy::Served::default(),
        );
        session
            .ask("how are blank lines treated?", None)
            .expect("accepted");
        wait_until(&session, "turn one", settled);
        session
            .ask("where is turn_two_marker defined?", None)
            .expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
                && settled(log)
        });
        assert_eq!(window_meditations(&log).len(), 1, "{log:#?}");
        session.declare_meditation(None).expect("declared");
        assert!(
            !session
                .trunk()
                .iter()
                .any(|m| m.content.contains("turn_two_marker")),
            "the operator's meditation dropped turn two"
        );
        let state = session.shared.lock();
        let found: Vec<String> = state
            .archive
            .literal("turn_two_marker")
            .iter()
            .map(|found| found.item.key.clone())
            .collect();
        assert!(
            !found.is_empty(),
            "turn two's ask left the trunk and is not in the archive"
        );
    }

    /// REVIEW: a turn whose step was meditated under it (#617) settles
    /// `final`, its gap forks off the refilled trunk, and the log reads back.
    #[test]
    fn review_a_gap_fork_after_a_window_meditation_under_its_turn_reads_back() {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.cadence = Cadence::TurnBoundary;
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        let mut shape = template();
        shape.limits.context_window = Some(100_000);
        declare_self_capture(&mut shape, Some(&interview));
        let mut acts = two_turns_measuring(90_000);
        acts.insert(2, deltas(&["NONE"]));
        acts.push(deltas(&["NONE"]));
        let session =
            Session::open_with(Canned::new(acts), shape, None, None, None, Some(interview));
        let forks_settled = |log: &[Logged]| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::ForkSettled { .. }))
                .count()
        };
        session
            .ask("how are blank lines treated?", None)
            .expect("accepted");
        wait_until(&session, "turn one and its fork", |log| {
            settled(log) && forks_settled(log) == 1
        });
        session.ask("second", None).expect("accepted");
        let log = wait_until(&session, "turn two and its fork", |log| {
            settled(log) && forks_settled(log) == 2
        });
        assert_eq!(window_meditations(&log).len(), 1, "{log:#?}");
        reads_whole(&session);
    }

    /// REVIEW: the request an overflow retries (#617) is the same step sent
    /// again, not a step of its own against `max_steps`.
    #[test]
    fn review_an_overflow_retry_does_not_spend_a_step() {
        const TYPED: &str = r#"{"error":{"code":400,"type":"exceed_context_size_error"}}"#;
        const SAID: &str = "The parser drops blank lines before it tokenizes.";
        let tree = scratch("review-retry-step");
        let mut interview = interviewing(&[]);
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        let mut shape = looping();
        shape.limits.context_window = Some(1_000_000);
        declare_self_capture(&mut shape, Some(&interview));
        let acts = vec![
            vec![Step::Delta(SAID.to_owned()), record("call-1", "fact", SAID)],
            deltas(&["done"]),
            vec![bash("b-1", "touch m1")],
            vec![Step::Reject(400, TYPED.to_owned())],
            vec![bash("b-2", "touch m2")],
            deltas(&["finished"]),
        ];
        let session = Session::open_with(
            Canned::new(acts),
            shape,
            None,
            Some(tools(
                Confinement::Unconfined,
                &tree,
                &["touch"],
                Some(3),
                Decider::Decline,
            )),
            None,
            Some(interview),
        );
        session.ask("first", None).expect("accepted");
        wait_until(&session, "turn one", settled);
        session.ask("touch two files", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        assert_eq!(window_meditations(&log).len(), 1, "{log:#?}");
        let m2 = tree.join("m2").exists();
        let reason = settled_as(&log);
        tidy(&[&tree]);
        assert!(m2, "the retried step's call was refused: {reason:?}");
        assert_eq!(reason, Some(SettleReason::Final));
    }

    /// REVIEW: under `prune_meditation = "turn_end"` (#612), a prune made in a
    /// turn whose next step a window meditation ran under (#617) is still
    /// replaced when the turn ends.
    #[test]
    fn review_a_turn_end_prune_survives_a_window_meditation_under_its_turn() {
        const SAID: &str = "The parser drops blank lines before it tokenizes.";
        let tree = scratch("review-prune-window");
        let notes: String = (0..80)
            .map(|n| format!("note line {n}\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(tree.join("notes.md"), &notes).expect("the notes");
        let mut interview = interviewing(&[]);
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        interview.prune = Some(super::super::prune::PruneMeditation::TurnEnd);
        let mut shape = looping();
        shape.tools.push(super::super::prune::definition());
        shape.limits.context_window = Some(100_000);
        declare_self_capture(&mut shape, Some(&interview));
        let acts = vec![
            vec![Step::Delta(SAID.to_owned()), record("call-r", "fact", SAID)],
            deltas(&["done"]),
            vec![bash("call-1", "cat notes.md")],
            vec![
                prune_step("call-2", "cat notes.md"),
                Step::Timings(Timings {
                    prompt_n: Some(90_000),
                    cache_n: Some(0),
                    ..Timings::default()
                }),
            ],
            deltas(&["after"]),
        ];
        let session = Session::open_with(
            Canned::new(acts),
            shape,
            None,
            Some(tools(
                Confinement::Unconfined,
                &tree,
                &["cat"],
                None,
                Decider::Decline,
            )),
            None,
            Some(interview),
        );
        session.ask("first", None).expect("accepted");
        wait_until(&session, "turn one", settled);
        session.ask("read the notes", None).expect("accepted");
        // Turn two settled -- and, the prune now due at its end, the
        // meditation that applies it logged after the settle.
        let log = wait_until(&session, "turn two", |log| {
            (settled(log)
                || log
                    .last()
                    .is_some_and(|logged| matches!(logged.event, Event::Meditated { .. })))
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        tidy(&[&tree]);
        assert_eq!(prunes_in(&log).len(), 1, "{log:#?}");
        let meditations = meditations_in(&log);
        assert!(
            !session.trunk().iter().any(|m| m.content == notes),
            "the pruned result is still on the trunk whole after its turn ended; \
             meditations {meditations:?}, their prunes {:?}",
            meditation_prunes(&log)
        );
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
        assert_eq!(names, ["bash", "update_record", "resolve_entry",]);
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

    /// #689, the operator-edits run (`runs/2026-10-10-operator-edits-agent-driven`,
    /// #690): its `update_record` (`tool_call` 733, the run's own arguments)
    /// superseding the operator's entry was refused over it (`patch.refused`
    /// 732), yet capture 731 read `recorded` and the model was shown
    /// `recorded: r710/…/decision`. Now the capture line and the result the
    /// model is shown name the refusal and its reason, no entry is claimed,
    /// and nothing the model sent is in working memory.
    #[test]
    fn a_capture_refused_over_the_operators_entry_says_so() {
        const FACT: &str = "The parser drops blank lines before it tokenizes.";
        let turn_one = || vec![Step::Delta(FACT.to_owned()), record("call-1", "fact", FACT)];
        // The operator's entry: the turn's fact, edited. Its id is the
        // log's length at the edit, the same in both sessions below.
        let edited = |session: &Session<Canned>| {
            session
                .ask("how are blank lines treated?", None)
                .expect("accepted");
            wait_until(session, "turn one", settled);
            let fact = live_entries(session)[0].0.clone();
            session
                .edit_entry(&fact, "The parser keeps blank lines.")
                .expect("an edit");
            live_entries(session)
                .into_iter()
                .find(|(_, content)| content == "The parser keeps blank lines.")
                .expect("the operator's entry")
                .0
        };
        let probe = self_capturing(vec![turn_one(), deltas(&["done"])], 3);
        let operator = edited(&probe);
        let session = self_capturing(
            vec![
                turn_one(),
                deltas(&["done"]),
                vec![
                    Step::Delta(RUN_CONTENT.to_owned()),
                    Step::call(
                        0,
                        "call-2",
                        "update_record",
                        &serde_json::json!({
                            "content": RUN_CONTENT,
                            "field": "decision",
                            "supersedes": operator,
                        })
                        .to_string(),
                    ),
                ],
                deltas(&["ok"]),
            ],
            3,
        );
        assert_eq!(edited(&session), operator);
        session.ask("say again?", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            log.iter()
                .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                .count()
                == 2
        });
        let recorded = captures(&log);
        assert_eq!(recorded.len(), 2, "{recorded:?}");
        assert_eq!(
            recorded[1],
            ("refused".to_owned(), Vec::new()),
            "{recorded:?}"
        );
        let shown = lines(&log)
            .last()
            .and_then(|line| line.shown.clone())
            .expect("a result shown");
        assert_eq!(shown, format!("refused: operator-entry: {operator}"));
        assert!(
            !live_entries(&session)
                .iter()
                .any(|(_, content)| content == RUN_CONTENT),
        );
        reads_whole(&session);
    }

    /// The content of the operator-edits run's refused `update_record`
    /// (`tool_call` 733), verbatim.
    const RUN_CONTENT: &str = "notes.txt date format is now YYYY-MM-DD HH:MM (datetime.now() \
                               formatted in notes.py main), replacing the earlier date-only \
                               YYYY-MM-DD decision (operator-547).";

    /// #689, the fork route: a fork answering through the capture tools
    /// whose `update_record` would supersede the operator's entry settles
    /// that capture `refused`, naming the reason and the entry; its
    /// `patch.refused` line is kept; and no entry is claimed or written.
    #[test]
    fn a_forks_capture_refused_over_the_operators_entry_says_so() {
        const SAID: &str = "The parser drops blank lines before it tokenizes.";
        let first = || vec![deltas(&[SAID]), vec![record_at(0, "f-1", "fact", SAID)]];
        let edited = |session: &Session<Canned>| {
            session
                .ask_marked("how are blank lines treated?", None, true)
                .expect("accepted");
            wait_until(session, "the fork", |log| {
                settled(log) && !fork_outcomes(log).is_empty()
            });
            let fact = live_entries(session)[0].0.clone();
            session
                .edit_entry(&fact, "The parser keeps blank lines.")
                .expect("an edit");
            live_entries(session)
                .into_iter()
                .find(|(_, content)| content == "The parser keeps blank lines.")
                .expect("the operator's entry")
                .0
        };
        let operator = edited(&forks_through_tools(first()));
        let mut acts = first();
        acts.push(deltas(&[RUN_CONTENT]));
        acts.push(vec![Step::call(
            0,
            "f-2",
            "update_record",
            &serde_json::json!({
                "content": RUN_CONTENT,
                "field": "decision",
                "supersedes": operator,
            })
            .to_string(),
        )]);
        let session = forks_through_tools(acts);
        assert_eq!(edited(&session), operator);
        session
            .ask_marked("and the dates?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the second fork", |log| {
            settled(log) && fork_outcomes(log).len() == 2
        });
        let refused: Vec<(String, Vec<String>, Option<String>)> = log
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::Captured {
                    outcome,
                    entries,
                    why,
                    ..
                } => Some((outcome.clone(), entries.clone(), why.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            refused.last(),
            Some(&(
                "refused".to_owned(),
                Vec::new(),
                Some(format!("operator-entry: {operator}"))
            )),
            "{refused:?}"
        );
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::PatchRefused { entry, .. } if *entry == operator
        )));
        assert!(
            !live_entries(&session)
                .iter()
                .any(|(_, content)| content == RUN_CONTENT),
        );
        reads_whole(&session);
    }

    /// #609, run 3: the operator's ask is what the model saw too, so a fact
    /// quoted from it grounds -- the gate's source counts the turn's ask.
    #[test]
    fn a_fact_quoted_from_the_operators_ask_is_grounded() {
        let session = self_capturing(
            vec![
                vec![
                    Step::Delta("Noted.".to_owned()),
                    record("call-1", "fact", "the dev server must use port 5180"),
                ],
                deltas(&["done"]),
            ],
            3,
        );
        session
            .ask("Remember: the dev server must use port 5180", None)
            .expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        let recorded = captures(&log);
        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0].0, "recorded", "{recorded:?}");
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
        // The set written for the tools (#610).
        interview.asks = &crate::dogma::asks::T1;
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
        // Asked in t1's words: the tools, not the fields.
        let question = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Forked { question, .. } => Some(question.clone()),
                _ => None,
            })
            .expect("a fork");
        assert!(question.contains("update_record"), "{question}");
        assert!(!question.contains("Answer with the fields"), "{question}");
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

    /// A session whose scoping forks answer in fields (the default) with
    /// self-capture on, so a fork's request carries the record tools
    /// (#806), playing `acts`.
    fn forks_on_fields(acts: Vec<Vec<Step>>) -> Session<Canned> {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        let mut shape = template();
        declare_self_capture(&mut shape, Some(&interview));
        Session::open_with(Canned::new(acts), shape, None, None, None, Some(interview))
    }

    /// The fork's settled line: its outcome and the calls it ran nothing
    /// for.
    fn fork_settled(log: &[Logged]) -> (log::ForkOutcome, Option<u64>) {
        log.iter()
            .find_map(|logged| match &logged.event {
                Event::ForkSettled {
                    outcome, not_run, ..
                } => Some((*outcome, *not_run)),
                _ => None,
            })
            .expect("a fork.settled")
    }

    /// The names of the calls a request's pieces logged.
    fn called_names(log: &[Logged]) -> Vec<String> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::ToolCallPiece {
                    name: Some(name), ..
                } => Some(name.clone()),
                _ => None,
            })
            .collect()
    }

    /// #806: on the fields route, a fork that answers only through
    /// `update_record` is folded as on the tools route: `value`, its call
    /// logged, its patch the fork's.
    #[test]
    fn a_fields_fork_answering_only_through_update_record_settles_value_with_its_patches() {
        const SAID: &str = "The parser drops blank lines before it tokenizes.";
        let session = forks_on_fields(vec![
            deltas(&[SAID]),
            vec![record_at(0, "f-1", "fact", SAID)],
        ]);
        session
            .ask_marked("how does the parser treat blank lines?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        reads_whole(&session);
        assert_eq!(fork_settled(&log), (log::ForkOutcome::Value, None));
        assert_eq!(called_names(&log), ["update_record"]);
        let recorded = captures(&log);
        assert_eq!(recorded.len(), 1, "{recorded:?}");
        assert_eq!(recorded[0].0, "recorded");
        assert_eq!(
            patches(&log)
                .into_iter()
                .map(|patched| patched.id)
                .collect::<Vec<_>>(),
            recorded[0].1
        );
    }

    /// #806: on the fields route, a fork that writes its fields and calls
    /// `update_record` beside them has both folded.
    #[test]
    fn a_fields_fork_answering_in_prose_and_a_call_folds_both() {
        const SAID: &str = "The parser drops blank lines before it tokenizes.";
        let session = forks_on_fields(vec![
            deltas(&[SAID]),
            vec![
                Step::Delta("DECISION: a tracker\n".to_owned()),
                record_at(0, "f-1", "fact", SAID),
            ],
        ]);
        session
            .ask_marked("scope it", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        reads_whole(&session);
        assert_eq!(fork_settled(&log), (log::ForkOutcome::Value, None));
        assert_eq!(patches(&log).len(), 2, "{:?}", patches(&log));
        let held = session.shared.lock();
        let object = &held.interview.as_ref().expect("working memory").object;
        assert_eq!(object.live().count(), 2);
    }

    /// #806: on the fields route, a fork that calls `bash` has the call
    /// logged and run nothing, and its settled line says how many calls
    /// ran nothing; with no answer beside it, it is `unparseable`.
    #[test]
    fn a_fields_fork_calling_bash_logs_the_call_runs_nothing_and_says_so() {
        let session = forks_on_fields(vec![
            deltas(&["Scoped."]),
            vec![Step::call(0, "f-1", "bash", r#"{"command":"ls"}"#)],
        ]);
        session
            .ask_marked("scope it", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the fork to settle", |log| {
            settled(log) && !fork_outcomes(log).is_empty()
        });
        reads_whole(&session);
        assert_eq!(called_names(&log), ["bash"]);
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::ToolCalled(_))),
            "nothing a fork calls runs"
        );
        assert_eq!(fork_settled(&log), (log::ForkOutcome::Unparseable, Some(1)));
        assert!(patches(&log).is_empty());
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
    /// reminder, logged, in its own message before the operator's words --
    /// the ask is what the model answers last (run 3); the system prompt is
    /// untouched.
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
        // In the ask's own message, before its words, so the ask is what
        // the model answers (#609, run 3).
        let second = &sent[1].messages;
        assert_eq!(second.last(), Some(&noted("second", &[reminder])));
        assert_eq!(
            second[0], sent[0].messages[0],
            "the system prompt is unchanged"
        );
        reads_whole(&session);
    }

    /// #609's c2: the session's contract set is the one declared, so c2's
    /// parameter descriptions reach the request and c1's absence of them
    /// stays as it was.
    #[test]
    fn self_capture_declares_the_sessions_contract_set() {
        use crate::formats::record::json::Value;
        let described = |set| {
            let mut interview = interviewing(&[]);
            interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
            interview.capture_contract = set;
            let mut shape = looping();
            declare_self_capture(&mut shape, Some(&interview));
            let Value::Object(schema) = &shape.tools[1].schema else {
                panic!("an object schema");
            };
            let Some(Value::Object(properties)) = schema.get("properties") else {
                panic!("properties");
            };
            properties
                .values()
                .filter(|property| {
                    matches!(property, Value::Object(p) if p.contains_key("description"))
                })
                .count()
        };
        assert_eq!(described(&crate::dogma::capture::C1), 0);
        assert_eq!(described(&crate::dogma::capture::C2), 3);
    }

    /// #609's c2: the set is read from the regimen leniently, c1 by default.
    #[test]
    fn the_self_capture_contract_is_read_leniently_and_defaults_to_c1() {
        let read =
            |text: &str| self_capture_contract(&regimen::parse(text).expect("a regimen")).name;
        assert_eq!(read(""), "c1");
        assert_eq!(read("self_capture_contract = \"c2\"\n"), "c2");
        assert_eq!(read("self_capture_contract = \"c9\"\n"), "c1");
        assert_eq!(read("self_capture_contract = 2\n"), "c1");
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
    // derived meditations: the regimen's cadence and budget
    // -----------------------------------------------------------------------

    /// An interview whose regimen declares `meditations`.
    fn meditating(meditations: crate::meditation::policy::Served) -> Interview {
        Interview {
            meditations,
            ..interviewing(&[log::Warrant::Scoping])
        }
    }

    // -----------------------------------------------------------------------
    // two keys (#788)
    // -----------------------------------------------------------------------

    /// A session under `phased()`'s graph with no self-capture, the phase
    /// tool offered because phases are active, `unilateral` its operator's
    /// setting, playing `acts`.
    fn two_keys(acts: Vec<Vec<Step>>, unilateral: bool) -> Session<Canned> {
        let interview = Interview {
            operator_unilateral: unilateral,
            ..phased()
        };
        let mut shape = template();
        declare_phase_tool(&mut shape, Some(&interview));
        Session::open_with(Canned::new(acts), shape, None, None, None, Some(interview))
    }

    fn propose(id: &str, args: &serde_json::Value) -> Step {
        Step::call(0, id, "propose_phase_transition", &args.to_string())
    }

    /// A proposal as its line has it: who, the call, the move and the
    /// evidence.
    type Proposed = (
        Key,
        Option<String>,
        Option<String>,
        String,
        log::PhaseEvidence,
    );

    fn proposed(log: &[Logged]) -> Vec<Proposed> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::PhaseProposed {
                    by,
                    call,
                    from,
                    to,
                    evidence,
                    ..
                } => Some((*by, call.clone(), from.clone(), to.clone(), *evidence)),
                _ => None,
            })
            .collect()
    }

    fn trunk_requests(log: &[Logged], turn: u32) -> usize {
        log.iter()
            .filter(|logged| {
                matches!(logged.event, Event::Requested { turn: at, lane: Lane::Trunk, .. } if at == turn)
            })
            .count()
    }

    /// #788: the model's proposal ends its turn -- one trunk request, the
    /// tool's result saying so, the turn settled -- and is logged with its
    /// evidence; it waits for the operator, whose `keep` moves the phase.
    #[test]
    fn a_model_proposal_ends_its_turn_with_its_evidence() {
        let session = two_keys(
            vec![
                vec![
                    Step::Delta("The plan is settled.".to_owned()),
                    propose(
                        "call-p",
                        &serde_json::json!({"to": "build", "reason": "the plan is settled"}),
                    ),
                ],
                deltas(&["never reached"]),
            ],
            true,
        );
        session.ask("plan it", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        assert_eq!(trunk_requests(&log, 1), 1, "the proposal ended the turn");
        assert!(log.iter().any(|logged| matches!(
            logged.event,
            Event::TurnSettled {
                turn: 1,
                reason: SettleReason::Final
            }
        )));
        let [(by, call, from, to, evidence)] = &proposed(&log)[..] else {
            panic!("one proposal: {:?}", proposed(&log));
        };
        assert_eq!(*by, Key::Model);
        assert_eq!(call.as_deref(), Some("call-p"));
        assert_eq!((from.as_deref(), to.as_str()), (Some("plan"), "build"));
        assert_eq!(evidence.phase_steps, 1);
        assert!(evidence.phase_tokens > 0);
        assert_eq!(evidence.tokens_since_meditation, evidence.phase_tokens);
        assert!(!evidence.meditated_by_hand);
        let shown = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::ToolCalled(line) if line.id == "call-p" => line.shown.clone(),
                _ => None,
            })
            .expect("the proposal's result");
        assert_eq!(shown, proposal_result("build"));
        session
            .ratify_phase("call-p", PhaseChoice::Keep)
            .expect("ruled");
        assert_eq!(session.shared.lock().phase.as_deref(), Some("build"));
        reads_whole(&session);
    }

    /// #788: the operator's proposal is put to the model in a note on its
    /// next ask; the model accepts through the same tool and the phase
    /// moves, its turn going on; or declines with its reason and nothing
    /// moves.
    #[test]
    fn the_operators_proposal_is_accepted_or_declined_by_the_model() {
        for accept in [true, false] {
            let session = two_keys(
                vec![
                    vec![propose(
                        "call-a",
                        &serde_json::json!({"to": "build", "accept": accept, "reason": "the tests pass"}),
                    )],
                    deltas(&["Going on."]),
                ],
                true,
            );
            session.propose_phase("build", false).expect("proposed");
            let log = session.events_from(0);
            let [(by, call, ..)] = &proposed(&log)[..] else {
                panic!("one proposal");
            };
            assert_eq!((*by, call.as_deref()), (Key::Operator, None));
            session.ask("go on", None).expect("accepted");
            let log = wait_until(&session, "the turn", settled);
            assert_eq!(
                trunk_requests(&log, 1),
                2,
                "an answer does not end the turn"
            );
            assert!(
                log.iter().any(|logged| matches!(
                    &logged.event,
                    Event::Notified { turn: 1, text } if *text == operator_proposal_note(Some("plan"), "build")
                )),
                "the note rode the ask"
            );
            assert!(log.iter().any(|logged| matches!(
                &logged.event,
                Event::PhaseAnswered { call, accepted, reason: Some(reason) }
                    if call == "call-a" && *accepted == accept && reason == "the tests pass"
            )));
            let moved = log.iter().any(|logged| matches!(
                &logged.event,
                Event::PhaseRuled { by: Some(Key::Model), choice: PhaseChoice::Keep, to, .. } if to == "build"
            ));
            assert_eq!(moved, accept, "accept {accept}");
            assert_eq!(
                session.shared.lock().phase.as_deref(),
                Some(if accept { "build" } else { "plan" })
            );
            reads_whole(&session);
        }
    }

    /// #788: the operator's proposal says whether the move it proposes
    /// meditates -- on its line -- and when the model accepts, the
    /// meditation that moves the phase runs once the turn has ended.
    #[test]
    fn an_accepted_proposal_to_meditate_moves_through_a_meditation_after_the_turn() {
        const SAID: &str = "The schema has two tables.";
        let mut interview = Interview {
            self_capture: Some(crate::capture::tools::Cadence::DEFAULT),
            ..phased()
        };
        interview.rules = Vec::new();
        let mut shape = template();
        declare_self_capture(&mut shape, Some(&interview));
        let session = Session::open_with(
            Canned::new(vec![
                vec![
                    Step::Delta(SAID.to_owned()),
                    Step::call(
                        0,
                        "call-f",
                        "update_record",
                        &serde_json::json!({ "field": "fact", "content": SAID }).to_string(),
                    ),
                    Step::call(
                        1,
                        "call-a",
                        PHASE_TOOL,
                        &serde_json::json!({"to": "build", "accept": true, "reason": "planned"})
                            .to_string(),
                    ),
                ],
                deltas(&["Going on."]),
            ]),
            shape,
            None,
            None,
            None,
            Some(interview),
        );
        session.propose_phase("build", true).expect("proposed");
        assert!(session.events_from(0).iter().any(|logged| matches!(
            logged.event,
            Event::PhaseProposed {
                by: Key::Operator,
                meditate: Some(true),
                ..
            }
        )));
        session.ask("plan it", None).expect("accepted");
        let log = wait_until(&session, "the meditation", meditated);
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::PhaseRuled { by: Some(Key::Model), choice: PhaseChoice::Meditation, to, .. }
                if to == "build"
        )));
        let settled_at = log
            .iter()
            .find_map(|logged| {
                matches!(logged.event, Event::TurnSettled { .. }).then_some(logged.seq)
            })
            .expect("the turn");
        let meditated_at = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Meditated {
                    phase: Some(moved), ..
                } if moved.to == "build" => Some(logged.seq),
                _ => None,
            })
            .expect("a meditation that moved to build");
        assert!(meditated_at > settled_at, "after the turn");
        assert_eq!(session.shared.lock().phase.as_deref(), Some("build"));
        reads_whole(&session);
    }

    /// #788: the operator moves alone only with `operator_unilateral`; a
    /// move or proposal outside the phase's transitions is refused; the
    /// starting phase is chosen only before the first ask; and the
    /// operator's proposal waits while the model's does.
    #[test]
    fn the_operators_moves_and_their_refusals() {
        let session = two_keys(vec![deltas(&["ok"])], false);
        assert_eq!(
            session.move_phase("build", false),
            Err(Rejected::Refused(Refusal::NotUnilateral))
        );
        let session = two_keys(
            vec![vec![propose(
                "call-p",
                &serde_json::json!({"to": "review", "reason": "the build is done"}),
            )]],
            true,
        );
        assert_eq!(
            session.move_phase("review", false),
            Err(Rejected::Refused(Refusal::NoPhaseEdge))
        );
        assert_eq!(
            session.propose_phase("review", true),
            Err(Rejected::Refused(Refusal::NoPhaseEdge))
        );
        session.start_phase("build").expect("before the first ask");
        assert_eq!(session.shared.lock().phase.as_deref(), Some("build"));
        assert!(session.events_from(0).iter().any(|logged| matches!(
            &logged.event,
            Event::PhaseStarted { phase, .. } if phase == "build"
        )));
        session.ask("build it", None).expect("accepted");
        wait_until(&session, "the turn", settled);
        assert_eq!(
            session.start_phase("plan"),
            Err(Rejected::Refused(Refusal::AlreadyAsked))
        );
        assert_eq!(
            session.propose_phase("review", true),
            Err(Rejected::Refused(Refusal::ProposalPending)),
            "the model's proposal waits on a ruling"
        );
        session.move_phase("review", false).expect("unilateral");
        let log = session.events_from(0);
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::PhaseRuled { call: None, by: Some(Key::Operator), choice: PhaseChoice::Keep, to, .. }
                if to == "review"
        )));
        reads_whole(&session);
    }

    /// An interview under a three-phase graph: plan to build to review.
    fn phased() -> Interview {
        let mut graph = crate::meditation::phase::PhaseGraph::of(&[
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

    /// A self-capturing session over `phased()`'s graph whose first turn
    /// records a fact and proposes `to` (#124); the log once it settled.
    fn proposing(to: &str, rest: Vec<Vec<Step>>) -> (Session<Canned>, Vec<Logged>) {
        proposing_in(phased(), to, rest)
    }

    /// As [`proposing`], under `interview`'s phases.
    fn proposing_in(
        mut interview: Interview,
        to: &str,
        rest: Vec<Vec<Step>>,
    ) -> (Session<Canned>, Vec<Logged>) {
        const SAID: &str = "The schema has two tables.";
        interview.rules = Vec::new();
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        let mut shape = template();
        declare_self_capture(&mut shape, Some(&interview));
        let mut acts = vec![
            vec![
                Step::Delta(SAID.to_owned()),
                Step::call(
                    0,
                    "call-f",
                    "update_record",
                    &serde_json::json!({ "field": "fact", "content": SAID }).to_string(),
                ),
                Step::call(
                    1,
                    "call-p",
                    "propose_phase_transition",
                    &serde_json::json!({ "to": to, "reason": "the plan is settled" }).to_string(),
                ),
            ],
            deltas(&["done"]),
        ];
        acts.extend(rest);
        let session =
            Session::open_with(Canned::new(acts), shape, None, None, None, Some(interview));
        session.ask("plan it", None).expect("accepted");
        let log = wait_until(&session, "the turn", settled);
        (session, log)
    }

    /// The model's proposal as the log has it (#124, #788): its result's
    /// outcome word and why, as the model was shown it, and the move -- from
    /// the `phase.proposed` line, when it is pending, and the phase it named.
    fn proposal_line(log: &[Logged]) -> (String, Option<String>, Option<String>, Option<String>) {
        let (shown, to) = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::ToolCalled(line) if line.name == PHASE_TOOL => Some((
                    line.shown.clone().unwrap_or_default(),
                    serde_json::from_str::<serde_json::Value>(&line.arguments)
                        .ok()
                        .and_then(|args| args["to"].as_str().map(str::to_owned)),
                )),
                _ => None,
            })
            .expect("the proposal's call");
        let (outcome, why) = shown.split_once(": ").unwrap_or((shown.as_str(), ""));
        let why = (outcome == "refused").then(|| why.to_owned());
        let from = log.iter().find_map(|logged| match &logged.event {
            Event::PhaseProposed { from, .. } => from.clone(),
            _ => None,
        });
        (outcome.to_owned(), why, from, to)
    }

    /// #124: a proposal the graph allows is pending -- `proposed`, its move
    /// on the capture line -- and nothing moves until the operator rules;
    /// `continue` moves the phase and keeps the trunk, and the ruling is
    /// spent.
    #[test]
    fn a_proposal_the_graph_allows_waits_and_continue_moves_the_phase_on_the_trunk() {
        let (session, log) = proposing("build", Vec::new());
        assert_eq!(
            proposal_line(&log),
            (
                "proposed".to_owned(),
                None,
                Some("plan".to_owned()),
                Some("build".to_owned())
            )
        );
        assert_eq!(
            session.shared.lock().phase.as_deref(),
            Some("plan"),
            "nothing moved yet"
        );
        let trunk = session.trunk();
        session
            .ratify_phase("call-p", PhaseChoice::Keep)
            .expect("ruled");
        assert_eq!(session.shared.lock().phase.as_deref(), Some("build"));
        assert_eq!(session.trunk(), trunk, "the trunk is kept");
        let log = session.events_from(0);
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::PhaseRuled { call, choice: PhaseChoice::Keep, from: Some(from), to, .. }
                if call.as_deref() == Some("call-p") && from == "plan" && to == "build"
        )));
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::Meditated { .. }))
        );
        assert!(matches!(
            session.ratify_phase("call-p", PhaseChoice::Keep),
            Err(Rejected::Refused(Refusal::NoProposal))
        ));
        reads_whole(&session);
    }

    /// #124: a proposal the graph refuses is `refused` in the graph's word,
    /// as the model is shown, and leaves nothing to rule on.
    #[test]
    fn a_proposal_the_graph_refuses_is_refused_in_its_word() {
        let (session, log) = proposing("review", Vec::new());
        assert_eq!(
            proposal_line(&log),
            (
                "refused".to_owned(),
                Some("no-phase-edge".to_owned()),
                None,
                Some("review".to_owned())
            )
        );
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::ToolCalled(line) if line.shown.as_deref() == Some("refused: no-phase-edge")
        )));
        assert!(matches!(
            session.ratify_phase("call-p", PhaseChoice::Meditation),
            Err(Rejected::Refused(Refusal::NoProposal))
        ));
        reads_whole(&session);
    }

    /// An interview under the starter phase library (#787), opening in `plan`.
    fn in_the_starter_library() -> Interview {
        Interview {
            phases: crate::meditation::phase::PhaseGraph::from_library(
                &crate::meditation::library::Library::starter(),
                "plan",
            )
            .expect("the starter library has a plan phase"),
            ..interviewing(&[log::Warrant::Scoping])
        }
    }

    /// The starter document of the phase `name` (#787).
    fn starter_document(name: &str) -> log::PhaseDocument {
        crate::meditation::library::Library::starter()
            .document(name)
            .expect("a starter phase")
            .logged()
    }

    /// The phase notes the log carries, by turn (#787).
    fn phase_notes(log: &[Logged]) -> Vec<(u32, String)> {
        log.iter()
            .filter_map(|logged| match &logged.event {
                Event::PhaseNoted { turn, text } => Some((*turn, text.clone())),
                _ => None,
            })
            .collect()
    }

    /// #787: a session opens in the phase its regimen names, logs that
    /// phase's document, and -- the head being the session's own -- carries
    /// its ground rules as a note on the first ask, the system prompt
    /// untouched.
    #[test]
    fn a_session_opens_in_a_named_phase_and_notes_its_ground_rules_on_the_first_ask() {
        let (session, log) = proposing_in(in_the_starter_library(), "build", Vec::new());
        let plan = starter_document("plan");
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::Started { phase_document: Some(document), phases: Some((_, _, first)), .. }
                if *document == plan && first == "plan"
        )));
        let note = crate::meditation::render::phase_note("plan", &plan.ground_rules);
        assert_eq!(phase_notes(&log), [(1, note.clone())]);
        let trunk = session.trunk();
        assert_eq!(
            trunk[0].content, HEAD,
            "the system prompt is the head's own"
        );
        assert!(trunk[1].notes.contains(&note), "{:?}", trunk[1].notes);
        reads_whole(&session);
    }

    /// #787: `continue` moves the phase with no meditation, so the new
    /// phase's ground rules ride as a note on the next ask -- once -- and
    /// the ruling names the document.
    #[test]
    fn a_continue_carries_the_new_phases_ground_rules_as_a_note_on_the_next_ask() {
        let (session, _) = proposing_in(
            in_the_starter_library(),
            "build",
            vec![deltas(&["built"]), deltas(&["tested"])],
        );
        session
            .ratify_phase("call-p", PhaseChoice::Keep)
            .expect("ruled");
        let build = starter_document("build");
        assert!(session.events_from(0).iter().any(|logged| matches!(
            &logged.event,
            Event::PhaseRuled { phase_document: Some(document), .. } if *document == build
        )));
        session.ask("build it", None).expect("accepted");
        wait_until(&session, "turn two", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|l| matches!(l.event, Event::Asked { .. }))
                    .count()
                    == 2
        });
        session.ask("and again", None).expect("accepted");
        let log = wait_until(&session, "turn three", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|l| matches!(l.event, Event::Asked { .. }))
                    .count()
                    == 3
        });
        let note = crate::meditation::render::phase_note("build", &build.ground_rules);
        assert_eq!(
            phase_notes(&log).last(),
            Some(&(2, note)),
            "the next ask, and only it"
        );
        assert_eq!(session.trunk()[0].content, HEAD);
        reads_whole(&session);
    }

    /// #787, refining #597: at a meditation the system prompt becomes the
    /// head's plus the current phase's ground rules, and they are nowhere
    /// else -- not in the refill's summary, and no note follows.
    #[test]
    fn a_meditation_puts_the_phases_ground_rules_after_the_system_prompt_and_nowhere_else() {
        let (session, _) =
            proposing_in(in_the_starter_library(), "build", vec![deltas(&["built"])]);
        session
            .ratify_phase("call-p", PhaseChoice::Meditation)
            .expect("ruled");
        let log = wait_until(&session, "the meditation", |log| {
            log.iter()
                .any(|l| matches!(l.event, Event::Meditated { .. }))
        });
        let build = starter_document("build");
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::Meditated { phase_document: Some(document), .. } if *document == build
        )));
        let trunk = session.trunk();
        assert_eq!(
            trunk[0].content,
            format!(
                "{HEAD}\n\nGround rules for the build phase:\n\n{}",
                build.ground_rules
            )
        );
        assert!(
            trunk[1..]
                .iter()
                .all(|message| !message.content.contains(&build.ground_rules)),
            "the rules ride in the system prompt alone"
        );
        session.ask("build it", None).expect("accepted");
        let log = wait_until(&session, "turn two", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|l| matches!(l.event, Event::Asked { .. }))
                    .count()
                    == 2
        });
        assert_eq!(phase_notes(&log).len(), 1, "the opening note, and no other");
        reads_whole(&session);
    }

    /// #124: `meditation` declares the meditation that moves to the proposed phase;
    /// `stay` declines, moving nothing; a ruling naming another call is
    /// `no-proposal`.
    #[test]
    fn ratifying_meditation_moves_the_phase_at_a_meditation_and_stay_declines() {
        let (session, _) = proposing("build", Vec::new());
        assert!(matches!(
            session.ratify_phase("call-other", PhaseChoice::Meditation),
            Err(Rejected::Refused(Refusal::NoProposal))
        ));
        session
            .ratify_phase("call-p", PhaseChoice::Meditation)
            .expect("ruled");
        let log = session.events_from(0);
        assert!(log.iter().any(|logged| matches!(
            &logged.event,
            Event::Meditated { reason: crate::meditation::Reason::Phase, phase: Some(moved), .. }
                if moved.from == "plan" && moved.to == "build"
        )));
        assert_eq!(session.shared.lock().phase.as_deref(), Some("build"));
        reads_whole(&session);

        let (session, _) = proposing("build", Vec::new());
        session
            .ratify_phase("call-p", PhaseChoice::Stay)
            .expect("ruled");
        assert_eq!(session.shared.lock().phase.as_deref(), Some("plan"));
        assert!(
            !session
                .events_from(0)
                .iter()
                .any(|logged| matches!(logged.event, Event::Meditated { .. }))
        );
        reads_whole(&session);
    }

    /// #563: a served session opens in its graph's first phase and logs the
    /// graph; a meditation that names a move the graph allows moves, the refill
    /// renders the new phase, and the meditation line says from and to; a move it
    /// refuses is refused with the graph's reason, logged; with no phase
    /// named the session stays where it is; and the projection rebuilds the
    /// refilled head.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_served_meditation_moves_between_the_phases_its_graph_allows() {
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
                session.declare_meditation_to(None, Some(to)),
                Err(Rejected::Refused(refused)),
                "{to}"
            );
        }
        session
            .declare_meditation_to(None, Some("build"))
            .expect("plan to build is allowed");
        let render = |phase: &str| {
            let held = session.shared.lock();
            let object = &held.interview.as_ref().expect("interviewing").object;
            crate::meditation::render::render(object, Some(phase))
        };
        assert_eq!(
            session.trunk(),
            crate::meditation::render::refill(&template().messages, &render("build"))
        );
        // No phase named: a meditation that stays in `build`.
        session
            .declare_meditation(None)
            .expect("a meditation that stays");
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
                log::Event::Meditation { phase, .. } => Some(phase.clone()),
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

    /// #566, `literal`: a meditation archives what it drops; a later ask that
    /// names it gets one note after it, in Qwen Code's reminder shape, logged
    /// `recalled` with each item's key, digest and score; the note stays on
    /// the trunk and the projection rebuilds the head. Under `off`, nothing.
    #[test]
    fn a_literal_recall_brings_back_what_a_meditation_archived() {
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
            session.declare_meditation(None).expect("a meditation");
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
            // The scoping turn's answer, archived by the meditation, names the
            // schema; its key, which the model reads, says `compaction`, the
            // maintainer's word for it (#583).
            assert_eq!(items[0].key, "compaction-1/message-2");
            assert!(text.contains("(compaction-1/message-2)"), "{text}");
            assert!(!text.contains("seam-"), "{text}");
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
            // The note rides in the ask's message, before its words, on the
            // request and on the trunk (#609).
            assert_eq!(
                asked.messages.last(),
                Some(&noted("how should the `schema` look?", &[text]))
            );
            assert!(session.trunk().iter().any(|m| m.notes.contains(text)));
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
            session.declare_meditation_to(None, Some("build")),
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
        prune: super::super::prune::PruneMeditation,
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
                meditations: crate::meditation::policy::Served {
                    tail_tokens,
                    ..crate::meditation::policy::Served::default()
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

    /// The meditation lines' `pruned`.
    fn meditation_prunes(log: &[Logged]) -> Vec<Option<Vec<String>>> {
        log.iter()
            .filter_map(|logged| match line_of(logged).event {
                log::Event::Meditation { pruned, .. } => Some(pruned),
                _ => None,
            })
            .collect()
    }

    /// #612, `turn_end`: the tool answers at once and the trunk keeps the
    /// result through the turn -- the prefix rule -- then a meditation derived as
    /// the turn settles replaces it, in the kept tail, by its reference
    /// line; the next request carries the line, and the projection rebuilds
    /// every head.
    #[test]
    fn a_pruned_result_is_replaced_by_its_line_at_the_meditation_its_turn_end_derives() {
        let tree = scratch("t612-turn-end");
        let session = pruning(
            &tree,
            super::super::prune::PruneMeditation::TurnEnd,
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
        let log = wait_until(&session, "the prune's meditation", |log| {
            !meditations_in(log).is_empty()
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
        assert_eq!(meditations_in(&log), [(1, log::MeditationReason::Prune)]);
        assert_eq!(meditation_prunes(&log), [Some(vec!["call-1".to_owned()])]);
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

    /// #612, `next` (the default): no meditation is derived for a prune; the
    /// operator's meditation applies it, a total refill carrying its line in the
    /// section under `evict`, as reference. The tool answers a name no call
    /// matches, a result already pruned, and one a meditation compacted away with
    /// what is wrong.
    #[test]
    fn a_pruned_result_waits_for_the_next_meditation_and_the_tool_says_what_it_cannot_prune() {
        let tree = scratch("t612-next");
        let session = pruning(
            &tree,
            super::super::prune::PruneMeditation::Next,
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
        assert!(
            meditations_in(&log).is_empty(),
            "no meditation is derived under `next`"
        );
        session.declare_meditation(None).expect("a meditation");
        let log = wait_until(&session, "the meditation", |log| {
            !meditations_in(log).is_empty()
        });
        assert_eq!(meditation_prunes(&log), [Some(vec!["call-1".to_owned()])]);
        let (state, section, ..) = meditation_outputs_of(&log);
        assert_eq!(state, Some(log::MeditationToolOutputs::Evict));
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
    /// quoting `line two`, whose second runs `echo tail`, and which meditations
    /// every `every` turns at `tail_tokens` under `state` (#553).
    fn meditating_outputs(
        tree: &Path,
        state: log::MeditationToolOutputs,
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
                meditations: crate::meditation::policy::Served {
                    reconcile_gate: crate::meditation::policy::ReconcileGate::Carry,
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

    /// The meditation line's tool-output fields: its state, its section, how many
    /// outputs and how many bytes it carried.
    fn meditation_outputs_of(
        log: &[Logged],
    ) -> (
        Option<log::MeditationToolOutputs>,
        Option<String>,
        Option<u64>,
        Option<u64>,
    ) {
        log.iter()
            .find_map(|logged| match line_of(logged).event {
                log::Event::Meditation {
                    tool_outputs,
                    outputs,
                    carried_outputs,
                    carried_output_bytes,
                    ..
                } => Some((tool_outputs, outputs, carried_outputs, carried_output_bytes)),
                _ => None,
            })
            .expect("a meditation line")
    }

    /// Runs `meditating_outputs`' turns through the meditation and one ask after it:
    /// the log, and the request that ask sent.
    fn through_the_meditation(
        session: &Session<Canned>,
        turns: u32,
    ) -> (Vec<Logged>, RequestShape) {
        session.ask("read the notes", None).expect("accepted");
        // Settled, or -- at a cadence of one -- meditated after the fork.
        wait_until(session, "the first turn and its fork", |log| {
            !fork_outcomes(log).is_empty() && (settled(log) || !meditations_in(log).is_empty())
        });
        if turns == 2 {
            session.ask("echo something", None).expect("accepted");
        }
        wait_until(session, "the meditation", |log| {
            !meditations_in(log).is_empty()
        });
        session.ask("go on", None).expect("accepted");
        let log = wait_until(session, "the turn after the meditation", |log| {
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
    /// compacts away, and the meditation line names the state alone (#553).
    #[test]
    fn evict_carries_no_tool_output_across_the_meditation() {
        let tree = scratch("meditation-evict");
        let session = meditating_outputs(&tree, log::MeditationToolOutputs::Evict, (1, 0), None);
        let (log, sent) = through_the_meditation(&session, 1);
        assert_eq!(
            meditation_outputs_of(&log),
            (Some(log::MeditationToolOutputs::Evict), None, None, None)
        );
        let head = template().messages.len();
        assert!(
            !sent.messages[head]
                .content
                .contains(crate::meditation::outputs::HEADER)
        );
        // Nothing of turn 1 after the refill: the next ask follows it.
        assert_eq!(sent.messages[head + 1].content, "go on");
        every_head_rebuilds(&log);
        tidy(&[&tree]);
    }

    /// After a meditation the system message is the session's first, byte for
    /// byte, and the render rides in a user message after it (#597). A log
    /// written before #597 -- its meditation with no `placement`, its heads hashed
    /// over the render in the system message -- still rebuilds every head,
    /// and the digest still decides: the wrong placement verifies nothing.
    #[test]
    fn the_system_message_survives_a_meditation_and_a_log_from_before_597_still_rebuilds() {
        let tree = scratch("meditation-placement");
        let session = meditating_outputs(&tree, log::MeditationToolOutputs::Evict, (1, 0), None);
        let (log, _) = through_the_meditation(&session, 1);
        let head = template().messages.len();
        let sent = session.shared.transport.sent();
        let after: Vec<&RequestShape> = sent
            .iter()
            .filter(|shape| {
                shape.messages.len() > head
                    && shape.messages[head].content.starts_with("<summary>\n")
            })
            .collect();
        assert!(!after.is_empty(), "a request after the meditation");
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
                        log::Event::Meditation { placement: at, .. } => *at = placement,
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
        let tree = scratch("meditation-keep");
        let session = meditating_outputs(&tree, log::MeditationToolOutputs::Keep, (2, 100), None);
        let (log, sent) = through_the_meditation(&session, 2);
        let (state, section, carried, bytes) = meditation_outputs_of(&log);
        let section = section.expect("a section");
        assert_eq!(state, Some(log::MeditationToolOutputs::Keep));
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
        let tree = scratch("meditation-reference");
        let recording = scratch("meditation-reference-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let session = meditating_outputs(
            &tree,
            log::MeditationToolOutputs::Reference,
            (1, 0),
            Some(&recording),
        );
        let (log, sent) = through_the_meditation(&session, 1);
        let notes = std::fs::read_to_string(tree.join("notes.md")).expect("the notes");
        let sha256 = crate::digest::sha256_hex(notes.as_bytes());
        let saved = recording.join("files").join(&sha256);
        let (state, section, carried, _) = meditation_outputs_of(&log);
        let section = section.expect("a section");
        assert_eq!(state, Some(log::MeditationToolOutputs::Reference));
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
        let tree = scratch("meditation-salient");
        let session = meditating_outputs(&tree, log::MeditationToolOutputs::Salient, (2, 0), None);
        let (log, sent) = through_the_meditation(&session, 2);
        let (state, section, carried, _) = meditation_outputs_of(&log);
        let section = section.expect("a section");
        assert_eq!(state, Some(log::MeditationToolOutputs::Salient));
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

    /// The meditation lines in `log`, as the log writes them: turn and reason.
    fn meditations_in(log: &[Logged]) -> Vec<(u32, log::MeditationReason)> {
        log.iter()
            .filter_map(|logged| match line_of(logged).event {
                log::Event::Meditation {
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
    /// memory, and the meditation fires on its own once the gap is over, logged
    /// `cadence`; the next ask runs on the refilled trunk, and the turn after
    /// it fires the next. Nobody declared either.
    #[test]
    fn a_cadence_fires_a_meditation_on_its_own_after_each_counted_turn() {
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
            Some(meditating(crate::meditation::policy::Served {
                reconcile_gate: crate::meditation::policy::ReconcileGate::Carry,
                every_turns: Some(1),
                at_trunk_tokens: None,
                tail_tokens: 0,
                outputs: log::MeditationToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the cadence's meditation", |log| {
            !meditations_in(log).is_empty()
        });
        assert_eq!(
            meditations_in(&log),
            vec![(1, log::MeditationReason::Cadence)]
        );
        assert!(
            !session.trunk().iter().any(|m| m.content == SCOPED),
            "the meditation carried a turn of the old trunk"
        );
        let refilled = session.trunk();

        session.ask("build it", None).expect("accepted");
        let log = wait_until(&session, "the second meditation", |log| {
            meditations_in(log).len() == 2
        });
        assert_eq!(
            meditations_in(&log),
            vec![
                (1, log::MeditationReason::Cadence),
                (2, log::MeditationReason::Cadence)
            ]
        );
        let sent = session.shared.transport.sent();
        let mut expected = refilled;
        expected.push(user("build it"));
        assert_eq!(
            sent[2].messages, expected,
            "the ask after the meditation runs on the refill"
        );
        assert_eq!(session.settlement(), Settlement::Awaiting);
        reads_whole(&session);
    }

    /// The compaction depth (#552): a meditation with a tail budget keeps the
    /// old trunk's most recent whole turn after the refill, as it sat on the
    /// trunk; the meditation line names the depth and what it kept; the next ask
    /// runs on head, render and tail; the projection rebuilds every head. A
    /// budget no whole turn fits keeps none, and its meditation line names the
    /// newest turn's size for the operator's warning (#693); one that kept a
    /// turn names none.
    #[test]
    fn a_meditation_with_a_tail_budget_keeps_the_most_recent_whole_turns() {
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
                Some(meditating(crate::meditation::policy::Served {
                    reconcile_gate: crate::meditation::policy::ReconcileGate::Carry,
                    every_turns: Some(1),
                    at_trunk_tokens: None,
                    tail_tokens,
                    outputs: log::MeditationToolOutputs::Evict,
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
        let log = wait_until(&session, "the meditation", |log| {
            !meditations_in(log).is_empty()
        });
        let trunk = session.trunk();
        let kept = [
            user("what are we building?"),
            Message::new(Role::Assistant, SCOPED),
        ];
        assert_eq!(trunk[trunk.len() - 2..], kept[..], "{trunk:#?}");
        // The head, the refill message (#597), then the tail.
        assert_eq!(trunk.len(), template().messages.len() + 3);
        let meditation = log
            .iter()
            .find_map(|logged| match line_of(logged).event {
                log::Event::Meditation {
                    tail_tokens,
                    carried_turns,
                    carried_tokens,
                    tool_outputs: Some(log::MeditationToolOutputs::Evict),
                    outputs: None,
                    newest_turn_tokens,
                    ..
                } => Some((
                    tail_tokens,
                    carried_turns,
                    carried_tokens,
                    newest_turn_tokens,
                )),
                _ => None,
            })
            .expect("a meditation line");
        let estimated: u64 = kept
            .iter()
            .map(crate::meditation::render::estimated_tokens)
            .sum();
        assert_eq!(meditation, (Some(10_000), 1, Some(estimated), None));

        session.ask("build it", None).expect("accepted");
        let log = wait_until(&session, "the second meditation", |log| {
            meditations_in(log).len() == 2
        });
        reads_whole(&session);
        let mut expected = trunk.clone();
        expected.push(user("build it"));
        assert_eq!(session.shared.transport.sent()[2].messages, expected);
        every_head_rebuilds(&log);

        // A budget no whole turn fits: the total refill, and the newest
        // turn's size on the meditation line (#693, levers run A's meditation 2804).
        let session = tailed(1);
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the meditation", |log| {
            !meditations_in(log).is_empty()
        });
        assert_eq!(session.trunk().len(), template().messages.len() + 1);
        let warned = log.iter().find_map(|logged| match line_of(logged).event {
            log::Event::Meditation {
                tail_tokens,
                carried_turns,
                newest_turn_tokens,
                ..
            } => Some((tail_tokens, carried_turns, newest_turn_tokens)),
            _ => None,
        });
        assert_eq!(warned, Some((Some(1), 0, Some(estimated))));
        reads_whole(&session);
    }

    /// A budget: the trunk call reports 18 prefilled, 160 reused and 66
    /// generated, 244 tokens, against a limit of 200, so the meditation fires
    /// `budget` though the cadence (every 5 turns) has not come round. The
    /// refilled trunk is unmeasured until its next call.
    #[test]
    fn a_budget_fires_a_meditation_once_the_trunk_call_reports_the_limit_reached() {
        let session = Session::open_with(
            Canned::new([
                a_reply_with_timings(SCOPED, 18, 160, 66),
                deltas(&[DECIDED]),
            ]),
            template(),
            None,
            None,
            None,
            Some(meditating(crate::meditation::policy::Served {
                reconcile_gate: crate::meditation::policy::ReconcileGate::Carry,
                every_turns: Some(5),
                at_trunk_tokens: Some(200),
                tail_tokens: 0,
                outputs: log::MeditationToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the budget's meditation", |log| {
            !meditations_in(log).is_empty()
        });
        assert_eq!(
            meditations_in(&log),
            vec![(1, log::MeditationReason::Budget)]
        );
        assert_eq!(session.shared.lock().trunk_tokens, None);
        reads_whole(&session);
    }

    /// Under the limit, no meditation: 18 + 160 + 66 is 244, and the limit is 245.
    #[test]
    fn a_trunk_under_the_budget_fires_no_meditation() {
        let session = Session::open_with(
            Canned::new([
                a_reply_with_timings(SCOPED, 18, 160, 66),
                deltas(&[DECIDED]),
            ]),
            template(),
            None,
            None,
            None,
            Some(meditating(crate::meditation::policy::Served {
                reconcile_gate: crate::meditation::policy::ReconcileGate::Carry,
                every_turns: None,
                at_trunk_tokens: Some(245),
                tail_tokens: 0,
                outputs: log::MeditationToolOutputs::Evict,
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
        assert!(
            meditations_in(&log).is_empty(),
            "{:#?}",
            meditations_in(&log)
        );
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
            Some(meditating(crate::meditation::policy::Served {
                reconcile_gate: crate::meditation::policy::ReconcileGate::Carry,
                every_turns: None,
                at_trunk_tokens: Some(100),
                tail_tokens: 0,
                outputs: log::MeditationToolOutputs::Evict,
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
        // The settling and any meditation it makes due are pushed under one lock.
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
        assert!(
            meditations_in(&log).is_empty(),
            "{:#?}",
            meditations_in(&log)
        );
        assert_eq!(session.shared.lock().trunk_tokens, Some(10));
    }

    /// A cadence that comes round with working memory empty fires nothing
    /// (a refill would drop every turn and carry nothing) and stays due: the
    /// first turn that leaves an entry fires it, at that turn.
    #[test]
    fn a_due_meditation_with_nothing_to_refill_from_waits_for_an_entry() {
        let session = Session::open_with(
            Canned::new([deltas(&["hello"]), deltas(&[SCOPED]), deltas(&[DECIDED])]),
            template(),
            None,
            None,
            None,
            Some(meditating(crate::meditation::policy::Served {
                reconcile_gate: crate::meditation::policy::ReconcileGate::Carry,
                every_turns: Some(1),
                at_trunk_tokens: None,
                tail_tokens: 0,
                outputs: log::MeditationToolOutputs::Evict,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
            })),
        );
        session.ask("hi", None).expect("accepted");
        let log = wait_until(&session, "the first turn", settled);
        assert!(meditations_in(&log).is_empty());
        session
            .ask_marked("what are we building?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the meditation", |log| {
            !meditations_in(log).is_empty()
        });
        assert_eq!(
            meditations_in(&log),
            vec![(2, log::MeditationReason::Cadence)]
        );
    }

    /// An operator's meditation resets the cadence: with a cadence of two, a
    /// declared meditation after turn 1 means turn 2 does not fire one.
    #[test]
    fn an_operator_meditation_restarts_the_cadence() {
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
            Some(meditating(crate::meditation::policy::Served {
                reconcile_gate: crate::meditation::policy::ReconcileGate::Carry,
                every_turns: Some(2),
                at_trunk_tokens: None,
                tail_tokens: 0,
                outputs: log::MeditationToolOutputs::Evict,
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
        session.declare_meditation(None).expect("admitted");
        session.ask("build it", None).expect("accepted");
        let log = wait_until(&session, "the second turn", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == 2
        });
        assert_eq!(
            meditations_in(&log),
            vec![(1, log::MeditationReason::Operator)]
        );
    }

    // -----------------------------------------------------------------------
    // the operator's meditation (#493)
    // -----------------------------------------------------------------------

    /// #493's test: one ask, a declared meditation, one ask after it. The meditation
    /// refills the trunk from working memory -- the head, the render after
    /// it, and no turn of the old trunk -- the next ask is sent on exactly
    /// that, and the log says so: the meditation line carries the render, what was
    /// carried, and the head's digest either side, which the projection's
    /// rebuild of the next request's head checks.
    /// #565: a meditation under the regimen's render budget refills from the
    /// budgeted render, and its `meditation` line names the budget, the tokens the
    /// render ran to, and how many entries it reduced.
    #[test]
    fn a_meditation_under_a_render_budget_records_the_budget_and_what_it_reduced() {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.meditations.render_budget = Some(crate::meditation::render::Budget {
            tokens: 1,
            over: crate::meditation::render::OverBudget::Elide,
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
            .declare_meditation(Some(gap(opened_by, GapEnd::Meditation)))
            .expect("admitted");
        let lines = whole_log(&session);
        let meditation = lines
            .iter()
            .find_map(|line| match &line.event {
                log::Event::Meditation {
                    render,
                    render_budget,
                    ..
                } => Some((render.clone(), render_budget.clone())),
                _ => None,
            })
            .expect("a meditation line");
        let (render, Some(budget)) = meditation.clone() else {
            panic!("the meditation names its budget: {meditation:?}");
        };
        assert_eq!((budget.tokens, budget.over.as_str()), (1, "elide"));
        assert_eq!(budget.reduced, 3, "the three decisions, elided: {render}");
        assert!(render.ends_with("3 older entries elided\n"), "{render}");
        assert_eq!(budget.rendered, (render.chars().count() as u64).div_ceil(4));
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_declared_meditation_refills_the_trunk_from_working_memory_and_the_next_ask_runs_on_it() {
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
            .declare_meditation(Some(gap(opened_by, GapEnd::Meditation)))
            .expect("a meditation after a settled turn is admitted");

        let held = session.shared.lock();
        let object = &held.interview.as_ref().expect("interviewing").object;
        let render = crate::meditation::render::render(object, None);
        drop(held);
        let refilled = crate::meditation::render::refill(&template().messages, &render);
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
        let meditation = log
            .iter()
            .position(|logged| matches!(logged.event, Event::Meditated { .. }))
            .expect("a meditation line");
        assert!(
            matches!(&log[meditation - 1].event, Event::IdleGap(gap) if gap.ended_by == GapEnd::Meditation),
            "the gap the meditation ended is logged just before it"
        );
        let log::Event::Meditation {
            prefix_hash_before,
            prefix_hash_after,
            ..
        } = line_of(&log[meditation]).event
        else {
            panic!("the meditation's line is a meditation");
        };
        assert_eq!(
            prefix_hash_before,
            crate::client::head::Head::of(&on_before).digest()
        );
        assert_eq!(
            line_of(&log[meditation]).event,
            log::Event::Meditation {
                phase_document: None,
                newest_turn_tokens: None,
                refilled_anyway: None,
                operator_changes: None,
                unaddressed: None,
                at_turn: 1,
                reason: log::MeditationReason::Operator,
                prefix_hash_before: prefix_hash_before.clone(),
                prefix_hash_after: prefix_hash_after.clone(),
                frame: crate::meditation::render::FRAME_VERSION.to_owned(),
                render: render.clone(),
                carried_entries: 3,
                carried_turns: 0,
                tail_tokens: None,
                carried_tokens: None,
                phase: None,
                tool_outputs: Some(log::MeditationToolOutputs::Evict),
                outputs: None,
                carried_outputs: None,
                carried_output_bytes: None,
                background: None,
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
            "a turn, its fork, and the turn after the meditation"
        );
        let mut expected = refilled.clone();
        expected.push(user("build it"));
        assert_eq!(
            sent[2].messages, expected,
            "the next ask runs on the refill"
        );
        // The meditation's "after" is the head the next trunk request carried, as
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
        // The meditation's record row needs its turn's row, which needs timings
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
            "every head after the meditation verifies"
        );
    }

    /// A meditation needs something to refill from and a session at rest: before
    /// any turn, or over working memory with no entry, it is refused
    /// `nothing-to-meditate`; mid-turn, `in-flight`. Each refusal is logged and
    /// changes nothing.
    #[test]
    fn a_meditation_with_nothing_to_refill_from_or_under_a_turn_is_refused() {
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
            session.declare_meditation(None),
            Err(Rejected::Refused(Refusal::NothingToMeditate))
        );
        session.ask("one", None).expect("accepted");
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        assert_eq!(
            session.declare_meditation(None),
            Err(Rejected::Refused(Refusal::InFlight))
        );
        gate.open();
        wait_until(&session, "the turn to settle", settled);
        // A settled turn, and a working object with nothing in it: a refill
        // would drop the turn and carry nothing.
        assert_eq!(
            session.declare_meditation(None),
            Err(Rejected::Refused(Refusal::NothingToMeditate))
        );
        assert_eq!(session.trunk()[1..], [user("one"), assistant("Hel")]);
        assert!(
            !session
                .events_from(0)
                .iter()
                .any(|logged| matches!(logged.event, Event::Meditated { .. }))
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

    // -----------------------------------------------------------------
    // adversarial review (memory area)
    // -----------------------------------------------------------------

    /// #655: a model's write over an operator-edited entry is refused. A
    /// fork that restates the operator's words dedups onto their entry and
    /// appends its own provenance last, so `by_operator` reads false and the
    /// audit's UPDATE of it is then applied.
    #[test]
    fn review_a_restatement_of_the_operators_entry_frees_it_for_the_model() {
        const OPERATORS: &str = "decision: for the one team that files bugs";
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.meditations.audit = true;
        let session = Session::open_with(
            Canned::new(vec![
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&["Agreed. Next, I will write the schema."]),
                deltas(&["DECISION: for the one team that files bugs\nPLAN: NONE\n"]),
                deltas(&["1. KEEP\n2. KEEP\n3. UPDATE: a model rewrite of the operator's note\n"]),
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
        wait_until(&session, "the first fork", |log| {
            settled(log) && fork_outcomes(log).len() == 1
        });
        let edited = live_entries(&session)[1].0.clone();
        session.edit_entry(&edited, OPERATORS).expect("an edit");
        let operator = live_entries(&session)
            .into_iter()
            .find(|(_, content)| content == OPERATORS)
            .expect("the operator's entry")
            .0;
        session
            .ask_marked("so, one team?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the second fork", |log| {
            settled(log) && fork_outcomes(log).len() == 2
        });
        eprintln!("fork outcomes: {:?}", fork_outcomes(&log));
        let still_theirs = session
            .shared
            .lock()
            .interview
            .as_ref()
            .expect("memory")
            .object
            .by_operator(&crate::object::EntryId::new(&operator).expect("an id"));
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the meditation", meditated);
        let refused = log
            .iter()
            .any(|logged| matches!(&logged.event, Event::PatchRefused { entry, .. } if *entry == operator));
        let held: Vec<String> = live_entries(&session)
            .into_iter()
            .map(|(_, content)| content)
            .collect();
        eprintln!(
            "by_operator after the restatement: {still_theirs}; audit refused: {refused}; live: {held:?}"
        );
        assert!(
            held.contains(&OPERATORS.to_owned()),
            "the audit's UPDATE voided the operator's entry {operator}: live {held:?}"
        );
        assert!(still_theirs, "the operator's entry stopped being theirs");
    }

    /// #608: a meditation while a tangent is open is refused `tangent-open`,
    /// since the close restores the fork point the refill would replace.
    /// The operator's refill anyway (#698) is not.
    #[test]
    fn review_a_refill_anyway_under_an_open_tangent_is_undone_by_its_close() {
        let session = three_decisions_then(
            |meditations| {
                meditations.audit = true;
                meditations.reconcile_gate = crate::meditation::policy::ReconcileGate::Refuse;
            },
            vec![deltas(&["1. KEEP\n2. KEEP\n"]), deltas(&["nothing to say"])],
        );
        let flagged = live_entries(&session)[0].0.clone();
        session.flag_entry(&flagged, "still so?").expect("a flag");
        session.declare_meditation(None).expect("admitted");
        wait_until(&session, "the hold", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::MeditationHeld { .. }))
        });
        let before = session.trunk();
        session
            .open_tangent("aside")
            .expect("a tangent over a held meditation");
        assert_eq!(
            session.declare_meditation(None),
            Err(Rejected::Refused(Refusal::TangentOpen))
        );
        let anyway = session.refill_anyway();
        eprintln!("refill_anyway under an open tangent: {anyway:?}");
        if anyway.is_ok() {
            let log = wait_until(&session, "the refill", meditated);
            let refilled = session.trunk();
            session
                .close_tangent(&BTreeMap::new())
                .expect("an empty tangent closes");
            let after = session.trunk();
            eprintln!(
                "trunk before {} msgs, refilled {} msgs, after close {} msgs; meditated logged: {}; close rolled_back: {:?}",
                before.len(),
                refilled.len(),
                after.len(),
                log.iter()
                    .any(|l| matches!(l.event, Event::Meditated { .. })),
                session.events_from(0).iter().find_map(|l| match &l.event {
                    Event::TangentClosed { rolled_back, .. } => Some(*rolled_back),
                    _ => None,
                })
            );
            assert_eq!(after, refilled, "the close undid the logged refill");
        }
        assert_eq!(anyway, Err(Rejected::Refused(Refusal::TangentOpen)));
    }

    /// #698: under `refuse` the meditation re-asks once, then holds. After
    /// a turn that did not settle `final`, no audit fires at all, so it
    /// holds having asked nothing -- and holds again on every re-declare,
    /// since no ask can be admitted to make a final turn.
    #[test]
    fn review_under_refuse_a_meditation_after_a_failed_turn_holds_having_asked_nothing() {
        let session = three_decisions_then(
            |meditations| {
                meditations.audit = true;
                meditations.reconcile_gate = crate::meditation::policy::ReconcileGate::Refuse;
            },
            vec![vec![Step::Fail(TransportFailure::Connect(
                "reset".to_owned(),
            ))]],
        );
        session.ask("go on", None).expect("accepted");
        wait_until(&session, "the failed turn", |log| {
            settled(log)
                && log
                    .iter()
                    .any(|logged| matches!(logged.event, Event::TurnSettled { turn: 2, .. }))
        });
        let flagged = live_entries(&session)[0].0.clone();
        session.flag_entry(&flagged, "still so?").expect("a flag");
        let holds = |n: usize| {
            move |log: &[Logged]| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::MeditationHeld { .. }))
                    .count()
                    == n
                    && session_settled_awaiting(log)
            }
        };
        session.declare_meditation(None).expect("admitted");
        wait_until(&session, "the hold", holds(1));
        session.declare_meditation(None).expect("admitted again");
        let log = wait_until(&session, "the second hold", holds(2));
        assert_eq!(
            session.ask("go on", None),
            Err(Rejected::Refused(Refusal::MeditationHeld))
        );
        let audits = audited_templates(&log);
        eprintln!("audits asked across two holds: {audits:?}");
        assert!(
            !audits.is_empty(),
            "held twice without asking the model anything"
        );
    }

    fn session_settled_awaiting(log: &[Logged]) -> bool {
        log.iter().rev().find_map(|logged| match &logged.event {
            Event::Settled { to, .. } => Some(*to == Settlement::Awaiting),
            _ => None,
        }) == Some(true)
    }

    /// #655/#671: a flag on an entry by the id the surface was shown for it
    /// -- a dedup's patch line names its alias -- is accepted, but its
    /// marker is keyed by the alias, never matches the audit's canonical
    /// item, and so is never marked or addressed.
    #[test]
    fn review_a_flag_on_an_alias_is_marked_and_addressed() {
        let mut interview = interviewing(&[log::Warrant::Scoping]);
        interview.meditations.audit = true;
        let session = Session::open_with(
            Canned::new(vec![
                deltas(&[SCOPED]),
                deltas(&[DECIDED]),
                deltas(&["Agreed. Next, I will write the schema."]),
                deltas(&["DECISION: a tracker\nPLAN: NONE\n"]),
                deltas(&["1. UPDATE: still so\n2. KEEP\n3. KEEP\n"]),
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
        wait_until(&session, "the first fork", |log| {
            settled(log) && fork_outcomes(log).len() == 1
        });
        session
            .ask_marked("a tracker?", None, true)
            .expect("accepted");
        let log = wait_until(&session, "the second fork", |log| {
            settled(log) && fork_outcomes(log).len() == 2
        });
        let shown: Vec<String> = patches(&log).into_iter().map(|entry| entry.id).collect();
        eprintln!("patch lines name {shown:?}");
        assert!(shown.contains(&"interview-t2-0".to_owned()));
        session
            .flag_entry("interview-t2-0", "still so?")
            .expect("a flag by the id the patch line named");
        session.declare_meditation(None).expect("admitted");
        let log = wait_until(&session, "the meditation", meditated);
        let question = log
            .iter()
            .find_map(|logged| match &logged.event {
                Event::Audited { question, .. } => Some(question.clone()),
                _ => None,
            })
            .expect("an audit");
        eprintln!("carried {:?}", carried(&log));
        assert!(question.contains("[flagged: still so?]"), "{question}");
        assert_eq!(
            carried(&log).1,
            Some(Vec::new()),
            "the audit's UPDATE addressed it"
        );
    }

    /// #698 x #651: a held phase meditation keeps the move the graph ruled
    /// on when it was declared; a `continue` ruling while it holds moves the
    /// phase, and refill anyway then makes the held move from the new phase
    /// with no ruling -- here build to review, an edge the graph lacks.
    #[test]
    fn review_refill_anyway_after_a_continue_moves_along_no_edge() {
        const SAID: &str = "The schema has two tables.";
        let mut graph = crate::meditation::phase::PhaseGraph::of(&[
            "plan".to_owned(),
            "build".to_owned(),
            "review".to_owned(),
        ])
        .expect("three phases");
        graph.allow("plan", "build").expect("an edge");
        graph.allow("plan", "review").expect("an edge");
        let mut interview = Interview {
            phases: graph,
            ..interviewing(&[])
        };
        interview.self_capture = Some(crate::capture::tools::Cadence::DEFAULT);
        interview.meditations.audit = true;
        interview.meditations.reconcile_gate = crate::meditation::policy::ReconcileGate::Refuse;
        let mut shape = template();
        declare_self_capture(&mut shape, Some(&interview));
        let acts = vec![
            vec![
                Step::Delta(SAID.to_owned()),
                Step::call(
                    0,
                    "call-f",
                    "update_record",
                    &serde_json::json!({ "field": "fact", "content": SAID }).to_string(),
                ),
                Step::call(
                    1,
                    "call-p",
                    "propose_phase_transition",
                    &serde_json::json!({ "to": "build", "reason": "the plan is settled" })
                        .to_string(),
                ),
            ],
            deltas(&["done"]),
            deltas(&["nothing to say"]),
            deltas(&["nothing to say"]),
        ];
        let session =
            Session::open_with(Canned::new(acts), shape, None, None, None, Some(interview));
        session.ask("plan it", None).expect("accepted");
        wait_until(&session, "the turn", settled);
        let fact = live_entries(&session)[0].0.clone();
        session.flag_entry(&fact, "two?").expect("a flag");
        session
            .declare_meditation_to(None, Some("review"))
            .expect("plan to review is an edge");
        wait_until(&session, "the hold", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::MeditationHeld { .. }))
                && session_settled_awaiting(log)
        });
        session
            .ratify_phase("call-p", PhaseChoice::Keep)
            .expect("continue, while the meditation holds");
        assert_eq!(session.shared.lock().phase.as_deref(), Some("build"));
        assert_eq!(
            session.declare_meditation_to(None, Some("review")),
            Err(Rejected::Refused(Refusal::NoPhaseEdge)),
            "the graph has no build to review"
        );
        session.refill_anyway().expect("refilled anyway");
        let log = wait_until(&session, "the refill", meditated);
        let moved = log.iter().find_map(|logged| match &logged.event {
            Event::Meditated { phase, .. } => phase.clone(),
            _ => None,
        });
        eprintln!(
            "refill anyway moved {moved:?}; phase now {:?}",
            session.shared.lock().phase
        );
        assert_ne!(
            session.shared.lock().phase.as_deref(),
            Some("review"),
            "moved build -> review, an edge the graph refuses: {moved:?}"
        );
    }

    /// #655: an operator edit is applied or refused (409, logged). An edit
    /// whose content is the entry's own, or another entry's, is refused by
    /// the object, and `edit_entry` reaches `unreachable!`.
    #[test]
    fn review_an_edit_that_restates_an_entry_is_refused_not_a_panic() {
        let session = three_decisions_then(|_| {}, Vec::new());
        let entries = live_entries(&session);
        let (id, own) = entries[0].clone();
        let other = entries[1].1.clone();
        for content in [own, other] {
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                session.edit_entry(&id, &content)
            }));
            eprintln!(
                "edit {id} to {content:?}: {:?}",
                outcome.as_ref().map_err(|_| "panicked")
            );
            assert!(outcome.is_ok(), "edit_entry panicked on {content:?}");
        }
    }

    /// #655 "neither command ends an idle gap"; #698 the same of
    /// refill-anyway. A flag admitted in the gap after a turn: the ask that
    /// then carries that gap should be admitted with it.
    #[test]
    fn review_a_flag_ends_no_idle_gap() {
        let control = three_decisions_then(|_| {}, vec![deltas(&["two"])]);
        let first = settling_seq(&control.events_from(0), 1);
        assert!(
            control.ask("go on", Some(gap(first, GapEnd::Ask))).is_ok(),
            "control: with no flag the gap is carried"
        );
        let session = three_decisions_then(|_| {}, vec![deltas(&["two"])]);
        let log = session.events_from(0);
        let first = settling_seq(&log, 1);
        let flagged = live_entries(&session)[0].0.clone();
        session.flag_entry(&flagged, "still so?").expect("a flag");
        let asked = session.ask("go on", Some(gap(first, GapEnd::Ask)));
        eprintln!("ask carrying the gap after a flag: {asked:?}");
        assert!(asked.is_ok(), "{asked:?}");
    }

    /// #608 x #651: a `meditation` ruling on a proposal while a tangent is
    /// open refills the trunk the close then restores.
    #[test]
    fn review_a_meditation_ruling_under_an_open_tangent_is_refused() {
        let (session, _) = proposing("build", Vec::new());
        session.open_tangent("aside").expect("a tangent");
        let ruled = session.ratify_phase("call-p", PhaseChoice::Meditation);
        let log = session.events_from(0);
        eprintln!(
            "ratify meditation under a tangent: {ruled:?}; meditated: {}",
            log.iter()
                .any(|l| matches!(l.event, Event::Meditated { .. }))
        );
        assert_eq!(ruled, Err(Rejected::Refused(Refusal::TangentOpen)));
    }

    // ---- review probes (exec area) ----

    /// PROBE, as Dispatch ruled #734 item 2: the standard tools keep no
    /// timeout (Pi and `OpenCode` 2 agree), and the operator's cancel ends
    /// one -- a `read` of a FIFO nobody writes -- its helper's whole group
    /// killed, and the turn settles `cancelled`.
    #[test]
    fn probe_a_cancel_ends_a_standard_read_of_a_fifo() {
        let tree = scratch("probe-std-cancel");
        let fifo = tree.join("pipe");
        let made = std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo");
        assert!(made.success());
        let mut shape = looping();
        shape.tools = tool_loop::ToolSurface::Standard.tools_timed(false, Some(500));
        let mut tools = tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline);
        tools.surface = tool_loop::ToolSurface::Standard;
        tools.read_tool = tool_loop::ToolSurface::Standard.read_tool();
        tools.timeout_ms = Some(500);
        tools.approvals_off = true;
        let session = Session::open_looping(
            Canned::new([
                vec![Step::call(0, "call-1", "read", r#"{"path":"pipe"}"#)],
                deltas(&["done"]),
            ]),
            shape,
            None,
            tools,
        );
        session.ask("read the pipe", None).expect("accepted");
        // Past the session's 500 ms timeout: a standard tool has none.
        std::thread::sleep(Duration::from_millis(1500));
        assert!(
            !settled(&session.shared.lock().log),
            "the read is still waiting"
        );
        let cancelled = Instant::now();
        session.cancel(1, None).expect("cancelled");
        let log = wait_until(&session, "the cancelled turn", settled);
        assert!(cancelled.elapsed() < Duration::from_secs(5));
        assert!(log.iter().any(|logged| matches!(
            logged.event,
            Event::TurnSettled {
                turn: 1,
                reason: SettleReason::Cancelled
            }
        )));
        tidy(&[&tree]);
    }

    /// PROBE: #578's claim, "a later call reads a slice of it through bash",
    /// under the default sandbox policy (Seatbelt, `sandbox_reads` listed)
    /// with the recording outside the worktree.
    #[test]
    fn probe_capped_output_is_readable_under_the_default_sandbox() {
        use crate::drive::output::OutputCap;
        if crate::isolation::Platform::here() != crate::isolation::Platform::MacOs {
            return;
        }
        let tree = scratch("probe-cap-sb");
        let recording = scratch("probe-cap-sb-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let whole: String = (1..=5000)
            .flat_map(|n| [n.to_string(), "\n".to_owned()])
            .collect();
        let sha256 = crate::digest::sha256_hex(whole.as_bytes());
        let kept = recording.join("files").join(&sha256);
        let slice = format!("sed -n '2500,2502p' \"{}\"", kept.display());
        let mut replies: Vec<Vec<Step>> = ["seq 1 5000", slice.as_str()]
            .iter()
            .enumerate()
            .map(|(n, command)| vec![bash(&format!("call-{n}"), command)])
            .collect();
        replies.push(deltas(&["done"]));
        let mut tools = tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline);
        let policy = crate::isolation::Policy::merged_usr();
        tools.confinement = crate::isolation::open_on(&policy, crate::isolation::Platform::MacOs)
            .expect("Seatbelt");
        tools.policy = policy;
        tools.output_cap = OutputCap::DEFAULT;
        tools.recording = Some(recording.clone());
        tools.approvals_off = true;
        let session = Session::open_looping(Canned::new(replies), looping(), None, tools);
        session.ask("count", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let written = lines(&log);
        eprintln!(
            "PROBE call-0 shown head: {:?}",
            written[0].shown.as_deref().map(|s| &s[..s.len().min(300)])
        );
        eprintln!("PROBE call-1 shown: {:?}", written[1].shown);
        assert!(kept.exists(), "the drive kept the whole");
        assert_eq!(written[1].shown.as_deref(), Some("2500\n2501\n2502\n"));
        tidy(&[&tree, &recording]);
    }

    /// PROBE: #632's start text tells the model to read the output and
    /// status files; under the default sandbox, can it?
    #[test]
    fn probe_background_files_are_readable_under_the_default_sandbox() {
        if crate::isolation::Platform::here() != crate::isolation::Platform::MacOs {
            return;
        }
        let tree = scratch("probe-bg-sb");
        let recording = scratch("probe-bg-sb-recording");
        let recording = std::fs::canonicalize(&recording).expect("the recording");
        let look = format!(
            "ls \"{0}/background\"; for f in \"{0}\"/background/shell-*; do cat \"$f\"; done",
            recording.display()
        );
        let mut tools = tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline);
        let policy = crate::isolation::Policy::merged_usr();
        tools.confinement = crate::isolation::open_on(&policy, crate::isolation::Platform::MacOs)
            .expect("Seatbelt");
        tools.policy = policy;
        tools.background = true;
        tools.approvals_off = true;
        tools.recording = Some(recording.clone());
        let mut shape = looping();
        shape.tools = tool_loop::ToolSurface::Bash.tools_with(true);
        let session = Session::open_looping(
            Canned::new(vec![
                vec![in_the_background("call-1", "echo ready")],
                deltas(&["started"]),
                vec![bash("call-2", &look)],
                deltas(&["looked"]),
            ]),
            shape,
            None,
            tools,
        );
        session.ask("start it", None).expect("accepted");
        let _ = wait_until(&session, "the job's end", |log| ended_jobs(log).len() == 1);
        session.ask("look", None).expect("accepted");
        let log = wait_until(&session, "the second turn", |log| turns_settled(log) == 2);
        let written = lines(&log);
        eprintln!("PROBE start text: {:?}", written[0].shown);
        eprintln!("PROBE look: {:?}", written[1].shown);
        session.end_commands();
        let shown = written[1].shown.clone().unwrap_or_default();
        tidy(&[&tree, &recording]);
        assert!(shown.contains("ready"), "{shown}");
    }

    /// PROBE: a recording inside the worktree (where a listed-reads sandbox
    /// can read capped outputs): can a sandboxed command steer the drive's
    /// own unconfined write of a capped output outside the policy?
    #[test]
    fn probe_kept_output_cannot_be_steered_outside_the_sandbox() {
        use crate::drive::output::OutputCap;
        if crate::isolation::Platform::here() != crate::isolation::Platform::MacOs {
            return;
        }
        let tree = scratch("probe-steer-tree");
        let tree = std::fs::canonicalize(&tree).expect("the tree");
        let outside = scratch("probe-steer-outside");
        let outside = std::fs::canonicalize(&outside).expect("outside");
        let recording = tree.join("rec");
        std::fs::create_dir_all(&recording).expect("rec");
        let plant = format!("ln -s \"{}\" rec/files && echo planted", outside.display());
        let payload = "seq 1 5000";
        let mut replies: Vec<Vec<Step>> = [
            plant.as_str(),
            payload,
            "echo 'direct' > \"$(dirname rec)/../probe-steer-direct\" ; ls rec/files/",
        ]
        .iter()
        .enumerate()
        .map(|(n, command)| vec![bash(&format!("call-{n}"), command)])
        .collect();
        replies.push(deltas(&["done"]));
        let mut tools = tools(Confinement::Unconfined, &tree, &[], None, Decider::Decline);
        let policy = crate::isolation::Policy::merged_usr();
        tools.confinement = crate::isolation::open_on(&policy, crate::isolation::Platform::MacOs)
            .expect("Seatbelt");
        tools.policy = policy;
        tools.output_cap = OutputCap::DEFAULT;
        tools.recording = Some(recording.clone());
        tools.approvals_off = true;
        let session = Session::open_looping(Canned::new(replies), looping(), None, tools);
        session.ask("go", None).expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        let written = lines(&log);
        for line in &written {
            eprintln!(
                "PROBE {:?}: {:?}",
                line.argv,
                line.shown.as_deref().map(|s| &s[..s.len().min(200)])
            );
        }
        let landed: Vec<_> = std::fs::read_dir(&outside)
            .expect("outside")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        eprintln!("PROBE files in the directory outside the sandbox: {landed:?}");
        tidy(&[&tree, &outside]);
        assert!(
            landed.is_empty(),
            "the drive wrote outside the sandbox: {landed:?}"
        );
    }

    /// Two forks in one turn's gap (#564 cadences) that record different
    /// facts: the fold names both `interview-t<turn>-0`, so the second is
    /// refused by the object as an id reused and settles `unparseable`.
    #[test]
    fn review_two_forks_in_one_turn_that_record_different_facts_both_fold() {
        let tree = scratch("review-two-forks");
        std::fs::write(tree.join("notes.md"), "a note\n").expect("a note");
        std::fs::write(tree.join("lib.rs"), "pub fn f() {}\n").expect("a source");
        let replies = vec![
            vec![bash("call-1", "cat notes.md")],
            vec![bash("call-2", "cat lib.rs")],
            deltas(&["Done. Next, I will write the schema."]),
            deltas(&["LEARNED: the notes hold one note\n"]),
            deltas(&["LEARNED: the library exports f\n"]),
        ];
        let session = Session::open_with(
            Canned::new(replies),
            looping(),
            None,
            Some(tools(
                Confinement::Unconfined,
                &tree,
                &["cat", "echo"],
                None,
                Decider::Decline,
            )),
            None,
            Some(Interview {
                cadence: Cadence::PerClass,
                threshold_bytes: None,
                ..interviewing(&[log::Warrant::Scoping, log::Warrant::Read])
            }),
        );
        let log = through_the_gap(&session, 2);
        let written: Vec<(String, String)> = patches(&log)
            .into_iter()
            .map(|entry| (entry.id, entry.text))
            .collect();
        eprintln!(
            "fork outcomes {:?}; patches {written:?}",
            fork_outcomes(&log)
        );
        tidy(&[&tree]);
        assert_eq!(
            fork_outcomes(&log),
            [log::ForkOutcome::Value, log::ForkOutcome::Value],
            "patches {written:?}"
        );
    }

    /// #648: `→` or `->` stands between the cited id and its replacement.
    /// An arrow inside the replacement's own text is taken as that
    /// separator, so the cited "id" becomes the id plus prose and the pair
    /// cites nothing: the supersede is lost and the field stays an add.
    #[test]
    fn review_a_supersede_whose_new_entry_holds_an_arrow_still_supersedes() {
        for arrow in ["->", "→"] {
            let object = object_of(&["interview-t1-0"]);
            let text = format!("interview-t1-0 the pipeline is build {arrow} test {arrow} ship");
            let patches = cited(vec![supersede_field("interview-t2-0", &text)], &object).patches;
            assert!(
                matches!(
                    patches.as_slice(),
                    [Patch::Supersede { voids, .. }] if voids.as_str() == "interview-t1-0"
                ),
                "{arrow}: {patches:#?}"
            );
        }
    }

    /// #611: the fields an ask asks for are read off its answer
    /// instruction. The first `field ` in the question is taken, and the
    /// stated intent (and v4's record) come before the instruction.
    #[test]
    fn review_an_asks_fields_are_its_instructions_not_its_intents() {
        let ask = router::Ask {
            kind: AskKind::Judgment,
            intent: router::stated_intent("Done. Next, I will add the fields for the email."),
        }
        .render_in(&crate::dogma::asks::V3, &Facts::default(), None);
        assert_eq!(asked_fields(&ask), ["decision", "plan"], "{ask}");
    }

    /// #611 under the tools route (t1, #666): its asks name their fields in
    /// lower case, so no fork is ever read as covered.
    #[test]
    fn review_a_tools_route_asks_fields_are_read() {
        let ask = router::Ask {
            kind: AskKind::Judgment,
            intent: None,
        }
        .render_in(&crate::dogma::asks::T1, &Facts::default(), None);
        assert_eq!(asked_fields(&ask), ["decision", "plan"], "{ask}");
    }
}
