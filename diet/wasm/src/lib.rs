//! The browser-callable read side of `diet` (#78).
//!
//! `check_record` and `check_regimen` are the wasm-facing form of
//! `diet check-record` and `diet check-regimen` -- pure pass-throughs to
//! [`diet::formats::record::project`] and [`diet::formats::regimen::project`].
//! This crate adds no logic of its own on purpose: the conformance test in
//! `tests/wasm_conformance.rs` claims "the same function, compiled for two
//! targets, reaches the same verdict," and that claim is only true if this
//! crate does not reimplement anything the native path already does.
//!
//! Both functions are string in, string out -- no `Path`, no file handle, no
//! process. That is not a restriction this crate adds; it is what was
//! already true of `project` on both formats before this crate existed.
//!
//! Empty unless the `wasm` feature is on, so the workspace-wide native build
//! and test run `verify.sh` performs compile nothing here. Its own crate
//! rather than a module of `discipline-diet` for the `cdylib` alone -- see
//! this crate's `Cargo.toml`.

#![cfg(feature = "wasm")]

use wasm_bindgen::prelude::wasm_bindgen;

use diet::formats::record::json::{Value, render};

/// The record's own JSON-shaped envelope around a projection's verdict:
/// `{"ok":true,"value":<projected value>}` or `{"ok":false,"error":<reason>}`.
///
/// Rendered with the record's own [`render`] rather than a general-purpose
/// JSON writer, on both branches, so the error string gets the same escaping
/// the record's value space already defines instead of a second one hand-
/// rolled for this boundary.
fn envelope(result: Result<Value, String>) -> String {
    let mut out = String::new();
    match result {
        Ok(value) => {
            out.push_str(r#"{"ok":true,"value":"#);
            render(&value, &mut out);
            out.push('}');
        }
        Err(error) => {
            out.push_str(r#"{"ok":false,"error":"#);
            render(&Value::String(error), &mut out);
            out.push('}');
        }
    }
    out
}

/// The browser-callable form of `diet check-record`.
#[wasm_bindgen]
#[must_use]
pub fn check_record(source: &str) -> String {
    envelope(diet::formats::record::project(source))
}

/// The browser-callable form of `diet check-regimen`.
#[wasm_bindgen]
#[must_use]
pub fn check_regimen(source: &str) -> String {
    envelope(diet::formats::regimen::project(source))
}

/// Exists only to prove the conformance job's row-4 control against a real
/// failure rather than an assumed one: a host call reachable from the read
/// path must fail the job, not pass silently.
///
/// Built only when `wasm-seeded-host-call-trap` is explicitly enabled --
/// never in a shipped artifact, never by the plain `wasm` feature -- and
/// asserted to fail the calling `node` process by
/// `tests/wasm_conformance.rs::the_seeded_host_call_trap_actually_fails_the_job`.
///
/// `std::fs::read_to_string(...).unwrap()` is the shape of a real regression
/// here, not a synthetic one: on `wasm32-unknown-unknown` the read itself
/// returns `Err` rather than trapping (measured directly, disclosed on #78)
/// -- it is the `.unwrap()` on that `Err`, the same mistake an accidental
/// host call reaching this module would actually make, that panics and
/// traps.
///
/// # Panics
///
/// Always, on the `Err` `std::fs::read_to_string` returns on this target.
/// That panic is the entire point: see above.
#[cfg(feature = "wasm-seeded-host-call-trap")]
#[wasm_bindgen]
#[must_use]
pub fn seeded_host_call_trap() -> String {
    std::fs::read_to_string("/this-path-does-not-exist-and-is-the-point").unwrap();
    String::new()
}
