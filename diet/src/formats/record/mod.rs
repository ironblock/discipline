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

/// Declare a closed vocabulary: the enum, its `ALL`, its tag and its lookup.
///
/// One invocation produces all four, so a variant cannot exist without being
/// in `ALL` and having a tag. With a hand-written `ALL`, a variant omitted
/// from it is simultaneously invisible to every check that iterates the list
/// AND unparseable -- the format gains a case nobody can write, nobody can
/// see, and no gate goes red about.
macro_rules! vocabulary {
    (
        $(#[$enum_doc:meta])*
        $name:ident { $( $(#[$doc:meta])* $variant:ident => $tag:literal ),+ $(,)? }
    ) => {
        $(#[$enum_doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name { $( $(#[$doc])* $variant ),+ }

        impl $name {
            /// Every variant, generated beside the enum so the two cannot
            /// drift apart.
            pub const ALL: &'static [Self] = &[ $(Self::$variant),+ ];

            /// The name this variant is written under.
            #[must_use]
            pub fn tag(self) -> &'static str {
                match self { $(Self::$variant => $tag),+ }
            }

            /// The variant `tag` names, if there is one.
            fn from_tag(tag: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|variant| variant.tag() == tag)
            }
        }
    };
}

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

vocabulary! {
    /// The reasoning state a run was served under.
    Reasoning {
        /// Not requested.
        Off => "off",
        /// Requested, and returned.
        On => "on",
        /// Requested, and NOT returned -- the substrate was configured to emit
        /// no reasoning while reasoning was enabled. Its own state rather than
        /// a flavour of `On` because the combination is a known footgun: a
        /// mechanism that depends on seeing reasoning is silently defeated,
        /// and a record that cannot express the difference cannot explain the
        /// result.
        Suppressed => "suppressed",
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

vocabulary! {
    /// Which kind of event a row is.
    ///
    /// Separated from [`Event`] so that the coverage test can enumerate the
    /// kinds without constructing one of each. A kind here with no fixture is
    /// a kind with an untested serialization path, and in prior work that is
    /// exactly where a silent drop lived.
    Kind {
        /// The session begins. Carries the regime.
        Start => "start",
        /// A turn happened.
        Turn => "turn",
        /// A request went to the substrate.
        Request => "request",
        /// A response came back.
        Response => "response",
        /// An interview fork was opened.
        Fork => "fork",
        /// A capture wrote to the working object.
        Capture => "capture",
        /// The working object was rendered into a prompt.
        Seam => "seam",
        /// A tool was called.
        ToolCall => "tool_call",
        /// A lane's output rejected whole by the groundedness floor.
        Rejected => "rejected",
        /// One hypothesis, one result, and what recomputing it consumes.
        Claim => "claim",
        /// The session's totals.
        Summary => "summary",
    }
}

/// A count the record's value space can hold.
///
/// The value space's integer is an `i64`, so a `u64` count above `i64::MAX`
/// has no spelling. Rendering used to saturate it to `i64::MAX` under a
/// comment claiming the result "fails to round-trip loudly" -- it does not:
/// `i64::MAX` reads back as a perfectly ordinary count, silently different
/// from the one written. This type makes the unrepresentable value
/// unconstructable instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Count(u64);

impl Count {
    /// The largest count the value space can spell.
    pub const MAX: u64 = i64::MAX as u64;

    /// The count `value` names.
    ///
    /// # Errors
    ///
    /// Returns [`CountTooLarge`] for a value the value space cannot spell.
    pub fn new(value: u64) -> Result<Self, CountTooLarge> {
        if value > Self::MAX {
            return Err(CountTooLarge(value));
        }
        Ok(Self(value))
    }

    /// The count as a number.
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }

    /// This count plus `other`, saturating at [`Count::MAX`].
    #[must_use]
    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0).min(Self::MAX))
    }
}

impl fmt::Display for Count {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A count the record's value space cannot spell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CountTooLarge(pub u64);

impl fmt::Display for CountTooLarge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} is above {}, which the value space cannot spell",
            self.0,
            Count::MAX
        )
    }
}

impl Error for CountTooLarge {}

/// An artifact a claim consumes to be recomputed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    /// Where it lives, relative to the results directory.
    pub path: String,
    /// Its digest, 64 lowercase hex characters.
    pub sha256: String,
}

vocabulary! {
    /// What a claim concluded.
    Verdict {
        /// The evidence supports the hypothesis.
        Supported => "supported",
        /// The evidence refutes it.
        Refuted => "refuted",
        /// The evidence does neither, which is a result and not a failure.
        Inconclusive => "inconclusive",
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
        prefill_tokens: Count,
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
        output_tokens: Count,
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
        rendered_bytes: Count,
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
    /// A lane's output rejected whole because too little of it was grounded
    /// in the input the lane was told to work from.
    ///
    /// Carries the score, because a rejection nobody can audit is a fallback
    /// output standing for a reason somebody remembers.
    Rejected {
        /// This rejection's identifier.
        id: String,
        /// Which lane was rejected.
        lane: String,
        /// The turn it happened in. Every id-bearing row links to something
        /// already seen, and this is the anchor for a lane that need not
        /// correspond to a fork.
        at_turn: u32,
        /// How many of its entries were grounded.
        grounded: Count,
        /// How many entries it emitted.
        of: Count,
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
        prefill_tokens_total: Count,
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
            Self::Rejected { .. } => Kind::Rejected,
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
            | Self::Rejected { id, .. }
            | Self::Claim { id, .. } => Some(id),
            Self::Start { .. } | Self::Turn { .. } | Self::Summary { .. } => None,
        }
    }
}

/// A parsed session record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// Every event, in the order it appeared, `start` included.
    ///
    /// The regime is NOT a second field beside this. It was, and a hand-built
    /// `Record` could then hold two copies that disagreed while `render` wrote
    /// only one of them -- the exact failure the schema's own sentence warns
    /// about, "a tag repeated on every row is a tag that can disagree with
    /// itself", applied to the type instead of to a row.
    pub events: Vec<Event>,
}

impl Record {
    /// The regime every event in this record ran under.
    ///
    /// Read from the required `start` event rather than stored beside it, so
    /// there is only ever one copy to be right.
    #[must_use]
    pub fn regime(&self) -> &Regime {
        match self.events.first() {
            Some(Event::Start { regime }) => regime,
            _ => unreachable!("validate() refuses a record whose first event is not a start"),
        }
    }

    /// Every event kind this record carries.
    #[must_use]
    pub fn kinds(&self) -> BTreeSet<Kind> {
        self.events.iter().map(Event::kind).collect()
    }
}

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

/// The deepest nesting a record may carry.
///
/// A record's own deepest legitimate shape is four -- `regime.substrate.
/// sampler.<setting>` -- so this is generous by an order of magnitude and
/// still far below where recursive descent runs out of stack. Without it a
/// 2 KB file of nested objects ABORTS the process, and this format's own
/// corpus says, in `invalid/not-utf8.reason`, that a format must return a
/// verdict on arbitrary bytes rather than crash on them. `Substrate.sampler`
/// is the arrival vector: the one untyped, arbitrarily-nested field here.
pub const MAX_DEPTH: usize = 32;

/// Why a text is not a session record.
#[derive(Debug)]
pub enum ParseError {
    /// The text nests deeper than [`MAX_DEPTH`].
    ///
    /// Checked before the grammar runs, because the grammar is what would
    /// otherwise recurse until the stack ran out -- and a stack overflow is
    /// not a verdict, it is the absence of one.
    TooDeep {
        /// How deep it went.
        depth: usize,
        /// The limit.
        limit: usize,
    },
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
    /// A required field is present and says nothing, which is the same thing
    /// as absent for every purpose this schema serves.
    BlankField {
        /// The event kind.
        of: &'static str,
        /// The field.
        field: &'static str,
    },
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
    /// A rejection that is not a rejection: an empty lane, or one whose
    /// grounded count is not below its total.
    NotARejection {
        /// The rejection's identifier.
        id: String,
        /// The numerator.
        grounded: u64,
        /// The denominator.
        of: u64,
    },
    /// A summary that does not summarise the rows above it.
    SummaryDisagrees {
        /// Which total.
        field: &'static str,
        /// What the summary claims.
        says: u64,
        /// What the rows add up to.
        counted: u64,
    },
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
            Self::TooDeep { depth, limit } => write!(
                f,
                "not a session record: nested {depth} deep, and the limit is \
                 {limit}; deeper than that the parser runs out of stack, and a \
                 crash is not a verdict"
            ),
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
            Self::BlankField { of, field } => write!(
                f,
                "a `{of}` row's required `{field}` is blank, which is absent \
                 with the key still there"
            ),
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
            Self::NotARejection { id, grounded, of } => write!(
                f,
                "rejection `{id}` scores {grounded}/{of}, which is not a lane \
                 the floor rejected: a rejection is below its floor, and an \
                 empty lane is not below any floor"
            ),
            Self::SummaryDisagrees {
                field,
                says,
                counted,
            } => write!(
                f,
                "the summary's `{field}` says {says}, but the rows above it add \
                 up to {counted}"
            ),
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
    let depth = nesting_depth(input);
    if depth > MAX_DEPTH {
        return Err(ParseError::TooDeep {
            depth,
            limit: MAX_DEPTH,
        });
    }
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

    validate(&events)?;
    Ok(Record { events })
}

/// How deeply `input` nests, counted from the bytes.
///
/// Deliberately not from the parse tree: the parse is what has to be
/// protected. Brackets inside strings do not count, so a record whose text
/// happens to contain `{` is not rejected for it.
fn nesting_depth(input: &str) -> usize {
    let (mut depth, mut deepest) = (0_usize, 0_usize);
    let (mut in_string, mut escaped) = (false, false);
    for byte in input.bytes() {
        if in_string {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                b'"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'{' | b'[' => {
                depth += 1;
                deepest = deepest.max(depth);
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    deepest
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
        Kind::Rejected => Event::Rejected {
            id: take_string(&mut members, of, "id")?,
            lane: take_string(&mut members, of, "lane")?,
            at_turn: take_u32(&mut members, of, "at_turn")?,
            grounded: take_u64(&mut members, of, "grounded")?,
            of: take_u64(&mut members, of, "of")?,
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
        sampler: {
            // A substrate whose sampler settings are the empty object records
            // that the run had settings and declines to say which. The issue
            // names sampler settings as one of the substrate's facets, and an
            // empty map satisfies the type while satisfying nothing else.
            let settings = take_object(&mut substrate_members, of, "sampler")?;
            if settings.is_empty() {
                return Err(SchemaError::BlankField {
                    of,
                    field: "substrate.sampler",
                }
                .into());
            }
            settings
        },
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
        // Blank is absent. Every string this schema requires is an identifier
        // or a statement, and one that says nothing is a required field
        // satisfied by typing two quotes -- which is what "the key is there"
        // enforcement buys you and provenance does not.
        Some(Value::String(text)) if text.trim().is_empty() => {
            Err(SchemaError::BlankField { of, field }.into())
        }
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
        Some(Value::String(text)) if text.trim().is_empty() => {
            Err(SchemaError::BlankField { of, field }.into())
        }
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
) -> Result<Count, ParseError> {
    match members.remove(field) {
        Some(Value::Integer(number)) if number >= 0 => {
            Ok(Count::new(number.unsigned_abs()).unwrap_or_default())
        }
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
    let number = take_u64(members, of, field)?.get();
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

/// What the walk has seen so far.
///
/// Held apart from the loop so that "what a row is checked against" is a value
/// with a name rather than five locals: every link resolves against THIS, and
/// this only ever holds rows that already went past.
#[derive(Default)]
struct Seen<'a> {
    ids: BTreeMap<&'a str, Kind>,
    turns: BTreeSet<u32>,
    next_turn: u32,
    prefill_tokens: Count,
}

impl<'a> Seen<'a> {
    fn new() -> Self {
        Self {
            next_turn: 1,
            ..Self::default()
        }
    }

    /// Record `event`'s identifier, refusing a second row that claims it.
    ///
    /// Called AFTER [`Self::admit`], never before. The other order put a row's
    /// own id in the set before checking that row's links, so a request could
    /// retry itself and a claim supersede itself -- a cycle of length one,
    /// which is the only cycle a backwards-only rule does not already exclude.
    fn claim_id(&mut self, event: &'a Event) -> Result<(), ParseError> {
        if let Some(id) = event.id()
            && self.ids.insert(id, event.kind()).is_some()
        {
            return Err(StructureError::DuplicateId(id.to_owned()).into());
        }
        Ok(())
    }

    /// The rules one row carries about rows before it.
    fn admit(&mut self, event: &Event) -> Result<(), ParseError> {
        match event {
            Event::Turn {
                index,
                prefill_tokens,
            } => {
                if *index != self.next_turn {
                    return Err(StructureError::TurnOutOfOrder {
                        want: self.next_turn,
                        found: *index,
                    }
                    .into());
                }
                self.turns.insert(*index);
                self.next_turn += 1;
                self.prefill_tokens = self.prefill_tokens.saturating_add(*prefill_tokens);
            }
            Event::Request { id, retry_of, .. } => {
                if let Some(previous) = retry_of {
                    link(&self.ids, id, "retry_of", previous, Kind::Request)?;
                }
            }
            Event::Response { id, to_request, .. } => {
                link(&self.ids, id, "to_request", to_request, Kind::Request)?;
            }
            Event::Fork { of_turn, .. } => self.require_turn(*of_turn)?,
            Event::Capture { id, from_fork, .. } => {
                link(&self.ids, id, "from_fork", from_fork, Kind::Fork)?;
            }
            Event::Seam { at_turn, .. } | Event::ToolCall { at_turn, .. } => {
                self.require_turn(*at_turn)?;
            }
            Event::Rejected {
                id,
                at_turn,
                grounded,
                of,
                ..
            } => {
                self.require_turn(*at_turn)?;
                // A rejection is a lane that FAILED the floor. Recorded at a
                // perfect score, or over an empty lane, it is a row the gate
                // that writes it can never produce -- and a row nothing can
                // produce is a row nothing can be checked against.
                if of.get() == 0 || grounded >= of {
                    return Err(StructureError::NotARejection {
                        id: id.clone(),
                        grounded: grounded.get(),
                        of: of.get(),
                    }
                    .into());
                }
            }
            Event::Claim {
                id,
                consumes,
                supersedes,
                ..
            } => self.admit_claim(id, consumes, supersedes.as_deref())?,
            Event::Summary {
                turns,
                prefill_tokens_total,
                product_sha256,
            } => self.admit_summary(*turns, *prefill_tokens_total, product_sha256)?,
            Event::Start { .. } => {}
        }
        Ok(())
    }

    fn require_turn(&self, index: u32) -> Result<(), ParseError> {
        if self.turns.contains(&index) {
            Ok(())
        } else {
            Err(StructureError::UnknownTurn(index).into())
        }
    }

    fn admit_claim(
        &self,
        id: &str,
        consumes: &[Artifact],
        supersedes: Option<&str>,
    ) -> Result<(), ParseError> {
        // Recompute-sufficiency, as a rule rather than a habit. A claim that
        // names nothing it consumes can be re-read but not re-derived, and a
        // number nobody can re-derive is a number nobody can check.
        if consumes.is_empty() {
            return Err(StructureError::ClaimConsumesNothing(id.to_owned()).into());
        }
        for artifact in consumes {
            if !digest_ok(&artifact.sha256) {
                return Err(StructureError::BadDigest(artifact.sha256.clone()).into());
            }
        }
        if let Some(superseded) = supersedes {
            link(&self.ids, id, "supersedes", superseded, Kind::Claim)?;
        }
        Ok(())
    }

    /// The totals are already in hand, so a summary that disagrees with the
    /// rows it summarises costs one comparison -- and an expensive omission,
    /// because a report's front-matter numbers are verified against THIS row
    /// rather than against the rows themselves. An inconsistent summary would
    /// launder a wrong number into a green results gate.
    fn admit_summary(
        &self,
        turns: u32,
        prefill_tokens_total: Count,
        product_sha256: &str,
    ) -> Result<(), ParseError> {
        if !digest_ok(product_sha256) {
            return Err(StructureError::BadDigest(product_sha256.to_owned()).into());
        }
        let counted = self.next_turn - 1;
        if turns != counted {
            return Err(StructureError::SummaryDisagrees {
                field: "turns",
                says: u64::from(turns),
                counted: u64::from(counted),
            }
            .into());
        }
        if prefill_tokens_total != self.prefill_tokens {
            return Err(StructureError::SummaryDisagrees {
                field: "prefill_tokens_total",
                says: prefill_tokens_total.get(),
                counted: self.prefill_tokens.get(),
            }
            .into());
        }
        Ok(())
    }
}

/// The rules that hold across rows rather than within one.
///
/// Every link is checked against events ALREADY SEEN, never against the whole
/// record and never against the row holding the link. A lineage that can only
/// be resolved by reading ahead cannot be walked while the record is being
/// written, which is the only time anyone would want to walk it.
fn validate(events: &[Event]) -> Result<(), ParseError> {
    let mut regime = None;
    let mut seen = Seen::new();
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
        seen.admit(event)?;
        seen.claim_id(event)?;
        summary_seen |= matches!(event, Event::Summary { .. });
    }

    if regime.is_none() {
        return Err(StructureError::NoStart.into());
    }
    Ok(())
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
        ("regime".to_owned(), regime_value(record.regime())),
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
        Event::Rejected {
            id,
            lane,
            at_turn,
            grounded,
            of,
        } => {
            put("id", Value::String(id.clone()));
            put("lane", Value::String(lane.clone()));
            put("at_turn", Value::Integer(i64::from(*at_turn)));
            put("grounded", integer(*grounded));
            put("of", integer(*of));
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
            put("consumes", artifacts_value(consumes));
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

/// The artifacts a claim consumes, as the value space.
fn artifacts_value(consumes: &[Artifact]) -> Value {
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
    )
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

/// A count as the value space holds it.
///
/// Total, and not by clamping: [`Count`] cannot hold a value the value space
/// cannot spell, so there is nothing here to saturate.
fn integer(count: Count) -> Value {
    Value::Integer(i64::try_from(count.get()).unwrap_or(i64::MAX))
}

#[cfg(test)]
mod tests {
    use super::json::Value;
    use super::{
        Count, Event, Kind, ParseError, Reasoning, Regime, SchemaError, StructureError, Verdict,
        parse, regime_value, render,
    };

    /// A `start` line whose regime is complete, as every record needs one.
    const START: &str = r#"{"record":"start","regime":{"arm":"baseline","dogma_version":0,"substrate":{"name":"local","model":"a-model","quantization":"q4","sampler":{"seed":7,"temperature":0.7},"reasoning":"on","hardware":"one-gpu"}}}"#;

    fn record(rest: &str) -> String {
        format!("{START}\n{rest}")
    }

    #[test]
    fn the_regime_comes_from_the_required_start() {
        let parsed = parse(&record("")).expect("a record");
        assert_eq!(parsed.regime().arm, "baseline");
        assert_eq!(parsed.regime().substrate.name, "local");
        assert_eq!(parsed.regime().substrate.reasoning, Reasoning::On);
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
                 \"sampler\":{{\"seed\":0}},\"reasoning\":\"{}\",\"hardware\":\"h\"}}}}}}\n",
                state.tag()
            );
            let parsed = parse(&source).expect("a record");
            assert_eq!(parsed.regime().substrate.reasoning, *state);
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
    // trio. Three things were wrong with the first version of this test, and
    // an adversarial review found all three:
    //
    //   * it compared against `Regime::TAGS`, a hand-written const with no
    //     link to the struct, so a field added to `Regime` did not surface;
    //   * it scanned the whole file, so the table could be moved OUT of the
    //     front matter and the test still passed;
    //   * it left a third copy of the trio, in check-results.py, unjoined.
    //
    // The trio now comes from `regime_value`, which is the renderer -- a field
    // added to `Regime` has to be rendered or the round-trip test fails, so
    // the struct and the tags are joined by code rather than by hand. Both
    // other copies are compared against it, and both scans are anchored.
    fn front_matter_of(path: &std::path::Path) -> String {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
        let body = text
            .strip_prefix("+++\n")
            .unwrap_or_else(|| panic!("{} does not open with front matter", path.display()));
        let (front, _) = body
            .split_once("\n+++")
            .unwrap_or_else(|| panic!("{} has unclosed front matter", path.display()));
        front.to_owned()
    }

    #[test]
    fn the_regime_trio_is_what_the_renderer_writes() {
        let source = format!("{START}\n");
        let record = parse(&source).expect("a record");
        let Value::Object(members) = regime_value(record.regime()) else {
            panic!("a regime renders to an object");
        };
        // Sets, not sequences: the renderer sorts its keys and a TOML table
        // has no order that means anything. What is being compared is WHICH
        // keys, which is the whole content of "mirrors".
        let rendered: std::collections::BTreeSet<&str> =
            members.keys().map(String::as_str).collect();
        let declared: std::collections::BTreeSet<&str> = Regime::TAGS.iter().copied().collect();
        assert_eq!(
            rendered, declared,
            "the renderer and the declared trio disagree"
        );
    }

    #[test]
    fn the_report_front_matter_mirrors_the_regime_trio() {
        let template =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../results/_template/README.md");
        let front = front_matter_of(&template);
        let Some((_, after)) = front.split_once("\n[regime]\n") else {
            panic!(
                "{} has no [regime] table IN ITS FRONT MATTER",
                template.display()
            )
        };
        let keys: std::collections::BTreeSet<&str> = after
            .lines()
            .take_while(|line| !line.starts_with('['))
            .filter_map(|line| line.split_once('=').map(|(key, _)| key.trim()))
            .collect();
        assert!(!keys.is_empty(), "the [regime] table binds nothing");
        let declared: std::collections::BTreeSet<&str> = Regime::TAGS.iter().copied().collect();
        assert_eq!(
            keys, declared,
            "the report's [regime] table and the schema's regime disagree"
        );
    }

    // The third copy. `check-results.py` reads the same table with its own
    // list, and a key added to one without the other is a divergence between
    // two gates about one file.
    #[test]
    fn the_report_linter_requires_the_same_regime_trio() {
        let script =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/check-results.py");
        let text = std::fs::read_to_string(&script)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", script.display()));
        let (_, after) = text
            .split_once("REQUIRED_REGIME_KEYS")
            .unwrap_or_else(|| panic!("{} declares no REQUIRED_REGIME_KEYS", script.display()));
        let (list, _) = after
            .split_once('}')
            .unwrap_or_else(|| panic!("{}: unterminated REQUIRED_REGIME_KEYS", script.display()));
        let declared: std::collections::BTreeSet<&str> = list
            .split('"')
            .filter(|piece| piece.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
            .filter(|piece| !piece.is_empty())
            .collect();
        let ours: std::collections::BTreeSet<&str> = Regime::TAGS.iter().copied().collect();
        assert_eq!(
            declared, ours,
            "check-results.py and the schema disagree about the regime trio"
        );
    }

    // A self-link is a cycle of length one -- the only cycle a backwards-only
    // rule does not already exclude -- and a claim that supersedes itself IS a
    // compound row: one cell holding both the retraction and the replacement.
    #[test]
    fn nothing_can_link_to_itself() {
        let retry = record(concat!(
            r#"{"record":"request","id":"q1","lane":"main","retry_of":"q1"}"#,
            "\n"
        ));
        assert!(matches!(
            parse(&retry),
            Err(ParseError::Structure(StructureError::DanglingLink { .. }))
        ));
        let digest = "a".repeat(64);
        let correction = record(&format!(
            "{{\"record\":\"claim\",\"id\":\"c1\",\"hypothesis\":\"h\",\"result\":\"refuted\",\
             \"consumes\":[{{\"path\":\"p\",\"sha256\":\"{digest}\"}}],\"supersedes\":\"c1\"}}\n"
        ));
        assert!(matches!(
            parse(&correction),
            Err(ParseError::Structure(StructureError::DanglingLink { .. }))
        ));
    }

    // A crash is not a verdict. Without a depth limit, recursive descent runs
    // out of stack and ABORTS the process on about two kilobytes of input --
    // and this format's own corpus says a format must return a verdict on
    // arbitrary bytes rather than crash on them.
    #[test]
    fn a_deeply_nested_document_is_a_verdict_and_not_a_crash() {
        let deep = format!(
            "{{\"record\":\"start\",\"regime\":{}1{}}}\n",
            "{\"a\":".repeat(5000),
            "}".repeat(5000)
        );
        assert!(
            matches!(parse(&deep), Err(ParseError::TooDeep { .. })),
            "5000 deep must be a verdict"
        );
        assert!(
            parse(&record("")).is_ok(),
            "the limit does not reject a record"
        );
    }

    // Blank is absent with the key still there. A required field satisfied by
    // typing two quotes buys presence rather than provenance.
    #[test]
    fn a_required_string_that_says_nothing_is_absent() {
        let blank = r#"{"record":"start","regime":{"arm":"","dogma_version":0,"substrate":{"name":"n","model":"m","quantization":"q","sampler":{"seed":0},"reasoning":"on","hardware":"h"}}}"#;
        assert!(matches!(
            parse(blank),
            Err(ParseError::Schema(SchemaError::BlankField { .. }))
        ));
        let no_settings = r#"{"record":"start","regime":{"arm":"a","dogma_version":0,"substrate":{"name":"n","model":"m","quantization":"q","sampler":{},"reasoning":"on","hardware":"h"}}}"#;
        assert!(
            matches!(
                parse(no_settings),
                Err(ParseError::Schema(SchemaError::BlankField { .. }))
            ),
            "a substrate that declines to say which sampler settings is a partial regime"
        );
    }

    // A report's front-matter numbers are verified against the summary row
    // rather than against the rows, so an inconsistent summary would launder a
    // wrong number into a green results gate.
    #[test]
    fn a_summary_must_summarise_the_rows_above_it() {
        let digest = "b".repeat(64);
        let source = record(&format!(
            "{{\"record\":\"turn\",\"index\":1,\"prefill_tokens\":10}}\n\
             {{\"record\":\"summary\",\"turns\":99,\"prefill_tokens_total\":10,\
             \"product_sha256\":\"{digest}\"}}\n"
        ));
        assert!(matches!(
            parse(&source),
            Err(ParseError::Structure(StructureError::SummaryDisagrees {
                field: "turns",
                ..
            }))
        ));
    }

    #[test]
    fn negative_zero_is_a_second_spelling_and_is_refused() {
        let source = record("{\"record\":\"turn\",\"index\":1,\"prefill_tokens\":-0}\n");
        assert!(parse(&source).is_err(), "one spelling per value");
    }

    // A count the value space cannot spell has no business existing. Rendering
    // used to clamp it to i64::MAX under a comment claiming the result would
    // round-trip loudly; it read back as an ordinary count instead.
    #[test]
    fn a_count_above_what_the_value_space_can_spell_cannot_be_built() {
        assert!(Count::new(Count::MAX).is_ok());
        assert!(Count::new(u64::MAX).is_err());
        assert_eq!(Count::new(7).expect("a count").get(), 7);
    }

    #[test]
    fn a_raw_newline_inside_a_string_does_not_parse() {
        let source = "{\"record\":\"start\",\"regime\":{\"arm\":\"a\nb\"}}";
        assert!(parse(source).is_err(), "one event, one line");
    }
}
