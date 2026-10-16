//! An MCP server over `drive serve`'s operator commands (#758): a stdio
//! process a Claude app or Claude Code session attaches, so an agent drives
//! a served session through tools instead of a browser pane.
//!
//! It speaks JSON-RPC 2.0, one message a line, on its standard input and
//! output (MCP's stdio transport), and answers `initialize`, `ping`,
//! `tools/list` and `tools/call`. Each tool is one request to the running
//! `serve` at `--url`, with the `--auth-file` credential when one is given,
//! and returns serve's status and body as they came -- a refusal and its
//! `refused` tag included. No tool reaches anything serve does not expose:
//! `attach` reads the local file it is named, and uploads it to `POST
//! /files`, as the page's own picker does.
//!
//! | tool | what it calls |
//! | --- | --- |
//! | `status`, `wait` | `GET /state`, `GET /wait` (#756) |
//! | `ask` | `POST /commands` `ask` |
//! | `attach` | `POST /files` |
//! | `approve` | `POST /approve` |
//! | `cancel`, `end` | `POST /commands` |
//! | `memory` | `GET /memory` (#756) |
//! | `edit`, `flag` | `POST /commands` `edit-entry`, `flag-entry` |
//! | `declare_meditation`, `refill_anyway`, `ratify_phase` | `POST /commands` |
//! | `open_tangent`, `close_tangent` | `POST /commands` |
//! | `events` | `GET /events`, the log lines after a seq |

use std::fmt::Write as _;
use std::io::{BufRead, Read as _, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

use serde_json::{Map, Value, json};

/// The MCP protocol versions this server speaks, newest first: a client
/// asking for one of them gets it, any other the newest.
const PROTOCOLS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

/// How long a request to serve may take: past `/wait`'s cap of 300 s.
const PATIENCE: Duration = Duration::from_secs(330);

/// How long one read of `events`' stream waits before it looks at its
/// deadline again.
const QUIET: Duration = Duration::from_millis(300);

/// How long `events` reads toward the log's end before it answers with what
/// it has (#780).
const EVENTS_PATIENCE: Duration = Duration::from_secs(60);

/// How many log lines `events` returns unless told.
const EVENTS_LIMIT: u64 = 200;

/// A running `serve`: where it listens, and the credential it wants.
#[derive(Debug, Clone)]
pub struct Serve {
    /// `host:port`.
    address: String,
    /// `Basic` and the base64 of `user:password`, when serve wants one.
    authorization: Option<String>,
}

/// One of serve's answers: its status, and its body -- JSON as it came,
/// a body that is not JSON as text, none as an empty object.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    /// The HTTP status.
    pub status: u16,
    /// The body.
    pub body: Value,
}

impl Answer {
    fn json(&self) -> Value {
        json!({"status": self.status, "body": self.body})
    }
}

impl Serve {
    /// The `serve` at `url` -- `http://host:port`, or `host:port` -- with
    /// `credential`, the auth file's `user:password`, when it wants one.
    ///
    /// # Errors
    ///
    /// When `url` is not plain HTTP to a host and port.
    pub fn new(url: &str, credential: Option<&str>) -> Result<Self, String> {
        let address = url.strip_prefix("http://").unwrap_or(url);
        let address = address.strip_suffix('/').unwrap_or(address);
        if address.is_empty() || address.contains('/') || !address.contains(':') {
            return Err(format!(
                "`{url}` is not serve's address: give http://HOST:PORT, as its `listening`"
            ));
        }
        Ok(Self {
            address: address.to_owned(),
            authorization: credential
                .map(|credential| format!("Basic {}", super::serve::base64(credential.as_bytes()))),
        })
    }

    /// One request, its answer read to the end (serve closes each
    /// connection after its answer).
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<(&str, &[u8])>,
    ) -> Result<Answer, String> {
        let mut stream = self.connect()?;
        stream
            .set_read_timeout(Some(PATIENCE))
            .map_err(|why| why.to_string())?;
        self.send(&mut stream, method, path, body)?;
        let mut reply = Vec::new();
        stream
            .read_to_end(&mut reply)
            .map_err(|why| format!("serve's answer could not be read: {why}"))?;
        parsed(&reply)
    }

    fn connect(&self) -> Result<TcpStream, String> {
        TcpStream::connect(&self.address)
            .map_err(|why| format!("serve at {} could not be reached: {why}", self.address))
    }

    fn send(
        &self,
        stream: &mut TcpStream,
        method: &str,
        path: &str,
        body: Option<(&str, &[u8])>,
    ) -> Result<(), String> {
        let mut head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n",
            self.address
        );
        if let Some(authorization) = &self.authorization {
            let _ = write!(head, "Authorization: {authorization}\r\n");
        }
        if let Some((content_type, bytes)) = body {
            let _ = write!(
                head,
                "Content-Type: {content_type}\r\nContent-Length: {}\r\n",
                bytes.len()
            );
        }
        head.push_str("\r\n");
        stream
            .write_all(head.as_bytes())
            .and_then(|()| stream.write_all(body.map_or(&[][..], |(_, bytes)| bytes)))
            .map_err(|why| format!("serve could not be sent the request: {why}"))
    }

    fn get(&self, path: &str) -> Result<Answer, String> {
        self.request("GET", path, None)
    }

    fn post(&self, path: &str, body: &Value) -> Result<Answer, String> {
        // One line, no `null`: serve reads a body by the record's grammar.
        let text = body.to_string();
        self.request("POST", path, Some(("application/json", text.as_bytes())))
    }

    fn command(&self, body: &Value) -> Result<Answer, String> {
        self.post("/commands", body)
    }

    /// The log lines after `after`, of `kinds` when given, at most `limit`:
    /// `GET /events` read up to the log's end as `/state` names it -- its
    /// `last_seq` -- or until `limit` lines are found, or the stream ends.
    ///
    /// NOT UNTIL IT GOES QUIET (#780). Serve sends its head, then renders
    /// the backlog before the first event; on a long log that took longer
    /// than the quiet spell this once waited for, so `events` read the head
    /// alone and answered nothing, after an early seq only -- a late one has
    /// little to render.
    fn events(&self, after: Option<u64>, kinds: &[String], limit: u64) -> Result<Value, String> {
        let from = after.map_or(0, |after| after + 1);
        let state = self.get("/state")?;
        let Some(last) = state.body.get("last_seq").and_then(Value::as_u64) else {
            return Ok(state.json());
        };
        if from > last {
            return Ok(json!({"status": 200, "events": [], "more": false}));
        }
        let mut stream = self.connect()?;
        self.send(&mut stream, "GET", &format!("/events?from={from}"), None)?;
        stream
            .set_read_timeout(Some(QUIET))
            .map_err(|why| why.to_string())?;
        let until = Instant::now() + EVENTS_PATIENCE;
        let mut pending: Vec<u8> = Vec::new();
        let mut head: Option<u16> = None;
        let mut lines = Vec::new();
        let mut more = false;
        let mut reached = false;
        let mut chunk = vec![0_u8; 64 * 1024];
        while !reached && !more {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => pending.extend_from_slice(&chunk[..n]),
                Err(why)
                    if matches!(
                        why.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    if Instant::now() >= until {
                        break;
                    }
                    continue;
                }
                Err(why) => return Err(format!("serve's events could not be read: {why}")),
            }
            if head.is_none() {
                let Some(end) = pending.windows(4).position(|window| window == b"\r\n\r\n") else {
                    continue;
                };
                let status = status_of(&String::from_utf8_lossy(&pending[..end]))?;
                pending.drain(..end + 4);
                if status != 200 {
                    // An answer, not a stream: read it to its end.
                    let _ = stream.read_to_end(&mut pending);
                    return Ok(Answer {
                        status,
                        body: body_of(&pending),
                    }
                    .json());
                }
                head = Some(status);
            }
            // Each complete event, ended by a blank line; a named event
            // (`waiting`, `answered`) carries no `id:` and is not a log line.
            while let Some(end) = pending.windows(2).position(|window| window == b"\n\n") {
                let event: Vec<u8> = pending.drain(..end + 2).collect();
                let event = String::from_utf8_lossy(&event);
                if !event.lines().any(|line| line.starts_with("id:")) {
                    continue;
                }
                let Some(line) = event
                    .lines()
                    .find_map(|line| line.strip_prefix("data: "))
                    .and_then(|data| serde_json::from_str::<Value>(data).ok())
                else {
                    continue;
                };
                if line.get("seq").and_then(Value::as_u64) >= Some(last) {
                    reached = true;
                }
                let kind = line.get("kind").and_then(Value::as_str).unwrap_or_default();
                if kinds.is_empty() || kinds.iter().any(|wanted| wanted == kind) {
                    if lines.len() as u64 >= limit {
                        more = true;
                        break;
                    }
                    lines.push(line);
                }
                if reached {
                    break;
                }
            }
        }
        if head.is_none() {
            return Err("serve's events answer had no head".to_owned());
        }
        Ok(json!({"status": 200, "events": lines, "more": more}))
    }
}

/// An answer's status and body, from its raw bytes.
fn parsed(reply: &[u8]) -> Result<Answer, String> {
    let split = reply
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or("serve's answer had no head")?;
    let head = String::from_utf8_lossy(&reply[..split]);
    Ok(Answer {
        status: status_of(&head)?,
        body: body_of(&reply[split + 4..]),
    })
}

fn status_of(head: &str) -> Result<u16, String> {
    head.lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| "serve's answer had no status".to_owned())
}

fn body_of(bytes: &[u8]) -> Value {
    if bytes.iter().all(u8::is_ascii_whitespace) {
        return json!({});
    }
    serde_json::from_slice(bytes)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(bytes).into_owned()))
}

/// `text` safe in a query string.
fn query(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            _ => format!("%{byte:02X}"),
        })
        .collect()
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    let mut tool = Map::new();
    tool.insert("name".to_owned(), json!(name));
    tool.insert("description".to_owned(), json!(description));
    tool.insert("inputSchema".to_owned(), schema);
    Value::Object(tool)
}

fn string(description: &str) -> Value {
    json!({"type": "string", "description": description})
}

fn integer(description: &str) -> Value {
    json!({"type": "integer", "minimum": 0, "description": description})
}

fn object(properties: Value, required: &[&str]) -> Value {
    let mut schema = Map::new();
    schema.insert("type".to_owned(), json!("object"));
    schema.insert("properties".to_owned(), properties);
    schema.insert("required".to_owned(), json!(required));
    schema.insert("additionalProperties".to_owned(), Value::Bool(false));
    Value::Object(schema)
}

fn none() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

/// The tools, as `tools/list` names them.
#[must_use]
pub fn tools() -> Value {
    let mut all = lifecycle_tools();
    all.extend(first_tools());
    all.extend(later_tools());
    all.extend(phase_tools());
    all.extend(last_tools());
    Value::Array(all)
}

/// The state, the ask, and the turn's commands.
fn first_tools() -> Vec<Value> {
    vec![
        tool(
            "status",
            "The session's state now (GET /state): its settlement, the turn and how it stands, \
             what is in flight, a held meditation, a waiting approval, the last refusal, \
             last_seq, and whether it is idle.",
            none(),
        ),
        tool(
            "wait",
            "Wait until the session is idle, or until a log line of a kind appears after a seq \
             (GET /wait). Returns the state, timed_out, and the matching line as event.",
            object(
                json!({
                    "until": string("`idle`, or a log line's kind, such as `turn.settled`."),
                    "after": integer("Only a line after this seq: pass the last state's last_seq to wait for something new."),
                    "timeout": integer("Seconds; serve's default is 30 and its cap 300.")
                }),
                &["until"],
            ),
        ),
        tool(
            "ask",
            "Send the operator's next message (the `ask` command) and wait for the turn: until \
             the session is idle, holds a meditation, waits on an approval, or has ended, or \
             the timeout passes. Returns why it returned, the last state, and a digest of what \
             the turn did -- the final answer, tool calls, forks and audits with their patches, \
             meditations, refusals, and the phase's proposals and rulings. A model's phase \
             proposal ends its turn: the ask returns `proposal`, the proposal in the digest, \
             for `rule_phase`. With wait false, returns at once with its seq and turn, or \
             serve's refusal.",
            object(
                json!({
                    "text": string("The message."),
                    "scoping": {"type": "boolean", "description": "Mark the ask as scoping."},
                    "files": {
                        "type": "array",
                        "items": {"type": "string"},
                        "description": "The sha256 of each file `attach` uploaded, in order."
                    },
                    "wait": {"type": "boolean", "description": "Wait for the turn: true unless set false, when the ask returns at once with its seq and turn."},
                    "timeout": integer("Seconds to wait for the turn; 1800 when omitted.")
                }),
                &["text"],
            ),
        ),
        tool(
            "attach",
            "Upload a local PNG to serve (POST /files) for a later ask. Returns its sha256 and \
             size.",
            object(
                json!({"path": string("The PNG's absolute path.")}),
                &["path"],
            ),
        ),
        tool(
            "approve",
            "Answer the approval a command waits on (POST /approve): `once`, `session`, \
             `workspace`, or `decline`.",
            object(
                json!({
                    "call": string("The waiting call's id, as status's approval names it."),
                    "scope": {"type": "string", "enum": ["once", "session", "workspace", "decline"]}
                }),
                &["call", "scope"],
            ),
        ),
        tool(
            "cancel",
            "Cancel the turn in flight (the `cancel` command).",
            object(
                json!({"turn": integer("The turn; the current one, from status, when omitted.")}),
                &[],
            ),
        ),
        tool("end", "End the session (the `end` command).", none()),
    ]
}

/// Working memory, its meditations and tangents, and the log.
fn later_tools() -> Vec<Value> {
    vec![
        tool(
            "memory",
            "The session's working memory (GET /memory): each live entry's id, content, state, \
             provenances and the operator's edit or flag no meditation has addressed yet; with \
             all, every entry, voided, resolved, retired and parked ones too.",
            object(json!({"all": {"type": "boolean"}}), &[]),
        ),
        tool(
            "edit",
            "Replace a live entry's text as the operator (the `edit-entry` command).",
            object(
                json!({"id": string("The entry's id."), "content": string("Its new text.")}),
                &["id", "content"],
            ),
        ),
        tool(
            "flag",
            "Flag a live entry with the operator's note (the `flag-entry` command).",
            object(
                json!({"id": string("The entry's id."), "note": string("The note.")}),
                &["id", "note"],
            ),
        ),
        tool(
            "declare_meditation",
            "Declare a meditation (the `declare-meditation` command), optionally moving to a \
             phase.",
            object(json!({"phase": string("The phase to move to.")}), &[]),
        ),
        tool(
            "refill_anyway",
            "Refill from a held meditation as it stands (the `refill-anyway` command).",
            none(),
        ),
    ]
}

/// A phase's two keys (#788, #790): the operator's proposal, the operator's
/// move alone, and a ruling on the model's.
fn phase_tools() -> Vec<Value> {
    vec![
        tool(
            "propose_phase",
            "Propose a move to another phase as the operator (the `propose-phase` command, \
             #788): the model is asked on its next ask, and accepts or declines. With meditate \
             (true unless set false) an accepted move goes through a meditation.",
            object(
                json!({
                    "to": string("The phase to move to."),
                    "meditate": {"type": "boolean", "description": "Meditate on the move; true when omitted."}
                }),
                &["to"],
            ),
        ),
        tool(
            "move_phase",
            "Move to another phase as the operator alone (the `move-phase` command, #788), \
             asking the model nothing; refused `not-unilateral` when the regimen sets \
             `operator_unilateral = false`. With meditate (false unless set true) the move goes \
             through a meditation.",
            object(
                json!({
                    "to": string("The phase to move to."),
                    "meditate": {"type": "boolean", "description": "Meditate on the move; false when omitted."}
                }),
                &["to"],
            ),
        ),
        tool(
            "rule_phase",
            "Rule on the model's pending phase proposal (the `ratify-phase` command, #788): \
             `meditation` meditates and moves, the suggested choice; `keep` moves without \
             meditating; `stay` stays. Weigh the evidence `status` shows on the proposal.",
            object(
                json!({
                    "choice": {"type": "string", "enum": ["meditation", "keep", "stay"]},
                    "call": string("The proposal's call id; the pending one when omitted.")
                }),
                &["choice"],
            ),
        ),
    ]
}

/// Tangents, and the log.
fn last_tools() -> Vec<Value> {
    vec![
        tool(
            "open_tangent",
            "Open a tangent (the `open-tangent` command).",
            object(json!({"id": string("The tangent's id.")}), &["id"]),
        ),
        tool(
            "close_tangent",
            "Close the open tangent (the `close-tangent` command), saying of each entry it \
             recorded whether to keep, drop or park it.",
            object(
                json!({
                    "dispositions": {
                        "type": "object",
                        "additionalProperties": {"type": "string", "enum": ["keep", "drop", "park"]},
                        "description": "Each entry's id and its disposition."
                    }
                }),
                &["dispositions"],
            ),
        ),
        tool(
            "events",
            "The session's log lines after a seq (GET /events), of the kinds named, at most a \
             limit; `more` says whether there were others.",
            object(
                json!({
                    "after": integer("Only lines after this seq."),
                    "kinds": {"type": "array", "items": {"type": "string"}, "description": "Only these kinds."},
                    "limit": integer("At most this many lines; 200 when omitted.")
                }),
                &[],
            ),
        ),
    ]
}

/// A tool's arguments, read.
struct Args<'a>(&'a Map<String, Value>);

impl Args<'_> {
    fn string(&self, key: &str) -> Result<&str, String> {
        self.0
            .get(key)
            .and_then(Value::as_str)
            .ok_or_else(|| format!("`{key}` is required, a string"))
    }

    fn optional_string(&self, key: &str) -> Result<Option<&str>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text)),
            Some(_) => Err(format!("`{key}` must be a string")),
        }
    }

    fn optional_count(&self, key: &str) -> Result<Option<u64>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .map(Some)
                .ok_or_else(|| format!("`{key}` must be a non-negative integer")),
        }
    }

    fn optional_bool(&self, key: &str) -> Result<Option<bool>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(flag)) => Ok(Some(*flag)),
            Some(_) => Err(format!("`{key}` must be a boolean")),
        }
    }

    fn optional_strings(&self, key: &str) -> Result<Option<Vec<String>>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| format!("`{key}` must be a list of strings"))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Some),
            Some(_) => Err(format!("`{key}` must be a list of strings")),
        }
    }
}

/// The `ask` command `args` describe.
fn ask_body(args: &Args<'_>) -> Result<Value, String> {
    let mut body = json!({"kind": "ask", "text": args.string("text")?});
    if let Some(scoping) = args.optional_bool("scoping")? {
        body["scoping"] = Value::Bool(scoping);
    }
    if let Some(files) = args.optional_strings("files")? {
        body["files"] = json!(files);
    }
    Ok(body)
}

/// The log kinds that say how the session's phase stands (#790).
const PHASE_KINDS: &[&str] = &[
    "session.start",
    "phase.started",
    "phase.proposed",
    "phase.answered",
    "phase.ruled",
];

/// The phase the session is in and the proposal pending, if any, from its
/// log lines (#790, after #788's two keys): the opening phase, then each
/// `phase.started` and each ruling that moved; a `phase.proposed` not yet
/// ruled on -- or, the operator's, answered -- is pending, with the evidence
/// it carries.
fn phase_of(lines: &[Value]) -> Value {
    let mut current = Value::Null;
    let mut pending: Option<Value> = None;
    for line in lines {
        match line.get("kind").and_then(Value::as_str).unwrap_or_default() {
            "session.start" => {
                if let Some(opening) = line.get("opening_phase") {
                    current = opening.clone();
                }
            }
            "phase.started" => current = line.get("phase").cloned().unwrap_or(Value::Null),
            "phase.proposed" => pending = Some(line.clone()),
            "phase.answered" => {
                // A declined answer ends the operator's proposal; an accepted
                // one is followed by the ruling that moves.
                if line.get("accepted").and_then(Value::as_bool) == Some(false) {
                    pending = None;
                }
            }
            "phase.ruled" => {
                pending = None;
                if line.get("choice").and_then(Value::as_str) != Some("stay")
                    && let Some(to) = line.get("to")
                {
                    current = to.clone();
                }
            }
            _ => {}
        }
    }
    json!({"current": current, "pending": pending.unwrap_or(Value::Null)})
}

/// `status` (#790): `/state`, with the phase and a pending proposal and its
/// evidence -- which `/state` does not carry -- read from the log.
fn status(serve: &Serve) -> Result<Value, String> {
    let state = serve.get("/state")?;
    let kinds: Vec<String> = PHASE_KINDS.iter().map(|kind| (*kind).to_owned()).collect();
    let lines = serve.events(None, &kinds, u64::MAX)?;
    let lines = lines["events"].as_array().cloned().unwrap_or_default();
    let mut answered = state.json();
    answered["phase"] = phase_of(&lines);
    Ok(answered)
}

/// `propose_phase`, `move_phase` or `rule_phase` (#790): the operator's
/// proposal, the operator's move alone, or a ruling on the model's -- the
/// pending one when no call is named.
fn phase_command(serve: &Serve, name: &str, args: &Args<'_>) -> Result<Answer, String> {
    if name == "propose_phase" || name == "move_phase" {
        let kind = if name == "move_phase" {
            "move-phase"
        } else {
            "propose-phase"
        };
        let mut body = json!({"kind": kind, "to": args.string("to")?});
        if let Some(meditate) = args.optional_bool("meditate")? {
            body["meditate"] = Value::Bool(meditate);
        }
        return serve.command(&body);
    }
    let call = match args.optional_string("call")? {
        Some(call) => call.to_owned(),
        None => {
            pending_model_proposal(serve)?.ok_or("no proposal of the model's awaits a ruling")?
        }
    };
    serve.command(&json!({
        "kind": "ratify-phase",
        "call": call,
        "choice": args.string("choice")?
    }))
}

/// The call of the model's proposal awaiting the operator's ruling.
fn pending_model_proposal(serve: &Serve) -> Result<Option<String>, String> {
    let kinds: Vec<String> = PHASE_KINDS.iter().map(|kind| (*kind).to_owned()).collect();
    let lines = serve.events(None, &kinds, u64::MAX)?;
    let lines = lines["events"].as_array().cloned().unwrap_or_default();
    let pending = &phase_of(&lines)["pending"];
    Ok((pending.get("by").and_then(Value::as_str) == Some("model"))
        .then(|| {
            pending
                .get("call")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .flatten())
}

/// Tool `name` called with `arguments` against `serve`: its result as JSON
/// text, or why it could not be called.
///
/// # Errors
///
/// An unknown tool, an argument of the wrong shape, or serve unreachable.
pub fn call(serve: &Serve, name: &str, arguments: &Map<String, Value>) -> Result<Value, String> {
    let args = Args(arguments);
    let answer = match name {
        "status" => return status(serve),
        "wait" => {
            let mut path = format!("/wait?until={}", query(args.string("until")?));
            if let Some(after) = args.optional_count("after")? {
                let _ = write!(path, "&after={after}");
            }
            if let Some(timeout) = args.optional_count("timeout")? {
                let _ = write!(path, "&timeout={timeout}");
            }
            serve.get(&path)?
        }
        "ask" => serve.command(&ask_body(&args)?)?,
        "attach" => {
            let path = args.string("path")?;
            let bytes =
                std::fs::read(path).map_err(|why| format!("{path} could not be read: {why}"))?;
            serve.request("POST", "/files", Some(("image/png", &bytes)))?
        }
        "approve" => serve.post(
            "/approve",
            &json!({"call": args.string("call")?, "scope": args.string("scope")?}),
        )?,
        "cancel" => {
            let turn = if let Some(turn) = args.optional_count("turn")? {
                turn
            } else {
                let state = serve.get("/state")?;
                match state.body.pointer("/turn/number").and_then(Value::as_u64) {
                    Some(turn) => turn,
                    None => return Ok(state.json()),
                }
            };
            serve.command(&json!({"kind": "cancel", "turn": turn}))?
        }
        "end" => serve.command(&json!({"kind": "end"}))?,
        "memory" => serve.get(if args.optional_bool("all")? == Some(true) {
            "/memory?all"
        } else {
            "/memory"
        })?,
        "edit" => serve.command(&json!({
            "kind": "edit-entry",
            "id": args.string("id")?,
            "content": args.string("content")?
        }))?,
        "flag" => serve.command(&json!({
            "kind": "flag-entry",
            "id": args.string("id")?,
            "note": args.string("note")?
        }))?,
        "declare_meditation" => {
            let mut body = json!({"kind": "declare-meditation"});
            if let Some(phase) = args.optional_string("phase")? {
                body["phase"] = json!(phase);
            }
            serve.command(&body)?
        }
        "refill_anyway" => serve.command(&json!({"kind": "refill-anyway"}))?,
        "propose_phase" | "move_phase" | "rule_phase" => phase_command(serve, name, &args)?,
        "open_tangent" => {
            serve.command(&json!({"kind": "open-tangent", "id": args.string("id")?}))?
        }
        "close_tangent" => {
            let dispositions = arguments
                .get("dispositions")
                .filter(|value| value.is_object())
                .ok_or("`dispositions` is required, an object")?;
            serve.command(&json!({"kind": "close-tangent", "dispositions": dispositions}))?
        }
        "events" => {
            return serve.events(
                args.optional_count("after")?,
                &args.optional_strings("kinds")?.unwrap_or_default(),
                args.optional_count("limit")?.unwrap_or(EVENTS_LIMIT),
            );
        }
        other => return Err(format!("no tool is named `{other}`")),
    };
    Ok(answer.json())
}

/// How long `start` waits for serve's first line: its engine check and
/// warmup come before it.
const START_PATIENCE: Duration = Duration::from_secs(120);

/// How long `stop` waits for a serve it started to log `ended` after `end`:
/// one that has not by then is killed. One that has is writing its record,
/// and is waited for however long that takes (#879).
const STOP_PATIENCE: Duration = Duration::from_secs(30);

/// How long `ask` waits for its turn unless told (#772): 30 minutes.
const ASK_PATIENCE: u64 = 30 * 60;

/// How long each `GET /wait` an `ask` makes lasts: short enough that a
/// pending approval -- which `/state` does not count idle -- is seen
/// promptly, and progress goes out between them.
const WAIT_SLICE: u64 = 5;

/// Why a waiting `ask` returned (#772), from a `/state` or `/wait` body:
/// `None` while the turn goes on.
fn settled_as(state: &Value) -> Option<&'static str> {
    if state.get("session").and_then(Value::as_str) == Some("ended")
        || state.get("settlement").and_then(Value::as_str) == Some("ended")
    {
        return Some("ended");
    }
    if state.get("approval").is_some() {
        return Some("approval");
    }
    if state.pointer("/meditation/state").and_then(Value::as_str) == Some("held") {
        return Some("held");
    }
    (state.get("idle").and_then(Value::as_bool) == Some(true)).then_some("idle")
}

/// What a turn did, from its log lines (#772): the final answer on the
/// trunk, each tool call and its outcome, each fork -- audits included --
/// with its outcome and the patches it wrote, meditations, refusals, and
/// how the turn settled.
fn digest(lines: &[Value]) -> Value {
    let text = |line: &Value, key: &str| line.get(key).cloned().unwrap_or(Value::Null);
    let kind = |line: &Value| {
        line.get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    let mut lane_of = std::collections::BTreeMap::new();
    let mut answer = Value::Null;
    let mut tool_calls = Vec::new();
    let mut forks: Vec<Value> = Vec::new();
    let mut meditations = Vec::new();
    let mut refusals = Vec::new();
    let mut settled = Vec::new();
    let mut phase = Vec::new();
    let mut proposal = Value::Null;
    for line in lines {
        let seq = line.get("seq").and_then(Value::as_u64);
        match kind(line).as_str() {
            "request" => {
                if let (Some(seq), Some(lane)) = (seq, line.get("lane").and_then(Value::as_str)) {
                    lane_of.insert(seq, lane.to_owned());
                }
            }
            "response" => {
                let to = line.get("to_request").and_then(Value::as_u64);
                if to.and_then(|to| lane_of.get(&to)).map(String::as_str) == Some("trunk") {
                    answer = text(line, "text");
                }
            }
            "tool_call" => tool_calls.push(json!({
                "seq": seq,
                "name": text(line, "name"),
                "arguments": text(line, "arguments"),
                "outcome": text(line, "outcome"),
                "exit": text(line, "exit"),
            })),
            "fork" => forks.push(json!({
                "seq": seq,
                "lane": text(line, "lane"),
                "why": text(line, "why"),
                "patches": [],
            })),
            "fork.settled" | "patch" => {
                let fork = line.get("fork").and_then(Value::as_u64);
                if let Some(entry) = forks
                    .iter_mut()
                    .find(|entry| entry["seq"].as_u64() == fork && fork.is_some())
                {
                    if kind(line) == "patch" {
                        if let Some(patches) = entry["patches"].as_array_mut() {
                            patches.push(
                                json!({"op": text(line, "op"), "entry": text(line, "entry")}),
                            );
                        }
                    } else {
                        entry["outcome"] = text(line, "outcome");
                    }
                }
            }
            "meditation" | "meditation.held" => {
                let mut kept = line.clone();
                if let Some(fields) = kept.as_object_mut() {
                    for big in ["render", "outputs", "background"] {
                        fields.remove(big);
                    }
                }
                meditations.push(kept);
            }
            "refused" => refusals.push(line.clone()),
            // The phase's two keys (#788, #790): every proposal, answer,
            // ruling and start, and the model's proposal itself -- which ends
            // its turn and waits on the operator.
            "phase.proposed" | "phase.answered" | "phase.ruled" | "phase.started" => {
                if kind(line) == "phase.proposed"
                    && line.get("by").and_then(Value::as_str) == Some("model")
                {
                    proposal = line.clone();
                }
                phase.push(line.clone());
            }
            "turn.settled" => {
                settled.push(json!({"turn": text(line, "turn"), "reason": text(line, "reason")}));
            }
            _ => {}
        }
    }
    json!({
        "answer": answer,
        "tool_calls": tool_calls,
        "forks": forks,
        "meditations": meditations,
        "refusals": refusals,
        "settled": settled,
        "phase": phase,
        "proposal": proposal,
    })
}

/// `ask`, waiting for its turn (#772): the ask sent, then `GET /wait` in
/// [`WAIT_SLICE`]s -- a progress notification between them -- until the
/// session is idle, holds a meditation, waits on an approval, or has ended,
/// or `timeout` seconds pass. Returns the ask's answer, why it returned,
/// the last state, and a digest of the log after the ask.
fn ask_and_wait(
    serve: &Serve,
    body: &Value,
    timeout: u64,
    progress: &mut dyn FnMut(u64, &str),
) -> Result<Value, String> {
    let asked = serve.command(body)?;
    let Some(seq) = asked
        .body
        .get("seq")
        .and_then(Value::as_u64)
        .filter(|_| asked.status == 200)
    else {
        return Ok(asked.json());
    };
    let until = Instant::now() + Duration::from_secs(timeout);
    let mut round = 0;
    let (returned, state) = loop {
        let left = until.saturating_duration_since(Instant::now()).as_secs();
        let slice = left.min(WAIT_SLICE);
        let waited = serve.get(&format!("/wait?until=idle&after={seq}&timeout={slice}"))?;
        if waited.status != 200 {
            return Ok(json!({"asked": asked.body, "wait": waited.json()}));
        }
        if let Some(why) = settled_as(&waited.body) {
            break (why, waited.body);
        }
        if left <= slice {
            break ("timed_out", waited.body);
        }
        round += 1;
        let turn = waited
            .body
            .pointer("/turn/state")
            .and_then(Value::as_str)
            .unwrap_or("running");
        progress(round, &format!("turn {}: {turn}", asked.body["turn"]));
    };
    let lines = serve.events(Some(seq.saturating_sub(1)), &[], u64::MAX)?;
    let lines = lines["events"].as_array().cloned().unwrap_or_default();
    let digest = digest(&lines);
    // The model proposed a phase move, which ends its turn (#788): the turn
    // waits on the operator's ruling, and says so (#790).
    let returned = if returned == "idle" && !digest["proposal"].is_null() {
        "proposal"
    } else {
        returned
    };
    Ok(json!({
        "asked": asked.body,
        "returned": returned,
        "timed_out": returned == "timed_out",
        "state": state,
        "digest": digest,
    }))
}

/// A serve this server started (#717).
#[derive(Debug)]
struct Started {
    child: std::process::Child,
}

/// What the server drives: the serve it is attached to, if any, and the
/// one it started, if it started it (#717).
#[derive(Debug)]
pub struct Driver {
    serve: Option<Serve>,
    started: Option<Started>,
    /// This program, which `start` runs as `serve`.
    exe: std::path::PathBuf,
    /// The per-user config `start` reads its profile from.
    config: Option<std::path::PathBuf>,
    /// Where a profile's `~` points.
    home: Option<std::path::PathBuf>,
    /// How long `stop` waits for a serve it started: [`STOP_PATIENCE`].
    stop_patience: Duration,
}

/// A credential file's `user:password`, one trailing newline taken off.
///
/// # Errors
///
/// When it cannot be read.
pub fn credential_in(path: &str) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|why| format!("the auth file {path} could not be read: {why}"))?;
    Ok(text.strip_suffix('\n').map_or(text.clone(), |line| {
        line.strip_suffix('\r').unwrap_or(line).to_owned()
    }))
}

impl Driver {
    /// A driver attached to `serve` when one is given, starting serves as
    /// `exe` from profiles in `config`, their `~` at `home`.
    #[must_use]
    pub fn new(
        serve: Option<Serve>,
        exe: std::path::PathBuf,
        config: Option<std::path::PathBuf>,
        home: Option<std::path::PathBuf>,
    ) -> Self {
        Self {
            serve,
            started: None,
            exe,
            config,
            home,
            stop_patience: STOP_PATIENCE,
        }
    }

    /// `start` (#717): `serve --profile NAME` launched, its first line read,
    /// and the server attached to the address it prints, with the profile's
    /// auth file. A refused start is answered with what serve said.
    fn start(&mut self, profile: &str) -> Result<Value, String> {
        use std::io::BufRead as _;
        if let Some(serve) = &self.serve {
            return Err(format!(
                "already driving the serve at {}: `stop` it first",
                serve.address
            ));
        }
        let mut flags = vec!["--profile".to_owned(), profile.to_owned()];
        if let Some(config) = &self.config {
            flags.extend(["--config".to_owned(), config.to_string_lossy().into_owned()]);
        }
        let resolved =
            super::profile::with_profile(&flags, self.config.clone(), self.home.as_deref())?;
        let credential = resolved
            .chunks(2)
            .find(|pair| pair[0] == "--auth-file")
            .map(|pair| credential_in(&pair[1]))
            .transpose()?;
        let mut child = std::process::Command::new(&self.exe)
            .arg("serve")
            .args(&flags)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|why| format!("serve could not be started: {why}"))?;
        let stdout = child.stdout.take().ok_or("serve's output was not piped")?;
        let (said, first) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut lines = std::io::BufReader::new(stdout).lines();
            let _ = said.send(lines.next());
            // The rest -- the record's report at the end -- read and let go,
            // so serve never blocks on a full pipe.
            lines.for_each(drop);
        });
        let line = match first.recv_timeout(START_PATIENCE) {
            Ok(Some(Ok(line))) => line,
            outcome => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match outcome {
                    Err(_) => format!("serve said nothing in {}s", START_PATIENCE.as_secs()),
                    Ok(_) => "serve exited without a word".to_owned(),
                });
            }
        };
        let announced: Value =
            serde_json::from_str(&line).unwrap_or_else(|_| Value::String(line.clone()));
        let Some(listening) = announced.get("listening").and_then(Value::as_str) else {
            // A refused start: serve said why, and exits.
            let exited = child.wait().ok().and_then(|status| status.code());
            return Ok(json!({"started": false, "said": announced, "exited": exited}));
        };
        let pid = child.id();
        match Serve::new(listening, credential.as_deref()) {
            Ok(serve) => self.serve = Some(serve),
            Err(why) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(why);
            }
        }
        self.started = Some(Started { child });
        Ok(json!({"started": true, "pid": pid, "said": announced}))
    }

    /// `stop` (#717): the session ended, and a serve this server started
    /// waited for, so nothing is left behind; the server is then attached to
    /// nothing. Once serve has logged `ended` it is writing its record --
    /// projected from the whole log, which on a long session takes minutes
    /// -- and it is waited for with no limit (#879); one that has neither
    /// exited nor logged `ended` in [`STOP_PATIENCE`] is killed.
    fn stop(&mut self) -> Result<Value, String> {
        let Some(serve) = self.serve.take() else {
            return Err("not driving a serve: `start` a profile, or attach with --url".to_owned());
        };
        let ended = serve.command(&json!({"kind": "end"}));
        let mut stopped = json!({});
        if let Ok(ended) = &ended {
            stopped = ended.json();
        }
        if let Some(Started { mut child }) = self.started.take() {
            let until = Instant::now() + self.stop_patience;
            let mut ended = false;
            let mut asked = Instant::now();
            let exited = loop {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        break status.code().map_or(json!("by signal"), |code| json!(code));
                    }
                    Ok(None) if ended || Instant::now() < until => {
                        // Whether it has logged `ended`, asked twice a second.
                        if !ended && asked.elapsed() >= Duration::from_millis(500) {
                            asked = Instant::now();
                            if serve.get("/state").is_ok_and(|state| {
                                state.body.get("session").and_then(Value::as_str) == Some("ended")
                            }) {
                                ended = true;
                            }
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break json!("killed");
                    }
                }
            };
            stopped["exited"] = exited;
        }
        if let Err(why) = ended {
            stopped["end"] = json!(why);
        }
        Ok(stopped)
    }

    /// Tool `name`, `start` and `stop` here, the rest against the serve
    /// attached.
    fn call(
        &mut self,
        name: &str,
        arguments: &Map<String, Value>,
        progress: &mut dyn FnMut(u64, &str),
    ) -> Result<Value, String> {
        let args = Args(arguments);
        match name {
            "start" => {
                let mut started = self.start(args.string("profile")?)?;
                // The phase it starts in (#790, #788's `start-phase`), before
                // any ask.
                if let (Some(phase), Some(serve)) = (args.optional_string("phase")?, &self.serve) {
                    started["phase"] = serve
                        .command(&json!({"kind": "start-phase", "phase": phase}))?
                        .json();
                }
                Ok(started)
            }
            "stop" => self.stop(),
            // `ask` waits for its turn unless told not to (#772).
            "ask" if args.optional_bool("wait")? != Some(false) => match &self.serve {
                Some(serve) => ask_and_wait(
                    serve,
                    &ask_body(&args)?,
                    args.optional_count("timeout")?.unwrap_or(ASK_PATIENCE),
                    progress,
                ),
                None => {
                    Err("not driving a serve: `start` a profile, or attach with --url".to_owned())
                }
            },
            _ => match &self.serve {
                Some(serve) => call(serve, name, arguments),
                None => {
                    Err("not driving a serve: `start` a profile, or attach with --url".to_owned())
                }
            },
        }
    }
}

impl Drop for Driver {
    /// A serve this server started does not outlive it.
    fn drop(&mut self) {
        if self.started.is_some() {
            let _ = self.stop();
        }
    }
}

/// `start` and `stop` (#717): a session from nothing, and back.
fn lifecycle_tools() -> Vec<Value> {
    vec![
        tool(
            "start",
            "Start a session from a profile in the per-user config: `diet-drive serve --profile \
             NAME` launched, and this server attached to the address it prints. Returns what \
             serve announced, or why it refused to start.",
            object(
                json!({
                    "profile": string("The profile's name."),
                    "phase": string("The phase to start in (`start-phase`), before any ask; the regimen's starting phase when omitted.")
                }),
                &["profile"],
            ),
        ),
        tool(
            "stop",
            "End the session, and the serve process `start` launched: nothing is left running. \
             Once serve has logged `ended` it is writing its record, and this waits for it to \
             exit however long that takes.",
            none(),
        ),
    ]
}

/// One JSON-RPC message answered: the response to send, or `None` for a
/// notification.
#[must_use]
pub fn answer(driver: &mut Driver, message: &str, notify: &mut dyn FnMut(&Value)) -> Option<Value> {
    let Ok(request) = serde_json::from_str::<Value>(message) else {
        return Some(failure(&Value::Null, -32700, "not JSON"));
    };
    // A notification -- `notifications/initialized` and the rest -- has no
    // id, and no answer.
    let id = request.get("id")?.clone();
    let params = request.get("params").cloned().unwrap_or_else(|| json!({}));
    let result = match request.get("method").and_then(Value::as_str) {
        Some("initialize") => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            let protocol = asked
                .filter(|asked| PROTOCOLS.contains(asked))
                .unwrap_or(PROTOCOLS[0]);
            json!({
                "protocolVersion": protocol,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "diet-drive", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Drives one `diet-drive serve` session: `start` one from a \
                     profile, or attach with --url. Read `status`, \
                     act with a command tool, then `wait` with `after` set to the last \
                     state's last_seq. Each tool returns serve's status and body as they came."
            })
        }
        Some("ping") => json!({}),
        Some("tools/list") => json!({"tools": tools()}),
        Some("tools/call") => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let empty = Map::new();
            let arguments = params
                .get("arguments")
                .and_then(Value::as_object)
                .unwrap_or(&empty);
            // Progress, when the call asked for it with a token (#772).
            let token = params.pointer("/_meta/progressToken").cloned();
            let mut progress = |done: u64, message: &str| {
                if let Some(token) = &token {
                    notify(&json!({
                        "jsonrpc": "2.0",
                        "method": "notifications/progress",
                        "params": {"progressToken": token, "progress": done, "message": message}
                    }));
                }
            };
            match driver.call(name, arguments, &mut progress) {
                Ok(result) => json!({
                    "content": [{"type": "text", "text": result.to_string()}],
                    "structuredContent": result,
                    "isError": false
                }),
                Err(why) => json!({
                    "content": [{"type": "text", "text": why}],
                    "isError": true
                }),
            }
        }
        _ => return Some(failure(&id, -32601, "no such method")),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

fn failure(id: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

/// The server's loop: each line of `input` answered on `output`, until
/// `input` ends.
///
/// # Errors
///
/// When `input` cannot be read or `output` written.
pub fn run(
    driver: &mut Driver,
    input: impl BufRead,
    mut output: impl Write,
) -> std::io::Result<()> {
    for line in input.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let mut notify = |notification: &Value| {
            let _ = writeln!(output, "{notification}");
            let _ = output.flush();
        };
        if let Some(response) = answer(driver, &line, &mut notify) {
            writeln!(output, "{response}")?;
            output.flush()?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer_quietly(driver: &mut Driver, message: &str) -> Option<Value> {
        answer(driver, message, &mut |_| {})
    }

    fn nowhere() -> Driver {
        Driver::new(
            Some(Serve::new("http://127.0.0.1:9", None).expect("an address")),
            std::path::PathBuf::from("/nowhere/diet-drive"),
            None,
            None,
        )
    }

    #[test]
    fn an_address_is_http_host_and_port() {
        assert!(Serve::new("http://127.0.0.1:7900", None).is_ok());
        assert!(Serve::new("127.0.0.1:7900/", None).is_ok());
        assert!(Serve::new("https://example.com/x", None).is_err());
        assert!(Serve::new("localhost", None).is_err());
        let serve = Serve::new("127.0.0.1:1", Some("author:s3cret")).expect("an address");
        assert_eq!(
            serve.authorization.as_deref(),
            Some("Basic YXV0aG9yOnMzY3JldA==")
        );
    }

    #[test]
    fn initialize_names_the_protocol_and_the_tools() {
        let mut serve = nowhere();
        let reply = answer_quietly(
            &mut serve,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        )
        .expect("an answer");
        assert_eq!(reply["result"]["protocolVersion"], "2025-03-26");
        assert!(reply["result"]["capabilities"]["tools"].is_object());
        let reply = answer_quietly(
            &mut serve,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}"#,
        )
        .expect("an answer");
        assert_eq!(reply["result"]["protocolVersion"], PROTOCOLS[0]);
        assert_eq!(
            answer_quietly(
                &mut serve,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            ),
            None
        );
        let listed = answer_quietly(
            &mut serve,
            r#"{"jsonrpc":"2.0","id":"x","method":"tools/list"}"#,
        )
        .expect("an answer");
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .map(|tool| tool["name"].as_str().expect("a name"))
            .collect();
        assert_eq!(
            names,
            [
                "start",
                "stop",
                "status",
                "wait",
                "ask",
                "attach",
                "approve",
                "cancel",
                "end",
                "memory",
                "edit",
                "flag",
                "declare_meditation",
                "refill_anyway",
                "propose_phase",
                "move_phase",
                "rule_phase",
                "open_tangent",
                "close_tangent",
                "events"
            ]
        );
        assert_eq!(listed["id"], "x");
        let missing = answer_quietly(&mut serve, r#"{"jsonrpc":"2.0","id":2,"method":"nope"}"#)
            .expect("an answer");
        assert_eq!(missing["error"]["code"], -32601);
        let garbled = answer_quietly(&mut serve, "{").expect("an answer");
        assert_eq!(garbled["error"]["code"], -32700);
    }

    #[test]
    fn a_tool_that_cannot_reach_serve_or_is_misused_says_so() {
        let mut serve = nowhere();
        let reply = answer_quietly(
            &mut serve,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"ask","arguments":{}}}"#,
        )
        .expect("an answer");
        assert_eq!(reply["result"]["isError"], true);
        assert_eq!(
            reply["result"]["content"][0]["text"],
            "`text` is required, a string"
        );
        let reply = answer_quietly(
            &mut serve,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"status"}}"#,
        )
        .expect("an answer");
        assert_eq!(reply["result"]["isError"], true);
    }

    /// A serve that answers `end` with 200 and `/state` with `session`, on
    /// a thread for as long as the test runs.
    fn ending_serve(session: &'static str) -> Serve {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = std::net::TcpListener::local_addr(&listener).expect("its address");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut head = Vec::new();
                let mut byte = [0_u8; 1];
                while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let length: usize = head
                    .lines()
                    .find_map(|line| line.strip_prefix("Content-Length: "))
                    .and_then(|n| n.trim().parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0_u8; length];
                let _ = stream.read_exact(&mut body);
                let answer = if head.starts_with("GET /state") {
                    json!({"session": session}).to_string()
                } else {
                    "{}".to_owned()
                };
                let _ = write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
            }
        });
        Serve::new(&format!("http://{address}"), None).expect("an address")
    }

    /// A driver attached to `serve`, having started `child` as it, waiting
    /// `patience` for it.
    fn started(serve: Serve, child: std::process::Child, patience: Duration) -> Driver {
        let mut driver = Driver::new(
            Some(serve),
            std::path::PathBuf::from("/nowhere/diet-drive"),
            None,
            None,
        );
        driver.started = Some(Started { child });
        driver.stop_patience = patience;
        driver
    }

    /// #879: a serve that has logged `ended` is writing its record, which on
    /// a long session takes longer than any fixed wait; `stop` waits for it
    /// to exit, however long that takes, and never kills it mid-write.
    #[test]
    fn stop_waits_out_a_serve_writing_its_record() {
        let writing = std::process::Command::new("/bin/sleep")
            .arg("3")
            .spawn()
            .expect("a slow writer");
        let mut driver = started(ending_serve("ended"), writing, Duration::from_secs(1));
        let stopped = driver.stop().expect("stopped");
        assert_eq!(stopped["exited"], 0, "{stopped}");
    }

    /// And a serve that never logged `ended` is killed once `stop` has
    /// waited its patience.
    #[test]
    fn stop_kills_a_serve_that_never_logged_ended() {
        let stuck = std::process::Command::new("/bin/sleep")
            .arg("30")
            .spawn()
            .expect("a stuck serve");
        let begun = Instant::now();
        let mut driver = started(ending_serve("running"), stuck, Duration::from_secs(1));
        let stopped = driver.stop().expect("stopped");
        assert_eq!(stopped["exited"], "killed", "{stopped}");
        assert!(
            begun.elapsed() < Duration::from_secs(10),
            "{:?}",
            begun.elapsed()
        );
    }

    #[test]
    fn an_answer_is_its_status_and_its_body() {
        let answer = parsed(
            b"HTTP/1.1 409 Conflict\r\nContent-Length: 22\r\n\r\n{\"refused\":\"in-flight\"}",
        )
        .expect("parsed");
        assert_eq!(answer.status, 409);
        assert_eq!(answer.body, json!({"refused": "in-flight"}));
        let empty = parsed(b"HTTP/1.1 204 No Content\r\n\r\n").expect("parsed");
        assert_eq!(empty.json(), json!({"status": 204, "body": {}}));
        assert_eq!(query("turn.settled"), "turn.settled");
        assert_eq!(query("a b&c"), "a%20b%26c");
    }
}
