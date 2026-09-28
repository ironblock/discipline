//! The session event log, v0 (#117, ruled 2026-09-26).
//!
//! `diet/formats/log/grammar.pest` says what a log document is: one event
//! per line, in the record's value space. This module is its one reader and
//! its one writer. What each `kind` carries, and the rules that span lines,
//! are checked here under a name, as the record checks its own.
//!
//! Every line carries `seq` (its position, the primary key), `t`
//! (milliseconds since the session opened) and `kind`. The vocabulary is
//! CLOSED: an unknown kind is not a line of v0, and a later kind is a
//! versioned bump, because a line nobody can conformance-test is a line two
//! readers can disagree about.
//!
//! The draft is `diet/drive/plans/r2c-proposal.md`, D4, as ruled on #117:
//! names from a ruling first, then the record, then the drive's own tags. A
//! cancelled call is `cancelled`, never a `response`; the drive's rejected,
//! failed and crashed calls are one `request.failed` with a reason, because
//! the record's `rejected` means something else. References are the `seq` of
//! the `request` they answer -- an identifier the log issues, not one anybody
//! invents.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use pest::Parser;
use pest_derive::Parser;

use super::record::json::{self, Value};
use super::record::vocabulary;

#[derive(Parser)]
#[grammar = "../formats/record/grammar.pest"]
#[grammar = "../formats/log/grammar.pest"]
#[grammar = "../formats/number.pest"]
struct LogParser;

/// The version this module reads and writes, as `session.start` states it.
pub const VERSION: i64 = 0;

vocabulary! {
    /// What each line is.
    Kind {
        /// The session opened. The first line, and only the first.
        SessionStart => "session.start",
        /// An ask was admitted, and a turn begins on it.
        Ask => "ask",
        /// The session's state moved.
        Settlement => "settlement",
        /// A call was made to the model.
        Request => "request",
        /// A command was refused.
        Refused => "refused",
        /// A piece of a call's answer arrived.
        Delta => "delta",
        /// A stop was asked for a turn's call.
        StopAsked => "stop.asked",
        /// A call answered.
        Response => "response",
        /// A call was stopped.
        Cancelled => "cancelled",
        /// A call ended without an answer: refused by the server, failed, or
        /// its thread crashed.
        RequestFailed => "request.failed",
        /// A turn is over.
        TurnSettled => "turn.settled",
        /// A person's idle gap after a settled turn, as the surface measured
        /// it (Q4, ruled on #117 2026-09-27).
        IdleGap => "idle.gap",
    }
}

vocabulary! {
    /// What ended an idle gap.
    GapEnd {
        /// An ask was sent.
        Ask => "ask",
        /// A seam was declared.
        Seam => "seam",
        /// A stop was sent.
        Cancel => "cancel",
        /// The session ended.
        End => "end",
    }
}

vocabulary! {
    /// What the session is doing.
    State {
        /// Nothing in flight; an ask is welcome.
        Awaiting => "awaiting",
        /// The trunk is answering.
        Turn => "turn",
        /// The idle-gap work after a settled turn.
        Capture => "capture",
        /// Over.
        Ended => "ended",
    }
}

vocabulary! {
    /// A command a person sent.
    Command {
        /// An ask.
        Ask => "ask",
        /// A stop.
        Cancel => "cancel",
        /// A seam.
        DeclareSeam => "declare-seam",
        /// The end.
        End => "end",
    }
}

vocabulary! {
    /// Why a command was refused.
    Refusal {
        /// A turn or a capture is in flight.
        InFlight => "in-flight",
        /// The session has ended.
        Ended => "ended",
        /// The turn named has no call in flight.
        NothingInFlight => "nothing-in-flight",
        /// Seams are not built yet.
        SeamNotBuilt => "seam-not-built",
        /// A stop named a turn older than the latest.
        Stale => "stale",
    }
}

vocabulary! {
    /// Which lane a request was made on.
    Lane {
        /// The canonical session.
        Trunk => "trunk",
    }
}

vocabulary! {
    /// Who a head message is from.
    Role {
        /// The standing instruction.
        System => "system",
        /// The person.
        User => "user",
        /// The model.
        Assistant => "assistant",
    }
}

vocabulary! {
    /// Why a call ended without an answer.
    FailReason {
        /// The server refused it.
        Server => "server",
        /// It ran out of time.
        Timeout => "timeout",
        /// The connection failed.
        Transport => "transport",
        /// The thread making it crashed.
        Crashed => "crashed",
    }
}

vocabulary! {
    /// How a turn ended.
    SettleReason {
        /// Answered.
        Final => "final",
        /// Stopped.
        Cancelled => "cancelled",
        /// The tool loop ran out of steps.
        MaxSteps => "max_steps",
        /// Out of time.
        Timeout => "timeout",
        /// Anything else that ended it without an answer.
        Failed => "failed",
    }
}

/// One message of the head the trunk starts from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadMessage {
    /// Who it is from.
    pub role: Role,
    /// What it says.
    pub content: String,
}

/// What one delta carries: exactly one of the two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Piece {
    /// Answer text.
    Text(String),
    /// Reasoning text, from a model that thinks.
    Reasoning(String),
}

/// What happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The session opened.
    SessionStart {
        /// When: milliseconds since the Unix epoch.
        opened: u64,
        /// The model name requests are sent with -- a name, not an identity.
        model: String,
        /// The messages the trunk starts from.
        head: Vec<HeadMessage>,
    },
    /// An ask was admitted.
    Ask {
        /// The turn it begins, from 1.
        turn: u32,
        /// What was asked.
        text: String,
    },
    /// The state moved.
    Settlement {
        /// Where it was.
        from: State,
        /// Where it is now.
        to: State,
    },
    /// A call was made.
    Request {
        /// The turn it belongs to.
        turn: u32,
        /// The lane it was made on.
        lane: Lane,
    },
    /// A command was refused.
    Refused {
        /// Which command.
        command: Command,
        /// Why.
        because: Refusal,
        /// What the session was doing.
        during: State,
    },
    /// A piece of an answer.
    Delta {
        /// The `seq` of the `request` it answers.
        request: u64,
        /// What arrived.
        piece: Piece,
    },
    /// A stop was asked.
    StopAsked {
        /// The turn it was asked for.
        turn: u32,
    },
    /// A call answered.
    Response {
        /// The `seq` of the `request` it answers.
        to_request: u64,
        /// The whole answer.
        text: String,
        /// Why the server stopped, as it spelled it, if it said.
        finish_reason: Option<String>,
    },
    /// A call was stopped.
    Cancelled {
        /// The `seq` of the `request` stopped.
        request: u64,
        /// What arrived before the stop. Never an answer.
        partial: String,
    },
    /// A call ended without an answer.
    RequestFailed {
        /// The `seq` of the `request`.
        request: u64,
        /// Why.
        reason: FailReason,
        /// What the server, the transport or the panic said.
        message: String,
        /// The HTTP status, when the server refused it.
        status: Option<u16>,
        /// What arrived before it ended, when anything did.
        partial: Option<String>,
    },
    /// A turn is over.
    TurnSettled {
        /// The turn.
        turn: u32,
        /// How it ended.
        reason: SettleReason,
    },
    /// An idle gap, emitted by the surface once, when the gap ends. Integer
    /// milliseconds on the surface's monotonic clock; durations only, never
    /// content (Q4).
    IdleGap {
        /// The `seq` of the `turn.settled` that opened the gap.
        opened_by: u64,
        /// From the settling to the first sign the person is present.
        notice: u64,
        /// From the end of `notice` to the first keystroke or seam click.
        read: u64,
        /// From the first keystroke to the accepted send or declare.
        compose: u64,
        /// The time the page was hidden, taken out of the phase it
        /// interrupted.
        away: u64,
        /// From the first send refused because work was in flight to the
        /// accepted send.
        blocked: u64,
        /// What ended it.
        ended_by: GapEnd,
    },
}

/// One line of a log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// Its position in the log: gapless, from 0.
    pub seq: u64,
    /// Milliseconds since the session opened.
    pub t: u64,
    /// What happened.
    pub event: Event,
}

/// Why a log was not read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogError {
    /// The 1-based line it is about; 0 when it is about the whole text.
    pub line: usize,
    /// What is wrong.
    pub why: String,
}

impl fmt::Display for LogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.why)
        } else {
            write!(f, "line {}: {}", self.line, self.why)
        }
    }
}

impl std::error::Error for LogError {}

/// Read one line: what a reader that resumes mid-log reads. The rules that
/// span lines are [`parse`]'s.
///
/// # Errors
///
/// When the text is not one object line of the record's value space, or the
/// object is not a line of v0.
pub fn line(text: &str) -> Result<Line, String> {
    let object = json::line(text).map_err(|err| err.to_string())?;
    from_object(&object)
}

/// Read a whole log, including the rules that span lines.
///
/// # Errors
///
/// [`LogError`] naming the first line that is not v0, or the first rule a
/// line breaks.
pub fn parse(text: &str) -> Result<Vec<Line>, LogError> {
    let document = LogParser::parse(Rule::log_document, text).map_err(|err| {
        let line = match err.line_col {
            pest::error::LineColLocation::Pos((line, _))
            | pest::error::LineColLocation::Span((line, _), _) => line,
        };
        let written = text.split('\n').nth(line.saturating_sub(1)).unwrap_or("");
        let why = if written.is_empty() {
            "a blank line: one event per line and nothing between them"
        } else if written.starts_with([' ', '\t']) {
            "an indented line: each event starts its line"
        } else {
            "not one event per line"
        };
        LogError {
            line,
            why: why.to_owned(),
        }
    })?;
    let mut lines = Vec::new();
    let log_lines = document
        .flatten()
        .filter(|pair| pair.as_rule() == Rule::log_line);
    for (index, pair) in log_lines.enumerate() {
        let object = pair
            .into_inner()
            .find(|inner| inner.as_rule() == Rule::object)
            .map_or("", |object| object.as_str());
        lines.push(line(object).map_err(|why| LogError {
            line: index + 1,
            why,
        })?);
    }
    check(&lines)?;
    Ok(lines)
}

/// Write one line: sorted keys, no raw line break, so one event is one line.
#[must_use]
pub fn render(line: &Line) -> String {
    let mut out = String::new();
    json::render(&to_value(line), &mut out);
    out
}

/// Parse `source` and project it: every line, as it reads back. What
/// `diet check-log` prints and the conformance corpus pins.
///
/// # Errors
///
/// When `source` is not a v0 log.
pub fn project(source: &str) -> Result<Value, String> {
    parse(source)
        .map(|lines| Value::Array(lines.iter().map(to_value).collect()))
        .map_err(|err| err.to_string())
}

// ---------------------------------------------------------------------------
// the rules that span lines
// ---------------------------------------------------------------------------

/// The first line is `session.start`: it carries the version, so a reader
/// knows what it is reading before it reads anything else.
fn begins_with_the_session(lines: &[Line]) -> Result<(), LogError> {
    if matches!(
        lines.first().map(|line| &line.event),
        Some(Event::SessionStart { .. })
    ) {
        Ok(())
    } else {
        Err(LogError {
            line: 1,
            why: "the first line is not `session.start`".to_owned(),
        })
    }
}

/// Every rule that no single line can break alone.
fn check(lines: &[Line]) -> Result<(), LogError> {
    let at = |index: usize, why: String| LogError {
        line: index + 1,
        why,
    };
    begins_with_the_session(lines)?;
    let mut state = State::Awaiting;
    let mut turns = 0_u32;
    let mut settled = BTreeSet::new();
    let mut requests = BTreeSet::new();
    let mut settlings = BTreeSet::new();
    let mut last_t = 0_u64;
    for (index, line) in lines.iter().enumerate() {
        if line.seq != index as u64 {
            return Err(at(
                index,
                format!(
                    "`seq` is {}, where the line's position is {index}",
                    line.seq
                ),
            ));
        }
        if line.t < last_t {
            return Err(at(
                index,
                format!("`t` went back from {last_t} to {}", line.t),
            ));
        }
        last_t = line.t;
        match &line.event {
            Event::SessionStart { .. } if index > 0 => {
                return Err(at(index, "a second `session.start`".to_owned()));
            }
            Event::Settlement { from, to } => {
                if *from != state {
                    return Err(at(
                        index,
                        format!(
                            "a settlement from `{}` where the state is `{}`",
                            from.tag(),
                            state.tag()
                        ),
                    ));
                }
                state = *to;
            }
            Event::Ask { turn, .. } => {
                if *turn != turns + 1 {
                    return Err(at(index, format!("turn {turn} follows turn {turns}")));
                }
                turns = *turn;
            }
            Event::Request { turn, .. } => {
                if *turn == 0 || *turn != turns {
                    return Err(at(
                        index,
                        format!("a request for turn {turn} where the latest is {turns}"),
                    ));
                }
                requests.insert(line.seq);
            }
            Event::StopAsked { turn } if *turn == 0 || *turn > turns => {
                return Err(at(index, format!("a stop for turn {turn}, never asked")));
            }
            Event::TurnSettled { turn, .. } => {
                if *turn == 0 || *turn > turns {
                    return Err(at(index, format!("turn {turn} settled, never asked")));
                }
                if !settled.insert(*turn) {
                    return Err(at(index, format!("turn {turn} settled twice")));
                }
                settlings.insert(line.seq);
            }
            Event::IdleGap { opened_by, .. } if !settlings.contains(opened_by) => {
                return Err(at(
                    index,
                    format!("seq {opened_by} is not an earlier `turn.settled`"),
                ));
            }
            Event::Delta { request, .. }
            | Event::Response {
                to_request: request,
                ..
            }
            | Event::Cancelled { request, .. }
            | Event::RequestFailed { request, .. }
                if !requests.contains(request) =>
            {
                return Err(at(
                    index,
                    format!("seq {request} is not an earlier `request`"),
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// one line, both ways
// ---------------------------------------------------------------------------

/// The keys every line carries.
const COMMON: &[&str] = &["seq", "t", "kind"];

/// A line from its object: every key it must have, none it may not.
fn from_object(object: &BTreeMap<String, Value>) -> Result<Line, String> {
    let fields = Fields(object);
    let kind_tag = fields.string("kind")?;
    let kind = Kind::from_tag(&kind_tag).ok_or_else(|| format!("unknown kind `{kind_tag}`"))?;
    let (required, optional) = keys(kind);
    for key in object.keys() {
        let known = COMMON.contains(&key.as_str())
            || required.contains(&key.as_str())
            || optional.contains(&key.as_str());
        if !known {
            return Err(format!("`{kind_tag}` carries no `{key}`"));
        }
    }
    let event = match kind {
        Kind::SessionStart => {
            let version = fields.integer("version")?;
            if version != VERSION {
                return Err(format!(
                    "version {version}, where this reader reads {VERSION}"
                ));
            }
            Event::SessionStart {
                opened: fields.count("opened")?,
                model: fields.string("model")?,
                head: fields.head("head")?,
            }
        }
        Kind::Ask => Event::Ask {
            turn: fields.turn("turn")?,
            text: fields.string("text")?,
        },
        Kind::Settlement => Event::Settlement {
            from: fields.tag("from", State::from_tag)?,
            to: fields.tag("to", State::from_tag)?,
        },
        Kind::Request => Event::Request {
            turn: fields.turn("turn")?,
            lane: fields.tag("lane", Lane::from_tag)?,
        },
        Kind::Refused => Event::Refused {
            command: fields.tag("command", Command::from_tag)?,
            because: fields.tag("because", Refusal::from_tag)?,
            during: fields.tag("during", State::from_tag)?,
        },
        Kind::Delta => Event::Delta {
            request: fields.count("request")?,
            piece: match (object.get("text"), object.get("reasoning")) {
                (Some(_), None) => Piece::Text(fields.string("text")?),
                (None, Some(_)) => Piece::Reasoning(fields.string("reasoning")?),
                (Some(_), Some(_)) => {
                    return Err("a delta carries both `text` and `reasoning`".to_owned());
                }
                (None, None) => {
                    return Err("a delta carries neither `text` nor `reasoning`".to_owned());
                }
            },
        },
        Kind::StopAsked => Event::StopAsked {
            turn: fields.turn("turn")?,
        },
        Kind::Response => Event::Response {
            to_request: fields.count("to_request")?,
            text: fields.string("text")?,
            finish_reason: fields.optional_string("finish_reason")?,
        },
        Kind::Cancelled => Event::Cancelled {
            request: fields.count("request")?,
            partial: fields.string("partial")?,
        },
        Kind::RequestFailed => Event::RequestFailed {
            request: fields.count("request")?,
            reason: fields.tag("reason", FailReason::from_tag)?,
            message: fields.string("message")?,
            status: match object.get("status") {
                None => None,
                Some(_) => Some(
                    u16::try_from(fields.count("status")?)
                        .map_err(|_| "`status` is not an HTTP status".to_owned())?,
                ),
            },
            partial: fields.optional_string("partial")?,
        },
        Kind::TurnSettled => Event::TurnSettled {
            turn: fields.turn("turn")?,
            reason: fields.tag("reason", SettleReason::from_tag)?,
        },
        Kind::IdleGap => Event::IdleGap {
            opened_by: fields.count("opened_by")?,
            notice: fields.count("notice")?,
            read: fields.count("read")?,
            compose: fields.count("compose")?,
            away: fields.count("away")?,
            blocked: fields.count("blocked")?,
            ended_by: fields.tag("ended_by", GapEnd::from_tag)?,
        },
    };
    Ok(Line {
        seq: fields.count("seq")?,
        t: fields.count("t")?,
        event,
    })
}

/// The keys a kind must carry, then the keys it may.
fn keys(kind: Kind) -> (&'static [&'static str], &'static [&'static str]) {
    match kind {
        Kind::SessionStart => (&["version", "opened", "model", "head"], &[]),
        Kind::Ask => (&["turn", "text"], &[]),
        Kind::Settlement => (&["from", "to"], &[]),
        Kind::Request => (&["turn", "lane"], &[]),
        Kind::Refused => (&["command", "because", "during"], &[]),
        Kind::Delta => (&["request"], &["text", "reasoning"]),
        Kind::StopAsked => (&["turn"], &[]),
        Kind::Response => (&["to_request", "text"], &["finish_reason"]),
        Kind::Cancelled => (&["request", "partial"], &[]),
        Kind::RequestFailed => (&["request", "reason", "message"], &["status", "partial"]),
        Kind::TurnSettled => (&["turn", "reason"], &[]),
        Kind::IdleGap => (
            &[
                "opened_by",
                "notice",
                "read",
                "compose",
                "away",
                "blocked",
                "ended_by",
            ],
            &[],
        ),
    }
}

/// A line as the record's value space holds it.
///
/// One arm per kind, each the mirror of its arm in [`from_object`]: split
/// apart, the two directions of a kind would sit in different places.
#[allow(clippy::too_many_lines)]
fn to_value(line: &Line) -> Value {
    let mut object = BTreeMap::new();
    let mut put = |key: &str, value: Value| {
        object.insert(key.to_owned(), value);
    };
    put("seq", count(line.seq));
    put("t", count(line.t));
    let kind = match &line.event {
        Event::SessionStart {
            opened,
            model,
            head,
        } => {
            put("version", Value::Integer(VERSION));
            put("opened", count(*opened));
            put("model", text(model));
            put(
                "head",
                Value::Array(
                    head.iter()
                        .map(|message| {
                            Value::Object(BTreeMap::from([
                                ("role".to_owned(), text(message.role.tag())),
                                ("content".to_owned(), text(&message.content)),
                            ]))
                        })
                        .collect(),
                ),
            );
            Kind::SessionStart
        }
        Event::Ask { turn, text: asked } => {
            put("turn", count(u64::from(*turn)));
            put("text", text(asked));
            Kind::Ask
        }
        Event::Settlement { from, to } => {
            put("from", text(from.tag()));
            put("to", text(to.tag()));
            Kind::Settlement
        }
        Event::Request { turn, lane } => {
            put("turn", count(u64::from(*turn)));
            put("lane", text(lane.tag()));
            Kind::Request
        }
        Event::Refused {
            command,
            because,
            during,
        } => {
            put("command", text(command.tag()));
            put("because", text(because.tag()));
            put("during", text(during.tag()));
            Kind::Refused
        }
        Event::Delta { request, piece } => {
            put("request", count(*request));
            match piece {
                Piece::Text(piece) => put("text", text(piece)),
                Piece::Reasoning(piece) => put("reasoning", text(piece)),
            }
            Kind::Delta
        }
        Event::StopAsked { turn } => {
            put("turn", count(u64::from(*turn)));
            Kind::StopAsked
        }
        Event::Response {
            to_request,
            text: answer,
            finish_reason,
        } => {
            put("to_request", count(*to_request));
            put("text", text(answer));
            if let Some(reason) = finish_reason {
                put("finish_reason", text(reason));
            }
            Kind::Response
        }
        Event::Cancelled { request, partial } => {
            put("request", count(*request));
            put("partial", text(partial));
            Kind::Cancelled
        }
        Event::RequestFailed {
            request,
            reason,
            message,
            status,
            partial,
        } => {
            put("request", count(*request));
            put("reason", text(reason.tag()));
            put("message", text(message));
            if let Some(status) = status {
                put("status", count(u64::from(*status)));
            }
            if let Some(partial) = partial {
                put("partial", text(partial));
            }
            Kind::RequestFailed
        }
        Event::TurnSettled { turn, reason } => {
            put("turn", count(u64::from(*turn)));
            put("reason", text(reason.tag()));
            Kind::TurnSettled
        }
        Event::IdleGap {
            opened_by,
            notice,
            read,
            compose,
            away,
            blocked,
            ended_by,
        } => {
            put("opened_by", count(*opened_by));
            put("notice", count(*notice));
            put("read", count(*read));
            put("compose", count(*compose));
            put("away", count(*away));
            put("blocked", count(*blocked));
            put("ended_by", text(ended_by.tag()));
            Kind::IdleGap
        }
    };
    put("kind", text(kind.tag()));
    Value::Object(object)
}

fn count(number: u64) -> Value {
    Value::Integer(i64::try_from(number).unwrap_or(i64::MAX))
}

fn text(value: &str) -> Value {
    Value::String(value.to_owned())
}

/// A line's object, read field by field with the field's name in every
/// error.
struct Fields<'a>(&'a BTreeMap<String, Value>);

impl Fields<'_> {
    fn get(&self, key: &str) -> Result<&Value, String> {
        self.0.get(key).ok_or_else(|| format!("no `{key}`"))
    }

    fn string(&self, key: &str) -> Result<String, String> {
        match self.get(key)? {
            Value::String(text) => Ok(text.clone()),
            _ => Err(format!("`{key}` is not a string")),
        }
    }

    fn optional_string(&self, key: &str) -> Result<Option<String>, String> {
        match self.0.get(key) {
            None => Ok(None),
            Some(_) => self.string(key).map(Some),
        }
    }

    fn integer(&self, key: &str) -> Result<i64, String> {
        match self.get(key)? {
            Value::Integer(number) => Ok(*number),
            _ => Err(format!("`{key}` is not an integer")),
        }
    }

    fn count(&self, key: &str) -> Result<u64, String> {
        u64::try_from(self.integer(key)?).map_err(|_| format!("`{key}` is negative"))
    }

    fn turn(&self, key: &str) -> Result<u32, String> {
        u32::try_from(self.count(key)?)
            .ok()
            .filter(|turn| *turn > 0)
            .ok_or_else(|| format!("`{key}` is not a turn (1 or more)"))
    }

    fn tag<T>(&self, key: &str, from_tag: fn(&str) -> Option<T>) -> Result<T, String> {
        let tag = self.string(key)?;
        from_tag(&tag).ok_or_else(|| format!("`{key}` is `{tag}`, which v0 does not name"))
    }

    fn head(&self, key: &str) -> Result<Vec<HeadMessage>, String> {
        let Value::Array(messages) = self.get(key)? else {
            return Err(format!("`{key}` is not a list"));
        };
        messages
            .iter()
            .map(|message| {
                let Value::Object(message) = message else {
                    return Err(format!("a `{key}` message is not an object"));
                };
                if message
                    .keys()
                    .any(|field| field != "role" && field != "content")
                {
                    return Err(format!(
                        "a `{key}` message carries more than `role` and `content`"
                    ));
                }
                let fields = Fields(message);
                Ok(HeadMessage {
                    role: fields.tag("role", Role::from_tag)?,
                    content: fields.string("content")?,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start() -> Line {
        Line {
            seq: 0,
            t: 0,
            event: Event::SessionStart {
                opened: 1_790_000_000_000,
                model: "a-model".to_owned(),
                head: vec![HeadMessage {
                    role: Role::System,
                    content: "you are the trunk".to_owned(),
                }],
            },
        }
    }

    /// One of every event, in an order the rules that span lines accept.
    #[allow(clippy::too_many_lines)]
    fn every_event() -> Vec<Line> {
        let events = vec![
            start().event,
            Event::Ask {
                turn: 1,
                text: "say hi".to_owned(),
            },
            Event::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Event::Request {
                turn: 1,
                lane: Lane::Trunk,
            },
            Event::Delta {
                request: 3,
                piece: Piece::Reasoning("thinking\nabout it".to_owned()),
            },
            Event::Delta {
                request: 3,
                piece: Piece::Text("Hi".to_owned()),
            },
            Event::Refused {
                command: Command::Ask,
                because: Refusal::InFlight,
                during: State::Turn,
            },
            Event::StopAsked { turn: 1 },
            Event::Cancelled {
                request: 3,
                partial: "Hi".to_owned(),
            },
            Event::TurnSettled {
                turn: 1,
                reason: SettleReason::Cancelled,
            },
            Event::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
            Event::Ask {
                turn: 2,
                text: "again".to_owned(),
            },
            Event::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Event::Request {
                turn: 2,
                lane: Lane::Trunk,
            },
            Event::RequestFailed {
                request: 13,
                reason: FailReason::Server,
                message: "busy".to_owned(),
                status: Some(503),
                partial: Some(String::new()),
            },
            Event::TurnSettled {
                turn: 2,
                reason: SettleReason::Failed,
            },
            Event::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
            Event::IdleGap {
                opened_by: 15,
                notice: 120,
                read: 2_400,
                compose: 3_100,
                away: 0,
                blocked: 0,
                ended_by: GapEnd::Ask,
            },
            Event::Ask {
                turn: 3,
                text: "once more".to_owned(),
            },
            Event::Settlement {
                from: State::Awaiting,
                to: State::Turn,
            },
            Event::Request {
                turn: 3,
                lane: Lane::Trunk,
            },
            Event::Response {
                to_request: 20,
                text: "Done.".to_owned(),
                finish_reason: Some("stop".to_owned()),
            },
            Event::TurnSettled {
                turn: 3,
                reason: SettleReason::Final,
            },
            Event::Settlement {
                from: State::Turn,
                to: State::Awaiting,
            },
        ];
        events
            .into_iter()
            .enumerate()
            .map(|(seq, event)| Line {
                seq: seq as u64,
                t: seq as u64 * 5,
                event,
            })
            .collect()
    }

    #[test]
    fn every_event_renders_to_one_line_that_reads_back_as_itself() {
        let lines = every_event();
        let mut document = String::new();
        for line in &lines {
            let rendered = render(line);
            assert!(
                !rendered.contains('\n'),
                "a rendered line broke: {rendered}"
            );
            assert_eq!(super::line(&rendered).as_ref(), Ok(line), "{rendered}");
            document.push_str(&rendered);
            document.push('\n');
        }
        assert_eq!(parse(&document), Ok(lines));
    }

    #[test]
    fn a_gap_in_seq_is_refused() {
        let mut lines = every_event();
        lines.remove(4);
        let document: String = lines.iter().map(|line| render(line) + "\n").collect();
        let refused = parse(&document).expect_err("a gap was read as a log");
        assert!(refused.why.contains("`seq`"), "{refused}");
    }

    #[test]
    fn a_reference_to_anything_but_an_earlier_request_is_refused() {
        let mut lines = every_event();
        lines[5].event = Event::Delta {
            request: 1,
            piece: Piece::Text("Hi".to_owned()),
        };
        let document: String = lines.iter().map(|line| render(line) + "\n").collect();
        let refused = parse(&document).expect_err("a reference to an ask was read");
        assert!(
            refused.why.contains("not an earlier `request`"),
            "{refused}"
        );
    }

    #[test]
    fn a_settlement_that_does_not_leave_the_state_it_is_in_is_refused() {
        let mut lines = every_event();
        lines[10].event = Event::Settlement {
            from: State::Capture,
            to: State::Awaiting,
        };
        let document: String = lines.iter().map(|line| render(line) + "\n").collect();
        let refused = parse(&document).expect_err("a broken chain was read");
        assert!(refused.why.contains("a settlement from"), "{refused}");
    }

    #[test]
    fn the_vocabularies_are_the_words_the_ruling_names() {
        let tags = |all: Vec<&str>| all.join(" ");
        assert_eq!(
            tags(Kind::ALL.iter().map(|it| it.tag()).collect()),
            "session.start ask settlement request refused delta stop.asked response \
             cancelled request.failed turn.settled idle.gap"
        );
        assert_eq!(
            tags(Refusal::ALL.iter().map(|it| it.tag()).collect()),
            "in-flight ended nothing-in-flight seam-not-built stale"
        );
        assert_eq!(
            tags(SettleReason::ALL.iter().map(|it| it.tag()).collect()),
            "final cancelled max_steps timeout failed"
        );
    }
}
