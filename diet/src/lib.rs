//! `diet` — the dogma and core mechanics of discipline.
//!
//! A *regimen* is a versioned, fixed combination of variables describing the
//! character, performance, and timing of an agentic coding session, live or
//! replayed. This crate owns the regimen and the formats it is expressed in.

// Reading a foreign harness's session log into this crate's types (#28): the
// adoption path is pointing the library at the harness you already use.
pub mod adapters;
pub mod capture;
pub mod client;
pub mod digest;
pub mod dogma;
pub mod drive;
pub mod formats;
pub mod isolation;
pub mod object;
pub mod seam;
