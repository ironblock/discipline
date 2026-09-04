//! Ending a tangent: prefix rollback, and a disposition scoped by provenance.
//!
//! A tree-shaped harness ends an exploratory branch by summarising the
//! branch's transcript and folding the summary back into the parent -- "the
//! user explored another branch before returning here." That is the right
//! answer to the problem those harnesses have, which is transcript
//! economics: the parent must not pay for the branch's tokens. It is the
//! wrong answer here, because there is no transcript to fold. A tangent's
//! value was already captured turn by turn into the working object, and
//! summarising it back would be a lossy re-encode of something the object
//! already holds losslessly.
//!
//! So ending a tangent needs no compaction at all. It decomposes into two
//! operations the object already owns:
//!
//! * **Prefix rollback is free.** The trunk was never touched, so returning
//!   the canonical prefix to the fork point is not an operation -- it is an
//!   observation. [`Closed::prefix_intact`] is that observation, made by
//!   comparing the bytes recorded at [`Tangent::open`] against the bytes the
//!   trunk renders now. A claim that the trunk is untouched, asserted rather
//!   than compared, is a claim about the design and not about the run.
//! * **Disposition is scoped, and total.** Every entry born under the
//!   tangent gets [`Disposition::Keep`], [`Disposition::Drop`] or
//!   [`Disposition::Park`], and [`Tangent::close`] refuses a closure that
//!   leaves one out. A branch half-closed is worse than one never closed:
//!   the entries nobody ruled on stay live and the trunk silently inherits
//!   whatever the tangent was exploring.
//!
//! **Scope is provenance, never recency.** Deciding that an entry belongs to
//! the tangent because its turn is at or after the fork turn is the obvious
//! reading and it is wrong: the trunk goes on writing while a tangent runs,
//! and every fact it records after the fork would be swept into the
//! tangent's scope and retired by a closure that never created it. The
//! tangent is stamped into [`super::Provenance::tangent`] at birth by
//! [`Tangent::provenance`], and closure reads that.
//!
//! **Nothing here deletes.** `Drop` is eviction to the archive: the entry is
//! marked [`super::EntryState::Retired`] and stays in
//! [`super::WorkingObject::entries`]. `Park` is
//! [`super::EntryState::Parked`]: retained, out of the live set, and marked
//! as the tangent's rather than the trunk's, so a later reader can tell a
//! fact that stopped mattering from one that never belonged here.
//!
//! Out of scope, and named so nobody has to work it out: **who chooses**.
//! The disposition map is the seam. An agent nominating and a person
//! ratifying are the same machinery with a different chooser -- which is
//! what makes that comparison nearly free -- so this module takes the map
//! and asks nothing about where it came from. Turning a written verdict into
//! a disposition is the collector's job and needs the verdict grammar, which
//! does not exist yet.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use super::{Entry, EntryId, ObjectError, Patch, Provenance, WorkingObject};

/// The lane a closure files its own patches under.
///
/// The canonical lane, not a lane of the tangent's own. A tangent is a scope
/// over the object, not a producer of content, and coining a lane name for
/// it would put a second name in the record for something the record already
/// says: the patches carry the tangent in their provenance.
const CLOSING_LANE: &str = "main";

/// What the closure does with an entry the tangent created.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Disposition {
    /// The trunk needs it. It stays live.
    Keep,
    /// Evict it to the archive. It is marked retired and kept.
    Drop,
    /// Retain it as the tangent's. It leaves the live set and is marked
    /// parked, so reopening the tangent has something to reopen.
    Park,
}

impl Disposition {
    /// Every disposition, so a closure cannot forget one and a test that
    /// enumerates them cannot go stale when one is added.
    pub const ALL: &'static [Self] = &[Self::Keep, Self::Drop, Self::Park];

    /// The spelling a report and an error message use.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Drop => "drop",
            Self::Park => "park",
        }
    }
}

/// An open tangent: a scope over the object, and the fork point it can be
/// measured against.
///
/// Holds no entries of its own. The entries are the object's, and what makes
/// them the tangent's is the id this stamps into their provenance -- which
/// is why a tangent can be dropped and reopened from the record without
/// losing anything, and why closure does not depend on this value having
/// been kept alive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tangent {
    id: String,
    at_turn: u32,
    at_version: u32,
    prefix_at_open: String,
}

/// What a closure did to the trunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Closed {
    /// Whether the canonical prefix is byte-identical to the fork point.
    ///
    /// The comparison is over the dump of live entries **not** born in the
    /// tangent, so the tangent's own rows are not counted against it and the
    /// version header -- which moves on every patch -- is not either.
    ///
    /// False is a finding, not a failure: a tangent that superseded a trunk
    /// entry, or that restated one and so recorded a second provenance on
    /// it, moved the trunk. Rollback is free only for a tangent that left
    /// the trunk alone, and this is how a caller finds out which kind it
    /// had.
    pub prefix_intact: bool,
}

/// Why a tangent could not be opened or closed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TangentError {
    /// A tangent id that is blank. It would stamp an empty string into every
    /// provenance it made, which reads as a scope and names none.
    EmptyId,
    /// A tangent id the record already carries. Reopening an id would put
    /// the earlier tangent's entries into this one's scope, and a closure
    /// would rule on facts it never created.
    IdInUse {
        /// The id.
        id: String,
    },
    /// The closure names an entry the tangent did not create. Scope is by
    /// provenance: an entry the trunk wrote after the fork is the trunk's.
    NotInScope {
        /// The entry.
        id: EntryId,
        /// What the closure asked for.
        disposition: Disposition,
    },
    /// An entry born in the tangent that the closure does not rule on.
    /// Closure is total, or the trunk inherits what nobody decided.
    Undisposed {
        /// The entry.
        id: EntryId,
    },
    /// The object refused a patch the closure emitted.
    ///
    /// The object is the authority on whether a patch applies, and a closure
    /// that swallowed that refusal would report a disposition it did not
    /// make. No closure the scope rules admit reaches this today; it is here
    /// rather than an `expect` because the day one does, the caller should
    /// be told which entry and why.
    Refused {
        /// What the object said.
        error: ObjectError,
    },
}

impl fmt::Display for TangentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => write!(f, "a tangent id that is blank"),
            Self::IdInUse { id } => write!(
                f,
                "the record already holds entries born under tangent `{id}`, so a \
                 second tangent under that id would inherit their disposition"
            ),
            Self::NotInScope { id, disposition } => write!(
                f,
                "the closure asks to {} entry `{id}`, which was not born in this \
                 tangent; scope is by provenance, not by recency",
                disposition.tag()
            ),
            Self::Undisposed { id } => write!(
                f,
                "entry `{id}` was born in this tangent and the closure rules on it \
                 nowhere; a half-closed tangent leaves the trunk what nobody decided"
            ),
            Self::Refused { error } => {
                write!(f, "the object refused a patch the closure emitted: {error}")
            }
        }
    }
}

impl Error for TangentError {}

impl Tangent {
    /// Open a tangent over `object` at turn `at_turn`, under `id`.
    ///
    /// Records the fork point: the version, and the bytes the trunk renders
    /// now. The bytes are the prefix rather than the whole dump because the
    /// dump's header carries the version, which moves on every patch the
    /// tangent makes -- a fork point recorded as the whole dump could never
    /// compare equal again and would report every tangent as having moved
    /// the trunk.
    ///
    /// # Errors
    ///
    /// [`TangentError::EmptyId`] for a blank id, and
    /// [`TangentError::IdInUse`] when the object already holds an entry born
    /// under that id.
    pub fn open(object: &WorkingObject, id: &str, at_turn: u32) -> Result<Self, TangentError> {
        if id.trim().is_empty() {
            return Err(TangentError::EmptyId);
        }
        if object.entries().any(|entry| born_under(entry) == Some(id)) {
            return Err(TangentError::IdInUse { id: id.to_owned() });
        }
        Ok(Self {
            id: id.to_owned(),
            at_turn,
            at_version: object.version(),
            prefix_at_open: trunk_prefix(object, id),
        })
    }

    /// The tangent's id, as it is stamped into provenance.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The turn the tangent forked at.
    #[must_use]
    pub fn at_turn(&self) -> u32 {
        self.at_turn
    }

    /// The object's version at the fork point.
    #[must_use]
    pub fn at_version(&self) -> u32 {
        self.at_version
    }

    /// The fork point's rendered prefix, kept so closure can compare against
    /// it rather than argue.
    #[must_use]
    pub fn prefix_at_open(&self) -> &str {
        &self.prefix_at_open
    }

    /// A provenance for a patch made under this tangent.
    ///
    /// The only way to build one: a lane working inside a tangent gets its
    /// provenance from the tangent, so an entry born there carries the scope
    /// by construction and cannot be attributed to the tangent afterwards by
    /// anything as fragile as its turn.
    #[must_use]
    pub fn provenance(&self, turn: u32, lane: &str, fork: Option<&str>, index: u32) -> Provenance {
        Provenance {
            turn,
            lane: lane.to_owned(),
            fork: fork.map(str::to_owned),
            tangent: Some(self.id.clone()),
            index,
        }
    }

    /// Every live entry born under this tangent, in id order.
    ///
    /// What a chooser walks to build the disposition map, and exactly what
    /// [`Tangent::close`] requires that map to cover. An entry already voided,
    /// resolved or retired inside the tangent has been ruled on by the record
    /// and is not offered again.
    #[must_use]
    pub fn scope<'o>(&self, object: &'o WorkingObject) -> Vec<&'o Entry> {
        object
            .entries()
            .filter(|entry| entry.state.is_live() && born_under(entry) == Some(self.id.as_str()))
            .collect()
    }

    /// Close the tangent, applying `dispositions` as one turn at `at_turn`.
    ///
    /// `at_turn` is a parameter rather than the fork turn: closure happens
    /// later than the fork, and filing its patches at the turn the tangent
    /// opened would put a ruling in the record before the facts it ruled on.
    ///
    /// Atomic: the dispositions are checked whole before any patch is
    /// applied, and the patches go through [`WorkingObject::apply_turn`],
    /// which leaves the object as it was if it refuses one.
    ///
    /// # Errors
    ///
    /// [`TangentError::NotInScope`] for an id the tangent did not create,
    /// [`TangentError::Undisposed`] for one it created that the map does not
    /// name, and [`TangentError::Refused`] if the object refuses a patch.
    pub fn close(
        &self,
        object: &mut WorkingObject,
        at_turn: u32,
        dispositions: &BTreeMap<EntryId, Disposition>,
    ) -> Result<Closed, TangentError> {
        let scope: BTreeSet<EntryId> = self
            .scope(object)
            .into_iter()
            .map(|entry| entry.id.clone())
            .collect();
        // Both directions, before anything is written. One direction alone
        // is half a closure: refusing what is out of scope still lets an
        // entry the tangent created go unruled, and requiring every entry to
        // be ruled on still lets a ruling land on the trunk.
        if let Some((id, disposition)) = dispositions.iter().find(|(id, _)| !scope.contains(id)) {
            return Err(TangentError::NotInScope {
                id: id.clone(),
                disposition: *disposition,
            });
        }
        if let Some(id) = scope.iter().find(|id| !dispositions.contains_key(id)) {
            return Err(TangentError::Undisposed { id: id.clone() });
        }

        let mut patches = Vec::new();
        let mut index: u32 = 0;
        for (id, disposition) in dispositions {
            let patch = match disposition {
                Disposition::Keep => continue,
                Disposition::Drop => Patch::Retire {
                    target: id.clone(),
                    provenance: self.provenance(at_turn, CLOSING_LANE, None, index),
                },
                Disposition::Park => Patch::Park {
                    target: id.clone(),
                    provenance: self.provenance(at_turn, CLOSING_LANE, None, index),
                },
            };
            index = index.saturating_add(1);
            patches.push(patch);
        }
        object
            .apply_turn(&patches)
            .map_err(|error| TangentError::Refused { error })?;

        Ok(Closed {
            prefix_intact: trunk_prefix(object, &self.id) == self.prefix_at_open,
        })
    }
}

/// The tangent an entry was born under, if it was born under one.
///
/// The **first** provenance, not any of them. A tangent that restates a fact
/// the trunk already holds does not create an entry -- dedup folds it into
/// the trunk's, recording a second provenance there. Reading "born here" off
/// the whole list would put that trunk entry into the tangent's scope and let
/// a closure retire a fact the tangent only agreed with.
fn born_under(entry: &Entry) -> Option<&str> {
    entry
        .provenances
        .first()
        .and_then(|provenance| provenance.tangent.as_deref())
}

/// The trunk's rendered prefix: the dump lines of live entries not born in
/// `tangent`, in id order.
///
/// No header. The header carries the version, which advances on every patch,
/// so including it would make the comparison a test of whether anything
/// happened at all rather than of whether the trunk moved.
fn trunk_prefix(object: &WorkingObject, tangent: &str) -> String {
    let mut out = String::new();
    for (id, line) in object.dump_lines() {
        let Some(entry) = object.entry(&id) else {
            continue;
        };
        if !entry.state.is_live() || born_under(entry) == Some(tangent) {
            continue;
        }
        out.push_str(&line);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::{Closed, Disposition, Tangent, TangentError};
    use crate::formats::record::{self, Regime};
    use crate::object::{EntryId, EntryState, ObjectError, Patch, Provenance, WorkingObject};

    /// A regime parsed through the record schema, so the object cannot be
    /// opened under one the record would reject.
    fn regime() -> Regime {
        let source = r#"{"record":"start","regime":{"arm":"baseline","dogma_version":0,"substrate":{"name":"local","model":"m","quantization":"q","sampler":{"seed":0},"reasoning":"on","hardware":"h"}}}"#;
        record::parse(source).expect("a record").regime().clone()
    }

    fn object() -> WorkingObject {
        WorkingObject::open(regime())
    }

    fn id(text: &str) -> EntryId {
        EntryId::new(text).expect("a well-formed id")
    }

    /// A trunk provenance: no tangent.
    fn trunk(turn: u32, index: u32) -> Provenance {
        Provenance {
            turn,
            lane: "interview".to_owned(),
            fork: None,
            tangent: None,
            index,
        }
    }

    fn add(id_text: &str, content: &str, provenance: Provenance) -> Patch {
        Patch::Add {
            id: id(id_text),
            content: content.to_owned(),
            provenance,
        }
    }

    fn dispositions(rows: &[(&str, Disposition)]) -> BTreeMap<EntryId, Disposition> {
        rows.iter()
            .map(|(text, disposition)| (id(text), *disposition))
            .collect()
    }

    fn state_of(object: &WorkingObject, text: &str) -> EntryState {
        object
            .entry(&id(text))
            .unwrap_or_else(|| panic!("the object no longer holds `{text}`"))
            .state
            .clone()
    }

    fn live_ids(object: &WorkingObject) -> Vec<String> {
        object
            .live()
            .map(|entry| entry.id.as_str().to_owned())
            .collect()
    }

    /// A trunk of two facts, and a tangent forked over it at turn 3.
    fn forked() -> (WorkingObject, Tangent) {
        let mut object = object();
        object
            .apply(&add(
                "trunk-a",
                "the resolver refuses a stale binary",
                trunk(1, 0),
            ))
            .expect("a trunk fact");
        object
            .apply(&add(
                "trunk-b",
                "the regime is fixed for the session",
                trunk(2, 0),
            ))
            .expect("another trunk fact");
        let tangent = Tangent::open(&object, "t-cache", 3).expect("a tangent");
        (object, tangent)
    }

    // Acceptance: five turns on a tangent producing four entries, closed with
    // KEEP, DROP, PARK, KEEP. The rendered object shows exactly the two kept,
    // the archive retains all four, and the prefix is byte-identical to the
    // fork point.
    #[test]
    fn closing_a_tangent_keeps_what_the_trunk_needs_and_archives_the_rest() {
        let (mut object, tangent) = forked();
        let fork_prefix = tangent.prefix_at_open().to_owned();

        // Five turns, 3 through 7. Turn 6 produced no entry, which is the
        // ordinary case and the reason turns and entries are counted apart.
        for (turn, entry, content) in [
            (3, "t1", "the cache is keyed by the prompt prefix"),
            (4, "t2", "the second request reused the prefix"),
            (5, "t3", "the eviction policy is least recently used"),
            (7, "t4", "a warm fork costs one forward pass"),
        ] {
            object
                .apply(&add(
                    entry,
                    content,
                    tangent.provenance(turn, "interview", Some("f1"), 0),
                ))
                .unwrap_or_else(|err| panic!("turn {turn} on the tangent: {err}"));
        }
        assert_eq!(
            tangent
                .scope(&object)
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["t1", "t2", "t3", "t4"],
            "the tangent's scope is not the four entries it created"
        );

        let closed = tangent
            .close(
                &mut object,
                8,
                &dispositions(&[
                    ("t1", Disposition::Keep),
                    ("t2", Disposition::Drop),
                    ("t3", Disposition::Park),
                    ("t4", Disposition::Keep),
                ]),
            )
            .expect("a total closure");

        assert_eq!(
            object.entries().count(),
            6,
            "the archive does not hold every entry the tangent created"
        );
        assert_eq!(
            live_ids(&object),
            vec!["t1", "t4", "trunk-a", "trunk-b"],
            "the live set after closure is not the trunk plus exactly what it kept"
        );
        assert_eq!(state_of(&object, "t1"), EntryState::Live);
        assert_eq!(state_of(&object, "t2"), EntryState::Retired);
        assert_eq!(state_of(&object, "t3"), EntryState::Parked);
        assert_eq!(state_of(&object, "t4"), EntryState::Live);
        assert_eq!(
            closed,
            Closed {
                prefix_intact: true
            },
            "the trunk moved while a tangent that never touched it ran"
        );
        assert_eq!(
            super::trunk_prefix(&object, tangent.id()),
            fork_prefix,
            "the prefix is not byte-identical to the fork point"
        );
        assert!(
            !fork_prefix.is_empty(),
            "the fork point rendered nothing, so comparing against it proves nothing"
        );
    }

    #[test]
    fn a_dropped_entry_is_evicted_to_the_archive_and_never_deleted() {
        let (mut object, tangent) = forked();
        object
            .apply(&add(
                "t1",
                "the eviction policy is least recently used",
                tangent.provenance(3, "interview", None, 0),
            ))
            .expect("a fact on the tangent");
        tangent
            .close(&mut object, 4, &dispositions(&[("t1", Disposition::Drop)]))
            .expect("a total closure");

        let dropped = object
            .entry(&id("t1"))
            .expect("a drop evicts to the archive and never deletes");
        assert_eq!(
            dropped.state,
            EntryState::Retired,
            "a drop evicts to the archive and never deletes"
        );
        assert_eq!(
            dropped.content, "the eviction policy is least recently used",
            "the archived entry no longer says what it said"
        );
        assert_eq!(object.entries().count(), 3);
    }

    #[test]
    fn a_parked_entry_is_retained_and_stops_speaking_for_the_object() {
        let (mut object, tangent) = forked();
        object
            .apply(&add(
                "t1",
                "the eviction policy is least recently used",
                tangent.provenance(3, "interview", None, 0),
            ))
            .expect("a fact on the tangent");
        tangent
            .close(&mut object, 4, &dispositions(&[("t1", Disposition::Park)]))
            .expect("a total closure");

        assert!(
            !live_ids(&object).contains(&"t1".to_owned()),
            "a parked entry still speaks for the object"
        );
        assert_eq!(
            state_of(&object, "t1"),
            EntryState::Parked,
            "a parked entry is not marked as the tangent's"
        );
        assert_ne!(
            state_of(&object, "t1"),
            EntryState::Retired,
            "parked and retired collapsed into one state, so reopening the \
             tangent has nothing to tell them apart by"
        );
    }

    // Scope is by provenance. The trunk goes on writing while a tangent runs,
    // and a closure that swept those entries up by their turn would retire
    // facts it never created.
    #[test]
    fn an_entry_the_trunk_created_after_the_fork_is_not_in_the_tangents_scope() {
        let (mut object, tangent) = forked();
        object
            .apply(&add(
                "t1",
                "the eviction policy is least recently used",
                tangent.provenance(4, "interview", None, 0),
            ))
            .expect("a fact on the tangent");
        object
            .apply(&add(
                "trunk-late",
                "the gate runs twelve checks",
                trunk(5, 0),
            ))
            .expect("the trunk went on writing");

        assert_eq!(
            tangent
                .scope(&object)
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["t1"],
            "scope is by provenance, not by recency"
        );
        assert_eq!(
            tangent.close(
                &mut object,
                6,
                &dispositions(&[("t1", Disposition::Keep), ("trunk-late", Disposition::Drop),]),
            ),
            Err(TangentError::NotInScope {
                id: id("trunk-late"),
                disposition: Disposition::Drop,
            }),
            "scope is by provenance, not by recency"
        );
        assert_eq!(
            state_of(&object, "trunk-late"),
            EntryState::Live,
            "a refused closure retired a trunk entry anyway"
        );
    }

    // Dedup is the other half of the same rule: a tangent that agrees with a
    // fact the trunk already holds creates nothing, so there is nothing of
    // its own to dispose of.
    #[test]
    fn a_fact_the_trunk_already_held_is_not_born_in_the_tangent_that_restated_it() {
        let (mut object, tangent) = forked();
        object
            .apply(&add(
                "t1",
                "the resolver   refuses a stale\nbinary",
                tangent.provenance(4, "interview", None, 0),
            ))
            .expect("the tangent agreed with the trunk");

        assert!(
            tangent.scope(&object).is_empty(),
            "a fact the trunk already held was counted as born in the tangent"
        );
        assert_eq!(
            tangent.close(&mut object, 5, &dispositions(&[("t1", Disposition::Drop)]),),
            Err(TangentError::NotInScope {
                id: id("t1"),
                disposition: Disposition::Drop,
            }),
            "a closure could retire a trunk fact by naming the alias a tangent \
             left on it"
        );
    }

    #[test]
    fn a_tangent_born_entry_left_undisposed_refuses_the_closure() {
        let (mut object, tangent) = forked();
        for (entry, content) in [
            ("t1", "the cache is keyed by the prompt prefix"),
            ("t2", "the eviction policy is least recently used"),
        ] {
            object
                .apply(&add(
                    entry,
                    content,
                    tangent.provenance(4, "interview", Some(entry), 0),
                ))
                .expect("a fact on the tangent");
        }

        assert_eq!(
            tangent.close(&mut object, 5, &dispositions(&[("t1", Disposition::Keep)])),
            Err(TangentError::Undisposed { id: id("t2") }),
            "closure is total: a tangent-born entry was left undisposed"
        );
        assert_eq!(
            state_of(&object, "t1"),
            EntryState::Live,
            "a refused closure moved an entry anyway"
        );
    }

    // The prefix is compared, not asserted. A tangent that corrects a trunk
    // entry has moved the trunk, and rollback is not free for it.
    #[test]
    fn a_tangent_that_touched_the_trunk_says_the_prefix_moved() {
        let (mut object, tangent) = forked();
        object
            .apply(&Patch::Supersede {
                id: id("t1"),
                content: "the resolver refuses a stale binary and says which".to_owned(),
                voids: id("trunk-a"),
                provenance: tangent.provenance(4, "interview", None, 0),
            })
            .expect("the tangent corrected the trunk");

        let closed = tangent
            .close(&mut object, 5, &dispositions(&[("t1", Disposition::Drop)]))
            .expect("a total closure");
        assert!(
            !closed.prefix_intact,
            "the prefix was reported intact after the trunk moved"
        );
        assert_eq!(
            state_of(&object, "trunk-a"),
            EntryState::Voided { by: id("t1") },
            "the correction was unwound, and nothing here unwinds"
        );
    }

    #[test]
    fn a_tangent_id_the_record_already_carries_is_refused() {
        let (mut object, tangent) = forked();
        object
            .apply(&add(
                "t1",
                "the eviction policy is least recently used",
                tangent.provenance(4, "interview", None, 0),
            ))
            .expect("a fact on the tangent");
        tangent
            .close(&mut object, 5, &dispositions(&[("t1", Disposition::Park)]))
            .expect("a total closure");

        assert_eq!(
            Tangent::open(&object, "t-cache", 6),
            Err(TangentError::IdInUse {
                id: "t-cache".to_owned(),
            }),
            "a tangent id the record already carries was opened again"
        );
    }

    #[test]
    fn a_tangent_id_that_says_nothing_is_refused() {
        let object = object();
        for blank in ["", "   ", "\t"] {
            assert_eq!(
                Tangent::open(&object, blank, 1),
                Err(TangentError::EmptyId),
                "a blank tangent id would stamp a scope that names nothing"
            );
        }
    }

    // Every disposition, from the list, so one added without a rule here
    // fails rather than falling through to whatever the closure does by
    // default.
    #[test]
    fn every_disposition_is_total_over_a_tangent_born_entry() {
        for disposition in Disposition::ALL {
            let (mut object, tangent) = forked();
            object
                .apply(&add(
                    "t1",
                    "the eviction policy is least recently used",
                    tangent.provenance(4, "interview", None, 0),
                ))
                .expect("a fact on the tangent");
            tangent
                .close(&mut object, 5, &dispositions(&[("t1", *disposition)]))
                .unwrap_or_else(|err| panic!("{} was refused: {err}", disposition.tag()));

            let expected = match disposition {
                Disposition::Keep => EntryState::Live,
                Disposition::Drop => EntryState::Retired,
                Disposition::Park => EntryState::Parked,
            };
            assert_eq!(
                state_of(&object, "t1"),
                expected,
                "{} left the entry in the wrong state",
                disposition.tag()
            );
            assert_eq!(
                object.entries().count(),
                3,
                "{} lost an entry: the archive keeps everything",
                disposition.tag()
            );
        }
    }

    #[test]
    fn a_tangent_stamps_the_provenance_of_every_patch_made_under_it() {
        let (mut object, tangent) = forked();
        let provenance = tangent.provenance(4, "interview", Some("f2"), 3);
        assert_eq!(provenance.tangent.as_deref(), Some("t-cache"));
        assert_eq!(provenance.lane, "interview");
        assert_eq!(provenance.fork.as_deref(), Some("f2"));
        assert_eq!(provenance.index, 3);
        object
            .apply(&add(
                "t1",
                "the eviction policy is least recently used",
                provenance,
            ))
            .expect("a fact on the tangent");
        assert!(
            object.dump().contains(r#""tangent":"t-cache""#),
            "the tangent a patch was made under is not in the dump: {}",
            object.dump()
        );
        // And the closure's own patches say which tangent they closed.
        tangent
            .close(&mut object, 5, &dispositions(&[("t1", Disposition::Park)]))
            .expect("a total closure");
        assert!(
            object
                .entry(&id("t1"))
                .expect("the parked entry")
                .provenances
                .iter()
                .all(|p| p.tangent.as_deref() == Some("t-cache")),
            "a closure patch does not say which tangent it closed"
        );
    }

    // `born_under` reads the first provenance and calls it the birth. This is
    // what makes that true.
    #[test]
    fn the_first_provenance_is_the_one_that_created_the_entry() {
        let (mut object, tangent) = forked();
        let birth = tangent.provenance(4, "interview", Some("f1"), 0);
        object
            .apply(&add(
                "t1",
                "the eviction policy is least recently used",
                birth.clone(),
            ))
            .expect("a fact on the tangent");
        // A second lane, on the trunk, says the same thing. Dedup records its
        // provenance on the entry the tangent created.
        object
            .apply(&add(
                "t1-again",
                "the eviction policy is least recently used",
                trunk(5, 0),
            ))
            .expect("the trunk agreed");

        let entry = object.entry(&id("t1")).expect("the entry");
        assert_eq!(
            entry.provenances.first(),
            Some(&birth),
            "the first provenance is not the one that created the entry, so \
             `born_under` reads whichever lane happened to speak last"
        );
        assert_eq!(entry.provenances.len(), 2);
        assert_eq!(
            tangent
                .scope(&object)
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>(),
            vec!["t1"],
            "a later provenance moved an entry out of the scope it was born in"
        );
    }

    // Every variant, rendered. An error nobody can read is a refusal the
    // caller reconstructs from the type name, and `Refused` in particular has
    // no other test: the scope rules admit no closure that reaches it.
    #[test]
    fn every_tangent_error_says_what_it_is_about() {
        for (error, expected) in [
            (TangentError::EmptyId, vec!["blank"]),
            (
                TangentError::IdInUse {
                    id: "t-cache".to_owned(),
                },
                vec!["t-cache"],
            ),
            (
                TangentError::NotInScope {
                    id: id("trunk-late"),
                    disposition: Disposition::Drop,
                },
                vec!["trunk-late", "drop", "provenance"],
            ),
            (
                TangentError::Undisposed { id: id("t2") },
                vec!["t2", "nowhere"],
            ),
            (
                TangentError::Refused {
                    error: ObjectError::EmptyContent(id("t3")),
                },
                vec!["t3", "refused"],
            ),
        ] {
            let rendered = error.to_string();
            for needle in expected {
                assert!(
                    rendered.contains(needle),
                    "{error:?} rendered as {rendered:?}, which never mentions {needle:?}"
                );
            }
        }
    }

    #[test]
    fn a_tangent_records_the_fork_point_it_is_measured_against() {
        let (object, tangent) = forked();
        assert_eq!(tangent.id(), "t-cache");
        assert_eq!(tangent.at_turn(), 3);
        assert_eq!(
            tangent.at_version(),
            object.version(),
            "the fork point records a version the object was never at"
        );
        assert_eq!(
            tangent.prefix_at_open().lines().count(),
            2,
            "the fork point is not the trunk as it stood: {}",
            tangent.prefix_at_open()
        );
        assert!(
            !tangent.prefix_at_open().contains(r#""object":"dump""#),
            "the fork point carries the dump header, whose version moves on \
             every patch: {}",
            tangent.prefix_at_open()
        );
    }
}
