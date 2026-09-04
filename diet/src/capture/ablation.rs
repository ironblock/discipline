//! Which words of the fork-local imperative do the work, and what each one
//! costs in silence.
//!
//! One sentence added to the interview ask -- an instruction to answer from
//! the current turn alone -- raised engagement by 22 points over a 630-call,
//! seven-arm experiment, and the lift survived multiple-comparison
//! correction. The same sentence placed at the top of the conversation rather
//! than inside the fork was indistinguishable from a sham arm. Two things
//! came out of that: wording is load-bearing, and an imperative works at the
//! cheapest site where it is effective and no further up.
//!
//! What it did not establish is WHICH CLAUSE does the work. The sentence
//! bundles three: a restriction of scope to the turn, an exclusion of the
//! session behind it, and an implicit permission to be brief. Guessing which
//! one carries the lift has already been paid for once -- on a related
//! instruction an anti-lead-in clause looked like a clean win on the mean and
//! turned out to produce SILENT COLLAPSES, an answer of nothing at all, in a
//! minority of cases. Worst-case silence is strictly worse than worst-case
//! junk. So silence is a pre-registered endpoint here, beside engagement, and
//! not a diagnostic somebody reads off the tail afterwards.
//!
//! ## What is pre-registered
//!
//! * **Endpoints.** [`Endpoint::ALL`]: engagement, higher is better; silence,
//!   lower is better. Both are declared before any arm runs, and both are
//!   reported for every arm. An arm that buys engagement with occasional
//!   silence is not a winner, and the only way to keep that judgement honest
//!   is to have written the second endpoint down first.
//! * **Arms.** [`arms`] is the power set of [`Clause::ALL`], the empty set
//!   included. The empty set is [`Arm::CONTROL`] -- the sentence removed
//!   entirely -- and it is the arm that says whether the instrument measures
//!   what the original measured. An ablation whose control is missing can
//!   rank its clauses against each other and cannot say that any of them
//!   beats saying nothing.
//! * **The test.** A paired bootstrap over the same forks, [`paired_bootstrap`],
//!   with its attainable p floor reported beside every p it produces. A
//!   sample size bounds the smallest p it can produce, and a p quoted without
//!   that bound reads as a strength it does not have.
//!
//! ## What has been run
//!
//! Nothing. Driving an arm needs a model to answer the ask, and there is none
//! here. Every number this module produces is arithmetic over its own
//! fixtures: the grades in `diet/capture/ablation/corpus/`, and bootstrap
//! p-values over outcome vectors a caller hands in. The clause texts, the
//! arms, the graders and the test are the instrument; the run is not in this
//! tree and no result of it is claimed anywhere in it.

use std::collections::BTreeSet;
use std::error::Error;
use std::fmt;

use crate::formats::interview::{Answer, Outcome};
use crate::formats::record::json::{self, Value};

// ---------------------------------------------------------------------------
// the clauses, and the arms they make
// ---------------------------------------------------------------------------

/// One clause of the fork-local imperative.
///
/// An enum rather than a row index, because an arm is a set of these and a
/// set of row indices into a file that can be reordered is a set that means
/// something different tomorrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Clause {
    /// Restricts the answer to the current turn.
    Scope,
    /// Excludes the session behind the current turn.
    Exclusion,
    /// Permits a short answer, which the bundled sentence only implied.
    Brevity,
}

impl Clause {
    /// Every clause, in the order an arm renders them.
    pub const ALL: &'static [Self] = &[Self::Scope, Self::Exclusion, Self::Brevity];

    /// The name this clause is written under in the versioned data.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Scope => "scope",
            Self::Exclusion => "exclusion",
            Self::Brevity => "brevity",
        }
    }

    /// The clause `tag` names, if there is one.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|clause| clause.tag() == tag)
    }

    /// Where this clause sits in [`Clause::ALL`].
    fn position(self) -> u32 {
        let mut index = 0;
        for candidate in Self::ALL {
            if *candidate == self {
                break;
            }
            index += 1;
        }
        index
    }
}

/// One arm of the ablation: the set of clauses its imperative carries.
///
/// A set, held as a bit per clause, rather than a named variant per
/// combination. Named variants would have to be written out by hand, and the
/// combination somebody forgot to write is exactly the one the ablation was
/// run to find out about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Arm(u32);

impl Arm {
    /// The arm with no clauses at all: the imperative removed.
    ///
    /// The seeded control. Every other arm is measured against it, because
    /// "clause A beats clause B" says nothing about whether either beats an
    /// ask with no imperative in it.
    pub const CONTROL: Self = Self(0);

    /// The arm carrying exactly `clauses`.
    #[must_use]
    pub fn of(clauses: &[Clause]) -> Self {
        let mut mask = 0;
        for clause in clauses {
            mask |= 1 << clause.position();
        }
        Self(mask)
    }

    /// Whether this arm carries `clause`.
    #[must_use]
    pub fn contains(self, clause: Clause) -> bool {
        self.0 & (1 << clause.position()) != 0
    }

    /// The clauses this arm carries, in [`Clause::ALL`] order.
    #[must_use]
    pub fn clauses(self) -> Vec<Clause> {
        Clause::ALL
            .iter()
            .copied()
            .filter(|clause| self.contains(*clause))
            .collect()
    }

    /// The name this arm is reported under: its clause tags joined, or
    /// [`CONTROL_TAG`] when it carries none.
    #[must_use]
    pub fn tag(self) -> String {
        let clauses = self.clauses();
        if clauses.is_empty() {
            return CONTROL_TAG.to_owned();
        }
        clauses
            .iter()
            .map(|clause| clause.tag())
            .collect::<Vec<_>>()
            .join("+")
    }
}

/// What the control arm is reported under. Not a clause name, so it cannot
/// collide with one.
pub const CONTROL_TAG: &str = "none";

/// Every arm: the power set of [`Clause::ALL`], the empty set included.
///
/// Computed rather than listed. A clause added to the vocabulary doubles this
/// list without anybody remembering to extend it, and the arm nobody
/// remembered is the one whose absence would be read as a null result.
#[must_use]
pub fn arms() -> Vec<Arm> {
    let count = 1u32 << Clause::ALL.len();
    (0..count).map(Arm).collect()
}

// ---------------------------------------------------------------------------
// the versioned clause table
// ---------------------------------------------------------------------------

/// The versioned clause table, as shipped.
const SHIPPED: &str = include_str!("router/imperative.jsonl");

/// The key each row's version is written under.
const VERSION_KEY: &str = "version";
/// The key each row's clause name is written under.
const CLAUSE_KEY: &str = "clause";
/// The key each row's text is written under.
const TEXT_KEY: &str = "text";

/// The fork-local imperative, decomposed into clauses and versioned.
///
/// The imperative used to be one sentence in one file, which is why nobody
/// could say which part of it was doing the work. Marking the clauses is what
/// makes the question askable at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ablation {
    version: i64,
    /// One entry per [`Clause`], in [`Clause::ALL`] order. Completeness is
    /// established by [`Ablation::load`] and relied on by everything below.
    texts: Vec<(Clause, String)>,
}

impl Ablation {
    /// The clause table this crate ships.
    ///
    /// # Errors
    ///
    /// Returns [`ClauseError`] if the shipped table has drifted out of the
    /// schema. A test holds this to `Ok`, so a bad edit to the data file is a
    /// failing test rather than a runtime surprise.
    pub fn shipped() -> Result<Self, ClauseError> {
        Self::load(SHIPPED)
    }

    /// Read a clause table from JSON Lines.
    ///
    /// # Errors
    ///
    /// Returns [`ClauseError`] for a text that is not JSON Lines in the
    /// record's value space, a row with an unknown or missing key, two rows
    /// for one clause, rows that disagree about the version, a clause with no
    /// row, or a clause text that is blank or spans lines.
    pub fn load(source: &str) -> Result<Self, ClauseError> {
        let rows =
            json::objects(source).map_err(|err| ClauseError::NotJsonLines(err.to_string()))?;
        if rows.is_empty() {
            return Err(ClauseError::NoRows);
        }
        let mut version: Option<i64> = None;
        let mut found: Vec<Option<String>> = vec![None; Clause::ALL.len()];
        let mut seen: BTreeSet<Clause> = BTreeSet::new();
        for mut row in rows {
            let row_version = match row.remove(VERSION_KEY) {
                Some(Value::Integer(number)) => number,
                Some(_) => return Err(ClauseError::WrongType(VERSION_KEY)),
                None => return Err(ClauseError::MissingKey(VERSION_KEY)),
            };
            match version {
                None => version = Some(row_version),
                // Two versions in one file is a file that says two things at
                // once, and the reader that picks one of them picks silently.
                Some(first) if first != row_version => {
                    return Err(ClauseError::MixedVersions {
                        first,
                        second: row_version,
                    });
                }
                Some(_) => {}
            }
            let written = match row.remove(CLAUSE_KEY) {
                Some(Value::String(text)) => text,
                Some(_) => return Err(ClauseError::WrongType(CLAUSE_KEY)),
                None => return Err(ClauseError::MissingKey(CLAUSE_KEY)),
            };
            let clause =
                Clause::from_tag(&written).ok_or(ClauseError::UnknownClause(written.clone()))?;
            let text = match row.remove(TEXT_KEY) {
                Some(Value::String(text)) => text,
                Some(_) => return Err(ClauseError::WrongType(TEXT_KEY)),
                None => return Err(ClauseError::MissingKey(TEXT_KEY)),
            };
            if text.trim().is_empty() {
                return Err(ClauseError::BlankText(clause));
            }
            // One clause is one sentence. A clause carrying a line break is
            // two clauses wearing one label, and an ablation cannot separate
            // what the data has already welded together.
            if text.contains('\n') {
                return Err(ClauseError::TextSpansLines(clause));
            }
            // A closed schema. An unread key is a key somebody is editing in
            // the belief that it does something.
            if let Some(extra) = row.keys().next() {
                return Err(ClauseError::UnknownKey(extra.clone()));
            }
            if !seen.insert(clause) {
                return Err(ClauseError::DuplicateClause(clause));
            }
            found[clause.position() as usize] = Some(text);
        }
        let mut texts = Vec::with_capacity(Clause::ALL.len());
        for clause in Clause::ALL.iter().copied() {
            let text = found[clause.position() as usize]
                .take()
                .ok_or(ClauseError::NoRowForClause(clause))?;
            texts.push((clause, text));
        }
        Ok(Self {
            version: version.ok_or(ClauseError::NoRows)?,
            texts,
        })
    }

    /// Which version of the imperative this table is.
    #[must_use]
    pub fn version(&self) -> i64 {
        self.version
    }

    /// One clause's text.
    #[must_use]
    pub fn text(&self, clause: Clause) -> &str {
        self.texts
            .iter()
            .find(|(candidate, _)| *candidate == clause)
            .map_or("", |(_, text)| text.as_str())
    }

    /// The imperative an arm puts in the fork.
    ///
    /// Clauses in [`Clause::ALL`] order, whatever order the caller named
    /// them: two arms that differ only in the order somebody listed their
    /// clauses are one arm, and rendering them differently would make the
    /// ablation compare a sentence against itself.
    #[must_use]
    pub fn render(&self, arm: Arm) -> String {
        let mut out = String::new();
        for (clause, text) in &self.texts {
            if !arm.contains(*clause) {
                continue;
            }
            if !out.is_empty() {
                out.push(' ');
            }
            out.push_str(text);
        }
        out
    }

    /// The pre-registration: the endpoints, the arms, and the attainable p
    /// floor at `resamples`.
    #[must_use]
    pub fn plan(&self, resamples: u32) -> Plan {
        Plan {
            version: self.version,
            arms: arms()
                .into_iter()
                .map(|arm| (arm, self.render(arm)))
                .collect(),
            resamples,
        }
    }
}

/// Why a clause table is not a clause table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClauseError {
    /// The text is not JSON Lines in the record's value space.
    NotJsonLines(String),
    /// The text holds no rows, so it declares no imperative.
    NoRows,
    /// A row is missing a key the schema requires.
    MissingKey(&'static str),
    /// A row binds a key to the wrong kind of value.
    WrongType(&'static str),
    /// A row carries a key the schema does not define.
    UnknownKey(String),
    /// A row names a clause the vocabulary does not have.
    UnknownClause(String),
    /// Two rows claim the same clause.
    DuplicateClause(Clause),
    /// A clause in the vocabulary has no row.
    NoRowForClause(Clause),
    /// A clause's text is blank, so the arm carrying it is the control under
    /// another name.
    BlankText(Clause),
    /// A clause's text spans lines, so it is more than one clause.
    TextSpansLines(Clause),
    /// Rows disagree about which version of the imperative this is.
    MixedVersions {
        /// The version the first row declared.
        first: i64,
        /// The version a later row declared.
        second: i64,
    },
}

impl fmt::Display for ClauseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJsonLines(err) => write!(f, "not JSON Lines: {err}"),
            Self::NoRows => write!(f, "a clause table with no rows declares no imperative"),
            Self::MissingKey(key) => write!(f, "a row with no `{key}`"),
            Self::WrongType(key) => write!(f, "`{key}` is bound to the wrong kind of value"),
            Self::UnknownKey(key) => write!(
                f,
                "the key `{key}` is not in the schema, so nothing reads it"
            ),
            Self::UnknownClause(written) => {
                write!(f, "`{written}` is not a clause of the imperative")
            }
            Self::DuplicateClause(clause) => {
                write!(f, "two rows claim the clause `{}`", clause.tag())
            }
            Self::NoRowForClause(clause) => write!(
                f,
                "the clause `{}` has no row, so half the arms carry nothing",
                clause.tag()
            ),
            Self::BlankText(clause) => write!(
                f,
                "the clause `{}` is blank, which makes its arm the control \
                 wearing another name",
                clause.tag()
            ),
            Self::TextSpansLines(clause) => write!(
                f,
                "the clause `{}` spans lines, so it is more than one clause",
                clause.tag()
            ),
            Self::MixedVersions { first, second } => write!(
                f,
                "the table declares version {first} and version {second} at once"
            ),
        }
    }
}

impl Error for ClauseError {}

// ---------------------------------------------------------------------------
// the graders
// ---------------------------------------------------------------------------

/// What an answer amounts to, for the ablation's two endpoints.
///
/// One verdict with three values rather than two independent predicates.
/// Independent predicates are how an answer ends up graded as both silent and
/// engaged: two rules, drifting, each true on its own reading of the same
/// text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Grade {
    /// No content at all. The collapse case, and the reason silence is an
    /// endpoint rather than a footnote.
    Silent,
    /// Text, and none of it an answer: declines, placeholders, a form handed
    /// back with its blanks still in it.
    Inert,
    /// At least one field says something.
    Engaged,
}

impl Grade {
    /// Every grade, so a corpus can be summarised without a match that
    /// forgets one.
    pub const ALL: &'static [Self] = &[Self::Silent, Self::Inert, Self::Engaged];

    /// The name this grade is written under in a corpus expectation.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Silent => "silent",
            Self::Inert => "inert",
            Self::Engaged => "engaged",
        }
    }

    /// The grade `tag` names, if there is one.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|grade| grade.tag() == tag)
    }
}

/// Shapes a template's own blank comes back in when nothing filled it.
///
/// Prefix and suffix, matched against the whole trimmed field. A field that
/// merely CONTAINS a bracketed span is a field quoting something; a field
/// that IS one is a blank.
const PLACEHOLDER_SHAPES: &[(&str, &str)] = &[("<", ">"), ("[", "]"), ("{", "}"), ("(your", ")")];

/// Words that stand in for an answer nobody wrote.
const PLACEHOLDER_WORDS: &[&str] = &["tbd", "todo", "...", "\u{2026}"];

/// Whether a field's text is a blank rather than an answer.
fn is_placeholder(text: &str) -> bool {
    let trimmed = text.trim().to_lowercase();
    if PLACEHOLDER_WORDS.contains(&trimmed.as_str()) {
        return true;
    }
    PLACEHOLDER_SHAPES.iter().any(|(open, close)| {
        trimmed.len() > open.len() + close.len()
            && trimmed.starts_with(open)
            && trimmed.ends_with(close)
    })
}

/// What `answer` amounts to.
///
/// The one rule. [`engagement`] and [`silence`] both read it, so the two
/// endpoints cannot disagree about the same answer.
///
/// A field says something when its text is not blank, not a decline, and not
/// a placeholder. Whether it carried a tag does not enter into it: an answer
/// written as prose is still an answer, and grading it as silence would count
/// a formatting miss as a collapse. Echo of the ask is a fourth way to say
/// nothing and is not read here -- it needs the ask, which these graders are
/// not given, and it is the mimicry detector's endpoint rather than this
/// ablation's.
#[must_use]
pub fn grade(answer: &Answer) -> Grade {
    let mut any_text = false;
    let mut any_content = false;
    for field in &answer.fields {
        if field.raw.trim().is_empty() {
            continue;
        }
        any_text = true;
        if matches!(field.outcome, Outcome::Decline(_)) {
            continue;
        }
        if is_placeholder(&field.raw) {
            continue;
        }
        any_content = true;
    }
    if !any_text {
        return Grade::Silent;
    }
    if any_content {
        Grade::Engaged
    } else {
        Grade::Inert
    }
}

/// Whether the answer engaged: the original experiment's endpoint.
#[must_use]
pub fn engagement(answer: &Answer) -> bool {
    matches!(grade(answer), Grade::Engaged)
}

/// Whether the answer collapsed into silence: the endpoint the original did
/// not carry, and the one a clause can buy engagement with.
#[must_use]
pub fn silence(answer: &Answer) -> bool {
    matches!(grade(answer), Grade::Silent)
}

// ---------------------------------------------------------------------------
// exact rates, and the paired bootstrap
// ---------------------------------------------------------------------------

/// An exact rate: the counts it came from, never a float.
///
/// Division happens once, at the point of rendering, with the digit count
/// stated. A rate carried as a double is a rate that reads back as something
/// slightly else, and a p-value is exactly the kind of number that gets
/// compared against a threshold at its last digit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rate {
    numerator: u64,
    denominator: u64,
}

impl Rate {
    /// A rate of `numerator` over `denominator`, or `None` for a rate over
    /// nothing.
    #[must_use]
    pub fn new(numerator: u64, denominator: u64) -> Option<Self> {
        if denominator == 0 {
            return None;
        }
        Some(Self {
            numerator,
            denominator,
        })
    }

    /// How many the rate counted.
    #[must_use]
    pub fn numerator(self) -> u64 {
        self.numerator
    }

    /// Out of how many.
    #[must_use]
    pub fn denominator(self) -> u64 {
        self.denominator
    }

    /// The rate as a fixed-point decimal of `digits` digits, rounded half up.
    ///
    /// Rounded rather than truncated because the numbers this renders are
    /// small: a floor of 1/1001 truncated to three digits is `0.000`, which
    /// reads as a p-value of zero -- the exact overstatement reporting the
    /// floor exists to prevent.
    ///
    /// # Panics
    ///
    /// Panics if `digits` exceeds 38, where the scale no longer fits a
    /// `u128`. Callers pass a constant.
    #[must_use]
    pub fn fixed(self, digits: u32) -> String {
        let scale = 10u128.pow(digits);
        let denominator = u128::from(self.denominator);
        let scaled = (u128::from(self.numerator) * scale * 2 + denominator) / (denominator * 2);
        let whole = scaled / scale;
        if digits == 0 {
            return whole.to_string();
        }
        let width = digits as usize;
        format!("{whole}.{:0width$}", scaled % scale)
    }
}

/// How many digits a p-value and its floor are reported to.
pub const P_DIGITS: u32 = 4;

/// The smallest p a bootstrap of `resamples` resamples can produce.
///
/// One over the number of resamples plus one, because the observed statistic
/// is itself one of the draws. Every p this module reports carries it: a
/// sample size bounds the smallest p it can produce, and a p at that bound
/// means "as far as this run could see", not "vanishingly unlikely".
#[must_use]
pub fn attainable_p_floor(resamples: u32) -> Rate {
    Rate {
        numerator: 1,
        denominator: u64::from(resamples) + 1,
    }
}

/// A deterministic generator, so a resample is reproducible.
///
/// Xorshift, seeded by the caller. Nothing here reads the clock or the
/// operating system: a bootstrap whose resamples cannot be reproduced is a
/// number nobody can check.
struct Prng(u64);

impl Prng {
    fn next_u64(&mut self) -> u64 {
        let mut state = self.0;
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        self.0 = state;
        state
    }

    /// A draw below `bound`, rejecting the tail that modulo would skew.
    fn below(&mut self, bound: u64) -> u64 {
        let span = (u64::MAX / bound) * bound;
        loop {
            let draw = self.next_u64();
            if draw < span {
                return draw % bound;
            }
        }
    }
}

/// A paired bootstrap of `a` against `b`, and what it may be reported as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bootstrap {
    successes_a: u64,
    successes_b: u64,
    pairs: u64,
    resamples: u32,
    crossings: u64,
}

impl Bootstrap {
    /// How often the first arm's outcome held.
    #[must_use]
    pub fn rate_a(self) -> Rate {
        Rate {
            numerator: self.successes_a,
            denominator: self.pairs,
        }
    }

    /// How often the second arm's outcome held.
    #[must_use]
    pub fn rate_b(self) -> Rate {
        Rate {
            numerator: self.successes_b,
            denominator: self.pairs,
        }
    }

    /// The one-sided p for "the first arm is above the second".
    ///
    /// The observed statistic counts as one of the draws, which is why the
    /// numerator and the denominator both carry a plus one and why this can
    /// never be zero.
    #[must_use]
    pub fn p_value(self) -> Rate {
        Rate {
            numerator: self.crossings + 1,
            denominator: u64::from(self.resamples) + 1,
        }
    }

    /// The smallest p this run could have produced.
    #[must_use]
    pub fn attainable_p_floor(self) -> Rate {
        attainable_p_floor(self.resamples)
    }

    /// The line this result is reported as.
    ///
    /// The floor is in the same string as the p, not beside it in a caller's
    /// discretion. A p that travels without its floor gets quoted without it.
    #[must_use]
    pub fn report(&self) -> String {
        format!(
            "{}/{} against {}/{}: p {}, attainable floor {} over {} resamples",
            self.successes_a,
            self.pairs,
            self.successes_b,
            self.pairs,
            self.p_value().fixed(P_DIGITS),
            self.attainable_p_floor().fixed(P_DIGITS),
            self.resamples,
        )
    }
}

/// Why a paired bootstrap could not be run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootstrapError {
    /// The two arms were graded over different numbers of forks, so they are
    /// not paired and resampling them together would compare an arm against
    /// somebody else's forks.
    NotPaired {
        /// How many outcomes the first arm has.
        a: usize,
        /// How many the second has.
        b: usize,
    },
    /// No forks at all.
    NoPairs,
    /// No resamples, which would make the p a division by one and the floor
    /// the whole interval.
    NoResamples,
    /// A seed of zero, which this generator cannot leave: every draw would be
    /// the first fork, and the p would be an artefact of the seed.
    ZeroSeed,
    /// More forks than the counting here can hold.
    TooManyPairs(usize),
}

impl fmt::Display for BootstrapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotPaired { a, b } => write!(
                f,
                "{a} outcomes against {b}: a paired test needs the same forks \
                 on both sides"
            ),
            Self::NoPairs => write!(f, "a bootstrap over no forks"),
            Self::NoResamples => write!(f, "a bootstrap of no resamples"),
            Self::ZeroSeed => write!(
                f,
                "a seed of zero, which this generator never leaves: every \
                 resample would be the same fork"
            ),
            Self::TooManyPairs(count) => write!(f, "{count} forks is more than this can count"),
        }
    }
}

impl Error for BootstrapError {}

/// Resample `a` against `b` over the forks they share.
///
/// Paired: each resample draws a FORK, and takes both arms' outcomes for it.
/// Drawing the two arms independently would throw away the pairing the design
/// bought and widen every interval for nothing.
///
/// # Errors
///
/// Returns [`BootstrapError`] for arms of different lengths, no forks, no
/// resamples, a zero seed, or more forks than the counters hold.
pub fn paired_bootstrap(
    a: &[bool],
    b: &[bool],
    resamples: u32,
    seed: u64,
) -> Result<Bootstrap, BootstrapError> {
    if a.len() != b.len() {
        return Err(BootstrapError::NotPaired {
            a: a.len(),
            b: b.len(),
        });
    }
    if a.is_empty() {
        return Err(BootstrapError::NoPairs);
    }
    if resamples == 0 {
        return Err(BootstrapError::NoResamples);
    }
    if seed == 0 {
        return Err(BootstrapError::ZeroSeed);
    }
    let pairs = u64::try_from(a.len()).map_err(|_| BootstrapError::TooManyPairs(a.len()))?;
    let differences: Vec<i64> = a
        .iter()
        .zip(b)
        .map(|(left, right)| i64::from(*left) - i64::from(*right))
        .collect();
    let mut prng = Prng(seed);
    let mut crossings = 0;
    for _ in 0..resamples {
        let mut total: i64 = 0;
        for _ in 0..pairs {
            let index = prng.below(pairs);
            // `pairs` came from `a.len()`, so every draw below it indexes.
            total += differences[usize::try_from(index).unwrap_or(0)];
        }
        if total <= 0 {
            crossings += 1;
        }
    }
    Ok(Bootstrap {
        successes_a: count(a),
        successes_b: count(b),
        pairs,
        resamples,
        crossings,
    })
}

/// How many outcomes held.
fn count(outcomes: &[bool]) -> u64 {
    outcomes.iter().filter(|held| **held).count() as u64
}

// ---------------------------------------------------------------------------
// the pre-registration
// ---------------------------------------------------------------------------

/// An endpoint the ablation is pre-registered on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Endpoint {
    /// The original experiment's metric.
    Engagement,
    /// The collapse case. Declared up front because a wording that buys
    /// engagement with occasional silence is not a win, and that judgement
    /// cannot be made after seeing which way the numbers went.
    Silence,
}

impl Endpoint {
    /// Both endpoints. A plan that reported one of them would be the original
    /// experiment again.
    pub const ALL: &'static [Self] = &[Self::Engagement, Self::Silence];

    /// The name this endpoint is reported under.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Engagement => "engagement",
            Self::Silence => "silence",
        }
    }

    /// Which direction counts as better.
    #[must_use]
    pub fn preferred(self) -> Preferred {
        match self {
            Self::Engagement => Preferred::Higher,
            Self::Silence => Preferred::Lower,
        }
    }

    /// What the endpoint counts, in words.
    #[must_use]
    pub fn definition(self) -> &'static str {
        match self {
            Self::Engagement => {
                "a field of the answer that is not blank, a decline or a blank left in"
            }
            Self::Silence => "an answer carrying no text at all",
        }
    }
}

/// Which way an endpoint is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Preferred {
    /// More of it is better.
    Higher,
    /// Less of it is better.
    Lower,
}

impl Preferred {
    /// The name this direction is reported under.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Higher => "higher is better",
            Self::Lower => "lower is better",
        }
    }
}

/// The pre-registration, printable.
///
/// Printed before a run rather than written up after one. The endpoints, the
/// arms and the smallest p the design can produce are all decidable without
/// any data, and every one of them is a thing that gets chosen conveniently
/// once the data is in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    version: i64,
    arms: Vec<(Arm, String)>,
    resamples: u32,
}

impl Plan {
    /// The smallest p the planned run could produce.
    #[must_use]
    pub fn attainable_p_floor(&self) -> Rate {
        attainable_p_floor(self.resamples)
    }
}

impl fmt::Display for Plan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "imperative clause ablation, version {}", self.version)?;
        writeln!(f, "endpoints")?;
        for endpoint in Endpoint::ALL {
            writeln!(
                f,
                "  {}  ({})  {}",
                endpoint.tag(),
                endpoint.preferred().tag(),
                endpoint.definition()
            )?;
        }
        writeln!(f, "arms")?;
        for (arm, rendered) in &self.arms {
            writeln!(f, "  {}  {rendered}", arm.tag())?;
        }
        writeln!(
            f,
            "paired bootstrap over {} resamples; attainable p floor {}",
            self.resamples,
            self.attainable_p_floor().fixed(P_DIGITS)
        )?;
        write!(
            f,
            "no arm has been run: driving one needs a model, and there is none here"
        )
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};

    use super::{
        Ablation, Arm, Bootstrap, BootstrapError, CONTROL_TAG, Clause, ClauseError, Endpoint,
        Grade, P_DIGITS, Rate, arms, attainable_p_floor, engagement, grade, paired_bootstrap,
        silence,
    };
    use crate::formats::interview;

    fn shipped() -> Ablation {
        Ablation::shipped().expect("the shipped clause table reads")
    }

    fn answer(text: &str) -> interview::Answer {
        interview::parse(text)
            .unwrap_or_else(|err| panic!("{text:?} is an interview answer: {err}"))
    }

    fn corpus_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("capture")
            .join("ablation")
            .join("corpus")
    }

    /// Every `<case>.answer.txt` in the corpus, with the grade beside it.
    ///
    /// A directory walk that finds nothing passes every assertion over it, so
    /// the emptiness is the first thing asserted rather than the last thing
    /// noticed.
    fn corpus() -> Vec<(String, interview::Answer, Grade)> {
        const SUFFIX: &str = ".answer.txt";
        let dir = corpus_dir();
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
            .filter_map(Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter_map(|name| name.strip_suffix(SUFFIX).map(str::to_owned))
            .collect();
        names.sort();
        assert!(
            !names.is_empty(),
            "{}: no cases, so every assertion over this corpus would hold vacuously",
            dir.display()
        );
        let mut cases = Vec::new();
        for name in names {
            let source = std::fs::read_to_string(dir.join(format!("{name}{SUFFIX}")))
                .unwrap_or_else(|err| panic!("{name}{SUFFIX}: {err}"));
            let expectation = dir.join(format!("{name}.grade"));
            let written = std::fs::read_to_string(&expectation).unwrap_or_else(|err| {
                panic!(
                    "{}: every case is paired with its expected grade: {err}",
                    expectation.display()
                )
            });
            let grade = Grade::from_tag(written.trim()).unwrap_or_else(|| {
                panic!(
                    "{}: `{}` is not a grade",
                    expectation.display(),
                    written.trim()
                )
            });
            cases.push((name, answer(&source), grade));
        }
        cases
    }

    // Acceptance: the seeded control -- an arm with the sentence removed
    // entirely -- has to exist, or the ablation can rank clauses against each
    // other and cannot say any of them beats saying nothing.
    #[test]
    fn the_arms_include_the_control_with_no_imperative_at_all() {
        let all = arms();
        assert!(
            all.contains(&Arm::CONTROL),
            "the control arm is missing: an ablation with no sentence-removed \
             arm cannot say that any clause beats saying nothing"
        );
        assert_eq!(
            all.len(),
            1usize << Clause::ALL.len(),
            "the arms are not the whole power set of the clauses"
        );
        assert_eq!(shipped().render(Arm::CONTROL), "");
        assert_eq!(Arm::CONTROL.tag(), CONTROL_TAG);
    }

    #[test]
    fn every_arm_renders_a_different_imperative() {
        let table = shipped();
        let mut seen: BTreeMap<String, Arm> = BTreeMap::new();
        for arm in arms() {
            let rendered = table.render(arm);
            if let Some(previous) = seen.insert(rendered.clone(), arm) {
                panic!(
                    "two arms render the same imperative: `{}` and `{}` both \
                     give {rendered:?}",
                    previous.tag(),
                    arm.tag()
                );
            }
        }
        assert_eq!(seen.len(), arms().len());
    }

    #[test]
    fn every_clause_has_a_row_in_the_shipped_table() {
        let table = shipped();
        for clause in Clause::ALL.iter().copied() {
            assert!(
                !table.text(clause).is_empty(),
                "the clause `{}` has no text, so half the arms are the control",
                clause.tag()
            );
        }
        assert_eq!(table.version(), 1);
    }

    #[test]
    fn an_arm_renders_its_clauses_in_one_order_however_they_were_named() {
        let table = shipped();
        let rendered = table.render(Arm::of(&[Clause::Brevity, Clause::Scope]));
        assert_eq!(
            rendered,
            table.render(Arm::of(&[Clause::Scope, Clause::Brevity]))
        );
        // The order is the vocabulary's, not the caller's. Two arms that
        // differ only in the order somebody listed their clauses are one arm,
        // and rendering them differently makes the ablation compare a
        // sentence against itself.
        assert!(
            rendered.starts_with(table.text(Clause::Scope)),
            "an arm rendered its clauses out of vocabulary order: {rendered:?}"
        );
        assert!(
            rendered.ends_with(table.text(Clause::Brevity)),
            "an arm rendered its clauses out of vocabulary order: {rendered:?}"
        );
    }

    #[test]
    fn a_clause_table_missing_a_clause_is_refused() {
        let partial = "{\"version\":1,\"clause\":\"scope\",\"text\":\"Answer from this turn.\"}\n";
        assert_eq!(
            Ablation::load(partial),
            Err(ClauseError::NoRowForClause(Clause::Exclusion))
        );
    }

    #[test]
    fn a_clause_table_with_a_key_nothing_reads_is_refused() {
        let extra = "{\"version\":1,\"clause\":\"scope\",\"text\":\"x\",\"weight\":2}\n";
        assert_eq!(
            Ablation::load(extra),
            Err(ClauseError::UnknownKey("weight".to_owned()))
        );
    }

    // The clause table goes through the record's value space and no other
    // reader, so what a record refuses it refuses: a `null`, an exponent, a
    // line that is not an object at all.
    #[test]
    fn a_clause_table_outside_the_records_value_space_is_refused() {
        for source in [
            "clause: scope\n",
            "{\"version\":1,\"clause\":\"scope\",\"text\":null}\n",
            "{\"version\":1e0,\"clause\":\"scope\",\"text\":\"x\"}\n",
        ] {
            assert!(
                matches!(Ablation::load(source), Err(ClauseError::NotJsonLines(_))),
                "{source:?} was read as a clause table"
            );
        }
    }

    #[test]
    fn a_clause_table_that_disagrees_about_its_version_is_refused() {
        let mixed = "{\"version\":1,\"clause\":\"scope\",\"text\":\"x\"}\n\
                     {\"version\":2,\"clause\":\"exclusion\",\"text\":\"y\"}\n";
        assert_eq!(
            Ablation::load(mixed),
            Err(ClauseError::MixedVersions {
                first: 1,
                second: 2
            })
        );
    }

    // Acceptance: a fixture set of answers grades as the corpus says.
    #[test]
    fn the_corpus_grades_every_answer_as_labelled() {
        let mut counted: BTreeMap<Grade, usize> = BTreeMap::new();
        for (name, parsed, expected) in corpus() {
            let got = grade(&parsed);
            assert_eq!(
                got,
                expected,
                "{name}: graded `{}` where the corpus says `{}`",
                got.tag(),
                expected.tag()
            );
            *counted.entry(expected).or_default() += 1;
        }
        for grade in Grade::ALL {
            assert!(
                counted.contains_key(grade),
                "no corpus case grades `{}`, so nothing pins it",
                grade.tag()
            );
        }
    }

    // Acceptance: the two endpoints are exclusive. A clause that buys
    // engagement with occasional silence is only visible if an answer cannot
    // be counted as both.
    #[test]
    fn silence_and_engagement_are_never_both_true() {
        let cases = corpus();
        assert!(
            cases.iter().any(|(_, _, grade)| *grade == Grade::Silent),
            "no corpus case is silent, so this assertion would hold vacuously"
        );
        for (name, parsed, _) in cases {
            assert!(
                !(engagement(&parsed) && silence(&parsed)),
                "{name}: counted as engagement and as silence at once, so a \
                 collapse would be reported as a win"
            );
        }
    }

    #[test]
    fn a_decline_is_not_engagement_and_is_not_silence() {
        let declined = answer("DECISION: NONE (not building yet)\n");
        assert_eq!(grade(&declined), Grade::Inert);
        assert!(!engagement(&declined));
        assert!(!silence(&declined));
    }

    // Acceptance: every p ships with its attainable floor. A sample size
    // bounds the smallest p it can produce, and a p quoted without that bound
    // reads as a strength it does not have.
    #[test]
    fn every_reported_p_carries_its_attainable_floor() {
        let a = [true, true, true, false, true, true];
        let b = [false, true, false, false, true, false];
        let result = paired_bootstrap(&a, &b, 99, 7).expect("a well-formed bootstrap");
        let floor = result.attainable_p_floor().fixed(P_DIGITS);
        assert_eq!(floor, "0.0100");
        assert_ne!(
            result.p_value().fixed(P_DIGITS),
            floor,
            "the p and the floor coincide here, so the assertion below could \
             not tell them apart"
        );
        assert!(
            result.report().contains(&floor),
            "a p reported without its attainable floor: {}",
            result.report()
        );
        assert!(result.report().contains(&result.p_value().fixed(P_DIGITS)));
        let plan = shipped().plan(99);
        let rendered = plan.to_string();
        assert!(
            rendered.contains(&floor),
            "the plan states no attainable p floor: {rendered}"
        );
    }

    #[test]
    fn the_plan_states_both_endpoints_and_every_arm() {
        let plan = shipped().plan(999);
        let rendered = plan.to_string();
        for endpoint in Endpoint::ALL {
            assert!(
                rendered.contains(endpoint.tag()),
                "the plan omits the endpoint `{}`",
                endpoint.tag()
            );
            assert!(rendered.contains(endpoint.preferred().tag()));
        }
        for arm in arms() {
            assert!(
                rendered.contains(&arm.tag()),
                "the plan omits the arm `{}`",
                arm.tag()
            );
        }
        assert_eq!(plan.attainable_p_floor().fixed(P_DIGITS), "0.0010");
    }

    #[test]
    fn the_same_seed_resamples_the_same_way() {
        let a = [true, false, true, true];
        let b = [false, false, true, false];
        let once = paired_bootstrap(&a, &b, 50, 11).expect("a well-formed bootstrap");
        let again = paired_bootstrap(&a, &b, 50, 11).expect("a well-formed bootstrap");
        assert_eq!(once, again);
        let other: Bootstrap = paired_bootstrap(&a, &b, 50, 12).expect("a well-formed bootstrap");
        assert_eq!(other.rate_a(), once.rate_a());
    }

    #[test]
    fn a_bootstrap_refuses_arms_that_are_not_paired() {
        let a = [true, false];
        let b = [true];
        assert_eq!(
            paired_bootstrap(&a, &b, 10, 1),
            Err(BootstrapError::NotPaired { a: 2, b: 1 })
        );
        assert_eq!(
            paired_bootstrap(&a, &a, 10, 0),
            Err(BootstrapError::ZeroSeed)
        );
        assert_eq!(
            paired_bootstrap(&a, &a, 0, 1),
            Err(BootstrapError::NoResamples)
        );
        assert_eq!(
            paired_bootstrap(&[], &[], 10, 1),
            Err(BootstrapError::NoPairs)
        );
    }

    #[test]
    fn an_arm_that_won_every_fork_reaches_the_attainable_floor() {
        let a = [true; 12];
        let b = [false; 12];
        let result = paired_bootstrap(&a, &b, 199, 5).expect("a well-formed bootstrap");
        assert_eq!(
            result.p_value(),
            result.attainable_p_floor(),
            "an arm that beat the other on every fork did not reach the \
             smallest p this design can produce, so the resamples are not \
             reading the outcomes"
        );
    }

    #[test]
    fn two_arms_that_graded_the_same_are_not_a_difference() {
        let same = [true, false, true, false, true];
        let result = paired_bootstrap(&same, &same, 199, 3).expect("a well-formed bootstrap");
        assert_eq!(result.p_value(), Rate::new(200, 200).expect("a rate"));
    }

    #[test]
    fn a_rate_renders_the_digits_it_was_asked_for_and_rounds_up() {
        assert_eq!(attainable_p_floor(1000).fixed(3), "0.001");
        assert_eq!(Rate::new(1, 3).expect("a rate").fixed(4), "0.3333");
        assert_eq!(Rate::new(2, 3).expect("a rate").fixed(4), "0.6667");
        assert_eq!(Rate::new(1, 2).expect("a rate").fixed(0), "1");
        assert!(Rate::new(1, 0).is_none());
    }
}
