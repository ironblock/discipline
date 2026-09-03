//! The CLI is the boundary this crate exists to publish, and it was the one
//! artefact nothing ran.
//!
//! `project()` is exercised through the conformance harness, which calls it as
//! a function. The exit codes, the JSON on stdout, and *which format sits
//! behind which verb* are only observable by running the program -- and until
//! this file existed, none of them was. Three mutations survived the whole
//! gate, `--selftest` included: a usage error that exits 0, a dispatch that
//! sends every verb to the same format, and a run that prints nothing at all.
//!
//! The binary's own `every_format_has_a_command_and_every_command_a_format`
//! cannot catch the second of those, because it checks the command table
//! against itself: `format_for` derives the format name from the same tuple
//! the assertion compares it to, so a table with two rows swapped passes.
//! What catches it is data. Running `parse-interview` over a file from
//! `formats/interview/fixtures/valid/` and requiring exit 0 is not a claim
//! about a table -- a regimen reader handed an interview answer fails.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The verb each format is published under.
///
/// Written from the CLI's usage text rather than read from its table, so that
/// the two are able to disagree. A second copy of a table is usually a defect;
/// here it is the instrument.
const VERBS: &[(&str, &str)] = &[
    ("classify-decline", "decline"),
    ("parse-interview", "interview"),
    ("check-record", "record"),
    ("check-regimen", "regimen"),
    ("parse-shell", "shell"),
    ("parse-verdict", "verdict"),
];

/// The binary cargo built for this test run. Not a path this file composes:
/// a hand-built path is how a gate ends up running something else.
const DIET: &str = env!("CARGO_BIN_EXE_diet");

fn formats_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("formats")
}

/// Run the binary and report what it did: exit code, stdout, stderr.
fn run(args: &[&str]) -> (i32, String, String) {
    let output = Command::new(DIET)
        .args(args)
        .output()
        .unwrap_or_else(|err| panic!("{DIET} did not run: {err}"));
    (
        output
            .status
            .code()
            .expect("the process exited rather than being signalled"),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// Every case file in one of a format's fixture buckets.
fn cases(format: &str, bucket: &str) -> Vec<PathBuf> {
    let extension = diet::formats::format(format)
        .unwrap_or_else(|| panic!("`{format}` is a declared format"))
        .case_extension;
    let dir = formats_dir().join(format).join("fixtures").join(bucket);
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("{}: {err}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == extension))
        .collect();
    found.sort();
    assert!(
        !found.is_empty(),
        "{}: no cases, so every assertion over this bucket would hold vacuously",
        dir.display()
    );
    found
}

fn as_text(path: &Path) -> String {
    path.to_str()
        .unwrap_or_else(|| panic!("{} is not UTF-8", path.display()))
        .to_owned()
}

#[test]
fn every_verb_reads_its_own_formats_valid_fixtures() {
    let mut read = 0;
    for (verb, format) in VERBS {
        for case in cases(format, "valid") {
            let path = as_text(&case);
            let (code, out, err) = run(&[verb, &path]);
            assert_eq!(
                code, 0,
                "{verb} {path}: a valid fixture of its own format did not read \
                 (exit {code}); stderr {err:?}"
            );
            let value: serde_json::Value = serde_json::from_str(&out)
                .unwrap_or_else(|err| panic!("{verb} {path}: stdout is not JSON ({err}): {out:?}"));
            // The envelope names the format the verb actually reached. A verb
            // wired to the wrong format answers here in someone else's name.
            assert_eq!(
                value["format"],
                serde_json::json!(*format),
                "{verb} {path}: answered in another format's name, `{}`",
                value["format"]
            );
            assert_eq!(value["ok"], serde_json::json!(true), "{verb} {path}");
            assert!(
                value.get("value").is_some(),
                "{verb} {path}: exit 0 with no value in the envelope"
            );
            read += 1;
        }
    }
    assert!(read > 40, "only {read} documents were read");
}

#[test]
fn every_verb_refuses_its_own_formats_invalid_fixtures() {
    for (verb, format) in VERBS {
        for case in cases(format, "invalid") {
            let path = as_text(&case);
            let (code, out, err) = run(&[verb, &path]);
            assert_eq!(
                code, 1,
                "{verb} {path}: a document the format rejects must exit 1, not {code}; stderr {err:?}"
            );
            let value: serde_json::Value = serde_json::from_str(&out)
                .unwrap_or_else(|err| panic!("{verb} {path}: stdout is not JSON ({err}): {out:?}"));
            assert_eq!(value["ok"], serde_json::json!(false), "{verb} {path}");
            assert_eq!(value["format"], serde_json::json!(*format), "{verb} {path}");
            assert!(
                value.get("error").is_some(),
                "{verb} {path}: a refusal with no reason in the envelope"
            );
        }
    }
}

#[test]
fn a_usage_error_exits_two_and_prints_no_result() {
    let good = as_text(&cases("regimen", "valid")[0]);
    for args in [
        vec![],
        vec!["check-regimen"],
        vec!["no-such-verb", good.as_str()],
        vec!["check-regimen", good.as_str(), "and-another"],
        vec!["check-regimen", "no/such/file.toml"],
    ] {
        let (code, out, _) = run(&args);
        assert_eq!(
            code, 2,
            "{args:?}: a usage error must be distinguishable from a bad document"
        );
        assert!(
            out.is_empty(),
            "{args:?}: printed a result for a call it could not make: {out:?}"
        );
    }
}

#[test]
fn every_format_is_published_under_exactly_one_verb() {
    let mut verbs: Vec<&str> = VERBS.iter().map(|(verb, _)| *verb).collect();
    verbs.sort_unstable();
    let published = verbs.len();
    verbs.dedup();
    assert_eq!(published, verbs.len(), "a verb is listed twice");

    let mut named: Vec<&str> = VERBS.iter().map(|(_, format)| *format).collect();
    named.sort_unstable();
    let mut declared: Vec<&str> = diet::formats::FORMATS
        .iter()
        .map(|format| format.name)
        .collect();
    declared.sort_unstable();
    assert_eq!(
        named, declared,
        "every format is published under a verb, and every verb names a format"
    );
}
