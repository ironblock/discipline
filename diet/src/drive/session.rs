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
//! where R4's idle-gap interview will run; until it exists the session
//! passes through `capture` and straight back to `awaiting`, and says so in
//! the log rather than skipping the state.
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
//! **What this does not do yet**, and says: no fork in the capture gap (R4),
//! no patches (R5), and no
//! seam -- [`Session::declare_seam`] is refused as [`Refusal::SeamNotBuilt`]
//! until R6, so the command exists for the surface to wire and answers
//! truthfully meanwhile. The log is in memory. It is a format,
//! `diet/formats/log` (ruled on #117), and what each event carries is what
//! R2c's plan (`diet/drive/plans/r2c-proposal.md`, D4) drafts for it: a
//! header line, a turn counter, a `request` per call that the call's other
//! events cite by its sequence number, and a `turn.settled` with a reason.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::client::shape::{Message, RequestShape, Role};
use crate::client::stream::{
    Cancel, Ended as StreamEnded, Piece, Progress, Rejection, Streaming, Timings,
};
use crate::client::transport::TransportFailure;
use crate::client::vocabulary;
use crate::formats::log;

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
        /// Seams are not built yet (#117 R6).
        SeamNotBuilt => "seam-not-built",
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
    /// Which lane a request is made on. The trunk is the only one until R4.
    Lane {
        /// The canonical session.
        Trunk => "trunk",
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
    },
    /// An ask was accepted, and a turn begins on it.
    Asked {
        /// The turn it begins: 1 for the first ask admitted, then counting.
        turn: u32,
        /// What the person asked.
        text: String,
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
    /// was already done, or [`Event::Rejected`], [`Event::Failed`] or
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
    /// The call hit its output cap (`finish_reason: "length"`): what it
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
    #[must_use]
    pub fn open(transport: S, template: RequestShape) -> Self {
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
        };
        state.push(Event::Started {
            opened,
            model: template.model.clone(),
            head: template.messages.clone(),
        });
        Self {
            shared: Arc::new(Shared {
                transport,
                template,
                state: Mutex::new(state),
                changed: Condvar::new(),
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
    /// [`Refusal::Ended`] once the session has ended; either is also logged.
    /// [`Rejected::BadGap`] when `gap` cannot be logged, and then nothing is.
    pub fn ask(&self, text: &str, gap: Option<IdleGap>) -> Result<Admitted, Rejected> {
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
            text: text.to_owned(),
        });
        state.move_to(Settlement::Turn);
        let mut shape = self.shared.template.clone();
        shape.messages.clone_from(&state.trunk);
        shape.messages.push(Message::new(Role::User, text));
        // Pushed here, under the lock that admits the ask, and never on the
        // turn's thread: a thread that cannot start still settles with a
        // `request.failed` that cites a request that exists (#117, R2c
        // finding 21).
        let request = state.push(Event::Requested {
            turn,
            lane: Lane::Trunk,
            head_sha256: crate::client::head::Head::of(&shape).digest().to_owned(),
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
        let ask = text.to_owned();
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
    /// Otherwise refused, and logged: [`Refusal::Ended`] once the session
    /// has ended, [`Refusal::Stale`] for a turn older than the latest, and
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
            (Settlement::Turn, Some(flight)) => {
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

    /// Declare a seam. Not built until #117 R6.
    ///
    /// # Errors
    ///
    /// Always: [`Refusal::SeamNotBuilt`], or [`Refusal::Ended`] once the
    /// session has ended. Logged either way; a refused command's `gap` is
    /// neither logged nor closed.
    pub fn declare_seam(&self, gap: Option<IdleGap>) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        state.carry(gap, CommandKind::DeclareSeam);
        let because = if state.settlement == Settlement::Ended {
            Refusal::Ended
        } else {
            Refusal::SeamNotBuilt
        };
        let refused = state.refuse(CommandKind::DeclareSeam, because);
        drop(state);
        self.shared.changed.notify_all();
        Err(Rejected::Refused(refused))
    }

    /// End the session.
    ///
    /// # Errors
    ///
    /// [`Refusal::InFlight`] while a turn or a capture is in flight -- stop
    /// it first -- and [`Refusal::Ended`] if it already ended; logged, and the
    /// refused `gap` is neither logged nor closed. Or [`Rejected::BadGap`]
    /// when an admitted end's `gap` cannot be logged: then it does not end,
    /// and nothing is logged.
    pub fn end(&self, gap: Option<IdleGap>) -> Result<(), Rejected> {
        let mut state = self.shared.lock();
        state.carry(gap, CommandKind::End);
        let outcome = match state.settlement {
            Settlement::Awaiting => state.admit().map(|()| state.move_to(Settlement::Ended)),
            Settlement::Turn | Settlement::Capture => Err(Rejected::Refused(
                state.refuse(CommandKind::End, Refusal::InFlight),
            )),
            Settlement::Ended => Err(Rejected::Refused(
                state.refuse(CommandKind::End, Refusal::Ended),
            )),
        };
        drop(state);
        self.shared.changed.notify_all();
        outcome
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
            serving: None,
        },
        Event::Asked { turn, text } => log::Event::Ask {
            turn: *turn,
            text: text.clone(),
        },
        Event::Requested {
            turn,
            lane,
            head_sha256,
        } => log::Event::Request {
            turn: *turn,
            lane: match lane {
                Lane::Trunk => log::Lane::Trunk,
            },
            head_sha256: Some(head_sha256.clone()),
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
        Event::Answered {
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
        Event::TurnSettled { turn, reason } => log::Event::TurnSettled {
            turn: *turn,
            reason: settle_reason_in_the_log(*reason),
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

fn role_of(role: Role) -> log::Role {
    match role {
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
        Refusal::SeamNotBuilt => log::Refusal::SeamNotBuilt,
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

/// One turn's call, on its own thread: stream the answer into the log, then
/// settle.
fn call<S: Streaming>(
    shared: &Shared<S>,
    shape: &RequestShape,
    cancel: &Cancel,
    ask: String,
    turn: u32,
    request: u64,
) {
    let deadline = Instant::now() + shared.template.limits.call;
    let mut partial = String::new();
    let mut reasoning = String::new();
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
            };
            shared.lock().push(event);
            shared.changed.notify_all();
        });

    let mut state = shared.lock();
    state.flight = None;
    match result {
        Ok(StreamEnded::Finished {
            finish_reason,
            timings,
        }) => settle_finished(
            &mut state,
            ask,
            (request, turn),
            (partial, reasoning),
            (finish_reason, timings),
        ),
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
}

/// A call that finished. One its output cap ended is not an answer: off the
/// trunk, settled `failed` (#290, ruled 5969297103) -- a truncated reasoning
/// sent back as history looped a model on `</think>` (#94's measured
/// specimen). Any other is the turn's answer.
fn settle_finished(
    state: &mut State,
    ask: String,
    (request, turn): (u64, u32),
    (partial, reasoning): (String, String),
    (finish_reason, timings): (Option<String>, Option<Timings>),
) {
    if finish_reason.as_deref() == Some(CAPPED) {
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
        return;
    }
    state.trunk.push(Message::new(Role::User, ask));
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
    // R4's interviews run here. Until they exist there is nothing to
    // capture, and the log says the session passed through.
    state.move_to(Settlement::Awaiting);
}

/// The `finish_reason` a server gives a call its output cap ended.
const CAPPED: &str = "length";

/// How a failed call settles its turn: a call that ran out of time is a
/// `timeout`, and every other failure is `failed`.
fn settle_reason_of(failure: &TransportFailure) -> SettleReason {
    match failure {
        TransportFailure::Timeout { .. } => SettleReason::Timeout,
        _ => SettleReason::Failed,
    }
}

/// Settle a turn that ended without [`call`] settling it.
fn crashed<S>(shared: &Shared<S>, why: String) {
    let mut state = shared.lock();
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
                    text: "say hello".to_owned()
                },
                Event::Settled {
                    from: Settlement::Awaiting,
                    to: Settlement::Turn
                },
                Event::Requested {
                    turn: 1,
                    lane: Lane::Trunk,
                    head_sha256: first_head("say hello"),
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
            Err(Rejected::Refused(Refusal::SeamNotBuilt))
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
                    Refusal::SeamNotBuilt,
                    Settlement::Awaiting
                ),
                (CommandKind::Ask, Refusal::Ended, Settlement::Ended),
                (CommandKind::Cancel, Refusal::Ended, Settlement::Ended),
                (CommandKind::DeclareSeam, Refusal::Ended, Settlement::Ended),
                (CommandKind::End, Refusal::Ended, Settlement::Ended),
            ]
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
            },
            Event::Asked {
                turn: 1,
                text: "say \"hi\"\n".to_owned(),
            },
            Event::Settled {
                from: Settlement::Awaiting,
                to: Settlement::Turn,
            },
            Event::Requested {
                turn: 1,
                lane: Lane::Trunk,
                head_sha256: "a".repeat(64),
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
            });
        }
        assert_eq!(kinds.len(), 17, "a variant has no sample");
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
                serving: None,
                head: vec![log::HeadMessage {
                    role: log::Role::System,
                    content: HEAD.to_owned(),
                }],
            },
            log::Event::Ask {
                turn: 1,
                text: "say \"hi\"\n".to_owned(),
            },
            log::Event::Settlement {
                from: log::State::Awaiting,
                to: log::State::Turn,
            },
            log::Event::Request {
                turn: 1,
                lane: log::Lane::Trunk,
                head_sha256: Some("a".repeat(64)),
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
        ]
    }

    /// The writer's half of #249's rule: a server that reports `timings`
    /// gets no `usage` on its response line -- on llama.cpp the two are equal
    /// (measured on #157), and the reader refuses a line carrying both.
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

        // Refused: a seam is not built. The gap it carried is not logged, and
        // it stays open (ruled on #146, amending D13 (c)).
        assert_eq!(
            session.declare_seam(Some(gap(first, GapEnd::Seam))),
            Err(Rejected::Refused(Refusal::SeamNotBuilt))
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
            "in-flight ended nothing-in-flight seam-not-built stale"
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
            "trunk"
        );
    }
}
