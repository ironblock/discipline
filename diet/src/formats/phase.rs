//! The `phase` format (#787): one phase of a session's phase library.
//!
//! The grammar at `diet/formats/phase/grammar.pest` is normative; this module
//! is its one implementation. A phase document names the phase, holds its
//! ground rules -- the text the model is given while the session is in it --
//! and lists the phases it may move to, each `suggested` (offered as the
//! recommended next step) or `allowed` (listed as available). Either kind is
//! a move the model may propose and the operator rules on; a move to a phase
//! not listed is refused. A phase needs no outputs: working memory carries
//! what the next phase needs.
//!
//! Every refusal names the key it is about, so a hand-edited document says
//! what to fix.

use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;
use pest_derive::Parser;

use crate::formats::record::json::Value;

#[derive(Parser)]
#[grammar = "../formats/phase/grammar.pest"]
struct PhaseParser;

/// How a move to a phase is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Offered as the recommended next step.
    Suggested,
    /// Listed as available.
    Allowed,
}

impl Kind {
    /// Every kind.
    pub const ALL: &'static [Self] = &[Self::Suggested, Self::Allowed];

    /// The document's word for it.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Suggested => "suggested",
            Self::Allowed => "allowed",
        }
    }

    /// The kind a word names.
    #[must_use]
    pub fn from_tag(word: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.tag() == word)
    }
}

/// One move a phase lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    /// The phase it moves to.
    pub to: String,
    /// How it is presented.
    pub kind: Kind,
}

/// A phase document, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Phase {
    /// Its name: lowercase letters, digits, `-` and `_`.
    pub name: String,
    /// What the model is given while the session is in it, its surrounding
    /// blank lines trimmed.
    pub ground_rules: String,
    /// The moves it lists, in the document's order.
    pub transitions: Vec<Transition>,
}

/// Why a text is not a phase document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PhaseError {
    /// The text is not the grammar's.
    Syntax(String),
    /// A required key is absent: where (`phase` or `transitions[n]`), and
    /// which.
    Missing(String, &'static str),
    /// A key the document does not define.
    Unknown(String, String),
    /// A key written twice in one place.
    Duplicate(String, String),
    /// A key whose value does not fit it: where, which key, and why.
    Malformed(String, &'static str, String),
}

impl fmt::Display for PhaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(why) => write!(f, "not a phase document: {why}"),
            Self::Missing(at, key) => write!(f, "{at}: no `{key}`"),
            Self::Unknown(at, key) => write!(f, "{at}: `{key}` is no key of a phase document"),
            Self::Duplicate(at, key) => write!(f, "{at}: `{key}` is written twice"),
            Self::Malformed(at, key, why) => write!(f, "{at}: `{key}` {why}"),
        }
    }
}

impl std::error::Error for PhaseError {}

/// Whether `text` is a phase's name.
#[must_use]
pub fn is_a_name(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// The keys of one place in the document, as written: key -> value.
type Keys = Vec<(String, String)>;

/// Parse a phase document.
///
/// # Errors
///
/// [`PhaseError`], naming the key: the grammar refused it; `name` or
/// `ground_rules` is absent, empty or written twice; a key the document does
/// not define; a transition without `to` or `kind`, to no name, of a kind
/// that is neither `suggested` nor `allowed`, to the phase itself, or
/// listed twice.
pub fn parse(input: &str) -> Result<Phase, PhaseError> {
    let file = PhaseParser::parse(Rule::file, input)
        .map_err(|err| PhaseError::Syntax(err.to_string()))?
        .next()
        .ok_or_else(|| PhaseError::Syntax("no document".to_owned()))?;
    let mut top: Keys = Vec::new();
    let mut transitions: Vec<Keys> = Vec::new();
    for pair in file.into_inner() {
        match pair.as_rule() {
            Rule::transitions => transitions.push(Vec::new()),
            Rule::pair => {
                let (key, value) = key_value(pair);
                match transitions.last_mut() {
                    Some(transition) => transition.push((key, value)),
                    None => top.push((key, value)),
                }
            }
            _ => {}
        }
    }
    let at = "phase".to_owned();
    let name = one(&top, &at, "name")?;
    if !is_a_name(&name) {
        return Err(PhaseError::Malformed(
            at,
            "name",
            format!("is `{name}`: a name is lowercase letters, digits, `-` and `_`"),
        ));
    }
    let ground_rules = one(&top, &at, "ground_rules")?.trim().to_owned();
    if ground_rules.is_empty() {
        return Err(PhaseError::Malformed(
            at,
            "ground_rules",
            "is empty".to_owned(),
        ));
    }
    only(&top, &at, &["name", "ground_rules"])?;
    let mut read: Vec<Transition> = Vec::new();
    for (index, keys) in transitions.iter().enumerate() {
        let at = format!("transitions[{index}]");
        only(keys, &at, &["to", "kind"])?;
        let to = one(keys, &at, "to")?;
        if !is_a_name(&to) {
            return Err(PhaseError::Malformed(
                at,
                "to",
                format!("is `{to}`, no phase's name"),
            ));
        }
        if to == name {
            return Err(PhaseError::Malformed(
                at,
                "to",
                format!("is `{to}`, this phase itself"),
            ));
        }
        if read.iter().any(|transition| transition.to == to) {
            return Err(PhaseError::Malformed(
                at,
                "to",
                format!("lists `{to}` a second time"),
            ));
        }
        let word = one(keys, &at, "kind")?;
        let kind = Kind::from_tag(&word).ok_or_else(|| {
            PhaseError::Malformed(
                at.clone(),
                "kind",
                format!("is `{word}`: a transition is `suggested` or `allowed`"),
            )
        })?;
        read.push(Transition { to, kind });
    }
    Ok(Phase {
        name,
        ground_rules,
        transitions: read,
    })
}

/// A `pair`'s key and its value as text.
fn key_value(pair: Pair<'_, Rule>) -> (String, String) {
    let mut inner = pair.into_inner();
    let key = inner
        .next()
        .map_or_else(String::new, |key| key.as_str().to_owned());
    let value = inner
        .next()
        .and_then(|value| value.into_inner().next())
        .map_or_else(String::new, |text| text.as_str().to_owned());
    (key, value)
}

/// The one value of `key` in `keys`.
fn one(keys: &Keys, at: &str, key: &'static str) -> Result<String, PhaseError> {
    let mut found = keys.iter().filter(|(written, _)| written == key);
    let value = found
        .next()
        .ok_or_else(|| PhaseError::Missing(at.to_owned(), key))?;
    if found.next().is_some() {
        return Err(PhaseError::Duplicate(at.to_owned(), key.to_owned()));
    }
    Ok(value.1.clone())
}

/// Refuse a key of `keys` that `known` does not name.
fn only(keys: &Keys, at: &str, known: &[&str]) -> Result<(), PhaseError> {
    match keys.iter().find(|(key, _)| !known.contains(&key.as_str())) {
        Some((key, _)) => Err(PhaseError::Unknown(at.to_owned(), key.clone())),
        None => Ok(()),
    }
}

/// A phase as the record's value space writes it.
#[must_use]
pub fn to_value(phase: &Phase) -> Value {
    let text = |text: &str| Value::String(text.to_owned());
    Value::Object(
        [
            ("name".to_owned(), text(&phase.name)),
            ("ground_rules".to_owned(), text(&phase.ground_rules)),
            (
                "transitions".to_owned(),
                Value::Array(
                    phase
                        .transitions
                        .iter()
                        .map(|transition| {
                            Value::Object(
                                [
                                    ("to".to_owned(), text(&transition.to)),
                                    ("kind".to_owned(), text(transition.kind.tag())),
                                ]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            ),
        ]
        .into(),
    )
}

/// Parse `source` and project it: what `diet check-phase` prints and the
/// conformance corpus pins.
///
/// # Errors
///
/// When `source` is not a phase document.
pub fn project(source: &str) -> Result<Value, String> {
    parse(source)
        .map(|phase| to_value(&phase))
        .map_err(|err| err.to_string())
}
