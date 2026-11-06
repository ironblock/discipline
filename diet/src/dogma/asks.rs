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
    /// Whether a fork the operator fires -- in their own words, or a pinned
    /// ask by name -- is sent with the fields route's imperative as its first
    /// line (#1050): from `v5` and `t3`; no set before them. Its answer is
    /// read as text on either route, so it is told no tool can run.
    operator_imperative: bool,
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
    operator_imperative: false,
};

/// v3, with the judgment ask showing the working record and asking for
/// SUPERSEDE (#595 (a), #562).
pub const V4: AskSet = AskSet {
    name: "v4",
    modality: Modality::Fields,
    files: set_files!("v4": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "reminder"),
    operator_imperative: false,
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
    operator_imperative: false,
};

/// t1's asks, each wrapped in `<interview>` tags, with the priming paragraph
/// the system prompt carries from the session's start saying what the tags
/// mean (#598): the maintainer's approval of the drafted wording.
pub const T2: AskSet = AskSet {
    name: "t2",
    modality: Modality::Tools,
    files: set_files!("t2": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "priming", "reminder"),
    operator_imperative: false,
};

/// v4 with the fields route's imperative (#1050): no tool can run in a
/// fork, so it answers in text; the operator's forks are sent with it too.
/// The maintainer's wording, approved 2026-11-04.
pub const V5: AskSet = AskSet {
    name: "v5",
    modality: Modality::Fields,
    files: set_files!("v5": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "reminder"),
    operator_imperative: true,
};

/// t2 with the tools route's imperative (#1050): no tool runs in a fork but
/// the record tools it answers through. The operator's forks, read as text,
/// are sent with v5's instead. The maintainer's wording, approved 2026-11-04.
pub const T3: AskSet = AskSet {
    name: "t3",
    modality: Modality::Tools,
    files: set_files!("t3": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "priming", "reminder"),
    operator_imperative: true,
};

/// v5 with the judgment ask's SUPERSEDE sentence asking for the corrected
/// entry in full (#1050, item 3): every detail of it still true, with the
/// correction made. The maintainer's approved wording (relayed by Dispatch).
pub const V6: AskSet = AskSet {
    name: "v6",
    modality: Modality::Fields,
    files: set_files!("v6": "api_surface", "change", "finding", "generic", "imperative",
        "judgment", "outcome", "reminder"),
    operator_imperative: true,
};

/// Every set, in the order they were added.
pub const SETS: &[&AskSet] = &[&V3, &V4, &T1, &T2, &V5, &T3, &V6];

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

    /// The imperative a fork the operator fires is sent with as its first
    /// line, its own words or a pinned ask after it (#1050): the fields
    /// route's -- `v5`'s -- under `v5` and `t3`, since an operator fork's
    /// answer is read as text on both routes; none before them.
    #[must_use]
    pub fn operator_imperative(&self) -> Option<&'static str> {
        self.operator_imperative
            .then(|| V5.text("imperative"))
            .flatten()
            .map(str::trim)
    }

    /// The fields-route set an operator fork's set kind is rendered in when
    /// this set's route answers through tools (#1050): `t3`'s is `v5`. An
    /// accepted chip is the operator's fork, read as text, so it is asked in
    /// the text-shaped form.
    #[must_use]
    pub fn fields_counterpart(&self) -> Option<&'static AskSet> {
        (self.name == T3.name).then_some(&V5)
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
    use super::{DEFAULT, Modality, SETS, T1, T2, T3, V3, V4, V5, V6, set};
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

    /// #1050: v5 is v4 and t3 is t2, each with its route's imperative in the
    /// maintainer's words; both send the operator's forks with the fields
    /// line, and no earlier set does.
    #[test]
    fn v5_and_t3_carry_their_routes_imperative_and_send_the_operators_forks_with_the_fields_line() {
        for (new, old, imperative) in [
            (
                &V5,
                &V4,
                "Answer from this turn alone. No tool can run here: answer in text, from what you have already read.",
            ),
            (
                &T3,
                &T2,
                "Answer from this turn alone. No tool runs here except update_record and resolve_entry, which are how you answer.",
            ),
        ] {
            assert_eq!(set(new.name), Some(new));
            assert_eq!(new.modality, old.modality);
            assert_eq!(new.files().len(), old.files().len());
            for (file, text) in old.files() {
                let changed = new.text(file) != Some(*text);
                assert_eq!(changed, *file == "imperative", "{}: {file}", new.name);
            }
            assert_eq!(
                new.text("imperative"),
                Some(format!("{imperative}\n").as_str())
            );
        }
        // An operator fork is read as text on both routes: the fields line.
        for new in [&V5, &T3] {
            assert_eq!(
                new.operator_imperative(),
                Some(
                    "Answer from this turn alone. No tool can run here: answer in text, from what you have already read."
                ),
                "{}",
                new.name
            );
        }
        for old in [&V3, &V4, &T1, &T2] {
            assert_eq!(old.operator_imperative(), None, "{}", old.name);
        }
        // A chip under t3 is asked in v5's words; no other set has a counterpart.
        assert_eq!(T3.fields_counterpart(), Some(&V5));
        for other in [&V3, &V4, &T1, &T2, &V5] {
            assert_eq!(other.fields_counterpart(), None, "{}", other.name);
        }
    }

    /// #1050: v6 is v5 with the judgment ask's SUPERSEDE sentence asking
    /// for the corrected entry in full, in the maintainer's words.
    #[test]
    fn v6_is_v5_with_the_supersede_sentence_changed() {
        assert_eq!(set("v6"), Some(&V6));
        for (file, text) in V5.files() {
            let changed = V6.text(file) != Some(*text);
            assert_eq!(changed, *file == "judgment", "{file}");
        }
        let judgment = V6.text("judgment").expect("a judgment ask");
        assert!(judgment.contains(
            "SUPERSEDE cites the id of a working-record entry this turn corrects, then that \
             entry in full as it should now stand: every detail of it that is still true, with \
             the correction made;"
        ));
        assert_eq!(V6.operator_imperative(), V5.operator_imperative());
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
