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

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;
use pest_derive::Parser;

use crate::formats::record::json::Value;

#[derive(Parser)]
#[grammar = "../formats/decline/grammar.pest"]
struct DeclineParser;

/// A decline, decomposed.
///
/// The parts are kept apart because they are answers to different questions,
/// and collapsing them is how a decline's reason text ends up in the record as
/// though it were a finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decline {
    /// The subject an English decline puts in front of the marker -- the
    /// `I have` of `I have nothing to add`. `None` for the bare registers.
    pub subject: Option<String>,
    /// The decline phrase exactly as written: `NONE`, `no plan change`,
    /// `nothing further to report`, `None at this time`. Preserved verbatim --
    /// the register an emitter used is evidence about the emitter.
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

    let mut subject = None;
    let mut marker = None;
    let mut scope = None;
    let mut reason = None;

    for pair in decline.into_inner() {
        match pair.as_rule() {
            Rule::subject => subject = Some(pair.as_str().to_owned()),
            Rule::phrase => marker = Some(pair.as_str().to_owned()),
            Rule::scope => scope = Some(text_of(&pair, Rule::scope_text)?),
            Rule::reason => reason = Some(reason_text(&pair)?),
            other => return Err(shape_of(other)),
        }
    }

    Ok(Decline {
        subject,
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
        Rule::paren_reason => text_of(&inner, Rule::paren_text),
        Rule::setoff_reason => text_of(&inner, Rule::reason_text),
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
        Rule::subject => ParseError::Shape("second subject"),
        Rule::phrase => ParseError::Shape("second phrase"),
        Rule::scope => ParseError::Shape("second scope"),
        Rule::reason => ParseError::Shape("second reason"),
        _ => ParseError::Shape("unexpected rule inside a decline"),
    }
}

/// This answer's classification, as the record's value space.
///
/// The one projection of this format. The conformance harness and the `diet`
/// CLI both call it, so a fixture cannot agree with the corpus while
/// disagreeing with what a caller actually receives.
///
/// # Errors
///
/// Returns the reason the answer is content rather than a decline.
pub fn project(source: &str) -> Result<Value, String> {
    match classify(source) {
        Classification::Decline(parsed) => Ok(projected(&parsed)),
        Classification::Content => Err(match parse(source) {
            Err(err) => err.to_string(),
            Ok(_) => "classify says content where parse succeeds: the two \
                      disagree about the same bytes"
                .to_owned(),
        }),
    }
}

/// One decline as a record value.
///
/// Public within the crate because the interview format carries declines
/// inside its fields and has to write them the same way. It had its own copy
/// of these six lines, which is the second implementation this crate exists
/// to refuse -- and the interview corpus covered neither `subject` nor
/// `scope`, so the copy could have dropped both and stayed green.
pub(crate) fn projected(parsed: &Decline) -> Value {
    let mut decline = BTreeMap::from([("marker".to_owned(), Value::String(parsed.marker.clone()))]);
    // Absent is an omitted key, as in the record: a null says the field
    // exists and holds nothing, which cannot be told apart from a field
    // nobody filled in.
    for (key, held) in [
        ("subject", &parsed.subject),
        ("scope", &parsed.scope),
        ("reason", &parsed.reason),
    ] {
        if let Some(text) = held {
            decline.insert(key.to_owned(), Value::String(text.clone()));
        }
    }
    Value::Object(BTreeMap::from([(
        "decline".to_owned(),
        Value::Object(decline),
    )]))
}

#[cfg(test)]
mod tests {
    use super::{Classification, Decline, classify, parse};

    /// The decline `answer` classifies as, or a panic naming what came back.
    ///
    /// Through `classify` on purpose. `classify` is the entry point every
    /// caller is told to use, and the tests that came before this one all went
    /// through `parse` -- so a `classify` that returned `Content` for
    /// everything passed all of them, and the whole gate stayed green.
    fn declined(answer: &str) -> Decline {
        match classify(answer) {
            Classification::Decline(declined) => declined,
            Classification::Content => panic!("{answer:?} classified as content"),
        }
    }

    fn is_content(answer: &str) -> bool {
        matches!(classify(answer), Classification::Content)
    }

    #[test]
    fn a_bare_marker_is_a_decline() {
        let parsed = declined("NONE");
        assert_eq!(parsed.marker, "NONE");
        assert_eq!(parsed.subject, None);
        assert_eq!(parsed.scope, None);
        assert_eq!(parsed.reason, None);
        assert!(classify("NONE").is_decline());
    }

    #[test]
    fn a_parenthetical_reason_is_preserved() {
        assert_eq!(
            declined("NONE (not building yet)").reason.as_deref(),
            Some("not building yet")
        );
    }

    #[test]
    fn a_scoped_decline_records_what_it_declines_about() {
        let parsed = declined("Nothing to add about the interview parser.");
        assert_eq!(parsed.marker, "Nothing to add");
        assert_eq!(parsed.scope.as_deref(), Some("the interview parser"));
    }

    #[test]
    fn an_english_decline_with_a_subject_is_a_decline() {
        let parsed = declined("I have nothing to add.");
        assert_eq!(parsed.subject.as_deref(), Some("I have"));
        assert_eq!(parsed.marker, "nothing to add");
    }

    // The bug this whole format exists to make impossible, in both the
    // adversative and the coordinating form. Only the first was caught when
    // this grammar first shipped, and the second read as a decline whose scope
    // was the rest of the sentence.
    #[test]
    fn a_coordinated_continuation_is_content_not_a_decline() {
        assert!(is_content(
            "Nothing to add about the parser, but the record schema changed."
        ));
        assert!(is_content(
            "Nothing to add about the parser and the record schema changed."
        ));
        assert!(is_content(
            "No plan change — and I did rename the capture lane."
        ));
    }

    // A conjunction is a token, not a substring: deleting one space must not
    // hide it.
    #[test]
    fn a_conjunction_after_a_comma_still_ends_the_reason() {
        assert!(is_content("No updates - a replay,but I shipped the parser"));
    }

    // Without a bound on the scope, an unpunctuated continuation is
    // indistinguishable from the name of a thing.
    #[test]
    fn a_scope_is_bounded_to_the_name_of_a_thing() {
        assert_eq!(
            declined("Nothing to add about the second interview parser.")
                .scope
                .as_deref(),
            Some("the second interview parser")
        );
        assert!(is_content("Nothing to add on Tuesday I shipped the parser"));
        assert!(is_content(
            "Nothing to add about the second interview parser rewrite"
        ));
    }

    // A dash set-off needs whitespace on both sides, or a hyphenated word
    // splits into a marker and a reason.
    #[test]
    fn a_hyphen_inside_a_word_is_not_a_set_off() {
        assert!(is_content("none-ish"));
        assert!(is_content("nil-terminated strings are the bug"));
        assert!(is_content("None-the-less the plan changed."));
    }

    // A parenthesis is a boundary the emitter drew, so the conjunction rule
    // does not apply inside one. Without this, a reason that merely contains
    // an ordinary `but` made the whole answer read as content.
    #[test]
    fn a_conjunction_inside_parentheses_is_ordinary_prose() {
        assert_eq!(
            declined("NONE (nothing but formatting)").reason.as_deref(),
            Some("nothing but formatting")
        );
        assert_eq!(
            declined("NONE (not building yet, and no plan to)")
                .reason
                .as_deref(),
            Some("not building yet, and no plan to")
        );
    }

    // A period ends a sentence only when whitespace follows it, so a version
    // number stays inside its reason and a second sentence does not.
    #[test]
    fn a_reason_is_one_sentence() {
        assert_eq!(
            declined("No changes - the pin is still v0.3")
                .reason
                .as_deref(),
            Some("the pin is still v0.3")
        );
        assert!(is_content(
            "No updates - the run was a replay. I also shipped the schema."
        ));
    }

    #[test]
    fn a_decline_is_one_line() {
        assert!(is_content(
            "NONE\n- rewrote the reconciler\n- shipped the schema"
        ));
        assert!(classify("\n\n  NONE  \n\n").is_decline());
    }

    #[test]
    fn a_decline_inside_a_sentence_is_not_a_decline() {
        assert!(is_content("The fork returned NONE for every field."));
        assert!(is_content("Nonetheless the plan changed."));
    }

    // Absence is not a decline. A lane that emitted nothing has not reported
    // that it had nothing to say, and conflating the two hides an empty lane.
    #[test]
    fn an_empty_answer_is_content_not_a_decline() {
        assert!(is_content(""));
        assert!(is_content("   \n\t "));
    }

    // A set-off announces a reason; one with nothing after it is a reason that
    // never arrived.
    #[test]
    fn a_set_off_with_no_reason_is_not_a_decline() {
        assert!(is_content("none:"));
        assert!(is_content("NONE -"));
    }

    // `classify` and `parse` are two views of one decision. If they can
    // disagree, the entry point and the diagnostic describe different formats.
    #[test]
    fn classify_and_parse_agree_over_the_committed_corpus() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("formats/decline/fixtures");
        let mut cases = 0_usize;
        for (bucket, want_decline) in [("valid", true), ("invalid", false)] {
            for entry in std::fs::read_dir(root.join(bucket)).expect("the corpus is readable") {
                let path = entry.expect("a readable entry").path();
                if path.extension().is_none_or(|ext| ext != "txt") {
                    continue;
                }
                let Ok(source) = std::fs::read_to_string(&path) else {
                    continue; // the deliberately non-UTF-8 case
                };
                cases += 1;
                let by_classify = classify(&source).is_decline();
                let by_parse = parse(&source).is_ok();
                assert_eq!(
                    by_classify,
                    by_parse,
                    "classify and parse disagree about {}",
                    path.display()
                );
                assert_eq!(
                    by_classify,
                    want_decline,
                    "{} is in {bucket}/ but classify says otherwise",
                    path.display()
                );
            }
        }
        // Without this the loop above would hold vacuously over an empty
        // corpus, which is the failure this repository exists to prevent.
        assert!(cases > 40, "the corpus shrank to {cases} readable cases");
    }
}
