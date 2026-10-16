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
use super::record::json::Value;

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
    /// what it emits must be checkable against that source. The dogma writes
    /// it `SURFACE`; the archived corpus writes it `API_SURFACE`.
    ApiSurface,
    /// A rule the rest of the project must honour.
    Constraint,
    /// What the turn has not seen but expects to need.
    Dark,
    /// What the turn decided.
    Decision,
    /// Excerpts from tool output. Verbatim-class. The dogma writes it
    /// `EXCERPT`.
    Evidence,
    /// A concrete, span-grounded fact extracted from material the turn read.
    ///
    /// Not [`Self::Learned`]: a fact is quoted or closely matched from a
    /// source and is checkable against it, where what a turn learned is the
    /// turn's own claim. Collapsing the two would make an extraction and an
    /// assertion count as one thing.
    Fact,
    /// An API question left pending rather than settled.
    Followup,
    /// Something that behaved differently than expected, and what it implies.
    Gotcha,
    /// The new quote a supersede question is asked about.
    Latest,
    /// What the turn learned.
    Learned,
    /// The structure as the turn now holds it.
    Map,
    /// A note about the working notes themselves, from an audit.
    Note,
    /// A question left open, unverified, or assumed but not checked.
    Open,
    /// What the turn intends next. The dogma writes it `NEXT`.
    Plan,
    /// What a piece of material changes about the plan. The dogma writes it
    /// `PLAN_IMPACT` after a document and `PLAN_DELTA` after doctrine.
    ///
    /// Separate from [`Self::Plan`]: one is the next actions, the other is
    /// what moved. A turn can report either without the other.
    PlanImpact,
    /// Something noticed that is not needed now but might matter later.
    Pointer,
    /// A why that is not visible in the artifacts.
    Rationale,
    /// The earlier entry a supersede question is asked about.
    Recorded,
    /// What the turn could not get past.
    Stuck,
    /// An earlier record this turn changes or replaces.
    Supersede,
    /// The ruling on whether the latest information replaces the recorded one.
    ///
    /// The FIELD is here; its VALUE has a closed vocabulary of its own
    /// (`REPLACES | RESOLVES | CONTRADICTS | UNRELATED`) that nothing yet
    /// parses. That is a second vocabulary and it is named in neither the
    /// ruling nor this type.
    Verdict,
}

/// The verdict an audit returns on one working note.
///
/// Its own type rather than a [`FieldKind`]: an audit answer rules on a note
/// somebody else wrote, where every field kind above is something the turn is
/// reporting about itself. Both are closed vocabularies and they are not the
/// same vocabulary, which a single enum would have hidden.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AuditVerdict {
    /// A note that should be there and is not.
    Add,
    /// The note stands.
    Keep,
    /// The note stopped mattering, or duplicates another.
    Remove,
    /// The note is right in outline and wrong in detail.
    Update,
}

impl AuditVerdict {
    /// Every verdict, in declaration order.
    pub const ALL: &'static [Self] = &[Self::Add, Self::Keep, Self::Remove, Self::Update];

    /// The spelling an audit answer uses, lower case.
    #[must_use]
    pub fn canonical_tag(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Keep => "keep",
            Self::Remove => "remove",
            Self::Update => "update",
        }
    }
}

/// The surface-tag table, as data.
///
/// Compiled in rather than read at run time, so a binary cannot be shipped
/// without it, and read here rather than transcribed into a `match`: three
/// things have to agree about this vocabulary -- this file, the grammar's
/// `tag_word`, and the dogma's templates -- and the way they stay in step is
/// that two tests read all three and compare them.
const TAG_TABLE: &str = include_str!("../../formats/interview/tags.tsv");

/// Where a tag sits, which is the table's second column.
///
/// A type rather than the column's text. Seven places asked this question and
/// every one of them asked it by comparing against a string literal -- one of
/// them on the path that turns a written tag into a `FieldKind`, where a
/// mistyped position would simply find nothing and report the tag unknown.
/// The compiler cannot check a string; it can check this, and a fourth
/// position added to the table now fails to compile everywhere that must care.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum TagPosition {
    /// A tag that opens a line of its own.
    Line,
    /// A second field on another tag's line, so it has no kind of its own.
    Inline,
    /// One of an audit's four verdicts.
    Audit,
}

impl TagPosition {
    /// Every position, in the order the table uses.
    pub const ALL: &'static [Self] = &[Self::Line, Self::Inline, Self::Audit];

    /// The spelling the table uses.
    #[must_use]
    pub fn canonical_tag(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Inline => "inline",
            Self::Audit => "audit",
        }
    }

    /// The position a column of the table names, if it names one.
    ///
    /// Iterating `ALL` rather than matching the text, so that a variant added
    /// to the enum is covered here without anybody remembering to come back.
    #[must_use]
    pub fn from_tag(text: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|position| position.canonical_tag() == text)
    }
}

/// Every row of the table: `(tag, position, kind)`.
///
/// A line that is not three tab-separated fields is skipped, and so is one
/// whose position is not a `TagPosition`. Neither is silent:
/// `every_table_row_is_three_fields` compares this count against the number of
/// three-field lines, so a row this reader drops is a row that fails a test.
fn tag_rows() -> impl Iterator<Item = (&'static str, TagPosition, &'static str)> {
    TAG_TABLE
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split('\t');
            match (fields.next(), fields.next(), fields.next(), fields.next()) {
                (Some(tag), Some(position), Some(kind), None) => {
                    TagPosition::from_tag(position).map(|position| (tag, position, kind))
                }
                _ => None,
            }
        })
}

/// A tag as written, reduced to the one spelling the table is keyed by.
fn normalise(written: &str) -> String {
    written
        .chars()
        .map(|c| {
            if c == '-' || c.is_whitespace() {
                '_'
            } else {
                c
            }
        })
        .flat_map(char::to_lowercase)
        .collect()
}

impl FieldKind {
    /// Every kind, in declaration order.
    ///
    /// Used by the coverage test: a kind absent from this list is a kind no
    /// fixture is required to exercise.
    pub const ALL: &'static [Self] = &[
        Self::ApiSurface,
        Self::Constraint,
        Self::Dark,
        Self::Decision,
        Self::Evidence,
        Self::Fact,
        Self::Followup,
        Self::Gotcha,
        Self::Latest,
        Self::Learned,
        Self::Map,
        Self::Note,
        Self::Open,
        Self::Plan,
        Self::PlanImpact,
        Self::Pointer,
        Self::Rationale,
        Self::Recorded,
        Self::Stuck,
        Self::Supersede,
        Self::Verdict,
    ];

    /// The canonical tag for this kind, lower case and underscored.
    #[must_use]
    pub fn canonical_tag(self) -> &'static str {
        match self {
            Self::ApiSurface => "api_surface",
            Self::Constraint => "constraint",
            Self::Dark => "dark",
            Self::Decision => "decision",
            Self::Evidence => "evidence",
            Self::Fact => "fact",
            Self::Followup => "followup",
            Self::Gotcha => "gotcha",
            Self::Latest => "latest",
            Self::Learned => "learned",
            Self::Map => "map",
            Self::Note => "note",
            Self::Open => "open",
            Self::Plan => "plan",
            Self::PlanImpact => "plan_impact",
            Self::Pointer => "pointer",
            Self::Rationale => "rationale",
            Self::Recorded => "recorded",
            Self::Stuck => "stuck",
            Self::Supersede => "supersede",
            Self::Verdict => "verdict",
        }
    }

    /// The kind a tag names, whatever case and separator it was written in.
    ///
    /// The grammar has already established that the text is one of the known
    /// names; this only has to say which. A `None` here means the grammar and
    /// this function have drifted apart, which is why the caller treats it as
    /// a shape error rather than as an unknown field.
    fn from_tag(written: &str) -> Option<Self> {
        let normalised = normalise(written);
        // Through the table, not through a match on the spelling: a tag has
        // several registers and a kind has several tags, and the place that
        // knows which is which is the data file the grammar is checked
        // against.
        let named = tag_rows().find(|(tag, position, _)| {
            *position == TagPosition::Line && normalise(tag) == normalised
        })?;
        Self::ALL
            .iter()
            .copied()
            .find(|kind| kind.canonical_tag() == named.2)
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

/// This answer, as the record's value space.
///
/// The one projection of this format, called by the conformance harness and
/// by the `diet` CLI alike.
///
/// # Errors
///
/// Returns the reason the text is not an interview answer.
pub fn project(source: &str) -> Result<Value, String> {
    let answer = parse(source).map_err(|err| err.to_string())?;
    let fields: Vec<Value> = answer
        .fields
        .iter()
        .map(|field| {
            let mut members = std::collections::BTreeMap::from([
                ("raw".to_owned(), Value::String(field.raw.clone())),
                ("outcome".to_owned(), outcome_value(&field.outcome)),
            ]);
            if let Some(tag) = &field.tag {
                members.insert(
                    "tag".to_owned(),
                    Value::Object(std::collections::BTreeMap::from([
                        (
                            "kind".to_owned(),
                            Value::String(tag.kind.canonical_tag().to_owned()),
                        ),
                        (
                            "as_written".to_owned(),
                            Value::String(tag.as_written.clone()),
                        ),
                    ])),
                );
            }
            Value::Object(members)
        })
        .collect();
    let completion = match answer.completion {
        Completion::Complete => Value::String("complete".to_owned()),
        Completion::Empty => Value::String("empty".to_owned()),
        Completion::Truncated(signal) => Value::Object(std::collections::BTreeMap::from([(
            "truncated".to_owned(),
            Value::String(signal.name().to_owned()),
        )])),
    };
    let mut members = std::collections::BTreeMap::from([
        ("completion".to_owned(), completion),
        ("fields".to_owned(), Value::Array(fields)),
    ]);
    if let Some(wrapper) = &answer.wrapper {
        let mut wrapped = std::collections::BTreeMap::from([
            ("open".to_owned(), Value::String(wrapper.open.clone())),
            (
                "fields_inside".to_owned(),
                Value::Integer(i64::try_from(wrapper.fields_inside).unwrap_or(i64::MAX)),
            ),
        ]);
        if let Some(close) = &wrapper.close {
            wrapped.insert("close".to_owned(), Value::String(close.clone()));
        }
        members.insert("wrapper".to_owned(), Value::Object(wrapped));
    }
    Ok(Value::Object(members))
}

fn outcome_value(outcome: &Outcome) -> Value {
    match outcome {
        Outcome::Value(text) => Value::Object(std::collections::BTreeMap::from([(
            "value".to_owned(),
            Value::String(text.clone()),
        )])),
        // The decline format's own projection, not a second copy of it.
        Outcome::Decline(declined) => super::decline::projected(declined),
        Outcome::Unparseable => Value::String("unparseable".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AuditVerdict, Completion, FieldKind, Outcome, TAG_TABLE, TagPosition, TruncationSignal,
        parse, tag_rows,
    };
    use std::collections::BTreeSet;

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

    // ---------------------------------------------------------------------
    // the vocabulary's three readers, held against each other
    // ---------------------------------------------------------------------
    //
    // The tag vocabulary is written down three times -- in tags.tsv, in the
    // grammar's `tag_word`, and in the dogma's templates -- and it has to be,
    // because a pest grammar cannot read a TSV and a template is prose a model
    // is shown. Three copies is the two-readers hazard with a third reader
    // added, so the copies are not trusted to agree: these tests read all
    // three and compare them, and a tag added to any one of them turns the
    // other two red until it is added there too.

    /// The reader skips a line it cannot split into three, which would make a
    /// mistyped row vanish instead of failing. Nothing else would notice.
    #[test]
    fn every_table_row_is_three_fields() {
        let declared = TAG_TABLE
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .count();
        assert_eq!(
            tag_rows().count(),
            declared,
            "a row of {} is not three tab-separated fields, or names a position \
             that is not one, and the reader dropped it silently",
            "tags.tsv"
        );
        assert!(
            declared > 0,
            "the table is empty, so every check over it passes"
        );
    }

    /// Every kind named in the table is a real kind.
    #[test]
    fn every_table_kind_is_a_field_kind_or_an_audit_verdict() {
        for (tag, position, kind) in tag_rows() {
            match position {
                TagPosition::Line => assert!(
                    FieldKind::ALL.iter().any(|k| k.canonical_tag() == kind),
                    "`{tag}` names the kind `{kind}`, which is not a FieldKind"
                ),
                TagPosition::Audit => assert!(
                    AuditVerdict::ALL.iter().any(|v| v.canonical_tag() == kind),
                    "`{tag}` names the verdict `{kind}`, which is not an AuditVerdict"
                ),
                TagPosition::Inline => assert_eq!(
                    kind, "-",
                    "`{tag}` is a second field on another tag's line, so it has \
                     no kind of its own"
                ),
            }
        }
        for verdict in AuditVerdict::ALL {
            assert!(
                tag_rows().any(|(_, position, kind)| position == TagPosition::Audit
                    && kind == verdict.canonical_tag()),
                "the audit verdict `{}` is in the enum and not in the table",
                verdict.canonical_tag()
            );
        }
    }

    /// The table's `line` rows and the grammar's `tag_word` are one list
    /// written twice. A spelling in one and not the other is a tag the parser
    /// admits and cannot name, or a tag named and never admitted.
    #[test]
    fn the_table_and_the_grammar_admit_the_same_tags() {
        const GRAMMAR: &str = include_str!("../../formats/interview/grammar.pest");
        let body = GRAMMAR
            .split_once("tag_word = _{")
            .expect("the grammar defines tag_word")
            .1
            .split_once("\n}")
            .expect("tag_word is closed")
            .0;
        // EVERY ALTERNATIVE IS READ, and one this cannot read is a FAILURE
        // rather than a skip. `filter_map` dropped anything not spelled
        // `^"..."` on the floor: append `| "IMPACT"` -- a case-sensitive
        // literal, which is legal pest -- and the grammar admits a tag the
        // table does not carry while all 582 tests stay green. A reader that
        // silently ignores what it does not understand is not comparing two
        // lists; it is comparing one list against the part of the other it
        // happened to parse.
        let mut unreadable: Vec<&str> = Vec::new();
        let mut admitted: BTreeSet<String> = BTreeSet::new();
        for alt in body.split('|') {
            let alt = alt.trim();
            if alt.is_empty() {
                continue;
            }
            match alt
                .strip_prefix("^\"")
                .and_then(|rest| rest.split_once('"'))
            {
                // Nothing may trail the closing quote: `^"plan" ~ x` is a
                // sequence, not a bare alternative, and reading only its head
                // would understate what the grammar admits.
                Some((word, rest)) if rest.trim().is_empty() => {
                    admitted.insert(word.to_owned());
                }
                _ => unreadable.push(alt),
            }
        }
        assert!(
            unreadable.is_empty(),
            "tag_word holds {} alternative(s) this test cannot read: {unreadable:?}. \
             Every alternative must be a bare `^\"word\"`; anything else is a \
             spelling the grammar admits and this comparison would not see",
            unreadable.len()
        );
        let tabled: BTreeSet<String> = tag_rows()
            .filter(|(_, position, _)| *position == TagPosition::Line)
            .map(|(tag, _, _)| tag.to_lowercase())
            .collect();
        assert!(
            !admitted.is_empty(),
            "no alternative was read out of tag_word"
        );
        assert_eq!(
            admitted, tabled,
            "the grammar's tag_word and the table's `line` rows disagree"
        );
    }

    /// The drift guard the batch asked for: a tag written in a dogma template
    /// and absent from this vocabulary is a field the interview parser will
    /// read as prose, silently, on every answer that carries it.
    ///
    /// Every capitalised run before a colon counts, down to a single letter --
    /// `Q:` and `A:` are real sub-fields of an audit's UPDATE answer, and a
    /// rule that only saw three-letter tags would have let them through while
    /// reading exactly like a rule that found nothing.
    #[test]
    fn no_dogma_tag_is_missing_from_the_table() {
        let known: BTreeSet<String> = tag_rows().map(|(tag, _, _)| tag.to_owned()).collect();
        let mut missing: Vec<String> = Vec::new();
        for template in crate::dogma::Template::ALL {
            for tag in capitalised_tags(template.text()) {
                if !known.contains(&tag) {
                    missing.push(format!("{}: {tag}", template.name()));
                }
            }
        }
        assert!(
            missing.is_empty(),
            "dogma tag(s) absent from diet/formats/interview/tags.tsv: {missing:?}"
        );
        assert!(
            !crate::dogma::Template::ALL.is_empty(),
            "no templates were scanned, so this found nothing by looking at nothing"
        );
    }

    /// The separator alphabets, READ OUT OF THE GRAMMAR rather than retyped.
    ///
    /// This is the whole point of the fix below. The lint knew about `:` and
    /// nothing else, while the grammar has admitted a second register --
    /// `dash = { "—" | "–" | "--" | "-" | "=" | "»" | ">" }` -- the entire
    /// time. A tag written `IMPACT — what this changes` was invisible to the
    /// lint, read as prose by the parser, and `tags.tsv`'s header said "add a
    /// tag to a template and two tests go red until it is named here": false
    /// for every tag not followed by a colon.
    ///
    /// Harvested, so a THIRD register added to the grammar cannot leave this
    /// behind the way the second did.
    fn separators() -> (Vec<String>, Vec<String>) {
        const GRAMMAR: &str = include_str!("../../formats/interview/grammar.pest");
        fn alternatives(rule: &str) -> Vec<String> {
            let body = GRAMMAR
                .split_once(&format!("\n{rule} = {{"))
                .unwrap_or_else(|| panic!("the grammar defines {rule}"))
                .1
                .split_once('}')
                .expect("the rule is closed")
                .0;
            let found: Vec<String> = body
                .split('|')
                .map(|alt| {
                    let alt = alt.trim();
                    alt.strip_prefix('"')
                        .and_then(|rest| rest.strip_suffix('"'))
                        .unwrap_or_else(|| {
                            panic!("{rule} holds an alternative this cannot read: {alt:?}")
                        })
                        .to_owned()
                })
                .collect();
            assert!(!found.is_empty(), "{rule} yielded no alternatives");
            found
        }
        (alternatives("colon"), alternatives("dash"))
    }

    /// Every run of `[A-Z_]` a dogma template writes in a tag's SHAPE.
    ///
    /// ANYWHERE IN THE LINE, not only where a tag may open, and that is the
    /// table's design rather than an oversight. `tags.tsv`'s header: inline
    /// rows are listed "so the drift lint can tell *a tag nobody declared*
    /// from *a tag deliberately not a tag*". A word mid-line that is shaped
    /// like a tag is exactly what this must surface; the six `inline` rows
    /// are the answer to it, not a reason to stop looking.
    ///
    /// I anchored this to a line opening once, reasoning from the grammar's
    /// "A TAG OPENS A LINE" — true of the PARSER and beside the point here.
    /// The seeded case `a dogma tag in no vocabulary` went green: it removes
    /// the `IMPLICATION` inline row, and `IMPLICATION` appears mid-line by
    /// definition. The gate caught it; the reasoning was wrong.
    ///
    /// Three shapes count, each because the grammar admits it:
    ///
    ///   `TAG:`         `hspace* ~ colon`, a colon may hug the tag
    ///   `TAG —`        `hspace+ ~ dash ~ &(hspace | nl | EOI)`, a dash may
    ///                  not, because `Evidence-based` is a word
    ///   `## TAG`       a heading tag needs no separator at all -- the one
    ///                  shape that IS positional, because a heading marker
    ///                  is only a heading marker at the head of a line
    ///
    /// The dash register is the fix this function exists for: the grammar has
    /// admitted `—`, `–`, `--`, `-`, `=`, `»` and `>` the entire time and
    /// this saw none of them, so `IMPACT — what this changes` was invisible
    /// while tags.tsv's header claimed "add a tag to a template and two tests
    /// go red until it is named here".
    fn capitalised_tags(text: &str) -> BTreeSet<String> {
        let (colons, dashes) = separators();
        let chars: Vec<char> = text.chars().collect();
        let hspace = |c: char| c == ' ' || c == '\t';
        let mut found = BTreeSet::new();

        // The two separator registers, at any position.
        for end in 0..=chars.len() {
            let mut start = end;
            while start > 0 && (chars[start - 1].is_ascii_uppercase() || chars[start - 1] == '_') {
                start -= 1;
            }
            if start == end {
                continue;
            }
            let run: String = chars[start..end].iter().collect();
            if !run.chars().any(|r| r.is_ascii_uppercase()) {
                continue;
            }
            // Maximal on the left: `xABC:` is not a tag, it is the tail of a
            // word. `**` and `_` are emphasis and DO open one.
            let opens =
                start == 0 || !(chars[start - 1].is_alphanumeric() || chars[start - 1] == '_');
            if !opens {
                continue;
            }
            let mut tail: String = chars[end..].iter().collect();
            // `**TAG**: value` -- the emphasis may close before the separator.
            for mark in ["***", "**", "__", "*", "_", "`"] {
                if let Some(after) = tail.strip_prefix(mark) {
                    tail = after.to_owned();
                    break;
                }
            }
            let after_spaces = tail.trim_start_matches(hspace);
            let hugged = colons.iter().any(|c| after_spaces.starts_with(c.as_str()));
            let spaced = tail.starts_with(hspace)
                && dashes.iter().any(|d| {
                    after_spaces
                        .strip_prefix(d.as_str())
                        .is_some_and(|rest| rest.is_empty() || rest.starts_with(hspace))
                });
            if hugged || spaced {
                found.insert(run);
            }
        }

        // The heading shape, which has no separator to find and so has to be
        // looked for where a heading marker means anything: the line's head.
        for line in text.lines() {
            let mut rest = line.trim_start_matches(hspace);
            if !rest.starts_with('#') {
                continue;
            }
            rest = rest.trim_start_matches('#').trim_start_matches(hspace);
            let run: String = rest
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                .collect();
            // Nothing else on the line, or `# Evidence of the leak` is an
            // EVIDENCE field -- the constraint the grammar states too.
            if !run.is_empty()
                && run.chars().any(|r| r.is_ascii_uppercase())
                && rest[run.len()..].trim().is_empty()
            {
                found.insert(run);
            }
        }
        found
    }

    /// The corpus carries the dogma's tags, which is the acceptance this
    /// batch names -- and which the coverage test above would satisfy with a
    /// fixture per kind whatever the tags on it. This pins the SPELLINGS.
    #[test]
    fn the_corpus_carries_the_dogma_spellings() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("formats/interview/fixtures/valid");
        let mut written: BTreeSet<String> = BTreeSet::new();
        for entry in std::fs::read_dir(&dir).expect("the corpus is readable") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|ext| ext != "txt") {
                continue;
            }
            let source = std::fs::read_to_string(&path).expect("a valid case is UTF-8");
            let answer = parse(&source).expect("a valid case parses");
            for field in &answer.fields {
                if let Some(tag) = field.tag.as_ref() {
                    let spelling = tag
                        .as_written
                        .trim_end_matches([':', ' ', '\t'])
                        .trim_matches(|c: char| !c.is_alphanumeric() && c != '_' && c != ' ')
                        .to_owned();
                    written.insert(spelling.to_uppercase());
                }
            }
        }
        let dogma_line_tags: Vec<&str> = tag_rows()
            .filter(|(_, position, _)| *position == TagPosition::Line)
            .map(|(tag, _, _)| tag)
            .collect();
        let uncovered: Vec<&&str> = dogma_line_tags
            .iter()
            .filter(|tag| !written.contains(&tag.to_uppercase()))
            .collect();
        assert!(
            uncovered.is_empty(),
            "tag spelling(s) the corpus never exercises: {uncovered:?}"
        );
    }
}
