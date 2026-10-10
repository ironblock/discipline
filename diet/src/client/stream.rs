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
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::formats::record::Count;
use crate::formats::record::json::Decimal;

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
        /// What the server measured of the request, if it said.
        timings: Option<Timings>,
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
        /// What kind of refusal the server's typed field says it is, when it
        /// says one this client names ([`Rejection::of`]).
        class: Option<Rejection>,
    },
}

/// A request's timings as the server reported them: llama.cpp's `timings`
/// object, which it sends once, on a stream's last data chunk (#117 R3, D1).
///
/// Named as the server names them. Each is absent when the server did not
/// send it, never zero, and absent too when it is not a number the record
/// can spell (an exponent, a negative): a count the server did not report is
/// not a count of none. Nothing here is computed from anything else.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Timings {
    /// Prompt tokens the server prefilled for this request.
    pub prompt_n: Option<u64>,
    /// Prompt tokens it reused from the slot's cache rather than prefilling.
    pub cache_n: Option<u64>,
    /// How long the prefill took.
    pub prompt_ms: Option<Millis>,
    /// Tokens it generated.
    pub predicted_n: Option<u64>,
    /// How long generating them took.
    pub predicted_ms: Option<Millis>,
    /// Tokens a speculative decoder drafted, when the server decodes
    /// speculatively: the in-band evidence of that regime (#117 R3 Q3,
    /// ruled). A missing key says nothing about speculation either way.
    pub draft_n: Option<u64>,
    /// How many of the drafted tokens were accepted.
    pub draft_n_accepted: Option<u64>,
}

impl Timings {
    /// The timings in a `timings` object, each read where the server put it.
    pub(crate) fn read(object: &Value) -> Self {
        let count = |key: &str| {
            object
                .get(key)
                .and_then(Value::as_u64)
                .filter(|count| *count <= Count::MAX)
        };
        let millis = |key: &str| {
            object
                .get(key)
                .and_then(|value| value.as_number().map(ToString::to_string))
                .and_then(|text| Millis::new(&text))
        };
        Self {
            prompt_n: count("prompt_n"),
            cache_n: count("cache_n"),
            prompt_ms: millis("prompt_ms"),
            predicted_n: count("predicted_n"),
            predicted_ms: millis("predicted_ms"),
            draft_n: count("draft_n"),
            draft_n_accepted: count("draft_n_accepted"),
        }
    }
}

/// A duration in milliseconds, as the digits the server wrote: a
/// non-negative integer or exact decimal the record can spell. Never through
/// a float, so `1.10` stays `1.10`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Millis(String);

impl Millis {
    /// The number `text`, or nothing when the record could not spell it.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        let integer = text
            .parse::<u64>()
            .is_ok_and(|number| number <= Count::MAX && number.to_string() == text);
        let decimal = !text.starts_with('-') && Decimal::new(text).is_some();
        (integer || decimal).then(|| Self(text.to_owned()))
    }

    /// The digits, as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A kind of refusal, read from the server's own typed field -- never from
/// its message, which is prose (#117 R3, D5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rejection {
    /// The prompt is longer than the server's context, refused before any
    /// prefill: llama.cpp's `error.type` `exceed_context_size_error`, as
    /// R3.0's C3 captured it.
    ContextOverflow,
}

impl Rejection {
    /// The typed `error.type` a llama.cpp server refuses with, for each kind.
    const TYPES: &'static [(&'static str, Self)] =
        &[("exceed_context_size_error", Self::ContextOverflow)];

    /// The kind of refusal `body` is, by its `error.type`, if it is one this
    /// client names. A body that is not JSON, or whose type is not named, is
    /// no kind -- whatever its message says.
    #[must_use]
    pub fn of(body: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(body).ok()?;
        let typed = value.pointer("/error/type").and_then(Value::as_str)?;
        Self::TYPES
            .iter()
            .find(|(name, _)| *name == typed)
            .map(|(_, kind)| *kind)
    }
}

/// The server's count of a call's prompt, prefilled so far: llama.cpp's
/// `prompt_progress` object, streamed when `return_progress` is asked for
/// (R3.0's C1). Keys as the server names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Prompt tokens in all.
    pub total: u64,
    /// Prompt tokens reused from the slot's cache.
    pub cache: u64,
    /// Prompt tokens processed so far.
    pub processed: u64,
    /// Milliseconds of prefill so far, by the server's clock.
    pub time_ms: u64,
}

impl Progress {
    /// A progress frame, or nothing when any of its four counts is missing
    /// or not a count: a partial frame is not a measurement.
    fn read(object: &Value) -> Option<Self> {
        let count = |key: &str| object.get(key).and_then(Value::as_u64);
        Some(Self {
            total: count("total")?,
            cache: count("cache")?,
            processed: count("processed")?,
            time_ms: count("time_ms")?,
        })
    }
}

/// One piece of a streamed answer: answer text, or the reasoning a thinking
/// model streams before it. Kept apart from the first byte, because the two
/// go back to the server differently (`reasoning_content` beside `content`,
/// #117 Q10) and are shown differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Piece<'a> {
    /// Answer text: `delta.content`.
    Text(&'a str),
    /// Reasoning: `delta.reasoning_content`.
    Reasoning(&'a str),
    /// The server's prefill progress: not answer text, and never sent back.
    Progress(Progress),
    /// One fragment of a call the model is making: `delta.tool_calls[i]`,
    /// exactly as the server sent it (#298 T11). Never answer text: the
    /// arguments are the call's, and they go back as the call.
    ToolCall {
        /// Which call of the response it belongs to.
        index: u64,
        /// The call's id, where the server sent one (its first fragment).
        id: Option<&'a str>,
        /// The function's name, where the server sent one.
        name: Option<&'a str>,
        /// This fragment of the arguments text; empty where it sent none.
        arguments: &'a str,
    },
}

/// A transport that delivers an answer as it arrives.
pub trait Streaming: Send + Sync {
    /// Send `shape`, calling `on_delta` with each piece of reasoning or answer
    /// text as it arrives, until the server finishes, `cancel` is asked, or `deadline`
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
        on_delta: &mut dyn FnMut(Piece<'_>),
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
    bearer: Option<Bearer>,
    trust: super::tls::Trust,
}

/// A credential sent as `Authorization: Bearer <key>` -- what the DoD-1
/// endpoint requires (#117, the I5 manual check). Never printed: its
/// `Debug` says only that there is one, so a transport logged for the
/// record does not carry the key into it.
#[derive(Clone, PartialEq, Eq)]
pub struct Bearer(String);

impl Bearer {
    /// A bearer credential, or nothing when `key` is empty or holds a
    /// character that would end the header line and start another.
    #[must_use]
    pub fn new(key: &str) -> Option<Self> {
        let usable = !key.is_empty() && !key.chars().any(char::is_control);
        usable.then(|| Self(key.to_owned()))
    }
}

impl fmt::Debug for Bearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Bearer(<redacted>)")
    }
}

impl HttpStream {
    /// A streaming transport pointed at `endpoint`, reading at most
    /// [`transport::MAX_REPLY_BYTES`].
    #[must_use]
    pub fn new(endpoint: Endpoint) -> Self {
        Self {
            endpoint,
            reply_cap: transport::MAX_REPLY_BYTES,
            bearer: None,
            trust: super::tls::Trust::default(),
        }
    }

    /// The same, trusting `trust`'s roots for an `https` endpoint.
    #[must_use]
    pub fn with_trust(mut self, trust: super::tls::Trust) -> Self {
        self.trust = trust;
        self
    }

    /// The same, sending `bearer` with every request.
    #[must_use]
    pub fn with_bearer(mut self, bearer: Bearer) -> Self {
        self.bearer = Some(bearer);
        self
    }

    /// The `Authorization` header line every request carries, or nothing
    /// when there is no bearer.
    fn authorization(&self) -> String {
        self.bearer
            .as_ref()
            .map_or_else(String::new, |Bearer(key)| {
                format!("Authorization: Bearer {key}\r\n")
            })
    }

    /// The server's `GET /props`, at the root of the endpoint's host and
    /// port whatever the endpoint's path, with the bearer when there is one:
    /// where llama.cpp reports its `build_info` (the engine check, #157).
    ///
    /// # Errors
    ///
    /// Returns [`TransportFailure`] when no reply arrived. A status other
    /// than `200` is a reply, and is the caller's to judge.
    pub fn props(&self, deadline: Instant) -> Result<transport::HttpReply, TransportFailure> {
        self.get("/props", deadline)
    }

    /// `GET path` at the root of the endpoint's host and port, with the
    /// bearer when there is one: where an engine reports itself (`/props`,
    /// `TabbyAPI`'s `/v1/model`, #509).
    ///
    /// # Errors
    ///
    /// As [`Self::props`].
    pub fn get(
        &self,
        path: &str,
        deadline: Instant,
    ) -> Result<transport::HttpReply, TransportFailure> {
        let authorization = self.authorization();
        let request = format!(
            "GET {path} HTTP/1.1\r\nHost: {}:{}\r\nAccept: application/json\r\n\
             {authorization}Connection: close\r\n\r\n",
            self.endpoint.host, self.endpoint.port
        );
        transport::exchange(
            (&self.endpoint, &self.trust),
            &request,
            self.reply_cap,
            deadline,
        )
    }

    /// The same, with a smaller cap -- the unstreamed transport's reason,
    /// and its name: a 64 MiB guard is one no test can afford to fire, and a
    /// guard nobody has seen fire is not a guard. This is how it is seen.
    #[must_use]
    pub fn with_reply_cap(endpoint: Endpoint, reply_cap: usize) -> Self {
        Self {
            endpoint,
            reply_cap,
            bearer: None,
            trust: super::tls::Trust::default(),
        }
    }
}

impl Streaming for HttpStream {
    fn stream(
        &self,
        shape: &RequestShape,
        deadline: Instant,
        cancel: &Cancel,
        on_delta: &mut dyn FnMut(Piece<'_>),
    ) -> Result<Ended, TransportFailure> {
        let started = Instant::now();
        let timeout = || TransportFailure::Timeout {
            after: started.elapsed(),
        };
        let remaining = || left_before(deadline, Instant::now());

        // A stop already asked for is honoured before anything is opened.
        // One asked for DURING the connect is not: `std` offers no way to
        // interrupt a `connect_timeout` from another thread, so it lands
        // when the connect returns -- immediate on loopback, up to the
        // budget against a host that drops SYNs. Disclosed on #120.
        if cancel.is_asked() {
            return Ok(Ended::Cancelled);
        }
        // Over TLS for an `https` endpoint (#555): the handshake is part
        // of the connect, and the stopper below shuts the socket under it.
        let mut socket = super::tls::connect(&self.endpoint, &self.trust, deadline, started)?;

        // The stopper: a second handle on the same socket, shut from
        // whichever thread asks. Shutting it is what makes a read blocked
        // below return, and what the server sees as the client leaving.
        //
        // The same handle is shut when this call returns, WHATEVER it
        // returns with -- a cap passed, a framing failure, an `error` event.
        // Otherwise the stopper's handle kept the connection open for as
        // long as the `Cancel` lived, and against llama-server an open
        // connection is a slot still generating (measured; see above).
        // Taken out of its slot either way, so the socket closes once and a
        // reused `Cancel` holds no open connection per call.
        let handle = Arc::new(Mutex::new(Some(socket.socket().try_clone().map_err(
            |why| TransportFailure::Connect(format!("no second handle to cancel through: {why}")),
        )?)));
        let waker = Arc::clone(&handle);
        cancel.on_cancel(move || close(&waker));
        let _closing = Closing(handle);

        let body = wire::streaming_body(shape);
        let authorization = self.authorization();
        let request = format!(
            "POST {} HTTP/1.1\r\nHost: {}:{}\r\nContent-Type: application/json\r\n\
             Accept: text/event-stream\r\n{authorization}Content-Length: {}\r\n\
             Connection: close\r\n\r\n{}",
            self.endpoint.path,
            self.endpoint.host,
            self.endpoint.port,
            body.len(),
            body
        );
        let budget = remaining().ok_or_else(timeout)?;
        socket
            .socket()
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
            // NO flag check here, before the read. A check here only races
            // the stopper -- it wins when a stop lands between two reads and
            // loses when the read is already blocked -- so a test cancelling
            // right after a piece passed whether or not the stopper worked
            // (#120's review, and the fault that left the socket open). The
            // stopper is what ends a blocked read; the check below, after a
            // read returns, is what stops pieces that were already buffered
            // from being delivered after the stop.
            //
            // Re-armed on every pass, as the unstreamed transport does: one
            // timeout bounds one read, not the call.
            let budget = remaining().ok_or_else(timeout)?;
            if let Err(why) = socket.socket().set_read_timeout(Some(budget)) {
                // A stop that landed since the last read has shut the socket,
                // and on macOS an option set on a shut socket fails (`EINVAL`).
                // That is the stop landing too.
                return if cancel.is_asked() {
                    Ok(Ended::Cancelled)
                } else {
                    Err(TransportFailure::Read(why.to_string()))
                };
            }
            let count = match socket.read(&mut buffer) {
                Ok(count) => count,
                Err(why) if why.kind() == io::ErrorKind::Interrupted => continue,
                // A timeout is a timeout, even after a stop. A stop that WORKED
                // ends the read by shutting the socket, not by the clock; a
                // read that ran to its deadline after a stop is the stopper
                // failing, and calling it `Cancelled` hid exactly that (#120's
                // second review: the session test passed with no stopper).
                Err(why) if is_timeout(&why) => return Err(timeout()),
                // A read that fails because the socket was shut under it is
                // the stop landing, not the transport failing.
                Err(_) if cancel.is_asked() => return Ok(Ended::Cancelled),
                Err(why) => return Err(TransportFailure::Read(why.to_string())),
            };
            if cancel.is_asked() {
                return Ok(Ended::Cancelled);
            }
            if count == 0 {
                return reading.closed(on_delta);
            }
            if let Some(ended) = reading.feed(&buffer[..count], self.reply_cap, on_delta)? {
                return Ok(ended);
            }
        }
    }

    fn describes(&self) -> String {
        format!(
            "{}://{}:{}{} (streamed)",
            self.endpoint.scheme(),
            self.endpoint.host,
            self.endpoint.port,
            self.endpoint.path
        )
    }
}

/// Shut the connection behind `handle` and release the handle, once.
fn close(handle: &Mutex<Option<TcpStream>>) {
    let taken = handle.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(socket) = taken {
        let _ = socket.shutdown(Shutdown::Both);
    }
}

/// Closes a streamed call's connection when the call returns.
struct Closing(Arc<Mutex<Option<TcpStream>>>);

impl Drop for Closing {
    fn drop(&mut self) {
        close(&self.0);
    }
}

/// How long is left before `deadline`, or `None` when it has arrived.
///
/// `Duration::ZERO` counts as arrived: a zero timeout is `InvalidInput` to a
/// socket, so the exact instant of the deadline would otherwise surface as a
/// `Read` or `Connect` failure for what is really time running out.
fn left_before(deadline: Instant, now: Instant) -> Option<Duration> {
    deadline
        .checked_duration_since(now)
        .filter(|left| !left.is_zero())
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
    /// The last `timings` object any chunk carried.
    timings: Option<Timings>,
}

impl Reading {
    fn feed(
        &mut self,
        bytes: &[u8],
        cap: usize,
        on_delta: &mut dyn FnMut(Piece<'_>),
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
        on_delta: &mut dyn FnMut(Piece<'_>),
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
            return self.closed(on_delta).map(Some);
        }
        Ok(None)
    }

    /// The body is over, by its framing or by the connection closing: read
    /// whatever event a held line ending was keeping back, then settle.
    fn closed(&mut self, on_delta: &mut dyn FnMut(Piece<'_>)) -> Result<Ended, TransportFailure> {
        if matches!(self.head, Some((200, _))) {
            for data in self.events.finish()? {
                if let Some(ended) = self.event(&data, on_delta)? {
                    return Ok(ended);
                }
            }
        }
        self.at_close()
    }

    fn refused(&self, status: u16) -> Ended {
        let body = String::from_utf8_lossy(&self.refusal).into_owned();
        Ended::Rejected {
            status,
            class: Rejection::of(&body),
            body,
        }
    }

    /// One event's data. `Some` when it ends the stream.
    fn event(
        &mut self,
        data: &str,
        on_delta: &mut dyn FnMut(Piece<'_>),
    ) -> Result<Option<Ended>, TransportFailure> {
        if data == "[DONE]" {
            return Ok(Some(Ended::Finished {
                finish_reason: self.finish_reason.take(),
                timings: self.timings.take(),
            }));
        }
        let value: Value = serde_json::from_str(data).map_err(|why| {
            TransportFailure::Framing(format!("an event's data is not JSON ({why}): {data}"))
        })?;
        if value.get("error").is_some() {
            return Ok(Some(Ended::Rejected {
                status: 200,
                body: data.to_owned(),
                class: Rejection::of(data),
            }));
        }
        // Before the frame's delta: it counts the prefill the delta follows.
        if let Some(progress) = value.get("prompt_progress").and_then(Progress::read) {
            on_delta(Piece::Progress(progress));
        }
        // On whichever chunk carries it, the last one winning: the usage
        // chunk (`choices: []`) when usage was asked for, the last chunk with
        // choices when it was not (`wire.rs`, `streaming_body`).
        if let Some(timings) = value.get("timings").filter(|timings| timings.is_object()) {
            self.timings = Some(Timings::read(timings));
        }
        if let Some(choice) = value
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
        {
            // Measured on e7051ef (#117, Q10): with no `--reasoning-format`,
            // a thinking model streams `reasoning_content` deltas first and
            // `content` deltas after, one field per event. Both are read, and
            // each is delivered as what it is.
            if let Some(piece) = choice
                .pointer("/delta/reasoning_content")
                .and_then(Value::as_str)
                && !piece.is_empty()
            {
                on_delta(Piece::Reasoning(piece));
            }
            if let Some(piece) = choice.pointer("/delta/content").and_then(Value::as_str)
                && !piece.is_empty()
            {
                on_delta(Piece::Text(piece));
            }
            if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
                self.finish_reason = Some(reason.to_owned());
            }
        }
        // A call's fragments (#298 T11), measured on I0's capture: the
        // first carries `index`, `id`, `type` and `function.name` with the
        // first piece of `function.arguments`; each later one `index` and
        // a piece of the arguments only. Delivered one by one, as sent.
        if let Some(calls) = value
            .pointer("/choices/0/delta/tool_calls")
            .and_then(Value::as_array)
        {
            for call in calls {
                let Some(index) = call.get("index").and_then(Value::as_u64) else {
                    return Err(TransportFailure::Framing(format!(
                        "a streamed tool call carries no `index`: {data}"
                    )));
                };
                on_delta(Piece::ToolCall {
                    index,
                    id: call.get("id").and_then(Value::as_str),
                    name: call.pointer("/function/name").and_then(Value::as_str),
                    arguments: call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                });
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
                timings: self.timings.take(),
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
///
/// A line may end in `\r\n`, `\n` or a bare `\r` (the SSE rule), and one
/// event may mix them. Every ending is normalised to `\n` before events are
/// cut -- except a `\r` that is the LAST byte so far, which stays undecided
/// until the next byte says whether it was half of a `\r\n`. Deciding early
/// would read a `\r\n` split across two reads as two line endings, and
/// that is a blank line: an event cut where there is none.
///
/// Linear in what arrives: only the new bytes (and a held `\r`) are
/// normalised, and the search for a blank line starts where the last one
/// stopped. Re-normalising and re-searching the whole pending buffer on every
/// read was quadratic in one long event, which a server may grow to the reply
/// cap (#120's second review measured 20 s for one 4 MiB event).
#[derive(Debug, Default)]
struct Events {
    pending: Vec<u8>,
}

impl Events {
    fn feed(&mut self, bytes: &[u8]) -> Result<Vec<String>, TransportFailure> {
        let mut incoming = Vec::with_capacity(bytes.len() + 1);
        if self.pending.last() == Some(&b'\r') {
            self.pending.pop();
            incoming.push(b'\r');
        }
        incoming.extend_from_slice(bytes);
        // A blank line can straddle the old end: its first `\n` already
        // pending, its second just arrived.
        let from = self.pending.len().saturating_sub(1);
        self.pending.extend(normalised(&incoming));
        self.cut(from)
    }

    /// The body is over: a `\r` held for a `\n` that never came was a line
    /// ending after all.
    fn finish(&mut self) -> Result<Vec<String>, TransportFailure> {
        let from = self.pending.len().saturating_sub(2);
        if let Some(last) = self.pending.last_mut()
            && *last == b'\r'
        {
            *last = b'\n';
        }
        self.cut(from)
    }

    /// Every whole event from `from` on. Everything before `from` has been
    /// searched and holds no blank line.
    fn cut(&mut self, mut from: usize) -> Result<Vec<String>, TransportFailure> {
        let mut out = Vec::new();
        loop {
            let Some(at) = find(&self.pending[from..], b"\n\n").map(|at| at + from) else {
                break;
            };
            // What is left after this event was all past `from`: unsearched.
            from = 0;
            let event: Vec<u8> = self.pending.drain(..at + 2).collect();
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

/// Every line ending in `bytes` as `\n`, except a trailing `\r`, which is
/// kept as it is: it may be the first half of a `\r\n` not yet arrived.
fn normalised(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match (bytes[at], bytes.get(at + 1)) {
            (b'\r', None) => out.push(b'\r'),
            (b'\r', Some(b'\n')) => {}
            (b'\r', Some(_)) => out.push(b'\n'),
            (byte, _) => out.push(byte),
        }
        at += 1;
    }
    out
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
    /// A piece of reasoning.
    Reasoning(String),
    /// Wait here until the gate opens or the call is cancelled.
    Hold(Gate),
    /// Fail as a transport would.
    Fail(TransportFailure),
    /// Answer with a refusal rather than a stream.
    Reject(u16, String),
    /// Report these timings when the call finishes.
    Timings(Timings),
    /// A prefill progress frame.
    Progress(Progress),
    /// Finish with this `finish_reason` rather than `stop` -- `length` is a
    /// call its output cap ended (#290).
    FinishReason(String),
    /// One fragment of a streamed tool call, delivered as
    /// [`Piece::ToolCall`].
    ToolCall {
        /// Which call of the response it belongs to.
        index: u64,
        /// The call's id, on the fragment that carries it.
        id: Option<String>,
        /// The function's name, on the fragment that carries it.
        name: Option<String>,
        /// This fragment of the arguments.
        arguments: String,
    },
}

impl Step {
    /// A whole call in one fragment: what a test means by "the model called
    /// `name` with `arguments`".
    #[must_use]
    pub fn call(index: u64, id: &str, name: &str, arguments: &str) -> Self {
        Self::ToolCall {
            index,
            id: Some(id.to_owned()),
            name: Some(name.to_owned()),
            arguments: arguments.to_owned(),
        }
    }
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
        on_delta: &mut dyn FnMut(Piece<'_>),
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
        let mut timings = None;
        let mut finish_reason = "stop".to_owned();
        for step in steps {
            if cancel.is_asked() {
                return Ok(Ended::Cancelled);
            }
            match step {
                Step::Delta(text) => on_delta(Piece::Text(&text)),
                Step::Reasoning(text) => on_delta(Piece::Reasoning(&text)),
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
                Step::Reject(status, body) => {
                    return Ok(Ended::Rejected {
                        status,
                        class: Rejection::of(&body),
                        body,
                    });
                }
                Step::Progress(progress) => on_delta(Piece::Progress(progress)),
                Step::Timings(measured) => timings = Some(measured),
                Step::FinishReason(reason) => finish_reason = reason,
                Step::ToolCall {
                    index,
                    id,
                    name,
                    arguments,
                } => on_delta(Piece::ToolCall {
                    index,
                    id: id.as_deref(),
                    name: name.as_deref(),
                    arguments: &arguments,
                }),
            }
        }
        if cancel.is_asked() {
            return Ok(Ended::Cancelled);
        }
        Ok(Ended::Finished {
            finish_reason: Some(finish_reason),
            timings,
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
                context_window: None,
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
                pieces.push_str(&text(piece));
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

    #[test]
    fn a_canned_stream_plays_reasoning_as_reasoning_and_text_as_text() {
        let canned = Canned::new([vec![
            Step::Reasoning("thinking\n".to_owned()),
            Step::Delta("answer".to_owned()),
        ]]);
        let mut pieces: Vec<(bool, String)> = Vec::new();
        let ended = canned.stream(&shape(), deadline(), &Cancel::new(), &mut |piece| {
            pieces.push(match piece {
                Piece::Reasoning(reasoning) => (true, reasoning.to_owned()),
                Piece::Text(text) => (false, text.to_owned()),
                Piece::Progress(progress) => panic!("progress where none was sent: {progress:?}"),
                Piece::ToolCall { arguments, .. } => {
                    panic!("a call where none was sent: {arguments:?}")
                }
            });
        });
        assert!(ended.is_ok(), "{ended:?}");
        assert_eq!(
            pieces,
            [
                (true, "thinking\n".to_owned()),
                (false, "answer".to_owned())
            ]
        );
    }

    #[test]
    fn a_stub_with_props_answers_them_before_and_after_its_acts() {
        // The canned server's engine check, and a check made again later:
        // `/props` never spends an act (#219 item 11).
        let stub = Stub::serving_with_props(vec![Act::Raw(CAPTURED.to_vec())], "canned-x")
            .expect("loopback");
        let transport = HttpStream::new(endpoint(&stub));
        let props = || transport.props(deadline()).expect("answered").body;
        assert_eq!(props(), "{\"build_info\":\"canned-x\"}");
        let ended = transport.stream(&shape(), deadline(), &Cancel::new(), &mut |_| {});
        assert!(ended.is_ok(), "the act is still there: {ended:?}");
        assert_eq!(props(), "{\"build_info\":\"canned-x\"}");
        // A call beyond the script is a defect, made to look like one: a
        // 503, recorded -- never a silent close.
        let extra = transport.stream(&shape(), deadline(), &Cancel::new(), &mut |_| {});
        assert!(
            matches!(extra, Ok(Ended::Rejected { status: 503, .. })),
            "{extra:?}"
        );
        assert!(
            stub.heads()
                .last()
                .is_some_and(|head| head.starts_with("<beyond the script: POST ")),
            "{:?}",
            stub.heads()
        );
    }

    #[test]
    fn props_is_asked_at_the_servers_root_with_the_bearer() {
        let stub = Stub::serving(vec![Act::Answer(
            "{\"build_info\":\"b1-4ceb171\"}".to_owned(),
        )])
        .expect("loopback");
        let bearer = Bearer::new("k3y-for-the-endpoint").expect("a usable key");
        let reply = HttpStream::new(endpoint(&stub))
            .with_bearer(bearer)
            .props(deadline())
            .expect("a reply");
        assert_eq!(
            (reply.status, reply.body.as_str()),
            (200, "{\"build_info\":\"b1-4ceb171\"}")
        );
        let heads = stub.heads();
        assert!(
            heads[0].starts_with("GET /props HTTP/1.1\r\n")
                && heads[0].contains("\r\nAuthorization: Bearer k3y-for-the-endpoint\r\n"),
            "at the root, not under the endpoint's /v1/chat/completions: {heads:?}"
        );
    }

    #[test]
    fn a_bearer_credential_is_sent_as_an_authorization_header_and_only_then() {
        let with = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
        let bearer = Bearer::new("k3y-for-the-endpoint").expect("a usable key");
        let ended = HttpStream::new(endpoint(&with)).with_bearer(bearer).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(ended.is_ok(), "{ended:?}");
        let heads = with.heads();
        assert!(
            heads[0].contains("\r\nAuthorization: Bearer k3y-for-the-endpoint\r\n"),
            "{heads:?}"
        );

        let without = Stub::serving(vec![Act::Raw(CAPTURED.to_vec())]).expect("loopback");
        let ended = HttpStream::new(endpoint(&without)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |_| {},
        );
        assert!(ended.is_ok(), "{ended:?}");
        assert!(
            !without.heads()[0].contains("Authorization"),
            "a transport with no key sent a credential"
        );
    }

    #[test]
    fn a_bearer_credential_is_never_printed_and_never_breaks_a_line() {
        let transport = HttpStream::new(
            Endpoint::parse("http://127.0.0.1:1/v1/chat/completions").expect("an endpoint"),
        )
        .with_bearer(Bearer::new("k3y-for-the-endpoint").expect("a usable key"));
        let printed = format!("{transport:?}");
        assert!(!printed.contains("k3y"), "the key was printed: {printed}");
        assert!(printed.contains("<redacted>"), "{printed}");
        for unusable in ["", "k3y\r\nX-Injected: yes", "k3y\n"] {
            assert!(Bearer::new(unusable).is_none(), "{unusable:?} was accepted");
        }
    }

    /// A piece's text, where the stream under test carries answer text only.
    fn text(piece: Piece<'_>) -> String {
        match piece {
            Piece::Text(text) => text.to_owned(),
            Piece::Reasoning(reasoning) => {
                panic!("reasoning where only text was sent: {reasoning:?}")
            }
            Piece::Progress(progress) => panic!("progress where only text was sent: {progress:?}"),
            Piece::ToolCall { arguments, .. } => {
                panic!("a tool call where only text was sent: {arguments:?}")
            }
        }
    }

    /// A thinking model's reply, captured off a raw socket from the drive
    /// endpoint (llama-server `e7051ef`, 2026-09-28, #117 Q10 by track four):
    /// 295 `reasoning_content` deltas, then 14 `content` deltas, then usage.
    const REASONING_CAPTURE: &[u8] =
        include_bytes!("../../client/fixtures/llama-server-e7051ef-reasoning-stream.http");

    #[test]
    fn a_thinking_models_reply_streams_its_reasoning_apart_from_its_answer() {
        // The capture is the one track four measured, not one edited since.
        assert_eq!(
            crate::digest::sha256_hex(REASONING_CAPTURE),
            "b91695d816f39ec5a22fd29d1879ccdcca32fd4b028f14bd4fb854f77a91cd31"
        );
        let stub = Stub::serving(vec![Act::Raw(REASONING_CAPTURE.to_vec())]).expect("loopback");
        let mut pieces: Vec<(bool, String)> = Vec::new();
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |piece| {
                pieces.push(match piece {
                    Piece::Reasoning(reasoning) => (true, reasoning.to_owned()),
                    Piece::Text(text) => (false, text.to_owned()),
                    Piece::Progress(progress) => {
                        panic!("progress where none was sent: {progress:?}")
                    }
                    Piece::ToolCall { arguments, .. } => {
                        panic!("a call where none was sent: {arguments:?}")
                    }
                });
            },
        );
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("stop".to_owned()),
                timings: Some(cold_timings()),
            })
        );
        let reasoning: Vec<&str> = pieces
            .iter()
            .filter(|(is_reasoning, _)| *is_reasoning)
            .map(|(_, piece)| piece.as_str())
            .collect();
        let answer: String = pieces
            .iter()
            .filter(|(is_reasoning, _)| !*is_reasoning)
            .map(|(_, piece)| piece.as_str())
            .collect();
        assert_eq!(reasoning.len(), 295);
        assert_eq!(pieces.len(), 295 + 14);
        assert!(
            pieces[..295].iter().all(|(is_reasoning, _)| *is_reasoning),
            "an answer piece arrived before the reasoning was done"
        );
        assert_eq!(answer, "225. The journey is 225 minutes long.");
        assert!(
            reasoning.concat().ends_with('\n'),
            "the reasoning's own trailing newline is part of what goes back"
        );
    }

    /// How a stream of these events' data ends, read by the transport's own
    /// reader: a 200, the events, then the connection closing.
    fn ended_by(events: &[&str]) -> Result<Ended, TransportFailure> {
        let mut raw = String::from("HTTP/1.1 200 OK\r\n\r\n");
        for event in events {
            raw.push_str("data: ");
            raw.push_str(event);
            raw.push_str("\n\n");
        }
        let mut reading = Reading::default();
        if let Some(ended) = reading.feed(raw.as_bytes(), usize::MAX, &mut |_| {})? {
            return Ok(ended);
        }
        reading.closed(&mut |_| {})
    }

    /// The timings a stream ending in `[DONE]` reported, with `timings`
    /// spliced into its usage chunk as written.
    fn reported(timings: &str) -> Option<Timings> {
        let ended = ended_by(&[
            r#"{"choices":[{"index":0,"delta":{"content":"x"},"finish_reason":"stop"}]}"#,
            &format!(r#"{{"choices":[],"usage":{{"prompt_tokens":3}},"timings":{timings}}}"#),
            "[DONE]",
        ]);
        match ended {
            Ok(Ended::Finished { timings, .. }) => timings,
            other => panic!("not a finished call: {other:?}"),
        }
    }

    #[test]
    fn timings_on_the_last_chunk_with_choices_are_read_too() {
        // Without `include_usage` there is no `choices: []` chunk, and the
        // server puts its timings on the last chunk that still has choices.
        let ended = ended_by(&[
            r#"{"choices":[{"index":0,"delta":{"content":"x"},"finish_reason":null}]}"#,
            r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"timings":{"prompt_n":4,"cache_n":2}}"#,
            "[DONE]",
        ]);
        let Ok(Ended::Finished {
            timings: Some(timings),
            ..
        }) = ended
        else {
            panic!("no timings: {ended:?}");
        };
        assert_eq!((timings.prompt_n, timings.cache_n), (Some(4), Some(2)));
    }

    #[test]
    fn the_last_timings_a_stream_carries_is_the_one_reported() {
        let ended = ended_by(&[
            r#"{"choices":[{"index":0,"delta":{"content":"x"},"finish_reason":"stop"}],"timings":{"prompt_n":1}}"#,
            r#"{"choices":[],"timings":{"prompt_n":2}}"#,
            "[DONE]",
        ]);
        let Ok(Ended::Finished {
            timings: Some(timings),
            ..
        }) = ended
        else {
            panic!("no timings: {ended:?}");
        };
        assert_eq!(timings.prompt_n, Some(2), "the last one wins");
    }

    #[test]
    fn a_timings_that_is_not_an_object_reports_nothing() {
        // Not an object with every field absent: nothing was reported.
        assert_eq!(reported("null"), None);
        // And it does not erase an object that came before it.
        let ended = ended_by(&[
            r#"{"choices":[{"index":0,"delta":{"content":"x"},"finish_reason":"stop"}],"timings":{"prompt_n":1}}"#,
            r#"{"choices":[],"timings":null}"#,
            "[DONE]",
        ]);
        let Ok(Ended::Finished {
            timings: Some(timings),
            ..
        }) = ended
        else {
            panic!("the earlier timings were lost: {ended:?}");
        };
        assert_eq!(timings.prompt_n, Some(1));
    }

    #[test]
    fn a_timing_the_server_did_not_send_is_absent_not_zero() {
        assert_eq!(
            reported(r#"{"prompt_n":5}"#),
            Some(Timings {
                prompt_n: Some(5),
                ..Timings::default()
            }),
            "only what was sent"
        );
        // And a stream with no timings object reports none at all.
        assert_eq!(
            ended_by(&[
                r#"{"choices":[{"index":0,"delta":{"content":"x"},"finish_reason":"stop"}]}"#,
                "[DONE]",
            ]),
            Ok(Ended::Finished {
                finish_reason: Some("stop".to_owned()),
                timings: None,
            })
        );
    }

    #[test]
    fn a_timings_digits_survive_the_transport() {
        // Seeded, because the captures' own digits survive a round trip
        // through a float by accident (R3 proposal, section 5); `1.10` does
        // not. And an integer written as one stays one.
        let timings = reported(r#"{"prompt_ms":1.10,"predicted_ms":290}"#).expect("timings");
        assert_eq!(timings.prompt_ms.as_ref().map(Millis::as_str), Some("1.10"));
        assert_eq!(
            timings.predicted_ms.as_ref().map(Millis::as_str),
            Some("290")
        );
        // An integral one past the record's bound is not.
        assert_eq!(Millis::new("9223372036854775808"), None);
    }

    #[test]
    fn a_timing_the_record_cannot_spell_is_absent() {
        // An exponent is a second spelling (`number.pest`), a negative
        // duration or count is not a measurement, and a count past `i64::MAX`
        // is past what the record can hold (`Count::MAX`); each is absent
        // rather than rewritten into something the server did not say.
        let timings = reported(
            r#"{"prompt_ms":2.9e2,"predicted_ms":-1.5,"prompt_n":1e2,"cache_n":-3,"predicted_n":7,"draft_n":9223372036854775808}"#,
        )
        .expect("timings");
        assert_eq!(
            timings,
            Timings {
                predicted_n: Some(7),
                ..Timings::default()
            }
        );
    }

    #[test]
    fn a_timings_chunk_before_a_cancel_is_not_a_finished_call() {
        let piece =
            r#"data: {"choices":[{"index":0,"delta":{"content":"Hel"},"finish_reason":null}]}"#;
        let measured = r#"data: {"choices":[],"timings":{"prompt_n":3,"cache_n":0}}"#;
        let stub = Stub::serving(vec![Act::StreamThenHold(vec![
            format!("{piece}\n\n"),
            format!("{measured}\n\n"),
        ])])
        .expect("loopback");
        let transport = HttpStream::new(endpoint(&stub));
        let cancel = Cancel::new();
        let asker = cancel.clone();
        let (first, arrived) = std::sync::mpsc::channel();
        let (result, finished) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let ended = transport.stream(&shape(), deadline(), &cancel, &mut |piece| {
                let _ = first.send(text(piece));
            });
            let _ = result.send(ended);
        });
        assert_eq!(
            arrived.recv_timeout(Duration::from_secs(10)).as_deref(),
            Ok("Hel")
        );
        cancel_and_expect(&asker, &finished);
    }

    #[test]
    fn a_canned_stream_finishes_with_the_reason_it_is_given() {
        // `length` is how a server says a call's output cap ended it (#290).
        let canned = Canned::new([
            vec![
                Step::Delta("cut".to_owned()),
                Step::FinishReason("length".to_owned()),
            ],
            vec![Step::Delta("ok".to_owned())],
        ]);
        let reasons: Vec<Option<String>> = (0..2)
            .map(
                |_| match canned.stream(&shape(), deadline(), &Cancel::new(), &mut |_| {}) {
                    Ok(Ended::Finished { finish_reason, .. }) => finish_reason,
                    other => panic!("{other:?}"),
                },
            )
            .collect();
        assert_eq!(
            reasons,
            [Some("length".to_owned()), Some("stop".to_owned())]
        );
    }

    #[test]
    fn a_canned_stream_reports_its_timings_when_it_finishes() {
        let measured = Timings {
            prompt_n: Some(3),
            cache_n: Some(0),
            ..Timings::default()
        };
        let canned = Canned::new([vec![
            Step::Delta("ok".to_owned()),
            Step::Timings(measured.clone()),
        ]]);
        assert_eq!(
            canned.stream(&shape(), deadline(), &Cancel::new(), &mut |_| {}),
            Ok(Ended::Finished {
                finish_reason: Some("stop".to_owned()),
                timings: Some(measured),
            })
        );
    }

    /// What the reference build measured of the warm captured call
    /// (`4df29be`, line 38 of its capture).
    fn warm_timings() -> Timings {
        Timings {
            prompt_n: Some(1),
            cache_n: Some(28),
            prompt_ms: Millis::new("229.366"),
            predicted_n: Some(6),
            predicted_ms: Millis::new("105.445"),
            // `4df29be` does not decode speculatively, and sends no pair.
            draft_n: None,
            draft_n_accepted: None,
        }
    }

    /// What the `DoD` 1 instance measured of the cold thinking turn
    /// (`e7051ef`, the capture's last data chunk).
    fn cold_timings() -> Timings {
        Timings {
            prompt_n: Some(89),
            cache_n: Some(0),
            prompt_ms: Millis::new("297.198"),
            predicted_n: Some(312),
            predicted_ms: Millis::new("2591.561"),
            draft_n: Some(312),
            draft_n_accepted: Some(207),
        }
    }

    /// A cold prefill of 9,276 tokens with progress asked for, captured off
    /// a raw socket from the drive endpoint (`e7051ef`, 2026-09-29, #117
    /// R3.0's C1 by track four): 13 `prompt_progress` frames, then the
    /// answer.
    const PROGRESS_CAPTURE: &[u8] =
        include_bytes!("../../client/fixtures/llama-server-e7051ef-prompt-progress-stream.http");

    /// A prompt one past the context, refused before any prefill (R3.0's C3).
    const OVERFLOW_CAPTURE: &[u8] =
        include_bytes!("../../client/fixtures/llama-server-e7051ef-context-overflow.http");

    /// Every progress frame, and whether an answer piece came before any.
    fn progress_and_order(pieces: &[Piece<'_>]) -> (Vec<Progress>, bool) {
        let mut frames = Vec::new();
        let mut answer_seen = false;
        let mut frame_after_answer = false;
        for piece in pieces {
            match piece {
                Piece::Progress(progress) => {
                    frame_after_answer |= answer_seen;
                    frames.push(*progress);
                }
                Piece::Text(_) | Piece::Reasoning(_) | Piece::ToolCall { .. } => answer_seen = true,
            }
        }
        (frames, frame_after_answer)
    }

    fn the_captured_progress(frames: &[Progress]) {
        assert_eq!(frames.len(), 13, "{frames:?}");
        assert_eq!(
            frames.first(),
            Some(&Progress {
                total: 9276,
                cache: 0,
                processed: 0,
                time_ms: 0
            })
        );
        assert_eq!(
            frames.last(),
            Some(&Progress {
                total: 9276,
                cache: 0,
                processed: 9276,
                time_ms: 6086
            })
        );
        assert!(
            frames
                .windows(2)
                .all(|pair| pair[0].processed < pair[1].processed),
            "processed rises frame by frame: {frames:?}"
        );
    }

    #[test]
    fn the_servers_prefill_progress_arrives_frame_by_frame_before_its_answer() {
        assert_eq!(
            crate::digest::sha256_hex(PROGRESS_CAPTURE),
            "a966fb3a578cd8901043098a2e2cbf3c2ae0e714f37bf9aff6c22e635d512877"
        );
        let stub = Stub::serving(vec![Act::Raw(PROGRESS_CAPTURE.to_vec())]).expect("loopback");
        let mut owned: Vec<(u8, String, Option<Progress>)> = Vec::new();
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |piece| {
                owned.push(match piece {
                    Piece::Text(text) => (0, text.to_owned(), None),
                    Piece::Reasoning(text) => (1, text.to_owned(), None),
                    Piece::Progress(progress) => (2, String::new(), Some(progress)),
                    Piece::ToolCall { arguments, .. } => (3, arguments.to_owned(), None),
                });
            },
        );
        assert!(matches!(ended, Ok(Ended::Finished { .. })), "{ended:?}");
        let pieces: Vec<Piece<'_>> = owned
            .iter()
            .map(|(kind, text, progress)| match (kind, progress) {
                (_, Some(progress)) => Piece::Progress(*progress),
                (1, None) => Piece::Reasoning(text),
                _ => Piece::Text(text),
            })
            .collect();
        let (frames, frame_after_answer) = progress_and_order(&pieces);
        the_captured_progress(&frames);
        assert!(
            !frame_after_answer,
            "a progress frame came after the answer began"
        );
        assert!(
            owned.iter().any(|(kind, _, _)| *kind < 2),
            "the answer after the prefill arrived too"
        );

        // And one byte at a time: a frame split across reads is one frame.
        let mut reading = Reading::default();
        let mut frames = Vec::new();
        for byte in PROGRESS_CAPTURE {
            if reading
                .feed(std::slice::from_ref(byte), usize::MAX, &mut |piece| {
                    if let Piece::Progress(progress) = piece {
                        frames.push(progress);
                    }
                })
                .expect("the captured bytes are a well-formed stream")
                .is_some()
            {
                break;
            }
        }
        the_captured_progress(&frames);
    }

    #[test]
    fn a_progress_frame_missing_a_count_is_not_delivered() {
        // Each of the four left out in turn: a frame without any one of them
        // is not a measurement, and none of them is filled in as zero.
        for missing in ["total", "cache", "processed", "time_ms"] {
            let mut frame =
                serde_json::json!({"total": 10, "cache": 0, "processed": 5, "time_ms": 3});
            frame.as_object_mut().expect("an object").remove(missing);
            let chunk = format!(
                r#"{{"choices":[{{"index":0,"delta":{{"role":"assistant","content":null}}}}],"prompt_progress":{frame}}}"#
            );
            let pieces = pieces_of(&[
                &chunk,
                r#"{"choices":[{"index":0,"delta":{"content":"x"},"finish_reason":"stop"}]}"#,
                "[DONE]",
            ]);
            assert!(
                !pieces.iter().any(|piece| piece.starts_with("progress")),
                "a frame with no `{missing}` was delivered: {pieces:?}"
            );
        }
    }

    /// What a stream of these events' data delivers, in order, read by the
    /// transport's own reader.
    fn pieces_of(events: &[&str]) -> Vec<String> {
        let mut raw = String::from("HTTP/1.1 200 OK\r\n\r\n");
        for data in events {
            raw.push_str("data: ");
            raw.push_str(data);
            raw.push_str("\n\n");
        }
        let mut pieces = Vec::new();
        let mut reading = Reading::default();
        let ended = reading
            .feed(raw.as_bytes(), usize::MAX, &mut |piece| {
                pieces.push(match piece {
                    Piece::Progress(progress) => format!("progress {}", progress.processed),
                    Piece::Text(text) => format!("text {text}"),
                    Piece::Reasoning(text) => format!("reasoning {text}"),
                    Piece::ToolCall { arguments, .. } => format!("tool_call {arguments}"),
                });
            })
            .expect("a well-formed stream");
        assert!(matches!(ended, Some(Ended::Finished { .. })), "{ended:?}");
        pieces
    }

    #[test]
    fn a_chunks_progress_comes_before_that_chunks_text() {
        // C1's frames ride chunks with no text; this one carries both, and
        // the count is of the prefill the text follows.
        assert_eq!(
            pieces_of(&[
                r#"{"choices":[{"index":0,"delta":{"content":"x"}}],"prompt_progress":{"total":9,"cache":0,"processed":9,"time_ms":4}}"#,
                r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}"#,
                "[DONE]",
            ]),
            ["progress 9", "text x"]
        );
    }

    #[test]
    fn an_error_event_inside_a_stream_is_classified_from_its_typed_field() {
        // D5's second source: an `error` event in place of the answer. No
        // capture shows one; the body is C3's error object.
        let event = r#"data: {"error":{"code":400,"message":"request (262149 tokens) exceeds the available context size (262144 tokens), try increasing it","type":"exceed_context_size_error"}}"#;
        let stub =
            Stub::serving(vec![Act::Chunked(vec![format!("{event}\n\n")])]).expect("loopback");
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |_| panic!("a refusal delivered a piece"),
        );
        let Ok(Ended::Rejected { status, class, .. }) = ended else {
            panic!("not a refusal: {ended:?}");
        };
        assert_eq!((status, class), (200, Some(Rejection::ContextOverflow)));
    }

    #[test]
    fn a_canned_refusal_is_classified_as_a_served_one_is() {
        let canned = Canned::new([vec![Step::Reject(
            400,
            r#"{"error":{"type":"exceed_context_size_error"}}"#.to_owned(),
        )]]);
        let ended = canned.stream(&shape(), deadline(), &Cancel::new(), &mut |_| {});
        let Ok(Ended::Rejected { class, .. }) = ended else {
            panic!("not a refusal: {ended:?}");
        };
        assert_eq!(class, Some(Rejection::ContextOverflow));
    }

    #[test]
    fn a_context_overflow_is_classified_from_its_typed_field() {
        assert_eq!(
            crate::digest::sha256_hex(OVERFLOW_CAPTURE),
            "634e1ce484f54032dd354b2c5717983426ff16f9eee136db428215ed023b6bb4"
        );
        let stub = Stub::serving(vec![Act::Raw(OVERFLOW_CAPTURE.to_vec())]).expect("loopback");
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |_| panic!("a refusal delivered a piece"),
        );
        let Ok(Ended::Rejected { status, class, .. }) = ended else {
            panic!("not a refusal: {ended:?}");
        };
        assert_eq!((status, class), (400, Some(Rejection::ContextOverflow)));
    }

    #[test]
    fn a_refusal_whose_message_mentions_context_is_not_an_overflow() {
        // The message is prose (#140's ruling); only the typed field
        // classifies.
        assert_eq!(
            Rejection::of(
                r#"{"error":{"code":400,"message":"exceeds the available context size","type":"invalid_request_error"}}"#
            ),
            None
        );
        assert_eq!(Rejection::of("exceed_context_size_error"), None, "not JSON");
        assert_eq!(
            Rejection::of(r#"{"error":{"type":"exceed_context_size_error"}}"#),
            Some(Rejection::ContextOverflow)
        );
    }

    #[test]
    fn a_canned_stream_plays_its_progress_before_its_answer() {
        let frame = Progress {
            total: 10,
            cache: 4,
            processed: 10,
            time_ms: 3,
        };
        let canned = Canned::new([vec![Step::Progress(frame), Step::Delta("ok".to_owned())]]);
        let mut seen = Vec::new();
        let ended = canned.stream(&shape(), deadline(), &Cancel::new(), &mut |piece| {
            seen.push(match piece {
                Piece::Progress(progress) => format!("{progress:?}"),
                Piece::Text(text)
                | Piece::Reasoning(text)
                | Piece::ToolCall {
                    arguments: text, ..
                } => text.to_owned(),
            });
        });
        assert!(ended.is_ok(), "{ended:?}");
        assert_eq!(seen, [format!("{frame:?}"), "ok".to_owned()]);
    }

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
            pieces.push(text(piece));
        });
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("length".to_owned()),
                timings: Some(warm_timings()),
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

    /// I0's turn 1 (#29, 2026-10-02): a real llama-server streaming one
    /// `bash` call, captured off a raw socket. Read in place, from the
    /// measurement it belongs to, so the fixture has one source.
    const I0_CALL: &[u8] = include_bytes!(
        "../../../substrates/measurements/2026-10-02-i0-tool-call-captures/turn1.http"
    );

    /// One fragment as the stream delivered it.
    type Fragment = (u64, Option<String>, Option<String>, String);

    /// Every piece a transport delivers for `reply`: the call fragments, and
    /// any text, kept apart.
    fn fragments_of(reply: &[u8]) -> (Vec<Fragment>, Vec<String>, Result<Ended, TransportFailure>) {
        let stub = Stub::serving(vec![Act::Raw(reply.to_vec())]).expect("loopback");
        let transport = HttpStream::new(endpoint(&stub));
        let mut calls = Vec::new();
        let mut texts = Vec::new();
        let ended = transport.stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |piece| match piece {
                Piece::ToolCall {
                    index,
                    id,
                    name,
                    arguments,
                } => calls.push((
                    index,
                    id.map(str::to_owned),
                    name.map(str::to_owned),
                    arguments.to_owned(),
                )),
                Piece::Text(text) | Piece::Reasoning(text) => texts.push(text.to_owned()),
                Piece::Progress(_) => {}
            },
        );
        (calls, texts, ended)
    }

    /// T11 (#298): a real server's streamed call arrives as the fragments it
    /// sent, the first naming the call and every one a piece of its
    /// arguments, and none of it as answer text.
    #[test]
    fn a_real_servers_streamed_call_is_its_fragments_and_never_text() {
        let (calls, texts, ended) = fragments_of(I0_CALL);
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("tool_calls".to_owned()),
                timings: ended.as_ref().ok().and_then(|ended| match ended {
                    Ended::Finished { timings, .. } => timings.clone(),
                    _ => None,
                }),
            })
        );
        assert!(
            texts.is_empty(),
            "no answer text came from a call: {texts:?}"
        );
        let id = "7GJeYs3ux1SaqFVPB5ee2AExFLbsukd7";
        let expected: Vec<Fragment> = [
            "{",
            "\"command\":\"",
            "ls",
            " |",
            " wc",
            " -",
            "l",
            "\"",
            "}",
        ]
        .iter()
        .enumerate()
        .map(|(at, piece)| {
            let first = at == 0;
            (
                0,
                first.then(|| id.to_owned()),
                first.then(|| "bash".to_owned()),
                (*piece).to_owned(),
            )
        })
        .collect();
        assert_eq!(calls, expected, "each fragment as the server sent it");
        let assembled: String = calls
            .iter()
            .map(|(_, _, _, piece)| piece.as_str())
            .collect();
        assert_eq!(assembled, "{\"command\":\"ls | wc -l\"}");

        // And a reply with no call yields none.
        let (calls, texts, _) = fragments_of(CAPTURED);
        assert!(calls.is_empty(), "{calls:?}");
        assert!(!texts.is_empty());
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
                    pieces.push(text(piece));
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
                finish_reason: Some("length".to_owned()),
                timings: Some(warm_timings()),
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
                let _ = first.send(text(piece));
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

    #[test]
    fn a_stop_asked_while_a_piece_is_delivered_is_a_cancel_not_a_failure() {
        // The stop lands between two reads: asked from inside the delivery of
        // a piece, so the stopper shuts the socket before the next read's
        // timeout is set. On macOS setting an option on a shut socket fails
        // (`EINVAL`), and that failure was reported as `Read` -- a cancel that
        // worked, logged as a transport failure (found running R2c's tests).
        let piece =
            r#"data: {"choices":[{"index":0,"delta":{"content":"Hel"},"finish_reason":null}]}"#;
        let stub = Stub::serving(vec![Act::StreamThenHold(vec![format!("{piece}\n\n")])])
            .expect("loopback");
        let transport = HttpStream::new(endpoint(&stub));
        let cancel = Cancel::new();
        let asker = cancel.clone();
        let ended = transport.stream(&shape(), deadline(), &cancel, &mut |_piece| {
            asker.ask();
        });
        assert_eq!(ended, Ok(Ended::Cancelled));
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
            &mut |piece| seen.push(text(piece)),
        );
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("stop".to_owned()),
                timings: None,
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
    fn a_stream_that_fails_on_its_own_closes_its_connection_while_the_cancel_lives() {
        // The cap fires and the call returns -- but the `Cancel` it was given
        // is still alive, holding the stopper and its handle on the socket.
        // The server must still see the client leave: against llama-server
        // an open connection is a slot still generating.
        let piece =
            r#"data: {"choices":[{"index":0,"delta":{"content":"more"},"finish_reason":null}]}"#;
        let stub = Stub::serving(vec![Act::StreamThenHold(vec![format!("{piece}\n\n"); 8])])
            .expect("loopback");
        let cancel = Cancel::new();
        let ended = HttpStream::with_reply_cap(endpoint(&stub), 256).stream(
            &shape(),
            deadline(),
            &cancel,
            &mut |_| {},
        );
        assert!(matches!(ended, Err(TransportFailure::Read(_))), "{ended:?}");
        let seen = wait_for_a_hangup(&stub);
        let Held::HungUp(after) = seen else {
            panic!("the connection outlived the call: {seen:?}");
        };
        assert!(
            after < Duration::from_secs(5),
            "the client left after {after:?}"
        );
        drop(cancel);
    }

    #[test]
    fn a_stop_asked_before_the_call_opens_no_connection() {
        // Listening, and never accepting: a connection made would sit in this
        // listener's backlog, where `accept` finds it. A stopped call that
        // connected anyway would still come back `Cancelled` -- its stopper
        // shuts the socket before the request is written -- so the verdict
        // is read off the listener, not off the return value.
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("loopback");
        listener.set_nonblocking(true).expect("nonblocking");
        let port = std::net::TcpListener::local_addr(&listener)
            .expect("a bound address")
            .port();
        let endpoint = Endpoint::parse(&format!("http://127.0.0.1:{port}/v1/chat/completions"))
            .expect("an endpoint");
        let cancel = Cancel::new();
        cancel.ask();
        let ended = HttpStream::new(endpoint).stream(&shape(), deadline(), &cancel, &mut |_| {
            panic!("a stopped call delivered a piece")
        });
        assert_eq!(ended, Ok(Ended::Cancelled));
        match listener.accept() {
            Err(why) if why.kind() == io::ErrorKind::WouldBlock => {}
            other => panic!("a stopped call connected anyway: {other:?}"),
        }
    }

    #[test]
    fn a_deadline_that_has_arrived_is_arrived_even_at_the_exact_instant() {
        let now = Instant::now();
        assert_eq!(left_before(now, now), None, "a zero budget is not a budget");
        assert_eq!(
            left_before(now + Duration::from_secs(2), now),
            Some(Duration::from_secs(2))
        );
        assert_eq!(left_before(now, now + Duration::from_secs(1)), None);
    }

    #[test]
    fn every_legal_line_ending_ends_an_event_and_a_split_crlf_is_one_ending() {
        let read = |reads: &[&[u8]]| {
            let mut events = Events::default();
            let mut out = Vec::new();
            for bytes in reads {
                out.extend(events.feed(bytes).expect("well-formed"));
            }
            out
        };
        assert_eq!(read(&[b"data: a\r\rdata: b\r\r"]), ["a"], "bare CR");
        assert_eq!(read(&[b"data: a\n\r\n"]), ["a"], "LF then CRLF");
        assert_eq!(read(&[b"data: a\r\n\r\n"]), ["a"], "CRLF");
        assert_eq!(
            read(&[b"data: a\r", b"\n", b"data: b\r\n\r\n"]),
            ["a\nb"],
            "a CRLF split across two reads is one line ending, not a blank line"
        );
        assert_eq!(read(&[b"data: a\r", b"\n\r", b"\n"]), ["a"]);
    }

    #[test]
    fn an_event_ended_by_a_bare_cr_as_the_last_bytes_of_the_stream_is_read() {
        // Its blank line is the final `\r`, held for a `\n` that never comes:
        // the end of the body is what says it was a line ending.
        let event =
            r#"data: {"choices":[{"index":0,"delta":{"content":"x"},"finish_reason":"stop"}]}"#;
        let stub =
            Stub::serving(vec![Act::Chunked(vec![format!("{event}\r\r")])]).expect("loopback");
        let mut seen = Vec::new();
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |piece| seen.push(text(piece)),
        );
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("stop".to_owned()),
                timings: None,
            })
        );
        assert_eq!(seen, ["x"]);
    }

    #[test]
    fn a_close_framed_stream_whose_last_bytes_are_a_bare_cr_is_read_when_it_closes() {
        // No `Content-Length`, not chunked: the body ends when the server
        // closes, so the held `\r` is resolved on the CLOSE path, not the
        // framing path the chunked case above takes (#120's third review).
        let event =
            r#"data: {"choices":[{"index":0,"delta":{"content":"x"},"finish_reason":"stop"}]}"#;
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{event}\r\r"
        );
        let stub = Stub::serving(vec![Act::Raw(reply.into_bytes())]).expect("loopback");
        let mut seen = Vec::new();
        let ended = HttpStream::new(endpoint(&stub)).stream(
            &shape(),
            deadline(),
            &Cancel::new(),
            &mut |piece| seen.push(text(piece)),
        );
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("stop".to_owned()),
                timings: None,
            })
        );
        assert_eq!(seen, ["x"]);
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
                body: r#"{"error":"busy"}"#.to_owned(),
                class: None,
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
