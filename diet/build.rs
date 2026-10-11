//! The build the binaries were made from (#757): the commit, and whether its
//! tracked files differed, so a session's log names its build rather than a
//! run's README typing it from memory.
//!
//! Read with `git` at build time. Where there is no repository -- a source
//! tarball -- both are empty and the log names no build.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn main() {
    let commit = git(&["rev-parse", "HEAD"]).unwrap_or_default();
    // Tracked files only: an untracked scratch file is not the build.
    let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).map_or("", |status| {
        if status.is_empty() { "false" } else { "true" }
    });
    println!("cargo:rustc-env=DIET_BUILD_COMMIT={commit}");
    println!("cargo:rustc-env=DIET_BUILD_DIRTY={dirty}");
    // Read again when the commit moves or a source file changes.
    for path in ["HEAD", "index"] {
        if let Some(path) = git(&["rev-parse", "--git-path", path]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(head) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(path) = git(&["rev-parse", "--git-path", &head])
    {
        println!("cargo:rerun-if-changed={path}");
    }
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=Cargo.toml");
}
