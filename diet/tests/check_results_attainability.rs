//! The attainability rule, operative (ruled on #17; the construction audit
//! of 2026-10-13, A19): `scripts/check-results.py --attainability` holds a
//! decision rule's clauses to six mechanical checks. Each failing fixture
//! under `tests/fixtures/results-attainability/bad/` fails exactly its own
//! check, named on its `.reason`'s first line; the passing one passes; and
//! the committed results that predate the rule are reported, not failed,
//! each beside its audit row.

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
fn a_rule_that_declares_its_attainability_passes() {
    let ran = attainability(&fixtures("good").join("2026-10-14-attainable"));
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
