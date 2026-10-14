//! The attainability rule, operative (ruled on #17; the construction audit
//! of 2026-10-13, A19): `scripts/check-results.py --attainability` holds a
//! decision rule's clauses to its mechanical checks. Each failing fixture
//! under `tests/fixtures/results-attainability/bad/` fails exactly its own
//! check, named on its `.reason`'s first line; each passing one passes; and
//! the committed results that predate the rule are reported, not failed,
//! each beside its audit row. `--docs` holds a doc's ledger words to the
//! records it links: each failing doc under `docs/bad/` fails, each under
//! `docs/good/` passes, and the program's own docs pass.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the repository")
        .to_path_buf()
}

fn fixtures(kind: &str) -> PathBuf {
    root()
        .join("tests/fixtures/results-attainability")
        .join(kind)
}

fn attainability(dir: &Path) -> Output {
    Command::new("python3")
        .current_dir(root())
        .args(["scripts/check-results.py", "--attainability"])
        .arg(dir)
        .output()
        .expect("python3 runs the checker")
}

fn said(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn each_failing_fixture_fails_its_own_check_and_no_other() {
    let bad = fixtures("bad");
    let mut cases: Vec<PathBuf> = std::fs::read_dir(&bad)
        .expect("the failing fixtures")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();
    assert!(cases.len() >= 8, "a fixture per check: {cases:?}");
    for case in cases {
        let reason = std::fs::read_to_string(case.with_extension("reason")).expect("a .reason");
        let class = reason.lines().next().expect("the class line").trim();
        let ran = attainability(&case);
        let out = said(&ran);
        assert_eq!(ran.status.code(), Some(1), "{}: {out}", case.display());
        assert!(
            out.contains(&format!("[{class}]")),
            "{}: wants [{class}]: {out}",
            case.display()
        );
        let others = out
            .lines()
            .filter(|line| {
                line.contains("[results.attainability-") && !line.contains(&format!("[{class}]"))
            })
            .collect::<Vec<_>>();
        assert!(
            others.is_empty(),
            "{}: fails only its own check: {others:?}",
            case.display()
        );
    }
}

#[test]
fn each_rule_that_declares_its_attainability_passes() {
    let mut cases: Vec<PathBuf> = std::fs::read_dir(fixtures("good"))
        .expect("the passing fixtures")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    cases.sort();
    assert!(
        cases.len() >= 3,
        "a passing fixture per declaration: {cases:?}"
    );
    for case in cases {
        let ran = attainability(&case);
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{}: {}",
            case.display(),
            said(&ran)
        );
    }
}

fn ledger_words(doc: &Path, records: &Path) -> Output {
    Command::new("python3")
        .current_dir(root())
        .args(["scripts/check-results.py", "--docs"])
        .arg(doc)
        .arg("--root")
        .arg(records)
        .output()
        .expect("python3 runs the checker")
}

fn docs(kind: &str, extension: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(fixtures("docs").join(kind))
        .expect("the doc fixtures")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|e| e == extension))
        .collect();
    found.sort();
    found
}

#[test]
fn a_doc_that_strengthens_a_ledger_word_fails_and_one_that_keeps_it_passes() {
    let records = fixtures("docs").join("results");
    let bad = docs("bad", "md");
    assert!(bad.len() >= 2, "{bad:?}");
    for doc in bad {
        let reason = std::fs::read_to_string(doc.with_extension("reason")).expect("a .reason");
        let class = reason.lines().next().expect("the class line").trim();
        let ran = ledger_words(&doc, &records);
        let out = said(&ran);
        assert_eq!(ran.status.code(), Some(1), "{}: {out}", doc.display());
        assert!(
            out.contains(&format!("[{class}]")),
            "{}: wants [{class}]: {out}",
            doc.display()
        );
    }
    for doc in docs("good", "md") {
        let ran = ledger_words(&doc, &records);
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{}: {}",
            doc.display(),
            said(&ran)
        );
    }
}

#[test]
fn the_programs_docs_say_no_ledger_word_their_records_do_not() {
    let ran = ledger_words(&root().join("docs"), &root().join("results"));
    assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));
}

#[test]
fn the_committed_results_that_predate_the_rule_are_reported_beside_their_audit_rows() {
    let ran = Command::new("python3")
        .current_dir(root())
        .args([
            "scripts/check-results.py",
            "--attainability",
            "--root",
            "results",
        ])
        .output()
        .expect("python3 runs the checker");
    let out = said(&ran);
    assert_eq!(ran.status.code(), Some(0), "history stays green: {out}");
    for (dir, row) in [
        ("2026-09-20-false-nomination-framing", "A3"),
        ("2026-09-17-sense-bakeoff-adjudication", "A2"),
        ("2026-09-19-entry-to-turn-nomination-adjudication", "A1"),
    ] {
        assert!(
            out.lines().any(|line| line.contains(dir)
                && line.contains("reported, not failed")
                && line.contains(row)),
            "{dir} reported beside {row}: {out}"
        );
    }
}
