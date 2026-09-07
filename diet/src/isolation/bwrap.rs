//! The Linux sandbox: namespaces, through `bwrap`.
//!
//! **Namespaces, and NOT seccomp.** The ruling says "namespaces plus seccomp
//! or an equivalent"; this composes no `--seccomp` filter, and inside the
//! sandbox `/proc/self/status` reports `Seccomp: 0`. The file used to say
//! seccomp in its own title while composing none, which is a claim about
//! confinement that nothing backed. Namespaces alone are a defensible floor
//! for accident containment -- what this module is for -- and a syscall
//! filter is what the "or an equivalent" clause would have to be argued
//! about separately, against a threat model this module explicitly does not
//! have.
//!
//! The confinement is composed rather than implemented. `bubblewrap` is the
//! unprivileged user-namespace sandbox, it is packaged everywhere, and it is
//! the thing whose correctness has been looked at by more people than will
//! ever look at this file. A hand-rolled `unshare` plus `pivot_root` pipeline
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
        push(&["--proc", "/proc", "--dev", "/dev", "--tmpfs", "/dev/shm"]);

        let tree = worktree.to_string_lossy().into_owned();
        push(&["--bind", &tree, &tree]);
        // ONE REMOUNT PER MOUNT. `--remount-ro` does not recurse -- bwrap's own
        // help says so -- and `--dev` creates a separate tmpfs, with
        // `/dev/shm` a further one inside it. So a remount of `/` alone left
        // both writable while the advisory told the model the working tree was
        // the only writable path. A command wrote to `/dev/shm`, was told it
        // had, and the file evaporated with the sandbox: the outcome this
        // module names as worse than a denial, produced by the module.
        push(&["--remount-ro", "/"]);
        push(&["--remount-ro", "/dev"]);
        push(&["--remount-ro", "/dev/shm"]);
        push(&["--chdir", &tree]);
        push(&["--"]);

        out.extend(argv.iter().cloned());
        out
    }

    /// Whether `stderr` is the runner failing to build the sandbox rather
    /// than the command failing.
    ///
    /// The runner names itself on its own first line and exits `1` without
    /// ever executing the command.
    ///
    /// **`bwrap: execvp ` is excluded, and that exclusion is the whole
    /// subtlety.** The runner uses the same prefix and the same exit status
    /// for a command it could not execute -- which happens after the sandbox
    /// was built perfectly well, and happens *often*: the default policy binds
    /// `/usr` and `/etc`, so anything in a home directory, `/opt` or
    /// `/usr/local` outside those is simply not there. Reading that as a setup
    /// failure threw away the exit status, the output and the real diagnosis,
    /// and handed the caller "the sandbox runner did not start" about a
    /// sandbox that started. It is the mirror of the fabrication this type
    /// exists to prevent, and just as fabricated.
    ///
    /// The remaining limitation, stated because a discriminator whose failure
    /// mode is unwritten is one somebody will trust further than it goes: a
    /// command whose own first line of standard error begins `bwrap: ` and
    /// which exits `1` is still mistaken for a setup failure.
    #[must_use]
    pub fn setup_failure(&self, stderr: &str, exit: Option<i32>) -> Option<String> {
        let first = stderr.lines().next().unwrap_or_default();
        let is_runner = first.starts_with("bwrap: ") && !first.starts_with("bwrap: execvp ");
        (exit == Some(1) && is_runner).then(|| first.to_owned())
    }
}
