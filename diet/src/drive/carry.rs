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
            rerooted: None,
        },
        source: (!source.is_empty()).then_some(source),
    })
}

/// The carry rerooted into this session's worktree (#948, log v28): every
/// occurrence of the first session's worktree root in the carried text
/// replaced with this session's, for each `(from, to)` of `roots` in turn (a
/// worktree's resolved path and the path it was given, which a symlinked
/// temporary directory makes two). The text is the trunk's -- each
/// message's content, reasoning, notes and call arguments -- and the working
/// object's dump, so the native summary and the interview notes alike name
/// the worktree the session runs in. No other byte changes. A root followed
/// by a character a name goes on with (a letter, a digit, `_`, `-`) is another
/// path and is left. Reasoning a server signed is left too: it goes back
/// byte for byte or not at all. Given any root, `carried_from` names how many
/// were replaced, 0 among them (the carry was rerooted and nothing named the
/// old root), and the text's digests as sent. Returns how many.
pub fn reroot(carry: &mut Carry, roots: &[(String, String)]) -> u64 {
    let roots: Vec<(&str, &str)> = roots
        .iter()
        .map(|(from, to)| (from.trim_end_matches('/'), to.trim_end_matches('/')))
        .filter(|(from, to)| !from.is_empty() && from != to)
        .collect();
    let mut replaced = 0;
    let mut swap = |text: &mut String| {
        for (from, to) in &roots {
            let (out, n) = rooted_at(text, from, to);
            if n > 0 {
                *text = out;
                replaced += n;
            }
        }
    };
    for message in &mut carry.trunk {
        swap(&mut message.content);
        if message.reasoning_signature.is_none()
            && let Some(reasoning) = message.reasoning.as_mut()
        {
            swap(reasoning);
        }
        for note in &mut message.notes {
            swap(note);
        }
        for call in &mut message.tool_calls {
            swap(&mut call.arguments);
        }
    }
    if let Some(object) = carry.object.as_mut() {
        swap(object);
    }
    if !roots.is_empty() {
        let sent = crate::client::shape::messages_json(&carry.trunk);
        carry.from.rerooted = Some(Box::new(log::Rerooted {
            replaced,
            trunk_sha256: crate::digest::sha256_hex(sent.as_bytes()),
            object_sha256: carry
                .object
                .as_deref()
                .map(|object| crate::digest::sha256_hex(object.as_bytes())),
        }));
    }
    replaced
}

/// `text` with each whole occurrence of the root `from` replaced with `to`,
/// and how many: one followed by a letter, a digit, `_` or `-` is a longer
/// name and is left.
fn rooted_at(text: &str, from: &str, to: &str) -> (String, u64) {
    let mut out = String::with_capacity(text.len());
    let mut n = 0;
    let mut rest = text;
    while let Some(at) = rest.find(from) {
        let after = &rest[at + from.len()..];
        let whole = !after
            .chars()
            .next()
            .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-');
        out.push_str(&rest[..at]);
        out.push_str(if whole { to } else { from });
        n += u64::from(whole);
        rest = after;
    }
    out.push_str(rest);
    (out, n)
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

    /// #948 (smoke 2): the carried text names the first session's worktree;
    /// rerooted, every occurrence names the new one instead, in the trunk's
    /// content, reasoning, notes and call arguments and in the object's dump,
    /// a longer path that only starts with the root is left, and the start
    /// names how many were replaced and the digests of what was sent.
    #[test]
    fn a_rerooted_carry_names_the_new_worktree_and_never_the_old() {
        let old = "/tmp/diet-scenario-worktree-1";
        let new = "/tmp/diet-scenario-worktree-2";
        let mut summary = Message::new(
            Role::User,
            format!(
                "<state_snapshot>main.ts is at {old}/h1/main.ts; {old}-10/x is not mine</state_snapshot>"
            ),
        );
        summary.notes = vec![format!("notes: {old}/t1 and /private{old}/t2")];
        let mut answer = Message::new(Role::Assistant, "");
        answer.reasoning = Some(format!("I will cd {old}"));
        answer.tool_calls = vec![crate::client::shape::ToolCall {
            id: "c1".to_owned(),
            name: "bash".to_owned(),
            arguments: format!("{{\"command\":\"ls {old}/h1\"}}"),
        }];
        // A server's signed reasoning goes back byte for byte: left.
        let mut signed = Message::new(Role::Assistant, "done");
        signed.reasoning = Some(format!("signed: {old}/h1"));
        signed.reasoning_signature = Some("sig".to_owned());
        let dump = format!("{{\"facts\":[\"the stub is {old}/t1/main.ts\"]}}");
        let mut carry = Carry {
            trunk: vec![
                Message::new(Role::System, "the head"),
                summary,
                answer,
                signed,
            ],
            object: Some(dump),
            from: log::CarriedFrom {
                log_sha256: "a".repeat(64),
                meditation_seq: 1,
                trunk_sha256: "b".repeat(64),
                object_sha256: Some("c".repeat(64)),
                worktree: None,
                variant_sha256: None,
                rerooted: None,
            },
            source: None,
        };
        let roots = [
            (format!("/private{old}"), format!("/private{new}")),
            (old.to_owned(), new.to_owned()),
        ];
        let replaced = reroot(&mut carry, &roots);
        let sent = crate::client::shape::messages_json(&carry.trunk);
        let object = carry.object.clone().expect("the object");
        assert_eq!(replaced, 6, "{sent}\n{object}");
        assert!(
            sent.contains(&format!("/private{new}/t2")),
            "the resolved form, rerooted to its own: {sent}"
        );
        assert_eq!(
            carry.trunk[3].reasoning.as_deref(),
            Some(format!("signed: {old}/h1").as_str()),
            "signed reasoning is left"
        );
        let unsigned: Vec<_> = carry.trunk[..3].to_vec();
        let unsigned = crate::client::shape::messages_json(&unsigned);
        assert!(
            !unsigned.replace(&format!("{old}-10"), "").contains(old),
            "{unsigned}"
        );
        assert!(!object.contains(old), "{object}");
        assert!(
            sent.contains(&format!("{new}/h1/main.ts")) && sent.contains(&format!("{old}-10/x")),
            "{sent}"
        );
        assert!(object.contains(&format!("{new}/t1/main.ts")), "{object}");
        assert_eq!(carry.trunk[0].content, "the head", "no other byte changes");
        assert_eq!(
            carry.from.rerooted,
            Some(Box::new(log::Rerooted {
                replaced: 6,
                trunk_sha256: crate::digest::sha256_hex(sent.as_bytes()),
                object_sha256: Some(crate::digest::sha256_hex(object.as_bytes())),
            }))
        );
        assert_eq!(
            carry.from.trunk_sha256,
            "b".repeat(64),
            "the kept digest stays"
        );
        // Nothing left to replace: still named, with 0.
        let again = reroot(&mut carry, &roots);
        assert_eq!(again, 0);
        assert_eq!(carry.from.rerooted.as_ref().map(|r| r.replaced), Some(0));
        // No root given: no rerooting named at all.
        carry.from.rerooted = None;
        assert_eq!(reroot(&mut carry, &[]), 0);
        assert_eq!(carry.from.rerooted, None);
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
