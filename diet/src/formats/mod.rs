//! Parsed formats.
//!
//! The convention, which every format added here must follow:
//!
//! * Exactly one grammar, expressed as data, at
//!   `diet/formats/<name>/grammar.pest`. The grammar file is normative; the
//!   Rust parser implements it and is not itself the definition.
//! * Exactly one conformance-fixture directory at
//!   `diet/formats/<name>/fixtures/`, containing `valid/` and `invalid/`.
//!   Every `valid/<case>.<ext>` is paired with `valid/<case>.expected.json`
//!   stating what it must parse to. Every `invalid/<case>.<ext>` is paired
//!   with `invalid/<case>.reason` stating why it must be rejected.
//! * Exactly one authorized implementation. Anything else that must read the
//!   format calls this crate or passes this corpus; a second reader that does
//!   neither is not an implementation of the format, it is a second opinion.
//!
//! `diet/tests/conformance.rs` walks that directory and is the only place
//! conformance is asserted. Adding a format means adding a grammar, a fixture
//! directory, an entry in [`FORMATS`] and a name in the harness's
//! `per_format!` list -- not a new test file.

pub mod decline;
pub mod interview;
pub mod record;
pub mod regimen;
pub mod shell;

/// A format with a grammar and a conformance-fixture directory.
#[derive(Debug, Clone, Copy)]
pub struct Format {
    /// Directory name under `diet/formats/`, and the name the harness
    /// dispatches on.
    pub name: &'static str,
    /// Parse `source` and render the result as the record's value space.
    ///
    /// A function pointer, not a name the caller matches on. The conformance
    /// harness used to dispatch on `name` with a string match that panicked on
    /// an unknown one -- a guard, but a guard that fires at run time. A format
    /// added to this list without a projection now fails to compile, because
    /// the struct literal has nowhere to leave the field blank.
    pub project: fn(&str) -> Result<record::json::Value, String>,
    /// Extension of a fixture case file, without the dot.
    ///
    /// Carried as data rather than assumed. The first format was TOML and the
    /// harness hard-coded that; a corpus with any other extension would have
    /// been walked as zero cases, which the empty-bucket assertion does catch
    /// -- but only as "no fixtures at all", never as "the wrong files".
    pub case_extension: &'static str,
}

/// Every format with a grammar and a conformance-fixture directory.
///
/// The conformance harness iterates this list; a format absent from it is not
/// covered, which is why it lives next to the modules rather than in the test.
pub const FORMATS: &[Format] = &[
    Format {
        name: "decline",
        case_extension: "txt",
        project: decline::project,
    },
    Format {
        name: "interview",
        case_extension: "txt",
        project: interview::project,
    },
    Format {
        name: "record",
        case_extension: "jsonl",
        project: record::project,
    },
    Format {
        name: "regimen",
        case_extension: "toml",
        project: regimen::project,
    },
    Format {
        name: "shell",
        case_extension: "sh",
        project: shell::project,
    },
];

/// The format called `name`, if there is one.
#[must_use]
pub fn format(name: &str) -> Option<&'static Format> {
    FORMATS.iter().find(|format| format.name == name)
}
