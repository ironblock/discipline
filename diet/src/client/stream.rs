//! Streaming: an answer delivered piece by piece, and a cancel that reaches
//! the call delivering it.
//!
//! [`super::transport::Transport`] sends a request and hands back the whole
//! reply. A person reading an answer as it is written needs the pieces as
//! they arrive, and a person who has read enough needs the call to STOP --
//! not to be ignored while it runs to its cap. Those are the two things this
//! module adds (#117, R2), and each is a type:
//!
//! * [`Streaming`] delivers answer text through a callback as it arrives, and
//!   says how the call ended with [`Ended`].
//! * [`Cancel`] is shared between whoever may ask a call to stop and the
//!   transport running it. Asking sets a flag AND runs whatever the transport
//!   registered with [`Cancel::on_cancel`] -- because a flag alone reaches
//!   nothing that is blocked. A transport blocked in a socket read never
//!   looks at a flag; it has to be woken, and the thing that wakes it (for
//!   HTTP, shutting the socket) is the transport's to register.
//!
//! **A cancelled call is not an answer.** [`Ended::Cancelled`] is its own
//! variant, and what arrived before it is the caller's partial text, never a
//! finished reply. The rule this crate already holds for timeouts one layer
//! down, applied to the person's own stop.
//!
//! [`Canned`] is the in-memory transport: scripted pieces in call order, and
//! a [`Gate`] that holds a stream mid-answer until a test opens it or the
//! call is cancelled. It exists so the session above this can be driven
//! without a server and without a sleep -- a test that waits for a race to
//! land is a test whose verdict depends on the machine it ran on.

use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use super::shape::RequestShape;
use super::transport::TransportFailure;

/// A request to stop, shared between whoever may ask and the call it stops.
///
/// Cloning shares it: every clone sees the same flag and runs the same
/// stoppers.
#[derive(Clone, Default)]
pub struct Cancel {
    inner: Arc<CancelInner>,
}

#[derive(Default)]
struct CancelInner {
    asked: AtomicBool,
    stoppers: Mutex<Vec<Box<dyn FnOnce() + Send>>>,
}

impl fmt::Debug for Cancel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cancel")
            .field("asked", &self.is_asked())
            .finish_non_exhaustive()
    }
}

impl Cancel {
    /// A cancel nobody has asked for yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask the call to stop: set the flag, then run every registered stopper.
    ///
    /// Asking twice is asking once. The stoppers run on the asking thread.
    pub fn ask(&self) {
        // The flag first, so a stopper that wakes a reader finds it set.
        self.inner.asked.store(true, Ordering::SeqCst);
        let stoppers = std::mem::take(
            &mut *self
                .inner
                .stoppers
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        for stop in stoppers {
            stop();
        }
    }

    /// Whether a stop has been asked for.
    #[must_use]
    pub fn is_asked(&self) -> bool {
        self.inner.asked.load(Ordering::SeqCst)
    }

    /// Register what wakes this call if a stop is asked for.
    ///
    /// Run at once when a stop has ALREADY been asked -- a call that
    /// registers after the person pressed stop must not miss it. The check
    /// and the registration happen under one lock with [`Cancel::ask`]'s
    /// take, so there is no window in which a stopper is registered after
    /// the take and never runs.
    pub fn on_cancel(&self, stop: impl FnOnce() + Send + 'static) {
        let mut stoppers = self
            .inner
            .stoppers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.is_asked() {
            drop(stoppers);
            stop();
        } else {
            stoppers.push(Box::new(stop));
        }
    }
}

/// How a streamed call ended, when it ended with a reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// The server said the answer was done.
    Finished {
        /// Why it stopped, as the server spelled it, if it said.
        finish_reason: Option<String>,
    },
    /// A stop was asked for and the call stopped. What arrived before it is
    /// partial, and is the caller's to keep as partial.
    Cancelled,
}

/// A transport that delivers an answer as it arrives.
pub trait Streaming: Send + Sync {
    /// Send `shape`, calling `on_delta` with each piece of answer text as it
    /// arrives, until the server finishes, `cancel` is asked, or `deadline`
    /// passes.
    ///
    /// # Errors
    ///
    /// [`TransportFailure`] when the call failed rather than finished or was
    /// stopped -- including [`TransportFailure::Timeout`] at the deadline. A
    /// failure is never an [`Ended`], so a stream that broke off is never
    /// mistaken for one somebody stopped.
    fn stream(
        &self,
        shape: &RequestShape,
        deadline: Instant,
        cancel: &Cancel,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Ended, TransportFailure>;

    /// Where this transport sends, for the record.
    fn describes(&self) -> String;
}

/// A latch a canned stream waits at until it is opened.
///
/// Shared by cloning. Opening is permanent. It also says when somebody is
/// WAITING at it, so a test can act on a call it knows is blocked rather
/// than on one it hopes has got there: a cancel sent between a call's last
/// piece and its reaching the gate is caught by the call's own flag check,
/// and a test racing that window proves the flag, not the stopper -- and
/// proves it only on the runs where it loses the race.
#[derive(Debug, Clone, Default)]
pub struct Gate {
    inner: Arc<(Mutex<GateState>, Condvar)>,
}

#[derive(Debug, Default)]
struct GateState {
    open: bool,
    waiting: usize,
}

impl Gate {
    /// A closed gate.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Open it, waking everything waiting.
    pub fn open(&self) {
        let (state, changed) = &*self.inner;
        state.lock().unwrap_or_else(PoisonError::into_inner).open = true;
        changed.notify_all();
    }

    /// Block until somebody is waiting at the gate, or `patience` runs out.
    /// Whether somebody arrived.
    #[must_use]
    pub fn wait_for_a_waiter(&self, patience: Duration) -> bool {
        let (state, changed) = &*self.inner;
        let guard = state.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = changed
            .wait_timeout_while(guard, patience, |gate| gate.waiting == 0)
            .unwrap_or_else(PoisonError::into_inner);
        guard.waiting > 0
    }

    fn wait(&self) {
        let (state, changed) = &*self.inner;
        let mut guard = state.lock().unwrap_or_else(PoisonError::into_inner);
        guard.waiting += 1;
        changed.notify_all();
        while !guard.open {
            guard = changed.wait(guard).unwrap_or_else(PoisonError::into_inner);
        }
        guard.waiting -= 1;
    }
}

/// One step of a canned stream.
#[derive(Debug, Clone)]
pub enum Step {
    /// A piece of answer text.
    Delta(String),
    /// Wait here until the gate opens or the call is cancelled.
    Hold(Gate),
    /// Fail as a transport would.
    Fail(TransportFailure),
}

/// Scripted streams, one per call, in call order; and every request it was
/// sent, in the order it was sent.
#[derive(Debug, Default)]
pub struct Canned {
    replies: Mutex<VecDeque<Vec<Step>>>,
    sent: Mutex<Vec<RequestShape>>,
}

impl Canned {
    /// A transport that plays `replies` in order, one per call.
    #[must_use]
    pub fn new(replies: impl IntoIterator<Item = Vec<Step>>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            sent: Mutex::new(Vec::new()),
        }
    }

    /// Every request sent so far, in order.
    #[must_use]
    pub fn sent(&self) -> Vec<RequestShape> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Streaming for Canned {
    fn stream(
        &self,
        shape: &RequestShape,
        _deadline: Instant,
        cancel: &Cancel,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Ended, TransportFailure> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(shape.clone());
        let Some(steps) = self
            .replies
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front()
        else {
            // Out of script is a connection nobody answered, not an empty
            // answer: the rule the whole client is built on.
            return Err(TransportFailure::Connect(
                "the canned transport has no reply left for this call".to_owned(),
            ));
        };
        for step in steps {
            if cancel.is_asked() {
                return Ok(Ended::Cancelled);
            }
            match step {
                Step::Delta(text) => on_delta(&text),
                Step::Hold(gate) => {
                    // What a socket read blocked on a silent server looks
                    // like: nothing here polls the flag. Only the stopper
                    // registered below, or the test opening the gate, wakes
                    // it -- which is exactly what an HTTP transport has to
                    // arrange with its socket.
                    let waker = gate.clone();
                    cancel.on_cancel(move || waker.open());
                    gate.wait();
                }
                Step::Fail(failure) => return Err(failure),
            }
        }
        if cancel.is_asked() {
            return Ok(Ended::Cancelled);
        }
        Ok(Ended::Finished {
            finish_reason: Some("stop".to_owned()),
        })
    }

    fn describes(&self) -> String {
        "canned (in memory)".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    use crate::client::shape::{Limits, Message, Role, SamplerCard};

    fn shape() -> RequestShape {
        RequestShape {
            model: "a-model".to_owned(),
            messages: vec![Message::new(Role::User, "an ask")],
            sampler: SamplerCard::empty(),
            limits: Limits {
                attempt: Duration::from_secs(1),
                call: Duration::from_secs(1),
                max_output_tokens: 16,
                retries: 0,
            },
            grammar: None,
            template_kwargs: std::collections::BTreeMap::new(),
            tools: Vec::new(),
        }
    }

    #[test]
    fn asking_runs_every_stopper_once_and_asking_again_runs_none() {
        let cancel = Cancel::new();
        let ran = Arc::new(AtomicUsize::new(0));
        for _ in 0..2 {
            let ran = Arc::clone(&ran);
            cancel.on_cancel(move || {
                ran.fetch_add(1, Ordering::SeqCst);
            });
        }
        assert!(!cancel.is_asked());
        cancel.ask();
        assert!(cancel.is_asked());
        assert_eq!(ran.load(Ordering::SeqCst), 2);
        cancel.ask();
        assert_eq!(ran.load(Ordering::SeqCst), 2, "a stopper ran twice");
    }

    #[test]
    fn a_stopper_registered_after_the_ask_runs_at_once() {
        let cancel = Cancel::new();
        cancel.ask();
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        cancel.on_cancel(move || flag.store(true, Ordering::SeqCst));
        assert!(
            ran.load(Ordering::SeqCst),
            "a call that registered after the stop was asked missed it"
        );
    }

    #[test]
    fn a_canned_stream_held_mid_answer_is_woken_by_a_cancel_and_says_so() {
        let gate = Gate::new();
        let canned = Arc::new(Canned::new([vec![
            Step::Delta("Hel".to_owned()),
            Step::Hold(gate.clone()),
            Step::Delta("lo".to_owned()),
        ]]));
        let cancel = Cancel::new();
        let (sender, receiver) = std::sync::mpsc::channel();
        // Detached, and read with a timeout: a stop that does not wake the
        // held call must fail this test, not hang it.
        let call = Arc::clone(&canned);
        let held = cancel.clone();
        std::thread::spawn(move || {
            let mut pieces = String::new();
            let ended = call.stream(&shape(), Instant::now(), &held, &mut |piece| {
                pieces.push_str(piece);
            });
            let _ = sender.send((ended, pieces));
        });
        // Not a sleep and not a race: the ask is sent only once the call is
        // blocked at the gate, where nothing but the stopper can reach it.
        assert!(
            gate.wait_for_a_waiter(Duration::from_secs(10)),
            "the call never reached the gate"
        );
        cancel.ask();
        let (ended, seen) = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("the held call was never woken by the cancel");
        assert_eq!(ended, Ok(Ended::Cancelled));
        assert!(
            !seen.contains("lo"),
            "a piece past the stop was delivered: {seen:?}"
        );
    }

    #[test]
    fn a_canned_transport_out_of_script_fails_rather_than_answering_nothing() {
        let canned = Canned::new([]);
        let ended = canned.stream(&shape(), Instant::now(), &Cancel::new(), &mut |_| {});
        assert!(matches!(ended, Err(TransportFailure::Connect(_))));
    }
}
