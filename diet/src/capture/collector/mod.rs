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
    ///
    /// The two tiers cost different things to be wrong about, so a reader
    /// weighing a confirm fork's outcome has to be able to say which one
    /// spent it, and it has to be spelled the same way every time.
    #[must_use]
    pub fn tier(&self) -> &'static str {
        match self.evidence {
            Evidence::Literal { .. } => "literal",
            Evidence::Sense { .. } => "sense",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::literal::{Anchor, AnchorKind, Hit, Source};
    use super::sense::Register;
    use super::{Evidence, Nomination};
    use crate::formats::record::json::Decimal;
    use crate::object::EntryId;

    /// Every spelling this module puts in a record, named rather than
    /// derived. A test that only walked `ALL` and asked `from_tag` to agree
    /// with `tag` would pass on an empty `ALL` and on any permutation of the
    /// tags: it would be checking that the vocabulary agrees with itself.
    /// The words are the interface, so the words are written out.
    #[test]
    fn the_collectors_vocabularies_are_the_words_a_record_carries() {
        fn check<T: Copy + PartialEq + std::fmt::Debug>(
            name: &str,
            all: &[T],
            tag: impl Fn(T) -> &'static str,
            from_tag: impl Fn(&str) -> Option<T>,
            expected: &[&str],
        ) {
            let tags: Vec<&str> = all.iter().map(|item| tag(*item)).collect();
            assert_eq!(
                tags, expected,
                "the {name} vocabulary is not what it promises"
            );
            let unique: BTreeSet<&str> = tags.iter().copied().collect();
            assert_eq!(unique.len(), all.len(), "two variants share a tag: {all:?}");
            for item in all {
                assert_eq!(from_tag(tag(*item)), Some(*item));
            }
            assert!(from_tag("").is_none());
            assert!(from_tag("no such tag").is_none());
        }
        check(
            "anchor kind",
            AnchorKind::ALL,
            AnchorKind::tag,
            AnchorKind::from_tag,
            &["identifier", "path", "quoted"],
        );
        check(
            "source",
            Source::ALL,
            Source::tag,
            Source::from_tag,
            &["prose", "tool_output"],
        );
        check(
            "register",
            Register::ALL,
            Register::tag,
            Register::from_tag,
            &["reversal", "intent"],
        );
    }

    /// The tier is the other spelling, and it is not a vocabulary with a
    /// table behind it -- it is read off the evidence, so it is checked
    /// against evidence of each kind.
    #[test]
    fn a_nomination_names_the_tier_that_made_it() {
        let entry = EntryId::new("e1").expect("an id");
        let by_anchor = Nomination {
            entry: entry.clone(),
            evidence: Evidence::Literal {
                anchor: Anchor {
                    text: "check_record".to_owned(),
                    kind: AnchorKind::Identifier,
                },
                source: Source::Prose,
                hit: Hit { offset: 0 },
            },
        };
        let by_score = Nomination {
            entry,
            evidence: Evidence::Sense {
                register: Register::Intent,
                score: Decimal::new("0.913").expect("a decimal"),
            },
        };
        assert_eq!(
            (by_anchor.tier(), by_score.tier()),
            ("literal", "sense"),
            "a nomination named the wrong tier"
        );
    }
}
