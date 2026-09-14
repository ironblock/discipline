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
#[grammar = "../formats/number.pest"]
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

vocabulary! {
    /// How a substrate's weights are identified.
    ///
    /// TWO WAYS, TYPED, because there are two and the difference decides what
    /// a result may claim. Never one field that is sometimes a digest and
    /// sometimes a name: that is identity spelled two ways in one place, and
    /// the reader cannot tell which it got.
    WeightsKind {
        /// Weights on disk, identified by what they are.
        Digest => "digest",
        /// Weights behind an endpoint, identified by who serves them and what
        /// they are called.
        Hosted => "hosted",
        /// No weights at all: a canned server replaying authored acts,
        /// identified by the digest of the acts it plays.
        Canned => "canned",
    }
}

/// Which weights a substrate ran, and how they are identified.
///
/// A HOSTED MODEL IS A SUBSTRATE WHOSE WEIGHTS CAN CHANGE UNDER YOU -- the
/// engine's re-pointed tag, at scale -- so the variants are not spellings of
/// one thing. Gate 0 re-derives a result from committed artifacts and is
/// indifferent to which this is; gate 1 re-fires it on a declared substrate
/// and cannot, so [`Weights::is_reproducible`] is what `check-results.py`
/// asks before letting a directory call itself `reproducible-by-config`.
///
/// THE THREE ARE REPRODUCED BY DIFFERENT MECHANISMS, which is why a kind is
/// not decoration on a digest:
///
/// - [`Weights::Digest`] is re-fired on the declared hardware and compared
///   within a pre-registered band, because sampling and hardware move.
/// - [`Weights::Canned`] is REPLAYED, and compared exactly. There is no band,
///   because there is nothing to vary: the same acts play again, byte for
///   byte. It is the strongest form of reproducible-by-config and not the
///   same claim as parity.
/// - [`Weights::Hosted`] is neither. It may be observed and never certified.
///
/// Ruled 2026-09-08, and extended 2026-09-11. The alternative refused twice
/// is one field whose meaning depends on what happens to be in it: first a
/// `weights_digest` that also accepts a name, then a canned server's acts
/// digest read through `Digest`. The second is the subtler one -- it makes
/// `Digest` mean weights OR replay acts, told apart only by knowing the
/// engine's name, which is a label standing in for a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Weights {
    /// Weights on disk, by sha256.
    Digest(String),
    /// Weights behind an endpoint.
    Hosted {
        /// Who serves them.
        provider: String,
        /// What they call the model.
        model_id: String,
        /// The version if the provider publishes one, else the date the run
        /// observed. Either way it is what a later reader compares against;
        /// neither is a guarantee, which is the point.
        version_or_date_observed: String,
    },
    /// No weights: a canned server replaying acts written in advance.
    Canned {
        /// The digest of the acts it plays, which is its whole identity.
        ///
        /// A canned server has no weights and that is not the same as having
        /// nothing identifiable: the acts decided every reply, they are an
        /// artifact somebody can hold, and this is their sha256.
        acts_sha256: String,
    },
}

impl Weights {
    /// Which kind these are.
    #[must_use]
    pub fn kind(&self) -> WeightsKind {
        match self {
            Self::Digest(_) => WeightsKind::Digest,
            Self::Hosted { .. } => WeightsKind::Hosted,
            Self::Canned { .. } => WeightsKind::Canned,
        }
    }

    /// Whether a run on these weights can be re-fired and expected to match.
    ///
    /// False for hosted weights, and that is the whole reason this type is a
    /// vocabulary rather than a string. A provider can re-point a tag without
    /// telling anyone, so parity on a hosted substrate is a claim nobody can
    /// keep -- a result under one may be `historical-observation` and never
    /// `reproducible-by-config`.
    #[must_use]
    pub fn is_reproducible(&self) -> bool {
        // Canned is reproducible BY REPLAY rather than by re-firing, and it is
        // the stronger of the two: the acts play again exactly. See
        // [`Weights::is_replayed`] for the half that decides HOW a gate
        // compares, which is a different question from whether it may.
        matches!(self, Self::Digest(_) | Self::Canned { .. })
    }

    /// Whether reproducing this means replaying acts rather than re-firing.
    ///
    /// Gate 1 compares a replay EXACTLY and a re-firing within a band. Asking
    /// this rather than reading the engine's name is the whole reason the kind
    /// exists.
    #[must_use]
    pub fn is_replayed(&self) -> bool {
        matches!(self, Self::Canned { .. })
    }
}

/// The stack that served a substrate's weights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    /// The serving stack, as it calls itself.
    pub name: String,
    /// Which build of it. A version when the stack publishes one, a digest
    /// when it does not -- #47's own spelling, and the reason the field is
    /// named for both rather than for the one that happened to be available.
    pub version_or_digest: String,
}

/// One thing a run was served by, declared once and referenced by id.
///
/// Was a single `substrate` on the regime. It is a list now because a run can
/// have more than one -- a drive whose interview fork is served by a small
/// local model while the main lane is served by a large one is the case this
/// repository is built to measure, and a single substrate could not say it.
///
/// Identity is the WEIGHTS, typed, not a name. A name is prose: two runs can
/// spell the same weights differently and a third can spell different weights
/// the same, and then a regime comparison compares strings. A digest cannot do
/// either, and the hosted variant does not pretend to -- it says who served
/// them and declines to claim more, which is why nothing under it may claim
/// parity. See [`Weights`].
///
/// Every field is required. An optional substrate field is a substrate field
/// that will be absent exactly when it matters -- the run whose result
/// surprises someone is the run nobody thought to tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Substrate {
    /// What rows reference. Unique within a run; nothing outside it.
    pub id: String,
    /// What served the weights.
    pub engine: Engine,
    /// Which weights, and how they are identified.
    pub weights: Weights,
    /// A fingerprint for the hardware it was served from.
    pub hardware_fingerprint: String,
    /// Sampler settings, exactly as they were set.
    pub sampler_card: BTreeMap<String, Value>,
    /// Whether reasoning was on, and whether it came back.
    ///
    /// Not in #47's list of substrate fields and kept anyway: it is a property
    /// of how these weights were run, two valid fixtures pin it, and dropping
    /// it would be a distinction the schema stopped being able to make.
    pub reasoning: Reasoning,
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
    /// What served it, declared once. At least one, ids unique.
    pub substrates: Vec<Substrate>,
    /// Which version of the dogma was in force.
    pub dogma_version: u32,
}

impl Regime {
    /// The keys a report's front-matter `[regime]` table must mirror.
    ///
    /// Named here rather than in the report linter so that the schema is the
    /// definition and the report is the mirror, which is the direction the
    /// two are supposed to run in.
    pub const TAGS: &'static [&'static str] = &["arm", "substrates", "dogma_version"];

    /// Whether this run declared a substrate under that id.
    #[must_use]
    pub fn declares(&self, id: &str) -> bool {
        self.substrates.iter().any(|s| s.id == id)
    }

    /// Every declared id, in declaration order.
    #[must_use]
    pub fn substrate_ids(&self) -> Vec<&str> {
        self.substrates.iter().map(|s| s.id.as_str()).collect()
    }

    /// The declared substrates nothing can re-fire, by id.
    ///
    /// Empty for a run served entirely by weights on disk. Non-empty is what
    /// stops a results directory calling itself `reproducible-by-config`: the
    /// provider can re-point a tag without telling anyone, so parity under
    /// one of these is a claim nobody can keep. Gate 0 does not read this --
    /// re-deriving numbers from committed artifacts is indifferent to what
    /// served them. Ruled 2026-09-08.
    #[must_use]
    pub fn hosted_substrate_ids(&self) -> Vec<&str> {
        self.substrates
            .iter()
            // MATCHED, not inferred from `!is_reproducible()`. With two
            // variants those were the same set; with three they are only the
            // same until a fourth, and this list is named for hosted
            // substrates rather than for whatever is left over.
            .filter(|s| matches!(s.weights, Weights::Hosted { .. }))
            .map(|s| s.id.as_str())
            .collect()
    }
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
        /// Which declared substrate it was put to, by id.
        ///
        /// Required, and required even when the run declares exactly one:
        /// a row that may omit it is a row whose substrate was decided
        /// somewhere else, which is the thing declaring them exists to end.
        /// A `response` inherits this through its `to_request`, and a retry
        /// is a new request row, so a retry served by a different substrate
        /// is already expressible without a second place to say so.
        substrate: String,
        /// The request this one retries, if it is a retry. A retry is a new
        /// request that names its predecessor; it is not an annotation on the
        /// old one, because the old one already happened.
        retry_of: Option<String>,
        /// What was sent, when the record keeps it.
        ///
        /// A record that carries the texts is an ARCHIVE of the drive: it can
        /// be replayed through a router, mined for the asks that invited an
        /// echo, matched against the object's entries. One without them is a
        /// ledger, and a ledger is still a record -- provenance never
        /// depended on the payload being kept, only on its being named.
        text: Option<String>,
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
        /// What came back, when the record keeps it. On the canonical lane
        /// this is the model's own prose for the turn; on an interview lane it
        /// is the answer. It may be empty: an answer of nothing at all is a
        /// typed outcome, and a record that could not spell it would be a
        /// record that could not show the collapse it exists to catch.
        text: Option<String>,
    },
    /// An interview fork was opened.
    Fork {
        /// This fork's identifier.
        id: String,
        /// Which lane it belongs to.
        lane: String,
        /// Which declared substrate the fork was served by, by id.
        ///
        /// The field that makes the point of declaring substrates: a fork
        /// served by a small local model while the main lane runs a large one
        /// is the arrangement this repository is built to measure.
        substrate: String,
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
        /// The arguments it was called with, when the record keeps them. The
        /// mechanical lane derives the working directory and the files
        /// touched from these, and an entry it writes grounds itself by
        /// naming this row's id -- which is only grounding if the row holds
        /// what the entry was derived from.
        args: Option<BTreeMap<String, Value>>,
        /// The exit status the tool reported, when the record keeps it.
        exit: Option<i64>,
        /// What the tool returned, when the record keeps it. May be empty: a
        /// command that printed nothing is a fact about the command.
        output: Option<String>,
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
        ///
        /// And, by reference, which substrate: a lane is a role on a
        /// substrate and cannot change it mid-run, so [`Record::substrate_of`]
        /// answers for this row. No `substrate` field here on purpose -- it
        /// would be the lane's fact written a second time, in the one place
        /// that could contradict it. Ruled 2026-09-08.
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
    /// What the run amounted to, in the terms its own kind is measured in.
    Summary {
        /// Which kind of run this was, and the totals that kind has.
        summary: Summary,
        /// The digest of the product the run produced.
        ///
        /// HERE AND NOT IN A KIND, because binary provenance is universal: a
        /// recompute record was produced by a `diet` binary exactly as a drive
        /// record was, and the ruling that made the digest mandatory at the
        /// boundary did not exempt one of them. It lived inside
        /// [`Summary::Drive`] until 2026-09-11, which made every results
        /// directory whose record was a recompute impossible to lint --
        /// `check-results.py` requires `product_sha256` in every front-matter
        /// and requires it to equal the summary's, and a recompute summary had
        /// nowhere to put one. Duplicating the field across variants would
        /// have been one name meaning a thing in two places; teaching the
        /// linter which kinds carry it would have been a second reader of this
        /// schema. Ruled (a) on #68.
        product_sha256: String,
    },
}

// ---------------------------------------------------------------------------
// summaries
// ---------------------------------------------------------------------------

vocabulary! {
    /// What a run was, which decides what its summary can say.
    ///
    /// `replay` is named by #47 and is deliberately NOT here: the issue says
    /// its fields are "to be stated", and a variant with no stated fields is
    /// either a guess about what a replay measures or a variant nothing can
    /// construct. Both are worse than refusing the word until it means
    /// something, and refusing is the direction this format is allowed to
    /// grow in.
    SummaryKind {
        /// A session that drove a model: turns, prefill, and a product.
        Drive => "drive",
        /// A re-derivation: how many targets were checked, how many matched,
        /// and the digests that say so.
        Recompute => "recompute",
    }
}

/// A run's totals, in the terms of the kind of run it was.
///
/// An enum with per-kind fields rather than one struct with optionals, for
/// the reason `Patch` gives one module over: a summary that could be any of
/// these depending on which fields happen to be set is a summary whose
/// meaning is decided at the call site rather than by the schema.
///
/// This is also what refuses a sentinel. #47 asks for "sentinel numbers
/// meaning 'not applicable'" to be refused, and the way to refuse them is not
/// to check for -1: it is to leave nowhere to put one. A recompute summary
/// has no `turns` field, so `turns: -1` is an unknown key and the schema
/// already refuses those by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Summary {
    /// A session that drove a model.
    Drive {
        /// How many turns.
        turns: u32,
        /// Prefill tokens across the session.
        prefill_tokens_total: Count,
    },
    /// A re-derivation of a result that already exists.
    Recompute {
        /// How many targets the recompute set out to check.
        targets_checked: u32,
        /// How many of them matched. Never more than were checked.
        targets_matched: u32,
        /// The digests it compared, in the order it compared them.
        digests: Vec<String>,
    },
}

impl Summary {
    /// Which kind this summary is.
    #[must_use]
    pub fn kind(&self) -> SummaryKind {
        match self {
            Self::Drive { .. } => SummaryKind::Drive,
            Self::Recompute { .. } => SummaryKind::Recompute,
        }
    }
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

    /// Which substrate served `lane`, if any row named one.
    ///
    /// A LANE IS A ROLE ON A SUBSTRATE, and [`validate`] refuses a record
    /// whose rows say otherwise, so there is at most one answer and this
    /// cannot pick. It is what a `rejected` row's substrate IS: the row does
    /// not carry the field, it inherits it from its lane, and the inheritance
    /// is this function rather than a sentence each reader implements again.
    ///
    /// Ruled 2026-09-08. The alternative was a `substrate` on `rejected`,
    /// which is the same fact written twice and therefore a fact that can
    /// disagree with itself.
    #[must_use]
    pub fn substrate_of(&self, lane: &str) -> Option<&str> {
        self.events.iter().find_map(|event| match event {
            Event::Request {
                lane: on,
                substrate,
                ..
            }
            | Event::Fork {
                lane: on,
                substrate,
                ..
            } if on == lane => Some(substrate.as_str()),
            _ => None,
        })
    }
}

// ---------------------------------------------------------------------------
// errors
// ---------------------------------------------------------------------------

/// The deepest nesting a record may carry.
///
/// A record's own deepest legitimate shape is five -- `regime.substrates[].
/// sampler_card.<setting>` -- so this is generous by an order of magnitude and
/// still far below where recursive descent runs out of stack. Without it a
/// 2 KB file of nested objects ABORTS the process, and this format's own
/// corpus says, in `invalid/not-utf8.reason`, that a format must return a
/// verdict on arbitrary bytes rather than crash on them. The sampler card is
/// the arrival vector: the one untyped, arbitrarily-nested field here.
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
    /// A summary whose two totals cannot both be true of each other.
    ///
    /// Deliberately not [`Self::SummaryDisagrees`], which means "you said X
    /// and I counted Y". Nothing counts a recompute's targets -- the reader
    /// sees no target rows -- so borrowing that variant would print "the rows
    /// above it add up to 1" about a 1 that came from the same row, and a
    /// message that names the wrong evidence sends the next reader to the
    /// wrong place.
    SummaryImpossible {
        /// The total that cannot be that large.
        field: &'static str,
        /// What it claims.
        says: u64,
        /// The field that bounds it.
        bound: &'static str,
        /// What that field claims.
        limit: u64,
    },
    /// Two substrates declared under one id.
    SubstrateDeclaredTwice(String),
    /// A lane whose rows name two different substrates.
    LaneChangedSubstrate {
        /// The lane that changed.
        lane: String,
        /// What its first row named.
        was: String,
        /// What this row names.
        now: String,
    },
    /// A row naming a substrate the run never declared.
    UndeclaredSubstrate {
        /// The row that names it.
        row: &'static str,
        /// The id it names.
        id: String,
        /// What the run did declare, so the reader can see the typo.
        declared: Vec<String>,
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
            Self::SummaryImpossible {
                field,
                says,
                bound,
                limit,
            } => write!(
                f,
                "the summary's `{field}` says {says} of the {limit} its \
                 `{bound}` says were looked at"
            ),
            Self::SubstrateDeclaredTwice(id) => write!(
                f,
                "two substrates are declared as `{id}`, so every row naming it \
                 would have to be read as one of them"
            ),
            Self::LaneChangedSubstrate { lane, was, now } => write!(
                f,
                "the `{lane}` lane is served by `{was}` and then by `{now}`, \
                 and a lane is a role on a substrate: a lane that changes \
                 substrate is two lanes, and a row that inherits its lane's \
                 substrate would have two answers"
            ),
            Self::UndeclaredSubstrate { row, id, declared } => write!(
                f,
                "a `{row}` row is served by `{id}`, which this run does not \
                 declare; it declares {}",
                if declared.is_empty() {
                    "none".to_owned()
                } else {
                    declared
                        .iter()
                        .map(|d| format!("`{d}`"))
                        .collect::<Vec<_>>()
                        .join(", ")
                }
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

/// Read a JSON-lines document as plain objects, stopping before the schema.
///
/// A lane's contract file and a lane corpus's expectation are JSON that is
/// not a record: rows with no `record` tag and no event schema to satisfy.
/// They are read here rather than by a second decoder because the crate's
/// standing finding about formats applies to its own value space first --
/// eleven divergent readers of one format, and "I recreated it to be quick".
/// This is the same grammar and the same [`json`] value space as a record,
/// with the schema layer left off.
///
/// # Errors
///
/// Returns [`ParseError`] for text the grammar rejects, for nesting past
/// [`MAX_DEPTH`], and for a value the record's value space cannot hold. Never
/// [`ParseError::Schema`] or [`ParseError::Structure`]: those layers are
/// exactly what this function does not apply.
pub fn objects(input: &str) -> Result<Vec<BTreeMap<String, Value>>, ParseError> {
    let depth = nesting_depth(input);
    if depth > MAX_DEPTH {
        return Err(ParseError::TooDeep {
            depth,
            limit: MAX_DEPTH,
        });
    }
    let mut parsed = RecordParser::parse(Rule::document, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?;
    let document = parsed.next().ok_or(ParseError::Value(ValueError::Shape(
        "a document with no rows",
    )))?;

    let mut rows = Vec::new();
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
        rows.push(json::object(&object)?);
    }
    Ok(rows)
}

/// Parse a session record.
///
/// # Errors
///
/// Returns [`ParseError`] naming which of the four layers rejected it: the
/// grammar, the value space, one row's schema, or the record's structure. The
/// layers are kept apart because "this is not a record" is not a useful thing
/// to be told.
pub fn parse(input: &str) -> Result<Record, ParseError> {
    if let Some(depth) = too_deep(input) {
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

/// How deep `input` nests, when that is deeper than [`MAX_DEPTH`].
///
/// The limit is applied here and nowhere else. Every public reader of this
/// grammar goes through this function before handing text to the parser,
/// because two readers with two answers to how deep is too deep is one reader
/// that returns a verdict and one that aborts the process -- and which of the
/// two a caller reached would depend on which module it happened to import.
fn too_deep(input: &str) -> Option<usize> {
    let depth = nesting_depth(input);
    if depth > MAX_DEPTH {
        return Some(depth);
    }
    None
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
            substrate: take_string(&mut members, of, "substrate")?,
            retry_of: take_optional_string(&mut members, of, "retry_of")?,
            text: take_optional_text(&mut members, of, "text")?,
        },
        Kind::Response => Event::Response {
            id: take_string(&mut members, of, "id")?,
            to_request: take_string(&mut members, of, "to_request")?,
            output_tokens: take_u64(&mut members, of, "output_tokens")?,
            text: take_optional_text(&mut members, of, "text")?,
        },
        Kind::Fork => Event::Fork {
            id: take_string(&mut members, of, "id")?,
            lane: take_string(&mut members, of, "lane")?,
            substrate: take_string(&mut members, of, "substrate")?,
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
            args: take_optional_object(&mut members, of, "args")?,
            exit: take_optional_integer(&mut members, of, "exit")?,
            output: take_optional_text(&mut members, of, "output")?,
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
            summary: summary(&mut members, of)?,
            // Read here rather than inside `summary`, because it belongs to
            // the row and not to the kind: every summary carries it, and a
            // reader that took it per-kind would have to be told twice.
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

/// A summary, from a `summary` row's members.
///
/// `kind` first, then only that kind's fields. Anything else is left in
/// `members` and refused by the caller as an unknown key -- which is what
/// makes `turns` on a recompute summary an error rather than a number nobody
/// reads. Its own function rather than an arm of [`event`] because the arm is
/// the only one that dispatches on a second vocabulary, and a reader looking
/// for what a summary may say should not have to read ten other kinds first.
fn summary(members: &mut BTreeMap<String, Value>, of: &'static str) -> Result<Summary, ParseError> {
    let tag = take_string(members, of, "kind")?;
    let kind = SummaryKind::from_tag(&tag).ok_or(SchemaError::BadValue {
        of,
        field: "kind",
        found: tag,
    })?;
    Ok(match kind {
        SummaryKind::Drive => Summary::Drive {
            turns: take_u32(members, of, "turns")?,
            prefill_tokens_total: take_u64(members, of, "prefill_tokens_total")?,
        },
        SummaryKind::Recompute => Summary::Recompute {
            targets_checked: take_u32(members, of, "targets_checked")?,
            targets_matched: take_u32(members, of, "targets_matched")?,
            digests: take_digests(members, of)?,
        },
    })
}

/// The regime, from a `start` row's `regime` object.
fn regime(members: &mut BTreeMap<String, Value>, of: &'static str) -> Result<Regime, ParseError> {
    let arm = take_string(members, of, "arm")?;
    let dogma_version = take_u32(members, of, "dogma_version")?;
    let substrates = substrates(members, of)?;
    if let Some(field) = members.keys().next() {
        return Err(SchemaError::UnknownField {
            of,
            field: format!("regime.{field}"),
        }
        .into());
    }
    Ok(Regime {
        arm,
        substrates,
        dogma_version,
    })
}

/// The substrates a run declares, from the regime's `substrates` list.
///
/// A single-substrate run declares a list of one. Not a shorthand for it: a
/// run whose rows may omit the reference when there is only one is a run
/// whose rows mean different things depending on a count elsewhere in the
/// file, and the reference is what this whole item exists to make explicit.
fn substrates(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
) -> Result<Vec<Substrate>, ParseError> {
    let Some(value) = members.remove("substrates") else {
        return Err(SchemaError::MissingField {
            of,
            field: "substrates",
        }
        .into());
    };
    let Value::Array(items) = value else {
        return Err(SchemaError::WrongType {
            of,
            field: "substrates".to_owned(),
            want: "a list of substrates",
        }
        .into());
    };
    // A run served by nothing is not a run. The empty list satisfies the type
    // and says nothing, which is the shape the empty sampler card was refused
    // for one field down.
    if items.is_empty() {
        return Err(SchemaError::BlankField {
            of,
            field: "regime.substrates",
        }
        .into());
    }
    let mut declared: Vec<Substrate> = Vec::with_capacity(items.len());
    for item in items {
        let Value::Object(mut fields) = item else {
            return Err(SchemaError::WrongType {
                of,
                field: "substrates[]".to_owned(),
                want: "a substrate",
            }
            .into());
        };
        let one = substrate(&mut fields, of)?;
        // Two substrates under one id would make every reference to it
        // ambiguous, and the reader would have to pick. It refuses instead.
        if declared.iter().any(|s| s.id == one.id) {
            return Err(StructureError::SubstrateDeclaredTwice(one.id).into());
        }
        declared.push(one);
    }
    Ok(declared)
}

/// Which weights a substrate ran, from its `weights` object.
///
/// KIND FIRST, then only that kind's fields -- the shape [`summary`] uses,
/// and for the same reason: a field that belongs to the other variant is left
/// in `fields` and refused by the caller as unknown, so a hosted substrate
/// carrying a `sha256` is an error rather than a digest nobody reads.
fn weights(fields: &mut BTreeMap<String, Value>, of: &'static str) -> Result<Weights, ParseError> {
    let mut members = take_object(fields, of, "weights")?;
    let tag = take_string(&mut members, of, "kind")?;
    let kind = WeightsKind::from_tag(&tag).ok_or(SchemaError::BadValue {
        of,
        field: "weights.kind",
        found: tag,
    })?;
    let built = match kind {
        WeightsKind::Digest => {
            // Identity, so it is checked as a digest rather than accepted as
            // a name. This is the variant that says what the weights ARE, and
            // a string that is not a digest cannot say it.
            let text = take_string(&mut members, of, "sha256")?;
            if !digest_ok(&text) {
                return Err(StructureError::BadDigest(text).into());
            }
            Weights::Digest(text)
        }
        // Not checked as a digest, because there is nothing to digest. Three
        // required strings and no identity claim: what a later reader has is
        // who served it and what they called it, which is why nothing under
        // these weights may claim parity.
        WeightsKind::Hosted => Weights::Hosted {
            provider: take_string(&mut members, of, "provider")?,
            model_id: take_string(&mut members, of, "model_id")?,
            version_or_date_observed: take_string(&mut members, of, "version_or_date_observed")?,
        },
        // Checked as a digest, like `Digest` and for the same reason: it is
        // an identity claim about an artifact, and a string that is not a
        // digest cannot make one. What it digests is the acts rather than
        // weights, which is exactly why it is not spelled `sha256` here --
        // two kinds sharing a field name is how the reader stops being able
        // to say which it got.
        WeightsKind::Canned => {
            let text = take_string(&mut members, of, "acts_sha256")?;
            if !digest_ok(&text) {
                return Err(StructureError::BadDigest(text).into());
            }
            Weights::Canned { acts_sha256: text }
        }
    };
    if let Some(field) = members.keys().next() {
        return Err(SchemaError::UnknownField {
            of,
            field: format!("substrates[].weights.{field}"),
        }
        .into());
    }
    Ok(built)
}

/// One substrate, from its object.
fn substrate(
    fields: &mut BTreeMap<String, Value>,
    of: &'static str,
) -> Result<Substrate, ParseError> {
    let id = take_string(fields, of, "id")?;
    if id.trim().is_empty() {
        return Err(SchemaError::BlankField {
            of,
            field: "substrates[].id",
        }
        .into());
    }
    let mut engine_fields = take_object(fields, of, "engine")?;
    let engine = Engine {
        name: take_string(&mut engine_fields, of, "name")?,
        version_or_digest: take_string(&mut engine_fields, of, "version_or_digest")?,
    };
    if let Some(field) = engine_fields.keys().next() {
        return Err(SchemaError::UnknownField {
            of,
            field: format!("substrates[].engine.{field}"),
        }
        .into());
    }
    let built = Substrate {
        id,
        engine,
        weights: weights(fields, of)?,
        hardware_fingerprint: take_string(fields, of, "hardware_fingerprint")?,
        sampler_card: {
            // A substrate whose sampler card is the empty object records that
            // the run had settings and declines to say which. The issue names
            // the card as one of the substrate's facets, and an empty map
            // satisfies the type while satisfying nothing else.
            let settings = take_object(fields, of, "sampler_card")?;
            if settings.is_empty() {
                return Err(SchemaError::BlankField {
                    of,
                    field: "substrates[].sampler_card",
                }
                .into());
            }
            settings
        },
        reasoning: {
            let text = take_string(fields, of, "reasoning")?;
            Reasoning::from_tag(&text).ok_or(SchemaError::BadValue {
                of,
                field: "reasoning",
                found: text,
            })?
        },
    };
    if let Some(field) = fields.keys().next() {
        return Err(SchemaError::UnknownField {
            of,
            field: format!("substrates[].{field}"),
        }
        .into());
    }
    Ok(built)
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

/// A `digests` list, with every entry checked to be one.
///
/// Modelled on `take_artifacts`. The per-entry check is here rather than in
/// the structural pass because a digest that is not a digest is a SHAPE
/// error: nothing downstream can compare it, and reporting it as a
/// disagreement about a number would name the wrong defect.
fn take_digests(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
) -> Result<Vec<String>, ParseError> {
    let Some(value) = members.remove("digests") else {
        return Err(SchemaError::MissingField {
            of,
            field: "digests",
        }
        .into());
    };
    let Value::Array(items) = value else {
        return Err(SchemaError::WrongType {
            of,
            field: "digests".to_owned(),
            want: "a list of sha256 digests",
        }
        .into());
    };
    let mut digests = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(text) = item else {
            return Err(SchemaError::WrongType {
                of,
                field: "digests[]".to_owned(),
                want: "a sha256 digest",
            }
            .into());
        };
        if !digest_ok(&text) {
            return Err(SchemaError::BadValue {
                of,
                field: "digests",
                found: text,
            }
            .into());
        }
        digests.push(text);
    }
    Ok(digests)
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

/// An optional payload: text that may legitimately be empty.
///
/// Distinct from [`take_optional_string`] on purpose. That one refuses a
/// blank because every string it reads is an identifier or a statement, and
/// a blank identifier is a required field satisfied by typing two quotes. A
/// payload is different: a response of no characters and a command that
/// printed nothing are facts, and the silence one of them records is the
/// collapse the ablation exists to measure. A record that could not hold an
/// empty answer would report the worst case as missing data.
fn take_optional_text(
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

/// An optional object. Empty is allowed: a tool called with no arguments has
/// an argument list, and it is empty.
fn take_optional_object(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
    field: &'static str,
) -> Result<Option<BTreeMap<String, Value>>, ParseError> {
    match members.remove(field) {
        Some(Value::Object(inner)) => Ok(Some(inner)),
        Some(_) => Err(SchemaError::WrongType {
            of,
            field: field.to_owned(),
            want: "an object",
        }
        .into()),
        None => Ok(None),
    }
}

/// An optional signed integer. An exit status may be anything the value
/// space can spell; a shell reports signals as numbers past 128, and a
/// harness that failed to run the tool at all may report a negative one.
fn take_optional_integer(
    members: &mut BTreeMap<String, Value>,
    of: &'static str,
    field: &'static str,
) -> Result<Option<i64>, ParseError> {
    match members.remove(field) {
        Some(Value::Integer(number)) => Ok(Some(number)),
        Some(_) => Err(SchemaError::WrongType {
            of,
            field: field.to_owned(),
            want: "an integer",
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
                summary,
                product_sha256,
            } => self.admit_summary(summary, product_sha256)?,
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
    /// What a summary must agree with, in the terms of its own kind.
    ///
    /// The cross-checks were written when there was one kind of summary and
    /// they are about a DRIVE: `turns` against the turns this reader counted,
    /// `prefill_tokens_total` against the prefill it added up. A recompute
    /// counted no turns and consumed no prefill, so running those checks
    /// against it would compare a number to nothing and call the answer a
    /// disagreement. Splitting the summary into kinds is what makes that
    /// impossible to write rather than merely wrong.
    fn admit_summary(&self, summary: &Summary, product_sha256: &str) -> Result<(), ParseError> {
        // The digest is checked once, before the kinds, because every kind
        // carries it. It was checked inside the drive arm until 2026-09-11,
        // which is how a recompute summary came to have no digest to check.
        if !digest_ok(product_sha256) {
            return Err(StructureError::BadDigest(product_sha256.to_owned()).into());
        }
        match summary {
            Summary::Drive {
                turns,
                prefill_tokens_total,
            } => {
                let counted = self.next_turn - 1;
                if *turns != counted {
                    return Err(StructureError::SummaryDisagrees {
                        field: "turns",
                        says: u64::from(*turns),
                        counted: u64::from(counted),
                    }
                    .into());
                }
                if *prefill_tokens_total != self.prefill_tokens {
                    return Err(StructureError::SummaryDisagrees {
                        field: "prefill_tokens_total",
                        says: prefill_tokens_total.get(),
                        counted: self.prefill_tokens.get(),
                    }
                    .into());
                }
            }
            Summary::Recompute {
                targets_checked,
                targets_matched,
                ..
            } => {
                // More matched than were checked is not a disagreement with
                // something this reader counted -- it is a claim that cannot
                // be true of itself, and it is the shape a "not applicable"
                // sentinel would arrive in if one were still possible here.
                if targets_matched > targets_checked {
                    return Err(StructureError::SummaryImpossible {
                        field: "targets_matched",
                        says: u64::from(*targets_matched),
                        bound: "targets_checked",
                        limit: u64::from(*targets_checked),
                    }
                    .into());
                }
            }
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
    // Which substrate each lane has been served by, so far. Not on `Seen`:
    // that one holds what LINKS resolve against, and this is not a link -- it
    // is the lane's own identity accumulating.
    let mut lanes: BTreeMap<&str, &str> = BTreeMap::new();

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
        // Checked against the regime ALREADY SEEN, like every other link here.
        // `start` is first or the loop has already refused the record, so the
        // declarations are in hand by the time any row can name one, and a
        // record can be walked as it is written.
        let named = match event {
            Event::Request {
                lane, substrate, ..
            } => Some(("request", lane, substrate)),
            Event::Fork {
                lane, substrate, ..
            } => Some(("fork", lane, substrate)),
            _ => None,
        };
        if let Some((row, lane, id)) = named {
            if let Some(known) = regime.as_ref()
                && !known.declares(id)
            {
                return Err(StructureError::UndeclaredSubstrate {
                    row,
                    id: id.clone(),
                    declared: known
                        .substrate_ids()
                        .into_iter()
                        .map(ToOwned::to_owned)
                        .collect(),
                }
                .into());
            }
            // A LANE IS A ROLE ON A SUBSTRATE. Fixed for the run, so a lane
            // that changes substrate is two lanes and should be spelled as
            // two -- and so a `rejected` row, which carries no substrate of
            // its own, has exactly one to inherit. Ruled 2026-09-08.
            if let Some(was) = lanes.insert(lane.as_str(), id.as_str())
                && was != id
            {
                return Err(StructureError::LaneChangedSubstrate {
                    lane: lane.clone(),
                    was: was.to_owned(),
                    now: id.clone(),
                }
                .into());
            }
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

/// The members of one row, being assembled.
///
/// A struct rather than a closure over the map, so that the optional
/// payloads render through one function: an absent payload is an absent key,
/// never an empty one, and the rule lives in one place instead of in five
/// `if let` blocks that could each drift.
struct Members(BTreeMap<String, Value>);

impl Members {
    fn put(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }

    fn put_text(&mut self, key: &str, text: &str) {
        self.put(key, Value::String(text.to_owned()));
    }

    fn put_u32(&mut self, key: &str, number: u32) {
        self.put(key, Value::Integer(i64::from(number)));
    }

    fn put_count(&mut self, key: &str, count: Count) {
        self.put(key, integer(count));
    }

    /// Present when the record kept it, absent otherwise.
    fn put_optional(&mut self, key: &str, value: Option<Value>) {
        if let Some(value) = value {
            self.put(key, value);
        }
    }
}

/// One event, as the value space.
///
/// The identifier is written once, from [`Event::id`], rather than in every
/// arm that has one: that method is already the single statement of which
/// kinds carry an id, and a second copy here could disagree with it.
///
/// EXHAUSTIVE OVER EVERY EVENT KIND; SPLITTING SCATTERS THE MATCH. That is
/// the wording the 2026-09-12 ruling requires of this suppression, and the
/// ruling is a criterion rather than a grant: an `#[allow]` is acceptable for
/// exactly one shape, an exhaustive `match` over an enum whose length IS its
/// completeness, and anything long for another reason -- helpers inlined,
/// formatting repeated per kind -- gets split instead.
///
/// Measured against that criterion before claiming it. Eleven arms for the
/// eleven `Event` variants and no wildcard, so adding a kind fails to compile
/// until someone says how it is written. The shared work is hoisted OUT of
/// the match -- `record` and `id` are written once above it -- so it is not
/// formatting repeated per kind. The three structures with any depth to them
/// delegate to `regime_value`, `artifacts_value` and `summary_value`, so it
/// is not helpers inlined. What is left is the vocabulary, one arm each.
#[allow(clippy::too_many_lines)]
fn event_value(event: &Event) -> BTreeMap<String, Value> {
    let mut members = Members(BTreeMap::new());
    members.put_text("record", event.kind().tag());
    members.put_optional("id", event.id().map(|id| Value::String(id.to_owned())));
    match event {
        Event::Start { regime } => members.put("regime", regime_value(regime)),
        Event::Turn {
            index,
            prefill_tokens,
        } => {
            members.put_u32("index", *index);
            members.put_count("prefill_tokens", *prefill_tokens);
        }
        Event::Request {
            lane,
            substrate,
            retry_of,
            text,
            ..
        } => {
            members.put_text("lane", lane);
            members.put_text("substrate", substrate);
            members.put_optional("retry_of", retry_of.clone().map(Value::String));
            members.put_optional("text", text.clone().map(Value::String));
        }
        Event::Response {
            to_request,
            output_tokens,
            text,
            ..
        } => {
            members.put_text("to_request", to_request);
            members.put_count("output_tokens", *output_tokens);
            members.put_optional("text", text.clone().map(Value::String));
        }
        Event::Fork {
            lane,
            substrate,
            of_turn,
            ..
        } => {
            members.put_text("lane", lane);
            members.put_text("substrate", substrate);
            members.put_u32("of_turn", *of_turn);
        }
        Event::Capture {
            from_fork, entries, ..
        } => {
            members.put_text("from_fork", from_fork);
            members.put_u32("entries", *entries);
        }
        Event::Seam {
            at_turn,
            rendered_bytes,
            ..
        } => {
            members.put_u32("at_turn", *at_turn);
            members.put_count("rendered_bytes", *rendered_bytes);
        }
        Event::Rejected {
            lane,
            at_turn,
            grounded,
            of,
            ..
        } => {
            members.put_text("lane", lane);
            members.put_u32("at_turn", *at_turn);
            members.put_count("grounded", *grounded);
            members.put_count("of", *of);
        }
        Event::ToolCall {
            at_turn,
            tool,
            args,
            exit,
            output,
            ..
        } => {
            members.put_u32("at_turn", *at_turn);
            members.put_text("tool", tool);
            members.put_optional("args", args.clone().map(Value::Object));
            members.put_optional("exit", exit.map(Value::Integer));
            members.put_optional("output", output.clone().map(Value::String));
        }
        Event::Claim {
            hypothesis,
            result,
            consumes,
            supersedes,
            ..
        } => {
            members.put_text("hypothesis", hypothesis);
            members.put_text("result", result.tag());
            members.put("consumes", artifacts_value(consumes));
            members.put_optional("supersedes", supersedes.clone().map(Value::String));
        }
        Event::Summary {
            summary,
            product_sha256,
        } => summary_value(summary, product_sha256, &mut members),
    }
    members.0
}

/// A summary ROW into the value space: the fields every kind carries, and then
/// the fields of the kind this one is.
///
/// `kind` always, then the product digest every kind carries, then that kind's
/// own fields and no others -- so what comes back out is what the schema would
/// accept going in.
///
/// The digest is passed in rather than reached for, because [`Summary`] has no
/// field for it: it sits on [`Event::Summary`], where a fact about the row
/// rather than about the kind belongs. The reading side is the same shape --
/// [`summary`] returns the kind's part and the digest is taken beside it --
/// and the two are paired deliberately, so that a field added to one of them
/// has an obvious home in the other.
fn summary_value(summary: &Summary, product_sha256: &str, members: &mut Members) {
    members.put_text("kind", summary.kind().tag());
    members.put_text("product_sha256", product_sha256);
    match summary {
        Summary::Drive {
            turns,
            prefill_tokens_total,
        } => {
            members.put_u32("turns", *turns);
            members.put_count("prefill_tokens_total", *prefill_tokens_total);
        }
        Summary::Recompute {
            targets_checked,
            targets_matched,
            digests,
        } => {
            members.put_u32("targets_checked", *targets_checked);
            members.put_u32("targets_matched", *targets_matched);
            members.put(
                "digests",
                Value::Array(
                    digests
                        .iter()
                        .cloned()
                        .map(Value::String)
                        .collect::<Vec<_>>(),
                ),
            );
        }
    }
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

/// A substrate's weights as a record value.
///
/// Kind first and then only that kind's fields, so what the writer emits is
/// what [`weights`] accepts. A round trip through the other spelling would be
/// a second reader for the same bytes.
fn weights_value(weights: &Weights) -> Value {
    let mut members = BTreeMap::from([(
        "kind".to_owned(),
        Value::String(weights.kind().tag().to_owned()),
    )]);
    match weights {
        Weights::Digest(sha256) => {
            members.insert("sha256".to_owned(), Value::String(sha256.clone()));
        }
        Weights::Hosted {
            provider,
            model_id,
            version_or_date_observed,
        } => {
            members.insert("provider".to_owned(), Value::String(provider.clone()));
            members.insert("model_id".to_owned(), Value::String(model_id.clone()));
            members.insert(
                "version_or_date_observed".to_owned(),
                Value::String(version_or_date_observed.clone()),
            );
        }
        Weights::Canned { acts_sha256 } => {
            members.insert("acts_sha256".to_owned(), Value::String(acts_sha256.clone()));
        }
    }
    Value::Object(members)
}

/// The regime as a record value.
///
/// Crate-visible because the object's dump carries it: the dump is the only
/// durable half of a working object, and a dump that does not say which
/// regime produced it cannot be compared with one from another arm.
pub(crate) fn regime_value(regime: &Regime) -> Value {
    let substrates = regime
        .substrates
        .iter()
        .map(|s| {
            Value::Object(BTreeMap::from([
                ("id".to_owned(), Value::String(s.id.clone())),
                (
                    "engine".to_owned(),
                    Value::Object(BTreeMap::from([
                        ("name".to_owned(), Value::String(s.engine.name.clone())),
                        (
                            "version_or_digest".to_owned(),
                            Value::String(s.engine.version_or_digest.clone()),
                        ),
                    ])),
                ),
                ("weights".to_owned(), weights_value(&s.weights)),
                (
                    "hardware_fingerprint".to_owned(),
                    Value::String(s.hardware_fingerprint.clone()),
                ),
                (
                    "sampler_card".to_owned(),
                    Value::Object(s.sampler_card.clone()),
                ),
                (
                    "reasoning".to_owned(),
                    Value::String(s.reasoning.tag().to_owned()),
                ),
            ]))
        })
        .collect();
    Value::Object(BTreeMap::from([
        ("arm".to_owned(), Value::String(regime.arm.clone())),
        ("substrates".to_owned(), Value::Array(substrates)),
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

/// This record, as its own value space.
///
/// # Errors
///
/// Returns the reason the text is not a session record.
pub fn project(source: &str) -> Result<Value, String> {
    parse(source)
        .map(|parsed| {
            Value::Object(BTreeMap::from([
                (
                    "regime".to_owned(),
                    Value::Object(BTreeMap::from([
                        ("arm".to_owned(), Value::String(parsed.regime().arm.clone())),
                        (
                            // The declared ids, in declaration order. What a
                            // report's front-matter mirrors, and what a row's
                            // `substrate` must be one of -- so the mirror and
                            // the references are the same list of names.
                            "substrates".to_owned(),
                            Value::Array(
                                parsed
                                    .regime()
                                    .substrate_ids()
                                    .into_iter()
                                    .map(|id| Value::String(id.to_owned()))
                                    .collect(),
                            ),
                        ),
                        (
                            // The subset of those ids served by weights that
                            // can change under you. A gate that certifies
                            // parity refuses a directory whose run names any
                            // of these; gate 0 ignores the key entirely.
                            // Carried here rather than derived by each reader
                            // from `canonical`, because a second reader of the
                            // record is a second opinion about it.
                            "hosted_substrates".to_owned(),
                            Value::Array(
                                parsed
                                    .regime()
                                    .hosted_substrate_ids()
                                    .into_iter()
                                    .map(|id| Value::String(id.to_owned()))
                                    .collect(),
                            ),
                        ),
                        (
                            "dogma_version".to_owned(),
                            Value::Integer(i64::from(parsed.regime().dogma_version)),
                        ),
                    ])),
                ),
                (
                    "kinds".to_owned(),
                    Value::Array(
                        parsed
                            .kinds()
                            .iter()
                            .map(|kind| Value::String(kind.tag().to_owned()))
                            .collect(),
                    ),
                ),
                ("canonical".to_owned(), Value::String(render(&parsed))),
            ]))
        })
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use super::json::Value;
    use super::{
        Count, Event, Kind, MAX_DEPTH, ParseError, Reasoning, Regime, SchemaError, StructureError,
        Verdict, Weights, WeightsKind, objects, parse, regime_value, render,
    };

    /// A `start` line whose regime is complete, as every record needs one.
    const START: &str = r#"{"record":"start","regime":{"arm":"baseline","dogma_version":0,"substrates":[{"id":"local","engine":{"name":"a-runtime","version_or_digest":"1.0"},"weights":{"kind":"digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"hardware_fingerprint":"one-gpu","sampler_card":{"seed":7,"temperature":0.7},"reasoning":"on"}]}}"#;

    fn record(rest: &str) -> String {
        format!("{START}\n{rest}")
    }

    #[test]
    fn the_regime_comes_from_the_required_start() {
        let parsed = parse(&record("")).expect("a record");
        assert_eq!(parsed.regime().arm, "baseline");
        assert_eq!(parsed.regime().substrates[0].id, "local");
        assert_eq!(parsed.regime().substrates[0].reasoning, Reasoning::On);
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
            r#"{"record":"request","id":"r1","lane":"main","substrate":"local"}"#,
            "\n",
            r#"{"record":"request","id":"r2","lane":"main","substrate":"local","retry_of":"r1"}"#,
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
            r#"{"record":"fork","id":"f1","lane":"interview","substrate":"local","of_turn":1}"#,
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

    // The archive rows. A record that keeps the texts can be replayed; one
    // that does not is still a record. Both spellings must survive a round
    // trip, and the empty payload most of all.
    #[test]
    fn archive_rows_keep_their_payloads_through_a_round_trip() {
        let source = record(concat!(
            r#"{"record":"turn","index":1,"prefill_tokens":10}"#,
            "\n",
            r#"{"record":"request","id":"q1","lane":"main","substrate":"local","text":"list the tree"}"#,
            "\n",
            r#"{"record":"response","id":"a1","to_request":"q1","output_tokens":0,"text":""}"#,
            "\n",
            r#"{"record":"tool_call","id":"t1","at_turn":1,"tool":"bash","args":{"command":"ls -la"},"exit":0,"output":""}"#,
            "\n",
            r#"{"record":"tool_call","id":"t2","at_turn":1,"tool":"bash","args":{},"exit":127,"output":"xyzzy: not found"}"#,
            "\n",
        ));
        let parsed = parse(&source).expect("archive rows are record rows");
        let again = parse(&render(&parsed)).expect("a rendering is itself a record");
        assert_eq!(parsed, again);
        assert!(
            matches!(
                &parsed.events[3],
                Event::Response { text: Some(text), .. } if text.is_empty()
            ),
            "an empty answer is a recorded answer, not a missing one"
        );
        assert!(matches!(
            &parsed.events[4],
            Event::ToolCall { exit: Some(0), output: Some(output), args: Some(args), .. }
                if output.is_empty() && args.contains_key("command")
        ));
        assert!(matches!(
            &parsed.events[5],
            Event::ToolCall { exit: Some(127), args: Some(args), .. } if args.is_empty()
        ));
    }

    #[test]
    fn a_ledger_row_without_payloads_is_still_a_record() {
        let source = record(concat!(
            r#"{"record":"turn","index":1,"prefill_tokens":10}"#,
            "\n",
            r#"{"record":"tool_call","id":"t1","at_turn":1,"tool":"bash"}"#,
            "\n",
        ));
        let parsed = parse(&source).expect("a ledger is a record");
        assert!(matches!(
            &parsed.events[2],
            Event::ToolCall {
                args: None,
                exit: None,
                output: None,
                ..
            }
        ));
        assert!(
            !render(&parsed).contains("args"),
            "an absent payload renders as absent, never as an empty one"
        );
    }

    #[test]
    fn a_payload_of_the_wrong_type_is_refused() {
        for row in [
            r#"{"record":"tool_call","id":"t1","at_turn":1,"tool":"bash","args":"ls"}"#,
            r#"{"record":"tool_call","id":"t1","at_turn":1,"tool":"bash","exit":"0"}"#,
            r#"{"record":"tool_call","id":"t1","at_turn":1,"tool":"bash","output":0}"#,
            r#"{"record":"response","id":"a1","to_request":"q1","output_tokens":0,"text":["no"]}"#,
        ] {
            let source = record(&format!(
                "{{\"record\":\"turn\",\"index\":1,\"prefill_tokens\":10}}\n\
                 {{\"record\":\"request\",\"id\":\"q1\",\"lane\":\"main\",\"substrate\":\"local\"}}\n{row}\n"
            ));
            assert!(
                matches!(
                    parse(&source),
                    Err(ParseError::Schema(SchemaError::WrongType { .. }))
                ),
                "{row} was not refused as the wrong type"
            );
        }
    }

    #[test]
    fn rendering_a_record_round_trips_through_the_parser() {
        let digest = "b".repeat(64);
        let source = record(&format!(
            "{{\"record\":\"turn\",\"index\":1,\"prefill_tokens\":1024}}\n\
             {{\"record\":\"fork\",\"id\":\"f1\",\"lane\":\"interview\",\"substrate\":\"local\",\"of_turn\":1}}\n\
             {{\"record\":\"capture\",\"id\":\"p1\",\"from_fork\":\"f1\",\"entries\":3}}\n\
             {{\"record\":\"summary\",\"kind\":\"drive\",\"turns\":1,\"prefill_tokens_total\":1024,\
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
                 \"substrates\":[{{\"id\":\"n\",\"engine\":{{\"name\":\"a-runtime\",\
                 \"version_or_digest\":\"1.0\"}},\"weights\":{{\"kind\":\"digest\",\"sha256\":\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\"}},\
                 \"hardware_fingerprint\":\"h\",\"sampler_card\":{{\"seed\":0}},\
                 \"reasoning\":\"{}\"}}]}}}}\n",
                state.tag()
            );
            let parsed = parse(&source).expect("a record");
            assert_eq!(parsed.regime().substrates[0].reasoning, *state);
        }
    }

    // Both weights kinds have to survive a rendering, because the writer is
    // the only thing that puts a record back on disk and a variant it cannot
    // spell is a variant that silently becomes the other one.
    #[test]
    fn every_weights_kind_round_trips() {
        for weights in [
            r#"{"kind":"digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
            r#"{"kind":"hosted","provider":"a-provider","model_id":"a-model-4","version_or_date_observed":"2026-09-08"}"#,
        ] {
            let source = format!(
                "{{\"record\":\"start\",\"regime\":{{\"arm\":\"a\",\"dogma_version\":0,\
                 \"substrates\":[{{\"id\":\"n\",\"engine\":{{\"name\":\"a-runtime\",\
                 \"version_or_digest\":\"1.0\"}},\"weights\":{weights},\
                 \"hardware_fingerprint\":\"h\",\"sampler_card\":{{\"seed\":0}},\
                 \"reasoning\":\"on\"}}]}}}}\n"
            );
            let once = parse(&source).unwrap_or_else(|err| panic!("{weights}: {err}"));
            let twice = parse(&render(&once)).expect("a rendering is itself a record");
            assert_eq!(once, twice, "{weights} did not survive a rendering");
        }
    }

    // A rejected lane has a substrate, and it is the lane's. The rule that
    // makes the lookup total is the one `validate` enforces; this is the half
    // that shows a reader getting an answer out of it.
    #[test]
    fn a_rejected_lanes_substrate_is_its_lanes() {
        let source = record(concat!(
            r#"{"record":"turn","index":1,"prefill_tokens":10}"#,
            "\n",
            r#"{"record":"fork","id":"f1","lane":"reformat","substrate":"local","of_turn":1}"#,
            "\n",
            r#"{"record":"rejected","id":"x1","lane":"reformat","at_turn":1,"grounded":3,"of":30}"#,
            "\n",
        ));
        let parsed = parse(&source).expect("a record");
        assert_eq!(parsed.substrate_of("reformat"), Some("local"));
        assert_eq!(parsed.substrate_of("a-lane-nothing-ran"), None);
    }

    // A lane that changes substrate is two lanes. Without this the reference
    // above has two answers and `substrate_of` picks one.
    #[test]
    fn a_lane_cannot_change_substrate() {
        let source = format!(
            "{}\n{}\n{}\n{}\n",
            r#"{"record":"start","regime":{"arm":"a","dogma_version":0,"substrates":[{"id":"big","engine":{"name":"a-runtime","version_or_digest":"1.0"},"weights":{"kind":"digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"hardware_fingerprint":"h","sampler_card":{"seed":0},"reasoning":"on"},{"id":"small","engine":{"name":"a-runtime","version_or_digest":"1.0"},"weights":{"kind":"digest","sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"},"hardware_fingerprint":"h","sampler_card":{"seed":0},"reasoning":"on"}]}}"#,
            r#"{"record":"turn","index":1,"prefill_tokens":10}"#,
            r#"{"record":"request","id":"q1","lane":"main","substrate":"big"}"#,
            r#"{"record":"request","id":"q2","lane":"main","substrate":"small"}"#,
        );
        assert!(matches!(
            parse(&source),
            Err(ParseError::Structure(
                StructureError::LaneChangedSubstrate { .. }
            ))
        ));
        // The same two requests on two lanes are two lanes, and fine.
        let two_lanes = source.replace(r#""id":"q2","lane":"main""#, r#""id":"q2","lane":"aside""#);
        parse(&two_lanes).expect("two lanes, two substrates");
    }

    // The whole reason the identity is typed: gate 1 asks these questions, and
    // a kind added later that nobody classified would answer them by accident.
    // Enumerating the vocabulary means a new kind fails to COMPILE here, which
    // is how `Canned` came to be classified rather than defaulted: adding it
    // broke this match, and a variant that cannot be added silently is the
    // point of spelling the match out instead of writing a wildcard.
    #[test]
    fn every_weights_kind_answers_gate_one_the_way_its_mechanism_does() {
        for kind in WeightsKind::ALL {
            let weights = match kind {
                WeightsKind::Digest => Weights::Digest("a".repeat(64)),
                WeightsKind::Hosted => Weights::Hosted {
                    provider: "a-provider".to_owned(),
                    model_id: "a-model-4".to_owned(),
                    version_or_date_observed: "2026-09-08".to_owned(),
                },
                WeightsKind::Canned => Weights::Canned {
                    acts_sha256: "b".repeat(64),
                },
            };
            assert_eq!(weights.kind(), *kind, "{} mislabels itself", kind.tag());

            // MAY it be reproduced. Digest by re-firing, Canned by replay;
            // Hosted by neither, because the weights can change under you.
            assert_eq!(
                weights.is_reproducible(),
                matches!(kind, WeightsKind::Digest | WeightsKind::Canned),
                "{} answers gate 1 wrongly about whether it may be reproduced",
                kind.tag()
            );

            // HOW. A replay is compared exactly and a re-firing within a band,
            // so a gate that confused them would apply a tolerance meant for
            // sampling and hardware to a run where neither can vary.
            assert_eq!(
                weights.is_replayed(),
                matches!(kind, WeightsKind::Canned),
                "{} answers gate 1 wrongly about how it is reproduced",
                kind.tag()
            );
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
            r#"{"record":"request","id":"q1","lane":"main","substrate":"local","retry_of":"q1"}"#,
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

    // The same crash, through the other door. `objects` is a second entry
    // into the same recursive descent, and a limit on the door nobody walked
    // through is not a limit -- the lane contract and every lane corpus
    // expectation arrive this way.
    #[test]
    fn a_deeply_nested_object_document_is_a_verdict_and_not_a_crash() {
        let just_past = format!(
            "{{\"a\":{}1{}}}\n",
            "{\"a\":".repeat(MAX_DEPTH),
            "}".repeat(MAX_DEPTH)
        );
        assert!(
            matches!(objects(&just_past), Err(ParseError::TooDeep { .. })),
            "one level past the limit must be a verdict from `objects` as well \
             as from `parse`"
        );
        let deep = format!(
            "{{\"a\":{}1{}}}\n",
            "{\"a\":".repeat(5000),
            "}".repeat(5000)
        );
        assert!(
            matches!(objects(&deep), Err(ParseError::TooDeep { .. })),
            "5000 deep is a verdict and not a crash"
        );
        assert!(
            objects(r#"{"tool":"update_record","parameters":{"content":{"type":"string"}}}"#)
                .is_ok(),
            "the limit does not reject a contract row"
        );
    }

    // Blank is absent with the key still there. A required field satisfied by
    // typing two quotes buys presence rather than provenance.
    #[test]
    fn a_required_string_that_says_nothing_is_absent() {
        let blank = r#"{"record":"start","regime":{"arm":"","dogma_version":0,"substrates":[{"id":"n","engine":{"name":"a-runtime","version_or_digest":"1.0"},"weights":{"kind":"digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"hardware_fingerprint":"h","sampler_card":{"seed":0},"reasoning":"on"}]}}"#;
        assert!(matches!(
            parse(blank),
            Err(ParseError::Schema(SchemaError::BlankField { .. }))
        ));
        let no_settings = r#"{"record":"start","regime":{"arm":"a","dogma_version":0,"substrates":[{"id":"n","engine":{"name":"a-runtime","version_or_digest":"1.0"},"weights":{"kind":"digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"hardware_fingerprint":"h","sampler_card":{},"reasoning":"on"}]}}"#;
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
             {{\"record\":\"summary\",\"kind\":\"drive\",\"turns\":99,\"prefill_tokens_total\":10,\
             \"product_sha256\":\"{digest}\"}}\n"
        ));
        assert!(matches!(
            parse(&source),
            Err(ParseError::Structure(StructureError::SummaryDisagrees { field, .. }))
                if field == "turns"
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
