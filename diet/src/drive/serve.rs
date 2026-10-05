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
//!   with its tag, and the session has logged it (D8). An ask may carry
//!   `"scoping": true`, the operator's mark that warrants the capture gap's
//!   fork (#374); its `ask` line carries it.
//! * `POST /approve` takes `{"call": <id>, "scope": once|session|workspace|
//!   decline}`, the operator's answer to the prompt waiting on that call
//!   (#298 point 8): `204`, or `409` with its tag (`nothing-waiting`,
//!   `stale`; `not-standing`, `no-store` beside them), logging nothing.
//!   Beside the log's lines `/events` carries two named events with no
//!   `id:`, never logged (#389, ruled 5982826097, as #400's surface reads
//!   them): `waiting` `{request, id, command, cwd, reason, segments}`, re-sent
//!   after the history on every connect while it waits, and `answered`
//!   `{request, id}` once the operator decides. The log (v4) records the
//!   decision on the call's own `tool_call` line.
//!
//! # Who may drive it
//!
//! Loopback is not a boundary against the author's own browser: any page they
//! visit can reach `127.0.0.1`, and was measured doing so (R2c critique,
//! finding 1). So every request's `Host` must be one this server answers to,
//! an `Origin`, when present, must be on the allowed list, and a post must be
//! `application/json` -- which a page on another site can only send after a
//! preflight, and the preflight has no route (D17). What this does not stop
//! is another program on the same machine, or another host once the server
//! listens off loopback; that is Basic auth's job (I7, D10). With a
//! [`Credential`] set, every request that passes those checks must also
//! present it, or is `401` -- on every route, and on paths that are none.
//!
//! # One thread per connection
//!
//! A reader holds its thread for as long as it reads, and readers get owned
//! copies of the log (`Session::wait_from`), so none of them can stall the
//! session or another client. Each connection has a read timeout while its
//! request arrives and a write timeout for as long as it is written to, and
//! there is a cap on how many there are at once (D11).
//!
//! How a logged event is written is a parameter, [`Render`] -- the binary
//! passes `session::render`, the log format's own writer (`diet/formats/log`)
//! -- and nothing here reads an event's kind.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::io::{self, Read as _, Write as _};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use super::attach::{self, Attaching, Unattachable};
use super::session::{
    CommandKind, GapEnd, IdleGap, Logged, Refusal, Rejected, Session, Settlement,
};
use super::tool_loop::{Decision, Prompt};
use crate::client::stream::Streaming;
use crate::digest::sha256;
use crate::formats::record::json::{self, Value};

/// How one logged event is written as the `data:` of its server-sent event.
/// It must not contain a line break: one event, one `data:` line.
pub type Render = fn(&Logged) -> String;

/// Write `session`'s log to `out` as each line is appended (`--log`, #157):
/// each line's `render`, then a line break, in one write, flushed -- byte for
/// byte the `data:` text `GET /events` streams, one line each, from the
/// first. The write is the appending thread's own ([`Session::write_through`],
/// #230), so a line is written before its append returns; a kill can tear
/// only the line being written. The first write that fails is handed to
/// `failed`, and nothing is written after it.
pub fn write_through<S: Streaming + 'static>(
    session: &Session<S>,
    render: Render,
    mut out: impl io::Write + Send + 'static,
    mut failed: impl FnMut(io::Error) + Send + 'static,
) {
    let mut broken = false;
    session.write_through(Box::new(move |logged| {
        if broken {
            return;
        }
        let line = render(logged);
        debug_assert!(!line.contains('\n'), "one event, one line: {line}");
        if let Err(why) = out
            .write_all(format!("{line}\n").as_bytes())
            .and_then(|()| out.flush())
        {
            broken = true;
            failed(why);
        }
    }));
}

/// What starting a session's writers emptied that held something -- a
/// previous record, its sidecar, a previous log -- so that a failure after
/// it says so: a previous record lost without a trace is what the first real
/// drive would meet (#264, ruled (i)).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Emptied(Vec<String>);

impl Emptied {
    /// Note that `what` was emptied.
    pub fn push(&mut self, what: String) {
        self.0.push(what);
    }

    /// `why`, and what was already emptied before it, when anything was.
    #[must_use]
    pub fn named(&self, why: String) -> String {
        if self.0.is_empty() {
            why
        } else {
            format!(
                "{why}; already emptied before this failure: {}",
                self.0.join(", ")
            )
        }
    }
}

/// How a server behaves at its edges.
#[derive(Debug, Clone)]
pub struct Config {
    /// How long a stream may be silent before a comment line is sent. The
    /// write is also how a reader that left is noticed.
    pub heartbeat: Duration,
    /// How long a request may take to arrive, head and body together: one
    /// deadline for the whole request, not a bound on each read, so a client
    /// that sends a byte at a time cannot hold its connection open.
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
    /// The Basic credential every request must present. `None` asks for
    /// none, which the binary allows on loopback only.
    pub credential: Option<Credential>,
    /// What an ask's named PNGs are checked against and copied to (#372).
    /// The default has no read scope, and attaches nothing.
    pub attaching: Attaching,
}

/// A Basic credential, `user:password` (D10). Only a digest of what a client
/// sends for it -- the base64 of `user:password` -- is kept, and it is never
/// printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Credential([u8; 32]);

impl Credential {
    /// The credential `user:password`, or nothing when there is no `:`, the
    /// user is empty, or the text holds a control character, which no
    /// browser prompt produces. An empty password is the operator's call.
    #[must_use]
    pub fn basic(user_password: &str) -> Option<Self> {
        let usable = user_password
            .split_once(':')
            .is_some_and(|(user, _)| !user.is_empty())
            && !user_password.chars().any(char::is_control);
        usable.then(|| Self(sha256(base64(user_password.as_bytes()).as_bytes())))
    }

    /// Whether an `Authorization` value presents this credential. The
    /// scheme is compared without case; the token is hashed and all 32 bytes
    /// compared with no early exit, so how long this takes says nothing about
    /// how much of a guess was right.
    fn admits(&self, authorization: Option<&str>) -> bool {
        let Some((scheme, token)) = authorization.and_then(|value| value.split_once(' ')) else {
            return false;
        };
        let presented = sha256(token.trim_start().as_bytes());
        let differs = self
            .0
            .iter()
            .zip(presented)
            .fold(0_u8, |differs, (want, got)| differs | (want ^ got));
        scheme.eq_ignore_ascii_case("basic") && differs == 0
    }
}

impl fmt::Debug for Credential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Credential(<redacted>)")
    }
}

/// Base64, standard alphabet, padded, no line breaks (RFC 4648 section 4):
/// what a browser sends a Basic credential as, and what an attached image's
/// `data:` URI carries (`client::wire`, #372). One encoder, here, where its
/// seeded fault and its RFC vectors already are.
pub(crate) fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let byte = |at: usize| usize::from(chunk.get(at).copied().unwrap_or(0));
        let group = (byte(0) << 16) | (byte(1) << 8) | byte(2);
        for sextet in 0..4 {
            if sextet <= chunk.len() {
                out.push(char::from(ALPHABET[(group >> (18 - 6 * sextet)) & 63]));
            } else {
                out.push('=');
            }
        }
    }
    out
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
            credential: None,
            attaching: Attaching::default(),
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
    /// Stop accepting, and give the open streams up to `grace` to finish:
    /// an `/events` stream closes itself once it has delivered `ended`, so
    /// a page sees the session's last line before the process exits (#291).
    pub fn finish(self, grace: Duration) {
        let live = Arc::clone(&self.live);
        drop(self);
        let until = Instant::now() + grace;
        while live.load(Ordering::SeqCst) > 0 && Instant::now() < until {
            thread::sleep(Duration::from_millis(10));
        }
    }

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
        // Wake the accept loop so it sees the flag. If the wake cannot be
        // sent, the loop is left to end with the process rather than joined:
        // a join on a loop nothing will wake is a hang.
        let woken = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1)).is_ok();
        if let Some(accept) = self.accept.take()
            && woken
        {
            let _ = accept.join();
        }
    }
}

/// The `Host` values this server answers to: loopback at its own port, and
/// each allowed origin's host and port.
fn hosts_for(addr: SocketAddr, origins: &[String]) -> Vec<String> {
    let port = addr.port();
    // The bound address as a browser writes it (`[::1]:PORT` for IPv6), and
    // the loopback names.
    let mut hosts = vec![
        addr.to_string(),
        format!("127.0.0.1:{port}"),
        format!("localhost:{port}"),
    ];
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
    /// When the whole request must have arrived.
    deadline: Instant,
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
fn routes<S: Streaming + 'static>() -> [(&'static str, &'static str, Handler<S>); 3] {
    [
        ("GET", "/events", Serving::<S>::events),
        ("POST", "/commands", Serving::<S>::command),
        ("POST", "/approve", Serving::<S>::approve),
    ]
}

/// The reason phrase for each status this server sends.
const REASONS: &[(u16, &str)] = &[
    (200, "OK"),
    (400, "Bad Request"),
    (401, "Unauthorized"),
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
            let Ok(mut stream) = incoming else {
                // Out of descriptors, most likely: every `accept` fails at
                // once until some are freed, so wait rather than spin.
                thread::sleep(Duration::from_millis(10));
                continue;
            };
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
            .set_write_timeout(Some(self.config.write_timeout))
            .is_err()
        {
            return;
        }
        let deadline = Instant::now() + self.config.read_timeout;
        let request = match read_request(&mut stream, self.config.max_head, deadline) {
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
        if let Some(credential) = &self.config.credential
            && !credential.admits(request.header("authorization"))
        {
            return respond_with(&mut stream, 401, CHALLENGE, &empty());
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
                Ok(seq) => match seq.checked_add(1) {
                    Some(first) => first,
                    None => return respond(stream, 400, &empty()),
                },
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
        // The prompt this stream last showed: a waiting prompt is shown once
        // per stream, after the lines before it.
        let mut shown: Option<Prompt> = None;
        while !self.stopping.load(Ordering::SeqCst) {
            // Once ended, nothing more is logged (#291): a stream past the
            // last line, `ended`, is done, and closes -- at once, not after
            // a heartbeat, which a browser reconnecting after the close
            // would otherwise wait out.
            if self.session.settlement() == Settlement::Ended
                && self.session.events_from(next).is_empty()
            {
                return;
            }
            let (batch, prompt, decided) =
                self.session
                    .wait_for(next, self.config.heartbeat, shown.as_ref());
            let fresh = prompt.is_some() && prompt != shown;
            // The prompt this stream showed is over: `answered` when the
            // operator decided it (#389 5982826097 point 3), before the
            // call's own line.
            let answered = shown.as_ref().filter(|was| {
                prompt.as_ref() != Some(*was)
                    && decided
                        .as_ref()
                        .is_some_and(|(request, id)| *request == was.request && *id == was.id)
            });
            let mut out = String::new();
            if let Some(was) = answered {
                let _ = write!(out, "event: answered\ndata: {}\n\n", was.answered());
            }
            if batch.is_empty() && !fresh && answered.is_none() {
                out.push_str(":\n\n");
            }
            for logged in &batch {
                let data = (self.render)(logged);
                debug_assert!(
                    !data.contains(['\n', '\r']),
                    "a renderer broke an event across lines: {data:?}"
                );
                let _ = write!(out, "id: {}-{}\ndata: {data}\n\n", self.opened, logged.seq);
                next = logged.seq + 1;
            }
            if let Some(waiting) = prompt.as_ref().filter(|_| fresh) {
                let _ = write!(out, "event: waiting\ndata: {}\n\n", waiting.render());
            }
            shown = prompt;
            if stream.write_all(out.as_bytes()).is_err() || stream.flush().is_err() {
                return;
            }
        }
    }

    /// `POST /commands`: one command, answered with what the session did.
    fn command(&self, stream: &mut TcpStream, request: &Request) {
        let Some(object) = self.posted(stream, request) else {
            return;
        };
        let Some(command) = Command::from_object(&object) else {
            return respond(stream, 400, &empty());
        };
        let (status, reply) = self.run(command);
        respond(stream, status, &Value::Object(reply));
    }

    /// `POST /approve`: the operator's answer to the prompt waiting on a
    /// call (#298 point 8). `200` when it answered it; `409` with the
    /// refusal's tag when it did not, and nothing is logged.
    fn approve(&self, stream: &mut TcpStream, request: &Request) {
        let Some(object) = self.posted(stream, request) else {
            return;
        };
        let (Some(Value::String(call)), Some(Value::String(scope)), 2) =
            (object.get("call"), object.get("scope"), object.len())
        else {
            return respond(stream, 400, &empty());
        };
        let Some(decision) = Decision::ALL
            .iter()
            .copied()
            .find(|decision| decision.tag() == scope)
        else {
            return respond(stream, 400, &empty());
        };
        match self.session.approve(call, decision) {
            // 204, as ruled (#389 5982826097 point 4): nothing to say.
            Ok(()) => respond_no_content(stream),
            Err(refusal) => respond(
                stream,
                409,
                &Value::Object(BTreeMap::from([(
                    "refused".to_owned(),
                    Value::String(refusal.tag().to_owned()),
                )])),
            ),
        }
    }

    /// A post's JSON object, or `None` once its refusal is answered: `415`
    /// for a body that is not `application/json`, `400` for a length that is
    /// not one or a body that is not one object, `413` past the cap.
    fn posted(&self, stream: &mut TcpStream, request: &Request) -> Option<BTreeMap<String, Value>> {
        let json_body = request.header("content-type").is_some_and(|kind| {
            kind.split(';')
                .next()
                .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
        });
        if !json_body {
            respond(stream, 415, &empty());
            return None;
        }
        let Some(length) = request
            .header("content-length")
            .and_then(|length| length.trim().parse::<usize>().ok())
        else {
            respond(stream, 400, &empty());
            return None;
        };
        if length > self.config.max_body {
            respond(stream, 413, &empty());
            return None;
        }
        let body = read_body(stream, &request.early_body, length, request.deadline)?;
        let object = std::str::from_utf8(&body)
            .ok()
            .and_then(|text| json::line(text.trim_end()).ok());
        if object.is_none() {
            respond(stream, 400, &empty());
        }
        object
    }

    fn run(&self, command: Posted) -> (u16, BTreeMap<String, Value>) {
        let refused = |because: Refusal| {
            (
                409,
                BTreeMap::from([(
                    "refused".to_owned(),
                    Value::String(because.tag().to_owned()),
                )]),
            )
        };
        let Posted {
            command,
            gap,
            scoping,
        } = command;
        let rejected = |rejection: Rejected| match rejection {
            Rejected::Refused(because) => refused(because),
            // Not a command the session could refuse, or a gap it could not
            // log: either way nothing was logged, and the request was wrong.
            Rejected::NoSuchTurn(_) | Rejected::BadGap(_) => (400, BTreeMap::new()),
        };
        match command {
            Command::Ask(text) => match attach::attached(&text, &self.config.attaching) {
                Err(unattachable) => (400, unattached(&unattachable)),
                Ok(attached) => match self.session.ask_attached(attached, gap, scoping) {
                    Ok(admitted) => (
                        200,
                        BTreeMap::from([
                            ("seq".to_owned(), integer(admitted.seq)),
                            ("turn".to_owned(), integer(u64::from(admitted.turn))),
                        ]),
                    ),
                    Err(rejection) => rejected(rejection),
                },
            },
            Command::Cancel(turn) => match self.session.cancel(turn, gap) {
                Ok(()) => (200, BTreeMap::new()),
                Err(rejection) => rejected(rejection),
            },
            Command::DeclareSeam => match self.session.declare_seam(gap) {
                Ok(()) => (200, BTreeMap::new()),
                Err(rejection) => rejected(rejection),
            },
            Command::End => match self.session.end(gap) {
                Ok(()) => (200, BTreeMap::new()),
                Err(rejection) => rejected(rejection),
            },
        }
    }
}

/// A command as posted, the idle gap it ended, if the surface sent one, and
/// for an ask whether the operator marked it a scoping question (#374):
/// `"scoping": true` on the post.
struct Posted {
    command: Command,
    gap: Option<IdleGap>,
    scoping: bool,
}

/// A command.
enum Command {
    Ask(String),
    Cancel(u32),
    DeclareSeam,
    End,
}

impl Command {
    /// A command from its body: `kind` names it, and every other key must be
    /// one that kind takes.
    fn from_object(object: &BTreeMap<String, Value>) -> Option<Posted> {
        let Some(Value::String(kind)) = object.get("kind") else {
            return None;
        };
        let kind = CommandKind::ALL
            .iter()
            .copied()
            .find(|candidate| candidate.tag() == kind)?;
        // Any command may carry the idle gap it ended (#117, D13 (c)).
        let takes: &[&str] = match kind {
            CommandKind::Ask => &["kind", "text", "idle_gap", "scoping"],
            CommandKind::Cancel => &["kind", "turn", "idle_gap"],
            CommandKind::DeclareSeam | CommandKind::End => &["kind", "idle_gap"],
        };
        if object.keys().any(|key| !takes.contains(&key.as_str())) {
            return None;
        }
        let gap = match object.get("idle_gap") {
            None => None,
            Some(gap) => Some(idle_gap(gap)?),
        };
        // Only an ask takes the operator's mark (`takes`, above), and only as
        // a boolean.
        let scoping = match object.get("scoping") {
            None => false,
            Some(Value::Boolean(mark)) => *mark,
            Some(_) => return None,
        };
        let command = match kind {
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
        }?;
        Some(Posted {
            command,
            gap,
            scoping,
        })
    }
}

/// An `idle_gap` object: exactly the log format's shape for one (Q4),
/// and nothing else. Durations are non-negative integers; `ended_by` is one
/// of its words; `opened_by` is a sequence number.
fn idle_gap(value: &Value) -> Option<IdleGap> {
    const KEYS: [&str; 7] = [
        "opened_by",
        "notice",
        "read",
        "compose",
        "away",
        "blocked",
        "ended_by",
    ];
    let Value::Object(gap) = value else {
        return None;
    };
    if gap.len() != KEYS.len() || gap.keys().any(|key| !KEYS.contains(&key.as_str())) {
        return None;
    }
    let count = |key: &str| match gap.get(key) {
        Some(Value::Integer(number)) => u64::try_from(*number).ok(),
        _ => None,
    };
    let ended_by = match gap.get("ended_by") {
        Some(Value::String(tag)) => GapEnd::ALL.iter().copied().find(|end| end.tag() == tag)?,
        _ => return None,
    };
    Some(IdleGap {
        opened_by: count("opened_by")?,
        notice: count("notice")?,
        read: count("read")?,
        compose: count("compose")?,
        away: count("away")?,
        blocked: count("blocked")?,
        ended_by,
    })
}

/// A `400`'s body for an ask whose named PNG cannot be attached (#372):
/// `{"unattachable": {"path", "check", "reason"}}` -- the path as named, the
/// check it failed, one of [`attach::Check`]'s words, and a sentence. Nothing
/// was logged, copied or sent.
fn unattached(unattachable: &Unattachable) -> BTreeMap<String, Value> {
    BTreeMap::from([(
        "unattachable".to_owned(),
        Value::Object(BTreeMap::from([
            ("path".to_owned(), Value::String(unattachable.path.clone())),
            (
                "check".to_owned(),
                Value::String(unattachable.check.tag().to_owned()),
            ),
            ("reason".to_owned(), Value::String(unattachable.to_string())),
        ])),
    )])
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

/// Read into `buffer`, giving up at `deadline` however the bytes trickle in.
fn read_by(stream: &mut TcpStream, buffer: &mut [u8], deadline: Instant) -> io::Result<usize> {
    let left = deadline
        .checked_duration_since(Instant::now())
        .filter(|left| !left.is_zero())
        .ok_or_else(|| io::Error::from(io::ErrorKind::TimedOut))?;
    stream.set_read_timeout(Some(left))?;
    stream.read(buffer)
}

/// Read a request head, up to `max_head` bytes, by `deadline`.
fn read_request(
    stream: &mut TcpStream,
    max_head: usize,
    deadline: Instant,
) -> Result<Request, Unread> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 4096];
    let end = loop {
        if let Some(end) = find(&buffer, b"\r\n\r\n") {
            break end;
        }
        if buffer.len() > max_head {
            return Err(Unread::TooLarge);
        }
        match read_by(stream, &mut chunk, deadline) {
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
        deadline,
    })
}

/// Read the rest of a body of `length` bytes by `deadline`. Nothing when the
/// client went away or the deadline passed.
fn read_body(
    stream: &mut TcpStream,
    early: &[u8],
    length: usize,
    deadline: Instant,
) -> Option<Vec<u8>> {
    let mut body = early.get(..length.min(early.len()))?.to_vec();
    let mut chunk = [0_u8; 4096];
    while body.len() < length {
        let want = (length - body.len()).min(chunk.len());
        match read_by(stream, &mut chunk[..want], deadline) {
            Ok(0) | Err(_) => return None,
            Ok(read) => body.extend_from_slice(&chunk[..read]),
        }
    }
    Some(body)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// What a `401` asks for: a browser that sees it prompts for the pair.
const CHALLENGE: &str = "WWW-Authenticate: Basic realm=\"diet\"\r\n";

/// Answer with `status` and a JSON body, and close.
fn respond(stream: &mut TcpStream, status: u16, body: &Value) {
    respond_with(stream, status, "", body);
}

/// The same, with `headers` -- each line ending in CRLF -- added.
/// `204 No Content`: a reply with no body at all.
fn respond_no_content(stream: &mut TcpStream) {
    let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n");
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
}

fn respond_with(stream: &mut TcpStream, status: u16, headers: &str, body: &Value) {
    let reason = REASONS
        .iter()
        .find(|(code, _)| *code == status)
        .map_or("", |(_, reason)| reason);
    let mut text = String::new();
    json::render(body, &mut text);
    let reply = format!(
        "HTTP/1.1 {status} {reason}\r\n{headers}Content-Type: application/json\r\n\
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
    use crate::client::transport::TransportFailure;
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
        session.ask("first", None).expect("accepted");
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

        session.ask("second", None).expect("accepted");
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
        session.ask("first", None).expect("accepted");
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
        let (session, server) = serve(Canned::new([]), quick());
        // The last of these is well formed and this process's own, but no
        // event follows the largest sequence number there is.
        let past_the_end = format!("{}-{}", session.opened(), u64::MAX);
        for id in ["20", "abc-2", "1-x", past_the_end.as_str()] {
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
    fn base64_matches_rfc4648() {
        // RFC 4648 section 10's test vectors.
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64(plain.as_bytes()), encoded, "{plain:?}");
        }
    }

    #[test]
    fn a_wrong_credential_is_401_on_every_route() {
        let config = Config {
            credential: Credential::basic("author:s3cret"),
            ..quick()
        };
        assert!(
            !format!("{config:?}").contains(&base64(b"author:s3cret")),
            "the credential was printed"
        );
        let (session, server) = serve(Canned::new([deltas(&["ok"])]), config);
        let ask = r#"{"kind":"ask","text":"hi"}"#;
        let request = |method: &str, path: &str, authorization: &str| {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
                 Content-Length: {}\r\n{authorization}\r\n{ask}",
                host(&server),
                ask.len()
            )
        };
        let wrong = [
            String::new(),
            format!("Authorization: Basic {}\r\n", base64(b"author:guess")),
            format!("Authorization: Bearer {}\r\n", base64(b"author:s3cret")),
            format!("Authorization: Basic{}\r\n", base64(b"author:s3cret")),
            "Authorization: Basic\r\n".to_owned(),
        ];
        let mut paths: Vec<(&str, &str)> = routes::<Canned>()
            .iter()
            .map(|(method, path, _)| (*method, *path))
            .collect();
        paths.push(("GET", "/nowhere"));
        for (method, path) in &paths {
            for authorization in &wrong {
                let reply = Client::send(&server, &request(method, path, authorization)).reply();
                assert_eq!(
                    status(&reply),
                    401,
                    "{method} {path} with {authorization:?}: {reply}"
                );
                assert!(
                    reply.contains("\r\nWWW-Authenticate: Basic realm=\"diet\"\r\n"),
                    "{reply}"
                );
            }
        }
        assert_eq!(asked(&session), 0, "a refused request reached the session");

        // D17 comes first: a rebound host is refused as such, with no
        // challenge that would have a browser prompt a page's visitor.
        let rebound = Client::send(
            &server,
            &format!(
                "GET /events?from=0 HTTP/1.1\r\nHost: rebound.example:{}\r\n\r\n",
                server.addr().port()
            ),
        )
        .reply();
        assert_eq!(status(&rebound), 403, "{rebound}");
        assert!(!rebound.contains("WWW-Authenticate"), "{rebound}");

        // The right pair, with the scheme in any case, is admitted.
        let right = format!("Authorization: basic {}\r\n", base64(b"author:s3cret"));
        let reply = Client::send(&server, &request("POST", "/commands", &right)).reply();
        assert_eq!(status(&reply), 200, "{reply}");
        assert_eq!(asked(&session), 1);
        let mut reader = Client::send(
            &server,
            &events_request(&server, "?from=0", &right.replace("basic", "BASIC")),
        );
        assert!(
            reader.read_until(Duration::from_secs(5), |read| read.contains("Asked")),
            "{}",
            reader.read
        );
        assert_eq!(status(&reader.read), 200, "{}", reader.read);
    }

    #[test]
    fn a_credential_is_a_user_password_pair_with_no_control_character() {
        let credential = Credential::basic("author:s3cret").expect("a usable pair");
        assert_eq!(format!("{credential:?}"), "Credential(<redacted>)");
        assert!(
            Credential::basic("author:").is_some(),
            "an empty password is the author's call"
        );
        for unusable in [
            "",
            "author",
            ":",
            ":s3cret",
            "author:s3cret\n",
            "author:s3\r\ncret",
        ] {
            assert!(
                Credential::basic(unusable).is_none(),
                "{unusable:?} was accepted"
            );
        }
    }

    #[test]
    fn a_request_for_a_rebound_host_name_is_403() {
        let (session, server) = serve(Canned::new([deltas(&["x"])]), quick());
        session.ask("private", None).expect("accepted");
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

        // No Host at all is not a host this server answers to either.
        let hostless = Client::send(&server, "GET /events?from=0 HTTP/1.1\r\n\r\n").reply();
        assert_eq!(status(&hostless), 403, "{hostless}");
        assert!(
            !hostless.contains("data: "),
            "a request with no host read the log"
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

    /// A writer whose bytes the test can read while the session holds it.
    #[derive(Clone, Default)]
    struct Shared(Arc<std::sync::Mutex<Vec<u8>>>);

    impl Shared {
        fn text(&self) -> String {
            String::from_utf8(
                self.0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone(),
            )
            .expect("text")
        }
    }

    impl io::Write for Shared {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn the_written_log_is_every_line_rendered_as_it_is_appended() {
        let session = Session::open(Canned::new([deltas(&["Hel", "lo"])]), template());
        let written = Shared::default();
        write_through(
            &session,
            crate::drive::session::render,
            written.clone(),
            |why| panic!("{why}"),
        );
        session.ask("one", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        assert_eq!(session.end(None), Ok(()));
        let expected: String = session
            .events_from(0)
            .iter()
            .map(|logged| crate::drive::session::render(logged) + "\n")
            .collect();
        assert_eq!(written.text(), expected);
    }

    #[test]
    fn an_appended_line_is_written_before_its_append_returns() {
        // #230, ruled: no signal handler; the log's write is the appending
        // thread's own, so a line a command was answered for is in the file
        // when the answer goes out, and a kill can tear only the line being
        // written. Red against the tee thread this replaced, 5 runs of 5.
        let session = Session::open(Canned::new([deltas(&["Hel", "lo"])]), template());
        let written = Shared::default();
        write_through(
            &session,
            crate::drive::session::render,
            written.clone(),
            |why| panic!("{why}"),
        );
        let admitted = session.ask("one", None).expect("accepted");
        let held = written.text();
        let asked = session.events_from(admitted.seq);
        let line = crate::drive::session::render(&asked[0]);
        assert!(
            held.contains(&line),
            "the ask's line, when ask returned: {held}"
        );
    }

    #[test]
    fn a_failed_log_write_is_handed_over_once_and_nothing_follows_it() {
        struct Refusing(Arc<std::sync::Mutex<u32>>);
        impl io::Write for Refusing {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                *self
                    .0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
                Err(io::Error::other("full"))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let session = Session::open(Canned::new([deltas(&["Hel", "lo"])]), template());
        let tries = Arc::new(std::sync::Mutex::new(0));
        let failures = Arc::new(std::sync::Mutex::new(Vec::new()));
        let told = Arc::clone(&failures);
        write_through(
            &session,
            crate::drive::session::render,
            Refusing(Arc::clone(&tries)),
            move |why| {
                told.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(why.to_string());
            },
        );
        session.ask("one", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        assert_eq!(
            (
                *tries
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
                failures
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            ),
            (1, vec!["full".to_owned()])
        );
    }

    #[test]
    fn a_failure_after_the_writers_names_what_they_emptied() {
        // The `serve` binary's message for a failure after its writers start,
        // `Server::start`'s among them, which cannot be made to fail from
        // outside the process (#264, ruled (i)).
        let mut emptied = Emptied::default();
        assert_eq!(emptied.named("why".to_owned()), "why");
        emptied.push("the previous record at r".to_owned());
        emptied.push("the previous log at l".to_owned());
        assert_eq!(
            emptied.named("the server did not start: x".to_owned()),
            "the server did not start: x; already emptied before this failure: the previous \
             record at r, the previous log at l"
        );
    }

    #[test]
    fn every_data_line_is_one_log_line_and_its_id_is_opened_and_seq() {
        let gate = Gate::new();
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
        let session = Arc::new(Session::open(
            Canned::new([
                deltas(&["Hel", "lo"]),
                vec![Step::Delta("par".to_owned()), Step::Hold(gate.clone())],
                vec![Step::Fail(TransportFailure::Connect("refused".to_owned()))],
                vec![Step::Reject(503, "busy".to_owned())],
            ]),
            template(),
        ));
        let server = Server::start(
            listener,
            Arc::clone(&session),
            quick(),
            crate::drive::session::render,
        )
        .expect("the server starts");
        // Every way a turn ends that a canned transport can play (a crash
        // needs a transport that panics; `drive::session` covers it), and
        // every refusal a session in these states gives.
        session.ask("one", None).expect("accepted");
        wait_until(&session, "the first turn to settle", settled);
        let _ = session.declare_seam(None);
        session.ask("two", None).expect("accepted");
        assert!(gate.wait_for_a_waiter(Duration::from_secs(10)));
        let _ = session.ask("while the second is in flight", None);
        assert_eq!(session.cancel(2, None), Ok(()));
        for turn in 2..=4_usize {
            if turn > 2 {
                session.ask("again", None).expect("accepted");
            }
            wait_until(&session, "the turn to settle", |log| {
                log.iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == turn
                    && settled(log)
            });
        }
        let _ = session.cancel(1, None);
        assert_eq!(session.end(None), Ok(()));
        let _ = session.ask("after the end", None);
        let log = session.events_from(0);

        let mut reader = Client::send(&server, &events_request(&server, "?from=0", ""));
        assert!(
            reader.read_until(Duration::from_secs(5), |read| {
                read.matches("data: ").count() >= log.len()
            }),
            "{:?}",
            reader.read
        );
        let ids: Vec<&str> = reader
            .read
            .lines()
            .filter_map(|line| line.strip_prefix("id: "))
            .collect();
        let data = reader.data();
        assert_eq!(ids.len(), data.len());
        for (id, line) in ids.iter().zip(&data) {
            let read = crate::formats::log::line(line)
                .unwrap_or_else(|why| panic!("{line} is not a log line: {why}"));
            assert_eq!(*id, format!("{}-{}", session.opened(), read.seq), "{line}");
        }
        let document = data.join("\n") + "\n";
        crate::formats::log::parse(&document).expect("the stream is a log the format reads");
    }

    #[test]
    fn a_stream_opened_after_ended_past_its_last_line_closes_at_once() {
        // #315's review: a browser reconnecting after the close asks from
        // past `ended`; it is closed at once, not after a heartbeat.
        let (session, server) = serve(
            Canned::new([]),
            Config {
                heartbeat: Duration::from_secs(30),
                ..quick()
            },
        );
        assert_eq!(session.end(None), Ok(()));
        let past = session.events_from(0).len();
        let mut client = Client::send(
            &server,
            &events_request(&server, &format!("?from={past}"), ""),
        );
        let started = Instant::now();
        client.read_until(Duration::from_secs(5), |_| false);
        assert!(client.closed, "{}", client.read);
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "{:?}",
            started.elapsed()
        );
    }

    #[test]
    fn after_ended_the_written_log_is_sealed_byte_for_byte() {
        // #291, ruled: after `ended` every command is refused and writes
        // nothing -- the log file is the same bytes before and after.
        let session = Session::open(Canned::new([deltas(&["one"])]), template());
        let written = Shared::default();
        write_through(
            &session,
            crate::drive::session::render,
            written.clone(),
            |why| panic!("{why}"),
        );
        session.ask("go", None).expect("accepted");
        wait_until(&session, "the turn to settle", settled);
        assert_eq!(session.end(None), Ok(()));
        let sealed = written.text();
        assert!(session.ask("too late", None).is_err());
        assert!(session.cancel(1, None).is_err());
        assert!(session.declare_seam(None).is_err());
        assert!(session.end(None).is_err());
        assert_eq!(written.text(), sealed);
    }

    #[test]
    fn an_events_stream_closes_itself_once_it_has_delivered_ended() {
        // #291: a page following the log sees `ended`, then the stream ends,
        // while the server itself keeps running -- the close is the stream's
        // own, not the server's stop.
        let (session, server) = serve(Canned::new([]), quick());
        let mut client = Client::send(&server, &events_request(&server, "?from=0", ""));
        assert!(client.read_until(Duration::from_secs(5), |read| { read.contains("Started") }));
        assert_eq!(session.end(None), Ok(()));
        client.read_until(Duration::from_secs(5), |_| false);
        assert!(
            client.closed,
            "the stream stayed open after ended: {}",
            client.read
        );
        assert!(client.read.contains("to: Ended"), "{}", client.read);
        drop(server);
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
        // The two before `end` are logged; the one after is answered and not
        // logged, so `ended` stays the log's last line (#291).
        assert_eq!(
            refusals, 2,
            "a refusal answered over HTTP before the end is not in the log"
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
        session.ask("fill the socket", None).expect("accepted");
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
    fn a_client_that_trickles_a_request_is_let_go_at_one_deadline_for_the_whole_of_it() {
        let config = Config {
            read_timeout: Duration::from_millis(300),
            ..quick()
        };
        let (_session, server) = serve(Canned::new([]), config);
        let mut stream = TcpStream::connect(server.addr()).expect("connects");
        let (gone, let_go) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // A head that never ends, a byte every 50 ms: each read is quick,
            // and the request as a whole never arrives.
            let give_up = Instant::now() + Duration::from_secs(8);
            let mut head = b"GET /events HTTP/1.1\r\nX: "
                .iter()
                .chain(std::iter::repeat(&b'a'));
            while Instant::now() < give_up {
                let byte = *head.next().expect("endless");
                if stream.write_all(&[byte]).is_err() {
                    let _ = gone.send(());
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        assert!(
            let_go.recv_timeout(Duration::from_secs(5)).is_ok(),
            "a client trickling bytes was never let go"
        );
    }

    #[test]
    fn a_head_past_its_cap_is_413() {
        let config = Config {
            max_head: 1024,
            ..quick()
        };
        let (_session, server) = serve(Canned::new([]), config);
        let head = format!(
            "GET /events HTTP/1.1\r\nHost: {}\r\nX: {}",
            host(&server),
            "a".repeat(2048)
        );
        let mut client = Client::send(&server, &head);
        assert!(
            client.read_until(Duration::from_secs(5), |read| read.contains("\r\n\r\n")),
            "no answer to a head past the cap: {:?}",
            client.read
        );
        assert_eq!(status(&client.read), 413, "{}", client.read);
    }

    #[test]
    fn a_body_trickled_in_is_held_to_the_same_deadline_as_its_head() {
        let config = Config {
            read_timeout: Duration::from_millis(300),
            ..quick()
        };
        let (session, server) = serve(Canned::new([]), config);
        let mut stream = TcpStream::connect(server.addr()).expect("connects");
        let head = format!(
            "POST /commands HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Content-Length: 1000\r\n\r\n",
            host(&server)
        );
        stream
            .write_all(head.as_bytes())
            .expect("the head is written");
        let (gone, let_go) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            // The head arrived whole; the body comes a byte every 50 ms.
            let give_up = Instant::now() + Duration::from_secs(8);
            while Instant::now() < give_up {
                if stream.write_all(b" ").is_err() {
                    let _ = gone.send(());
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        assert!(
            let_go.recv_timeout(Duration::from_secs(5)).is_ok(),
            "a client trickling a body was never let go"
        );
        assert_eq!(asked(&session), 0);
    }

    #[test]
    fn a_whole_head_past_its_cap_is_413_even_when_it_arrives_in_one_read() {
        let config = Config {
            max_head: 1024,
            ..quick()
        };
        let (_session, server) = serve(Canned::new([]), config);
        // Terminated, and under one read's size, so the head is found before
        // the running cap is ever consulted: only the check on the head
        // itself can refuse it.
        let head = format!(
            "GET /events HTTP/1.1\r\nHost: {}\r\nX: {}\r\n\r\n",
            host(&server),
            "a".repeat(2048)
        );
        let reply = Client::send(&server, &head).reply();
        assert_eq!(status(&reply), 413, "{reply}");
    }

    #[test]
    fn a_server_bound_on_ipv6_loopback_answers_the_host_a_browser_sends_it() {
        let listener = TcpListener::bind("[::1]:0").expect("IPv6 loopback");
        let session = Arc::new(Session::open(Canned::new([]), template()));
        let server = Server::start(listener, Arc::clone(&session), quick(), render)
            .expect("the server starts");
        let request = format!(
            "GET /events HTTP/1.1\r\nHost: [::1]:{}\r\n\r\n",
            server.addr().port()
        );
        let mut reader = Client::send(&server, &request);
        assert!(
            reader.read_until(Duration::from_secs(5), |read| read.contains("Started")),
            "{:?}",
            reader.read
        );
        assert_eq!(status(&reader.read), 200);
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
    fn a_command_carrying_its_idle_gap_logs_it_and_a_bad_one_is_400() {
        let (session, server) = serve(Canned::new([deltas(&["one"]), deltas(&["two"])]), quick());
        assert_eq!(
            status(&post(&server, r#"{"kind":"ask","text":"a"}"#, "")),
            200
        );
        let log = wait_until(&session, "the turn to settle", settled);
        let settling = log
            .iter()
            .find(|logged| matches!(logged.event, Event::TurnSettled { .. }))
            .map(|logged| logged.seq)
            .expect("the turn settled");
        let before = log.len();
        let gap = |extra: &str| {
            format!(
                r#"{{"kind":"ask","text":"b","idle_gap":{{"opened_by":{settling},"notice":1,"read":2,"compose":3,"away":0,"blocked":0,"ended_by":"ask"{extra}}}}}"#
            )
        };
        // Content in the gap, a duration that is not a count, a word that is
        // not one of `ended_by`'s, a gap that cites something else: each is
        // not the v0 shape (or not a gap the session can log), and nothing
        // is logged for it.
        for bad in [
            gap(r#","text":"what I was about to type""#),
            gap("").replace(r#""read":2"#, r#""read":-2"#),
            gap("").replace(r#""ended_by":"ask""#, r#""ended_by":"timeout""#),
            gap("").replace(&format!(r#""opened_by":{settling}"#), r#""opened_by":1"#),
        ] {
            let reply = post(&server, &bad, "");
            assert_eq!(status(&reply), 400, "{bad}: {reply}");
            assert_eq!(session.events_from(0).len(), before, "{bad} left a line");
        }

        let reply = post(&server, &gap(""), "");
        assert_eq!(status(&reply), 200, "{reply}");
        let log = session.events_from(0);
        assert!(
            matches!(log[before].event, Event::IdleGap(IdleGap { opened_by, .. }) if opened_by == settling),
            "the gap is not the line before the ask: {log:#?}"
        );
        assert!(matches!(
            log[before + 1].event,
            Event::Asked { turn: 2, .. }
        ));
    }

    #[test]
    fn a_malformed_command_is_400_and_not_logged() {
        let (session, server) = serve(Canned::new([]), quick());
        for json in [
            r#"{"kind":"ask"}"#,
            r#"{"kind":"ask","text":"x","also":1}"#,
            r#"{"kind":"launch"}"#,
            r#"{"kind":"cancel","turn":"one"}"#,
            // A turn that was never admitted: not a stop the session can
            // refuse, so it is not a command.
            r#"{"kind":"cancel","turn":5}"#,
            "not json",
        ] {
            let reply = post(&server, json, "");
            assert_eq!(status(&reply), 400, "{json}: {reply}");
        }
        let log = session.events_from(0);
        assert_eq!(log.len(), 1, "a malformed command was logged: {log:#?}");
    }

    /// Point 8 over HTTP: the loop's prompt is an `event: waiting` on
    /// `/events`, not a log line; `POST /approve` answers it, and a refused
    /// answer is `409` with its tag.
    #[test]
    #[allow(clippy::too_many_lines)]
    fn a_waiting_prompt_is_shown_on_events_and_answered_by_post_approve() {
        use crate::drive::session::tests::{bash, lines, looping, tools};
        use crate::drive::tool_loop::Decider;
        use crate::drive::tool_loop::tests::scratch;
        use crate::isolation::Confinement;
        let tree = scratch("serve-approve");
        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
        let session = Arc::new(Session::open_looping(
            Canned::new([vec![bash("call-1", "touch a")], deltas(&["ok"])]),
            looping(),
            None,
            tools(Confinement::Unconfined, &tree, &[], None, Decider::Operator),
        ));
        let server = Server::start(listener, Arc::clone(&session), quick(), render)
            .expect("the server starts");
        let approve = |json: &str| {
            Client::send(
                &server,
                &format!(
                    "POST /approve HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
                     Content-Length: {}\r\n\r\n{json}",
                    host(&server),
                    json.len()
                ),
            )
            .reply()
        };
        assert_eq!(
            body(&approve(r#"{"call":"call-1","scope":"once"}"#)),
            r#"{"refused":"nothing-waiting"}"#
        );
        let mut reader = Client::send(&server, &events_request(&server, "", ""));
        assert_eq!(
            status(&post(&server, r#"{"kind":"ask","text":"go"}"#, "")),
            200
        );
        assert!(
            reader.read_until(Duration::from_secs(10), |read| read
                .contains("event: waiting")),
            "no prompt was shown: {}",
            reader.read
        );
        let shown = reader
            .read
            .split("event: waiting\ndata: ")
            .nth(1)
            .and_then(|rest| rest.split('\n').next())
            .expect("the prompt's data line");
        // The wire shape #400's surface reads (`exercise/src/drive/http.ts`,
        // `#waiting`; ruled at #389 5982826097 point 2): exactly these keys.
        let prompt: serde_json::Value = serde_json::from_str(shown).expect("JSON");
        assert_eq!(
            prompt,
            serde_json::json!({
                "request": prompt["request"].as_u64().expect("a request number"),
                "id": "call-1",
                "command": "touch a",
                "cwd": "~/git/a-worktree",
                "reason": "not_approved",
                "segments": [{
                    "text": "touch a",
                    "program": "touch",
                    "verdict": "prompt",
                    "why": "not_approved",
                }],
            })
        );
        let logged = session.events_from(0).len();
        assert_eq!(
            reader.data().len(),
            logged + 1,
            "every log line, and the prompt beside them"
        );

        assert_eq!(
            status(&approve(r#"{"call":"call-1","scope":"always"}"#)),
            400
        );
        assert_eq!(status(&approve(r#"{"call":"call-1"}"#)), 400);
        let other = approve(r#"{"call":"call-2","scope":"once"}"#);
        assert_eq!(
            (status(&other), body(&other)),
            (409, r#"{"refused":"stale"}"#)
        );
        let taken = approve(r#"{"call":"call-1","scope":"once"}"#);
        assert_eq!((status(&taken), body(&taken)), (204, ""), "{taken}");
        // `answered {request, id}` once decided, before the call's line.
        assert!(
            reader.read_until(Duration::from_secs(10), |read| read
                .contains("\"kind\":\"tool_call\"")
                || read.contains("ToolCalled")),
            "{}",
            reader.read
        );
        let answered = reader
            .read
            .split("event: answered\ndata: ")
            .nth(1)
            .and_then(|rest| rest.split('\n').next())
            .expect("the answered event");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(answered).expect("JSON"),
            serde_json::json!({"request": prompt["request"], "id": "call-1"})
        );
        let (before, after) = reader.read.split_once("event: answered").expect("answered");
        assert!(!before.contains("ToolCalled") && after.contains("ToolCalled"));
        let log = wait_until(&session, "the turn to settle", settled);
        let [line] = lines(&log).try_into().expect("one line");
        assert_eq!(
            line.approval.map(|a| a.scope),
            Some(crate::formats::log::ApprovalScope::Once)
        );
        assert!(tree.join("a").exists());
        let _ = std::fs::remove_dir_all(&tree);
    }

    // -----------------------------------------------------------------------
    // the capture gap's fork, served (#374)
    // -----------------------------------------------------------------------

    /// A trunk answer that reports the warm turn-2 capture's counts
    /// (`e7051ef`), so the projection can write its turn row.
    fn timed(text: &str) -> Vec<Step> {
        vec![
            Step::Delta(text.to_owned()),
            Step::Timings(crate::client::stream::Timings {
                prompt_n: Some(18),
                cache_n: Some(160),
                predicted_n: Some(66),
                ..crate::client::stream::Timings::default()
            }),
        ]
    }

    /// The record a served log projects to, validated: its fork rows off
    /// turn 1, and each capture row's entries.
    fn projected(lines: &[crate::formats::log::Line]) -> (usize, Vec<u32>) {
        use crate::drive::projection::{self, Engine};
        use crate::drive::session::tests::regime;
        use crate::formats::record::{self, Event as Row, Record};

        let projected = projection::project(lines, &regime(), Some(Engine::Commit("e7051ef")))
            .expect("projected");
        let rendered = record::render(&Record {
            events: projected.events.clone(),
        });
        record::parse(&rendered).unwrap_or_else(|why| panic!("{why:?}\n{rendered}"));
        let fork_rows = projected
            .events
            .iter()
            .filter(|row| matches!(row, Row::Fork { of_turn: 1, .. }))
            .count();
        let captures: Vec<u32> = projected
            .events
            .iter()
            .filter_map(|row| match row {
                Row::Capture { entries, .. } => Some(*entries),
                _ => None,
            })
            .collect();
        (fork_rows, captures)
    }

    /// #374's definition of done, against a stub engine that answers the
    /// interview with three decisions: an ask posted `"scoping": true` fires
    /// one fork in its gap and a plain ask after it fires none; `/events`
    /// carries the fork, its call, its settling and its three `patch` lines
    /// as log lines; the served log is one `diet check-log` accepts; the
    /// projection writes one fork row and one capture of three entries; and
    /// side calls per ask -- the receipt's count of `fork` lines -- is 1 for
    /// the scoping turn.
    #[test]
    fn a_scoping_ask_over_http_fires_one_fork_whose_three_patches_project_to_one_capture() {
        use crate::drive::session::tests::{DECIDED, SCOPED, interviewing};
        use crate::formats::log::{self, Event as Line};

        let listener = TcpListener::bind("127.0.0.1:0").expect("loopback");
        let session = Arc::new(Session::open_with(
            Canned::new([timed(SCOPED), deltas(&[DECIDED]), timed("Sure.")]),
            template(),
            None,
            None,
            None,
            Some(interviewing(&[log::Warrant::Scoping])),
        ));
        let server = Server::start(
            listener,
            Arc::clone(&session),
            quick(),
            crate::drive::session::render,
        )
        .expect("the server starts");
        let mut reader = Client::send(&server, &events_request(&server, "?from=0", ""));
        let reply = post(
            &server,
            r#"{"kind":"ask","text":"what are we building?","scoping":true}"#,
            "",
        );
        assert_eq!(status(&reply), 200, "{reply}");
        wait_until(&session, "the fork to settle", |log| {
            settled(log)
                && log
                    .iter()
                    .any(|logged| matches!(logged.event, Event::ForkSettled { .. }))
        });
        let reply = post(&server, r#"{"kind":"ask","text":"thanks"}"#, "");
        assert_eq!(status(&reply), 200, "{reply}");
        let whole = wait_until(&session, "turn 2 to settle", |log| {
            settled(log)
                && log
                    .iter()
                    .any(|logged| matches!(logged.event, Event::TurnSettled { turn: 2, .. }))
        });
        assert!(
            reader.read_until(Duration::from_secs(10), |read| {
                read.lines()
                    .filter(|line| line.starts_with("data: "))
                    .count()
                    >= whole.len()
            }),
            "the stream fell short of the log: {:?}",
            reader.read
        );
        let document: String = reader
            .data()
            .iter()
            .map(|data| data.clone() + "\n")
            .collect();
        // `diet check-log`'s own reader.
        log::project(&document).unwrap_or_else(|why| panic!("{why}\n{document}"));
        let lines = log::parse(&document).expect("the served log reads");
        let forks: Vec<u32> = lines
            .iter()
            .filter_map(|line| match &line.event {
                Line::Fork { of_turn, .. } => Some(*of_turn),
                _ => None,
            })
            .collect();
        assert_eq!(forks, [1], "one fork, after the scoping turn: {document}");
        let patched = lines
            .iter()
            .filter(|line| matches!(line.event, Line::Patch { .. }))
            .count();
        assert_eq!(patched, 3, "{document}");
        let asks = lines
            .iter()
            .filter(|line| matches!(line.event, Line::Ask { .. }))
            .count();
        assert_eq!(asks, 2);

        let (fork_rows, captures) = projected(&lines);
        assert_eq!(fork_rows, 1);
        assert_eq!(captures, [3]);

        // The mark is an ask's, and a boolean: anything else is 400, logged
        // nowhere.
        for malformed in [
            r#"{"kind":"ask","text":"x","scoping":"yes"}"#,
            r#"{"kind":"end","scoping":true}"#,
        ] {
            assert_eq!(status(&post(&server, malformed, "")), 400, "{malformed}");
        }
        assert_eq!(session.events_from(0).len(), whole.len());
    }

    // -----------------------------------------------------------------------
    // the operator's PNG (#372, the loop half)
    // -----------------------------------------------------------------------

    /// A stub engine's reply, captured off a live server: what each turn of
    /// the attachment tests is answered with.
    const REPLY: &[u8] =
        include_bytes!("../../client/fixtures/llama-server-e7051ef-reasoning-stream.http");

    /// A session on a stub engine serving `replies` copies of [`REPLY`],
    /// served with `attaching`, its log written through to `log` when one is
    /// named.
    fn serve_attaching(
        replies: usize,
        attaching: crate::drive::attach::Attaching,
        log: Option<&std::path::Path>,
    ) -> (
        Arc<Session<crate::client::stream::HttpStream>>,
        Server,
        crate::client::stub::Stub,
    ) {
        use crate::client::stream::HttpStream;
        use crate::client::stub::{Act, Stub};
        use crate::client::transport::Endpoint;

        let stub = Stub::serving(vec![Act::Raw(REPLY.to_vec()); replies]).expect("loopback");
        let transport = HttpStream::new(Endpoint::parse(&stub.url()).expect("the stub's URL"));
        let session = Arc::new(Session::open(transport, template()));
        if let Some(log) = log {
            let file = std::fs::File::create(log).expect("the log");
            write_through(&session, crate::drive::session::render, file, |why| {
                panic!("the log could not be written: {why}")
            });
        }
        let server = Server::start(
            TcpListener::bind("127.0.0.1:0").expect("loopback"),
            Arc::clone(&session),
            Config {
                attaching,
                ..quick()
            },
            crate::drive::session::render,
        )
        .expect("the server starts");
        (session, server, stub)
    }

    fn ask_json(text: &str) -> String {
        let mut out = String::new();
        json::render(
            &Value::Object(BTreeMap::from([
                ("kind".to_owned(), Value::String("ask".to_owned())),
                ("text".to_owned(), Value::String(text.to_owned())),
            ])),
            &mut out,
        );
        out
    }

    fn turns_settled<S: Streaming + 'static>(session: &Session<S>, turns: usize) {
        wait_until(session, "the turns to settle", |log| {
            settled(log)
                && log
                    .iter()
                    .filter(|logged| matches!(logged.event, Event::TurnSettled { .. }))
                    .count()
                    == turns
        });
    }

    /// #372's loop half, end to end against a stub engine: the operator
    /// names a PNG in the worktree; the engine receives the ask as
    /// `[image_url, text]` with the file's data URI; the `ask` line names the
    /// copy by reference; the copy is the file's bytes; `diet check-log`'s
    /// reader takes the log; and the next request's head -- the digest the
    /// log records -- still carries the image.
    #[test]
    fn an_operators_png_is_sent_as_an_image_part_logged_by_reference_and_copied() {
        use crate::drive::attach::{
            Attaching,
            tests::{confinement_for, png},
        };
        use crate::drive::tool_loop::tests::scratch;
        use crate::formats::log::{self, Event as Line, RecordedFile};

        let tree = scratch("attach-serve-tree");
        let recording = scratch("attach-serve-recording");
        let bytes = png("the operator's screenshot");
        std::fs::write(tree.join("shot.png"), &bytes).expect("the screenshot");
        let sha256 = crate::digest::sha256_hex(&bytes);
        let data_uri = format!("data:image/png;base64,{}", base64(&bytes));
        let log_path = recording.join("session.jsonl");
        let (session, server, stub) = serve_attaching(
            2,
            Attaching {
                policy: Some(crate::isolation::Policy::merged_usr()),
                worktree: Some(tree.clone()),
                confinement: confinement_for(&crate::isolation::Policy::merged_usr()),
                recording: Some(recording.clone()),
            },
            Some(&log_path),
        );

        let asked = "what is wrong in `shot.png`?";
        let reply = post(&server, &ask_json(asked), "");
        assert_eq!(status(&reply), 200, "{reply}");
        turns_settled(&session, 1);
        let reply = post(&server, &ask_json("and then?"), "");
        assert_eq!(status(&reply), 200, "{reply}");
        turns_settled(&session, 2);
        let heads: Vec<String> = session
            .events_from(0)
            .iter()
            .filter_map(|logged| match &logged.event {
                Event::Requested { head_sha256, .. } => Some(head_sha256.clone()),
                _ => None,
            })
            .collect();
        drop(server);
        drop(session);

        // What the engine received.
        let received = stub.received();
        assert_eq!(received.len(), 2);
        let first: serde_json::Value = serde_json::from_str(&received[0]).expect("JSON");
        assert_eq!(
            first["messages"][1]["content"],
            serde_json::json!([
                {"type": "image_url", "image_url": {"url": data_uri}},
                {"type": "text", "text": asked},
            ]),
            "{}",
            received[0]
        );

        // The log: the reference, never the bytes, on the ask that named it.
        let document = std::fs::read_to_string(&log_path).expect("the log");
        log::project(&document).unwrap_or_else(|why| panic!("{why}\n{document}"));
        assert!(!document.contains(&base64(&bytes)), "inlined: {document}");
        let asks: Vec<Option<Vec<RecordedFile>>> = log::parse(&document)
            .expect("the log reads")
            .into_iter()
            .filter_map(|line| match line.event {
                Line::Ask { files, .. } => Some(files),
                _ => None,
            })
            .collect();
        assert_eq!(
            asks,
            [
                Some(vec![RecordedFile {
                    path: format!("files/{sha256}"),
                    sha256: sha256.clone(),
                    media_type: "image/png".to_owned(),
                    bytes: bytes.len() as u64,
                }]),
                None,
            ]
        );

        // The copy, where the reference says, relative to the log.
        assert_eq!(
            std::fs::read(recording.join("files").join(&sha256)).expect("the copy"),
            bytes
        );

        // The image stays on the trunk: the second request's head -- the
        // opening of what went out, before the new ask -- carries it, and is
        // what the log recorded.
        let second = &received[1];
        let tail = second
            .find(r#",{"role":"user","content":"and then?"}"#)
            .expect("the second ask is the last message");
        let head = &second[..tail];
        assert!(
            head.contains(&data_uri),
            "the image left the trunk: {second}"
        );
        assert_eq!(heads[1], crate::digest::sha256_hex(head.as_bytes()));

        let _ = std::fs::remove_dir_all(&tree);
        let _ = std::fs::remove_dir_all(&recording);
    }

    /// The record of a session the operator's image rode in (#372, the head
    /// rebuild; #458's review NB2): projected with the recording's directory,
    /// every trunk head -- the one the image was sent in and the one after,
    /// which still carries it -- is rebuilt and verified; projected without,
    /// the image turn's head is named unattributed with the reason, never
    /// silently wrong.
    #[test]
    fn a_head_the_operators_image_rode_in_is_rebuilt_from_the_recording() {
        use crate::drive::attach::{
            Attaching,
            tests::{confinement_for, png},
        };
        use crate::drive::projection;
        use crate::drive::tool_loop::tests::scratch;

        let tree = scratch("rebuild-serve-tree");
        let recording = scratch("rebuild-serve-recording");
        std::fs::write(tree.join("shot.png"), png("the operator's screenshot"))
            .expect("the screenshot");
        let log_path = recording.join("session.jsonl");
        let (session, server, _stub) = serve_attaching(
            2,
            Attaching {
                policy: Some(crate::isolation::Policy::merged_usr()),
                worktree: Some(tree.clone()),
                confinement: confinement_for(&crate::isolation::Policy::merged_usr()),
                recording: Some(recording.clone()),
            },
            Some(&log_path),
        );
        for (turn, asked) in [(1, "what is wrong in `shot.png`?"), (2, "and then?")] {
            let reply = post(&server, &ask_json(asked), "");
            assert_eq!(status(&reply), 200, "{reply}");
            turns_settled(&session, turn);
        }
        let lines: Vec<_> = session
            .events_from(0)
            .iter()
            .map(crate::drive::session::line_of)
            .collect();
        drop(server);
        drop(session);
        let regime = crate::drive::session::tests::regime();
        let unrebuilt = |projected: &projection::Projection| -> Vec<String> {
            projected
                .unspellable
                .iter()
                .filter(|u| u.kind == "request" && u.why.contains("could not be rebuilt"))
                .map(|u| u.why.clone())
                .collect()
        };

        let with =
            projection::project_in(&lines, &regime, None, Some(&recording)).expect("projected");
        assert_eq!(unrebuilt(&with), Vec::<String>::new());

        let without = projection::project(&lines, &regime, None).expect("projected");
        let named = unrebuilt(&without);
        assert_eq!(named.len(), 2, "{named:?}");
        assert!(
            named[0].contains("turn 1's attachment: no recording directory"),
            "{named:?}"
        );

        let _ = std::fs::remove_dir_all(&tree);
        let _ = std::fs::remove_dir_all(&recording);
    }

    /// With no `--log` or `--record`, the PNG is attached and sent, nothing
    /// is copied, and the `ask` line names no file.
    #[test]
    fn with_no_recording_an_operators_png_is_sent_and_nothing_is_copied() {
        use crate::drive::attach::{
            Attaching,
            tests::{confinement_for, png},
        };
        use crate::drive::tool_loop::tests::scratch;

        let tree = scratch("attach-serve-unrecorded");
        let bytes = png("unrecorded");
        std::fs::write(tree.join("shot.png"), &bytes).expect("the screenshot");
        let (session, server, stub) = serve_attaching(
            1,
            Attaching {
                policy: Some(crate::isolation::Policy::merged_usr()),
                worktree: Some(tree.clone()),
                confinement: confinement_for(&crate::isolation::Policy::merged_usr()),
                recording: None,
            },
            None,
        );
        let reply = post(&server, &ask_json("see shot.png"), "");
        assert_eq!(status(&reply), 200, "{reply}");
        turns_settled(&session, 1);
        let lines: Vec<_> = session
            .events_from(0)
            .iter()
            .map(crate::drive::session::line_of)
            .collect();
        drop(server);
        drop(session);
        let received = stub.received();
        assert_eq!(received.len(), 1);
        assert!(
            received[0].contains(&format!("data:image/png;base64,{}", base64(&bytes))),
            "{}",
            received[0]
        );
        assert!(
            lines.iter().any(|line| matches!(
                line.event,
                crate::formats::log::Event::Ask { files: None, .. }
            )),
            "{lines:?}"
        );
        let held: Vec<_> = std::fs::read_dir(&tree)
            .expect("the tree")
            .map(|entry| entry.expect("an entry").file_name())
            .collect();
        assert_eq!(held, ["shot.png"], "something was copied into the tree");
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// A named `.png` outside the read scope, inside a secret, missing, or
    /// not a PNG: `400` naming the path and the check, and nothing logged,
    /// copied or sent.
    #[test]
    fn an_ask_naming_a_png_that_fails_a_check_is_400_and_nothing_is_logged_or_sent() {
        use crate::drive::attach::{
            Attaching,
            tests::{confinement_for, png},
        };
        use crate::drive::tool_loop::tests::scratch;

        let tree = scratch("attach-serve-refused-tree");
        let outside = scratch("attach-serve-refused-outside");
        let recording = scratch("attach-serve-refused-recording");
        let secret = outside.join("readable/keys");
        let mut policy = crate::isolation::Policy::merged_usr();
        policy
            .readable
            .push(outside.join("readable").to_string_lossy().into_owned());
        policy.secrets.push(secret.to_string_lossy().into_owned());
        std::fs::create_dir_all(&secret).expect("the secret's directory");
        std::fs::write(outside.join("away.png"), png("away")).expect("written");
        std::fs::write(secret.join("key.png"), png("key")).expect("written");
        std::fs::write(tree.join("text.png"), b"plain text").expect("written");
        // Links in the tree, each judged where it leads (#461 F1, F5).
        std::os::unix::fs::symlink(secret.join("key.png"), tree.join("to-key.png"))
            .expect("a link");
        std::os::unix::fs::symlink(outside.join("away.png"), tree.join("to-away.png"))
            .expect("a link");
        let (session, server, stub) = serve_attaching(
            1,
            Attaching {
                confinement: confinement_for(&policy),
                policy: Some(policy),
                worktree: Some(tree.clone()),
                recording: Some(recording.clone()),
            },
            None,
        );
        for (path, check) in [
            (
                outside.join("away.png").display().to_string(),
                "outside-read-scope",
            ),
            (secret.join("key.png").display().to_string(), "secret"),
            ("gone.png".to_owned(), "missing"),
            ("text.png".to_owned(), "not-png"),
            ("to-key.png".to_owned(), "secret"),
            ("to-away.png".to_owned(), "outside-read-scope"),
        ] {
            let reply = post(&server, &ask_json(&format!("look at {path}")), "");
            assert_eq!(status(&reply), 400, "{path}: {reply}");
            let refused = json::line(body(&reply)).expect("a JSON object");
            let Some(Value::Object(unattachable)) = refused.get("unattachable") else {
                panic!("{path}: {reply}");
            };
            assert_eq!(
                (unattachable.get("path"), unattachable.get("check")),
                (
                    Some(&Value::String(path.clone())),
                    Some(&Value::String(check.to_owned()))
                ),
                "{reply}"
            );
        }
        assert_eq!(session.events_from(0).len(), 1, "something was logged");
        assert!(!recording.join("files").exists(), "something was copied");
        drop(server);
        drop(session);
        assert!(stub.received().is_empty(), "something was sent");
        for dir in [&tree, &outside, &recording] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}
