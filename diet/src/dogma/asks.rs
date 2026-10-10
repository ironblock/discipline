//! The fork ask sets (#595): the words a served fork asks in, as named sets.
//!
//! A set is one directory under `diet/dogma/fork-asks/<name>/`: one file per
//! router ask kind, the fork-local imperative, and optionally the priming
//! paragraph the session's system prompt carries from its start. Every file
//! is pinned in `MANIFEST.tsv` as `fork-asks/<name>/<file>`, and a set's
//! digest is the digest of those manifest lines, so it can be recomputed
//! from the manifest alone. A regimen picks a set by name; a change adds a
//! set and never edits one, so a run under an earlier set still reproduces.
//! Speculative by the maintainer's word (2026-10-09): "be prepared to change
//! the list, allow different asks in different regimens".

use std::fmt::Write as _;

use super::digest;

/// One named set of fork asks.
#[derive(Debug, PartialEq, Eq)]
pub struct AskSet {
    /// Its name: the directory under `diet/dogma/fork-asks/`.
    pub name: &'static str,
    /// Each file's stem and text, in manifest order.
    files: &'static [(&'static str, &'static str)],
}

macro_rules! set_files {
    ($set:literal: $($file:literal),* $(,)?) => {
        &[$(($file, include_str!(concat!("../../dogma/fork-asks/", $set, "/", $file, ".txt")))),*]
    };
}

/// Today's router asks, moved into the dogma unchanged (#595 (c)).
pub const V3: AskSet = AskSet {
    name: "v3",
    files: set_files!("v3": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "reminder"),
};

/// v3, with the judgment ask showing the working record and asking for
/// SUPERSEDE (#595 (a), #562).
pub const V4: AskSet = AskSet {
    name: "v4",
    files: set_files!("v4": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "reminder"),
};

/// Every set, in the order they were added.
pub const SETS: &[&AskSet] = &[&V3, &V4];

/// The set a served regimen that names none runs: v3, today's asks, by the
/// maintainer's decision (2026-10-09); v4 is selectable beside it.
pub const DEFAULT: &AskSet = &V3;

/// The set named `name`, if there is one.
#[must_use]
pub fn set(name: &str) -> Option<&'static AskSet> {
    SETS.iter().copied().find(|set| set.name == name)
}

impl AskSet {
    /// The text of the file `stem` (an ask kind's tag, `imperative`, or
    /// `priming`), if the set has one.
    #[must_use]
    pub fn text(&self, stem: &str) -> Option<&'static str> {
        self.files
            .iter()
            .find(|(file, _)| *file == stem)
            .map(|(_, text)| *text)
    }

    /// Each file's stem and text.
    #[must_use]
    pub fn files(&self) -> &'static [(&'static str, &'static str)] {
        self.files
    }

    /// The priming paragraph the system prompt carries from the session's
    /// start, when the set has one. Never added to a session already open.
    #[must_use]
    pub fn priming(&self) -> Option<&'static str> {
        self.text("priming")
    }

    /// The set's manifest lines, as `MANIFEST.tsv` writes them.
    #[must_use]
    pub fn manifest_lines(&self) -> String {
        let mut lines = String::new();
        for (file, text) in self.files {
            let _ = writeln!(
                lines,
                "fork-asks/{}/{file}\t{}\t{}",
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
    use super::{DEFAULT, SETS, V3, V4, set};
    use crate::dogma::MANIFEST;
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Every set's files are pinned in the manifest, line for line, and the
    /// manifest pins no set file that is not embedded.
    #[test]
    fn every_set_file_matches_its_manifest_line_and_nothing_else_does() {
        let pinned: BTreeSet<String> = MANIFEST
            .lines()
            .filter(|line| line.starts_with("fork-asks/"))
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
            "\n\ndiet/dogma/fork-asks/ and MANIFEST.tsv disagree. A set is never edited: add a \
             set, pin its files in MANIFEST.tsv, bump dogma::VERSION, and say why.\n"
        );
    }

    /// Every file on disk under `fork-asks/` belongs to a set.
    #[test]
    fn every_set_file_on_disk_is_embedded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("dogma/fork-asks");
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
                let stem = file.file_stem().and_then(|s| s.to_str()).expect("a stem");
                on_disk.insert(format!("{name}/{stem}"));
            }
        }
        let embedded: BTreeSet<String> = SETS
            .iter()
            .flat_map(|set| {
                set.files()
                    .iter()
                    .map(|(file, _)| format!("{}/{file}", set.name))
            })
            .collect();
        assert_eq!(on_disk, embedded);
    }

    #[test]
    fn sets_are_found_by_name_and_v3_is_the_default() {
        assert_eq!(set("v3"), Some(&V3));
        assert_eq!(set("v4"), Some(&V4));
        assert_eq!(set("v9"), None);
        assert_eq!(DEFAULT, &V3);
        assert_ne!(V3.digest(), V4.digest());
        assert_eq!(
            V3.priming(),
            None,
            "no priming ships until its wording is approved"
        );
        assert_eq!(V4.priming(), None);
    }

    /// v4 differs from v3 in the judgment ask alone.
    #[test]
    fn v4_is_v3_with_the_judgment_ask_changed() {
        for (file, text) in V3.files() {
            let changed = V4.text(file) != Some(*text);
            assert_eq!(changed, *file == "judgment", "{file}");
        }
    }
}
