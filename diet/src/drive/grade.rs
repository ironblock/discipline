//! A scenario's `grade` step (#942): the check the runner makes of a
//! settled turn, outside the model's sandbox, and the feedback it sends.
//!
//! The check is the operator's command, run by the runner in the session's
//! worktree, never by serve. It must not change what the model sees: the
//! worktree's entries are compared before and after it by path, mode, size
//! and modification time, and a check that changed any fails the run. A
//! metadata snapshot is cheap on a worktree of many files (a dependency
//! tree, say); it cannot hash an entry's bytes from before the check, so a
//! change of metadata alone -- a `touch` -- counts as a change, which the
//! model could see.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::formats::log::{self, Event, Lane};

/// One worktree entry's metadata: its mode, its size and its modification
/// time in nanoseconds since the epoch.
type Entry = (u32, u64, i128);

/// A worktree's entries, by path relative to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot(BTreeMap<PathBuf, Entry>);

impl Snapshot {
    /// Every entry under `root`, symbolic links read as themselves.
    ///
    /// # Errors
    ///
    /// When a directory under it cannot be read.
    pub fn of(root: &Path) -> Result<Self, String> {
        let mut entries = BTreeMap::new();
        let mut pending = vec![root.to_path_buf()];
        while let Some(dir) = pending.pop() {
            let read = std::fs::read_dir(&dir)
                .map_err(|why| format!("{} cannot be read: {why}", dir.display()))?;
            for entry in read {
                let entry =
                    entry.map_err(|why| format!("{} cannot be read: {why}", dir.display()))?;
                let path = entry.path();
                let meta = std::fs::symlink_metadata(&path)
                    .map_err(|why| format!("{} cannot be read: {why}", path.display()))?;
                let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
                entries.insert(relative, metadata(&meta));
                if meta.is_dir() {
                    pending.push(path);
                }
            }
        }
        Ok(Self(entries))
    }

    /// The paths whose entry `after` added, removed or changed.
    #[must_use]
    pub fn changed(&self, after: &Self) -> Vec<String> {
        let mut changed: Vec<String> = self
            .0
            .iter()
            .filter(|(path, entry)| after.0.get(*path) != Some(*entry))
            .map(|(path, _)| path.display().to_string())
            .collect();
        changed.extend(
            after
                .0
                .keys()
                .filter(|path| !self.0.contains_key(*path))
                .map(|path| path.display().to_string()),
        );
        changed.sort();
        changed
    }
}

#[cfg(unix)]
fn metadata(meta: &std::fs::Metadata) -> Entry {
    use std::os::unix::fs::MetadataExt as _;
    (
        meta.mode(),
        meta.size(),
        i128::from(meta.mtime()) * 1_000_000_000 + i128::from(meta.mtime_nsec()),
    )
}

#[cfg(not(unix))]
fn metadata(meta: &std::fs::Metadata) -> Entry {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| {
            i128::try_from(since.as_nanos()).unwrap_or(i128::MAX)
        });
    (u32::from(meta.permissions().readonly()), meta.len(), mtime)
}

/// What a check or a `before` command did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// What it printed on stdout.
    pub stdout: String,
    /// What it printed on stderr.
    pub stderr: String,
    /// Its exit status, when it exited.
    pub status: Option<i32>,
    /// Whether it ran out of time and was killed, its process group with it.
    pub timed_out: bool,
}

/// Run `command` with `bash -c` in `worktree`, with `env` added to the
/// runner's environment, for at most `timeout`; and fail it if it changed
/// the worktree.
///
/// # Errors
///
/// When it cannot start, when the worktree cannot be read, or when the
/// command changed it, naming the paths.
pub fn guarded(
    command: &str,
    worktree: &Path,
    env: &[(&str, String)],
    timeout: std::time::Duration,
) -> Result<Ran, String> {
    let before = Snapshot::of(worktree)?;
    let ran = run(command, worktree, env, timeout)?;
    let after = Snapshot::of(worktree)?;
    let changed = before.changed(&after);
    if !changed.is_empty() {
        return Err(format!(
            "the grade's command changed the worktree the model sees: {}",
            changed.join(", ")
        ));
    }
    Ok(ran)
}

/// Run `command` with `bash -c` in `worktree`, leading a process group of
/// its own, for at most `timeout`; on timeout its group is killed.
fn run(
    command: &str,
    worktree: &Path,
    env: &[(&str, String)],
    timeout: std::time::Duration,
) -> Result<Ran, String> {
    use std::io::Read as _;
    let mut child = spawn(
        std::process::Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(worktree)
            .envs(env.iter().map(|(key, value)| (*key, value.as_str())))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped()),
    )
    .map_err(|why| format!("the grade's command cannot start: {why}"))?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stdout.read_to_string(&mut text);
        text
    });
    let err = std::thread::spawn(move || {
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });
    let give_up = std::time::Instant::now() + timeout;
    let (status, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(status)) => break (status.code(), false),
            Ok(None) if std::time::Instant::now() < give_up => {
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            _ => {
                // Its own group, by its own pid: never a pattern.
                let _ = std::process::Command::new("/bin/kill")
                    .args(["-KILL", "--", &format!("-{}", child.id())])
                    .output();
                let _ = child.kill();
                let _ = child.wait();
                break (None, true);
            }
        }
    };
    Ok(Ran {
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
        status,
        timed_out,
    })
}

#[cfg(unix)]
fn spawn(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0).spawn()
}

#[cfg(not(unix))]
fn spawn(command: &mut std::process::Command) -> std::io::Result<std::process::Child> {
    command.spawn()
}

/// A check's verdict, as it printed it: `{"right", "expected", "got"}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Whether the attempt is right.
    pub right: bool,
    /// What was expected, in the check's words.
    pub expected: String,
    /// What the attempt gave.
    pub got: String,
}

impl Verdict {
    /// The verdict a check printed: its stdout's last line, one JSON object
    /// of exactly `right` (a boolean), `expected` and `got` (strings).
    ///
    /// # Errors
    ///
    /// When it printed no such object.
    pub fn read(stdout: &str) -> Result<Self, String> {
        let last = stdout.lines().rev().find(|line| !line.trim().is_empty());
        let object: serde_json::Map<String, serde_json::Value> = last
            .and_then(|line| serde_json::from_str(line).ok())
            .ok_or_else(|| format!("the check printed no JSON object: {stdout:?}"))?;
        if object.len() != 3 {
            return Err(format!(
                "the check's verdict holds right, expected and got, and only those: {object:?}"
            ));
        }
        match (
            object.get("right").and_then(serde_json::Value::as_bool),
            object.get("expected").and_then(serde_json::Value::as_str),
            object.get("got").and_then(serde_json::Value::as_str),
        ) {
            (Some(right), Some(expected), Some(got)) => Ok(Self {
                right,
                expected: expected.to_owned(),
                got: got.to_owned(),
            }),
            _ => Err(format!(
                "the check's verdict holds a boolean `right` and string `expected` and `got`: {object:?}"
            )),
        }
    }
}

/// The most of `got` a feedback ask carries, in characters.
pub const GOT_CAP: usize = 1_500;

/// A feedback template filled (#942): `{expected}`, `{got}` (clipped at
/// [`GOT_CAP`] characters, `(nothing)` when blank), `{task}`, `{attempt}`
/// and `{attempts}`.
#[must_use]
pub fn feedback(
    template: &str,
    verdict: &Verdict,
    task: &str,
    (attempt, attempts): (u64, u64),
) -> String {
    let got = verdict.got.trim();
    let got = if got.is_empty() {
        "(nothing)".to_owned()
    } else if got.chars().count() > GOT_CAP {
        const CUT: &str = "...[truncated]";
        let kept: String = got.chars().take(GOT_CAP - CUT.len()).collect();
        format!("{kept}{CUT}")
    } else {
        got.to_owned()
    };
    template
        .replace("{expected}", verdict.expected.trim_end())
        .replace("{got}", &got)
        .replace("{task}", task)
        .replace("{attempt}", &attempt.to_string())
        .replace("{attempts}", &attempts.to_string())
}

/// The turn a check reads (#942), from the log's lines: the latest turn's
/// `tool_call`s, each with its whole streams as the log holds them, and
/// the turn's final answer, its last trunk response's text.
///
/// A stream past what a call holds in memory (#734) is held by the log as
/// its head and tail; its `bytes` then exceed its text's, and a check that
/// compares it whole finds it unequal.
#[must_use]
pub fn turn_of(lines: &[log::Line], (task, attempt): (&str, u64)) -> serde_json::Value {
    let asked = lines
        .iter()
        .rposition(|line| matches!(line.event, Event::Ask { .. }))
        .unwrap_or(0);
    let turn = lines[asked..].iter().find_map(|line| match &line.event {
        Event::Ask { turn, .. } => Some(*turn),
        _ => None,
    });
    let mut trunk = std::collections::BTreeSet::new();
    let mut calls = Vec::new();
    let mut answer = String::new();
    for line in &lines[asked..] {
        match &line.event {
            Event::Request {
                lane: Lane::Trunk, ..
            } => {
                trunk.insert(line.seq);
            }
            Event::Response {
                to_request, text, ..
            } if trunk.contains(to_request) && !text.is_empty() => {
                answer.clone_from(text);
            }
            Event::ToolCall {
                name,
                outcome,
                exit,
                stdout,
                stderr,
                ..
            } => {
                let stream = |output: &Option<log::Output>| {
                    output.as_ref().map_or(
                        serde_json::json!({"text": "", "bytes": 0}),
                        |output| serde_json::json!({"text": output.text, "bytes": output.bytes}),
                    )
                };
                calls.push(serde_json::json!({
                    "seq": line.seq,
                    "name": name,
                    "outcome": outcome.tag(),
                    "exit": exit,
                    "stdout": stream(stdout),
                    "stderr": stream(stderr),
                }));
            }
            _ => {}
        }
    }
    serde_json::json!({
        "task": task,
        "attempt": attempt,
        "turn": turn,
        "answer": answer,
        "calls": calls,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "diet-grade-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    /// A check that only reads passes the guard; one that writes, or
    /// touches, is refused, naming the path.
    #[test]
    fn the_guard_refuses_a_check_that_changes_the_worktree() {
        let dir = scratch("guard");
        std::fs::create_dir_all(dir.join("t1")).expect("a task directory");
        std::fs::write(dir.join("t1/answer.txt"), "42\n").expect("a file");
        let quick = std::time::Duration::from_secs(10);
        let read = guarded("cat t1/answer.txt", &dir, &[], quick).expect("a read");
        assert_eq!((read.stdout.as_str(), read.status), ("42\n", Some(0)));
        for (writes, names) in [
            ("echo 43 > t1/answer.txt", "t1/answer.txt"),
            ("touch -t 200001010000 t1/answer.txt", "t1/answer.txt"),
            ("mkdir t1/new", "t1/new"),
        ] {
            let refused = guarded(writes, &dir, &[], quick).expect_err(writes);
            assert!(refused.contains(names), "{writes}: {refused}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A check past its time is killed, and says so.
    #[test]
    fn a_check_past_its_time_is_killed_and_says_so() {
        let dir = scratch("timeout");
        let started = std::time::Instant::now();
        let ran =
            guarded("sleep 30", &dir, &[], std::time::Duration::from_millis(300)).expect("it ran");
        assert!(ran.timed_out && ran.status.is_none(), "{ran:?}");
        assert!(started.elapsed() < std::time::Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A verdict is one JSON object of exactly its three keys, read from
    /// the last line printed.
    #[test]
    fn a_verdict_is_its_three_keys_and_nothing_else() {
        let read =
            Verdict::read("checking\n{\"right\": false, \"expected\": \"a\", \"got\": \"b\"}\n")
                .expect("a verdict");
        assert_eq!(
            read,
            Verdict {
                right: false,
                expected: "a".to_owned(),
                got: "b".to_owned()
            }
        );
        for broken in [
            "",
            "not json",
            r#"{"right": "yes", "expected": "a", "got": "b"}"#,
            r#"{"right": true, "expected": "a"}"#,
            r#"{"right": true, "expected": "a", "got": "b", "why": "c"}"#,
        ] {
            assert!(Verdict::read(broken).is_err(), "{broken}");
        }
    }

    /// The templates fill, `got` clipped and `(nothing)` when blank.
    #[test]
    fn a_feedback_template_fills_its_words() {
        let verdict = Verdict {
            right: false,
            expected: "main\n".to_owned(),
            got: "  ".to_owned(),
        };
        assert_eq!(
            feedback(
                "Graded: wrong. Expected: {expected}. Got: {got}. Try again in ./{task} ({attempt}/{attempts}).",
                &verdict,
                "T3",
                (1, 3)
            ),
            "Graded: wrong. Expected: main. Got: (nothing). Try again in ./T3 (1/3)."
        );
        let long = Verdict {
            got: "x".repeat(2_000),
            ..verdict
        };
        let filled = feedback("{got}", &long, "T3", (1, 3));
        assert_eq!(filled.chars().count(), GOT_CAP);
        assert!(filled.ends_with("...[truncated]"));
    }

    /// The turn a check reads is the latest turn's: its calls with their
    /// streams, and its last trunk answer.
    #[test]
    fn the_turn_a_check_reads_is_the_latest_turns() {
        let text =
            include_str!("../../formats/log/fixtures/valid/a-v18-attempt-graded-right.jsonl");
        let lines = log::parse(text).expect("a log");
        let turn = turn_of(&lines, ("t1", 1));
        assert_eq!(turn["task"], "t1");
        assert_eq!(turn["turn"], 1);
        assert_eq!(turn["answer"], "hello");
        assert_eq!(turn["calls"], serde_json::json!([]));
    }
}
