//! Instruction files (#559): the `AGENTS.md` a worktree and its parents
//! carry, found and put into the system prompt, as the harnesses we compare
//! against do (`docs/harness-baseline.md`, Instructions).
//!
//! Where Pi and `OpenCode` 2 agree, this follows them; where they differ,
//! Qwen Code decides. Each source is read at the baseline's commits, Pi
//! `42a3497d`, `OpenCode` `055d95bb`, Qwen Code `c0c697c8`:
//!
//! * **Which files: `AGENTS.md` only.** Both Pi and `OpenCode` 2 read
//!   `AGENTS.md`, and neither reads `QWEN.md`. Pi also takes
//!   `AGENTS.override.md`, `AGENTS.MD`, `CLAUDE.md` and `CLAUDE.MD`, the first
//!   found per directory (`pi:packages/coding-agent/src/core/resource-loader.ts:185`).
//!   `OpenCode` 2 takes `AGENTS.md` alone
//!   (`oc:packages/core/src/instruction-context.ts:51`). Qwen Code, which
//!   decides the fallbacks, reads `QWEN.md` and `AGENTS.md` and no `CLAUDE.md`
//!   (`qc:packages/core/src/utils/memory-constants.ts:34-36`); `QWEN.md` is
//!   left out because Pi and `OpenCode` 2 agree on not reading it.
//! * **Where the walk stops: the git root.** Pi walks to the filesystem
//!   root (`resource-loader.ts:253-268`). `OpenCode` 2 stops at the project
//!   directory (`instruction-context.ts:40-58`, `oc:packages/core/src/fs-util.ts:168-181`). Qwen
//!   Code stops at the nearest ancestor holding `.git`, as a directory or a
//!   file (`qc:…/utils/projectRoot.ts:29-37`, `memory/memoryDiscovery.ts:168-193`).
//! * **Order: outermost first.** Pi puts the outermost file first
//!   (`unshift`, `resource-loader.ts:262`). `OpenCode` 2 puts the nearest
//!   first (`fs-util.ts:174`). Qwen Code puts the outermost first
//!   (`unshift`, `memoryDiscovery.ts:187`).
//! * **Wrapper:** Qwen Code's `--- Context from: <path> ---` …
//!   `--- End of Context from: <path> ---`, the content trimmed, the path
//!   relative to the working directory, blocks joined by a blank line
//!   (`memoryDiscovery.ts:405-407`). Pi wraps each file in
//!   `<project_instructions path=…>` (`pi:…/core/system-prompt.ts:79-86`);
//!   `OpenCode` 2 prefixes `Instructions from: <path>`
//!   (`instruction-context.ts:99-100`).
//! * **Placement:** at the end of the system prompt, after `\n\n---\n\n`
//!   (`qc:…/core/prompts.ts:840-843,884-888`). All three put them in the
//!   system prompt.
//!
//! **Never a global or user file** (#559's constraint), whatever a harness
//! does: no `~/.pi/agent`, `~/.config/opencode` or `~/.qwen` file, and the
//! walk never reaches the home directory or above it. Where no git root is
//! found, only the worktree's own directory is read. Qwen Code would walk
//! to the home directory's parent there (`memoryDiscovery.ts:173`), and that
//! is a user's directory.

use std::path::{Path, PathBuf};

/// The regimen key for the lever: `"off"` turns discovery off; anything
/// else, or nothing, leaves it on (read leniently).
pub const INSTRUCTION_FILES: &str = "instruction_files";

/// The one file name read.
pub const FILENAME: &str = "AGENTS.md";

/// One instruction file found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its path relative to the worktree: `AGENTS.md`, or `../AGENTS.md`
    /// and so on for a parent's. Never absolute, never under a home.
    pub path: String,
    /// Its text, as read.
    pub content: String,
    /// The sha256 of its bytes.
    pub sha256: String,
}

/// Whether the regimen leaves the lever on: anything but `"off"`.
#[must_use]
pub fn enabled(regimen: &crate::formats::regimen::Regimen) -> bool {
    !matches!(
        regimen.get(INSTRUCTION_FILES),
        Some(crate::formats::regimen::Value::String(state)) if state == "off"
    )
}

/// The `AGENTS.md` files from `worktree` up to its git root, outermost
/// first, each read once; only `worktree`'s own when no git root is found.
/// The walk stops below `home` and never reads it or above it.
#[must_use]
pub fn discover(worktree: &Path, home: Option<&Path>) -> Vec<Found> {
    let start = worktree
        .canonicalize()
        .unwrap_or_else(|_| worktree.to_path_buf());
    let home = home.map(|home| home.canonicalize().unwrap_or_else(|_| home.to_path_buf()));
    let under_home = |dir: &Path| home.as_ref().is_some_and(|home| home.starts_with(dir));
    let root = git_root(&start);
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut current = start.clone();
    loop {
        if under_home(&current) {
            break;
        }
        dirs.push(current.clone());
        let at_root = root.as_ref().is_none_or(|root| *root == current);
        if at_root {
            break;
        }
        let Some(parent) = current.parent() else {
            break;
        };
        current = parent.to_path_buf();
    }
    // Outermost first.
    dirs.iter()
        .enumerate()
        .rev()
        .filter_map(|(up, dir)| {
            let path = dir.join(FILENAME);
            if !path.is_file() {
                return None;
            }
            let bytes = std::fs::read(&path).ok()?;
            Some(Found {
                path: format!("{}{FILENAME}", "../".repeat(up)),
                sha256: crate::digest::sha256_hex(&bytes),
                content: String::from_utf8_lossy(&bytes).into_owned(),
            })
        })
        .collect()
}

/// The nearest ancestor of `start`, itself included, holding `.git` as a
/// directory or a file (a worktree or a submodule).
fn git_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find(|dir| {
            std::fs::symlink_metadata(dir.join(".git"))
                .is_ok_and(|meta| meta.is_dir() || meta.is_file())
        })
        .map(Path::to_path_buf)
}

/// The files as Qwen Code wraps them, joined by a blank line; a file whose
/// trimmed text is empty is left out, as Qwen Code leaves it.
#[must_use]
pub fn render(files: &[Found]) -> String {
    files
        .iter()
        .filter(|file| !file.content.trim().is_empty())
        .map(|file| {
            format!(
                "--- Context from: {path} ---\n{}\n--- End of Context from: {path} ---",
                file.content.trim(),
                path = file.path
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// `head` with the files' render appended, as Qwen Code appends its context
/// layer: `\n\n---\n\n` and the render, or `head` unchanged when nothing
/// renders.
#[must_use]
pub fn into_system(head: &str, files: &[Found]) -> String {
    let rendered = render(files);
    if rendered.trim().is_empty() {
        head.to_owned()
    } else {
        format!("{head}\n\n---\n\n{}", rendered.trim())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("diet-instructions-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir.canonicalize().expect("canonical")
    }

    #[test]
    fn the_walk_reads_agents_md_from_the_worktree_to_its_git_root_outermost_first() {
        let top = scratch("walk");
        let repo = top.join("repo");
        let tree = repo.join("pkg").join("app");
        std::fs::create_dir_all(&tree).expect("dirs");
        std::fs::create_dir_all(repo.join(".git")).expect("a git root");
        // Above the git root: never read.
        std::fs::write(top.join(FILENAME), "outside").expect("write");
        std::fs::write(repo.join(FILENAME), "  root rules\n").expect("write");
        std::fs::write(tree.join(FILENAME), "app rules").expect("write");
        // Other names are not read.
        std::fs::write(tree.join("CLAUDE.md"), "no").expect("write");
        std::fs::write(tree.join("QWEN.md"), "no").expect("write");
        let found = discover(&tree, None);
        assert_eq!(
            found
                .iter()
                .map(|file| (file.path.as_str(), file.content.as_str()))
                .collect::<Vec<_>>(),
            [
                ("../../AGENTS.md", "  root rules\n"),
                ("AGENTS.md", "app rules")
            ]
        );
        assert_eq!(found[1].sha256, crate::digest::sha256_hex(b"app rules"));
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn a_git_file_marks_the_root_and_no_git_root_reads_only_the_worktree() {
        let top = scratch("roots");
        let tree = top.join("a").join("b");
        std::fs::create_dir_all(&tree).expect("dirs");
        std::fs::write(top.join(FILENAME), "top").expect("write");
        std::fs::write(top.join("a").join(FILENAME), "a").expect("write");
        // No git root anywhere below `top`: the worktree's own only (none).
        assert_eq!(discover(&tree, None), Vec::new());
        // A `.git` file (a linked worktree) marks `a` as the root.
        std::fs::write(top.join("a").join(".git"), "gitdir: elsewhere").expect("write");
        let found = discover(&tree, None);
        assert_eq!(
            found
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["../AGENTS.md"]
        );
        let _ = std::fs::remove_dir_all(&top);
    }

    #[test]
    fn the_walk_never_reaches_the_home_directory() {
        let home = scratch("home");
        let tree = home.join("project");
        std::fs::create_dir_all(&tree).expect("dirs");
        std::fs::create_dir_all(home.join(".git")).expect("home is a repository");
        std::fs::write(home.join(FILENAME), "a user's file").expect("write");
        std::fs::write(tree.join(FILENAME), "project").expect("write");
        let found = discover(&tree, Some(&home));
        assert_eq!(
            found
                .iter()
                .map(|file| file.path.as_str())
                .collect::<Vec<_>>(),
            ["AGENTS.md"]
        );
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_files_are_appended_as_qwen_code_wraps_them() {
        let files = [
            Found {
                path: "../AGENTS.md".to_owned(),
                content: "\nroot\n".to_owned(),
                sha256: String::new(),
            },
            Found {
                path: "AGENTS.md".to_owned(),
                content: "   ".to_owned(),
                sha256: String::new(),
            },
            Found {
                path: "AGENTS.md".to_owned(),
                content: "app".to_owned(),
                sha256: String::new(),
            },
        ];
        assert_eq!(
            into_system("you are the trunk", &files),
            "you are the trunk\n\n---\n\n\
             --- Context from: ../AGENTS.md ---\nroot\n--- End of Context from: ../AGENTS.md ---\n\n\
             --- Context from: AGENTS.md ---\napp\n--- End of Context from: AGENTS.md ---"
        );
        assert_eq!(into_system("head", &[]), "head");
    }

    #[test]
    fn the_lever_is_off_only_when_it_says_off() {
        let read = |text: &str| enabled(&crate::formats::regimen::parse(text).expect("a regimen"));
        assert!(read("arm = \"a\"\n"));
        assert!(read("instruction_files = \"on\"\n"));
        assert!(read("instruction_files = \"maybe\"\n"));
        assert!(!read("instruction_files = \"off\"\n"));
    }
}
