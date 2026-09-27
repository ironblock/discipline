//! A session served over HTTP: its log as a stream of server-sent events,
//! its commands as JSON posts (#117 R2c, increment I4 of
//! `diet/drive/plans/r2c-proposal.md`).
//!
//! Two routes, and nothing else answers:
//!
//! * `GET /events` replays the log from a sequence number and then tails it.
//!   Each event is `id: <opened>-<seq>` and one `data:` line; `opened` is the
//!   session's own start time, read from its first event, so a page that
//!   resumes against a restarted process is told `410` instead of being fed
//!   another session's events as if they continued its own (D7).
//! * `POST /commands` takes `{"kind": <command>, ...}`. A refusal is `409`
//!   with its tag, and the session has logged it (D8).
//!
//! # Who may drive it
//!
//! Loopback is not a boundary against the author's own browser: any page they
//! visit can reach `127.0.0.1`, and was measured doing so (R2c critique,
//! finding 1). So every request's `Host` must be one this server answers to,
//! an `Origin`, when present, must be on the allowed list, and a post must be
//! `application/json` -- which a page on another site can only send after a
//! preflight, and the preflight has no route (D17). What this does not stop
//! is another program on the same machine; that is Basic auth's job (I7).
//!
//! # One thread per connection
//!
//! A reader holds its thread for as long as it reads, and readers get owned
//! copies of the log (`Session::wait_from`), so none of them can stall the
//! session or another client. Each connection has a read timeout while its
//! request arrives and a write timeout for as long as it is written to, and
//! there is a cap on how many there are at once (D11).
//!
//! How a logged event is written is a parameter, [`Render`]: the log's own
//! format (`diet/formats/log`, I1) is not built yet, and nothing here reads an
//! event's kind.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{self, Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::session::{CancelError, CommandKind, Logged, Refusal, Session};
use crate::client::stream::Streaming;
use crate::formats::record::json::{self, Value};

/// How one logged event is written as the `data:` of its server-sent event.
/// It must not contain a line break: one event, one `data:` line.
pub type Render = fn(&Logged) -> String;

/// How a server behaves at its edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// How long a stream may be silent before a comment line is sent. The
    /// write is also how a reader that left is noticed.
    pub heartbeat: Duration,
    /// How long a request may take to arrive.
    pub read_timeout: Duration,
    /// How long one write to a client may block.
    pub write_timeout: Duration,
    /// How many connections may be open at once.
    pub max_connections: usize,
    /// The largest request head read, in bytes.
    pub max_head: usize,
    /// The largest command body read, in bytes.
    pub max_body: usize,
    /// The origins allowed to drive the session, as a browser sends them
    /// (`http://localhost:5173`). Each one's host and port is also accepted
    /// as a `Host`, which is what a development proxy forwards.
    pub allowed_origins: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            heartbeat: Duration::from_secs(15),
            read_timeout: Duration::from_secs(10),
            write_timeout: Duration::from_secs(10),
            max_connections: 32,
            max_head: 16 * 1024,
            max_body: 1024 * 1024,
            allowed_origins: Vec::new(),
        }
    }
}

/// A running server. Dropping it stops it accepting; its open streams end at
/// their next heartbeat.
#[derive(Debug)]
pub struct Server {
    addr: SocketAddr,
    stopping: Arc<AtomicBool>,
    live: Arc<AtomicUsize>,
    accept: Option<JoinHandle<()>>,
}

impl Server {
    /// Serve `session` on `listener`.
    ///
    /// # Errors
    ///
    /// When the listener's address cannot be read, or the accepting thread
    /// cannot start.
    pub fn start<S: Streaming + 'static>(
        listener: TcpListener,
        session: Arc<Session<S>>,
        config: Config,
        render: Render,
    ) -> io::Result<Self> {
        let addr = TcpListener::local_addr(&listener)?;
        let stopping = Arc::new(AtomicBool::new(false));
        let live = Arc::new(AtomicUsize::new(0));
        let serving = Serving {
            opened: session.opened(),
            session,
            hosts: hosts_for(addr, &config.allowed_origins),
            config,
            render,
            stopping: Arc::clone(&stopping),
            live: Arc::clone(&live),
        };
        let accept = thread::Builder::new()
            .name("diet-serve".to_owned())
            .spawn(move || serving.accept(&listener))?;
        Ok(Self {
            addr,
            stopping,
            live,
            accept: Some(accept),
        })
    }

    /// Where it listens.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// How many connections are open now.
    #[must_use]
    pub fn connections(&self) -> usize {
        self.live.load(Ordering::SeqCst)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        // Wake the accept loop so it sees the flag.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1));
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
    }
}

/// The `Host` values this server answers to: loopback at its own port, and
/// each allowed origin's host and port.
fn hosts_for(addr: SocketAddr, origins: &[String]) -> Vec<String> {
    let port = addr.port();
    let mut hosts = vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    hosts.extend(
        origins
            .iter()
            .filter_map(|origin| origin.split_once("://").map(|(_, authority)| authority))
            .map(str::to_ascii_lowercase),
    );
    hosts
}

struct Serving<S: Streaming + 'static> {
    session: Arc<Session<S>>,
    opened: u64,
    hosts: Vec<String>,
    config: Config,
    render: Render,
    stopping: Arc<AtomicBool>,
    live: Arc<AtomicUsize>,
}

impl<S: Streaming + 'static> Clone for Serving<S> {
    fn clone(&self) -> Self {
        Self {
            session: Arc::clone(&self.session),
            opened: self.opened,
            hosts: self.hosts.clone(),
            config: self.config.clone(),
            render: self.render,
            stopping: Arc::clone(&self.stopping),
            live: Arc::clone(&self.live),
        }
    }
}

/// Counts a connection for as long as its thread holds it.
struct Live(Arc<AtomicUsize>);

impl Drop for Live {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A request's head, and whatever of its body arrived with it.
struct Request {
    method: String,
    path: String,
    query: String,
    headers: Vec<(String, String)>,
    early_body: Vec<u8>,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }
}

/// Why no request could be read.
enum Unread {
    /// The head passed the cap.
    TooLarge,
    /// The head was not HTTP.
    Malformed,
    /// The client went away, or took longer than the read timeout.
    Gone,
}

type Handler<S> = fn(&Serving<S>, &mut TcpStream, &Request);

/// Every route there is. Anything else, `OPTIONS` included, is `404`.
fn routes<S: Streaming + 'static>() -> [(&'static str, &'static str, Handler<S>); 2] {
    [
        ("GET", "/events", Serving::<S>::events),
        ("POST", "/commands", Serving::<S>::command),
    ]
}

/// The reason phrase for each status this server sends.
const REASONS: &[(u16, &str)] = &[
    (200, "OK"),
    (400, "Bad Request"),
    (403, "Forbidden"),
    (404, "Not Found"),
    (409, "Conflict"),
    (410, "Gone"),
    (413, "Content Too Large"),
    (415, "Unsupported Media Type"),
    (503, "Service Unavailable"),
];

impl<S: Streaming + 'static> Serving<S> {
    fn accept(&self, listener: &TcpListener) {
        for incoming in listener.incoming() {
            if self.stopping.load(Ordering::SeqCst) {
                return;
            }
            let Ok(mut stream) = incoming else { continue };
            if self.live.load(Ordering::SeqCst) >= self.config.max_connections {
                let _ = stream.set_write_timeout(Some(self.config.write_timeout));
                respond(&mut stream, 503, &Value::Object(BTreeMap::new()));
                continue;
            }
            self.live.fetch_add(1, Ordering::SeqCst);
            let live = Live(Arc::clone(&self.live));
            let serving = self.clone();
            let spawned = thread::Builder::new()
                .name("diet-serve-conn".to_owned())
                .spawn(move || {
                    let _live = live;
                    serving.connection(stream);
                });
            // A refused thread drops its closure, and with it `live` and the
            // stream: the count goes back down and the client sees a close.
            drop(spawned);
        }
    }

    fn connection(&self, mut stream: TcpStream) {
        if stream
            .set_read_timeout(Some(self.config.read_timeout))
            .and_then(|()| stream.set_write_timeout(Some(self.config.write_timeout)))
            .is_err()
        {
            return;
        }
        let request = match read_request(&mut stream, self.config.max_head) {
            Ok(request) => request,
            Err(Unread::TooLarge) => return respond(&mut stream, 413, &empty()),
            Err(Unread::Malformed) => return respond(&mut stream, 400, &empty()),
            Err(Unread::Gone) => return,
        };
        let host_allowed = request.header("host").is_some_and(|host| {
            self.hosts
                .iter()
                .any(|allowed| allowed.eq_ignore_ascii_case(host))
        });
        let origin_allowed = request.header("origin").is_none_or(|origin| {
            self.config
                .allowed_origins
                .iter()
                .any(|allowed| allowed == origin)
        });
        if !host_allowed || !origin_allowed {
            return respond(&mut stream, 403, &empty());
        }
        let route = routes::<S>()
            .into_iter()
            .find(|(method, path, _)| *method == request.method && *path == request.path);
        match route {
            Some((_, _, handler)) => handler(self, &mut stream, &request),
            None => respond(&mut stream, 404, &empty()),
        }
        let _ = stream.shutdown(Shutdown::Both);
    }

    /// `GET /events`: replay from a sequence number, then tail.
    fn events(&self, stream: &mut TcpStream, request: &Request) {
        let first = match request.header("last-event-id") {
            Some(id) => match resume_after(id, self.opened) {
                Ok(seq) => seq + 1,
                Err(status) => return respond(stream, status, &empty()),
            },
            None => match from_query(&request.query) {
                Some(first) => first,
                None => return respond(stream, 400, &empty()),
            },
        };
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\
                    Cache-Control: no-cache\r\nConnection: close\r\n\r\n";
        if stream.write_all(head.as_bytes()).is_err() {
            return;
        }
        let mut next = first;
        while !self.stopping.load(Ordering::SeqCst) {
            let batch = self.session.wait_from(next, self.config.heartbeat);
            let mut out = String::new();
            if batch.is_empty() {
                out.push_str(":\n\n");
            }
            for logged in &batch {
                let _ = write!(
                    out,
                    "id: {}-{}\ndata: {}\n\n",
                    self.opened,
                    logged.seq,
                    (self.render)(logged)
                );
                next = logged.seq + 1;
            }
            if stream.write_all(out.as_bytes()).is_err() || stream.flush().is_err() {
                return;
            }
        }
    }

    /// `POST /commands`: one command, answered with what the session did.
    fn command(&self, stream: &mut TcpStream, request: &Request) {
        let json_body = request.header("content-type").is_some_and(|kind| {
            kind.split(';')
                .next()
                .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
        });
        if !json_body {
            return respond(stream, 415, &empty());
        }
        let Some(length) = request
            .header("content-length")
            .and_then(|length| length.trim().parse::<usize>().ok())
        else {
            return respond(stream, 400, &empty());
        };
        if length > self.config.max_body {
            return respond(stream, 413, &empty());
        }
        let Some(body) = read_body(stream, &request.early_body, length) else {
            return;
        };
        let Some(command) = std::str::from_utf8(&body)
            .ok()
            .and_then(|text| json::line(text.trim_end()).ok())
            .and_then(|object| Command::from_object(&object))
        else {
            return respond(stream, 400, &empty());
        };
        let (status, reply) = self.run(command);
        respond(stream, status, &Value::Object(reply));
    }

    fn run(&self, command: Command) -> (u16, BTreeMap<String, Value>) {
        let refused = |because: Refusal| {
            (
                409,
                BTreeMap::from([(
                    "refused".to_owned(),
                    Value::String(because.tag().to_owned()),
                )]),
            )
        };
        match command {
            Command::Ask(text) => match self.session.ask(&text) {
                Ok(admitted) => (
                    200,
                    BTreeMap::from([
                        ("seq".to_owned(), integer(admitted.seq)),
                        ("turn".to_owned(), integer(u64::from(admitted.turn))),
                    ]),
                ),
                Err(because) => refused(because),
            },
            Command::Cancel(turn) => match self.session.cancel(turn) {
                Ok(()) => (200, BTreeMap::new()),
                Err(CancelError::Refused(because)) => refused(because),
                Err(CancelError::NoSuchTurn(_)) => (400, BTreeMap::new()),
            },
            Command::DeclareSeam => match self.session.declare_seam() {
                Ok(()) => (200, BTreeMap::new()),
                Err(because) => refused(because),
            },
            Command::End => match self.session.end() {
                Ok(()) => (200, BTreeMap::new()),
                Err(because) => refused(because),
            },
        }
    }
}

/// A command as posted.
enum Command {
    Ask(String),
    Cancel(u32),
    DeclareSeam,
    End,
}

impl Command {
    /// A command from its body: `kind` names it, and every other key must be
    /// one that kind takes.
    fn from_object(object: &BTreeMap<String, Value>) -> Option<Self> {
        let Some(Value::String(kind)) = object.get("kind") else {
            return None;
        };
        let kind = CommandKind::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.tag() == kind)?;
        let takes: &[&str] = match kind {
            CommandKind::Ask => &["kind", "text"],
            CommandKind::Cancel => &["kind", "turn"],
            CommandKind::DeclareSeam | CommandKind::End => &["kind"],
        };
        if object.keys().any(|key| !takes.contains(&key.as_str())) {
            return None;
        }
        match kind {
            CommandKind::Ask => match object.get("text") {
                Some(Value::String(text)) => Some(Self::Ask(text.clone())),
                _ => None,
            },
            CommandKind::Cancel => match object.get("turn") {
                Some(Value::Integer(turn)) => u32::try_from(*turn).ok().map(Self::Cancel),
                _ => None,
            },
            CommandKind::DeclareSeam => Some(Self::DeclareSeam),
            CommandKind::End => Some(Self::End),
        }
    }
}

fn empty() -> Value {
    Value::Object(BTreeMap::new())
}

fn integer(number: u64) -> Value {
    Value::Integer(i64::try_from(number).unwrap_or(i64::MAX))
}

/// Where a `Last-Event-ID` resumes: the sequence number it names, if it was
/// issued by this process. `400` for an id that is not `<opened>-<seq>`, and
/// `410` for one from another process's log.
fn resume_after(id: &str, opened: u64) -> Result<u64, u16> {
    let (process, seq) = id.trim().split_once('-').ok_or(400_u16)?;
    let process: u64 = process.parse().map_err(|_| 400_u16)?;
    let seq: u64 = seq.parse().map_err(|_| 400_u16)?;
    if process == opened { Ok(seq) } else { Err(410) }
}

/// The `from` a first connection asks to start at: `0` when it names none,
/// and nothing when what it names is not a number.
fn from_query(query: &str) -> Option<u64> {
    let named = query.split('&').find_map(|pair| pair.strip_prefix("from="));
    named.map_or(Some(0), |from| from.parse().ok())
}

/// Read a request head, up to `max_head` bytes.
fn read_request(stream: &mut TcpStream, max_head: usize) -> Result<Request, Unread> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let end = loop {
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            break end;
        }
        if buffer.len() > max_head {
            return Err(Unread::TooLarge);
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return Err(Unread::Gone),
            Ok(read) => buffer.extend_from_slice(&chunk[..read]),
        }
    };
    if end > max_head {
        return Err(Unread::TooLarge);
    }
    let head = std::str::from_utf8(&buffer[..end]).map_err(|_| Unread::Malformed)?;
    let mut lines = head.split("\r\n");
    let mut first = lines.next().ok_or(Unread::Malformed)?.split(' ');
    let (Some(method), Some(target), Some(_version)) = (first.next(), first.next(), first.next())
    else {
        return Err(Unread::Malformed);
    };
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let headers = lines
        .map(|line| {
            line.split_once(':')
                .map(|(name, value)| (name.trim().to_owned(), value.trim().to_owned()))
                .ok_or(Unread::Malformed)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Request {
        method: method.to_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        headers,
        early_body: buffer[end + 4..].to_vec(),
    })
}

/// Read the rest of a body of `length` bytes. Nothing when the client went
/// away or took longer than the read timeout.
fn read_body(stream: &mut TcpStream, early: &[u8], length: usize) -> Option<Vec<u8>> {
    let mut body = early.get(..length.min(early.len()))?.to_vec();
    let mut rest = vec![0_u8; length - body.len()];
    stream.read_exact(&mut rest).ok()?;
    body.extend_from_slice(&rest);
    Some(body)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Answer with `status` and a JSON body, and close.
fn respond(stream: &mut TcpStream, status: u16, body: &Value) {
    let reason = REASONS
        .iter()
        .find(|(code, _)| *code == status)
        .map_or("", |(_, reason)| reason);
    let mut text = String::new();
    json::render(body, &mut text);
    let reply = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
        text.len()
    );
    let _ = stream.write_all(reply.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::client::stream::{Canned, Gate, Step};
    use crate::drive::session::Event;
    use crate::drive::session::tests::{deltas, settled, template, wait_until};

    /// Every test's renderer: the sequence number, then the event's debug
    /// form, which has no raw line break in it.
    fn render(logged: &Logged) -> String {
        format!("{} {:?}", logged.seq, logged.event)
    }

    fn quick() -> Config {
        Config {
            heartbeat: Duration::from_millis(100),
            read_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(2),
            ..Config::default()
        }
    }

    fn serve(canned: Canned, config: Config) -> (Arc<Session<Canned>>, Server) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
        let session = Arc::new(Session::open(canned, template()));
        let server = Server::start(listener, Arc::clone(&session), config, render)
            .expect("the server starts");
        (session, server)
    }

    fn host(server: &Server) -> String {
        format!("127.0.0.1:{}", server.addr().port())
    }

    /// A connection and what it has read. Every read gives up at one
    /// overall deadline, never on a per-read timeout: heartbeats reset a
    /// per-read timeout forever, and a reader built that way hangs instead
    /// of failing (R2c proposal, section 5; measured twice).
    struct Client {
        stream: TcpStream,
        read: String,
        closed: bool,
    }

    impl Client {
        fn send(server: &Server, raw: &str) -> Self {
            let mut stream = TcpStream::connect(server.addr()).expect("connects");
            stream
                .set_read_timeout(Some(Duration::from_millis(50)))
                .expect("a read timeout");
            stream
                .write_all(raw.as_bytes())
                .expect("the request is written");
            Self {
                stream,
                read: String::new(),
                closed: false,
            }
        }

        /// Read until `done` holds of everything read, the connection
        /// closes, or `within` passes. Returns whether `done` held.
        fn read_until(&mut self, within: Duration, done: impl Fn(&str) -> bool) -> bool {
            let give_up = Instant::now() + within;
            let mut chunk = [0_u8; 16384];
            while !done(&self.read) && !self.closed && Instant::now() < give_up {
                match self.stream.read(&mut chunk) {
                    Ok(0) => self.closed = true,
                    Ok(read) => self.read.push_str(&String::from_utf8_lossy(&chunk[..read])),
                    Err(err)
                        if matches!(
                            err.kind(),
                            io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => self.closed = true,
                }
            }
            done(&self.read)
        }

        /// The whole reply of a request answered with a close.
        fn reply(mut self) -> String {
            self.read_until(Duration::from_secs(5), |_| false);
            self.read
        }

        fn data(&self) -> Vec<String> {
            self.read
                .lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .map(str::to_owned)
                .collect()
        }
    }

    fn status(reply: &str) -> u16 {
        reply
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or_else(|| panic!("not an HTTP reply: {reply:?}"))
    }

    fn body(reply: &str) -> &str {
        reply.split_once("\r\n\r\n").map_or("", |(_, body)| body)
    }

    fn events_request(server: &Server, query: &str, headers: &str) -> String {
        format!(
            "GET /events{query} HTTP/1.1\r\nHost: {}\r\n{headers}\r\n",
            host(server)
        )
    }

    fn post(server: &Server, json: &str, headers: &str) -> String {
        Client::send(
            server,
            &format!(
                "POST /commands HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\n{headers}\r\n{json}",
                host(server),
                json.len()
            ),
        )
        .reply()
    }

    fn asked(session: &Session<Canned>) -> usize {
        session
            .events_from(0)
            .iter()
            .filter(|logged| matches!(logged.event, Event::Asked { .. }))
            .count()
    }

    fn eventually(what: &str, done: impl Fn() -> bool) {
        let give_up = Instant::now() + Duration::from_secs(10);
        while !done() {
            assert!(Instant::now() < give_up, "gave up waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn an_ask_over_http_streams_its_answer_to_a_reader_already_listening() {
        let (_session, server) = serve(Canned::new([deltas(&["Hel", "lo"])]), quick());
        let mut reader = Client::send(&server, &events_request(&server, "", ""));
        assert!(
            reader.read_until(Duration::from_secs(5), |read| read.contains("Started")),
            "the reader never saw the session open: {:?}",
            reader.read
        );
        let reply = post(&server, r#"{"kind":"ask","text":"hi"}"#, "");
        assert_eq!(status(&reply), 200, "{reply}");
        assert_eq!(body(&reply), r#"{"seq":1,"turn":1}"#);
        assert!(
            reader.read_until(Duration::from_secs(10), |read| read.contains("TurnSettled")),
            "the answer never reached the reader: {:?}",
            reader.read
        );
        let data = reader.data().join("\n");
        for piece in [r#"text: "Hel""#, r#"text: "lo""#, r#"text: "Hello""#] {
            assert!(data.contains(piece), "{piece} is not in {data}");
        }
    }

    #[test]
    fn a_late_reader_replays_from_zero_then_tails() {
        let (session, server) = serve(Canned::new([deltas(&["one"]), deltas(&["two"])]), quick());
        session.ask("first").expect("accepted");
        wait_until(&session, "the first turn to settle", settled);
        let so_far = session.events_from(0).len();

        let mut reader = Client::send(&server, &events_request(&server, "?from=0", ""));
        assert!(
            reader.read_until(Duration::from_secs(5), |read| {
                read.matches("data: ").count() >= so_far
            }),
            "the late reader did not get the log so far: {:?}",
            reader.read
        );
        assert!(
            reader.data()[0].starts_with("0 Started"),
            "{:?}",
            reader.data()
        );

        session.ask("second").expect("accepted");
        assert!(
            reader.read_until(Duration::from_secs(10), |read| read
                .contains("turn: 2, reason")),
            "the reader did not go on to tail: {:?}",
            reader.read
        );

        let mut from_three = Client::send(&server, &events_request(&server, "?from=3", ""));
        assert!(from_three.read_until(Duration::from_secs(5), |read| read.contains("data: ")));
        assert!(
            from_three.data()[0].starts_with("3 "),
            "{:?}",
            from_three.data()
        );
    }

    #[test]
    fn a_reader_resuming_after_an_id_of_this_process_starts_at_the_next() {
        let (session, server) = serve(Canned::new([deltas(&["one"])]), quick());
        session.ask("first").expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        let resume = format!("Last-Event-ID: {}-2\r\n", session.opened());
        // The header wins over the query, which is what a browser sends when
        // it reconnects: the same URL, plus the last id it saw.
        let mut reader = Client::send(&server, &events_request(&server, "?from=0", &resume));
        assert!(reader.read_until(Duration::from_secs(5), |read| read.contains("data: ")));
        assert!(reader.data()[0].starts_with("3 "), "{:?}", reader.data());
        assert!(
            reader
                .read
                .contains(&format!("id: {}-3\n", session.opened())),
            "{:?}",
            reader.read
        );
    }

    #[test]
    fn a_resume_from_another_processs_log_is_410() {
        let (session, server) = serve(Canned::new([]), quick());
        let foreign = format!("Last-Event-ID: {}-2\r\n", session.opened() + 1);
        let reply = Client::send(&server, &events_request(&server, "", &foreign)).reply();
        assert_eq!(status(&reply), 410, "{reply}");
        assert!(!reply.contains("data: "), "{reply}");
    }

    #[test]
    fn a_malformed_last_event_id_is_400() {
        let (_session, server) = serve(Canned::new([]), quick());
        for id in ["20", "abc-2", "1-x"] {
            let header = format!("Last-Event-ID: {id}\r\n");
            let reply = Client::send(&server, &events_request(&server, "", &header)).reply();
            assert_eq!(status(&reply), 400, "{id}: {reply}");
        }
    }

    #[test]
    fn a_cross_site_simple_post_is_415_and_never_reaches_the_log() {
        let (session, server) = serve(Canned::new([deltas(&["x"])]), quick());
        let json = r#"{"kind":"ask","text":"sent by another site"}"#;
        let reply = Client::send(
            &server,
            &format!(
                "POST /commands HTTP/1.1\r\nHost: {}\r\nContent-Type: text/plain;charset=UTF-8\r\n\
                 Content-Length: {}\r\n\r\n{json}",
                host(&server),
                json.len()
            ),
        )
        .reply();
        assert_eq!(status(&reply), 415, "{reply}");
        // The preflight a browser would send first has no route.
        let preflight = Client::send(
            &server,
            &format!(
                "OPTIONS /commands HTTP/1.1\r\nHost: {}\r\n\r\n",
                host(&server)
            ),
        )
        .reply();
        assert_eq!(status(&preflight), 404, "{preflight}");
        assert_eq!(asked(&session), 0, "a cross-site post reached the log");
    }

    #[test]
    fn a_request_for_a_rebound_host_name_is_403() {
        let (session, server) = serve(Canned::new([deltas(&["x"])]), quick());
        session.ask("private").expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        let rebound = format!(
            "GET /events?from=0 HTTP/1.1\r\nHost: rebound.example:{}\r\n\r\n",
            server.addr().port()
        );
        let reply = Client::send(&server, &rebound).reply();
        assert_eq!(status(&reply), 403, "{reply}");
        assert!(
            !reply.contains("data: "),
            "a rebound host read the log: {reply}"
        );
    }

    #[test]
    fn a_foreign_origin_is_403_and_never_reaches_the_log() {
        let config = Config {
            allowed_origins: vec!["http://localhost:5173".to_owned()],
            ..quick()
        };
        let (session, server) = serve(Canned::new([deltas(&["x"])]), config);
        let json = r#"{"kind":"ask","text":"hi"}"#;
        let reply = post(&server, json, "Origin: http://elsewhere.example\r\n");
        assert_eq!(status(&reply), 403, "{reply}");
        assert_eq!(asked(&session), 0, "a foreign origin reached the log");

        // What a development proxy forwards: the page's origin, and its host.
        let proxied = Client::send(
            &server,
            &format!(
                "POST /commands HTTP/1.1\r\nHost: localhost:5173\r\n\
                 Origin: http://localhost:5173\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\n\r\n{json}",
                json.len()
            ),
        )
        .reply();
        assert_eq!(status(&proxied), 200, "{proxied}");
        assert_eq!(asked(&session), 1);
    }

    #[test]
    fn a_refused_command_is_409_with_its_tag_and_is_in_the_log() {
        let gate = Gate::new();
        let (session, server) = serve(
            Canned::new([vec![
                Step::Delta("Hel".to_owned()),
                Step::Hold(gate.clone()),
            ]]),
            quick(),
        );
        assert_eq!(
            status(&post(&server, r#"{"kind":"ask","text":"a"}"#, "")),
            200
        );
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        let again = post(&server, r#"{"kind":"ask","text":"b"}"#, "");
        assert_eq!(status(&again), 409, "{again}");
        assert_eq!(body(&again), r#"{"refused":"in-flight"}"#);
        let seam = post(&server, r#"{"kind":"declare-seam"}"#, "");
        assert_eq!(status(&seam), 409, "{seam}");
        assert_eq!(body(&seam), r#"{"refused":"seam-not-built"}"#);

        gate.open();
        wait_until(&session, "the turn to settle", settled);
        assert_eq!(status(&post(&server, r#"{"kind":"end"}"#, "")), 200);
        let late = post(&server, r#"{"kind":"ask","text":"c"}"#, "");
        assert_eq!(status(&late), 409, "{late}");
        assert_eq!(body(&late), r#"{"refused":"ended"}"#);

        let refusals = session
            .events_from(0)
            .iter()
            .filter(|logged| matches!(logged.event, Event::Refused { .. }))
            .count();
        assert_eq!(
            refusals, 3,
            "a refusal answered over HTTP is not in the log"
        );
    }

    #[test]
    fn a_cancel_over_http_reaches_a_call_blocked_mid_answer() {
        // Held, and never opened by this test: only the cancel can wake it.
        let gate = Gate::new();
        let (session, server) = serve(
            Canned::new([vec![
                Step::Delta("Hel".to_owned()),
                Step::Hold(gate.clone()),
            ]]),
            quick(),
        );
        assert_eq!(
            status(&post(&server, r#"{"kind":"ask","text":"a"}"#, "")),
            200
        );
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        let reply = post(&server, r#"{"kind":"cancel","turn":1}"#, "");
        assert_eq!(status(&reply), 200, "{reply}");
        let log = wait_until(&session, "the stopped turn to settle", settled);
        assert!(
            log.iter()
                .any(|logged| matches!(logged.event, Event::Cancelled { .. })),
            "{log:#?}"
        );
    }

    #[test]
    fn a_reader_that_leaves_is_let_go() {
        let (_session, server) = serve(Canned::new([]), quick());
        let mut reader = Client::send(&server, &events_request(&server, "", ""));
        assert!(reader.read_until(Duration::from_secs(5), |read| read.contains("Started")));
        assert_eq!(server.connections(), 1);
        drop(reader);
        // Only a write shows that a reader left, and with nothing to send the
        // only write is the heartbeat.
        eventually("the stream's thread to let the reader go", || {
            server.connections() == 0
        });
    }

    #[test]
    fn a_reader_that_reads_nothing_does_not_keep_another_clients_ask_from_being_answered() {
        let config = Config {
            read_timeout: Duration::from_secs(30),
            ..quick()
        };
        let (_session, server) = serve(Canned::new([deltas(&["x"])]), config);
        // Connected, and silent: its thread waits for a request that does not
        // come.
        let _silent = TcpStream::connect(server.addr()).expect("connects");
        let json = r#"{"kind":"ask","text":"hi"}"#;
        let mut other = Client::send(
            &server,
            &format!(
                "POST /commands HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\n\r\n{json}",
                host(&server),
                json.len()
            ),
        );
        assert!(
            other.read_until(Duration::from_secs(5), |read| read.contains("\r\n\r\n")),
            "one silent connection kept another from being answered"
        );
        assert_eq!(status(&other.read), 200, "{}", other.read);
    }

    #[test]
    fn a_reader_that_stops_reading_is_let_go_within_the_write_timeout() {
        let config = Config {
            write_timeout: Duration::from_millis(200),
            ..quick()
        };
        let big = "x".repeat(64 * 1024);
        let pieces: Vec<Step> = (0..200).map(|_| Step::Delta(big.clone())).collect();
        let (session, server) = serve(Canned::new([pieces]), config);
        // Asks for the stream, then never reads a byte of it.
        let _stalled = Client::send(&server, &events_request(&server, "", ""));
        eventually("the reader to connect", || server.connections() == 1);
        session.ask("fill the socket").expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        eventually(
            "the stream's thread to give up on a reader that stopped",
            || server.connections() == 0,
        );
    }

    #[test]
    fn a_client_that_sends_half_a_request_is_let_go_within_the_read_timeout() {
        let config = Config {
            read_timeout: Duration::from_millis(200),
            ..quick()
        };
        let (_session, server) = serve(Canned::new([]), config);
        let mut half = Client::send(&server, "GET /ev");
        half.read_until(Duration::from_secs(5), |_| false);
        assert!(half.closed, "the server kept waiting on half a request");
        eventually("the connection to be let go", || server.connections() == 0);
    }

    #[test]
    fn a_request_past_its_cap_is_413_before_it_is_read_whole() {
        let config = Config {
            max_body: 1024,
            read_timeout: Duration::from_secs(30),
            ..quick()
        };
        let (session, server) = serve(Canned::new([]), config);
        // The head announces more than the cap and no body follows: a server
        // that read before it checked would wait for it.
        let mut client = Client::send(
            &server,
            &format!(
                "POST /commands HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
                 Content-Length: 2048\r\n\r\n",
                host(&server)
            ),
        );
        assert!(
            client.read_until(Duration::from_secs(5), |read| read.contains("\r\n\r\n")),
            "no answer before the body arrived"
        );
        assert_eq!(status(&client.read), 413, "{}", client.read);
        assert_eq!(asked(&session), 0);
    }

    #[test]
    fn a_connection_past_the_cap_is_503() {
        let config = Config {
            max_connections: 2,
            ..quick()
        };
        let (_session, server) = serve(Canned::new([]), config);
        let _first = Client::send(&server, &events_request(&server, "", ""));
        let _second = Client::send(&server, &events_request(&server, "", ""));
        eventually("two readers to connect", || server.connections() == 2);
        let third = Client::send(&server, &events_request(&server, "", "")).reply();
        assert_eq!(status(&third), 503, "{third}");
    }

    #[test]
    fn a_malformed_command_is_400_and_not_logged() {
        let (session, server) = serve(Canned::new([]), quick());
        for json in [
            r#"{"kind":"ask"}"#,
            r#"{"kind":"ask","text":"x","also":1}"#,
            r#"{"kind":"launch"}"#,
            r#"{"kind":"cancel","turn":"one"}"#,
            "not json",
        ] {
            let reply = post(&server, json, "");
            assert_eq!(status(&reply), 400, "{json}: {reply}");
        }
        let log = session.events_from(0);
        assert_eq!(log.len(), 1, "a malformed command was logged: {log:#?}");
    }
}
