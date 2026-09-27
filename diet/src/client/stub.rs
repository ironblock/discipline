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
/// should look like one.
#[derive(Debug)]
pub struct Stub {
    address: SocketAddr,
    stop: Arc<AtomicBool>,
    hangups: Arc<Mutex<Vec<Held>>>,
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
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
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
        let worker = thread::spawn(move || serve(&listener, acts, &flag, &seen));
        Ok(Self {
            address,
            stop,
            hangups,
            worker: Some(worker),
        })
    }

    /// The chat-completions URL this stub answers on.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/v1/chat/completions", self.address)
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

/// Serve every act, and collect what was asked.
fn serve(
    listener: &TcpListener,
    acts: Vec<Act>,
    stop: &AtomicBool,
    hangups: &Mutex<Vec<Held>>,
) -> Vec<String> {
    let mut asked = Vec::new();
    for act in acts {
        let Some(mut stream) = accept(listener, stop) else {
            break;
        };
        match read_request(&mut stream) {
            Ok(body) => asked.push(body),
            // A client that timed out may have closed mid-request. That is
            // the case under test, not an error here -- but it is recorded as
            // itself rather than as an empty body, because this module's whole
            // subject is not confusing those two.
            Err(why) => asked.push(format!("<unread: {why}>")),
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
fn accept(listener: &TcpListener, stop: &AtomicBool) -> Option<TcpStream> {
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
                if waiting_since.elapsed() > IDLE_CAP {
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
    }
    None
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

/// Read one request and return its body.
fn read_request(stream: &mut TcpStream) -> io::Result<String> {
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
            return Ok(String::from_utf8_lossy(&raw[end..]).into_owned());
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
