//! Parsed formats.
//!
//! The convention, which every format added here must follow:
//!
//! * Exactly one grammar, expressed as data, at
//!   `diet/formats/<name>/grammar.pest`. The grammar file is normative; the
//!   Rust parser implements it and is not itself the definition.
//! * Exactly one conformance-fixture directory at
//!   `diet/formats/<name>/fixtures/`, containing `valid/` and `invalid/`.
//!   Every `valid/<case>.toml` is paired with `valid/<case>.expected.json`
//!   stating what it must parse to. Every `invalid/<case>.toml` is paired
//!   with `invalid/<case>.reason` stating why it must be rejected.
//!
//! `diet/tests/conformance.rs` walks that directory and is the only place
//! conformance is asserted. Adding a format means adding a grammar, a
//! fixture directory, and a line in [`FORMATS`] — not a new test file.

pub mod regimen;

/// Every format with a grammar and a conformance-fixture directory.
///
/// The conformance harness iterates this list; a format absent from it is
/// not covered, which is why it lives next to the modules rather than in the
/// test.
pub const FORMATS: &[&str] = &["regimen"];
