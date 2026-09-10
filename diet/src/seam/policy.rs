//! What the regimen says about when a seam fires.
//!
//! Every trigger is read from the regimen and is a number or a list. Nothing
//! here is a judgment, and nothing here is a fraction of anything: "the
//! working set is approaching the declared ceiling" is spelled as the byte
//! count at which a seam fires, because a threshold computed from a ceiling
//! is a trigger a reader has to do arithmetic to predict, and a trigger
//! nobody can predict is one nobody can tell from the model's judgment.
//!
//! A regimen may declare no trigger at all. That is the scale-adaptive case
//! the issue asks for -- forcing a three-phase ceremony onto a one-line
//! change is a known rigidity cost -- and it is spelled by leaving the keys
//! out, not by setting them to zero.

use std::error::Error;
use std::fmt;

use crate::formats::regimen::{Regimen, Value};

use super::phase::PhaseGraph;

/// The keys a regimen may carry for the seam controller.
///
/// Written out so a misspelling in a regimen is a key nobody reads rather
/// than a trigger nobody notices is missing. Nothing checks for unknown keys:
/// a regimen carries far more than this module's business.
pub const SEAM_EVERY_TURNS: &str = "seam_every_turns";
/// The byte count at which the working set fires a seam.
pub const SEAM_AT_WORKING_SET_BYTES: &str = "seam_at_working_set_bytes";
/// The ordered list of phases the work moves through.
pub const PHASES: &str = "phases";
/// The table of allowed transitions, one array per phase.
pub const PHASE_TRANSITIONS: &str = "phase_transitions";

/// When this session's seams fire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// A seam every N turns, if a cadence is declared.
    pub every_turns: Option<u32>,
    /// A seam when the working set reaches this many bytes, if declared.
    pub at_working_set_bytes: Option<u64>,
    /// The phase graph, which may be empty.
    pub phases: PhaseGraph,
}

/// Why a regimen does not describe a seam policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// A key that must hold a whole number holds something else.
    NotAnInteger(&'static str),
    /// A count that must be positive is zero or negative. Zero is not "no
    /// cadence": a cadence of zero would fire on every turn or on none
    /// depending on how it is read, and the way to declare no cadence is to
    /// leave the key out.
    NotPositive(&'static str),
    /// A count too large for the field it names. Refused rather than
    /// clamped: a cadence silently becoming `u32::MAX` is a cadence that
    /// never fires, declared by somebody who thought it would.
    DoesNotFit {
        /// The key.
        key: &'static str,
        /// What was written.
        value: u64,
    },
    /// A key that must hold a list of names holds something else.
    NotAListOfNames(String),
    /// A transition table names a phase the phase list does not.
    UnknownPhase {
        /// The phase the edge is written under, or the edge's target.
        named: String,
        /// Where it was named.
        under: String,
    },
    /// Transitions were declared with no phases to move between.
    TransitionsWithoutPhases,
    /// The same phase appears twice in the list.
    DuplicatePhase(String),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnInteger(key) => write!(f, "`{key}` is not a whole number"),
            Self::NotPositive(key) => write!(
                f,
                "`{key}` must be positive; leave the key out to declare none"
            ),
            Self::DoesNotFit { key, value } => write!(
                f,
                "`{key} = {value}` is larger than the field can hold; it would have \
                 become a cadence that never fires"
            ),
            Self::NotAListOfNames(key) => {
                write!(f, "`{key}` is not a list of names")
            }
            Self::UnknownPhase { named, under } => write!(
                f,
                "`{named}`, named under `{under}`, is not one of the declared phases"
            ),
            Self::TransitionsWithoutPhases => f.write_str(
                "transitions are declared and phases are not, so every transition names \
                 a phase that does not exist",
            ),
            Self::DuplicatePhase(name) => {
                write!(f, "`{name}` appears twice in the phase list")
            }
        }
    }
}

impl Error for PolicyError {}

impl Policy {
    /// Read the policy `regimen` declares.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] for a key of the wrong shape, a non-positive
    /// count, or a transition naming a phase that was never declared.
    pub fn from_regimen(regimen: &Regimen) -> Result<Self, PolicyError> {
        let every_turns = match positive(regimen, SEAM_EVERY_TURNS)? {
            // The comment here used to say the cast was safe because
            // `positive` had refused anything that did not fit. It had not:
            // `positive` refuses non-positive numbers and the regimen carries
            // an `i64`, so `seam_every_turns = 5000000000` clamped silently to
            // `u32::MAX` -- a declared cadence turning into a different,
            // effectively-never one with no error anywhere.
            Some(count) => Some(u32::try_from(count).map_err(|_| PolicyError::DoesNotFit {
                key: SEAM_EVERY_TURNS,
                value: count,
            })?),
            None => None,
        };
        let at_working_set_bytes = positive(regimen, SEAM_AT_WORKING_SET_BYTES)?;

        let phases = names(regimen, PHASES)?.unwrap_or_default();
        let mut graph = PhaseGraph::of(&phases)?;

        if let Some(value) = regimen.get(PHASE_TRANSITIONS) {
            let Value::Table(table) = value else {
                return Err(PolicyError::NotAListOfNames(PHASE_TRANSITIONS.to_owned()));
            };
            if phases.is_empty() && !table.is_empty() {
                return Err(PolicyError::TransitionsWithoutPhases);
            }
            for (from, targets) in table {
                let Value::Array(items) = targets else {
                    return Err(PolicyError::NotAListOfNames(format!(
                        "{PHASE_TRANSITIONS}.{from}"
                    )));
                };
                for item in items {
                    let Value::String(to) = item else {
                        return Err(PolicyError::NotAListOfNames(format!(
                            "{PHASE_TRANSITIONS}.{from}"
                        )));
                    };
                    graph.allow(from, to)?;
                }
            }
        }

        Ok(Self {
            every_turns,
            at_working_set_bytes,
            phases: graph,
        })
    }

    /// Whether anything in this policy can fire a seam.
    ///
    /// A controller under a policy that declares nothing never renders, which
    /// is a legal session and a surprising one; a caller that wants to know
    /// asks here rather than inspecting three fields and getting it wrong.
    #[must_use]
    pub fn declares_a_trigger(&self) -> bool {
        self.every_turns.is_some() || self.at_working_set_bytes.is_some() || !self.phases.is_empty()
    }
}

/// A positive integer under `key`, if the regimen carries one.
fn positive(regimen: &Regimen, key: &'static str) -> Result<Option<u64>, PolicyError> {
    let Some(value) = regimen.get(key) else {
        return Ok(None);
    };
    let Value::Integer(count) = value else {
        return Err(PolicyError::NotAnInteger(key));
    };
    if *count <= 0 {
        return Err(PolicyError::NotPositive(key));
    }
    // The cast is safe: the branch above has established the value is
    // positive, so it fits an unsigned integer of the same width.
    Ok(Some(count.unsigned_abs()))
}

/// A list of names under `key`, if the regimen carries one.
fn names(regimen: &Regimen, key: &'static str) -> Result<Option<Vec<String>>, PolicyError> {
    let Some(value) = regimen.get(key) else {
        return Ok(None);
    };
    let Value::Array(items) = value else {
        return Err(PolicyError::NotAListOfNames(key.to_owned()));
    };
    let mut out = Vec::with_capacity(items.len());
    for item in items {
        let Value::String(name) = item else {
            return Err(PolicyError::NotAListOfNames(key.to_owned()));
        };
        out.push(name.clone());
    }
    Ok(Some(out))
}
