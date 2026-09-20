//! Row 1 and row 4 of #78, as ruled: the `wasm32-unknown-unknown` build of
//! the read side reaches the same verdict as native on every fixture in the
//! `record` and `regimen` corpora, *in a wasm runtime* -- and a host call
//! reachable from the read path fails that job rather than passing quietly.
//!
//! `diet::wasm::check_record`/`check_regimen` carry no logic of their own
//! (see that module) -- they are [`crate::formats::record::project`] and
//! [`crate::formats::regimen::project`] with a `#[wasm_bindgen]` attribute,
//! nothing else. So calling them in-process, compiled for THIS test
//! binary's own native target, is calling the identical code the wasm32
//! build ships; the comparison below is the same function on two targets,
//! not two functions that happen to agree today. Both sides render through
//! the record's own canonical `render`, so the bar is byte-for-byte string
//! equality, not a second JSON-equality reader that could itself disagree
//! with the first.
//!
//! The wasm32 side runs under Node (`wasm-bindgen --target nodejs` +
//! `node`), the same pairing #78's own investigation measured: `fs`,
//! `process`, and `net` calls degrade to a returned `Err` at this boundary
//! rather than trapping, so `Instant`-style panics are the only calls that
//! genuinely trap on their own. Row 4's control does not rely on finding
//! one of those -- it seeds a host call **and unwraps its `Result`**
//! (`wasm::seeded_host_call_trap`), the shape a real regression on this
//! path would actually take, and that traps regardless of which stdlib call
//! it is.
//!
//! This test builds both wasm32 artifacts itself (`cargo build` + a
//! `wasm-bindgen` subprocess) rather than assuming a CI step already did --
//! `cargo test --features wasm` reproduces the whole claim locally, on a
//! contributor's machine, the same "gate seen red before it is trusted"
//! standard the seeded-fault manifests hold to. It requires the
//! `wasm32-unknown-unknown` target and a `wasm-bindgen` binary on `PATH`
//! whose version matches the pinned crate version exactly (see the
//! `Cargo.toml` comment on why that pin is exact); missing either is a
//! loud failure, not a skip -- a conformance job that passes because its
//! own toolchain silently was not there is the isolation lane's `bwrap`
//! lesson wearing a different lane's clothes.

#![cfg(feature = "wasm")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../target/wasm-conformance")
}

/// Every file directly in `dir` whose extension is exactly `ext`, sorted.
fn fixtures(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("cannot read fixture directory {}: {err}", dir.display()))
        .map(|entry| {
            entry
                .expect("a readable directory yields readable entries")
                .path()
        })
        .filter(|path| path.extension().is_some_and(|found| found == ext))
        .collect();
    paths.sort();
    paths
}

/// Build the wasm32 artifact for `features` (always includes `wasm`) and run
/// `wasm-bindgen --target nodejs` over it. Returns the path to the generated
/// Node glue (`<crate>.js`).
///
/// One `--target-dir` per distinct feature set, so the plain conformance
/// build and the seeded-trap build -- which must not carry the same code --
/// cannot race on or clobber each other's cached artifacts.
fn build_and_bind(label: &str, features: &str) -> PathBuf {
    let target_dir = target_dir().join(label);
    let status = Command::new("cargo")
        .current_dir(crate_root())
        .args([
            "build",
            "--target",
            "wasm32-unknown-unknown",
            "--no-default-features",
            "--features",
            features,
            "--lib",
            "--target-dir",
        ])
        .arg(&target_dir)
        .status()
        .unwrap_or_else(|err| panic!("could not run cargo build: {err}"));
    assert!(
        status.success(),
        "cargo build --target wasm32-unknown-unknown --features {features} failed; \
         is the wasm32-unknown-unknown target installed (`rustup target add wasm32-unknown-unknown`)?"
    );

    let wasm_artifact = target_dir.join("wasm32-unknown-unknown/debug/diet.wasm");
    assert!(
        wasm_artifact.is_file(),
        "cargo build succeeded but {} is missing",
        wasm_artifact.display()
    );

    let out_dir = target_dir.join("bindgen-out");
    let status = Command::new("wasm-bindgen")
        .args(["--target", "nodejs", "--out-dir"])
        .arg(&out_dir)
        .arg(&wasm_artifact)
        .status()
        .unwrap_or_else(|err| {
            panic!(
                "could not run `wasm-bindgen` ({err}); install the exact version \
                 diet/Cargo.toml pins with `cargo install wasm-bindgen-cli --version 0.2.100 --locked`"
            )
        });
    assert!(
        status.success(),
        "wasm-bindgen --target nodejs over {} failed",
        wasm_artifact.display()
    );

    let glue = out_dir.join("diet.js");
    assert!(
        glue.is_file(),
        "wasm-bindgen did not produce {}",
        glue.display()
    );
    glue
}

fn plain_glue() -> &'static Path {
    static GLUE: OnceLock<PathBuf> = OnceLock::new();
    GLUE.get_or_init(|| build_and_bind("plain", "wasm"))
}

fn seeded_trap_glue() -> &'static Path {
    static GLUE: OnceLock<PathBuf> = OnceLock::new();
    GLUE.get_or_init(|| build_and_bind("seeded-trap", "wasm,wasm-seeded-host-call-trap"))
}

struct Line {
    id: String,
    ok: bool,
    result: Option<String>,
    error: Option<String>,
}

/// Hand-parsed rather than derived: this crate depends on `serde_json`, not
/// on `serde` with its derive feature, and one three-field, four-line
/// struct is not a reason to add it.
fn parse_line(text: &str) -> Line {
    let value: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|err| panic!("harness printed an unreadable line {text:?}: {err}"));
    let field = |name: &str| {
        value
            .get(name)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    Line {
        id: field("id").unwrap_or_default(),
        ok: value
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        result: field("result"),
        error: field("error"),
    }
}

/// Call `(func, path)` for every `(id, func, path)` in `calls`, batched
/// through one `node` process rather than one per fixture. `Ok` holds every
/// entry's raw result string; a call that threw in Node surfaces as an
/// `Err` naming which id and why, since that shape is a trap and every
/// caller here treats it as this test's own failure to explain, not a
/// value to compare.
fn call_wasm_batch(glue: &Path, calls: &[(String, String, PathBuf)]) -> BTreeMap<String, String> {
    let manifest: Vec<serde_json::Value> = calls
        .iter()
        .map(|(id, func, path)| {
            serde_json::json!({"id": id, "func": func, "path": path.to_string_lossy()})
        })
        .collect();
    let manifest_path = glue.with_file_name("manifest.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string(&manifest).expect("a Vec<Value> always serializes"),
    )
    .unwrap_or_else(|err| panic!("cannot write {}: {err}", manifest_path.display()));

    let harness = crate_root().join("tests/wasm_conformance_harness.js");
    let output = Command::new("node")
        .arg(&harness)
        .arg(glue)
        .arg(&manifest_path)
        .output()
        .unwrap_or_else(|err| panic!("could not run node ({err}); is Node.js on PATH?"));
    assert!(
        output.status.success(),
        "the harness process itself failed (exit {:?}):\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = BTreeMap::new();
    for line in stdout.lines() {
        let parsed = parse_line(line);
        if parsed.ok {
            results.insert(parsed.id, parsed.result.expect("ok line carries a result"));
        } else {
            panic!(
                "wasm call for {} threw rather than returning: {}",
                parsed.id,
                parsed.error.unwrap_or_default()
            );
        }
    }
    results
}

/// The native reference: the identical `diet::wasm::*` function, compiled
/// for this test binary's own target.
fn native_verdict(func: &str, source: &str) -> String {
    match func {
        "check_record" => diet::wasm::check_record(source),
        "check_regimen" => diet::wasm::check_regimen(source),
        other => panic!("no native counterpart wired for {other}"),
    }
}

/// Fixtures this boundary cannot represent at all, named rather than
/// detected-and-skipped silently.
///
/// `check_record`/`check_regimen` take `&str`: the native `project` path
/// reads *bytes* and treats invalid UTF-8 as its own rejection reason
/// (`formats/record/fixtures/invalid/not-utf8.jsonl` pins exactly that), but
/// a `&str` argument at the wasm-bindgen boundary is a JS string that has
/// already been decoded -- there is no byte sequence a JS string can carry
/// that is not valid UTF-8, the same way a browser's own `File`/`fetch` text
/// APIs work. So this fixture's own claim is not reproducible through a
/// string boundary; excluding it is a fact about the boundary's shape, not
/// a gap in what got tested. Named, and length-checked below, so a SECOND
/// non-UTF-8 fixture added later fails loudly here rather than silently
/// joining an unbounded skip list.
fn unrepresentable_at_this_boundary(format_dir: &str) -> &'static [&'static str] {
    match format_dir {
        "record" => &["invalid/not-utf8.jsonl"],
        _ => &[],
    }
}

fn assert_corpus_matches(format_dir: &str, ext: &str, func: &str) {
    let root = crate_root()
        .join("formats")
        .join(format_dir)
        .join("fixtures");
    let excluded = unrepresentable_at_this_boundary(format_dir);
    let mut seen_excluded = Vec::new();
    let mut calls = Vec::new();
    let mut cases = Vec::new();
    for bucket in ["valid", "invalid"] {
        for case in fixtures(&root.join(bucket), ext) {
            let id = format!("{bucket}/{}", case.file_name().unwrap().to_string_lossy());
            if excluded.contains(&id.as_str()) {
                seen_excluded.push(id);
                continue;
            }
            calls.push((id.clone(), func.to_owned(), case.clone()));
            cases.push((id, case));
        }
    }
    assert_eq!(
        seen_excluded.len(),
        excluded.len(),
        "{format_dir}: expected to exclude exactly {excluded:?}, actually excluded {seen_excluded:?} \
         -- a name here no longer matches a fixture on disk, or a new non-UTF-8 fixture needs naming"
    );
    assert!(
        !cases.is_empty(),
        "{format_dir}: no *.{ext} fixtures found under {}; this test would compare nothing",
        root.display()
    );

    let wasm_results = call_wasm_batch(plain_glue(), &calls);

    let mut failures = Vec::new();
    for (id, case) in cases {
        let source = std::fs::read_to_string(&case)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", case.display()));
        let native = native_verdict(func, &source);
        let Some(wasm) = wasm_results.get(&id) else {
            failures.push(format!("{id}: the wasm batch produced no result at all"));
            continue;
        };
        if *wasm != native {
            failures.push(format!(
                "{id}: native and wasm disagree\n    native: {native}\n    wasm:   {wasm}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} {format_dir} fixture(s) disagree between native and wasm:\n  {}",
        failures.len(),
        calls.len(),
        failures.join("\n  ")
    );
}

#[test]
fn record_corpus_matches_between_native_and_wasm() {
    assert_corpus_matches("record", "jsonl", "check_record");
}

#[test]
fn regimen_corpus_matches_between_native_and_wasm() {
    assert_corpus_matches("regimen", "toml", "check_regimen");
}

/// Row 4's control, proved rather than assumed: a host call reachable from
/// the read path must fail the job. `wasm::seeded_host_call_trap` is that
/// call, built only into this test's own separate artifact -- never the
/// artifact `record_corpus_matches_between_native_and_wasm` exercises, and
/// never a shipped one, since the feature it lives behind is never enabled
/// except here.
#[test]
fn the_seeded_host_call_trap_actually_fails_the_job() {
    let glue = seeded_trap_glue();
    let manifest_path = glue.with_file_name("seeded-manifest.json");
    let manifest = serde_json::json!([{"id": "seeded", "func": "seeded_host_call_trap", "path": crate_root().join("Cargo.toml").to_string_lossy()}]);
    std::fs::write(&manifest_path, serde_json::to_string(&manifest).unwrap())
        .unwrap_or_else(|err| panic!("cannot write {}: {err}", manifest_path.display()));

    let harness = crate_root().join("tests/wasm_conformance_harness.js");
    let output = Command::new("node")
        .arg(&harness)
        .arg(glue)
        .arg(&manifest_path)
        .output()
        .unwrap_or_else(|err| panic!("could not run node ({err}); is Node.js on PATH?"));
    assert!(
        output.status.success(),
        "the harness process itself failed unexpectedly (exit {:?})",
        output.status.code()
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let line = parse_line(stdout.trim());
    assert!(
        !line.ok,
        "seeded_host_call_trap returned {:?} instead of trapping -- row 4's control does not control",
        line.result
    );
    let error = line.error.unwrap_or_default();
    assert!(
        error.contains("unreachable"),
        "seeded_host_call_trap failed, but not with a wasm trap: {error}"
    );
}
