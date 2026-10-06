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
        /// A seam has nothing to refill from: no turn has settled, or the
        /// session keeps no working memory (#493).
        NothingToSeam => "nothing-to-seam",
        /// A cancel named a turn older than the latest one: it arrived after
        /// that turn settled and must not stop the next (the admission
        /// counter ruled on #117).
        Stale => "stale",
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
    }
}

/// One thing that happened in a session.
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
    },
    /// How the fork ended: once per fork, after its call's last event.
    ForkSettled {
        /// The sequence number of its [`Event::Forked`].
        fork: u64,
        /// How.
        outcome: log::ForkOutcome,
    },
    /// The operator declared a seam, and the trunk was refilled from working
    /// memory (#493): the head with the working object rendered after it, and
    /// no turn of the old trunk.
    Seamed {
        /// The latest turn, settled, which it follows.
        at_turn: u32,
        /// The digest `client::head` gives of a request on the trunk before.
        prefix_hash_before: String,
        /// The same, after.
        prefix_hash_after: String,
        /// The working object, rendered.
        render: String,
        /// How many entries the render carried.
        carried_entries: u64,
    },
    /// One entry the fork's answer patched into the session's working
    /// object, after the fork settled `value`.
    Patched {
        /// The sequence number of its [`Event::Forked`].
        fork: u64,
        /// What it did.
        op: log::PatchOp,
        /// The entry.
        entry: log::PatchEntry,
        /// The id of the entry it replaced, exactly when `op` is `supersede`.
        supersedes: Option<String>,
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
        }
    }
}

/// The regimen key naming the rules that warrant a fork in the capture gap
/// (#374, ruled 5985110649): a list of the log's [`log::Warrant`] words,
/// `read` and `scoping`. A drive-read key, like
/// [`tool_loop::APPROVAL_POLICY`]: the regimen format does not register it.
/// Absent or empty, no fork ever fires.
pub const INTERVIEW_WARRANT: &str = "interview_warrant";

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
            regimen::Value::String(word) => log::Warrant::from_tag(word).ok_or_else(|| {
                format!(
                    "`{INTERVIEW_WARRANT}` names `{word}`, which is not a rule: {}",
                    log::Warrant::ALL
                        .iter()
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

/// What the capture gap runs under (#374): the rules that warrant its fork,
/// and the working object the fork's patches are applied to.
#[derive(Debug, Clone)]
pub struct Interview {
    /// The rules enabled, as [`interview_warrant`] read them.
    pub rules: Vec<log::Warrant>,
    /// The session's working object.
    pub object: WorkingObject,
}

/// The rule that warrants a fork after `turn` settled `final`, and the
/// question it asks, or `None` (#374, the predicate ruled at 5985110649).
///
/// (b) `scoping`: the turn's ask carried the operator's mark. The question
/// is the router's turn-boundary ask, [`AskKind::Judgment`].
///
/// (a) `read`: one of the turn's `tool_call`s `ran` and the router's table
/// classes it [`Class::DocumentRead`] or [`Class::SourceRead`]. The question
/// is the ask the router routes that class to, quoting back the command.
///
/// Both quote back what the trunk last said it was about to do, as the
/// router does. Scoping is checked first: it is the operator's own mark.
fn warranted(
    rules: &[log::Warrant],
    log: &[Logged],
    turn: u32,
    answer: &str,
) -> Option<(log::Warrant, String)> {
    let intent = router::stated_intent(answer);
    let ask = |kind: AskKind, last_command: Option<String>| {
        router::Ask {
            kind,
            intent: intent.clone(),
        }
        .render(&Facts {
            cwd: None,
            last_edited: None,
            last_command,
        })
    };
    let scoping = log.iter().any(|logged| {
        matches!(&logged.event, Event::Asked { turn: asked, scoping: true, .. } if *asked == turn)
    });
    if scoping && rules.contains(&log::Warrant::Scoping) {
        return Some((log::Warrant::Scoping, ask(AskKind::Judgment, None)));
    }
    if !rules.contains(&log::Warrant::Read) {
        return None;
    }
    let mut table = Router::new().ok()?;
    let read = log.iter().rev().find_map(|logged| {
        let Event::ToolCalled(line) = &logged.event else {
            return None;
        };
        if line.turn != turn || line.outcome != log::ToolOutcome::Ran {
            return None;
        }
        let command = tool_loop::command_of(&line.arguments)?;
        let decided = table.observe(&RecordEvent::ToolCall {
            id: line.id.clone(),
            at_turn: turn,
            tool: line.name.clone(),
            args: Some(BTreeMap::from([(
                "command".to_owned(),
                Value::String(command.clone()),
            )])),
            exit: None,
            output: None,
            exec: None,
        });
        let class = decided.first()?.class;
        let Routing::Fork(kind) = class.routing() else {
            return None;
        };
        READS.contains(&class).then_some((kind, command))
    })?;
    Some((log::Warrant::Read, ask(read.0, Some(read.1))))
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
    fn after_the_turn(&mut self) {
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
        Self::opened_as(transport, template, None, None, None, None)
    }

    /// [`Session::open`], declaring what serves it -- the dialect it speaks
    /// and, when the operator declared it, its concurrency -- which the log's
    /// `session.start` carries (#292).
    #[must_use]
    pub fn open_serving(transport: S, template: RequestShape, serving: Serving) -> Self {
        Self::opened_as(transport, template, Some(serving), None, None, None)
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
        Self::opened_as(transport, template, serving, Some(tools), None, None)
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
        Self::opened_as(transport, template, serving, tools, claim, interview)
    }

    fn opened_as(
        transport: S,
        template: RequestShape,
        serving: Option<Serving>,
        tools: Option<Tools>,
        claim: Option<log::SubstrateClaim>,
        interview: Option<Interview>,
    ) -> Self {
        // A head is the trunk before any turn; a tool result answers a call
        // made in one, and the log's head has no word for it (`role_of`).
        assert!(
            template.messages.iter().all(|m| m.role != Role::Tool),
            "a session's head holds no tool result"
        );
        let trunk = template.messages.clone();
        let opened = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| {
                u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
            });
        let mut state = State {
            settlement: Settlement::Awaiting,
            trunk,
            log: Vec::new(),
            flight: None,
            turns: 0,
            opened_at: Instant::now(),
            gap_open: None,
            carried: None,
            pending_gap: None,
            sink: None,
            allowed: tools
                .as_ref()
                .map(|t| t.allowed.clone())
                .unwrap_or_default(),
            counts: Counts {
                preseeded: tools.as_ref().map_or(0, |t| {
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
        };
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
        });
        Self {
            shared: Arc::new(Shared {
                transport,
                template,
                state: Mutex::new(state),
                changed: Condvar::new(),
                tools,
            }),
        }
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
        // Pushed here, under the lock that admits the ask, and never on the
        // turn's thread: a thread that cannot start still settles with a
        // `request.failed` that cites a request that exists (#117, R2c
        // finding 21).
        let request = state.push(Event::Requested {
            turn,
            lane: Lane::Trunk,
            head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
            fork: None,
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
        let ask = message;
        let spawned = std::thread::Builder::new()
            .name("diet-turn".to_owned())
            .spawn(move || {
                // Caught rather than left to unwind: a panicking transport, or
                // an overflowing deadline, must still settle the turn -- and
                // say why -- or the session sits in `turn` for good, refusing
                // every ask (#120's first review).
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    call(&shared, &shape, &cancel, ask, turn, request);
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
    /// [`Refusal::NothingToSeam`] before any turn has settled or in a session
    /// that keeps no working memory: both logged. [`Refusal::Ended`] once the
    /// session has ended, not logged (#291). A refused command's `gap` is
    /// neither logged nor closed. [`Rejected::BadGap`] when `gap` cannot be
    /// logged, and then nothing is.
    pub fn declare_seam(&self, gap: Option<IdleGap>) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        state.carry(gap, CommandKind::DeclareSeam);
        let because = match state.settlement {
            Settlement::Ended => Some(Refusal::Ended),
            Settlement::Turn | Settlement::Capture => Some(Refusal::InFlight),
            Settlement::Awaiting if state.turns == 0 || state.interview.is_none() => {
                Some(Refusal::NothingToSeam)
            }
            Settlement::Awaiting => None,
        };
        if let Some(because) = because {
            let refused = state.refuse(CommandKind::DeclareSeam, because);
            drop(state);
            self.shared.changed.notify_all();
            return Err(Rejected::Refused(refused));
        }
        state.admit()?;
        let Some(interview) = state.interview.as_ref() else {
            unreachable!("refused above when the session keeps no working memory");
        };
        let render = crate::seam::render::render(&interview.object, None);
        let carried_entries = interview.object.live().count() as u64;
        let head = self.shared.template.messages.clone();
        let digest = |messages: &[Message]| {
            let mut shape = self.shared.template.clone();
            shape.messages = messages.to_vec();
            crate::client::head::Head::of(&shape).digest().to_owned()
        };
        let prefix_hash_before = digest(&state.trunk);
        let refilled = crate::seam::render::refill(&head, &render);
        let prefix_hash_after = digest(&refilled);
        state.trunk = refilled;
        let at_turn = state.turns;
        state.push(Event::Seamed {
            at_turn,
            prefix_hash_before,
            prefix_hash_after,
            render,
            carried_entries,
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
        if let Some(tools) = &self.shared.tools {
            tools.confinement.end_session();
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
        } => log::Event::SessionStart {
            version: log::VERSION,
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
        } => log::Event::Request {
            turn: *turn,
            lane: match lane {
                Lane::Trunk => log::Lane::Trunk,
                Lane::Interview => log::Lane::Interview,
            },
            head_sha256: Some(head_sha256.clone()),
            fork: *fork,
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
        }
        | Event::Called {
            request,
            text,
            finish_reason,
            reasoning,
            timings,
        } => log::Event::Response {
            to_request: *request,
            text: text.clone(),
            finish_reason: finish_reason.clone(),
            reasoning: reasoning.clone(),
            timings: timings.as_ref().map(timings_line),
            // v2's keys, written by the session once it carries them
            // (`usage` for a dialect with no timings); `capped` is written
            // only by a capped call, below.
            usage: None,
            capped: None,
        },
        Event::Capped {
            request,
            text,
            finish_reason,
            reasoning,
            timings,
        } => log::Event::Response {
            to_request: *request,
            text: text.clone(),
            finish_reason: finish_reason.clone(),
            reasoning: reasoning.clone(),
            timings: timings.as_ref().map(timings_line),
            usage: None,
            capped: Some(true),
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
        } => log::Event::RequestFailed {
            request: *request,
            reason: match class {
                Some(Rejection::ContextOverflow) => log::FailReason::ContextOverflow,
                None => log::FailReason::Server,
            },
            message: body.clone(),
            status: Some(*status),
            partial: arrived(partial),
        },
        Event::Failed {
            request,
            failure,
            partial,
        } => failed_line(*request, failure, partial),
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
                files: None,
                shown,
            }
        }
        Event::TurnSettled { turn, reason } => log::Event::TurnSettled {
            turn: *turn,
            reason: settle_reason_in_the_log(*reason),
        },
        Event::Forked {
            of_turn,
            at,
            why,
            question,
        } => log::Event::Fork {
            lane: log::Lane::Interview,
            of_turn: *of_turn,
            at: *at,
            why: *why,
            question: question.clone(),
        },
        Event::ForkSettled { fork, outcome } => log::Event::ForkSettled {
            fork: *fork,
            outcome: *outcome,
        },
        Event::Patched {
            fork,
            op,
            entry,
            supersedes,
        } => log::Event::Patch {
            fork: *fork,
            op: *op,
            entry: entry.clone(),
            supersedes: supersedes.clone(),
        },
        Event::Seamed {
            at_turn,
            prefix_hash_before,
            prefix_hash_after,
            render,
            carried_entries,
        } => log::Event::Seam {
            at_turn: *at_turn,
            // The only seam a served session fires is the operator's.
            reason: log::SeamReason::Operator,
            prefix_hash_before: prefix_hash_before.clone(),
            prefix_hash_after: prefix_hash_after.clone(),
            frame: crate::seam::render::FRAME_VERSION.to_owned(),
            render: render.clone(),
            carried_entries: *carried_entries,
            // A total compaction: no turn of the old trunk is carried.
            carried_turns: 0,
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

/// A failed call's line: a timeout is `timeout`, every other transport
/// failure `transport`, and the failure's own words are the message.
fn failed_line(request: u64, failure: &TransportFailure, partial: &str) -> log::Event {
    log::Event::RequestFailed {
        request,
        reason: match failure {
            TransportFailure::Timeout { .. } => log::FailReason::Timeout,
            _ => log::FailReason::Transport,
        },
        message: failure.to_string(),
        status: None,
        partial: arrived(partial),
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
    }
}

fn refusal_of(refusal: Refusal) -> log::Refusal {
    match refusal {
        Refusal::InFlight => log::Refusal::InFlight,
        Refusal::Ended => log::Refusal::Ended,
        Refusal::NothingInFlight => log::Refusal::NothingInFlight,
        Refusal::NothingToSeam => log::Refusal::NothingToSeam,
        Refusal::Stale => log::Refusal::Stale,
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
    ask: Message,
    turn: u32,
    request: u64,
) {
    let mut shape = shape.clone();
    let mut request = request;
    let mut steps = 1;
    // The turn's exchange so far: its ask, then each step's calls and their
    // results. It joins the trunk when the turn settles `final` or
    // `max_steps` (Q12), and never otherwise (D13).
    let mut exchange = vec![ask];
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
    let result = shared
        .transport
        .stream(shape, deadline, cancel, &mut |piece: Piece<'_>| {
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

    let mut state = shared.lock();
    state.flight = None;
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
                (finish_reason, timings),
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
            let mut said = Message::new(Role::Assistant, partial.clone());
            said.reasoning.clone_from(&reasoning);
            said.tool_calls = calls
                .iter()
                .map(|call| ToolCall {
                    id: call.id.clone(),
                    name: call.name.clone(),
                    arguments: call.arguments.clone(),
                })
                .collect();
            state.push(Event::Called {
                request,
                text: partial,
                finish_reason,
                reasoning,
                timings,
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
            state.push(Event::Rejected {
                request,
                status,
                body,
                class,
                partial,
            });
            state.push(Event::TurnSettled {
                turn,
                reason: SettleReason::Failed,
            });
            state.move_to(Settlement::Awaiting);
        }
        Err(failure) => {
            let reason = settle_reason_of(&failure);
            state.push(Event::Failed {
                request,
                failure,
                partial,
            });
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
        // The one string the result message carries, logged as given.
        line.shown.clone_from(&shown);
        unknown |= line.reason == Some(log::ToolRefusal::UnknownTool);
        stopped |= line.outcome == log::ToolOutcome::Cancelled;
        if let Some(shown) = shown {
            results.push(Message::tool_result(call.id.clone(), shown));
        }
        shared.lock().push(Event::ToolCalled(Box::new(line)));
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
        if reason == SettleReason::MaxSteps {
            let ran = std::mem::take(exchange);
            state.trunk.extend(ran);
        }
        state.push(Event::TurnSettled { turn, reason });
        state.after_the_turn();
        drop(state);
        shared.changed.notify_all();
        return None;
    }
    exchange.push(said.clone());
    exchange.extend(results.iter().cloned());
    shape.messages.push(said);
    shape.messages.extend(results);
    let next = state.push(Event::Requested {
        turn,
        lane: Lane::Trunk,
        head_sha256: crate::client::head::Head::of(shape).digest().to_owned(),
        fork: None,
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
/// entry that covered its first approved segment.
fn approval_of(judged: &Judged, allowed: &[Entry]) -> Option<log::Approval> {
    let entry = judged
        .covered_by
        .iter()
        .flatten()
        .next()
        .and_then(|at| allowed.get(*at))?;
    Some(log::Approval {
        scope: tool_loop::scope_tag(entry.scope),
        decided_at: entry.decided_at,
        why: entry.why.clone(),
    })
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
    let parsed = |mut line: ToolLine| {
        line.argv = Some(tool_loop::argv_of(&command));
        line.cwd = Some(tools.cwd.clone());
        line
    };
    if last {
        return (parsed(refused(log::ToolRefusal::MaxSteps)), None);
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
    let mut line = parsed(ToolLine::of(request, turn, call, log::ToolOutcome::Ran));
    line.approval = approval;
    let profiled = tools.confinement.isolation() != crate::isolation::Isolation::None;
    match tools
        .confinement
        .run(&tools.policy, &tools.worktree, &judged.run)
    {
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
            (line, Some(ran.as_the_model_sees_it()))
        }
        Err(not_run) => {
            // It never ran: what would have run, and why it did not, as the
            // command's own failure under its confinement.
            let said = not_run.to_string();
            let confined = tools
                .confinement
                .compose(&tools.policy, &tools.worktree, &judged.run);
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
    }
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
    (finish_reason, timings): (Option<String>, Option<Timings>),
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
        });
        state.push(Event::TurnSettled {
            turn,
            reason: SettleReason::Failed,
        });
        state.move_to(Settlement::Awaiting);
        return false;
    }
    state.trunk.extend(exchange);
    // The reasoning goes back with the answer, byte for byte and
    // untrimmed: measured on e7051ef (#117, Q10), dropping it
    // diverges the next prompt at this turn, and a stray newline
    // diverges it inside this turn. ONE binding feeds the trunk and
    // the response line, so the two cannot differ (R3.4).
    let reasoning = (!reasoning.is_empty()).then_some(reasoning);
    let mut answer = Message::new(Role::Assistant, partial.clone());
    answer.reasoning.clone_from(&reasoning);
    state.trunk.push(answer);
    state.push(Event::Answered {
        request,
        text: partial,
        finish_reason,
        reasoning,
        timings,
    });
    state.push(Event::TurnSettled {
        turn,
        reason: SettleReason::Final,
    });
    state.move_to(Settlement::Capture);
    true
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
    let fired = state
        .interview
        .as_ref()
        .filter(|_| !state.ending)
        .and_then(|interview| warranted(&interview.rules, &state.log, turn, answer));
    let Some((why, question)) = fired else {
        state.after_the_turn();
        return None;
    };
    let mut shape = shared.template.clone();
    shape.messages.clone_from(&state.trunk);
    shape
        .messages
        .push(Message::new(Role::User, question.clone()));
    let fork = state.push(Event::Forked {
        of_turn: turn,
        at,
        why,
        question,
    });
    let request = state.push(Event::Requested {
        turn,
        lane: Lane::Interview,
        head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
        fork: Some(fork),
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
/// run nor logged as a piece, and its answer is `unparseable`.
#[allow(clippy::too_many_lines)]
fn interview<S: Streaming>(shared: &Shared<S>, forked: Fired) {
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
    let result = shared
        .transport
        .stream(&shape, deadline, &cancel, &mut |piece: Piece<'_>| {
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
                Piece::ToolCall { .. } => {
                    called = true;
                    return;
                }
            };
            shared.lock().push(event);
            shared.changed.notify_all();
        });
    let mut state = shared.lock();
    state.flight = None;
    state.forking = None;
    let reasoning = Some(reasoning).filter(|thought| !thought.is_empty());
    let mut patches = Vec::new();
    let outcome = match result {
        Ok(StreamEnded::Finished {
            finish_reason,
            timings,
        }) => {
            let text = partial.clone();
            let cut = capped(finish_reason.as_deref());
            state.push(if cut {
                Event::Capped {
                    request,
                    text: partial,
                    finish_reason,
                    reasoning,
                    timings,
                }
            } else {
                Event::Answered {
                    request,
                    text: partial,
                    finish_reason,
                    reasoning,
                    timings,
                }
            });
            if cancel.is_asked() {
                log::ForkOutcome::Cancelled
            } else if cut {
                log::ForkOutcome::Truncated
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
            state.push(Event::Rejected {
                request,
                status,
                body,
                class,
                partial,
            });
            log::ForkOutcome::Failed
        }
        Err(failure) => {
            state.push(Event::Failed {
                request,
                failure,
                partial,
            });
            log::ForkOutcome::Failed
        }
    };
    state.push(Event::ForkSettled { fork, outcome });
    for patch in patches {
        state.push(patch);
    }
    state.after_the_turn();
    drop(state);
    shared.changed.notify_all();
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
    let Some(interview) = state.interview.as_mut() else {
        return (log::ForkOutcome::Unparseable, Vec::new());
    };
    if interview.object.apply_turn(&patches).is_err() {
        return (log::ForkOutcome::Unparseable, Vec::new());
    }
    let lines = patches
        .iter()
        .map(|patch| patched(fork, patch, &interview.object))
        .collect();
    (log::ForkOutcome::Value, lines)
}

/// One applied patch as its log line: its op, its entry -- the patch's own
/// content, or for a verdict on an entry that entry's -- and what it voided.
fn patched(fork: u64, patch: &Patch, object: &WorkingObject) -> Event {
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
        op,
        entry: log::PatchEntry {
            id: id.as_str().to_owned(),
            text,
            category: None,
        },
        supersedes,
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
        crashed_fork(&mut state, why);
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
fn crashed_fork(state: &mut State, why: String) {
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
        });
    }
    state.after_the_turn();
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
    fn a_cancel_reaches_a_call_blocked_mid_answer_and_leaves_the_trunk_alone() {
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
        assert_eq!(session.trunk(), [Message::new(Role::System, HEAD)]);

        // The next ask goes out on the prefix the last SETTLED turn left:
        // nothing of the stopped one.
        session
            .ask("second", None)
            .expect("accepted after a cancel");
        wait_until(&session, "the second turn", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::Answered { .. }))
                && settled(log)
        });
        assert_eq!(
            session.shared.transport.sent()[1].messages,
            [Message::new(Role::System, HEAD), user("second")]
        );
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
                partial: "par".to_owned()
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
                partial: "par".to_owned()
            }));
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::Answered { .. })),
            "a refusal was logged as an answer"
        );
        assert_eq!(session.trunk(), [Message::new(Role::System, HEAD)]);
    }

    /// The session's whole log so far, as the log format reads it.
    fn whole_log(session: &Session<Canned>) -> Vec<log::Line> {
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
            engine_build: "b1-0123abc".to_owned(),
            engine_identity: log::EngineIdentity::CheckedCommit,
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
                opened: 1_790_000_000_000,
                model: "a-model".to_owned(),
                head: vec![Message::new(Role::System, HEAD)],
                serving: Some(Serving {
                    concurrency: Concurrency::Declared(2),
                    dialect: crate::client::shape::Dialect::llama_cpp(),
                }),
                claim: Some(claimed()),
                tools: Vec::new(),
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
            },
            Event::Rejected {
                request: 3,
                status: 400,
                body: "too long".to_owned(),
                class: Some(Rejection::ContextOverflow),
                partial: String::new(),
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
            })),
            Event::Forked {
                of_turn: 1,
                at: 3,
                why: log::Warrant::Scoping,
                question: "what did you decide?".to_owned(),
            },
            Event::Requested {
                turn: 1,
                lane: Lane::Interview,
                head_sha256: "b".repeat(64),
                fork: Some(20),
            },
            Event::ForkSettled {
                fork: 20,
                outcome: log::ForkOutcome::Value,
            },
            Event::Patched {
                fork: 20,
                op: log::PatchOp::Add,
                entry: log::PatchEntry {
                    id: "interview-t1-0".to_owned(),
                    text: "decision: keep it".to_owned(),
                    category: None,
                },
                supersedes: None,
            },
            Event::Seamed {
                at_turn: 1,
                prefix_hash_before: "c".repeat(64),
                prefix_hash_after: "d".repeat(64),
                render: "# regime\n".to_owned(),
                carried_entries: 1,
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
            });
        }
        assert_eq!(kinds.len(), 24, "a variant has no sample");
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
            },
            log::Event::RequestFailed {
                request: 3,
                reason: log::FailReason::ContextOverflow,
                message: "too long".to_owned(),
                status: Some(400),
                partial: None,
            },
            // Nothing arrived before this crash: no `partial` at all.
            log::Event::RequestFailed {
                request: 3,
                reason: log::FailReason::Crashed,
                message: "a panic".to_owned(),
                status: None,
                partial: None,
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
            },
            log::Event::Fork {
                lane: log::Lane::Interview,
                of_turn: 1,
                at: 3,
                why: log::Warrant::Scoping,
                question: "what did you decide?".to_owned(),
            },
            log::Event::Request {
                turn: 1,
                lane: log::Lane::Interview,
                head_sha256: Some("b".repeat(64)),
                fork: Some(20),
            },
            log::Event::ForkSettled {
                fork: 20,
                outcome: log::ForkOutcome::Value,
            },
            log::Event::Patch {
                fork: 20,
                op: log::PatchOp::Add,
                entry: log::PatchEntry {
                    id: "interview-t1-0".to_owned(),
                    text: "decision: keep it".to_owned(),
                    category: None,
                },
                supersedes: None,
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
            failed_line(3, &TransportFailure::Connect("refused".to_owned()), ""),
            log::Event::RequestFailed {
                request: 3,
                reason: log::FailReason::Transport,
                message: TransportFailure::Connect("refused".to_owned()).to_string(),
                status: None,
                partial: None,
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
            "ask cancel declare-seam end"
        );
        assert_eq!(
            tags(&Refusal::ALL.iter().map(|it| it.tag()).collect::<Vec<_>>()),
            "in-flight ended nothing-in-flight nothing-to-seam stale"
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
            "trunk interview"
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
    /// `openai`-shape turn 2, byte for byte.
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
            tools: vec![tool_loop::bash_tool()],
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
        }
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
                } => Some((logged.seq, *of_turn, *at, *why, question.clone())),
                _ => None,
            })
            .collect()
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
    // the operator's seam (#493)
    // -----------------------------------------------------------------------

    /// #493's test: one ask, a declared seam, one ask after it. The seam
    /// refills the trunk from working memory -- the head, the render after
    /// it, and no turn of the old trunk -- the next ask is sent on exactly
    /// that, and the log says so: the seam line carries the render, what was
    /// carried, and the head's digest either side, which the projection's
    /// rebuild of the next request's head checks.
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

        let digest = |messages: &[Message]| {
            let mut shape = template();
            shape.messages = messages.to_vec();
            crate::client::head::Head::of(&shape).digest().to_owned()
        };
        let log = session.events_from(0);
        let seam = log
            .iter()
            .position(|logged| matches!(logged.event, Event::Seamed { .. }))
            .expect("a seam line");
        assert!(
            matches!(&log[seam - 1].event, Event::IdleGap(gap) if gap.ended_by == GapEnd::Seam),
            "the gap the seam ended is logged just before it"
        );
        assert_eq!(
            line_of(&log[seam]).event,
            log::Event::Seam {
                at_turn: 1,
                reason: log::SeamReason::Operator,
                prefix_hash_before: digest(&before),
                prefix_hash_after: digest(&refilled),
                frame: crate::seam::render::FRAME_VERSION.to_owned(),
                render: render.clone(),
                carried_entries: 3,
                carried_turns: 0,
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
    /// any turn, or with no working memory, it is refused `nothing-to-seam`;
    /// mid-turn, `in-flight`. Each refusal is logged and changes nothing.
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
