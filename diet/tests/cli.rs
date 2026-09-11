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
    ("check-operating-points", "operating_points"),
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

/// The register the bakeoff fixture is built over.
const BAKEOFF_REGISTER: &str = include_str!("../capture/sense/register/authored-mistake.jsonl");

/// The `start` row of the bakeoff fixture's run record.
const BAKEOFF_START: &str = concat!(
    r#"{"record":"start","regime":{"arm":"bakeoff","dogma_version":0,"substrates":[{"id":"#,
    r#""processor","engine":{"name":"none","version_or_digest":"0"},"weights":{"kind":"#,
    r#""digest","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"#,
    r#""hardware_fingerprint":"one-cpu","sampler_card":{"seed":0},"reasoning":"off"}]}}"#
);

/// Its `summary` row.
const BAKEOFF_SUMMARY: &str = concat!(
    r#"{"record":"summary","kind":"recompute","targets_checked":1,"targets_matched":1,"#,
    r#""digests":["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"]}"#
);

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

/// The bakeoff verb runs the bakeoff, proven by running the program.
///
/// `bakeoff` was the one verb with no subprocess test: every other format is
/// exercised here by data, and the runner's own lane tested `capture::bakeoff`
/// as a FUNCTION. A review pointed the `bakeoff` row of the command table at
/// `Operation::Route`, rebuilt, and ran the whole suite: 582 tests, all green,
/// while `diet bakeoff` answered with the router's refusal. That is exactly the
/// dispatch defect this file's docstring says data catches and a table-checked-
/// against-itself does not.
///
/// The assertion is not "the output says bakeoff". A mis-wired verb still
/// labels its answer, and an error path would satisfy that while proving
/// nothing ran. It is that the PROGRAM's answer equals the LIBRARY function's
/// answer for the same directory -- so the verb is pinned to the lane behind
/// it, and the run has to have actually happened to produce anything to
/// compare.
#[test]
fn the_bakeoff_verb_runs_the_bakeoff_and_not_another_lane() {
    let dir = std::env::temp_dir().join(format!(
        "diet-bakeoff-cli-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or_default()
    ));
    let path = write_bakeoff_run(&dir);

    // The library's answer for this directory, and then the program's.
    let expected = diet::capture::bakeoff::run(&path)
        .unwrap_or_else(|err| panic!("the library did not report: {err}"));
    let mut rendered = String::new();
    diet::formats::record::json::render(&expected, &mut rendered);

    let (code, out, err) = run(&["bakeoff", path.to_str().expect("a UTF-8 path")]);
    assert_eq!(code, 0, "bakeoff {}: {err}", path.display());
    assert!(err.is_empty(), "bakeoff wrote to stderr: {err}");

    let answer: serde_json::Value = serde_json::from_str(&out)
        .unwrap_or_else(|why| panic!("stdout is not JSON ({why}): {out:?}"));
    assert_eq!(answer["ok"], serde_json::json!(true), "{out}");
    assert_eq!(answer["format"], serde_json::json!("bakeoff"), "{out}");

    // The whole report, not a field of it. `cells`, `comparisons` and `budget`
    // are the bakeoff's own shape; no other lane produces them, and a verb
    // pointed anywhere else cannot match this however it labels itself.
    let library: serde_json::Value = serde_json::from_str(&rendered)
        .unwrap_or_else(|why| panic!("the library's report is not JSON ({why})"));
    assert_eq!(
        answer["value"], library,
        "the program's report and the library's differ for the same input"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// A bakeoff run directory: the shipped register, two caches, and a record
/// that consumes all three by digest.
///
/// Built through the crate's PUBLIC api -- `sense::Fixture` is the embedder
/// the instrument's own tests use -- so there is no second embedding here to
/// drift from the first. It is assembled rather than committed because the
/// vectors are derived: a committed cache would freeze an answer this crate
/// computes, and the first change to `Fixture::embed` would make it a fixture
/// that tests a number nobody can re-derive.
fn write_bakeoff_run(dir: &Path) -> PathBuf {
    use diet::capture::sense::{self, Embedder, Fixture};
    use std::fmt::Write as _;

    std::fs::create_dir_all(dir).expect("a directory");
    let senses = sense::shipped_senses().expect("the shipped senses");
    let rows = sense::register(BAKEOFF_REGISTER).expect("the shipped register");
    let register_texts: Vec<String> = rows.iter().map(|row| row.text.clone()).collect();
    let mut texts = register_texts.clone();
    texts.extend(senses.iter().map(|sense| sense.text.clone()));
    for scoring in sense::Scoring::ALL {
        let (top, bottom) = scoring.extremes();
        for set in sense::SenseSet::ALL {
            let embedded =
                sense::EmbeddedSet::embed(&senses, *set, &Fixture).expect("an embeddable set");
            texts.push(top.row(&embedded).text);
            texts.push(bottom.row(&embedded).text);
        }
    }
    texts.sort();
    texts.dedup();

    let cache = |lean: bool| {
        let mut out = String::new();
        for text in &texts {
            let mut vector = Fixture.embed(text);
            if lean && register_texts.contains(text) {
                vector[Fixture::DIMENSIONS - 1] += 0.5;
            }
            let spelled: Vec<String> = vector.iter().map(|value| format!("{value:.8}")).collect();
            let quoted = serde_json::to_string(text).expect("a JSON string");
            let _ = writeln!(
                out,
                "{{\"text\":{quoted},\"vector\":[{}]}}",
                spelled.join(",")
            );
        }
        out
    };

    let files = [
        ("authored-mistake.jsonl", BAKEOFF_REGISTER.to_owned()),
        ("even.vectors.jsonl", cache(false)),
        ("lean.vectors.jsonl", cache(true)),
    ];
    let mut consumes = Vec::new();
    for (name, body) in &files {
        std::fs::write(dir.join(name), body).expect("a written input");
        consumes.push(format!(
            "{{\"path\":\"{name}\",\"sha256\":\"{}\"}}",
            diet::digest::sha256(body.as_bytes())
        ));
    }

    let record = format!(
        "{BAKEOFF_START}\n{{\"record\":\"claim\",\"id\":\"c1\",\"hypothesis\":\"the cells are \
         comparable\",\"result\":\"supported\",\"consumes\":[{}]}}\n{BAKEOFF_SUMMARY}\n",
        consumes.join(",")
    );
    let path = dir.join("run.jsonl");
    std::fs::write(&path, record).expect("a written record");
    path
}
