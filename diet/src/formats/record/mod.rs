//! The `record` format, v0 -- the session record.
//!
//! The grammar at `diet/formats/record/grammar.pest` is normative for the
//! syntax; this module is normative for the schema, and is the one authorized
//! implementation of both.
//!
//! Every session produces a record: the event stream, in the order the events
//! happened. Four classes of provenance failure were each paid for one at a
//! time in prior work, and the schema carries all four from its first commit
//! because retrofitting provenance was the most expensive recurring tax there
//! was.
//!
//! 1. **Regime tags.** Results without them do not transfer, and results with
//!    partial regimes were repeatedly compared as though they were
//!    comparable. The regime is carried once, by the required `start` event,
//!    and a record whose first event is not a complete `start` does not parse.
//!    Stating it once and structurally beats restating it per line: a tag
//!    repeated on every row is a tag that can disagree with itself.
//! 2. **Retry lineage.** A log that echoed the arm's pin on a retried call
//!    could not say which of two requests produced the answer. Here a retry is
//!    a new `request` naming its predecessor, and every `response` names the
//!    request it answers. Both links are checked against events already seen.
//! 3. **Recompute-sufficiency.** A run that stored an answer's character count
//!    but not the answer could only ever produce a bound, never a number. A
//!    `claim` must name at least one artifact that recomputing it consumes,
//!    each with a digest, or it is not a claim.
//! 4. **Claim atomicity.** One row is one (hypothesis, result) pair. A
//!    correction is a new `claim` linked by `supersedes`, never an edit that
//!    accretes retraction and replacement into one cell. A compound row is
//!    un-gateable by construction, so the type offers no way to write one.

pub mod json;

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;
use pest_derive::Parser;

use json::{Value, ValueError};

#[derive(Parser)]
#[grammar = "../formats/record/grammar.pest"]
struct RecordParser;

// ---------------------------------------------------------------------------
// the regime
// ---------------------------------------------------------------------------

/// Which substrate produced a session.
///
/// Every field is required. An optional substrate field is a substrate field
/// that will be absent exactly when it matters -- the run whose result
/// surprises someone is the run nobody thought to tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substrate {
    /// The short name the substrate is known by. This is the string a report's
    /// front-matter carries as `regime.substrate`.
    pub name: String,
    /// The model, as the serving stack names it.
    pub model: String,
    /// The quantization, as the serving stack names it.
    pub quantization: String,
    /// Sampler settings, exactly as they were set.
    pub sampler: BTreeMap<String, Value>,
    /// Whether reasoning was on, and whether it came back.
    pub reasoning: Reasoning,
    /// A fingerprint for the hardware the run was served from.
    pub hardware: String,
}

/// The reasoning state a run was served under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reasoning {
    /// Not requested.
    Off,
    /// Requested, and returned.
    On,
    /// Requested, and NOT returned -- the substrate was configured to emit no
    /// reasoning while reasoning was enabled. This is its own state rather
    /// than a flavour of `On` because the combination is a known footgun: a
    /// mechanism that depends on seeing reasoning is silently defeated, and a
    /// record that cannot express the difference cannot explain the result.
    Suppressed,
}

impl Reasoning {
    /// Every state, for the round-trip test.
    pub const ALL: &'static [Self] = &[Self::Off, Self::On, Self::Suppressed];

    /// The name this state is written under.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::On => "on",
            Self::Suppressed => "suppressed",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|state| state.tag() == tag)
    }
}

/// The fixed combination of variables a session ran under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Regime {
    /// Which arm of the experiment.
    pub arm: String,
    /// What served it.
    pub substrate: Substrate,
    /// Which version of the dogma was in force.
    pub dogma_version: u32,
}

impl Regime {
    /// The keys a report's front-matter `[regime]` table must mirror.
    ///
    /// Named here rather than in the report linter so that the schema is the
    /// definition and the report is the mirror, which is the direction the
    /// two are supposed to run in.
    pub const TAGS: &'static [&'static str] = &["arm", "substrate", "dogma_version"];
}

// ---------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------

/// Which kind of event a row is.
///
/// Separated from [`Event`] so that the coverage test can enumerate the kinds
/// without constructing one of each. A kind here with no fixture is a kind
/// with an untested serialization path, and in prior work that is exactly
/// where a silent drop lived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// The session begins. Carries the regime.
    Start,
    /// A turn happened.
    Turn,
    /// A request went to the substrate.
    Request,
    /// A response came back.
    Response,
    /// An interview fork was opened.
    Fork,
    /// A capture wrote to the working object.
    Capture,
    /// The working object was rendered into a prompt.
    Seam,
    /// A tool was called.
    ToolCall,
    /// One hypothesis, one result, and what recomputing it consumes.
    Claim,
    /// The session's totals.
    Summary,
}

impl Kind {
    /// Every kind. The coverage test iterates this.
    pub const ALL: &'static [Self] = &[
        Self::Start,
        Self::Turn,
        Self::Request,
        Self::Response,
        Self::Fork,
        Self::Capture,
        Self::Seam,
        Self::ToolCall,
        Self::Claim,
        Self::Summary,
    ];

    /// The value of the row's `record` key for this kind.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Turn => "turn",
            Self::Request => "request",
            Self::Response => "response",
            Self::Fork => "fork",
            Self::Capture => "capture",
            Self::Seam => "seam",
            Self::ToolCall => "tool_call",
            Self::Claim => "claim",
            Self::Summary => "summary",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.tag() == tag)
    }
}

/// An artifact a claim consumes to be recomputed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Where it lives, relative to the results directory.
    pub path: String,
    /// Its digest, 64 lowercase hex characters.
    pub sha256: String,
}

/// What a claim concluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The evidence supports the hypothesis.
    Supported,
    /// The evidence refutes it.
    Refuted,
    /// The evidence does neither, which is a result and not a failure.
    Inconclusive,
}

impl Verdict {
    /// Every verdict.
    pub const ALL: &'static [Self] = &[Self::Supported, Self::Refuted, Self::Inconclusive];

    /// The name this verdict is written under.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::Refuted => "refuted",
            Self::Inconclusive => "inconclusive",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|verdict| verdict.tag() == tag)
    }
}

/// One row of a session record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The session begins.
    Start {
        /// The regime every later event in this record ran under.
        regime: Box<Regime>,
    },
    /// A turn happened.
    Turn {
        /// Its position in the session, from 1.
        index: u32,
        /// How many tokens the prompt carried.
        prefill_tokens: u64,
    },
    /// A request went to the substrate.
    Request {
        /// This request's identifier, unique in the record.
        id: String,
        /// Which lane sent it.
        lane: String,
        /// The request this one retries, if it is a retry. A retry is a new
        /// request that names its predecessor; it is not an annotation on the
        /// old one, because the old one already happened.
        retry_of: Option<String>,
    },
    /// A response came back.
    Response {
        /// This response's identifier.
        id: String,
        /// The request that produced it. Required: a response that cannot name
        /// its request is the failure this field exists for.
        to_request: String,
        /// How many tokens came back.
        output_tokens: u64,
    },
    /// An interview fork was opened.
    Fork {
        /// This fork's identifier.
        id: String,
        /// Which lane it belongs to.
        lane: String,
        /// The turn it forked from.
        of_turn: u32,
    },
    /// A capture wrote to the working object.
    Capture {
        /// This capture's identifier.
        id: String,
        /// The fork whose output it captured.
        from_fork: String,
        /// How many entries it wrote.
        entries: u32,
    },
    /// The working object was rendered into a prompt.
    Seam {
        /// This seam's identifier.
        id: String,
        /// The turn it was rendered for.
        at_turn: u32,
        /// How large the render was.
        rendered_bytes: u64,
    },
    /// A tool was called.
    ToolCall {
        /// This call's identifier.
        id: String,
        /// The turn that called it.
        at_turn: u32,
        /// Which tool.
        tool: String,
    },
    /// One hypothesis, one result.
    Claim {
        /// This claim's identifier.
        id: String,
        /// The claim, stated so that it could be wrong.
        hypothesis: String,
        /// What the evidence said.
        result: Verdict,
        /// What recomputing this claim consumes. Never empty.
        consumes: Vec<Artifact>,
        /// The claim this one supersedes, if it is a correction.
        supersedes: Option<String>,
    },
    /// The session's totals.
    Summary {
        /// How many turns.
        turns: u32,
        /// Prefill tokens across the session.
        prefill_tokens_total: u64,
        /// The digest of the product the session produced.
        product_sha256: String,
    },
}

impl Event {
    /// Which kind this event is.
    #[must_use]
    pub fn kind(&self) -> Kind {
        match self {
            Self::Start { .. } => Kind::Start,
            Self::Turn { .. } => Kind::Turn,
            Self::Request { .. } => Kind::Request,
            Self::Response { .. } => Kind::Response,
            Self::Fork { .. } => Kind::Fork,
            Self::Capture { .. } => Kind::Capture,
            Self::Seam { .. } => Kind::Seam,
            Self::ToolCall { .. } => Kind::ToolCall,
            Self::Claim { .. } => Kind::Claim,
            Self::Summary { .. } => Kind::Summary,
        }
    }

    /// This event's identifier, for the kinds that carry one.
    #[must_use]
    pub fn id(&self) -> Option<&str> {
        match self {
            Self::Request { id, .. }
            | Self::Response { id, .. }
            | Self::Fork { id, .. }
            | Self::Capture { id, .. }
            | Self::Seam { id, .. }
            | Self::ToolCall { id, .. }
            | Self::Claim { id, .. } => Some(id),
            Self::Start { .. } | Self::Turn { .. } | Self::Summary { .. } => None,
        }
    }
}

/// A parsed session record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The regime, from the required `start` event. Not an `Option`: a record
    /// without one does not parse, so every `Record` that exists is tagged.
    pub regime: Regime,
    /// Every event, in the order it appeared, `start` included.
    pub events: Vec<Event>,
}

impl Record {
    /// Every event kind this record carries.
    #[must_use]
    pub fn kinds(&self) -> BTreeSet<Kind> {
        self.events.iter().map(Event::kind).collect()
    }
}

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

/// Why a text is not a session record.
#[derive(Debug)]
pub enum ParseError {
    /// The text does not match the grammar.
    Syntax(Box<pest::error::Error<Rule>>),
    /// A line's values could not be read.
    Value(ValueError),
    /// A row is not a well-formed event.
    Schema(SchemaError),
    /// The rows are individually well-formed but do not make a record.
    Structure(StructureError),
}

/// Why one row is not a well-formed event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// The row carries no `record` key, so nothing says what it is.
    NoKind,
    /// The row's `record` names no kind this schema knows.
    UnknownKind(String),
    /// A required field is absent. Required means required: the whole point of
    /// this schema is that provenance cannot be omitted.
    MissingField {
        /// The event kind.
        of: &'static str,
        /// The field.
        field: &'static str,
    },
    /// A field holds the wrong sort of value.
    WrongType {
        /// The event kind.
        of: &'static str,
        /// The field.
        field: String,
        /// What was wanted.
        want: &'static str,
    },
    /// The row carries a field this schema does not define. Rejected rather
    /// than ignored: a typo'd key that is silently dropped is a field nobody
    /// notices is missing.
    UnknownField {
        /// The event kind.
        of: &'static str,
        /// The field.
        field: String,
    },
    /// A field whose value comes from a closed vocabulary holds something
    /// outside it.
    BadValue {
        /// The event kind.
        of: &'static str,
        /// The field.
        field: &'static str,
        /// What it held.
        found: String,
    },
}

/// Why a sequence of well-formed rows is not a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StructureError {
    /// No `start`, so no regime, so nothing the record says transfers.
    NoStart,
    /// A `start` that is not the first row leaves rows before it untagged.
    StartNotFirst,
    /// Two `start` rows: two regimes, and no way to say which events ran
    /// under which.
    SecondStart,
    /// Two rows claim one identifier, so a link naming it names both.
    DuplicateId(String),
    /// A link names an event that never appeared before it. A forward or
    /// dangling link is a lineage that cannot be walked.
    DanglingLink {
        /// The event holding the link.
        from: String,
        /// The linking field.
        field: &'static str,
        /// What it pointed at.
        to: String,
    },
    /// A link names an event of the wrong kind.
    WrongLinkKind {
        /// The linking field.
        field: &'static str,
        /// What it pointed at.
        to: String,
        /// The kind it should have named.
        want: Kind,
    },
    /// A claim that names no artifact can produce a bound, never a number.
    ClaimConsumesNothing(String),
    /// A digest that is not 64 lowercase hex characters.
    BadDigest(String),
    /// A turn index a link names that no turn ever had.
    UnknownTurn(u32),
    /// Turn indices that do not run 1, 2, 3.
    TurnOutOfOrder {
        /// What was expected next.
        want: u32,
        /// What was found.
        found: u32,
    },
    /// Two `summary` rows, or one that is not last.
    SummaryNotLast,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "not a session record: {err}"),
            Self::Value(err) => write!(f, "not a session record: {err}"),
            Self::Schema(err) => write!(f, "not a session record: {err}"),
            Self::Structure(err) => write!(f, "not a session record: {err}"),
        }
    }
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoKind => write!(f, "a row with no `record` key, so nothing says what it is"),
            Self::UnknownKind(tag) => write!(f, "`{tag}` names no event kind"),
            Self::MissingField { of, field } => {
                write!(f, "a `{of}` row is missing its required `{field}`")
            }
            Self::WrongType { of, field, want } => {
                write!(f, "a `{of}` row's `{field}` is not {want}")
            }
            Self::UnknownField { of, field } => {
                write!(
                    f,
                    "a `{of}` row carries `{field}`, which this schema does not define"
                )
            }
            Self::BadValue { of, field, found } => {
                write!(
                    f,
                    "a `{of}` row's `{field}` holds `{found}`, which is outside its vocabulary"
                )
            }
        }
    }
}

impl fmt::Display for StructureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoStart => write!(f, "the record has no `start`, so it carries no regime"),
            Self::StartNotFirst => write!(
                f,
                "`start` is not the first row, so rows before it are untagged"
            ),
            Self::SecondStart => write!(
                f,
                "a second `start`: two regimes, and no way to say which events ran under which"
            ),
            Self::DuplicateId(id) => write!(f, "two rows claim the identifier `{id}`"),
            Self::DanglingLink { from, field, to } => {
                write!(
                    f,
                    "`{from}` links `{field}` to `{to}`, which no earlier row is"
                )
            }
            Self::WrongLinkKind { field, to, want } => {
                write!(f, "`{field}` names `{to}`, which is not a `{}`", want.tag())
            }
            Self::ClaimConsumesNothing(id) => write!(
                f,
                "claim `{id}` names no artifact it consumes, so recomputing it \
                 could produce a bound but never a number"
            ),
            Self::BadDigest(text) => {
                write!(f, "`{text}` is not 64 lowercase hex characters")
            }
            Self::UnknownTurn(index) => write!(f, "a row names turn {index}, which never happened"),
            Self::TurnOutOfOrder { want, found } => {
                write!(f, "turn {found} follows where turn {want} was expected")
            }
            Self::SummaryNotLast => write!(f, "`summary` is not the last row, or there are two"),
        }
    }
}

impl Error for ParseError {}

impl From<ValueError> for ParseError {
    fn from(err: ValueError) -> Self {
        Self::Value(err)
    }
}

impl From<SchemaError> for ParseError {
    fn from(err: SchemaError) -> Self {
        Self::Schema(err)
    }
}

impl From<StructureError> for ParseError {
    fn from(err: StructureError) -> Self {
        Self::Structure(err)
    }
}

// ---------------------------------------------------------------------------
// parsing
// ---------------------------------------------------------------------------

/// Parse a session record.
///
/// # Errors
///
/// Returns [`ParseError`] naming which of the four layers rejected it: the
/// grammar, the value space, one row's schema, or the record's structure. The
/// layers are kept apart because "this is not a record" is not a useful thing
/// to be told.
pub fn parse(input: &str) -> Result<Record, ParseError> {
    let mut parsed = RecordParser::parse(Rule::document, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?;
    let document = parsed
        .next()
        .ok_or(ParseError::Schema(SchemaError::NoKind))?;

    let mut events = Vec::new();
    for line in document.into_inner() {
        if line.as_rule() != Rule::event_line {
            continue; // a blank line
        }
        let object = line
            .into_inner()
            .find(|pair| pair.as_rule() == Rule::object)
            .ok_or(ParseError::Value(ValueError::Shape(
                "a line with no object",
            )))?;
        events.push(event(&object)?);
    }

    let regime = validate(&events)?;
    Ok(Record { regime, events })
}

/// One row.
fn event(object: &Pair<'_, Rule>) -> Result<Event, ParseError> {
    let mut members = json::object(object)?;
    let tag = match members.remove("record") {
        Some(Value::String(tag)) => tag,
        Some(_) => {
            return Err(SchemaError::WrongType {
                of: "row",
                field: "record".to_owned(),
                want: "a string",
            }
            .into());
        }
        None => return Err(SchemaError::NoKind.into()),
    };
    let kind = Kind::from_tag(&tag).ok_or_else(|| SchemaError::UnknownKind(tag.clone()))?;
    let of = kind.tag();

    let built = match kind {
        Kind::Start => Event::Start {
            regime: Box::new(regime(&mut take_object(&mut members, of, "regime")?, of)?),
        },
        Kind::Turn => Event::Turn {
            index: take_u32(&mut members, of, "index")?,
            prefill_tokens: take_u64(&mut members, of, "prefill_tokens")?,
        },
        Kind::Request => Event::Request {
            id: take_string(&mut members, of, "id")?,
            lane: take_string(&mut members, of, "lane")?,
            retry_of: take_optional_string(&mut members, of, "retry_of")?,
        },
        Kind::Response => Event::Response {
            id: take_string(&mut members, of, "id")?,
            to_request: take_string(&mut members, of, "to_request")?,
            output_tokens: take_u64(&mut members, of, "output_tokens")?,
        },
        Kind::Fork => Event::Fork {
            id: take_string(&mut members, of, "id")?,
            lane: take_string(&mut members, of, "lane")?,
            of_turn: take_u32(&mut members, of, "of_turn")?,
        },
        Kind::Capture => Event::Capture {
            id: take_string(&mut members, of, "id")?,
            from_fork: take_string(&mut members, of, "from_fork")?,
            entries: take_u32(&mut members, of, "entries")?,
        },
        Kind::Seam => Event::Seam {
            id: take_string(&mut members, of, "id")?,
            at_turn: take_u32(&mut members, of, "at_turn")?,
            rendered_bytes: take_u64(&mut members, of, "rendered_bytes")?,
        },
        Kind::ToolCall => Event::ToolCall {
            id: take_string(&mut members, of, "id")?,
            at_turn: take_u32(&mut members, of, "at_turn")?,
            tool: take_string(&mut members, of, "tool")?,
        },
        Kind::Claim => Event::Claim {
            id: take_string(&mut members, of, "id")?,
            hypothesis: take_string(&mut members, of, "hypothesis")?,
            result: {
                let text = take_string(&mut members, of, "result")?;
                Verdict::from_tag(&text).ok_or(SchemaError::BadValue {
                    of,
                    field: "result",
                    found: text,
                })?
            },
            consumes: take_artifacts(&mut members, of)?,
            supersedes: take_optional_string(&mut members, of, "supersedes")?,
        },
        Kind::Summary => Event::Summary {
            turns: take_u32(&mut members, of, "turns")?,
            prefill_tokens_total: take_u64(&mut members, of, "prefill_tokens_total")?,
            product_sha256: take_string(&mut members, of, "product_sha256")?,
        },
    };

    // Anything left is a key this schema does not define. A typo'd key that is
    // silently dropped is a field nobody notices is missing.
    if let Some(field) = members.keys().next() {
        return Err(SchemaError::UnknownField {
            of,
            field: field.clone(),
        }
        .into());
    }
    Ok(built)
}

/// The regime, from a `start` row's `regime` object.
fn regime(members: &mut BTreeMap<String, Value>, of: &'static str) -> Result<Regime, ParseError> {
    let arm = take_string(members, of, "arm")?;
    let dogma_version = take_u32(members, of, "dogma_version")?;
    let mut substrate_members = take_object(members, of, "substrate")?;
    let substrate = Substrate {
        name: take_string(&mut substrate_members, of, "name")?,
        model: take_string(&mut substrate_members, of, "model")?,
        quantization: take_string(&mut substrate_members, of, "quantization")?,
        sampler: take_object(&mut substrate_members, of, "sampler")?,
        reasoning: {
            let text = take_string(&mut substrate_members, of, "reasoning")?;
            Reasoning::from_tag(&text).ok_or(SchemaError::BadValue {
                of,
                field: "reasoning",
                found: text,
            })?
        },
        hardware: take_string(&mut substrate_members, of, "hardware")?,
    };
    if let Some(field) = substrate_members.keys().next() {
        return Err(SchemaError::UnknownField {
            of,
            field: format!("substrate.{field}"),
        }
        .into());
    }
    if let Some(field) = members.keys().next() {
        return Err(SchemaError::UnknownField {
            of,
            field: format!("regime.{field}"),
        }
        .into());
    }
    Ok(Regime {
        arm,
        substrate,
        dogma_version,
    })
}

fn take_artifacts(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
) -> Result<Vec<Artifact>, ParseError> {
    let Some(value) = members.remove("consumes") else {
        return Err(SchemaError::MissingField {
            of,
            field: "consumes",
        }
        .into());
    };
    let Value::Array(items) = value else {
        return Err(SchemaError::WrongType {
            of,
            field: "consumes".to_owned(),
            want: "a list of artifacts",
        }
        .into());
    };
    let mut artifacts = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(mut fields) = item else {
            return Err(SchemaError::WrongType {
                of,
                field: "consumes[]".to_owned(),
                want: "an object",
            }
            .into());
        };
        let artifact = Artifact {
            path: take_string(&mut fields, of, "path")?,
            sha256: take_string(&mut fields, of, "sha256")?,
        };
        if let Some(field) = fields.keys().next() {
            return Err(SchemaError::UnknownField {
                of,
                field: format!("consumes[].{field}"),
            }
            .into());
        }
        artifacts.push(artifact);
    }
    Ok(artifacts)
}

fn take_string(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
    field: &'static str,
) -> Result<String, ParseError> {
    match members.remove(field) {
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(SchemaError::WrongType {
            of,
            field: field.to_owned(),
            want: "a string",
        }
        .into()),
        None => Err(SchemaError::MissingField { of, field }.into()),
    }
}

fn take_optional_string(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
    field: &'static str,
) -> Result<Option<String>, ParseError> {
    match members.remove(field) {
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(SchemaError::WrongType {
            of,
            field: field.to_owned(),
            want: "a string",
        }
        .into()),
        None => Ok(None),
    }
}

fn take_object(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
    field: &'static str,
) -> Result<BTreeMap<String, Value>, ParseError> {
    match members.remove(field) {
        Some(Value::Object(inner)) => Ok(inner),
        Some(_) => Err(SchemaError::WrongType {
            of,
            field: field.to_owned(),
            want: "an object",
        }
        .into()),
        None => Err(SchemaError::MissingField { of, field }.into()),
    }
}

fn take_u64(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
    field: &'static str,
) -> Result<u64, ParseError> {
    match members.remove(field) {
        Some(Value::Integer(number)) if number >= 0 => Ok(number.unsigned_abs()),
        Some(_) => Err(SchemaError::WrongType {
            of,
            field: field.to_owned(),
            want: "a non-negative integer",
        }
        .into()),
        None => Err(SchemaError::MissingField { of, field }.into()),
    }
}

fn take_u32(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
    field: &'static str,
) -> Result<u32, ParseError> {
    let number = take_u64(members, of, field)?;
    u32::try_from(number).map_err(|_| {
        SchemaError::WrongType {
            of,
            field: field.to_owned(),
            want: "an integer that fits in 32 bits",
        }
        .into()
    })
}

// ---------------------------------------------------------------------------
// structure
// ---------------------------------------------------------------------------

/// A digest, as this schema requires it.
fn digest_ok(text: &str) -> bool {
    text.len() == 64
        && text
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// The rules that hold across rows rather than within one.
///
/// Every link is checked against events ALREADY SEEN, never against the whole
/// record. A lineage that can only be resolved by reading ahead is a lineage
/// that cannot be walked while the record is being written, which is the only
/// time anyone would want to walk it.
fn validate(events: &[Event]) -> Result<Regime, ParseError> {
    let mut regime = None;
    let mut ids: BTreeMap<&str, Kind> = BTreeMap::new();
    let mut turns: BTreeSet<u32> = BTreeSet::new();
    let mut next_turn = 1_u32;
    let mut summary_seen = false;

    for (position, event) in events.iter().enumerate() {
        if summary_seen {
            return Err(StructureError::SummaryNotLast.into());
        }
        match event {
            Event::Start { .. } if position != 0 => {
                return Err(if regime.is_some() {
                    StructureError::SecondStart.into()
                } else {
                    StructureError::StartNotFirst.into()
                });
            }
            Event::Start { regime: found } => regime = Some((**found).clone()),
            _ if position == 0 => return Err(StructureError::StartNotFirst.into()),
            _ => {}
        }

        if let Some(id) = event.id()
            && ids.insert(id, event.kind()).is_some()
        {
            return Err(StructureError::DuplicateId(id.to_owned()).into());
        }

        match event {
            Event::Turn { index, .. } => {
                if *index != next_turn {
                    return Err(StructureError::TurnOutOfOrder {
                        want: next_turn,
                        found: *index,
                    }
                    .into());
                }
                turns.insert(*index);
                next_turn += 1;
            }
            Event::Request { id, retry_of, .. } => {
                if let Some(previous) = retry_of {
                    link(&ids, id, "retry_of", previous, Kind::Request)?;
                }
            }
            Event::Response { id, to_request, .. } => {
                link(&ids, id, "to_request", to_request, Kind::Request)?;
            }
            Event::Fork { of_turn, .. } => {
                if !turns.contains(of_turn) {
                    return Err(StructureError::UnknownTurn(*of_turn).into());
                }
            }
            Event::Capture { id, from_fork, .. } => {
                link(&ids, id, "from_fork", from_fork, Kind::Fork)?;
            }
            Event::Seam { at_turn, .. } | Event::ToolCall { at_turn, .. } => {
                if !turns.contains(at_turn) {
                    return Err(StructureError::UnknownTurn(*at_turn).into());
                }
            }
            Event::Claim {
                id,
                consumes,
                supersedes,
                ..
            } => {
                // Recompute-sufficiency, as a rule rather than a habit. A claim
                // that names nothing it consumes can be re-read but not
                // re-derived, and a number nobody can re-derive is a number
                // nobody can check.
                if consumes.is_empty() {
                    return Err(StructureError::ClaimConsumesNothing(id.clone()).into());
                }
                for artifact in consumes {
                    if !digest_ok(&artifact.sha256) {
                        return Err(StructureError::BadDigest(artifact.sha256.clone()).into());
                    }
                }
                if let Some(superseded) = supersedes {
                    link(&ids, id, "supersedes", superseded, Kind::Claim)?;
                }
            }
            Event::Summary { product_sha256, .. } => {
                if !digest_ok(product_sha256) {
                    return Err(StructureError::BadDigest(product_sha256.clone()).into());
                }
                summary_seen = true;
            }
            Event::Start { .. } => {}
        }
    }

    regime.ok_or_else(|| StructureError::NoStart.into())
}

/// One link, checked against what has already been seen.
fn link(
    ids: &BTreeMap<&str, Kind>,
    from: &str,
    field: &'static str,
    to: &str,
    want: Kind,
) -> Result<(), ParseError> {
    match ids.get(to) {
        None => Err(StructureError::DanglingLink {
            from: from.to_owned(),
            field,
            to: to.to_owned(),
        }
        .into()),
        Some(kind) if *kind != want => Err(StructureError::WrongLinkKind {
            field,
            to: to.to_owned(),
            want,
        }
        .into()),
        Some(_) => Ok(()),
    }
}

// ---------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------

/// Render a record back to its own spelling.
///
/// One spelling per record: a record read and written back is the same bytes,
/// which is what lets two dumps of the same object be diffed and the diff
/// mean something.
#[must_use]
pub fn render(record: &Record) -> String {
    let mut out = String::new();
    for event in &record.events {
        json::render(&Value::Object(event_value(event)), &mut out);
        out.push('\n');
    }
    out
}

/// The whole record as the value space: its regime, and its events.
///
/// Public because it is what the conformance harness pins and what the CLI
/// will emit. Deriving both from one function means a fixture cannot agree
/// with the parser while disagreeing with what a caller actually receives.
#[must_use]
pub fn to_value(record: &Record) -> Value {
    Value::Object(BTreeMap::from([
        ("regime".to_owned(), regime_value(&record.regime)),
        (
            "events".to_owned(),
            Value::Array(
                record
                    .events
                    .iter()
                    .map(|event| Value::Object(event_value(event)))
                    .collect(),
            ),
        ),
    ]))
}

/// One event, as the value space.
fn event_value(event: &Event) -> BTreeMap<String, Value> {
    let mut members = BTreeMap::new();
    let mut put = |key: &str, value: Value| {
        members.insert(key.to_owned(), value);
    };
    put("record", Value::String(event.kind().tag().to_owned()));
    match event {
        Event::Start { regime } => put("regime", regime_value(regime)),
        Event::Turn {
            index,
            prefill_tokens,
        } => {
            put("index", Value::Integer(i64::from(*index)));
            put("prefill_tokens", integer(*prefill_tokens));
        }
        Event::Request { id, lane, retry_of } => {
            put("id", Value::String(id.clone()));
            put("lane", Value::String(lane.clone()));
            if let Some(previous) = retry_of {
                put("retry_of", Value::String(previous.clone()));
            }
        }
        Event::Response {
            id,
            to_request,
            output_tokens,
        } => {
            put("id", Value::String(id.clone()));
            put("to_request", Value::String(to_request.clone()));
            put("output_tokens", integer(*output_tokens));
        }
        Event::Fork { id, lane, of_turn } => {
            put("id", Value::String(id.clone()));
            put("lane", Value::String(lane.clone()));
            put("of_turn", Value::Integer(i64::from(*of_turn)));
        }
        Event::Capture {
            id,
            from_fork,
            entries,
        } => {
            put("id", Value::String(id.clone()));
            put("from_fork", Value::String(from_fork.clone()));
            put("entries", Value::Integer(i64::from(*entries)));
        }
        Event::Seam {
            id,
            at_turn,
            rendered_bytes,
        } => {
            put("id", Value::String(id.clone()));
            put("at_turn", Value::Integer(i64::from(*at_turn)));
            put("rendered_bytes", integer(*rendered_bytes));
        }
        Event::ToolCall { id, at_turn, tool } => {
            put("id", Value::String(id.clone()));
            put("at_turn", Value::Integer(i64::from(*at_turn)));
            put("tool", Value::String(tool.clone()));
        }
        Event::Claim {
            id,
            hypothesis,
            result,
            consumes,
            supersedes,
        } => {
            put("id", Value::String(id.clone()));
            put("hypothesis", Value::String(hypothesis.clone()));
            put("result", Value::String(result.tag().to_owned()));
            put(
                "consumes",
                Value::Array(
                    consumes
                        .iter()
                        .map(|artifact| {
                            Value::Object(BTreeMap::from([
                                ("path".to_owned(), Value::String(artifact.path.clone())),
                                ("sha256".to_owned(), Value::String(artifact.sha256.clone())),
                            ]))
                        })
                        .collect(),
                ),
            );
            if let Some(superseded) = supersedes {
                put("supersedes", Value::String(superseded.clone()));
            }
        }
        Event::Summary {
            turns,
            prefill_tokens_total,
            product_sha256,
        } => {
            put("turns", Value::Integer(i64::from(*turns)));
            put("prefill_tokens_total", integer(*prefill_tokens_total));
            put("product_sha256", Value::String(product_sha256.clone()));
        }
    }
    members
}

fn regime_value(regime: &Regime) -> Value {
    let substrate = BTreeMap::from([
        (
            "name".to_owned(),
            Value::String(regime.substrate.name.clone()),
        ),
        (
            "model".to_owned(),
            Value::String(regime.substrate.model.clone()),
        ),
        (
            "quantization".to_owned(),
            Value::String(regime.substrate.quantization.clone()),
        ),
        (
            "sampler".to_owned(),
            Value::Object(regime.substrate.sampler.clone()),
        ),
        (
            "reasoning".to_owned(),
            Value::String(regime.substrate.reasoning.tag().to_owned()),
        ),
        (
            "hardware".to_owned(),
            Value::String(regime.substrate.hardware.clone()),
        ),
    ]);
    Value::Object(BTreeMap::from([
        ("arm".to_owned(), Value::String(regime.arm.clone())),
        ("substrate".to_owned(), Value::Object(substrate)),
        (
            "dogma_version".to_owned(),
            Value::Integer(i64::from(regime.dogma_version)),
        ),
    ]))
}

/// A `u64` count as the value space holds it.
///
/// Saturating rather than wrapping: a count above `i64::MAX` is not a count,
/// and silently writing a negative one would be a lie the reader could not
/// see. The schema rejects negatives on the way back in, so a saturated value
/// fails to round-trip loudly.
fn integer(count: u64) -> Value {
    Value::Integer(i64::try_from(count).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::{
        Event, Kind, ParseError, Reasoning, Regime, StructureError, Verdict, parse, render,
    };

    /// A `start` line whose regime is complete, as every record needs one.
    const START: &str = r#"{"record":"start","regime":{"arm":"baseline","dogma_version":0,"substrate":{"name":"local","model":"a-model","quantization":"q4","sampler":{"seed":7,"temperature":0.7},"reasoning":"on","hardware":"one-gpu"}}}"#;

    fn record(rest: &str) -> String {
        format!("{START}\n{rest}")
    }

    #[test]
    fn the_regime_comes_from_the_required_start() {
        let parsed = parse(&record("")).expect("a record");
        assert_eq!(parsed.regime.arm, "baseline");
        assert_eq!(parsed.regime.substrate.name, "local");
        assert_eq!(parsed.regime.substrate.reasoning, Reasoning::On);
    }

    // The acceptance case: provenance is not optional, and a record that omits
    // it does not become a Record at all.
    #[test]
    fn a_regime_missing_its_substrate_does_not_parse() {
        let source = r#"{"record":"start","regime":{"arm":"baseline","dogma_version":0}}"#;
        let err = parse(source).expect_err("a regime without a substrate is not a regime");
        assert!(
            format!("{err}").contains("substrate"),
            "the error must name the missing field, got: {err}"
        );
    }

    #[test]
    fn a_record_with_no_start_does_not_parse() {
        let source = r#"{"record":"turn","index":1,"prefill_tokens":10}"#;
        assert!(matches!(
            parse(source),
            Err(ParseError::Structure(StructureError::StartNotFirst))
        ));
    }

    #[test]
    fn an_empty_record_carries_no_regime_and_so_is_not_a_record() {
        assert!(matches!(
            parse(""),
            Err(ParseError::Structure(StructureError::NoStart))
        ));
    }

    // Retry lineage: the answer names the request that produced it, and a
    // retry names the request it replaces. Both resolve backwards.
    #[test]
    fn a_retry_names_the_request_it_replaces() {
        let source = record(concat!(
            r#"{"record":"request","id":"r1","lane":"main"}"#,
            "\n",
            r#"{"record":"request","id":"r2","lane":"main","retry_of":"r1"}"#,
            "\n",
            r#"{"record":"response","id":"a1","to_request":"r2","output_tokens":12}"#,
            "\n",
        ));
        let parsed = parse(&source).expect("a record");
        assert!(matches!(
            parsed.events.last(),
            Some(Event::Response { to_request, .. }) if to_request == "r2"
        ));
    }

    #[test]
    fn a_response_naming_no_earlier_request_does_not_parse() {
        let source = record(concat!(
            r#"{"record":"response","id":"a1","to_request":"r9","output_tokens":12}"#,
            "\n",
        ));
        assert!(matches!(
            parse(&source),
            Err(ParseError::Structure(StructureError::DanglingLink { .. }))
        ));
    }

    #[test]
    fn a_link_to_the_wrong_kind_does_not_parse() {
        let source = record(concat!(
            r#"{"record":"turn","index":1,"prefill_tokens":10}"#,
            "\n",
            r#"{"record":"fork","id":"f1","lane":"interview","of_turn":1}"#,
            "\n",
            r#"{"record":"response","id":"a1","to_request":"f1","output_tokens":3}"#,
            "\n",
        ));
        assert!(matches!(
            parse(&source),
            Err(ParseError::Structure(StructureError::WrongLinkKind { .. }))
        ));
    }

    // Recompute-sufficiency: a claim that names nothing it consumes can be
    // re-read but never re-derived.
    #[test]
    fn a_claim_consuming_nothing_does_not_parse() {
        let source = record(concat!(
            r#"{"record":"claim","id":"c1","hypothesis":"h","result":"supported","consumes":[]}"#,
            "\n",
        ));
        assert!(matches!(
            parse(&source),
            Err(ParseError::Structure(StructureError::ClaimConsumesNothing(
                _
            )))
        ));
    }

    #[test]
    fn a_claim_consuming_an_artifact_with_a_bad_digest_does_not_parse() {
        let source = record(concat!(
            r#"{"record":"claim","id":"c1","hypothesis":"h","result":"supported",
"consumes":[{"path":"run.jsonl","sha256":"nope"}]}"#,
            "\n",
        ));
        assert!(parse(&source).is_err());
    }

    // Claim atomicity: a correction is a new row linked to the old one, and
    // the old one is still there to be read.
    #[test]
    fn a_correction_is_a_second_claim_linked_to_the_first() {
        let digest = "a".repeat(64);
        let source = record(&format!(
            "{{\"record\":\"claim\",\"id\":\"c1\",\"hypothesis\":\"h\",\"result\":\"supported\",\
             \"consumes\":[{{\"path\":\"run.jsonl\",\"sha256\":\"{digest}\"}}]}}\n\
             {{\"record\":\"claim\",\"id\":\"c2\",\"hypothesis\":\"h\",\"result\":\"refuted\",\
             \"consumes\":[{{\"path\":\"run.jsonl\",\"sha256\":\"{digest}\"}}],\"supersedes\":\"c1\"}}\n"
        ));
        let parsed = parse(&source).expect("a record");
        let claims: Vec<_> = parsed
            .events
            .iter()
            .filter(|event| event.kind() == Kind::Claim)
            .collect();
        assert_eq!(
            claims.len(),
            2,
            "the superseded claim is still in the record"
        );
        assert!(matches!(
            claims[1],
            Event::Claim {
                result: Verdict::Refuted,
                supersedes: Some(_),
                ..
            }
        ));
    }

    #[test]
    fn a_field_the_schema_does_not_define_is_rejected_not_ignored() {
        let source = record(concat!(
            r#"{"record":"turn","index":1,"prefill_tokens":10,"prefil_tokens":10}"#,
            "\n",
        ));
        assert!(parse(&source).is_err(), "a typo'd key must not be dropped");
    }

    #[test]
    fn turn_indices_must_run_in_order() {
        let source = record(concat!(
            r#"{"record":"turn","index":1,"prefill_tokens":10}"#,
            "\n",
            r#"{"record":"turn","index":3,"prefill_tokens":10}"#,
            "\n",
        ));
        assert!(matches!(
            parse(&source),
            Err(ParseError::Structure(StructureError::TurnOutOfOrder { .. }))
        ));
    }

    #[test]
    fn an_exact_decimal_survives_the_round_trip() {
        let parsed = parse(&record("")).expect("a record");
        let rendered = render(&parsed);
        assert!(
            rendered.contains("0.7"),
            "the sampler's exact decimal must come back as written: {rendered}"
        );
        assert!(!rendered.contains("0.699"), "a float crept in: {rendered}");
    }

    #[test]
    fn rendering_a_record_round_trips_through_the_parser() {
        let digest = "b".repeat(64);
        let source = record(&format!(
            "{{\"record\":\"turn\",\"index\":1,\"prefill_tokens\":1024}}\n\
             {{\"record\":\"fork\",\"id\":\"f1\",\"lane\":\"interview\",\"of_turn\":1}}\n\
             {{\"record\":\"capture\",\"id\":\"p1\",\"from_fork\":\"f1\",\"entries\":3}}\n\
             {{\"record\":\"summary\",\"turns\":1,\"prefill_tokens_total\":1024,\
             \"product_sha256\":\"{digest}\"}}\n"
        ));
        let once = parse(&source).expect("a record");
        let twice = parse(&render(&once)).expect("a rendering is itself a record");
        assert_eq!(once, twice);
        assert_eq!(render(&once), render(&twice));
    }

    #[test]
    fn every_reasoning_state_round_trips() {
        for state in Reasoning::ALL {
            let source = format!(
                "{{\"record\":\"start\",\"regime\":{{\"arm\":\"a\",\"dogma_version\":0,\
                 \"substrate\":{{\"name\":\"n\",\"model\":\"m\",\"quantization\":\"q\",\
                 \"sampler\":{{}},\"reasoning\":\"{}\",\"hardware\":\"h\"}}}}}}\n",
                state.tag()
            );
            let parsed = parse(&source).expect("a record");
            assert_eq!(parsed.regime.substrate.reasoning, *state);
        }
    }

    /// Where the committed corpus lives.
    fn corpus() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("formats/record/fixtures/valid")
    }

    // An event kind with no fixture has an untested serialization path, and in
    // prior work that is exactly where a silent drop lived. Adding a variant
    // to `Kind` is therefore not finished until a fixture exercises it.
    #[test]
    fn every_event_kind_appears_in_the_committed_corpus() {
        let dir = corpus();
        let mut seen = std::collections::BTreeSet::new();
        let mut cases = 0_usize;
        for entry in std::fs::read_dir(&dir).expect("the corpus is readable") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|ext| ext != "jsonl") {
                continue;
            }
            cases += 1;
            let source = std::fs::read_to_string(&path).expect("a valid case is UTF-8");
            let parsed = parse(&source)
                .unwrap_or_else(|err| panic!("{} does not parse: {err}", path.display()));
            seen.extend(parsed.kinds());
        }
        // Without this the assertion below would hold vacuously over an empty
        // directory, which is the failure this repository exists to prevent.
        assert!(cases > 0, "{} holds no cases", dir.display());
        let missing: Vec<_> = Kind::ALL
            .iter()
            .filter(|kind| !seen.contains(kind))
            .map(|kind| kind.tag())
            .collect();
        assert!(
            missing.is_empty(),
            "event kind(s) with no fixture in {}: {missing:?}",
            dir.display()
        );
    }

    // The acceptance criterion from the issue, checked rather than asserted in
    // prose: a report's front-matter `[regime]` table mirrors the schema's
    // trio. The schema is the definition and the report is the mirror, so a
    // key added here without being added there is a divergence, and this is
    // where it surfaces.
    #[test]
    fn the_report_front_matter_mirrors_the_regime_trio() {
        let template =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../results/_template/README.md");
        let text = std::fs::read_to_string(&template)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", template.display()));
        let Some((_, after)) = text.split_once("\n[regime]\n") else {
            panic!("{} has no [regime] table", template.display())
        };
        let keys: Vec<&str> = after
            .lines()
            .take_while(|line| !line.starts_with("+++") && !line.starts_with('['))
            .filter_map(|line| line.split_once('=').map(|(key, _)| key.trim()))
            .collect();
        assert!(!keys.is_empty(), "the [regime] table binds nothing");
        assert_eq!(
            keys,
            Regime::TAGS,
            "the report's [regime] table and the schema's regime disagree"
        );
    }

    #[test]
    fn a_raw_newline_inside_a_string_does_not_parse() {
        let source = "{\"record\":\"start\",\"regime\":{\"arm\":\"a\nb\"}}";
        assert!(parse(source).is_err(), "one event, one line");
    }
}
