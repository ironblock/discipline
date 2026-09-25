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
//! **What this does not do yet**, and says: no timings or cache telemetry on
//! a turn (R3), no fork in the capture gap (R4), no patches (R5), and no
//! seam -- [`Session::declare_seam`] is refused as [`Refusal::SeamNotBuilt`]
//! until R6, so the command exists for the surface to wire and answers
//! truthfully meanwhile. The log is in memory; how it is serialised for
//! anything outside this crate is an open question on #117.

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use crate::client::shape::{Message, RequestShape, Role};
use crate::client::stream::{Cancel, Ended as StreamEnded, Streaming};
use crate::client::transport::TransportFailure;
use crate::client::vocabulary;

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
    }
}

/// One thing that happened in a session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// An ask was accepted, and a turn begins on it.
    Asked {
        /// What the person asked.
        text: String,
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
    /// A piece of the trunk's answer arrived.
    Delta {
        /// The piece, as the server sent it.
        text: String,
    },
    /// A stop was asked for the call in flight. What the call did about it
    /// is the next terminal event of the turn, which may be any of them:
    /// [`Event::Cancelled`] if it stopped, [`Event::Answered`] if the answer
    /// was already done, or [`Event::Rejected`], [`Event::Failed`] or
    /// [`Event::Crashed`] if the call ended some other way first.
    StopAsked,
    /// The trunk answered. The ask and this answer are now on the trunk.
    Answered {
        /// The whole answer: every [`Event::Delta`] of this turn, in order.
        text: String,
        /// Why the server stopped, as it spelled it, if it said.
        finish_reason: Option<String>,
    },
    /// The turn was stopped. Neither its ask nor `partial` is on the trunk.
    Cancelled {
        /// What arrived before the stop. Never an answer.
        partial: String,
    },
    /// The server refused the turn rather than answering it. Neither its ask
    /// nor `partial` is on the trunk.
    Rejected {
        /// The HTTP status, or `200` for an `error` event inside a stream.
        status: u16,
        /// What the server said.
        body: String,
        /// What arrived before the refusal.
        partial: String,
    },
    /// The turn's own thread panicked, or could not be started, before it
    /// could settle. Neither its ask nor anything that arrived is on the
    /// trunk; what arrived is in the [`Event::Delta`]s before this.
    Crashed {
        /// Why: the panic's message, or the operating system's refusal.
        why: String,
    },
    /// The turn's call failed. Neither its ask nor `partial` is on the trunk.
    Failed {
        /// How it failed.
        failure: TransportFailure,
        /// What arrived before it failed.
        partial: String,
    },
}

/// An event and its place in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    /// Its position in the log: gapless, from zero.
    pub seq: u64,
    /// What happened.
    pub event: Event,
}

struct State {
    settlement: Settlement,
    trunk: Vec<Message>,
    log: Vec<Logged>,
    cancel: Option<Cancel>,
}

impl State {
    fn push(&mut self, event: Event) -> u64 {
        let seq = u64::try_from(self.log.len()).expect("a log longer than u64 cannot be built");
        self.log.push(Logged { seq, event });
        seq
    }

    fn move_to(&mut self, to: Settlement) {
        let from = self.settlement;
        self.settlement = to;
        self.push(Event::Settled { from, to });
    }

    fn refuse(&mut self, command: CommandKind, because: Refusal) -> Refusal {
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
        Self {
            shared: Arc::new(Shared {
                transport,
                template,
                state: Mutex::new(State {
                    settlement: Settlement::Awaiting,
                    trunk,
                    log: Vec::new(),
                    cancel: None,
                }),
                changed: Condvar::new(),
            }),
        }
    }

    /// Send an ask to the trunk. Returns the sequence number of its
    /// [`Event::Asked`]; the answer arrives in the log.
    ///
    /// # Errors
    ///
    /// [`Refusal::InFlight`] while a turn or a capture is in flight, and
    /// [`Refusal::Ended`] once the session has ended. Either is also logged.
    pub fn ask(&self, text: &str) -> Result<u64, Refusal> {
        let mut state = self.shared.lock();
        match state.settlement {
            Settlement::Awaiting => {}
            Settlement::Turn | Settlement::Capture => {
                let refused = state.refuse(CommandKind::Ask, Refusal::InFlight);
                self.shared.changed.notify_all();
                return Err(refused);
            }
            Settlement::Ended => {
                let refused = state.refuse(CommandKind::Ask, Refusal::Ended);
                self.shared.changed.notify_all();
                return Err(refused);
            }
        }
        let seq = state.push(Event::Asked {
            text: text.to_owned(),
        });
        state.move_to(Settlement::Turn);
        let cancel = Cancel::new();
        state.cancel = Some(cancel.clone());
        let mut shape = self.shared.template.clone();
        shape.messages.clone_from(&state.trunk);
        shape.messages.push(Message::new(Role::User, text));
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
                    turn(&shared, &shape, &cancel, ask);
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
        Ok(seq)
    }

    /// Ask the call in flight to stop.
    ///
    /// # Errors
    ///
    /// [`Refusal::NothingInFlight`] when no turn is in flight, and
    /// [`Refusal::Ended`] once the session has ended. Either is also logged.
    pub fn cancel(&self) -> Result<(), Refusal> {
        let mut state = self.shared.lock();
        let refused = match (state.settlement, state.cancel.clone()) {
            (Settlement::Turn, Some(cancel)) => {
                state.push(Event::StopAsked);
                drop(state);
                self.shared.changed.notify_all();
                // Outside the lock: a stopper may do anything a transport
                // needs to wake its call, and none of it should be done while
                // holding the session.
                cancel.ask();
                return Ok(());
            }
            (Settlement::Ended, _) => state.refuse(CommandKind::Cancel, Refusal::Ended),
            _ => state.refuse(CommandKind::Cancel, Refusal::NothingInFlight),
        };
        drop(state);
        self.shared.changed.notify_all();
        Err(refused)
    }

    /// Declare a seam. Not built until #117 R6.
    ///
    /// # Errors
    ///
    /// Always: [`Refusal::SeamNotBuilt`], or [`Refusal::Ended`] once the
    /// session has ended. Logged either way.
    pub fn declare_seam(&self) -> Result<(), Refusal> {
        let mut state = self.shared.lock();
        let because = if state.settlement == Settlement::Ended {
            Refusal::Ended
        } else {
            Refusal::SeamNotBuilt
        };
        let refused = state.refuse(CommandKind::DeclareSeam, because);
        drop(state);
        self.shared.changed.notify_all();
        Err(refused)
    }

    /// End the session.
    ///
    /// # Errors
    ///
    /// [`Refusal::InFlight`] while a turn or a capture is in flight -- stop
    /// it first -- and [`Refusal::Ended`] if it already ended. Logged.
    pub fn end(&self) -> Result<(), Refusal> {
        let mut state = self.shared.lock();
        let outcome = match state.settlement {
            Settlement::Awaiting => {
                state.move_to(Settlement::Ended);
                Ok(())
            }
            Settlement::Turn | Settlement::Capture => {
                Err(state.refuse(CommandKind::End, Refusal::InFlight))
            }
            Settlement::Ended => Err(state.refuse(CommandKind::End, Refusal::Ended)),
        };
        drop(state);
        self.shared.changed.notify_all();
        outcome
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
        let cancel = self.shared.lock().cancel.clone();
        if let Some(cancel) = cancel {
            cancel.ask();
        }
    }
}

fn from(log: &[Logged], first: u64) -> Vec<Logged> {
    let start = usize::try_from(first).map_or(log.len(), |start| start.min(log.len()));
    log[start..].to_vec()
}

/// One turn's call, on its own thread: stream the answer into the log, then
/// settle.
fn turn<S: Streaming>(shared: &Shared<S>, shape: &RequestShape, cancel: &Cancel, ask: String) {
    let deadline = Instant::now() + shared.template.limits.call;
    let mut partial = String::new();
    let result = shared
        .transport
        .stream(shape, deadline, cancel, &mut |piece: &str| {
            partial.push_str(piece);
            shared.lock().push(Event::Delta {
                text: piece.to_owned(),
            });
            shared.changed.notify_all();
        });

    let mut state = shared.lock();
    state.cancel = None;
    match result {
        Ok(StreamEnded::Finished { finish_reason }) => {
            state.trunk.push(Message::new(Role::User, ask));
            state
                .trunk
                .push(Message::new(Role::Assistant, partial.clone()));
            state.push(Event::Answered {
                text: partial,
                finish_reason,
            });
            state.move_to(Settlement::Capture);
            // R4's interviews run here. Until they exist there is nothing to
            // capture, and the log says the session passed through.
            state.move_to(Settlement::Awaiting);
        }
        Ok(StreamEnded::Cancelled) => {
            state.push(Event::Cancelled { partial });
            state.move_to(Settlement::Awaiting);
        }
        Ok(StreamEnded::Rejected { status, body }) => {
            state.push(Event::Rejected {
                status,
                body,
                partial,
            });
            state.move_to(Settlement::Awaiting);
        }
        Err(failure) => {
            state.push(Event::Failed { failure, partial });
            state.move_to(Settlement::Awaiting);
        }
    }
    drop(state);
    shared.changed.notify_all();
}

/// Settle a turn that ended without [`turn`] settling it.
fn crashed<S>(shared: &Shared<S>, why: String) {
    let mut state = shared.lock();
    if state.settlement == Settlement::Turn {
        state.cancel = None;
        state.push(Event::Crashed { why });
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
mod tests {
    use super::*;
    use crate::client::shape::{Limits, SamplerCard};
    use crate::client::stream::{Canned, Gate, Step};

    const HEAD: &str = "you are the trunk";

    fn template() -> RequestShape {
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

    fn deltas(pieces: &[&str]) -> Vec<Step> {
        pieces
            .iter()
            .map(|piece| Step::Delta((*piece).to_owned()))
            .collect()
    }

    /// Wait until `done` holds of the whole log, or fail -- never hang. A
    /// cancel that does not reach its call would otherwise show up as a test
    /// that never finishes rather than one that fails.
    fn wait_until<S: Streaming + 'static>(
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

    fn settled(log: &[Logged]) -> bool {
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
        assert_eq!(session.ask("say hello"), Ok(0));
        let log = wait_until(&session, "the turn to settle", settled);
        assert_eq!(
            events(&log),
            [
                Event::Asked {
                    text: "say hello".to_owned()
                },
                Event::Settled {
                    from: Settlement::Awaiting,
                    to: Settlement::Turn
                },
                Event::Delta {
                    text: "Hel".to_owned()
                },
                Event::Delta {
                    text: "lo".to_owned()
                },
                Event::Delta {
                    text: "!".to_owned()
                },
                Event::Answered {
                    text: "Hello!".to_owned(),
                    finish_reason: Some("stop".to_owned())
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
            session.ask(ask).expect("an ask while awaiting is accepted");
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
        session.ask("first").expect("accepted");
        wait_until(&session, "the first piece", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::Delta { .. }))
        });

        assert_eq!(session.ask("second"), Err(Refusal::InFlight));
        assert_eq!(session.end(), Err(Refusal::InFlight));
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
        session.ask("first").expect("accepted");
        // The cancel goes in once the call is BLOCKED, never in the window
        // between its first piece and the gate: there, the call's own flag
        // check would stop it and this test would pass with the stopper
        // broken, on whichever runs happened to land in the window.
        assert!(
            gate.wait_for_a_waiter(Duration::from_secs(10)),
            "the call never reached the gate"
        );

        assert_eq!(session.cancel(), Ok(()));
        let log = wait_until(&session, "the stopped turn to settle", settled);
        let tail = events(&log[log.len() - 3..]);
        assert_eq!(
            tail,
            [
                Event::StopAsked,
                Event::Cancelled {
                    partial: "Hel".to_owned()
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
        session.ask("second").expect("accepted after a cancel");
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
        session.ask("first").expect("accepted");
        let log = wait_until(&session, "the failed turn to settle", settled);
        assert!(log.iter().any(|logged| logged.event
            == Event::Failed {
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
        session.ask("first").expect("accepted");
        let log = wait_until(&session, "the refused turn to settle", settled);
        assert!(log.iter().any(|logged| logged.event
            == Event::Rejected {
                status: 503,
                body: "busy".to_owned(),
                partial: "par".to_owned()
            }));
        assert!(
            !log.iter()
                .any(|logged| matches!(logged.event, Event::Answered { .. })),
            "a refusal was logged as an answer"
        );
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
            on_delta: &mut dyn FnMut(&str),
        ) -> Result<StreamEnded, TransportFailure> {
            on_delta("half");
            panic!("a transport that panics mid-answer (seeded by this test)");
        }

        fn describes(&self) -> String {
            "a transport that panics".to_owned()
        }
    }

    #[test]
    fn a_turn_whose_thread_panics_still_settles_and_the_session_goes_on() {
        let session = Session::open(Panics, template());
        session.ask("first").expect("accepted");
        let log = wait_until(&session, "the crashed turn to settle", settled);
        let tail = events(&log[log.len() - 2..]);
        assert_eq!(
            tail,
            [
                Event::Crashed {
                    why: "a transport that panics mid-answer (seeded by this test)".to_owned()
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
            session.ask("again").is_ok(),
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
        session.ask("first").expect("accepted");
        wait_until(&session, "the first piece", |log| {
            log.iter()
                .any(|logged| matches!(logged.event, Event::Delta { .. }))
        });
        // The server is holding the connection open and sending nothing: the
        // turn's thread is blocked in a read that only the stopper can end.
        assert_eq!(session.cancel(), Ok(()));
        let log = wait_until(&session, "the stopped turn to settle", settled);
        assert!(
            log.iter().any(|logged| logged.event
                == Event::Cancelled {
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
    fn a_command_with_nothing_to_act_on_is_refused_and_logged() {
        let session = Session::open(Canned::new([]), template());
        assert_eq!(session.cancel(), Err(Refusal::NothingInFlight));
        assert_eq!(session.declare_seam(), Err(Refusal::SeamNotBuilt));
        assert_eq!(session.end(), Ok(()));
        assert_eq!(session.settlement(), Settlement::Ended);
        assert_eq!(session.ask("too late"), Err(Refusal::Ended));
        assert_eq!(session.cancel(), Err(Refusal::Ended));
        assert_eq!(session.declare_seam(), Err(Refusal::Ended));
        assert_eq!(session.end(), Err(Refusal::Ended));
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
        assert!(
            session.shared.transport.sent().is_empty(),
            "a refused ask reached the transport"
        );
    }

    #[test]
    fn the_log_is_numbered_by_position_and_a_reader_can_resume_from_any_point() {
        let session = Session::open(Canned::new([deltas(&["a", "b"])]), template());
        session.ask("go").expect("accepted");
        let log = wait_until(&session, "the turn to settle", settled);
        for (index, logged) in log.iter().enumerate() {
            assert_eq!(logged.seq, index as u64);
        }
        assert_eq!(session.events_from(3), log[3..]);
        assert!(session.events_from(log.len() as u64).is_empty());
        assert!(session.events_from(u64::MAX).is_empty());
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
            "in-flight ended nothing-in-flight seam-not-built"
        );
    }
}
