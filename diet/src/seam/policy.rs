//! What the regimen says about when a seam fires.
//!
//! Every trigger is read from the regimen and is a number or a list. Nothing
//! here is a judgment. The scripted drive's budget is the byte count of the
//! working set at which a seam fires. A served session's budget is trunk
//! tokens as a fraction of the substrate's serving context
//! ([`SEAM_AT_CONTEXT_FRACTION`]): prefill cost scales with the trunk, which
//! is the premise's unit, and the threshold in tokens is reported with the
//! policy so a reader need not do the arithmetic.
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
/// The fraction of the serving context the trunk may reach before a served
/// session seams, written as a decimal in (0, 1].
pub const SEAM_AT_CONTEXT_FRACTION: &str = "seam_at_context_fraction";

/// The compaction depth (#552): how many estimated tokens of the old
/// trunk's most recent whole turns a served seam keeps after the refill.
/// Absent or 0 is the total refill (#505).
pub const SEAM_TAIL_TOKENS: &str = "seam_tail_tokens";
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
    /// A key that must hold a fraction in (0, 1] holds something else.
    NotAFraction(&'static str),
    /// A context fraction was declared and the substrate's serving context
    /// is not known, so no token count can be derived from it.
    NoServingContext,
    /// A key a served session does not read: refused rather than ignored,
    /// so a regimen never declares a trigger that never fires.
    NotServed(&'static str),
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
            Self::NotAFraction(key) => {
                write!(f, "`{key}` must be a decimal greater than 0 and at most 1")
            }
            Self::NoServingContext => write!(
                f,
                "`{SEAM_AT_CONTEXT_FRACTION}` is declared and the substrate's \
                 `serving_context` is not in the registry, so it names no token count"
            ),
            Self::NotServed(key) => write!(
                f,
                "a served session does not read `{key}`; its triggers are \
                 `{SEAM_EVERY_TURNS}` and `{SEAM_AT_CONTEXT_FRACTION}`"
            ),
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
        let every_turns = Self::from_regimen_cadence(regimen)?;
        let at_working_set_bytes = positive(regimen, SEAM_AT_WORKING_SET_BYTES)?;
        Ok(Self {
            every_turns,
            at_working_set_bytes,
            phases: phase_graph(regimen)?,
        })
    }
}

/// The phase graph `regimen` declares under [`PHASES`] and
/// [`PHASE_TRANSITIONS`]: the one reader the scripted drive and a served
/// session share (#563). Empty when none is declared.
///
/// # Errors
///
/// [`PolicyError`] for a key of the wrong shape, a duplicate phase, or a
/// transition naming a phase that was never declared.
pub fn phase_graph(regimen: &Regimen) -> Result<PhaseGraph, PolicyError> {
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
    Ok(graph)
}

impl Policy {
    /// The cadence `regimen` declares under [`SEAM_EVERY_TURNS`], which the
    /// scripted and the served policy read alike.
    fn from_regimen_cadence(regimen: &Regimen) -> Result<Option<u32>, PolicyError> {
        Ok(match positive(regimen, SEAM_EVERY_TURNS)? {
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

/// When a served session's derived seams fire: after an operator turn
/// returns the session to awaiting, a cadence of operator turns since the
/// last seam, or the trunk reaching a token count. The operator's own
/// declaration is not here: it is a command, always available.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Served {
    /// A seam once this many operator turns have settled since the last.
    pub every_turns: Option<u32>,
    /// A seam once the trunk holds this many tokens:
    /// [`SEAM_AT_CONTEXT_FRACTION`] of the serving context, rounded down.
    pub at_trunk_tokens: Option<u64>,
    /// The compaction depth: estimated tokens of recent whole turns a seam
    /// keeps after the refill ([`SEAM_TAIL_TOKENS`]); 0, the total refill.
    pub tail_tokens: u64,
}

impl Served {
    /// Read the served policy `regimen` declares, against the substrate's
    /// `serving_context` when the registry has one.
    ///
    /// # Errors
    ///
    /// [`PolicyError`] for a cadence that is not a positive whole number, a
    /// fraction outside (0, 1], a fraction with no serving context to take
    /// it of, or a key only the scripted drive reads.
    pub fn from_regimen(
        regimen: &Regimen,
        serving_context: Option<u64>,
    ) -> Result<Self, PolicyError> {
        // The phase graph is read beside this, by `phase_graph` (#563).
        if regimen.get(SEAM_AT_WORKING_SET_BYTES).is_some() {
            return Err(PolicyError::NotServed(SEAM_AT_WORKING_SET_BYTES));
        }
        let every_turns = Policy::from_regimen_cadence(regimen)?;
        let at_trunk_tokens = match regimen.get(SEAM_AT_CONTEXT_FRACTION) {
            None => None,
            Some(value) => {
                let fraction =
                    fraction(value).ok_or(PolicyError::NotAFraction(SEAM_AT_CONTEXT_FRACTION))?;
                let window = serving_context.ok_or(PolicyError::NoServingContext)?;
                // An f64 holds every serving context exactly (they are far
                // below 2^53), and the product is floored, so the threshold
                // never exceeds the fraction declared.
                #[allow(
                    clippy::cast_precision_loss,
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let tokens = (fraction * window as f64).floor() as u64;
                Some(tokens.max(1))
            }
        };
        let tail_tokens = match regimen.get(SEAM_TAIL_TOKENS) {
            None => 0,
            Some(Value::Integer(count)) if *count >= 0 => count.unsigned_abs(),
            Some(Value::Integer(_)) => {
                return Err(PolicyError::NotPositive(SEAM_TAIL_TOKENS));
            }
            Some(_) => return Err(PolicyError::NotAnInteger(SEAM_TAIL_TOKENS)),
        };
        Ok(Self {
            every_turns,
            at_trunk_tokens,
            tail_tokens,
        })
    }

    /// Whether anything in this policy can fire a derived seam.
    #[must_use]
    pub fn declares_a_trigger(&self) -> bool {
        self.every_turns.is_some() || self.at_trunk_tokens.is_some()
    }

    /// The trigger due, if any: the budget first, as the scripted
    /// controller ranks it. `since_seam` counts operator turns settled since
    /// the last seam, or since the session opened; `trunk_tokens` is the
    /// trunk as the latest trunk call measured it.
    #[must_use]
    pub fn due(&self, since_seam: u32, trunk_tokens: Option<u64>) -> Option<super::Reason> {
        if let (Some(limit), Some(tokens)) = (self.at_trunk_tokens, trunk_tokens)
            && tokens >= limit
        {
            return Some(super::Reason::Budget);
        }
        if self.every_turns.is_some_and(|every| since_seam >= every) {
            return Some(super::Reason::Cadence);
        }
        None
    }
}

/// A value in (0, 1], written as a decimal or as the integer 1.
fn fraction(value: &Value) -> Option<f64> {
    let parsed = match value {
        Value::Float(decimal) => decimal.as_str().parse::<f64>().ok()?,
        Value::Integer(1) => 1.0,
        _ => return None,
    };
    (parsed > 0.0 && parsed <= 1.0).then_some(parsed)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formats::regimen;
    use crate::seam::Reason;

    fn served(text: &str, window: Option<u64>) -> Result<Served, PolicyError> {
        Served::from_regimen(&regimen::parse(text).expect("a regimen"), window)
    }

    /// The compaction depth (#552): absent is 0, the total refill; a
    /// whole number of tokens otherwise, 0 included; anything else refused.
    #[test]
    fn the_seam_tail_is_a_whole_number_of_tokens_defaulting_to_none() {
        assert_eq!(served("", None).map(|s| s.tail_tokens), Ok(0));
        assert_eq!(
            served("seam_tail_tokens = 0\n", None).map(|s| s.tail_tokens),
            Ok(0)
        );
        assert_eq!(
            served("seam_tail_tokens = 8000\n", None).map(|s| s.tail_tokens),
            Ok(8000)
        );
        assert!(served("seam_tail_tokens = -1\n", None).is_err());
        assert!(served("seam_tail_tokens = \"lots\"\n", None).is_err());
    }

    #[test]
    fn a_served_policy_is_read_from_the_regimen_or_refused_with_a_reason() {
        assert_eq!(served("", Some(160_000)), Ok(Served::default()));
        assert_eq!(
            served(
                "seam_every_turns = 4\nseam_at_context_fraction = 0.6\n",
                Some(160_000)
            ),
            Ok(Served {
                every_turns: Some(4),
                at_trunk_tokens: Some(96_000),
                tail_tokens: 0,
            })
        );
        assert_eq!(
            served("seam_at_context_fraction = 1\n", Some(8192)),
            Ok(Served {
                every_turns: None,
                at_trunk_tokens: Some(8192),
                tail_tokens: 0,
            })
        );
        for bad in ["0.0", "1.5", "-0.2", "\"half\""] {
            assert_eq!(
                served(&format!("seam_at_context_fraction = {bad}\n"), Some(8192)),
                Err(PolicyError::NotAFraction(SEAM_AT_CONTEXT_FRACTION)),
                "{bad}"
            );
        }
        assert_eq!(
            served("seam_at_context_fraction = 0.5\n", None),
            Err(PolicyError::NoServingContext)
        );
        assert_eq!(
            served("seam_every_turns = 0\n", None),
            Err(PolicyError::NotPositive(SEAM_EVERY_TURNS))
        );
        assert_eq!(
            served("seam_at_working_set_bytes = 4096\n", None),
            Err(PolicyError::NotServed(SEAM_AT_WORKING_SET_BYTES))
        );
        // #563: a served session reads its phase graph (`phase_graph`).
        assert!(served("phases = [\"plan\", \"build\"]\n", None).is_ok());
    }

    #[test]
    fn the_budget_outranks_the_cadence_and_each_fires_only_when_reached() {
        let policy = Served {
            every_turns: Some(3),
            at_trunk_tokens: Some(1000),
            tail_tokens: 0,
        };
        assert_eq!(policy.due(2, Some(999)), None);
        assert_eq!(policy.due(2, None), None);
        assert_eq!(policy.due(3, Some(999)), Some(Reason::Cadence));
        assert_eq!(policy.due(2, Some(1000)), Some(Reason::Budget));
        assert_eq!(policy.due(3, Some(1000)), Some(Reason::Budget));
        assert_eq!(Served::default().due(100, Some(u64::MAX)), None);
    }
}
