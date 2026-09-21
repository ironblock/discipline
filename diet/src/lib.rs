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
// Test support shared by every lane's drift guard. Ruled onto the crate
// root: a second copy of a checker in test code is the two-readers class
// wearing a `#[cfg(test)]`, and four copies of this one had already drifted.
#[cfg(test)]
mod gate;
pub mod isolation;
pub mod object;
pub mod seam;
// The browser-callable read side (#78): a thin, pure pass-through to
// `formats::record`/`formats::regimen`, so the conformance test's claim is
// "the same function, two targets" rather than "two functions that agree
// today."
#[cfg(feature = "wasm")]
pub mod wasm;
