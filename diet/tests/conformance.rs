//! The conformance harness for every format in [`diet::formats`].
//!
//! This is the only place format conformance is asserted. It walks
//! `diet/formats/<name>/fixtures/`, requires that `valid/` and `invalid/`
//! account for every file in them, and checks each case against its pair:
//! a `valid/<case>.toml` must parse to exactly its `valid/<case>.expected.json`,
//! and an `invalid/<case>.toml` must be rejected.
//!
//! The fixture root can be redirected with `DISCIPLINE_FORMATS_DIR`, so a
//! candidate fixture tree can be checked without mutating the committed one.
//! `verify.sh --selftest` does not use it: it injects a non-conforming fixture
//! into a sandbox copy of the repository and runs the real `verify.sh` there,
//! so what goes red is the gate itself rather than a test-only path.

use std::path::{Path, PathBuf};

use diet::formats::FORMATS;
use diet::formats::regimen::{self, Value};
use serde_json::{Map, Value as Json};

/// Where fixtures live. `DISCIPLINE_FORMATS_DIR` overrides it so a seeded
/// fixture tree can be pointed at without editing the committed one.
fn formats_dir() -> PathBuf {
    std::env::var_os("DISCIPLINE_FORMATS_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("formats"),
        PathBuf::from,
    )
}

/// Parse `source` as `format` and render the result as tagged JSON, so that a
/// fixture pins the parsed *type* and not merely the parsed text.
///
/// Adding a name to [`FORMATS`] without wiring it here is a panic, not a
/// silently skipped format.
fn parse_to_json(format: &str, source: &str) -> Result<Json, String> {
    match format {
        "regimen" => regimen::parse(source)
            .map(|parsed| {
                let mut map = Map::new();
                for (key, value) in parsed.iter() {
                    let tagged = match value {
                        Value::String(text) => serde_json::json!({ "string": text }),
                        Value::Integer(number) => serde_json::json!({ "integer": number }),
                        Value::Boolean(flag) => serde_json::json!({ "boolean": flag }),
                    };
                    map.insert(key.to_owned(), tagged);
                }
                Json::Object(map)
            })
            .map_err(|err| err.to_string()),
        other => panic!("format `{other}` is listed in FORMATS but not wired into the harness"),
    }
}

/// Every file in `dir`, sorted, so failures are reported in a stable order.
fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("cannot read fixture directory {}: {err}", dir.display()))
        .map(|entry| {
            entry
                .expect("a readable directory yields readable entries")
                .path()
        })
        .collect();
    paths.sort();
    paths
}

/// The `.toml` cases in `dir`.
fn cases_in(dir: &Path) -> Vec<PathBuf> {
    files_in(dir)
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

fn report(failures: &[String]) {
    assert!(
        failures.is_empty(),
        "{} conformance failure(s):\n  {}",
        failures.len(),
        failures.join("\n  ")
    );
}

#[test]
fn every_format_has_a_grammar_and_a_populated_fixture_directory() {
    let root = formats_dir();
    let mut failures = Vec::new();

    for format in FORMATS {
        let grammar = root.join(format).join("grammar.pest");
        if !grammar.is_file() {
            failures.push(format!(
                "{format}: missing grammar at {}",
                grammar.display()
            ));
        }
        for bucket in ["valid", "invalid"] {
            let dir = root.join(format).join("fixtures").join(bucket);
            if !dir.is_dir() {
                failures.push(format!(
                    "{format}: missing fixture bucket {}",
                    dir.display()
                ));
                continue;
            }
            if cases_in(&dir).is_empty() {
                failures.push(format!("{format}: fixture bucket {bucket} holds no cases"));
            }
        }
    }

    report(&failures);
}

#[test]
fn every_fixture_file_is_paired() {
    let root = formats_dir();
    let mut failures = Vec::new();

    for format in FORMATS {
        let fixtures = root.join(format).join("fixtures");

        for case in cases_in(&fixtures.join("valid")) {
            let expected = case.with_extension("expected.json");
            if !expected.is_file() {
                failures.push(format!("{format}: {} has no expected.json", case.display()));
            }
        }
        for case in cases_in(&fixtures.join("invalid")) {
            let reason = case.with_extension("reason");
            if !reason.is_file() {
                failures.push(format!("{format}: {} has no .reason", case.display()));
            }
        }

        // The other direction: nothing in a bucket may be an orphan or a
        // stray. A fixture directory that quietly accumulates unread files is
        // a fixture directory that quietly stops covering things.
        for (bucket, companion) in [("valid", "expected.json"), ("invalid", "reason")] {
            for path in files_in(&fixtures.join(bucket)) {
                let name = path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or_default();
                if path.extension().is_some_and(|ext| ext == "toml") {
                    continue;
                }
                let Some(stem) = name.strip_suffix(&format!(".{companion}")) else {
                    failures.push(format!("{format}: stray file {}", path.display()));
                    continue;
                };
                if !fixtures.join(bucket).join(format!("{stem}.toml")).is_file() {
                    failures.push(format!("{format}: orphaned companion {}", path.display()));
                }
            }
        }
    }

    report(&failures);
}

#[test]
fn valid_fixtures_parse_to_their_expected_value() {
    let root = formats_dir();
    let mut failures = Vec::new();

    for format in FORMATS {
        for case in cases_in(&root.join(format).join("fixtures").join("valid")) {
            let expected_path = case.with_extension("expected.json");
            if !expected_path.is_file() {
                continue; // reported by `every_fixture_file_is_paired`
            }
            let expected: Json = match serde_json::from_str(&read(&expected_path)) {
                Ok(json) => json,
                Err(err) => {
                    failures.push(format!("{}: unreadable expectation: {err}", case.display()));
                    continue;
                }
            };
            match parse_to_json(format, &read(&case)) {
                Ok(actual) if actual == expected => {}
                Ok(actual) => failures.push(format!(
                    "{}: parsed to {actual}, expected {expected}",
                    case.display()
                )),
                Err(err) => failures.push(format!("{}: rejected: {err}", case.display())),
            }
        }
    }

    report(&failures);
}

#[test]
fn invalid_fixtures_are_rejected() {
    let root = formats_dir();
    let mut failures = Vec::new();

    for format in FORMATS {
        for case in cases_in(&root.join(format).join("fixtures").join("invalid")) {
            let reason = case.with_extension("reason");
            let reason = if reason.is_file() {
                read(&reason).trim().to_owned()
            } else {
                "no reason recorded".to_owned()
            };
            if let Ok(parsed) = parse_to_json(format, &read(&case)) {
                failures.push(format!(
                    "{}: accepted as {parsed}, but must be rejected: {reason}",
                    case.display()
                ));
            }
        }
    }

    report(&failures);
}
