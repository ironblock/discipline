//! Row 1 and row 4 of #78, as ruled: the `wasm32-unknown-unknown` build of
//! the read side reaches the same verdict as native on every fixture in the
//! `record` and `regimen` corpora, *in a wasm runtime* -- and a host call
//! reachable from the read path fails that job rather than passing quietly.
//!
//! `diet_wasm::check_record`/`check_regimen` carry no logic of their own
//! (see that crate) -- they are `diet::formats::record::project` and
//! `diet::formats::regimen::project` with a `#[wasm_bindgen]` attribute,
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
//! (`diet_wasm::seeded_host_call_trap`), the shape a real regression on this
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
//! `diet/wasm/Cargo.toml` comment on why that pin is exact); missing either is a
//! loud failure, not a skip -- a conformance job that passes because its
//! own toolchain silently was not there is the isolation lane's `bwrap`
//! lesson wearing a different lane's clothes.
//!
//! Row 3 lives here too rather than in a file of its own: it is the same
//! `wasm-bindgen` invocation this file already runs, checked for one more
//! property. A generator whose output depended on its caller's `$PWD` would
//! drift the checked-in bindings the instant CI's checkout path or a
//! contributor's shell differed from whoever last ran it -- so the claim is
//! not "the bindings look right," it's "the bindings are the same bytes
//! regardless of where this ran from," proved by literally running it twice
//! from two different directories and diffing the output.

#![cfg(feature = "wasm")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn crate_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

/// `diet/`, where the `record` and `regimen` conformance corpora live -- this
/// crate is a member nested inside it, not a second home for the fixtures.
fn diet_root() -> PathBuf {
    crate_root().join("..")
}

fn target_dir() -> PathBuf {
    crate_root().join("../../target/wasm-conformance")
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

/// Build the wasm32 artifact for `features` (always includes `wasm`).
/// Returns the path to the produced `.wasm`.
///
/// One `--target-dir` per distinct feature set, so the plain conformance
/// build and the seeded-trap build -- which must not carry the same code --
/// cannot race on or clobber each other's cached artifacts.
fn build_wasm(label: &str, features: &str) -> PathBuf {
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

    let wasm_artifact = target_dir.join("wasm32-unknown-unknown/debug/diet_wasm.wasm");
    assert!(
        wasm_artifact.is_file(),
        "cargo build succeeded but {} is missing",
        wasm_artifact.display()
    );
    wasm_artifact
}

/// Run `wasm-bindgen --target nodejs` over `wasm_artifact` into `out_dir`,
/// itself launched from `cwd` -- both `wasm_artifact` and `out_dir` are
/// absolute, so `cwd` has no reason to matter, and row 3 exists to check
/// that belief rather than assume it. Returns the generated Node glue
/// (`diet_wasm.js`).
fn bind_nodejs(wasm_artifact: &Path, out_dir: &Path, cwd: &Path) -> PathBuf {
    let status = Command::new("wasm-bindgen")
        .current_dir(cwd)
        .args(["--target", "nodejs", "--out-dir"])
        .arg(out_dir)
        .arg(wasm_artifact)
        .status()
        .unwrap_or_else(|err| {
            panic!(
                "could not run `wasm-bindgen` ({err}); install the exact version \
                 diet/wasm/Cargo.toml pins with `cargo install wasm-bindgen-cli --version 0.2.100 --locked`"
            )
        });
    assert!(
        status.success(),
        "wasm-bindgen --target nodejs over {} (cwd {}) failed",
        wasm_artifact.display(),
        cwd.display()
    );

    let glue = out_dir.join("diet_wasm.js");
    assert!(
        glue.is_file(),
        "wasm-bindgen did not produce {}",
        glue.display()
    );
    glue
}

fn build_and_bind(label: &str, features: &str) -> PathBuf {
    let wasm_artifact = build_wasm(label, features);
    let out_dir = target_dir().join(label).join("bindgen-out");
    bind_nodejs(&wasm_artifact, &out_dir, &crate_root())
}

/// Cached separately from [`plain_glue`]'s bound output, so
/// `ts_bindings_are_byte_identical_from_two_different_working_directories`
/// can bind the same `.wasm` a second time without a second `cargo build`
/// subprocess racing this one under CI's default parallel test threading.
fn plain_wasm_artifact() -> &'static Path {
    static ARTIFACT: OnceLock<PathBuf> = OnceLock::new();
    ARTIFACT.get_or_init(|| build_wasm("plain", "wasm"))
}

fn plain_glue() -> &'static Path {
    static GLUE: OnceLock<PathBuf> = OnceLock::new();
    GLUE.get_or_init(|| {
        let out_dir = target_dir().join("plain").join("bindgen-out");
        bind_nodejs(plain_wasm_artifact(), &out_dir, &crate_root())
    })
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
///
/// `label` names this call's own manifest file, distinctly from any other
/// call sharing the same `glue`. Two of this file's tests share
/// [`plain_glue`]'s single cached artifact and run concurrently -- Rust
/// tests are parallel by default, and only this crate's own local runs
/// happened to pass `--test-threads=1`. A manifest path built from `glue`
/// alone let both tests race on writing the same file: measured on CI,
/// which does not pass that flag, as "the wasm batch produced no result at
/// all" for every regimen fixture, not a native/wasm disagreement.
fn call_wasm_batch(
    glue: &Path,
    label: &str,
    calls: &[(String, String, PathBuf)],
) -> BTreeMap<String, String> {
    let manifest: Vec<serde_json::Value> = calls
        .iter()
        .map(|(id, func, path)| {
            serde_json::json!({"id": id, "func": func, "path": path.to_string_lossy()})
        })
        .collect();
    let manifest_path = glue.with_file_name(format!("manifest-{label}.json"));
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

/// The native reference: the identical `diet_wasm::*` function, compiled
/// for this test binary's own target.
fn native_verdict(func: &str, source: &str) -> String {
    match func {
        "check_record" => diet_wasm::check_record(source),
        "check_regimen" => diet_wasm::check_regimen(source),
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
    let root = diet_root()
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

    let wasm_results = call_wasm_batch(plain_glue(), format_dir, &calls);

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
/// the read path must fail the job. `diet_wasm::seeded_host_call_trap` is that
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

/// Every file under `dir`, walked recursively, as paths relative to `dir`
/// and sorted. Not assumed flat: `wasm-bindgen --target nodejs` writes a
/// `snippets/<hash>/...` subdirectory whenever a binding uses `inline_js`
/// or a local JS module -- not true of this crate's bindings today, but a
/// listing that only saw top-level entries would let a future nested file
/// go uncompared, or crash trying to `read` a directory as if it were one.
fn relative_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, prefix: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir)
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        {
            let entry = entry.expect("a readable directory yields readable entries");
            let relative = prefix.join(entry.file_name());
            let file_type = entry
                .file_type()
                .unwrap_or_else(|err| panic!("cannot stat {}: {err}", entry.path().display()));
            if file_type.is_dir() {
                walk(&entry.path(), &relative, out);
            } else {
                out.push(relative);
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, Path::new(""), &mut out);
    out.sort();
    out
}

/// `relative_files` itself, against a manufactured nested tree -- the exact
/// shape (`snippets/<hash>/...`) a fresh-instance review of this test found
/// its earlier, non-recursive directory listing would either crash on or
/// silently fail to compare, and that this crate's own bindings do not
/// happen to produce today.
#[test]
fn relative_files_walks_into_subdirectories() {
    let root = target_dir().join("relative-files-selftest");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("snippets/abc123"))
        .unwrap_or_else(|err| panic!("cannot create {}: {err}", root.display()));
    std::fs::write(root.join("diet_wasm.js"), b"top-level").unwrap();
    std::fs::write(root.join("snippets/abc123/inline.js"), b"nested").unwrap();

    assert_eq!(
        relative_files(&root),
        vec![
            PathBuf::from("diet_wasm.js"),
            PathBuf::from("snippets/abc123/inline.js"),
        ],
        "the walk must find the top-level file AND the one nested under snippets/"
    );
}

/// Row 3: `wasm-bindgen --target nodejs`, invoked twice from two different
/// working directories against the same `.wasm` artifact, produces
/// byte-identical output. Compares every file the generator writes,
/// including nested ones, named by a recursive directory walk rather than a
/// guessed set, so a future `wasm-bindgen` version adding, renaming, or
/// nesting an output file is caught by this test noticing the two listings
/// still match each other -- not by a hardcoded, non-recursive shape
/// silently going uncompared.
#[test]
fn ts_bindings_are_byte_identical_from_two_different_working_directories() {
    let wasm_artifact = plain_wasm_artifact();
    let root = target_dir().join("plain").join("bindgen-cwd-proof");

    let out_a = root.join("from-crate-root");
    let cwd_a = crate_root();
    let out_b = root.join("from-workspace-root");
    let cwd_b = std::fs::canonicalize(crate_root().join("../.."))
        .expect("the workspace root, two levels above diet/wasm/, exists");
    assert_ne!(
        std::fs::canonicalize(&cwd_a).expect("the crate root exists"),
        cwd_b,
        "the two runs must actually differ in cwd"
    );

    bind_nodejs(wasm_artifact, &out_a, &cwd_a);
    bind_nodejs(wasm_artifact, &out_b, &cwd_b);

    let names_a = relative_files(&out_a);
    let names_b = relative_files(&out_b);
    assert_eq!(
        names_a, names_b,
        "wasm-bindgen wrote a different set of files depending on cwd"
    );
    assert!(!names_a.is_empty(), "wasm-bindgen wrote nothing to compare");

    let mut mismatches = Vec::new();
    for name in &names_a {
        let bytes_a = std::fs::read(out_a.join(name))
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", out_a.join(name).display()));
        let bytes_b = std::fs::read(out_b.join(name))
            .unwrap_or_else(|err| panic!("cannot read {}: {err}", out_b.join(name).display()));
        if bytes_a != bytes_b {
            mismatches.push(name.display().to_string());
        }
    }
    assert!(
        mismatches.is_empty(),
        "cwd-dependent output in: {}\n  {} (cwd {})\n  {} (cwd {})",
        mismatches.join(", "),
        out_a.display(),
        cwd_a.display(),
        out_b.display(),
        cwd_b.display()
    );
}
