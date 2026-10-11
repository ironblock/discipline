//! `scripts/assemble-results.py` over a committed run (#737): a record whose
//! summary has no prefill total -- optional since #654, absent when a turn's
//! first trunk request timed out, was cancelled or failed -- assembles, and
//! the directory it builds passes `scripts/check-results.py`.

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the repository")
        .to_path_buf()
}

/// A scratch directory of this test's own, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("diet-assemble-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

#[test]
fn a_run_whose_record_has_no_prefill_total_assembles_and_checks() {
    let run = root().join("runs/2026-10-10-seam-smoke-run-3-agent-driven");
    let record = std::fs::read_to_string(run.join("record.jsonl")).expect("the record");
    assert!(
        !record.contains("\"prefill_tokens_total\""),
        "the run this test is about is the one whose summary has no prefill total"
    );
    // Staged under the names `serve` writes, which the assembler reads.
    let stage = scratch("stage");
    for (from, to) in [
        ("session.log", "session.log"),
        ("record.jsonl", "record.jsonl"),
        ("product.txt", "record.jsonl.product.txt"),
        ("unspellable.json", "record.jsonl.unspellable.json"),
        ("receipt.json", "record.jsonl.receipt.json"),
        ("regimen.toml", "regimen.toml"),
    ] {
        std::fs::copy(run.join(from), stage.join(to)).expect("staged");
    }
    let results = scratch("results");
    let out = results.join("2026-10-10-seam-smoke-run-3");
    let assembled = Command::new("python3")
        .current_dir(root())
        .arg("scripts/assemble-results.py")
        .args(["--log", &stage.join("session.log").to_string_lossy()])
        .args(["--record", &stage.join("record.jsonl").to_string_lossy()])
        .args(["--regimen", &stage.join("regimen.toml").to_string_lossy()])
        .args(["--out", &out.to_string_lossy()])
        .output()
        .expect("python3 runs the assembler");
    assert!(
        assembled.status.success(),
        "the assembler refused: {}{}",
        String::from_utf8_lossy(&assembled.stdout),
        String::from_utf8_lossy(&assembled.stderr)
    );
    let readme = std::fs::read_to_string(out.join("README.md")).expect("a README");
    assert!(
        !readme.contains("prefill_tokens_total"),
        "a total the record does not carry is not written: {readme}"
    );
    let checked = Command::new("python3")
        .current_dir(root())
        .arg("scripts/check-results.py")
        .args(["--root", &results.to_string_lossy()])
        .output()
        .expect("python3 runs the checker");
    assert!(
        checked.status.success(),
        "check-results refused the assembled directory: {}{}",
        String::from_utf8_lossy(&checked.stdout),
        String::from_utf8_lossy(&checked.stderr)
    );
    let _ = std::fs::remove_dir_all(&stage);
    let _ = std::fs::remove_dir_all(&results);
}
