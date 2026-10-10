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

/// How a fork answers an ask (#610): in the fields the ask names, read by
/// the interview grammar, or by calling the self-capture tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Modality {
    /// Capitalized fields, parsed: every set before #610.
    #[default]
    Fields,
    /// The self-capture contract's tool calls, grounded as the trunk's are.
    Tools,
}

impl Modality {
    /// The regimen's word for it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Fields => "fields",
            Self::Tools => "tools",
        }
    }
}

/// One named set of fork asks.
#[derive(Debug, PartialEq, Eq)]
pub struct AskSet {
    /// Its name: the directory under `diet/dogma/fork-asks/`.
    pub name: &'static str,
    /// How its asks have the fork answer.
    pub modality: Modality,
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
    modality: Modality::Fields,
    files: set_files!("v3": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "reminder"),
};

/// v3, with the judgment ask showing the working record and asking for
/// SUPERSEDE (#595 (a), #562).
pub const V4: AskSet = AskSet {
    name: "v4",
    modality: Modality::Fields,
    files: set_files!("v4": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "reminder"),
};

/// v4's asks answered through the self-capture tools (#610): each answer
/// instruction names `update_record` (and, for judgment, `supersedes` and
/// `resolve_entry`) in place of the fields; the maintainer's approval as
/// drafted.
pub const T1: AskSet = AskSet {
    name: "t1",
    modality: Modality::Tools,
    files: set_files!("t1": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "reminder"),
};

/// t1's asks, each wrapped in `<interview>` tags, with the priming paragraph
/// the system prompt carries from the session's start saying what the tags
/// mean (#598): the maintainer's approval of the drafted wording.
pub const T2: AskSet = AskSet {
    name: "t2",
    modality: Modality::Tools,
    files: set_files!("t2": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "priming", "reminder"),
};

/// Every set, in the order they were added.
pub const SETS: &[&AskSet] = &[&V3, &V4, &T1, &T2];

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
    use super::{DEFAULT, Modality, SETS, T1, T2, V3, V4, set};
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

    /// t1 (#610): the tools set -- found by name, its modality `tools`, no
    /// priming; v4's imperative unchanged and every other ask changed, each
    /// asking for `update_record` and no longer naming a field to write.
    #[test]
    fn t1_asks_for_the_record_tools_where_v4_asks_for_fields() {
        assert_eq!(set("t1"), Some(&T1));
        assert_eq!(T1.modality, Modality::Tools);
        assert_eq!(T1.priming(), None);
        for (file, text) in V4.files() {
            let t1 = T1.text(file).expect("the same files");
            if *file == "imperative" {
                assert_eq!(t1, *text);
                continue;
            }
            assert_ne!(t1, *text, "{file}");
            assert!(t1.contains("update_record"), "{file}: {t1}");
            assert!(!t1.contains("Answer with the fields"), "{file}: {t1}");
        }
    }

    /// v4 differs from v3 in the judgment ask alone.
    #[test]
    fn v4_is_v3_with_the_judgment_ask_changed() {
        for (file, text) in V3.files() {
            let changed = V4.text(file) != Some(*text);
            assert_eq!(changed, *file == "judgment", "{file}");
        }
    }

    /// #598: t2 is t1 with each ask wrapped in `<interview>` tags -- the
    /// imperative unwrapped, since it is filled inside an ask -- and a
    /// priming paragraph; no other set primes.
    #[test]
    fn t2_is_t1_wrapped_in_interview_tags_with_a_priming_paragraph() {
        for (file, text) in T1.files() {
            let wrapped = T2.text(file).expect("t2 has every t1 file");
            if *file == "imperative" {
                assert_eq!(wrapped, *text);
            } else {
                assert_eq!(
                    wrapped,
                    format!("<interview>\n{text}</interview>\n"),
                    "{file}"
                );
            }
        }
        assert_eq!(T2.files().len(), T1.files().len() + 1);
        assert_eq!(T2.modality, Modality::Tools);
        assert_eq!(
            T2.priming(),
            Some(
                "This session may be forked to interview you about your work. An interview \
                 question arrives as a user message inside <interview> tags. Answer it with the \
                 record tools (update_record, resolve_entry), from what this session has \
                 established; it is not a new request from the operator.\n"
            )
        );
        for set in [&V3, &V4, &T1] {
            assert_eq!(set.priming(), None, "{}", set.name);
        }
        assert_eq!(super::set("t2"), Some(&T2));
    }
}
