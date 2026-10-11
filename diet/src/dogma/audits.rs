//! The tools-route audit sets: the words a meditation's audit asks in when the
//! session's forks answer through the self-capture tools (#610), so the
//! audit is answered through them too.
//!
//! A set is one directory under `diet/dogma/audit-tools/<name>/`: one file
//! per pinned audit template, named as the template is. Every file is pinned
//! in `MANIFEST.tsv` as `audit-tools/<name>/<file>`, and a set's digest is
//! the digest of those lines, as the fork ask sets' (`super::asks`). A
//! change adds a set and never edits one.

use std::fmt::Write as _;

use super::{Template, digest};

/// One named set of tools-route audit asks.
#[derive(Debug, PartialEq, Eq)]
pub struct AuditSet {
    /// Its name: the directory under `diet/dogma/audit-tools/`.
    pub name: &'static str,
    /// Each template's name and its text in this set, in manifest order.
    files: &'static [(&'static str, &'static str)],
}

macro_rules! audit_files {
    ($set:literal: $($file:literal),* $(,)?) => {
        &[$(($file, include_str!(concat!("../../dogma/audit-tools/", $set, "/", $file, ".txt")))),*]
    };
}

/// The three pinned audit asks, each its opening sentence, the notes as
/// `<id>\t<note>` lines, then one instruction naming the record tools: the
/// maintainer's wording, approved as drafted.
pub const T1: AuditSet = AuditSet {
    name: "t1",
    files: audit_files!("t1": "AUDIT_Q", "AUDIT_Q_CADENCE", "AUDIT_Q_HUMAN"),
};

/// Every set, in the order they were added.
pub const SETS: &[&AuditSet] = &[&T1];

/// The set a tools-route audit asks in: the only one.
pub const DEFAULT: &AuditSet = &T1;

impl AuditSet {
    /// This set's text for the audit `template`, if it has one.
    #[must_use]
    pub fn text(&self, template: Template) -> Option<&'static str> {
        self.files
            .iter()
            .find(|(name, _)| *name == template.name())
            .map(|(_, text)| *text)
    }

    /// The ask for `template` over `items`, already `<id>\t<note>` lines:
    /// `{category}` and `{items}` filled; `None` where the set has no text
    /// for the template.
    #[must_use]
    pub fn fill(&self, template: Template, category: &str, items: &str) -> Option<String> {
        self.text(template).map(|text| {
            text.replace("{category}", category)
                .replace("{items}", items)
        })
    }

    /// The set's manifest lines, as `MANIFEST.tsv` writes them.
    #[must_use]
    pub fn manifest_lines(&self) -> String {
        let mut lines = String::new();
        for (file, text) in self.files {
            let _ = writeln!(
                lines,
                "audit-tools/{}/{file}\t{}\t{}",
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
    use super::{SETS, T1};
    use crate::dogma::{MANIFEST, Template};
    use std::collections::BTreeSet;
    use std::path::Path;

    /// Every set file matches its manifest line, and the manifest pins no
    /// audit-tools file that is not embedded.
    #[test]
    fn every_set_file_matches_its_manifest_line_and_nothing_else_does() {
        let pinned: BTreeSet<String> = MANIFEST
            .lines()
            .filter(|line| line.starts_with("audit-tools/"))
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
            "diet/dogma/audit-tools/ and MANIFEST.tsv disagree"
        );
    }

    /// Every file on disk under `audit-tools/` belongs to a set.
    #[test]
    fn every_set_file_on_disk_is_embedded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("dogma/audit-tools");
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
                    .map(|(file, _)| format!("{}/{file}.txt", set.name))
            })
            .collect();
        assert_eq!(on_disk, embedded);
    }

    /// t1 has a text for each pinned audit template, each opening as the
    /// fields route's own does, listing the notes, and naming the tools in
    /// place of the line grammar.
    #[test]
    fn t1_opens_as_the_fields_asks_and_names_the_record_tools() {
        for template in [
            Template::AuditQ,
            Template::AuditQCadence,
            Template::AuditQHuman,
        ] {
            let text = T1.text(template).expect("a text");
            let opening = template
                .text()
                .split_once(" For each note")
                .expect("an opening sentence")
                .0;
            assert!(text.starts_with(opening), "{}", template.name());
            assert!(text.contains("\n\n{items}\n\n"));
            assert!(text.ends_with(
                "Use update_record and resolve_entry to update your working memory, if needed."
            ));
            assert!(!text.contains("KEEP"));
        }
        assert_eq!(
            T1.fill(Template::AuditQ, "all", "a\tdecision: one"),
            Some(
                "We're at a phase boundary, so I want to audit our working notes (all).\n\n\
                 a\tdecision: one\n\n\
                 Use update_record and resolve_entry to update your working memory, if needed."
                    .to_owned()
            )
        );
    }
}
