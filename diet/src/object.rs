//! The working object: the session's curated state, and the patches that
//! change it.
//!
//! The object is mutated by **patches** -- the output of interview forks,
//! structuring lanes, and eventually tool-mediated self-capture -- merged by
//! ordinary code, not by a model. That makes the reconciler the place
//! correctness lives once judgment moves out of the model: it has no model to
//! blame, so it has to be inspectable and diffable every turn.
//!
//! Two design lessons are built in rather than followed:
//!
//! * **`Add` is load-bearing.** A patch's added entries are not just appended.
//!   They drive dedup against existing entries, supersede-linking, provenance
//!   stamping, and the accounting that lets a later reviewer attribute a
//!   silently-wrong outcome to the model or to the reconciler. Treating `Add`
//!   as a plain append meant every one of those consequences was implemented
//!   ad hoc at call sites, and one was missed. Here they are one function.
//! * **Stringly predicates leak.** Field kinds, verdicts and outcome classes
//!   were matched as strings in more than one place and the places drifted. A
//!   typed predicate is caught by the compiler when a variant is added; a
//!   string comparison is not. `scripts/check-library.py` keeps the rule
//!   from being re-broken by hand.
//!
//! **Nothing here deletes.** A supersede voids the entry it replaces and links
//! the pair; a retire marks it and keeps it. Claim atomicity at the object
//! level: a correction is a linked row, never an edit that accretes
//! retraction and replacement into one cell. There is no method that removes
//! an entry, so there is nothing to enforce.
//!
//! Out of scope here, and named so nobody has to work it out: supersede
//! *detection* -- deciding that a new entry voids an old one -- belongs to the
//! collector, and this module implements the patch's semantics once a verdict
//! exists. So does rendering the object into a prompt. The object is never in
//! the prompt as the source of truth; the prompt is a render of it, produced
//! once at a seam.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use crate::formats::record::json::{self, Value};
use crate::formats::record::{Regime, regime_value};

/// An entry's identity.
///
/// A newtype rather than a `String`, so that an entry id and a lane name
/// cannot be passed to each other's parameter.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EntryId(String);

impl EntryId {
    /// The id `text` names.
    ///
    /// # Errors
    ///
    /// Returns [`ObjectError::EmptyId`] for an id that is blank.
    pub fn new(text: &str) -> Result<Self, ObjectError> {
        if text.trim().is_empty() {
            return Err(ObjectError::EmptyId);
        }
        Ok(Self(text.to_owned()))
    }

    /// The id as text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EntryId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where a patch came from.
///
/// The regime is deliberately NOT here. It is carried once by the object,
/// for the reason the record schema carries it once by the `start` event: a
/// tag repeated on every row is a tag that can disagree with itself. Every
/// patch applied to an object is by construction under that object's regime,
/// and merging patches across regimes is a different operation that does not
/// exist yet.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Provenance {
    /// The turn the patch came from.
    pub turn: u32,
    /// The lane that produced it.
    pub lane: String,
    /// The fork it came from, when it came from one.
    pub fork: Option<String>,
}

/// What a patch does.
///
/// Explicit variants rather than one shape with optional fields: a patch that
/// could be any of these depending on which fields are set is a patch whose
/// meaning is decided at the call site, which is how the consequences of
/// `Add` got spread across call sites in the first place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Patch {
    /// Record a new fact.
    Add {
        /// The entry's identity.
        id: EntryId,
        /// What it says.
        content: String,
        /// Where it came from.
        provenance: Provenance,
    },
    /// Record a fact that replaces an earlier one. The earlier entry is
    /// voided and linked, never removed.
    Supersede {
        /// The new entry's identity.
        id: EntryId,
        /// What it says.
        content: String,
        /// The entry it voids.
        voids: EntryId,
        /// Where it came from.
        provenance: Provenance,
    },
    /// Mark an entry settled.
    Resolve {
        /// The entry.
        target: EntryId,
        /// Where the verdict came from.
        provenance: Provenance,
    },
    /// Mark an entry no longer relevant. It stays in the object.
    Retire {
        /// The entry.
        target: EntryId,
        /// Where the verdict came from.
        provenance: Provenance,
    },
}

impl Patch {
    /// Where this patch came from.
    #[must_use]
    pub fn provenance(&self) -> &Provenance {
        match self {
            Self::Add { provenance, .. }
            | Self::Supersede { provenance, .. }
            | Self::Resolve { provenance, .. }
            | Self::Retire { provenance, .. } => provenance,
        }
    }
}

/// What an entry currently is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryState {
    /// Current.
    Live,
    /// Replaced by a later entry, which is named. The pair is linked in both
    /// directions so a reader arriving at either end can walk to the other.
    Voided {
        /// The entry that replaced it.
        by: EntryId,
    },
    /// Settled: what it described has been decided.
    Resolved,
    /// No longer relevant, and kept.
    Retired,
}

impl EntryState {
    /// A stable name, for dumps and fixtures.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Voided { .. } => "voided",
            Self::Resolved => "resolved",
            Self::Retired => "retired",
        }
    }

    /// Whether an entry in this state still speaks for the object.
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(self, Self::Live)
    }
}

/// One fact in the object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its identity.
    pub id: EntryId,
    /// What it says.
    pub content: String,
    /// What it currently is.
    pub state: EntryState,
    /// The entry this one replaced, if it replaced one. The other half of the
    /// supersede link.
    pub supersedes: Option<EntryId>,
    /// Everywhere this fact came from, in order, deduplicated.
    ///
    /// Plural because two forks in one turn will say the same thing, and
    /// which of them said it is exactly what a later reviewer needs to
    /// attribute a wrong outcome to the model or to the reconciler.
    pub provenances: Vec<Provenance>,
}

/// Why a patch could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectError {
    /// An id that is blank.
    EmptyId,
    /// An entry with no content says nothing, and a fact that says nothing
    /// cannot be checked against anything.
    EmptyContent(EntryId),
    /// A patch names an entry the object does not hold.
    UnknownTarget(EntryId),
    /// An `Add` reuses an id that already names a different fact. Dedup by
    /// content is a merge; dedup by identity onto different content is a
    /// silent overwrite.
    IdReused {
        /// The id.
        id: EntryId,
        /// What the object already holds under it.
        existing: String,
    },
    /// An entry cannot supersede itself.
    SelfSupersede(EntryId),
    /// An entry already voided cannot be voided again: the first link would
    /// have to be overwritten, and overwriting is what this module refuses.
    AlreadyVoided {
        /// The entry.
        id: EntryId,
        /// What voided it.
        by: EntryId,
    },
    /// A `Supersede` whose content the object already holds. Dedup would fold
    /// the correction into the held entry, so no new entry exists to carry the
    /// link -- and the pair could only be linked by overwriting a link the
    /// held entry may already have. A correction that overwrites is the thing
    /// this module exists to refuse.
    SupersedeRestates {
        /// The id the patch asked to create.
        id: EntryId,
        /// The entry that already holds that content.
        held: EntryId,
    },
    /// A patch names an entry that no longer speaks for the object. Writing
    /// through it would either overwrite a supersede link (the state is one
    /// slot, and `Voided` carries the link in it) or silently discard the mark
    /// that took it out of the live set.
    TargetNotLive {
        /// The entry.
        id: EntryId,
        /// What it currently is.
        state: EntryState,
    },
    /// An `Add` restating content the object holds under an entry that is no
    /// longer live. Dedup is for two forks saying the same thing in one turn;
    /// folding a later assertion into a fact that was already corrected away
    /// files it onto a corpse, and the disagreement -- the interesting part --
    /// is gone.
    RestatesInactive {
        /// The id the patch asked to create.
        id: EntryId,
        /// The entry that already holds that content.
        held: EntryId,
        /// What that entry currently is.
        state: EntryState,
    },
}

impl fmt::Display for ObjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId => write!(f, "an entry id that is blank"),
            Self::EmptyContent(id) => write!(f, "entry `{id}` says nothing"),
            Self::UnknownTarget(id) => {
                write!(
                    f,
                    "the patch names entry `{id}`, which the object does not hold"
                )
            }
            Self::IdReused { id, existing } => write!(
                f,
                "entry `{id}` already holds {existing:?}; adding different content \
                 under the same id would overwrite it"
            ),
            Self::SelfSupersede(id) => write!(f, "entry `{id}` cannot supersede itself"),
            Self::AlreadyVoided { id, by } => write!(
                f,
                "entry `{id}` was already voided by `{by}`, and overwriting that \
                 link would lose which correction came first"
            ),
            Self::TargetNotLive { id, state } => write!(
                f,
                "entry `{id}` is {}, so it no longer speaks for the object; \
                 writing through it would overwrite what took it out",
                state.name()
            ),
            Self::SupersedeRestates { id, held } => write!(
                f,
                "entry `{id}` would restate `{held}`, so the supersede creates no \
                 new entry and could only link the pair by overwriting `{held}`"
            ),
            Self::RestatesInactive { id, held, state } => write!(
                f,
                "entry `{id}` restates `{held}`, which is {}; folding it in would \
                 file the assertion onto a fact that is no longer live",
                state.name()
            ),
        }
    }
}

impl Error for ObjectError {}

/// What applying a patch did, and to which entries.
///
/// Named rather than inferred, because the acceptance this module is built
/// against is "each diff is attributable to exactly one patch" -- and an
/// attribution nobody returns is one somebody reconstructs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applied {
    /// A new entry was created.
    Created(EntryId),
    /// The content already existed. The patch's provenance was recorded on
    /// the entry that already held it, and no second entry was made.
    Deduped(EntryId),
    /// A new entry was created and an existing one voided and linked to it.
    Superseded {
        /// The new entry.
        added: EntryId,
        /// The entry it voided.
        voided: EntryId,
    },
    /// An existing entry changed state.
    StateChanged(EntryId),
    /// The object already held everything the patch said, provenance
    /// included. No entry moved.
    ///
    /// Distinct from `Deduped` because attribution is the acceptance this
    /// module is built against: a patch that names an entry whose dump line
    /// is byte-identical afterwards has not touched it, and saying otherwise
    /// makes `touched()` a claim a diff cannot support.
    Unchanged(EntryId),
}

impl Applied {
    /// Every entry this application touched.
    #[must_use]
    pub fn touched(&self) -> Vec<EntryId> {
        match self {
            Self::Created(id) | Self::Deduped(id) | Self::StateChanged(id) => vec![id.clone()],
            Self::Superseded { added, voided } => vec![added.clone(), voided.clone()],
            Self::Unchanged(_) => Vec::new(),
        }
    }
}

/// The working object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingObject {
    regime: Regime,
    entries: BTreeMap<EntryId, Entry>,
    by_content: BTreeMap<String, EntryId>,
    version: u32,
}

impl WorkingObject {
    /// Open an object under `regime`.
    #[must_use]
    pub fn open(regime: Regime) -> Self {
        Self {
            regime,
            entries: BTreeMap::new(),
            by_content: BTreeMap::new(),
            version: 0,
        }
    }

    /// The regime every patch in this object was applied under.
    #[must_use]
    pub fn regime(&self) -> &Regime {
        &self.regime
    }

    /// How many patches have been applied.
    #[must_use]
    pub fn version(&self) -> u32 {
        self.version
    }

    /// The entry `id` names.
    #[must_use]
    pub fn entry(&self, id: &EntryId) -> Option<&Entry> {
        self.entries.get(id)
    }

    /// Every entry, in id order.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// Every entry that still speaks for the object.
    pub fn live(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values().filter(|entry| entry.state.is_live())
    }

    /// Apply one patch.
    ///
    /// Deterministic: the same object and the same patch give the same result
    /// and the same dump, every time. That is what makes a diff between two
    /// dumps mean something.
    ///
    /// # Errors
    ///
    /// Returns [`ObjectError`] when the patch names an entry that is not
    /// there, reuses an id for different content, or would overwrite a link.
    pub fn apply(&mut self, patch: &Patch) -> Result<Applied, ObjectError> {
        let applied = match patch {
            Patch::Add {
                id,
                content,
                provenance,
            } => self.add(id, content, provenance)?,
            Patch::Supersede {
                id,
                content,
                voids,
                provenance,
            } => self.supersede(id, content, voids, provenance)?,
            Patch::Resolve { target, provenance } => {
                self.set_state(target, EntryState::Resolved, provenance)?
            }
            Patch::Retire { target, provenance } => {
                self.set_state(target, EntryState::Retired, provenance)?
            }
        };
        self.version += 1;
        Ok(applied)
    }

    /// Record a fact, merging it into an identical one if the object already
    /// holds it.
    ///
    /// Dedup is by content, exactly, rather than by a hash of it. A hash is a
    /// lossy stand-in for the comparison actually wanted, and here the
    /// comparison is affordable -- so there is no collision to reason about.
    fn add(
        &mut self,
        id: &EntryId,
        content: &str,
        provenance: &Provenance,
    ) -> Result<Applied, ObjectError> {
        let key = normalise(content);
        self.check_admissible(id, &key)?;
        // Two forks in one turn say the same thing. One entry, both
        // provenances: which of them said it is what a later reviewer needs to
        // attribute a wrong outcome to the model or to the reconciler.
        //
        // Only onto a LIVE entry. The held entry may have been voided,
        // resolved or retired since; folding a later assertion into it files
        // that lane's claim onto a fact the object no longer holds, and a
        // reader diffing dumps sees a provenance appear on a dead row rather
        // than a disagreement between two lanes.
        if let Some(held) = self.by_content.get(&key).cloned() {
            if let Some(entry) = self.entries.get(&held)
                && !entry.state.is_live()
            {
                return Err(ObjectError::RestatesInactive {
                    id: id.clone(),
                    held,
                    state: entry.state.clone(),
                });
            }
            return Ok(if self.record_provenance(&held, provenance) {
                Applied::Deduped(held)
            } else {
                Applied::Unchanged(held)
            });
        }
        Ok(Applied::Created(self.create(id, content, key, provenance)))
    }

    /// Whether an entry under `id` carrying content that normalises to `key`
    /// may be written at all, independent of what the object already holds
    /// under that content.
    fn check_admissible(&self, id: &EntryId, key: &str) -> Result<(), ObjectError> {
        if key.is_empty() {
            return Err(ObjectError::EmptyContent(id.clone()));
        }
        if let Some(existing) = self.entries.get(id)
            && normalise(&existing.content) != key
        {
            return Err(ObjectError::IdReused {
                id: id.clone(),
                existing: existing.content.clone(),
            });
        }
        Ok(())
    }

    /// Write a genuinely new entry and index it.
    ///
    /// Callers establish first that the object holds nothing under `key`, so
    /// this cannot fail -- which is what lets `supersede` know it has a new
    /// entry to link, rather than discovering afterwards that dedup handed it
    /// one that was already there.
    fn create(
        &mut self,
        id: &EntryId,
        content: &str,
        key: String,
        provenance: &Provenance,
    ) -> EntryId {
        self.entries.insert(
            id.clone(),
            Entry {
                id: id.clone(),
                content: content.to_owned(),
                state: EntryState::Live,
                supersedes: None,
                provenances: vec![provenance.clone()],
            },
        );
        self.by_content.insert(key, id.clone());
        id.clone()
    }

    /// Record a fact that replaces an earlier one, linking the pair.
    fn supersede(
        &mut self,
        id: &EntryId,
        content: &str,
        voids: &EntryId,
        provenance: &Provenance,
    ) -> Result<Applied, ObjectError> {
        if id == voids {
            return Err(ObjectError::SelfSupersede(id.clone()));
        }
        let Some(old) = self.entries.get(voids) else {
            return Err(ObjectError::UnknownTarget(voids.clone()));
        };
        if let EntryState::Voided { by } = &old.state {
            return Err(ObjectError::AlreadyVoided {
                id: voids.clone(),
                by: by.clone(),
            });
        }
        // Resolved and retired are marks, and the state is one slot: voiding
        // over them drops the mark with nothing left to say it was ever there.
        if !old.state.is_live() {
            return Err(ObjectError::TargetNotLive {
                id: voids.clone(),
                state: old.state.clone(),
            });
        }
        // A correction states a NEW fact. If the object already holds this
        // content, dedup would hand back the entry holding it and no new entry
        // would exist -- so the link below would either point an entry at
        // itself, or overwrite whatever link the held entry already carries.
        // Both are refused here rather than written and discovered in a dump.
        let key = normalise(content);
        self.check_admissible(id, &key)?;
        if let Some(already) = self.by_content.get(&key).cloned() {
            return Err(if already == *voids {
                ObjectError::SelfSupersede(voids.clone())
            } else {
                ObjectError::SupersedeRestates {
                    id: id.clone(),
                    held: already,
                }
            });
        }
        let added = self.create(id, content, key, provenance);
        // Both directions. A reader arriving at either end can walk to the
        // other, which is what makes a correction inspectable rather than a
        // pair of rows somebody has to notice are related.
        if let Some(new) = self.entries.get_mut(&added) {
            new.supersedes = Some(voids.clone());
        }
        if let Some(old) = self.entries.get_mut(voids) {
            old.state = EntryState::Voided { by: added.clone() };
        }
        Ok(Applied::Superseded {
            added,
            voided: voids.clone(),
        })
    }

    fn set_state(
        &mut self,
        target: &EntryId,
        state: EntryState,
        provenance: &Provenance,
    ) -> Result<Applied, ObjectError> {
        let Some(entry) = self.entries.get(target) else {
            return Err(ObjectError::UnknownTarget(target.clone()));
        };
        // A voided entry keeps the other half of a supersede link in the same
        // slot the mark lives in. Resolving or retiring it would write over
        // the link -- the correction erased with nothing to show it happened
        // -- and the double-void guard reads that slot too, so it would stop
        // firing and a second entry could claim the same one.
        if let EntryState::Voided { by } = &entry.state {
            return Err(ObjectError::TargetNotLive {
                id: target.clone(),
                state: EntryState::Voided { by: by.clone() },
            });
        }
        let already_in_state = entry.state == state;
        if let Some(entry) = self.entries.get_mut(target) {
            entry.state = state;
        }
        let recorded = self.record_provenance(target, provenance);
        if already_in_state && !recorded {
            return Ok(Applied::Unchanged(target.clone()));
        }
        Ok(Applied::StateChanged(target.clone()))
    }

    /// Add `provenance` to an entry, unless it already carries it.
    ///
    /// Reports whether it wrote, so a caller can tell a patch that merged
    /// something new into an entry from one the object had already absorbed
    /// in full.
    fn record_provenance(&mut self, id: &EntryId, provenance: &Provenance) -> bool {
        if let Some(entry) = self.entries.get_mut(id)
            && !entry.provenances.contains(provenance)
        {
            entry.provenances.push(provenance.clone());
            return true;
        }
        false
    }

    /// The object as one line per entry, keyed by entry.
    ///
    /// Keyed rather than a bare string so that a diff between two versions can
    /// say WHICH entries moved, which is what "attributable to exactly one
    /// patch" needs.
    /// The dump's first line: which version this is, and under which regime.
    ///
    /// The dump is the only durable half of the object. Without this line two
    /// dumps taken at different versions are byte-identical whenever no entry
    /// moved, and two objects under different regimes are byte-identical
    /// always -- so "versioned dump" would name a file carrying neither. It
    /// is kept out of `dump_lines`, which is the keyed map a diff walks: a
    /// header is not an entry, and it would appear in every diff.
    fn header_value(&self) -> Value {
        Value::Object(BTreeMap::from([
            ("object".to_owned(), Value::String("dump".to_owned())),
            (
                "version".to_owned(),
                Value::Integer(i64::from(self.version)),
            ),
            ("regime".to_owned(), regime_value(&self.regime)),
        ]))
    }

    #[must_use]
    pub fn dump_lines(&self) -> BTreeMap<EntryId, String> {
        self.entries
            .values()
            .map(|entry| {
                let mut line = String::new();
                json::render(&entry_value(entry), &mut line);
                (entry.id.clone(), line)
            })
            .collect()
    }

    /// The object as JSONL, one entry per line, in id order.
    ///
    /// Rendered through the record's own value layer, so the object and the
    /// record are written by one implementation rather than two that agree
    /// today.
    #[must_use]
    pub fn dump(&self) -> String {
        let mut out = String::new();
        json::render(&self.header_value(), &mut out);
        out.push('\n');
        for line in self.dump_lines().values() {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

/// Which entries differ between two dumps.
///
/// Added, removed and changed alike: what a reader wants is the set of
/// entries a turn moved, and whether one appeared or altered is visible in
/// the dumps themselves.
#[must_use]
pub fn changed_between(
    before: &BTreeMap<EntryId, String>,
    after: &BTreeMap<EntryId, String>,
) -> BTreeSet<EntryId> {
    let mut moved = BTreeSet::new();
    for (id, line) in after {
        if before.get(id) != Some(line) {
            moved.insert(id.clone());
        }
    }
    for id in before.keys() {
        if !after.contains_key(id) {
            moved.insert(id.clone());
        }
    }
    moved
}

/// One entry as the record's value space.
fn entry_value(entry: &Entry) -> Value {
    let mut members = BTreeMap::from([
        ("id".to_owned(), Value::String(entry.id.0.clone())),
        ("content".to_owned(), Value::String(entry.content.clone())),
        (
            "state".to_owned(),
            Value::String(entry.state.name().to_owned()),
        ),
        (
            "provenances".to_owned(),
            Value::Array(entry.provenances.iter().map(provenance_value).collect()),
        ),
    ]);
    if let EntryState::Voided { by } = &entry.state {
        members.insert("voided_by".to_owned(), Value::String(by.0.clone()));
    }
    if let Some(superseded) = &entry.supersedes {
        members.insert("supersedes".to_owned(), Value::String(superseded.0.clone()));
    }
    Value::Object(members)
}

fn provenance_value(provenance: &Provenance) -> Value {
    let mut members = BTreeMap::from([
        (
            "turn".to_owned(),
            Value::Integer(i64::from(provenance.turn)),
        ),
        ("lane".to_owned(), Value::String(provenance.lane.clone())),
    ]);
    if let Some(fork) = &provenance.fork {
        members.insert("fork".to_owned(), Value::String(fork.clone()));
    }
    Value::Object(members)
}

/// Whitespace runs collapse to one space, and the ends are trimmed. Two forks
/// wrapping the same sentence differently said the same thing.
fn normalise(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{
        Applied, EntryId, EntryState, ObjectError, Patch, Provenance, WorkingObject,
        changed_between,
    };
    use crate::formats::record::{self, Regime};

    /// A regime for the tests, parsed through the record schema so that the
    /// object cannot be opened under a regime the record would reject.
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

    fn from(turn: u32, lane: &str, fork: Option<&str>) -> Provenance {
        Provenance {
            turn,
            lane: lane.to_owned(),
            fork: fork.map(str::to_owned),
        }
    }

    fn add(id_text: &str, content: &str, provenance: Provenance) -> Patch {
        Patch::Add {
            id: id(id_text),
            content: content.to_owned(),
            provenance,
        }
    }

    // Acceptance: the voided entry is retained, linked, and marked -- not
    // deleted. There is no method on the object that removes an entry, and
    // this is the test that says so out loud.
    #[test]
    fn a_supersede_voids_and_links_and_never_deletes() {
        let mut object = object();
        object
            .apply(&add("e1", "the rate is 0.42", from(1, "interview", None)))
            .expect("the first fact");
        let applied = object
            .apply(&Patch::Supersede {
                id: id("e2"),
                content: "the rate is 0.31 after the regrade".to_owned(),
                voids: id("e1"),
                provenance: from(2, "interview", None),
            })
            .expect("a correction");

        assert_eq!(
            applied,
            Applied::Superseded {
                added: id("e2"),
                voided: id("e1"),
            }
        );
        let old = object
            .entry(&id("e1"))
            .expect("the voided entry is still here");
        assert_eq!(
            old.content, "the rate is 0.42",
            "the old entry is unchanged"
        );
        assert_eq!(old.state, EntryState::Voided { by: id("e2") });
        let new = object.entry(&id("e2")).expect("the new entry");
        assert_eq!(
            new.supersedes.as_ref(),
            Some(&id("e1")),
            "the link runs both ways"
        );
        assert_eq!(object.entries().count(), 2);
        assert_eq!(object.live().count(), 1);
    }

    // Acceptance: two forks add the same fact in the same turn, and the
    // reconciler dedups to one entry with BOTH provenances recorded. Which
    // fork said it is what lets a later reviewer attribute a wrong outcome to
    // the model or to the reconciler.
    #[test]
    fn two_forks_saying_the_same_thing_dedup_with_both_provenances() {
        let mut object = object();
        let first = from(3, "interview", Some("f1"));
        let second = from(3, "interview", Some("f2"));
        assert_eq!(
            object
                .apply(&add(
                    "e1",
                    "the resolver refuses a stale binary",
                    first.clone()
                ))
                .expect("the first fork"),
            Applied::Created(id("e1"))
        );
        assert_eq!(
            object
                .apply(&add(
                    "e2",
                    "the resolver   refuses\na stale binary",
                    second.clone()
                ))
                .expect("the second fork"),
            Applied::Deduped(id("e1")),
            "the same fact, wrapped differently, is the same fact"
        );

        assert_eq!(object.entries().count(), 1, "one fact, one entry");
        let entry = object.entry(&id("e1")).expect("the entry");
        assert_eq!(
            entry.provenances,
            vec![first, second],
            "both forks are recorded on the one entry"
        );
        assert!(object.entry(&id("e2")).is_none());
    }

    #[test]
    fn the_same_provenance_twice_is_recorded_once() {
        let mut object = object();
        let once = from(1, "interview", Some("f1"));
        object
            .apply(&add("e1", "a fact", once.clone()))
            .expect("first");
        object
            .apply(&add("e2", "a fact", once.clone()))
            .expect("second");
        assert_eq!(
            object.entry(&id("e1")).expect("the entry").provenances,
            vec![once]
        );
    }

    // Acceptance: five turns, a dump after each, and every diff attributable
    // to exactly one patch. `Applied::touched` is the attribution, so the
    // assertion compares it against what actually moved rather than against a
    // count.
    #[test]
    fn every_diff_between_dumps_is_attributable_to_exactly_one_patch() {
        let mut object = object();
        let patches = [
            add(
                "e1",
                "the resolver refuses a stale binary",
                from(1, "interview", None),
            ),
            add("e2", "the rate is 0.42", from(2, "interview", None)),
            Patch::Supersede {
                id: id("e3"),
                content: "the rate is 0.31 after the regrade".to_owned(),
                voids: id("e2"),
                provenance: from(3, "interview", None),
            },
            Patch::Resolve {
                target: id("e1"),
                provenance: from(4, "reconciler", None),
            },
            Patch::Retire {
                target: id("e3"),
                provenance: from(5, "reconciler", None),
            },
        ];

        let mut before = object.dump_lines();
        for (turn, patch) in patches.iter().enumerate() {
            let applied = object.apply(patch).expect("the patch applies");
            let after = object.dump_lines();
            let moved = changed_between(&before, &after);
            let claimed: std::collections::BTreeSet<_> = applied.touched().into_iter().collect();
            assert_eq!(
                moved,
                claimed,
                "turn {} moved {moved:?} but the patch claims {claimed:?}",
                turn + 1
            );
            before = after;
        }
        assert_eq!(object.version(), 5);
        assert_eq!(object.entries().count(), 3, "nothing was deleted");
    }

    #[test]
    fn a_dump_is_stable_and_ordered_by_entry() {
        let mut object = object();
        object
            .apply(&add("e2", "second", from(1, "a", None)))
            .expect("b");
        object
            .apply(&add("e1", "first", from(1, "a", None)))
            .expect("a");
        let once = object.dump();
        assert_eq!(once, object.dump(), "dumping twice gives the same bytes");
        // By ID, which is what this test is about. Reading the field by
        // position picked out `content` instead -- and passed, because the
        // fixture's contents happened to sort the same way its ids did.
        let ids: Vec<String> = once
            .lines()
            .skip(1)
            .map(|line| {
                let at = line
                    .find(r#""id":""#)
                    .expect("every entry line names its id");
                line[at + 6..]
                    .split('"')
                    .next()
                    .expect("the id is a quoted string")
                    .to_owned()
            })
            .collect();
        assert_eq!(ids, vec!["e1", "e2"], "entries come out in id order");
    }

    #[test]
    fn a_dump_line_is_a_record_value() {
        let mut object = object();
        object
            .apply(&add(
                "e1",
                "a fact with a \"quote\" and a \n newline",
                from(1, "a", Some("f1")),
            ))
            .expect("a fact");
        let dump = object.dump();
        let lines: Vec<&str> = dump.lines().collect();
        assert_eq!(lines.len(), 2, "a header line, then one line per entry");
        assert!(lines[1].contains(r#"\"quote\""#), "quotes are escaped");
        assert!(!lines[1].contains('\n'), "one entry, one line");
        // The header says which version and which regime, or the dump is not
        // a versioned dump.
        assert!(lines[0].contains(r#""version":1"#), "header: {}", lines[0]);
        assert!(
            lines[0].contains(r#""arm":"baseline""#),
            "header: {}",
            lines[0]
        );
    }

    #[test]
    fn a_dump_distinguishes_versions_and_regimes() {
        let mut object = object();
        object
            .apply(&add("e1", "a fact", from(1, "interview", None)))
            .expect("a fact");
        let first = object.dump();
        // A patch that moves no entry still advances the version, and the
        // dump has to show it or two turns are indistinguishable on disk.
        object
            .apply(&Patch::Resolve {
                target: id("e1"),
                provenance: from(2, "structuring", None),
            })
            .expect("a state change");
        assert_ne!(first, object.dump(), "two versions, one dump");

        let mut other = WorkingObject::open(
            record::parse(
                "{\"record\":\"start\",\"regime\":{\"arm\":\"treatment\",\"dogma_version\":0,\
                 \"substrate\":{\"name\":\"local\",\"model\":\"m\",\"quantization\":\"q\",\
                 \"sampler\":{\"seed\":0},\"reasoning\":\"on\",\"hardware\":\"h\"}}}",
            )
            .expect("a record")
            .regime()
            .clone(),
        );
        other
            .apply(&add("e1", "a fact", from(1, "interview", None)))
            .expect("the same fact");
        assert_ne!(
            first,
            other.dump(),
            "two arms produced byte-identical dumps of the same fact"
        );
    }

    #[test]
    fn a_patch_naming_an_entry_the_object_does_not_hold_is_refused() {
        let mut object = object();
        for patch in [
            Patch::Resolve {
                target: id("nope"),
                provenance: from(1, "a", None),
            },
            Patch::Retire {
                target: id("nope"),
                provenance: from(1, "a", None),
            },
            Patch::Supersede {
                id: id("e1"),
                content: "x".to_owned(),
                voids: id("nope"),
                provenance: from(1, "a", None),
            },
        ] {
            assert_eq!(
                object.apply(&patch),
                Err(ObjectError::UnknownTarget(id("nope"))),
                "{patch:?}"
            );
        }
    }

    // Dedup by identity onto different content is a silent overwrite, which is
    // the one thing this module exists to refuse.
    #[test]
    fn reusing_an_id_for_different_content_is_refused() {
        let mut object = object();
        object
            .apply(&add("e1", "a fact", from(1, "a", None)))
            .expect("first");
        assert!(matches!(
            object.apply(&add("e1", "a different fact", from(2, "a", None))),
            Err(ObjectError::IdReused { .. })
        ));
        assert_eq!(
            object.entry(&id("e1")).expect("the entry").content,
            "a fact"
        );
    }

    #[test]
    fn an_entry_cannot_supersede_itself_or_be_voided_twice() {
        let mut object = object();
        object
            .apply(&add("e1", "a fact", from(1, "a", None)))
            .expect("first");
        assert_eq!(
            object.apply(&Patch::Supersede {
                id: id("e1"),
                content: "a fact".to_owned(),
                voids: id("e1"),
                provenance: from(2, "a", None),
            }),
            Err(ObjectError::SelfSupersede(id("e1")))
        );
        object
            .apply(&Patch::Supersede {
                id: id("e2"),
                content: "a correction".to_owned(),
                voids: id("e1"),
                provenance: from(2, "a", None),
            })
            .expect("the first correction");
        assert!(matches!(
            object.apply(&Patch::Supersede {
                id: id("e3"),
                content: "another correction".to_owned(),
                voids: id("e1"),
                provenance: from(3, "a", None),
            }),
            Err(ObjectError::AlreadyVoided { .. })
        ));
    }

    #[test]
    fn an_entry_that_says_nothing_is_refused() {
        let mut object = object();
        assert_eq!(
            object.apply(&add("e1", "   \n\t ", from(1, "a", None))),
            Err(ObjectError::EmptyContent(id("e1")))
        );
        assert!(EntryId::new("  ").is_err());
    }

    // Applying the same patches in the same order gives the same bytes. That
    // is what makes a diff between two dumps mean something.
    #[test]
    fn applying_the_same_patches_twice_gives_the_same_object() {
        let patches = [
            add("e1", "one", from(1, "a", Some("f1"))),
            add("e2", "two", from(1, "a", Some("f2"))),
            Patch::Resolve {
                target: id("e1"),
                provenance: from(2, "b", None),
            },
        ];
        let mut first = object();
        let mut second = object();
        for patch in &patches {
            first.apply(patch).expect("applies");
            second.apply(patch).expect("applies");
        }
        assert_eq!(first, second);
        assert_eq!(first.dump(), second.dump());
    }

    // A correction whose content the object already holds creates no new
    // entry. Every one of these three said "applied" before, and lost a fact
    // doing it: the object reported a supersede it had not performed, an
    // entry voided by itself, an overwritten link, and a later lane's
    // disagreement filed onto a row nobody reads.

    #[test]
    fn a_supersede_that_restates_the_entry_it_voids_is_refused() {
        let mut object = object();
        object
            .apply(&add(
                "e1",
                "the parser rejects empty input",
                from(1, "interview", None),
            ))
            .expect("the first fact");
        let version = object.version();
        // The same fact, re-wrapped. normalise() collapses whitespace, so as
        // far as dedup is concerned this content IS e1's.
        let refused = object.apply(&Patch::Supersede {
            id: id("e2"),
            content: "the  parser   rejects\nempty input".to_owned(),
            voids: id("e1"),
            provenance: from(2, "interview", None),
        });
        assert_eq!(
            refused,
            Err(ObjectError::SelfSupersede(id("e1"))),
            "an entry was voided by itself"
        );
        // Refused means untouched: e1 still speaks, and no version was spent.
        let entry = object.entry(&id("e1")).expect("e1 is retained");
        assert_eq!(entry.state, EntryState::Live);
        assert_eq!(entry.supersedes, None);
        assert_eq!(object.version(), version);
        assert!(object.entry(&id("e2")).is_none());
    }

    #[test]
    fn a_supersede_that_restates_a_third_entry_is_refused() {
        let mut object = object();
        for (entry, content) in [("e0", "the rate is 0.42"), ("e1", "the sampler is greedy")] {
            object
                .apply(&add(entry, content, from(1, "interview", None)))
                .expect("a fact");
        }
        object
            .apply(&Patch::Supersede {
                id: id("e3"),
                content: "the rate is 0.31 after the regrade".to_owned(),
                voids: id("e0"),
                provenance: from(2, "interview", None),
            })
            .expect("a correction");
        // Correcting e1 with content that restates e3 would have to reuse e3
        // as the new entry -- overwriting the link e3 already carries to e0.
        let refused = object.apply(&Patch::Supersede {
            id: id("e4"),
            content: "the rate is 0.31 after the regrade".to_owned(),
            voids: id("e1"),
            provenance: from(3, "interview", None),
        });
        assert_eq!(
            refused,
            Err(ObjectError::SupersedeRestates {
                id: id("e4"),
                held: id("e3"),
            }),
            "a correction overwrote the link an entry already carried"
        );
        // The first correction's link survives, in both directions.
        assert_eq!(
            object.entry(&id("e3")).expect("e3").supersedes,
            Some(id("e0"))
        );
        assert_eq!(
            object.entry(&id("e0")).expect("e0").state,
            EntryState::Voided { by: id("e3") }
        );
        assert_eq!(object.entry(&id("e1")).expect("e1").state, EntryState::Live);
    }

    #[test]
    fn restating_a_fact_that_was_corrected_away_is_refused() {
        let mut object = object();
        object
            .apply(&add("e1", "the rate is 0.42", from(1, "interview", None)))
            .expect("the first fact");
        object
            .apply(&Patch::Supersede {
                id: id("e2"),
                content: "the rate is 0.31".to_owned(),
                voids: id("e1"),
                provenance: from(2, "interview", None),
            })
            .expect("a correction");
        // A later lane, not having seen the correction, asserts the old fact
        // again. Folding it into e1 would file a live disagreement onto an
        // entry no reader of the object will ever see.
        let refused = object.apply(&add("e9", "the rate is 0.42", from(3, "structuring", None)));
        assert_eq!(
            refused,
            Err(ObjectError::RestatesInactive {
                id: id("e9"),
                held: id("e1"),
                state: EntryState::Voided { by: id("e2") },
            }),
            "an assertion was filed onto an entry that is no longer live"
        );
        assert_eq!(
            object.entry(&id("e1")).expect("e1").provenances.len(),
            1,
            "the corrected-away entry gained a provenance it never said"
        );
    }

    #[test]
    fn every_object_error_says_which_entries_it_is_about() {
        for (error, expected) in [
            (
                ObjectError::SupersedeRestates {
                    id: id("e4"),
                    held: id("e3"),
                },
                vec!["e4", "e3"],
            ),
            (
                ObjectError::RestatesInactive {
                    id: id("e9"),
                    held: id("e1"),
                    state: EntryState::Voided { by: id("e2") },
                },
                vec!["e9", "e1", "voided"],
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
    fn an_object_never_goes_dark_from_ordinary_corrections() {
        // Two facts, then each corrected with content that happens to restate
        // the other. Nothing about these patches is pathological, and before
        // the guard every one returned Ok and the object ended with no live
        // entry at all.
        let mut object = object();
        object
            .apply(&add("a", "the rate is 0.42", from(1, "interview", None)))
            .expect("a fact");
        object
            .apply(&add(
                "b",
                "the resolver refuses a stale binary",
                from(1, "interview", None),
            ))
            .expect("another fact");
        object
            .apply(&Patch::Supersede {
                id: id("c"),
                content: "the resolver refuses a stale binary".to_owned(),
                voids: id("a"),
                provenance: from(2, "interview", None),
            })
            .expect_err("a correction that restates a held fact is refused");
        object
            .apply(&Patch::Supersede {
                id: id("d"),
                content: "the rate is 0.42".to_owned(),
                voids: id("b"),
                provenance: from(3, "interview", None),
            })
            .expect_err("and so is its mirror image");
        assert_eq!(
            object.live().count(),
            2,
            "the object went dark: every patch returned Ok and nothing was left alive"
        );
    }

    #[test]
    fn a_patch_that_changes_nothing_claims_nothing() {
        let mut object = object();
        let said = from(1, "interview", None);
        object
            .apply(&Patch::Add {
                id: id("e1"),
                content: "a fact".to_owned(),
                provenance: said.clone(),
            })
            .expect("the fact");
        for (label, patch) in [
            (
                "the same fact from the same place",
                Patch::Add {
                    id: id("e2"),
                    content: "a fact".to_owned(),
                    provenance: said.clone(),
                },
            ),
            (
                "resolving what is already resolved",
                Patch::Resolve {
                    target: id("e1"),
                    provenance: said.clone(),
                },
            ),
        ] {
            if label.starts_with("resolving") {
                object
                    .apply(&Patch::Resolve {
                        target: id("e1"),
                        provenance: said.clone(),
                    })
                    .expect("the first resolve");
            }
            let before = object.dump_lines();
            let applied = object.apply(&patch).expect("the patch applies");
            let moved = changed_between(&before, &object.dump_lines());
            let claimed: std::collections::BTreeSet<_> = applied.touched().into_iter().collect();
            assert!(
                moved.is_empty(),
                "{label}: expected nothing to move, {moved:?} did"
            );
            assert_eq!(
                claimed, moved,
                "{label}: the patch claimed entries no diff can support"
            );
        }
    }

    #[test]
    fn a_voided_entry_cannot_be_resolved_retired_or_voided_again() {
        let mut object = object();
        object
            .apply(&add("e1", "the rate is 0.42", from(1, "interview", None)))
            .expect("the fact");
        object
            .apply(&Patch::Supersede {
                id: id("e2"),
                content: "the rate is 0.31".to_owned(),
                voids: id("e1"),
                provenance: from(2, "interview", None),
            })
            .expect("a correction");
        for patch in [
            Patch::Resolve {
                target: id("e1"),
                provenance: from(3, "structuring", None),
            },
            Patch::Retire {
                target: id("e1"),
                provenance: from(3, "structuring", None),
            },
        ] {
            assert_eq!(
                object.apply(&patch),
                Err(ObjectError::TargetNotLive {
                    id: id("e1"),
                    state: EntryState::Voided { by: id("e2") },
                }),
                "a correction was erased by a later state change"
            );
        }
        // The link is still there, in both directions, and still guards.
        assert_eq!(
            object.entry(&id("e1")).expect("e1").state,
            EntryState::Voided { by: id("e2") }
        );
        assert_eq!(
            object.entry(&id("e2")).expect("e2").supersedes,
            Some(id("e1"))
        );
    }

    #[test]
    fn a_retired_or_resolved_entry_cannot_be_superseded_over() {
        for (mark, retire) in [(EntryState::Retired, true), (EntryState::Resolved, false)] {
            let mut object = object();
            object
                .apply(&add("e1", "the rate is 0.42", from(1, "interview", None)))
                .expect("the fact");
            object
                .apply(&if retire {
                    Patch::Retire {
                        target: id("e1"),
                        provenance: from(2, "structuring", None),
                    }
                } else {
                    Patch::Resolve {
                        target: id("e1"),
                        provenance: from(2, "structuring", None),
                    }
                })
                .expect("the mark");
            assert_eq!(
                object.apply(&Patch::Supersede {
                    id: id("e2"),
                    content: "the rate is 0.31".to_owned(),
                    voids: id("e1"),
                    provenance: from(3, "interview", None),
                }),
                Err(ObjectError::TargetNotLive {
                    id: id("e1"),
                    state: mark.clone(),
                }),
                "a supersede wrote over the mark that took the entry out of the live set"
            );
            assert_eq!(object.entry(&id("e1")).expect("e1").state, mark);
        }
    }
}
