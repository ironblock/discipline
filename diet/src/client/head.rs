//! The frozen head: what it hashes to, what moved in it, and what may not be
//! in it.
//!
//! A cache miss on a live drive has a cause, and until #79 the record could
//! count misses and not name them. The split the billion-context study could
//! only estimate -- 84% "provider eviction", by residual -- is measurable
//! here, because **the head is a byte string the harness controls**. Hash it
//! on every request, keep the digest beside the cache telemetry, and a miss
//! is attributable to head mutation or it is not.
//!
//! Three things live in this module, and they are three because they answer
//! three different questions.
//!
//! * [`Head`] is the measurement: the rendered head, its digest, and the
//!   parts a diff is computed from. `Head::of` renders through
//!   [`crate::client::wire::head`] -- the same function [`crate::client::wire::body`]
//!   starts from -- so the bytes that are hashed are the bytes that are sent.
//! * [`Change`] is the attribution: what moved between two heads, as typed
//!   [`PrefixDelta`]s, and the class the record names for it. The class comes
//!   from [`crate::formats::record::reason_of`], which is also what
//!   `validate` checks a row against, so the writer and the reader apply one
//!   rule.
//! * [`timestamps`] is the lint: a rendered head carrying a calendar date or
//!   a clock is a head that expires at midnight, and #79's first specimen is
//!   exactly that -- a system prompt with the current date in it,
//!   invalidating every session's prefix when local midnight passes. Volatile
//!   facts belong in the turn envelope, in the mutable tail.
//!
//! # What this module will not claim
//!
//! It names a message that LEFT the frozen history; it does not say the
//! message was reasoning. #79's live-incident class 1 is a harness dropping
//! the model's own thinking between tool calls, and nothing in a head marks a
//! message as thinking -- a rule that guessed would fire on every edited
//! system prompt. It names the role a message was SENT under; it does not say
//! what the chat template rendered it as, because no server this client
//! speaks to returns its rendered head. Both halves are stated rather than
//! filled in, which is the same disclosure `drive`'s module header already
//! makes about #94's resolved effort level.

use std::collections::BTreeMap;

use crate::formats::record::json::{self, Value};
use crate::formats::record::{Count, Event, PrefixDelta, PrefixReason, reason_of};

use super::shape::{Message, RequestShape, ToolDefinition};

/// One request's frozen head: its bytes, its digest, and its parts.
///
/// The parts are kept beside the digest rather than re-derived from it,
/// because a digest answers *did it move* and a diff answers *what moved*,
/// and the second is the whole point of #79. They are clones of the shape's
/// own fields: this type states what a head IS, and the shape states what a
/// request is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    digest: String,
    model: String,
    tools: Vec<ToolDefinition>,
    template_kwargs: BTreeMap<String, Value>,
    frozen: Vec<Message>,
}

impl Head {
    /// The head `shape` would send.
    ///
    /// The digest is [`crate::digest::sha256_hex`] over exactly the bytes
    /// [`crate::client::wire::head`] renders -- the crate's one hasher over
    /// the crate's one head renderer. Nothing here re-spells either.
    #[must_use]
    pub fn of(shape: &RequestShape) -> Self {
        let rendered = super::wire::head(shape);
        let frozen = shape.messages.len().saturating_sub(1);
        Self {
            digest: crate::digest::sha256_hex(rendered.as_bytes()),
            model: shape.model.clone(),
            tools: shape.tools.clone(),
            template_kwargs: shape.template_kwargs.clone(),
            frozen: shape.messages.iter().take(frozen).cloned().collect(),
        }
    }

    /// This head's fingerprint: 64 lowercase hex characters.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// What moved between `previous` and this head, or `None` when nothing
    /// did.
    ///
    /// **The digests decide whether there is a change; the parts decide what
    /// to call it.** A diff that came back empty over two different digests
    /// is not a bug to paper over -- it is
    /// [`PrefixReason::Unattributed`], the residual, and saying so is what
    /// keeps the other five classes meaning something.
    #[must_use]
    pub fn change_from(&self, previous: &Self) -> Option<Change> {
        if previous.digest == self.digest {
            return None;
        }
        let mut diff = Vec::new();
        if previous.model != self.model {
            diff.push(PrefixDelta::ModelChanged {
                was: previous.model.clone(),
                now: self.model.clone(),
            });
        }
        tools_diff(&previous.tools, &self.tools, &mut diff);
        kwargs_diff(&previous.template_kwargs, &self.template_kwargs, &mut diff);
        messages_diff(&previous.frozen, &self.frozen, &mut diff);
        Some(Change {
            reason: reason_of(&diff),
            diff,
        })
    }

    /// The first timestamp this head carries, if it carries one.
    ///
    /// **Pointed at the frozen messages and the tool schemas, and at nothing
    /// else.** A dated model identifier -- `a-model-2026-08-06` -- is a NAME,
    /// not a fact that moves at midnight, and refusing it would refuse a
    /// drive against half the hosted surfaces on the market. A tool's own
    /// name is an identifier for the same reason. What is scanned is the text
    /// a template renders into the prompt and the schemas that travel with
    /// it, which is where a rendered date actually lands.
    #[must_use]
    pub fn timestamp(&self) -> Option<Timestamped> {
        for (index, message) in self.frozen.iter().enumerate() {
            if let Some(found) = timestamps(&message.content).into_iter().next() {
                return Some(Timestamped {
                    site: format!("message {index} ({})", message.role.tag()),
                    found,
                });
            }
        }
        for tool in &self.tools {
            let mut rendered = String::new();
            json::render(&tool.schema, &mut rendered);
            if let Some(found) = timestamps(&rendered).into_iter().next() {
                return Some(Timestamped {
                    site: format!("the `{}` tool's schema", tool.name),
                    found,
                });
            }
        }
        None
    }
}

/// What moved between two heads, and the class the record names for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    /// The class, which is [`reason_of`] over `diff` and not a second
    /// opinion about it.
    pub reason: PrefixReason,
    /// What moved, in detail. Empty exactly when `reason` is
    /// [`PrefixReason::Unattributed`].
    pub diff: Vec<PrefixDelta>,
}

impl Change {
    /// This change as the record row for it.
    ///
    /// The row carries no digest of its own: both are on the `request` rows,
    /// where they cannot contradict each other. See
    /// [`Event::PrefixChanged`].
    #[must_use]
    pub fn event(&self, id: impl Into<String>, at_request: impl Into<String>) -> Event {
        Event::PrefixChanged {
            id: id.into(),
            at_request: at_request.into(),
            reason: self.reason,
            diff: self.diff.clone(),
        }
    }
}

/// A timestamp found in a rendered head, and where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timestamped {
    /// Where it was found, in words a person can act on.
    pub site: String,
    /// The text that matched.
    pub found: String,
}

/// The heads each lane has sent, so a change can be noticed as it happens.
///
/// Per LANE, because a head is a lane's own: the interview fork, the main
/// lane and the ratify lane send different heads on purpose, and comparing
/// one against another would report a mutation on every call. The record says
/// the same thing structurally -- a `prefix.changed` inherits its lane from
/// the request it names.
#[derive(Debug, Clone, Default)]
pub struct Watch {
    latest: BTreeMap<String, Head>,
}

impl Watch {
    /// A watch that has seen nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `head` as what `lane` just sent, and say what changed.
    ///
    /// `None` on a lane's FIRST head, and `None` when the head did not move.
    /// The two are different facts and this collapses them deliberately: both
    /// mean *no `prefix.changed` row*, and the record refuses a row for
    /// either ([`crate::formats::record::PrefixChangeError::FirstOnItsLane`],
    /// [`crate::formats::record::PrefixChangeError::NotAChange`]), so a
    /// caller that had to tell them apart would be a caller about to write a
    /// row the format rejects.
    pub fn observe(&mut self, lane: &str, head: Head) -> Option<Change> {
        let change = self
            .latest
            .get(lane)
            .and_then(|previous| head.change_from(previous));
        self.latest.insert(lane.to_owned(), head);
        change
    }
}

/// Every timestamp `text` carries, in the order they appear.
///
/// TWO PATTERNS, hand-rolled rather than bought, because a regex crate is a
/// dependency this crate does not otherwise need and these two shapes are
/// fixed:
///
/// * an ISO calendar date, `NNNN-NN-NN`;
/// * a clock, `NN:NN:NN`.
///
/// **A bare `HH:MM` is deliberately NOT matched.** `12:30` is a ratio, a
/// score, a verse reference and a version range as often as it is a time, and
/// a lint that fires on all of them is a lint people turn off -- which
/// protects nothing. The two shapes above are unambiguous enough to act on.
///
/// Digits only, and no calendar validation: `2026-13-45` is refused as
/// readily as `2026-09-13`. A head carrying something date-shaped is a head
/// somebody templated a date into, whether or not the date exists.
#[must_use]
pub fn timestamps(text: &str) -> Vec<String> {
    let bytes: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if let Some(matched) = matches_shape(&bytes[at..], DATE) {
            found.push(matched.iter().collect());
            at += DATE.len();
            continue;
        }
        if let Some(matched) = matches_shape(&bytes[at..], CLOCK) {
            found.push(matched.iter().collect());
            at += CLOCK.len();
            continue;
        }
        at += 1;
    }
    found
}

/// A calendar date's shape: four digits, a dash, two digits, a dash, two.
const DATE: &[Shape] = &[
    Shape::Digit,
    Shape::Digit,
    Shape::Digit,
    Shape::Digit,
    Shape::Dash,
    Shape::Digit,
    Shape::Digit,
    Shape::Dash,
    Shape::Digit,
    Shape::Digit,
];

/// A clock's shape: two digits, a colon, two digits, a colon, two.
const CLOCK: &[Shape] = &[
    Shape::Digit,
    Shape::Digit,
    Shape::Colon,
    Shape::Digit,
    Shape::Digit,
    Shape::Colon,
    Shape::Digit,
    Shape::Digit,
];

/// One position of a pattern. A tiny vocabulary rather than a parser: these
/// two shapes are all this lint claims to know.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// An ASCII digit.
    Digit,
    /// A hyphen-minus.
    Dash,
    /// A colon.
    Colon,
}

impl Shape {
    fn accepts(self, ch: char) -> bool {
        match self {
            Self::Digit => ch.is_ascii_digit(),
            Self::Dash => ch == '-',
            Self::Colon => ch == ':',
        }
    }
}

/// `text`'s opening, when it is exactly `shape`.
fn matches_shape<'a>(text: &'a [char], shape: &[Shape]) -> Option<&'a [char]> {
    let window = text.get(..shape.len())?;
    shape
        .iter()
        .zip(window)
        .all(|(position, ch)| position.accepts(*ch))
        .then_some(window)
}

/// What moved in the tool surface.
///
/// BY NAME, then by position and definition. A tool is identified by what it
/// calls itself -- that is what an `mcp__*` server's key is -- so a tool that
/// reappears at another index MOVED, and one whose name is gone LEFT. Both
/// position and definition are reported when both changed: a reordering that
/// also edited a schema is two facts, and folding them would lose the one
/// nobody was looking for.
fn tools_diff(previous: &[ToolDefinition], now: &[ToolDefinition], diff: &mut Vec<PrefixDelta>) {
    let index_of =
        |list: &[ToolDefinition], name: &str| list.iter().position(|tool| tool.name == name);
    for (was, tool) in previous.iter().enumerate() {
        let Some(is) = index_of(now, &tool.name) else {
            diff.push(PrefixDelta::ToolRemoved {
                tool: tool.name.clone(),
            });
            continue;
        };
        if was != is {
            diff.push(PrefixDelta::ToolMoved {
                tool: tool.name.clone(),
                was: index(was),
                now: index(is),
            });
        }
        if now[is].schema != tool.schema {
            diff.push(PrefixDelta::ToolChanged {
                tool: tool.name.clone(),
            });
        }
    }
    for tool in now {
        if index_of(previous, &tool.name).is_none() {
            diff.push(PrefixDelta::ToolAdded {
                tool: tool.name.clone(),
            });
        }
    }
}

/// What moved in the arguments handed to the chat template.
///
/// The values are rendered through the record's own renderer, so what a delta
/// reports is the spelling that went out rather than a debug rendering of it
/// -- the same rule `drive::regimen::sampled` exists to keep one crossing
/// over.
fn kwargs_diff(
    previous: &BTreeMap<String, Value>,
    now: &BTreeMap<String, Value>,
    diff: &mut Vec<PrefixDelta>,
) {
    let spelled = |value: &Value| {
        let mut out = String::new();
        json::render(value, &mut out);
        out
    };
    for (key, was) in previous {
        match now.get(key) {
            None => diff.push(PrefixDelta::KwargRemoved {
                key: key.clone(),
                was: spelled(was),
            }),
            Some(is) if is != was => diff.push(PrefixDelta::KwargChanged {
                key: key.clone(),
                was: spelled(was),
                now: spelled(is),
            }),
            Some(_) => {}
        }
    }
    for (key, is) in now {
        if !previous.contains_key(key) {
            diff.push(PrefixDelta::KwargAdded {
                key: key.clone(),
                now: spelled(is),
            });
        }
    }
}

/// What moved in the frozen history.
///
/// Common prefix and common suffix are trimmed first, so an injection in the
/// middle is reported as one message arriving rather than as every message
/// after it changing. What is left is paired position by position where the
/// two middles are the same length AND the roles line up -- then the pair is
/// an EDIT and gets a line diff -- and reported as a removal and an arrival
/// otherwise.
fn messages_diff(previous: &[Message], now: &[Message], diff: &mut Vec<PrefixDelta>) {
    let head = previous
        .iter()
        .zip(now)
        .take_while(|(was, is)| was == is)
        .count();
    let tail = previous[head..]
        .iter()
        .rev()
        .zip(now[head..].iter().rev())
        .take_while(|(was, is)| was == is)
        .count();
    let was = &previous[head..previous.len() - tail];
    let is = &now[head..now.len() - tail];

    let paired = was.len() == is.len()
        && was
            .iter()
            .zip(is)
            .all(|(before, after)| before.role == after.role);
    if paired {
        for (offset, (before, after)) in was.iter().zip(is).enumerate() {
            lines_diff(index(head + offset), &before.content, &after.content, diff);
        }
        return;
    }
    for (offset, message) in was.iter().enumerate() {
        diff.push(PrefixDelta::MessageRemoved {
            at: index(head + offset),
            role: message.role.tag().to_owned(),
            chars: chars(&message.content),
        });
    }
    for (offset, message) in is.iter().enumerate() {
        diff.push(PrefixDelta::MessageAdded {
            at: index(head + offset),
            role: message.role.tag().to_owned(),
            chars: chars(&message.content),
        });
    }
}

/// What moved inside one frozen message, line by line.
///
/// **SPLIT ON `'\n'`, NOT THROUGH `str::lines()`, and this is the whole
/// point of the function.** `lines()` strips a trailing `\r` and drops the
/// empty segment after a trailing `\n`, so two messages differing ONLY in a
/// gained or lost trailing newline, or in CRLF versus LF endings, came back
/// with an EMPTY diff -- and an empty diff is
/// [`PrefixReason::Unattributed`], the residual, reported for a real and
/// perfectly attributable edit. Splitting on the byte keeps both: a trailing
/// newline is an empty final segment that arrived or left, and a CRLF ending
/// is a line whose text carries a `\r`.
fn lines_diff(message: u32, previous: &str, now: &str, diff: &mut Vec<PrefixDelta>) {
    let was: Vec<&str> = previous.split('\n').collect();
    let is: Vec<&str> = now.split('\n').collect();
    let head = was.iter().zip(&is).take_while(|(a, b)| a == b).count();
    let tail = was[head..]
        .iter()
        .rev()
        .zip(is[head..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let was = &was[head..was.len() - tail];
    let is = &is[head..is.len() - tail];

    let shared = was.len().min(is.len());
    for offset in 0..shared {
        diff.push(PrefixDelta::LineChanged {
            message,
            line: index(head + offset),
            was: (*was[offset]).to_owned(),
            now: (*is[offset]).to_owned(),
        });
    }
    for (offset, line) in was.iter().enumerate().skip(shared) {
        diff.push(PrefixDelta::LineRemoved {
            message,
            line: index(head + offset),
            was: (*line).to_owned(),
        });
    }
    for (offset, line) in is.iter().enumerate().skip(shared) {
        diff.push(PrefixDelta::LineAdded {
            message,
            line: index(head + offset),
            now: (*line).to_owned(),
        });
    }
}

/// A position, as the record spells one.
///
/// Saturating rather than panicking, and unreachable in practice: a head with
/// more than four billion messages or lines is not a thing. A total
/// conversion still has to produce something, and the something must not be a
/// number smaller than the truth.
fn index(position: usize) -> u32 {
    u32::try_from(position).unwrap_or(u32::MAX)
}

/// How many characters a message carries, as the record spells a count.
fn chars(content: &str) -> Count {
    Count::new(content.chars().count() as u64).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use super::super::shape::{
        Limits, Message, RequestShape, Role, SamplerCard, SamplerSetting, ToolDefinition,
    };
    use super::super::wire;
    use super::{Head, PrefixDelta, PrefixReason, Watch, timestamps};
    use crate::formats::record::json::Value;

    fn shape(messages: Vec<Message>) -> RequestShape {
        RequestShape {
            model: "a-model".to_owned(),
            tools: Vec::new(),
            messages,
            sampler: SamplerCard::empty(),
            limits: Limits {
                attempt: Duration::from_secs(1),
                call: Duration::from_secs(1),
                max_output_tokens: 64,
                retries: 0,
            },
            grammar: None,
            template_kwargs: BTreeMap::new(),
        }
    }

    fn tool(name: &str, required: &str) -> ToolDefinition {
        ToolDefinition {
            name: name.to_owned(),
            schema: Value::Object(BTreeMap::from([(
                "required".to_owned(),
                Value::String(required.to_owned()),
            )])),
        }
    }

    /// THE DRIFT GUARD. A digest of bytes nobody sent is worse than no
    /// digest, because it reads like a measurement -- so the head has to be
    /// the opening of the body, not a second rendering that agrees today.
    ///
    /// Four shapes, because the interesting cases are at the edges: no
    /// message at all, one (so the head's array is empty), several, and one
    /// carrying tools and kwargs so the optional keys are on the path too.
    #[test]
    fn the_head_is_the_bytes_the_body_starts_with() {
        let mut kwargs = BTreeMap::new();
        kwargs.insert("enable_thinking".to_owned(), Value::Boolean(false));
        let shapes = [
            shape(Vec::new()),
            shape(vec![Message::new(Role::User, "just the turn")]),
            shape(vec![
                Message::new(Role::System, "the regimen"),
                Message::new(Role::Assistant, "last time"),
                Message::new(Role::User, "this turn"),
            ]),
            RequestShape {
                tools: vec![tool("search", "query")],
                template_kwargs: kwargs,
                sampler: SamplerCard::empty()
                    .with_decimal(SamplerSetting::Temperature, "0.6")
                    .expect("0.6 is a decimal"),
                ..shape(vec![
                    Message::new(Role::System, "the regimen"),
                    Message::new(Role::User, "this turn"),
                ])
            },
        ];
        for one in &shapes {
            let head = wire::head(one);
            let body = wire::body(one);
            assert!(
                body.starts_with(&head),
                "the head is not the body's opening:\nhead={head}\nbody={body}"
            );
            assert_eq!(
                Head::of(one).digest(),
                crate::digest::sha256_hex(head.as_bytes()),
                "the digest is over exactly those bytes"
            );
            // And the body is still a request a server can read, which is
            // what stops "starts with" being satisfied by breaking it.
            let parsed: serde_json::Value =
                serde_json::from_str(&body).expect("the body is still JSON");
            assert_eq!(
                parsed["messages"].as_array().map(Vec::len),
                Some(one.messages.len())
            );
        }
    }

    /// The sampler is NOT part of the head: a temperature change is a regime
    /// change, and a server prefills the same bytes either way. Without this
    /// every pinned retry would read as a prefix mutation.
    #[test]
    fn the_sampler_and_the_last_message_are_outside_the_head() {
        let base = shape(vec![
            Message::new(Role::System, "the regimen"),
            Message::new(Role::User, "turn one"),
        ]);
        let hotter = RequestShape {
            sampler: SamplerCard::empty()
                .with_decimal(SamplerSetting::Temperature, "0.9")
                .expect("0.9 is a decimal"),
            ..base.clone()
        };
        let next = RequestShape {
            messages: vec![
                Message::new(Role::System, "the regimen"),
                Message::new(Role::User, "turn two"),
            ],
            ..base.clone()
        };
        assert_eq!(Head::of(&base), Head::of(&hotter));
        assert_eq!(Head::of(&base).digest(), Head::of(&next).digest());
        assert!(Head::of(&base).change_from(&Head::of(&next)).is_none());
    }

    /// Acceptance row 1, at the unit: the date in the system prompt moved,
    /// and the diff names the line it moved on.
    #[test]
    fn a_date_that_moved_is_a_text_change_naming_the_line() {
        let before = Head::of(&shape(vec![
            Message::new(
                Role::System,
                "You are helpful.\nToday is 2026-09-13.\nBe brief.",
            ),
            Message::new(Role::User, "turn"),
        ]));
        let after = Head::of(&shape(vec![
            Message::new(
                Role::System,
                "You are helpful.\nToday is 2026-09-14.\nBe brief.",
            ),
            Message::new(Role::User, "turn"),
        ]));
        let change = after.change_from(&before).expect("the head moved");
        assert_eq!(change.reason, PrefixReason::Text);
        assert_eq!(
            change.diff,
            vec![PrefixDelta::LineChanged {
                message: 0,
                line: 1,
                was: "Today is 2026-09-13.".to_owned(),
                now: "Today is 2026-09-14.".to_owned(),
            }],
            "the common lines either side are trimmed, so one line is named"
        );
    }

    /// Acceptance row 2, at the unit: definitions reordered, and the diff
    /// names the tools that moved and not the one that did not.
    #[test]
    fn reordered_tool_definitions_name_the_tool_that_moved() {
        let turn = vec![Message::new(Role::User, "turn")];
        let before = Head::of(&RequestShape {
            tools: vec![tool("alpha", "a"), tool("beta", "b"), tool("gamma", "c")],
            ..shape(turn.clone())
        });
        let after = Head::of(&RequestShape {
            tools: vec![tool("gamma", "c"), tool("beta", "b"), tool("alpha", "a")],
            ..shape(turn)
        });
        let change = after.change_from(&before).expect("the head moved");
        assert_eq!(change.reason, PrefixReason::Tools);
        assert_eq!(
            change.diff,
            vec![
                PrefixDelta::ToolMoved {
                    tool: "alpha".to_owned(),
                    was: 0,
                    now: 2,
                },
                PrefixDelta::ToolMoved {
                    tool: "gamma".to_owned(),
                    was: 2,
                    now: 0,
                },
            ],
            "`beta` did not move, and a delta for it would be a fact nobody measured"
        );
    }

    /// The trailing-newline and CRLF cases, which a `str::lines()` split
    /// swallows whole.
    ///
    /// All three of these are REAL, ATTRIBUTABLE edits -- a template that
    /// gained a trailing newline, one that lost it, a file that crossed a
    /// platform -- and under `lines()` each produced an empty diff and a
    /// `prefix.changed` reading `unattributed`. The residual has to be rare
    /// to mean anything, and these are the three commonest ways a head moves
    /// without a word changing.
    #[test]
    fn a_trailing_newline_or_a_crlf_ending_is_an_ordinary_line_delta() {
        let cases = [
            ("gained", "Be brief.", "Be brief.\n"),
            ("lost", "Be brief.\n", "Be brief."),
            ("crlf to lf", "one\r\ntwo", "one\ntwo"),
        ];
        for (what, before_text, after_text) in cases {
            let before = Head::of(&shape(vec![
                Message::new(Role::System, before_text),
                Message::new(Role::User, "turn"),
            ]));
            let after = Head::of(&shape(vec![
                Message::new(Role::System, after_text),
                Message::new(Role::User, "turn"),
            ]));
            let change = after
                .change_from(&before)
                .unwrap_or_else(|| panic!("{what}: the head moved"));
            assert!(
                !change.diff.is_empty(),
                "{what}: a newline the head gained or lost is attributable, and an \
                 empty diff would report it as the residual"
            );
            assert_eq!(
                change.reason,
                PrefixReason::Text,
                "{what}: it is a change to the text of a frozen message"
            );
        }
    }

    /// #79's live-incident class 2, and the half of class 1 this module can
    /// honestly claim.
    ///
    /// A message that arrived is named with its role AS SENT; a message that
    /// left is named as a message, never as reasoning. The second assertion
    /// is the one that matters: nothing in a head marks thinking as thinking.
    #[test]
    fn history_dropped_from_the_head_is_a_message_that_left() {
        let before = Head::of(&shape(vec![
            Message::new(Role::System, "the regimen"),
            Message::new(Role::Assistant, "weighing it up"),
            Message::new(Role::Assistant, "the answer"),
            Message::new(Role::User, "turn"),
        ]));
        let after = Head::of(&shape(vec![
            Message::new(Role::System, "the regimen"),
            Message::new(Role::Assistant, "the answer"),
            Message::new(Role::User, "turn"),
        ]));
        let change = after.change_from(&before).expect("the head moved");
        assert_eq!(change.reason, PrefixReason::Injection);
        assert_eq!(
            change.diff,
            vec![PrefixDelta::MessageRemoved {
                at: 1,
                role: "assistant".to_owned(),
                chars: crate::formats::record::Count::new(14).expect("14 is a count"),
            }]
        );

        // And the other direction: a reminder injected after a tool call.
        let injected = Head::of(&shape(vec![
            Message::new(Role::System, "the regimen"),
            Message::new(Role::User, "just a reminder, keep going"),
            Message::new(Role::Assistant, "the answer"),
            Message::new(Role::User, "turn"),
        ]));
        let change = injected.change_from(&after).expect("the head moved");
        assert_eq!(change.reason, PrefixReason::Injection);
        assert!(matches!(
            change.diff.as_slice(),
            [PrefixDelta::MessageAdded { at: 1, role, .. }] if role == "user"
        ));
    }

    /// #79's live-incident class 3, and the disclosure that comes with it:
    /// what is detected is that a template argument moved, and which key.
    #[test]
    fn a_template_argument_that_moved_is_named_by_its_key() {
        let turn = vec![Message::new(Role::User, "turn")];
        let kwargs = |level: &str| {
            BTreeMap::from([(
                "reasoning_effort".to_owned(),
                Value::String(level.to_owned()),
            )])
        };
        let before = Head::of(&RequestShape {
            template_kwargs: kwargs("low"),
            ..shape(turn.clone())
        });
        let after = Head::of(&RequestShape {
            template_kwargs: kwargs("high"),
            ..shape(turn)
        });
        let change = after.change_from(&before).expect("the head moved");
        assert_eq!(change.reason, PrefixReason::Effort);
        assert_eq!(
            change.diff,
            vec![PrefixDelta::KwargChanged {
                key: "reasoning_effort".to_owned(),
                was: "\"low\"".to_owned(),
                now: "\"high\"".to_owned(),
            }]
        );
    }

    /// The residual can fire, and a residual nothing can produce is not a
    /// residual.
    ///
    /// Built by hand, because the diff covers every part `Head::of` reads:
    /// two heads whose digests differ while every part this module compares
    /// agrees. That is a head built outside `Head::of` -- which is exactly
    /// the state an adapter or a future field would arrive in, and the one
    /// the record has a class for.
    #[test]
    fn bytes_that_moved_with_no_part_naming_it_are_unattributed() {
        let real = Head::of(&shape(vec![
            Message::new(Role::System, "the regimen"),
            Message::new(Role::User, "turn"),
        ]));
        let mut relabelled = real.clone();
        relabelled.digest = "f".repeat(64);
        let change = relabelled
            .change_from(&real)
            .expect("the digests differ, so the head moved");
        assert!(change.diff.is_empty());
        assert_eq!(change.reason, PrefixReason::Unattributed);
    }

    /// The precedence is the declaration order, and it is applied by
    /// `reason_of` rather than by this module's own opinion.
    #[test]
    fn a_head_that_moved_in_two_places_names_the_stronger_class() {
        let turn = Message::new(Role::User, "turn");
        let before = Head::of(&RequestShape {
            tools: vec![tool("alpha", "a")],
            ..shape(vec![Message::new(Role::System, "one\ntwo"), turn.clone()])
        });
        let after = Head::of(&RequestShape {
            tools: vec![tool("alpha", "a"), tool("beta", "b")],
            ..shape(vec![Message::new(Role::System, "one\nthree"), turn])
        });
        let change = after.change_from(&before).expect("the head moved");
        assert_eq!(
            change.reason,
            PrefixReason::Tools,
            "`tools` is declared before `text`, so it outranks it"
        );
        assert!(
            change.diff.len() >= 2,
            "both facts are kept: {:?}",
            change.diff
        );
    }

    /// The lint, and the two shapes it deliberately does not know.
    #[test]
    fn a_date_and_a_clock_are_timestamps_and_an_hour_and_a_minute_are_not() {
        assert_eq!(timestamps("Today is 2026-09-13."), vec!["2026-09-13"]);
        assert_eq!(timestamps("at 09:41:07 UTC"), vec!["09:41:07"]);
        assert_eq!(
            timestamps("2026-09-13T09:41:07"),
            vec!["2026-09-13", "09:41:07"],
            "both halves of an ISO instant are found"
        );
        assert!(
            timestamps("the ratio was 12:30").is_empty(),
            "a bare hour and minute is a ratio as often as a time, and a lint \
             people turn off protects nothing"
        );
        assert!(timestamps("version 1.2.3").is_empty());
        assert!(timestamps("").is_empty());
    }

    /// The lint reads the head's TEXT, not its identifiers. A dated model id
    /// is a name; refusing it would refuse a drive against half the hosted
    /// surfaces on the market.
    #[test]
    fn a_dated_model_identifier_is_a_name_and_not_a_timestamp() {
        let dated = RequestShape {
            model: "a-model-2026-08-06".to_owned(),
            ..shape(vec![
                Message::new(Role::System, "the regimen"),
                Message::new(Role::User, "turn"),
            ])
        };
        assert!(Head::of(&dated).timestamp().is_none());

        let templated = shape(vec![
            Message::new(Role::System, "the regimen\nToday is 2026-08-06."),
            Message::new(Role::User, "turn"),
        ]);
        let found = Head::of(&templated)
            .timestamp()
            .expect("a date in the head");
        assert_eq!(found.found, "2026-08-06");
        assert!(found.site.contains("message 0"), "{}", found.site);
    }

    /// A watch is per lane, and a lane's first head is not a head that moved.
    #[test]
    fn a_watch_reports_a_change_per_lane_and_never_on_the_first_head() {
        let mut watch = Watch::new();
        let first = Head::of(&shape(vec![
            Message::new(Role::System, "one"),
            Message::new(Role::User, "turn"),
        ]));
        let second = Head::of(&shape(vec![
            Message::new(Role::System, "two"),
            Message::new(Role::User, "turn"),
        ]));
        assert!(watch.observe("main", first.clone()).is_none());
        // Another lane's first head, which is not the main lane's second.
        assert!(watch.observe("interview", second.clone()).is_none());
        let change = watch.observe("main", second).expect("the main head moved");
        assert_eq!(change.reason, PrefixReason::Text);
        assert!(watch.observe("main", first).is_some(), "and back again");
    }
}
