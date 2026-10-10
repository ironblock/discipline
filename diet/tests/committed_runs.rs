//! Every committed run still reads (#583): its log, its record and its
//! regimen, as they were written. A run declares its version, and a reader
//! that moves on -- the boundary respelled `meditation` -- reads the old
//! spelling in the old versions, so nothing committed is rewritten.

use std::path::{Path, PathBuf};

use diet::formats::{log, record, regimen};

/// Every directory under `runs/`.
fn runs() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../runs");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
        .expect("runs/")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    assert!(!dirs.is_empty(), "no committed runs to read");
    dirs
}

#[test]
fn every_committed_runs_log_record_and_regimen_still_read() {
    let mut failures = Vec::new();
    let mut read = 0;
    for dir in runs() {
        let name = dir
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("?")
            .to_owned();
        let log_file = dir.join("session.log");
        if log_file.is_file() {
            read += 1;
            let text = std::fs::read_to_string(&log_file).expect("the log");
            if let Err(why) = log::parse(&text) {
                failures.push(format!("{name}/session.log: {why}"));
            }
        }
        let record_file = dir.join("record.jsonl");
        if record_file.is_file() {
            read += 1;
            let text = std::fs::read_to_string(&record_file).expect("the record");
            if let Err(why) = record::parse(&text) {
                failures.push(format!("{name}/record.jsonl: {why}"));
            }
        }
        let regimen_file = dir.join("regimen.toml");
        if regimen_file.is_file() {
            read += 1;
            let text = std::fs::read_to_string(&regimen_file).expect("the regimen");
            if let Err(why) = regimen::parse(&text) {
                failures.push(format!("{name}/regimen.toml: {why}"));
            }
        }
    }
    assert!(read > 0, "no committed log, record or regimen was found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
