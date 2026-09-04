//! Authored senses, matched by distance: the instrument for the embedding
//! bakeoff that chooses the collector's nomination arm.
//!
//! A sense-description is judgment compiled into data. "The operator assumed
//! something that wasn't true" is authored once, by something capable of
//! judgment, and applied to every transcript sentence by something capable
//! only of distance. The hazard is that the description is abstract and the
//! sentence is concrete: **an abstract sentence matched concrete transcript
//! prose at cosine 0.82**, a number that read as a hit and was mostly a
//! property of the description sitting near everything. The relation the
//! collector wants is asymmetric -- the sentence entails the description --
//! and cosine is symmetric; an abstract description is a hub, close to every
//! concrete sentence at once. A nomination arm built on raw cosine over-fires,
//! and a bakeoff that reports only raw cosine cannot see that it does.
//!
//! So the instrument is built around the failures it has to be able to show:
//!
//! * **The scoring function is a factor, not a detail.** [`Scoring::ALL`]
//!   spans raw cosine, contrastive (the positive sense minus an authored
//!   *negative* sense -- the repair for a hub is a second hub beside it),
//!   softmax over the sense set, and the paraphrase-ensemble max. Every cell
//!   of the bakeoff names its scoring.
//! * **The lexical pre-gate is a factor.** [`Gate`] is with and without,
//!   applied before scoring, so precision at a fixed budget is measured both
//!   ways rather than assumed to improve.
//! * **Every metric ships a demonstrated failure.** [`Reported::take`]
//!   mirrors the groundedness gate's measurement: a metric that has not been
//!   seen fail on its own failure fixture is not reported. A number from an
//!   instrument that cannot fail is a number about the probe.
//! * **Every p-value travels with its attainable floor.** A paired bootstrap
//!   of `R` resamples cannot produce a p below `1/(R+1)`. [`PValue`] carries
//!   that floor and prints it, so a small register cannot report a
//!   significance it could not have reached.
//! * **The controls are rows.** The sense text verbatim must rank first; a
//!   row of unrelated words -- or the negative sense verbatim, under a scoring
//!   that subtracts it -- must rank last; and a shuffled-label null must sit
//!   at chance. A run whose controls are not at their extremes is not a run.
//!
//! Negation is why there is a `reversal` set at all. Embedding models read
//! "X" and "not X" as neighbours, so supersession is nominated by an authored,
//! positive-form description of the reversal event, never by negating the
//! recorded entry.
//!
//! **Nothing here is a result.** The pre-registered endpoints are
//! [`PRE_REGISTRATION`] and they are unfilled: the run needs archived
//! transcripts, a judge and embedders, and [`Blocker::ALL`] says so by name.
//! The register under `diet/capture/sense/register/` is authored for this
//! instrument's own tests, every row says `source: authored`, and it is not
//! the corpus.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use crate::formats::record::json::{self, Decimal, LineError, Value};

/// The sense sets, as shipped: `diet/capture/sense/sets.jsonl`.
///
/// Compiled in rather than read at run time, because the collector applies
/// them, and a nomination policy that reads its senses from wherever the
/// process happened to start is a policy whose version nobody knows.
pub const SETS: &str = include_str!("../../capture/sense/sets.jsonl");

// ---------------------------------------------------------------------------
// vocabularies
// ---------------------------------------------------------------------------

/// Which event class a sense set describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SenseSet {
    /// The operator assumed something that wasn't true.
    Mistake,
    /// A non-obvious fact about how the systems work that a new agent would
    /// re-derive at cost.
    DurableFact,
    /// A recorded limitation no longer applies. The collector's supersession
    /// signal, matched in positive form because negation does not embed.
    Reversal,
}

impl SenseSet {
    /// Every set, so a table keyed by set cannot silently omit one.
    pub const ALL: &'static [Self] = &[Self::Mistake, Self::DurableFact, Self::Reversal];

    /// The spelling the data files use.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Mistake => "mistake",
            Self::DurableFact => "durable_fact",
            Self::Reversal => "reversal",
        }
    }

    /// The set a tag names, found by iterating [`SenseSet::ALL`].
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|set| set.tag() == tag)
    }
}

/// Whether a sense describes the event class or its authored opposite.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Polarity {
    /// The event class itself.
    Positive,
    /// What the class is most often confused with, authored so that a
    /// contrastive scoring has something to subtract.
    Negative,
}

impl Polarity {
    /// Both polarities.
    pub const ALL: &'static [Self] = &[Self::Positive, Self::Negative];

    /// The spelling the data files use.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
        }
    }

    /// The polarity a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|polarity| polarity.tag() == tag)
    }
}

/// What a register row is, against its set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Label {
    /// An instance of the event class.
    Positive,
    /// Not an instance: matched prose from the same speakers.
    Negative,
    /// Not an instance, and adjacent to one: abstract discussion of the class,
    /// hypotheticals. The over-firing endpoint is measured on these.
    HardNegative,
}

impl Label {
    /// Every label.
    pub const ALL: &'static [Self] = &[Self::Positive, Self::Negative, Self::HardNegative];

    /// The spelling the register uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Positive => "positive",
            Self::Negative => "negative",
            Self::HardNegative => "hard_negative",
        }
    }

    /// The label a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|label| label.tag() == tag)
    }

    /// Whether nominating this row is a hit.
    #[must_use]
    pub fn is_positive(self) -> bool {
        matches!(self, Self::Positive)
    }

    /// Whether nominating this row is the over-firing the bakeoff exists to
    /// price.
    #[must_use]
    pub fn is_hard_negative(self) -> bool {
        matches!(self, Self::HardNegative)
    }
}

/// Where a register row came from.
///
/// This began with one variant, refusing a row that claimed to be mined
/// because nothing had mined one. Something has now: a register mined from
/// archived drives and judged under withheld controls arrives beside the
/// authored one. The two are not interchangeable and the distinction is the
/// whole reason this is a vocabulary rather than a comment. An authored row
/// states a rule and was written to be a test; a mined row is evidence and
/// carries a judge's verdict behind it. A metric computed over the authored
/// register says the instrument works. Only one computed over the mined
/// register says anything about the world, and a reader who cannot tell them
/// apart will read the first as the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Source {
    /// Written by hand for the instrument's own tests. Not the corpus.
    Authored,
    /// Mined from archived drives and labelled by a judge. The corpus.
    Mined,
}

impl Source {
    /// Every source.
    pub const ALL: &'static [Self] = &[Self::Authored, Self::Mined];

    /// The spelling the register uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Authored => "authored",
            Self::Mined => "mined",
        }
    }

    /// The source a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|source| source.tag() == tag)
    }
}

// ---------------------------------------------------------------------------
// data: sense sets and registers
// ---------------------------------------------------------------------------

/// What a register file's name says is in it: `<source>-<set>.jsonl`.
///
/// Both halves are in the name because both are what a reader needs before
/// opening it, and because a directory that holds one authored register and
/// one mined one is a directory where the difference has to be visible from
/// the listing. The rows say it too, and the walk below refuses a file whose
/// rows disagree with its name -- one spelling per value, checked at the one
/// place the two spellings meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegisterName {
    /// Where its rows came from.
    pub source: Source,
    /// The set they are labelled against.
    pub set: SenseSet,
}

impl RegisterName {
    /// The name a file stem spells, if it spells one.
    ///
    /// `durable_fact` carries an underscore and the separator is a hyphen, so
    /// the first hyphen ends the source and the rest is the set.
    #[must_use]
    pub fn of(stem: &str) -> Option<Self> {
        let (source, set) = stem.split_once('-')?;
        Some(Self {
            source: Source::from_tag(source)?,
            set: SenseSet::from_tag(set)?,
        })
    }

    /// The stem this name is written as.
    #[must_use]
    pub fn stem(self) -> String {
        format!("{}-{}", self.source.tag(), self.set.tag())
    }
}

/// A file in the register directory that is not a register.
///
/// Named rather than skipped: a walk that ignored what it did not recognise
/// would ignore a register whose name was mistyped, and report the directory
/// as clean.
pub const REGISTER_SIDECARS: &[&str] = &[".provenance.jsonl"];

/// One authored sense: a literal or a paraphrase, of one polarity of one set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sense {
    /// The set it belongs to.
    pub set: SenseSet,
    /// The version of that set. Senses amend by bump: a row edited in place is
    /// a row whose banked scores no longer say what they scored.
    pub version: u32,
    /// Which side of the set it describes.
    pub polarity: Polarity,
    /// The description itself.
    pub text: String,
}

/// One register row: a sentence and what it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Its identity, unique in the register.
    pub id: String,
    /// The sentence.
    pub text: String,
    /// What it is, against the register's set.
    pub label: Label,
    /// Where it came from.
    pub source: Source,
}

/// Why a data file is not the data it claims to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataError {
    /// A line the record grammar or value space rejected.
    Line {
        /// The 1-based line.
        line: usize,
        /// What was wrong with it.
        error: LineError,
    },
    /// A key the schema requires and the row omits.
    MissingKey {
        /// The 1-based line.
        line: usize,
        /// The key.
        key: &'static str,
    },
    /// A key the schema does not name. The schema is closed: a field nobody
    /// reads is a field whose meaning drifts.
    UnknownKey {
        /// The 1-based line.
        line: usize,
        /// The key.
        key: String,
    },
    /// A value of the wrong kind.
    WrongType {
        /// The 1-based line.
        line: usize,
        /// The key.
        key: &'static str,
    },
    /// A closed vocabulary's tag that names no variant.
    UnknownTag {
        /// The 1-based line.
        line: usize,
        /// The key.
        key: &'static str,
        /// What was written.
        value: String,
    },
    /// A value that must say something and says nothing.
    Empty {
        /// The 1-based line.
        line: usize,
        /// The key.
        key: &'static str,
    },
    /// An integer where a version was wanted, out of the range a version has.
    /// Zero is the kind of value the schema names, so calling it a type error
    /// sends whoever reads the refusal looking for the wrong thing.
    NotAVersion {
        /// The 1-based line.
        line: usize,
        /// The key.
        key: &'static str,
        /// What was written.
        value: i64,
    },
    /// An id bound twice.
    DuplicateId {
        /// The 1-based line of the second binding.
        line: usize,
        /// The id.
        id: String,
    },
    /// A text present twice. In a sense set a paraphrase repeated is not a
    /// paraphrase; in a vector cache it is two vectors for one text.
    DuplicateText {
        /// The 1-based line of the second occurrence.
        line: usize,
        /// The text.
        text: String,
    },
    /// Vectors of different dimension in one cache, which is two caches.
    Ragged {
        /// The 1-based line.
        line: usize,
        /// The dimension the first row established.
        expected: usize,
        /// The dimension this row has.
        got: usize,
    },
    /// No rows at all. A file of nothing is not an empty set, it is a missing
    /// one, and every assertion over it would hold vacuously.
    NoRows,
}

impl fmt::Display for DataError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Line { line, error } => write!(f, "line {line}: {error}"),
            Self::MissingKey { line, key } => write!(f, "line {line}: no `{key}`"),
            Self::UnknownKey { line, key } => {
                write!(f, "line {line}: `{key}` is not a key of this schema")
            }
            Self::WrongType { line, key } => write!(
                f,
                "line {line}: `{key}` is not the kind of value the schema names"
            ),
            Self::UnknownTag { line, key, value } => {
                write!(f, "line {line}: `{key}` is {value:?}, which names nothing")
            }
            Self::Empty { line, key } => write!(f, "line {line}: `{key}` is empty"),
            Self::NotAVersion { line, key, value } => {
                write!(
                    f,
                    "line {line}: `{key}` is {value}, and versions start at one"
                )
            }
            Self::DuplicateId { line, id } => {
                write!(f, "line {line}: the id {id:?} is already bound")
            }
            Self::DuplicateText { line, text } => {
                write!(f, "line {line}: the text {text:?} is already present")
            }
            Self::Ragged {
                line,
                expected,
                got,
            } => write!(
                f,
                "line {line}: a vector of {got} components where the cache holds {expected}"
            ),
            Self::NoRows => write!(f, "no rows: a file of nothing is a missing file"),
        }
    }
}

impl Error for DataError {}

/// One decoded line of a data file: where it sat, and what it said.
type DataLine = (usize, BTreeMap<String, Value>);

/// Every non-blank line of a data file, decoded through the one reader.
fn rows(source: &str) -> Result<Vec<DataLine>, DataError> {
    let mut decoded = Vec::new();
    for (index, text) in source.lines().enumerate() {
        if text.trim().is_empty() {
            continue;
        }
        let line = index + 1;
        let members = json::line(text).map_err(|error| DataError::Line { line, error })?;
        decoded.push((line, members));
    }
    if decoded.is_empty() {
        return Err(DataError::NoRows);
    }
    Ok(decoded)
}

/// A required, non-empty string member, removed from the row.
fn take_text(
    members: &mut BTreeMap<String, Value>,
    line: usize,
    key: &'static str,
) -> Result<String, DataError> {
    match members.remove(key) {
        Some(Value::String(text)) if text.trim().is_empty() => Err(DataError::Empty { line, key }),
        Some(Value::String(text)) => Ok(text),
        Some(_) => Err(DataError::WrongType { line, key }),
        None => Err(DataError::MissingKey { line, key }),
    }
}

/// A required version member: an integer above zero, removed from the row.
fn take_version(
    members: &mut BTreeMap<String, Value>,
    line: usize,
    key: &'static str,
) -> Result<u32, DataError> {
    match members.remove(key) {
        Some(Value::Integer(number)) => match u32::try_from(number) {
            Ok(version) if version > 0 => Ok(version),
            _ => Err(DataError::NotAVersion {
                line,
                key,
                value: number,
            }),
        },
        Some(_) => Err(DataError::WrongType { line, key }),
        None => Err(DataError::MissingKey { line, key }),
    }
}

/// A required member of a closed vocabulary, removed from the row.
fn take_tag<T>(
    members: &mut BTreeMap<String, Value>,
    line: usize,
    key: &'static str,
    from_tag: impl Fn(&str) -> Option<T>,
) -> Result<T, DataError> {
    let value = take_text(members, line, key)?;
    from_tag(&value).ok_or(DataError::UnknownTag { line, key, value })
}

/// Nothing left in the row: the schema is closed.
fn closed(members: &BTreeMap<String, Value>, line: usize) -> Result<(), DataError> {
    match members.keys().next() {
        Some(key) => Err(DataError::UnknownKey {
            line,
            key: key.clone(),
        }),
        None => Ok(()),
    }
}

/// Read a sense-set file.
///
/// Rows are `{"set","version","polarity","text"}` and nothing else. The first
/// row of a polarity of a set is its **literal**; the rows after it are
/// paraphrases. Order is meaning here, so a reader that sorted would change
/// which sentence raw cosine is taken against.
///
/// # Errors
///
/// Returns [`DataError`] for a line that is not a row of this schema, a text
/// repeated, or a file with no rows.
pub fn senses(source: &str) -> Result<Vec<Sense>, DataError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (line, mut members) in rows(source)? {
        let set = take_tag(&mut members, line, "set", SenseSet::from_tag)?;
        let version = take_version(&mut members, line, "version")?;
        let polarity = take_tag(&mut members, line, "polarity", Polarity::from_tag)?;
        let text = take_text(&mut members, line, "text")?;
        closed(&members, line)?;
        if !seen.insert(text.clone()) {
            return Err(DataError::DuplicateText { line, text });
        }
        out.push(Sense {
            set,
            version,
            polarity,
            text,
        });
    }
    Ok(out)
}

/// The sense sets as shipped, read from [`SETS`].
///
/// # Errors
///
/// Returns [`DataError`] if the shipped file does not read, which a test keeps
/// from ever being the case.
pub fn shipped_senses() -> Result<Vec<Sense>, DataError> {
    senses(SETS)
}

/// Read a register file.
///
/// Rows are `{"id","text","label","source"}` and nothing else. Ids are unique
/// because the paired bootstrap pairs by row, and two rows under one id are
/// one row counted twice.
///
/// # Errors
///
/// Returns [`DataError`] for a line that is not a row of this schema, an id
/// bound twice, or a file with no rows.
pub fn register(source: &str) -> Result<Vec<Row>, DataError> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (line, mut members) in rows(source)? {
        let id = take_text(&mut members, line, "id")?;
        let text = take_text(&mut members, line, "text")?;
        let label = take_tag(&mut members, line, "label", Label::from_tag)?;
        let source = take_tag(&mut members, line, "source", Source::from_tag)?;
        closed(&members, line)?;
        if !seen.insert(id.clone()) {
            return Err(DataError::DuplicateId { line, id });
        }
        out.push(Row {
            id,
            text,
            label,
            source,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// embedders
// ---------------------------------------------------------------------------

/// Something that places a text in a vector space.
///
/// The bakeoff's models reach the instrument through [`Cached`]: the register
/// is embedded once per model and every scoring reads that cache, so a sense
/// version or a scoring function is re-run at no cost. The register is
/// infrastructure, not consumable.
pub trait Embedder {
    /// The text's vector. Empty when the embedder cannot place the text, which
    /// every scoring reads as a refusal rather than as a point.
    fn embed(&self, text: &str) -> Vec<f64>;

    /// Which model, pinned. Part of every cell's regime.
    fn id(&self) -> &str;
}

/// Vectors read from a `<model>.vectors.jsonl` cache.
///
/// Rows are `{"text","vector"}`, the vector a list of decimals -- never binary
/// floats, so the cache is the same bytes on every machine that writes it. A
/// text the cache does not hold embeds to nothing: a miss means the register
/// changed after the cache was built, and guessing a vector for it would score
/// a sentence the model never saw.
#[derive(Debug, Clone, PartialEq)]
pub struct Cached {
    id: String,
    dimensions: usize,
    vectors: BTreeMap<String, Vec<f64>>,
}

impl Cached {
    /// Read a cache for the model `id`.
    ///
    /// # Errors
    ///
    /// Returns [`DataError`] for a row outside the schema, a component that is
    /// not a decimal, an empty vector, rows of different dimension, a text
    /// present twice, or a file with no rows.
    pub fn load(id: &str, source: &str) -> Result<Self, DataError> {
        let mut vectors: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        let mut dimensions = None;
        for (line, mut members) in rows(source)? {
            let text = take_text(&mut members, line, "text")?;
            let vector = take_vector(&mut members, line)?;
            closed(&members, line)?;
            let expected = *dimensions.get_or_insert(vector.len());
            if vector.len() != expected {
                return Err(DataError::Ragged {
                    line,
                    expected,
                    got: vector.len(),
                });
            }
            if vectors.insert(text.clone(), vector).is_some() {
                return Err(DataError::DuplicateText { line, text });
            }
        }
        Ok(Self {
            id: id.to_owned(),
            dimensions: dimensions.unwrap_or(0),
            vectors,
        })
    }

    /// Whether the cache holds a vector for `text`.
    #[must_use]
    pub fn holds(&self, text: &str) -> bool {
        self.vectors.contains_key(text)
    }

    /// The dimension every vector here has.
    #[must_use]
    pub fn dimensions(&self) -> usize {
        self.dimensions
    }

    /// How many texts the cache holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.vectors.len()
    }

    /// Whether the cache holds nothing. Not reachable through
    /// [`Cached::load`], which refuses a file with no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
}

/// The `vector` member: a non-empty list of decimals.
fn take_vector(members: &mut BTreeMap<String, Value>, line: usize) -> Result<Vec<f64>, DataError> {
    let key = "vector";
    let Some(value) = members.remove(key) else {
        return Err(DataError::MissingKey { line, key });
    };
    let Value::Array(items) = value else {
        return Err(DataError::WrongType { line, key });
    };
    if items.is_empty() {
        return Err(DataError::Empty { line, key });
    }
    items
        .iter()
        .map(|item| match item {
            Value::Decimal(number) => number
                .as_str()
                .parse::<f64>()
                .map_err(|_| DataError::WrongType { line, key }),
            _ => Err(DataError::WrongType { line, key }),
        })
        .collect()
}

impl Embedder for Cached {
    fn embed(&self, text: &str) -> Vec<f64> {
        self.vectors.get(text).cloned().unwrap_or_default()
    }

    fn id(&self) -> &str {
        &self.id
    }
}

/// A deterministic embedder for tests: tokens hashed into a small vector.
///
/// Not a model and not a stand-in for one. It exists so that the seeded
/// control rows land at their extremes **by construction** -- a sense text
/// verbatim has the sense's own token multiset and so sits at cosine one, and
/// a row sharing no token with any sense sits at cosine zero -- which lets the
/// instrument's own guards be tested with no model in the room. Components are
/// token counts, so every cosine it produces is non-negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fixture;

impl Fixture {
    /// How many buckets tokens hash into. Enough that the handful of tokens in
    /// a sentence rarely collide; small enough to read in a test failure.
    pub const DIMENSIONS: usize = 512;

    /// Its id. A fixture, and it says so in every regime that names it.
    pub const ID: &'static str = "fixture";

    /// The tokens of `text`: lower-cased runs of letters and digits.
    #[must_use]
    pub fn tokens(text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|ch: char| !ch.is_alphanumeric())
            .filter(|token| !token.is_empty())
            .map(str::to_owned)
            .collect()
    }
}

/// FNV-1a over the bytes. Not a cryptographic hash; a stable one.
fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// The bucket a hash lands in.
fn bucket(hash: u64) -> usize {
    let dimensions = u64::try_from(Fixture::DIMENSIONS).unwrap_or(u64::MAX);
    usize::try_from(hash % dimensions).unwrap_or(0)
}

impl Embedder for Fixture {
    fn embed(&self, text: &str) -> Vec<f64> {
        let mut vector = vec![0.0; Self::DIMENSIONS];
        for token in Self::tokens(text) {
            vector[bucket(fnv1a(token.as_bytes()))] += 1.0;
        }
        vector
    }

    fn id(&self) -> &str {
        Self::ID
    }
}

// ---------------------------------------------------------------------------
// scoring
// ---------------------------------------------------------------------------

/// The cosine between two vectors, or nothing when there is none.
///
/// `None` for vectors of different dimension -- a cache from one model read
/// against senses embedded by another -- and for a vector with no direction. A
/// scoring handed a zero for either would be reporting a distance between a
/// sentence and nothing.
#[must_use]
pub fn cosine(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let dot: f64 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let norm_a = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b = b.iter().map(|y| y * y).sum::<f64>().sqrt();
    if norm_a <= 0.0 || norm_b <= 0.0 {
        return None;
    }
    Some((dot / (norm_a * norm_b)).clamp(-1.0, 1.0))
}

/// The temperature of the softmax scoring.
///
/// Pre-registered here rather than tuned on the data it is about to judge:
/// cosines live in a narrow band, and a softmax at temperature one over them
/// is nearly flat. `softmax_scoring_is_a_probability_over_the_sense_set`
/// holds it to that -- at a temperature that flattens the mass, the positive
/// literal stops taking almost all of it.
pub const SOFTMAX_TEMPERATURE: f64 = 0.1;

/// One sense, placed by one embedder.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedded {
    /// The sense.
    pub sense: Sense,
    /// Where the embedder put it.
    pub vector: Vec<f64>,
}

/// One set at one version, every paraphrase of both polarities embedded.
///
/// Built once per set, version and embedder, and scored against many times.
/// The literal of each polarity is the first row the file listed, and the
/// scorings that use a single sense use that one.
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedSet {
    set: SenseSet,
    version: u32,
    embedder: String,
    positive: Vec<Embedded>,
    negative: Vec<Embedded>,
}

/// Why a set could not be embedded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetError {
    /// A polarity with no rows. A contrastive scoring with nothing to subtract
    /// is raw cosine wearing another tag.
    NoParaphrase {
        /// The set.
        set: SenseSet,
        /// The polarity with no rows.
        polarity: Polarity,
    },
    /// Rows of more than one version in one set. Scores across versions are
    /// scores across instruments.
    MixedVersions {
        /// The set.
        set: SenseSet,
    },
    /// A sense the embedder could not place.
    Unembeddable {
        /// The set.
        set: SenseSet,
        /// The sense.
        text: String,
    },
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoParaphrase { set, polarity } => write!(
                f,
                "{} has no {} sense, so there is nothing to score against",
                set.tag(),
                polarity.tag()
            ),
            Self::MixedVersions { set } => {
                write!(f, "{} carries more than one version", set.tag())
            }
            Self::Unembeddable { set, text } => {
                write!(f, "{}: the embedder could not place {text:?}", set.tag())
            }
        }
    }
}

impl Error for SetError {}

impl EmbeddedSet {
    /// Embed every sense of `set` in `senses` with `embedder`.
    ///
    /// # Errors
    ///
    /// Returns [`SetError`] when a polarity has no rows, the rows span more
    /// than one version, or the embedder cannot place a sense.
    pub fn embed(
        senses: &[Sense],
        set: SenseSet,
        embedder: &dyn Embedder,
    ) -> Result<Self, SetError> {
        let mut positive = Vec::new();
        let mut negative = Vec::new();
        let mut versions = BTreeSet::new();
        for sense in senses.iter().filter(|sense| sense.set == set) {
            versions.insert(sense.version);
            let vector = embedder.embed(&sense.text);
            if cosine(&vector, &vector).is_none() {
                return Err(SetError::Unembeddable {
                    set,
                    text: sense.text.clone(),
                });
            }
            let placed = Embedded {
                sense: sense.clone(),
                vector,
            };
            match sense.polarity {
                Polarity::Positive => positive.push(placed),
                Polarity::Negative => negative.push(placed),
            }
        }
        for (polarity, rows) in [
            (Polarity::Positive, &positive),
            (Polarity::Negative, &negative),
        ] {
            if rows.is_empty() {
                return Err(SetError::NoParaphrase { set, polarity });
            }
        }
        let mut versions = versions.into_iter();
        let (Some(version), None) = (versions.next(), versions.next()) else {
            return Err(SetError::MixedVersions { set });
        };
        Ok(Self {
            set,
            version,
            embedder: embedder.id().to_owned(),
            positive,
            negative,
        })
    }

    /// The set.
    #[must_use]
    pub fn set(&self) -> SenseSet {
        self.set
    }

    /// The version every row here carries.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The embedder that placed the rows.
    #[must_use]
    pub fn embedder(&self) -> &str {
        &self.embedder
    }

    /// Every embedded sense of `polarity`, the literal first.
    #[must_use]
    pub fn paraphrases(&self, polarity: Polarity) -> &[Embedded] {
        match polarity {
            Polarity::Positive => &self.positive,
            Polarity::Negative => &self.negative,
        }
    }

    /// The literal of `polarity`: the first row the file listed.
    ///
    /// Never absent, because [`EmbeddedSet::embed`] refuses a polarity with no
    /// rows.
    ///
    /// # Panics
    ///
    /// Only if that refusal is ever removed.
    #[must_use]
    pub fn literal(&self, polarity: Polarity) -> &Embedded {
        self.paraphrases(polarity)
            .first()
            .expect("a polarity with no senses is refused when the set is embedded")
    }
}

/// How an instance is scored against a set. The factor most bakeoffs skip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Scoring {
    /// Cosine against the positive literal. The baseline, and the arm that
    /// over-fires: an abstract description is near everything.
    RawCosine,
    /// Cosine against the positive literal, minus cosine against the negative
    /// literal. The repair for a hub is a second hub beside it.
    Contrastive,
    /// The share of the softmax mass, over every paraphrase of both polarities
    /// at [`SOFTMAX_TEMPERATURE`], that lands on the positive side.
    Softmax,
    /// The best cosine over the positive paraphrases.
    EnsembleMax,
}

impl Scoring {
    /// Every scoring, so a bakeoff cannot report a subset as the set.
    pub const ALL: &'static [Self] = &[
        Self::RawCosine,
        Self::Contrastive,
        Self::Softmax,
        Self::EnsembleMax,
    ];

    /// The spelling a cell uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::RawCosine => "raw_cosine",
            Self::Contrastive => "contrastive",
            Self::Softmax => "softmax",
            Self::EnsembleMax => "ensemble_max",
        }
    }

    /// The scoring a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|scoring| scoring.tag() == tag)
    }

    /// The lowest value this scoring can produce.
    ///
    /// A row the lexical gate dropped sits here, so it can never outrank a
    /// row that was scored. Zero would not do: two of these scorings go
    /// negative, and a dropped row parked at zero would sit above every row
    /// the scoring placed on the negative side.
    #[must_use]
    pub fn floor(self) -> f64 {
        match self {
            Self::RawCosine | Self::EnsembleMax => -1.0,
            Self::Contrastive => -2.0,
            Self::Softmax => 0.0,
        }
    }

    /// The control rows this scoring must put first and last.
    ///
    /// The top is always the positive sense verbatim. The bottom depends on
    /// what the scoring does with the negative sense: a scoring that ignores
    /// it must put unrelated words last, and a scoring that subtracts it must
    /// put the negative sense verbatim last -- under those, unrelated words
    /// score at the set's prior, which is not an extreme, and a row that leans
    /// negative is meant to sit below them.
    #[must_use]
    pub fn extremes(self) -> (Control, Control) {
        match self {
            Self::RawCosine | Self::EnsembleMax => {
                (Control::VerbatimPositive, Control::UnrelatedWords)
            }
            Self::Contrastive | Self::Softmax => {
                (Control::VerbatimPositive, Control::VerbatimNegative)
            }
        }
    }

    /// Score one embedded instance against `set`.
    ///
    /// `None` when a cosine the scoring needs does not exist: an instance the
    /// embedder could not place, or a set embedded in another space.
    #[must_use]
    pub fn score(self, instance: &[f64], set: &EmbeddedSet) -> Option<f64> {
        match self {
            Self::RawCosine => cosine(instance, &set.literal(Polarity::Positive).vector),
            Self::Contrastive => {
                let toward = cosine(instance, &set.literal(Polarity::Positive).vector)?;
                let away = cosine(instance, &set.literal(Polarity::Negative).vector)?;
                Some(toward - away)
            }
            Self::Softmax => {
                let mass = |polarity| {
                    set.paraphrases(polarity)
                        .iter()
                        .map(|sense| {
                            cosine(instance, &sense.vector)
                                .map(|value| (value / SOFTMAX_TEMPERATURE).exp())
                        })
                        .sum::<Option<f64>>()
                };
                let toward = mass(Polarity::Positive)?;
                let away = mass(Polarity::Negative)?;
                Some(toward / (toward + away))
            }
            Self::EnsembleMax => set
                .paraphrases(Polarity::Positive)
                .iter()
                .map(|sense| cosine(instance, &sense.vector))
                .try_fold(f64::NEG_INFINITY, |best, value| {
                    value.map(|value| best.max(value))
                }),
        }
    }
}

// ---------------------------------------------------------------------------
// the lexical pre-gate
// ---------------------------------------------------------------------------

/// Whether a lexical seed must be present before a row is scored at all.
///
/// A factor of the bakeoff, not a setting: the claim is that the gate improves
/// precision at a fixed budget, and a claim is measured both ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Gate {
    /// Every row is scored.
    Without,
    /// Only a row carrying one of its set's seeds is scored; the rest sit at
    /// the scoring's floor.
    With,
}

/// The seeds, per set.
///
/// The program's own recorded mistakes were found by these words before there
/// was an embedder to find them, which is what makes the gate a baseline worth
/// measuring against and not a convenience.
pub const SEEDS: &[(SenseSet, &[&str])] = &[
    (
        SenseSet::Mistake,
        &["actually", "i was wrong", "turns out", "it turns out"],
    ),
    (
        SenseSet::DurableFact,
        &["actually", "turns out", "it turns out"],
    ),
    (
        SenseSet::Reversal,
        &["oh, i see", "is available as", "turns out"],
    ),
];

/// The seeds of `set`.
///
/// Empty for a set the table does not cover, which a test forbids for every
/// member of [`SenseSet::ALL`].
#[must_use]
pub fn seeds(set: SenseSet) -> &'static [&'static str] {
    SEEDS
        .iter()
        .find(|(candidate, _)| *candidate == set)
        .map_or(&[], |(_, seeds)| *seeds)
}

/// Lower-cased, with runs of whitespace collapsed to one space.
fn normalised(text: &str) -> String {
    text.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Whether `phrase` occurs in `haystack` on word boundaries.
///
/// `actually` inside `factually` is not a seed. A gate that matched inside
/// words would admit rows for the letters they happen to contain, and the
/// with-versus-without comparison would be measuring that instead.
fn contains_phrase(haystack: &str, phrase: &str) -> bool {
    let mut from = 0;
    while let Some(at) = haystack[from..].find(phrase) {
        let start = from + at;
        let end = start + phrase.len();
        let bounded_before = haystack[..start]
            .chars()
            .next_back()
            .is_none_or(|ch| !ch.is_alphanumeric());
        let bounded_after = haystack[end..]
            .chars()
            .next()
            .is_none_or(|ch| !ch.is_alphanumeric());
        if bounded_before && bounded_after {
            return true;
        }
        from = end;
    }
    false
}

/// Whether `text` carries a seed of `set`.
fn carries_seed(set: SenseSet, text: &str) -> bool {
    let haystack = normalised(text);
    seeds(set)
        .iter()
        .any(|seed| contains_phrase(&haystack, seed))
}

impl Gate {
    /// Both arms, so a bakeoff measures both.
    pub const ALL: &'static [Self] = &[Self::Without, Self::With];

    /// The spelling a cell uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Without => "without_gate",
            Self::With => "with_gate",
        }
    }

    /// The gate a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|gate| gate.tag() == tag)
    }

    /// Whether `text` is scored at all, against `set`.
    #[must_use]
    pub fn admits(self, set: SenseSet, text: &str) -> bool {
        match self {
            Self::Without => true,
            Self::With => carries_seed(set, text),
        }
    }
}

// ---------------------------------------------------------------------------
// cells and scored rows
// ---------------------------------------------------------------------------

/// One cell of the bakeoff short of its embedder: a scoring and a gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cell {
    /// How rows are scored.
    pub scoring: Scoring,
    /// Whether they are gated first.
    pub gate: Gate,
}

impl Cell {
    /// Every cell: the product of the two factors.
    #[must_use]
    pub fn all() -> Vec<Self> {
        Scoring::ALL
            .iter()
            .flat_map(|scoring| {
                Gate::ALL.iter().map(|gate| Self {
                    scoring: *scoring,
                    gate: *gate,
                })
            })
            .collect()
    }

    /// The cell's name in a result.
    #[must_use]
    pub fn tag(self) -> String {
        format!("{}+{}", self.scoring.tag(), self.gate.tag())
    }
}

/// One register row, scored in one cell.
#[derive(Debug, Clone, PartialEq)]
pub struct Scored {
    /// The row.
    pub id: String,
    /// What it is.
    pub label: Label,
    /// Its score, or the scoring's floor if the gate dropped it.
    pub score: f64,
    /// Whether the gate let it through to be scored.
    pub admitted: bool,
}

/// Why a register could not be scored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScoreError {
    /// A row the embedder could not place. Not a zero: a cache miss scored as
    /// zero is a sentence the model never saw, ranked as though it had.
    Unembeddable {
        /// The row.
        id: String,
    },
}

impl fmt::Display for ScoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unembeddable { id } => write!(
                f,
                "{id}: the embedder could not place this row, and a miss is not a zero"
            ),
        }
    }
}

impl Error for ScoreError {}

/// Score every row of `rows` against `set` in `cell`.
///
/// # Errors
///
/// Returns [`ScoreError::Unembeddable`] for a row the gate admitted and the
/// embedder could not place.
pub fn score_rows(
    rows: &[Row],
    set: &EmbeddedSet,
    embedder: &dyn Embedder,
    cell: Cell,
) -> Result<Vec<Scored>, ScoreError> {
    rows.iter()
        .map(|row| {
            let admitted = cell.gate.admits(set.set(), &row.text);
            let score = if admitted {
                cell.scoring
                    .score(&embedder.embed(&row.text), set)
                    .ok_or_else(|| ScoreError::Unembeddable { id: row.id.clone() })?
            } else {
                cell.scoring.floor()
            };
            Ok(Scored {
                id: row.id.clone(),
                label: row.label,
                score,
                admitted,
            })
        })
        .collect()
}

/// Rows by score, best first, ties broken by id so a ranking is the same on
/// every run.
fn ranked(rows: &[Scored]) -> Vec<&Scored> {
    let mut ordered: Vec<&Scored> = rows.iter().collect();
    ordered.sort_by(|a, b| b.score.total_cmp(&a.score).then_with(|| a.id.cmp(&b.id)));
    ordered
}

// ---------------------------------------------------------------------------
// controls
// ---------------------------------------------------------------------------

/// A seeded-fault row: one whose rank is known before anything is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Control {
    /// The positive literal, verbatim. Must rank first under every scoring.
    VerbatimPositive,
    /// The negative literal, verbatim. Must rank last under a scoring that
    /// subtracts it.
    VerbatimNegative,
    /// Words that share nothing with any sense. Must rank last under a scoring
    /// that measures closeness to the positive side only.
    UnrelatedWords,
}

/// The unrelated-words control.
///
/// Chosen to share no token with any shipped sense. The control check, not
/// this sentence, is what holds that true.
pub const UNRELATED: &str = "zebra quartz umbrella lantern";

impl Control {
    /// Every control.
    pub const ALL: &'static [Self] = &[
        Self::VerbatimPositive,
        Self::VerbatimNegative,
        Self::UnrelatedWords,
    ];

    /// The spelling a result uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::VerbatimPositive => "verbatim_positive",
            Self::VerbatimNegative => "verbatim_negative",
            Self::UnrelatedWords => "unrelated_words",
        }
    }

    /// The control a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|control| control.tag() == tag)
    }

    /// The control row's id, derived from the set it is seeded into rather
    /// than minted.
    #[must_use]
    pub fn id(self, set: SenseSet) -> String {
        format!("control/{}/{}", set.tag(), self.tag())
    }

    /// The control row, against `set`.
    #[must_use]
    pub fn row(self, set: &EmbeddedSet) -> Row {
        let (text, label) = match self {
            Self::VerbatimPositive => (
                set.literal(Polarity::Positive).sense.text.clone(),
                Label::Positive,
            ),
            Self::VerbatimNegative => (
                set.literal(Polarity::Negative).sense.text.clone(),
                Label::Negative,
            ),
            Self::UnrelatedWords => (UNRELATED.to_owned(), Label::Negative),
        };
        Row {
            id: self.id(set.set()),
            text,
            label,
            source: Source::Authored,
        }
    }
}

/// Why a run's controls are not at their extremes.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlFailure {
    /// A row could not be scored at all.
    Unscorable(ScoreError),
    /// A register row reached the top control.
    NotAtTop {
        /// The control.
        control: Control,
        /// What it scored.
        score: f64,
        /// The row that reached it.
        row: String,
        /// What that row scored.
        other: f64,
    },
    /// A register row went under the bottom control.
    NotAtBottom {
        /// The control.
        control: Control,
        /// What it scored.
        score: f64,
        /// The row that went under it.
        row: String,
        /// What that row scored.
        other: f64,
    },
    /// The top control did not outscore the bottom one.
    Inverted {
        /// The top control's score.
        top: f64,
        /// The bottom control's score.
        bottom: f64,
    },
    /// A control was not scored at all, so its extreme is unknown.
    Missing {
        /// The control.
        control: Control,
    },
}

impl fmt::Display for ControlFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unscorable(err) => write!(f, "{err}"),
            Self::NotAtTop {
                control,
                score,
                row,
                other,
            } => write!(
                f,
                "{} scored {score} and {row} reached it at {other}: the scoring cannot tell \
                 the sense itself from the register",
                control.tag()
            ),
            Self::NotAtBottom {
                control,
                score,
                row,
                other,
            } => write!(
                f,
                "{} scored {score} and {row} went under it at {other}",
                control.tag()
            ),
            Self::Inverted { top, bottom } => write!(
                f,
                "the top control scored {top} and the bottom control {bottom}"
            ),
            Self::Missing { control } => {
                write!(f, "{} was never scored", control.tag())
            }
        }
    }
}

impl Error for ControlFailure {}

/// Check the seeded-fault rows for `scoring` against `set` and `register`.
///
/// The controls are scored ungated, because they test the scoring and the gate
/// is a separate factor. The top control must **strictly** outscore every
/// register row -- an embedder that cannot tell the sense from the register
/// ties them, and a tie is that failure -- and no register row may go under
/// the bottom control.
///
/// # Errors
///
/// Returns [`ControlFailure`] naming the control and the row that displaced
/// it.
pub fn controls(
    register: &[Row],
    set: &EmbeddedSet,
    embedder: &dyn Embedder,
    scoring: Scoring,
) -> Result<(), ControlFailure> {
    let (top, bottom) = scoring.extremes();
    let mut rows = register.to_vec();
    rows.push(top.row(set));
    rows.push(bottom.row(set));
    let cell = Cell {
        scoring,
        gate: Gate::Without,
    };
    let scored = score_rows(&rows, set, embedder, cell).map_err(ControlFailure::Unscorable)?;
    let score_of = |control: Control| {
        let id = control.id(set.set());
        scored
            .iter()
            .find(|row| row.id == id)
            .map(|row| row.score)
            .ok_or(ControlFailure::Missing { control })
    };
    let top_score = score_of(top)?;
    let bottom_score = score_of(bottom)?;
    if top_score <= bottom_score {
        return Err(ControlFailure::Inverted {
            top: top_score,
            bottom: bottom_score,
        });
    }
    let control_ids = [top.id(set.set()), bottom.id(set.set())];
    for row in scored.iter().filter(|row| !control_ids.contains(&row.id)) {
        if row.score >= top_score {
            return Err(ControlFailure::NotAtTop {
                control: top,
                score: top_score,
                row: row.id.clone(),
                other: row.score,
            });
        }
        if row.score < bottom_score {
            return Err(ControlFailure::NotAtBottom {
                control: bottom,
                score: bottom_score,
                row: row.id.clone(),
                other: row.score,
            });
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// metrics
// ---------------------------------------------------------------------------

/// A count over a count: what a precision is before it is a float.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fraction {
    /// How many.
    pub hits: u64,
    /// Out of how many.
    pub of: u64,
}

/// A small count as a float, for arithmetic that is about to be reported to a
/// fixed number of digits.
fn count(n: usize) -> f64 {
    f64::from(u32::try_from(n).unwrap_or(u32::MAX))
}

impl Fraction {
    /// The quotient, or nothing for a fraction over nothing.
    #[must_use]
    pub fn as_f64(self) -> Option<f64> {
        if self.of == 0 {
            return None;
        }
        let hits = f64::from(u32::try_from(self.hits).unwrap_or(u32::MAX));
        let of = f64::from(u32::try_from(self.of).unwrap_or(u32::MAX));
        Some(hits / of)
    }
}

impl fmt::Display for Fraction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.hits, self.of)
    }
}

/// Precision at a fixed nomination budget: of the `k` best-scored rows, how
/// many are positive. The primary endpoint.
///
/// A budget larger than the register nominates the register, and the fraction
/// is then over what was nominated rather than over `k`.
#[must_use]
pub fn precision_at_k(rows: &[Scored], k: usize) -> Fraction {
    let top = ranked(rows);
    let of = top.len().min(k);
    let hits = top[..of]
        .iter()
        .filter(|row| row.label.is_positive())
        .count();
    Fraction {
        hits: hits as u64,
        of: of as u64,
    }
}

/// Over-firing at a fixed budget: of the hard-negative rows, how many were
/// nominated within the `k` best. The endpoint the hard negatives exist for.
#[must_use]
pub fn over_firing(rows: &[Scored], k: usize) -> Fraction {
    let hard_negatives = rows
        .iter()
        .filter(|row| row.label.is_hard_negative())
        .count();
    let nominated = ranked(rows)
        .into_iter()
        .take(k)
        .filter(|row| row.label.is_hard_negative())
        .count();
    Fraction {
        hits: nominated as u64,
        of: hard_negatives as u64,
    }
}

/// The scores of the positive rows, and of everything else.
fn split(rows: &[Scored]) -> (Vec<f64>, Vec<f64>) {
    let (positive, negative): (Vec<&Scored>, Vec<&Scored>) =
        rows.iter().partition(|row| row.label.is_positive());
    (
        positive.iter().map(|row| row.score).collect(),
        negative.iter().map(|row| row.score).collect(),
    )
}

/// The area under the ROC curve: the probability that a positive row outscores
/// a negative one, a tie counting half.
///
/// Rank-based, so it does not care what scale a scoring uses -- which is the
/// only way the four scorings here are comparable at all.
///
/// `None` without at least one row of each kind: there is no ranking of one
/// class against nothing.
#[must_use]
pub fn auc(rows: &[Scored]) -> Option<f64> {
    let (positive, negative) = split(rows);
    if positive.is_empty() || negative.is_empty() {
        return None;
    }
    let mut wins = 0.0;
    for p in &positive {
        for n in &negative {
            wins += match p.total_cmp(n) {
                Ordering::Greater => 1.0,
                Ordering::Equal => 0.5,
                Ordering::Less => 0.0,
            };
        }
    }
    Some(wins / (count(positive.len()) * count(negative.len())))
}

/// A sample's mean and unbiased variance.
struct Moments {
    mean: f64,
    variance: f64,
}

/// The moments of `values`, or nothing for fewer than two of them.
fn moments(values: &[f64]) -> Option<Moments> {
    if values.len() < 2 {
        return None;
    }
    let n = count(values.len());
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|v| (v - mean) * (v - mean)).sum::<f64>() / (n - 1.0);
    Some(Moments { mean, variance })
}

/// The standardised separation of the two classes: the difference of their
/// mean scores over the pooled standard deviation.
///
/// `None` without at least two rows of each kind, or with no spread at all: a
/// separation over a zero spread is not infinite, it is undefined.
#[must_use]
pub fn d_prime(rows: &[Scored]) -> Option<f64> {
    let (positive, negative) = split(rows);
    let positive = moments(&positive)?;
    let negative = moments(&negative)?;
    let pooled = f64::midpoint(positive.variance, negative.variance).sqrt();
    if pooled <= 0.0 {
        return None;
    }
    Some((positive.mean - negative.mean) / pooled)
}

/// A seeded pseudo-random generator: xorshift with a multiplicative output.
///
/// Ours, so that a bootstrap draws the same bytes under the same seed on every
/// machine and every toolchain. A resampling nobody can reproduce is a number
/// nobody can check. Not for anything a secret depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Xorshift {
    state: u64,
}

impl Xorshift {
    /// A generator at `seed`. Every seed, zero included, yields a sequence.
    #[must_use]
    pub fn seeded(seed: u64) -> Self {
        let state = seed ^ 0x9E37_79B9_7F4A_7C15;
        Self {
            state: if state == 0 { 1 } else { state },
        }
    }

    /// The next 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.state = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// An index below `n`, or zero when there is no such index.
    pub fn below(&mut self, n: usize) -> usize {
        match u64::try_from(n) {
            Ok(bound) if bound > 0 => usize::try_from(self.next_u64() % bound).unwrap_or(0),
            _ => 0,
        }
    }
}

/// The smallest p-value `resamples` resamples can produce: one over
/// `resamples` plus one.
///
/// Printed beside every p. A bootstrap that never saw a crossing has not found
/// a p of zero; it has found the floor of its own resample count.
#[must_use]
pub fn attainable_p_floor(resamples: u32) -> f64 {
    1.0 / (f64::from(resamples) + 1.0)
}

/// A p-value and the floor it could not have gone below.
///
/// Both fields are private and there is no constructor: a p-value reaches a
/// caller only from [`paired_bootstrap`], with its floor attached.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PValue {
    value: f64,
    floor: f64,
}

impl PValue {
    /// The p-value.
    #[must_use]
    pub fn value(self) -> f64 {
        self.value
    }

    /// The smallest value the resampling could have produced.
    #[must_use]
    pub fn floor(self) -> f64 {
        self.floor
    }
}

impl fmt::Display for PValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "p={:.6} (attainable floor {:.6})",
            self.value, self.floor
        )
    }
}

/// A paired bootstrap's result.
#[derive(Debug, Clone, PartialEq)]
pub struct Bootstrap {
    /// The observed mean of the paired differences, `a` minus `b`.
    pub observed: f64,
    /// How often a resampled difference crossed zero, add-one corrected.
    pub p: PValue,
    /// How many resamples.
    pub resamples: u32,
    /// The seed they were drawn under.
    pub seed: u64,
}

/// Why a bootstrap could not run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootstrapError {
    /// The two samples are not the same rows.
    Unpaired {
        /// Rows in `a`.
        a: usize,
        /// Rows in `b`.
        b: usize,
    },
    /// No rows.
    Empty,
    /// No resamples, whose attainable floor would be one.
    NoResamples,
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unpaired { a, b } => {
                write!(f, "{a} rows against {b}: a paired bootstrap pairs by row")
            }
            Self::Empty => write!(f, "no rows to resample"),
            Self::NoResamples => write!(f, "no resamples, so no p below one is attainable"),
        }
    }
}

impl Error for BootstrapError {}

/// The mean of the paired differences, over one resample with replacement.
fn resampled_difference(differences: &[f64], rng: &mut Xorshift) -> f64 {
    let n = differences.len();
    let total: f64 = (0..n).map(|_| differences[rng.below(n)]).sum();
    total / count(n)
}

/// A paired bootstrap of the difference between `a` and `b`, which are one
/// value per row of the same register under two conditions.
///
/// The statistic is the mean of the **paired differences**, not the difference
/// of two means computed apart: the two are the same number in arithmetic and
/// not in floating point, and a pairing that cancels exactly should report an
/// observed difference of nothing rather than a residue of the summation
/// order.
///
/// Rows are resampled with replacement, the same rows on both sides. The
/// p-value is the share of resamples in which the difference crossed zero,
/// add-one corrected, so that a difference which never crossed in `R`
/// resamples reports one over `R` plus one and not zero. An observed
/// difference of zero has nothing to test and reports one.
///
/// # Errors
///
/// Returns [`BootstrapError`] for samples of different length, no rows, or no
/// resamples.
pub fn paired_bootstrap(
    a: &[f64],
    b: &[f64],
    resamples: u32,
    seed: u64,
) -> Result<Bootstrap, BootstrapError> {
    if a.len() != b.len() {
        return Err(BootstrapError::Unpaired {
            a: a.len(),
            b: b.len(),
        });
    }
    if a.is_empty() {
        return Err(BootstrapError::Empty);
    }
    if resamples == 0 {
        return Err(BootstrapError::NoResamples);
    }
    let differences: Vec<f64> = a.iter().zip(b).map(|(x, y)| x - y).collect();
    let observed = differences.iter().sum::<f64>() / count(differences.len());
    let mut rng = Xorshift::seeded(seed);
    let side = observed.partial_cmp(&0.0);
    let crossed = |difference: f64| match side {
        Some(Ordering::Greater) => difference <= 0.0,
        Some(Ordering::Less) => difference >= 0.0,
        _ => false,
    };
    let crossings = (0..resamples)
        .filter(|_| crossed(resampled_difference(&differences, &mut rng)))
        .count();
    let value = match side {
        Some(Ordering::Greater | Ordering::Less) => {
            (count(crossings) + 1.0) / (f64::from(resamples) + 1.0)
        }
        _ => 1.0,
    };
    Ok(Bootstrap {
        observed,
        p: PValue {
            value,
            floor: attainable_p_floor(resamples),
        },
        resamples,
        seed,
    })
}

/// Holm's step-down correction of `ps` for having been tested together.
///
/// Adjusted values come back in the order given, never below the raw value,
/// monotone in the raw order, capped at one. The cells of a bakeoff are many
/// comparisons, and a p reported uncorrected is a p that was chosen.
#[must_use]
pub fn holm(ps: &[f64]) -> Vec<f64> {
    let m = ps.len();
    let mut order: Vec<usize> = (0..m).collect();
    order.sort_by(|&i, &j| ps[i].total_cmp(&ps[j]));
    let mut adjusted = vec![0.0; m];
    let mut running = 0.0_f64;
    for (rank, &index) in order.iter().enumerate() {
        let scaled = (ps[index] * count(m - rank)).min(1.0);
        running = running.max(scaled);
        adjusted[index] = running;
    }
    adjusted
}

/// How many label shuffles the null averages over.
pub const NULL_SHUFFLES: u32 = 200;

/// How far from one half the null's mean area under the curve may sit.
///
/// Bounded on both sides, because a band is only a claim when something can
/// contradict it. The test named for that runs the null over sixteen seeds
/// and requires this band to exceed every excursion it measures, and requires
/// the same band to refuse a null sitting at an area of 0.85. A tolerance
/// that can only ever be widened admits the finding it was meant to catch.
pub const NULL_AUC_BAND: f64 = 0.1;

/// How far from zero the null's mean standardised separation may sit.
///
/// Bounded on both sides by the same test as [`NULL_AUC_BAND`]: wider than
/// every excursion measured over sixteen seeds, and narrow enough to refuse a
/// null separating the two classes by a whole standard deviation.
pub const NULL_D_PRIME_BAND: f64 = 0.25;

/// The shuffled-label null: what the metrics say when the labels are random
/// with respect to the scores.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Null {
    /// Mean area under the curve over the shuffles.
    pub mean_auc: f64,
    /// Mean standardised separation over the shuffles.
    pub mean_d_prime: f64,
    /// How many shuffles.
    pub shuffles: u32,
}

impl Null {
    /// Whether the null sits where a null sits.
    ///
    /// A metric that finds a separation in shuffled labels has found something
    /// in itself, and every number it reports on real labels is that finding
    /// plus whatever else is there.
    #[must_use]
    pub fn at_chance(self) -> bool {
        (self.mean_auc - 0.5).abs() <= NULL_AUC_BAND && self.mean_d_prime.abs() <= NULL_D_PRIME_BAND
    }
}

/// The null for `rows`: labels shuffled `shuffles` times under `seed`, the
/// metrics averaged.
///
/// `None` for no shuffles, or when a shuffle leaves a metric undefined -- too
/// few rows of a class, or no spread.
#[must_use]
pub fn shuffled_null(rows: &[Scored], shuffles: u32, seed: u64) -> Option<Null> {
    if shuffles == 0 || rows.is_empty() {
        return None;
    }
    let mut rng = Xorshift::seeded(seed);
    let mut labels: Vec<Label> = rows.iter().map(|row| row.label).collect();
    let mut auc_total = 0.0;
    let mut d_prime_total = 0.0;
    for _ in 0..shuffles {
        for i in (1..labels.len()).rev() {
            let j = rng.below(i + 1);
            labels.swap(i, j);
        }
        let relabelled: Vec<Scored> = rows
            .iter()
            .zip(&labels)
            .map(|(row, label)| Scored {
                label: *label,
                ..row.clone()
            })
            .collect();
        auc_total += auc(&relabelled)?;
        d_prime_total += d_prime(&relabelled)?;
    }
    Some(Null {
        mean_auc: auc_total / f64::from(shuffles),
        mean_d_prime: d_prime_total / f64::from(shuffles),
        shuffles,
    })
}

// ---------------------------------------------------------------------------
// reported metrics: each with a demonstrated failure
// ---------------------------------------------------------------------------

/// A metric the bakeoff reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Metric {
    /// [`precision_at_k`]: the primary endpoint.
    PrecisionAtK,
    /// [`over_firing`]: the endpoint the hard negatives exist for, and the one
    /// where a larger number is the worse result.
    OverFiring,
    /// [`auc`].
    Auc,
    /// [`d_prime`].
    DPrime,
}

/// One row of a failure fixture: an id, what it is, and the score it is given.
type FixtureRow = (&'static str, Label, f64);

/// For each metric, rows on which it must report the worst it can say.
///
/// The fixtures are rankings built against the metric: three of them put the
/// positives underneath, and the over-firing fixture puts the hard negatives
/// on top. A metric that reads its own fixture as anything but failure is not
/// measuring what its name says.
///
/// Both budgeted fixtures are budget-sensitive by construction. The precision
/// fixture holds eight non-positives above its positives, so it fails at any
/// budget up to eight; the over-firing fixture holds two hard negatives at the
/// top, so it fails at any budget of two or more. A budget outside those is
/// not a budget these fixtures demonstrate, and [`Reported::take`] refuses
/// rather than reports.
const FAILURE_FIXTURES: &[(Metric, &[FixtureRow])] = &[
    (
        Metric::PrecisionAtK,
        &[
            ("failing/negative/0.9", Label::Negative, 0.9),
            ("failing/hard_negative/0.8", Label::HardNegative, 0.8),
            ("failing/negative/0.7", Label::Negative, 0.7),
            ("failing/hard_negative/0.6", Label::HardNegative, 0.6),
            ("failing/negative/0.5", Label::Negative, 0.5),
            ("failing/negative/0.4", Label::Negative, 0.4),
            ("failing/negative/0.3", Label::Negative, 0.3),
            ("failing/negative/0.2", Label::Negative, 0.2),
            ("failing/positive/0.1", Label::Positive, 0.1),
            ("failing/positive/0.0", Label::Positive, 0.0),
        ],
    ),
    (
        Metric::OverFiring,
        &[
            ("failing/hard_negative/0.9", Label::HardNegative, 0.9),
            ("failing/hard_negative/0.8", Label::HardNegative, 0.8),
            ("failing/positive/0.2", Label::Positive, 0.2),
            ("failing/negative/0.1", Label::Negative, 0.1),
        ],
    ),
    (
        Metric::Auc,
        &[
            ("failing/positive/0.1", Label::Positive, 0.1),
            ("failing/positive/0.2", Label::Positive, 0.2),
            ("failing/negative/0.8", Label::Negative, 0.8),
            ("failing/negative/0.9", Label::Negative, 0.9),
        ],
    ),
    (
        Metric::DPrime,
        &[
            ("failing/positive/0.1", Label::Positive, 0.1),
            ("failing/positive/0.2", Label::Positive, 0.2),
            ("failing/negative/0.8", Label::Negative, 0.8),
            ("failing/negative/0.9", Label::Negative, 0.9),
        ],
    ),
];

impl Metric {
    /// Every metric, so a report cannot leave one unfixtured.
    pub const ALL: &'static [Self] = &[
        Self::PrecisionAtK,
        Self::OverFiring,
        Self::Auc,
        Self::DPrime,
    ];

    /// The spelling a result uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::PrecisionAtK => "precision_at_k",
            Self::OverFiring => "over_firing",
            Self::Auc => "auc",
            Self::DPrime => "d_prime",
        }
    }

    /// The metric a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|metric| metric.tag() == tag)
    }

    /// The reading at which this metric is saying the worst it can say: no
    /// hits, every hard negative nominated, chance, no separation.
    #[must_use]
    pub fn failure_reading(self) -> f64 {
        match self {
            Self::PrecisionAtK | Self::DPrime => 0.0,
            Self::OverFiring => 1.0,
            Self::Auc => 0.5,
        }
    }

    /// Whether `value` is that failure.
    ///
    /// The comparison has a direction because one of these metrics counts the
    /// wrong thing being nominated: for [`Metric::OverFiring`] a larger number
    /// is the worse result, and a shared "at or below the floor" reading would
    /// have certified its instrument on a fixture where it fired at nobody.
    #[must_use]
    pub fn failed(self, value: f64) -> bool {
        match self {
            Self::OverFiring => value >= self.failure_reading(),
            Self::PrecisionAtK | Self::Auc | Self::DPrime => value <= self.failure_reading(),
        }
    }

    /// The metric over `rows`, at budget `k` where a budget applies.
    #[must_use]
    pub fn compute(self, k: usize, rows: &[Scored]) -> Option<f64> {
        match self {
            Self::PrecisionAtK => precision_at_k(rows, k).as_f64(),
            Self::OverFiring => over_firing(rows, k).as_f64(),
            Self::Auc => auc(rows),
            Self::DPrime => d_prime(rows),
        }
    }

    /// The rows this metric must fail on.
    #[must_use]
    pub fn failure_fixture(self) -> Vec<Scored> {
        FAILURE_FIXTURES
            .iter()
            .find(|(metric, _)| *metric == self)
            .map_or_else(Vec::new, |(_, rows)| {
                rows.iter()
                    .map(|(id, label, score)| Scored {
                        id: (*id).to_owned(),
                        label: *label,
                        score: *score,
                        admitted: true,
                    })
                    .collect()
            })
    }
}

/// A metric's value, and the value the same code produced on the rows the
/// metric declares it must fail on.
///
/// Private fields, and [`Reported::take`] is the only door -- and it takes no
/// failing rows, so there is nothing for a caller to substitute. A report
/// whose demonstrated failure was staged somewhere else is the shape the
/// groundedness gate settled on after a perfect score turned out to be a
/// property of the probe rather than of the lane.
#[derive(Debug, Clone, PartialEq)]
pub struct Reported {
    metric: Metric,
    budget: usize,
    value: f64,
    on_failure_fixture: f64,
}

/// Why a metric is not reported.
#[derive(Debug, Clone, PartialEq)]
pub enum MetricError {
    /// The rows built to fail did not fail, so the subject's value says
    /// nothing about the subject.
    InstrumentNeverFailed {
        /// The metric.
        metric: Metric,
        /// What the failing rows actually scored.
        value: f64,
        /// The reading that would have counted as failure.
        reading: f64,
    },
    /// The metric is undefined on these rows.
    Undefined {
        /// The metric.
        metric: Metric,
        /// Which rows.
        on: &'static str,
    },
}

impl fmt::Display for MetricError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstrumentNeverFailed {
                metric,
                value,
                reading,
            } => write!(
                f,
                "{} scored {value} on rows built to fail, where {reading} is failure, and did \
                 not fail: an instrument that has never been seen fail cannot certify anything",
                metric.tag()
            ),
            Self::Undefined { metric, on } => {
                write!(f, "{} is undefined on {on}", metric.tag())
            }
        }
    }
}

impl Error for MetricError {}

impl Reported {
    /// Report `metric` over `subject`, having demonstrated on the metric's own
    /// failure fixture that the same code can report failure.
    ///
    /// The demonstration is not a parameter. It is [`Metric::failure_fixture`]
    /// and it is computed here, at the same budget, because a caller who could
    /// hand in the failing rows could hand in rows whose reading is structural
    /// rather than instrumental -- a precision fixture holding no positive row
    /// scores zero because there was nothing to rank -- and certify the
    /// subject with a failure staged somewhere else entirely. That is the
    /// incident this type exists for.
    ///
    /// # Errors
    ///
    /// Returns [`MetricError::InstrumentNeverFailed`] when the fixture does
    /// not fail at this budget, and [`MetricError::Undefined`] when the metric
    /// has no value on the fixture or on the subject.
    pub fn take(metric: Metric, k: usize, subject: &[Scored]) -> Result<Self, MetricError> {
        let failing = metric.failure_fixture();
        let on_failure_fixture = metric.compute(k, &failing).ok_or(MetricError::Undefined {
            metric,
            on: "its failure fixture",
        })?;
        if !metric.failed(on_failure_fixture) {
            return Err(MetricError::InstrumentNeverFailed {
                metric,
                value: on_failure_fixture,
                reading: metric.failure_reading(),
            });
        }
        let value = metric.compute(k, subject).ok_or(MetricError::Undefined {
            metric,
            on: "the subject",
        })?;
        if !value.is_finite() {
            return Err(MetricError::Undefined {
                metric,
                on: "the subject",
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
    pub fn metric(&self) -> Metric {
        self.metric
    }

    /// The budget it was taken at.
    #[must_use]
    pub fn budget(&self) -> usize {
        self.budget
    }

    /// Its value on the subject.
    #[must_use]
    pub fn value(&self) -> f64 {
        self.value
    }

    /// Its value on the rows built to fail. This report would not exist if
    /// that value were not a failure.
    #[must_use]
    pub fn demonstrated_failure(&self) -> f64 {
        self.on_failure_fixture
    }

    /// The report as a record value, numbers spelled to four digits.
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
                decimal(self.metric.failure_reading(), 4),
            ),
        ]))
    }
}

// ---------------------------------------------------------------------------
// the pre-registration
// ---------------------------------------------------------------------------

/// What the run cannot start without.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Blocker {
    /// The register is mined from archived transcripts by lexical seed, and
    /// there are none here to mine.
    ArchivedTranscripts,
    /// The mined rows are labelled by a judge under seeded controls.
    JudgeModel,
    /// The cells need embedders, and the accuracy ceiling needs a
    /// cross-encoder.
    EmbeddingModels,
    /// A results directory carries a regime, and a regime for a run on no
    /// hosted model has no agreed spelling yet.
    RegimeSpelling,
}

impl Blocker {
    /// Every blocker.
    pub const ALL: &'static [Self] = &[
        Self::ArchivedTranscripts,
        Self::JudgeModel,
        Self::EmbeddingModels,
        Self::RegimeSpelling,
    ];

    /// The spelling a result uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::ArchivedTranscripts => "archived_transcripts",
            Self::JudgeModel => "judge_model",
            Self::EmbeddingModels => "embedding_models",
            Self::RegimeSpelling => "regime_spelling",
        }
    }

    /// The blocker a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|blocker| blocker.tag() == tag)
    }

    /// What is needed.
    #[must_use]
    pub fn description(self) -> &'static str {
        match self {
            Self::ArchivedTranscripts => "archived transcripts to mine the register from",
            Self::JudgeModel => "a judge to label the mined register under seeded controls",
            Self::EmbeddingModels => {
                "embedding models and an entailment cross-encoder, each pinned by revision and \
                 instruction prefix"
            }
            Self::RegimeSpelling => {
                "a ruling on how a processor-only run of local or no models is spelled as a \
                 regime under `results/`"
            }
        }
    }
}

/// The endpoints, fixed before the run and unfilled until it.
#[derive(Debug, Clone, PartialEq)]
pub struct PreRegistration {
    /// The primary endpoint.
    pub primary: &'static str,
    /// The separation endpoint.
    pub separation: &'static str,
    /// The over-firing endpoint.
    pub over_firing: &'static str,
    /// The accuracy-ceiling comparator.
    pub comparator: &'static str,
    /// How significance is tested and corrected.
    pub correction: &'static str,
    /// Bootstrap resamples per comparison.
    pub resamples: u32,
    /// Label shuffles per null.
    pub null_shuffles: u32,
    /// What the run is waiting on.
    pub blocked_on: &'static [Blocker],
}

/// The pre-registration for the bakeoff.
///
/// Fixed here, in code, before there is data: an endpoint chosen after the
/// numbers are in is an endpoint the numbers chose.
pub const PRE_REGISTRATION: PreRegistration = PreRegistration {
    // The budget is over the register, not over a session: [`Row`] has no
    // session key, the schema is closed, and an endpoint spelled for a
    // grouping the instrument cannot compute is an endpoint nothing will
    // answer. Grouping is a schema change and a later version.
    primary: "precision at a fixed nomination budget, the top k of the register, per embedder, \
              scoring and gate",
    separation: "the area under the curve and the standardised separation, per cell",
    over_firing: "the share of hard-negative rows nominated within the budget",
    comparator: "an entailment cross-encoder as the accuracy ceiling, so the gap between it \
                 and an embedder is priced rather than assumed",
    correction: "paired bootstrap across embedders, Holm-corrected across cells, the \
                 attainable p floor printed beside every p",
    resamples: 9999,
    null_shuffles: NULL_SHUFFLES,
    blocked_on: Blocker::ALL,
};

/// A list of tags as a record value.
fn tags<T: Copy>(items: &[T], tag: impl Fn(T) -> &'static str) -> Value {
    Value::Array(
        items
            .iter()
            .map(|item| Value::String(tag(*item).to_owned()))
            .collect(),
    )
}

impl PreRegistration {
    /// The pre-registration as a record value: what a plan would carry, and
    /// what a results directory would have to match when there is one.
    #[must_use]
    pub fn value(&self) -> Value {
        let text = |value: &str| Value::String(value.to_owned());
        Value::Object(BTreeMap::from([
            ("primary".to_owned(), text(self.primary)),
            ("separation".to_owned(), text(self.separation)),
            ("over_firing".to_owned(), text(self.over_firing)),
            ("comparator".to_owned(), text(self.comparator)),
            ("correction".to_owned(), text(self.correction)),
            (
                "resamples".to_owned(),
                Value::Integer(i64::from(self.resamples)),
            ),
            (
                "attainable_p_floor".to_owned(),
                decimal(attainable_p_floor(self.resamples), 6),
            ),
            ("null".to_owned(), self.null_value()),
            (
                "softmax_temperature".to_owned(),
                decimal(SOFTMAX_TEMPERATURE, 4),
            ),
            ("sets".to_owned(), tags(SenseSet::ALL, SenseSet::tag)),
            ("controls".to_owned(), tags(Control::ALL, Control::tag)),
            (
                "cells".to_owned(),
                Value::Array(
                    Cell::all()
                        .into_iter()
                        .map(|cell| Value::String(cell.tag()))
                        .collect(),
                ),
            ),
            ("metrics".to_owned(), Self::metrics_value()),
            ("seeds".to_owned(), Self::seeds_value()),
            ("blocked_on".to_owned(), self.blocked_value()),
        ]))
    }

    fn null_value(&self) -> Value {
        Value::Object(BTreeMap::from([
            (
                "shuffles".to_owned(),
                Value::Integer(i64::from(self.null_shuffles)),
            ),
            ("auc_band".to_owned(), decimal(NULL_AUC_BAND, 4)),
            ("d_prime_band".to_owned(), decimal(NULL_D_PRIME_BAND, 4)),
        ]))
    }

    fn metrics_value() -> Value {
        Value::Array(
            Metric::ALL
                .iter()
                .map(|metric| {
                    Value::Object(BTreeMap::from([
                        ("metric".to_owned(), Value::String(metric.tag().to_owned())),
                        (
                            "failure_reading".to_owned(),
                            decimal(metric.failure_reading(), 4),
                        ),
                    ]))
                })
                .collect(),
        )
    }

    fn seeds_value() -> Value {
        Value::Object(
            SenseSet::ALL
                .iter()
                .map(|set| (set.tag().to_owned(), tags(seeds(*set), |seed| seed)))
                .collect(),
        )
    }

    fn blocked_value(&self) -> Value {
        Value::Array(
            self.blocked_on
                .iter()
                .map(|blocker| {
                    Value::Object(BTreeMap::from([
                        (
                            "blocker".to_owned(),
                            Value::String(blocker.tag().to_owned()),
                        ),
                        (
                            "needs".to_owned(),
                            Value::String(blocker.description().to_owned()),
                        ),
                    ]))
                })
                .collect(),
        )
    }
}

/// A float as a record decimal with `digits` after the point.
///
/// The one place a number a program computed becomes a number a record holds.
/// A value with no spelling -- not finite -- comes back as the string
/// `undefined` rather than as a number that is not one.
fn decimal(value: f64, digits: usize) -> Value {
    let spelled = format!("{value:.digits$}");
    // `-0.0000` is a second spelling of zero, which the record refuses; the
    // sign carries no information once every digit is a zero.
    let all_zeroes = !spelled.bytes().any(|b| b.is_ascii_digit() && b != b'0');
    let spelled = if all_zeroes {
        spelled.trim_start_matches('-').to_owned()
    } else {
        spelled
    };
    Decimal::parse(&spelled).map_or_else(|| Value::String("undefined".to_owned()), Value::Decimal)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::{Path, PathBuf};

    use super::{
        Blocker, BootstrapError, Cached, Cell, Control, ControlFailure, DataError, Embedded,
        EmbeddedSet, Embedder, Fixture, Fraction, Gate, Label, Metric, MetricError, NULL_AUC_BAND,
        NULL_D_PRIME_BAND, NULL_SHUFFLES, PRE_REGISTRATION, Polarity, REGISTER_SIDECARS,
        RegisterName, Reported, Row, ScoreError, Scored, Scoring, SenseSet, SetError, Source,
        Xorshift, attainable_p_floor, auc, controls, cosine, d_prime, holm, over_firing,
        paired_bootstrap, precision_at_k, register, score_rows, seeds, senses, shipped_senses,
        shuffled_null,
    };
    use crate::formats::record::json::{self, Decimal, Value};

    const EPSILON: f64 = 1e-9;

    fn near(a: f64, b: f64) -> bool {
        (a - b).abs() < EPSILON
    }

    /// A decimal as a record holds it: digits as written.
    fn decimal(spelled: &str) -> Value {
        Value::Decimal(Decimal::parse(spelled).expect("a decimal"))
    }

    fn shipped() -> Vec<super::Sense> {
        shipped_senses().expect("the shipped sense sets read")
    }

    fn register_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capture")
            .join("sense")
            .join("register")
    }

    fn register_rows() -> Vec<Row> {
        let path = register_dir().join("authored-mistake.jsonl");
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
        register(&source).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
    }

    fn embedded(set: SenseSet) -> EmbeddedSet {
        EmbeddedSet::embed(&shipped(), set, &Fixture).expect("the shipped set embeds")
    }

    fn scored(cell: Cell) -> Vec<Scored> {
        score_rows(
            &register_rows(),
            &embedded(SenseSet::Mistake),
            &Fixture,
            cell,
        )
        .expect("every register row embeds")
    }

    fn ungated() -> Vec<Scored> {
        scored(Cell {
            scoring: Scoring::RawCosine,
            gate: Gate::Without,
        })
    }

    fn row(id: &str, label: Label, score: f64) -> Scored {
        Scored {
            id: id.to_owned(),
            label,
            score,
            admitted: true,
        }
    }

    fn literal(set: SenseSet, polarity: Polarity) -> String {
        shipped()
            .into_iter()
            .find(|sense| sense.set == set && sense.polarity == polarity)
            .expect("a literal")
            .text
    }

    /// An embedder that places every text at the same point. The controls must
    /// catch it, or they catch nothing.
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

    /// The fixture embedder with one text missing, the way a vector cache
    /// built before the register grew is missing its newest row.
    struct Incomplete(String);

    impl Incomplete {
        const ID: &'static str = "incomplete";
    }

    impl Embedder for Incomplete {
        fn embed(&self, text: &str) -> Vec<f64> {
            if self.0 == text {
                Vec::new()
            } else {
                Fixture.embed(text)
            }
        }
        fn id(&self) -> &str {
            Self::ID
        }
    }

    /// The fixture embedder with one text placed exactly where another sits:
    /// an embedder that cannot tell the sense itself from a register row.
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

    /// An embedder reading its vectors from a table.
    ///
    /// The fixture embedder's components are token counts, so every cosine it
    /// produces is non-negative and no row it places can go under a control.
    /// A real model's components have signs. This one lets a test put a row
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

    // ---- sense sets as versioned data ----

    #[test]
    fn the_shipped_sense_sets_cover_every_set_and_polarity_with_paraphrases() {
        let senses = shipped();
        for set in SenseSet::ALL {
            for polarity in Polarity::ALL {
                let rows = senses
                    .iter()
                    .filter(|sense| sense.set == *set && sense.polarity == *polarity)
                    .count();
                assert!(
                    (2..=3).contains(&rows),
                    "{}/{}: {rows} rows, where two to three paraphrases were authored",
                    set.tag(),
                    polarity.tag()
                );
            }
        }
        let versions: BTreeSet<u32> = senses.iter().map(|sense| sense.version).collect();
        assert_eq!(versions, BTreeSet::from([1]), "one version ships");
        let texts: BTreeSet<&str> = senses.iter().map(|sense| sense.text.as_str()).collect();
        assert_eq!(
            texts.len(),
            senses.len(),
            "a paraphrase repeated is not a paraphrase"
        );
    }

    #[test]
    fn the_sense_set_schema_is_closed() {
        let good = r#"{"set":"mistake","version":1,"polarity":"positive","text":"x"}"#;
        assert_eq!(senses(good).expect("a well-formed row").len(), 1);
        // Each case carries the reason the reader must give for it. A refusal
        // whose text nothing pins can say the opposite of what happened, and
        // three refusals that collapse into one reason send whoever reads a
        // rejected file looking for the wrong thing.
        let cases = [
            (
                r#"{"set":"mistake","version":1,"polarity":"positive","text":"x","note":"y"}"#,
                "line 1: `note` is not a key of this schema",
            ),
            (
                r#"{"set":"mistake","version":1,"polarity":"positive"}"#,
                "line 1: no `text`",
            ),
            (
                r#"{"set":"regret","version":1,"polarity":"positive","text":"x"}"#,
                "line 1: `set` is \"regret\", which names nothing",
            ),
            (
                r#"{"set":"mistake","version":1,"polarity":"neutral","text":"x"}"#,
                "line 1: `polarity` is \"neutral\", which names nothing",
            ),
            (
                r#"{"set":"mistake","version":"1","polarity":"positive","text":"x"}"#,
                "line 1: `version` is not the kind of value the schema names",
            ),
            (
                r#"{"set":"mistake","version":0,"polarity":"positive","text":"x"}"#,
                "line 1: `version` is 0, and versions start at one",
            ),
            (
                r#"{"set":"mistake","version":1,"polarity":"positive","text":""}"#,
                "line 1: `text` is empty",
            ),
            ("", "no rows: a file of nothing is a missing file"),
            (
                "{\"set\":\"mistake\",\"version\":1,\"polarity\":\"positive\",\"text\":\"x\"}\n\
                 {\"set\":\"mistake\",\"version\":1,\"polarity\":\"positive\",\"text\":\"x\"}",
                "line 2: the text \"x\" is already present",
            ),
        ];
        let mut reasons = BTreeSet::new();
        for (source, reason) in cases {
            let err = senses(source)
                .err()
                .unwrap_or_else(|| panic!("the sense-set schema admitted {source}"));
            assert_eq!(
                err.to_string(),
                reason,
                "the sense-set schema refused {source} and said something else"
            );
            reasons.insert(err.to_string());
        }
        assert_eq!(
            reasons.len(),
            cases.len(),
            "two refusals of the sense-set schema gave the same reason"
        );
        // A line the grammar itself rejects is refused as a line, carrying the
        // parser's own complaint rather than a reason invented here.
        let err = senses("[1]").expect_err("an array is not a row");
        assert!(matches!(err, DataError::Line { line: 1, .. }), "{err:?}");
        assert!(
            err.to_string().starts_with("line 1: not an object line"),
            "{err}"
        );
    }

    // ---- the register ----

    // Every `.jsonl` in the register directory is classified: it is a
    // register whose name says its source and its set, or it is a declared
    // sidecar. Nothing is skipped, because a walk that ignored what it did
    // not recognise would ignore a register whose name was mistyped and call
    // the directory clean.
    #[test]
    fn every_register_is_named_for_what_is_in_it() {
        let dir = register_dir();
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
            .collect();
        files.sort();
        assert!(
            !files.is_empty(),
            "{}: no registers, so every assertion over them would hold vacuously",
            dir.display()
        );
        let mut registers = 0_usize;
        for path in &files {
            let name = path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            let Some(declared) = RegisterName::of(stem) else {
                assert!(
                    REGISTER_SIDECARS
                        .iter()
                        .any(|suffix| name.ends_with(suffix)),
                    "{}: not `<source>-<set>.jsonl` and not a declared sidecar, so \
                     nothing here knows what it is",
                    path.display()
                );
                continue;
            };
            registers += 1;
            let text = std::fs::read_to_string(path)
                .unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            let rows = register(&text).unwrap_or_else(|err| panic!("{}: {err}", path.display()));
            assert!(rows.len() >= 10, "{}: {} rows", path.display(), rows.len());
            // The name and the rows are two spellings of one fact, and this
            // is the one place they meet.
            assert!(
                rows.iter().all(|row| row.source == declared.source),
                "{}: the name says {} and a row says otherwise",
                path.display(),
                declared.source.tag()
            );
            for label in Label::ALL {
                assert!(
                    rows.iter().any(|row| row.label == *label),
                    "{}: no {} row",
                    path.display(),
                    label.tag()
                );
            }
        }
        assert!(registers > 0, "{}: sidecars only", dir.display());
    }

    #[test]
    fn a_register_name_spells_a_source_and_a_set_or_nothing() {
        for source in Source::ALL {
            for set in SenseSet::ALL {
                let name = RegisterName {
                    source: *source,
                    set: *set,
                };
                assert_eq!(
                    RegisterName::of(&name.stem()),
                    Some(name),
                    "{}",
                    name.stem()
                );
            }
        }
        // The set's own underscore is not the separator, and a name missing
        // either half names nothing.
        assert_eq!(
            RegisterName::of("authored-durable_fact").map(|n| n.set),
            Some(SenseSet::DurableFact)
        );
        for stem in [
            "mistake",
            "mined",
            "mined.provenance",
            "authored-",
            "-mistake",
            "invented-mistake",
            "authored-nonesuch",
        ] {
            assert!(
                RegisterName::of(stem).is_none(),
                "{stem:?} was read as a register name"
            );
        }
    }

    #[test]
    fn the_register_schema_is_closed() {
        let good = r#"{"id":"a/b","text":"x","label":"positive","source":"authored"}"#;
        assert_eq!(register(good).expect("a well-formed row").len(), 1);
        let cases = [
            (
                r#"{"id":"a/b","text":"x","label":"maybe","source":"authored"}"#,
                "line 1: `label` is \"maybe\", which names nothing",
            ),
            (
                r#"{"id":"a/b","text":"x","label":"positive","source":"guessed"}"#,
                "line 1: `source` is \"guessed\", which names nothing",
            ),
            (
                r#"{"id":"a/b","text":"x","label":"positive","source":"authored","set":"mistake"}"#,
                "line 1: `set` is not a key of this schema",
            ),
            (
                r#"{"id":"a/b","text":"x","label":"positive"}"#,
                "line 1: no `source`",
            ),
            (
                r#"{"id":"","text":"x","label":"positive","source":"authored"}"#,
                "line 1: `id` is empty",
            ),
            (
                r#"{"id":"a/b","text":"","label":"positive","source":"authored"}"#,
                "line 1: `text` is empty",
            ),
            ("", "no rows: a file of nothing is a missing file"),
            (
                "{\"id\":\"a\",\"text\":\"x\",\"label\":\"positive\",\"source\":\"authored\"}\n\
                 {\"id\":\"a\",\"text\":\"y\",\"label\":\"positive\",\"source\":\"authored\"}",
                "line 2: the id \"a\" is already bound",
            ),
        ];
        let mut reasons = BTreeSet::new();
        for (source, reason) in cases {
            let err = register(source)
                .err()
                .unwrap_or_else(|| panic!("the register schema admitted {source}"));
            assert_eq!(
                err.to_string(),
                reason,
                "the register schema refused {source} and said something else"
            );
            reasons.insert(err.to_string());
        }
        assert_eq!(
            reasons.len(),
            cases.len(),
            "two refusals of the register schema gave the same reason"
        );
        assert!(matches!(
            register(
                "{\"id\":\"a\",\"text\":\"x\",\"label\":\"positive\",\"source\":\"authored\"}\n\
                 {\"id\":\"a\",\"text\":\"y\",\"label\":\"positive\",\"source\":\"authored\"}"
            ),
            Err(DataError::DuplicateId { line: 2, .. })
        ));
    }

    // ---- embedders ----

    // A miss is a refusal, not a zero. A row the model never saw, scored at
    // zero, is ranked as though it had been seen and found unlike everything
    // -- which for a scoring whose floor is below zero is not even the bottom.
    #[test]
    fn a_row_the_embedder_cannot_place_is_refused_and_not_scored_at_zero() {
        let rows = register_rows();
        let set = embedded(SenseSet::Mistake);
        let absent = rows
            .iter()
            .find(|row| row.id == "authored/positive/actually-the-flag")
            .expect("the seeded positive")
            .clone();
        let incomplete = Incomplete(absent.text.clone());
        assert_eq!(incomplete.id(), "incomplete");
        for cell in Cell::all() {
            let outcome = score_rows(&rows, &set, &incomplete, cell);
            assert_eq!(
                outcome,
                Err(ScoreError::Unembeddable {
                    id: absent.id.clone()
                }),
                "{}: a row the embedder could not place was scored anyway",
                cell.tag()
            );
        }
        let err = ScoreError::Unembeddable {
            id: absent.id.clone(),
        };
        assert!(err.to_string().contains("a miss is not a zero"), "{err}");
        assert!(
            matches!(
                controls(&rows, &set, &incomplete, Scoring::RawCosine),
                Err(ControlFailure::Unscorable(_))
            ),
            "the controls passed a register the embedder could not place"
        );
        assert!(
            score_rows(
                &rows,
                &set,
                &Fixture,
                Cell {
                    scoring: Scoring::RawCosine,
                    gate: Gate::Without,
                },
            )
            .is_ok(),
            "the same register places fine under an embedder that holds it"
        );
        // And a sense the embedder cannot place is refused before any cell is
        // built on it, rather than becoming a set scored against nothing.
        assert!(matches!(
            EmbeddedSet::embed(
                &shipped(),
                SenseSet::Mistake,
                &Incomplete(literal(SenseSet::Mistake, Polarity::Positive)),
            ),
            Err(SetError::Unembeddable { .. })
        ));
    }

    #[test]
    fn the_fixture_embedder_is_deterministic_and_token_based() {
        let text = "Actually, the flag was never read.";
        let vector = Fixture.embed(text);
        assert_eq!(vector.len(), Fixture::DIMENSIONS);
        assert!(
            vector == Fixture.embed(text),
            "the same text twice is not the same vector"
        );
        assert!(
            vector == Fixture.embed("read never was flag the ACTUALLY"),
            "tokens, not order, case or punctuation"
        );
        assert!(vector != Fixture.embed("The build takes four minutes."));
        assert!(
            Fixture
                .embed("")
                .iter()
                .all(|component| component.abs() < EPSILON),
            "no tokens, no direction"
        );
        assert_eq!(Fixture.id(), "fixture");
        assert_eq!(
            Fixture::tokens("Oh, I see -- it's available!"),
            vec!["oh", "i", "see", "it", "s", "available"]
        );
        // Counts, not presence. Every score in this module's tests comes from
        // this embedder, and a set-of-tokens embedder places every register
        // row carrying a repeated word somewhere else -- so the contract the
        // control rows land at their extremes *by construction* under is this
        // one, and it is asserted rather than assumed.
        let twice = Fixture.embed("alpha alpha beta");
        let once = Fixture.embed("alpha beta");
        assert!(
            twice != once,
            "the fixture embedder counted a repeated token once, so it is a set and not a \
             multiset"
        );
        let alpha = super::bucket(super::fnv1a(b"alpha"));
        assert!(
            near(twice[alpha], 2.0) && near(once[alpha], 1.0),
            "a token twice is a component of two: {} against {}",
            twice[alpha],
            once[alpha]
        );
    }

    #[test]
    fn a_cached_embedder_reads_decimal_vectors_and_reports_a_miss() {
        let source = "{\"text\":\"a b\",\"vector\":[0.5000,0.5000,0.0000]}\n\
                      {\"text\":\"c\",\"vector\":[0.0000,0.0000,1.0000]}\n";
        let cached = Cached::load("model-x", source).expect("a well-formed cache");
        assert_eq!(cached.id(), "model-x");
        assert_eq!((cached.dimensions(), cached.len()), (3, 2));
        assert!(!cached.is_empty());
        assert!(cached.holds("a b") && !cached.holds("z"));
        assert!(near(cached.embed("c")[2], 1.0));
        assert!(near(
            cosine(&cached.embed("a b"), &cached.embed("a b")).expect("a cosine"),
            1.0
        ));
        assert!(
            cached.embed("z").is_empty(),
            "a text the cache does not hold embeds to nothing, not to a guess"
        );
        let cases = [
            (
                "{\"text\":\"a\",\"vector\":[1,0]}",
                "line 1: `vector` is not the kind of value the schema names",
            ),
            (
                "{\"text\":\"a\",\"vector\":[1.0,0.0]}\n{\"text\":\"b\",\"vector\":[1.0]}",
                "line 2: a vector of 1 components where the cache holds 2",
            ),
            (
                "{\"text\":\"a\",\"vector\":[1.0]}\n{\"text\":\"a\",\"vector\":[0.5]}",
                "line 2: the text \"a\" is already present",
            ),
            (
                "{\"text\":\"a\",\"vector\":[1.0],\"model\":\"m\"}",
                "line 1: `model` is not a key of this schema",
            ),
            (
                "{\"text\":\"a\",\"vector\":[]}",
                "line 1: `vector` is empty",
            ),
            ("{\"text\":\"a\"}", "line 1: no `vector`"),
            ("", "no rows: a file of nothing is a missing file"),
        ];
        let mut reasons = BTreeSet::new();
        for (source, reason) in cases {
            let err = Cached::load("m", source)
                .err()
                .unwrap_or_else(|| panic!("the cache schema admitted {source}"));
            assert_eq!(
                err.to_string(),
                reason,
                "the cache schema refused {source} and said something else"
            );
            reasons.insert(err.to_string());
        }
        assert_eq!(
            reasons.len(),
            cases.len(),
            "two refusals of the cache schema gave the same reason"
        );
    }

    #[test]
    fn a_set_is_refused_when_it_cannot_be_scored_against() {
        let one_sided = senses(
            r#"{"set":"mistake","version":1,"polarity":"positive","text":"only a positive"}"#,
        )
        .expect("one row");
        assert!(matches!(
            EmbeddedSet::embed(&one_sided, SenseSet::Mistake, &Fixture),
            Err(SetError::NoParaphrase {
                polarity: Polarity::Negative,
                ..
            })
        ));
        let mixed = senses(
            "{\"set\":\"mistake\",\"version\":1,\"polarity\":\"positive\",\"text\":\"a\"}\n\
             {\"set\":\"mistake\",\"version\":2,\"polarity\":\"negative\",\"text\":\"b\"}",
        )
        .expect("two rows");
        assert!(
            matches!(
                EmbeddedSet::embed(&mixed, SenseSet::Mistake, &Fixture),
                Err(SetError::MixedVersions { .. })
            ),
            "two versions in one set were scored against as though they were one instrument"
        );
        let set = embedded(SenseSet::Mistake);
        assert_eq!(set.version(), 1);
        assert_eq!(set.embedder(), "fixture");
        assert_eq!(set.set(), SenseSet::Mistake);
        for polarity in Polarity::ALL {
            let first: &Embedded = &set.paraphrases(*polarity)[0];
            assert_eq!(set.literal(*polarity).sense.text, first.sense.text);
        }
        // Each refusal says which one it is. They were unexecuted, and an
        // unexecuted reason can say the opposite of what happened.
        assert_eq!(
            SetError::NoParaphrase {
                set: SenseSet::Mistake,
                polarity: Polarity::Negative,
            }
            .to_string(),
            "mistake has no negative sense, so there is nothing to score against"
        );
        assert_eq!(
            SetError::MixedVersions {
                set: SenseSet::Reversal,
            }
            .to_string(),
            "reversal carries more than one version"
        );
        assert_eq!(
            SetError::Unembeddable {
                set: SenseSet::DurableFact,
                text: "a sense".to_owned(),
            }
            .to_string(),
            "durable_fact: the embedder could not place \"a sense\""
        );
    }

    // ---- scoring ----

    #[test]
    fn cosine_of_a_vector_with_itself_is_one() {
        for text in [
            "Actually, the flag was never read.",
            "one",
            "the quick brown fox jumps",
        ] {
            let vector = Fixture.embed(text);
            let value =
                cosine(&vector, &vector).expect("a nonzero vector has a cosine with itself");
            assert!(
                near(value, 1.0),
                "cosine of a vector with itself was not one: {value} for {text:?}"
            );
        }
        assert!(
            near(cosine(&[3.0, 4.0], &[6.0, 8.0]).expect("a cosine"), 1.0),
            "scale is not direction"
        );
        assert!(near(
            cosine(&[1.0, 0.0], &[0.0, 1.0]).expect("a cosine"),
            0.0
        ));
        assert!(near(
            cosine(&[1.0, 0.0], &[-1.0, 0.0]).expect("a cosine"),
            -1.0
        ));
        assert!(
            cosine(&[1.0, 0.0], &[1.0]).is_none(),
            "vectors of different dimension have no cosine"
        );
        assert!(
            cosine(&[0.0, 0.0], &[1.0, 0.0]).is_none(),
            "a zero vector has no direction"
        );
        assert!(cosine(&[], &[]).is_none());
    }

    #[test]
    fn every_scoring_function_puts_the_controls_at_the_extremes() {
        let rows = register_rows();
        for set in SenseSet::ALL {
            let embedded = embedded(*set);
            for scoring in Scoring::ALL {
                controls(&rows, &embedded, &Fixture, *scoring).unwrap_or_else(|err| {
                    panic!(
                        "{}/{}: a control row was not at its extreme: {err}",
                        set.tag(),
                        scoring.tag()
                    )
                });
            }
        }
        // The check must be able to fail, or it is not a check: an embedder
        // that puts every text at one point ties the verbatim sense with the
        // whole register.
        let flat = EmbeddedSet::embed(&shipped(), SenseSet::Mistake, &Constant)
            .expect("a constant embedder still embeds");
        for scoring in Scoring::ALL {
            let err = controls(&rows, &flat, &Constant, *scoring)
                .err()
                .unwrap_or_else(|| {
                    panic!(
                        "{}: the controls passed an embedder that cannot tell texts apart",
                        scoring.tag()
                    )
                });
            assert!(matches!(err, ControlFailure::Inverted { .. }), "{err:?}");
        }
        // Every reason, rendered. They were unexecuted but for one, and a
        // reason nothing renders can report the run that failed as the run
        // that passed.
        assert_eq!(
            ControlFailure::Missing {
                control: Control::VerbatimPositive,
            }
            .to_string(),
            "verbatim_positive was never scored"
        );
        assert_eq!(
            ControlFailure::Inverted {
                top: 0.25,
                bottom: 0.5,
            }
            .to_string(),
            "the top control scored 0.25 and the bottom control 0.5"
        );
        assert_eq!(
            ControlFailure::NotAtTop {
                control: Control::VerbatimPositive,
                score: 1.0,
                row: "authored/positive/actually-the-flag".to_owned(),
                other: 1.0,
            }
            .to_string(),
            "verbatim_positive scored 1 and authored/positive/actually-the-flag reached it at \
             1: the scoring cannot tell the sense itself from the register"
        );
        assert_eq!(
            ControlFailure::NotAtBottom {
                control: Control::UnrelatedWords,
                score: 0.0,
                row: "authored/hard_negative/mistakes-of-this-kind".to_owned(),
                other: -1.0,
            }
            .to_string(),
            "unrelated_words scored 0 and authored/hard_negative/mistakes-of-this-kind went \
             under it at -1"
        );
        assert_eq!(
            ControlFailure::Unscorable(ScoreError::Unembeddable {
                id: "authored/positive/actually-the-flag".to_owned(),
            })
            .to_string(),
            "authored/positive/actually-the-flag: the embedder could not place this row, and a \
             miss is not a zero"
        );
    }

    // The controls' whole claim is about the register: the sense verbatim
    // must outscore every row of it, and no row may go under the bottom
    // control. Under an embedder that ties them, the run is measuring the
    // embedder's blind spot and calling it a separation.
    #[test]
    fn a_register_row_that_reaches_a_control_is_named_as_the_row_that_displaced_it() {
        let rows = register_rows();
        let displaced = rows
            .iter()
            .find(|row| row.id == "authored/positive/actually-the-flag")
            .expect("the first register row")
            .clone();
        let impostor = Impostor {
            text: displaced.text.clone(),
            as_if: literal(SenseSet::Mistake, Polarity::Positive),
        };
        assert_eq!(impostor.id(), "impostor");
        let set = EmbeddedSet::embed(&shipped(), SenseSet::Mistake, &impostor)
            .expect("the shipped set embeds");
        for scoring in Scoring::ALL {
            let err = controls(&rows, &set, &impostor, *scoring)
                .err()
                .unwrap_or_else(|| {
                    panic!(
                        "{}: a register row reached the top control and the controls passed",
                        scoring.tag()
                    )
                });
            let ControlFailure::NotAtTop { control, row, .. } = &err else {
                panic!("{}: {err:?}", scoring.tag())
            };
            assert_eq!(
                (*control, row.as_str()),
                (scoring.extremes().0, displaced.id.as_str()),
                "{}: the wrong control or the wrong row was named",
                scoring.tag()
            );
        }
        // And a register row *under* the bottom control. The fixture
        // embedder's components are token counts, so nothing it places can go
        // there; a signed space is where a real embedder puts things.
        let under = rows
            .iter()
            .find(|row| row.id == "authored/hard_negative/mistakes-of-this-kind")
            .expect("a hard negative")
            .clone();
        let positive = literal(SenseSet::Mistake, Polarity::Positive);
        let placed = Placed::at(&[
            (positive.as_str(), [1.0, 0.0]),
            (super::UNRELATED, [0.0, 1.0]),
            (under.text.as_str(), [-1.0, 0.0]),
        ]);
        assert_eq!(placed.id(), "placed");
        let signed = EmbeddedSet::embed(&shipped(), SenseSet::Mistake, &placed)
            .expect("the shipped set embeds");
        let err = controls(&rows, &signed, &placed, Scoring::RawCosine)
            .expect_err("a register row went under the bottom control and the controls passed");
        let ControlFailure::NotAtBottom { control, row, .. } = &err else {
            panic!("{err:?}")
        };
        assert_eq!(
            (*control, row.as_str()),
            (Control::UnrelatedWords, under.id.as_str()),
            "the wrong control or the wrong row was named"
        );
    }

    #[test]
    fn contrastive_scoring_subtracts_the_negative_sense() {
        let set = embedded(SenseSet::Mistake);
        let negative = Fixture.embed(&literal(SenseSet::Mistake, Polarity::Negative));
        let raw = Scoring::RawCosine.score(&negative, &set).expect("a score");
        let contrastive = Scoring::Contrastive
            .score(&negative, &set)
            .expect("a score");
        assert!(
            contrastive < raw && contrastive < 0.0,
            "the contrastive score ignored the negative sense: raw {raw}, contrastive \
             {contrastive}"
        );
        let positive = Fixture.embed(&literal(SenseSet::Mistake, Polarity::Positive));
        let expected = 1.0 - cosine(&positive, &negative).expect("a cosine");
        assert!(near(
            Scoring::Contrastive
                .score(&positive, &set)
                .expect("a score"),
            expected
        ));
    }

    #[test]
    fn softmax_scoring_is_a_probability_over_the_sense_set() {
        let set = embedded(SenseSet::Mistake);
        for row in register_rows() {
            let mass = Scoring::Softmax
                .score(&Fixture.embed(&row.text), &set)
                .expect("a score");
            assert!((0.0..=1.0).contains(&mass), "{}: {mass}", row.id);
        }
        let on_positive = Scoring::Softmax
            .score(
                &Fixture.embed(&literal(SenseSet::Mistake, Polarity::Positive)),
                &set,
            )
            .expect("a score");
        let on_negative = Scoring::Softmax
            .score(
                &Fixture.embed(&literal(SenseSet::Mistake, Polarity::Negative)),
                &set,
            )
            .expect("a score");
        assert!(on_positive > 0.9, "the positive literal: {on_positive}");
        assert!(on_negative < 0.1, "the negative literal: {on_negative}");
        let prior = Scoring::Softmax
            .score(&Fixture.embed(super::UNRELATED), &set)
            .expect("a score");
        assert!(
            near(prior, 0.5),
            "a row equidistant from every sense gets the set's prior, not a verdict: {prior}"
        );
    }

    #[test]
    fn ensemble_max_takes_the_best_paraphrase() {
        let set = embedded(SenseSet::Mistake);
        let second = shipped()
            .into_iter()
            .filter(|sense| sense.set == SenseSet::Mistake && sense.polarity == Polarity::Positive)
            .nth(1)
            .expect("a second paraphrase")
            .text;
        let vector = Fixture.embed(&second);
        let raw = Scoring::RawCosine.score(&vector, &set).expect("a score");
        let ensemble = Scoring::EnsembleMax.score(&vector, &set).expect("a score");
        assert!(
            raw < 1.0 - EPSILON,
            "a paraphrase is not the literal: {raw}"
        );
        assert!(
            near(ensemble, 1.0),
            "the ensemble did not find the paraphrase it holds: {ensemble}"
        );
        assert!(ensemble > raw);
    }

    // ---- the lexical pre-gate ----

    #[test]
    fn the_lexical_gate_admits_only_rows_carrying_a_seed_of_the_set() {
        // The seeds by name, per set. The gate is one of the two factors the
        // bakeoff exists to measure; a seed list that can be replaced with
        // anything is a factor that measures nothing, and it is published
        // into the pre-registration besides.
        for (set, expected) in [
            (
                SenseSet::Mistake,
                &["actually", "i was wrong", "turns out", "it turns out"][..],
            ),
            (
                SenseSet::DurableFact,
                &["actually", "turns out", "it turns out"][..],
            ),
            (
                SenseSet::Reversal,
                &["oh, i see", "is available as", "turns out"][..],
            ),
        ] {
            assert_eq!(
                seeds(set),
                expected,
                "{}: the seeds are not the words the register was mined by",
                set.tag()
            );
        }
        for set in SenseSet::ALL {
            assert!(
                !seeds(*set).is_empty(),
                "{}: a set with no seeds cannot be gated",
                set.tag()
            );
        }
        assert!(Gate::With.admits(SenseSet::Mistake, "Actually, the flag was never read"));
        assert!(
            Gate::With.admits(SenseSet::Mistake, "I was  WRONG about that"),
            "case and spacing"
        );
        assert!(Gate::With.admits(
            SenseSet::Reversal,
            "Oh, I see -- it is available as a property"
        ));
        assert!(
            !Gate::With.admits(SenseSet::Mistake, "The build takes four minutes"),
            "the lexical gate admitted a row carrying no seed"
        );
        assert!(
            !Gate::With.admits(SenseSet::Mistake, "Factually speaking, it was fine"),
            "a seed inside a word is not a seed"
        );
        assert!(
            !Gate::With.admits(SenseSet::Mistake, "It turns outrageous quickly"),
            "a seed running into the next word is not a seed either"
        );
        assert!(
            !Gate::With.admits(
                SenseSet::Reversal,
                "The build is available assuming a cache"
            ),
            "a seed running into the next word is not a seed either"
        );
        assert!(Gate::Without.admits(SenseSet::Mistake, "The build takes four minutes"));
    }

    #[test]
    fn a_gated_out_row_sits_at_the_scorings_floor() {
        // Each floor by name: the lowest value its scoring can produce.
        // Contrastive is a difference of two cosines and so reaches -2, which
        // the shipped fixture embedder cannot show because its components are
        // token counts. A floor set above what the scoring can reach is a
        // floor a dropped row outranks scored rows from.
        for (scoring, floor) in [
            (Scoring::RawCosine, -1.0),
            (Scoring::Contrastive, -2.0),
            (Scoring::Softmax, 0.0),
            (Scoring::EnsembleMax, -1.0),
        ] {
            assert!(
                near(scoring.floor(), floor),
                "{}: the floor is {} where {floor} is the lowest the scoring can produce",
                scoring.tag(),
                scoring.floor()
            );
        }
        // And the contrastive floor is not spare: a signed embedder reaches
        // below a single cosine's -1, so a floor of -1 would rank a dropped
        // row above a row that was scored.
        let positive = literal(SenseSet::Mistake, Polarity::Positive);
        let negative = literal(SenseSet::Mistake, Polarity::Negative);
        let leaning = "a row leaning to the negative sense";
        let placed = Placed::at(&[
            (positive.as_str(), [1.0, 0.0]),
            (negative.as_str(), [0.0, 1.0]),
            (leaning, [-1.0, 1.0]),
        ]);
        let signed = EmbeddedSet::embed(&shipped(), SenseSet::Mistake, &placed)
            .expect("the shipped set embeds");
        let below = Scoring::Contrastive
            .score(&placed.embed(leaning), &signed)
            .expect("a score");
        assert!(
            below < -1.0 && below >= Scoring::Contrastive.floor(),
            "a contrastive score reached {below}, which the floor must sit under"
        );
        for scoring in Scoring::ALL {
            let rows = scored(Cell {
                scoring: *scoring,
                gate: Gate::With,
            });
            let dropped = rows
                .iter()
                .find(|row| row.id == "authored/positive/the-parser-was-fine")
                .expect("the seedless positive");
            assert!(!dropped.admitted);
            assert!(
                near(dropped.score, scoring.floor()),
                "{}: a row the gate dropped was not put at the floor: {}",
                scoring.tag(),
                dropped.score
            );
            let kept = rows
                .iter()
                .find(|row| row.id == "authored/positive/actually-the-flag")
                .expect("the seeded positive");
            assert!(kept.admitted && kept.score > scoring.floor());
            assert!(rows.iter().all(|row| row.score >= scoring.floor()));
        }
        assert!(ungated().iter().all(|row| row.admitted));
        // The gate decides on the row's *text*, row by row. The shipped ids
        // are slugs of their texts and mostly agree by accident, so a gate
        // asked about the wrong field drops a row that plainly carries its
        // seed and nothing named would notice.
        let shipped_rows = register_rows();
        let gated = scored(Cell {
            scoring: Scoring::RawCosine,
            gate: Gate::With,
        });
        for (row, outcome) in shipped_rows.iter().zip(&gated) {
            assert_eq!(
                outcome.admitted,
                Gate::With.admits(SenseSet::Mistake, &row.text),
                "{}: the gate did not decide on the row's text",
                row.id
            );
        }
        assert!(
            gated
                .iter()
                .find(|row| row.id == "authored/positive/i-was-wrong-about-the-cache")
                .expect("the row whose id is not its text")
                .admitted,
            "a row carrying its seed in the text and not in the id was dropped"
        );
    }

    #[test]
    fn every_cell_is_a_scoring_and_a_gate() {
        let cells = Cell::all();
        assert_eq!(cells.len(), Scoring::ALL.len() * Gate::ALL.len());
        let tags: BTreeSet<String> = cells.iter().map(|cell| cell.tag()).collect();
        assert_eq!(tags.len(), cells.len(), "two cells share a name");
        for scoring in Scoring::ALL {
            for gate in Gate::ALL {
                assert!(cells.contains(&Cell {
                    scoring: *scoring,
                    gate: *gate
                }));
            }
        }
    }

    // ---- metrics ----

    fn ranked_rows() -> Vec<Scored> {
        vec![
            row("p1", Label::Positive, 0.9),
            row("n1", Label::Negative, 0.8),
            row("p2", Label::Positive, 0.7),
            row("h1", Label::HardNegative, 0.6),
            row("p3", Label::Positive, 0.1),
        ]
    }

    #[test]
    fn precision_at_k_counts_positives_among_the_top_k() {
        let rows = ranked_rows();
        assert_eq!(
            precision_at_k(&rows, 3),
            Fraction { hits: 2, of: 3 },
            "precision at k was counted from the wrong end of the ranking"
        );
        assert_eq!(precision_at_k(&rows, 1), Fraction { hits: 1, of: 1 });
        assert_eq!(
            precision_at_k(&rows, 10),
            Fraction { hits: 3, of: 5 },
            "a budget larger than the register is the register"
        );
        assert_eq!(precision_at_k(&[], 3), Fraction { hits: 0, of: 0 });
        assert!(Fraction { hits: 0, of: 0 }.as_f64().is_none());
        assert!(near(
            Fraction { hits: 1, of: 4 }.as_f64().expect("a value"),
            0.25
        ));
        let tied = [
            row("b", Label::Negative, 0.5),
            row("a", Label::Positive, 0.5),
        ];
        assert_eq!(
            precision_at_k(&tied, 1),
            Fraction { hits: 1, of: 1 },
            "a tie is broken by id, so the ranking is the same on every run"
        );
        assert_eq!(precision_at_k(&rows, 3).to_string(), "2/3");
    }

    #[test]
    fn over_firing_is_the_share_of_hard_negatives_nominated() {
        let rows = ranked_rows();
        assert_eq!(
            over_firing(&rows, 4),
            Fraction { hits: 1, of: 1 },
            "the hard negative at rank four was nominated and was not counted"
        );
        assert_eq!(
            over_firing(&rows, 3),
            Fraction { hits: 0, of: 1 },
            "over-firing counted a row that is not a hard negative"
        );
        assert_eq!(
            over_firing(&[row("n", Label::Negative, 0.9)], 1),
            Fraction { hits: 0, of: 0 },
            "no hard negatives, no over-firing rate"
        );
    }

    #[test]
    fn auc_is_the_probability_a_positive_outranks_a_negative() {
        let perfect = [
            row("p1", Label::Positive, 0.9),
            row("p2", Label::Positive, 0.8),
            row("n1", Label::Negative, 0.2),
            row("h1", Label::HardNegative, 0.1),
        ];
        assert!(near(auc(&perfect).expect("an auc"), 1.0));
        let inverted = [
            row("p1", Label::Positive, 0.1),
            row("n1", Label::Negative, 0.9),
        ];
        assert!(near(auc(&inverted).expect("an auc"), 0.0));
        let tied = [
            row("p1", Label::Positive, 0.5),
            row("n1", Label::Negative, 0.5),
        ];
        assert!(
            near(auc(&tied).expect("an auc"), 0.5),
            "a tie is half a win"
        );
        // p1 beats both, p2 beats the hard negative only, p3 beats neither.
        assert!(near(auc(&ranked_rows()).expect("an auc"), 0.5));
        assert!(
            auc(&[row("p", Label::Positive, 0.5)]).is_none(),
            "no negatives, no ranking"
        );
        assert!(auc(&[]).is_none());
    }

    #[test]
    fn d_prime_is_the_standardised_separation_of_the_two_means() {
        let rows = [
            row("p1", Label::Positive, 0.8),
            row("p2", Label::Positive, 0.9),
            row("p3", Label::Positive, 1.0),
            row("n1", Label::Negative, 0.1),
            row("n2", Label::Negative, 0.2),
            row("h1", Label::HardNegative, 0.3),
        ];
        assert!(near(d_prime(&rows).expect("a d-prime"), 7.0));
        let inverted: Vec<Scored> = rows
            .iter()
            .map(|r| Scored {
                score: 1.1 - r.score,
                ..r.clone()
            })
            .collect();
        assert!(near(d_prime(&inverted).expect("a d-prime"), -7.0));
        // Both spreads, pooled. The rows above have the same variance in each
        // class, so they cannot tell a pooled standard deviation from one
        // class's own; these have variances of 0.005 and 0.125.
        let lopsided = [
            row("p1", Label::Positive, 1.0),
            row("p2", Label::Positive, 1.1),
            row("n1", Label::Negative, 0.0),
            row("n2", Label::Negative, 0.5),
        ];
        let separation = d_prime(&lopsided).expect("a d-prime");
        assert!(
            near(separation, 3.137_858_162_210_944),
            "d-prime was standardised by one class's spread and not by both: {separation}"
        );
        let too_few = [
            row("p1", Label::Positive, 0.8),
            row("n1", Label::Negative, 0.1),
            row("n2", Label::Negative, 0.2),
        ];
        assert!(d_prime(&too_few).is_none(), "one positive has no spread");
        let flat = [
            row("p1", Label::Positive, 0.5),
            row("p2", Label::Positive, 0.5),
            row("n1", Label::Negative, 0.5),
            row("n2", Label::Negative, 0.5),
        ];
        assert!(
            d_prime(&flat).is_none(),
            "no spread, no standardised separation"
        );
    }

    /// Rows a metric must find a separation in. The null over them must not.
    fn separated() -> Vec<Scored> {
        vec![
            row("p1", Label::Positive, 0.97),
            row("p2", Label::Positive, 0.93),
            row("p3", Label::Positive, 0.89),
            row("p4", Label::Positive, 0.85),
            row("p5", Label::Positive, 0.81),
            row("n1", Label::Negative, 0.19),
            row("n2", Label::Negative, 0.15),
            row("n3", Label::Negative, 0.11),
            row("n4", Label::Negative, 0.07),
            row("h1", Label::HardNegative, 0.23),
            row("h2", Label::HardNegative, 0.27),
        ]
    }

    #[test]
    fn a_shuffled_label_null_sits_at_chance() {
        // The rows are separated on purpose: a "null" that does not shuffle
        // reports this separation as what chance looks like, and every cell
        // measured against it is then measured against itself.
        let rows = separated();
        assert!(
            d_prime(&rows).expect("a d-prime") > 5.0,
            "the null's own subject must carry a separation for the null to erase"
        );
        let null = shuffled_null(&rows, NULL_SHUFFLES, 7).expect("a null over separated rows");
        // Against the numbers, not against the constants: an assertion that
        // reads the band back is loosened by the same edit that widens it.
        assert!(
            null.mean_d_prime.abs() <= 0.25,
            "d-prime on a shuffled-label null was far from zero: {}",
            null.mean_d_prime
        );
        assert!(
            (null.mean_auc - 0.5).abs() <= 0.1,
            "the area under the curve on a shuffled-label null was outside the band around \
             chance: {}",
            null.mean_auc
        );
        assert!(null.at_chance());
        assert_eq!(null.shuffles, NULL_SHUFFLES);
        let again = shuffled_null(&rows, NULL_SHUFFLES, 7).expect("a null");
        assert!(
            near(again.mean_auc, null.mean_auc) && near(again.mean_d_prime, null.mean_d_prime),
            "the same seed is not the same null"
        );
        assert!(shuffled_null(&rows, 0, 7).is_none());
        assert!(shuffled_null(&[], NULL_SHUFFLES, 7).is_none());
        let real = super::Null {
            mean_auc: 0.9,
            mean_d_prime: 2.0,
            shuffles: 1,
        };
        assert!(!real.at_chance(), "a separation was read as chance");
    }

    // A band is a claim only when something can contradict it. These two are
    // bounded on both sides: wider than every excursion the null has been
    // measured at, and narrow enough that a null carrying a real separation
    // in one metric alone is refused -- which the other band cannot do for it.
    #[test]
    fn the_null_bands_are_wider_than_measurement_and_narrower_than_a_finding() {
        assert!(
            near(NULL_D_PRIME_BAND, 0.25) && near(NULL_AUC_BAND, 0.1),
            "the null's bands are not the numbers they were registered as: {NULL_D_PRIME_BAND} \
             and {NULL_AUC_BAND}"
        );
        let rows = separated();
        let mut widest_d_prime = 0.0_f64;
        let mut widest_auc = 0.0_f64;
        for seed in 1..=16 {
            let null = shuffled_null(&rows, NULL_SHUFFLES, seed).expect("a null");
            widest_d_prime = widest_d_prime.max(null.mean_d_prime.abs());
            widest_auc = widest_auc.max((null.mean_auc - 0.5).abs());
            assert!(
                null.at_chance(),
                "seed {seed}: a shuffled null was not read as chance: {null:?}"
            );
        }
        assert!(
            widest_d_prime < NULL_D_PRIME_BAND && widest_auc < NULL_AUC_BAND,
            "the band is not wider than the null it was measured against: {widest_d_prime} and \
             {widest_auc}"
        );
        // Each band alone, so neither can alibi the other. A null separating
        // the two classes by a whole standard deviation, or ranking a
        // positive above a negative five times in six, is not chance.
        let d_prime_only = super::Null {
            mean_auc: 0.5,
            mean_d_prime: 1.0,
            shuffles: NULL_SHUFFLES,
        };
        assert!(
            !d_prime_only.at_chance(),
            "a shuffled null separating the classes by a standard deviation was read as chance"
        );
        let auc_only = super::Null {
            mean_auc: 0.85,
            mean_d_prime: 0.0,
            shuffles: NULL_SHUFFLES,
        };
        assert!(
            !auc_only.at_chance(),
            "a shuffled null ranking the classes apart was read as chance"
        );
    }

    #[test]
    fn a_p_value_never_travels_without_its_attainable_floor() {
        assert!(near(attainable_p_floor(999), 0.001));
        assert!(
            near(attainable_p_floor(0), 1.0),
            "no resamples can attain anything below one"
        );
        let a = [1.0; 8];
        let b = [0.0; 8];
        let boot = paired_bootstrap(&a, &b, 999, 1).expect("a bootstrap");
        assert!(
            near(boot.p.floor(), attainable_p_floor(999)),
            "a bootstrap p-value came without its attainable floor: {:?}",
            boot.p
        );
        assert!(
            boot.p.value() >= boot.p.floor(),
            "a p below its own floor: {:?}",
            boot.p
        );
        assert!(
            near(boot.p.value(), attainable_p_floor(999)),
            "a difference that never crosses zero sits at the floor, not at zero: {:?}",
            boot.p
        );
        let text = boot.p.to_string();
        assert!(
            text.contains("attainable floor"),
            "a p-value was printed without its attainable floor: {text}"
        );
        let value = PRE_REGISTRATION.value();
        let Value::Object(members) = &value else {
            panic!("an object")
        };
        assert!(
            matches!(members.get("attainable_p_floor"), Some(Value::Decimal(_))),
            "the pre-registration states a resample count without the floor it implies"
        );
    }

    #[test]
    fn paired_bootstrap_is_deterministic_under_a_seed() {
        // Halves and quarters, so the pairs cancel exactly and "no observed
        // difference" is not a residue of the order the sums were taken in.
        let a = [0.875, 0.75, 0.625, 0.375, 0.25, 0.125];
        let b = [0.125, 0.25, 0.375, 0.625, 0.75, 0.875];
        let first = paired_bootstrap(&a, &b, 499, 42).expect("a bootstrap");
        let second = paired_bootstrap(&a, &b, 499, 42).expect("a bootstrap");
        assert!(
            near(first.observed, second.observed) && near(first.p.value(), second.p.value()),
            "the same seed is not the same resampling"
        );
        assert!(
            near(first.observed, 0.0),
            "the pairs cancel: {}",
            first.observed
        );
        assert!(
            near(first.p.value(), 1.0),
            "no observed difference has nothing to test: {:?}",
            first.p
        );
        assert_eq!((first.resamples, first.seed), (499, 42));
        let same = paired_bootstrap(&a, &a, 499, 42).expect("a bootstrap");
        assert!(near(same.p.value(), 1.0));
        let clear = paired_bootstrap(&[1.0; 6], &[0.0; 6], 499, 3).expect("a bootstrap");
        assert!(clear.p.value() < 0.01, "{:?}", clear.p);
        assert!(near(clear.observed, 1.0));
        assert!(matches!(
            paired_bootstrap(&a, &b[..3], 9, 1),
            Err(BootstrapError::Unpaired { a: 6, b: 3 })
        ));
        assert!(matches!(
            paired_bootstrap(&[], &[], 9, 1),
            Err(BootstrapError::Empty)
        ));
        assert!(matches!(
            paired_bootstrap(&a, &b, 0, 1),
            Err(BootstrapError::NoResamples)
        ));
        // And the seed is read. A generator that ignores it draws one
        // sequence for every run, and the seed a result carries is decoration.
        assert!(
            Xorshift::seeded(1).next_u64() != Xorshift::seeded(2).next_u64(),
            "two seeds drew the same first number, so the seed was never read"
        );
        assert!(Xorshift::seeded(0).next_u64() != 0, "every seed draws");
        assert!(Xorshift::seeded(9).below(0) == 0, "no index below nothing");
        // And it generates. Adding one to a counter satisfies both of the
        // assertions above, and every label shuffle and every resample index
        // in this module would then be drawn by counting.
        let mut generator = Xorshift::seeded(0);
        let draws: Vec<u64> = (0..64).map(|_| generator.next_u64()).collect();
        let strides: Vec<u64> = draws
            .windows(2)
            .map(|pair| pair[1].wrapping_sub(pair[0]))
            .collect();
        assert!(
            strides.windows(2).any(|pair| pair[0] != pair[1]),
            "the generator advanced by a constant stride, which is a counter and not a \
             generator"
        );
        for bit in 0..64_u32 {
            let ones = draws.iter().filter(|draw| (*draw >> bit) & 1 == 1).count();
            assert!(
                ones > 0 && ones < draws.len(),
                "bit {bit} never changed across sixty-four draws, so the high bits are a \
                 counter's"
            );
        }
        assert_eq!(
            BootstrapError::Unpaired { a: 6, b: 3 }.to_string(),
            "6 rows against 3: a paired bootstrap pairs by row"
        );
        assert_eq!(BootstrapError::Empty.to_string(), "no rows to resample");
        assert_eq!(
            BootstrapError::NoResamples.to_string(),
            "no resamples, so no p below one is attainable"
        );
    }

    // The resampling is the procedure. A bootstrap that returns the observed
    // statistic for every "resample" never crosses zero, so every p the
    // bakeoff reports is the attainable floor -- and both samples above are
    // degenerate enough that a true bootstrap agrees with it. This one is not:
    // one row of six pulls the other way.
    #[test]
    fn a_paired_bootstrap_resamples_rather_than_repeating_the_observed_difference() {
        let a = [1.0, 1.0, 1.0, -3.0, 1.0, 1.0];
        let b = [0.0; 6];
        let boot = paired_bootstrap(&a, &b, 999, 1).expect("a bootstrap");
        assert!(near(boot.observed, 1.0 / 3.0), "{}", boot.observed);
        assert!(
            boot.p.value() > boot.p.floor() && boot.p.value() < 1.0,
            "the resamples never crossed zero, so every resample was the observed difference \
             and the p sits at its own floor: {:?}",
            boot.p
        );
        assert!(near(boot.p.value(), 0.266), "{:?}", boot.p);
        let other = paired_bootstrap(&a, &b, 999, 11).expect("a bootstrap");
        assert!(
            !near(other.p.value(), boot.p.value()),
            "two seeds drew the same resamples: {:?} against {:?}",
            boot.p,
            other.p
        );
        assert!(near(other.p.value(), 0.253), "{:?}", other.p);
    }

    #[test]
    fn holm_correction_is_monotone_and_never_below_the_raw_p() {
        let raw = [0.01, 0.04, 0.03];
        let adjusted = holm(&raw);
        let expected = [0.03, 0.06, 0.06];
        for (index, (got, want)) in adjusted.iter().zip(expected).enumerate() {
            assert!(
                near(*got, want),
                "Holm correction left a p-value uncorrected: cell {index} raw {} adjusted \
                 {got}, expected {want}",
                raw[index]
            );
        }
        assert!(adjusted.iter().zip(raw).all(|(a, r)| *a >= r));
        let capped = holm(&[0.5, 0.6]);
        assert!(
            capped.iter().all(|p| near(*p, 1.0)),
            "a corrected p above one: {capped:?}"
        );
        assert!(near(holm(&[0.2])[0], 0.2), "one cell needs no correction");
        assert!(holm(&[]).is_empty());
    }

    #[test]
    fn every_metric_is_reported_only_after_failing_its_own_fixture() {
        let subject = ungated();
        for metric in Metric::ALL {
            let fixture = metric.failure_fixture();
            assert!(
                !fixture.is_empty(),
                "{}: no failure fixture, so it can never be reported",
                metric.tag()
            );
            let reported = Reported::take(*metric, 3, &subject).unwrap_or_else(|err| {
                panic!(
                    "{}: not reportable, its failure fixture did not demonstrate failure: {err}",
                    metric.tag()
                )
            });
            assert!(metric.failed(reported.demonstrated_failure()));
            assert_eq!((reported.metric(), reported.budget()), (*metric, 3));
            assert!(reported.value().is_finite());
            // The demonstration is this metric's own fixture at this budget,
            // and it is not a parameter: there is nothing for a caller to
            // substitute a staged failure into.
            assert!(
                near(
                    reported.demonstrated_failure(),
                    metric
                        .compute(3, &fixture)
                        .expect("the fixture has a value")
                ),
                "{}: the demonstration is not the metric's own fixture",
                metric.tag()
            );
            let Value::Object(members) = reported.record() else {
                panic!("a record value")
            };
            assert_eq!(
                members.get("metric"),
                Some(&Value::String(metric.tag().to_owned()))
            );
        }
        // And the record carries the numbers, not merely their kind. This is
        // precision at a budget of two over rows ranked exactly right, which
        // is one; against a fixture holding eight non-positives above its
        // positives, which is nothing; and the reading that counted as the
        // failure travels with both.
        let exact = [
            row("s/p1", Label::Positive, 0.9),
            row("s/p2", Label::Positive, 0.8),
            row("s/n1", Label::Negative, 0.2),
            row("s/h1", Label::HardNegative, 0.05),
        ];
        let reported = Reported::take(Metric::PrecisionAtK, 2, &exact).expect("reportable");
        assert!(near(reported.value(), 1.0));
        assert_eq!(
            reported.record(),
            Value::Object(BTreeMap::from([
                (
                    "metric".to_owned(),
                    Value::String("precision_at_k".to_owned())
                ),
                ("budget".to_owned(), Value::Integer(2)),
                ("value".to_owned(), decimal("1.0000")),
                ("demonstrated_failure".to_owned(), decimal("0.0000")),
                ("failure_reading".to_owned(), decimal("0.0000")),
            ])),
            "the record of a metric is not the numbers the metric produced"
        );
    }

    // "Has been seen fail" is only as strong as the reading it is compared
    // against. Moved, the same fixtures certify an instrument that never
    // failed at all, and the moved number is written into the record besides.
    #[test]
    fn every_metric_names_the_reading_at_which_it_has_failed() {
        for (metric, reading) in [
            (Metric::PrecisionAtK, 0.0),
            (Metric::OverFiring, 1.0),
            (Metric::Auc, 0.5),
            (Metric::DPrime, 0.0),
        ] {
            assert!(
                near(metric.failure_reading(), reading),
                "{}: failure is read at {} where {reading} is the worst the metric can say",
                metric.tag(),
                metric.failure_reading()
            );
            assert!(metric.failed(reading), "{}", metric.tag());
        }
        // Short of the reading is not failure. An area under the curve of
        // 0.85 is nearly perfect separation, and a reading that counted it as
        // failure would certify every number the bakeoff went on to report.
        assert!(
            !Metric::Auc.failed(0.85),
            "a near-perfect ranking was counted as a failure to rank"
        );
        assert!(!Metric::PrecisionAtK.failed(0.5) && !Metric::DPrime.failed(0.5));
        // Over-firing is the one metric where a larger number is the worse
        // result, so its comparison runs the other way.
        assert!(
            !Metric::OverFiring.failed(0.5),
            "firing at half the hard negatives was counted as firing at all of them"
        );
        assert!(Metric::OverFiring.failed(1.0) && !Metric::OverFiring.failed(0.99));
    }

    #[test]
    fn a_metric_that_has_not_been_seen_fail_is_not_reported() {
        let subject = ungated();
        // Both budgeted fixtures are budget-sensitive by construction. Outside
        // the budget they demonstrate, they report success -- and a metric
        // whose instrument has not been seen fail at the budget it is being
        // reported at says nothing about the subject.
        for (metric, budget) in [(Metric::PrecisionAtK, 9), (Metric::OverFiring, 1)] {
            let err = Reported::take(metric, budget, &subject)
                .err()
                .unwrap_or_else(|| {
                    panic!(
                        "{}: a metric was reported whose instrument was never seen fail",
                        metric.tag()
                    )
                });
            let MetricError::InstrumentNeverFailed { value, reading, .. } = &err else {
                panic!("{}: {err:?}", metric.tag())
            };
            assert!(!metric.failed(*value), "{}: {err:?}", metric.tag());
            assert!(near(*reading, metric.failure_reading()));
            assert!(err.to_string().contains("never been seen fail"), "{err}");
        }
        assert!(matches!(
            Reported::take(Metric::Auc, 3, &[]),
            Err(MetricError::Undefined { .. })
        ));
        assert_eq!(
            MetricError::Undefined {
                metric: Metric::DPrime,
                on: "the subject",
            }
            .to_string(),
            "d_prime is undefined on the subject"
        );
    }

    // ---- the pre-registration ----

    #[test]
    fn the_pre_registration_names_its_endpoints_and_its_blockers() {
        let value = PRE_REGISTRATION.value();
        let Value::Object(members) = &value else {
            panic!("an object")
        };
        let keys: Vec<&str> = members.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "attainable_p_floor",
                "blocked_on",
                "cells",
                "comparator",
                "controls",
                "correction",
                "metrics",
                "null",
                "over_firing",
                "primary",
                "resamples",
                "seeds",
                "separation",
                "sets",
                "softmax_temperature",
            ],
            "the pre-registration does not carry what it was registered with"
        );
        // The endpoints in words. A pre-registration nothing pins can be
        // rewritten once the numbers are in, which is the one thing it exists
        // to prevent.
        let text = |value: &str| Some(Value::String(value.to_owned()));
        assert_eq!(
            members.get("primary").cloned(),
            text(
                "precision at a fixed nomination budget, the top k of the register, per \
                 embedder, scoring and gate"
            ),
            "the primary endpoint is not the endpoint that was registered"
        );
        assert_eq!(
            members.get("separation").cloned(),
            text("the area under the curve and the standardised separation, per cell"),
            "the separation endpoint is not the endpoint that was registered"
        );
        assert_eq!(
            members.get("over_firing").cloned(),
            text("the share of hard-negative rows nominated within the budget"),
            "the over-firing endpoint is not the endpoint that was registered"
        );
        assert_eq!(
            members.get("comparator").cloned(),
            text(
                "an entailment cross-encoder as the accuracy ceiling, so the gap between it and \
                 an embedder is priced rather than assumed"
            ),
            "the accuracy-ceiling comparator is not the one that was registered"
        );
        assert_eq!(
            members.get("correction").cloned(),
            text(
                "paired bootstrap across embedders, Holm-corrected across cells, the attainable \
                 p floor printed beside every p"
            ),
            "the correction is not the one that was registered"
        );
        // And what the run is waiting on, in words rather than by tag: a
        // blocker whose description says nothing is a blocker nobody can act
        // on, and the tags alone would still list four of them.
        let blocker = |tag: &str, needs: &str| {
            Value::Object(BTreeMap::from([
                ("blocker".to_owned(), Value::String(tag.to_owned())),
                ("needs".to_owned(), Value::String(needs.to_owned())),
            ]))
        };
        assert_eq!(
            members.get("blocked_on").cloned(),
            Some(Value::Array(vec![
                blocker(
                    "archived_transcripts",
                    "archived transcripts to mine the register from",
                ),
                blocker(
                    "judge_model",
                    "a judge to label the mined register under seeded controls",
                ),
                blocker(
                    "embedding_models",
                    "embedding models and an entailment cross-encoder, each pinned by revision \
                     and instruction prefix",
                ),
                blocker(
                    "regime_spelling",
                    "a ruling on how a processor-only run of local or no models is spelled as a \
                     regime under `results/`",
                ),
            ])),
            "the pre-registration does not say, in words, what the run is waiting on"
        );
        assert_eq!(PRE_REGISTRATION.blocked_on, Blocker::ALL);
    }

    // The instrument's own settings, published with the endpoints. A cell
    // list, a seed table or a resample count that can be changed without the
    // pre-registration noticing is a plan that describes a different run.
    #[test]
    fn the_pre_registration_carries_the_settings_the_instrument_runs_under() {
        let value = PRE_REGISTRATION.value();
        let Value::Object(members) = &value else {
            panic!("an object")
        };
        assert_eq!(members.get("resamples"), Some(&Value::Integer(9999)));
        assert_eq!(
            members.get("attainable_p_floor").cloned(),
            Some(decimal("0.000100")),
            "the resample count and the floor it implies are not the same claim"
        );
        assert_eq!(
            members.get("softmax_temperature").cloned(),
            Some(decimal("0.1000"))
        );
        assert_eq!(
            members.get("null").cloned(),
            Some(Value::Object(BTreeMap::from([
                ("shuffles".to_owned(), Value::Integer(200)),
                ("auc_band".to_owned(), decimal("0.1000")),
                ("d_prime_band".to_owned(), decimal("0.2500")),
            ])))
        );
        let list = |items: &[&str]| {
            Value::Array(
                items
                    .iter()
                    .map(|item| Value::String((*item).to_owned()))
                    .collect(),
            )
        };
        assert_eq!(
            members.get("sets").cloned(),
            Some(list(&["mistake", "durable_fact", "reversal"]))
        );
        assert_eq!(
            members.get("controls").cloned(),
            Some(list(&[
                "verbatim_positive",
                "verbatim_negative",
                "unrelated_words",
            ]))
        );
        assert_eq!(
            members.get("cells").cloned(),
            Some(list(&[
                "raw_cosine+without_gate",
                "raw_cosine+with_gate",
                "contrastive+without_gate",
                "contrastive+with_gate",
                "softmax+without_gate",
                "softmax+with_gate",
                "ensemble_max+without_gate",
                "ensemble_max+with_gate",
            ]))
        );
        let Some(Value::Array(cells)) = members.get("cells") else {
            panic!("cells is a list")
        };
        assert_eq!(cells.len(), Cell::all().len());
        let metric_entry = |tag: &str, reading: &str| {
            Value::Object(BTreeMap::from([
                ("metric".to_owned(), Value::String(tag.to_owned())),
                ("failure_reading".to_owned(), decimal(reading)),
            ]))
        };
        assert_eq!(
            members.get("metrics").cloned(),
            Some(Value::Array(vec![
                metric_entry("precision_at_k", "0.0000"),
                metric_entry("over_firing", "1.0000"),
                metric_entry("auc", "0.5000"),
                metric_entry("d_prime", "0.0000"),
            ])),
            "the readings the pre-registration calls failure are not the readings the metrics \
             use"
        );
        assert_eq!(
            members.get("seeds").cloned(),
            Some(Value::Object(BTreeMap::from([
                (
                    "mistake".to_owned(),
                    list(&["actually", "i was wrong", "turns out", "it turns out"]),
                ),
                (
                    "durable_fact".to_owned(),
                    list(&["actually", "turns out", "it turns out"]),
                ),
                (
                    "reversal".to_owned(),
                    list(&["oh, i see", "is available as", "turns out"]),
                ),
            ]))),
            "the seeds the pre-registration publishes are not the seeds the gate applies"
        );
        let mut rendered = String::new();
        json::render(&value, &mut rendered);
        assert!(
            json::line(&rendered).is_ok(),
            "the pre-registration does not read back as a record value: {rendered}"
        );
    }

    // ---- vocabularies ----

    // Every vocabulary, spelled out. The tag list is pinned rather than
    // derived, because an `ALL` with a member missing makes every test that
    // iterates it agree: the cells shrink, the round trip still holds, and the
    // arm that vanished is the one nobody measured.
    #[test]
    fn every_vocabulary_names_its_members_and_round_trips_their_tags() {
        fn check<T: Copy + PartialEq + std::fmt::Debug>(
            all: &[T],
            tag: impl Fn(T) -> &'static str,
            from_tag: impl Fn(&str) -> Option<T>,
            expected: &[&str],
        ) {
            let tags: Vec<&str> = all.iter().map(|item| tag(*item)).collect();
            assert_eq!(tags, expected, "the vocabulary is not what it promises");
            let unique: BTreeSet<&str> = tags.iter().copied().collect();
            assert_eq!(unique.len(), all.len(), "two variants share a tag: {all:?}");
            for item in all {
                assert_eq!(from_tag(tag(*item)), Some(*item));
            }
            assert!(from_tag("").is_none());
            assert!(from_tag("no such tag").is_none());
        }
        check(
            SenseSet::ALL,
            SenseSet::tag,
            SenseSet::from_tag,
            &["mistake", "durable_fact", "reversal"],
        );
        check(
            Polarity::ALL,
            Polarity::tag,
            Polarity::from_tag,
            &["positive", "negative"],
        );
        check(
            Label::ALL,
            Label::tag,
            Label::from_tag,
            &["positive", "negative", "hard_negative"],
        );
        check(
            Source::ALL,
            Source::tag,
            Source::from_tag,
            &["authored", "mined"],
        );
        check(
            Scoring::ALL,
            Scoring::tag,
            Scoring::from_tag,
            &["raw_cosine", "contrastive", "softmax", "ensemble_max"],
        );
        check(
            Gate::ALL,
            Gate::tag,
            Gate::from_tag,
            &["without_gate", "with_gate"],
        );
        check(
            Metric::ALL,
            Metric::tag,
            Metric::from_tag,
            &["precision_at_k", "over_firing", "auc", "d_prime"],
        );
        check(
            Control::ALL,
            Control::tag,
            Control::from_tag,
            &["verbatim_positive", "verbatim_negative", "unrelated_words"],
        );
        check(
            Blocker::ALL,
            Blocker::tag,
            Blocker::from_tag,
            &[
                "archived_transcripts",
                "judge_model",
                "embedding_models",
                "regime_spelling",
            ],
        );
        assert!(Label::Positive.is_positive() && !Label::HardNegative.is_positive());
        assert!(Label::HardNegative.is_hard_negative() && !Label::Negative.is_hard_negative());
    }
}
