//! The pairs register: the collector's own task, an entry against a later
//! turn of the same drive.
//!
//! [`sense`](super::sense) scores a sentence against an authored set. The
//! collector's nominator does something else: at every turn it asks, of each
//! live entry, whether this turn is about it (`collector::sense::Register::Intent`
//! is the turn's stated intent against the entry's content, plain cosine),
//! and tier 0 asks whether the entry's anchors recur in the turn
//! (`collector::literal`). Nothing measured either against a labelled
//! register, so the calibrated policy the collector refuses to run without
//! could not exist. This is the instrument for that measurement, pre-registered
//! on #17 and ratified as amended (2026-09-19).
//!
//! Two registers, one file each, because they score different things:
//!
//! * `pairs-intent.jsonl`, the **primary**: one row per (entry, later turn)
//!   pair, scored pairwise. Two scorings have a pairwise form and they are the
//!   two cells of this register: [`PairScoring::RawCosine`], the entry's text
//!   against the turn's stated intent, which is the surface the collector
//!   ships; and [`PairScoring::EnsembleMax`], the best cosine over the entry's
//!   two surfaces (its text, its origin step's stated intent) against the
//!   turn's two (its stated intent, its prose). Contrastive and softmax need an
//!   authored set with a negative pole, and a pair has none.
//! * `turns-sense.jsonl`: one row per later turn, its prose against the
//!   shipped `reversal` senses under every [`sense::Scoring`], because that
//!   register scores the turn and a per-pair copy of a per-turn score ranks
//!   nothing.
//!
//! **The gate is the anchored pre-gate**, computed here from the collector's
//! own tier 0: a pair is admitted when at least [`ANCHORS_REQUIRED`] distinct
//! anchors of the entry, by [`literal::anchors`], recur whole-token in the
//! turn's prose or tool output, by [`literal::find`]. A turn row carries the
//! flag its builder computed over every live entry of the drive, since a turn
//! row has no single entry to anchor to.
//!
//! **Precision and over-firing are pooled per drive** (amendment 3): rows are
//! ranked within each drive, the top `k` of each drive taken, and the fraction
//! computed over the pooled top-`k`. A budget is a per-session nomination
//! budget and a ranking across drives would spend one drive's budget on
//! another's rows. Beside the pooled figure, precision is broken out by
//! [`PairSource`] (amendment 4): planted reversals may be easier than mined
//! ones, and the split says whether the pooled figure was carried by the
//! planted half.
//!
//! Every metric ships a demonstrated failure, as in `sense`; the fixtures are
//! built over two drives so that the pooling itself is what is demonstrated.
//! The shuffled-label null is computed per cell and reported beside it.
//!
//! **Nothing here is a result.** The readings above were fixed on #17 before
//! any cache for this register existed.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use crate::capture::collector::literal;
use crate::capture::sense::{
    self, Control, ControlFailure, DataError, EmbeddedSet, Embedder, Fraction, Label, Null,
    ScoreError, Scored, Scoring, SetError, UndefinedCause, closed, cosine, decimal, rows, take_tag,
    take_text,
};
use crate::formats::record::json::Value;

// ---------------------------------------------------------------------------
// data
// ---------------------------------------------------------------------------

/// Where a pair came from.
///
/// Distinct from [`sense::Source`] because the words mean different things: an
/// authored sense row states a rule for the instrument's tests, while a planted
/// pair is a superseding turn written into a real drive to be found, and it is
/// scored in the corpus beside the mined ones. The report breaks precision out
/// by this, ruled on #17 (amendment 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PairSource {
    /// A superseding turn authored against a real entry of its own drive.
    Planted,
    /// A pair mined from the archived drive and labelled by a judge.
    Mined,
}

impl PairSource {
    /// Every source.
    pub const ALL: &'static [Self] = &[Self::Planted, Self::Mined];

    /// The spelling the register uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Planted => "planted",
            Self::Mined => "mined",
        }
    }

    /// The source a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|source| source.tag() == tag)
    }
}

/// One tool call of a turn: what was run and what it printed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// The command.
    pub command: String,
    /// Its output, as the register carries it.
    pub output: String,
}

/// One row of the primary register: an entry and a later turn of its drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pair {
    /// Its identity, unique in the register.
    pub id: String,
    /// The drive both sides belong to; the budget is per drive.
    pub drive: String,
    /// The entry's recorded text.
    pub entry: String,
    /// The stated intent of the step that recorded the entry, if one was
    /// found.
    pub entry_intent: Option<String>,
    /// The turn's stated intent, if one was found. A pair without one is not
    /// a row of the intent register, and is counted.
    pub turn_intent: Option<String>,
    /// The turn's prose.
    pub turn_prose: String,
    /// The turn's tool calls.
    pub turn_tools: Vec<Tool>,
    /// What the turn does to the entry's standing, by the judge.
    pub label: Label,
    /// Where the pair came from.
    pub source: PairSource,
}

/// One row of the authored-sense register: a later turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// Its identity, unique in the register.
    pub id: String,
    /// The drive.
    pub drive: String,
    /// The turn's prose.
    pub text: String,
    /// Whether at least [`ANCHORS_REQUIRED`] anchors of some live entry recur
    /// in the turn, computed by the register's builder over every live entry
    /// of the drive.
    pub anchored: bool,
    /// Derived from the judged pairs at this turn.
    pub label: Label,
    /// Where the turn came from.
    pub source: PairSource,
}

/// Which register a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PairRegister {
    /// `pairs-intent.jsonl`, the primary.
    Intent,
    /// `turns-sense.jsonl`.
    Sense,
}

impl PairRegister {
    /// Every register.
    pub const ALL: &'static [Self] = &[Self::Intent, Self::Sense];

    /// The file name that is this register.
    #[must_use]
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Intent => "pairs-intent.jsonl",
            Self::Sense => "turns-sense.jsonl",
        }
    }

    /// The register a file name is, if it is one.
    #[must_use]
    pub fn of(file_name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|register| register.file_name() == file_name)
    }

    /// The spelling a result uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Intent => "intent",
            Self::Sense => "sense",
        }
    }
}

/// An optional string member: absent is none, a string is some. The record
/// grammar has no `null`, so absence is the only spelling of none.
fn take_optional_text(
    members: &mut BTreeMap<String, Value>,
    line: usize,
    key: &'static str,
) -> Result<Option<String>, DataError> {
    match members.remove(key) {
        None => Ok(None),
        Some(Value::String(text)) if text.trim().is_empty() => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(DataError::WrongType { line, key }),
    }
}

/// A required boolean member.
fn take_bool(
    members: &mut BTreeMap<String, Value>,
    line: usize,
    key: &'static str,
) -> Result<bool, DataError> {
    match members.remove(key) {
        Some(Value::Boolean(flag)) => Ok(flag),
        Some(_) => Err(DataError::WrongType { line, key }),
        None => Err(DataError::MissingKey { line, key }),
    }
}

/// A member the register carries for its reader and the instrument does not
/// score on: the turn and step numbers, the pair ids under a turn row. Its
/// presence is required, so the schema stays closed, and its value is not
/// read.
fn take_carried(
    members: &mut BTreeMap<String, Value>,
    line: usize,
    key: &'static str,
) -> Result<(), DataError> {
    members
        .remove(key)
        .map(|_| ())
        .ok_or(DataError::MissingKey { line, key })
}

/// The tool calls of a pair row: a list of `{"command","output","truncated"}`.
fn take_tools(members: &mut BTreeMap<String, Value>, line: usize) -> Result<Vec<Tool>, DataError> {
    const KEY: &str = "turn_tools";
    let Some(value) = members.remove(KEY) else {
        return Err(DataError::MissingKey { line, key: KEY });
    };
    let Value::Array(items) = value else {
        return Err(DataError::WrongType { line, key: KEY });
    };
    let mut tools = Vec::new();
    for item in items {
        let Value::Object(mut fields) = item else {
            return Err(DataError::WrongType { line, key: KEY });
        };
        let Some(Value::String(command)) = fields.remove("command") else {
            return Err(DataError::WrongType { line, key: KEY });
        };
        let Some(Value::String(output)) = fields.remove("output") else {
            return Err(DataError::WrongType { line, key: KEY });
        };
        take_bool(&mut fields, line, "truncated")?;
        closed(&fields, line)?;
        tools.push(Tool { command, output });
    }
    Ok(tools)
}

/// Read `pairs-intent.jsonl`.
///
/// Rows are `{"id","drive","turn","step","entry","entry_intent","turn_intent",
/// "turn_prose","turn_tools","label","source"}` and nothing else; either
/// intent may be absent, since the record grammar has no null and an intent
/// the router did not find is not a value.
///
/// # Errors
///
/// Returns [`DataError`] for a line outside the schema, an id bound twice, or
/// a file with no rows.
pub fn pairs(source: &str) -> Result<Vec<Pair>, DataError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (line, mut members) in rows(source)? {
        let id = take_text(&mut members, line, "id")?;
        let drive = take_text(&mut members, line, "drive")?;
        take_carried(&mut members, line, "turn")?;
        take_carried(&mut members, line, "step")?;
        let entry = take_text(&mut members, line, "entry")?;
        let entry_intent = take_optional_text(&mut members, line, "entry_intent")?;
        let turn_intent = take_optional_text(&mut members, line, "turn_intent")?;
        let turn_prose = take_text(&mut members, line, "turn_prose")?;
        let turn_tools = take_tools(&mut members, line)?;
        let label = take_tag(&mut members, line, "label", Label::from_tag)?;
        let source = take_tag(&mut members, line, "source", PairSource::from_tag)?;
        closed(&members, line)?;
        if !seen.insert(id.clone()) {
            return Err(DataError::DuplicateId { line, id });
        }
        out.push(Pair {
            id,
            drive,
            entry,
            entry_intent,
            turn_intent,
            turn_prose,
            turn_tools,
            label,
            source,
        });
    }
    Ok(out)
}

/// Read `turns-sense.jsonl`.
///
/// Rows are `{"id","drive","turn","step","text","anchored","label","source",
/// "pairs"}` and nothing else.
///
/// # Errors
///
/// Returns [`DataError`] for a line outside the schema, an id bound twice, or
/// a file with no rows.
pub fn turns(source: &str) -> Result<Vec<Turn>, DataError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (line, mut members) in rows(source)? {
        let id = take_text(&mut members, line, "id")?;
        let drive = take_text(&mut members, line, "drive")?;
        take_carried(&mut members, line, "turn")?;
        take_carried(&mut members, line, "step")?;
        let text = take_text(&mut members, line, "text")?;
        let anchored = take_bool(&mut members, line, "anchored")?;
        let label = take_tag(&mut members, line, "label", Label::from_tag)?;
        let source = take_tag(&mut members, line, "source", PairSource::from_tag)?;
        take_carried(&mut members, line, "pairs")?;
        closed(&members, line)?;
        if !seen.insert(id.clone()) {
            return Err(DataError::DuplicateId { line, id });
        }
        out.push(Turn {
            id,
            drive,
            text,
            anchored,
            label,
            source,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// embedders, by role
// ---------------------------------------------------------------------------

/// One embedder, seen from both sides of a pair.
///
/// An instruction-tuned embedder places a query and a document under
/// different prefixes, and one text can be both: an entry's origin intent is
/// a query beside its entry and the document of the turn that stated it. So a
/// pairs run reads two caches per embedder -- `<id>.query.vectors.jsonl` and
/// `<id>.document.vectors.jsonl` -- or one `<id>.vectors.jsonl` for both when
/// the embedder prefixes neither side. Entries, their intents and the senses
/// are queries; turn intents, turn prose and the control words are documents.
#[derive(Clone, Copy)]
pub struct Roles<'a> {
    /// The embedder's id, part of every cell's regime.
    pub id: &'a str,
    /// The query side.
    pub query: &'a dyn Embedder,
    /// The document side.
    pub document: &'a dyn Embedder,
}

impl fmt::Debug for Roles<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Roles")
            .field("id", &self.id)
            .field("query", &self.query.id())
            .field("document", &self.document.id())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// the anchored pre-gate
// ---------------------------------------------------------------------------

/// How many distinct anchors of the entry must recur in the turn for the gate
/// to admit the pair. Ratified on #17: two, with the collector's one-anchor
/// tier 0 carried on the verdict as a reading rather than adjudicated.
pub const ANCHORS_REQUIRED: usize = 2;

/// The distinct anchors of `entry` that recur, whole-token, in the turn's
/// prose or tool output.
///
/// The anchors are tier 0's own ([`literal::anchors`]) and so is the match
/// ([`literal::find`]); this function composes them and adds nothing. Prose
/// before tool output, as tier 0 scans.
#[must_use]
pub fn recurring_anchors(entry: &str, prose: &str, tools: &[Tool]) -> Vec<String> {
    let mut haystack = String::from(prose);
    for tool in tools {
        haystack.push('\n');
        haystack.push_str(&tool.command);
        haystack.push('\n');
        haystack.push_str(&tool.output);
    }
    literal::anchors(entry)
        .into_iter()
        .filter(|anchor| !literal::find(&anchor.text, &haystack).is_empty())
        .map(|anchor| anchor.text)
        .collect()
}

/// Whether a pair is anchored: at least [`ANCHORS_REQUIRED`] anchors recur.
#[must_use]
pub fn anchored(pair: &Pair) -> bool {
    recurring_anchors(&pair.entry, &pair.turn_prose, &pair.turn_tools).len() >= ANCHORS_REQUIRED
}

/// Whether the anchored pre-gate is applied before scoring.
///
/// The same two arms as [`sense::Gate`], under the same tags, so a decision
/// rule written for "(scoring, gate) cells" reads both instruments alike.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PairGate {
    /// Every row is scored.
    Without,
    /// Only an anchored row is scored; the rest sit at the scoring's floor.
    Anchored,
}

impl PairGate {
    /// Both arms.
    pub const ALL: &'static [Self] = &[Self::Without, Self::Anchored];

    /// The spelling a cell uses: the sense instrument's, so the two read alike.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Without => sense::Gate::Without.tag(),
            Self::Anchored => sense::Gate::With.tag(),
        }
    }
}

// ---------------------------------------------------------------------------
// scoring
// ---------------------------------------------------------------------------

/// A scoring with a pairwise form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PairScoring {
    /// Cosine of the entry's text against the turn's stated intent: the
    /// surface the collector ships.
    RawCosine,
    /// The best cosine over the entry's surfaces against the turn's.
    EnsembleMax,
}

impl PairScoring {
    /// Both scorings.
    pub const ALL: &'static [Self] = &[Self::RawCosine, Self::EnsembleMax];

    /// The spelling a cell uses: the sense instrument's tag for the same
    /// scoring, so a rule that names `raw_cosine` names it here too.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::RawCosine => Scoring::RawCosine.tag(),
            Self::EnsembleMax => Scoring::EnsembleMax.tag(),
        }
    }

    /// The lowest value this scoring can produce; a gated-out row sits here.
    #[must_use]
    pub fn floor(self) -> f64 {
        -1.0
    }

    /// Score one pair: the entry's surfaces as queries, the turn's as
    /// documents. `None` when a vector the scoring needs does not exist.
    #[must_use]
    pub fn score(self, pair: &Pair, roles: Roles<'_>) -> Option<f64> {
        let entry = roles.query.embed(&pair.entry);
        match self {
            Self::RawCosine => {
                let intent = roles.document.embed(pair.turn_intent.as_deref()?);
                cosine(&entry, &intent)
            }
            Self::EnsembleMax => {
                let mut queries = vec![entry];
                if let Some(intent) = &pair.entry_intent {
                    queries.push(roles.query.embed(intent));
                }
                let mut documents = vec![roles.document.embed(&pair.turn_prose)];
                if let Some(intent) = &pair.turn_intent {
                    documents.push(roles.document.embed(intent));
                }
                let mut best = f64::NEG_INFINITY;
                for query in &queries {
                    for document in &documents {
                        best = best.max(cosine(query, document)?);
                    }
                }
                Some(best)
            }
        }
    }
}

/// One cell of the intent register short of its embedder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PairCell {
    /// How pairs are scored.
    pub scoring: PairScoring,
    /// Whether they are gated first.
    pub gate: PairGate,
}

/// One cell of the sense register short of its embedder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TurnCell {
    /// How turns are scored against the reversal set.
    pub scoring: Scoring,
    /// Whether they are gated first.
    pub gate: PairGate,
}

/// One row scored in one cell, with what the pooled metrics need.
#[derive(Debug, Clone, PartialEq)]
pub struct ScoredPair {
    /// The row.
    pub id: String,
    /// Its drive.
    pub drive: String,
    /// Where it came from.
    pub source: PairSource,
    /// What it is.
    pub label: Label,
    /// Its score, or the scoring's floor if the gate dropped it.
    pub score: f64,
    /// Whether the gate let it through.
    pub admitted: bool,
}

impl ScoredPair {
    /// The row as the sense instrument's scored row, for the metrics that do
    /// not pool: area under the curve, separation, the null.
    #[must_use]
    pub fn as_scored(&self) -> Scored {
        Scored {
            id: self.id.clone(),
            label: self.label,
            score: self.score,
            admitted: self.admitted,
        }
    }
}

/// The rows of the intent register: pairs whose turn stated an intent.
///
/// The rest are not rows of this register (ruled on #17, reading 3) and their
/// count is reported.
#[must_use]
pub fn intent_rows(pairs: &[Pair]) -> Vec<&Pair> {
    pairs
        .iter()
        .filter(|pair| pair.turn_intent.is_some())
        .collect()
}

/// Score every pair in `cell`.
///
/// # Errors
///
/// Returns [`ScoreError::Unembeddable`] for a row the gate admitted and an
/// embedder could not place.
pub fn score_pairs(
    pairs: &[&Pair],
    roles: Roles<'_>,
    cell: PairCell,
) -> Result<Vec<ScoredPair>, ScoreError> {
    pairs
        .iter()
        .map(|pair| {
            let admitted = match cell.gate {
                PairGate::Without => true,
                PairGate::Anchored => anchored(pair),
            };
            let score = if admitted {
                cell.scoring
                    .score(pair, roles)
                    .ok_or_else(|| ScoreError::Unembeddable {
                        id: pair.id.clone(),
                    })?
            } else {
                cell.scoring.floor()
            };
            Ok(ScoredPair {
                id: pair.id.clone(),
                drive: pair.drive.clone(),
                source: pair.source,
                label: pair.label,
                score,
                admitted,
            })
        })
        .collect()
}

/// Score every turn against the reversal `set` in `cell`.
///
/// # Errors
///
/// Returns [`ScoreError::Unembeddable`] for a row the gate admitted and the
/// document embedder could not place.
pub fn score_turns(
    turns: &[Turn],
    set: &EmbeddedSet,
    document: &dyn Embedder,
    cell: TurnCell,
) -> Result<Vec<ScoredPair>, ScoreError> {
    turns
        .iter()
        .map(|turn| {
            let admitted = match cell.gate {
                PairGate::Without => true,
                PairGate::Anchored => turn.anchored,
            };
            let score = if admitted {
                cell.scoring
                    .score(&document.embed(&turn.text), set)
                    .ok_or_else(|| ScoreError::Unembeddable {
                        id: turn.id.clone(),
                    })?
            } else {
                cell.scoring.floor()
            };
            Ok(ScoredPair {
                id: turn.id.clone(),
                drive: turn.drive.clone(),
                source: turn.source,
                label: turn.label,
                score,
                admitted,
            })
        })
        .collect()
}

// ---------------------------------------------------------------------------
// controls
// ---------------------------------------------------------------------------

/// The controls of the intent register, per (embedder, scoring), ungated.
///
/// Two seeded pairs are scored beside the register: the first positive's
/// entry against its own text as the turn's intent, which every pairwise
/// scoring must put first (cosine of a vector with itself is one); and the
/// same entry against [`sense::UNRELATED`] as both intent and prose, which no
/// positive may sink below. The positives-only bound is the sense
/// instrument's, ruled on #24 (2026-09-15), for the same reason.
///
/// # Errors
///
/// Returns [`ControlFailure`] as the sense instrument does: `Unscorable` for
/// a vector that does not exist, `Inverted`, `NotAtTop`, `NotAtBottom`, or
/// `Missing` when the register has no positive to seed from.
pub fn pair_controls(
    pairs: &[&Pair],
    roles: Roles<'_>,
    scoring: PairScoring,
) -> Result<(), ControlFailure> {
    let Some(positive) = pairs.iter().find(|pair| pair.label.is_positive()) else {
        return Err(ControlFailure::Missing {
            control: Control::VerbatimPositive,
        });
    };
    let cell = PairCell {
        scoring,
        gate: PairGate::Without,
    };
    let scored = score_pairs(pairs, roles, cell).map_err(ControlFailure::Unscorable)?;
    // The top control's turn is the entry itself, placed on the QUERY side
    // twice: the check is that a scoring puts a vector first against itself,
    // and a document cache holds no entries.
    let top = Roles {
        id: roles.id,
        query: roles.query,
        document: roles.query,
    };
    let verbatim = Pair {
        turn_intent: Some(positive.entry.clone()),
        turn_prose: positive.entry.clone(),
        turn_tools: Vec::new(),
        ..(*positive).clone()
    };
    let top_score = scoring.score(&verbatim, top).ok_or_else(|| {
        ControlFailure::Unscorable(ScoreError::Unembeddable {
            id: Control::VerbatimPositive.tag().to_owned(),
        })
    })?;
    let unrelated = Pair {
        turn_intent: Some(sense::UNRELATED.to_owned()),
        turn_prose: sense::UNRELATED.to_owned(),
        turn_tools: Vec::new(),
        entry_intent: None,
        ..(*positive).clone()
    };
    let bottom_score = scoring.score(&unrelated, roles).ok_or_else(|| {
        ControlFailure::Unscorable(ScoreError::Unembeddable {
            id: Control::UnrelatedWords.tag().to_owned(),
        })
    })?;
    if top_score <= bottom_score {
        return Err(ControlFailure::Inverted {
            top: top_score,
            bottom: bottom_score,
        });
    }
    for row in &scored {
        if row.score >= top_score {
            return Err(ControlFailure::NotAtTop {
                control: Control::VerbatimPositive,
                score: top_score,
                row: row.id.clone(),
                other: row.score,
            });
        }
        if row.label.is_positive() && row.score < bottom_score {
            return Err(ControlFailure::NotAtBottom {
                control: Control::UnrelatedWords,
                score: bottom_score,
                row: row.id.clone(),
                other: row.score,
            });
        }
    }
    Ok(())
}

/// The controls of the sense register: the sense instrument's own, over the
/// turn rows as register rows, the document embedder placing the rows and
/// the control texts.
///
/// # Errors
///
/// As [`sense::controls`].
pub fn turn_controls(
    turns: &[Turn],
    set: &EmbeddedSet,
    document: &dyn Embedder,
    scoring: Scoring,
) -> Result<(), ControlFailure> {
    let rows: Vec<sense::Row> = turns
        .iter()
        .map(|turn| sense::Row {
            id: turn.id.clone(),
            text: turn.text.clone(),
            label: turn.label,
            source: sense::Source::Mined,
        })
        .collect();
    sense::controls(&rows, set, document, scoring)
}

// ---------------------------------------------------------------------------
// pooled metrics
// ---------------------------------------------------------------------------

/// The top `k` of every drive, ranked within the drive by score then id,
/// pooled in drive order.
#[must_use]
pub fn pooled_top_k(rows: &[ScoredPair], k: usize) -> Vec<&ScoredPair> {
    let mut by_drive: BTreeMap<&str, Vec<&ScoredPair>> = BTreeMap::new();
    for row in rows {
        by_drive.entry(row.drive.as_str()).or_default().push(row);
    }
    let mut pooled = Vec::new();
    for group in by_drive.values_mut() {
        group.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
        pooled.extend(group.iter().take(k).copied());
    }
    pooled
}

/// Precision at budget `k` per drive, pooled: of the rows nominated within
/// each drive's top `k`, how many are positive. The primary endpoint.
#[must_use]
pub fn precision_at_k(rows: &[ScoredPair], k: usize) -> Fraction {
    let pooled = pooled_top_k(rows, k);
    let hits = pooled.iter().filter(|row| row.label.is_positive()).count();
    Fraction {
        hits: hits as u64,
        of: pooled.len() as u64,
    }
}

/// [`precision_at_k`] over the nominated rows of one source only: the split
/// that says whether the pooled figure was carried by the planted half.
#[must_use]
pub fn precision_by_source(rows: &[ScoredPair], k: usize, source: PairSource) -> Fraction {
    let pooled: Vec<&ScoredPair> = pooled_top_k(rows, k)
        .into_iter()
        .filter(|row| row.source == source)
        .collect();
    let hits = pooled.iter().filter(|row| row.label.is_positive()).count();
    Fraction {
        hits: hits as u64,
        of: pooled.len() as u64,
    }
}

/// [`precision_at_k`] over only the nominated rows the gate actually
/// admitted, beside the pooled figure and never replacing it: a drive with
/// fewer than `k` anchored rows still fills its budget from
/// [`pooled_top_k`] with the rejected rows sitting at the scoring's floor,
/// tie-broken by id, so the pooled reading over a with-gate arm is partly
/// over nominations the gate never made (measured by review: 10 of 90
/// with-gate nominations at k = 5 were rejected rows). This is the reading
/// over nominations the gate did make.
#[must_use]
pub fn precision_admitted_only(rows: &[ScoredPair], k: usize) -> Fraction {
    let pooled: Vec<&ScoredPair> = pooled_top_k(rows, k)
        .into_iter()
        .filter(|row| row.admitted)
        .collect();
    let hits = pooled.iter().filter(|row| row.label.is_positive()).count();
    Fraction {
        hits: hits as u64,
        of: pooled.len() as u64,
    }
}

/// Over-firing at budget `k` per drive, pooled: of every hard-negative row,
/// how many were nominated within their drive's top `k`.
#[must_use]
pub fn over_firing(rows: &[ScoredPair], k: usize) -> Fraction {
    let hard = rows
        .iter()
        .filter(|row| row.label.is_hard_negative())
        .count();
    let nominated = pooled_top_k(rows, k)
        .iter()
        .filter(|row| row.label.is_hard_negative())
        .count();
    Fraction {
        hits: nominated as u64,
        of: hard as u64,
    }
}

/// One arm's pooled precision at `k`, per drive: hits and nominated, from
/// just that drive's own rows -- the unit [`precision_bootstrap`] resamples.
fn precision_by_drive(rows: &[ScoredPair], k: usize) -> BTreeMap<&str, (u64, u64)> {
    let mut by_drive: BTreeMap<&str, Vec<&ScoredPair>> = BTreeMap::new();
    for row in rows {
        by_drive.entry(row.drive.as_str()).or_default().push(row);
    }
    by_drive
        .into_iter()
        .map(|(drive, mut group)| {
            group.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
            let top = &group[..group.len().min(k)];
            let hits = top.iter().filter(|row| row.label.is_positive()).count();
            (drive, (hits as u64, top.len() as u64))
        })
        .collect()
}

/// A small count as a float, for arithmetic reported to a fixed precision --
/// never a metric total, which stays exact in the record.
fn count(n: u64) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

/// The pooled precision over one resample: counts summed across the sampled
/// drives (a drive drawn twice counts twice), then divided.
fn pooled_precision(by_drive: &BTreeMap<&str, (u64, u64)>, sample: &[&str]) -> f64 {
    let (hits, of) = sample.iter().fold((0u64, 0u64), |(hits, of), drive| {
        let (drive_hits, drive_of) = by_drive[drive];
        (hits + drive_hits, of + drive_of)
    });
    if of == 0 {
        0.0
    } else {
        count(hits) / count(of)
    }
}

/// A paired bootstrap's result, in [`sense::Bootstrap`]'s shape short of the
/// type [`sense::paired_bootstrap`] alone may construct.
#[derive(Debug, Clone, PartialEq)]
pub struct PrecisionBootstrap {
    /// The observed difference: the first arm's pooled precision at `k` minus
    /// the second's, over every drive, unresampled.
    pub observed: f64,
    /// How often a drive-resampled difference crossed zero, add-one
    /// corrected.
    pub p: f64,
    /// The smallest value these resamples could have produced.
    pub p_floor: f64,
    /// How many resamples.
    pub resamples: u32,
}

/// A paired bootstrap of the **pooled precision at `k`** between two arms of
/// the same register, resampling by drive.
///
/// [`pair_comparisons`]'s score-difference bootstrap is the wrong statistic
/// for the anchored arm against the ungated one: a row the gate rejects sits
/// at the scoring's floor, so the mean score difference is negative by
/// construction whatever the two arms' precision did. The pre-gate sub-rule
/// names precision at k; the natural paired unit under a per-drive pooled
/// metric is the drive itself (ruled on #17, 2026-09-20) -- resampled with
/// replacement, each resample's statistic the difference of the two arms'
/// pooled precision over the resampled drives. The observed statistic is
/// unresampled: each arm's own pooled [`precision_at_k`], as already
/// reported.
///
/// # Errors
///
/// [`sense::BootstrapError::Unpaired`] when the two arms do not share the
/// same drives, `Empty` for no drives, `NoResamples` for zero resamples.
pub fn precision_bootstrap(
    first: &[ScoredPair],
    second: &[ScoredPair],
    k: usize,
    resamples: u32,
    seed: u64,
) -> Result<PrecisionBootstrap, sense::BootstrapError> {
    let first_by_drive = precision_by_drive(first, k);
    let second_by_drive = precision_by_drive(second, k);
    if first_by_drive.len() != second_by_drive.len()
        || first_by_drive.keys().ne(second_by_drive.keys())
    {
        return Err(sense::BootstrapError::Unpaired {
            a: first_by_drive.len(),
            b: second_by_drive.len(),
        });
    }
    let drives: Vec<&str> = first_by_drive.keys().copied().collect();
    if drives.is_empty() {
        return Err(sense::BootstrapError::Empty);
    }
    if resamples == 0 {
        return Err(sense::BootstrapError::NoResamples);
    }
    let observed =
        pooled_precision(&first_by_drive, &drives) - pooled_precision(&second_by_drive, &drives);
    let mut rng = sense::Xorshift::seeded(seed);
    let side = observed.partial_cmp(&0.0);
    let crossed = |difference: f64| match side {
        Some(Ordering::Greater) => difference <= 0.0,
        Some(Ordering::Less) => difference >= 0.0,
        _ => false,
    };
    let crossings = (0..resamples)
        .filter(|_| {
            let sample: Vec<&str> = (0..drives.len())
                .map(|_| drives[rng.below(drives.len())])
                .collect();
            let difference = pooled_precision(&first_by_drive, &sample)
                - pooled_precision(&second_by_drive, &sample);
            crossed(difference)
        })
        .count();
    let p = match side {
        Some(Ordering::Greater | Ordering::Less) => {
            let crossings = f64::from(u32::try_from(crossings).unwrap_or(u32::MAX));
            (crossings + 1.0) / (f64::from(resamples) + 1.0)
        }
        _ => 1.0,
    };
    Ok(PrecisionBootstrap {
        observed,
        p,
        p_floor: sense::attainable_p_floor(resamples),
        resamples,
    })
}

/// The rows as the sense instrument's, for the metrics that do not pool.
fn as_scored(rows: &[ScoredPair]) -> Vec<Scored> {
    rows.iter().map(ScoredPair::as_scored).collect()
}

/// The pooled instrument's metrics: the sense instrument's four, the two
/// budgeted ones pooled per drive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PairMetric {
    /// [`precision_at_k`], pooled.
    PrecisionAtK,
    /// [`over_firing`], pooled.
    OverFiring,
    /// [`sense::auc`].
    Auc,
    /// [`sense::d_prime`].
    DPrime,
}

impl PairMetric {
    /// Every metric.
    pub const ALL: &'static [Self] = &[
        Self::PrecisionAtK,
        Self::OverFiring,
        Self::Auc,
        Self::DPrime,
    ];

    /// The sense instrument's metric this pools or reuses, whose tag, failure
    /// reading and direction this one shares.
    #[must_use]
    pub fn sense_metric(self) -> sense::Metric {
        match self {
            Self::PrecisionAtK => sense::Metric::PrecisionAtK,
            Self::OverFiring => sense::Metric::OverFiring,
            Self::Auc => sense::Metric::Auc,
            Self::DPrime => sense::Metric::DPrime,
        }
    }

    /// The spelling a result uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        self.sense_metric().tag()
    }

    /// The metric over `rows` at budget `k`.
    ///
    /// # Errors
    ///
    /// Returns the [`UndefinedCause`] when the rows do not meet the metric's
    /// precondition.
    pub fn compute(self, k: usize, rows: &[ScoredPair]) -> Result<f64, UndefinedCause> {
        match self {
            Self::PrecisionAtK => precision_at_k(rows, k)
                .as_f64()
                .ok_or(UndefinedCause::NoValue),
            Self::OverFiring => over_firing(rows, k).as_f64().ok_or(UndefinedCause::NoValue),
            Self::Auc => sense::auc(&as_scored(rows)).ok_or(UndefinedCause::NoValue),
            Self::DPrime => sense::d_prime_or_why(&as_scored(rows)),
        }
    }

    /// The rows this metric must fail on, laid over TWO drives so that the
    /// pooling is what is demonstrated: the sense instrument's fixture per
    /// drive, and the pooled reading must be the worst the metric can say.
    #[must_use]
    pub fn failure_fixture(self) -> Vec<ScoredPair> {
        let per_drive = self.sense_metric().failure_fixture();
        ["failing/a", "failing/b"]
            .iter()
            .flat_map(|drive| {
                per_drive.iter().map(move |row| ScoredPair {
                    id: format!("{drive}/{}", row.id),
                    drive: (*drive).to_owned(),
                    source: PairSource::Planted,
                    label: row.label,
                    score: row.score,
                    admitted: row.admitted,
                })
            })
            .collect()
    }
}

/// A pooled metric's value, and the value the same code produced on the rows
/// it must fail on. Private fields; [`PairReported::take`] is the only door.
#[derive(Debug, Clone, PartialEq)]
pub struct PairReported {
    metric: PairMetric,
    budget: usize,
    value: f64,
    on_failure_fixture: f64,
}

/// Why a pooled metric is not reported, in the sense instrument's terms.
#[derive(Debug, Clone, PartialEq)]
pub enum PairMetricError {
    /// The rows built to fail did not fail.
    InstrumentNeverFailed {
        /// The metric.
        metric: PairMetric,
        /// The budget.
        budget: usize,
        /// What the failing rows scored.
        value: f64,
        /// The reading that would have counted as failure.
        reading: f64,
    },
    /// The metric is undefined on these rows.
    Undefined {
        /// The metric.
        metric: PairMetric,
        /// Which rows.
        on: sense::MetricSubject,
        /// Why.
        cause: UndefinedCause,
    },
}

impl fmt::Display for PairMetricError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstrumentNeverFailed {
                metric,
                budget,
                value,
                reading,
            } => write!(
                f,
                "{} at budget {budget} read {value} on rows built to fail, and failure is {reading}",
                metric.tag()
            ),
            Self::Undefined { metric, on, cause } => {
                write!(f, "{} is undefined on {on}: {cause}", metric.tag())
            }
        }
    }
}

impl Error for PairMetricError {}

impl PairReported {
    /// Report `metric` over `subject` at budget `k`, having demonstrated on
    /// the metric's own two-drive failure fixture that the same code can
    /// report failure.
    ///
    /// # Errors
    ///
    /// [`PairMetricError::InstrumentNeverFailed`] when the fixture does not
    /// fail at this budget; [`PairMetricError::Undefined`] when the metric has
    /// no value on the fixture or on the subject.
    pub fn take(
        metric: PairMetric,
        k: usize,
        subject: &[ScoredPair],
    ) -> Result<Self, PairMetricError> {
        let failing = metric.failure_fixture();
        let on_failure_fixture =
            metric
                .compute(k, &failing)
                .map_err(|cause| PairMetricError::Undefined {
                    metric,
                    on: sense::MetricSubject::FailureFixture,
                    cause,
                })?;
        if !metric.sense_metric().failed(on_failure_fixture) {
            return Err(PairMetricError::InstrumentNeverFailed {
                metric,
                budget: k,
                value: on_failure_fixture,
                reading: metric.sense_metric().failure_reading(),
            });
        }
        let value = metric
            .compute(k, subject)
            .map_err(|cause| PairMetricError::Undefined {
                metric,
                on: sense::MetricSubject::Subject,
                cause,
            })?;
        if !value.is_finite() {
            return Err(PairMetricError::Undefined {
                metric,
                on: sense::MetricSubject::Subject,
                cause: UndefinedCause::NotFinite,
            });
        }
        Ok(Self {
            metric,
            budget: k,
            value,
            on_failure_fixture,
        })
    }

    /// The metric.
    #[must_use]
    pub fn metric(&self) -> PairMetric {
        self.metric
    }

    /// The budget.
    #[must_use]
    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Its value on the subject.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.value
    }

    /// The report as a record value, in the sense instrument's shape.
    #[must_use]
    pub fn record(&self) -> Value {
        Value::Object(BTreeMap::from([
            (
                "metric".to_owned(),
                Value::String(self.metric.tag().to_owned()),
            ),
            (
                "budget".to_owned(),
                Value::Integer(i64::try_from(self.budget).unwrap_or(i64::MAX)),
            ),
            ("value".to_owned(), decimal(self.value, 4)),
            (
                "demonstrated_failure".to_owned(),
                decimal(self.on_failure_fixture, 4),
            ),
            (
                "failure_reading".to_owned(),
                decimal(self.metric.sense_metric().failure_reading(), 4),
            ),
        ]))
    }
}

// ---------------------------------------------------------------------------
// cells
// ---------------------------------------------------------------------------

/// Why a register could not be turned into cells.
#[derive(Debug, Clone, PartialEq)]
pub enum CellError {
    /// The reversal set could not be embedded on the query side.
    Set(SetError),
    /// A row the gate admitted and an embedder could not place.
    Score(ScoreError),
    /// The controls of one (embedder, register, scoring) did not land, and
    /// the failure is a cache miss, which is a fact about the inputs rather
    /// than a reading about the cell.
    Unscorable {
        /// The embedder.
        embedder: String,
        /// The register.
        register: PairRegister,
        /// The scoring's tag.
        scoring: &'static str,
        /// The row.
        error: ScoreError,
    },
    /// A metric could not be reported for a reason that is the instrument's.
    Metric {
        /// The embedder.
        embedder: String,
        /// The register.
        register: PairRegister,
        /// The cell.
        cell: String,
        /// Why.
        error: PairMetricError,
    },
    /// The intent register has no row: no pair's turn stated an intent.
    NoIntentRows,
}

impl fmt::Display for CellError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Set(error) => write!(f, "the reversal set: {error}"),
            Self::Score(error) => write!(f, "{error}"),
            Self::Unscorable {
                embedder,
                register,
                scoring,
                error,
            } => write!(
                f,
                "{embedder}/{}/{scoring}: the controls could not be scored: {error}",
                register.tag()
            ),
            Self::Metric {
                embedder,
                register,
                cell,
                error,
            } => write!(f, "{embedder}/{}/{cell}: {error}", register.tag()),
            Self::NoIntentRows => write!(
                f,
                "no pair's turn stated an intent, so the intent register has no row"
            ),
        }
    }
}

impl Error for CellError {}

/// One scored cell of a pairs run.
#[derive(Debug, Clone, PartialEq)]
pub struct CellReport {
    /// The embedder.
    pub embedder: String,
    /// The register.
    pub register: PairRegister,
    /// The scoring's tag.
    pub scoring: &'static str,
    /// The gate.
    pub gate: PairGate,
    /// The metrics that computed, each with its demonstrated failure.
    pub reported: Vec<PairReported>,
    /// The metrics that had no value on this cell, typed.
    pub undefined: Vec<(PairMetric, usize, UndefinedCause)>,
    /// Precision at every budget, by source.
    pub by_source: Vec<(usize, PairSource, Fraction)>,
    /// [`precision_admitted_only`] at every budget: the pooled figure
    /// restricted to nominations the gate actually admitted, beside
    /// [`PairMetric::PrecisionAtK`]'s pooled reading and never replacing it.
    pub admitted_only: Vec<(usize, Fraction)>,
    /// The shuffled-label null over this cell's rows.
    pub null: Option<Null>,
    /// The scored rows, in register order: the score-difference bootstrap's
    /// input, and the drive and label the precision bootstrap needs besides.
    pub rows: Vec<ScoredPair>,
}

impl CellReport {
    /// The rows' scores alone, in register order, for a bootstrap that
    /// compares raw scores rather than a pooled metric.
    #[must_use]
    pub fn scores(&self) -> Vec<f64> {
        self.rows.iter().map(|row| row.score).collect()
    }
}

/// A cell whose controls did not land, reported as that.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlFailed {
    /// The embedder.
    pub embedder: String,
    /// The register.
    pub register: PairRegister,
    /// The scoring's tag.
    pub scoring: &'static str,
    /// What happened.
    pub failure: ControlFailure,
}

/// What a cell came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Numbers.
    Scored(CellReport),
    /// The control reading that stopped them.
    ControlFailed(ControlFailed),
}

impl CellReport {
    /// How a cell is named where two of them are compared.
    #[must_use]
    pub fn key(&self) -> String {
        format!(
            "{}/{}/{}",
            self.register.tag(),
            self.scoring,
            self.gate.tag()
        )
    }

    /// The cell as a record value.
    #[must_use]
    pub fn value(&self) -> Value {
        let undefined = self
            .undefined
            .iter()
            .map(|(metric, budget, cause)| {
                Value::Object(BTreeMap::from([
                    ("metric".to_owned(), Value::String(metric.tag().to_owned())),
                    (
                        "budget".to_owned(),
                        Value::Integer(i64::try_from(*budget).unwrap_or(i64::MAX)),
                    ),
                    ("cause".to_owned(), Value::String(cause.to_string())),
                ]))
            })
            .collect();
        let by_source = self
            .by_source
            .iter()
            .map(|(budget, source, fraction)| {
                Value::Object(BTreeMap::from([
                    (
                        "budget".to_owned(),
                        Value::Integer(i64::try_from(*budget).unwrap_or(i64::MAX)),
                    ),
                    ("source".to_owned(), Value::String(source.tag().to_owned())),
                    (
                        "hits".to_owned(),
                        Value::Integer(i64::try_from(fraction.hits).unwrap_or(i64::MAX)),
                    ),
                    (
                        "of".to_owned(),
                        Value::Integer(i64::try_from(fraction.of).unwrap_or(i64::MAX)),
                    ),
                ]))
            })
            .collect();
        let admitted_only = self
            .admitted_only
            .iter()
            .map(|(budget, fraction)| {
                Value::Object(BTreeMap::from([
                    (
                        "budget".to_owned(),
                        Value::Integer(i64::try_from(*budget).unwrap_or(i64::MAX)),
                    ),
                    (
                        "hits".to_owned(),
                        Value::Integer(i64::try_from(fraction.hits).unwrap_or(i64::MAX)),
                    ),
                    (
                        "of".to_owned(),
                        Value::Integer(i64::try_from(fraction.of).unwrap_or(i64::MAX)),
                    ),
                ]))
            })
            .collect();
        let null = self.null.map_or_else(
            || Value::String("undefined".to_owned()),
            |null| {
                Value::Object(BTreeMap::from([
                    ("mean_auc".to_owned(), decimal(null.mean_auc, 4)),
                    ("mean_d_prime".to_owned(), decimal(null.mean_d_prime, 4)),
                    (
                        "shuffles".to_owned(),
                        Value::Integer(i64::from(null.shuffles)),
                    ),
                    ("at_chance".to_owned(), Value::Boolean(null.at_chance())),
                ]))
            },
        );
        Value::Object(BTreeMap::from([
            ("embedder".to_owned(), Value::String(self.embedder.clone())),
            (
                "register".to_owned(),
                Value::String(self.register.tag().to_owned()),
            ),
            ("scoring".to_owned(), Value::String(self.scoring.to_owned())),
            ("gate".to_owned(), Value::String(self.gate.tag().to_owned())),
            ("result".to_owned(), Value::String("scored".to_owned())),
            (
                "metrics".to_owned(),
                Value::Array(self.reported.iter().map(PairReported::record).collect()),
            ),
            ("undefined".to_owned(), Value::Array(undefined)),
            ("by_source".to_owned(), Value::Array(by_source)),
            ("admitted_only".to_owned(), Value::Array(admitted_only)),
            ("null".to_owned(), null),
        ]))
    }
}

impl ControlFailed {
    /// The cell as a record value, in the sense instrument's shape.
    #[must_use]
    pub fn value(&self) -> Value {
        let mut members = BTreeMap::from([
            ("embedder".to_owned(), Value::String(self.embedder.clone())),
            (
                "register".to_owned(),
                Value::String(self.register.tag().to_owned()),
            ),
            ("scoring".to_owned(), Value::String(self.scoring.to_owned())),
            (
                "result".to_owned(),
                Value::String("control_failed".to_owned()),
            ),
            (
                "failure".to_owned(),
                Value::String(self.failure.to_string()),
            ),
        ]);
        let readings = match &self.failure {
            ControlFailure::NotAtTop {
                control,
                score,
                row,
                other,
            }
            | ControlFailure::NotAtBottom {
                control,
                score,
                row,
                other,
            } => {
                members.insert(
                    "control".to_owned(),
                    Value::String(control.tag().to_owned()),
                );
                BTreeMap::from([
                    ("control_score".to_owned(), decimal(*score, 8)),
                    ("row".to_owned(), Value::String(row.clone())),
                    ("row_score".to_owned(), decimal(*other, 8)),
                ])
            }
            ControlFailure::Inverted { top, bottom } => BTreeMap::from([
                ("top_score".to_owned(), decimal(*top, 8)),
                ("bottom_score".to_owned(), decimal(*bottom, 8)),
            ]),
            ControlFailure::Missing { control } => {
                members.insert(
                    "control".to_owned(),
                    Value::String(control.tag().to_owned()),
                );
                BTreeMap::new()
            }
            ControlFailure::Unscorable(_) => BTreeMap::new(),
        };
        members.insert("readings".to_owned(), Value::Object(readings));
        Value::Object(members)
    }
}

impl Outcome {
    /// The cell as a record value.
    #[must_use]
    pub fn value(&self) -> Value {
        match self {
            Self::Scored(cell) => cell.value(),
            Self::ControlFailed(cell) => cell.value(),
        }
    }
}

/// Every metric at every pre-registered budget over `scored`, the by-source
/// split, and the null, as one cell.
fn cell_over(
    embedder: &str,
    register: PairRegister,
    scoring: &'static str,
    gate: PairGate,
    scored: &[ScoredPair],
    seed: u64,
) -> Result<CellReport, CellError> {
    let mut reported = Vec::new();
    let mut undefined = Vec::new();
    let mut by_source = Vec::new();
    let mut admitted_only = Vec::new();
    for budget in sense::BUDGETS.iter().copied() {
        for metric in PairMetric::ALL.iter().copied() {
            match PairReported::take(metric, budget, scored) {
                Ok(taken) => reported.push(taken),
                Err(PairMetricError::Undefined {
                    on: sense::MetricSubject::Subject,
                    cause,
                    ..
                }) => undefined.push((metric, budget, cause)),
                Err(error) => {
                    return Err(CellError::Metric {
                        embedder: embedder.to_owned(),
                        register,
                        cell: format!("{scoring}/{}", gate.tag()),
                        error,
                    });
                }
            }
        }
        for source in PairSource::ALL.iter().copied() {
            by_source.push((budget, source, precision_by_source(scored, budget, source)));
        }
        admitted_only.push((budget, precision_admitted_only(scored, budget)));
    }
    Ok(CellReport {
        embedder: embedder.to_owned(),
        register,
        scoring,
        gate,
        reported,
        undefined,
        by_source,
        admitted_only,
        null: sense::shuffled_null(&as_scored(scored), sense::NULL_SHUFFLES, seed),
        rows: scored.to_vec(),
    })
}

/// Every cell of one embedder over the intent register: a scored cell per
/// (scoring, gate), or one `control_failed` per scoring whose controls did
/// not land.
///
/// # Errors
///
/// [`CellError`] when the register has no intent row, a control could not be
/// scored, a row could not be placed, or a metric's own fixture did not fail.
pub fn intent_cells(
    pairs: &[Pair],
    roles: Roles<'_>,
    seed: u64,
) -> Result<Vec<Outcome>, CellError> {
    let rows = intent_rows(pairs);
    if rows.is_empty() {
        return Err(CellError::NoIntentRows);
    }
    let mut cells = Vec::new();
    for scoring in PairScoring::ALL.iter().copied() {
        match pair_controls(&rows, roles, scoring) {
            Ok(()) => {}
            Err(ControlFailure::Unscorable(error)) => {
                return Err(CellError::Unscorable {
                    embedder: roles.id.to_owned(),
                    register: PairRegister::Intent,
                    scoring: scoring.tag(),
                    error,
                });
            }
            Err(failure) => {
                cells.push(Outcome::ControlFailed(ControlFailed {
                    embedder: roles.id.to_owned(),
                    register: PairRegister::Intent,
                    scoring: scoring.tag(),
                    failure,
                }));
                continue;
            }
        }
        for gate in PairGate::ALL.iter().copied() {
            let scored =
                score_pairs(&rows, roles, PairCell { scoring, gate }).map_err(CellError::Score)?;
            cells.push(Outcome::Scored(cell_over(
                roles.id,
                PairRegister::Intent,
                scoring.tag(),
                gate,
                &scored,
                seed,
            )?));
        }
    }
    Ok(cells)
}

/// Every cell of one embedder over the sense register, against the shipped
/// reversal senses embedded on the query side.
///
/// # Errors
///
/// [`CellError`] when the set could not be embedded, a control could not be
/// scored, a row could not be placed, or a metric's own fixture did not fail.
pub fn sense_cells(
    turns: &[Turn],
    senses: &[sense::Sense],
    roles: Roles<'_>,
    seed: u64,
) -> Result<Vec<Outcome>, CellError> {
    let set = EmbeddedSet::embed(senses, sense::SenseSet::Reversal, roles.query)
        .map_err(CellError::Set)?;
    let mut cells = Vec::new();
    for scoring in Scoring::ALL.iter().copied() {
        match turn_controls(turns, &set, roles.document, scoring) {
            Ok(()) => {}
            Err(ControlFailure::Unscorable(error)) => {
                return Err(CellError::Unscorable {
                    embedder: roles.id.to_owned(),
                    register: PairRegister::Sense,
                    scoring: scoring.tag(),
                    error,
                });
            }
            Err(failure) => {
                cells.push(Outcome::ControlFailed(ControlFailed {
                    embedder: roles.id.to_owned(),
                    register: PairRegister::Sense,
                    scoring: scoring.tag(),
                    failure,
                }));
                continue;
            }
        }
        for gate in PairGate::ALL.iter().copied() {
            let scored = score_turns(turns, &set, roles.document, TurnCell { scoring, gate })
                .map_err(CellError::Score)?;
            cells.push(Outcome::Scored(cell_over(
                roles.id,
                PairRegister::Sense,
                scoring.tag(),
                gate,
                &scored,
                seed,
            )?));
        }
    }
    Ok(cells)
}

// ---------------------------------------------------------------------------
// the pre-registration
// ---------------------------------------------------------------------------

/// The hypothesis the claim row carries: the ratified text's, verbatim.
pub const HYPOTHESIS: &str = "An entry recorded in the working object at turn N can be matched \
    against the prose of a later turn sharply enough to nominate supersession at a fixed \
    per-session budget without over-firing on mentions that do not supersede -- in each of the \
    two registers #17 names, intent-to-intent and authored-sense -- and the literal tier's \
    anchors, applied as a pre-gate, do not lower precision at that budget.";

/// The primary endpoint, as amended: top k per drive, pooled.
pub const PRIMARY: &str = "precision at a fixed nomination budget of k = 5 per drive, the top k \
    of each drive pooled across drives, on the intent register, per embedder, scoring and gate";

/// The separation endpoint.
pub const SEPARATION: &str = "the area under the curve and the standardised separation, per cell, \
    positives against every other row";

/// The over-firing endpoint.
pub const OVER_FIRING: &str = "the share of hard-negative rows -- later turns that name the entry's \
    anchors without superseding it -- nominated within the pooled per-drive budget";

/// How significance is tested and corrected.
pub const CORRECTION: &str = "paired bootstrap across embedders within a cell and between the two \
    gate arms within an (embedder, scoring), Holm-corrected together, the attainable p floor \
    printed beside every p";

/// The pre-registration of a pairs run, as a record value: what the results
/// directory's `pre-registration.json` carries and is pinned to by digest.
///
/// Every reading the ratified text left to the instrument is spelled here so
/// that the adjudication has none to disclose: which file is which register,
/// which scorings have a pairwise form, how the turn's intent is derived, what
/// the turn surface is, what anchored means, and why a pair without an intent
/// is not a row.
/// The two registers, as the pre-registration describes them.
fn registers_value() -> Value {
    let text = |value: &str| Value::String(value.to_owned());
    let list = |values: Vec<String>| Value::Array(values.into_iter().map(Value::String).collect());
    let cells = |scorings: Vec<&'static str>| {
        list(
            scorings
                .into_iter()
                .flat_map(|scoring| {
                    PairGate::ALL
                        .iter()
                        .map(move |gate| format!("{scoring}+{}", gate.tag()))
                })
                .collect(),
        )
    };
    Value::Object(BTreeMap::from([
        (
            "intent".to_owned(),
            Value::Object(BTreeMap::from([
                ("file".to_owned(), text(PairRegister::Intent.file_name())),
                ("role".to_owned(), text("primary")),
                (
                    "row".to_owned(),
                    text(
                        "one (entry, later turn) pair; a pair whose turn stated no intent \
                                  is not a row of this register and is counted",
                    ),
                ),
                (
                    "cells".to_owned(),
                    cells(
                        PairScoring::ALL
                            .iter()
                            .map(|scoring| scoring.tag())
                            .collect(),
                    ),
                ),
                (
                    "raw_cosine".to_owned(),
                    text(
                        "cosine of the entry's text against the turn's stated intent, the \
                                  surface the collector ships",
                    ),
                ),
                (
                    "ensemble_max".to_owned(),
                    text(
                        "the best cosine over the entry's surfaces (text; its origin step's \
                                  stated intent) against the turn's (stated intent; prose)",
                    ),
                ),
                (
                    "not_cells".to_owned(),
                    text(
                        "contrastive and softmax: they need an authored set with a negative \
                                  pole, and a pair has none",
                    ),
                ),
            ])),
        ),
        (
            "sense".to_owned(),
            Value::Object(BTreeMap::from([
                ("file".to_owned(), text(PairRegister::Sense.file_name())),
                ("role".to_owned(), text("the authored-sense register")),
                (
                    "row".to_owned(),
                    text(
                        "one later turn, its prose against the shipped reversal senses; \
                                  positive if any judged pair at the turn supersedes, hard_negative \
                                  if none does and one mentions, negative otherwise",
                    ),
                ),
                (
                    "cells".to_owned(),
                    cells(Scoring::ALL.iter().map(|scoring| scoring.tag()).collect()),
                ),
            ])),
        ),
    ]))
}

/// The anchored pre-gate, as the pre-registration describes it.
fn gate_value() -> Value {
    let text = |value: &str| Value::String(value.to_owned());
    Value::Object(BTreeMap::from([
        (
            "anchored".to_owned(),
            text(
                "at least two distinct anchors of the entry, by the collector's tier 0 \
                          (identifiers, paths, quoted terms; never an English word, by shape), \
                          recur whole-token in the turn's prose or tool output",
            ),
        ),
        (
            "anchors_required".to_owned(),
            Value::Integer(i64::try_from(ANCHORS_REQUIRED).unwrap_or(i64::MAX)),
        ),
        (
            "turn_rows".to_owned(),
            text(
                "a turn row carries the flag its builder computed over every live entry \
                          of the drive",
            ),
        ),
    ]))
}

#[must_use]
pub fn pre_registration() -> Value {
    let text = |value: &str| Value::String(value.to_owned());
    let list = |values: Vec<String>| Value::Array(values.into_iter().map(Value::String).collect());
    Value::Object(BTreeMap::from([
        ("hypothesis".to_owned(), text(HYPOTHESIS)),
        ("primary".to_owned(), text(PRIMARY)),
        ("separation".to_owned(), text(SEPARATION)),
        ("over_firing".to_owned(), text(OVER_FIRING)),
        (
            "by_source".to_owned(),
            text("precision at every budget broken out by source, planted against mined, beside \
                  the pooled figure; the pooled verdict stands and the split says whether it was \
                  carried by the planted half"),
        ),
        ("correction".to_owned(), text(CORRECTION)),
        (
            "budgets".to_owned(),
            Value::Array(
                sense::BUDGETS
                    .iter()
                    .map(|k| Value::Integer(i64::try_from(*k).unwrap_or(i64::MAX)))
                    .collect(),
            ),
        ),
        ("registers".to_owned(), registers_value()),
        ("gate".to_owned(), gate_value()),
        (
            "intent".to_owned(),
            text("not a field in any archived drive; derived by the gym's own router::stated_intent \
                  over the step's reasoning followed by its visible content, fenced code stripped; \
                  an entry's intent is its origin step's"),
        ),
        (
            "turn_surface".to_owned(),
            text("the step's prose (reasoning then content) and its tool calls as the register \
                  carries them; anchors are scanned over prose and tool output, embeddings are \
                  taken over intent and prose only"),
        ),
        (
            "roles".to_owned(),
            text("entries, their intents and the senses are queries; turn intents, turn prose and \
                  the control words are documents; one cache per role per embedder, or one cache \
                  for both where the embedder prefixes neither side"),
        ),
        (
            "controls".to_owned(),
            list(vec![
                "verbatim_positive: the first positive's entry against itself, first under every scoring".to_owned(),
                "unrelated_words: the same entry against the control words, no positive below it".to_owned(),
                "the sense register: the sense instrument's own extremes".to_owned(),
                "shuffled-label null per cell, reported beside it".to_owned(),
                "every metric's two-drive failure fixture, demonstrated at every budget".to_owned(),
            ]),
        ),
        (
            "resamples".to_owned(),
            Value::Integer(i64::from(sense::PRE_REGISTRATION.resamples)),
        ),
        (
            "attainable_p_floor".to_owned(),
            decimal(
                sense::attainable_p_floor(sense::PRE_REGISTRATION.resamples),
                6,
            ),
        ),
        (
            "null".to_owned(),
            Value::Object(BTreeMap::from([
                ("shuffles".to_owned(), Value::Integer(i64::from(sense::NULL_SHUFFLES))),
                ("auc_band".to_owned(), decimal(sense::NULL_AUC_BAND, 4)),
                ("d_prime_band".to_owned(), decimal(sense::NULL_D_PRIME_BAND, 4)),
            ])),
        ),
        (
            "ratified".to_owned(),
            text("#17, 2026-09-19, as amended: matched floor cell; the ladder named; top k per \
                  drive pooled; source planted|mined broken out"),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use super::{
        ANCHORS_REQUIRED, CellError, Outcome, Pair, PairCell, PairGate, PairMetric,
        PairMetricError, PairReported, PairScoring, PairSource, Roles, ScoredPair, Tool, anchored,
        intent_cells, over_firing, pair_controls, pairs, pooled_top_k, precision_admitted_only,
        precision_at_k, precision_bootstrap, precision_by_source, recurring_anchors, score_pairs,
        sense_cells, turns,
    };
    use crate::capture::sense::{self, Control, ControlFailure, Embedder, Fixture, Label};

    fn row(id: &str, drive: &str, label: Label, source: PairSource, score: f64) -> ScoredPair {
        ScoredPair {
            id: id.to_owned(),
            drive: drive.to_owned(),
            source,
            label,
            score,
            admitted: true,
        }
    }

    /// A row the gate rejected, sitting at the scoring's floor -- what a
    /// drive with too few anchored rows fills its budget with.
    fn rejected(id: &str, drive: &str, label: Label, source: PairSource, score: f64) -> ScoredPair {
        ScoredPair {
            admitted: false,
            ..row(id, drive, label, source, score)
        }
    }

    /// Two drives; in each the best-scored row is a negative and the second a
    /// positive, so a global top-2 and a per-drive top-1 disagree.
    fn two_drives() -> Vec<ScoredPair> {
        vec![
            row("a1", "a", Label::Negative, PairSource::Mined, 0.9),
            row("a2", "a", Label::Positive, PairSource::Planted, 0.8),
            row("a3", "a", Label::HardNegative, PairSource::Mined, 0.1),
            row("b1", "b", Label::Negative, PairSource::Mined, 0.5),
            row("b2", "b", Label::Positive, PairSource::Mined, 0.4),
            row("b3", "b", Label::HardNegative, PairSource::Mined, 0.3),
        ]
    }

    #[test]
    fn the_budget_is_spent_per_drive_and_the_top_k_pooled() {
        let rows = two_drives();
        let pooled: Vec<&str> = pooled_top_k(&rows, 1)
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(
            pooled,
            ["a1", "b1"],
            "one nomination per drive, not the global top two"
        );
        let at_one = precision_at_k(&rows, 1);
        assert_eq!((at_one.hits, at_one.of), (0, 2));
        let at_two = precision_at_k(&rows, 2);
        assert_eq!(
            (at_two.hits, at_two.of),
            (2, 4),
            "each drive's second row is its positive"
        );
    }

    #[test]
    fn over_firing_counts_every_hard_negative_and_the_pooled_nominations_of_them() {
        let rows = two_drives();
        let none = over_firing(&rows, 2);
        assert_eq!((none.hits, none.of), (0, 2));
        let all = over_firing(&rows, 3);
        assert_eq!((all.hits, all.of), (2, 2));
    }

    #[test]
    fn precision_is_broken_out_by_source_over_the_same_pooled_nominations() {
        let rows = two_drives();
        let planted = precision_by_source(&rows, 2, PairSource::Planted);
        let mined = precision_by_source(&rows, 2, PairSource::Mined);
        assert_eq!((planted.hits, planted.of), (1, 1));
        assert_eq!((mined.hits, mined.of), (1, 3));
    }

    #[test]
    fn a_drive_with_too_few_anchored_rows_fills_its_budget_with_a_rejected_row_the_pooled_figure_counts_and_the_admitted_only_figure_does_not()
     {
        // Drive `a` has one anchored (admitted) row; drive `b` has none, so
        // its only row at k = 2 is the rejected one sitting at the floor.
        let rows = vec![
            row("a1", "a", Label::Positive, PairSource::Mined, 0.9),
            rejected("b1", "b", Label::Negative, PairSource::Mined, -1.0),
        ];
        let pooled = precision_at_k(&rows, 2);
        assert_eq!(
            (pooled.hits, pooled.of),
            (1, 2),
            "the pooled figure nominates the rejected row to fill the budget"
        );
        let admitted = precision_admitted_only(&rows, 2);
        assert_eq!(
            (admitted.hits, admitted.of),
            (1, 1),
            "the admitted-only figure is over nominations the gate actually made"
        );
    }

    /// Two drives where every drive's positive outranks its negative -- the
    /// pattern a gate that helps precision would produce.
    fn two_drives_positive_on_top() -> Vec<ScoredPair> {
        vec![
            row("a1", "a", Label::Positive, PairSource::Planted, 0.9),
            row("a2", "a", Label::Negative, PairSource::Mined, 0.8),
            row("a3", "a", Label::HardNegative, PairSource::Mined, 0.1),
            row("b1", "b", Label::Positive, PairSource::Mined, 0.5),
            row("b2", "b", Label::Negative, PairSource::Mined, 0.4),
            row("b3", "b", Label::HardNegative, PairSource::Mined, 0.3),
        ]
    }

    #[test]
    fn the_precision_bootstrap_reads_the_pooled_precision_difference_between_two_arms() {
        let with = two_drives_positive_on_top();
        let without = two_drives();
        let test = precision_bootstrap(&with, &without, 1, 999, 7).expect("a bootstrap");
        assert!(
            (test.observed - 1.0).abs() < 1e-12,
            "every drive's positive leads at k = 1 in `with` and neither does in `without`: \
             observed {}",
            test.observed
        );
        assert_eq!(test.resamples, 999);
        assert!(
            (test.p - test.p_floor).abs() < 1e-12,
            "the difference cannot cross zero on this fixture whichever drives are resampled, so \
             p sits at the attainable floor: p {} floor {}",
            test.p,
            test.p_floor
        );
    }

    #[test]
    fn the_precision_bootstrap_pools_counts_across_resampled_drives_rather_than_averaging_fractions()
     {
        // Drive `a` has two admitted rows at k = 2 (both positive, precision
        // 2/2); drive `b` has only one row at all, so its own top-2 is just
        // that one row (positive, precision 1/1). Pooled over both drives:
        // 3 hits of 3 nominated, not the mean of two 1.0 fractions -- the
        // same number either way here, so a second drive is added with a
        // worse precision to force pooled and averaged readings apart.
        let with = vec![
            row("a1", "a", Label::Positive, PairSource::Mined, 0.9),
            row("a2", "a", Label::Positive, PairSource::Mined, 0.8),
            row("b1", "b", Label::Negative, PairSource::Mined, 0.5),
        ];
        let without = vec![
            row("a1", "a", Label::Negative, PairSource::Mined, 0.9),
            row("a2", "a", Label::Negative, PairSource::Mined, 0.8),
            row("b1", "b", Label::Negative, PairSource::Mined, 0.5),
        ];
        let test = precision_bootstrap(&with, &without, 2, 1, 7).expect("a bootstrap");
        // `with`'s pooled precision: drive a contributes 2/2, drive b 0/1;
        // pooled is 2/3, not the mean of a's fraction (1.0) and b's (0.0),
        // which would be 0.5.
        assert!(
            (test.observed - (2.0 / 3.0)).abs() < 1e-12,
            "pooled precision over both drives' counts is 2/3, not the mean of their fractions: \
             observed {}",
            test.observed
        );
    }

    #[test]
    fn the_precision_bootstrap_refuses_arms_that_do_not_share_drives() {
        let with = two_drives();
        let mut without = two_drives();
        without.retain(|row| row.drive != "b");
        let error = precision_bootstrap(&with, &without, 1, 10, 7).expect_err("unpaired drives");
        assert!(
            matches!(error, sense::BootstrapError::Unpaired { a: 2, b: 1 }),
            "unexpected error: {error:?}"
        );
    }

    #[test]
    fn the_precision_bootstrap_refuses_no_drives_and_no_resamples() {
        let empty = precision_bootstrap(&[], &[], 1, 10, 7).expect_err("no drives");
        assert!(matches!(empty, sense::BootstrapError::Empty), "{empty:?}");
        let rows = two_drives();
        let no_resamples = precision_bootstrap(&rows, &rows, 1, 0, 7).expect_err("no resamples");
        assert!(
            matches!(no_resamples, sense::BootstrapError::NoResamples),
            "{no_resamples:?}"
        );
    }

    #[test]
    fn every_pooled_metric_fails_on_its_two_drive_fixture_at_every_budget() {
        for metric in PairMetric::ALL {
            for budget in sense::BUDGETS {
                let value = metric
                    .compute(*budget, &metric.failure_fixture())
                    .unwrap_or_else(|why| {
                        panic!("{} undefined on its fixture: {why}", metric.tag())
                    });
                assert!(
                    metric.sense_metric().failed(value),
                    "{} read {value} on its fixture at budget {budget}",
                    metric.tag()
                );
            }
        }
    }

    #[test]
    fn a_reported_metric_carries_its_budget_and_value() {
        let rows = two_drives();
        let taken = PairReported::take(PairMetric::PrecisionAtK, 2, &rows).expect("reported");
        assert!((taken.value() - 0.5).abs() < 1e-12);
        assert_eq!(taken.budget(), 2);
        assert_eq!(taken.metric(), PairMetric::PrecisionAtK);
    }

    /// The refusal the demonstrated-failure discipline exists for: at a budget
    /// the ladder does not name, past the widest rung, the pooled precision
    /// fixture's top-k reaches its positives and no longer reads as failure --
    /// and a metric whose own fixture did not fail is not reported, whatever
    /// the subject scored. The sense instrument has the same test.
    #[test]
    fn a_metric_whose_fixture_did_not_fail_is_refused_not_reported() {
        let rows = two_drives();
        let past = sense::widest_budget() + 1;
        match PairReported::take(PairMetric::PrecisionAtK, past, &rows) {
            Err(PairMetricError::InstrumentNeverFailed {
                metric,
                budget,
                value,
                reading,
            }) => {
                assert_eq!(metric, PairMetric::PrecisionAtK);
                assert_eq!(budget, past);
                assert!(
                    value > reading,
                    "the fixture read {value}, which is not the failure {reading}"
                );
            }
            other => panic!("a metric whose fixture did not fail was reported: {other:?}"),
        }
    }

    fn pair(id: &str, entry: &str, prose: &str, intent: Option<&str>, label: Label) -> Pair {
        Pair {
            id: id.to_owned(),
            drive: "d".to_owned(),
            entry: entry.to_owned(),
            entry_intent: None,
            turn_intent: intent.map(str::to_owned),
            turn_prose: prose.to_owned(),
            turn_tools: Vec::new(),
            label,
            source: PairSource::Mined,
        }
    }

    // -----------------------------------------------------------------------
    // control-failure test embedders
    //
    // `pair_controls`'s own success path is exercised above
    // (`the_controls_land_under_the_fixture_embedder`); these three embedders
    // force each of its four failure branches, mirroring `sense`'s own
    // `Constant`/`Impostor`/`Placed` test embedders (private to that
    // module's tests, so not reusable here directly).
    // -----------------------------------------------------------------------

    /// An embedder that places every text at the same point. The controls
    /// must catch it, or they catch nothing.
    struct Constant;

    impl Constant {
        const ID: &'static str = "constant";
    }

    impl Embedder for Constant {
        fn embed(&self, _text: &str) -> Vec<f64> {
            vec![1.0, 1.0, 1.0]
        }
        fn id(&self) -> &str {
            Self::ID
        }
    }

    /// The fixture embedder with one text placed exactly where another sits:
    /// an embedder that cannot tell a register row from the sense itself.
    struct Impostor {
        text: String,
        as_if: String,
    }

    impl Impostor {
        const ID: &'static str = "impostor";
    }

    impl Embedder for Impostor {
        fn embed(&self, text: &str) -> Vec<f64> {
            if self.text == text {
                Fixture.embed(&self.as_if)
            } else {
                Fixture.embed(text)
            }
        }
        fn id(&self) -> &str {
            Self::ID
        }
    }

    /// An embedder reading its vectors from a table. The fixture embedder's
    /// components are token counts, so every cosine it produces is
    /// non-negative and no row it places can go under a control; a real
    /// model's components have signs, and this one lets a test put a row
    /// where a real model could put it.
    struct Placed(Vec<(String, Vec<f64>)>);

    impl Placed {
        const ID: &'static str = "placed";

        /// Where a text the table does not name sits: between the extremes,
        /// so a row that trips a control is the row the test placed there.
        const ELSEWHERE: [f64; 2] = [1.0, 1.0];

        fn at(pairs: &[(&str, [f64; 2])]) -> Self {
            Self(
                pairs
                    .iter()
                    .map(|(text, vector)| ((*text).to_owned(), vector.to_vec()))
                    .collect(),
            )
        }
    }

    impl Embedder for Placed {
        fn embed(&self, text: &str) -> Vec<f64> {
            self.0
                .iter()
                .find(|(placed, _)| placed == text)
                .map_or_else(|| Self::ELSEWHERE.to_vec(), |(_, vector)| vector.clone())
        }
        fn id(&self) -> &str {
            Self::ID
        }
    }

    #[test]
    fn a_register_with_no_positive_refuses_the_top_control_by_name() {
        let roles = Roles {
            id: "fixture",
            query: &Fixture,
            document: &Fixture,
        };
        let n = pair("n", "an entry", "prose", Some("intent"), Label::Negative);
        let rows = [&n];
        let err = pair_controls(&rows, roles, PairScoring::RawCosine)
            .expect_err("no positive in the register and the controls passed");
        assert!(
            matches!(
                err,
                ControlFailure::Missing {
                    control: Control::VerbatimPositive
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn an_embedder_that_cannot_tell_texts_apart_inverts_the_controls() {
        let constant = Constant;
        let roles = Roles {
            id: "constant",
            query: &constant,
            document: &constant,
        };
        let p = pair(
            "p",
            "the resolver reads the flag",
            "Actually the resolver never read the flag.",
            Some("Let me check the resolver."),
            Label::Positive,
        );
        let rows = [&p];
        for scoring in PairScoring::ALL {
            let err = pair_controls(&rows, roles, *scoring)
                .expect_err("an embedder that cannot tell texts apart passed the controls");
            assert!(
                matches!(err, ControlFailure::Inverted { .. }),
                "{}: {err:?}",
                scoring.tag()
            );
        }
    }

    #[test]
    fn a_register_row_that_reaches_the_top_control_is_named_as_the_row_that_displaced_it() {
        let p = pair(
            "p",
            "the resolver reads the flag",
            "Actually the resolver never read the flag.",
            Some("Let me check the resolver."),
            Label::Positive,
        );
        let n = pair(
            "n",
            "an unrelated note",
            "Reading the docs.",
            Some("Let me read."),
            Label::Negative,
        );
        let impostor = Impostor {
            text: n.turn_intent.clone().expect("n states an intent"),
            as_if: n.entry.clone(),
        };
        let roles = Roles {
            id: "impostor",
            query: &impostor,
            document: &impostor,
        };
        let rows = [&p, &n];
        let err = pair_controls(&rows, roles, PairScoring::RawCosine)
            .expect_err("a register row reached the top control and the controls passed");
        let ControlFailure::NotAtTop { row, .. } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(
            row, "n",
            "the wrong row was named as having displaced the control"
        );
    }

    #[test]
    fn a_positive_row_under_the_bottom_control_is_named_as_the_row_that_went_under_it() {
        let p = pair(
            "p",
            "the resolver reads the flag",
            "Actually the resolver never read the flag.",
            Some("Let me check the resolver."),
            Label::Positive,
        );
        let u = pair(
            "u",
            "the resolver reads the flag",
            "the resolver reads the flag",
            Some("the opposite of the resolver"),
            Label::Positive,
        );
        let placed = Placed::at(&[
            (p.entry.as_str(), [1.0, 0.0]),
            (sense::UNRELATED, [0.0, 1.0]),
            (
                u.turn_intent.as_deref().expect("u states an intent"),
                [-1.0, 0.0],
            ),
        ]);
        let roles = Roles {
            id: "placed",
            query: &placed,
            document: &placed,
        };
        let rows = [&p, &u];
        let err = pair_controls(&rows, roles, PairScoring::RawCosine)
            .expect_err("a positive went under the bottom control and the controls passed");
        let ControlFailure::NotAtBottom { row, .. } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(
            row, "u",
            "the wrong row was named as having gone under the control"
        );
    }

    #[test]
    fn the_gate_admits_two_recurring_anchors_and_not_one() {
        let entry = "`parse_args` in src/cli.rs returns a Vec";
        let one = pair(
            "one",
            entry,
            "I will read parse_args first.",
            None,
            Label::Positive,
        );
        let two = pair(
            "two",
            entry,
            "Reading parse_args in src/cli.rs now.",
            None,
            Label::Positive,
        );
        assert_eq!(recurring_anchors(entry, &one.turn_prose, &[]).len(), 1);
        assert!(
            !anchored(&one),
            "one anchor is tier 0's threshold, not this gate's"
        );
        assert!(recurring_anchors(entry, &two.turn_prose, &[]).len() >= ANCHORS_REQUIRED);
        assert!(anchored(&two));
    }

    #[test]
    fn an_anchor_in_tool_output_counts_as_tier_zero_scans_it() {
        let entry = "`parse_args` in src/cli.rs returns a Vec";
        let mut prose_only = pair("t", entry, "Let me look.", None, Label::Positive);
        assert!(!anchored(&prose_only));
        prose_only.turn_tools.push(Tool {
            command: "grep -n parse_args src/cli.rs".to_owned(),
            output: "src/cli.rs:41: fn parse_args".to_owned(),
        });
        assert!(anchored(&prose_only));
    }

    #[test]
    fn a_pair_whose_turn_stated_no_intent_is_not_a_row_of_the_intent_register() {
        let rows = vec![
            pair(
                "with",
                "an entry",
                "prose",
                Some("Let me read it."),
                Label::Positive,
            ),
            pair("without", "an entry", "prose", None, Label::Negative),
        ];
        let kept: Vec<&str> = super::intent_rows(&rows)
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(kept, ["with"]);
    }

    #[test]
    fn ensemble_max_is_never_below_raw_cosine_on_the_same_pair() {
        let roles = Roles {
            id: "fixture",
            query: &Fixture,
            document: &Fixture,
        };
        let mut p = pair(
            "p",
            "the resolver reads the flag",
            "Actually the resolver never read the flag.",
            Some("Let me check the resolver."),
            Label::Positive,
        );
        p.entry_intent = Some("I will read the resolver.".to_owned());
        let raw = PairScoring::RawCosine.score(&p, roles).expect("a score");
        let best = PairScoring::EnsembleMax.score(&p, roles).expect("a score");
        assert!(
            best >= raw - 1e-12,
            "the max over surfaces includes the raw surface"
        );
    }

    #[test]
    fn a_gated_out_pair_sits_at_the_floor() {
        let roles = Roles {
            id: "fixture",
            query: &Fixture,
            document: &Fixture,
        };
        let p = pair(
            "p",
            "an entry with no anchors",
            "a turn",
            Some("Let me go."),
            Label::Negative,
        );
        let rows = [&p];
        let scored = score_pairs(
            &rows,
            roles,
            PairCell {
                scoring: PairScoring::RawCosine,
                gate: PairGate::Anchored,
            },
        )
        .expect("scored");
        assert!(!scored[0].admitted);
        assert!((scored[0].score - PairScoring::RawCosine.floor()).abs() < 1e-12);
    }

    #[test]
    fn the_controls_land_under_the_fixture_embedder() {
        let roles = Roles {
            id: "fixture",
            query: &Fixture,
            document: &Fixture,
        };
        let p = pair(
            "p",
            "the resolver reads the flag",
            "Actually the resolver never read the flag.",
            Some("Let me check the resolver."),
            Label::Positive,
        );
        let n = pair(
            "n",
            "an unrelated note",
            "Reading the docs.",
            Some("Let me read."),
            Label::Negative,
        );
        let rows = [&p, &n];
        for scoring in PairScoring::ALL {
            pair_controls(&rows, roles, *scoring)
                .unwrap_or_else(|why| panic!("{}: {why}", scoring.tag()));
        }
    }

    #[test]
    fn a_register_with_no_intent_row_is_refused_by_name() {
        let roles = Roles {
            id: "fixture",
            query: &Fixture,
            document: &Fixture,
        };
        let rows = vec![pair("without", "an entry", "prose", None, Label::Positive)];
        assert_eq!(
            intent_cells(&rows, roles, 1).unwrap_err(),
            CellError::NoIntentRows
        );
    }

    #[test]
    fn a_run_over_the_fixture_reports_a_cell_per_scoring_and_gate() {
        let roles = Roles {
            id: "fixture",
            query: &Fixture,
            document: &Fixture,
        };
        let mut rows = Vec::new();
        for i in 0..6 {
            rows.push(pair(
                &format!("p{i}"),
                &format!("the resolver reads flag {i} from `config_{i}.toml`"),
                &format!("Actually flag {i} in `config_{i}.toml` is never read by the resolver."),
                Some(&format!("Let me check flag {i}.")),
                Label::Positive,
            ));
            rows.push(pair(
                &format!("n{i}"),
                &format!("note {i} about the parser"),
                &format!("Reading the docs for module {i}."),
                Some(&format!("Let me read module {i}.")),
                Label::Negative,
            ));
            rows.push(pair(
                &format!("h{i}"),
                &format!("the `lexer_{i}` is slow"),
                &format!("Using `lexer_{i}` as before."),
                Some(&format!("Let me use lexer {i}.")),
                Label::HardNegative,
            ));
        }
        let cells = intent_cells(&rows, roles, 7).unwrap_or_else(|why| panic!("{why}"));
        assert_eq!(cells.len(), PairScoring::ALL.len() * PairGate::ALL.len());
        for cell in &cells {
            let Outcome::Scored(report) = cell else {
                panic!("a control failed under the fixture: {cell:?}")
            };
            assert_eq!(
                report.reported.len() + report.undefined.len(),
                PairMetric::ALL.len() * sense::BUDGETS.len()
            );
            assert_eq!(report.rows.len(), rows.len());
        }
        let senses = sense::shipped_senses().expect("senses");
        let turn_rows = turns(
            "{\"id\":\"t1\",\"drive\":\"d\",\"turn\":1,\"step\":2,\"text\":\"Actually it turns out the flag was never read.\",\"anchored\":true,\"label\":\"positive\",\"source\":\"mined\",\"pairs\":[\"p0\"]}\n\
             {\"id\":\"t2\",\"drive\":\"d\",\"turn\":1,\"step\":3,\"text\":\"Reading the docs.\",\"anchored\":false,\"label\":\"negative\",\"source\":\"mined\",\"pairs\":[\"n0\"]}\n\
             {\"id\":\"t3\",\"drive\":\"d\",\"turn\":1,\"step\":4,\"text\":\"Using the lexer as before.\",\"anchored\":true,\"label\":\"hard_negative\",\"source\":\"mined\",\"pairs\":[\"h0\"]}\n",
        )
        .expect("turn rows");
        let turn_cells =
            sense_cells(&turn_rows, &senses, roles, 7).unwrap_or_else(|why| panic!("{why}"));
        assert_eq!(
            turn_cells.len(),
            sense::Scoring::ALL.len() * PairGate::ALL.len()
        );
    }

    #[test]
    fn the_pair_schema_is_closed_and_intents_may_be_null() {
        let one = "{\"id\":\"p\",\"drive\":\"d\",\"turn\":1,\"step\":2,\"entry\":\"e\",\"turn_intent\":\"Let me.\",\"turn_prose\":\"x\",\"turn_tools\":[{\"command\":\"ls\",\"output\":\"a\",\"truncated\":false}],\"label\":\"positive\",\"source\":\"planted\"}\n";
        let rows = pairs(one).expect("a row");
        assert_eq!(rows[0].entry_intent, None);
        assert_eq!(rows[0].turn_tools.len(), 1);
        let extra = one.replace(
            "\"source\":\"planted\"",
            "\"source\":\"planted\",\"lane\":\"x\"",
        );
        assert!(pairs(&extra).is_err(), "an unknown key is refused");
        let source = one.replace("\"source\":\"planted\"", "\"source\":\"authored\"");
        assert!(
            pairs(&source).is_err(),
            "a source outside the vocabulary is refused"
        );
    }
}
