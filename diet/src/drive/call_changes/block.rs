//! How what a call changed reads in its result (#1149): the one place the
//! block's words live, apart from how the changes are counted ([`super`]),
//! so a different form (a diff, say) changes this file alone.
//!
//! The wording is the maintainer's to approve: data, not an instruction.

use std::fmt::Write as _;

use super::Entry;
use crate::meditation::changed;

/// What the block says first.
pub const CHANGED: &str = "Files this call changed:";

/// What the block says of the files it names whose text the context does
/// not show (the shown rule of 2026-11-20).
pub const NOT_SHOWN: &str = "Not shown in this context by read, write or edit:";

/// A call's block, as counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Block<'a> {
    /// What it changed: the worktree's `entries`; the files an `edit` or
    /// `write` named `outside` the working directory, never counted; and,
    /// of both, those whose text the context does not show, `not_shown`.
    Counted {
        entries: &'a [Entry],
        outside: &'a [String],
        not_shown: &'a [String],
    },
    /// It returned while still running: in the background, or moved there.
    StillRunning,
    /// Its worktree could not be listed.
    Unlisted,
}

/// `block` as the model reads it, a line for each of its parts, each ending
/// with a newline. Past [`changed::MAX_NAMED`] names on a line, the rest are
/// counted.
#[must_use]
pub fn render(block: &Block) -> String {
    let (entries, outside, not_shown) = match block {
        Block::Counted {
            entries,
            outside,
            not_shown,
        } => (*entries, *outside, *not_shown),
        Block::StillRunning => {
            return format!("{CHANGED} not counted, the command is still running.\n");
        }
        Block::Unlisted => {
            return format!("{CHANGED} not counted, the working directory could not be listed.\n");
        }
    };
    let changed: Vec<String> = entries
        .iter()
        .map(text)
        .chain(
            outside
                .iter()
                .map(|path| format!("{path} (outside the working directory, not counted)")),
        )
        .collect();
    let mut out = if changed.is_empty() {
        format!("{CHANGED} none.\n")
    } else {
        format!("{CHANGED} {}.\n", named(&changed))
    };
    if !not_shown.is_empty() {
        let _ = writeln!(out, "{NOT_SHOWN} {}.", named(not_shown));
    }
    out
}

/// `items` joined, past [`changed::MAX_NAMED`] the rest counted.
fn named(items: &[String]) -> String {
    let mut text = items
        .iter()
        .take(changed::MAX_NAMED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    if items.len() > changed::MAX_NAMED {
        let _ = write!(text, ", and {} more", items.len() - changed::MAX_NAMED);
    }
    text
}

/// One entry as the block names it.
fn text(entry: &Entry) -> String {
    let not_counted = "lines not counted";
    match entry {
        Entry::Modified(path, Some((added, removed))) => format!("{path} (+{added} -{removed})"),
        Entry::Modified(path, None) => format!("{path} ({not_counted})"),
        Entry::Created(path, Some(lines)) => format!("{path} (new, +{lines})"),
        Entry::Created(path, None) => format!("{path} (new, {not_counted})"),
        Entry::Deleted(path, Some(lines)) => format!("{path} (deleted, -{lines})"),
        Entry::Deleted(path, None) => format!("{path} (deleted)"),
        Entry::CreatedDir(path, 1) => format!("{path}/ (1 new file)"),
        Entry::CreatedDir(path, n) => format!("{path}/ ({n} new files)"),
        Entry::DeletedDir(path, 1) => format!("{path}/ (1 file deleted)"),
        Entry::DeletedDir(path, n) => format!("{path}/ ({n} files deleted)"),
    }
}

/// `shown`, a call's result after the output cap, with `block` after it, a
/// blank line between.
#[must_use]
pub fn appended(shown: &str, block: &Block) -> String {
    let block = render(block);
    if shown.is_empty() {
        return block;
    }
    let gap = if shown.ends_with('\n') { "\n" } else { "\n\n" };
    format!("{shown}{gap}{block}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counted(entries: &[Entry], outside: &[String], not_shown: &[String]) -> String {
        render(&Block::Counted {
            entries,
            outside,
            not_shown,
        })
    }

    #[test]
    fn each_entry_reads_as_its_change() {
        let entries = [
            Entry::Modified("app.py".to_owned(), Some((1, 1))),
            Entry::Modified("blob.bin".to_owned(), None),
            Entry::Created("new.md".to_owned(), Some(12)),
            Entry::Created("link".to_owned(), None),
            Entry::Deleted("old.txt".to_owned(), Some(4)),
            Entry::Deleted("gone.bin".to_owned(), None),
            Entry::CreatedDir("build".to_owned(), 1),
            Entry::CreatedDir("dist".to_owned(), 3),
            Entry::DeletedDir("cache".to_owned(), 1),
            Entry::DeletedDir("tmp".to_owned(), 2),
        ];
        assert_eq!(
            counted(&entries, &["/tmp/out.txt".to_owned()], &[]),
            "Files this call changed: app.py (+1 -1), blob.bin (lines not counted), new.md \
             (new, +12), link (new, lines not counted), old.txt (deleted, -4), gone.bin \
             (deleted), build/ (1 new file), dist/ (3 new files), cache/ (1 file deleted), tmp/ \
             (2 files deleted), /tmp/out.txt (outside the working directory, not counted).\n"
        );
    }

    #[test]
    fn the_block_names_none_and_counts_past_twenty() {
        assert_eq!(counted(&[], &[], &[]), "Files this call changed: none.\n");
        assert_eq!(
            counted(&[], &[], &["app.py".to_owned()]),
            "Files this call changed: none.\nNot shown in this context by read, write or edit: \
             app.py.\n"
        );
        let many: Vec<Entry> = (0..22)
            .map(|n| Entry::Modified(format!("f{n:02}"), Some((1, 0))))
            .collect();
        assert!(counted(&many, &[], &[]).ends_with("f19 (+1 -0), and 2 more.\n"));
        assert_eq!(
            render(&Block::StillRunning),
            "Files this call changed: not counted, the command is still running.\n"
        );
        assert_eq!(
            render(&Block::Unlisted),
            "Files this call changed: not counted, the working directory could not be listed.\n"
        );
    }

    #[test]
    fn the_block_follows_the_result_after_a_blank_line() {
        let none = Block::Counted {
            entries: &[],
            outside: &[],
            not_shown: &[],
        };
        let block = "Files this call changed: none.\n";
        assert_eq!(appended("", &none), block);
        assert_eq!(appended("out\n", &none), format!("out\n\n{block}"));
        assert_eq!(appended("Updated.", &none), format!("Updated.\n\n{block}"));
    }
}
