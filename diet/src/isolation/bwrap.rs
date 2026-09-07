//! The Linux sandbox: namespaces and seccomp, through `bwrap`.
//!
//! The confinement is composed rather than implemented. `bubblewrap` is the
//! unprivileged user-namespace sandbox the ruling's "namespaces plus seccomp
//! or an equivalent" describes, it is packaged everywhere, and it is the
//! thing whose correctness has been looked at by more people than will ever
//! look at this file. A hand-rolled `unshare` plus `pivot_root` pipeline
//! composed out of shell would be the same sandbox with a much larger surface
//! for being subtly wrong -- and a sandbox that is subtly wrong is worse than
//! none, because it is trusted.
//!
//! What this module owns is the **composition**: which paths are bound, in
//! which order, with the root remounted read-only at the end. That is a pure
//! function and it is asserted as one, so it can be checked on a host that
//! has no runner at all.

use std::path::{Path, PathBuf};

use super::Unavailable;
use super::policy::{Isolation, Network, Policy};

/// The program this module composes for.
pub const RUNNER: &str = "bwrap";

/// A found runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bubblewrap {
    runner: PathBuf,
}

impl Bubblewrap {
    /// Find the runner on `PATH`.
    ///
    /// # Errors
    ///
    /// Returns [`Unavailable::NoRunner`] if it is not there. Never a fallback.
    pub fn found() -> Result<Self, Unavailable> {
        let Some(path) = std::env::var_os("PATH") else {
            return Err(Unavailable::NoRunner {
                mechanism: Isolation::Sandbox,
                looked_for: RUNNER.to_owned(),
            });
        };
        std::env::split_paths(&path)
            .map(|dir| dir.join(RUNNER))
            .find(|candidate| candidate.is_file())
            .map(|runner| Self { runner })
            .ok_or_else(|| Unavailable::NoRunner {
                mechanism: Isolation::Sandbox,
                looked_for: RUNNER.to_owned(),
            })
    }

    /// A runner at a named path, for a caller that has one.
    #[must_use]
    pub fn at(runner: PathBuf) -> Self {
        Self { runner }
    }

    /// Where the runner is.
    #[must_use]
    pub fn runner(&self) -> &Path {
        &self.runner
    }

    /// The argv that runs `argv` under `policy`.
    ///
    /// Order is load-bearing and is the reason this is one function rather
    /// than a builder a caller assembles: `--remount-ro /` has to come after
    /// every bind, or it remounts a root the binds then reopen. And the
    /// working tree is bound AFTER the read-only paths, so a tree that lives
    /// inside one of them is still writable.
    ///
    /// `/tmp` is not bound and neither is the home directory. Under
    /// `bubblewrap` the sandbox's root is a fresh writable tmpfs, so a policy
    /// that stopped here would let `echo x > /tmp/outside` SUCCEED -- into a
    /// throwaway the caller never sees again. That is worse than a denial:
    /// the model is told it wrote a file, and the file evaporates. The
    /// read-only remount is what turns it back into a refusal, and it was
    /// added because the write was watched succeeding without it.
    #[must_use]
    pub fn compose(&self, policy: &Policy, worktree: &Path, argv: &[String]) -> Vec<String> {
        let mut out = vec![self.runner.to_string_lossy().into_owned()];
        let mut push = |args: &[&str]| out.extend(args.iter().map(|arg| (*arg).to_owned()));

        push(&["--unshare-all"]);
        if policy.network == Network::Host {
            push(&["--share-net"]);
        }
        // `--die-with-parent` so a command outliving the drive is not a thing
        // that can happen; `--new-session` so it cannot push characters back
        // into the operator's terminal.
        push(&["--die-with-parent", "--new-session"]);

        for path in &policy.readable {
            push(&["--ro-bind", path, path]);
        }
        for (name, target) in &policy.symlinks {
            push(&["--symlink", target, &format!("/{name}")]);
        }
        push(&["--proc", "/proc", "--dev", "/dev"]);

        let tree = worktree.to_string_lossy().into_owned();
        push(&["--bind", &tree, &tree]);
        push(&["--remount-ro", "/"]);
        push(&["--chdir", &tree]);
        push(&["--"]);

        out.extend(argv.iter().cloned());
        out
    }

    /// Whether `stderr` is the runner failing to build the sandbox rather
    /// than the command failing.
    ///
    /// The runner names itself on its own first line and exits `1` without
    /// ever executing the command. The discriminator is therefore the first
    /// line's prefix, and its limitation is that a command whose own first
    /// line of standard error begins `bwrap: ` would be mistaken for one --
    /// stated because a discriminator whose failure mode is unwritten is one
    /// somebody will trust further than it goes.
    #[must_use]
    pub fn setup_failure(&self, stderr: &str, exit: Option<i32>) -> Option<String> {
        let first = stderr.lines().next().unwrap_or_default();
        (exit == Some(1) && first.starts_with("bwrap: ")).then(|| first.to_owned())
    }
}
