//! The `decline` format, v0.
//!
//! The grammar at `diet/formats/decline/grammar.pest` is normative. This
//! module implements it and is the **one authorized implementation** of the
//! decline family: anything else that must tell a decline from content --
//! a grader, an instrument, the SPA -- calls [`classify`] or passes the same
//! conformance corpus. A second detector that does not pass the corpus is not
//! a decline detector, however plausible its regex looks.
//!
//! The rule that closed this class: a decline is not a substring, it is a
//! property of the whole answer. See the grammar for why.

use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "../formats/decline/grammar.pest"]
struct DeclineParser;

/// A decline, decomposed.
///
/// The three parts are kept apart because they are answers to different
/// questions, and collapsing them is how a decline's reason text ends up in
/// the record as though it were a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decline {
    /// The decline phrase exactly as written: `NONE`, `no plan change`,
    /// `nothing further to report`. Preserved verbatim -- the register an
    /// emitter used is evidence about the emitter.
    pub marker: String,
    /// What the answer declines *about*, if it said: the text after
    /// `about` / `regarding` / `on`.
    pub scope: Option<String>,
    /// Why, if the answer gave a reason. Preserved verbatim, and typed as a
    /// reason so that no downstream pass can mistake it for content.
    pub reason: Option<String>,
}

/// What an answer is.
///
/// An enum rather than a `bool` so that a caller must name the case it is
/// handling, and so that a later third outcome cannot be bolted on as a
/// second boolean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    /// The answer reports there is nothing to record.
    Decline(Decline),
    /// The answer carries content. This includes an empty answer: absence is
    /// not a decline, and treating it as one would let a lane that produced
    /// nothing at all read as a lane that had nothing to say.
    Content,
}

impl Classification {
    /// Whether this answer declines.
    #[must_use]
    pub fn is_decline(&self) -> bool {
        matches!(self, Self::Decline(_))
    }
}

/// Why a text is not a decline.
#[derive(Debug)]
pub enum ParseError {
    /// The text does not match the decline grammar.
    Syntax(Box<pest::error::Error<Rule>>),
    /// The grammar matched but produced a shape this module does not expect,
    /// which means the two have drifted apart.
    Shape(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "not a decline: {err}"),
            Self::Shape(what) => write!(f, "grammar and parser disagree: {what}"),
        }
    }
}

impl Error for ParseError {}

/// Classify an answer. This is the entry point every caller should use.
///
/// Never returns an error: an answer that is not a decline is
/// [`Classification::Content`], which is a verdict rather than a failure.
#[must_use]
pub fn classify(answer: &str) -> Classification {
    parse(answer).map_or(Classification::Content, Classification::Decline)
}

/// Parse `input` as a decline, reporting *why* it is not one when it is not.
///
/// [`classify`] is the ordinary entry point; this exists for the conformance
/// harness and for diagnostics, where the reason a text was read as content
/// is the interesting part.
///
/// # Errors
///
/// Returns [`ParseError::Syntax`] if the text is not a decline.
pub fn parse(input: &str) -> Result<Decline, ParseError> {
    let mut document = DeclineParser::parse(Rule::document, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?;

    let document = document.next().ok_or(ParseError::Shape("no document"))?;
    let decline = document
        .into_inner()
        .find(|pair| pair.as_rule() == Rule::decline)
        .ok_or(ParseError::Shape("document holds no decline"))?;

    let mut marker = None;
    let mut scope = None;
    let mut reason = None;

    for pair in decline.into_inner() {
        match pair.as_rule() {
            Rule::phrase => marker = Some(pair.as_str().to_owned()),
            Rule::scope => scope = Some(text_of(&pair, Rule::scope_text)?),
            Rule::reason => reason = Some(reason_text(&pair)?),
            other => return Err(shape_of(other)),
        }
    }

    Ok(Decline {
        marker: marker.ok_or(ParseError::Shape("decline with no phrase"))?,
        scope,
        reason,
    })
}

/// The span of the first `wanted` inside `pair`.
fn text_of(pair: &Pair<'_, Rule>, wanted: Rule) -> Result<String, ParseError> {
    pair.clone()
        .into_inner()
        .find(|inner| inner.as_rule() == wanted)
        .map(|inner| inner.as_str().to_owned())
        .ok_or(ParseError::Shape(
            "a rule is missing the text it must carry",
        ))
}

/// The reason text, from whichever of the two delimited forms matched.
fn reason_text(pair: &Pair<'_, Rule>) -> Result<String, ParseError> {
    let inner = pair
        .clone()
        .into_inner()
        .next()
        .ok_or(ParseError::Shape("reason with no form"))?;
    match inner.as_rule() {
        Rule::paren_reason | Rule::setoff_reason => text_of(&inner, Rule::reason_text),
        other => Err(shape_of(other)),
    }
}

/// A `Shape` error naming the rule that surprised us.
///
/// The rule set is generated from the grammar, so a new rule appearing in a
/// position this module walks is a drift between the two; saying which rule
/// is what makes that drift diagnosable rather than merely fatal.
fn shape_of(rule: Rule) -> ParseError {
    match rule {
        Rule::document => ParseError::Shape("nested document"),
        Rule::decline => ParseError::Shape("nested decline"),
        Rule::phrase => ParseError::Shape("second phrase"),
        Rule::scope => ParseError::Shape("second scope"),
        Rule::reason => ParseError::Shape("second reason"),
        _ => ParseError::Shape("unexpected rule inside a decline"),
    }
}

#[cfg(test)]
mod tests {
    use super::{Classification, classify, parse};

    #[test]
    fn a_bare_marker_is_a_decline() {
        let parsed = parse("NONE").expect("NONE is a decline");
        assert_eq!(parsed.marker, "NONE");
        assert_eq!(parsed.scope, None);
        assert_eq!(parsed.reason, None);
    }

    #[test]
    fn a_parenthetical_reason_is_preserved() {
        let parsed = parse("NONE (not building yet)").expect("a reason is still a decline");
        assert_eq!(parsed.reason.as_deref(), Some("not building yet"));
    }

    #[test]
    fn a_scoped_decline_records_what_it_declines_about() {
        let parsed = parse("Nothing to add about the interview parser.").expect("a decline");
        assert_eq!(parsed.marker, "Nothing to add");
        assert_eq!(parsed.scope.as_deref(), Some("the interview parser"));
    }

    // The bug this whole format exists to make impossible: an answer that
    // opens with a decline's words and then says something.
    #[test]
    fn a_coordinated_continuation_is_content_not_a_decline() {
        let answer = "Nothing to add about the parser, but the record schema changed.";
        assert_eq!(classify(answer), Classification::Content);
    }

    #[test]
    fn a_decline_inside_a_sentence_is_not_a_decline() {
        assert_eq!(
            classify("The fork returned NONE for every field."),
            Classification::Content
        );
    }

    // Absence is not a decline. A lane that emitted nothing has not reported
    // that it had nothing to say, and conflating the two hides an empty lane.
    #[test]
    fn an_empty_answer_is_content_not_a_decline() {
        assert_eq!(classify(""), Classification::Content);
        assert_eq!(classify("   \n\t "), Classification::Content);
    }

    #[test]
    fn classify_never_errors_on_arbitrary_prose() {
        for answer in ["", "\u{0}", "but", "none-ish", "NONETHELESS"] {
            let _ = classify(answer);
        }
    }
}
