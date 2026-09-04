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
//!   the turn is now about? It is the entry's stated intent against the
//!   turn's, measured as plain cosine, because both sides are transcript
//!   prose rather than an authored description and there is no authored
//!   opposite to contrast against.
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// How a sense set is scored against a text.
    pub scoring: Scoring,
    /// Whether the lexical pre-gate is applied.
    pub gate: Gate,
    /// The score at or above which a register fires. A decimal, not a float:
    /// a threshold that reaches a record is a number the record reads back.
    pub threshold: Decimal,
    /// The most entries this tier may nominate in one turn.
    pub budget: u32,
    /// Where the threshold came from.
    pub calibrated_on: Calibration,
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

impl Policy {
    /// Read a policy for use.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Uncalibrated`] for a policy that names no run,
    /// [`PolicyError::Fixture`] for the fixture policy, and
    /// [`PolicyError::Data`] for a file that is not one policy row.
    pub fn load(source: &str) -> Result<Self, PolicyError> {
        let policy = Self::read(source)?;
        match policy.calibrated_on {
            Calibration::Fixture => Err(PolicyError::Fixture),
            Calibration::Run(_) => Ok(policy),
        }
    }

    /// The shipped fixture policy, for tests and for nothing else.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError::Uncalibrated`] if the shipped fixture ever
    /// stops declaring itself a fixture -- which would make it a policy
    /// claiming a calibration it does not have.
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
    use crate::capture::collector::Evidence;
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
    }

    #[test]
    fn a_policy_the_record_could_not_read_back_is_refused() {
        let bad = [
            // A threshold that is not a decimal at all. A float spelled as
            // an integer, or as a second spelling of zero, is a number that
            // reads back differently from the one that was written.
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"1","budget":3,"calibrated_on":"results/x"}"#,
                "an integer threshold",
            ),
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"5e-1","budget":3,"calibrated_on":"results/x"}"#,
                "an exponent threshold",
            ),
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"-0.0","budget":3,"calibrated_on":"results/x"}"#,
                "a negative zero threshold",
            ),
            // A budget of zero is a tier switched off, and a policy is not
            // the switch.
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"0.500","budget":0,"calibrated_on":"results/x"}"#,
                "a budget of zero",
            ),
            // A scoring nobody implemented.
            (
                r#"{"scoring":"vibes","gate":"with_gate","threshold":"0.500","budget":3,"calibrated_on":"results/x"}"#,
                "an unknown scoring",
            ),
            // A key the schema does not have: a policy that carries one has
            // a setting the reader is ignoring.
            (
                r#"{"scoring":"contrastive","gate":"with_gate","threshold":"0.500","budget":3,"calibrated_on":"results/x","tweak":1}"#,
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

    // A turn that reverses nothing nominates nothing, and that is the tier's
    // ordinary answer rather than a failure.
    #[test]
    fn a_turn_that_is_not_a_reversal_nominates_nothing() {
        let object = object_with(&[("e1", "the parser drops continuation lines")]);
        let senses = shipped_senses().expect("the shipped senses");
        let nominations = nominate_sense(
            &object,
            SenseText {
                turn: 18,
                prose: "I read the file and it is as described.",
                intent: "read the parser",
            },
            &senses,
            &Fixture,
            &shipped("0.900", 3, Gate::Without),
        )
        .expect("scored");
        assert!(
            nominations.is_empty(),
            "an unremarkable turn nominated: {nominations:?}"
        );
    }

    // The budget is the number of confirm forks one turn may spend, and it
    // binds even when every entry clears the threshold.
    #[test]
    fn the_budget_is_the_most_a_turn_may_spend() {
        let object = object_with(&[
            ("e1", "the parser drops continuation lines"),
            ("e2", "the parser drops continuation lines too"),
            ("e3", "the parser drops continuation lines as well"),
            ("e4", "the parser drops continuation lines besides"),
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
        assert_eq!(ask(2).len(), 2, "the budget did not bind");
        assert!(ask(9).len() > 2, "the budget bound when it should not");
        // Whatever the budget, the evidence says which register measured and
        // what it measured, so a confirm fork is not handed a bare id.
        let one = ask(1);
        let Some(Evidence::Sense { register, score }) = one.first().map(|n| n.evidence.clone())
        else {
            panic!("tier 1 answered with something else: {one:?}");
        };
        assert_eq!(register, Register::Intent);
        assert!(score.as_str().contains('.'), "{score:?}");
    }

    // An entry born in the turn that is reading it is not evidence about
    // itself -- the same rule tier 0 keeps.
    #[test]
    fn an_entry_is_not_nominated_by_the_turn_that_made_it() {
        let mut object = object_with(&[("e1", "the parser drops continuation lines")]);
        object
            .apply(&Patch::Add {
                id: EntryId::new("e18").expect("an id"),
                content: "the parser drops continuation lines".to_owned(),
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
        assert!(
            nominations.iter().all(|n| n.entry.as_str() != "e18"),
            "an entry nominated itself: {nominations:?}"
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
    }
}
