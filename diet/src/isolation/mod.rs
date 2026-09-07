//! `diet::isolation` -- what a session's commands may touch, and what happens
//! when they reach past it.
//!
//! The reference harness runs agent-issued commands. Two facts from prior
//! work decide the design: a session's commands must not be able to touch the
//! operator's environment beyond the working tree -- an archived drive
//! includes an agent `rm`-ing outside its directory once, caught by a guard
//! that should not have been the last line -- and the mechanism has to be
//! cheap enough to use **by default**, or it becomes the guard nobody
//! enables.
//!
//! So a lightweight OS-native sandbox is the default and a VM is a
//! declaration. A regimen that says nothing about isolation gets the sandbox;
//! the way to run unconfined is to write `isolation = "none"`.
//!
//! # This is accident containment, not adversarial security
//!
//! It is built against a session that goes somewhere it did not mean to. It
//! is **not** built against a model trying to escape, and nothing here should
//! be read as a claim that it would hold against one.
//!
//! # No silent fallback
//!
//! A declared mechanism that is unavailable **refuses to start**. Not because
//! failing loudly is a virtue in itself, but because the alternative is a
//! drive that reports `isolation = "vm"` in its record while running under
//! something weaker, and every result banked from it is mislabelled.
//! [`Unavailable::EXIT`] is the code that refusal carries.
//!
//! # What is deferred, and says so
//!
//! **The macOS half is not built.** The ruling names the platform's
//! per-process sandbox profile mechanism there; this seat has no Mac to see
//! it deny anything on, and a sandbox nobody has watched refuse is worse than
//! none because it is trusted. [`Platform::MacOs`] therefore refuses to start
//! with [`Unavailable::NotImplemented`], and a test asserts it -- so the
//! deferral is a fact of the tree rather than a comment.
//!
//! # What has been seen red
//!
//! Twelve mutations, each applied to the source and run. Recorded here rather
//! than as seeded cases in the gate because `verify.sh` and
//! `tools/gate/faults.toml` are not this seat's files; that is disclosed on
//! the pull request.
//!
//! | break this | and this fails |
//! | --- | --- |
//! | a declared `vm` falls back to the sandbox | `a_declared_vm_refuses_to_start_rather_than_falling_back_to_the_sandbox` |
//! | macOS quietly uses the Linux sandbox | `the_macos_half_is_not_built_and_says_so_from_here` |
//! | a regimen that says nothing runs unconfined | `a_regimen_that_says_nothing_about_isolation_gets_the_sandbox` |
//! | the root is never remounted read-only | `the_seeded_escapes_…` (+2) |
//! | the remount comes before the binds | the same (+2) |
//! | the network is always shared | `the_network_is_shared_only_where_the_regimen_declares_it` (+2) |
//! | every line of standard error is a denial | `every_denial_line_is_kept_and_none_is_invented` (+2) |
//! | `absent_or_outside` reads as unambiguous | `an_ambiguous_denial_is_named_as_one` |
//! | a runner's setup failure is read as the command's exit | `a_runner_that_could_not_build_the_sandbox_is_not_a_command_result` |
//! | the model is shown a paraphrase instead of the command's words | `the_model_is_shown_the_commands_own_words_and_nothing_instead_of_them` |
//! | a mechanism outside the vocabulary reads as the default | `a_policy_is_read_from_the_regimen_or_refused_with_a_reason` |
//! | a relative path is accepted in a sandbox policy | the same |
//!
//! Two of those touch the **real** mechanism rather than the composition, and
//! they are the ones worth naming. With no `--remount-ro /` at all, `echo x >
//! /tmp/outside` exits **0**: bubblewrap's root is a fresh writable tmpfs, so
//! the write succeeds into a throwaway the caller never sees again. That is
//! worse than a denial -- the model is told it wrote a file, and the file
//! evaporates -- and it is why the remount is there. With the remount moved
//! *before* the binds, the sandbox fails to set up instead, and
//! [`SetupFailed`] is what comes back rather than a command result.

pub mod bwrap;
pub mod policy;

use std::error::Error;
use std::fmt::{self, Write as _};
use std::path::Path;
use std::process::Command;

use crate::client::vocabulary;

use bwrap::Bubblewrap;
pub use policy::{Isolation, Network, Policy, PolicyError};

vocabulary! {
    /// The platform a drive is being confined on.
    ///
    /// A value rather than a `#[cfg]`, so that the macOS refusal can be
    /// SEEN refusing from a Linux test. A deferral that compiles away on the
    /// platform where the tests run is a deferral nobody can check.
    Platform {
        /// Namespaces plus seccomp, through a runner.
        Linux => "linux",
        /// The platform's per-process sandbox profile mechanism. Not built.
        MacOs => "macos",
        /// Anything else.
        Other => "other",
    }
}

impl Platform {
    /// The platform this build is for.
    #[must_use]
    pub fn here() -> Self {
        if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Other
        }
    }
}

/// Why a declared mechanism cannot be used here.
///
/// Never a reason to use a weaker one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unavailable {
    /// The mechanism exists in this library and its runner is not on this
    /// host.
    NoRunner {
        /// What was declared.
        mechanism: Isolation,
        /// The program that was looked for.
        looked_for: String,
    },
    /// The mechanism is not built for this platform.
    NotImplemented {
        /// What was declared.
        mechanism: Isolation,
        /// Where.
        on: Platform,
    },
}

impl Unavailable {
    /// The exit code a drive refusing to start carries.
    ///
    /// Two, not one: a drive that could not start is not a drive that failed,
    /// and a census that could not tell them apart would count a missing
    /// runner as a failed run.
    pub const EXIT: i32 = 2;
}

impl fmt::Display for Unavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRunner {
                mechanism,
                looked_for,
            } => write!(
                f,
                "the regimen declares `isolation = \"{}\"` and `{looked_for}` is not on \
                 this host; refusing to start rather than running under something weaker \
                 than the record would say",
                mechanism.tag()
            ),
            Self::NotImplemented { mechanism, on } => write!(
                f,
                "`isolation = \"{}\"` is not built for {}; refusing to start rather than \
                 running under something weaker than the record would say",
                mechanism.tag(),
                on.tag()
            ),
        }
    }
}

impl Error for Unavailable {}

/// A confinement that is ready to run commands.
#[derive(Debug, Clone)]
pub enum Confinement {
    /// Declared unconfined.
    Unconfined,
    /// The Linux sandbox, through a runner that was found.
    Sandbox(Bubblewrap),
}

/// Open the confinement `policy` declares, or refuse.
///
/// # Errors
///
/// Returns [`Unavailable`] where the declared mechanism has no runner here,
/// or is not built for this platform. Never a weaker mechanism.
pub fn open(policy: &Policy) -> Result<Confinement, Unavailable> {
    open_on(policy, Platform::here())
}

/// The same, for a platform named rather than compiled in.
///
/// # Errors
///
/// As [`open`].
pub fn open_on(policy: &Policy, platform: Platform) -> Result<Confinement, Unavailable> {
    match policy.isolation {
        Isolation::None => Ok(Confinement::Unconfined),
        Isolation::Vm => Err(Unavailable::NotImplemented {
            mechanism: Isolation::Vm,
            on: platform,
        }),
        Isolation::Sandbox => match platform {
            Platform::Linux => Bubblewrap::found().map(Confinement::Sandbox),
            Platform::MacOs | Platform::Other => Err(Unavailable::NotImplemented {
                mechanism: Isolation::Sandbox,
                on: platform,
            }),
        },
    }
}

vocabulary! {
    /// What a command's own words say it ran into.
    ///
    /// **A classification of the command's output, not an observation of the
    /// kernel.** The sandbox denies at the syscall; what reaches this module
    /// is whatever the command chose to print about it. Three of these are
    /// unambiguous under a sandbox policy and one is not, and the one that is
    /// not says so in its name.
    DenialKind {
        /// The command was told the filesystem is read-only. Under a sandbox
        /// policy nothing else produces this: the tree is the writable path.
        ReadOnly => "read_only",
        /// The command could not reach the network.
        Network => "network",
        /// The command was refused by permissions.
        PermissionDenied => "permission_denied",
        /// The path was not there. **Ambiguous by construction**: a path the
        /// policy did not bind and a path that genuinely does not exist say
        /// the same thing to a command, and this module cannot tell them
        /// apart. Named so that nobody reads it as the first alone.
        AbsentOrOutside => "absent_or_outside",
    }
}

impl DenialKind {
    /// Whether this kind means one thing under a sandbox policy.
    ///
    /// [`DenialKind::AbsentOrOutside`] does not, and a census that counted it
    /// as a denial would be counting every typo for a missing file as an
    /// attempted escape.
    #[must_use]
    pub fn is_unambiguous(self) -> bool {
        match self {
            Self::ReadOnly | Self::Network | Self::PermissionDenied => true,
            Self::AbsentOrOutside => false,
        }
    }
}

/// The words a kernel's error reaches a command's output as.
///
/// A table rather than arms, because it is the interface between this module
/// and every program a drive might run, and it should be readable in one
/// place. Matched case-insensitively against each line of standard error.
const MARKS: &[(DenialKind, &str)] = &[
    (DenialKind::ReadOnly, "read-only file system"),
    (DenialKind::Network, "network is unreachable"),
    (DenialKind::Network, "temporary failure in name resolution"),
    (DenialKind::Network, "name or service not known"),
    (DenialKind::Network, "could not resolve host"),
    (DenialKind::PermissionDenied, "permission denied"),
    (DenialKind::AbsentOrOutside, "no such file or directory"),
    (DenialKind::AbsentOrOutside, "directory nonexistent"),
];

/// One line of a command's output that reads like a confinement refusing it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Denial {
    /// What it reads like.
    pub kind: DenialKind,
    /// The line, **verbatim**. The command's own words are the evidence, and
    /// they are what the model is shown; a paraphrase here would be the
    /// harness inventing a reason and the model believing it.
    pub evidence: String,
}

/// What became of a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ran {
    /// The command as the drive asked for it.
    pub argv: Vec<String>,
    /// What was actually executed, runner and all. The record carries this so
    /// a later reader can say what the command COULD have touched, which is
    /// the question a policy in prose cannot answer.
    pub confined: Vec<String>,
    /// The mechanism it ran under.
    pub isolation: Isolation,
    /// The network it had.
    pub network: Network,
    /// The status it exited with, or `None` if a signal ended it.
    pub exit: Option<i32>,
    /// What it printed.
    pub stdout: String,
    /// What it printed on standard error.
    pub stderr: String,
}

impl Ran {
    /// Every line of standard error that reads like a refusal.
    #[must_use]
    pub fn denials(&self) -> Vec<Denial> {
        self.stderr
            .lines()
            .filter_map(|line| {
                let lowered = line.to_ascii_lowercase();
                MARKS
                    .iter()
                    .find(|(_, mark)| lowered.contains(mark))
                    .map(|(kind, _)| Denial {
                        kind: *kind,
                        evidence: line.to_owned(),
                    })
            })
            .collect()
    }

    /// The tool result, as the model is shown it.
    ///
    /// The command's own output, unrewritten, followed by one advisory line
    /// naming the policy it ran under. The requirement this satisfies is
    /// exact: a denial is *surfaced in the ordinary register*, never silently
    /// swallowed and never rewritten into something the model has not seen.
    /// So the command's words are carried whole and the note is added after
    /// them, not instead of them -- and it is advisory, because an imperative
    /// at this site is what produces capitulation on a false positive.
    #[must_use]
    pub fn as_the_model_sees_it(&self) -> String {
        let mut out = String::new();
        if !self.stdout.is_empty() {
            out.push_str(&self.stdout);
            if !self.stdout.ends_with('\n') {
                out.push('\n');
            }
        }
        if !self.stderr.is_empty() {
            out.push_str(&self.stderr);
            if !self.stderr.ends_with('\n') {
                out.push('\n');
            }
        }
        if !self.denials().is_empty() {
            // Infallible: the target is a String.
            let _ = write!(
                out,
                "\n(this command ran under isolation `{}` with network `{}`: the working \
                 tree is the only writable path, and paths outside the policy are not \
                 present.)\n",
                self.isolation.tag(),
                self.network.tag()
            );
        }
        out
    }
}

/// The runner failed to set the sandbox up, so the command never ran.
///
/// Its own error rather than an exit status, because a setup failure reported
/// as the command's exit code is a fabricated result: the command produced
/// nothing, and something else's failure would be banked as its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupFailed {
    /// What the runner said.
    pub said: String,
}

impl fmt::Display for SetupFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the sandbox runner did not start: {}", self.said)
    }
}

impl Error for SetupFailed {}

impl Confinement {
    /// The mechanism in force.
    #[must_use]
    pub fn isolation(&self) -> Isolation {
        match self {
            Self::Unconfined => Isolation::None,
            Self::Sandbox(_) => Isolation::Sandbox,
        }
    }

    /// The argv this confinement would execute for `argv`.
    ///
    /// A pure function, exposed so that what a command runs under can be
    /// asserted without running anything. The composition is the whole of the
    /// confinement; a test that could only check it by observing a denial
    /// could not check it on a host without the runner.
    #[must_use]
    pub fn compose(&self, policy: &Policy, worktree: &Path, argv: &[String]) -> Vec<String> {
        match self {
            Self::Unconfined => argv.to_vec(),
            Self::Sandbox(runner) => runner.compose(policy, worktree, argv),
        }
    }

    /// Run `argv` under this confinement.
    ///
    /// # Errors
    ///
    /// Returns [`SetupFailed`] where the runner could not build the sandbox,
    /// so the command never ran. A command that ran and failed is an `Ok`
    /// carrying its status.
    pub fn run(
        &self,
        policy: &Policy,
        worktree: &Path,
        argv: &[String],
    ) -> Result<Ran, SetupFailed> {
        let confined = self.compose(policy, worktree, argv);
        let Some((program, rest)) = confined.split_first() else {
            return Err(SetupFailed {
                said: "an empty command".to_owned(),
            });
        };

        let output = Command::new(program)
            .args(rest)
            .current_dir(worktree)
            .output()
            .map_err(|why| SetupFailed {
                said: format!("{program} could not be run: {why}"),
            })?;

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        if let Self::Sandbox(runner) = self
            && let Some(said) = runner.setup_failure(&stderr, output.status.code())
        {
            return Err(SetupFailed { said });
        }

        Ok(Ran {
            argv: argv.to_vec(),
            confined,
            isolation: self.isolation(),
            network: policy.network,
            exit: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process;

    use super::bwrap::{Bubblewrap, RUNNER};
    use super::policy::{Isolation, Network, Policy, PolicyError};
    use super::{Confinement, Denial, DenialKind, Platform, Ran, Unavailable, open_on};
    use crate::formats::regimen;

    fn policy(text: &str) -> Policy {
        Policy::from_regimen(&regimen::parse(text).expect("a regimen")).expect("a policy")
    }

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_owned()).collect()
    }

    /// A worktree, and a sibling directory the policy does not bind.
    ///
    /// The sibling stands in for a home directory: a path that exists, holds
    /// something worth not leaking, and is not in the policy. Using the real
    /// `$HOME` would make the test write to the operator's account to prove
    /// that it cannot read it, which is a strange way round.
    struct Ground {
        tree: PathBuf,
        outside: PathBuf,
    }

    impl Ground {
        fn make(name: &str) -> Self {
            let base = std::env::temp_dir().join(format!(
                "diet-isolation-{}-{name}-{}",
                process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|since| since.as_nanos())
                    .unwrap_or_default()
            ));
            let tree = base.join("tree");
            let outside = base.join("outside");
            fs::create_dir_all(&tree).expect("a working tree");
            fs::create_dir_all(&outside).expect("a directory outside it");
            fs::write(outside.join("secret"), "s3cret\n").expect("a secret to not read");
            Self { tree, outside }
        }
    }

    impl Drop for Ground {
        fn drop(&mut self) {
            if let Some(base) = self.tree.parent() {
                let _ = fs::remove_dir_all(base);
            }
        }
    }

    // -----------------------------------------------------------------------
    // row four, and the rule it is an instance of: no silent fallback
    // -----------------------------------------------------------------------

    #[test]
    fn a_declared_vm_refuses_to_start_rather_than_falling_back_to_the_sandbox() {
        let declared = policy("isolation = \"vm\"\n");
        assert_eq!(declared.isolation, Isolation::Vm);

        let refusal = open_on(&declared, Platform::Linux)
            .expect_err("no VM runner is built, and the drive does not start");
        assert_eq!(
            refusal,
            Unavailable::NotImplemented {
                mechanism: Isolation::Vm,
                on: Platform::Linux,
            }
        );
        assert_eq!(
            Unavailable::EXIT,
            2,
            "not one: a drive that could not start is not a drive that failed"
        );
        assert!(
            refusal.to_string().contains("refusing to start"),
            "and it says so: {refusal}"
        );
    }

    #[test]
    fn the_macos_half_is_not_built_and_says_so_from_here() {
        // The deferral as a fact of the tree rather than a comment. A `#[cfg]`
        // would compile this away on the platform the tests run on, which is
        // the platform where somebody would want to check it.
        let refusal = open_on(&Policy::merged_usr(), Platform::MacOs)
            .expect_err("the platform's sandbox profile mechanism is not built");
        assert_eq!(
            refusal,
            Unavailable::NotImplemented {
                mechanism: Isolation::Sandbox,
                on: Platform::MacOs,
            }
        );
        assert!(matches!(
            open_on(&Policy::merged_usr(), Platform::Other),
            Err(Unavailable::NotImplemented { .. })
        ));
    }

    #[test]
    fn a_regimen_that_says_nothing_about_isolation_gets_the_sandbox() {
        // The cheap-by-default ruling in its operative form: the way to run
        // unconfined is to write it down.
        let silent = policy("model = \"a-model\"\n");
        assert_eq!(silent.isolation, Isolation::Sandbox);
        assert_eq!(silent.network, Network::None);
        assert_eq!(silent, Policy::merged_usr());

        let declared = policy("isolation = \"none\"\n");
        assert_eq!(declared.isolation, Isolation::None);
        assert_eq!(
            open_on(&declared, Platform::MacOs)
                .expect("unconfined needs no runner")
                .isolation(),
            Isolation::None,
            "and it needs no mechanism, on any platform"
        );
    }

    // -----------------------------------------------------------------------
    // the composition, which is checkable without a runner
    // -----------------------------------------------------------------------

    #[test]
    fn the_composition_is_exactly_what_the_policy_declares() {
        let runner = Confinement::Sandbox(Bubblewrap::at(PathBuf::from("/usr/bin/bwrap")));
        let composed = runner.compose(
            &Policy::merged_usr(),
            &PathBuf::from("/work/tree"),
            &argv(&["cargo", "test"]),
        );

        assert_eq!(
            composed,
            argv(&[
                "/usr/bin/bwrap",
                "--unshare-all",
                "--die-with-parent",
                "--new-session",
                "--ro-bind",
                "/usr",
                "/usr",
                "--ro-bind",
                "/etc",
                "/etc",
                "--symlink",
                "usr/bin",
                "/bin",
                "--symlink",
                "usr/lib",
                "/lib",
                "--symlink",
                "usr/lib64",
                "/lib64",
                "--symlink",
                "usr/sbin",
                "/sbin",
                "--proc",
                "/proc",
                "--dev",
                "/dev",
                "--bind",
                "/work/tree",
                "/work/tree",
                "--remount-ro",
                "/",
                "--chdir",
                "/work/tree",
                "--",
                "cargo",
                "test",
            ])
        );
    }

    #[test]
    fn the_read_only_remount_comes_after_every_bind() {
        // Order is the whole confinement. A remount before the binds remounts
        // a root the binds then reopen, and the sandbox starts, and does not
        // confine.
        let runner = Confinement::Sandbox(Bubblewrap::at(PathBuf::from("bwrap")));
        let composed = runner.compose(
            &Policy::merged_usr(),
            &PathBuf::from("/work/tree"),
            &argv(&["true"]),
        );
        let at = |needle: &str| {
            composed
                .iter()
                .position(|arg| arg == needle)
                .unwrap_or_else(|| panic!("`{needle}` is not in {composed:?}"))
        };
        assert!(
            at("--ro-bind") < at("--bind"),
            "the tree is bound after the read-only paths, so a tree inside one is still writable"
        );
        assert!(at("--bind") < at("--remount-ro"));
        assert!(at("--remount-ro") < at("--chdir"));
        assert!(at("--remount-ro") < at("--"), "and before the command");
    }

    #[test]
    fn the_network_is_shared_only_where_the_regimen_declares_it() {
        let runner = Confinement::Sandbox(Bubblewrap::at(PathBuf::from("bwrap")));
        let compose = |policy: &Policy| {
            runner.compose(policy, &PathBuf::from("/work/tree"), &argv(&["true"]))
        };

        assert!(
            !compose(&policy("network = \"none\"\n")).contains(&"--share-net".to_owned()),
            "the default denies"
        );
        assert!(
            compose(&policy("network = \"host\"\n")).contains(&"--share-net".to_owned()),
            "and a declaration is what opens it"
        );
    }

    #[test]
    fn an_unconfined_policy_composes_the_command_and_nothing_else() {
        let composed = Confinement::Unconfined.compose(
            &Policy::unconfined(),
            &PathBuf::from("/work/tree"),
            &argv(&["echo", "hello"]),
        );
        assert_eq!(composed, argv(&["echo", "hello"]));
    }

    // -----------------------------------------------------------------------
    // rows two and three: the seeded escapes, against the real mechanism
    // -----------------------------------------------------------------------

    /// The four seeded escapes, and the two controls without which they prove
    /// nothing.
    ///
    /// One test rather than six, because the branch matters: on a host with
    /// the runner every escape is executed, and on a host without it the
    /// REFUSAL is what is asserted. A test that quietly skipped where the
    /// runner is absent would be a test that cannot fail, and this module is
    /// the last thing that should have one.
    #[test]
    fn the_seeded_escapes_are_denied_where_the_mechanism_is_present() {
        let policy = Policy::merged_usr();
        let confinement = match open_on(&policy, Platform::Linux) {
            Err(unavailable) => {
                assert_eq!(
                    unavailable,
                    Unavailable::NoRunner {
                        mechanism: Isolation::Sandbox,
                        looked_for: RUNNER.to_owned(),
                    },
                    "no runner, so the drive refuses; the escapes are not executed here"
                );
                eprintln!(
                    "isolation: `{RUNNER}` absent, 0 of 4 seeded escapes executed; \
                     the refusal is what this host proves"
                );
                return;
            }
            Ok(confinement) => confinement,
        };

        let ground = Ground::make("escapes");
        let run = |parts: &[&str]| {
            confinement
                .run(&policy, &ground.tree, &argv(parts))
                .expect("the sandbox set up")
        };

        // Control one. Without it, a sandbox that denied everything would
        // pass every row below.
        let inside = run(&[
            "sh",
            "-c",
            "echo written > ./in-tree.txt && cat ./in-tree.txt",
        ]);
        assert_eq!(inside.exit, Some(0), "{}", inside.stderr);
        assert_eq!(inside.stdout.trim(), "written");
        assert!(inside.denials().is_empty());
        assert!(
            ground.tree.join("in-tree.txt").is_file(),
            "and the write reached the real tree, not a throwaway the caller never sees"
        );

        // Row two: a write outside the tree.
        let outside = run(&["sh", "-c", "echo x > /tmp/outside"]);
        assert_ne!(outside.exit, Some(0), "the write was denied");
        let denials = outside.denials();
        assert!(
            denials
                .iter()
                .any(|denial| denial.kind == DenialKind::ReadOnly
                    || denial.kind == DenialKind::AbsentOrOutside),
            "and a typed denial is recorded: {denials:?} from {:?}",
            outside.stderr
        );
        assert!(
            !std::path::Path::new("/tmp/outside").exists(),
            "and nothing reached the host"
        );

        // A path outside the policy: present on this host, absent in there.
        let secret = ground.outside.join("secret");
        let read = run(&["cat", &secret.to_string_lossy()]);
        assert_ne!(read.exit, Some(0), "the secret was not readable");
        assert!(
            !read.stdout.contains("s3cret"),
            "and did not leak: {:?}",
            read.stdout
        );
        assert!(secret.is_file(), "though it is right there on the host");

        // Row three: a network call under `network = none`.
        let reached = run(&[
            "python3",
            "-c",
            "import socket; socket.create_connection(('1.1.1.1', 80), 2)",
        ]);
        assert_ne!(reached.exit, Some(0), "the call was denied");
        assert!(
            reached
                .denials()
                .iter()
                .any(|denial| denial.kind == DenialKind::Network),
            "and recorded as a network denial: {:?}",
            reached.stderr
        );

        // Control two: the same call with the network declared. Without it,
        // "denied" could be the command being broken rather than the policy
        // working.
        let declared = Policy {
            network: Network::Host,
            ..policy.clone()
        };
        let shared = confinement
            .run(
                &declared,
                &ground.tree,
                &argv(&[
                    "python3",
                    "-c",
                    "import socket; print('socket layer:', socket.socket().connect_ex(('127.0.0.1', 1)))",
                ]),
            )
            .expect("the sandbox set up");
        assert_eq!(shared.exit, Some(0), "{}", shared.stderr);
        assert!(
            shared.stdout.contains("socket layer:"),
            "with the network declared the same command reaches the socket layer: {:?}",
            shared.stdout
        );

        eprintln!("isolation: `{RUNNER}` present, 4 of 4 seeded escapes executed and denied");
    }

    // -----------------------------------------------------------------------
    // what the model is shown, and what the record carries
    // -----------------------------------------------------------------------

    fn ran(stderr: &str) -> Ran {
        Ran {
            argv: argv(&["sh", "-c", "echo x > /etc/hosts"]),
            confined: argv(&["bwrap", "--", "sh"]),
            isolation: Isolation::Sandbox,
            network: Network::None,
            exit: Some(1),
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn the_model_is_shown_the_commands_own_words_and_nothing_instead_of_them() {
        let said = "sh: 1: cannot create /etc/hosts: Read-only file system";
        let shown = ran(said).as_the_model_sees_it();

        assert!(
            shown.contains(said),
            "the command's words are carried whole, never rewritten into something \
             the model has not seen: {shown}"
        );
        assert!(
            shown.contains("ran under isolation `sandbox` with network `none`"),
            "and the policy is named after them, not instead of them: {shown}"
        );
        assert!(
            !shown.contains("must") && !shown.contains("Do not"),
            "advisory, because an imperative at this site is what produces \
             capitulation on a false positive: {shown}"
        );
    }

    #[test]
    fn a_command_that_was_not_denied_gets_no_note_at_all() {
        let quiet = Ran {
            exit: Some(0),
            stdout: "hello\n".to_owned(),
            stderr: String::new(),
            ..ran("")
        };
        assert_eq!(quiet.as_the_model_sees_it(), "hello\n");
        assert!(quiet.denials().is_empty());
    }

    #[test]
    fn an_ambiguous_denial_is_named_as_one() {
        // The honest half of the classification: a path the policy did not
        // bind and a path that genuinely does not exist say the same thing to
        // a command, and this module cannot tell them apart.
        let missing = ran("cat: /opt/keys/deploy.pem: No such file or directory");
        assert_eq!(
            missing.denials(),
            vec![Denial {
                kind: DenialKind::AbsentOrOutside,
                evidence: "cat: /opt/keys/deploy.pem: No such file or directory".to_owned(),
            }]
        );
        assert!(
            !DenialKind::AbsentOrOutside.is_unambiguous(),
            "a census that counted this as an attempted escape would be counting \
             every typo for a missing file as one"
        );
        assert!(DenialKind::ReadOnly.is_unambiguous());
        assert!(DenialKind::Network.is_unambiguous());
        assert!(DenialKind::PermissionDenied.is_unambiguous());
    }

    #[test]
    fn every_denial_line_is_kept_and_none_is_invented() {
        let many = ran("cargo: error one\n\
             sh: cannot create /x: Read-only file system\n\
             ordinary output\n\
             curl: (6) Could not resolve host: example.invalid");
        let kinds: Vec<DenialKind> = many.denials().iter().map(|denial| denial.kind).collect();
        assert_eq!(
            kinds,
            vec![DenialKind::ReadOnly, DenialKind::Network],
            "two lines read like refusals and two do not"
        );
        for denial in many.denials() {
            assert!(
                many.stderr.contains(&denial.evidence),
                "the evidence is a line the command actually printed"
            );
        }
    }

    #[test]
    fn the_record_carries_what_the_command_could_have_touched() {
        let subject = ran("");
        assert_eq!(subject.isolation, Isolation::Sandbox);
        assert_eq!(subject.network, Network::None);
        assert!(
            !subject.confined.is_empty() && subject.confined != subject.argv,
            "the confined argv is kept beside the asked-for one: a policy in prose \
             cannot answer what a command could have reached, and this can"
        );
    }

    #[test]
    fn a_runner_that_could_not_build_the_sandbox_is_not_a_command_result() {
        let runner = Bubblewrap::at(PathBuf::from("bwrap"));
        assert_eq!(
            runner.setup_failure("bwrap: Can't mkdir /dev: Read-only file system\n", Some(1)),
            Some("bwrap: Can't mkdir /dev: Read-only file system".to_owned()),
            "a setup failure reported as the command's exit code is a fabricated \
             result: the command produced nothing"
        );
        assert_eq!(
            runner.setup_failure("cat: /nope: No such file or directory\n", Some(1)),
            None,
            "and an ordinary command failure is not one"
        );
        assert_eq!(
            runner.setup_failure("bwrap: something\n", Some(0)),
            None,
            "the runner exits 1 when it fails to set up"
        );
    }

    // -----------------------------------------------------------------------
    // the policy, read from a regimen
    // -----------------------------------------------------------------------

    #[test]
    fn a_policy_is_read_from_the_regimen_or_refused_with_a_reason() {
        let read = |text: &str| Policy::from_regimen(&regimen::parse(text).expect("a regimen"));

        let declared = read(
            "isolation = \"sandbox\"\n\
             network = \"host\"\n\
             sandbox_readable = [\"/usr\", \"/opt/toolchain\"]\n\
             [sandbox_symlinks]\n\
             bin = \"usr/bin\"\n",
        )
        .expect("a policy");
        assert_eq!(
            declared.readable,
            ["/usr".to_owned(), "/opt/toolchain".to_owned()]
        );
        assert_eq!(declared.symlinks.len(), 1);
        assert_eq!(declared.network, Network::Host);

        assert_eq!(
            read("isolation = \"jail\"\n"),
            Err(PolicyError::NotAName {
                key: "isolation",
                written: "jail".to_owned(),
                allowed: vec!["none", "sandbox", "vm"],
            }),
            "a mechanism outside the vocabulary is refused rather than read as the default"
        );
        assert!(matches!(
            read("network = 1\n"),
            Err(PolicyError::NotAName { .. })
        ));
        assert!(matches!(
            read("sandbox_readable = \"/usr\"\n"),
            Err(PolicyError::NotAListOfPaths(_))
        ));
        assert_eq!(
            read("sandbox_readable = [\"usr\"]\n"),
            Err(PolicyError::NotAbsolute("usr".to_owned())),
            "a relative path in a sandbox policy means whatever the harness's \
             working directory happened to be"
        );
    }

    /// Every fault in this lane's `gate.toml` still names source that exists.
    ///
    /// The manifest carries each mutation's exact source text so the
    /// orchestrator (#46) can apply it. That only works while the text is
    /// still in the file it names, and nothing else checks: the orchestrator
    /// is not built, and `check-fault-manifest.py` reads the gate's own
    /// manifest rather than a package's. A manifest nobody checks is a
    /// manifest that goes stale, and a stale seeded fault is one that
    /// silently stops testing what it says it tests.
    ///
    /// Scanned rather than parsed as TOML: this crate has no TOML reader for
    /// multi-line strings, and writing one to check a file this repository
    /// generates would be a second reader of a format that already has one.
    /// The scan mirrors the generator's shape exactly, and a scan that finds
    /// no faults fails rather than passing over nothing.
    #[test]
    fn every_seeded_fault_still_names_source_that_is_there() {
        let manifest = include_str!("../../isolation/gate.toml");
        let mut checked = 0;
        for block in manifest.split("\n[[fault]]\n").skip(1) {
            let id = between(block, "id = \"", "\"").expect("a fault has an id");
            let target = between(block, "target = \"", "\"").expect("a fault has a target");
            let anchor =
                between(block, "anchor = '''\n", "'''\nbecomes = ").expect("a fault has an anchor");

            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("the workspace root")
                .join(target);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|why| panic!("{id}: {} could not be read: {why}", path.display()));
            assert_eq!(
                source.matches(anchor).count(),
                1,
                "{id}: its anchor no longer appears exactly once in {target}. The \
                 manifest is stale: either the mutation has to move with the code, \
                 or the fault it seeds is gone."
            );
            checked += 1;
        }
        let declared: usize = between(manifest, "\nfaults = ", "\n")
            .expect("the package block declares a count")
            .parse()
            .expect("the count is a number");
        assert_eq!(
            checked, declared,
            "the manifest declares its own count and this reads it: hardcoding the \
             number here let `faults = 9001` pass"
        );
    }

    /// The text between `open` and the next `close` after it.
    fn between<'a>(haystack: &'a str, open: &str, close: &str) -> Option<&'a str> {
        let start = haystack.find(open)? + open.len();
        let end = haystack[start..].find(close)? + start;
        Some(&haystack[start..end])
    }

    #[test]
    fn the_isolations_vocabularies_are_the_words_a_regimen_and_a_record_carry() {
        assert_eq!(
            Isolation::ALL.iter().map(|i| i.tag()).collect::<Vec<_>>(),
            ["none", "sandbox", "vm"]
        );
        assert_eq!(
            Network::ALL.iter().map(|n| n.tag()).collect::<Vec<_>>(),
            ["none", "host"]
        );
        assert_eq!(
            DenialKind::ALL.iter().map(|d| d.tag()).collect::<Vec<_>>(),
            [
                "read_only",
                "network",
                "permission_denied",
                "absent_or_outside"
            ]
        );
        assert_eq!(
            Platform::ALL.iter().map(|p| p.tag()).collect::<Vec<_>>(),
            ["linux", "macos", "other"]
        );
    }
}
