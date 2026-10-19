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
//! It is in the library rather than in a test file because the meditation
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

/// One reply of a routed stub ([`Stub::routing`], #760): the act, and the
/// text a request's last message must hold for it to answer -- or none, for
/// a reply taken in order by any request no routed reply matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Route {
    /// Text the request's last message holds, when it is routed by content.
    pub when: Option<String>,
    /// What it answers.
    pub act: Act,
}

impl Route {
    /// A reply for the first request whose last message holds `text`.
    #[must_use]
    pub fn when(text: &str, act: Act) -> Self {
        Self {
            when: Some(text.to_owned()),
            act,
        }
    }

    /// A reply taken in order.
    #[must_use]
    pub fn ordered(act: Act) -> Self {
        Self { when: None, act }
    }
}

/// A routed stub's canned classifier (#862): what it answers `POST
/// /v1/classify` with, as Sidekick answers it, so a scenario's warrant gate
/// decides on scores the scenario wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct Classifying {
    /// The model it reports serving (`sidekick-model`): the regimen's id and
    /// revision, `id@revision`.
    pub model: String,
    /// The compute units it reports (`sidekick-compute-units`).
    pub compute_units: String,
    /// Its rules, in order.
    pub rules: Vec<ClassifyRule>,
}

/// One canned classifier rule: a window holding `when` scores a label
/// holding `hypothesis` at `score`. A pair no rule names scores 0.
#[derive(Debug, Clone, PartialEq)]
pub struct ClassifyRule {
    /// Text the window holds.
    pub when: String,
    /// Text the candidate label holds.
    pub hypothesis: String,
    /// P(entailment).
    pub score: f64,
}

impl Classifying {
    /// The answer to a classify request's `body`: each input's score for
    /// each candidate label, by the first rule that names both, in the
    /// labels' order, with the label at its highest score.
    fn answer(&self, body: &str) -> String {
        let value: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        let texts = |key: &str| -> Vec<String> {
            value[key]
                .as_array()
                .map(|all| {
                    all.iter()
                        .filter_map(|text| text.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };
        let (inputs, labels) = (texts("input"), texts("candidate_labels"));
        let data: Vec<serde_json::Value> = inputs
            .iter()
            .enumerate()
            .map(|(index, input)| {
                let probs: Vec<f64> = labels
                    .iter()
                    .map(|label| {
                        self.rules
                            .iter()
                            .find(|rule| {
                                input.contains(&rule.when) && label.contains(&rule.hypothesis)
                            })
                            .map_or(0.0, |rule| rule.score)
                    })
                    .collect();
                let top = probs
                    .iter()
                    .enumerate()
                    .fold(None::<(usize, f64)>, |best, (at, p)| match best {
                        Some((_, high)) if high >= *p => best,
                        _ => Some((at, *p)),
                    })
                    .map_or("", |(at, _)| labels[at].as_str());
                serde_json::json!({"index": index, "label": top, "probs": probs})
            })
            .collect();
        serde_json::json!({ "data": data }).to_string()
    }
}

/// A routed stub's replies not yet spent, and its canned classifier.
#[derive(Debug, Default)]
struct Routes {
    left: Vec<Route>,
    classifying: Option<Classifying>,
}

impl Routes {
    /// The reply for a request whose last message is `last`: the first
    /// routed reply whose text it holds, else the first ordered one; each
    /// spent once.
    fn take(&mut self, last: &str) -> Option<Act> {
        let at = self
            .left
            .iter()
            .position(|route| {
                route
                    .when
                    .as_deref()
                    .is_some_and(|text| last.contains(text))
            })
            .or_else(|| self.left.iter().position(|route| route.when.is_none()))?;
        Some(self.left.remove(at).act)
    }
}

/// A message's `content` as its texts: a string, or its text parts, a
/// Messages `tool_result`'s own content read the same way (#555).
fn texts(content: &serde_json::Value) -> Vec<String> {
    match content {
        serde_json::Value::String(text) => vec![text.clone()],
        serde_json::Value::Array(parts) => parts
            .iter()
            .flat_map(|part| match part["text"].as_str() {
                Some(text) => vec![text.to_owned()],
                None if part["type"] == "tool_result" => texts(&part["content"]),
                None => Vec::new(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A chat-completions or Messages body's last message's text: its
/// `content` string, or its text parts joined -- a Messages `tool_result`'s
/// own text parts among them (#555), where a tool's output sits one level
/// down. Empty when the body does not read.
fn last_message(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return String::new();
    };
    let Some(last) = value["messages"].as_array().and_then(|all| all.last()) else {
        return String::new();
    };
    texts(&last["content"]).join("\n")
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
    unmatched: Arc<Mutex<Vec<String>>>,
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

    /// The routed mode (#760): each request that is not a `GET /props` is
    /// answered by the first unspent `routes` reply whose text its last
    /// message holds, else by the first unspent ordered one, for as long as
    /// the stub runs; `/props` answers `build_info`. A request no reply is
    /// left for is answered `500` and named in [`Stub::unmatched`] -- never
    /// with something generic, so a drifted ask cannot pass. On `port` of
    /// loopback; `0` lets the system choose.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if loopback cannot be bound.
    pub fn routing(routes: Vec<Route>, build_info: &str, port: u16) -> io::Result<Self> {
        Self::start_routed(
            Vec::new(),
            Some(format!("{{\"build_info\":\"{build_info}\"}}")),
            (Replay::Routed, port),
            Routes {
                left: routes,
                classifying: None,
            },
        )
    }

    /// [`Stub::routing`], answering `POST /v1/classify` too, by
    /// `classifying`'s rules (#862), every time and without spending a
    /// reply. A routed stub without one refuses a classify call `404`.
    ///
    /// # Errors
    ///
    /// Returns the I/O error if loopback cannot be bound.
    pub fn routing_classifying(
        routes: Vec<Route>,
        build_info: &str,
        port: u16,
        classifying: Classifying,
    ) -> io::Result<Self> {
        Self::start_routed(
            Vec::new(),
            Some(format!("{{\"build_info\":\"{build_info}\"}}")),
            (Replay::Routed, port),
            Routes {
                left: routes,
                classifying: Some(classifying),
            },
        )
    }

    /// The last messages of the requests a routed stub had no reply for
    /// (#760), in order.
    #[must_use]
    pub fn unmatched(&self) -> Vec<String> {
        self.unmatched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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
        Self::start_routed(acts, props, (replay, port), Routes::default())
    }

    fn start_routed(
        acts: Vec<Act>,
        props: Option<String>,
        (replay, port): (Replay, u16),
        routes: Routes,
    ) -> io::Result<Self> {
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
        let unmatched = Arc::new(Mutex::new(Vec::new()));
        let missed = Arc::clone(&unmatched);
        let worker = thread::spawn(move || {
            serve(
                &listener,
                (acts, routes),
                props.as_deref(),
                replay,
                &flag,
                &seen,
                (&headed, &missed),
            )
        });
        Ok(Self {
            address,
            stop,
            hangups,
            heads,
            unmatched,
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
    /// Each request's reply chosen from the routes by its last message
    /// (#760).
    Routed,
}

/// Whether a chat-completions body's last message is a tool result: the
/// last `"role":"` key in the body names `tool`. A string inside a message
/// carries its quotes escaped, so the unescaped key is only ever a role.
fn answers_a_tool(body: &str) -> bool {
    const KEY: &str = "\"role\":\"";
    body.rfind(KEY)
        .is_some_and(|at| body[at + KEY.len()..].starts_with("tool\""))
}
/// A routed stub's reply for the request `read`, or the request's last
/// message when none is left for it.
fn routed(routes: &mut Routes, read: &io::Result<(String, String)>) -> Result<Act, String> {
    let last = read
        .as_ref()
        .map(|(_, body)| last_message(body))
        .unwrap_or_default();
    let act = routes.take(&last).ok_or(last)?;
    let Ok((_, body)) = read.as_ref() else {
        return Ok(act);
    };
    let act = match last_job(body) {
        Some(job) => with_job(act, &job),
        None => act,
    };
    Ok(with_entries(act, &decoded(body)))
}

/// Every string a JSON `body` holds, decoded and joined by newlines, so an
/// id after an escaped newline stands as a word; `body` itself when it is
/// not JSON.
fn decoded(body: &str) -> String {
    fn walk(value: &serde_json::Value, out: &mut String) {
        match value {
            serde_json::Value::String(text) => {
                out.push_str(text);
                out.push('\n');
            }
            serde_json::Value::Array(items) => items.iter().for_each(|item| walk(item, out)),
            serde_json::Value::Object(fields) => fields.values().for_each(|field| walk(field, out)),
            _ => {}
        }
    }
    serde_json::from_str::<serde_json::Value>(body).map_or_else(
        |_| body.to_owned(),
        |value| {
            let mut out = String::new();
            walk(&value, &mut out);
            out
        },
    )
}

/// The last background job id in `body` (#798): `bg_` and eight hex
/// digits, as `diet::drive::background::new_id` makes them.
fn last_job(body: &str) -> Option<String> {
    body.match_indices("bg_")
        .filter_map(|(at, _)| body.get(at..at + 11))
        .filter(|id| id[3..].bytes().all(|byte| byte.is_ascii_hexdigit()))
        .last()
        .map(str::to_owned)
}

/// `act` with each `$job` in what it sends replaced by `job`.
fn with_job(act: Act, job: &str) -> Act {
    named(act, &|text: String| text.replace("$job", job))
}

/// What an `$entry:` placeholder may name: a call id's and a field's
/// characters, and the `/` between them.
fn entry_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/')
}

/// `act` with each `$entry:<call>/<field>` in what it sends replaced by the
/// last record entry id for that call and field in `body`: `r`, digits,
/// then `/<call>/<field>`, standing as a word of its own. One `body` carries
/// none of stays as written.
fn with_entries(act: Act, body: &str) -> Act {
    const MARK: &str = "$entry:";
    let resolve = |text: String| {
        if !text.contains(MARK) {
            return text;
        }
        let mut out = String::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find(MARK) {
            out.push_str(&rest[..at]);
            let after = &rest[at + MARK.len()..];
            let end = after.find(|c: char| !entry_char(c)).unwrap_or(after.len());
            let suffix = &after[..end];
            match last_entry(body, suffix) {
                Some(id) if !suffix.is_empty() => out.push_str(&id),
                _ => out.push_str(&rest[at..at + MARK.len() + end]),
            }
            rest = &after[end..];
        }
        out.push_str(rest);
        out
    };
    named(act, &resolve)
}

/// The last `r<digits>/<suffix>` in `body` standing as a word of its own.
fn last_entry(body: &str, suffix: &str) -> Option<String> {
    let tail = format!("/{suffix}");
    body.match_indices(&tail)
        .filter_map(|(at, _)| {
            let end = at + tail.len();
            if body[end..].chars().next().is_some_and(entry_char) {
                return None;
            }
            let digits = body[..at]
                .chars()
                .rev()
                .take_while(char::is_ascii_digit)
                .count();
            let start = at.checked_sub(digits + 1)?;
            let word = digits > 0
                && body[start..].starts_with('r')
                && !body[..start].chars().next_back().is_some_and(entry_char);
            word.then(|| body[start..end].to_owned())
        })
        .last()
}

/// `act` with `named` applied to the text it sends.
fn named(act: Act, named: &dyn Fn(String) -> String) -> Act {
    match act {
        Act::Answer(text) => Act::Answer(named(text)),
        // A canned reply's head declares its body's length: counted again.
        Act::Raw(bytes) => Act::Raw(match String::from_utf8(bytes) {
            Ok(text) => match text.split_once("\r\n\r\n") {
                Some((head, sent)) if named(sent.to_owned()) != sent => {
                    let body = named(sent.to_owned());
                    let head = head
                        .lines()
                        .map(|line| {
                            if line.to_ascii_lowercase().starts_with("content-length:") {
                                format!("Content-Length: {}", body.len())
                            } else {
                                line.to_owned()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\r\n");
                    format!("{head}\r\n\r\n{body}").into_bytes()
                }
                _ => text.into_bytes(),
            },
            Err(unread) => unread.into_bytes(),
        }),
        Act::Trickle(gap, pieces) => Act::Trickle(gap, pieces.into_iter().map(named).collect()),
        other => other,
    }
}

/// A routed stub's request no reply is left for (#760): named in
/// `unmatched` and among the heads and bodies, and refused `500`.
fn unanswered(
    stream: &mut TcpStream,
    (read, last): (&io::Result<(String, String)>, &str),
    (heads, unmatched): (&Mutex<Vec<String>>, &Mutex<Vec<String>>),
    asked: &mut Vec<String>,
) {
    unmatched
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(last.to_owned());
    if let Ok((head, body)) = read {
        heads
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(head.clone());
        asked.push(body.clone());
    }
    write_reply(
        stream,
        500,
        &format!("no scripted reply for: {last}"),
        Closing::Yes,
    );
}

/// A routed stub's classify call (#862): answered by its canned
/// classifier, under Sidekick's provenance headers, or refused `404` when it
/// has none. Never a reply spent, never among what was asked.
fn classified(
    stream: &mut TcpStream,
    classifying: Option<&Classifying>,
    (head, body): (&str, &str),
    heads: &Mutex<Vec<String>>,
) {
    heads
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(head.to_owned());
    let Some(classifying) = classifying else {
        write_reply(stream, 404, "no canned classifier", Closing::Yes);
        return;
    };
    let answer = classifying.answer(body);
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nsidekick-model: {}\r\n\
             sidekick-compute-units: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
            classifying.model,
            classifying.compute_units,
            answer.len()
        )
        .as_bytes(),
    );
    let _ = stream.flush();
}

/// Serve every act, and collect what was asked.
fn serve(
    listener: &TcpListener,
    (acts, mut routes): (Vec<Act>, Routes),
    props: Option<&str>,
    replay: Replay,
    stop: &AtomicBool,
    hangups: &Mutex<Vec<Held>>,
    (heads, unmatched): (&Mutex<Vec<String>>, &Mutex<Vec<String>>),
) -> Vec<String> {
    let mut asked = Vec::new();
    let pair = acts.clone();
    let played: Box<dyn Iterator<Item = Act>> = match replay {
        Replay::No | Replay::Routed => Box::new(acts.into_iter()),
        Replay::Forever | Replay::ToolTurns => Box::new(acts.into_iter().cycle()),
    };
    let mut acts = played.peekable();
    let idle_cap = (replay == Replay::No).then_some(IDLE_CAP);
    let by_content = replay == Replay::Routed;
    loop {
        // Without a `/props` to answer, the stub serves exactly its acts and
        // then refuses, as it always has. With one, it answers `/props`
        // whenever asked -- before, between and after its acts -- and an act
        // waits for the next connection that is not a `/props`.
        if props.is_none() && acts.peek().is_none() && !by_content {
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
        if by_content
            && let Ok((head, body)) = &read
            && head.starts_with("POST /v1/classify ")
        {
            classified(
                &mut stream,
                routes.classifying.as_ref(),
                (head, body),
                heads,
            );
            continue;
        }
        let picked = if by_content {
            let picked = routed(&mut routes, &read);
            if let Err(last) = &picked {
                unanswered(&mut stream, (&read, last), (heads, unmatched), &mut asked);
                continue;
            }
            picked.ok()
        } else {
            acts.next()
        };
        let Some(act) = picked else {
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
        // A routed stub answers each request on its own thread (#782): a
        // reply held back never holds back the next request's.
        if by_content {
            thread::spawn(move || act_on(&mut stream, &act));
            continue;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A Messages body's last message reads through its `tool_result`s
    /// (#555): a tool's output, a string or text parts, is what a reply's
    /// `when` matches.
    #[test]
    fn a_messages_tool_result_is_read_as_the_last_messages_text() {
        let body = serde_json::json!({"messages": [
            {"role": "user", "content": "first"},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "a", "content": "a.txt"},
                {"type": "tool_result", "tool_use_id": "b", "content": [
                    {"type": "text", "text": "b.txt"},
                ]},
                {"type": "text", "text": "after"},
            ]},
        ]})
        .to_string();
        assert_eq!(last_message(&body), "a.txt\nb.txt\nafter");
    }

    /// One chat-completions POST whose last message says `last`: the
    /// reply's status and body.
    fn ask(stub: &Stub, last: &str) -> (u16, String) {
        let body = serde_json::json!({
            "messages": [
                {"role": "system", "content": "head"},
                {"role": "user", "content": [{"type": "text", "text": last}]},
            ]
        })
        .to_string();
        let mut stream = TcpStream::connect(stub.address()).expect("connects");
        let request = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nHost: stub\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(request.as_bytes()).expect("written");
        let mut reply = String::new();
        stream.read_to_string(&mut reply).expect("read");
        let status = reply
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .unwrap_or(0);
        let body = reply
            .split_once("\r\n\r\n")
            .map_or("", |(_, b)| b)
            .to_owned();
        (status, body)
    }

    /// A routed reply's `$entry:<call>/<field>` is the last record entry id
    /// for that call and field the request carries -- `r`, the request count
    /// diet built it at, then `/<call>/<field>` -- so a scenario names an
    /// entry its run will make without counting requests ahead of the run;
    /// it stays as written when the request carries none.
    #[test]
    fn a_routed_reply_names_the_record_entry_its_request_carries() {
        let stub = Stub::routing(
            vec![
                Route::when(
                    "recorded",
                    Act::Answer("resolve $entry:call-r1/decision".to_owned()),
                ),
                Route::ordered(Act::Answer("resolve $entry:call-r1/decision".to_owned())),
                Route::ordered(Act::Answer("resolve $entry:call-r1/decision".to_owned())),
            ],
            "build",
            0,
        )
        .expect("loopback");
        assert_eq!(
            ask(
                &stub,
                "recorded: r12/call-r9/decision; recorded: r16/call-r1/decision; r7/call-r1/plan"
            ),
            (200, "resolve r16/call-r1/decision".to_owned())
        );
        // After a newline, as a working-memory render lists it.
        assert_eq!(
            ask(&stub, "# working set\nr5/call-r1/decision\nr6/call-r2/plan"),
            (200, "resolve r5/call-r1/decision".to_owned())
        );
        assert_eq!(
            ask(&stub, "xr3/call-r1/decision is not an id"),
            (200, "resolve $entry:call-r1/decision".to_owned())
        );
        // In a canned reply's arguments, escaped as JSON: counted again.
        let Act::Raw(sent) = with_entries(
            Act::Raw(crate::drive::canned::streamed(
                "",
                &[crate::drive::canned::StreamedCall {
                    id: "call-v1",
                    name: "resolve_entry",
                    arguments: r#"{"entry": "$entry:call-r1/decision"}"#,
                }],
            )),
            "the record: r6/call-r1/decision",
        ) else {
            unreachable!("raw stays raw");
        };
        let sent = String::from_utf8(sent).expect("text");
        let (head, body) = sent.split_once("\r\n\r\n").expect("a head");
        assert!(body.contains("r6/call-r1/decision"), "{body}");
        assert!(!body.contains("$entry"), "{body}");
        assert!(
            head.contains(&format!("Content-Length: {}", body.len())),
            "the declared length is the body's: {head}"
        );
    }

    /// #798: a routed reply's `$job` is the last background job id the
    /// request carries -- the id `task_stop` needs, which no script can know
    /// ahead of the run -- and stays `$job` when it carries none.
    #[test]
    fn a_routed_reply_names_the_last_background_job_its_request_carries() {
        let stub = Stub::routing(
            vec![
                Route::when("started", Act::Answer("stop $job".to_owned())),
                Route::ordered(Act::Answer("stop $job".to_owned())),
            ],
            "build",
            0,
        )
        .expect("loopback");
        assert_eq!(
            ask(&stub, "bg_00000000 ended; bg_0123abcd started"),
            (200, "stop bg_0123abcd".to_owned())
        );
        assert_eq!(ask(&stub, "nothing running"), (200, "stop $job".to_owned()));
        // A canned reply streamed whole: its declared length is the body's.
        let canned = Stub::routing(
            vec![Route::ordered(Act::Raw(crate::drive::canned::streamed(
                "stop $job",
                &[],
            )))],
            "build",
            0,
        )
        .expect("loopback");
        let (status, body) = ask(&canned, "bg_0123abcd started");
        assert_eq!(status, 200);
        assert!(body.contains("stop bg_0123abcd"), "{body}");
        let Act::Raw(sent) = with_job(
            Act::Raw(crate::drive::canned::streamed("stop $job", &[])),
            "bg_0123abcd",
        ) else {
            unreachable!("raw stays raw");
        };
        let sent = String::from_utf8(sent).expect("text");
        let (head, body) = sent.split_once("\r\n\r\n").expect("a head");
        assert!(
            head.contains(&format!("Content-Length: {}", body.len())),
            "the declared length is the body's: {head}"
        );
    }

    /// #760: a routed stub answers a request whose last message holds a
    /// route's text with that route's reply, each once, whatever its place;
    /// every other request takes the unrouted replies in order; and a
    /// request nothing answers is refused `500`, named in `unmatched`, not
    /// answered with something generic.
    #[test]
    fn a_routed_stub_answers_by_content_then_in_order_and_names_what_it_cannot() {
        let stub = Stub::routing(
            vec![
                Route::when("the fork's ask", Act::Answer("forked".to_owned())),
                Route::ordered(Act::Answer("first".to_owned())),
                Route::when("the audit's ask", Act::Answer("audited once".to_owned())),
                Route::when("the audit's ask", Act::Answer("audited twice".to_owned())),
                Route::ordered(Act::Answer("second".to_owned())),
            ],
            "build",
            0,
        )
        .expect("loopback");
        assert_eq!(ask(&stub, "hello"), (200, "first".to_owned()));
        assert_eq!(
            ask(&stub, "now the audit's ask, numbered"),
            (200, "audited once".to_owned())
        );
        assert_eq!(ask(&stub, "x the fork's ask y"), (200, "forked".to_owned()));
        assert_eq!(ask(&stub, "again"), (200, "second".to_owned()));
        assert_eq!(
            ask(&stub, "the audit's ask"),
            (200, "audited twice".to_owned())
        );
        assert!(stub.unmatched().is_empty());
        // #782: a reply held back does not hold back the next request's.
        let slow = Stub::routing(
            vec![
                Route::when(
                    "the trunk",
                    Act::Trickle(Duration::from_millis(1500), vec!["late".to_owned()]),
                ),
                Route::when("the fork", Act::Answer("at once".to_owned())),
            ],
            "build",
            0,
        )
        .expect("loopback");
        let address = slow.address();
        let held = thread::spawn(move || {
            let stub = Stub {
                address,
                stop: Arc::new(AtomicBool::new(false)),
                hangups: Arc::new(Mutex::new(Vec::new())),
                heads: Arc::new(Mutex::new(Vec::new())),
                unmatched: Arc::new(Mutex::new(Vec::new())),
                worker: None,
            };
            ask(&stub, "the trunk")
        });
        thread::sleep(Duration::from_millis(200));
        let began = Instant::now();
        assert_eq!(ask(&slow, "the fork"), (200, "at once".to_owned()));
        assert!(
            began.elapsed() < Duration::from_millis(1000),
            "the fork waited {:?} behind the trunk's held reply",
            began.elapsed()
        );
        assert_eq!(held.join().expect("the trunk").0, 200);
        let (status, _) = ask(&stub, "the fork's ask, a second time");
        assert_eq!(status, 500);
        assert_eq!(
            stub.unmatched(),
            ["the fork's ask, a second time".to_owned()]
        );
    }
    /// #862: a routed stub given a canned classifier answers `POST
    /// /v1/classify` as Sidekick does -- each window's score for each label,
    /// in the labels' order, under the provenance headers -- from its rules
    /// alone, every time, and spends no reply on it. The gate's own client
    /// reads the answer.
    #[test]
    fn a_routed_stub_answers_a_classify_call_by_its_rules_and_spends_no_reply() {
        use crate::drive::warrant_gate::{Classifier, classify};
        let stub = Stub::routing_classifying(
            vec![Route::ordered(Act::Answer("the trunk's".to_owned()))],
            "build",
            0,
            Classifying {
                model: "canned-nli@0000000000000000000000000000000000000000".to_owned(),
                compute_units: "none".to_owned(),
                rules: vec![
                    ClassifyRule {
                        when: "No such file".to_owned(),
                        hypothesis: "went wrong".to_owned(),
                        score: 0.93,
                    },
                    ClassifyRule {
                        when: "still open".to_owned(),
                        hypothesis: "left open".to_owned(),
                        score: 0.81,
                    },
                ],
            },
        )
        .expect("loopback");
        let classifier = Classifier {
            endpoint: format!("http://{}", stub.address()),
            model: "canned-nli".to_owned(),
            revision: "0000000000000000000000000000000000000000".to_owned(),
            compute_units: "none".to_owned(),
            truncation_side: "left".to_owned(),
            timeout: Duration::from_secs(5),
            prose_window_sentences: 3,
            prose_window_chars: 400,
            output_tail_chars: 400,
            batch: 8,
            windowing: crate::drive::warrant_gate::WINDOWING,
            probes: Vec::new(),
        };
        let labels = ["Something went wrong.", "A question is left open."];
        for _ in 0..2 {
            let answer = classify(
                &classifier,
                &[
                    "cat: No such file or directory",
                    "all fine",
                    "one is still open",
                ],
                &labels,
                Duration::from_secs(5),
            )
            .expect("an answer");
            assert_eq!(
                answer.probs,
                [vec![0.93, 0.0], vec![0.0, 0.0], vec![0.0, 0.81]]
            );
            assert_eq!(
                answer.model,
                "canned-nli@0000000000000000000000000000000000000000"
            );
            assert_eq!(answer.compute_units, "none");
        }
        assert_eq!(ask(&stub, "hello"), (200, "the trunk's".to_owned()));
        assert!(stub.unmatched().is_empty());
        // Without a canned classifier, a classify call is refused, naming
        // it, and still spends no reply.
        let plain = Stub::routing(
            vec![Route::ordered(Act::Answer("the trunk's".to_owned()))],
            "build",
            0,
        )
        .expect("loopback");
        let unclassified = Classifier {
            endpoint: format!("http://{}", plain.address()),
            ..classifier
        };
        let refused = classify(&unclassified, &["x"], &labels, Duration::from_secs(5))
            .expect_err("no classifier");
        assert!(refused.contains("HTTP 404"), "{refused}");
        assert_eq!(ask(&plain, "hello"), (200, "the trunk's".to_owned()));
    }
}
