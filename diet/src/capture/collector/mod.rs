//! The collector: supersession detection.
//!
//! The working object accumulates entries, and some of them stop being true:
//! a gotcha gets resolved, an open question gets answered, a plan gets
//! replaced. Without a mechanism to notice, the object silently accumulates
//! stale high-confidence facts, and a later turn acts on one -- the
//! silent-loss class in reverse. In the research phase this was the single
//! most load-bearing unbuilt piece: everything else in the capture
//! architecture is only safe once supersession is detected.
//!
//! The design is a tiered nominator with a constrained verdict, and it is
//! precision-first on purpose. A false nomination costs a confirm slot; a
//! missed supersession costs correctness, but the archive retains everything,
//! so misses are recoverable and over-firing is the enemy. Imperative
//! nominations in the injection experiments caused capitulation on false
//! positives; nomination here is advisory, and a precision gate is declared
//! before any policy is tuned.
//!
//! * [`literal`] -- tier 0: an entry's anchors recurring in new text.
//! * [`sense`] -- tier 1: an entry and new prose near each other in a
//!   register, under a policy that has passed a calibration gate.
//! * [`reconcile`] -- what a fork's verdict does to the entry it nominated.
//!
//! Small models classify; they never trigger or generate. Nothing in this
//! module writes an entry: a nomination goes to a fork whose answer is a
//! [`crate::formats::verdict`], and the reconciler applies a link.

pub mod literal;
pub mod reconcile;
pub mod sense;

use crate::formats::record::json::Decimal;
use crate::object::EntryId;

use literal::{Anchor, Hit, Source};

/// A live entry a tier says is worth one confirm fork, and why.
///
/// The reason travels with the nomination because the tiers are not
/// interchangeable and the confirm fork is not the place to find out which
/// one fired. A literal nomination is a fact -- this anchor recurred at this
/// offset -- and a sense nomination is a measurement, which is a different
/// thing to weigh and a different thing to calibrate.
#[derive(Debug, Clone, PartialEq)]
pub struct Nomination {
    /// The entry nominated.
    pub entry: EntryId,
    /// What nominated it.
    pub evidence: Evidence,
}

/// Why an entry was nominated.
#[derive(Debug, Clone, PartialEq)]
pub enum Evidence {
    /// Tier 0: one of the entry's anchors recurred in the new text.
    Literal {
        /// The anchor that recurred.
        anchor: Anchor,
        /// Which text it recurred in.
        source: Source,
        /// The first occurrence.
        hit: Hit,
    },
    /// Tier 1: the entry and the new text scored near each other.
    Sense {
        /// Which register was measured.
        register: sense::Register,
        /// What it scored, as a decimal, because a score that reaches a
        /// record is a number the record can read back.
        score: Decimal,
    },
}

impl Nomination {
    /// The tier that nominated, as a stable name.
    #[must_use]
    pub fn tier(&self) -> &'static str {
        match self.evidence {
            Evidence::Literal { .. } => "literal",
            Evidence::Sense { .. } => "sense",
        }
    }
}
