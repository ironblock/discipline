//! `diet-drive scenario` (#760): a scenario driven end to end on the canned
//! substrate -- `serve` started as a child, driven over its HTTP API, its
//! log checked against the scenario's expectations.

use std::path::{Path, PathBuf};
use std::process::Command;

const DRIVE: &str = env!("CARGO_BIN_EXE_diet-drive");

/// The repository root: where `diet-drive scenario` resolves `scripts/`.
fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the crate sits in the repository")
        .to_path_buf()
}

/// A fresh directory for one test.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "diet-scenario-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("after the epoch")
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

/// A scenario file in `dir` whose header names the canned regimen and the
/// floor's head by absolute path, then `body`'s lines.
fn scenario(dir: &Path, name: &str, body: &str) -> PathBuf {
    let root = root();
    let header = serde_json::json!({
        "scenario": name,
        "regimen": root.join("diet/scenarios/canned.toml"),
        "head": root.join("diet/drive/heads/floor.md"),
    });
    let path = dir.join(format!("{name}.jsonl"));
    std::fs::write(&path, format!("{header}\n{body}")).expect("the scenario");
    path
}

/// Run `diet-drive scenario <file> --out <out> --no-assemble`: its exit
/// code and its last stdout line, as JSON.
fn run(file: &Path, out: &Path) -> (Option<i32>, serde_json::Value) {
    let ran = Command::new(DRIVE)
        .current_dir(root())
        .args(["scenario"])
        .arg(file)
        .arg("--out")
        .arg(out)
        .arg("--no-assemble")
        .output()
        .expect("diet-drive runs");
    let stdout = String::from_utf8_lossy(&ran.stdout);
    let last = stdout.lines().last().unwrap_or("");
    let said = serde_json::from_str(last).unwrap_or_else(|_| {
        panic!(
            "not JSON: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&ran.stderr)
        )
    });
    (ran.status.code(), said)
}

/// A scenario that asks once and expects the turn to settle `final`
/// passes: exit 0, `ok`, the replies' digest named, and the log and the
/// record written under `--out`.
#[test]
fn a_scenario_on_the_canned_substrate_passes_and_names_its_replies() {
    let dir = scratch("passes");
    let file = scenario(
        &dir,
        "smoke",
        concat!(
            r#"{"reply":{"when":"hello there","text":"Hello."}}"#,
            "\n",
            r#"{"step":"ask","text":"hello there"}"#,
            "\n",
            r#"{"step":"end"}"#,
            "\n",
            r#"{"expect":"turn.settled","where":{"reason":"final"}}"#,
            "\n",
        ),
    );
    let out = dir.join("out");
    let (code, said) = run(&file, &out);
    assert_eq!(code, Some(0), "{said}");
    assert_eq!(said["ok"], true, "{said}");
    assert_eq!(said["scenario"], "smoke");
    assert_eq!(said["substrate"], "canned-scenario");
    assert_eq!(said["replies_sha256"].as_str().map(str::len), Some(64));
    assert!(out.join("session.log").is_file(), "{said}");
    assert!(out.join("session.record.jsonl").is_file(), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An expectation the log does not meet fails the run, naming it.
#[test]
fn an_unmet_expectation_fails_the_run_naming_it() {
    let dir = scratch("unmet");
    let file = scenario(
        &dir,
        "unmet",
        concat!(
            r#"{"reply":{"text":"Hello."}}"#,
            "\n",
            r#"{"step":"ask","text":"hi"}"#,
            "\n",
            r#"{"step":"end"}"#,
            "\n",
            r#"{"expect":"meditation.held"}"#,
            "\n",
        ),
    );
    let (code, said) = run(&file, &dir.join("out"));
    assert_eq!(code, Some(1), "{said}");
    assert_eq!(said["ok"], false);
    assert!(
        said["unmet"].to_string().contains("meditation.held"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A request no reply answers fails the run, naming the request's last
/// message -- never answered with something generic.
#[test]
fn a_request_no_reply_answers_fails_the_run_naming_it() {
    let dir = scratch("unmatched");
    let file = scenario(
        &dir,
        "unmatched",
        concat!(
            r#"{"reply":{"when":"something else","text":"Hello."}}"#,
            "\n",
            r#"{"step":"ask","text":"an ask nobody wrote a reply for"}"#,
            "\n",
            r#"{"step":"end"}"#,
            "\n",
        ),
    );
    let (code, said) = run(&file, &dir.join("out"));
    assert_eq!(code, Some(1), "{said}");
    assert!(
        said["unmatched"]
            .to_string()
            .contains("an ask nobody wrote a reply for"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
