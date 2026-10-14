//! A scenario (#760): a drive written as steps and expectations, run on the
//! canned substrate or re-fired on a named endpoint.
//!
//! The file is JSON Lines in the record's value space, read with the
//! record's own reader (`diet/scenarios/README.md` is its note). Its
//! first line names the scenario, its two regimens and its head; every other
//! line is one `reply` the canned substrate answers with, one `step` the
//! runner drives, or one `expect` the log must meet.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};

use crate::formats::record::json::{self, Value};

/// A scenario, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    /// Its name: what a run of it is labelled with.
    pub name: String,
    /// The regimen it runs under on the canned substrate, relative to the
    /// file.
    pub regimen: String,
    /// The regimen a live re-fire runs under, relative to the file.
    pub live_regimen: Option<String>,
    /// The trunk's head, relative to the file.
    pub head: String,
    /// How a prompt waiting on the operator is answered: a `POST /approve`
    /// scope. Absent, a prompt fails the run.
    pub approvals: Option<String>,
    /// Files the runner copies into the fresh worktree before serve starts:
    /// each path in the worktree, and its source beside the scenario.
    pub seed: BTreeMap<String, String>,
    /// The canned substrate's replies, in the file's order.
    pub replies: Vec<Reply>,
    /// What the runner drives, in order.
    pub steps: Vec<Step>,
    /// What the log must show.
    pub expects: Vec<Expect>,
}

/// One canned reply: the text a request's last message must hold, or none
/// for a reply taken in order; what it says; and the calls it makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    /// Text the request's last message holds.
    pub when: Option<String>,
    /// The answer's text.
    pub text: String,
    /// The calls it makes.
    pub calls: Vec<Call>,
    /// How long it holds its answer back, in milliseconds (#782): what
    /// orders a canned trunk's answer after a fork beside it.
    pub delay_ms: Option<u64>,
    /// The prompt its usage reports, in tokens (#798); absent, the canned
    /// server's own count.
    pub prompt_tokens: Option<u64>,
}

/// One call a reply makes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// Its id.
    pub id: String,
    /// The tool.
    pub name: String,
    /// Its arguments, as JSON text.
    pub arguments: String,
}

/// Which live entry a step acts on, read from `GET /memory`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// The live entry at this place, in id order.
    Index(usize),
    /// The first live entry whose content holds this text.
    Containing(String),
}

/// One thing the runner does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// An ask; `refused` names the refusal it must meet instead.
    Ask {
        /// What is asked.
        text: String,
        /// The operator's scoping mark.
        scoping: bool,
        /// The refusal it must be answered with, when it must be refused.
        refused: Option<String>,
        /// Worktree files uploaded through `POST /files` and attached to
        /// the ask by their digests (#372).
        attach: Vec<String>,
    },
    /// The operator picks the phase the session starts in, before the first
    /// ask (#788).
    StartPhase {
        /// The phase.
        phase: String,
    },
    /// The operator proposes a phase move for the model to accept or
    /// decline (#788): `meditate`, absent, is serve's default.
    ProposePhase {
        /// The phase proposed.
        to: String,
        /// Whether an accepted move goes through a meditation.
        meditate: Option<bool>,
    },
    /// The operator moves the phase alone (#788).
    MovePhase {
        /// The phase moved to.
        to: String,
        /// Whether the move goes through a meditation.
        meditate: Option<bool>,
    },
    /// The operator opens a tangent (#22).
    OpenTangent {
        /// Its id.
        id: String,
    },
    /// The operator closes the open tangent, ruling `each` -- `keep`,
    /// `drop` or `park` -- on every live entry born under it.
    CloseTangent {
        /// The ruling on each entry.
        each: String,
    },
    /// The operator flags an entry, bound to `bind` when named.
    Flag {
        /// The entry.
        pick: Pick,
        /// The flag's note.
        note: String,
        /// The name it is bound to.
        bind: Option<String>,
    },
    /// The operator edits an entry; the operator's new entry is bound.
    Edit {
        /// The entry.
        pick: Pick,
        /// The new text.
        text: String,
        /// The name the operator's entry is bound to.
        bind: Option<String>,
    },
    /// The operator declares a meditation.
    DeclareMeditation,
    /// The operator refills a held meditation anyway.
    RefillAnyway,
    /// The operator rules on the model's phase proposal (#787, #788): the
    /// proposal's call, and `meditation`, `keep` (read too as `continue`)
    /// or `stay`, posted as written.
    RulePhase {
        /// The `propose_phase_transition` call.
        call: String,
        /// The ruling.
        choice: String,
    },
    /// Wait until the session is idle or a line of this kind is logged.
    Wait {
        /// `idle`, or a log line's kind.
        until: String,
        /// How long to wait, in seconds.
        timeout: Option<u64>,
    },
    /// End the session.
    End,
}

impl Step {
    /// The `POST /commands` body of a step that is one command and waits
    /// for the session to settle after it.
    fn command(&self) -> serde_json::Value {
        let move_to = |kind: &str, to: &str, meditate: Option<bool>| {
            let mut command = serde_json::json!({"kind": kind, "to": to});
            if let Some(meditate) = meditate {
                command["meditate"] = serde_json::Value::Bool(meditate);
            }
            command
        };
        match self {
            Self::DeclareMeditation => serde_json::json!({"kind": "declare-meditation"}),
            Self::RefillAnyway => serde_json::json!({"kind": "refill-anyway"}),
            Self::RulePhase { call, choice } => {
                serde_json::json!({"kind": "ratify-phase", "call": call, "choice": choice})
            }
            Self::StartPhase { phase } => {
                serde_json::json!({"kind": "start-phase", "phase": phase})
            }
            Self::ProposePhase { to, meditate } => move_to("propose-phase", to, *meditate),
            Self::MovePhase { to, meditate } => move_to("move-phase", to, *meditate),
            _ => unreachable!("a step of its own"),
        }
    }
}

/// What one log line must show: its `kind` and fields, at least `at_least`
/// times, after the first line `after` names matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expect {
    /// The line's kind.
    pub kind: String,
    /// Each field and what it must be: `key` equal to the text, `key~`
    /// holding it. A `$name` in the text is the entry bound to it.
    pub fields: Vec<(String, Match)>,
    /// How many lines must match.
    pub at_least: u64,
    /// Its name, for another expectation's `after`.
    pub name: Option<String>,
    /// The expectation whose first match this one's must come after.
    pub after: Option<String>,
}

/// What a field must be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Match {
    /// Equal to this text.
    Equals(String),
    /// Holding this text.
    Contains(String),
}

impl Scenario {
    /// The routes the canned substrate serves (#760): each reply streamed as
    /// llama.cpp streams one, routed by its `when`, else in order.
    #[must_use]
    pub fn routes(&self) -> Vec<crate::client::stub::Route> {
        use crate::client::stub::{Act, Route};
        use crate::drive::canned::{
            PROMPT_TOKENS, StreamedCall, streamed_events_reporting, streamed_reporting,
        };
        self.replies
            .iter()
            .map(|reply| {
                let calls: Vec<StreamedCall<'_>> = reply
                    .calls
                    .iter()
                    .map(|call| StreamedCall {
                        id: &call.id,
                        name: &call.name,
                        arguments: &call.arguments,
                    })
                    .collect();
                // Held back: the same events, chunked, the first after the
                // delay (#782).
                let prompt = reply.prompt_tokens.unwrap_or(PROMPT_TOKENS);
                let act = match reply.delay_ms {
                    Some(ms) => Act::Trickle(
                        std::time::Duration::from_millis(ms),
                        vec![streamed_events_reporting(&reply.text, &calls, prompt)],
                    ),
                    None => Act::Raw(streamed_reporting(&reply.text, &calls, prompt)),
                };
                match &reply.when {
                    Some(text) => Route::when(text, act),
                    None => Route::ordered(act),
                }
            })
            .collect()
    }

    /// The canned substrate's identity (#760): the sha256 of its replies,
    /// each rendered as the record renders a value, one per line, in the
    /// file's order. What `canned-scenario`'s `engine_identity_is` names,
    /// and what a run says it played.
    #[must_use]
    pub fn replies_digest(&self) -> String {
        let text = |s: &str| Value::String(s.to_owned());
        let mut out = String::new();
        for reply in &self.replies {
            let mut members = BTreeMap::from([("text".to_owned(), text(&reply.text))]);
            if let Some(when) = &reply.when {
                members.insert("when".to_owned(), text(when));
            }
            if !reply.calls.is_empty() {
                let calls = reply
                    .calls
                    .iter()
                    .map(|call| {
                        Value::Object(BTreeMap::from([
                            ("id".to_owned(), text(&call.id)),
                            ("name".to_owned(), text(&call.name)),
                            ("arguments".to_owned(), text(&call.arguments)),
                        ]))
                    })
                    .collect();
                members.insert("calls".to_owned(), Value::Array(calls));
            }
            if let Some(ms) = reply.delay_ms {
                members.insert(
                    "delay_ms".to_owned(),
                    Value::Integer(i64::try_from(ms).unwrap_or(i64::MAX)),
                );
            }
            if let Some(tokens) = reply.prompt_tokens {
                members.insert(
                    "prompt_tokens".to_owned(),
                    Value::Integer(i64::try_from(tokens).unwrap_or(i64::MAX)),
                );
            }
            json::render(&Value::Object(members), &mut out);
            out.push('\n');
        }
        crate::digest::sha256_hex(out.as_bytes())
    }
}

/// Why a scenario does not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScenarioError {
    /// Not JSON Lines in the record's value space.
    Syntax(String),
    /// A line, by its number from 1, and what is wrong with it.
    Line(usize, String),
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(why) => write!(f, "the scenario does not read: {why}"),
            Self::Line(at, why) => write!(f, "scenario line {at}: {why}"),
        }
    }
}

impl std::error::Error for ScenarioError {}

/// Read a scenario.
///
/// # Errors
///
/// Returns [`ScenarioError`] for text that is not JSON Lines in the
/// record's value space, a first line that is not the header, or any line
/// with a key it does not know, without one it needs, or of the wrong type.
pub fn parse(text: &str) -> Result<Scenario, ScenarioError> {
    let lines = json::objects(text).map_err(|why| ScenarioError::Syntax(why.to_string()))?;
    let mut lines = lines
        .into_iter()
        .enumerate()
        .map(|(at, line)| (at + 1, line));
    let (_, first) = lines
        .next()
        .ok_or_else(|| ScenarioError::Syntax("no header line".to_owned()))?;
    let mut scenario = header(Fields::of(first, 1))?;
    let mut names: Vec<String> = Vec::new();
    for (at, line) in lines {
        let mut fields = Fields::of(line, at);
        if fields.has("reply") {
            let reply = fields.object("reply")?;
            fields.done()?;
            scenario.replies.push(reply_of(Fields::of(reply, at))?);
        } else if fields.has("step") {
            scenario.steps.push(step_of(fields)?);
        } else if fields.has("expect") {
            let expect = expect_of(fields, &names)?;
            if let Some(name) = &expect.name {
                names.push(name.clone());
            }
            scenario.expects.push(expect);
        } else {
            return Err(fields.wrong("a `reply`, `step` or `expect` line"));
        }
    }
    Ok(scenario)
}

/// The first line: `scenario`, `regimen`, `head`, and optionally
/// `live_regimen` and `approvals`.
fn header(mut fields: Fields) -> Result<Scenario, ScenarioError> {
    let name = fields.text("scenario")?;
    let regimen = fields.text("regimen")?;
    let head = fields.text("head")?;
    let live_regimen = fields.maybe_text("live_regimen")?;
    let approvals = fields.maybe_text("approvals")?;
    let seed = match fields.take("seed") {
        None => BTreeMap::new(),
        Some(Value::Object(seed)) => seed
            .into_iter()
            .map(|(path, source)| match source {
                Value::String(source) if is_relative(&path) && is_relative(&source) => {
                    Ok((path, source))
                }
                _ => Err(fields.wrong("`seed` mapping relative worktree paths to relative files")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(fields.wrong("`seed` an object")),
    };
    if let Some(scope) = &approvals
        && !crate::drive::tool_loop::Decision::ALL
            .iter()
            .any(|decision| decision.tag() == scope)
    {
        return Err(fields.wrong("`approvals` naming a `POST /approve` scope"));
    }
    fields.done()?;
    Ok(Scenario {
        name,
        regimen,
        live_regimen,
        head,
        approvals,
        seed,
        replies: Vec::new(),
        steps: Vec::new(),
        expects: Vec::new(),
    })
}

/// A path that stays where it is put: relative, with no `..`.
fn is_relative(path: &str) -> bool {
    let path = std::path::Path::new(path);
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
}

fn reply_of(mut fields: Fields) -> Result<Reply, ScenarioError> {
    let when = fields.maybe_text("when")?;
    let text = fields.text("text")?;
    let calls = match fields.take("calls") {
        None => Vec::new(),
        Some(Value::Array(calls)) => calls
            .into_iter()
            .map(|call| match call {
                Value::Object(call) => {
                    let mut call = Fields::of(call, fields.at);
                    let read = Call {
                        id: call.text("id")?,
                        name: call.text("name")?,
                        arguments: call.text("arguments")?,
                    };
                    call.done()?;
                    Ok(read)
                }
                _ => Err(fields.wrong("each call an object")),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(fields.wrong("`calls` a list")),
    };
    let delay_ms = fields.maybe_count("delay_ms")?;
    let prompt_tokens = fields.maybe_count("prompt_tokens")?;
    fields.done()?;
    Ok(Reply {
        when,
        text,
        calls,
        delay_ms,
        prompt_tokens,
    })
}

fn step_of(mut fields: Fields) -> Result<Step, ScenarioError> {
    let step = match fields.text("step")?.as_str() {
        "ask" => Step::Ask {
            text: fields.text("text")?,
            scoping: fields.maybe_bool("scoping")?.unwrap_or(false),
            refused: fields.maybe_text("refused")?,
            attach: match fields.take("attach") {
                None => Vec::new(),
                Some(Value::Array(paths)) => paths
                    .into_iter()
                    .map(|path| match path {
                        Value::String(path) if is_relative(&path) => Ok(path),
                        _ => Err(fields.wrong("each of `attach` a relative worktree path")),
                    })
                    .collect::<Result<_, _>>()?,
                Some(_) => return Err(fields.wrong("`attach` a list")),
            },
        },
        "start-phase" => Step::StartPhase {
            phase: fields.text("phase")?,
        },
        "propose-phase" => Step::ProposePhase {
            to: fields.text("to")?,
            meditate: fields.maybe_bool("meditate")?,
        },
        "move-phase" => Step::MovePhase {
            to: fields.text("to")?,
            meditate: fields.maybe_bool("meditate")?,
        },
        "open-tangent" => Step::OpenTangent {
            id: fields.text("id")?,
        },
        "close-tangent" => Step::CloseTangent {
            each: match fields.text("each")?.as_str() {
                word @ ("keep" | "drop" | "park") => word.to_owned(),
                _ => return Err(fields.wrong("`each` keep, drop or park")),
            },
        },
        "flag" => Step::Flag {
            pick: fields.pick()?,
            note: fields.text("note")?,
            bind: fields.maybe_text("as")?,
        },
        "edit" => Step::Edit {
            pick: fields.pick()?,
            text: fields.text("text")?,
            bind: fields.maybe_text("as")?,
        },
        "declare-meditation" => Step::DeclareMeditation,
        "refill-anyway" => Step::RefillAnyway,
        "rule-phase" => Step::RulePhase {
            call: fields.text("call")?,
            choice: fields.text("choice")?,
        },
        "wait" => Step::Wait {
            until: fields.text("until")?,
            timeout: fields.maybe_count("timeout")?,
        },
        "end" => Step::End,
        _ => return Err(fields.wrong("a step the runner knows")),
    };
    fields.done()?;
    Ok(step)
}

fn expect_of(mut fields: Fields, names: &[String]) -> Result<Expect, ScenarioError> {
    let kind = fields.text("expect")?;
    let mut matches = Vec::new();
    if let Some(wanted) = fields.take("where") {
        let Value::Object(wanted) = wanted else {
            return Err(fields.wrong("`where` an object"));
        };
        for (key, value) in wanted {
            let Value::String(text) = value else {
                return Err(fields.wrong("each `where` value text"));
            };
            matches.push(match key.strip_suffix('~') {
                Some(key) => (key.to_owned(), Match::Contains(text)),
                None => (key, Match::Equals(text)),
            });
        }
    }
    let at_least = fields.maybe_count("at_least")?.unwrap_or(1);
    if at_least == 0 {
        return Err(fields.wrong("`at_least` 1 or more"));
    }
    let name = fields.maybe_text("name")?;
    let after = fields.maybe_text("after")?;
    if after.as_ref().is_some_and(|after| !names.contains(after)) {
        return Err(fields.wrong("`after` naming an expectation above it"));
    }
    fields.done()?;
    Ok(Expect {
        kind,
        fields: matches,
        at_least,
        name,
        after,
    })
}

/// One line's fields, taken one by one; any left over is refused.
struct Fields {
    at: usize,
    left: BTreeMap<String, Value>,
}

impl Fields {
    fn of(left: BTreeMap<String, Value>, at: usize) -> Self {
        Self { at, left }
    }

    fn wrong(&self, wanted: &str) -> ScenarioError {
        ScenarioError::Line(self.at, format!("wanted {wanted}"))
    }

    fn has(&self, key: &str) -> bool {
        self.left.contains_key(key)
    }

    fn take(&mut self, key: &str) -> Option<Value> {
        self.left.remove(key)
    }

    fn maybe_text(&mut self, key: &str) -> Result<Option<String>, ScenarioError> {
        match self.take(key) {
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text)),
            Some(_) => Err(self.wrong(&format!("`{key}` text"))),
        }
    }

    fn text(&mut self, key: &str) -> Result<String, ScenarioError> {
        self.maybe_text(key)?
            .ok_or_else(|| self.wrong(&format!("`{key}`")))
    }

    fn maybe_bool(&mut self, key: &str) -> Result<Option<bool>, ScenarioError> {
        match self.take(key) {
            None => Ok(None),
            Some(Value::Boolean(flag)) => Ok(Some(flag)),
            Some(_) => Err(self.wrong(&format!("`{key}` true or false"))),
        }
    }

    fn maybe_count(&mut self, key: &str) -> Result<Option<u64>, ScenarioError> {
        match self.take(key) {
            None => Ok(None),
            Some(Value::Integer(n)) => u64::try_from(n)
                .map(Some)
                .map_err(|_| self.wrong(&format!("`{key}` 0 or more"))),
            Some(_) => Err(self.wrong(&format!("`{key}` a count"))),
        }
    }

    fn object(&mut self, key: &str) -> Result<BTreeMap<String, Value>, ScenarioError> {
        match self.take(key) {
            Some(Value::Object(object)) => Ok(object),
            _ => Err(self.wrong(&format!("`{key}` an object"))),
        }
    }

    /// `pick`: a place among the live entries, or text one holds.
    fn pick(&mut self) -> Result<Pick, ScenarioError> {
        match self.take("pick") {
            Some(Value::Integer(n)) => usize::try_from(n)
                .map(Pick::Index)
                .map_err(|_| self.wrong("`pick` 0 or more")),
            Some(Value::String(text)) if !text.is_empty() => Ok(Pick::Containing(text)),
            _ => Err(self.wrong("`pick`, a place or some text")),
        }
    }

    fn done(self) -> Result<(), ScenarioError> {
        match self.left.keys().next() {
            None => Ok(()),
            Some(key) => Err(ScenarioError::Line(
                self.at,
                format!("`{key}` is not a key this line takes"),
            )),
        }
    }
}

/// A serve this runner drives over its HTTP API (#756): its address, and
/// the Basic credential every request presents.
#[derive(Debug, Clone)]
pub struct Driver {
    address: String,
    authorization: Option<String>,
    /// Where the model's commands run: what an ask's `attach` reads from.
    worktree: Option<std::path::PathBuf>,
}

/// Why a drive stopped short of its last step, or a run failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure(pub String);

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How long one step may take to settle before the drive fails.
pub const STEP_PATIENCE: std::time::Duration = std::time::Duration::from_secs(900);

/// The names steps bound, each to an entry id.
pub type Bindings = BTreeMap<String, String>;

impl Driver {
    /// A driver for the serve listening at `address` (`host:port`), with
    /// `user_password` its `--auth-file`'s content when it has one.
    #[must_use]
    pub fn new(address: &str, user_password: Option<&str>) -> Self {
        Self {
            address: address.to_owned(),
            authorization: user_password
                .map(|pair| format!("Basic {}", crate::drive::serve::base64(pair.as_bytes()))),
            worktree: None,
        }
    }

    /// This driver, reading an ask's attachments from `worktree`.
    #[must_use]
    pub fn in_worktree(mut self, worktree: std::path::PathBuf) -> Self {
        self.worktree = Some(worktree);
        self
    }

    /// One request, answered with a close: its status and its body.
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), Failure> {
        self.sent(
            method,
            path,
            body.map(|body| ("application/json", body.as_bytes())),
        )
    }

    /// One request whose body is `body`'s bytes, of its media type.
    fn sent(
        &self,
        method: &str,
        path: &str,
        body: Option<(&str, &[u8])>,
    ) -> Result<(u16, String), Failure> {
        use std::io::{Read as _, Write as _};
        let failed = |why: std::io::Error| Failure(format!("{method} {path}: {why}"));
        let mut stream = std::net::TcpStream::connect(&self.address).map_err(failed)?;
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n", self.address);
        if let Some(authorization) = &self.authorization {
            let _ = write!(head, "Authorization: {authorization}\r\n");
        }
        if let Some((media_type, body)) = body {
            let _ = write!(
                head,
                "Content-Type: {media_type}\r\nContent-Length: {}\r\n",
                body.len()
            );
        }
        head.push_str("\r\n");
        let mut bytes = head.into_bytes();
        bytes.extend_from_slice(body.map_or(&[][..], |(_, body)| body));
        stream.write_all(&bytes).map_err(failed)?;
        let mut reply = String::new();
        stream.read_to_string(&mut reply).map_err(failed)?;
        let status = reply
            .split(' ')
            .nth(1)
            .and_then(|code| code.parse().ok())
            .ok_or_else(|| Failure(format!("{method} {path}: not an HTTP reply")))?;
        let body = reply.split_once("\r\n\r\n").map_or("", |(_, body)| body);
        Ok((status, body.to_owned()))
    }

    fn get(&self, path: &str) -> Result<serde_json::Value, Failure> {
        match self.request("GET", path, None)? {
            (200, body) => {
                serde_json::from_str(&body).map_err(|why| Failure(format!("GET {path}: {why}")))
            }
            (status, body) => Err(Failure(format!("GET {path}: {status} {body}"))),
        }
    }

    fn post(&self, path: &str, body: &serde_json::Value) -> Result<(u16, String), Failure> {
        self.request("POST", path, Some(&body.to_string()))
    }

    /// A command that must be admitted.
    fn command(&self, body: &serde_json::Value) -> Result<(), Failure> {
        match self.post("/commands", body)? {
            (200, _) => Ok(()),
            (status, reply) => Err(Failure(format!("{body} was answered {status} {reply}"))),
        }
    }

    /// The last `seq` logged.
    fn last_seq(&self) -> Result<u64, Failure> {
        self.get("/state")?["last_seq"]
            .as_u64()
            .ok_or_else(|| Failure("/state named no last_seq".to_owned()))
    }

    /// Wait until the session is idle, answering each prompt that waits on
    /// the operator with `approvals`' scope; a prompt with no scope to
    /// answer it fails the drive, as does [`STEP_PATIENCE`] passing.
    fn settle(&self, approvals: Option<&str>) -> Result<serde_json::Value, Failure> {
        let give_up = std::time::Instant::now() + STEP_PATIENCE;
        loop {
            let state = self.get("/wait?until=idle&timeout=30")?;
            if state["idle"] == true {
                return Ok(state);
            }
            if let Some(prompt) = state.get("approval") {
                let Some(scope) = approvals else {
                    return Err(Failure(format!(
                        "a command waited on the operator and the scenario names no approvals: {prompt}"
                    )));
                };
                let answer = serde_json::json!({"call": prompt["id"], "scope": scope});
                match self.post("/approve", &answer)? {
                    (204 | 409, _) => {}
                    (status, body) => {
                        return Err(Failure(format!("{answer} was answered {status} {body}")));
                    }
                }
            }
            if std::time::Instant::now() >= give_up {
                return Err(Failure(format!(
                    "not idle after {STEP_PATIENCE:?}: {state}"
                )));
            }
        }
    }

    /// The digest `POST /files` answers for the worktree file `path`.
    fn uploaded(&self, path: &str) -> Result<String, Failure> {
        let worktree = self.worktree.as_ref().ok_or_else(|| {
            Failure(format!(
                "`{path}` is attached and nothing runs in a worktree"
            ))
        })?;
        let bytes = std::fs::read(worktree.join(path))
            .map_err(|why| Failure(format!("`{path}` cannot be read from the worktree: {why}")))?;
        match self.sent("POST", "/files", Some(("image/png", &bytes)))? {
            (200, body) => serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|reply| reply["sha256"].as_str().map(str::to_owned))
                .ok_or_else(|| Failure(format!("POST /files answered {body}"))),
            (status, body) => Err(Failure(format!(
                "POST /files for `{path}`: {status} {body}"
            ))),
        }
    }

    /// Close the tangent `id` (#22), ruling `each` on every live entry born
    /// under it, read from `GET /memory`.
    fn closed(&self, id: &str, each: &str) -> Result<(), Failure> {
        let memory = self.get("/memory")?;
        let dispositions: serde_json::Map<String, serde_json::Value> = memory["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|entry| entry["provenances"][0]["tangent"] == id)
            .filter_map(|entry| entry["id"].as_str().map(str::to_owned))
            .map(|entry| (entry, serde_json::Value::String(each.to_owned())))
            .collect();
        self.command(&serde_json::json!({"kind": "close-tangent", "dispositions": dispositions}))
    }

    /// Post an ask -- `text`, its scoping mark, and the worktree files it
    /// attaches -- and wait until the session is idle; or, with `refused`,
    /// see it refused with that tag.
    fn asked(
        &self,
        (text, scoping, attach): (&str, bool, &[String]),
        refused: Option<&str>,
        approvals: Option<&str>,
    ) -> Result<(), Failure> {
        let mut ask = serde_json::json!({"kind": "ask", "text": text});
        if scoping {
            ask["scoping"] = serde_json::Value::Bool(true);
        }
        if !attach.is_empty() {
            let files = attach
                .iter()
                .map(|path| self.uploaded(path))
                .collect::<Result<Vec<_>, _>>()?;
            ask["files"] = serde_json::json!(files);
        }
        let Some(refused) = refused else {
            self.command(&ask)?;
            return self.settle(approvals).map(|_| ());
        };
        let (status, body) = self.post("/commands", &ask)?;
        let tag = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|reply| reply["refused"].as_str().map(str::to_owned));
        if status != 409 || tag.as_deref() != Some(refused) {
            return Err(Failure(format!(
                "the ask was to be refused `{refused}` and was answered {status} {body}"
            )));
        }
        Ok(())
    }

    /// The live entry `pick` names, from `GET /memory`.
    fn picked(&self, pick: &Pick) -> Result<String, Failure> {
        let memory = self.get("/memory")?;
        let entries = memory["entries"].as_array().cloned().unwrap_or_default();
        let found = match pick {
            Pick::Index(at) => entries.get(*at),
            Pick::Containing(text) => entries.iter().find(|entry| {
                entry["content"]
                    .as_str()
                    .is_some_and(|c| c.contains(text.as_str()))
            }),
        };
        found
            .and_then(|entry| entry["id"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| Failure(format!("no live entry is {pick:?}: {memory}")))
    }

    /// The live entry that superseded `id`.
    fn superseding(&self, id: &str) -> Result<String, Failure> {
        let memory = self.get("/memory")?;
        memory["entries"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| entry["supersedes"] == id)
            .and_then(|entry| entry["id"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| Failure(format!("no live entry supersedes {id}")))
    }
}

/// Drive `scenario`'s steps through `driver`, in order (#760): each command
/// posted and, once admitted, waited on until the session is idle; the
/// entries steps bound, by name.
///
/// # Errors
///
/// Returns the [`Failure`] of the first step that did not go as written: a
/// command refused that was to be admitted or admitted that was to be
/// refused, a pick that names no live entry, a prompt with no approvals to
/// answer it, a wait that timed out, or a session that never settled.
pub fn drive(scenario: &Scenario, driver: &Driver) -> Result<Bindings, Failure> {
    let approvals = scenario.approvals.as_deref();
    let mut bound = Bindings::new();
    let mut mark = driver.last_seq()?;
    let mut tangent: Option<&str> = None;
    for (at, step) in scenario.steps.iter().enumerate() {
        let failed = |why: Failure| Failure(format!("step {}: {why}", at + 1));
        match step {
            Step::Ask {
                text,
                scoping,
                refused,
                attach,
            } => {
                mark = driver.last_seq().map_err(failed)?;
                driver
                    .asked((text, *scoping, attach), refused.as_deref(), approvals)
                    .map_err(failed)?;
            }
            Step::Flag { pick, note, bind } => {
                let id = driver.picked(pick).map_err(failed)?;
                driver
                    .command(&serde_json::json!({"kind": "flag-entry", "id": id, "note": note}))
                    .map_err(failed)?;
                if let Some(name) = bind {
                    bound.insert(name.clone(), id);
                }
            }
            Step::Edit { pick, text, bind } => {
                let id = driver.picked(pick).map_err(failed)?;
                driver
                    .command(&serde_json::json!({"kind": "edit-entry", "id": id, "content": text}))
                    .map_err(failed)?;
                if let Some(name) = bind {
                    let theirs = driver.superseding(&id).map_err(failed)?;
                    bound.insert(name.clone(), theirs);
                }
            }
            Step::DeclareMeditation
            | Step::RefillAnyway
            | Step::RulePhase { .. }
            | Step::StartPhase { .. }
            | Step::ProposePhase { .. }
            | Step::MovePhase { .. } => {
                mark = driver.last_seq().map_err(failed)?;
                driver.command(&step.command()).map_err(failed)?;
                driver.settle(approvals).map_err(failed)?;
            }
            Step::OpenTangent { id } => {
                driver
                    .command(&serde_json::json!({"kind": "open-tangent", "id": id}))
                    .map_err(failed)?;
                tangent = Some(id);
            }
            Step::CloseTangent { each } => {
                let id = tangent
                    .take()
                    .ok_or_else(|| failed(Failure("no tangent is open to close".to_owned())))?;
                driver.closed(id, each).map_err(failed)?;
            }
            Step::Wait { until, timeout } => {
                let seconds = timeout.unwrap_or(300);
                let state = driver
                    .get(&format!(
                        "/wait?until={until}&after={mark}&timeout={seconds}"
                    ))
                    .map_err(failed)?;
                if state["timed_out"] == true {
                    return Err(failed(Failure(format!(
                        "no `{until}` within {seconds} s: {state}"
                    ))));
                }
            }
            Step::End => {
                driver
                    .command(&serde_json::json!({"kind": "end"}))
                    .map_err(failed)?;
            }
        }
    }
    Ok(bound)
}

/// Each of `scenario`'s expectations `log` -- the session's log -- does not
/// meet, with `bound`'s names in place of each `$name`; empty when it meets
/// every one. Matched by [`crate::formats::log_query::find_terms`], the one
/// matcher `diet log find` calls too: `kind=<kind>`, then each field as
/// `key=value` or `key~text`.
#[must_use]
pub fn unmet(scenario: &Scenario, log: &str, bound: &Bindings) -> Vec<String> {
    let bind = |wanted: &str| {
        bound.iter().fold(wanted.to_owned(), |out, (name, id)| {
            out.replace(&format!("${name}"), id)
        })
    };
    let mut first: BTreeMap<&str, i64> = BTreeMap::new();
    let mut missed = Vec::new();
    for expect in &scenario.expects {
        let floor = expect
            .after
            .as_deref()
            .and_then(|name| first.get(name).copied());
        if expect.after.is_some() && floor.is_none() {
            missed.push(format!(
                "{}: `after` names {:?}, which matched nothing",
                expect.kind, expect.after
            ));
            continue;
        }
        let mut terms = vec![format!("kind={}", expect.kind)];
        terms.extend(expect.fields.iter().map(|(key, wanted)| match wanted {
            Match::Equals(text) => format!("{key}={}", bind(text)),
            Match::Contains(text) => format!("{key}~{}", bind(text)),
        }));
        let found = match crate::formats::log_query::find_terms(log, &terms) {
            Ok(found) => found,
            Err(why) => {
                missed.push(format!("{terms:?}: {why}"));
                continue;
            }
        };
        let seqs: Vec<i64> = found
            .iter()
            .filter_map(|line| match line {
                Value::Object(line) => match line.get("seq") {
                    Some(Value::Integer(seq)) => Some(*seq),
                    _ => None,
                },
                _ => None,
            })
            .filter(|seq| floor.is_none_or(|floor| *seq > floor))
            .collect();
        if let (Some(name), Some(seq)) = (&expect.name, seqs.first()) {
            first.insert(name, *seq);
        }
        if (seqs.len() as u64) < expect.at_least {
            missed.push(format!(
                "{terms:?}: {} line(s) matched, {} wanted",
                seqs.len(),
                expect.at_least
            ));
        }
    }
    missed
}

/// What `diet-drive scenario` was told (#760).
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    /// The scenario file.
    pub file: std::path::PathBuf,
    /// Where the run's outputs go: the assembled run, or with `assemble`
    /// off, the log and the record themselves.
    pub out: std::path::PathBuf,
    /// A named endpoint, for a live re-fire; absent, the canned substrate.
    pub endpoint: Option<String>,
    /// The served model's name on that endpoint.
    pub model: Option<String>,
    /// The endpoint's key file, passed to `serve` as it is.
    pub key_file: Option<String>,
    /// A regimen that beats the scenario's.
    pub regimen: Option<std::path::PathBuf>,
    /// Where the model's commands run; absent, a fresh directory.
    pub worktree: Option<std::path::PathBuf>,
    /// Whether the outputs go to `scripts/assemble-run.py` once the
    /// expectations pass.
    pub assemble: bool,
}

/// Run a scenario (#760): `serve` started as a child of `drive` -- this
/// binary -- on the canned substrate, its replies the scenario's, or on
/// `options.endpoint`; driven through its HTTP API; its log checked against
/// the expectations; and, when they pass, its outputs handed to the
/// assembler. The outcome as one JSON object, and whether it passed.
#[must_use]
pub fn run(drive: &std::path::Path, options: &RunOptions) -> (serde_json::Value, bool) {
    let mut said = serde_json::json!({"ok": false});
    let passed = match running(drive, options, &mut said) {
        Ok(passed) => passed,
        Err(Failure(why)) => {
            said["failure"] = serde_json::Value::String(why);
            false
        }
    };
    said["ok"] = serde_json::Value::Bool(passed);
    (said, passed)
}

/// A file beside the scenario, or as written when absolute.
fn beside(file: &std::path::Path, named: &str) -> std::path::PathBuf {
    file.parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join(named)
}

/// Copy each of `seed`'s files, beside the scenario `file`, into `worktree`
/// at its path there.
fn seeded(
    worktree: &std::path::Path,
    file: &std::path::Path,
    seed: &BTreeMap<String, String>,
) -> Result<(), Failure> {
    for (path, source) in seed {
        let to = worktree.join(path);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|why| Failure(format!("{} cannot be made: {why}", parent.display())))?;
        }
        std::fs::copy(beside(file, source), &to)
            .map_err(|why| Failure(format!("the seed `{source}` cannot be copied: {why}")))?;
    }
    Ok(())
}

/// A directory no other run uses, under the system's temporary one.
fn fresh(what: &str) -> Result<std::path::PathBuf, Failure> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let dir = std::env::temp_dir().join(format!(
        "diet-scenario-{what}-{}-{stamp}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir)
        .map_err(|why| Failure(format!("{} cannot be made: {why}", dir.display())))?;
    Ok(dir)
}

/// Copy `work/files/` into `out/files/`, name for name: the recording's
/// copies of the files its asks attached. A partial copy serve never
/// renamed into place (a dot-named temporary) is not one.
fn kept_files(work: &std::path::Path, out: &std::path::Path) -> Result<(), Failure> {
    let from = work.join("files");
    let entries = match std::fs::read_dir(&from) {
        Ok(entries) => entries,
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(why) => return Err(Failure(format!("{} cannot be read: {why}", from.display()))),
    };
    let to = out.join("files");
    std::fs::create_dir_all(&to)
        .map_err(|why| Failure(format!("{} cannot be made: {why}", to.display())))?;
    for entry in entries {
        let entry =
            entry.map_err(|why| Failure(format!("{} cannot be read: {why}", from.display())))?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') || !entry.path().is_file() {
            continue;
        }
        std::fs::copy(entry.path(), to.join(&name))
            .map_err(|why| Failure(format!("{} cannot be kept: {why}", entry.path().display())))?;
    }
    Ok(())
}

#[allow(clippy::too_many_lines)]
fn running(
    drive: &std::path::Path,
    options: &RunOptions,
    said: &mut serde_json::Value,
) -> Result<bool, Failure> {
    use std::io::BufRead as _;
    let text = std::fs::read_to_string(&options.file)
        .map_err(|why| Failure(format!("{} cannot be read: {why}", options.file.display())))?;
    let scenario = parse(&text).map_err(|why| Failure(why.to_string()))?;
    said["scenario"] = serde_json::Value::String(scenario.name.clone());
    let live = options.endpoint.is_some();
    let regimen = match (&options.regimen, live, &scenario.live_regimen) {
        (Some(regimen), _, _) => regimen.clone(),
        (None, false, _) => beside(&options.file, &scenario.regimen),
        (None, true, Some(live_regimen)) => beside(&options.file, live_regimen),
        (None, true, None) => {
            return Err(Failure(
                "a re-fire on an endpoint needs the scenario's `live_regimen` or --regimen"
                    .to_owned(),
            ));
        }
    };
    let read = std::fs::read_to_string(&regimen)
        .map_err(|why| Failure(format!("{} cannot be read: {why}", regimen.display())))
        .and_then(|text| {
            crate::formats::regimen::parse(&text)
                .map_err(|why| Failure(format!("{}: {why:?}", regimen.display())))
        })?;
    let substrate = match read.get("substrate") {
        Some(crate::formats::regimen::Value::String(id)) => id.clone(),
        _ => return Err(Failure(format!("{} names no substrate", regimen.display()))),
    };
    said["substrate"] = serde_json::Value::String(substrate.clone());
    // Serve's own reading of whether the regimen runs commands.
    let runs_commands = crate::drive::tool_loop::declared(&read)
        .map_err(|why| Failure(format!("{}: {why}", regimen.display())))?
        .is_some();
    // The canned substrate: the scenario's own replies, routed.
    let stub = if live {
        None
    } else {
        said["replies_sha256"] = serde_json::Value::String(scenario.replies_digest());
        Some(
            crate::client::stub::Stub::routing(
                scenario.routes(),
                &crate::drive::canned::scenario_build_info(),
                0,
            )
            .map_err(|why| Failure(format!("the canned substrate cannot listen: {why}")))?,
        )
    };
    let endpoint = match (&options.endpoint, &stub) {
        (Some(endpoint), _) => endpoint.clone(),
        (None, Some(stub)) => stub.url(),
        (None, None) => unreachable!("a run is live or canned"),
    };
    let work = if options.assemble {
        fresh("work")?
    } else {
        std::fs::create_dir_all(&options.out)
            .map_err(|why| Failure(format!("{} cannot be made: {why}", options.out.display())))?;
        options.out.clone()
    };
    let (log, record) = (work.join("session.log"), work.join("session.record.jsonl"));
    said["log"] = serde_json::Value::String(log.display().to_string());
    said["record"] = serde_json::Value::String(record.display().to_string());
    // Every route asks for this credential; it never reaches the outputs.
    let secrets = fresh("auth")?;
    let user_password = format!(
        "scenario:{}",
        crate::digest::sha256_hex(
            format!("{:?}{}", std::time::SystemTime::now(), std::process::id()).as_bytes()
        )
    );
    let auth_file = secrets.join("drive.auth");
    std::fs::write(&auth_file, &user_password)
        .map_err(|why| Failure(format!("the auth file cannot be written: {why}")))?;
    let mut serve = std::process::Command::new(drive);
    serve
        .arg("serve")
        .arg("--endpoint")
        .arg(&endpoint)
        .arg("--model")
        .arg(options.model.as_deref().unwrap_or("scenario"))
        .arg("--head")
        .arg(beside(&options.file, &scenario.head))
        .arg("--regimen")
        .arg(&regimen)
        .arg("--port")
        .arg("0")
        .arg("--auth-file")
        .arg(&auth_file)
        .arg("--log")
        .arg(&log)
        .arg("--record")
        .arg(&record)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped());
    if let Some(key_file) = &options.key_file {
        serve.arg("--key-file").arg(key_file);
    }
    let mut worktree = None;
    if runs_commands {
        let dir = match &options.worktree {
            Some(worktree) => worktree.clone(),
            None => fresh("worktree")?,
        };
        seeded(&dir, &options.file, &scenario.seed)?;
        serve.arg("--worktree").arg(&dir);
        worktree = Some(dir);
    } else if !scenario.seed.is_empty() {
        return Err(Failure(
            "the scenario seeds a worktree and its regimen runs no commands".to_owned(),
        ));
    }
    let mut child = serve
        .spawn()
        .map_err(|why| Failure(format!("serve cannot start: {why}")))?;
    let stdout = child.stdout.take().expect("piped");
    let mut lines = std::io::BufReader::new(stdout).lines();
    let first = lines.next().and_then(Result::ok).unwrap_or_default();
    // The rest of serve's stdout is read as it comes, so it never blocks.
    let drained = std::thread::spawn(move || lines.map_while(Result::ok).collect::<Vec<_>>());
    let started: serde_json::Value = serde_json::from_str(&first).unwrap_or_default();
    let Some(listening) = started["listening"].as_str() else {
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&secrets);
        return Err(Failure(format!("serve did not start: {first}")));
    };
    let mut driver = Driver::new(listening, Some(&user_password));
    if let Some(worktree) = worktree {
        driver = driver.in_worktree(worktree);
    }
    let outcome = self::drive(&scenario, &driver);
    if outcome.is_err() {
        let _ = driver.command(&serde_json::json!({"kind": "end"}));
    }
    // Serve exits once the session ends and its record is written.
    let give_up = std::time::Instant::now() + std::time::Duration::from_secs(120);
    let exited = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if std::time::Instant::now() < give_up => {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let _ = drained.join();
    let _ = std::fs::remove_dir_all(&secrets);
    let unmatched = stub
        .as_ref()
        .map(crate::client::stub::Stub::unmatched)
        .unwrap_or_default();
    said["unmatched"] = serde_json::json!(unmatched);
    let bound = outcome?;
    if exited.is_none_or(|status| !status.success()) {
        return Err(Failure(format!("serve did not exit cleanly: {exited:?}")));
    }
    let written = std::fs::read_to_string(&log)
        .map_err(|why| Failure(format!("the log cannot be read: {why}")))?;
    let missed = unmet(&scenario, &written, &bound);
    said["unmet"] = serde_json::json!(missed);
    if !missed.is_empty() || !unmatched.is_empty() {
        return Ok(false);
    }
    if options.assemble {
        let mut driven_by = format!("scripted: scenario {}", scenario.name);
        if live {
            let _ = write!(driven_by, ", re-fired on {substrate}");
        } else {
            let _ = write!(
                driven_by,
                ", canned replies sha256 {}",
                scenario.replies_digest()
            );
        }
        let assembled = std::process::Command::new("python3")
            .arg("scripts/assemble-run.py")
            .arg("--log")
            .arg(&log)
            .arg("--record")
            .arg(&record)
            .arg("--regimen")
            .arg(&regimen)
            .arg("--out")
            .arg(&options.out)
            .arg("--driven-by")
            .arg(&driven_by)
            .output()
            .map_err(|why| Failure(format!("the assembler cannot run: {why}")))?;
        let reply = String::from_utf8_lossy(&assembled.stdout);
        said["assembled"] = serde_json::from_str(reply.lines().last().unwrap_or(""))
            .unwrap_or_else(|_| serde_json::Value::String(reply.into_owned()));
        if !assembled.status.success() {
            return Ok(false);
        }
        // The copies of the files its asks attached are the run's too, kept
        // as serve's recording keeps them (`files/<sha256>`, #372): the log
        // names them there and a replay fetches them from the run.
        kept_files(&work, &options.out)?;
        // The assembled copies, scrubbed, are the run's; the work goes.
        said["log"] =
            serde_json::Value::String(options.out.join("session.log").display().to_string());
        said["record"] =
            serde_json::Value::String(options.out.join("record.jsonl").display().to_string());
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = include_str!("../../scenarios/fixtures/minimal.jsonl");

    #[test]
    #[allow(clippy::too_many_lines)]
    fn the_minimal_fixture_reads_line_for_line() {
        let read = parse(MINIMAL).expect("it reads");
        assert_eq!(read.name, "minimal");
        assert_eq!(read.regimen, "minimal.toml");
        assert_eq!(read.live_regimen.as_deref(), Some("../drive/floor.toml"));
        assert_eq!(read.head, "../drive/heads/floor.md");
        assert_eq!(read.approvals.as_deref(), Some("session"));
        assert_eq!(
            read.seed,
            BTreeMap::from([("AGENTS.md".to_owned(), "minimal.agents.md".to_owned())])
        );
        assert_eq!(
            read.replies,
            [
                Reply {
                    when: Some("Record the decisions".to_owned()),
                    text: "DECISION: a tracker".to_owned(),
                    calls: Vec::new(),
                    delay_ms: None,
                    prompt_tokens: None,
                },
                Reply {
                    when: None,
                    text: "Writing it.".to_owned(),
                    calls: vec![Call {
                        id: "call-w".to_owned(),
                        name: "write".to_owned(),
                        arguments: r#"{"path":"a.py","content":"x"}"#.to_owned(),
                    }],
                    delay_ms: None,
                    prompt_tokens: None,
                },
                Reply {
                    when: None,
                    text: "Done.".to_owned(),
                    calls: Vec::new(),
                    delay_ms: Some(250),
                    prompt_tokens: Some(1200),
                },
            ]
        );
        assert_eq!(
            read.steps,
            [
                Step::StartPhase {
                    phase: "orient".to_owned(),
                },
                Step::Ask {
                    text: "write a.py".to_owned(),
                    scoping: true,
                    refused: None,
                    attach: Vec::new(),
                },
                Step::Ask {
                    text: "what is in shot.png".to_owned(),
                    scoping: false,
                    refused: None,
                    attach: vec!["shot.png".to_owned()],
                },
                Step::OpenTangent {
                    id: "t/1".to_owned(),
                },
                Step::CloseTangent {
                    each: "keep".to_owned(),
                },
                Step::Flag {
                    pick: Pick::Index(0),
                    note: "stale?".to_owned(),
                    bind: Some("flagged".to_owned()),
                },
                Step::Edit {
                    pick: Pick::Containing("tracker".to_owned()),
                    text: "decision: a ledger".to_owned(),
                    bind: Some("edited".to_owned()),
                },
                Step::DeclareMeditation,
                Step::Wait {
                    until: "meditation.held".to_owned(),
                    timeout: Some(60),
                },
                Step::Ask {
                    text: "go on".to_owned(),
                    scoping: false,
                    refused: Some("meditation-held".to_owned()),
                    attach: Vec::new(),
                },
                Step::RefillAnyway,
                Step::RulePhase {
                    call: "call-p".to_owned(),
                    choice: "keep".to_owned(),
                },
                Step::ProposePhase {
                    to: "build".to_owned(),
                    meditate: Some(false),
                },
                Step::MovePhase {
                    to: "plan".to_owned(),
                    meditate: None,
                },
                Step::End,
            ]
        );
        assert_eq!(
            read.expects,
            [
                Expect {
                    kind: "fork".to_owned(),
                    fields: vec![
                        ("trigger".to_owned(), Match::Contains("call:".to_owned())),
                        ("why".to_owned(), Match::Equals("read".to_owned())),
                    ],
                    at_least: 2,
                    name: Some("fork".to_owned()),
                    after: None,
                },
                Expect {
                    kind: "meditation.held".to_owned(),
                    fields: vec![(
                        "unaddressed".to_owned(),
                        Match::Contains("$flagged".to_owned())
                    )],
                    at_least: 1,
                    name: None,
                    after: Some("fork".to_owned()),
                },
            ]
        );
    }

    /// The replies' digest moves with any reply and with nothing else.
    #[test]
    fn the_replies_digest_is_of_the_replies_alone() {
        let read = parse(MINIMAL).expect("it reads");
        let digest = read.replies_digest();
        assert_eq!(digest.len(), 64);
        let mut fewer_steps = read.clone();
        fewer_steps.steps.pop();
        fewer_steps.expects.clear();
        assert_eq!(fewer_steps.replies_digest(), digest);
        let mut other = read.clone();
        other.replies[2].text.push('!');
        assert_ne!(other.replies_digest(), digest);
        assert_eq!(read.routes().len(), 3);
    }

    /// Every way a line can be wrong is refused, naming its line.
    #[test]
    fn a_line_it_cannot_read_is_refused_by_its_number() {
        let header = r#"{"scenario":"s","regimen":"r.toml","head":"h.md"}"#;
        for (body, at) in [
            (r#"{"step":"ask"}"#, 2),
            (r#"{"step":"dance"}"#, 2),
            (r#"{"step":"ask","text":"a","loud":true}"#, 2),
            (r#"{"step":"flag","pick":-1,"note":"n"}"#, 2),
            (r#"{"reply":{"when":"x"}}"#, 2),
            (r#"{"expect":"fork","where":{"a":1}}"#, 2),
            (r#"{"expect":"fork","after":"nobody"}"#, 2),
            (r#"{"expect":"fork","at_least":0}"#, 2),
            (r#"{"step":"end","reply":{"text":"x"}}"#, 2),
            (r#"{"step":"wait"}"#, 2),
        ] {
            let text = format!("{header}\n{body}\n");
            assert!(
                matches!(parse(&text), Err(ScenarioError::Line(line, _)) if line == at),
                "{body}: {:?}",
                parse(&text)
            );
        }
        assert!(matches!(
            parse(r#"{"step":"end"}"#),
            Err(ScenarioError::Line(1, _))
        ));
        assert!(matches!(
            parse(&format!("{header}\n{{\"step\":\"end\"}}\n{header}\n")),
            Err(ScenarioError::Line(3, _))
        ));
        assert!(matches!(
            parse("{\"scenario\":\n\"s\"}"),
            Err(ScenarioError::Syntax(_))
        ));
        assert!(matches!(
            parse(&format!(
                "{}\n",
                r#"{"scenario":"s","regimen":"r.toml","head":"h.md","approvals":"always"}"#
            )),
            Err(ScenarioError::Line(1, _))
        ));
    }
}
