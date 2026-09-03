//! The `verdict` format, v0.
//!
//! The grammar at `diet/formats/verdict/grammar.pest` is normative. This
//! module implements it and is the **one authorized implementation**: the
//! collector's confirm fork, the `resolve_entry` tool and the tangent's
//! disposition all read a verdict through here.
//!
//! A verdict is the whole answer of a single-turn fork asked what new prose
//! does to a nominated entry: `DONE`, `PARTIAL`, `NOT_THIS` or `SUPERSEDED`.
//! The model judges; the grammar formats; the reconciler applies -- a link,
//! never an overwrite. The reconciler branches on the word, which is why the
//! word must be whole, first and alone: a reader that searched the answer
//! for a verdict would find one in `not DONE yet`, with the opposite meaning,
//! and a reader that accepted two would hand the reconciler a branch nobody
//! wrote. See the grammar for the four rules.
//!
//! An empty answer is not a verdict. The caller records a silence, which is
//! a typed outcome of its own and never a default verdict.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest_derive::Parser;

use crate::formats::record::json::Value;

#[derive(Parser)]
#[grammar = "../formats/verdict/grammar.pest"]
struct VerdictParser;

/// What the fork judged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// The entry is settled by the new prose: resolve it.
    Done,
    /// The new prose bears on the entry without settling it: no patch.
    Partial,
    /// The nomination was wrong: no patch, and one false nomination counted.
    NotThis,
    /// The new prose replaces the entry: supersede it, link and void.
    Superseded,
}

impl Verdict {
    /// Every verdict, in declaration order.
    pub const ALL: &'static [Self] = &[Self::Done, Self::Partial, Self::NotThis, Self::Superseded];

    /// The canonical spelling.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Partial => "partial",
            Self::NotThis => "not_this",
            Self::Superseded => "superseded",
        }
    }

    /// The verdict a spelling names, whatever its case, with `NOT THIS`
    /// read as `NOT_THIS`. The grammar has already established that the
    /// text is one of the four; this only has to say which, by iterating
    /// [`Self::ALL`].
    fn from_written(written: &str) -> Option<Self> {
        let normalised: String = written
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .flat_map(char::to_lowercase)
            .collect();
        Self::ALL
            .iter()
            .copied()
            .find(|verdict| verdict.tag() == normalised)
    }
}

/// A verdict, with its reason if one was given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The judgment.
    pub verdict: Verdict,
    /// Why, verbatim, typed as a reason so no downstream pass can mistake
    /// it for content.
    pub reason: Option<String>,
}

/// Why a text is not a verdict.
#[derive(Debug)]
pub enum ParseError {
    /// The text does not match the grammar.
    Syntax(Box<pest::error::Error<Rule>>),
    /// The grammar matched but produced a shape this module does not expect,
    /// which means the two have drifted apart.
    Shape(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "not a verdict: {err}"),
            Self::Shape(what) => write!(f, "grammar and parser disagree: {what}"),
        }
    }
}

impl Error for ParseError {}

/// Parse an answer as a verdict.
///
/// # Errors
///
/// Returns [`ParseError::Syntax`] if the answer is not one verdict, whole,
/// first and alone.
pub fn parse(input: &str) -> Result<Answer, ParseError> {
    let mut document = VerdictParser::parse(Rule::document, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?;
    let document = document.next().ok_or(ParseError::Shape("no document"))?;
    let mut verdict = None;
    let mut reason = None;
    for pair in document.into_inner() {
        match pair.as_rule() {
            Rule::verdict => {
                verdict = Some(
                    Verdict::from_written(pair.as_str())
                        .ok_or(ParseError::Shape("a verdict the vocabulary does not name"))?,
                );
            }
            Rule::reason => {
                let text = pair
                    .into_inner()
                    .next()
                    .and_then(|kind| kind.into_inner().next())
                    .ok_or(ParseError::Shape("a reason with no text"))?;
                reason = Some(text.as_str().trim().to_owned());
            }
            Rule::EOI => {}
            _ => return Err(ParseError::Shape("an unexpected rule inside the document")),
        }
    }
    Ok(Answer {
        verdict: verdict.ok_or(ParseError::Shape("a document with no verdict"))?,
        reason,
    })
}

/// This answer, as the record's value space.
///
/// # Errors
///
/// Returns the reason the text is not a verdict.
pub fn project(source: &str) -> Result<Value, String> {
    parse(source)
        .map(|answer| {
            let mut members = BTreeMap::from([(
                "verdict".to_owned(),
                Value::String(answer.verdict.tag().to_owned()),
            )]);
            if let Some(reason) = answer.reason {
                members.insert("reason".to_owned(), Value::String(reason));
            }
            Value::Object(members)
        })
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::{Verdict, parse};

    #[test]
    fn every_verdict_reads_back_from_its_own_tag_in_any_case() {
        for verdict in Verdict::ALL {
            let upper = verdict.tag().to_uppercase();
            assert_eq!(
                parse(&upper).map(|a| a.verdict).ok(),
                Some(*verdict),
                "{upper}"
            );
            assert_eq!(parse(verdict.tag()).map(|a| a.verdict).ok(), Some(*verdict));
        }
        assert_eq!(
            parse("not this").map(|a| a.verdict).ok(),
            Some(Verdict::NotThis)
        );
    }

    #[test]
    fn a_reason_is_kept_and_typed_as_a_reason() {
        let answer =
            parse("SUPERSEDED: the prose names v0.3 where the entry says v0.2").expect("a verdict");
        assert_eq!(answer.verdict, Verdict::Superseded);
        assert_eq!(
            answer.reason.as_deref(),
            Some("the prose names v0.3 where the entry says v0.2")
        );
        assert_eq!(
            parse("PARTIAL (the test still fails)")
                .expect("a verdict")
                .reason
                .as_deref(),
            Some("the test still fails")
        );
        assert_eq!(parse("DONE").expect("a verdict").reason, None);
    }

    // The four rules, each with the tolerant reading it closes.
    #[test]
    fn prose_around_a_verdict_is_not_a_verdict() {
        for text in [
            "I think it is DONE",
            "not DONE yet",
            "DONEISH",
            "PARTIALLY",
            "DONE, or PARTIAL",
            "DONE\nPARTIAL",
            "SUPERSEDED because it is DONE",
            "",
            "   ",
            "(no reason)",
            "DONE -",
        ] {
            assert!(
                parse(text).is_err(),
                "{text:?} was read as a verdict, and it is not one"
            );
        }
    }
}
