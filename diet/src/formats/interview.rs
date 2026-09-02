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
    /// or a tag announced with no value at all. What it said is in
    /// [`Field::raw`], like every other field's text.
    Unparseable,
}

/// One region of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    /// The tag that introduced this region, or `None` for prose that carried
    /// no tag. `None` implies [`Outcome::Unparseable`].
    pub tag: Option<Tag>,
    /// The region's text exactly as written, before it was typed.
    ///
    /// Every field carries it, not only the unparseable ones. Typing is
    /// lossy -- a decline decomposes into a marker and a reason and its
    /// brackets are gone -- and a format built against silent loss cannot
    /// have a lossy step with nothing behind it.
    pub raw: String,
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
    /// The emitter produced nothing at all. Not `Complete`: a lane that
    /// emitted nothing has not finished an answer, and reading it as finished
    /// is the same conflation `decline` refuses to make about absence.
    Empty,
    /// The answer was cut off, on this evidence.
    Truncated(TruncationSignal),
}

/// The code fence an emitter wrapped the whole answer in.
///
/// Kept rather than discarded. The first version of this format dropped both
/// fence lines and everything after the closing one, which is how a format
/// built against silent loss shipped with silent loss in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wrapper {
    /// The opening fence line as written, info string included.
    pub open: String,
    /// The closing fence line, or `None` when the emission stopped first.
    pub close: Option<String>,
    /// How many fields came from inside the wrapper. Fields at or after this
    /// index were written after the closing fence, and are kept because an
    /// emitter that adds a remark after its own fence has still said it.
    pub fields_inside: usize,
}

/// A parsed interview answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The wrapper fence, if the answer had one.
    pub wrapper: Option<Wrapper>,
    /// Every region of the answer, in the order it appeared.
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

    /// Everything this parse kept, in document order.
    ///
    /// The claim "silent drop is not an outcome" is only worth making if a
    /// caller can check it, so this hands back every piece of text the parse
    /// holds. `every_byte_of_an_answer_is_accounted_for` compares it against
    /// the input; anything this does not return is content the parser lost.
    #[must_use]
    pub fn accounted_text(&self) -> String {
        let mut out = String::new();
        let inside = self.wrapper.as_ref().map_or(self.fields.len(), |wrapper| {
            out.push_str(&wrapper.open);
            out.push('\n');
            wrapper.fields_inside.min(self.fields.len())
        });
        for field in &self.fields[..inside] {
            field.append_text(&mut out);
        }
        if let Some(close) = self.wrapper.as_ref().and_then(|w| w.close.as_ref()) {
            out.push_str(close);
            out.push('\n');
        }
        for field in &self.fields[inside..] {
            field.append_text(&mut out);
        }
        out
    }
}

impl Field {
    /// This field's text -- its tag as written, then its content.
    fn append_text(&self, out: &mut String) {
        if let Some(tag) = &self.tag {
            out.push_str(&tag.as_written);
            out.push('\n');
        }
        out.push_str(&self.raw);
        out.push('\n');
    }
}

/// Why a text is not an interview answer.
#[derive(Debug)]
pub enum ParseError {
    /// The text does not match the grammar.
    Syntax(Box<pest::error::Error<Rule>>),
    /// The text carries a NUL byte, so it is not a text emission at all.
    ///
    /// The only content rejection this format makes. An earlier version
    /// rejected every C0 control, which threw away two well-formed fields
    /// because a third quoted a terminal escape from tool output -- and an
    /// `EVIDENCE` field is verbatim tool output, where escapes are ordinary.
    /// Rejecting a whole answer contradicts both of this format's own laws:
    /// silent drop is not an outcome, and a damaged emission is a typed
    /// outcome rather than a parse failure.
    NulByte {
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
            Self::NulByte { offset } => write!(
                f,
                "a NUL byte at offset {offset}: an emitted answer is text, and \
                 a NUL is where a byte stream ends rather than something an \
                 emitter writes"
            ),
            Self::Shape(what) => write!(f, "grammar and parser disagree: {what}"),
        }
    }
}

impl Error for ParseError {}

// ---------------------------------------------------------------------------
// lines
// ---------------------------------------------------------------------------

/// One line of an answer, classified.
#[derive(Debug, Clone)]
enum Line {
    /// A tag line: the tag as written, and whatever followed the separator.
    Tag {
        kind: FieldKind,
        as_written: String,
        head: String,
    },
    /// A fence line: the whole line, and its marker, which is what pairs it.
    Fence { as_written: String, marker: String },
    /// Anything else.
    Text { text: String },
}

impl Line {
    /// The line as written, without its terminator.
    fn as_written(&self) -> String {
        match self {
            Self::Tag {
                as_written, head, ..
            } => {
                if head.is_empty() {
                    as_written.clone()
                } else {
                    format!("{as_written}{head}")
                }
            }
            Self::Fence { as_written, .. } => as_written.clone(),
            Self::Text { text } => text.clone(),
        }
    }
}

/// Parse an interview answer.
///
/// # Errors
///
/// Returns [`ParseError`] only for text that is not an emitted answer at all.
/// Unrecognised registers, missing fields, prose with no tags, control
/// characters and truncated emissions all parse -- they are outcomes, not
/// failures.
pub fn parse(input: &str) -> Result<Answer, ParseError> {
    if let Some(offset) = input.find('\0') {
        return Err(ParseError::NulByte { offset });
    }

    let mut parsed = InterviewParser::parse(Rule::document, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?;
    let document = parsed.next().ok_or(ParseError::Shape("no document"))?;

    let mut lines = Vec::new();
    for pair in document.into_inner() {
        if pair.as_rule() != Rule::line {
            continue; // EOI
        }
        lines.push(classify_line(&pair)?);
    }

    Ok(assemble(&lines))
}

/// One line, from its parse.
fn classify_line(line: &Pair<'_, Rule>) -> Result<Line, ParseError> {
    let inner = line
        .clone()
        .into_inner()
        .next()
        .ok_or(ParseError::Shape("a line with no content"))?;
    match inner.as_rule() {
        Rule::tag_line => {
            // The tag as written runs from the start of the line through the
            // separator: leading whitespace, bullet, heading marks, emphasis
            // and all. The register an emitter used is evidence about it, and
            // the value must not carry any of it -- that was defect one.
            let mut as_written = None;
            let mut head: &str = "";
            for pair in inner.clone().into_inner() {
                match pair.as_rule() {
                    Rule::bullet | Rule::heading_tag | Rule::inline_tag => {
                        let end = pair.as_span().end() - inner.as_span().start();
                        as_written = Some(&inner.as_str()[..end]);
                    }
                    Rule::value_head => head = strip_eol(pair.as_str()),
                    _ => return Err(ParseError::Shape("unexpected rule inside a tag line")),
                }
            }
            let as_written = as_written.ok_or(ParseError::Shape("a tag line with no tag"))?;
            let kind = FieldKind::from_tag(strip_tag_punctuation(as_written)).ok_or(
                ParseError::Shape("the grammar accepted a tag this crate cannot name"),
            )?;
            Ok(Line::Tag {
                kind,
                as_written: as_written.to_owned(),
                head: head.to_owned(),
            })
        }
        Rule::fence_line => {
            let marker = inner
                .clone()
                .into_inner()
                .find(|pair| pair.as_rule() == Rule::fence_marker)
                .ok_or(ParseError::Shape("a fence line with no marker"))?;
            Ok(Line::Fence {
                as_written: strip_eol(inner.as_str()).to_owned(),
                marker: marker.as_str().to_owned(),
            })
        }
        Rule::text_line => Ok(Line::Text {
            text: strip_eol(inner.as_str()).to_owned(),
        }),
        _ => Err(ParseError::Shape("an unexpected kind of line")),
    }
}

/// Pair the wrapper's fences and group the rest into fields.
///
/// The fence pairing is the rule a PEG cannot state, because it is counting:
/// an answer that opens with a fence has closed it only if the number of
/// fences carrying that same marker is even. An odd count means one is
/// unpaired, and the unpaired one is the wrapper's -- which is exactly what an
/// emission cut off inside a code block looks like.
fn assemble(lines: &[Line]) -> Answer {
    let opening = match lines.first() {
        Some(Line::Fence { marker, .. }) => Some(marker.clone()),
        _ => None,
    };

    let Some(marker) = opening else {
        let fields = group(lines);
        let completion = completion_of(None, &fields, lines);
        return Answer {
            wrapper: None,
            fields,
            completion,
        };
    };

    let matching: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, line)| matches!(line, Line::Fence { marker: m, .. } if *m == marker))
        .map(|(index, _)| index)
        .collect();

    let open = lines[0].as_written();
    let (close_at, close) = if matching.len().is_multiple_of(2) {
        let last = *matching.last().expect("an even count is at least two");
        (last, Some(lines[last].as_written()))
    } else {
        (lines.len(), None)
    };

    let mut fields = group(&lines[1..close_at.min(lines.len())]);
    let fields_inside = fields.len();
    if close_at < lines.len() {
        fields.extend(group(&lines[close_at + 1..]));
    }
    let signal = if close.is_none() {
        Some(TruncationSignal::UnterminatedFence)
    } else {
        None
    };
    let completion = completion_of(signal, &fields, lines);
    Answer {
        wrapper: Some(Wrapper {
            open,
            close,
            fields_inside,
        }),
        fields,
        completion,
    }
}

/// Group a run of lines into fields.
///
/// A field ends where the next tag begins, and nowhere else. Not "at an
/// indented line", not "before a blank line": those heuristics are what lost
/// the 71, and there is nowhere here to express one.
fn group(lines: &[Line]) -> Vec<Field> {
    let mut fields = Vec::new();
    let mut loose: Vec<String> = Vec::new();
    let mut current: Option<(FieldKind, String, Vec<String>)> = None;

    for line in lines {
        if let Line::Tag {
            kind,
            as_written,
            head,
        } = line
        {
            flush_loose(&mut loose, &mut fields);
            flush_field(&mut current, &mut fields);
            let mut value = Vec::new();
            let trimmed = head.trim_start();
            if !trimmed.is_empty() {
                value.push(trimmed.to_owned());
            }
            current = Some((*kind, as_written.clone(), value));
        } else if let Some((_, _, value)) = current.as_mut() {
            value.push(line.as_written());
        } else {
            loose.push(line.as_written());
        }
    }
    flush_loose(&mut loose, &mut fields);
    flush_field(&mut current, &mut fields);
    fields
}

fn flush_loose(loose: &mut Vec<String>, fields: &mut Vec<Field>) {
    if loose.is_empty() {
        return;
    }
    let raw = std::mem::take(loose).join("\n");
    if raw.trim().is_empty() {
        return;
    }
    fields.push(Field {
        tag: None,
        raw: raw.trim_end().to_owned(),
        outcome: Outcome::Unparseable,
    });
}

fn flush_field(current: &mut Option<(FieldKind, String, Vec<String>)>, fields: &mut Vec<Field>) {
    let Some((kind, as_written, value)) = current.take() else {
        return;
    };
    let joined = value.join("\n");
    let text = joined.trim_end();
    let outcome = if text.is_empty() {
        Outcome::Unparseable
    } else {
        match decline::classify(text) {
            decline::Classification::Decline(declined) => Outcome::Decline(declined),
            decline::Classification::Content => Outcome::Value(text.to_owned()),
        }
    };
    fields.push(Field {
        tag: Some(Tag { kind, as_written }),
        raw: text.to_owned(),
        outcome,
    });
}

/// Which completion, given the wrapper's verdict and what was grouped.
fn completion_of(signal: Option<TruncationSignal>, fields: &[Field], lines: &[Line]) -> Completion {
    if let Some(signal) = signal {
        return Completion::Truncated(signal);
    }
    if lines.iter().all(|line| line.as_written().trim().is_empty()) {
        return Completion::Empty;
    }
    // A tag announced as the last thing in the answer, with nothing after it,
    // is what a token cap looks like when it lands between a field's tag and
    // its value. Reading it as an empty value would record a field the emitter
    // never filled in.
    match fields.last() {
        Some(Field {
            tag: Some(_),
            outcome: Outcome::Unparseable,
            ..
        }) => Completion::Truncated(TruncationSignal::TrailingTagWithNoValue),
        _ => Completion::Complete,
    }
}

/// A line without its terminator. Handles both endings, so an answer emitted
/// on a Windows host parses to the same value as one emitted anywhere else.
fn strip_eol(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// A tag with its decoration removed, for naming the kind: bullets, heading
/// marks, emphasis markers and the separator.
fn strip_tag_punctuation(as_written: &str) -> &str {
    as_written.trim_matches(|c: char| {
        matches!(
            c,
            '#' | '*'
                | '_'
                | '`'
                | '+'
                | ':'
                | '：'
                | '—'
                | '–'
                | '-'
                | '='
                | '»'
                | '>'
                | '.'
                | ')'
        ) || c.is_ascii_digit()
            || c.is_whitespace()
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

    /// Every non-whitespace character of `source`, in order.
    ///
    /// Whitespace is the one thing this format normalises: line terminators,
    /// the padding between a separator and its value, trailing blank lines.
    /// Everything else must survive.
    fn substance(text: &str) -> String {
        text.chars().filter(|c| !c.is_whitespace()).collect()
    }

    /// The property the whole format rests on, checked rather than promised.
    ///
    /// The first version of this format claimed in its own doc comment that
    /// nothing in the input was absent from the parse, and discarded both
    /// wrapper fence lines and everything after the closing one. A sweep of
    /// 22,220 generated answers found 5,428 that lost content. Nothing could
    /// have caught it: a fixture pins what the parser produced, never what it
    /// dropped.
    #[track_caller]
    fn accounts_for_everything(source: &str) {
        let answer = parse(source).expect("an interview answer");
        assert_eq!(
            substance(&answer.accounted_text()),
            substance(source),
            "content lost parsing {source:?}\n  kept: {:?}",
            answer.accounted_text()
        );
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
        assert_eq!(
            value("**DECISION:** keep it\n", FieldKind::Decision),
            "keep it"
        );
        assert_eq!(
            value("- **DECISION**: keep it\n", FieldKind::Decision),
            "keep it"
        );
        assert_eq!(
            value("1. DECISION: keep it\n", FieldKind::Decision),
            "keep it"
        );
    }

    // Defect two: a wrapper fence made the parser see zero fields. The fence
    // lines themselves are now kept rather than discarded.
    #[test]
    fn a_wrapper_fence_does_not_hide_the_fields_and_is_not_thrown_away() {
        let answer = parse("```json\nDECISION: keep it\n```\n").expect("an answer");
        assert_eq!(answer.fields.len(), 1);
        assert_eq!(answer.completion, Completion::Complete);
        let wrapper = answer.wrapper.as_ref().expect("a wrapper");
        assert_eq!(wrapper.open, "```json");
        assert_eq!(wrapper.close.as_deref(), Some("```"));
        accounts_for_everything("```json\nDECISION: keep it\n```\n");
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
        assert_eq!(
            value("DECISION: first\nsecond\n\nfourth\n", FieldKind::Decision),
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

    // Defect six, found by adversarial review: content on a fence line, and
    // content after the closing fence, were discarded with no signal.
    #[test]
    fn nothing_on_or_after_a_fence_line_is_discarded() {
        for source in [
            "```\nDECISION: keep it\n``` and then some words\n",
            "```json\nDECISION: keep it\n```\n",
            "```\n```\n",
            "```\nDECISION: keep it\nLEARNED: the fix\n```\nThat is the whole answer.\n",
            "```rust\nlet x = 1;\n```\nDECISION: keep it\n",
        ] {
            accounts_for_everything(source);
        }
    }

    // A hyphen inside a word is not a separator. Without this,
    // `Evidence-based debugging` was an EVIDENCE field reading
    // `based debugging`, and three continuation lines were reassigned to
    // fabricated fields.
    #[test]
    fn hyphenated_prose_does_not_fabricate_fields() {
        let source = "LEARNED: the resolver picks the newer binary\n\
                      Evidence-based debugging beat guesswork here.\n\
                      Decision-making was deferred to the next turn.\n\
                      Plan-of-record is unchanged.\n";
        let answer = parse(source).expect("an answer");
        assert_eq!(answer.fields.len(), 1, "got {:?}", answer.fields);
        assert_eq!(answer.field(FieldKind::Evidence), None);
        assert_eq!(
            value(source, FieldKind::Learned),
            "the resolver picks the newer binary\n\
             Evidence-based debugging beat guesswork here.\n\
             Decision-making was deferred to the next turn.\n\
             Plan-of-record is unchanged."
        );
    }

    // A tag name ends at a word boundary, and a heading with no separator must
    // have nothing else on its line. Without either, a prose heading was
    // promoted to a typed field with a value beginning mid-word.
    #[test]
    fn a_prose_heading_is_not_a_tag() {
        let answer = parse("## DECISIONS ABOUT THE PARSER\nkeep it\n").expect("an answer");
        assert_eq!(answer.field(FieldKind::Decision), None);
        let answer = parse("# Evidence of the leak was everywhere\nthe allocator never freed it\n")
            .expect("an answer");
        assert_eq!(answer.field(FieldKind::Evidence), None);
        assert_eq!(
            value("## DECISION\nkeep it\n", FieldKind::Decision),
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
        accounts_for_everything(source);
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
        let source = "Here is what I found.\nDECISION: keep it\n";
        let answer = parse(source).expect("an answer");
        assert!(matches!(
            answer.fields.first().map(|f| &f.outcome),
            Some(Outcome::Unparseable)
        ));
        assert_eq!(answer.fields.len(), 2);
        accounts_for_everything(source);
    }

    // Truncation, in both directions the review found wrong. A wrapper is
    // closed when the fences carrying its marker are even in number; an odd
    // count means one is unpaired, and the unpaired one is the wrapper's.
    #[test]
    fn truncation_is_decided_by_pairing_the_fences() {
        // Cut off before the wrapper closed.
        assert_eq!(
            parse("```\nDECISION: keep it\n").expect("a").completion,
            Completion::Truncated(TruncationSignal::UnterminatedFence)
        );
        // Cut off after an inner code block: three fences, so one is unpaired.
        assert_eq!(
            parse("```\nDECISION: ship\nEVIDENCE:\n```text\nerror\n```\n")
                .expect("a")
                .completion,
            Completion::Truncated(TruncationSignal::UnterminatedFence)
        );
        // Complete, with a remark after the closing fence.
        assert_eq!(
            parse("```\nDECISION: keep it\n```\nThat is the whole answer.\n")
                .expect("a")
                .completion,
            Completion::Complete
        );
        // A block opened with tildes is not closed by one opened with
        // backticks.
        assert_eq!(
            parse("~~~\nDECISION: keep it\n```\n")
                .expect("a")
                .completion,
            Completion::Truncated(TruncationSignal::UnterminatedFence)
        );
    }

    #[test]
    fn a_trailing_tag_with_no_value_reads_as_truncation() {
        assert_eq!(
            parse("DECISION: keep it\nLEARNED:").expect("a").completion,
            Completion::Truncated(TruncationSignal::TrailingTagWithNoValue)
        );
    }

    // An emitter that produced nothing has not finished an answer.
    #[test]
    fn an_empty_emission_is_empty_and_not_complete() {
        assert_eq!(parse("").expect("a").completion, Completion::Empty);
        assert_eq!(parse("\n\n  \n").expect("a").completion, Completion::Empty);
    }

    #[test]
    fn crlf_parses_to_the_same_value_as_lf() {
        assert_eq!(
            value("DECISION: a\r\nb\r\n", FieldKind::Decision),
            value("DECISION: a\nb\n", FieldKind::Decision)
        );
    }

    // A terminal escape in verbatim tool output is ordinary, and rejecting the
    // whole answer for one threw away every field that parsed.
    #[test]
    fn a_terminal_escape_does_not_cost_the_other_fields() {
        let source = "DECISION: keep it\nEVIDENCE: test result: \u{1b}[31mFAILED\u{1b}[0m\n";
        assert_eq!(value(source, FieldKind::Decision), "keep it");
        assert!(value(source, FieldKind::Evidence).contains('\u{1b}'));
        accounts_for_everything(source);
    }

    // A NUL is where a byte stream ends, not something an emitter writes.
    #[test]
    fn a_nul_byte_is_the_one_content_rejection() {
        assert!(parse("DECISION: a\u{0}b\n").is_err());
    }

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

    // The accounting property over the committed corpus, and over every
    // combination of a small pool of lines chosen to include the shapes that
    // broke it. This is the sweep the review used, run as a test.
    /// Lines chosen to include every shape that broke the accounting
    /// property, for the combination sweep below.
    const POOL: &[&str] = &[
        "```",
        "```rust",
        "~~~",
        "DECISION: keep it",
        "Evidence-based debugging",
        "## LEARNED",
        "- **PLAN**: ship",
        "The bug: the parser dropped lines",
        "",
        "  indented prose",
    ];

    #[test]
    fn every_byte_of_an_answer_is_accounted_for() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("formats/interview/fixtures/valid");
        let mut cases = 0_usize;
        for entry in std::fs::read_dir(&dir).expect("the corpus is readable") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|ext| ext != "txt") {
                continue;
            }
            cases += 1;
            let source = std::fs::read_to_string(&path).expect("a valid case is UTF-8");
            let answer = parse(&source).expect("a valid case parses");
            assert_eq!(
                substance(&answer.accounted_text()),
                substance(&source),
                "content lost parsing {}",
                path.display()
            );
        }
        assert!(cases > 0, "{} holds no cases", dir.display());

        let mut checked = 0_usize;
        for a in POOL {
            for b in POOL {
                for c in POOL {
                    for d in POOL {
                        let source = format!("{a}\n{b}\n{c}\n{d}\n");
                        let answer = parse(&source).expect("a generated answer parses");
                        assert_eq!(
                            substance(&answer.accounted_text()),
                            substance(&source),
                            "content lost parsing {source:?}"
                        );
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, POOL.len().pow(4));
    }

    // A field kind with no fixture has an untested parse path, and in the
    // prior system that is precisely where a silent drop lived.
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
