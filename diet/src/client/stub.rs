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

use std::io::{self, Read as _, Write as _};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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
    /// Accept the connection and close it without answering.
    Hangup,
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
        let address = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let worker = thread::spawn(move || serve(&listener, acts, &flag));
        Ok(Self {
            address,
            stop,
            worker: Some(worker),
        })
    }

    /// The chat-completions URL this stub answers on.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://{}/v1/chat/completions", self.address)
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
fn serve(listener: &TcpListener, acts: Vec<Act>, stop: &AtomicBool) -> Vec<String> {
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
        act_on(&mut stream, &act);
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

fn act_on(stream: &mut TcpStream, act: &Act) {
    match act {
        Act::Answer(body) => write_reply(stream, 200, body),
        Act::Status(status, body) => write_reply(stream, *status, body),
        Act::Stall(wait, body) => {
            thread::sleep(*wait);
            write_reply(stream, 200, body);
        }
        Act::Hangup => {}
    }
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

fn write_reply(stream: &mut TcpStream, status: u16, body: &str) {
    let head = format!(
        "HTTP/1.1 {status} \r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n",
        body.len()
    );
    // Nothing here is worth failing over: the client's view is the subject of
    // every test, and a write that fails because the client already gave up is
    // the case under test succeeding.
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

/// A plausible OpenAI-compatible answer, for tests that do not care what the
/// body looks like.
///
/// Anything an acceptance row is ABOUT is written out by the row, not built
/// here -- a helper that produced the mismatching temperature would be the
/// test and the fixture agreeing with each other.
#[must_use]
pub fn plain_answer(text: &str) -> String {
    format!(
        "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{text}\"}},\
         \"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":11,\
         \"completion_tokens\":3}}}}"
    )
}
