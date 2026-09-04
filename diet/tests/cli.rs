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

// The router is a lane, not a format, and `route` is the first verb that is
// not one format's projection. It must still behave like every other verb at
// the boundary: a structured result on stdout, exit 0 when the drive is a
// drive, exit 1 when it is not, and nothing a caller would have to parse out
// of prose.
#[test]
fn the_route_verb_answers_with_a_census_and_not_with_prose() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("capture/router/corpus");
    let mut drives: Vec<PathBuf> = std::fs::read_dir(&corpus)
        .unwrap_or_else(|err| panic!("{}: {err}", corpus.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    drives.sort();
    assert!(
        !drives.is_empty(),
        "{}: no drives, so this test would pass over nothing",
        corpus.display()
    );
    for drive in &drives {
        let path = drive.to_str().expect("a UTF-8 path");
        let (code, out, err) = run(&["route", path]);
        assert_eq!(code, 0, "route {path}: {err}");
        assert!(err.is_empty(), "route {path} wrote to stderr: {err}");
        let answer: serde_json::Value = serde_json::from_str(&out)
            .unwrap_or_else(|err| panic!("route {path}: stdout is not JSON ({err}): {out:?}"));
        assert_eq!(answer["ok"], serde_json::json!(true), "route {path}");
        // A lane is not a format, and a census answered under a format's
        // name would be read as that format's value.
        assert_eq!(
            answer["format"],
            serde_json::json!("route"),
            "route {path}: answered under another name, `{}`",
            answer["format"]
        );

        // The census the drive computes, not merely a census. The counts are
        // read from the corpus's own expectation file, which is authored by
        // hand beside the drive, so a binary that answered with an empty
        // census -- or with any other drive's -- says so here.
        let census = &answer["value"];
        let expected: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(drive.with_extension("expected.json"))
                .unwrap_or_else(|err| panic!("{}: {err}", drive.display())),
        )
        .expect("an expectation is JSON");
        for key in ["forks", "naive_forks", "judgment_asks"] {
            assert_eq!(
                census[key], expected[key],
                "route {path}: the census reports {key} {} where the drive spends {}",
                census[key], expected[key]
            );
        }
        assert_eq!(
            census["unclassified"].as_u64(),
            expected["unclassified"]
                .as_array()
                .map(|ids| ids.len() as u64),
            "route {path}: the census miscounts the calls the table could not place"
        );
        // Unwrapped, not compared as options: a missing count reads as
        // `None`, and `None < Some(_)` would let this pass over a census
        // that had no forks in it at all.
        let forks = census["forks"]
            .as_u64()
            .unwrap_or_else(|| panic!("route {path}: the census counts no forks: {census}"));
        let naive = census["naive_forks"]
            .as_u64()
            .unwrap_or_else(|| panic!("route {path}: the census counts no naive forks: {census}"));
        assert!(
            forks < naive,
            "route {path}: the router spent no fewer forks than the naive design: {census}"
        );
        assert!(
            census["reduction"].is_number(),
            "route {path}: the reduction is not a number: {census}"
        );
        assert!(
            census["per_class"]
                .as_object()
                .is_some_and(|fired| !fired.is_empty()),
            "route {path}: a census that names no class that fired: {census}"
        );
    }

    // A file that is not a record is the lane's `exit 1`, and it says so in
    // the same shape as a success rather than on stderr.
    let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cli.rs");
    let source = source.to_str().expect("a UTF-8 path");
    let (code, out, err) = run(&["route", source]);
    assert_eq!(code, 1, "a source file routed as a drive: {out}{err}");
    assert!(out.contains("\"ok\":false"), "{out}");
    assert!(out.contains("\"error\""), "{out}");
}
