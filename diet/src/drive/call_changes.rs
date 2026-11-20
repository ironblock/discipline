//! What one call changed in the worktree (#1149): after each `bash`, `edit`
//! and `write` call, the worktree is compared with its state just before the
//! call ran, and the tool result says which files the call changed and by
//! how many lines, or that it changed none. It is counted from the files,
//! never from the call's arguments, so a patch inside `bash` is seen, and a
//! replace that finds nothing says so.
//!
//! **A regimen switch, off by default** ([`CALL_CHANGES`]), named on the
//! start line. Off, no tool result changes by a byte.
//!
//! **Which files.** Each git repository's own rules: in one, the files
//! `git ls-files --cached --others --exclude-standard` lists, run as a free
//! git read ([`super::tool_loop::GIT_FREE_CONFIG`]), so an ignored path
//! stays out; a repository nested in it untracked, or a directory of
//! checkouts with none at its top, has each repository listed by its own
//! rules. A submodule's files are not listed. A file in no repository counts
//! (none is ignored), except under a `.git` directory, as the refill's
//! changed-files line counts it; so does every file of a worktree when git
//! cannot run. A path that leaves the listing while it is still on disk
//! became ignored, and a path that joins it whose inode last changed before
//! the call began (its ctime) was already there: neither is a change.
//!
//! **What is held.** The first count reads every listed file once. A text
//! file up to [`TEXT_MAX_BYTES`] is held by its sha256 and a hash of each
//! line, within [`KEPT_MAX_BYTES`] of line hashes in all, so its change is
//! counted in lines: the shortest edit script's, on whole lines (Myers).
//! Any other regular file is held by its sha256, or by its size and
//! modification time past [`super::carry::SNAPSHOT_MAX_BYTES`], and a symlink
//! by its target, never followed: such a change is named with its lines not
//! counted.
//!
//! **What a call costs.** The worktree is listed twice, just before the call
//! and after it, and every listed file is stat'ed each time. Only a file
//! whose stat changed is read, or one modified within [`RACY`] of the last
//! count, since a coarse clock can hide a same-size rewrite. Nothing is
//! hashed again unless its stat changed.
//!
//! **What the count includes.** A change made during the call by anything
//! (a background job, the user) is counted as the call's. A change made
//! between calls is not: the count just before the call takes it in.
//!
//! The wording is the maintainer's to approve: data, not an instruction.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs::Metadata;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime};

use crate::formats::regimen::{self, Regimen};
use crate::meditation::changed;

/// The regimen key of the per-call changes lever (#1149).
pub const CALL_CHANGES: &str = "call_changes";

/// Whether each `bash`, `edit` and `write` result says what the call changed
/// (#1149): `on`, or `off`, the default.
///
/// # Errors
///
/// When the key holds anything else: serve refuses it at start.
pub fn call_changes(regimen: &Regimen) -> Result<bool, String> {
    match regimen.get(CALL_CHANGES) {
        None => Ok(false),
        Some(regimen::Value::String(word)) if word == "off" => Ok(false),
        Some(regimen::Value::String(word)) if word == "on" => Ok(true),
        Some(other) => Err(format!(
            "`{CALL_CHANGES}` is {other:?}: it is `on`, or `off`"
        )),
    }
}

/// The largest file held by its lines, so its changes are counted in lines.
pub const TEXT_MAX_BYTES: u64 = 1024 * 1024;

/// The most line hashes held in all, in bytes (eight a line); past it, a
/// file is held by its digest alone.
pub const KEPT_MAX_BYTES: u64 = 64 * 1024 * 1024;

/// How recently a file was modified, at a count, for its stat not to be
/// trusted at the next: a filesystem with a one- or two-second clock can
/// hide a same-size rewrite in the same tick.
pub const RACY: Duration = Duration::from_secs(2);

/// The most edits the line count searches for in a file's changed middle;
/// past them, every line of the middle is counted removed and added.
pub const MAX_EDITS: usize = 2000;

/// What the block says first.
pub const CHANGED: &str = "Files this call changed:";

/// What the block says of the files it names whose text the context does
/// not show (#1149, the shown rule of 2026-11-20).
pub const NOT_SHOWN: &str = "Not shown in this context by read, write or edit:";

/// The block of a call that returned while still running (#1149): in the
/// background, or moved there.
pub const STILL_RUNNING: &str =
    "Files this call changed: not counted, the command is still running.\n";

/// The block of a call whose worktree could not be listed.
pub const UNLISTED: &str =
    "Files this call changed: not counted, the working directory could not be listed.\n";

/// The parts of a file's stat that change when it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Stat {
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
    ino: u64,
    dev: u64,
}

impl Stat {
    fn of(meta: &Metadata) -> Self {
        Self {
            len: meta.len(),
            mtime: (meta.mtime(), meta.mtime_nsec()),
            ctime: (meta.ctime(), meta.ctime_nsec()),
            ino: meta.ino(),
            dev: meta.dev(),
        }
    }
}

/// What a file is held by.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Held {
    /// A text file's sha256, and a hash of each of its lines.
    Text([u8; 32], Vec<u64>),
    /// Any other regular file's sha256.
    Digest([u8; 32]),
    /// A file too large to hash, or one that cannot be read: its size and
    /// modification time.
    Stat(u64, (i64, i64)),
    /// A symlink's target.
    Link(PathBuf),
}

impl Held {
    /// Whether `self` and `other` hold the same bytes.
    fn same(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a, _) | Self::Digest(a), Self::Text(b, _) | Self::Digest(b)) => a == b,
            _ => self == other,
        }
    }

    /// Its lines, when it is text.
    fn lines(&self) -> Option<u64> {
        match self {
            Self::Text(_, lines) => Some(lines.len() as u64),
            _ => None,
        }
    }

    /// The bytes of line hashes it holds.
    fn kept(&self) -> u64 {
        match self {
            Self::Text(_, lines) => 8 * lines.len() as u64,
            _ => 0,
        }
    }
}

/// One file the count holds.
#[derive(Debug)]
struct File {
    stat: Stat,
    held: Held,
    /// Modified within [`RACY`] of the count that read it.
    racy: bool,
}

/// One entry of a call's changes, in path order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A file whose bytes changed: lines added and removed, when counted.
    Modified(String, Option<(u64, u64)>),
    /// A file the call created: its lines, when counted.
    Created(String, Option<u64>),
    /// A file the call deleted: the lines it had, when counted.
    Deleted(String, Option<u64>),
    /// A directory that was not there before, with how many files it holds.
    CreatedDir(String, usize),
    /// A directory that is gone, with how many files it held.
    DeletedDir(String, usize),
}

impl Entry {
    /// The entry's path, relative to the worktree.
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Modified(path, _)
            | Self::Created(path, _)
            | Self::Deleted(path, _)
            | Self::CreatedDir(path, _)
            | Self::DeletedDir(path, _) => path,
        }
    }

    /// The file a mark may follow: one modified or created, never a deleted
    /// file or a directory counted whole.
    #[must_use]
    pub fn file(&self) -> Option<&str> {
        match self {
            Self::Modified(path, _) | Self::Created(path, _) => Some(path),
            _ => None,
        }
    }

    fn text(&self) -> String {
        let not_counted = "lines not counted";
        match self {
            Self::Modified(path, Some((added, removed))) => format!("{path} (+{added} -{removed})"),
            Self::Modified(path, None) => format!("{path} ({not_counted})"),
            Self::Created(path, Some(lines)) => format!("{path} (new, +{lines})"),
            Self::Created(path, None) => format!("{path} (new, {not_counted})"),
            Self::Deleted(path, Some(lines)) => format!("{path} (deleted, -{lines})"),
            Self::Deleted(path, None) => format!("{path} (deleted)"),
            Self::CreatedDir(path, 1) => format!("{path}/ (1 new file)"),
            Self::CreatedDir(path, n) => format!("{path}/ ({n} new files)"),
            Self::DeletedDir(path, 1) => format!("{path}/ (1 file deleted)"),
            Self::DeletedDir(path, n) => format!("{path}/ ({n} files deleted)"),
        }
    }
}

/// The worktree as the session last counted it (#1149): held across its
/// calls, so a call reads only what changed.
#[derive(Debug)]
pub struct Tree {
    root: PathBuf,
    files: BTreeMap<String, File>,
    /// The bytes of line hashes held.
    kept: u64,
    /// When the running call's count began: [`Self::before`]'s.
    began: Option<SystemTime>,
    /// Whether the first count has run.
    counted: bool,
}

impl Tree {
    /// A tree over `root`, the worktree, counted at its first call.
    #[must_use]
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            files: BTreeMap::new(),
            kept: 0,
            began: None,
            counted: false,
        }
    }

    /// The worktree as it stands, just before a call runs: what changed since
    /// the last count (a background job's, the user's) is taken in, so the
    /// call is not charged with it.
    ///
    /// # Errors
    ///
    /// When the worktree cannot be listed.
    pub fn before(&mut self) -> Result<(), String> {
        let began = SystemTime::now();
        self.began = None;
        self.count(None)?;
        self.began = Some(began);
        Ok(())
    }

    /// What changed since [`Self::before`], in path order: a directory that
    /// was not there before, or is gone, is one entry with its count.
    ///
    /// # Errors
    ///
    /// When the worktree cannot be listed, or [`Self::before`] did not run.
    pub fn after(&mut self) -> Result<Vec<Entry>, String> {
        let began = self
            .began
            .take()
            .ok_or("the worktree was not counted before the call")?;
        let mut changes = Vec::new();
        self.count(Some((began, &mut changes)))?;
        Ok(grouped(changes, &self.files))
    }

    /// One count of the worktree: each file whose stat changed read again,
    /// and, when `charged`, each change pushed onto it.
    fn count(&mut self, mut charged: Option<(SystemTime, &mut Vec<Entry>)>) -> Result<(), String> {
        let listed = listed(&self.root)?;
        let first = !self.counted;
        self.counted = true;
        let now = SystemTime::now();
        // A path the listing no longer names: deleted, or ignored now.
        let left: Vec<String> = self
            .files
            .keys()
            .filter(|path| !listed.contains(*path))
            .cloned()
            .collect();
        for path in left {
            let gone = std::fs::symlink_metadata(self.root.join(&path)).is_err();
            let file = self.forget(&path);
            if let (true, Some((_, changes)), Some(file)) = (gone, charged.as_mut(), file) {
                changes.push(Entry::Deleted(path, file.held.lines()));
            }
        }
        for path in listed {
            let at = self.root.join(&path);
            let meta = std::fs::symlink_metadata(&at)
                .ok()
                .filter(|meta| meta.is_file() || meta.file_type().is_symlink());
            let Some(meta) = meta else {
                // Listed and not a file: a tracked file deleted, or a
                // submodule's directory.
                if let (Some(file), Some((_, changes))) = (self.forget(&path), charged.as_mut()) {
                    changes.push(Entry::Deleted(path, file.held.lines()));
                }
                continue;
            };
            let stat = Stat::of(&meta);
            let racy = meta.modified().map_or(true, |at| {
                now.duration_since(at).map_or(true, |ago| ago < RACY)
            });
            if let Some(file) = self.files.get(&path)
                && file.stat == stat
                && !file.racy
            {
                continue;
            }
            let old = self.forget(&path);
            let held = self.held(&at, &meta);
            let entry = match (&old, charged.as_ref()) {
                (Some(old), Some(_)) if !old.held.same(&held) => Some(Entry::Modified(
                    path.clone(),
                    match (&old.held, &held) {
                        (Held::Text(_, was), Held::Text(_, is)) => Some(line_counts(was, is)),
                        _ => None,
                    },
                )),
                // Joined the listing: created by the call, or there before
                // and listed now (an ignore rule changed), which is no change.
                (None, Some((began, _))) if !first && !before(&meta, *began) => {
                    Some(Entry::Created(path.clone(), held.lines()))
                }
                _ => None,
            };
            self.kept += held.kept();
            self.files.insert(path, File { stat, held, racy });
            if let (Some(entry), Some((_, changes))) = (entry, charged.as_mut()) {
                changes.push(entry);
            }
        }
        Ok(())
    }

    /// `path` dropped from the count, its line hashes released.
    fn forget(&mut self, path: &str) -> Option<File> {
        let file = self.files.remove(path)?;
        self.kept = self.kept.saturating_sub(file.held.kept());
        Some(file)
    }

    /// What `at` is held by: a symlink's target, never followed; a text
    /// file's lines while there is room; any other file's digest.
    fn held(&self, at: &Path, meta: &Metadata) -> Held {
        let by_stat = Held::Stat(meta.len(), (meta.mtime(), meta.mtime_nsec()));
        if meta.file_type().is_symlink() {
            return std::fs::read_link(at).map_or(by_stat, Held::Link);
        }
        if meta.len() > super::carry::SNAPSHOT_MAX_BYTES {
            return by_stat;
        }
        let Some(bytes) = read_same(at, meta) else {
            return by_stat;
        };
        let digest = crate::digest::sha256(&bytes);
        if bytes.len() as u64 > TEXT_MAX_BYTES || bytes.contains(&0) {
            return Held::Digest(digest);
        }
        let lines = line_hashes(&bytes);
        if self.kept + 8 * lines.len() as u64 > KEPT_MAX_BYTES {
            return Held::Digest(digest);
        }
        Held::Text(digest, lines)
    }
}

/// Whether `meta`'s inode last changed before `began`: a file that joined
/// the listing untouched by the call.
fn before(meta: &Metadata, began: SystemTime) -> bool {
    let ctime = Duration::new(
        u64::try_from(meta.ctime()).unwrap_or(0),
        u32::try_from(meta.ctime_nsec()).unwrap_or(0),
    );
    began
        .duration_since(SystemTime::UNIX_EPOCH)
        .is_ok_and(|began| ctime < began)
}

/// `at`'s bytes, when the file opened is the one `meta` stat'ed: a symlink
/// swapped in after the stat is not read through.
fn read_same(at: &Path, meta: &Metadata) -> Option<Vec<u8>> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(at).ok()?;
    let opened = file.metadata().ok()?;
    if opened.ino() != meta.ino() || opened.dev() != meta.dev() || !opened.is_file() {
        return None;
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    Some(bytes)
}

/// The worktree's files, relative to `root`: each repository's by its own
/// rules, every other file but those under a `.git` directory.
fn listed(root: &Path) -> Result<BTreeSet<String>, String> {
    match git_listed(root) {
        Some(listed) => Ok(listed),
        None => walked(root),
    }
}

/// The files git lists under `root`, tracked and untracked, its ignore
/// rules applied, as a free git read, and those of each repository nested
/// in it untracked, by their own; `None` when git cannot list them.
fn git_listed(root: &Path) -> Option<BTreeSet<String>> {
    let mut out = BTreeSet::new();
    for path in git_files(root)? {
        // An untracked nested repository is listed as its directory, with a
        // slash: listed by its own rules, or walked when it cannot be.
        match path.strip_suffix('/') {
            Some(nested) => {
                let files = git_listed(&root.join(nested))
                    .map_or_else(|| walked(&root.join(nested)).ok(), Some)
                    .unwrap_or_default();
                out.extend(files.into_iter().map(|file| format!("{nested}/{file}")));
            }
            None => {
                out.insert(path);
            }
        }
    }
    Some(out)
}

/// What `git ls-files` lists under `root`, as it lists it.
fn git_files(root: &Path) -> Option<Vec<String>> {
    let mut command = Command::new("git");
    command.env_clear();
    for key in ["PATH", "HOME"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let output = command
        .args(super::tool_loop::GIT_FREE_CONFIG)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
        ])
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .filter_map(|path| String::from_utf8(path.to_vec()).ok())
            .collect(),
    )
}

/// Every file under `root` but those under a `.git` directory, symlinks not
/// followed, a repository below `root` listed by its own rules; a directory
/// that cannot be read is passed over.
fn walked(root: &Path) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries: Vec<std::fs::DirEntry> = match std::fs::read_dir(&dir) {
            Ok(entries) => entries.flatten().collect(),
            Err(why) if dir == root => return Err(format!("{}: {why}", root.display())),
            Err(_) => continue,
        };
        let rel = |path: &Path| {
            path.strip_prefix(root)
                .ok()
                .and_then(Path::to_str)
                .map(str::to_owned)
        };
        if dir != root
            && entries.iter().any(|entry| entry.file_name() == ".git")
            && let (Some(files), Some(prefix)) = (git_listed(&dir), rel(&dir))
        {
            out.extend(files.into_iter().map(|file| format!("{prefix}/{file}")));
            continue;
        }
        for entry in entries {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if entry.file_name() != ".git" {
                    stack.push(entry.path());
                }
            } else if let Some(rel) = rel(&entry.path()) {
                out.insert(rel);
            }
        }
    }
    Ok(out)
}

/// `changes`, `files` the count after them: a created file under a
/// directory that was not there before (no file under it before) folded
/// into that directory's entry, and a deleted file under one that is gone
/// (no file under it now) into its; in path order. The directories are
/// gathered only for a call that created or deleted a file.
fn grouped(changes: Vec<Entry>, files: &BTreeMap<String, File>) -> Vec<Entry> {
    let paths = |created: bool| -> Vec<String> {
        changes
            .iter()
            .filter_map(|change| match change {
                Entry::Created(path, _) if created => Some(path.clone()),
                Entry::Deleted(path, _) if !created => Some(path.clone()),
                _ => None,
            })
            .collect()
    };
    let (created_paths, deleted_paths) = (paths(true), paths(false));
    let known_after = if deleted_paths.is_empty() {
        BTreeSet::new()
    } else {
        changed::dirs(files.keys())
    };
    let known_before = if created_paths.is_empty() {
        BTreeSet::new()
    } else {
        changed::dirs(
            files
                .keys()
                .filter(|path| !created_paths.contains(path))
                .chain(&deleted_paths),
        )
    };
    let mut entries = Vec::new();
    let mut created: BTreeMap<String, usize> = BTreeMap::new();
    let mut deleted: BTreeMap<String, usize> = BTreeMap::new();
    for change in changes {
        match &change {
            Entry::Created(path, _) => {
                if let Some(dir) = changed::new_dir(path, &known_before) {
                    *created.entry(dir.to_owned()).or_default() += 1;
                    continue;
                }
            }
            Entry::Deleted(path, _) => {
                if let Some(dir) = changed::new_dir(path, &known_after) {
                    *deleted.entry(dir.to_owned()).or_default() += 1;
                    continue;
                }
            }
            _ => {}
        }
        entries.push(change);
    }
    entries.extend(
        created
            .into_iter()
            .map(|(dir, n)| Entry::CreatedDir(dir, n)),
    );
    entries.extend(
        deleted
            .into_iter()
            .map(|(dir, n)| Entry::DeletedDir(dir, n)),
    );
    entries.sort_by(|a, b| a.path().cmp(b.path()));
    entries
}

/// A hash of each of `bytes`' lines, its newline included: a last line
/// without one is a line, and differs from the same line with one.
fn line_hashes(bytes: &[u8]) -> Vec<u64> {
    use std::hash::{Hash as _, Hasher as _};
    bytes
        .split_inclusive(|byte| *byte == b'\n')
        .map(|line| {
            let mut hasher = std::hash::DefaultHasher::new();
            line.hash(&mut hasher);
            hasher.finish()
        })
        .collect()
}

/// Lines added and removed from `old` to `new`, each a file's line hashes
/// ([`line_hashes`]): the shortest edit script's, on whole lines (Myers),
/// after their common head and tail. Past [`MAX_EDITS`], every line of the
/// changed middle is counted.
fn line_counts(old: &[u64], new: &[u64]) -> (u64, u64) {
    let head = old.iter().zip(new).take_while(|(a, b)| a == b).count();
    let (old, new) = (&old[head..], &new[head..]);
    let tail = old
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let (old, new) = (&old[..old.len() - tail], &new[..new.len() - tail]);
    let (n, m) = (old.len(), new.len());
    let count = |n: usize| n as u64;
    let Some(edits) = edit_distance(old, new) else {
        return (count(m), count(n));
    };
    let common = (n + m - edits) / 2;
    (count(m - common), count(n - common))
}

/// The length of the shortest edit script from `old` to `new` (Myers 1986,
/// the greedy forward search), or `None` past [`MAX_EDITS`].
fn edit_distance(old: &[u64], new: &[u64]) -> Option<usize> {
    let (olds, news) = (old.len(), new.len());
    let most = (olds + news).min(MAX_EDITS);
    // `furthest[centre + k]`: the furthest line of `old` reached on
    // diagonal k, where k is that line less the line of `new` reached.
    let centre = most + 1;
    let mut furthest = vec![0_usize; 2 * most + 3];
    for edits in 0..=most {
        // k runs -edits, -edits + 2, ..., edits.
        for step in 0..=edits {
            let at = centre + 2 * step - edits;
            let mut in_old = if step == 0 || (step != edits && furthest[at - 1] < furthest[at + 1])
            {
                furthest[at + 1]
            } else {
                furthest[at - 1] + 1
            };
            let mut in_new = (in_old + edits).checked_sub(2 * step)?;
            while in_old < olds && in_new < news && old[in_old] == new[in_new] {
                in_old += 1;
                in_new += 1;
            }
            furthest[at] = in_old;
            if in_old >= olds && in_new >= news {
                return Some(edits);
            }
        }
    }
    None
}

/// The block a call's result ends with (#1149): `entries`, the files it
/// changed, or that it changed none; then the files it changed or named whose
/// text the context does not show, `not_shown`, when there are any. Past
/// [`changed::MAX_NAMED`] names on a line, the rest are counted.
#[must_use]
pub fn block(entries: &[Entry], not_shown: &[String]) -> String {
    let named = |items: Vec<String>| {
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
    };
    let mut out = if entries.is_empty() {
        format!("{CHANGED} none.\n")
    } else {
        format!(
            "{CHANGED} {}.\n",
            named(entries.iter().map(Entry::text).collect())
        )
    };
    if !not_shown.is_empty() {
        let _ = writeln!(out, "{NOT_SHOWN} {}.", named(not_shown.to_vec()));
    }
    out
}

/// `shown`, a call's result after the output cap, with `block` after it, a
/// blank line between.
#[must_use]
pub fn appended(shown: &str, block: &str) -> String {
    if shown.is_empty() {
        return block.to_owned();
    }
    let gap = if shown.ends_with('\n') { "\n" } else { "\n\n" };
    format!("{shown}{gap}{block}")
}

/// A file as the shown rule and a call's changes name it (#1149, #1150):
/// relative against the worktree, `.` and `..` taken by name, and a path
/// under the worktree's canonical spelling taken back to its own.
#[derive(Debug, Clone)]
pub struct Keys {
    worktree: PathBuf,
    canonical: Option<PathBuf>,
}

impl Keys {
    /// The keys of `worktree`'s files.
    #[must_use]
    pub fn of(worktree: &Path) -> Self {
        let worktree = normalized(worktree);
        let canonical = std::fs::canonicalize(&worktree)
            .ok()
            .filter(|canonical| *canonical != worktree);
        Self {
            worktree,
            canonical,
        }
    }

    /// The key of the file `named`, as a call's arguments name it.
    #[must_use]
    pub fn key(&self, named: &str) -> PathBuf {
        let path = normalized(&self.worktree.join(named));
        match self
            .canonical
            .as_ref()
            .and_then(|canonical| path.strip_prefix(canonical).ok())
        {
            Some(rel) => self.worktree.join(rel),
            None => path,
        }
    }

    /// `key` as the block names it: relative to the worktree under it,
    /// whole otherwise.
    #[must_use]
    pub fn named(&self, key: &Path) -> String {
        key.strip_prefix(&self.worktree)
            .unwrap_or(key)
            .to_string_lossy()
            .into_owned()
    }
}

/// `path` with `.` dropped and `..` taking off the part before it, by name.
fn normalized(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "diet-call-changes-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch dir");
        dir
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .args(super::super::tool_loop::GIT_FREE_CONFIG)
            .args(args)
            .current_dir(dir)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("git runs");
        assert!(status.success(), "git {args:?}");
    }

    /// Lines added and removed from `old` to `new`, as a count compares them.
    fn counts(old: &[u8], new: &[u8]) -> (u64, u64) {
        line_counts(&line_hashes(old), &line_hashes(new))
    }

    /// A call that runs `call` in `tree`'s worktree, counted.
    fn counted(tree: &mut Tree, call: impl FnOnce()) -> Vec<Entry> {
        tree.before().expect("listed");
        call();
        tree.after().expect("listed")
    }

    #[test]
    fn lines_added_and_removed_are_the_shortest_edit_scripts() {
        assert_eq!(counts(b"a\nb\nc\n", b"a\nb\nc\n"), (0, 0));
        assert_eq!(counts(b"a\nb\nc\n", b"a\nB\nc\n"), (1, 1));
        assert_eq!(counts(b"a\nc\n", b"a\nb\nc\n"), (1, 0));
        assert_eq!(counts(b"a\nb\nc\n", b"c\n"), (0, 2));
        assert_eq!(counts(b"", b"x\ny\n"), (2, 0));
        // A last line losing its newline is a changed line, as git counts it.
        assert_eq!(counts(b"a\nb\n", b"a\nb"), (1, 1));
        // A move is a line removed and one added, not a rewrite.
        assert_eq!(counts(b"a\nb\nc\nd\n", b"b\nc\nd\na\n"), (1, 1));
        // Two changes apart: what trimming the common head and tail alone
        // (the edit tool's snippet) would count as a rewrite of all between.
        assert_eq!(counts(b"1\n2\n3\n4\n5\n6\n", b"1\nX\n3\n4\nY\n6\n"), (2, 2));
    }

    /// The counts agree with a longest common subsequence, found the slow
    /// way, over many small inputs.
    #[test]
    fn the_line_counts_agree_with_a_longest_common_subsequence() {
        let mut seed = 7_u64;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            usize::try_from(seed >> 33).expect("31 bits")
        };
        for _ in 0..2000 {
            let (n, m) = (next() % 10, next() % 10);
            let mut text = |len: usize| -> String {
                (0..len)
                    .map(|_| ["x\n", "y\n", "z\n"][next() % 3])
                    .collect()
            };
            let (a, b) = (text(n), text(m));
            let lines = |text: &str| -> Vec<String> {
                text.split_inclusive('\n').map(str::to_owned).collect()
            };
            let (al, bl) = (lines(&a), lines(&b));
            let mut lcs = vec![vec![0_usize; bl.len() + 1]; al.len() + 1];
            for i in 1..=al.len() {
                for j in 1..=bl.len() {
                    lcs[i][j] = if al[i - 1] == bl[j - 1] {
                        lcs[i - 1][j - 1] + 1
                    } else {
                        lcs[i - 1][j].max(lcs[i][j - 1])
                    };
                }
            }
            let common = lcs[al.len()][bl.len()];
            assert_eq!(
                counts(a.as_bytes(), b.as_bytes()),
                ((bl.len() - common) as u64, (al.len() - common) as u64),
                "{a:?} -> {b:?}"
            );
        }
    }

    #[test]
    fn past_the_edit_bound_the_whole_middle_is_counted() {
        let (mut old, mut new) = (String::new(), String::new());
        for n in 0..3000 {
            let _ = writeln!(old, "old {n}");
            let _ = writeln!(new, "new {n}");
        }
        assert_eq!(
            counts(old.as_bytes(), new.as_bytes()),
            (3000, 3000),
            "6000 edits, past the bound"
        );
    }

    #[test]
    fn a_change_in_place_a_new_file_and_a_deletion_are_counted_in_lines() {
        let dir = scratch("in-place");
        std::fs::write(dir.join("app.py"), "a = 1\nb = 2\n").expect("written");
        std::fs::write(dir.join("old.txt"), "x\ny\nz\n").expect("written");
        let mut tree = Tree::new(&dir);
        let entries = counted(&mut tree, || {
            std::fs::write(dir.join("app.py"), "a = 1\nb = 3\n").expect("written");
            std::fs::write(dir.join("new.md"), "one\n").expect("written");
            std::fs::remove_file(dir.join("old.txt")).expect("removed");
        });
        assert_eq!(
            entries,
            [
                Entry::Modified("app.py".to_owned(), Some((1, 1))),
                Entry::Created("new.md".to_owned(), Some(1)),
                Entry::Deleted("old.txt".to_owned(), Some(3)),
            ]
        );
        assert_eq!(
            block(&entries, &[]),
            "Files this call changed: app.py (+1 -1), new.md (new, +1), old.txt (deleted, -3).\n"
        );
        // Nothing done: nothing changed.
        assert_eq!(counted(&mut tree, || {}), []);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The same bytes written again, a touch: no change.
    #[test]
    fn a_rewrite_of_the_same_bytes_is_no_change() {
        let dir = scratch("same-bytes");
        std::fs::write(dir.join("app.py"), "a = 1\n").expect("written");
        let mut tree = Tree::new(&dir);
        let entries = counted(&mut tree, || {
            std::fs::write(dir.join("app.py"), "a = 1\n").expect("written");
        });
        assert_eq!(entries, []);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// What changed between calls is taken in before the next one, never
    /// charged to it.
    #[test]
    fn a_change_between_calls_is_not_the_next_calls() {
        let dir = scratch("between");
        std::fs::write(dir.join("app.py"), "a = 1\n").expect("written");
        let mut tree = Tree::new(&dir);
        assert_eq!(counted(&mut tree, || {}), []);
        std::fs::write(dir.join("app.py"), "a = 2\n").expect("written");
        std::fs::write(dir.join("job.log"), "ran\n").expect("written");
        assert_eq!(counted(&mut tree, || {}), []);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// In a git repository its ignore rules hold: an ignored file is left
    /// out, and one that becomes ignored, or listed again, is no change.
    #[test]
    fn in_a_repository_an_ignored_file_is_left_out() {
        let dir = scratch("ignored");
        git(&dir, &["init", "-q"]);
        std::fs::write(dir.join(".gitignore"), "build/\n*.log\n").expect("written");
        std::fs::write(dir.join("app.py"), "a = 1\n").expect("written");
        std::fs::write(dir.join("kept.txt"), "kept\n").expect("written");
        std::fs::write(dir.join("old.log"), "old\n").expect("written");
        let mut tree = Tree::new(&dir);
        let entries = counted(&mut tree, || {
            std::fs::create_dir_all(dir.join("build")).expect("made");
            std::fs::write(dir.join("build/out.o"), "obj\n").expect("written");
            std::fs::write(dir.join("run.log"), "log\n").expect("written");
            std::fs::write(dir.join("app.py"), "a = 2\n").expect("written");
        });
        assert_eq!(
            entries,
            [Entry::Modified("app.py".to_owned(), Some((1, 1)))]
        );
        // `kept.txt` ignored now, and `*.log` listed again: only the rule
        // file changed.
        let entries = counted(&mut tree, || {
            std::fs::write(dir.join(".gitignore"), "build/\nkept.txt\n").expect("written");
        });
        assert_eq!(
            entries,
            [Entry::Modified(".gitignore".to_owned(), Some((1, 1)))]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A directory of checkouts with no repository at its top: each
    /// repository's files by its own rules, and an untracked repository
    /// nested in one by its own; a file in none counts.
    #[test]
    fn each_repository_is_listed_by_its_own_rules() {
        let dir = scratch("checkouts");
        for repo in ["one", "one/nested"] {
            std::fs::create_dir_all(dir.join(repo)).expect("made");
            git(&dir.join(repo), &["init", "-q"]);
        }
        std::fs::write(
            dir.join("one/.gitignore"),
            "node_modules/\nnested-ignored.txt\n",
        )
        .expect("written");
        std::fs::write(dir.join("one/nested/.gitignore"), "*.log\n").expect("written");
        let mut tree = Tree::new(&dir);
        let entries = counted(&mut tree, || {
            std::fs::create_dir_all(dir.join("one/node_modules/pkg")).expect("made");
            for file in [
                "one/node_modules/pkg/index.js",
                "one/app.py",
                "one/nested/run.log",
                "one/nested/lib.py",
                "loose.txt",
            ] {
                std::fs::write(dir.join(file), "x\n").expect("written");
            }
        });
        assert_eq!(
            entries,
            [
                Entry::Created("loose.txt".to_owned(), Some(1)),
                Entry::Created("one/app.py".to_owned(), Some(1)),
                Entry::Created("one/nested/lib.py".to_owned(), Some(1)),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// With no repository, every file but `.git`'s counts.
    #[test]
    fn with_no_repository_every_file_counts() {
        let dir = scratch("no-repository");
        let mut tree = Tree::new(&dir);
        let entries = counted(&mut tree, || {
            std::fs::create_dir_all(dir.join("node_modules/pkg")).expect("made");
            std::fs::write(dir.join("node_modules/pkg/index.js"), "x\n").expect("written");
            std::fs::write(dir.join("node_modules/pkg/package.json"), "{}\n").expect("written");
            std::fs::write(dir.join("a.txt"), "a\n").expect("written");
        });
        assert_eq!(
            entries,
            [
                Entry::Created("a.txt".to_owned(), Some(1)),
                Entry::CreatedDir("node_modules".to_owned(), 2),
            ]
        );
        assert_eq!(
            block(&entries, &["a.txt".to_owned()]),
            "Files this call changed: a.txt (new, +1), node_modules/ (2 new files).\n\
             Not shown in this context by read, write or edit: a.txt.\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A symlink is held by its target, never read through; a binary file by
    /// its digest; neither has lines counted.
    #[test]
    fn a_symlink_and_a_binary_file_are_named_without_lines() {
        let dir = scratch("binary");
        let outside = scratch("binary-outside");
        std::fs::write(outside.join("secret"), "s\n").expect("written");
        std::fs::write(dir.join("blob.bin"), b"\x00\x01").expect("written");
        let mut tree = Tree::new(&dir);
        let entries = counted(&mut tree, || {
            std::fs::write(dir.join("blob.bin"), b"\x00\x02").expect("written");
            std::os::unix::fs::symlink(outside.join("secret"), dir.join("link")).expect("linked");
        });
        assert_eq!(
            entries,
            [
                Entry::Modified("blob.bin".to_owned(), None),
                Entry::Created("link".to_owned(), None),
            ]
        );
        assert_eq!(
            block(&entries, &[]),
            "Files this call changed: blob.bin (lines not counted), link (new, lines not \
             counted).\n"
        );
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn the_block_names_none_and_counts_past_twenty() {
        assert_eq!(block(&[], &[]), "Files this call changed: none.\n");
        assert_eq!(
            block(&[], &["app.py".to_owned()]),
            "Files this call changed: none.\nNot shown in this context by read, write or edit: \
             app.py.\n"
        );
        let many: Vec<Entry> = (0..22)
            .map(|n| Entry::Modified(format!("f{n:02}"), Some((1, 0))))
            .collect();
        assert!(block(&many, &[]).ends_with("f19 (+1 -0), and 2 more.\n"));
    }

    #[test]
    fn the_block_follows_the_result_after_a_blank_line() {
        assert_eq!(appended("", "B\n"), "B\n");
        assert_eq!(appended("out\n", "B\n"), "out\n\nB\n");
        assert_eq!(appended("Updated.", "B\n"), "Updated.\n\nB\n");
    }

    #[test]
    fn a_file_is_keyed_by_name_whatever_the_spelling() {
        let dir = scratch("keys");
        let keys = Keys::of(&dir);
        assert_eq!(keys.key("src/app.py"), keys.key("./src/../src/app.py"));
        let canonical = std::fs::canonicalize(&dir).expect("canonical");
        assert_eq!(
            keys.key(&canonical.join("src/app.py").to_string_lossy()),
            keys.key("src/app.py")
        );
        assert_eq!(keys.named(&keys.key("src/app.py")), "src/app.py");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_switch_is_off_by_default_and_refuses_another_word() {
        let read = |text: &str| call_changes(&regimen::parse(text).expect("a regimen"));
        assert_eq!(read(""), Ok(false));
        assert_eq!(read("call_changes = \"off\"\n"), Ok(false));
        assert_eq!(read("call_changes = \"on\"\n"), Ok(true));
        assert!(read("call_changes = \"yes\"\n").is_err());
        assert!(read("call_changes = true\n").is_err());
    }
}
