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
    use crate::formats::record::{Reasoning, Regime, Substrate};
    use crate::formats::verdict;
    use crate::object::{EntryId, EntryState, Patch, Provenance, WorkingObject};

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

    /// Where a verdict came from. The confirm fork is an interview fork --
    /// a single ask with a constrained answer -- so it carries the lane that
    /// already exists rather than a name coined here for it.
    fn at(turn: u32) -> Provenance {
        Provenance {
            turn,
            lane: "interview".to_owned(),
            fork: Some("f1".to_owned()),
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
                    index: 0,
                },
            })
            .expect("added");
        object
    }

    const PROSE: &str = "Actually, check_record exists now: the CLI gained it.";

    #[test]
    fn a_superseded_verdict_links_and_voids_rather_than_deleting() {
        let mut object = object_with_stale_entry();
        let nominations = nominate(
            &object,
            NewText {
                turn: 18,
                prose: PROSE,
                tool_output: "",
            },
        );
        assert_eq!(nominations.len(), 1, "the pair was not nominated");
        let answer = verdict::parse("SUPERSEDED: the CLI has the verb now").expect("a verdict");
        let outcome = reconcile(
            &nominations[0],
            &answer,
            Replacement {
                event: "a18",
                content: PROSE,
                provenance: at(18),
            },
        )
        .expect("reconciled");
        let Outcome::Superseded(patch) = &outcome else {
            panic!("a SUPERSEDED verdict must supersede: {outcome:?}");
        };
        object.apply(patch).expect("applied");
        let old = object
            .entry(&EntryId::new("e1").expect("id"))
            .expect("the old entry is still in the object");
        assert!(
            matches!(old.state, EntryState::Voided { .. }),
            "the old entry was not voided: {:?}",
            old.state
        );
        let new = object
            .entry(&EntryId::new("a18/supersedes/e1").expect("id"))
            .expect("the new entry");
        assert_eq!(
            new.supersedes.as_ref().map(EntryId::as_str),
            Some("e1"),
            "a supersession that did not link the entry it voided"
        );
        // A link to an entry whose text is not the fact that replaced it is
        // the silent loss this module exists to prevent, wearing the shape
        // of a repair. What the new entry says is the nominating prose, and
        // where it came from is the fork that judged.
        assert_eq!(
            new.content, PROSE,
            "the superseding entry does not say what superseded the old one"
        );
        assert_eq!(
            new.provenances,
            vec![at(18)],
            "the superseding entry does not say which fork produced it"
        );
        assert_eq!(object.entries().count(), 2, "an entry was deleted");
        assert_eq!(object.live().count(), 1);
    }

    #[test]
    fn done_resolves_and_the_other_two_apply_nothing() {
        let mut object = object_with_stale_entry();
        let nominations = nominate(
            &object,
            NewText {
                turn: 18,
                prose: PROSE,
                tool_output: "",
            },
        );
        let replacement = || Replacement {
            event: "a18",
            content: PROSE,
            provenance: at(18),
        };
        let done = reconcile(
            &nominations[0],
            &verdict::parse("DONE").expect("v"),
            replacement(),
        )
        .expect("ok");
        let Outcome::Resolved(patch) = &done else {
            panic!("{done:?}")
        };
        // The accessor is how a caller gets at what the reconciler produced.
        // One that answered `None` for every verdict would leave the whole
        // module applying nothing, with the four outcomes still distinct and
        // every assertion about them still true.
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

        let mut untouched = object_with_stale_entry();
        let before = untouched.dump();
        for text in ["PARTIAL", "NOT_THIS"] {
            let outcome = reconcile(
                &nominations[0],
                &verdict::parse(text).expect("v"),
                replacement(),
            )
            .expect("ok");
            assert!(
                outcome.patch().is_none(),
                "{text} produced a patch: {outcome:?}"
            );
            if let Some(patch) = outcome.patch() {
                untouched.apply(patch).expect("never reached");
            }
        }
        assert_eq!(
            untouched.dump(),
            before,
            "a verdict that applies nothing changed the object"
        );
        // The two are not one answer. `NOT_THIS` is the false nomination the
        // precision gate is calibrated against, and `PARTIAL` is prose that
        // bears on the entry without settling it; counting a PARTIAL as a
        // false nomination would inflate the very number the gate reads.
        assert_eq!(
            reconcile(
                &nominations[0],
                &verdict::parse("NOT_THIS").expect("v"),
                replacement()
            )
            .expect("ok"),
            Outcome::NotThis,
            "a fork that said the nomination was wrong was read as something else"
        );
        assert_eq!(
            reconcile(
                &nominations[0],
                &verdict::parse("PARTIAL").expect("v"),
                replacement()
            )
            .expect("ok"),
            Outcome::Partial,
            "a fork that said the prose bears on the entry was read as a false nomination"
        );
    }
}
