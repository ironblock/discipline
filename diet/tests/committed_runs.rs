//! Every committed run still reads (#583): its log, its record and its
//! regimen, as they were written. A run declares its version, and a reader
//! that moves on -- the boundary respelled `meditation` -- reads the old
//! spelling in the old versions, so nothing committed is rewritten.

use std::path::{Path, PathBuf};

use diet::formats::{log, record, regimen};

/// Committed logs the reader refuses today, each with the refusal it
/// gives, word for word: a run recorded before a rule it breaks was
/// written. Pinned so a different refusal, or this one going away, fails
/// here rather than passing quietly.
///
/// The gate-and-tail run was recorded before #705: its audit fork after an
/// operator meditation that followed a meditation names no `after_seam`,
/// and #705's rule refuses the fork. The reader on develop refuses it the
/// same way; it is not the rename's.
const KNOWN_REFUSALS: &[(&str, &str)] = &[(
    "2026-10-10-gate-and-tail-agent-driven/session.log",
    "line 2094: a fork of turn 3 after a meditation that followed it: the meditation replaced \
     the trunk the fork would be cut from",
)];

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
            let file = format!("{name}/session.log");
            let known = KNOWN_REFUSALS.iter().find(|(path, _)| *path == file);
            match (log::parse(&text), known) {
                (Ok(_), None) => {}
                (Err(why), Some((_, expected))) if why.to_string() == *expected => {}
                (Err(why), _) => failures.push(format!("{file}: {why}")),
                (Ok(_), Some(_)) => {
                    failures.push(format!("{file}: reads now; take it out of KNOWN_REFUSALS"));
                }
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
            match regimen::parse(&text) {
                Err(why) => failures.push(format!("{name}/regimen.toml: {why}")),
                // What it bound under an old name, it still says under the
                // new one.
                Ok(read) => {
                    for (old, new) in regimen::RENAMED_KEYS {
                        let bound = read.iter().find(|(key, _)| key == old).map(|(_, v)| v);
                        if bound.is_some() && read.get(new) != bound {
                            failures
                                .push(format!("{name}/regimen.toml: `{old}` not read as `{new}`"));
                        }
                    }
                }
            }
        }
    }
    assert!(read > 0, "no committed log, record or regimen was found");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
