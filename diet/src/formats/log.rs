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

/// Q4: an `idle.gap` is emitted once per gap, and references the
/// `turn.settled` that opened it -- the latest one.
fn gap_once(
    opened_by: u64,
    settlings: &BTreeSet<u64>,
    latest: Option<u64>,
    gapped: &mut BTreeSet<u64>,
) -> Result<(), String> {
    if !settlings.contains(&opened_by) {
        return Err(format!("seq {opened_by} is not an earlier `turn.settled`"));
    }
    if latest != Some(opened_by) {
        return Err(format!("seq {opened_by} is not the latest `turn.settled`"));
    }
    if !gapped.insert(opened_by) {
        return Err(format!("a second `idle.gap` opened by seq {opened_by}"));
    }
    Ok(())
}

/// A request ends once: a `response`, a `cancelled` or a `request.failed`
/// (the module's own rule, "a cancelled call is `cancelled`, never a
/// `response`"; found by #137's review).
fn ends_once(event: &Event, request: u64, ended: &mut BTreeSet<u64>) -> Result<(), String> {
    if ended.insert(request) {
        return Ok(());
    }
    let kind = match event {
        Event::Response { .. } => Kind::Response,
        Event::Cancelled { .. } => Kind::Cancelled,
        _ => Kind::RequestFailed,
    };
    Err(format!(
        "a `{}` for request {request}, which has already ended",
        kind.tag()
    ))
}

/// `seq` counts from 0 without a gap, and `t` never goes back.
fn in_order(line: &Line, index: usize, last_t: u64) -> Result<(), String> {
    if line.seq != index as u64 {
        return Err(format!(
            "`seq` is {}, where the line's position is {index}",
            line.seq
        ));
    }
    if line.t < last_t {
        return Err(format!("`t` went back from {last_t} to {}", line.t));
    }
    Ok(())
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
    let mut ended = BTreeSet::new();
    let mut latest_settling = None;
    let mut gapped = BTreeSet::new();
    let mut last_t = 0_u64;
    for (index, line) in lines.iter().enumerate() {
        in_order(line, index, last_t).map_err(|why| at(index, why))?;
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
                latest_settling = Some(line.seq);
            }
            Event::IdleGap { opened_by, .. } => {
                gap_once(*opened_by, &settlings, latest_settling, &mut gapped)
                    .map_err(|why| at(index, why))?;
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
            Event::Delta { request, .. } if ended.contains(request) => {
                return Err(at(
                    index,
                    format!("a `delta` for request {request}, which has already ended"),
                ));
            }
            Event::Response {
                to_request: request,
                ..
            }
            | Event::Cancelled { request, .. }
            | Event::RequestFailed { request, .. } => {
                ends_once(&line.event, *request, &mut ended).map_err(|why| at(index, why))?;
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

/// The keys a kind must carry, then the keys it may: read off [`schema`],
/// the one place a kind's keys are declared.
fn keys(kind: Kind) -> (Vec<&'static str>, Vec<&'static str>) {
    let fields = schema(kind);
    (
        fields
            .iter()
            .filter(|f| f.required)
            .map(|f| f.key)
            .collect(),
        fields
            .iter()
            .filter(|f| !f.required)
            .map(|f| f.key)
            .collect(),
    )
}

// ---------------------------------------------------------------------------
// the schema, as data
// ---------------------------------------------------------------------------

/// A closed vocabulary a key holds one tag of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tags {
    /// [`State`].
    State,
    /// [`Lane`].
    Lane,
    /// [`Command`].
    Command,
    /// [`Refusal`].
    Refusal,
    /// [`FailReason`].
    FailReason,
    /// [`SettleReason`].
    SettleReason,
    /// [`GapEnd`].
    GapEnd,
    /// [`Role`].
    Role,
}

impl Tags {
    /// Every vocabulary a key can hold.
    pub const ALL: &'static [Self] = &[
        Self::State,
        Self::Lane,
        Self::Command,
        Self::Refusal,
        Self::FailReason,
        Self::SettleReason,
        Self::GapEnd,
        Self::Role,
    ];

    /// The Rust type's name, which the bindings name the union after.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::State => "State",
            Self::Lane => "Lane",
            Self::Command => "Command",
            Self::Refusal => "Refusal",
            Self::FailReason => "FailReason",
            Self::SettleReason => "SettleReason",
            Self::GapEnd => "GapEnd",
            Self::Role => "Role",
        }
    }

    /// Its tags, read off the vocabulary itself.
    #[must_use]
    pub fn tags(self) -> Vec<&'static str> {
        fn of<T: Copy>(all: &[T], tag: fn(T) -> &'static str) -> Vec<&'static str> {
            all.iter().map(|v| tag(*v)).collect()
        }
        match self {
            Self::State => of(State::ALL, State::tag),
            Self::Lane => of(Lane::ALL, Lane::tag),
            Self::Command => of(Command::ALL, Command::tag),
            Self::Refusal => of(Refusal::ALL, Refusal::tag),
            Self::FailReason => of(FailReason::ALL, FailReason::tag),
            Self::SettleReason => of(SettleReason::ALL, SettleReason::tag),
            Self::GapEnd => of(GapEnd::ALL, GapEnd::tag),
            Self::Role => of(Role::ALL, Role::tag),
        }
    }
}

/// What a key holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holds {
    /// A non-negative integer.
    Count,
    /// Text.
    Text,
    /// The format's version, which is [`VERSION`].
    Version,
    /// The session's head: a list of `{role, content}`.
    Head,
    /// One tag of a closed vocabulary.
    Tag(Tags),
}

/// One key a kind carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// The key.
    pub key: &'static str,
    /// What it holds.
    pub holds: Holds,
    /// Whether every line of the kind carries it.
    pub required: bool,
}

const fn must(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: true,
    }
}

const fn may(key: &'static str, holds: Holds) -> Field {
    Field {
        key,
        holds,
        required: false,
    }
}

/// Every key a kind carries beyond `seq`, `t` and `kind`, and what each
/// holds. THE ONE DECLARATION: the reader's key check ([`keys`]), the
/// TypeScript bindings ([`typescript`]) and the test that pins this table
/// against what [`render`] actually writes all read it.
#[must_use]
pub fn schema(kind: Kind) -> &'static [Field] {
    use Holds::{Count, Head, Tag, Text, Version};
    match kind {
        Kind::SessionStart => {
            const F: &[Field] = &[
                must("version", Version),
                must("opened", Count),
                must("model", Text),
                must("head", Head),
            ];
            F
        }
        Kind::Ask => {
            const F: &[Field] = &[must("turn", Count), must("text", Text)];
            F
        }
        Kind::Settlement => {
            const F: &[Field] = &[must("from", Tag(Tags::State)), must("to", Tag(Tags::State))];
            F
        }
        Kind::Request => {
            const F: &[Field] = &[must("turn", Count), must("lane", Tag(Tags::Lane))];
            F
        }
        Kind::Refused => {
            const F: &[Field] = &[
                must("command", Tag(Tags::Command)),
                must("because", Tag(Tags::Refusal)),
                must("during", Tag(Tags::State)),
            ];
            F
        }
        Kind::Delta => {
            const F: &[Field] = &[
                must("request", Count),
                may("text", Text),
                may("reasoning", Text),
            ];
            F
        }
        Kind::StopAsked => {
            const F: &[Field] = &[must("turn", Count)];
            F
        }
        Kind::Response => {
            const F: &[Field] = &[
                must("to_request", Count),
                must("text", Text),
                may("finish_reason", Text),
            ];
            F
        }
        Kind::Cancelled => {
            const F: &[Field] = &[must("request", Count), must("partial", Text)];
            F
        }
        Kind::RequestFailed => {
            const F: &[Field] = &[
                must("request", Count),
                must("reason", Tag(Tags::FailReason)),
                must("message", Text),
                may("status", Count),
                may("partial", Text),
            ];
            F
        }
        Kind::TurnSettled => {
            const F: &[Field] = &[must("turn", Count), must("reason", Tag(Tags::SettleReason))];
            F
        }
        Kind::IdleGap => {
            const F: &[Field] = &[
                must("opened_by", Count),
                must("notice", Count),
                must("read", Count),
                must("compose", Count),
                must("away", Count),
                must("blocked", Count),
                must("ended_by", Tag(Tags::GapEnd)),
            ];
            F
        }
    }
}

/// Optional keys of which a line of `kind` carries exactly one.
#[must_use]
pub fn exactly_one(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Delta => &["text", "reasoning"],
        _ => &[],
    }
}

// ---------------------------------------------------------------------------
// TypeScript bindings (#31: the SPA reads a log through types generated from
// this file, never through a hand-kept mirror of it)
// ---------------------------------------------------------------------------

/// The checked-in bindings' path, relative to the crate root.
pub const BINDINGS: &str = "formats/log/log.ts";

fn ts_holds(holds: Holds) -> String {
    match holds {
        Holds::Count => "number".to_owned(),
        Holds::Text => "string".to_owned(),
        Holds::Version => VERSION.to_string(),
        Holds::Head => "HeadMessage[]".to_owned(),
        Holds::Tag(tags) => tags.name().to_owned(),
    }
}

fn ts_name(kind: Kind) -> String {
    kind.tag()
        .split('.')
        .map(|part| {
            let mut chars = part.chars();
            chars.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(chars).collect()
            })
        })
        .chain(std::iter::once("Line".to_owned()))
        .collect()
}

/// The TypeScript bindings for this format, generated from [`schema`] and
/// the vocabularies. Deterministic, and independent of where it is run from:
/// it reads nothing but this module.
///
/// # Panics
///
/// If [`exactly_one`] names a key [`schema`] does not declare for the same
/// kind -- a table defect, and one the schema's own test refuses first.
#[must_use]
pub fn typescript() -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    out.push_str(
        "// GENERATED from diet/src/formats/log.rs -- do not edit by hand.\n\
         // Regenerate: cargo test -p discipline-diet --lib \
         formats::log::tests::write_the_bindings -- --ignored\n\
         // A number here is an integer in the log; the reader refuses one past\n\
         // i64, and a JavaScript number is exact only to 2^53.\n\n",
    );
    let _ = write!(out, "export const VERSION = {VERSION};\n\n");
    out.push_str("export type Kind =\n");
    for kind in Kind::ALL {
        let _ = writeln!(out, "  | \"{}\"", kind.tag());
    }
    out.push_str(";\n\n");
    for tags in Tags::ALL {
        let _ = writeln!(out, "export type {} =", tags.name());
        for tag in tags.tags() {
            let _ = writeln!(out, "  | \"{tag}\"");
        }
        out.push_str(";\n\n");
    }
    out.push_str("export interface HeadMessage {\n  role: Role;\n  content: string;\n}\n\n");
    for kind in Kind::ALL {
        let name = ts_name(*kind);
        let one = exactly_one(*kind);
        let _ = writeln!(out, "export type {name} = {{");
        out.push_str("  seq: number;\n  t: number;\n");
        let _ = writeln!(out, "  kind: \"{}\";", kind.tag());
        for field in schema(*kind) {
            if one.contains(&field.key) {
                continue;
            }
            let optional = if field.required { "" } else { "?" };
            let _ = writeln!(out, "  {}{optional}: {};", field.key, ts_holds(field.holds));
        }
        out.push('}');
        if !one.is_empty() {
            out.push_str(" & (");
            for (index, key) in one.iter().enumerate() {
                if index > 0 {
                    out.push_str(" | ");
                }
                let holds = schema(*kind)
                    .iter()
                    .find(|f| f.key == *key)
                    .map(|f| f.holds)
                    .expect("`exactly_one` names only keys the schema declares");
                let _ = write!(out, "{{ {key}: {}", ts_holds(holds));
                for other in one.iter().filter(|o| *o != key) {
                    let _ = write!(out, "; {other}?: never");
                }
                out.push_str(" }");
            }
            out.push(')');
        }
        out.push_str(";\n\n");
    }
    out.push_str("export type LogLine =\n");
    for kind in Kind::ALL {
        let _ = writeln!(out, "  | {}", ts_name(*kind));
    }
    out.push_str(";\n");
    out
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

    /// Whether `value` is what `holds` says a key holds.
    fn written_as(holds: Holds, value: &Value) -> bool {
        match (holds, value) {
            (Holds::Count, Value::Integer(n)) => *n >= 0,
            (Holds::Version, Value::Integer(n)) => *n == VERSION,
            (Holds::Text, Value::String(_)) => true,
            (Holds::Tag(tags), Value::String(tag)) => tags.tags().contains(&tag.as_str()),
            (Holds::Head, Value::Array(messages)) => messages.iter().all(|m| match m {
                Value::Object(message) => {
                    message.len() == 2
                        && matches!(message.get("content"), Some(Value::String(_)))
                        && message
                            .get("role")
                            .is_some_and(|r| written_as(Holds::Tag(Tags::Role), r))
                }
                _ => false,
            }),
            _ => false,
        }
    }

    /// Every line the schema is pinned against: [`every_event`], and every
    /// valid fixture -- which is where the optional keys go missing.
    fn corpus() -> Vec<Line> {
        let mut lines = every_event();
        let valid =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("formats/log/fixtures/valid");
        let mut files: Vec<_> = std::fs::read_dir(&valid)
            .expect("the valid fixtures")
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "jsonl"))
            .collect();
        files.sort();
        assert!(
            !files.is_empty(),
            "no valid fixture to pin the schema against"
        );
        for file in files {
            let text = std::fs::read_to_string(&file).expect("a fixture reads");
            lines.extend(parse(&text).unwrap_or_else(|e| panic!("{}: {e}", file.display())));
        }
        lines
    }

    /// THE SCHEMA IS WHAT THE WRITER WRITES, BOTH WAYS. [`schema`] is read by
    /// the reader's key check and by the TypeScript bindings, so a wrong row is
    /// a reader that accepts a key nothing writes, or an SPA typed against a
    /// shape no log has. Pinned against [`to_value`] over [`corpus`]:
    ///
    /// * the keys written for a kind are exactly the keys declared for it --
    ///   a declared key nothing writes is a key the reader would accept;
    /// * a required key is always written, and an optional one is left out
    ///   somewhere -- or the bindings mark it optional for nothing;
    /// * each value is the type declared, and a tag key's vocabulary is the
    ///   one the READER reads it with: every tag of the declared vocabulary
    ///   parses in its place, and a tag only another vocabulary has does not
    ///   (vocabularies share tags, so membership alone cannot tell them apart;
    ///   found by #144's review).
    #[test]
    fn the_schema_is_what_every_kind_writes() {
        let mut written: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
        let mut omitted: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for line in corpus() {
            let Value::Object(object) = to_value(&line) else {
                panic!("a line is an object");
            };
            let kind = Kind::from_tag(match object.get("kind") {
                Some(Value::String(k)) => k,
                _ => panic!("a line names its kind"),
            })
            .expect("a known kind");
            let fields = schema(kind);
            for (key, value) in &object {
                if COMMON.contains(&key.as_str()) {
                    continue;
                }
                let field = fields.iter().find(|f| f.key == key).unwrap_or_else(|| {
                    panic!(
                        "`{}` writes `{key}`, which its schema does not declare",
                        kind.tag()
                    )
                });
                assert!(
                    written_as(field.holds, value),
                    "`{}`'s `{key}` is written as {value:?}, which is not {:?}",
                    kind.tag(),
                    field.holds
                );
                if let Holds::Tag(tags) = field.holds {
                    the_reader_reads_it_as(&object, key, tags);
                }
                written.entry(kind.tag()).or_default().insert(key.clone());
            }
            for field in fields {
                if !object.contains_key(field.key) {
                    assert!(
                        !field.required,
                        "`{}` declares `{}` required and did not write it",
                        kind.tag(),
                        field.key
                    );
                    omitted.entry(kind.tag()).or_default().insert(field.key);
                }
            }
        }
        for kind in Kind::ALL {
            let declared: BTreeSet<String> =
                schema(*kind).iter().map(|f| f.key.to_owned()).collect();
            assert_eq!(
                written.get(kind.tag()).cloned().unwrap_or_default(),
                declared,
                "`{}`: the keys written are not the keys declared",
                kind.tag()
            );
            for field in schema(*kind).iter().filter(|f| !f.required) {
                assert!(
                    omitted
                        .get(kind.tag())
                        .is_some_and(|o| o.contains(field.key)),
                    "`{}` declares `{}` optional and every line writes it",
                    kind.tag(),
                    field.key
                );
            }
            for key in exactly_one(*kind) {
                assert!(
                    schema(*kind).iter().any(|f| f.key == *key && !f.required),
                    "`{}`: `exactly_one` names `{key}`, which is not an optional key",
                    kind.tag()
                );
            }
        }
    }

    /// `exactly_one` IS WHAT THE READER ENFORCES: for every line that writes
    /// one optional text key of its kind, adding another is refused exactly
    /// when the two are declared exclusive. An exclusivity the bindings drop
    /// is a union that admits a line the reader refuses.
    #[test]
    fn exclusive_keys_are_the_ones_the_reader_refuses_together() {
        let mut probed = 0;
        for line in corpus() {
            let Value::Object(object) = to_value(&line) else {
                panic!("a line is an object");
            };
            let Some(Value::String(tag)) = object.get("kind") else {
                panic!("a line names its kind");
            };
            let kind = Kind::from_tag(tag).expect("a known kind");
            let optional_text: Vec<&str> = schema(kind)
                .iter()
                .filter(|f| !f.required && f.holds == Holds::Text)
                .map(|f| f.key)
                .collect();
            for present in optional_text.iter().filter(|k| object.contains_key(**k)) {
                for absent in optional_text.iter().filter(|k| !object.contains_key(**k)) {
                    let mut both = object.clone();
                    both.insert((*absent).to_owned(), Value::String("x".to_owned()));
                    let mut rendered = String::new();
                    json::render(&Value::Object(both), &mut rendered);
                    let refused = self::line(&rendered).is_err();
                    let exclusive =
                        exactly_one(kind).contains(present) && exactly_one(kind).contains(absent);
                    assert_eq!(
                        refused,
                        exclusive,
                        "`{}` with both `{present}` and `{absent}`: the reader {} it, and \
                         `exactly_one` says they are{} exclusive",
                        kind.tag(),
                        if refused { "refuses" } else { "accepts" },
                        if exclusive { "" } else { " not" }
                    );
                    probed += 1;
                }
            }
        }
        assert!(probed > 0, "no pair of optional keys was probed");
    }

    /// `key` in `object` is read with `tags`: each of its tags parses there,
    /// and a tag only another vocabulary carries does not.
    fn the_reader_reads_it_as(object: &BTreeMap<String, Value>, key: &str, tags: Tags) {
        let with = |tag: &str| {
            let mut changed = object.clone();
            changed.insert(key.to_owned(), Value::String(tag.to_owned()));
            let mut rendered = String::new();
            json::render(&Value::Object(changed), &mut rendered);
            line(&rendered).is_ok()
        };
        let own = tags.tags();
        for tag in &own {
            assert!(
                with(tag),
                "`{key}` declared {} and `{tag}` is refused there",
                tags.name()
            );
        }
        for other in Tags::ALL.iter().filter(|t| **t != tags) {
            for tag in other.tags().into_iter().filter(|t| !own.contains(t)) {
                assert!(
                    !with(tag),
                    "`{key}` declared {} and accepts `{tag}`, which only {} has",
                    tags.name(),
                    other.name()
                );
            }
        }
    }

    /// `request.failed.message` IS PROSE (ruled on #140): the typed `reason`
    /// is the vocabulary, the message is diagnostic text, and nothing here
    /// reads it as anything else. Two fixtures identical but for the message
    /// -- empty, and a paragraph -- parse to the same lines and project to the
    /// same value, `message` aside.
    #[test]
    fn a_failures_message_is_read_as_nothing_but_prose() {
        let valid =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("formats/log/fixtures/valid");
        let read = |name: &str| {
            std::fs::read_to_string(valid.join(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
        };
        let empty = read("a-failure-whose-message-is-empty.jsonl");
        let paragraph = read("a-failure-whose-message-is-a-paragraph.jsonl");

        let without_message = |text: &str| -> Vec<Line> {
            parse(text)
                .expect("a valid fixture")
                .into_iter()
                .map(|mut line| {
                    if let Event::RequestFailed { message, .. } = &mut line.event {
                        message.clear();
                    }
                    line
                })
                .collect()
        };
        let (a, b) = (without_message(&empty), without_message(&paragraph));
        assert!(
            a.iter()
                .any(|l| matches!(l.event, Event::RequestFailed { .. })),
            "the fixtures carry a `request.failed`, or this proves nothing"
        );
        assert_eq!(a, b, "the two logs differ in more than a failure's message");
        // The MESSAGES differ, one of them empty -- not merely the files: two
        // files differing in whitespace would let a projection that reads the
        // message pass unseen (#147's review).
        let messages = |text: &str| -> Vec<String> {
            parse(text)
                .expect("a valid fixture")
                .into_iter()
                .filter_map(|line| match line.event {
                    Event::RequestFailed { message, .. } => Some(message),
                    _ => None,
                })
                .collect()
        };
        let (quiet, spoken) = (messages(&empty), messages(&paragraph));
        assert!(
            quiet.iter().all(String::is_empty) && spoken.iter().all(|m| !m.is_empty()),
            "one fixture's messages are empty and the other's are prose: {quiet:?} / {spoken:?}"
        );

        let projected_without_message = |text: &str| -> Value {
            let Value::Array(lines) = project(text).expect("a valid fixture projects") else {
                panic!("a log projects to an array");
            };
            Value::Array(
                lines
                    .into_iter()
                    .map(|line| match line {
                        Value::Object(mut object) => {
                            if matches!(object.get("kind"), Some(Value::String(k)) if k == "request.failed") {
                                object.remove("message");
                            }
                            Value::Object(object)
                        }
                        other => other,
                    })
                    .collect(),
            )
        };
        assert_eq!(
            projected_without_message(&empty),
            projected_without_message(&paragraph),
            "the projection reads a failure's message as more than prose"
        );
    }

    fn bindings_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(BINDINGS)
    }

    /// THE CHECKED-IN BINDINGS ARE WHAT THIS FILE GENERATES. The SPA reads
    /// `formats/log/log.ts`; a change here that is not regenerated there is a
    /// SPA reading a format that no longer exists, found only when it misreads
    /// a live log. Located from `CARGO_MANIFEST_DIR`, not the working
    /// directory, so the answer does not depend on where cargo was started.
    #[test]
    fn the_checked_in_bindings_are_current() {
        let checked_in = std::fs::read_to_string(bindings_path())
            .unwrap_or_else(|e| panic!("{BINDINGS} could not be read: {e}"));
        assert!(
            checked_in == typescript(),
            "{BINDINGS} is stale against log.rs; regenerate it: cargo test -p \
             discipline-diet --lib formats::log::tests::write_the_bindings -- --ignored"
        );
    }

    /// Writes the bindings. Ignored: a test run never writes the tree.
    #[test]
    #[ignore = "writes formats/log/log.ts; run it to regenerate the bindings"]
    fn write_the_bindings() {
        std::fs::write(bindings_path(), typescript()).expect("the bindings are written");
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
