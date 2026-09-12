//! Reading a foreign harness's session log into this crate's record types.
//!
//! The reference harness is not meant to replace the one you already use. The
//! adoption path is the opposite: point the library at your own harness and
//! see what the discipline would have captured from yesterday's work. That
//! needs adapters, and #28 is where they live.
//!
//! # What an adapter owes, and what it must never do
//!
//! A foreign log is not the record's subset and never will be. The record
//! admits no `null`, no exponents and eleven event kinds; a real Claude Code
//! session log carries `null` on its first line and **nine** top-level row
//! kinds, six of which this schema has no word for. So the interesting
//! question is not what an adapter maps. It is what it does with the rest.
//!
//! The answer this module enforces is a [`Census`]: every foreign kind is
//! counted, and every row is either MAPPED or UNMAPPED with nowhere else to
//! go. There is no third bucket and no silent drop -- which is the failure
//! the issue names, and the one practitioners running memory tools across
//! harnesses report: *the format moves under you and nothing says so.*
//!
//! # An unknown KIND is news; a missing FIELD is drift
//!
//! These are different failures and they get different answers, which is the
//! whole of [`Drift`]:
//!
//! * A row whose kind the adapter does not know is **counted and carried on
//!   from**. A harness that grew a new row type has not broken anything; the
//!   census says how much went uninterpreted and the operator decides whether
//!   that matters.
//! * A row whose kind the adapter DOES map, missing a field the mapping
//!   reads, is **a refusal**. That is the shape of a renamed field, and the
//!   one thing worse than not reading it is reading the wrong one. An adapter
//!   that shrugged here would map `message` to nothing and emit a turn that
//!   says the operator said nothing at all.
//!
//! So the adapter declares the fields it reads, and reads only the fields it
//! declares.
//!
//! # Lossy by declaration
//!
//! An adapted log is a VIEW of a foreign session, not a transcript of one.
//! The census is the measure of the difference and is meant to be read, not
//! filed away. Nothing here fabricates a value the foreign log did not carry:
//! where the record wants a number and the harness never said, the field is
//! absent rather than zero. A zero is a measurement.

pub mod claude_code;

use std::collections::BTreeMap;

use crate::formats::record::Event;

/// What a foreign log became, and what it cost to get there.
#[derive(Debug, Clone, PartialEq)]
pub struct Adapted {
    /// The events the log could be read as, in the order they occurred.
    pub events: Vec<Event>,
    /// What was seen, what was mapped, and what was not.
    pub census: Census,
}

/// Which foreign kinds an adapter saw, and which it could type.
///
/// The census is the adapter's answer to "what did you not tell me". It is
/// counted from the rows themselves rather than declared, so a kind that
/// appears once in a million-row log appears here once.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Census {
    /// The adapter that produced it.
    pub adapter: String,
    /// Foreign kind -> rows of that kind this adapter mapped.
    pub mapped: BTreeMap<String, u64>,
    /// Foreign kind -> rows of that kind this adapter had no word for.
    pub unmapped: BTreeMap<String, u64>,
    /// Content INSIDE a mapped row that the schema has no home for, by a
    /// qualified name such as `assistant/thinking`.
    ///
    /// Rows and content are counted separately on purpose. A row-level census
    /// alone would call an assistant row "mapped" and say nothing about the
    /// reasoning block inside it that went nowhere -- and in this repository's
    /// first real log that is five hundred and fifty-two blocks of the
    /// model's own thinking. Mapped is not the same as lossless, and a census
    /// that conflated them would be the reassuring kind.
    pub dropped: BTreeMap<String, u64>,
    /// Tool calls whose name this adapter rewrote into the vocabulary the
    /// capture lanes read, as `foreign -> native`.
    ///
    /// Declared in the census because a translation is an interpretation.
    /// A reader who disagrees with one can see it was made; a silent rename
    /// is a claim about a foreign contract with nothing standing behind it.
    pub translated: BTreeMap<String, u64>,
}

impl Census {
    /// Every row the log held, mapped or not.
    #[must_use]
    pub fn rows(&self) -> u64 {
        self.mapped_rows() + self.unmapped_rows()
    }

    /// Rows this adapter turned into events.
    #[must_use]
    pub fn mapped_rows(&self) -> u64 {
        self.mapped.values().sum()
    }

    /// Rows this adapter had no word for.
    #[must_use]
    pub fn unmapped_rows(&self) -> u64 {
        self.unmapped.values().sum()
    }

    /// Count one row of `kind` as mapped.
    pub fn mapped_one(&mut self, kind: &str) {
        *self.mapped.entry(kind.to_owned()).or_default() += 1;
    }

    /// Count one row of `kind` as having no mapping.
    pub fn unmapped_one(&mut self, kind: &str) {
        *self.unmapped.entry(kind.to_owned()).or_default() += 1;
    }

    /// Count one tool call whose name was translated into the lanes' vocabulary.
    pub fn translated_one(&mut self, foreign: &str, native: &str) {
        *self
            .translated
            .entry(format!("{foreign} -> {native}"))
            .or_default() += 1;
    }

    /// Count one piece of content inside a mapped row that has no schema home.
    pub fn dropped_one(&mut self, what: &str) {
        *self.dropped.entry(what.to_owned()).or_default() += 1;
    }

    /// Content inside mapped rows that went nowhere.
    #[must_use]
    pub fn dropped_content(&self) -> u64 {
        self.dropped.values().sum()
    }

    /// The census as one line of the record's JSON subset.
    ///
    /// Rendered here rather than through `serde_json` because this is output
    /// the gym reads back: no `null`, no exponents, keys in sorted order. The
    /// same discipline the record itself is held to.
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::from("{\"adapter\":");
        push_string(&mut out, &self.adapter);
        out.push_str(",\"dropped\":");
        push_counts(&mut out, &self.dropped);
        out.push_str(",\"dropped_content\":");
        out.push_str(&self.dropped_content().to_string());
        out.push_str(",\"mapped\":");
        push_counts(&mut out, &self.mapped);
        out.push_str(",\"mapped_rows\":");
        out.push_str(&self.mapped_rows().to_string());
        out.push_str(",\"rows\":");
        out.push_str(&self.rows().to_string());
        out.push_str(",\"translated\":");
        push_counts(&mut out, &self.translated);
        out.push_str(",\"unmapped\":");
        push_counts(&mut out, &self.unmapped);
        out.push_str(",\"unmapped_rows\":");
        out.push_str(&self.unmapped_rows().to_string());
        out.push('}');
        out
    }
}

/// A JSON object of `kind -> count`, keys sorted because the map is.
fn push_counts(out: &mut String, counts: &BTreeMap<String, u64>) {
    out.push('{');
    for (at, (kind, count)) in counts.iter().enumerate() {
        if at > 0 {
            out.push(',');
        }
        push_string(out, kind);
        out.push(':');
        out.push_str(&count.to_string());
    }
    out.push('}');
}

/// One JSON string, escaped the way the record's own writer escapes.
fn push_string(out: &mut String, text: &str) {
    use std::fmt::Write as _;
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Why an adapter refused a log rather than reading it wrong.
///
/// Every variant names the row it happened at, one-based, because the first
/// question of a refusal is always "where".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Drift {
    /// A line is not the JSON the adapter's format is made of.
    NotJson {
        /// One-based row number.
        at_row: usize,
        /// What the reader said.
        why: String,
    },
    /// A line is JSON but not an object, so it has no kind to dispatch on.
    NotAnObject {
        /// One-based row number.
        at_row: usize,
    },
    /// A row carries no kind at all, so nothing can be said about it --
    /// counted as unmapped would be a lie, since there is no kind to count.
    NoKind {
        /// One-based row number.
        at_row: usize,
    },
    /// A row of a kind this adapter maps is missing a field the mapping
    /// reads. **This is the renamed-field case**, and refusing is the point:
    /// the alternative is an event built from whatever was there instead.
    MissingField {
        /// One-based row number.
        at_row: usize,
        /// The foreign kind whose mapping wanted it.
        kind: String,
        /// The field, by the path the mapping reads.
        field: String,
    },
    /// A value the record's value space cannot hold, in a field the mapping
    /// carries across. The record admits no `null` and no exponents.
    Unrepresentable {
        /// One-based row number.
        at_row: usize,
        /// The field that held it.
        field: String,
        /// Why it will not fit.
        why: String,
    },
}

impl std::fmt::Display for Drift {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotJson { at_row, why } => {
                write!(f, "row {at_row} is not JSON: {why}")
            }
            Self::NotAnObject { at_row } => {
                write!(
                    f,
                    "row {at_row} is JSON but not an object, so it names no kind"
                )
            }
            Self::NoKind { at_row } => {
                write!(
                    f,
                    "row {at_row} carries no kind, so nothing can be counted for it"
                )
            }
            Self::MissingField {
                at_row,
                kind,
                field,
            } => write!(
                f,
                "row {at_row} is of kind `{kind}`, which this adapter maps, and it has no \
                 `{field}`. \
                 The format has moved: refusing rather than reading a different field into \
                 the one this mapping wanted."
            ),
            Self::Unrepresentable { at_row, field, why } => write!(
                f,
                "row {at_row}: `{field}` holds {why}, which the record's value space cannot spell"
            ),
        }
    }
}

impl std::error::Error for Drift {}

/// Reading one harness's session log into this crate's events.
pub trait Adapter {
    /// The name the census and the command line know it by.
    fn name(&self) -> &'static str;

    /// The foreign kinds this adapter maps, for the coverage test to read.
    ///
    /// Declared rather than derived so that a mapping quietly deleted is a
    /// difference between this list and what the census reports, rather than
    /// a silence nobody notices.
    fn maps(&self) -> &'static [&'static str];

    /// Read `log` into events and a census.
    ///
    /// # Errors
    ///
    /// [`Drift`] where the log is not the format this adapter reads, or where
    /// a row it maps is missing a field the mapping needs.
    fn adapt(&self, log: &str) -> Result<Adapted, Drift>;
}

#[cfg(test)]
mod tests {

    use super::{Adapter, Census, Drift, claude_code::ClaudeCode};

    #[test]
    fn a_census_counts_every_row_once_and_into_exactly_one_bucket() {
        let mut census = Census {
            adapter: "example".to_owned(),
            ..Census::default()
        };
        census.mapped_one("user");
        census.mapped_one("user");
        census.mapped_one("assistant");
        census.unmapped_one("mode");

        assert_eq!(census.mapped_rows(), 3);
        assert_eq!(census.unmapped_rows(), 1);
        assert_eq!(
            census.rows(),
            census.mapped_rows() + census.unmapped_rows(),
            "there is no third bucket: a row is mapped or it is not"
        );
        assert_eq!(census.rows(), 4);

        // Content loss is counted apart from rows, because a row can be
        // mapped and still have thrown something away.
        census.dropped_one("assistant/thinking");
        assert_eq!(census.dropped_content(), 1);
        assert_eq!(census.rows(), 4, "dropped content is not a row");
    }

    #[test]
    fn a_census_renders_as_one_line_of_the_records_own_subset() {
        let mut census = Census {
            adapter: "claude-code".to_owned(),
            ..Census::default()
        };
        census.mapped_one("assistant");
        census.unmapped_one("mode");

        let rendered = census.render();
        assert_eq!(
            rendered,
            "{\"adapter\":\"claude-code\",\"dropped\":{},\"dropped_content\":0,\
             \"mapped\":{\"assistant\":1},\"mapped_rows\":1,\
             \"rows\":2,\"translated\":{},\"unmapped\":{\"mode\":1},\"unmapped_rows\":1}"
        );
        // The census is read back by the gym, so it is held to the record's
        // own rules rather than to whatever a JSON writer happens to emit.
        assert!(
            !rendered.contains("null"),
            "the record's subset has no null"
        );
        assert!(!rendered.contains('\n'), "one line");
    }

    #[test]
    fn a_control_character_in_a_kind_is_escaped_rather_than_written_raw() {
        let mut census = Census::default();
        census.unmapped_one("a\u{1}b");
        assert!(
            census.render().contains("\"a\\u0001b\""),
            "a raw control character would make the census unparseable: {}",
            census.render()
        );
    }

    #[test]
    fn every_drift_says_which_row_it_happened_at() {
        let drifts = [
            Drift::NotJson {
                at_row: 1,
                why: "x".to_owned(),
            },
            Drift::NotAnObject { at_row: 2 },
            Drift::NoKind { at_row: 3 },
            Drift::MissingField {
                at_row: 4,
                kind: "user".to_owned(),
                field: "message".to_owned(),
            },
            Drift::Unrepresentable {
                at_row: 5,
                field: "f".to_owned(),
                why: "null".to_owned(),
            },
        ];
        for (at, drift) in drifts.iter().enumerate() {
            let said = drift.to_string();
            let row = at + 1;
            assert!(
                said.contains(&format!("row {row}")),
                "the first question of a refusal is where: {said}"
            );
        }
    }

    /// The lane's manifest still names source that is there.
    ///
    /// Written through `crate::gate` rather than copied from a neighbouring
    /// lane, because four copies of this check had already drifted apart by
    /// the time anybody compared them -- one never read `catches` at all.
    #[test]
    fn every_seeded_fault_still_names_source_that_is_there() {
        crate::gate::every_seeded_fault_still_names_source(
            include_str!("../../adapters/gate.toml"),
            // The lane's whole source, so a catcher can be looked for
            // wherever its test lives. `replay_cli.rs` is not optional here:
            // it holds the only tests that run `diet-replay`, and two faults
            // in this manifest mutate the binary and are caught by nothing
            // else.
            concat!(
                include_str!("mod.rs"),
                include_str!("claude_code.rs"),
                include_str!("../../tests/replay_cli.rs")
            ),
            "adapters",
        );
    }

    #[test]
    fn the_adapter_declares_the_kinds_it_maps() {
        let adapter = ClaudeCode;
        assert!(
            !adapter.maps().is_empty(),
            "an adapter that maps nothing is not an adapter"
        );
        assert_eq!(adapter.name(), "claude-code");
    }
}
