//! The `interview` format, v0.
//!
//! The grammar at `diet/formats/interview/grammar.pest` is normative. This
//! module implements it and is the one authorized implementation.
//!
//! An interview answer is the prose a forked model returns when asked what it
//! decided, learned, or got stuck on this turn. Five defects in one lane, all
//! the same shape -- content discarded or corrupted with no signal -- are what
//! this format is built against; the grammar names each of them as a rule.
//!
//! The design consequence, stated once: **silent drop is not an outcome.**
//! Every region of the answer leaves the parser as a [`Field`], and every
//! field carries an [`Outcome`] naming what its text turned out to be. Text
//! that names no field is kept raw rather than skipped, because an
//! unrecognised register is evidence about the emitter and throwing it away is
//! how a register drifts without anyone noticing.

use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;
use pest_derive::Parser;

use super::decline;

#[derive(Parser)]
#[grammar = "../formats/interview/grammar.pest"]
struct InterviewParser;

/// A field an interview answer may carry.
///
/// An enum, not a string. Field kinds were matched as strings in more than one
/// place in the prior system and the places drifted; a variant added here is
/// caught by the compiler at every exhaustive match, and a string comparison
/// is not. The vocabulary is closed on purpose -- see the grammar for why an
/// open one reintroduces the continuation bug.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum FieldKind {
    /// API names the turn touched. Structuring-class: it has a source, and
    /// what it emits must be checkable against that source.
    ApiSurface,
    /// What the turn decided.
    Decision,
    /// Excerpts from tool output. Verbatim-class.
    Evidence,
    /// What the turn learned.
    Learned,
    /// What the turn intends next.
    Plan,
    /// What the turn could not get past.
    Stuck,
}

impl FieldKind {
    /// Every kind, in declaration order.
    ///
    /// Used by the coverage test: a kind absent from this list is a kind no
    /// fixture is required to exercise.
    pub const ALL: &'static [Self] = &[
        Self::ApiSurface,
        Self::Decision,
        Self::Evidence,
        Self::Learned,
        Self::Plan,
        Self::Stuck,
    ];

    /// The canonical tag for this kind, lower case and underscored.
    #[must_use]
    pub fn canonical_tag(self) -> &'static str {
        match self {
            Self::ApiSurface => "api_surface",
            Self::Decision => "decision",
            Self::Evidence => "evidence",
            Self::Learned => "learned",
            Self::Plan => "plan",
            Self::Stuck => "stuck",
        }
    }

    /// The kind a tag names, whatever case and separator it was written in.
    ///
    /// The grammar has already established that the text is one of the known
    /// names; this only has to say which. A `None` here means the grammar and
    /// this function have drifted apart, which is why the caller treats it as
    /// a shape error rather than as an unknown field.
    fn from_tag(written: &str) -> Option<Self> {
        let normalised: String = written
            .chars()
            .map(|c| {
                if c == '-' || c.is_whitespace() {
                    '_'
                } else {
                    c
                }
            })
            .flat_map(char::to_lowercase)
            .collect();
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.canonical_tag() == normalised)
    }
}

/// A tag, and the register it was written in.
///
/// `as_written` is kept because the register is evidence: a lane that starts
/// emitting `Decision —` where it used to emit `DECISION:` has changed
/// behaviour, and the only place that shows up is here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// Which field this is.
    pub kind: FieldKind,
    /// The tag exactly as it appeared, separator included: `**DECISION**:`,
    /// `## LEARNED`, `Decision —`.
    pub as_written: String,
}

/// What a field's text turned out to be.
///
/// Three outcomes, and no fourth called "dropped". A parser that cannot say
/// what happened to a region of its input is a parser that loses content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Content, with every continuation line present.
    Value(String),
    /// A decline, classified by [`super::decline`] -- the one authorized
    /// implementation of that family.
    Decline(decline::Decline),
    /// Text this format cannot assign to a field: prose before the first tag,
    /// or a tag announced with no value at all. The raw text is preserved
    /// verbatim, because the alternative is the silent drop.
    Unparseable {
        /// The text, exactly as it appeared.
        raw: String,
    },
}

/// One region of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The tag that introduced this region, or `None` for prose that carried
    /// no tag. `None` implies [`Outcome::Unparseable`].
    pub tag: Option<Tag>,
    /// What the region's text turned out to be.
    pub outcome: Outcome,
}

/// Why an answer is believed to have been cut off.
///
/// Named rather than boolean, because the two signals mean different things
/// about the emitter and a later reader will want to tell them apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TruncationSignal {
    /// The answer opened a code fence it never closed.
    UnterminatedFence,
    /// The answer's last tag was announced with no value after it.
    TrailingTagWithNoValue,
}

impl TruncationSignal {
    /// A stable name for the signal, for records and fixtures.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::UnterminatedFence => "unterminated-fence",
            Self::TrailingTagWithNoValue => "trailing-tag-with-no-value",
        }
    }
}

/// Whether the answer is all of the answer.
///
/// Truncation is a typed outcome and never "malformed": an emission that hit
/// its token cap produced exactly as much valid answer as it had room for, and
/// grading it as a parse failure discards the part that arrived.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// The answer ended where the emitter meant it to.
    Complete,
    /// The answer was cut off, on this evidence.
    Truncated(TruncationSignal),
}

/// A parsed interview answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// Every region of the answer, in the order it appeared. Nothing in the
    /// input is absent from this list.
    pub fields: Vec<Field>,
    /// Whether the answer is complete.
    pub completion: Completion,
}

impl Answer {
    /// The first field of `kind`, if the answer carries one.
    #[must_use]
    pub fn field(&self, kind: FieldKind) -> Option<&Field> {
        self.fields
            .iter()
            .find(|field| field.tag.as_ref().is_some_and(|tag| tag.kind == kind))
    }

    /// Whether the answer was cut off.
    #[must_use]
    pub fn is_truncated(&self) -> bool {
        matches!(self.completion, Completion::Truncated(_))
    }
}

/// Why a text is not an interview answer.
#[derive(Debug)]
pub enum ParseError {
    /// The text does not match the grammar.
    Syntax(Box<pest::error::Error<Rule>>),
    /// The text carries a C0 control character no emitter produces. A format
    /// that lets these into the permanent record has accepted corruption as
    /// content.
    ControlCharacter {
        /// The offending code point.
        code: u32,
        /// Its byte offset in the input.
        offset: usize,
    },
    /// The grammar matched but produced a shape this module does not expect,
    /// which means the two have drifted apart.
    Shape(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "not an interview answer: {err}"),
            Self::ControlCharacter { code, offset } => write!(
                f,
                "control character U+{code:04X} at byte {offset}: an emitted \
                 answer carries text, and a C0 control other than tab, newline \
                 or carriage return is corruption"
            ),
            Self::Shape(what) => write!(f, "grammar and parser disagree: {what}"),
        }
    }
}

impl Error for ParseError {}

/// Parse an interview answer.
///
/// # Errors
///
/// Returns [`ParseError`] if the text is not an interview answer at all. Note
/// that this is a narrow door: unrecognised registers, missing fields, prose
/// with no tags and truncated emissions all parse -- they are outcomes, not
/// failures. What does not parse is text that is not an emitted answer.
pub fn parse(input: &str) -> Result<Answer, ParseError> {
    if let Some((offset, ch)) = input
        .char_indices()
        .find(|(_, ch)| ch.is_control() && !matches!(ch, '\t' | '\n' | '\r'))
    {
        return Err(ParseError::ControlCharacter {
            code: ch as u32,
            offset,
        });
    }

    let mut parsed = InterviewParser::parse(Rule::document, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?;
    let document = parsed.next().ok_or(ParseError::Shape("no document"))?;
    let shape = document
        .into_inner()
        .find(|pair| {
            matches!(
                pair.as_rule(),
                Rule::wrapped | Rule::unterminated | Rule::bare
            )
        })
        .ok_or(ParseError::Shape("document holds no body shape"))?;

    let unterminated = shape.as_rule() == Rule::unterminated;
    let body = shape
        .into_inner()
        .find(|pair| matches!(pair.as_rule(), Rule::fenced_body | Rule::plain_body))
        .ok_or(ParseError::Shape("body shape holds no body"))?;

    let fields = walk_body(&body)?;
    let completion = completion_of(unterminated, &fields);
    Ok(Answer { fields, completion })
}

/// Which truncation signal, if any, fired.
fn completion_of(unterminated: bool, fields: &[Field]) -> Completion {
    if unterminated {
        return Completion::Truncated(TruncationSignal::UnterminatedFence);
    }
    // A tag announced as the last thing in the answer, with nothing after it,
    // is what a token cap looks like when it lands between a field's tag and
    // its value. Reading it as an empty value would record a field the emitter
    // never filled in.
    match fields.last() {
        Some(Field {
            tag: Some(_),
            outcome: Outcome::Unparseable { .. },
        }) => Completion::Truncated(TruncationSignal::TrailingTagWithNoValue),
        _ => Completion::Complete,
    }
}

/// Every field in a body, in source order.
fn walk_body(body: &Pair<'_, Rule>) -> Result<Vec<Field>, ParseError> {
    let mut fields = Vec::new();
    for pair in body.clone().into_inner() {
        match pair.as_rule() {
            Rule::fenced_preamble | Rule::plain_preamble => {
                let raw = joined_lines(&pair);
                if !raw.trim().is_empty() {
                    fields.push(Field {
                        tag: None,
                        outcome: Outcome::Unparseable { raw },
                    });
                }
            }
            Rule::fenced_field | Rule::plain_field => fields.push(walk_field(&pair)?),
            _ => return Err(ParseError::Shape("unexpected rule inside a body")),
        }
    }
    Ok(fields)
}

/// One tagged field: its tag as written, and its value with every
/// continuation line present.
fn walk_field<'i>(field: &Pair<'i, Rule>) -> Result<Field, ParseError> {
    let mut inner = field.clone().into_inner();
    let tag_line = inner
        .next()
        .filter(|pair| pair.as_rule() == Rule::tag_line)
        .ok_or(ParseError::Shape("a field with no tag line"))?;

    let mut as_written = None;
    let mut head: &'i str = "";
    for pair in tag_line.into_inner() {
        match pair.as_rule() {
            Rule::heading_tag | Rule::inline_tag => {
                as_written = Some(pair.as_str().trim_end().to_owned());
            }
            Rule::value_head => head = strip_eol(pair.as_str()),
            _ => return Err(ParseError::Shape("unexpected rule inside a tag line")),
        }
    }
    let as_written = as_written.ok_or(ParseError::Shape("a tag line with no tag"))?;
    let kind = FieldKind::from_tag(strip_tag_punctuation(&as_written)).ok_or(ParseError::Shape(
        "the grammar accepted a tag this crate cannot name",
    ))?;

    // The continuation lines, unconditionally. Not "indented lines", not
    // "lines before the next blank line": those heuristics lost 71 answers'
    // worth of content, and the grammar deliberately offers no way to express
    // them here.
    let mut lines: Vec<String> = Vec::new();
    let trimmed_head = head.trim_start();
    if !trimmed_head.is_empty() {
        lines.push(trimmed_head.to_owned());
    }
    for pair in inner {
        match pair.as_rule() {
            Rule::fenced_loose | Rule::plain_loose => {
                lines.push(strip_eol(pair.as_str()).to_owned());
            }
            _ => return Err(ParseError::Shape("unexpected rule inside a field")),
        }
    }

    let value = lines.join("\n");
    let value = value.trim_end();

    let outcome = if value.is_empty() {
        Outcome::Unparseable {
            raw: field.as_str().to_owned(),
        }
    } else {
        match decline::classify(value) {
            decline::Classification::Decline(declined) => Outcome::Decline(declined),
            decline::Classification::Content => Outcome::Value(value.to_owned()),
        }
    };

    Ok(Field {
        tag: Some(Tag { kind, as_written }),
        outcome,
    })
}

/// The text of a run of loose lines, newlines preserved and the trailing one
/// removed.
fn joined_lines(pair: &Pair<'_, Rule>) -> String {
    pair.clone()
        .into_inner()
        .map(|line| strip_eol(line.as_str()).to_owned())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_owned()
}

/// A line without its terminator. Handles both endings, so an answer emitted
/// on a Windows host parses to the same value as one emitted anywhere else.
fn strip_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// A tag with its decoration removed, for naming the kind: heading marks,
/// emphasis markers and the trailing separator.
fn strip_tag_punctuation(as_written: &str) -> &str {
    as_written.trim_matches(|c: char| {
        matches!(
            c,
            '#' | '*' | '_' | '`' | ':' | '：' | '—' | '–' | '-' | '=' | '»' | '>'
        ) || c.is_whitespace()
    })
}

#[cfg(test)]
mod tests {
    use super::{Completion, FieldKind, Outcome, TruncationSignal, parse};

    fn value(source: &str, kind: FieldKind) -> String {
        let answer = parse(source).expect("an interview answer");
        match &answer.field(kind).expect("the field is present").outcome {
            Outcome::Value(text) => text.clone(),
            other => panic!("expected a value, got {other:?}"),
        }
    }

    // Defect one: the bold markers were captured as part of the value.
    #[test]
    fn emphasis_around_a_tag_is_not_part_of_the_value() {
        assert_eq!(
            value(
                "**DECISION**: keep the two-turn design\n",
                FieldKind::Decision
            ),
            "keep the two-turn design"
        );
    }

    // Defect two: a wrapper fence made the parser see zero fields.
    #[test]
    fn a_wrapper_fence_does_not_hide_the_fields() {
        let answer = parse("```\nDECISION: keep it\n```\n").expect("an answer");
        assert_eq!(answer.fields.len(), 1);
        assert_eq!(answer.completion, Completion::Complete);
    }

    // Defect three: a leading `#` that was part of the content was stripped.
    #[test]
    fn a_hash_in_the_content_survives() {
        assert_eq!(
            value("LEARNED: the fix\n# not a heading\n", FieldKind::Learned),
            "the fix\n# not a heading"
        );
    }

    // Defect four, the expensive one: 71 of 630 answers lost their second and
    // later lines.
    #[test]
    fn every_continuation_line_is_present_in_the_value() {
        let source = "DECISION: first\nsecond\n\nfourth\n";
        assert_eq!(
            value(source, FieldKind::Decision),
            "first\nsecond\n\nfourth"
        );
    }

    // Defect five: a close-cousin register matched nothing.
    #[test]
    fn a_cousin_register_is_the_same_field() {
        assert_eq!(
            value("Decision — keep it\n", FieldKind::Decision),
            "keep it"
        );
    }

    #[test]
    fn a_fence_inside_a_value_stays_in_the_value() {
        let source = "PLAN: ship\n```rust\nlet x = 1;\n```\n";
        assert_eq!(
            value(source, FieldKind::Plan),
            "ship\n```rust\nlet x = 1;\n```"
        );
    }

    #[test]
    fn a_decline_is_a_decline_and_not_a_value() {
        let answer = parse("DECISION: NONE (not building yet)\n").expect("an answer");
        let outcome = &answer.field(FieldKind::Decision).expect("present").outcome;
        let Outcome::Decline(declined) = outcome else {
            panic!("expected a decline, got {outcome:?}");
        };
        assert_eq!(declined.reason.as_deref(), Some("not building yet"));
    }

    #[test]
    fn prose_before_the_first_tag_is_kept_raw() {
        let answer = parse("Here is what I found.\nDECISION: keep it\n").expect("an answer");
        assert!(matches!(
            answer.fields.first().map(|f| &f.outcome),
            Some(Outcome::Unparseable { .. })
        ));
        assert_eq!(answer.fields.len(), 2);
    }

    #[test]
    fn an_unterminated_fence_reads_as_truncation_not_as_damage() {
        let answer = parse("```\nDECISION: keep it\n").expect("an answer");
        assert_eq!(
            answer.completion,
            Completion::Truncated(TruncationSignal::UnterminatedFence)
        );
    }

    #[test]
    fn a_trailing_tag_with_no_value_reads_as_truncation() {
        let answer = parse("DECISION: keep it\nLEARNED:").expect("an answer");
        assert_eq!(
            answer.completion,
            Completion::Truncated(TruncationSignal::TrailingTagWithNoValue)
        );
    }

    #[test]
    fn crlf_parses_to_the_same_value_as_lf() {
        assert_eq!(
            value("DECISION: a\r\nb\r\n", FieldKind::Decision),
            value("DECISION: a\nb\n", FieldKind::Decision)
        );
    }

    #[test]
    fn a_control_character_is_a_rejection_not_a_value() {
        assert!(parse("DECISION: a\u{7}b\n").is_err());
        assert!(parse("DECISION: a\u{0}b\n").is_err());
    }

    // `API SURFACE` and `api-surface` name the same field as `api_surface`.
    // The canonical-tag sweep below cannot see this: no canonical tag carries
    // a space, so a normaliser that dropped spaces instead of folding them
    // would pass it and fail here.
    #[test]
    fn a_spaced_or_hyphenated_tag_names_the_same_field() {
        for source in ["API SURFACE: parse\n", "api-surface: parse\n"] {
            let answer = parse(source).expect("an answer");
            assert!(
                answer.field(FieldKind::ApiSurface).is_some(),
                "{source:?} did not parse to api_surface"
            );
        }
    }

    // A field kind with no fixture has an untested parse path, and in the
    // prior system that is precisely where a silent drop lived. Adding a
    // variant to `FieldKind` without adding a fixture is a failure here.
    #[test]
    fn every_field_kind_appears_in_the_committed_corpus() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("formats/interview/fixtures/valid");
        let mut seen = std::collections::BTreeSet::new();
        let mut cases = 0_usize;
        for entry in std::fs::read_dir(&dir).expect("the corpus is readable") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|ext| ext != "txt") {
                continue;
            }
            cases += 1;
            let source = std::fs::read_to_string(&path).expect("a valid case is UTF-8");
            let answer = parse(&source).expect("a valid case parses");
            seen.extend(
                answer
                    .fields
                    .iter()
                    .filter_map(|f| f.tag.as_ref())
                    .map(|t| t.kind),
            );
        }
        // Without this the assertion below would hold vacuously over an empty
        // directory, which is the failure this repository exists to prevent.
        assert!(cases > 0, "{} holds no cases", dir.display());
        let missing: Vec<_> = FieldKind::ALL
            .iter()
            .filter(|kind| !seen.contains(kind))
            .map(|kind| kind.canonical_tag())
            .collect();
        assert!(
            missing.is_empty(),
            "field kind(s) with no fixture in {}: {missing:?}",
            dir.display()
        );
    }

    #[test]
    fn every_field_kind_is_reachable_from_its_canonical_tag() {
        for kind in FieldKind::ALL {
            let source = format!("{}: something\n", kind.canonical_tag());
            let answer = parse(&source).expect("a canonical tag is an answer");
            assert!(
                answer.field(*kind).is_some(),
                "{} did not parse to its own kind",
                kind.canonical_tag()
            );
        }
    }
}
