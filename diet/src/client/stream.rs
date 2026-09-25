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
use std::io::{self, Read as _, Write as _};
use std::net::{Shutdown, TcpStream, ToSocketAddrs as _};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

use super::shape::RequestShape;
use super::transport::{
    self, Dechunker, Endpoint, Framing, TransportFailure, framing, header_end, is_timeout,
    status_of,
};
use super::wire;

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
    /// The server answered, and the answer was a refusal rather than a
    /// stream: a status other than `200`, or an `error` event in place of
    /// the answer. A reply, so not a [`TransportFailure`]; not an answer, so
    /// never text on the trunk.
    Rejected {
        /// The HTTP status (`200` for an `error` event inside a stream).
        status: u16,
        /// What the server said, as it said it.
        body: String,
    },
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

/// Streamed chat completions over HTTP/1.1, in llama-server's dialect.
///
/// **The dialect is MEASURED, not read from documentation**: llama-server at
/// commit `4df29be`, driven over a raw socket on 2026-09-25 with a tiny
/// random-weight model (#117). `fixtures/llama-server-4df29be-stream.http`
/// is that reply, and [`tests::the_real_servers_stream_is_read_piece_by_piece`]
/// replays it byte for byte. What was seen, and what this reads because of it:
///
/// * `text/event-stream` inside `Transfer-Encoding: chunked`, and ONE chunk
///   carrying TWO events -- so events are cut from the decoded byte stream,
///   never from a chunk;
/// * each event a `data: {json}` line and a blank line; `delta.content`
///   carries the answer, `null` on the first chunk (the role);
/// * `finish_reason` on a chunk with an empty delta, then (because
///   `include_usage` is asked for) a chunk with `choices: []` carrying
///   `usage` and `timings`, then `data: [DONE]`;
/// * closing the connection mid-answer CANCELS the server's task: the log
///   read `stop: cancel task` and `/slots` showed the slot idle within
///   0.5 s, at 47 of 1500 tokens. A client that merely stopped READING did
///   not: the slot kept generating for as long as the socket stayed open.
///   So [`Cancel`] shuts the socket; it does not just stop the loop.
///
/// What is NOT measured and is read by shape only: an `error` event mid-
/// stream (read as [`Ended::Rejected`]), and any server but this one.
#[derive(Debug, Clone)]
pub struct HttpStream {
    endpoint: Endpoint,
    reply_cap: usize,
}

impl HttpStream {
    /// A streaming transport pointed at `endpoint`, reading at most
    /// [`transport::MAX_REPLY_BYTES`].
    #[must_use]
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            reply_cap: transport::MAX_REPLY_BYTES,
        }
    }

    /// The same, with a smaller cap -- the unstreamed transport's reason,
    /// and its name: a 64 MiB guard is one no test can afford to fire, and a
    /// guard nobody has seen fire is not a guard. This is how it is seen.
    #[must_use]
    pub fn with_reply_cap(endpoint: Endpoint, reply_cap: usize) -> Self {
        Self {
            endpoint,
            reply_cap,
        }
    }
}

impl Streaming for HttpStream {
    fn stream(
        &self,
        shape: &RequestShape,
        deadline: Instant,
        cancel: &Cancel,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Ended, TransportFailure> {
        let started = Instant::now();
        let timeout = || TransportFailure::Timeout {
            after: started.elapsed(),
        };
        let remaining = || deadline.checked_duration_since(Instant::now());

        let address = (self.endpoint.host.as_str(), self.endpoint.port)
            .to_socket_addrs()
            .map_err(|why| TransportFailure::Connect(why.to_string()))?
            .next()
            .ok_or_else(|| {
                TransportFailure::Connect("the host resolves to no address".to_owned())
            })?;
        let budget = remaining().ok_or_else(timeout)?;
        let mut socket = TcpStream::connect_timeout(&address, budget).map_err(|why| {
            if is_timeout(&why) {
                timeout()
            } else {
                TransportFailure::Connect(why.to_string())
            }
        })?;

        // The stopper: a second handle on the same socket, shut from
        // whichever thread asks. Shutting it is what makes a read blocked
        // below return, and what the server sees as the client leaving.
        let waker = socket.try_clone().map_err(|why| {
            TransportFailure::Connect(format!("no second handle to cancel through: {why}"))
        })?;
        cancel.on_cancel(move || {
            let _ = waker.shutdown(Shutdown::Both);
        });

        let body = wire::streaming_body(shape);
        let request = format!(
            "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\n\
             Accept: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.endpoint.path,
            self.endpoint.host,
            self.endpoint.port,
            body.len(),
            body
        );
        let budget = remaining().ok_or_else(timeout)?;
        socket
            .set_write_timeout(Some(budget))
            .map_err(|why| TransportFailure::Connect(why.to_string()))?;
        if let Err(why) = socket
            .write_all(request.as_bytes())
            .and_then(|()| socket.flush())
        {
            return if cancel.is_asked() {
                Ok(Ended::Cancelled)
            } else if is_timeout(&why) {
                Err(timeout())
            } else {
                Err(TransportFailure::Write(why.to_string()))
            };
        }

        let mut reading = Reading::default();
        let mut buffer = [0_u8; 16 * 1024];
        loop {
            if cancel.is_asked() {
                return Ok(Ended::Cancelled);
            }
            // Re-armed on every pass, as the unstreamed transport does: one
            // timeout bounds one read, not the call.
            let budget = remaining().ok_or_else(timeout)?;
            socket
                .set_read_timeout(Some(budget))
                .map_err(|why| TransportFailure::Read(why.to_string()))?;
            let count = match socket.read(&mut buffer) {
                Ok(count) => count,
                Err(why) if why.kind() == io::ErrorKind::Interrupted => continue,
                // A read that fails because the socket was shut under it is
                // the stop landing, not the transport failing.
                Err(_) if cancel.is_asked() => return Ok(Ended::Cancelled),
                Err(why) if is_timeout(&why) => return Err(timeout()),
                Err(why) => return Err(TransportFailure::Read(why.to_string())),
            };
            if count == 0 {
                if cancel.is_asked() {
                    return Ok(Ended::Cancelled);
                }
                return reading.at_close();
            }
            if let Some(ended) = reading.feed(&buffer[..count], self.reply_cap, on_delta)? {
                return Ok(ended);
            }
        }
    }

    fn describes(&self) -> String {
        format!(
            "http://{}:{}{} (streamed)",
            self.endpoint.host, self.endpoint.port, self.endpoint.path
        )
    }
}

/// A streamed reply, read as it arrives: headers, then a body in whatever
/// framing they declared, then events cut from that body.
#[derive(Debug, Default)]
struct Reading {
    /// Everything before the headers were whole.
    raw: Vec<u8>,
    /// Every byte read, against the cap.
    total: usize,
    /// The status and framing, once the headers are whole.
    head: Option<(u16, Framing)>,
    /// Body bytes received under `Framing::Length`.
    received: usize,
    chunks: Dechunker,
    events: Events,
    /// The body of a refusal, kept whole.
    refusal: Vec<u8>,
    finish_reason: Option<String>,
}

impl Reading {
    fn feed(
        &mut self,
        bytes: &[u8],
        cap: usize,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Option<Ended>, TransportFailure> {
        self.total += bytes.len();
        if self.total > cap {
            return Err(TransportFailure::Read(format!(
                "the reply passed {cap} bytes and was not finished"
            )));
        }
        let body = if self.head.is_some() {
            bytes.to_vec()
        } else {
            self.raw.extend_from_slice(bytes);
            let Some(end) = header_end(&self.raw) else {
                return Ok(None);
            };
            let headers = String::from_utf8_lossy(&self.raw[..end]).into_owned();
            self.head = Some((status_of(&headers)?, framing(&headers)));
            let rest = self.raw[end + 4..].to_vec();
            self.raw.clear();
            rest
        };
        self.body(&body, on_delta)
    }

    fn body(
        &mut self,
        bytes: &[u8],
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Option<Ended>, TransportFailure> {
        let Some((status, framing)) = self.head else {
            return Ok(None);
        };
        let (decoded, whole) = match framing {
            Framing::Chunked => {
                let decoded = self.chunks.feed(bytes).map_err(TransportFailure::Framing)?;
                (decoded, self.chunks.is_done())
            }
            Framing::Length(announced) => {
                let take = bytes.len().min(announced.saturating_sub(self.received));
                self.received += take;
                (bytes[..take].to_vec(), self.received >= announced)
            }
            Framing::ToClose => (bytes.to_vec(), false),
        };
        if status != 200 {
            self.refusal.extend_from_slice(&decoded);
            return Ok(whole.then(|| self.refused(status)));
        }
        for data in self.events.feed(&decoded)? {
            if let Some(ended) = self.event(&data, on_delta)? {
                return Ok(Some(ended));
            }
        }
        if whole {
            return self.at_close().map(Some);
        }
        Ok(None)
    }

    fn refused(&self, status: u16) -> Ended {
        Ended::Rejected {
            status,
            body: String::from_utf8_lossy(&self.refusal).into_owned(),
        }
    }

    /// One event's data. `Some` when it ends the stream.
    fn event(
        &mut self,
        data: &str,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<Option<Ended>, TransportFailure> {
        if data == "[DONE]" {
            return Ok(Some(Ended::Finished {
                finish_reason: self.finish_reason.take(),
            }));
        }
        let value: Value = serde_json::from_str(data).map_err(|why| {
            TransportFailure::Framing(format!("an event's data is not JSON ({why}): {data}"))
        })?;
        if value.get("error").is_some() {
            return Ok(Some(Ended::Rejected {
                status: 200,
                body: data.to_owned(),
            }));
        }
        if let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        {
            if let Some(piece) = choice.pointer("/delta/content").and_then(Value::as_str)
                && !piece.is_empty()
            {
                on_delta(piece);
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(reason.to_owned());
            }
        }
        Ok(None)
    }

    /// The server closed, or the body's framing said it was whole.
    fn at_close(&mut self) -> Result<Ended, TransportFailure> {
        let Some((status, framing)) = self.head else {
            return Err(TransportFailure::Malformed(
                String::from_utf8_lossy(&self.raw)
                    .chars()
                    .take(120)
                    .collect(),
            ));
        };
        match framing {
            Framing::Chunked if !self.chunks.is_done() => {
                return Err(TransportFailure::Framing(
                    "the connection closed inside a chunked body".to_owned(),
                ));
            }
            Framing::Length(announced) if self.received < announced => {
                return Err(TransportFailure::Truncated {
                    announced,
                    received: self.received,
                });
            }
            _ => {}
        }
        if status != 200 {
            return Ok(self.refused(status));
        }
        // A server that said why it stopped and then closed without `[DONE]`
        // finished; one that never said why did not.
        match self.finish_reason.take() {
            Some(reason) => Ok(Ended::Finished {
                finish_reason: Some(reason),
            }),
            None => Err(TransportFailure::Framing(
                "the stream ended before the server said it was done".to_owned(),
            )),
        }
    }
}

/// Server-sent events cut from a decoded body: each event ends at a blank
/// line, and its `data:` lines are its payload. Nothing is decoded as text
/// until its event is whole, so a character split across two reads -- or two
/// chunks -- is never split in what the caller sees.
#[derive(Debug, Default)]
struct Events {
    pending: Vec<u8>,
}

impl Events {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, TransportFailure> {
        self.pending.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            let lf = find(&self.pending, b"\n\n").map(|at| (at, 2));
            let crlf = find(&self.pending, b"\r\n\r\n").map(|at| (at, 4));
            let Some((at, width)) = [lf, crlf].into_iter().flatten().min() else {
                break;
            };
            let event: Vec<u8> = self.pending.drain(..at + width).collect();
            let text = std::str::from_utf8(&event[..at]).map_err(|why| {
                TransportFailure::Framing(format!("an event is not UTF-8: {why}"))
            })?;
            let data: Vec<&str> = text
                .lines()
                .filter_map(|line| line.strip_prefix("data:"))
                .map(|value| value.strip_prefix(' ').unwrap_or(value))
                .collect();
            // An event with no data line -- a comment, a keep-alive -- says
            // nothing about the answer.
            if !data.is_empty() {
                out.push(data.join("\n"));
            }
        }
        Ok(out)
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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
    /// Answer with a refusal rather than a stream.
    Reject(u16, String),
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
                Step::Reject(status, body) => return Ok(Ended::Rejected { status, body }),
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

    // ---- the HTTP transport, against real bytes and a real socket ----

    use crate::client::stub::{Act, Held, Stub};

    /// llama-server `4df29be`'s reply to a streamed request with
    /// `include_usage`, captured off the wire. One edit: the `model` field
    /// held the capturing machine's absolute path and now reads `tiny.gguf`,
    /// with each chunk's size recomputed and its boundary kept where the
    /// server put it -- including the chunk that carries two events.
    const CAPTURED: &[u8] =
        include_bytes!("../../client/fixtures/llama-server-4df29be-stream.http");

    /// What the captured stream's deltas say, in order: the model is random
    /// weights, so the words mean nothing, and `су` is two two-byte
    /// characters -- which is why they are here.
    const CAPTURED_PIECES: [&str; 6] = ["mittel", "су", " polity", " polity", " polity", " polity"];

    fn endpoint(stub: &Stub) -> Endpoint {
        Endpoint::parse(&stub.url()).expect("the stub's URL is an endpoint")
    }

    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(10)
    }

    #[test]
    fn the_real_servers_stream_is_read_piece_by_piece() {
        let stub = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
        let transport = HttpStream::new(endpoint(&stub));
        let mut pieces = Vec::new();
        let ended = transport.stream(&shape(), deadline(), &Cancel::new(), &mut |piece| {
            pieces.push(piece.to_owned());
        });
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("length".to_owned())
            })
        );
        assert_eq!(pieces, CAPTURED_PIECES);
        let sent = stub.received();
        assert!(
            sent[0].contains(r#""stream":true"#) && sent[0].contains(r#""include_usage":true"#),
            "the request did not ask to stream with usage: {}",
            sent[0]
        );
    }

    #[test]
    fn the_real_servers_stream_read_one_byte_at_a_time_says_the_same() {
        // Every boundary a read can fall on: inside a chunk-size line, inside
        // `\r\n`, inside an event's blank line, and between the two bytes of
        // a character.
        let mut reading = Reading::default();
        let mut pieces = Vec::new();
        let mut ended = None;
        for byte in CAPTURED {
            if let Some(done) = reading
                .feed(std::slice::from_ref(byte), usize::MAX, &mut |piece| {
                    pieces.push(piece.to_owned());
                })
                .expect("the captured bytes are a well-formed stream")
            {
                ended = Some(done);
                break;
            }
        }
        assert_eq!(
            ended,
            Some(Ended::Finished {
                finish_reason: Some("length".to_owned())
            })
        );
        assert_eq!(pieces, CAPTURED_PIECES);
    }

    #[test]
    fn a_cancel_mid_stream_closes_the_connection_and_the_server_sees_it() {
        let piece =
            r#"data: {"choices":[{"index":0,"delta":{"content":"Hel"},"finish_reason":null}]}"#;
        let stub = Stub::serving(vec![Act::StreamThenHold(vec![format!("{piece}\n\n")])])
            .expect("loopback");
        let transport = HttpStream::new(endpoint(&stub));
        let cancel = Cancel::new();
        let asker = cancel.clone();
        let (first, arrived) = std::sync::mpsc::channel();
        let (result, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ended = transport.stream(&shape(), deadline(), &cancel, &mut |piece| {
                let _ = first.send(piece.to_owned());
            });
            let _ = result.send(ended);
        });
        assert_eq!(
            arrived.recv_timeout(Duration::from_secs(10)).as_deref(),
            Ok("Hel"),
            "the first piece never arrived"
        );
        // The server is now holding the connection open and sending nothing:
        // the client is blocked in a read. Only the stopper can reach it.
        cancel_and_expect(&asker, &finished);

        // And from the SERVER's side of the socket: the client left, promptly.
        let seen = wait_for_a_hangup(&stub);
        let Held::HungUp(after) = seen else {
            panic!("the server never saw the client leave: {seen:?}");
        };
        assert!(
            after < Duration::from_secs(5),
            "the client left after {after:?}"
        );
    }

    fn cancel_and_expect(
        asker: &Cancel,
        finished: &std::sync::mpsc::Receiver<Result<Ended, TransportFailure>>,
    ) {
        asker.ask();
        assert_eq!(
            finished.recv_timeout(Duration::from_secs(5)),
            Ok(Ok(Ended::Cancelled)),
            "a cancel did not reach a call blocked mid-stream"
        );
    }

    /// Bounded, and read on the server's own schedule: the stub records the
    /// hang-up when its read returns, which is after the client's call has
    /// already come back.
    fn wait_for_a_hangup(stub: &Stub) -> Held {
        let give_up = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(held) = stub.hangups().first() {
                return *held;
            }
            assert!(Instant::now() < give_up, "the stub recorded nothing");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn a_chunk_boundary_inside_an_event_is_invisible_to_the_reader() {
        // The captured server only ever cut chunks at event boundaries, so a
        // reader that never de-chunked would read it anyway: a chunk-size line
        // lands between events, where it looks like a line with no `data:`.
        // Cut INSIDE the JSON and it lands in the middle of a value.
        let event = concat!(
            r#"data: {"choices":[{"index":0,"delta":{"content":"whole"},"finish_reason":"stop"}]}"#,
            "\n\ndata: [DONE]\n\n"
        );
        let pieces = vec![
            event[..20].to_owned(),
            event[20..47].to_owned(),
            event[47..].to_owned(),
        ];
        let stub = Stub::serving(vec![Act::Chunked(pieces)]).expect("loopback");
        let mut seen = Vec::new();
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |piece| seen.push(piece.to_owned()),
        );
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("stop".to_owned())
            })
        );
        assert_eq!(seen, ["whole"]);
    }

    #[test]
    fn a_stream_past_the_reply_cap_is_refused_rather_than_read_forever() {
        let piece =
            r#"data: {"choices":[{"index":0,"delta":{"content":"more"},"finish_reason":null}]}"#;
        let stub = Stub::serving(vec![Act::StreamThenHold(vec![format!("{piece}\n\n"); 8])])
            .expect("loopback");
        let ended = HttpStream::with_reply_cap(endpoint(&stub), 256).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(
            matches!(ended, Err(TransportFailure::Read(ref why)) if why.contains("passed 256 bytes")),
            "{ended:?}"
        );
    }

    #[test]
    fn a_status_other_than_200_is_a_rejection_carrying_what_the_server_said() {
        let stub = Stub::serving(vec![Act::Status(503, r#"{"error":"busy"}"#.to_owned())])
            .expect("loopback");
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |_| panic!("a refusal delivered a piece"),
        );
        assert_eq!(
            ended,
            Ok(Ended::Rejected {
                status: 503,
                body: r#"{"error":"busy"}"#.to_owned()
            })
        );
    }

    #[test]
    fn a_stream_that_ends_without_saying_it_is_done_is_a_failure_not_an_answer() {
        let piece =
            r#"data: {"choices":[{"index":0,"delta":{"content":"par"},"finish_reason":null}]}"#;
        let stub =
            Stub::serving(vec![Act::Chunked(vec![format!("{piece}\n\n")])]).expect("loopback");
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(
            matches!(ended, Err(TransportFailure::Framing(ref why)) if why.contains("before the server said it was done")),
            "{ended:?}"
        );
    }

    #[test]
    fn an_event_whose_data_is_not_json_is_a_framing_failure() {
        let stub = Stub::serving(vec![Act::Chunked(vec!["data: not json\n\n".to_owned()])])
            .expect("loopback");
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(
            matches!(ended, Err(TransportFailure::Framing(ref why)) if why.contains("not JSON")),
            "{ended:?}"
        );
    }
}
