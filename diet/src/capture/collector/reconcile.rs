//! The reconciler's side of a verdict: what a fork's answer does to the
//! nominated entry.
//!
//! The model judges; the grammar formats; this applies. What it applies is
//! never a deletion. `SUPERSEDED` is a [`Patch::Supersede`] -- the new fact
//! is created, the old one is voided and linked to it, and both stay in the
//! object -- because the archive is what makes a missed supersession
//! recoverable and a wrong one reversible. `DONE` resolves the entry.
//! `PARTIAL` and `NOT_THIS` change nothing, and `NOT_THIS` is counted: it is
//! the false nomination the precision gate is calibrated against.
//!
//! The new entry's identity is derived from the record's own ids -- the event
//! whose text nominated, and the entry it supersedes -- never minted.

use crate::formats::verdict::{Answer, Verdict};
use crate::object::{EntryId, ObjectError, Patch, Provenance};

use super::Nomination;

/// What a verdict came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The new prose replaces the entry: apply this, and the old entry is
    /// voided and linked, never removed.
    Superseded(Patch),
    /// The entry is settled: apply this.
    Resolved(Patch),
    /// The prose bears on the entry without settling it. Nothing to apply.
    Partial,
    /// The nomination was wrong. Nothing to apply, and one false nomination
    /// to count against the tier that made it.
    NotThis,
}

impl Outcome {
    /// The patch to apply, if the verdict produced one.
    #[must_use]
    pub fn patch(&self) -> Option<&Patch> {
        match self {
            Self::Superseded(patch) | Self::Resolved(patch) => Some(patch),
            Self::Partial | Self::NotThis => None,
        }
    }
}

/// What the new fact is, when a verdict says the old one is superseded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replacement<'a> {
    /// The record id of the event whose text nominated: a response or a
    /// tool call. The new entry's id is derived from it.
    pub event: &'a str,
    /// The new fact, as content. Grounded by construction when it is the
    /// nominating text itself; a lane that rewords it owes the gate a pass.
    pub content: &'a str,
    /// Where the verdict came from.
    pub provenance: Provenance,
}

/// Apply a verdict to a nomination.
///
/// # Errors
///
/// Returns [`ObjectError::EmptyId`] only if the derived id is empty, which
/// the record's own validation of event ids already excludes.
pub fn reconcile(
    nomination: &Nomination,
    answer: &Answer,
    replacement: Replacement<'_>,
) -> Result<Outcome, ObjectError> {
    Ok(match answer.verdict {
        Verdict::Superseded => Outcome::Superseded(Patch::Supersede {
            id: EntryId::new(&format!(
                "{}/supersedes/{}",
                replacement.event, nomination.entry
            ))?,
            content: replacement.content.to_owned(),
            voids: nomination.entry.clone(),
            provenance: replacement.provenance,
        }),
        Verdict::Done => Outcome::Resolved(Patch::Resolve {
            target: nomination.entry.clone(),
            provenance: replacement.provenance,
        }),
        Verdict::Partial => Outcome::Partial,
        Verdict::NotThis => Outcome::NotThis,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Outcome, Replacement, reconcile};
    use crate::capture::collector::literal::{NewText, nominate};
    use crate::formats::record::json::Value;
    use crate::formats::record::{Engine, Reasoning, Regime, Substrate, Weights};
    use crate::formats::verdict;
    use crate::object::{EntryId, EntryState, Patch, Provenance, WorkingObject};

    fn regime() -> Regime {
        Regime {
            arm: "baseline".to_owned(),
            dogma_version: 0,
            substrates: vec![Substrate {
                id: "local".to_owned(),
                engine: Engine {
                    name: "a-runtime".to_owned(),
                    version_or_digest: "1.0".to_owned(),
                },
                weights: Weights::Digest("a".repeat(64)),
                hardware_fingerprint:
                    "edaec1af6bd2226d6464c29bbbf6d0d139179ddd03bff236351a3ae3e2dad532".to_owned(),
                sampler_card: BTreeMap::from([("seed".to_owned(), Value::Integer(7))]),
                reasoning: Reasoning::On,
            }],
        }
    }

    /// Where a verdict came from. The confirm fork is an interview fork --
    /// a single ask with a constrained answer -- so it carries the lane that
    /// already exists rather than a name coined here for it.
    ///
    /// Trunk: `tangent` is written by `object::tangent::Tangent::provenance`
    /// and by nothing else, and a collector that stamped a scope by hand
    /// would put one in the record nothing checked.
    fn at(turn: u32) -> Provenance {
        Provenance {
            turn,
            lane: "interview".to_owned(),
            fork: Some("f1".to_owned()),
            tangent: None,
            index: 0,
        }
    }

    /// The acceptance drive: an entry at turn 5, contradicted at turn 18.
    fn object_with_stale_entry() -> WorkingObject {
        let mut object = WorkingObject::open(regime());
        object
            .apply(&Patch::Add {
                id: EntryId::new("e1").expect("id"),
                content: "`check_record` is missing from the CLI".to_owned(),
                provenance: Provenance {
                    turn: 5,
                    lane: "interview".to_owned(),
                    fork: None,
                    tangent: None,
                    index: 0,
                },
            })
            .expect("added");
        object
    }

    const PROSE: &str = "Actually, check_record exists now: the CLI gained it.";

    fn replacement() -> Replacement<'static> {
        Replacement {
            event: "a18",
            content: PROSE,
            provenance: at(18),
        }
    }

    /// The one nomination the acceptance drive produces, reconciled against
    /// `text`.
    ///
    /// SHARED BY THE TESTS BELOW, AND THAT IS THE POINT. Two tests used to
    /// carry every assertion about this module between them -- three about a
    /// supersession and four about the other three verdicts -- so the seven
    /// seeded faults that prove those rules broke two tests between them and
    /// could be told apart only by which `assert_eq!` message came back. That
    /// is prose lifted out of a panic, which is the staleness #46 exists to
    /// end. Split, each fault breaks a test of its own and cargo's own
    /// `test <path> ... FAILED` line is the class.
    fn reconciled(object: &WorkingObject, text: &str) -> Outcome {
        let nominations = nominate(
            object,
            NewText {
                turn: 18,
                prose: PROSE,
                tool_output: "",
            },
        );
        assert_eq!(nominations.len(), 1, "the pair was not nominated");
        let answer = verdict::parse(text).expect("a verdict");
        reconcile(&nominations[0], &answer, replacement()).expect("reconciled")
    }

    /// The acceptance drive with its supersession applied.
    fn superseded_object() -> WorkingObject {
        let mut object = object_with_stale_entry();
        let outcome = reconciled(&object, "SUPERSEDED: the CLI has the verb now");
        let Outcome::Superseded(patch) = &outcome else {
            panic!("a SUPERSEDED verdict must supersede: {outcome:?}");
        };
        object.apply(patch).expect("applied");
        object
    }

    fn superseding_entry(object: &WorkingObject) -> &crate::object::Entry {
        object
            .entry(&EntryId::new("a18/supersedes/e1").expect("id"))
            .expect("the new entry")
    }

    #[test]
    fn a_superseded_verdict_voids_the_old_entry_and_links_it_rather_than_deleting() {
        let object = superseded_object();
        let old = object
            .entry(&EntryId::new("e1").expect("id"))
            .expect("the old entry is still in the object");
        assert!(
            matches!(old.state, EntryState::Voided { .. }),
            "the old entry was not voided: {:?}",
            old.state
        );
        assert_eq!(
            superseding_entry(&object)
                .supersedes
                .as_ref()
                .map(EntryId::as_str),
            Some("e1"),
            "a supersession that did not link the entry it voided"
        );
        assert_eq!(object.entries().count(), 2, "an entry was deleted");
        assert_eq!(object.live().count(), 1);
    }

    /// A link to an entry whose text is not the fact that replaced it is the
    /// silent loss this module exists to prevent, wearing the shape of a
    /// repair. What the new entry says is the nominating prose.
    #[test]
    fn a_superseding_entry_says_what_superseded_the_old_one() {
        let object = superseded_object();
        assert_eq!(
            superseding_entry(&object).content,
            PROSE,
            "the superseding entry does not say what superseded the old one"
        );
    }

    /// And where it came from is the fork that judged: without the
    /// provenance, nothing can attribute a wrong supersession to the turn
    /// that made it.
    #[test]
    fn a_superseding_entry_says_which_fork_produced_it() {
        let object = superseded_object();
        assert_eq!(
            superseding_entry(&object).provenances,
            vec![at(18)],
            "the superseding entry does not say which fork produced it"
        );
    }

    /// The accessor is how a caller gets at what the reconciler produced. One
    /// that answered `None` for every verdict would leave the whole module
    /// applying nothing, with the four outcomes still distinct and every
    /// assertion about them still true.
    #[test]
    fn a_done_verdict_resolves_the_entry_and_hands_its_patch_back() {
        let mut object = object_with_stale_entry();
        let done = reconciled(&object, "DONE");
        let Outcome::Resolved(patch) = &done else {
            panic!("{done:?}")
        };
        assert_eq!(
            done.patch(),
            Some(patch),
            "a verdict that produced a patch did not hand it back"
        );
        object.apply(patch).expect("applied");
        assert_eq!(
            object
                .entry(&EntryId::new("e1").expect("id"))
                .map(|e| e.state.clone()),
            Some(EntryState::Resolved)
        );
    }

    /// `PARTIAL` says the prose bears on the entry without replacing it; a
    /// reconciler that resolves on it closes an entry the fork deliberately
    /// left open.
    #[test]
    fn a_partial_verdict_applies_nothing() {
        let mut object = object_with_stale_entry();
        let before = object.dump();
        let outcome = reconciled(&object, "PARTIAL");
        assert!(
            outcome.patch().is_none(),
            "PARTIAL produced a patch: {outcome:?}"
        );
        if let Some(patch) = outcome.patch() {
            object.apply(patch).expect("never reached");
        }
        assert_eq!(
            object.dump(),
            before,
            "a verdict that applies nothing changed the object"
        );
    }

    /// A mention is not a reversal, and the verdict is the only thing that
    /// tells them apart.
    #[test]
    fn a_not_this_verdict_applies_nothing() {
        let mut object = object_with_stale_entry();
        let before = object.dump();
        let outcome = reconciled(&object, "NOT_THIS");
        assert!(
            outcome.patch().is_none(),
            "NOT_THIS produced a patch: {outcome:?}"
        );
        assert_eq!(
            outcome,
            Outcome::NotThis,
            "a fork that said the nomination was wrong was read as something else"
        );
        if let Some(patch) = outcome.patch() {
            object.apply(patch).expect("never reached");
        }
        assert_eq!(
            object.dump(),
            before,
            "a verdict that applies nothing changed the object"
        );
    }

    /// The two are not one answer. `NOT_THIS` is the false nomination the
    /// precision gate is calibrated against, and `PARTIAL` is prose that
    /// bears on the entry without settling it; counting a PARTIAL as a false
    /// nomination would inflate the very number the gate reads.
    ///
    /// Asserted as the inequality it is, rather than as `PARTIAL == Partial`:
    /// the rule this test exists for is that the two verdicts do not collapse
    /// into one outcome, and a `PARTIAL == Partial` spelling is also tripped
    /// by `inject_reconcile_partial_applies_a_patch`, which
    /// `a_partial_verdict_applies_nothing` above already owns. `PARTIAL`
    /// applies nothing, `NOT_THIS` is `NotThis`, and the two differ -- which
    /// pins `PARTIAL` to `Partial` across the three tests without either
    /// fault breaking the other's.
    #[test]
    fn a_partial_is_not_a_false_nomination() {
        let object = object_with_stale_entry();
        assert_ne!(
            reconciled(&object, "PARTIAL"),
            reconciled(&object, "NOT_THIS"),
            "a fork that said the prose bears on the entry was read as a false nomination"
        );
    }
}
