//! The transport journal: what the client saw, in the order it saw it.
//!
//! The journal is the client's own record, and it is authoritative for the
//! transport layer. It exists as its own stream rather than as rows of a
//! session record because **record v0 cannot spell most of what happens
//! here** -- there is no `regime.mismatch` event kind, a `response` has no
//! outcome, and neither a request nor a response has anywhere to put a
//! sampler card, a cache count or a retry's reason.
//!
//! Two ways to handle that, and only one of them is honest. The first is to
//! project what fits and drop the rest, which is how a record ends up
//! carrying a timeout as a response with an empty string. The second is to
//! project what fits and **name what does not**, in a list a test asserts
//! against -- so that the day the schema grows, the assertion goes red and
//! this file has to be brought up to it.
//!
//! This is the second. [`project`] returns both halves.

use std::time::Duration;

use crate::formats::record::{Count, Event};

use super::echo::Verified;
use super::shape::{Concurrency, SamplerCard, SamplerSetting};
use super::transport::TransportFailure;
use super::wire::WireError;
use super::{Cache, RetryReason, vocabulary};

vocabulary! {
    /// Which kind of thing a journal entry is.
    ///
    /// Separated from [`Entry`] so a coverage test can enumerate the kinds
    /// without constructing one of each -- the same reason the record's own
    /// vocabulary is separated from its events.
    EntryKind {
        /// How the server was declared to be serving.
        Serving => "serving.declared",
        /// The client issued a request. NOT "bytes reached the server": a
        /// connection refused is an issued request that never arrived, and
        /// which of those happened is what the outcome entry says. The record
        /// spells only the first meaning, and that difference is itself
        /// unspellable -- see [`project`].
        Issued => "request.issued",
        /// An answer came back.
        Received => "response.received",
        /// The server contradicted a pin.
        Mismatch => "regime.mismatch",
        /// A pin could not be checked either way.
        Unverified => "regime.unverified",
        /// A retry removed a pin.
        Stripped => "regime.stripped",
        /// A retry was made.
        Retried => "request.retried",
        /// An attempt ran out of time.
        TimedOut => "outcome.timeout",
        /// An answer stopped because it ran out of room.
        Capped => "outcome.capped",
        /// The server answered with an error status.
        Refused => "outcome.refused",
        /// No reply arrived.
        Failed => "outcome.failed",
        /// A reply arrived and could not be read.
        Unreadable => "outcome.unreadable",
        /// What the server said about its prompt cache.
        Cache => "cache.observed",
    }
}

/// One thing the client saw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// How the server was declared to be serving, recorded once per call so
    /// that a throughput number taken from this call carries its question.
    Serving {
        /// How many streams the operator declared.
        concurrency: Concurrency,
        /// Which dialect said where to read the server's report of itself.
        dialect: String,
        /// Where the request went.
        endpoint: String,
    },
    /// The client issued a request.
    Issued {
        /// Its identifier.
        id: String,
        /// Which lane sent it.
        lane: String,
        /// The attempt it retries, if any.
        retry_of: Option<String>,
        /// Why it exists, if it is a retry.
        because: Option<RetryReason>,
        /// What it pinned. This is the half of the echo the server never
        /// sees, and without it a later reader cannot re-run the comparison.
        sampler: SamplerCard,
    },
    /// An answer came back.
    Received {
        /// The request it answers.
        to_request: String,
        /// How many tokens, when the server says.
        output_tokens: Option<Count>,
        /// Whether it stopped for running out of room.
        capped: bool,
    },
    /// The server contradicted at least one pin.
    Mismatch {
        /// The request whose pins were contradicted.
        id: String,
        /// Which ones, and how.
        settings: Vec<Verified>,
    },
    /// At least one pin could not be checked in either direction.
    Unverified {
        /// The request whose pins could not be checked.
        id: String,
        /// Which ones, and why not.
        settings: Vec<Verified>,
    },
    /// A pin was removed so that the next attempt would be accepted.
    Stripped {
        /// The request that was rejected.
        id: String,
        /// The pin that came out.
        setting: SamplerSetting,
    },
    /// A retry was made.
    Retried {
        /// The attempt it follows.
        after: String,
        /// Why.
        because: RetryReason,
    },
    /// An attempt ran out of time.
    TimedOut {
        /// The attempt.
        id: String,
        /// How long the call had run.
        after: Duration,
    },
    /// An answer stopped because it ran out of room.
    Capped {
        /// The attempt.
        id: String,
        /// The cap it hit.
        at: u32,
    },
    /// The server answered with an error status.
    Refused {
        /// The attempt.
        id: String,
        /// The status.
        status: u16,
        /// What the server said.
        body: String,
    },
    /// No reply arrived.
    Failed {
        /// The attempt.
        id: String,
        /// How.
        failure: TransportFailure,
    },
    /// A reply arrived and could not be read.
    Unreadable {
        /// The attempt.
        id: String,
        /// Why.
        why: WireError,
    },
    /// What the server said about its prompt cache.
    Cache {
        /// The attempt.
        id: String,
        /// What it said.
        cache: Cache,
    },
}

impl Entry {
    /// Which kind this entry is.
    #[must_use]
    pub fn kind(&self) -> EntryKind {
        match self {
            Self::Serving { .. } => EntryKind::Serving,
            Self::Issued { .. } => EntryKind::Issued,
            Self::Received { .. } => EntryKind::Received,
            Self::Mismatch { .. } => EntryKind::Mismatch,
            Self::Unverified { .. } => EntryKind::Unverified,
            Self::Stripped { .. } => EntryKind::Stripped,
            Self::Retried { .. } => EntryKind::Retried,
            Self::TimedOut { .. } => EntryKind::TimedOut,
            Self::Capped { .. } => EntryKind::Capped,
            Self::Refused { .. } => EntryKind::Refused,
            Self::Failed { .. } => EntryKind::Failed,
            Self::Unreadable { .. } => EntryKind::Unreadable,
            Self::Cache { .. } => EntryKind::Cache,
        }
    }
}

/// Everything the client saw, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Journal {
    entries: Vec<Entry>,
}

impl Journal {
    /// An empty journal.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append `entry`.
    pub fn push(&mut self, entry: Entry) {
        self.entries.push(entry);
    }

    /// Every entry, in order.
    #[must_use]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Every entry of `kind`, in order.
    #[must_use]
    pub fn of(&self, kind: EntryKind) -> Vec<&Entry> {
        self.entries
            .iter()
            .filter(|entry| entry.kind() == kind)
            .collect()
    }

    /// Whether any entry is of `kind`.
    #[must_use]
    pub fn carries(&self, kind: EntryKind) -> bool {
        self.entries.iter().any(|entry| entry.kind() == kind)
    }
}

/// A fact the journal holds and the session record cannot hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unspellable {
    /// The entry kind that carries it.
    pub kind: EntryKind,
    /// What is lost, in the words the request for a schema change uses.
    pub what: &'static str,
}

/// The journal as far as the session record can carry it, and what it cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projection {
    /// The record events, in order.
    pub events: Vec<Event>,
    /// Every fact that had no spelling, in the order its entry appeared.
    /// Deduplicated by kind: the request is for a schema, not a per-row
    /// complaint.
    pub unspellable: Vec<Unspellable>,
}

/// A response's identifier, derived from the request it answers.
///
/// A rule rather than an invention: a reader can go from either identifier to
/// the other, and no counter has to be threaded through a projection that has
/// no state of its own. The record requires a `response` to name its request;
/// this makes the reverse true as well.
#[must_use]
pub fn response_id(request: &str) -> String {
    format!("{request}#response")
}

/// What an entry holds that record v0 has no spelling for, if anything.
///
/// A table rather than arms inside the projection, so that the list of what
/// the schema is missing can be read in one place -- it is the content of the
/// request for a schema change, and a request nobody can read in one sitting
/// does not get made.
fn lost(entry: &Entry) -> Option<&'static str> {
    match entry {
        Entry::Serving { .. } => {
            Some("the server's declared concurrency: `start.serving.concurrency`")
        }
        Entry::Mismatch { .. } => {
            Some("a `regime.mismatch` event kind, carrying the settings the server contradicted")
        }
        Entry::Unverified { .. } => {
            Some("a `regime.unverified` event kind, carrying the pins nobody could check")
        }
        Entry::Stripped { .. } => Some(
            "a `regime.stripped` event kind: a retry that removed a pin changed the regime \
             the answer was produced under",
        ),
        Entry::TimedOut { .. } => Some(
            "a timeout as a typed outcome. Deliberately projected to NOTHING: a response \
             with an empty string is exactly the defect this client exists to stop, so a \
             timed-out request has no response row at all until the schema has one",
        ),
        Entry::Capped { .. } => Some("the cap a generation hit: `response.capped_at`"),
        Entry::Refused { .. } => {
            Some("an error status as a typed outcome: `response.refused`, with the status")
        }
        Entry::Failed { .. } => {
            Some("a transport failure as a typed outcome: `response.failed`, with the class")
        }
        Entry::Unreadable { .. } => {
            Some("a reply that could not be read as a typed outcome: `response.unreadable`")
        }
        Entry::Cache { .. } => Some(
            "cache telemetry per request: `response.cache`, with the prompt and reused \
             token counts and the path they were read from",
        ),
        // The retry LINK is spellable and is carried by the next request's
        // `retry_of`; only its reason is not, and that is noted on `Sent`
        // where the reason travels. `Sent` and `Received` are projected, and
        // what they lose is decided per-row inside the projection.
        Entry::Retried { .. } | Entry::Issued { .. } | Entry::Received { .. } => None,
    }
}

/// Project `journal` into record events, naming everything that does not fit.
///
/// The lane a request belongs to travels on the `Sent` entry, so this needs
/// nothing from outside the journal.
#[must_use]
pub fn project(journal: &Journal) -> Projection {
    let mut events = Vec::new();
    let mut unspellable: Vec<Unspellable> = Vec::new();
    // Deduplicated by kind AND by what, not by kind alone. One kind can lose
    // two different things -- a `request` loses both its sampler card and, when
    // it is a retry, its reason -- and deduping by kind would report whichever
    // came first and silently drop the other, which is the shape of loss this
    // whole function exists to make visible.
    let mut note = |kind: EntryKind, what: &'static str| {
        if !unspellable
            .iter()
            .any(|entry| entry.kind == kind && entry.what == what)
        {
            unspellable.push(Unspellable { kind, what });
        }
    };

    for entry in &journal.entries {
        match entry {
            Entry::Issued {
                id,
                lane,
                retry_of,
                because,
                sampler,
            } => {
                events.push(Event::Request {
                    id: id.clone(),
                    lane: lane.clone(),
                    retry_of: retry_of.clone(),
                    text: None,
                });
                if !sampler.is_empty() {
                    note(
                        EntryKind::Issued,
                        "the sampler card a request pinned: `request.sampler`",
                    );
                }
                if because.is_some() {
                    note(
                        EntryKind::Issued,
                        "why a retry exists: `request.retry_reason`, one of 4xx-strip, \
                         timeout, connection",
                    );
                }
                note(
                    EntryKind::Issued,
                    "the difference between a request the client ISSUED and one that \
                     reached the substrate: the record's `request` means the second, \
                     and a connection refused produces the first",
                );
            }
            Entry::Received {
                to_request,
                output_tokens,
                capped,
            } => {
                events.push(Event::Response {
                    id: response_id(to_request),
                    to_request: to_request.clone(),
                    // A count the server did not report is not zero. Zero is a
                    // measurement; this is its absence, and the record has one
                    // spelling for a count, so the absence is what is lost.
                    output_tokens: output_tokens.unwrap_or_default(),
                    text: None,
                });
                if output_tokens.is_none() {
                    note(
                        EntryKind::Received,
                        "a response whose token count the server did not report: \
                         `response.output_tokens` is required and zero is a measurement",
                    );
                }
                if *capped {
                    note(
                        EntryKind::Received,
                        "a typed outcome on a response: `response.outcome`, one of \
                         answered, capped",
                    );
                }
            }
            // Everything else loses all or nothing, and `lost` is the
            // exhaustive match: a new entry kind fails to compile there until
            // somebody decides whether the record can spell it.
            other => {
                if let Some(what) = lost(other) {
                    note(other.kind(), what);
                }
            }
        }
    }

    Projection {
        events,
        unspellable,
    }
}
