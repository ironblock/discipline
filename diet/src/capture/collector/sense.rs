//! Tier 1: nomination by sense.
//!
//! Tier 0 nominates on a fact -- this anchor recurred at this offset -- and
//! it is silent whenever a turn reverses an entry without naming any of its
//! anchors, which is most of the time a person would call it a reversal.
//! This tier is the measurement that covers that case, and a measurement is
//! a different kind of evidence: it has a threshold, the threshold has a
//! number, and the number is only worth anything if something measured it.
//!
//! So the policy is data and the data must say where its numbers came from.
//! [`Policy::load`] refuses a policy whose `calibrated_on` does not name a
//! results directory, and refuses the shipped fixture policy by name. An
//! uncalibrated policy is not a conservative policy, it is an unknown one:
//! nobody can say what it fires on, and a nomination tier nobody can
//! characterise spends confirm forks at a rate nobody predicted.
//!
//! Two registers, because a reversal has two halves and neither alone is a
//! nomination:
//!
//! * [`Register::Reversal`] asks of the turn, not of any entry: is this the
//!   kind of prose that retires a fact? It is scored against the authored
//!   `reversal` sense set through [`crate::capture::sense`], with the
//!   policy's scoring and lexical gate. A turn that does not clear the
//!   threshold nominates nothing at all, and that silence is the tier's main
//!   product -- most turns reverse nothing.
//! * [`Register::Intent`] asks of each live entry: is this entry about what
//!   the turn is now about? It is the turn's stated intent against the
//!   entry's content, measured as plain cosine, because both sides are
//!   transcript prose rather than an authored description and there is no
//!   authored opposite to contrast against. The entry side is its whole
//!   content and not a stated-intent field of its own: an entry in the
//!   working object is a fact and a provenance, with no field structure to
//!   narrow to, and a register that claimed to read a field the object does
//!   not carry would be tuned against something nobody could point at.
//!
//! Only [`Register::Intent`] reaches a nomination. The reversal register is
//! measured of the turn, once, and what it decides is whether there are any
//! nominations at all -- so no nomination is evidence *from* it, and the
//! vocabulary keeps both because a reader of either number has to be able to
//! say which one it is.
//!
//! An entry is nominated when the turn clears the first and the entry clears
//! the second, and at most `budget` entries are nominated per turn, taken by
//! score. The budget is not a performance knob: it is the number of confirm
//! forks this tier is allowed to spend on one turn, and a tier without one
//! nominates a whole object the first time a threshold is set too low.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use crate::capture::sense::{
    self, DataError, EmbeddedSet, Embedder, Gate, Scoring, Sense, SenseSet, SetError, cosine,
};
use crate::formats::record::json::{Decimal, Value};
use crate::object::WorkingObject;

use super::{Evidence, Nomination};

/// The policy shipped for tests, and refused by [`Policy::load`].
pub const FIXTURE: &str = include_str!("../../../capture/collector/policy.fixture.jsonl");

/// Which register a nomination was measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Register {
    /// The turn's prose against the authored `reversal` senses.
    Reversal,
    /// An entry's stated intent against the turn's stated intent.
    Intent,
}

impl Register {
    /// Both registers, so a report cannot name one and imply the set.
    pub const ALL: &'static [Self] = &[Self::Reversal, Self::Intent];

    /// The spelling a record uses.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Reversal => "reversal",
            Self::Intent => "intent",
        }
    }

    /// The register a tag names.
    #[must_use]
    pub fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.tag() == tag)
    }
}

/// Where a policy's numbers came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Calibration {
    /// The results directory of the run that set this threshold.
    Run(String),
    /// The fixture policy: authored so this tier has something to be tested
    /// against, and refused by [`Policy::load`] so it cannot be the one that
    /// ships. Its threshold was chosen to make the fixtures readable, which
    /// is exactly the property a shipped threshold must not have.
    Fixture,
}

impl Calibration {
    /// The spelling the data file uses. A run is named by its directory.
    #[must_use]
    pub fn tag(&self) -> &str {
        match self {
            Self::Run(directory) => directory,
            Self::Fixture => FIXTURE_TAG,
        }
    }
}

/// The one spelling that means "not calibrated against a run".
const FIXTURE_TAG: &str = "fixture";

/// When this tier nominates, and how much it may spend doing it.
///
/// The fields are private and [`Policy::load`] is the only way to make one,
/// because the calibration rule is a property of every policy in hand and
/// not of one function call. A struct whose fields are public is a door with
/// an open wall beside it: a caller assembles the numbers it likes, the
/// refusal never runs, and the tier fires on a threshold nobody measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// How a sense set is scored against a text.
    scoring: Scoring,
    /// Whether the lexical pre-gate is applied.
    gate: Gate,
    /// The score at or above which a register fires. A decimal, not a float:
    /// a threshold that reaches a record is a number the record reads back.
    threshold: Decimal,
    /// The most entries this tier may nominate in one turn.
    budget: u32,
    /// Where the threshold came from.
    calibrated_on: Calibration,
}

/// Why a policy could not be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// The file is not a policy row.
    Data(DataError),
    /// More than one row, or none. A policy is one row: two would leave the
    /// reader to decide which threshold is in force.
    NotOneRow(usize),
    /// `calibrated_on` names no run. The threshold is a number nobody
    /// measured, and this tier does not ship one.
    Uncalibrated,
    /// `calibrated_on` is the fixture. It exists for the tests and is not a
    /// calibration.
    Fixture,
    /// `threshold` is not a decimal the record would read back.
    Threshold(String),
    /// `budget` is zero: a tier that may nominate nothing is off, and a
    /// policy is not how you turn a tier off.
    EmptyBudget,
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Data(error) => write!(f, "not a policy: {error}"),
            Self::NotOneRow(rows) => write!(f, "a policy is one row, and this file has {rows}"),
            Self::Uncalibrated => write!(
                f,
                "this policy names no run it was calibrated on, so its threshold is a number \
                 nobody measured"
            ),
            Self::Fixture => write!(
                f,
                "this is the fixture policy, which is authored for the tests and calibrated on \
                 nothing"
            ),
            Self::Threshold(text) => write!(f, "the threshold {text} is not a decimal"),
            Self::EmptyBudget => write!(f, "a budget of zero is a tier switched off, not a policy"),
        }
    }
}

impl Error for PolicyError {}

impl From<DataError> for PolicyError {
    fn from(error: DataError) -> Self {
        Self::Data(error)
    }
}

/// Where a calibration run writes what it measured.
const RESULTS: &str = "results/";

/// Whether `name` names a run's results directory.
///
/// `results/` names its runs `YYYY-MM-DD-<slug>`, and that is the whole
/// check: a reader handed this policy has to be able to go and open the run
/// that set the threshold. What it refuses is what costs something -- a bare
/// word, the template, a date with no run behind it, the fixture's own tag
/// with a space on the end.
///
/// The shape is checked and the filesystem is not. This crate reads no
/// directory, and a policy naming a run that has not been committed yet is a
/// different failure from a policy naming no run at all -- the first is a
/// missing file, which whoever opens it will discover; the second is a
/// number nobody measured, which nobody discovers.
fn names_a_run(name: &str) -> bool {
    let Some(run) = name.strip_prefix(RESULTS) else {
        return false;
    };
    let mut parts = run.splitn(4, '-');
    let (Some(year), Some(month), Some(day), Some(slug)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return false;
    };
    let digits = |text: &str, count: usize| {
        text.len() == count && text.chars().all(|character| character.is_ascii_digit())
    };
    digits(year, 4)
        && digits(month, 2)
        && digits(day, 2)
        && !slug.is_empty()
        && slug.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
}

impl Policy {
    /// Read a policy for use.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Uncalibrated`] for a policy whose
    /// `calibrated_on` does not name a results directory,
    /// [`PolicyError::Fixture`] for the fixture policy, and
    /// [`PolicyError::Data`] for a file that is not one policy row.
    pub fn load(source: &str) -> Result<Self, PolicyError> {
        let policy = Self::read(source)?;
        match &policy.calibrated_on {
            Calibration::Fixture => Err(PolicyError::Fixture),
            Calibration::Run(directory) if !names_a_run(directory) => {
                Err(PolicyError::Uncalibrated)
            }
            Calibration::Run(_) => Ok(policy),
        }
    }

    /// The shipped fixture policy, for tests and for nothing else -- which
    /// is why it compiles into nothing else. Every other way to hold a
    /// [`Policy`] goes through [`Policy::load`], so the calibration rule is
    /// a property of the type rather than of a caller's discipline.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Uncalibrated`] if the shipped fixture ever
    /// stops declaring itself a fixture -- which would make it a policy
    /// claiming a calibration it does not have.
    #[cfg(test)]
    pub fn fixture() -> Result<Self, PolicyError> {
        let policy = Self::read(FIXTURE)?;
        match policy.calibrated_on {
            Calibration::Fixture => Ok(policy),
            Calibration::Run(_) => Err(PolicyError::Uncalibrated),
        }
    }

    /// The row, whatever it was calibrated on.
    fn read(source: &str) -> Result<Self, PolicyError> {
        let mut rows = sense::rows(source)?;
        if rows.len() != 1 {
            return Err(PolicyError::NotOneRow(rows.len()));
        }
        let (line, mut members) = rows.remove(0);
        let scoring = sense::take_tag(&mut members, line, "scoring", Scoring::from_tag)?;
        let gate = sense::take_tag(&mut members, line, "gate", Gate::from_tag)?;
        let written = sense::take_text(&mut members, line, "threshold")?;
        let threshold = Decimal::new(&written).ok_or(PolicyError::Threshold(written))?;
        let budget = match members.remove("budget") {
            Some(Value::Integer(number)) => u32::try_from(number).map_err(|_| {
                PolicyError::Data(DataError::WrongType {
                    line,
                    key: "budget",
                })
            })?,
            Some(_) => {
                return Err(PolicyError::Data(DataError::WrongType {
                    line,
                    key: "budget",
                }));
            }
            None => {
                return Err(PolicyError::Data(DataError::MissingKey {
                    line,
                    key: "budget",
                }));
            }
        };
        if budget == 0 {
            return Err(PolicyError::EmptyBudget);
        }
        let calibrated = sense::take_text(&mut members, line, "calibrated_on")?;
        sense::closed(&members, line)?;
        Ok(Self {
            scoring,
            gate,
            threshold,
            budget,
            calibrated_on: if calibrated == FIXTURE_TAG {
                Calibration::Fixture
            } else {
                Calibration::Run(calibrated)
            },
        })
    }

    /// The threshold as a number to compare against.
    ///
    /// The decimal is the written form -- what a record reads back -- and
    /// this is the same value as an `f64` for the one purpose of comparing a
    /// score to it.
    #[must_use]
    fn cut(&self) -> f64 {
        self.threshold.as_str().parse().unwrap_or(f64::INFINITY)
    }
}

/// What a turn brought that this tier measures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SenseText<'a> {
    /// The turn the text belongs to.
    pub turn: u32,
    /// The model's prose for the turn.
    pub prose: &'a str,
    /// What the turn said it was doing.
    pub intent: &'a str,
}

/// Why this tier could not measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SenseError {
    /// The `reversal` set could not be embedded.
    Set(SetError),
    /// The scoring produced no number for the turn's prose, which means the
    /// embedder could not place it. A text the embedder cannot place is not
    /// a text this tier scores low: it is one it does not score.
    Unscorable,
}

impl fmt::Display for SenseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Set(error) => write!(f, "the reversal set: {error}"),
            Self::Unscorable => write!(f, "the embedder could not place the turn's prose"),
        }
    }
}

impl Error for SenseError {}

/// Nominate live entries whose sense the turn has moved against.
///
/// The turn is scored first, against the authored `reversal` set. A turn
/// below the threshold nominates nothing -- and returns an empty vector
/// rather than an error, because "this turn reverses nothing" is the ordinary
/// case and not a failure.
///
/// # Errors
///
/// Returns [`SenseError::Set`] if the `reversal` senses cannot be embedded,
/// and [`SenseError::Unscorable`] if the embedder cannot place the turn's
/// prose. Neither is a low score: a measurement that did not happen must not
/// be reported as one that came out negative.
pub fn nominate_sense(
    object: &WorkingObject,
    new: SenseText<'_>,
    senses: &[Sense],
    embedder: &dyn Embedder,
    policy: &Policy,
) -> Result<Vec<Nomination>, SenseError> {
    let set = EmbeddedSet::embed(senses, SenseSet::Reversal, embedder).map_err(SenseError::Set)?;
    if !policy.gate.admits(SenseSet::Reversal, new.prose) {
        return Ok(Vec::new());
    }
    let prose = embedder.embed(new.prose);
    let turn_score = policy
        .scoring
        .score(&prose, &set)
        .ok_or(SenseError::Unscorable)?;
    if turn_score < policy.cut() {
        return Ok(Vec::new());
    }

    // The turn is reversal-shaped. Which entry it is about is a second
    // question, and the answer is the one the turn's stated intent is
    // nearest to.
    let intent = embedder.embed(new.intent);
    let mut scored: Vec<(f64, &crate::object::Entry)> = Vec::new();
    for entry in object.live() {
        if entry.provenances.iter().any(|p| p.turn >= new.turn) {
            continue;
        }
        let Some(score) = cosine(&intent, &embedder.embed(&entry.content)) else {
            continue;
        };
        if score >= policy.cut() {
            scored.push((score, entry));
        }
    }
    // Highest first, and by id where two entries score alike, so the budget
    // does not spend itself on whichever the object happened to hold first.
    scored.sort_by(|(left, a), (right, b)| {
        right
            .partial_cmp(left)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.id.cmp(&b.id))
    });
    scored.truncate(policy.budget as usize);
    Ok(scored
        .into_iter()
        .filter_map(|(score, entry)| {
            Some(Nomination {
                entry: entry.id.clone(),
                evidence: Evidence::Sense {
                    register: Register::Intent,
                    score: written(score)?,
                },
            })
        })
        .collect())
}

/// A score as a decimal of three places, rounded half away from zero.
///
/// Three places because that is what the bakeoff reports and a nomination
/// that cannot be compared to the run that calibrated it is not evidence.
/// `None` for a score that is not finite, which the caller drops rather than
/// banking as a number.
fn written(score: f64) -> Option<Decimal> {
    if !score.is_finite() {
        return None;
    }
    let scaled = (score * 1000.0).round();
    let sign = if scaled < 0.0 { "-" } else { "" };
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a cosine is bounded, so the scaled value fits and its sign is taken above"
    )]
    let magnitude = scaled.abs() as u64;
    Decimal::new(&format!(
        "{sign}{}.{:03}",
        magnitude / 1000,
        magnitude % 1000
    ))
}

/// This tier's own report, for a record.
#[must_use]
pub fn value(policy: &Policy, nominations: &[Nomination]) -> Value {
    Value::Object(BTreeMap::from([
        (
            "scoring".to_owned(),
            Value::String(policy.scoring.tag().to_owned()),
        ),
        (
            "gate".to_owned(),
            Value::String(policy.gate.tag().to_owned()),
        ),
        (
            "threshold".to_owned(),
            Value::Decimal(policy.threshold.clone()),
        ),
        (
            "calibrated_on".to_owned(),
            Value::String(policy.calibrated_on.tag().to_owned()),
        ),
        (
            "nominated".to_owned(),
            Value::Integer(i64::try_from(nominations.len()).unwrap_or(i64::MAX)),
        ),
    ]))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{
        Calibration, FIXTURE, Policy, PolicyError, Register, SenseText, nominate_sense, value,
        written,
    };
    use crate::capture::collector::{Evidence, Nomination};
    use crate::capture::sense::{Fixture, Gate, Scoring, shipped_senses};
    use crate::formats::record::json::Value;
    use crate::formats::record::{Reasoning, Regime, Substrate};
    use crate::object::{EntryId, Patch, Provenance, WorkingObject};

    fn regime() -> Regime {
        Regime {
            arm: "baseline".to_owned(),
            dogma_version: 0,
            substrate: Substrate {
                name: "local".to_owned(),
                model: "a-model".to_owned(),
                quantization: "q4".to_owned(),
                sampler: BTreeMap::new(),
                reasoning: Reasoning::On,
                hardware: "one-gpu".to_owned(),
            },
        }
    }

    fn object_with(entries: &[(&str, &str)]) -> WorkingObject {
        let mut object = WorkingObject::open(regime());
        for (id, content) in entries {
            object
                .apply(&Patch::Add {
                    id: EntryId::new(id).expect("an id"),
                    content: (*content).to_owned(),
                    provenance: Provenance {
                        turn: 5,
                        lane: "interview".to_owned(),
                        fork: None,
                        index: 0,
                    },
                })
                .expect("added");
        }
        object
    }

    // The whole point of the type. A threshold nobody measured is not a
    // cautious threshold, it is an unknown one, and this tier does not ship
    // one -- so the shipped fixture, which exists only for the tests below,
    // must be refused by the door production uses.
    #[test]
    fn the_shipped_fixture_policy_is_refused_by_the_door_that_ships() {
        assert_eq!(
            Policy::load(FIXTURE),
            Err(PolicyError::Fixture),
            "the fixture policy was accepted by the door that ships"
        );
        let fixture = Policy::fixture().expect("the fixture reads");
        assert_eq!(fixture.calibrated_on, Calibration::Fixture);
        assert_eq!(fixture.budget, 3);
    }

    #[test]
    fn a_policy_that_names_no_run_is_refused_and_one_that_names_a_run_is_not() {
        let row = |calibrated: &str| {
            format!(
                r#"{{"scoring":"contrastive","gate":"with_gate","threshold":"0.500","budget":3,"calibrated_on":"{calibrated}"}}"#
            )
        };
        let loaded = Policy::load(&row("results/2026-01-30-nomination")).expect("a policy");
        assert_eq!(
            loaded.calibrated_on,
            Calibration::Run("results/2026-01-30-nomination".to_owned())
        );
        assert_eq!(loaded.scoring, Scoring::Contrastive);
        assert_eq!(loaded.gate, Gate::With);
        assert_eq!(loaded.threshold.as_str(), "0.500");

        // A row with the key missing entirely, rather than set to the
        // fixture: the reader must not read an absent calibration as one.
        let missing =
            r#"{"scoring":"contrastive","gate":"with_gate","threshold":"0.500","budget":3}"#;
        assert!(
            matches!(Policy::load(missing), Err(PolicyError::Data(_))),
            "a policy with no calibrated_on key was read"
        );

        // Naming a run is not spelling anything at all. Each of these is a
        // threshold nobody could go and read the run for: a bare word, the
        // template nobody ran, a date with no run behind it, a directory
        // outside `results/`, and the fixture's own tag with a space on the
        // end -- one keystroke from the file this repository ships.
        for named in [
            "nowhere",
            "x",
            "results",
            "results/",
            "results/_template",
            "results/2026-01-30",
            "runs/2026-01-30-nomination",
            "fixture ",
            "Fixture",
        ] {
            assert_eq!(
                Policy::load(&row(named)),
                Err(PolicyError::Uncalibrated),
                "{named:?} was read as a calibration"
            );
        }
    }

    #[test]
    fn a_policy_the_record_could_not_read_back_is_refused() {
        let bad = [
            // A threshold that is not a decimal at all. A float spelled as
            // an integer, or as a second spelling of zero, is a number that
            // reads back differently from the one that was written.
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"1","budget":3,"calibrated_on":"results/2026-01-30-nomination"}"#,
                "an integer threshold",
            ),
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"5e-1","budget":3,"calibrated_on":"results/2026-01-30-nomination"}"#,
                "an exponent threshold",
            ),
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"-0.0","budget":3,"calibrated_on":"results/2026-01-30-nomination"}"#,
                "a negative zero threshold",
            ),
            // A budget of zero is a tier switched off, and a policy is not
            // the switch.
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"0.500","budget":0,"calibrated_on":"results/2026-01-30-nomination"}"#,
                "a budget of zero",
            ),
            // A scoring nobody implemented.
            (
                r#"{"scoring":"vibes","gate":"with_gate","threshold":"0.500","budget":3,"calibrated_on":"results/2026-01-30-nomination"}"#,
                "an unknown scoring",
            ),
            // A key the schema does not have: a policy that carries one has
            // a setting the reader is ignoring.
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"0.500","budget":3,"calibrated_on":"results/2026-01-30-nomination","tweak":1}"#,
                "an unknown key",
            ),
        ];
        for (row, what) in bad {
            assert!(Policy::load(row).is_err(), "{what} was accepted");
        }
        assert!(
            Policy::load("").is_err(),
            "an empty file was read as a policy"
        );
        let two = format!("{FIXTURE}{FIXTURE}");
        assert!(
            matches!(Policy::load(&two), Err(PolicyError::NotOneRow(2))),
            "two policies were read as one"
        );
    }

    // "0.5" is not a spelling the record reads back; three places is.
    #[test]
    fn a_score_is_written_as_a_decimal_the_record_reads_back() {
        for (score, spelled) in [
            (0.0, "0.000"),
            (1.0, "1.000"),
            (0.407_4, "0.407"),
            (0.407_6, "0.408"),
            (-0.25, "-0.250"),
        ] {
            assert_eq!(
                written(score).map(|number| number.as_str().to_owned()),
                Some(spelled.to_owned()),
                "{score}"
            );
        }
        // A score just below zero rounds to zero, and zero has one
        // spelling. `-0.000` is the other one, and the record refuses it, so
        // a tier that wrote it would bank a score no reader accepts.
        assert_eq!(
            written(-0.000_1).map(|number| number.as_str().to_owned()),
            Some("0.000".to_owned()),
            "a score rounding to zero from below was not written as zero"
        );
        assert_eq!(written(f64::NAN), None);
        assert_eq!(written(f64::INFINITY), None);
    }

    /// A policy calibrated on a run, so the tier under test is the tier that
    /// would ship. The threshold is the fixture's, and the embedder is the
    /// deterministic one: this measures the wiring, not a model.
    fn shipped(threshold: &str, budget: u32, gate: Gate) -> Policy {
        Policy::load(&format!(
            r#"{{"scoring":"raw_cosine","gate":"{}","threshold":"{threshold}","budget":{budget},"calibrated_on":"results/2026-01-30-nomination"}}"#,
            gate.tag()
        ))
        .expect("a policy")
    }

    /// The ids a call nominated, in the order it nominated them.
    fn ids(nominations: &[Nomination]) -> Vec<&str> {
        nominations.iter().map(|n| n.entry.as_str()).collect()
    }

    /// The scores those nominations carry, as the record would read them.
    fn scores(nominations: &[Nomination]) -> Vec<String> {
        nominations
            .iter()
            .map(|n| match &n.evidence {
                Evidence::Sense { score, .. } => score.as_str().to_owned(),
                other @ Evidence::Literal { .. } => panic!("tier 1 produced {other:?}"),
            })
            .collect()
    }

    // A turn that reverses nothing nominates nothing, and that is the tier's
    // ordinary answer rather than a failure. The entry here would clear the
    // second cut outright -- the turn's stated intent is its content, word
    // for word -- so what makes the answer empty is the turn's own score and
    // nothing else. A threshold that only holds because a second threshold
    // holds beside it is a threshold nobody has measured.
    #[test]
    fn a_turn_that_is_not_a_reversal_nominates_nothing() {
        let object = object_with(&[("e1", "the parser drops continuation lines")]);
        let senses = shipped_senses().expect("the shipped senses");
        let nominations = nominate_sense(
            &object,
            SenseText {
                turn: 18,
                prose: "I read the file and it is as described.",
                intent: "the parser drops continuation lines",
            },
            &senses,
            &Fixture,
            &shipped("0.500", 3, Gate::Without),
        )
        .expect("scored");
        assert!(
            nominations.is_empty(),
            "an unremarkable turn nominated: {nominations:?}"
        );
    }

    // The other cut, on its own. The turn is reversal-shaped and clears the
    // first threshold, and what the second one decides is which entries the
    // turn is about: one of these two is the turn's stated intent and the
    // other is a fact about the build. A tier that nominated both would hand
    // the confirm fork a bill for every live entry every time a turn
    // reversed anything.
    #[test]
    fn an_entry_the_turn_is_not_about_is_not_nominated() {
        let object = object_with(&[
            ("e1", "the parser drops continuation lines"),
            ("e2", "the build takes four minutes on this box"),
        ]);
        let senses = shipped_senses().expect("the shipped senses");
        let nominations = nominate_sense(
            &object,
            SenseText {
                turn: 18,
                prose: "the recorded limitation no longer applies",
                intent: "the parser drops continuation lines",
            },
            &senses,
            &Fixture,
            &shipped("0.500", 9, Gate::Without),
        )
        .expect("scored");
        assert_eq!(
            ids(&nominations),
            vec!["e1"],
            "the tier nominated an entry the turn was not about"
        );
    }

    // Which authored set the turn is scored against is the tier's premise.
    // Scored against `mistake`, this tier would nominate on the turn where
    // the operator got something wrong -- a real event class, and not the
    // one an entry is retired by. The two turns below are each shaped like
    // one set and not the other, and only one of them is this tier's.
    #[test]
    fn the_turn_is_scored_against_the_authored_reversal_senses() {
        let object = object_with(&[("e1", "the parser drops continuation lines")]);
        let senses = shipped_senses().expect("the shipped senses");
        let ask = |prose: &str| {
            nominate_sense(
                &object,
                SenseText {
                    turn: 18,
                    prose,
                    intent: "the parser drops continuation lines",
                },
                &senses,
                &Fixture,
                &shipped("0.500", 9, Gate::Without),
            )
            .expect("scored")
        };
        assert!(
            ask("the operator assumed something that was not true").is_empty(),
            "a turn about a mistaken assumption was scored as a reversal"
        );
        assert_eq!(
            ids(&ask("the recorded limitation no longer applies")),
            vec!["e1"],
            "a turn that retires a recorded limitation was not read as a reversal"
        );
    }

    // The second register asks which entry the turn is about, and what it
    // asks it of is the turn's stated intent. The prose is what said a
    // reversal happened; it is not what says which fact was reversed, and a
    // register that read it twice would nominate whichever entry happened to
    // share the reversal's words.
    #[test]
    fn the_second_register_measures_the_stated_intent_and_not_the_prose() {
        let object = object_with(&[
            ("e1", "the parser drops continuation lines"),
            ("e2", "a limitation that no longer applies"),
        ]);
        let senses = shipped_senses().expect("the shipped senses");
        let nominations = nominate_sense(
            &object,
            SenseText {
                turn: 18,
                prose: "the recorded limitation no longer applies",
                intent: "the parser drops continuation lines",
            },
            &senses,
            &Fixture,
            &shipped("0.500", 9, Gate::Without),
        )
        .expect("scored");
        assert_eq!(
            ids(&nominations),
            vec!["e1"],
            "the tier measured the prose of the turn where it should have measured the stated intent"
        );
    }

    // The lexical pre-gate is one of the four settings a calibration run
    // fixes, and the policy this repository ships selects it. The same turn,
    // scoring the same, is admitted or refused by that setting alone: the
    // seeded words are what the program's own reversals were found by before
    // there was an embedder to find them, and whether they must be present
    // is the factor the bakeoff varies.
    #[test]
    fn the_lexical_gate_decides_whether_a_turn_is_scored_at_all() {
        let object = object_with(&[("e1", "the parser drops continuation lines")]);
        let senses = shipped_senses().expect("the shipped senses");
        let ask = |prose: &str, gate: Gate| {
            nominate_sense(
                &object,
                SenseText {
                    turn: 18,
                    prose,
                    intent: "the parser drops continuation lines",
                },
                &senses,
                &Fixture,
                &shipped("0.500", 9, gate),
            )
            .expect("scored")
        };
        let unseeded = "the recorded limitation no longer applies";
        let seeded = "oh, i see: the recorded limitation no longer applies";
        assert_eq!(
            ids(&ask(unseeded, Gate::Without)),
            vec!["e1"],
            "the ungated policy did not nominate on a turn that scores well above its threshold"
        );
        assert!(
            ask(unseeded, Gate::With).is_empty(),
            "a turn carrying no seed was scored by a gated policy anyway"
        );
        assert_eq!(
            ids(&ask(seeded, Gate::With)),
            vec!["e1"],
            "a gated policy refused a turn that carries one of its seeds"
        );
    }

    // The budget is the number of confirm forks one turn may spend, and it
    // binds even when every entry clears the threshold. What it keeps is the
    // top of the ranking: a budget that truncated the bottom would spend the
    // tier's whole allowance on the entries least likely to be the one, and
    // that is worse than no budget at all.
    #[test]
    fn the_budget_is_the_most_a_turn_may_spend() {
        let object = object_with(&[
            ("e1", "the parser drops continuation lines"),
            ("e2", "the parser drops continuation lines too"),
            ("e3", "the build takes four minutes on this box"),
        ]);
        let senses = shipped_senses().expect("the shipped senses");
        let ask = |budget: u32| {
            nominate_sense(
                &object,
                SenseText {
                    turn: 18,
                    prose: "that limitation is gone now, it was fixed and no longer applies",
                    intent: "the parser drops continuation lines",
                },
                &senses,
                &Fixture,
                &shipped("0.000", budget, Gate::Without),
            )
            .expect("scored")
        };
        let two = ask(2);
        assert_eq!(two.len(), 2, "the budget did not bind");
        assert_eq!(
            ids(&two),
            vec!["e1", "e2"],
            "the budget was spent on the entries that scored lowest"
        );
        // The numbers a record reads back, and the numbers the fixture
        // embedder measures: the turn's stated intent is `e1` word for word,
        // `e2` says it again with one word more, and `e3` is about something
        // else. A nomination whose score is not the score that was measured
        // cannot be compared to the run that set the threshold, which is the
        // whole argument for the policy having one.
        let all = ask(9);
        assert_eq!(ids(&all), vec!["e1", "e2", "e3"]);
        assert_eq!(
            scores(&all),
            vec!["1.000".to_owned(), "0.913".to_owned(), "0.158".to_owned()],
            "a nomination carried a score nothing measured"
        );
        // The evidence says which register measured, so a confirm fork is
        // not handed a bare id.
        let one = ask(1);
        let Some(Evidence::Sense { register, .. }) = one.first().map(|n| n.evidence.clone()) else {
            panic!("tier 1 answered with something else: {one:?}");
        };
        assert_eq!(register, Register::Intent);
    }

    // An entry born in the turn that is reading it is not evidence about
    // itself -- the same rule tier 0 keeps. The two entries say different
    // things on purpose: the object folds a repeated content into an alias
    // of the entry that already holds it, so a second entry with the first
    // one's words is not a second entry at all, and a test built that way
    // would be naming an id nothing in the object has.
    #[test]
    fn an_entry_is_not_nominated_by_the_turn_that_made_it() {
        let mut object = object_with(&[("e1", "the parser drops continuation lines")]);
        object
            .apply(&Patch::Add {
                id: EntryId::new("e18").expect("an id"),
                content: "the reader keeps every continuation line now".to_owned(),
                provenance: Provenance {
                    turn: 18,
                    lane: "interview".to_owned(),
                    fork: None,
                    index: 0,
                },
            })
            .expect("added");
        let senses = shipped_senses().expect("the shipped senses");
        let nominations = nominate_sense(
            &object,
            SenseText {
                turn: 18,
                prose: "that limitation is gone now, it was fixed and no longer applies",
                intent: "the parser drops continuation lines",
            },
            &senses,
            &Fixture,
            &shipped("0.000", 9, Gate::Without),
        )
        .expect("scored");
        assert_eq!(
            ids(&nominations),
            vec!["e1"],
            "an entry born in this turn nominated itself"
        );
    }

    // Once a verdict has voided an entry, the object has retired that fact,
    // and this tier is the one with a budget: re-nominating a retired entry
    // spends a confirm fork on an answer already on file, every turn, for
    // as long as the object lives.
    #[test]
    fn a_voided_entry_is_not_nominated_again() {
        let mut object = object_with(&[("e1", "the parser drops continuation lines")]);
        object
            .apply(&Patch::Supersede {
                id: EntryId::new("a10/supersedes/e1").expect("an id"),
                content: "the parser keeps continuation lines now".to_owned(),
                voids: EntryId::new("e1").expect("an id"),
                provenance: Provenance {
                    turn: 10,
                    lane: "interview".to_owned(),
                    fork: None,
                    index: 0,
                },
            })
            .expect("superseded");
        let senses = shipped_senses().expect("the shipped senses");
        let nominations = nominate_sense(
            &object,
            SenseText {
                turn: 18,
                prose: "that limitation is gone now, it was fixed and no longer applies",
                intent: "the parser drops continuation lines",
            },
            &senses,
            &Fixture,
            &shipped("0.000", 9, Gate::Without),
        )
        .expect("scored");
        assert_eq!(
            ids(&nominations),
            vec!["a10/supersedes/e1"],
            "a voided entry was nominated again by the sense tier"
        );
    }

    // The report says what the policy was, because a nomination count with
    // no threshold beside it is a number nobody can read later.
    #[test]
    fn the_report_carries_the_policy_that_produced_it() {
        let policy = Policy::fixture().expect("the fixture");
        let Value::Object(members) = value(&policy, &[]) else {
            panic!("the report is an object");
        };
        assert_eq!(
            members.get("calibrated_on"),
            Some(&Value::String("fixture".to_owned()))
        );
        assert_eq!(
            members.get("threshold"),
            Some(&Value::Decimal(policy.threshold.clone()))
        );
        assert_eq!(members.get("nominated"), Some(&Value::Integer(0)));
        // The scoring and the gate are the other two factors a calibration
        // run varies. A count and a threshold with neither of them beside it
        // cannot be matched to the cell that produced the threshold, which
        // is the only thing that makes the number readable later.
        assert_eq!(
            members.get("scoring"),
            Some(&Value::String("contrastive".to_owned())),
            "the report did not say which scoring produced it"
        );
        assert_eq!(
            members.get("gate"),
            Some(&Value::String("with_gate".to_owned())),
            "the report did not say which lexical gate produced it"
        );
        assert_eq!(
            members.keys().collect::<Vec<_>>(),
            vec!["calibrated_on", "gate", "nominated", "scoring", "threshold"],
            "the report grew or lost a member"
        );
    }
}
