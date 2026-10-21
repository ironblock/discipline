//! What the regimen says about when a meditation fires.
//!
//! Every trigger is read from the regimen and is a number or a list. Nothing
//! here is a judgment. The scripted drive's budget is the byte count of the
//! working set at which a meditation fires. A served session's budget is trunk
//! tokens as a fraction of the substrate's serving context
//! ([`MEDITATION_AT_CONTEXT_FRACTION`]): prefill cost scales with the trunk, which
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

/// The keys a regimen may carry for the meditation controller.
///
/// Written out so a misspelling in a regimen is a key nobody reads rather
/// than a trigger nobody notices is missing. Nothing checks for unknown keys:
/// a regimen carries far more than this module's business.
pub const MEDITATION_EVERY_TURNS: &str = "meditation_every_turns";
/// The byte count at which the working set fires a meditation.
pub const MEDITATION_AT_WORKING_SET_BYTES: &str = "meditation_at_working_set_bytes";
/// The fraction of the serving context the trunk may reach before a served
/// session meditations, written as a decimal in (0, 1].
pub const MEDITATION_AT_CONTEXT_FRACTION: &str = "meditation_at_context_fraction";

/// The compaction depth (#552): how many estimated tokens of the old
/// trunk's most recent whole turns a served meditation keeps after the refill.
/// Absent or 0 is the total refill (#505).
pub const MEDITATION_TAIL_TOKENS: &str = "meditation_tail_tokens";
/// What a served meditation's refill carries of the tool outputs it compacts away
/// (#553): `evict` (the default), `reference`, `salient` or `keep`, read
/// leniently -- any other value is unset.
pub const MEDITATION_TOOL_OUTPUTS: &str = "meditation_tool_outputs";
/// What a served meditation's refill carries (#936): `render` (the default),
/// working memory rendered; or `native_summary`, a compaction summary the
/// model writes with the vote's prompt, and what Qwen Code puts beside it
/// ([`super::native`]). Read leniently: any other value is the default.
pub const MEDITATION_CARRY: &str = "meditation_carry";
/// The automatic meditation (#617): on unless the regimen says `"off"`. Before
/// every trunk request a served session that keeps working memory and knows
/// its window refills the trunk when the prompt would leave less than the
/// [`native_reserve`] of the window -- plus, with interview forks on, a
/// fork's room (#931).
pub const MEDITATION_WINDOW: &str = "meditation_window";
/// The least the native reserve keeps, in tokens (#931): Qwen Code's
/// `SUMMARY_RESERVE` of 20,000 and its 13,000 beside it
/// (`qc:packages/core/src/services/chatCompressionService.ts:58,109-124` at
/// `v0.25.3`).
pub const NATIVE_RESERVE_MIN: u64 = 33_000;

/// The room the automatic meditation keeps of a `window` (#617, #931), by
/// the harness vote: Pi keeps 16,384
/// (`pi:packages/coding-agent/src/core/settings-defaults.ts:9-13` at
/// `v1.3.0`) and `OpenCode` 2 the larger of a tenth of the window and 16,000
/// (`oc:packages/core/src/session/compaction.ts:97,911-918` at `v2.0.32`).
/// They differ, so Qwen Code decides: it compacts above min(0.85 × window,
/// window − 33,000), and above 0.85 × window when the window is no larger
/// than 33,000 (`qc:packages/core/src/services/chatCompressionService.ts:210-232`).
#[must_use]
pub fn native_reserve(window: u64) -> u64 {
    let fraction = window / 20 * 17 + window % 20 * 17 / 20;
    let line = if window > NATIVE_RESERVE_MIN {
        fraction.min(window - NATIVE_RESERVE_MIN)
    } else {
        fraction
    };
    window - line
}
/// A meditation's audit (#504): `"on"` (or `true`) puts the dogma's pinned audit
/// ask over working memory before the refill and folds its answer; off by
/// default.
pub const MEDITATION_AUDIT: &str = "meditation_audit";
/// The notes one audit fork asks over: an audit over more is split into
/// chunks of this many, each its own fork, the operator's changed notes in
/// the first. A whole number above 0; [`AUDIT_CHUNK_NOTES_DEFAULT`] unless
/// declared.
pub const MEDITATION_AUDIT_CHUNK_NOTES: &str = "meditation_audit_chunk_notes";
/// The notes an audit fork asks over unless the regimen says: the most whose
/// reasoning and answer fit a 32,768 cap in the T1 runs' audits (2026-10-11
/// and 12). At 69 and 95 notes, reasoning ran 13-15k tokens and the answer
/// 7-8 a note, inside the cap; at 231 to 517 notes, reasoning alone took the
/// whole cap and four answers never began.
pub const AUDIT_CHUNK_NOTES_DEFAULT: usize = 100;
/// How many of an audit's chunks may be in flight at once: a whole number
/// above 0, 1 -- one after another -- unless declared. The chunks overlap
/// no further than the trunk's server batches (its entry's
/// `served_max_batch_size`) and its pool holds them, and only on the lines
/// route: through the record tools (#610) a chunk's patches may supersede
/// another chunk's note, so there they run one at a time
/// ([`Served::audit_concurrency`]).
pub const MEDITATION_AUDIT_CONCURRENCY: &str = "meditation_audit_concurrency";
/// A meditation's pre-warm (#504): `"on"` (or `true`) sends the refilled trunk
/// once, with an output cap of one, so the next ask finds it cached; off by
/// default, and never after a `window` meditation, whose request warms it.
pub const MEDITATION_WARM: &str = "meditation_warm";
/// Whether a meditation refills over an operator change no audit addressed
/// (#150): `"carry"`, the default, refills and carries the change to the
/// next meditation; `"refuse"` re-asks the reconciling audit once over the
/// changes left, then holds the meditation for the operator. `"refuse"` needs
/// [`MEDITATION_AUDIT`] on: only an audit addresses a change.
pub const RECONCILE_GATE: &str = "reconcile_gate";

/// Whether `regimen` turns `key` on: `"on"` or `true`.
fn on(regimen: &Regimen, key: &str) -> bool {
    match regimen.get(key) {
        Some(Value::Boolean(on)) => *on,
        Some(Value::String(word)) => word == "on",
        _ => false,
    }
}

/// The render budget (#565): estimated tokens the meditation's render may run to.
/// Read leniently: absent, or anything but a positive integer, is no budget.
pub const RENDER_BUDGET_TOKENS: &str = "render_budget_tokens";

/// What the render does past its budget (#565): `"tier"` (the default) or
/// `"elide"`.
pub const RENDER_OVER_BUDGET: &str = "render_over_budget";
/// The phase library a session runs in (#787): `starter`, `user`, or a
/// directory of phase documents.
pub const PHASE_LIBRARY: &str = "phase_library";
/// The library's phase the session starts in (#787).
pub const STARTING_PHASE: &str = "starting_phase";
/// The ordered list of phases (#563), replaced by [`PHASE_LIBRARY`] and
/// [`STARTING_PHASE`] (#787): refused, naming them.
pub const PHASES: &str = "phases";
/// The table of allowed transitions (#563), replaced likewise.
pub const PHASE_TRANSITIONS: &str = "phase_transitions";

/// When this session's meditations fire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// A meditation every N turns, if a cadence is declared.
    pub every_turns: Option<u32>,
    /// A meditation when the working set reaches this many bytes, if declared.
    pub at_working_set_bytes: Option<u64>,
    /// The phase graph, which may be empty.
    pub phases: PhaseGraph,
}

/// Why a regimen does not describe a meditation policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// A key that must hold a whole number holds something else.
    NotAnInteger(&'static str),
    /// A key that must hold a whole number above 0 holds something else:
    /// a count of what runs at once, which a typo must not turn into the
    /// default.
    NotACount(&'static str),
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
    /// A key that must hold one word holds something else.
    NotAWord(&'static str),
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
    /// [`RECONCILE_GATE`] holds a word other than `carry` or `refuse`.
    NotAGate,
    /// [`RECONCILE_GATE`] is `refuse` with the audit off: nothing could
    /// address a change, so every meditation with one would hold.
    GateWithoutAudit,
    /// A key #787 replaced: `phases` or `phase_transitions`.
    Replaced(&'static str),
    /// [`PHASE_LIBRARY`] without [`STARTING_PHASE`], or the other way round.
    HalfAPhaseStart,
    /// The library [`PHASE_LIBRARY`] names does not read: why.
    PhaseLibrary(String),
    /// [`STARTING_PHASE`] names no phase of the library.
    NotInTheLibrary(String),
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAnInteger(key) => write!(f, "`{key}` is not a whole number"),
            Self::NotACount(key) => write!(f, "`{key}` is a whole number above 0"),
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
            Self::NotAWord(key) => write!(f, "`{key}` is not a quoted word"),
            Self::DuplicatePhase(name) => {
                write!(f, "`{name}` appears twice in the phase list")
            }
            Self::Replaced(key) => write!(
                f,
                "`{key}` is replaced by `{PHASE_LIBRARY}` and `{STARTING_PHASE}` (#787): name a \
                 library of phase documents and the phase to start in"
            ),
            Self::HalfAPhaseStart => write!(
                f,
                "`{PHASE_LIBRARY}` and `{STARTING_PHASE}` name one start: both or neither"
            ),
            Self::PhaseLibrary(why) => write!(f, "`{PHASE_LIBRARY}`: {why}"),
            Self::NotInTheLibrary(name) => write!(
                f,
                "`{STARTING_PHASE}` is `{name}`, a phase the library has no document for"
            ),
            Self::NotAFraction(key) => {
                write!(f, "`{key}` must be a decimal greater than 0 and at most 1")
            }
            Self::NoServingContext => write!(
                f,
                "`{MEDITATION_AT_CONTEXT_FRACTION}` is declared and the substrate's \
                 `serving_context` is not in the registry, so it names no token count"
            ),
            Self::NotServed(key) => write!(
                f,
                "a served session does not read `{key}`; its triggers are \
                 `{MEDITATION_EVERY_TURNS}` and `{MEDITATION_AT_CONTEXT_FRACTION}`"
            ),
            Self::NotAGate => write!(
                f,
                "`{RECONCILE_GATE}` takes \"carry\" (the default) or \"refuse\""
            ),
            Self::GateWithoutAudit => write!(
                f,
                "`{RECONCILE_GATE} = \"refuse\"` needs `{MEDITATION_AUDIT}` on: only the \
                 audit addresses an operator change, so every meditation with one would hold"
            ),
        }
    }
}

impl Error for PolicyError {}

/// [`MEDITATION_TOOL_OUTPUTS`] read, leniently: a state's word, or `evict`.
#[must_use]
pub fn meditation_tool_outputs(regimen: &Regimen) -> crate::formats::log::MeditationToolOutputs {
    use crate::formats::log::MeditationToolOutputs;
    match regimen.get(MEDITATION_TOOL_OUTPUTS) {
        Some(Value::String(word)) => MeditationToolOutputs::from_tag(word).unwrap_or_default(),
        _ => MeditationToolOutputs::default(),
    }
}

/// [`MEDITATION_CARRY`] read, leniently: a state's word, or `render`.
#[must_use]
pub fn meditation_carry(regimen: &Regimen) -> crate::formats::log::MeditationCarry {
    use crate::formats::log::MeditationCarry;
    match regimen.get(MEDITATION_CARRY) {
        Some(Value::String(word)) => MeditationCarry::from_tag(word).unwrap_or_default(),
        _ => MeditationCarry::default(),
    }
}

impl Policy {
    /// Read the policy `regimen` declares.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] for a key of the wrong shape, a non-positive
    /// count, or a transition naming a phase that was never declared.
    pub fn from_regimen(regimen: &Regimen) -> Result<Self, PolicyError> {
        let every_turns = Self::from_regimen_cadence(regimen)?;
        let at_working_set_bytes = positive(regimen, MEDITATION_AT_WORKING_SET_BYTES)?;
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
    for replaced in [PHASES, PHASE_TRANSITIONS] {
        if regimen.get(replaced).is_some() {
            return Err(PolicyError::Replaced(replaced));
        }
    }
    let word = |key: &'static str| match regimen.get(key) {
        None => Ok(None),
        Some(Value::String(word)) => Ok(Some(word.clone())),
        Some(_) => Err(PolicyError::NotAWord(key)),
    };
    match (word(PHASE_LIBRARY)?, word(STARTING_PHASE)?) {
        (None, None) => Ok(PhaseGraph::none()),
        (Some(library), Some(start)) => {
            let library = super::library::named(&library).map_err(PolicyError::PhaseLibrary)?;
            PhaseGraph::from_library(&library, &start)
        }
        _ => Err(PolicyError::HalfAPhaseStart),
    }
}

impl Policy {
    /// The cadence `regimen` declares under [`MEDITATION_EVERY_TURNS`], which the
    /// scripted and the served policy read alike.
    fn from_regimen_cadence(regimen: &Regimen) -> Result<Option<u32>, PolicyError> {
        Ok(match positive(regimen, MEDITATION_EVERY_TURNS)? {
            // The comment here used to say the cast was safe because
            // `positive` had refused anything that did not fit. It had not:
            // `positive` refuses non-positive numbers and the regimen carries
            // an `i64`, so `meditation_every_turns = 5000000000` clamped silently to
            // `u32::MAX` -- a declared cadence turning into a different,
            // effectively-never one with no error anywhere.
            Some(count) => Some(u32::try_from(count).map_err(|_| PolicyError::DoesNotFit {
                key: MEDITATION_EVERY_TURNS,
                value: count,
            })?),
            None => None,
        })
    }

    /// Whether anything in this policy can fire a meditation.
    ///
    /// A controller under a policy that declares nothing never renders, which
    /// is a legal session and a surprising one; a caller that wants to know
    /// asks here rather than inspecting three fields and getting it wrong.
    #[must_use]
    pub fn declares_a_trigger(&self) -> bool {
        self.every_turns.is_some() || self.at_working_set_bytes.is_some() || !self.phases.is_empty()
    }
}

/// When a served session's derived meditations fire: after an operator turn
/// returns the session to awaiting, a cadence of operator turns since the
/// last meditation, or the trunk reaching a token count. The operator's own
/// declaration is not here: it is a command, always available.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Served {
    /// A meditation once this many operator turns have settled since the last.
    pub every_turns: Option<u32>,
    /// A meditation once the trunk holds this many tokens:
    /// [`MEDITATION_AT_CONTEXT_FRACTION`] of the serving context, rounded down.
    pub at_trunk_tokens: Option<u64>,
    /// The compaction depth: estimated tokens of recent whole turns a meditation
    /// keeps after the refill ([`MEDITATION_TAIL_TOKENS`]); 0, the total refill.
    pub tail_tokens: u64,
    /// What a meditation carries of the tool outputs it compacts away
    /// ([`MEDITATION_TOOL_OUTPUTS`]).
    pub outputs: crate::formats::log::MeditationToolOutputs,
    /// What a meditation's refill carries ([`MEDITATION_CARRY`], #936).
    pub carry: crate::formats::log::MeditationCarry,
    /// The render's budget ([`RENDER_BUDGET_TOKENS`], #565), when declared.
    pub render_budget: Option<super::render::Budget>,
    /// Whether the regimen turned the automatic meditation off ([`MEDITATION_WINDOW`],
    /// #617): on by default, as Pi and `OpenCode` 2 both have it.
    pub window_off: bool,
    /// Whether a meditation puts the dogma's audit ask to the model before its
    /// refill ([`MEDITATION_AUDIT`], #504).
    pub audit: bool,
    /// Whether a meditation sends its refilled trunk once to warm the cache
    /// ([`MEDITATION_WARM`], #504).
    pub warm: bool,
    /// Whether a meditation refills over an unaddressed operator change
    /// ([`RECONCILE_GATE`], #150).
    pub reconcile_gate: ReconcileGate,
    /// The notes one audit fork asks over
    /// ([`MEDITATION_AUDIT_CHUNK_NOTES`]), when declared.
    pub audit_chunk_notes: Option<usize>,
    /// The audit chunks in flight at once ([`MEDITATION_AUDIT_CONCURRENCY`]),
    /// when declared.
    pub audit_concurrency: Option<usize>,
}

impl Served {
    /// The notes one audit fork asks over: the regimen's, else
    /// [`AUDIT_CHUNK_NOTES_DEFAULT`].
    #[must_use]
    pub fn audit_chunk(&self) -> usize {
        self.audit_chunk_notes
            .unwrap_or(AUDIT_CHUNK_NOTES_DEFAULT)
            .max(1)
    }

    /// The audit chunks in flight at once, and what lowered it from the
    /// regimen's: the regimen's (1 unless declared), no more than the
    /// trunk's server batches (`served_batch`, its entry's
    /// `served_max_batch_size`, when declared), and 1 on the record tools'
    /// route (`tools`). The pool's room lowers it further as the chunks go.
    #[must_use]
    pub fn audit_concurrency(
        &self,
        served_batch: Option<usize>,
        tools: bool,
    ) -> (usize, Option<&'static str>) {
        let declared = self.audit_concurrency.unwrap_or(1).max(1);
        if tools && declared > 1 {
            return (1, Some("tools_route"));
        }
        match served_batch.filter(|batch| *batch < declared) {
            Some(batch) => (batch.max(1), Some("served_max_batch_size")),
            None => (declared, None),
        }
    }
}

/// The audit chunks `regimen` declares in flight at once
/// ([`MEDITATION_AUDIT_CONCURRENCY`]), `None` when it declares none.
///
/// # Errors
///
/// [`PolicyError::NotACount`] for anything but a whole number above 0.
pub fn audit_concurrency_declared(regimen: &Regimen) -> Result<Option<usize>, PolicyError> {
    match regimen.get(MEDITATION_AUDIT_CONCURRENCY) {
        None => Ok(None),
        Some(Value::Integer(n)) if *n > 0 => usize::try_from(*n)
            .map(Some)
            .map_err(|_| PolicyError::NotACount(MEDITATION_AUDIT_CONCURRENCY)),
        Some(_) => Err(PolicyError::NotACount(MEDITATION_AUDIT_CONCURRENCY)),
    }
}

/// What a meditation does about an operator change no audit addressed
/// ([`RECONCILE_GATE`], #150).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ReconcileGate {
    /// Refill, and carry the change to the next meditation: the default.
    #[default]
    Carry,
    /// Re-ask the reconciling audit once over what is left, then hold the
    /// meditation for the operator.
    Refuse,
}

/// The render budget `regimen` declares (#565), leniently: a positive
/// [`RENDER_BUDGET_TOKENS`], with [`RENDER_OVER_BUDGET`] `"elide"` or else
/// tier; anything else is no budget.
#[must_use]
pub fn render_budget(regimen: &Regimen) -> Option<super::render::Budget> {
    use super::render::{Budget, OverBudget};
    let tokens = match regimen.get(RENDER_BUDGET_TOKENS) {
        Some(Value::Integer(count)) if *count > 0 => count.unsigned_abs(),
        _ => return None,
    };
    let over = match regimen.get(RENDER_OVER_BUDGET) {
        Some(Value::String(word)) if word == "elide" => OverBudget::Elide,
        _ => OverBudget::Tier,
    };
    Some(Budget { tokens, over })
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
        if regimen.get(MEDITATION_AT_WORKING_SET_BYTES).is_some() {
            return Err(PolicyError::NotServed(MEDITATION_AT_WORKING_SET_BYTES));
        }
        let every_turns = Policy::from_regimen_cadence(regimen)?;
        let at_trunk_tokens = match regimen.get(MEDITATION_AT_CONTEXT_FRACTION) {
            None => None,
            Some(value) => {
                let fraction = fraction(value)
                    .ok_or(PolicyError::NotAFraction(MEDITATION_AT_CONTEXT_FRACTION))?;
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
        let tail_tokens = match regimen.get(MEDITATION_TAIL_TOKENS) {
            None => 0,
            Some(Value::Integer(count)) if *count >= 0 => count.unsigned_abs(),
            Some(Value::Integer(_)) => {
                return Err(PolicyError::NotPositive(MEDITATION_TAIL_TOKENS));
            }
            Some(_) => return Err(PolicyError::NotAnInteger(MEDITATION_TAIL_TOKENS)),
        };
        let reconcile_gate = match regimen.get(RECONCILE_GATE) {
            None => ReconcileGate::Carry,
            Some(Value::String(word)) if word == "carry" => ReconcileGate::Carry,
            Some(Value::String(word)) if word == "refuse" => ReconcileGate::Refuse,
            Some(_) => return Err(PolicyError::NotAGate),
        };
        if reconcile_gate == ReconcileGate::Refuse && !on(regimen, MEDITATION_AUDIT) {
            return Err(PolicyError::GateWithoutAudit);
        }
        Ok(Self {
            reconcile_gate,
            every_turns,
            at_trunk_tokens,
            tail_tokens,
            outputs: meditation_tool_outputs(regimen),
            carry: meditation_carry(regimen),
            render_budget: render_budget(regimen),
            audit: on(regimen, MEDITATION_AUDIT),
            audit_chunk_notes: match regimen.get(MEDITATION_AUDIT_CHUNK_NOTES) {
                Some(Value::Integer(n)) if *n > 0 => usize::try_from(*n).ok(),
                _ => None,
            },
            audit_concurrency: audit_concurrency_declared(regimen)?,
            warm: on(regimen, MEDITATION_WARM),
            window_off: matches!(regimen.get(MEDITATION_WINDOW), Some(Value::String(word)) if word == "off")
                || matches!(regimen.get(MEDITATION_WINDOW), Some(Value::Boolean(false))),
        })
    }

    /// Whether anything in this policy can fire a derived meditation.
    #[must_use]
    pub fn declares_a_trigger(&self) -> bool {
        self.every_turns.is_some() || self.at_trunk_tokens.is_some()
    }

    /// The trigger due, if any: the budget first, as the scripted
    /// controller ranks it. `since_meditation` counts operator turns settled since
    /// the last meditation, or since the session opened; `trunk_tokens` is the
    /// trunk as the latest trunk call measured it.
    #[must_use]
    pub fn due(&self, since_meditation: u32, trunk_tokens: Option<u64>) -> Option<super::Reason> {
        if let (Some(limit), Some(tokens)) = (self.at_trunk_tokens, trunk_tokens)
            && tokens >= limit
        {
            return Some(super::Reason::Budget);
        }
        if self
            .every_turns
            .is_some_and(|every| since_meditation >= every)
        {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// #931: the native reserve is Qwen Code's -- the window less
    /// min(0.85 × window, window − 33,000), or less 0.85 × window at a
    /// window of 33,000 or under.
    #[test]
    fn the_native_reserve_is_qwen_codes() {
        assert_eq!(
            native_reserve(128_000),
            33_000,
            "w − 33,000 is the lower line"
        );
        assert_eq!(
            native_reserve(262_144),
            39_322,
            "0.85 × w is the lower line"
        );
        assert_eq!(native_reserve(220_000), 33_000, "where the two lines meet");
        assert_eq!(native_reserve(32_768), 4_916, "a small window keeps 15%");
        assert_eq!(native_reserve(0), 0);
    }
    use crate::formats::regimen;
    use crate::meditation::Reason;

    fn served(text: &str, window: Option<u64>) -> Result<Served, PolicyError> {
        Served::from_regimen(&regimen::parse(text).expect("a regimen"), window)
    }

    /// #150's gate: `carry` by default and as written, `refuse` as
    /// written with the audit on; any other word, or `refuse` with the audit
    /// off, is refused.
    #[test]
    fn the_reconcile_gate_is_carry_or_refuse_and_refuse_needs_the_audit() {
        let gate = |text: &str| served(text, None).map(|read| read.reconcile_gate);
        assert_eq!(gate(""), Ok(ReconcileGate::Carry));
        assert_eq!(
            gate("reconcile_gate = \"carry\"\n"),
            Ok(ReconcileGate::Carry)
        );
        assert_eq!(
            gate("meditation_audit = \"on\"\nreconcile_gate = \"refuse\"\n"),
            Ok(ReconcileGate::Refuse)
        );
        assert_eq!(
            gate("reconcile_gate = \"refuse\"\n"),
            Err(PolicyError::GateWithoutAudit)
        );
        assert_eq!(
            gate("meditation_audit = \"on\"\nreconcile_gate = \"hold\"\n"),
            Err(PolicyError::NotAGate)
        );
    }

    /// The audit chunks in flight at once: 1 unless declared, a whole number
    /// above 0 as written, and anything else refused at start, naming the key
    /// -- never read as the default.
    #[test]
    fn the_audit_concurrency_is_a_whole_number_above_0_or_refused() {
        let beside = |text: &str| served(text, None).map(|read| read.audit_concurrency);
        assert_eq!(beside(""), Ok(None));
        assert_eq!(beside("meditation_audit_concurrency = 4\n"), Ok(Some(4)));
        for wrong in ["0", "-1", "\"4\"", "4.0"] {
            let refused = beside(&format!("meditation_audit_concurrency = {wrong}\n"))
                .expect_err(wrong)
                .to_string();
            assert!(
                refused.contains("`meditation_audit_concurrency`")
                    && refused.contains("a whole number above 0"),
                "{wrong}: {refused}"
            );
        }
    }

    /// The compaction depth (#552): absent is 0, the total refill; a
    /// whole number of tokens otherwise, 0 included; anything else refused.
    #[test]
    fn the_meditation_tail_is_a_whole_number_of_tokens_defaulting_to_none() {
        assert_eq!(served("", None).map(|s| s.tail_tokens), Ok(0));
        assert_eq!(
            served("meditation_tail_tokens = 0\n", None).map(|s| s.tail_tokens),
            Ok(0)
        );
        assert_eq!(
            served("meditation_tail_tokens = 8000\n", None).map(|s| s.tail_tokens),
            Ok(8000)
        );
        assert!(served("meditation_tail_tokens = -1\n", None).is_err());
        assert!(served("meditation_tail_tokens = \"lots\"\n", None).is_err());
    }

    #[test]
    fn a_served_policy_is_read_from_the_regimen_or_refused_with_a_reason() {
        assert_eq!(served("", Some(160_000)), Ok(Served::default()));
        assert_eq!(
            served(
                "meditation_every_turns = 4\nmeditation_at_context_fraction = 0.6\n",
                Some(160_000)
            ),
            Ok(Served {
                reconcile_gate: ReconcileGate::Carry,
                every_turns: Some(4),
                at_trunk_tokens: Some(96_000),
                tail_tokens: 0,
                outputs: crate::formats::log::MeditationToolOutputs::Evict,
                carry: crate::formats::log::MeditationCarry::Render,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
                audit_chunk_notes: None,
                audit_concurrency: None,
            })
        );
        assert_eq!(
            served("meditation_at_context_fraction = 1\n", Some(8192)),
            Ok(Served {
                reconcile_gate: ReconcileGate::Carry,
                every_turns: None,
                at_trunk_tokens: Some(8192),
                tail_tokens: 0,
                outputs: crate::formats::log::MeditationToolOutputs::Evict,
                carry: crate::formats::log::MeditationCarry::Render,
                render_budget: None,
                window_off: false,
                warm: false,
                audit: false,
                audit_chunk_notes: None,
                audit_concurrency: None,
            })
        );
        for bad in ["0.0", "1.5", "-0.2", "\"half\""] {
            assert_eq!(
                served(
                    &format!("meditation_at_context_fraction = {bad}\n"),
                    Some(8192)
                ),
                Err(PolicyError::NotAFraction(MEDITATION_AT_CONTEXT_FRACTION)),
                "{bad}"
            );
        }
        assert_eq!(
            served("meditation_at_context_fraction = 0.5\n", None),
            Err(PolicyError::NoServingContext)
        );
        assert_eq!(
            served("meditation_every_turns = 0\n", None),
            Err(PolicyError::NotPositive(MEDITATION_EVERY_TURNS))
        );
        assert_eq!(
            served("meditation_at_working_set_bytes = 4096\n", None),
            Err(PolicyError::NotServed(MEDITATION_AT_WORKING_SET_BYTES))
        );
        // #563: a served session reads its phase graph (`phase_graph`).
        assert!(served("phases = [\"plan\", \"build\"]\n", None).is_ok());
    }

    #[test]
    fn the_budget_outranks_the_cadence_and_each_fires_only_when_reached() {
        let policy = Served {
            reconcile_gate: ReconcileGate::Carry,
            every_turns: Some(3),
            at_trunk_tokens: Some(1000),
            tail_tokens: 0,
            outputs: crate::formats::log::MeditationToolOutputs::Evict,
            carry: crate::formats::log::MeditationCarry::Render,
            render_budget: None,
            window_off: false,
            warm: false,
            audit: false,
            audit_chunk_notes: None,
            audit_concurrency: None,
        };
        assert_eq!(policy.due(2, Some(999)), None);
        assert_eq!(policy.due(2, None), None);
        assert_eq!(policy.due(3, Some(999)), Some(Reason::Cadence));
        assert_eq!(policy.due(2, Some(1000)), Some(Reason::Budget));
        assert_eq!(policy.due(3, Some(1000)), Some(Reason::Budget));
        assert_eq!(Served::default().due(100, Some(u64::MAX)), None);
    }
}
