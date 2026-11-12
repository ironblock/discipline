//! The files the model changed (N5): one line a refill carries, counted from
//! the worktree -- its files' digests at the session's start against the
//! refill's -- never from the commands the model ran, so a file written by a
//! script or a here-doc counts as one written by an edit tool, and a file
//! only read does not.
//!
//! The line's wording is proposed for the maintainer: "you", as the render's
//! own second person.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// The most entries the line names; the rest are counted.
pub const MAX_NAMED: usize = 20;

/// What the line says before the files.
pub const SENTENCE: &str = "You changed these files since the session started:";

/// One entry on the line: a file, or a directory that was not there before
/// (or is gone) with how many files under it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Entry {
    Modified(String),
    Created(String),
    Deleted(String),
    CreatedDir(String, usize),
    DeletedDir(String, usize),
}

impl Entry {
    fn path(&self) -> &str {
        match self {
            Self::Modified(path)
            | Self::Created(path)
            | Self::Deleted(path)
            | Self::CreatedDir(path, _)
            | Self::DeletedDir(path, _) => path,
        }
    }

    fn text(&self) -> String {
        match self {
            Self::Modified(path) => path.clone(),
            Self::Created(path) => format!("{path} (new)"),
            Self::Deleted(path) => format!("{path} (deleted)"),
            Self::CreatedDir(path, 1) => format!("{path}/ (1 new file)"),
            Self::CreatedDir(path, n) => format!("{path}/ ({n} new files)"),
            Self::DeletedDir(path, 1) => format!("{path}/ (1 file deleted)"),
            Self::DeletedDir(path, n) => format!("{path}/ ({n} files deleted)"),
        }
    }
}

/// Every directory holding a file of `files`, at any depth.
fn dirs(files: &BTreeMap<String, String>) -> BTreeSet<&str> {
    let mut out = BTreeSet::new();
    for path in files.keys() {
        let mut at = path.as_str();
        while let Some(cut) = at.rfind('/') {
            at = &at[..cut];
            out.insert(at);
        }
    }
    out
}

/// The shallowest directory above `path` that `known` does not hold, if any.
fn new_dir<'a>(path: &'a str, known: &BTreeSet<&str>) -> Option<&'a str> {
    path.match_indices('/')
        .map(|(cut, _)| &path[..cut])
        .find(|dir| !known.contains(dir))
}

/// The line naming what changed between `before` and `after` -- each a
/// worktree's files by relative path to their digest -- or `None` when
/// nothing did. A directory that was not there before, or is gone, is one
/// entry with its count, so a dependency install is not a thousand names.
#[must_use]
pub fn line(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) -> Option<String> {
    let (known_before, known_after) = (dirs(before), dirs(after));
    let mut entries = BTreeSet::new();
    let mut created_dirs: BTreeMap<&str, usize> = BTreeMap::new();
    let mut deleted_dirs: BTreeMap<&str, usize> = BTreeMap::new();
    for (path, digest) in after {
        match before.get(path) {
            Some(was) if was == digest => {}
            Some(_) => {
                entries.insert(Entry::Modified(path.clone()));
            }
            None => match new_dir(path, &known_before) {
                Some(dir) => *created_dirs.entry(dir).or_default() += 1,
                None => {
                    entries.insert(Entry::Created(path.clone()));
                }
            },
        }
    }
    for path in before.keys().filter(|path| !after.contains_key(*path)) {
        match new_dir(path, &known_after) {
            Some(dir) => *deleted_dirs.entry(dir).or_default() += 1,
            None => {
                entries.insert(Entry::Deleted(path.clone()));
            }
        }
    }
    entries.extend(
        created_dirs
            .into_iter()
            .map(|(dir, n)| Entry::CreatedDir(dir.to_owned(), n)),
    );
    entries.extend(
        deleted_dirs
            .into_iter()
            .map(|(dir, n)| Entry::DeletedDir(dir.to_owned(), n)),
    );
    if entries.is_empty() {
        return None;
    }
    let mut entries: Vec<Entry> = entries.into_iter().collect();
    entries.sort_by(|a, b| a.path().cmp(b.path()).then_with(|| a.cmp(b)));
    let named: Vec<String> = entries.iter().take(MAX_NAMED).map(Entry::text).collect();
    let mut text = format!("{SENTENCE} {}", named.join(", "));
    if entries.len() > MAX_NAMED {
        let _ = write!(text, ", and {} more", entries.len() - MAX_NAMED);
    }
    text.push('.');
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(path, digest)| ((*path).to_owned(), (*digest).to_owned()))
            .collect()
    }

    #[test]
    fn nothing_changed_is_no_line() {
        let tree = files(&[("a.rs", "1"), ("src/b.rs", "2")]);
        assert_eq!(line(&tree, &tree), None);
    }

    #[test]
    fn each_change_is_named_by_path_and_a_new_directory_is_counted() {
        let before = files(&[
            ("a.rs", "1"),
            ("gone.txt", "2"),
            ("old/x", "3"),
            ("old/y", "4"),
            ("src/b.rs", "5"),
        ]);
        let after = files(&[
            ("a.rs", "9"),
            ("node_modules/p/index.js", "6"),
            ("node_modules/q/index.js", "7"),
            ("src/b.rs", "5"),
            ("src/c.rs", "8"),
        ]);
        assert_eq!(
            line(&before, &after).as_deref(),
            Some(
                "You changed these files since the session started: a.rs, gone.txt (deleted), \
                 node_modules/ (2 new files), old/ (2 files deleted), src/c.rs (new)."
            )
        );
    }

    #[test]
    fn past_the_most_named_the_rest_are_counted() {
        let after: BTreeMap<String, String> = (0..MAX_NAMED + 3)
            .map(|n| (format!("f{n:02}"), "x".to_owned()))
            .collect();
        let text = line(&BTreeMap::new(), &after).expect("a line");
        assert!(text.ends_with("f19 (new), and 3 more."), "{text}");
    }
}
