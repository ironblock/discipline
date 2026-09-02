//! The groundedness gate.
//!
//! Any lane that writes with capture authority carries a mechanical,
//! LLM-free check that everything it emitted is present in the input it was
//! told to work from. Three incidents established that structure is not
//! truth, and the third is why this exists rather than a review habit:
//!
//! A structuring pass -- reformat an interview answer into typed JSON --
//! emitted **454 entries for one 4,222-character answer** into an
//! `api_surface` field whose definition invited enumeration. 397 were absent
//! from the answer it was told to restructure; 149 could be traced to the
//! session prefix it should not have been reading; **248 existed nowhere**.
//! Grounding score: 0.142. On a probe with no session in context the same
//! pass had scored a perfect 1.000 -- because fabrication was structurally
//! impossible there, not because the mechanism was sound.
//!
//! Two things follow, and both are in the types rather than in a convention:
//!
//! * **A score without a demonstrated failure is not a measurement.**
//!   [`Measurement::take`] refuses to hand back a score unless the same code,
//!   on a case built to fail, actually scored below the floor. A number whose
//!   instrument has never been seen fail is a number about the probe.
//! * **A threshold must be pre-registered, not argued.** [`Floor`] cannot be
//!   constructed without saying where its value came from, and there is no
//!   `Default`. Reading a threshold off the data you are about to judge is how
//!   a threshold becomes whatever it needed to be.
//!
//! Scope, precisely: the gate binds **per lane**, against that lane's contract
//! input. Judgment-class fields have no groundable source and are exempt --
//! grounding a plan is a category error that would reject legitimate content.

use std::error::Error;
use std::fmt;

use crate::formats::interview::FieldKind;
use crate::formats::record::Event;

/// What a lane's output can be grounded against.
///
/// The two halves are kept apart because the distinction is the finding: a
/// string traceable to the session prefix was **read**, and one traceable to
/// nothing was **invented**. Same verdict, different mechanism, and a lane
/// that bleeds needs a different repair from one that fabricates.
#[derive(Debug, Clone, Copy)]
pub struct ContractInput<'a> {
    /// What the lane was told to work from. This, and only this, grounds an
    /// entry.
    pub source: &'a str,
    /// What the lane could see but was not told to work from.
    pub session_prefix: &'a str,
}

/// How an entry relates to the contract input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Verdict {
    /// Present in the source. Kept.
    Grounded,
    /// Absent from the source, present in the session prefix. Dropped: a
    /// lane's contract input is what it was told to work from, not everything
    /// it could see.
    Bleed,
    /// Present in neither. Dropped.
    Invention,
}

impl Verdict {
    /// Every verdict, so a report can be summarised without a match that
    /// forgets one.
    pub const ALL: &'static [Self] = &[Self::Grounded, Self::Bleed, Self::Invention];

    /// A stable name, for records and fixtures.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Grounded => "grounded",
            Self::Bleed => "bleed",
            Self::Invention => "invention",
        }
    }

    /// Whether an entry with this verdict survives.
    #[must_use]
    pub fn keeps(self) -> bool {
        matches!(self, Self::Grounded)
    }
}

/// What a field's content can be checked against.
///
/// Attached to [`FieldKind`] rather than chosen per call, so the same field
/// cannot be judgment-class in one lane and structuring in another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldClass {
    /// Quotes and excerpts: grounds against the archive they claim to quote.
    Verbatim,
    /// A restructuring of something the model said: grounds against that.
    Structuring,
    /// Plans, decisions, opinions, next steps. There is no source to check
    /// against, and checking anyway rejects legitimate content.
    Judgment,
}

impl FieldClass {
    /// Whether the gate touches a field of this class.
    #[must_use]
    pub fn is_gated(self) -> bool {
        !matches!(self, Self::Judgment)
    }
}

impl FieldKind {
    /// What this field can be grounded against.
    ///
    /// The scoping was ruled after the third incident, and it is narrow on
    /// purpose: four of the six kinds are judgment, and the gate does not
    /// touch them.
    #[must_use]
    pub fn class(self) -> FieldClass {
        match self {
            // Names the turn touched: a restructuring of the answer, and the
            // field the 454-entry fabrication landed in.
            Self::ApiSurface => FieldClass::Structuring,
            // Excerpts from tool output, which either appear in it or do not.
            Self::Evidence => FieldClass::Verbatim,
            // What the turn decided, learned, intends, or could not get past.
            // None of these has a source to be present in.
            Self::Decision | Self::Learned | Self::Plan | Self::Stuck => FieldClass::Judgment,
        }
    }
}

/// An exact fraction. Never a float.
///
/// A banked number should read back as the number that was computed, and
/// comparisons here are cross-multiplied rather than divided, so a score is
/// the counts it came from and not the nearest double to their quotient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    /// How many entries were grounded.
    pub grounded: u64,
    /// How many entries there were.
    pub of: u64,
}

impl Score {
    /// Whether this score is at or above `floor`.
    ///
    /// A lane that emitted nothing is not below any floor: there is nothing
    /// to reject, and rejecting it would turn an empty lane into a failing
    /// one.
    #[must_use]
    pub fn meets(self, floor: &Floor) -> bool {
        if self.of == 0 {
            return true;
        }
        u128::from(self.grounded) * u128::from(floor.of)
            >= u128::from(floor.grounded) * u128::from(self.of)
    }
}

impl fmt::Display for Score {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.grounded, self.of)
    }
}

/// A per-lane floor, and where its value came from.
///
/// There is no `Default` and no constant. A floor is a claim about measured
/// data, and one that cannot say which data is a number somebody wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Floor {
    grounded: u64,
    of: u64,
    provenance: String,
}

impl Floor {
    /// A floor of `grounded`/`of`, registered as coming from `provenance`.
    ///
    /// # Errors
    ///
    /// Returns [`FloorError`] for a zero denominator, a fraction above one, or
    /// an empty provenance.
    pub fn pre_registered(grounded: u64, of: u64, provenance: &str) -> Result<Self, FloorError> {
        if of == 0 {
            return Err(FloorError::ZeroDenominator);
        }
        if grounded > of {
            return Err(FloorError::AboveOne { grounded, of });
        }
        if provenance.trim().is_empty() {
            return Err(FloorError::NoProvenance);
        }
        Ok(Self {
            grounded,
            of,
            provenance: provenance.to_owned(),
        })
    }

    /// Where this floor's value came from.
    #[must_use]
    pub fn provenance(&self) -> &str {
        &self.provenance
    }
}

impl fmt::Display for Floor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{} ({})", self.grounded, self.of, self.provenance)
    }
}

/// Why a floor is not a floor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FloorError {
    /// A fraction over nothing.
    ZeroDenominator,
    /// A fraction above one, which no score can meet.
    AboveOne {
        /// The numerator.
        grounded: u64,
        /// The denominator.
        of: u64,
    },
    /// A threshold that cannot say where its value came from.
    NoProvenance,
}

impl fmt::Display for FloorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroDenominator => write!(f, "a floor of something over nothing"),
            Self::AboveOne { grounded, of } => {
                write!(
                    f,
                    "a floor of {grounded}/{of} is above one, so no score meets it"
                )
            }
            Self::NoProvenance => write!(
                f,
                "a floor with no provenance: a threshold is a claim about \
                 measured data, and one that cannot say which data is a number \
                 somebody wanted"
            ),
        }
    }
}

impl Error for FloorError {}

/// One entry, and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The text the lane emitted, verbatim.
    pub text: String,
    /// What it was found to be.
    pub verdict: Verdict,
}

/// What became of a lane's whole output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneOutcome {
    /// The lane's surviving entries stand.
    Accepted,
    /// The lane's output is rejected whole and the fallback path's output
    /// stands. A pass that mostly fabricated has demonstrated it was not
    /// structuring; its individual survivors are not trustworthy either.
    Rejected,
}

/// A lane's output, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneReport {
    /// Every entry the lane emitted, in order, with its verdict. Nothing is
    /// absent: a dropped entry is dropped from the record's *content*, not
    /// from its account of what happened.
    pub entries: Vec<Entry>,
    /// The grounded fraction, as the counts it came from.
    pub score: Score,
    /// Whether the lane's output stands.
    pub outcome: LaneOutcome,
}

impl LaneReport {
    /// The entries that survive: grounded, and only if the lane was accepted.
    #[must_use]
    pub fn kept(&self) -> Vec<&str> {
        if self.outcome == LaneOutcome::Rejected {
            return Vec::new();
        }
        self.entries
            .iter()
            .filter(|entry| entry.verdict.keeps())
            .map(|entry| entry.text.as_str())
            .collect()
    }

    /// How many entries carried `verdict`.
    #[must_use]
    pub fn count(&self, verdict: Verdict) -> usize {
        self.entries
            .iter()
            .filter(|entry| entry.verdict == verdict)
            .count()
    }

    /// The `rejected` event this report calls for, if it calls for one.
    ///
    /// A rejection that is not in the record is a rejection nobody can audit:
    /// the fallback output stands, and the only trace of why is a number
    /// somebody remembers.
    #[must_use]
    pub fn rejection_event(&self, id: &str, lane: &str) -> Option<Event> {
        (self.outcome == LaneOutcome::Rejected).then(|| Event::Rejected {
            id: id.to_owned(),
            lane: lane.to_owned(),
            grounded: self.score.grounded,
            of: self.score.of,
        })
    }
}

/// Check one lane's entries against its contract input.
///
/// `class` decides whether the gate applies at all. A judgment-class field is
/// returned untouched, with every entry `Grounded` and the lane accepted --
/// not because they were checked, but because there is nothing to check them
/// against.
///
/// Matching is **presence, normalised for whitespace and nothing else**. Not
/// fuzzy, not case-folded: a fuzzy match is a judgment call, and the point of
/// this check is that it is not one. A lane whose contract is to reword needs
/// a different contract, not a looser check.
#[must_use]
pub fn check(
    entries: &[String],
    class: FieldClass,
    input: ContractInput<'_>,
    floor: &Floor,
) -> LaneReport {
    if !class.is_gated() {
        let entries: Vec<Entry> = entries
            .iter()
            .map(|text| Entry {
                text: text.clone(),
                verdict: Verdict::Grounded,
            })
            .collect();
        let score = Score {
            grounded: entries.len() as u64,
            of: entries.len() as u64,
        };
        return LaneReport {
            entries,
            score,
            outcome: LaneOutcome::Accepted,
        };
    }

    let source = normalise(input.source);
    let prefix = normalise(input.session_prefix);
    let checked: Vec<Entry> = entries
        .iter()
        .map(|text| {
            let needle = normalise(text);
            let verdict = if !needle.is_empty() && source.contains(&needle) {
                Verdict::Grounded
            } else if !needle.is_empty() && prefix.contains(&needle) {
                Verdict::Bleed
            } else {
                Verdict::Invention
            };
            Entry {
                text: text.clone(),
                verdict,
            }
        })
        .collect();

    let grounded = checked.iter().filter(|entry| entry.verdict.keeps()).count() as u64;
    let score = Score {
        grounded,
        of: checked.len() as u64,
    };
    let outcome = if score.meets(floor) {
        LaneOutcome::Accepted
    } else {
        LaneOutcome::Rejected
    };
    LaneReport {
        entries: checked,
        score,
        outcome,
    }
}

/// Whitespace runs collapse to one space, and the ends are trimmed. Nothing
/// else: case and punctuation are content.
fn normalise(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A grounding number, and the case that proves the number can be low.
///
/// The 1.000 that meant nothing was a real score, computed by real code, on a
/// probe where fabrication was structurally impossible. Nothing about the
/// number said so. This type exists so that a caller cannot obtain a score
/// without also obtaining, from the same code, a score on an input built to
/// fail -- and cannot obtain it at all if that input did not fail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    /// The lane being measured.
    pub subject: LaneReport,
    /// The same check, on an input built to be ungrounded.
    pub demonstrated_failure: LaneReport,
    /// The floor both were judged against.
    pub floor: Floor,
}

impl Measurement {
    /// Measure `entries`, and demonstrate on `failing` that the measurement
    /// can come out low.
    ///
    /// # Errors
    ///
    /// Returns [`MeasurementError::InstrumentNeverFailed`] when the
    /// demonstrated-failure case does not, in fact, fail. That is the whole
    /// point: an instrument that has never been seen fail is not an
    /// instrument, and a perfect subject score alongside it means nothing.
    pub fn take(
        entries: &[String],
        class: FieldClass,
        input: ContractInput<'_>,
        floor: &Floor,
        failing: (&[String], ContractInput<'_>),
    ) -> Result<Self, MeasurementError> {
        let (failing_entries, failing_input) = failing;
        let demonstrated_failure = check(failing_entries, class, failing_input, floor);
        if demonstrated_failure.outcome != LaneOutcome::Rejected {
            return Err(MeasurementError::InstrumentNeverFailed {
                score: demonstrated_failure.score,
                floor: floor.clone(),
            });
        }
        Ok(Self {
            subject: check(entries, class, input, floor),
            demonstrated_failure,
            floor: floor.clone(),
        })
    }
}

/// Why a measurement is not a measurement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MeasurementError {
    /// The case built to fail did not fail, so the subject's score says
    /// nothing about the subject.
    InstrumentNeverFailed {
        /// What the failing case actually scored.
        score: Score,
        /// The floor it was judged against.
        floor: Floor,
    },
}

impl fmt::Display for MeasurementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InstrumentNeverFailed { score, floor } => write!(
                f,
                "the demonstrated-failure case scored {score} against a floor \
                 of {floor} and was accepted: an instrument that has never \
                 been seen fail cannot certify anything"
            ),
        }
    }
}

impl Error for MeasurementError {}

#[cfg(test)]
mod tests {
    use super::{
        ContractInput, FieldClass, Floor, FloorError, LaneOutcome, Measurement, MeasurementError,
        Score, Verdict, check,
    };
    use crate::formats::interview::FieldKind;
    use crate::formats::record::{Event, Kind};

    /// The answer a reformat lane was told to restructure.
    const SOURCE: &str = "I renamed the capture lane and added a register fixture. \
                          The resolver now refuses when the binary is older than the source.";

    /// What the lane could see and was not told to work from.
    const PREFIX: &str = "Earlier this session I touched the wilson_interval helper \
                          and the sediment registrar.";

    fn input() -> ContractInput<'static> {
        ContractInput {
            source: SOURCE,
            session_prefix: PREFIX,
        }
    }

    /// A floor for the tests. Its value is not a finding -- the issue puts the
    /// real number out of scope until lanes exist to measure -- so it says so.
    fn floor() -> Floor {
        Floor::pre_registered(
            1,
            2,
            "test fixture; the shipped value is measured, not this",
        )
        .expect("a well-formed floor")
    }

    /// A floor low enough not to fire, for the tests about the PER-ENTRY rule.
    /// The two rules are separate -- an entry drops because it is ungrounded,
    /// a lane is rejected because too few of its entries survived -- and a
    /// test that let the floor fire would be testing the wrong one.
    fn permissive_floor() -> Floor {
        Floor::pre_registered(1, 100, "test fixture; deliberately below any lane here")
            .expect("a well-formed floor")
    }

    fn entries(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|t| (*t).to_owned()).collect()
    }

    // Acceptance: three strings absent from the source must all drop, and
    // telemetry must record them as invention.
    #[test]
    fn strings_absent_from_the_source_drop_as_invention() {
        let emitted = entries(&[
            "renamed the capture lane",
            "parse_interview_answer",
            "GroundingReport::finalize",
            "wilson_interval_bounds",
        ]);
        let report = check(
            &emitted,
            FieldClass::Structuring,
            input(),
            &permissive_floor(),
        );
        assert_eq!(report.count(Verdict::Invention), 3);
        assert_eq!(report.kept(), vec!["renamed the capture lane"]);
        for entry in &report.entries[1..] {
            assert_eq!(entry.verdict, Verdict::Invention, "{entry:?}");
        }
    }

    // Acceptance: a string present only in the session prefix must drop, and
    // be recorded as bleed rather than invention. Same verdict, different
    // mechanism -- a lane that reads what it was not given needs a different
    // repair from one that fabricates.
    #[test]
    fn a_string_from_the_session_prefix_drops_as_bleed() {
        let emitted = entries(&["the sediment registrar", "renamed the capture lane"]);
        let report = check(
            &emitted,
            FieldClass::Structuring,
            input(),
            &permissive_floor(),
        );
        assert_eq!(report.entries[0].verdict, Verdict::Bleed);
        assert_eq!(report.count(Verdict::Invention), 0);
        assert_eq!(report.kept(), vec!["renamed the capture lane"]);
    }

    // Acceptance: a lane at 0.1 grounded must be rejected whole, and a
    // `rejected` event emitted with the score. Its individual survivors are
    // not trustworthy: a pass that mostly fabricated was not structuring.
    #[test]
    fn a_lane_below_the_floor_is_rejected_whole_and_recorded() {
        let mut emitted = vec!["renamed the capture lane".to_owned()];
        for index in 0..9 {
            emitted.push(format!("Fabricated::name_{index}"));
        }
        let report = check(&emitted, FieldClass::Structuring, input(), &floor());
        assert_eq!(
            report.score,
            Score {
                grounded: 1,
                of: 10
            }
        );
        assert_eq!(
            report.outcome,
            LaneOutcome::Rejected,
            "a lane that mostly fabricated must be rejected whole"
        );
        assert!(
            report.kept().is_empty(),
            "a rejected lane keeps nothing, not even its grounded entries"
        );
        let event = report
            .rejection_event("x1", "reformat")
            .expect("a rejected lane emits a rejected event");
        assert_eq!(event.kind(), Kind::Rejected);
        assert!(matches!(
            event,
            Event::Rejected {
                grounded: 1,
                of: 10,
                ..
            }
        ));
    }

    #[test]
    fn a_lane_at_the_floor_is_accepted() {
        let emitted = entries(&["renamed the capture lane", "Fabricated::name"]);
        let report = check(&emitted, FieldClass::Structuring, input(), &floor());
        assert_eq!(report.score, Score { grounded: 1, of: 2 });
        assert_eq!(report.outcome, LaneOutcome::Accepted);
        assert_eq!(report.kept(), vec!["renamed the capture lane"]);
        assert!(report.rejection_event("x1", "reformat").is_none());
    }

    // Acceptance: the gate must not touch a judgment-class field. Grounding a
    // plan is a category error that would reject legitimate content.
    #[test]
    fn the_gate_does_not_touch_a_judgment_class_field() {
        let plan = entries(&[
            "ship the boundary first, then measure",
            "nothing here appears in the source at all",
        ]);
        let report = check(&plan, FieldClass::Judgment, input(), &floor());
        let untouched = report.outcome == LaneOutcome::Accepted
            && report.count(Verdict::Invention) == 0
            && report.count(Verdict::Bleed) == 0;
        assert!(
            untouched,
            "the gate touched a judgment-class field: {report:?}"
        );
        assert_eq!(report.kept().len(), 2, "a plan keeps everything it said");
    }

    #[test]
    fn every_field_kind_has_a_class_and_only_two_are_gated() {
        let gated: Vec<_> = FieldKind::ALL
            .iter()
            .filter(|kind| kind.class().is_gated())
            .map(|kind| kind.canonical_tag())
            .collect();
        assert_eq!(gated, vec!["api_surface", "evidence"]);
        assert_eq!(FieldKind::Plan.class(), FieldClass::Judgment);
        assert_eq!(FieldKind::Decision.class(), FieldClass::Judgment);
        assert_eq!(FieldKind::Evidence.class(), FieldClass::Verbatim);
    }

    // A threshold that cannot say where its value came from is a number
    // somebody wanted. There is no `Default` and no constant, so this is the
    // only way to get one.
    #[test]
    fn a_floor_must_be_pre_registered() {
        assert_eq!(
            Floor::pre_registered(7, 10, "   "),
            Err(FloorError::NoProvenance)
        );
        assert_eq!(
            Floor::pre_registered(1, 0, "measured"),
            Err(FloorError::ZeroDenominator)
        );
        assert!(matches!(
            Floor::pre_registered(11, 10, "measured"),
            Err(FloorError::AboveOne { .. })
        ));
        assert!(Floor::pre_registered(7, 10, "the bimodal separation").is_ok());
    }

    // The 1.000 that meant nothing was a real score on a probe where
    // fabrication was structurally impossible. A measurement now cannot be
    // obtained unless the same code, on a case built to fail, actually failed.
    #[test]
    fn a_measurement_requires_its_instrument_to_have_been_seen_fail() {
        let subject = entries(&["renamed the capture lane"]);
        let fabricating = entries(&["Invented::one", "Invented::two"]);

        let measurement = Measurement::take(
            &subject,
            FieldClass::Structuring,
            input(),
            &floor(),
            (&fabricating, input()),
        )
        .expect("the failing case fails, so the measurement stands");
        assert_eq!(measurement.subject.score, Score { grounded: 1, of: 1 });
        assert_eq!(
            measurement.demonstrated_failure.outcome,
            LaneOutcome::Rejected
        );

        // The probe on which fabrication was impossible: its "failing" case is
        // grounded, so it never failed, so the perfect subject score certifies
        // nothing and there is no measurement to hand back.
        let err = Measurement::take(
            &subject,
            FieldClass::Structuring,
            input(),
            &floor(),
            (&subject, input()),
        )
        .expect_err("a measurement was handed back whose instrument never failed");
        assert!(
            matches!(err, MeasurementError::InstrumentNeverFailed { .. }),
            "a measurement was handed back whose instrument never failed: {err:?}"
        );
    }

    #[test]
    fn matching_normalises_whitespace_and_nothing_else() {
        let emitted = entries(&[
            "renamed   the\n  capture lane",
            "Renamed the capture lane",
            "",
        ]);
        let report = check(
            &emitted,
            FieldClass::Structuring,
            input(),
            &permissive_floor(),
        );
        assert_eq!(
            report.entries[0].verdict,
            Verdict::Grounded,
            "whitespace runs collapse"
        );
        assert_eq!(
            report.entries[1].verdict,
            Verdict::Invention,
            "case is content, not formatting"
        );
        assert_eq!(
            report.entries[2].verdict,
            Verdict::Invention,
            "an empty entry is grounded in nothing"
        );
    }

    // A lane that emitted nothing is not a lane that failed. Rejecting it
    // would turn an empty lane into a failing one and hide the emptiness.
    #[test]
    fn an_empty_lane_is_not_below_the_floor() {
        let report = check(&[], FieldClass::Structuring, input(), &floor());
        assert_eq!(report.score, Score { grounded: 0, of: 0 });
        assert_eq!(report.outcome, LaneOutcome::Accepted);
    }

    // Scores are cross-multiplied, never divided. 1/3 against a floor of
    // 1/3 is exactly met; through f64 it is 0.3333333333333333 against
    // 0.3333333333333333, which happens to work and is not a reason to trust
    // it.
    #[test]
    fn a_score_is_the_counts_it_came_from() {
        let third = Floor::pre_registered(1, 3, "a fixture").expect("a floor");
        assert!(Score { grounded: 1, of: 3 }.meets(&third));
        assert!(Score { grounded: 2, of: 6 }.meets(&third));
        assert!(!Score { grounded: 1, of: 4 }.meets(&third));
        // Counts large enough that the product overflows u64 but not u128.
        let huge = Floor::pre_registered(u64::MAX, u64::MAX, "a fixture").expect("a floor");
        assert!(
            Score {
                grounded: u64::MAX,
                of: u64::MAX
            }
            .meets(&huge)
        );
    }
}
