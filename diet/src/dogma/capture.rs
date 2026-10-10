//! The self-capture contract sets (#609): the tools a served session offers
//! the model for recording into working memory, and the reminder's words.
//!
//! A set is one directory under `diet/dogma/self-capture/<name>/`: the
//! contract (`contract.jsonl`, the tools' names, descriptions and
//! parameters) and the advisory asks (`reminder.txt`, `sweep.txt`). Every
//! file is pinned in `MANIFEST.tsv` as `self-capture/<name>/<file>`, and a
//! set's digest is the digest of those lines, as the fork ask sets'
//! (`super::asks`). A change adds a set and never edits one.

use std::fmt::Write as _;

use super::digest;

/// One named self-capture contract set.
#[derive(Debug, PartialEq, Eq)]
pub struct CaptureSet {
    /// Its name: the directory under `diet/dogma/self-capture/`.
    pub name: &'static str,
    /// Each file's name and text, in manifest order.
    files: &'static [(&'static str, &'static str)],
}

/// Today's contract and reminder words, moved into the dogma unchanged
/// (#609): `capture::tools`' as it shipped.
pub const C1: CaptureSet = CaptureSet {
    name: "c1",
    files: &[
        (
            "contract.jsonl",
            include_str!("../../dogma/self-capture/c1/contract.jsonl"),
        ),
        (
            "reminder.txt",
            include_str!("../../dogma/self-capture/c1/reminder.txt"),
        ),
        (
            "sweep.txt",
            include_str!("../../dogma/self-capture/c1/sweep.txt"),
        ),
    ],
};

/// c1's contract with a description on every parameter (the maintainer's
/// approval of 2026-10-10): the vote found that `OpenCode` 2 and Qwen Code
/// both describe each parameter, worded in Qwen Code's lowercase fragments.
/// The tools, their descriptions and their parameters' types are c1's; the
/// reminder and sweep words are c1's, byte for byte.
pub const C2: CaptureSet = CaptureSet {
    name: "c2",
    files: &[
        (
            "contract.jsonl",
            include_str!("../../dogma/self-capture/c2/contract.jsonl"),
        ),
        (
            "reminder.txt",
            include_str!("../../dogma/self-capture/c2/reminder.txt"),
        ),
        (
            "sweep.txt",
            include_str!("../../dogma/self-capture/c2/sweep.txt"),
        ),
    ],
};

/// Every set, in the order they were added.
pub const SETS: &[&CaptureSet] = &[&C1, &C2];

/// The set a session offers when its regimen names none: c1, today's.
pub const DEFAULT: &CaptureSet = &C1;

/// The set called `name`, if there is one.
#[must_use]
pub fn set(name: &str) -> Option<&'static CaptureSet> {
    SETS.iter().copied().find(|set| set.name == name)
}

impl CaptureSet {
    /// The text of the file `name`, if the set has one.
    #[must_use]
    pub fn text(&self, name: &str) -> Option<&'static str> {
        self.files
            .iter()
            .find(|(file, _)| *file == name)
            .map(|(_, text)| *text)
    }

    /// The contract's JSON lines.
    #[must_use]
    pub fn contract(&self) -> &'static str {
        self.text("contract.jsonl")
            .unwrap_or_else(|| unreachable!("set {} has no contract", self.name))
    }

    /// The set's manifest lines, as `MANIFEST.tsv` writes them.
    #[must_use]
    pub fn manifest_lines(&self) -> String {
        let mut lines = String::new();
        for (file, text) in self.files {
            let stem = file.rsplit_once('.').map_or(*file, |(stem, _)| stem);
            let _ = writeln!(
                lines,
                "self-capture/{}/{stem}\t{}\t{}",
                self.name,
                digest(text),
                text.len()
            );
        }
        lines
    }

    /// The set's digest: [`digest`] of its manifest lines.
    #[must_use]
    pub fn digest(&self) -> String {
        digest(&self.manifest_lines())
    }
}

#[cfg(test)]
mod tests {
    use super::{C1, C2, DEFAULT, SETS, set};
    use crate::dogma::MANIFEST;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Every set file matches its manifest line, and the manifest pins no
    /// self-capture file that is not embedded.
    #[test]
    fn every_set_file_matches_its_manifest_line_and_nothing_else_does() {
        let pinned: BTreeSet<String> = MANIFEST
            .lines()
            .filter(|line| line.starts_with("self-capture/"))
            .map(|line| format!("{line}\n"))
            .collect();
        let embedded: BTreeSet<String> = SETS
            .iter()
            .flat_map(|set| {
                set.manifest_lines()
                    .lines()
                    .map(|line| format!("{line}\n"))
                    .collect::<Vec<_>>()
            })
            .collect();
        assert_eq!(
            pinned, embedded,
            "diet/dogma/self-capture/ and MANIFEST.tsv disagree"
        );
    }

    /// Every file on disk under `self-capture/` belongs to a set.
    #[test]
    fn every_set_file_on_disk_is_embedded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("dogma/self-capture");
        let mut on_disk = BTreeSet::new();
        for dir in std::fs::read_dir(&root).expect("the sets") {
            let dir = dir.expect("an entry").path();
            let name = dir
                .file_name()
                .and_then(|n| n.to_str())
                .expect("a name")
                .to_owned();
            for file in std::fs::read_dir(&dir).expect("a set") {
                let file = file.expect("an entry").path();
                let file = file.file_name().and_then(|s| s.to_str()).expect("a name");
                on_disk.insert(format!("{name}/{file}"));
            }
        }
        let embedded: BTreeSet<String> = SETS
            .iter()
            .flat_map(|set| {
                set.files
                    .iter()
                    .map(|(file, _)| format!("{}/{file}", set.name))
            })
            .collect();
        assert_eq!(on_disk, embedded);
    }

    /// c1 is the words `capture::tools` shipped with.
    #[test]
    fn c1_is_todays_contract_and_reminder() {
        assert_eq!(
            C1.text("reminder.txt"),
            Some("Anything you meant to record?")
        );
        assert!(C1.contract().contains("\"tool\":\"update_record\""));
    }

    /// c2 changes the contract alone: its reminder and sweep are c1's.
    #[test]
    fn c2_keeps_c1s_reminder_and_sweep() {
        for file in ["reminder.txt", "sweep.txt"] {
            assert_eq!(C2.text(file), C1.text(file), "{file}");
        }
        assert_ne!(C2.contract(), C1.contract());
    }

    /// A set is found by its name, and c1 is the default.
    #[test]
    fn a_set_is_found_by_name_and_c1_is_the_default() {
        assert_eq!(set("c1"), Some(&C1));
        assert_eq!(set("c2"), Some(&C2));
        assert_eq!(set("c3"), None);
        assert_eq!(DEFAULT, &C1);
    }
}
