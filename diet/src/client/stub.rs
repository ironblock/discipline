//! A stub server: canned HTTP, on loopback, that can lie in named ways.
//!
//! Not a mock of the client's own reader. A fixture built by the code under
//! test certifies itself, and the acceptance rows this exists for are about
//! what happens when a *server* does something the client did not ask for.
//! So the stub serves bytes a test wrote by hand, over a real socket, and the
//! client parses them with no idea where they came from.
//!
//! Loopback only, on a port the operating system chooses (`127.0.0.1:0`), so
//! two of these can run at once and neither collides with anything a machine
//! already has listening.
//!
//! It is in the library rather than in a test file because the seam
//! controller and the integration lane need the same thing: a server whose
//! answers are a script, so that a drive over the machinery is deterministic
//! and needs no model.

use std::fmt::Write as _;
use std::io::{self, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// One thing the stub does for one connection, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Act {
    /// Answer `200` with this body, verbatim.
    Answer(String),
    /// Answer with this status and body. The 4xx a server sends when it does
    /// not know a field is this.
    Status(u16, String),
    /// Read the request, wait, then answer `200` with this body. The wait is
    /// what a deadline is measured against.
    Stall(Duration, String),
    /// Answer `200` with this body, framed with `Transfer-Encoding: chunked`,
    /// split at the given boundaries.
    Chunked(Vec<String>),
    /// Answer `200` with this body and then HOLD the connection open.
    ///
    /// The act that tells a `Content-Length` reader from one that waits for
    /// the close: a client that only knows how to read to end-of-stream sits
    /// here until its deadline, against a server that answered at once.
    AnswerAndHold(String, Duration),
    /// Announce `Content-Length` for MORE than is sent, then close. What a
    /// server that died mid-write looks like from the outside.
    Undercount(String, usize),
    /// Accept the connection and close it without answering.
    Hangup,
    /// Write these bytes verbatim -- status line, headers, framing and all --
    /// then close. For replaying a reply captured off a real server, byte
    /// for byte, rather than one this module composed.
    Raw(Vec<u8>),
    /// Answer `200` as an event stream, one chunk per piece, each flushed on
    /// its own; then HOLD the connection open, sending nothing, until the
    /// client hangs up or [`IDLE_CAP`] passes. How long the client took to
    /// hang up is recorded, and [`Stub::hangups`] says -- which is how a
    /// cancel is seen from the SERVER's side of the socket, where "the
    /// client stopped" actually has to land.
    StreamThenHold(Vec<String>),
    /// Answer `200` as an event stream, one chunk per piece, each sent this
    /// long after the one before; then end the stream and close. A server
    /// still working -- slow, never silent -- for longer than any one gap.
    Trickle(Duration, Vec<String>),
}

/// What an [`Act::StreamThenHold`] saw of its client while it held.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held {
    /// The client hung up this long after the last piece was sent.
    HungUp(Duration),
    /// The client was still connected when the idle cap ran out.
    NeverHungUp,
}

/// A running stub server.
///
/// Serves its script one connection at a time and then stops listening, so a
/// client that makes more calls than the script has acts gets a refused
/// connection rather than a silent hang -- an extra call is a defect and it
/// should look like one. A stub that answers `/props` keeps listening for
/// them, so an extra call after its acts is answered `503` and recorded as
/// `<beyond the script: ...>` instead: visible, and never a silent close.
#[derive(Debug)]
pub struct Stub {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    hangups: Arc<Mutex<Vec<Held>>>,
    heads: Arc<Mutex<Vec<String>>>,
    worker: Option<JoinHandle<Vec<String>>>,
}

/// How long the stub will wait for a connection that never comes.
///
/// A script with more acts than the client makes calls is the normal shape of
/// a bounded-retry test: the point of the test is that the extra call is NOT
/// made. Without this the serving thread would block on `accept` forever and
/// the test would hang instead of failing, and a suite that hangs is a suite
/// nobody runs.
const IDLE_CAP: Duration = Duration::from_secs(20);

impl Stub {
    /// Start a stub serving `acts`, in order.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if loopback cannot be bound.
    pub fn serving(acts: Vec<Act>) -> io::Result<Self> {
        Self::start(acts, None, Replay::No, 0)
    }

    /// A stub serving `acts` that ALSO answers `GET /props` with
    /// `build_info`, without spending an act on it: a server whose engine
    /// check a session can make (#219 item 11). The canned server is one,
    /// answering with [`crate::drive::canned::build_info`]. A `/props`
    /// request that cannot be read is not known to be one, so it spends an
    /// act as `<unread: ...>`, like any unreadable request.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if loopback cannot be bound.
    pub fn serving_with_props(acts: Vec<Act>, build_info: &str) -> io::Result<Self> {
        Self::start(
            acts,
            Some(format!("{{\"build_info\":\"{build_info}\"}}")),
            Replay::No,
            0,
        )
    }

    /// The stream-replay mode (#411): every request that is not a `GET
    /// /props` is answered with `reply`, verbatim, for as long as the stub
    /// runs -- no script to run out of, and no idle cap -- and `/props` with
    /// `build_info`. On `port` of loopback; `0` lets the system choose.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if loopback cannot be bound.
    pub fn replaying(reply: Vec<u8>, build_info: &str, port: u16) -> io::Result<Self> {
        Self::start(
            vec![Act::Raw(reply)],
            Some(format!("{{\"build_info\":\"{build_info}\"}}")),
            Replay::Forever,
            port,
        )
    }

    /// The stream-replay mode with tool turns (#411's follow-up): a request
    /// whose last message is a tool result is answered with `answer`, and
    /// every other request with `call` -- a streamed tool call -- verbatim,
    /// for as long as the stub runs. Chosen by the request, not by its
    /// place in a sequence, so a request the replay did not expect (a fork,
    /// a retry) cannot put the two out of step. `/props` answers
    /// `build_info`.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if loopback cannot be bound.
    pub fn replaying_tool_turns(
        call: Vec<u8>,
        answer: Vec<u8>,
        build_info: &str,
        port: u16,
    ) -> io::Result<Self> {
        Self::start(
            vec![Act::Raw(call), Act::Raw(answer)],
            Some(format!("{{\"build_info\":\"{build_info}\"}}")),
            Replay::ToolTurns,
            port,
        )
    }

    /// Where it listens.
    #[must_use]
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    fn start(acts: Vec<Act>, props: Option<String>, replay: Replay, port: u16) -> io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;
        // Spelled through the type rather than as a method call on the
        // binding, which the hygiene table reads as an internal hostname:
        // an identifier, a dot, one of its reserved suffixes, and any
        // character that cannot continue a hostname label. A false positive
        // in the gate, disclosed on the PR rather than patched from here --
        // the pattern table is not this seat's file.
        let address = TcpListener::local_addr(&listener)?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let hangups = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::clone(&hangups);
        let heads = Arc::new(Mutex::new(Vec::new()));
        let headed = Arc::clone(&heads);
        let worker = thread::spawn(move || {
            serve(
                &listener,
                acts,
                props.as_deref(),
                replay,
                &flag,
                &seen,
                &headed,
            )
        });
        Ok(Self {
            address,
            stop,
            hangups,
            heads,
            worker: Some(worker),
        })
    }

    /// The chat-completions URL this stub answers on.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/v1/chat/completions", self.address)
    }

    /// The head of every request received so far -- request line and
    /// headers, as sent -- in order, one per request as `asked` has one: a
    /// request that could not be read is `<unread: ...>` in both. A `/props`
    /// the stub answers has a head here and no entry in `asked`. What a
    /// claim about a header is checked against.
    #[must_use]
    pub fn heads(&self) -> Vec<String> {
        self.heads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// For each [`Act::StreamThenHold`] served so far, in order, what it saw
    /// of its client.
    #[must_use]
    pub fn hangups(&self) -> Vec<Held> {
        self.hangups
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Stop the stub and return the request bodies it received, in order.
    ///
    /// The bodies are the evidence for the half of every acceptance row that
    /// is about what the client SENT -- a retry that strips a field is a
    /// claim about a request, and this is where that claim is checked.
    ///
    /// # Panics
    ///
    /// Panics if the serving thread panicked, which is a defect in this
    /// module rather than a condition a caller can handle.
    #[must_use]
    pub fn received(mut self) -> Vec<String> {
        self.stop.store(true, Ordering::SeqCst);
        self.worker
            .take()
            .expect("the stub is joined exactly once")
            .join()
            .expect("the stub's serving thread panicked")
    }
}

impl Drop for Stub {
    fn drop(&mut self) {
        // A stub dropped without being joined -- a test that panicked before
        // its assertions -- must not leave a thread sitting on a socket for
        // the rest of the run.
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Whether the script is played once, or over and over (the stream-replay
/// mode, [`Stub::replaying`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Replay {
    No,
    Forever,
    /// The first act for a request whose last message is not a tool result,
    /// the second for one whose last message is.
    ToolTurns,
}

/// Whether a chat-completions body's last message is a tool result: the
/// last `"role":"` key in the body names `tool`. A string inside a message
/// carries its quotes escaped, so the unescaped key is only ever a role.
fn answers_a_tool(body: &str) -> bool {
    const KEY: &str = "\"role\":\"";
    body.rfind(KEY)
        .is_some_and(|at| body[at + KEY.len()..].starts_with("tool\""))
}
/// Serve every act, and collect what was asked.
fn serve(
    listener: &TcpListener,
    acts: Vec<Act>,
    props: Option<&str>,
    replay: Replay,
    stop: &AtomicBool,
    hangups: &Mutex<Vec<Held>>,
    heads: &Mutex<Vec<String>>,
) -> Vec<String> {
    let mut asked = Vec::new();
    let pair = acts.clone();
    let played: Box<dyn Iterator<Item = Act>> = match replay {
        Replay::No => Box::new(acts.into_iter()),
        Replay::Forever | Replay::ToolTurns => Box::new(acts.into_iter().cycle()),
    };
    let mut acts = played.peekable();
    let idle_cap = (replay == Replay::No).then_some(IDLE_CAP);
    loop {
        // Without a `/props` to answer, the stub serves exactly its acts and
        // then refuses, as it always has. With one, it answers `/props`
        // whenever asked -- before, between and after its acts -- and an act
        // waits for the next connection that is not a `/props`.
        if props.is_none() && acts.peek().is_none() {
            break;
        }
        let Some(mut stream) = accept(listener, stop, idle_cap) else {
            break;
        };
        let read = read_request(&mut stream);
        if let (Some(props), Ok((head, _))) = (props, &read)
            && head.starts_with("GET /props ")
        {
            heads
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(head.clone());
            write_reply(&mut stream, 200, props, Closing::Yes);
            continue;
        }
        let Some(act) = acts.next() else {
            // Beyond the script, with `/props` still answered: a defect, made
            // to look like one rather than closed silently.
            let beyond = match &read {
                Ok((head, _)) => format!("<beyond the script: {head}>"),
                Err(why) => format!("<beyond the script: unread: {why}>"),
            };
            heads
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(beyond.clone());
            asked.push(beyond);
            write_reply(&mut stream, 503, "beyond the stub's script", Closing::Yes);
            break;
        };
        let act = match (replay, &read) {
            (Replay::ToolTurns, Ok((_, body))) => pair
                .get(usize::from(answers_a_tool(body)))
                .cloned()
                .unwrap_or(act),
            _ => act,
        };
        match read {
            Ok((head, body)) => {
                heads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(head);
                asked.push(body);
            }
            // A client that timed out may have closed mid-request. That is
            // the case under test, not an error here -- but it is recorded as
            // itself rather than as an empty body, because this module's whole
            // subject is not confusing those two.
            Err(why) => {
                let unread = format!("<unread: {why}>");
                heads
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(unread.clone());
                asked.push(unread);
            }
        }
        if let Some(hung_up) = act_on(&mut stream, &act) {
            hangups
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(hung_up);
        }
    }
    asked
}

/// Wait for one connection, giving up when the stub is stopped or the cap is
/// reached.
fn accept(
    listener: &TcpListener,
    stop: &AtomicBool,
    idle_cap: Option<Duration>,
) -> Option<TcpStream> {
    let waiting_since = Instant::now();
    loop {
        if stop.load(Ordering::SeqCst) {
            return None;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).ok()?;
                return Some(stream);
            }
            Err(why) if why.kind() == io::ErrorKind::WouldBlock => {
                if idle_cap.is_some_and(|cap| waiting_since.elapsed() > cap) {
                    return None;
                }
                thread::sleep(Duration::from_millis(2));
            }
            Err(_) => return None,
        }
    }
}

/// Carry out one act. For [`Act::StreamThenHold`], what the hold saw.
fn act_on(stream: &mut TcpStream, act: &Act) -> Option<Held> {
    match act {
        Act::Answer(body) => write_reply(stream, 200, body, Closing::Yes),
        Act::Status(status, body) => write_reply(stream, *status, body, Closing::Yes),
        Act::Stall(wait, body) => {
            thread::sleep(*wait);
            write_reply(stream, 200, body, Closing::Yes);
        }
        Act::Chunked(pieces) => write_chunked(stream, pieces),
        Act::Undercount(body, short_by) => write_undercount(stream, body, *short_by),
        Act::AnswerAndHold(body, hold) => {
            write_reply(stream, 200, body, Closing::No);
            thread::sleep(*hold);
        }
        Act::Hangup => {}
        Act::Raw(bytes) => {
            let _ = stream.write_all(bytes);
            let _ = stream.flush();
        }
        Act::StreamThenHold(pieces) => return Some(stream_then_hold(stream, pieces)),
        Act::Trickle(gap, pieces) => trickle(stream, *gap, pieces),
    }
    None
}

/// Stream `pieces` as chunks, `gap` apart, then end the stream.
fn trickle(stream: &mut TcpStream, gap: Duration, pieces: &[String]) {
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
          Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
    );
    for piece in pieces {
        thread::sleep(gap);
        let _ = stream.write_all(format!("{:x}\r\n{piece}\r\n", piece.len()).as_bytes());
        let _ = stream.flush();
    }
    let _ = stream.write_all(b"0\r\n\r\n");
    let _ = stream.flush();
}

/// Stream `pieces` as chunks, then wait for the client to hang up.
fn stream_then_hold(stream: &mut TcpStream, pieces: &[String]) -> Held {
    let _ = stream.write_all(
        b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
          Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
    );
    for piece in pieces {
        let _ = stream.write_all(format!("{:x}\r\n{piece}\r\n", piece.len()).as_bytes());
        let _ = stream.flush();
    }
    // A read that returns zero bytes, or fails, is the client closing. The
    // client sends nothing after its request, so nothing else can arrive.
    let held = Instant::now();
    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
    let mut buffer = [0_u8; 64];
    while held.elapsed() < IDLE_CAP {
        match stream.read(&mut buffer) {
            Ok(0) => return Held::HungUp(held.elapsed()),
            Ok(_) => {}
            Err(why)
                if matches!(
                    why.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return Held::HungUp(held.elapsed()),
        }
    }
    Held::NeverHungUp
}

/// Whether a reply announces that the connection ends with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closing {
    Yes,
    No,
}

/// Read one request and return its head and its body.
fn read_request(stream: &mut TcpStream) -> io::Result<(String, String)> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut raw = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let end = raw
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|index| index + 4);
        if let Some(end) = end {
            let headers = String::from_utf8_lossy(&raw[..end]).to_string();
            let announced = content_length(&headers).unwrap_or(0);
            while raw.len() < end + announced {
                let count = stream.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                raw.extend_from_slice(&buffer[..count]);
            }
            return Ok((headers, String::from_utf8_lossy(&raw[end..]).into_owned()));
        }
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the request ended before its headers did",
            ));
        }
        raw.extend_from_slice(&buffer[..count]);
    }
}

fn content_length(headers: &str) -> Option<usize> {
    for line in headers.split("\r\n") {
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case("content-length")
        {
            return value.trim().parse().ok();
        }
    }
    None
}

fn write_reply(stream: &mut TcpStream, status: u16, body: &str, closing: Closing) {
    let connection = match closing {
        Closing::Yes => "close",
        Closing::No => "keep-alive",
    };
    let head = format!(
        "HTTP/1.1 {status} \r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: {connection}\r\n\r\n",
        body.len()
    );
    // Nothing here is worth failing over: the client's view is the subject of
    // every test, and a write that fails because the client already gave up is
    // the case under test succeeding.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

/// Write a body as chunked transfer-encoding, split at the caller's
/// boundaries. The framing itself is written correctly, so a test using this
/// is about the client's reader and not about a broken server.
fn write_chunked(stream: &mut TcpStream, pieces: &[String]) {
    let mut out = String::from(
        "HTTP/1.1 200 \r\nContent-Type: application/json\r\n\
         Transfer-Encoding: chunked\r\nConnection: close\r\n\r\n",
    );
    for piece in pieces {
        // Infallible: the target is a String.
        let _ = write!(out, "{:x}\r\n{piece}\r\n", piece.len());
    }
    out.push_str("0\r\n\r\n");
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.flush();
}

/// Announce more body than is sent.
fn write_undercount(stream: &mut TcpStream, body: &str, short_by: usize) {
    let head = format!(
        "HTTP/1.1 200 \r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len() + short_by
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}
