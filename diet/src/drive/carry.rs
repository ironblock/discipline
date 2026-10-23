//! A session carried from another's meditation (#948): what `serve --carry
//! LOG` reads.
//!
//! The first session's last meditation kept, in its recording (the log's
//! directory, `files/<sha256>`), the refilled trunk exactly as sent and the
//! working object's dump, and named both on its line. The new session starts
//! from those, each checked against its digest: nothing is re-derived, so a
//! native refill's restored files, a phase's rules and a background list
//! cross as they were sent, and no archive and no other recording file
//! crosses at all.

use std::path::Path;

use crate::formats::log::{self, CarriedFrom, Event};

use super::session::Carry;

/// The carry `log`'s last meditation kept, with the files it embedded that
/// lie under one of `sealed` named relative to it.
///
/// # Errors
///
/// When the log does not read or holds no meditation that kept a trunk, or
/// a kept file is missing or is not the bytes its digest names.
pub fn read(log: &Path, sealed: &[String]) -> Result<Carry, String> {
    let bytes = std::fs::read(log).map_err(|why| format!("{}: {why}", log.display()))?;
    let text =
        String::from_utf8(bytes.clone()).map_err(|_| format!("{} is not UTF-8", log.display()))?;
    let lines = log::parse(&text).map_err(|why| format!("{}: {why}", log.display()))?;
    let Some((seq, trunk_sha256, object_sha256, restored, worktree_sha256)) =
        lines.iter().rev().find_map(|line| match &line.event {
            Event::Meditation {
                trunk_sha256: Some(trunk),
                object_sha256,
                restored_files,
                worktree_sha256,
                ..
            } => Some((
                line.seq,
                trunk.clone(),
                object_sha256.clone(),
                restored_files.clone(),
                worktree_sha256.clone(),
            )),
            _ => None,
        })
    else {
        return Err(format!(
            "{} holds no meditation that kept its trunk (log v24): nothing to carry",
            log.display()
        ));
    };
    let recording = log.parent().unwrap_or_else(|| Path::new("."));
    let kept = |sha256: &str| -> Result<String, String> {
        let path = recording.join("files").join(sha256);
        let bytes = std::fs::read(&path).map_err(|why| format!("{}: {why}", path.display()))?;
        if crate::digest::sha256_hex(&bytes) != sha256 {
            return Err(format!("{} is not the bytes its name says", path.display()));
        }
        String::from_utf8(bytes).map_err(|_| format!("{} is not UTF-8", path.display()))
    };
    let trunk = crate::client::shape::messages_of_json(&kept(&trunk_sha256)?)
        .map_err(|why| format!("the carried trunk: {why}"))?;
    let object = object_sha256.as_deref().map(kept).transpose()?;
    let source: Vec<String> = restored
        .unwrap_or_default()
        .iter()
        .filter_map(|file| {
            sealed.iter().find_map(|root| {
                file.strip_prefix(root.trim_end_matches('/'))
                    .and_then(|rest| rest.strip_prefix('/'))
                    .map(str::to_owned)
            })
        })
        .collect();
    Ok(Carry {
        trunk,
        object,
        from: CarriedFrom {
            log_sha256: crate::digest::sha256_hex(&bytes),
            meditation_seq: seq,
            trunk_sha256,
            object_sha256,
            worktree: worktree_sha256.map(|sha256| (sha256, log::WorktreeFrom::Meditation)),
            variant_sha256: None,
        },
        source: (!source.is_empty()).then_some(source),
    })
}

/// The largest file a worktree snapshot keeps; a larger one is named in the
/// manifest as skipped, with its size, and never restored.
pub const SNAPSHOT_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// A worktree's snapshot (#948, log v26): every regular file under it, kept
/// whole in the recording by digest, and a manifest naming each by its path
/// relative to the worktree and its sha256, sorted by path, itself kept by
/// digest. Symlinks and files over [`SNAPSHOT_MAX_BYTES`] are named as
/// skipped, never followed or kept. Returns the manifest's sha256.
///
/// # Errors
///
/// A file that cannot be read or kept.
pub fn snapshot(worktree: &Path, recording: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut stack = vec![worktree.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|why| format!("{}: {why}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|why| format!("{}: {why}", dir.display()))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|why| format!("{}: {why}", path.display()))?;
            let rel = path
                .strip_prefix(worktree)
                .map_err(|_| format!("{} is outside its worktree", path.display()))?
                .to_string_lossy()
                .into_owned();
            if kind.is_symlink() {
                skipped.push(serde_json::json!({"path": rel, "why": "symlink"}));
            } else if kind.is_dir() {
                stack.push(path);
            } else if kind.is_file() {
                let size = entry
                    .metadata()
                    .map_err(|why| format!("{}: {why}", path.display()))?
                    .len();
                if size > SNAPSHOT_MAX_BYTES {
                    skipped.push(serde_json::json!({"path": rel, "why": "size", "bytes": size}));
                    continue;
                }
                let bytes =
                    std::fs::read(&path).map_err(|why| format!("{}: {why}", path.display()))?;
                let kept = super::attach::kept_whole(recording, &bytes, "application/octet-stream")
                    .map_err(|(_, why)| format!("{}: {why}", path.display()))?;
                files.push((rel, kept.sha256));
            }
        }
    }
    files.sort();
    skipped.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
    let manifest = serde_json::json!({
        "worktree": 1,
        "files": files.iter().map(|(path, sha256)| serde_json::json!({"path": path, "sha256": sha256})).collect::<Vec<_>>(),
        "skipped": skipped,
    });
    let text = serde_json::to_string(&manifest).map_err(|why| why.to_string())?;
    super::attach::kept_whole(recording, text.as_bytes(), "application/json")
        .map(|kept| kept.sha256)
        .map_err(|(_, why)| why)
}

/// The worktree snapshot `manifest_sha256` names, restored into `into` from
/// `recording`: each file checked against its digest before it is written.
/// Returns how many files it wrote.
///
/// # Errors
///
/// A manifest or file that is missing, is not the bytes its name says, or
/// names a path outside `into`.
pub fn restore(manifest_sha256: &str, recording: &Path, into: &Path) -> Result<usize, String> {
    let kept = |sha256: &str| -> Result<Vec<u8>, String> {
        let path = recording.join("files").join(sha256);
        let bytes = std::fs::read(&path).map_err(|why| format!("{}: {why}", path.display()))?;
        if crate::digest::sha256_hex(&bytes) != sha256 {
            return Err(format!("{} is not the bytes its name says", path.display()));
        }
        Ok(bytes)
    };
    let manifest: serde_json::Value = serde_json::from_slice(&kept(manifest_sha256)?)
        .map_err(|why| format!("the worktree manifest: {why}"))?;
    let files = manifest["files"]
        .as_array()
        .ok_or("the worktree manifest names no files")?;
    for file in files {
        let (Some(rel), Some(sha256)) = (file["path"].as_str(), file["sha256"].as_str()) else {
            return Err(format!(
                "a manifest entry without its path or digest: {file}"
            ));
        };
        let rel = Path::new(rel);
        if rel.is_absolute()
            || rel
                .components()
                .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            return Err(format!("{} is outside the worktree", rel.display()));
        }
        let to = into.join(rel);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|why| format!("{}: {why}", parent.display()))?;
        }
        std::fs::write(&to, kept(sha256)?).map_err(|why| format!("{}: {why}", to.display()))?;
    }
    Ok(files.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::shape::{Message, Role};

    /// A log whose meditation kept `trunk` and named `restored`, its kept
    /// files beside it, in a directory of its own.
    fn kept(case: &str, trunk: &[Message], restored: &[&str]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("diet-carry-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("files")).expect("the recording");
        let json = crate::client::shape::messages_json(trunk);
        let sha256 = crate::digest::sha256_hex(json.as_bytes());
        std::fs::write(dir.join("files").join(&sha256), &json).expect("the trunk");
        let fixture = include_str!(
            "../../formats/log/fixtures/valid/a-v16-meditation-carrying-a-native-summary.jsonl"
        );
        let text: String = fixture
            .lines()
            .map(|line| {
                let mut value: serde_json::Value = serde_json::from_str(line).expect("a line");
                if value["kind"] == "session.start" {
                    value["version"] = serde_json::json!(log::VERSION);
                }
                if value["kind"] == "meditation" {
                    value["trunk_sha256"] = serde_json::json!(sha256);
                    value["restored_files"] = serde_json::json!(restored);
                }
                value.to_string() + "\n"
            })
            .collect();
        std::fs::write(dir.join("session.log"), text).expect("the log");
        dir
    }

    /// #948: a carry is the trunk the meditation kept, exactly; its files
    /// under a sealed root are named relative to it, and the rest are not
    /// named at all.
    #[test]
    fn a_carry_is_the_kept_trunk_and_names_the_sealed_files_it_restored() {
        let trunk = vec![
            Message::new(Role::System, "you are the trunk"),
            Message::new(Role::User, "<state_snapshot>a tracker</state_snapshot>"),
        ];
        let dir = kept(
            "sealed",
            &trunk,
            &["/srv/lite/src/scene-core.ts", "/srv/work/main.ts"],
        );
        let carry = read(&dir.join("session.log"), &["/srv/lite".to_owned()]).expect("it reads");
        assert_eq!(carry.trunk, trunk);
        assert_eq!(carry.source, Some(vec!["src/scene-core.ts".to_owned()]));
        assert_eq!(carry.object, None);
        let log = std::fs::read(dir.join("session.log")).expect("the log");
        assert_eq!(carry.from.log_sha256, crate::digest::sha256_hex(&log));
        let unsealed = read(&dir.join("session.log"), &[]).expect("it reads");
        assert_eq!(unsealed.source, None, "nothing sealed, nothing named");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #948: a worktree snapshot restores every file as it was, a skipped
    /// symlink is not followed, and a kept file altered after the snapshot
    /// is refused rather than restored.
    #[test]
    fn a_worktree_snapshot_restores_what_it_kept_and_refuses_an_altered_file() {
        let dir = std::env::temp_dir().join(format!("diet-snapshot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (tree, recording, into) = (dir.join("tree"), dir.join("recording"), dir.join("into"));
        std::fs::create_dir_all(tree.join("t1-late-box")).expect("the tree");
        std::fs::write(
            tree.join("t1-late-box").join("main.ts"),
            "const solved = 1;\n",
        )
        .expect("a file");
        std::fs::write(tree.join("notes.txt"), "kept").expect("a file");
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", tree.join("link")).expect("a symlink");
        let manifest = snapshot(&tree, &recording).expect("a snapshot");
        assert_eq!(restore(&manifest, &recording, &into).expect("restored"), 2);
        assert_eq!(
            std::fs::read_to_string(into.join("t1-late-box").join("main.ts")).expect("restored"),
            "const solved = 1;\n"
        );
        assert!(!into.join("link").exists(), "a symlink is never followed");
        let listed =
            std::fs::read_to_string(recording.join("files").join(&manifest)).expect("the manifest");
        assert!(
            listed.contains(r#""path":"link""#) && listed.contains(r#""why":"symlink""#),
            "the manifest names the symlink it skipped: {listed}"
        );
        let sha = crate::digest::sha256_hex(b"const solved = 1;\n");
        std::fs::write(recording.join("files").join(&sha), "altered").expect("altered");
        assert!(restore(&manifest, &recording, &dir.join("again")).is_err());
        // A manifest naming a path outside the worktree is refused, and
        // nothing is written there.
        let escape = serde_json::json!({"worktree": 1, "files": [{"path": "../escaped.txt", "sha256": crate::digest::sha256_hex(b"kept")}], "skipped": []}).to_string();
        let escape_sha =
            super::super::attach::kept_whole(&recording, escape.as_bytes(), "application/json")
                .expect("kept")
                .sha256;
        assert!(restore(&escape_sha, &recording, &dir.join("inside")).is_err());
        assert!(!dir.join("escaped.txt").exists(), "nothing written outside");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #948: a kept trunk that is not the bytes its name says is refused,
    /// and so is a log whose meditations kept none.
    #[test]
    fn a_carry_whose_trunk_was_altered_or_never_kept_is_refused() {
        let dir = kept("altered", &[Message::new(Role::System, "x")], &[]);
        for entry in std::fs::read_dir(dir.join("files")).expect("the files") {
            let path = entry.expect("an entry").path();
            std::fs::write(&path, "[]").expect("altered");
        }
        assert!(read(&dir.join("session.log"), &[]).is_err());
        let none = std::env::temp_dir().join(format!("diet-carry-none-{}", std::process::id()));
        std::fs::create_dir_all(&none).expect("a directory");
        std::fs::write(
            none.join("session.log"),
            include_str!(
                "../../formats/log/fixtures/valid/a-v16-meditation-carrying-a-native-summary.jsonl"
            ),
        )
        .expect("a log");
        let refused = read(&none.join("session.log"), &[]).expect_err("nothing kept");
        assert!(refused.contains("nothing to carry"), "{refused}");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&none);
    }
}
