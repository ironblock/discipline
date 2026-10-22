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
    let Some((seq, trunk_sha256, object_sha256, restored)) =
        lines.iter().rev().find_map(|line| match &line.event {
            Event::Meditation {
                trunk_sha256: Some(trunk),
                object_sha256,
                restored_files,
                ..
            } => Some((
                line.seq,
                trunk.clone(),
                object_sha256.clone(),
                restored_files.clone(),
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
        },
        source: (!source.is_empty()).then_some(source),
    })
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
