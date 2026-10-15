//! The three claim rules, operative (accepted 2026-10-14; Planning's draft,
//! adapted in `docs/experiments.md`): `scripts/check-results.py --claims`
//! holds a results root to them. A run outside its substrate's recommended
//! sampling is a characterisation and carries no verdict; every control is
//! seen red before the window opens; and a second fire of a claim under the
//! same rule names the first and never replaces its word. Each failing
//! fixture root under `tests/fixtures/results-claims/bad/` fails exactly its
//! own check, named on its `.reason`'s first line; each passing one passes;
//! and the committed results are reported against `GRANDFATHERED_CLAIMS`,
//! not failed. The ambient-context rule (2026-10-15) rides with them: an
//! instrument run through an agent harness declares what it carried, as
//! probed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CLASSES: [&str; 8] = [
    "results.characterisation-undeclared",
    "results.characterisation-mismatch",
    "results.characterisation-carries-verdict",
    "results.control-not-seen-red",
    "results.controls-undeclared",
    "results.repeat-fire",
    "results.ambient-undeclared",
    "results.ambient-unprobed",
];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the repository")
        .to_path_buf()
}

fn fixtures() -> PathBuf {
    root().join("tests/fixtures/results-claims")
}

fn claims(results: &Path, registry: &Path) -> Output {
    Command::new("python3")
        .current_dir(root())
        .args(["scripts/check-results.py", "--claims", "--root"])
        .arg(results)
        .arg("--registry")
        .arg(registry)
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

fn cases(kind: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(fixtures().join(kind))
        .expect("the fixtures")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    found.sort();
    found
}

#[test]
fn each_failing_root_fails_its_own_check_and_no_other() {
    let bad = cases("bad");
    assert!(bad.len() >= 6, "a fixture per check: {bad:?}");
    for case in bad {
        let reason = std::fs::read_to_string(case.with_extension("reason")).expect("a .reason");
        let class = reason.lines().next().expect("the class line").trim();
        assert!(CLASSES.contains(&class), "{}: {class}", case.display());
        let ran = claims(&case, &fixtures().join("registry.toml"));
        let out = said(&ran);
        assert_eq!(ran.status.code(), Some(1), "{}: {out}", case.display());
        assert!(
            out.contains(&format!("[{class}]")),
            "{}: wants [{class}]: {out}",
            case.display()
        );
        let others: Vec<&str> = out
            .lines()
            .filter(|line| {
                CLASSES
                    .iter()
                    .any(|other| *other != class && line.contains(&format!("[{other}]")))
            })
            .collect();
        assert!(
            others.is_empty(),
            "{}: fails only its own check: {others:?}",
            case.display()
        );
    }
}

#[test]
fn each_passing_root_passes() {
    let good = cases("good");
    assert!(good.len() >= 5, "{good:?}");
    for case in good {
        let ran = claims(&case, &fixtures().join("registry.toml"));
        assert_eq!(
            ran.status.code(),
            Some(0),
            "{}: {}",
            case.display(),
            said(&ran)
        );
    }
}

#[test]
fn the_committed_results_pass_and_the_one_repeat_is_reported() {
    let ran = claims(
        &root().join("results"),
        &root().join("substrates/registry.toml"),
    );
    let out = said(&ran);
    assert_eq!(ran.status.code(), Some(0), "history stays green: {out}");
    assert!(
        out.lines().any(
            |line| line.contains("2026-09-20-entry-to-turn-nomination-v2:")
                && line.contains("reported, not failed")
                && line.contains("[results.repeat-fire]")
        ),
        "#17's second draw reported as a repeat: {out}"
    );
}

/// What the ledger derives for one fixture root: `--claims --flags` prints
/// each directory's derived fields as one JSON line.
fn flags(case: &str) -> serde_json::Value {
    let ran = Command::new("python3")
        .current_dir(root())
        .args(["scripts/check-results.py", "--claims", "--flags", "--root"])
        .arg(fixtures().join("good").join(case))
        .arg("--registry")
        .arg(fixtures().join("registry.toml"))
        .output()
        .expect("python3 runs the checker");
    let out = said(&ran);
    assert_eq!(ran.status.code(), Some(0), "{out}");
    let line = out
        .lines()
        .find(|line| line.starts_with('{'))
        .unwrap_or_else(|| panic!("no flags line: {out}"));
    serde_json::from_str(line).expect("the flags are JSON")
}

/// A carded field the regimen leaves to the server's default is not checked
/// against the card -- the server's default may be off it -- so the ledger
/// names it `unstated`, and the gap is visible rather than read as in range.
#[test]
fn a_carded_field_the_regimen_leaves_to_the_server_is_named_unstated() {
    let left = flags("unstated-left-to-server");
    assert_eq!(left["characterisation"], false, "{left}");
    let names: Vec<&str> = left["unstated"]
        .as_array()
        .unwrap_or_else(|| panic!("a list: {left}"))
        .iter()
        .filter_map(|row| row["parameter"].as_str())
        .collect();
    assert_eq!(
        names,
        ["presence_penalty", "repetition_penalty", "top_k", "top_p"],
        "{left}"
    );
    let stated = flags("repeat-penalty-on-card");
    let names: Vec<&str> = stated["unstated"]
        .as_array()
        .unwrap_or_else(|| panic!("a list: {stated}"))
        .iter()
        .filter_map(|row| row["parameter"].as_str())
        .collect();
    assert_eq!(
        names,
        ["presence_penalty"],
        "diet's repeat_penalty is the card's repetition_penalty: {stated}"
    );
}
