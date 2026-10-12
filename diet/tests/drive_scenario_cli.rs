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

/// #798: on `canned-scenario-windowed`, whose registry entry declares a
/// context window, a reply reporting a large prompt fires the budget
/// meditation at its turn's end, and a large tool result fires the window
/// meditation inside a later turn, before the trunk's next request -- once
/// the trunk holds a turn it can drop.
#[test]
fn a_windowed_canned_substrate_fires_the_budget_and_the_window_meditations() {
    let dir = scratch("windowed");
    std::fs::write(
        dir.join("windowed.toml"),
        "arm = \"rehearsal\"\ndogma_version = 0\n\
         substrate = \"canned-scenario-windowed\"\nsubstrate_reasoning = \"off\"\n\
         substrate_hardware = \"b6bc80e9e8e4bc2342219f064dab89a8d17c02535600b32b84895fc0b9ba6513\"\n\
         isolation = \"none\"\napproval = \"none\"\n\
         approval_policy = \"approvals are off: every command runs without a gate decision or a prompt\"\n\
         interview_warrant = [\"scoping\"]\ncapture_modality = \"fields\"\n\
         max_output_tokens = 512\nmeditation_at_context_fraction = 0.2\n\
         [limits]\nmax_steps = 4\n[sampler]\ntemperature = 0.6\n",
    )
    .expect("the regimen");
    let header = serde_json::json!({
        "scenario": "windowed",
        "regimen": "windowed.toml",
        "head": root().join("diet/drive/heads/floor.md"),
    });
    let body = [
        r#"{"reply":{"when":"Scope it","text":"Scoped: one tracker.","prompt_tokens":5000}}"#,
        r#"{"reply":{"when":"The turn has ended","text":"DECISION: one tracker\nPLAN: NONE\nSUPERSEDE: NONE"}}"#,
        r#"{"reply":{"when":"Name it","text":"It is the tracker."}}"#,
        r#"{"reply":{"when":"Print it","text":"Printing.","calls":[{"id":"call-b","name":"bash","arguments":"{\"command\": \"python3 -c \\\"print('x' * 20000)\\\"\"}"}]}}"#,
        r#"{"reply":{"when":"xxxxxxxxxx","text":"Printed."}}"#,
        r#"{"reply":{"text":"."}}"#,
        r#"{"step":"ask","text":"Scope it: one tracker.","scoping":true}"#,
        r#"{"step":"ask","text":"Name it."}"#,
        r#"{"step":"ask","text":"Print it: a long line."}"#,
        r#"{"step":"end"}"#,
        r#"{"expect":"meditation","name":"budget","where":{"reason":"budget","at_turn":"1"}}"#,
        r#"{"expect":"meditation","where":{"reason":"window","at_turn":"3","window":"24000"},"after":"budget"}"#,
    ]
    .join("\n");
    let file = dir.join("windowed.jsonl");
    std::fs::write(&file, format!("{header}\n{body}\n")).expect("the scenario");
    let (code, said) = run(&file, &dir.join("out"));
    assert_eq!(code, Some(0), "{said}");
    let _ = std::fs::remove_dir_all(&dir);
}
