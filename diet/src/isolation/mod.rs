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
//! # Two backends, one word
//!
//! `isolation = "sandbox"` is `bwrap` on Linux ([`bwrap`]) and a default-deny
//! Seatbelt profile through `sandbox-exec` on macOS ([`seatbelt`], #299). The
//! record says `sandbox` for both; a reader tells them apart by
//! `Ran::confined[0]`, the runner. A missing `sandbox-exec` refuses to start,
//! as a missing `bwrap` does, and any other platform refuses with
//! [`Unavailable::NotImplemented`].
//!
//! Every confined run carries `Ran::policy`, a sha256: of exactly the profile
//! bytes under Seatbelt, of the composed argv up to `--` under `bwrap`.
//!
//! # What is deferred, and says so
//!
//! **`network_loopback` on Linux.** Under `bwrap --unshare-net` the command's
//! loopback is its own and cannot reach the host's ports, so a regimen that
//! declares loopback ports refuses to start there
//! ([`Unavailable::LoopbackNotImplemented`]); one that needs a host port on
//! Linux declares `network = "host"`.
//!
//! # What has been seen red
//!
//! Fifty-five mutations, each applied to the source and RUN, while the
//! seeded-fault gate existed (retired in #508). The rows below are the ones
//! worth keeping as the tests' reason.
//!
//! | break this | and this fails |
//! | --- | --- |
//! | a declared `vm` falls back to the sandbox | `a_declared_vm_refuses_to_start_…` |
//! | a Mac without `sandbox-exec` runs unconfined | `a_mac_without_seatbelt_refuses_to_start_…` |
//! | a regimen that says nothing runs unconfined | `a_regimen_that_says_nothing_…` (+1) |
//! | the remount comes before the binds | `the_read_only_remount_comes_after_every_bind` (+2) |
//! | the root is never remounted read-only | the same (+2) |
//! | `/dev` and `/dev/shm` stay writable | `the_seeded_escapes_…` (+1) |
//! | the network is always shared | `the_network_is_shared_only_…` (+2) |
//! | every line of standard error is a denial | `every_denial_line_is_kept_and_none_is_invented` (+2) |
//! | `absent_or_outside` reads as unambiguous | `an_ambiguous_denial_is_named_as_one` |
//! | the policy note is attached to every denial | the same |
//! | a runner's setup failure is read as the command's exit | `a_runner_that_could_not_build_…` (+1) |
//! | `bwrap: execvp` is read as a setup failure | `a_command_the_sandbox_could_not_execute_…` |
//! | the record's provenance is written as constants | `the_record_is_read_from_the_run_…` |
//! | an empty command runs the runner alone | `an_empty_command_is_refused_…` |
//! | a relative working tree is accepted | `a_working_tree_that_cannot_mean_one_thing_…` |
//! | the model is shown a paraphrase instead of the command's words | `the_model_is_shown_the_commands_own_words_…` |
//! | `network = "none"` under `isolation = "none"` is recorded | `a_policy_is_read_from_the_regimen_…` (+2) |
//! | a mechanism outside the vocabulary reads as the default | the same |
//! | a relative path is accepted in a sandbox policy | the same |
//! | the census under-reports | `the_seeded_escapes_…` (Linux and macOS) |
//! | a busy runner is never retried | `a_busy_runner_is_retried_…` (+1 on Linux) |
//! | the Seatbelt profile allows a write outside the tree | `the_seatbelt_profile_writes_only_…` (+1) |
//! | … or a write to the terminal | the same (+1) |
//! | … or, under `host`, every Unix-domain socket | `the_seatbelt_profile_opens_the_network_…` |
//! | ending the session kills nothing | `ending_the_session_kills_what_…` |
//! | a secret's reads are not denied | `the_seatbelt_profile_denies_secrets_…` (+1) |
//! | the secrets' deny precedes the allows (a writable dir wins) | the same (+1) |
//! | `.env` files outside the tree are readable | the same (+1) |
//! | a write to an undeclared dir is admitted | `a_writable_dir_enters_canonical_…` |
//! | the environment is not cleared | `the_command_receives_only_the_environment_…` |
//! | `sandbox_reads = "all"` on Linux runs anyway | `sandbox_reads_all_refuses_to_start_on_linux` |
//! | `~` is expanded from somewhere other than `HOME` | `a_tilde_path_is_the_drives_home_…` (+1) |
//! | a writable dir holding a secret opens | `a_writable_dir_or_tree_holding_a_secret_…` |
//! | a tree holding a secret runs | the same |
//! | a declared path holding a `.env` opens | `a_declared_path_holding_a_dot_env_file_…` |
//! | under bwrap a secret in a declared path is not masked | `bwrap_masks_each_secret_…` |
//! | the `.env` scan is case-sensitive | `the_dot_env_scan_ignores_case_…` |
//! | a declared path inside a secret opens | `a_declared_path_inside_a_secret_…` |
//! | an emptied process group is kept to session end | `a_session_forgets_a_process_group_…` |
//! | a not-yet-existing secret resolves without its ancestor | `a_secret_that_does_not_exist_yet_…` |
//! | the re-executed drive keeps its whole environment | `the_re_executed_drive_holds_only_…` |
//! | the drive's key file is not one of the session's secrets | `the_drives_key_file_joins_…` |
//! | `kill` is looked up on `PATH` | `no_helper_the_drive_runs_unconfined_…` |
//! | the Seatbelt profile allows the network under `none` | `the_seatbelt_profile_opens_the_network_…` (+1) |
//! | the Seatbelt profile allows an undeclared loopback port | the same (+1) |
//! | … a bind, or an inbound connection, on an undeclared port | the same |
//! | … the declared port reaches other hosts | the same (+1) |
//! | the worktree enters the profile uncanonicalised | `the_worktree_enters_seatbelt_canonical` |
//! | the profile is not validated at open | `a_profile_seatbelt_refuses_at_open_…` |
//! | `policy` is hashed over something other than the profile | `the_policy_is_the_digest_of_exactly_…` (+1) |
//! | the bwrap `policy` covers the command | the same (+1) |
//! | `network_loopback` on Linux runs anyway | `network_loopback_refuses_to_start_on_linux` |
//! | `network_loopback` under `host` is accepted | `a_loopback_declaration_is_read_only_…` |
//! | `not_permitted` reads as unambiguous | `an_operation_not_permitted_is_listed_…` |
//!
//! The macOS census rows are a second catcher, on a Mac, for the three
//! profile faults and the census one; they are not named in the manifest's
//! `catches`, because CI runs the manifest on Linux, where they cannot fail.
//!
//! Four of those touch the **real** mechanism rather than the composition,
//! and they are the ones worth naming. With no `--remount-ro /` at all, `echo
//! x > /tmp/outside` exits **0**: bubblewrap's root is a fresh writable
//! tmpfs, so the write succeeds into a throwaway the caller never sees again.
//! That is worse than a denial -- the model is told it wrote a file, and the
//! file evaporates. With only `/` remounted, the same is true of `/dev` and
//! `/dev/shm`, because `--remount-ro` does not recurse and `--dev` builds a
//! separate mount: the sandbox started and did not confine, while the
//! advisory told the model the working tree was the only writable path. With
//! the remount moved *before* the binds, the sandbox fails to set up instead,
//! and [`NotRun`] is what comes back rather than a command result.

pub mod bwrap;
pub mod declared;
pub mod policy;
pub mod seatbelt;

use std::error::Error;
use std::ffi::OsStr;
use std::fmt::{self, Write as _};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use crate::client::vocabulary;

use bwrap::Bubblewrap;
pub use policy::{Isolation, Network, Policy, PolicyError, Reads};
use seatbelt::Seatbelt;

vocabulary! {
    /// The platform a drive is being confined on.
    ///
    /// A value rather than a `#[cfg]`, so that the macOS refusal can be
    /// SEEN refusing from a Linux test. A deferral that compiles away on the
    /// platform where the tests run is a deferral nobody can check.
    Platform {
        /// Namespaces, through `bwrap`.
        Linux => "linux",
        /// A Seatbelt profile, through `sandbox-exec`.
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
    /// `network_loopback` is declared and this platform's sandbox cannot
    /// give a command the host's loopback while denying the rest.
    LoopbackNotImplemented {
        /// Where.
        on: Platform,
    },
    /// `sandbox_reads = "all"` is declared and this platform's sandbox cannot
    /// read everything while denying a secret set.
    ReadsAllNotImplemented {
        /// Where.
        on: Platform,
    },
    /// A `sandbox_writable` dir equals or contains a secret: renaming the
    /// secret's parent would move it out from under its deny.
    SecretUnderWritable {
        /// The writable dir.
        writable: std::path::PathBuf,
        /// The secret inside it.
        secret: std::path::PathBuf,
    },
    /// A declared readable or writable path equals or lies inside a secret.
    DeclaredInsideSecret {
        /// The declared path.
        path: std::path::PathBuf,
        /// The secret it is in.
        secret: std::path::PathBuf,
    },
    /// A declared readable or writable path holds a `.env` file.
    EnvFileDeclared {
        /// The file.
        found: std::path::PathBuf,
    },
    /// Seatbelt would not run `/usr/bin/true` under the generated profile.
    ProfileRefused {
        /// What `sandbox-exec` said, with its exit.
        said: String,
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
            Self::LoopbackNotImplemented { on } => write!(
                f,
                "`{}` (loopback ports under `network = \"none\"`) is not built for {}: \
                 the sandbox's loopback there is its own and cannot reach the host's \
                 ports; declare `network = \"host\"` instead. Refusing to start rather \
                 than running a command that cannot reach what the regimen declared",
                policy::NETWORK_LOOPBACK,
                on.tag()
            ),
            Self::ReadsAllNotImplemented { on } => write!(
                f,
                "`{} = \"all\"` is not built for {}: the sandbox there binds what it is \
                 told to and cannot read everything while hiding a secret set. Refusing \
                 to start rather than running under something other than the record \
                 would say",
                policy::SANDBOX_READS,
                on.tag()
            ),
            Self::SecretUnderWritable { writable, secret } => write!(
                f,
                "`{}` is declared writable and holds the secret `{}`; a command that can \
                 rename a secret's parent can read it at its new name. Refusing to start",
                writable.display(),
                secret.display()
            ),
            Self::DeclaredInsideSecret { path, secret } => write!(
                f,
                "`{}` is declared readable or writable and lies inside the secret `{}`; \
                 a declaration cannot open a secret. Refusing to start",
                path.display(),
                secret.display()
            ),
            Self::EnvFileDeclared { found } => write!(
                f,
                "a declared path holds `{}`; a `.env` file outside the working tree is a \
                 secret this sandbox cannot keep from a command that can move it into the \
                 tree (or, under bwrap, that sees it at all). Refusing to start",
                found.display()
            ),
            Self::ProfileRefused { said } => write!(
                f,
                "Seatbelt (`{}`) refused the generated profile when it was validated at \
                 open ({said}); refusing to start rather than running under something \
                 other than the record would say",
                seatbelt::RUNNER
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
    /// The sandbox, through a runner that was found.
    Sandbox(Backend),
}

/// Which runner `isolation = "sandbox"` is on this platform.
#[derive(Debug, Clone)]
pub enum Backend {
    /// Linux: namespaces, through `bwrap`.
    Bubblewrap(Bubblewrap),
    /// macOS: a Seatbelt profile, through `sandbox-exec`.
    Seatbelt(Seatbelt),
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
    open_in(policy, platform, std::env::var_os("PATH").as_deref())
}

/// The same, with the runner looked for on a `PATH` named rather than read,
/// so a refusal can be seen on a host that has the runner.
fn open_in(
    policy: &Policy,
    platform: Platform,
    path: Option<&OsStr>,
) -> Result<Confinement, Unavailable> {
    match policy.isolation {
        Isolation::None => Ok(Confinement::Unconfined),
        Isolation::Vm => Err(Unavailable::NotImplemented {
            mechanism: Isolation::Vm,
            on: platform,
        }),
        Isolation::Sandbox => {
            // What no profile can refuse by path, refused before anything
            // runs, on both backends (#299 review of 3f7cc1e, A and B).
            if let Some((writable, secret)) = declared::secret_under_writable(policy) {
                return Err(Unavailable::SecretUnderWritable { writable, secret });
            }
            if let Some((path, secret)) = declared::declared_inside_secret(policy) {
                return Err(Unavailable::DeclaredInsideSecret { path, secret });
            }
            if let Some(found) = declared::env_file_declared(policy) {
                return Err(Unavailable::EnvFileDeclared { found });
            }
            open_sandbox(policy, platform, path)
        }
    }
}

/// The sandbox's runner for `platform`.
fn open_sandbox(
    policy: &Policy,
    platform: Platform,
    path: Option<&OsStr>,
) -> Result<Confinement, Unavailable> {
    {
        match platform {
            Platform::Linux => {
                if !policy.loopback.is_empty() {
                    return Err(Unavailable::LoopbackNotImplemented { on: platform });
                }
                if policy.reads == Reads::All {
                    return Err(Unavailable::ReadsAllNotImplemented { on: platform });
                }
                Bubblewrap::found_in(path)
                    .map(|runner| Confinement::Sandbox(Backend::Bubblewrap(runner)))
            }
            Platform::MacOs => {
                let runner = Seatbelt::found_in(path)?;
                let runner = runner.validated(policy)?;
                Ok(Confinement::Sandbox(Backend::Seatbelt(runner)))
            }
            Platform::Other => Err(Unavailable::NotImplemented {
                mechanism: Isolation::Sandbox,
                on: platform,
            }),
        }
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
        /// The operation was not permitted (`EPERM`): what a Seatbelt denial
        /// reaches a command as. **Ambiguous**: `EPERM` also arises with no
        /// sandbox at all, so it is listed and never explained.
        NotPermitted => "not_permitted",
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
            Self::AbsentOrOutside | Self::NotPermitted => false,
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
    (DenialKind::NotPermitted, "operation not permitted"),
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
    /// The confinement's policy as a sha256, 64 lowercase hex: of exactly
    /// the profile bytes under Seatbelt, of the composed argv up to `--`
    /// (NUL-joined) under `bwrap`. `None` unconfined, where there is no
    /// policy to hash.
    pub policy: Option<String>,
    /// The status it exited with, or `None` if a signal ended it.
    pub exit: Option<i32>,
    /// What it printed.
    pub stdout: String,
    /// What it printed on standard error.
    pub stderr: String,
    /// How many bytes it printed: the count of the output itself, which a
    /// lossy decoding into `stdout` may not keep (log v3's `stdout_bytes`).
    pub stdout_bytes: u64,
    /// The same, of standard error.
    pub stderr_bytes: u64,
    /// Whether a stop ended it: its whole process group killed (#551).
    pub cancelled: bool,
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
        // Only where a denial means one thing. `absent_or_outside` is
        // ambiguous by construction -- a path the policy did not bind and a
        // path that does not exist say the same thing to a command -- so an
        // ordinary missing file inside the working tree used to come back with
        // a policy explanation attached, which the model would believe. The
        // module built the distinction and then did not use it at the one site
        // where a false positive reaches the model.
        if self
            .denials()
            .iter()
            .any(|denial| denial.kind.is_unambiguous())
        {
            // Infallible: the target is a String.
            let _ = write!(
                out,
                "\n(this command ran under isolation `{}` with network `{}`: the working \
                 tree is the only writable path, and paths outside the policy are either \
                 absent (under bwrap) or present but denied (under Seatbelt).)\n",
                self.isolation.tag(),
                self.network.tag()
            );
        }
        out
    }
}

/// The command never ran, and why.
///
/// Its own error rather than an exit status, because a failure to run
/// reported as the command's exit code is a fabricated result: the command
/// produced nothing, and something else's failure would be banked as its
/// answer. The variants are separate because they blame different things, and
/// blaming the runner for a bad working tree is its own small fabrication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotRun {
    /// The working tree is not usable as one.
    ///
    /// It is the single writable path in the whole policy and was checked by
    /// nothing. A relative one was resolved TWICE -- once by the harness's own
    /// working directory when spawning the runner, once by the runner inside
    /// the sandbox -- so `repo` under `/w/repo` mounted `/w/repo/repo`
    /// read-write, chdir'd there, and wrote to it, while the declared tree
    /// stayed read-only. `Ran::confined` then recorded a relative path no
    /// later reader could resolve, which is the field's whole purpose gone.
    Worktree {
        /// The path as given.
        path: String,
        /// What is wrong with it.
        why: &'static str,
    },
    /// There was no command to run.
    Nothing,
    /// The working tree equals or contains a secret: a command can rename
    /// the secret's parent, which is in the tree, and read it.
    SecretInTree {
        /// The tree.
        path: String,
        /// The secret.
        secret: String,
    },
    /// The runner could not build the sandbox.
    Runner {
        /// What it said.
        said: String,
    },
}

impl fmt::Display for NotRun {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Worktree { path, why } => {
                write!(f, "`{path}` is not a working tree: {why}")
            }
            Self::Nothing => f.write_str("there is no command to run"),
            Self::SecretInTree { path, secret } => write!(
                f,
                "`{path}` is the working tree and holds the secret `{secret}`; refusing to \
                 run anything in it"
            ),
            Self::Runner { said } => {
                write!(f, "the sandbox runner did not start: {said}")
            }
        }
    }
}

impl Error for NotRun {}

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
            Self::Sandbox(Backend::Bubblewrap(runner)) => runner.compose(policy, worktree, argv),
            Self::Sandbox(Backend::Seatbelt(runner)) => runner.compose(policy, worktree, argv),
        }
    }

    /// End the session: kill every process group a command of it led.
    ///
    /// Under Seatbelt only; `bwrap`'s pid namespace ends with each command,
    /// and unconfined is unconfined. Called once, when the drive ends -- NOT
    /// after each command, because a server one command starts must survive
    /// into the next. A child that called `setsid` itself escapes it.
    pub fn end_session(&self) {
        if let Self::Sandbox(Backend::Seatbelt(runner)) = self {
            runner.end_session();
        }
    }

    /// The `policy` digest a run whose composed argv is `confined` carries.
    #[must_use]
    pub fn policy_of(&self, confined: &[String]) -> Option<String> {
        match self {
            Self::Unconfined => None,
            Self::Sandbox(Backend::Bubblewrap(_)) => Bubblewrap::policy_of(confined),
            Self::Sandbox(Backend::Seatbelt(_)) => Seatbelt::policy_of(confined),
        }
    }

    /// Run `argv` under this confinement.
    ///
    /// # Errors
    ///
    /// Returns [`NotRun`] where the command never ran at all: an unusable
    /// working tree, an empty command, or a runner that could not build the
    /// sandbox. A command that ran and failed is an `Ok` carrying its status.
    pub fn run(&self, policy: &Policy, worktree: &Path, argv: &[String]) -> Result<Ran, NotRun> {
        self.run_until(policy, worktree, argv, &|| false)
    }

    /// [`Self::run`], stopped when `stop` says so (#551).
    ///
    /// The call is waited on by its LEADER, not by its output's end: once
    /// the command it ran has exited, what it printed so far is returned,
    /// though a process it started in the background still holds its pipes
    /// -- a server started with `&` keeps running, in the call's process
    /// group, until a stop or the session's end. When `stop` is asked, the
    /// call's whole process group is killed and the run returns
    /// [`Ran::cancelled`].
    ///
    /// # Errors
    ///
    /// As [`Self::run`].
    pub fn run_until(
        &self,
        policy: &Policy,
        worktree: &Path,
        argv: &[String],
        stop: &dyn Fn() -> bool,
    ) -> Result<Ran, NotRun> {
        // The CALLER's argv, not the composed one. Under `Sandbox` the composed
        // vector is never empty, so this guard only ever fired for
        // `Unconfined` -- and an empty command under the sandbox ran the runner
        // with no command at all and banked sixty lines of its usage text as
        // the command's standard error, at exit 1, as a successful `Ran`.
        if argv.is_empty() {
            return Err(NotRun::Nothing);
        }
        if !worktree.is_absolute() {
            return Err(NotRun::Worktree {
                path: worktree.to_string_lossy().into_owned(),
                why: "it is not absolute, and a relative working tree is resolved once by \
                      the harness and again inside the sandbox",
            });
        }
        if !worktree.is_dir() {
            return Err(NotRun::Worktree {
                path: worktree.to_string_lossy().into_owned(),
                why: "it is not a directory that exists",
            });
        }
        if let Self::Sandbox(_) = self
            && let Some(secret) = declared::secret_under_tree(policy, worktree)
        {
            return Err(NotRun::SecretInTree {
                path: worktree.to_string_lossy().into_owned(),
                secret: secret.to_string_lossy().into_owned(),
            });
        }

        let confined = self.compose(policy, worktree, argv);
        let Some((program, rest)) = confined.split_first() else {
            return Err(NotRun::Nothing);
        };

        let mut child = spawned(|| {
            let mut command = Command::new(program);
            // Only the variables the policy names, on every backend (#29,
            // planning 5981606817). Before this the command inherited the
            // drive's whole environment.
            command.env_clear();
            command.envs(passed(&policy.environment, std::env::vars_os()));
            command
                .args(rest)
                .current_dir(worktree)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            match self {
                // Its own process group, ended with the session (#299, F2).
                Self::Sandbox(Backend::Seatbelt(runner)) => runner.spawn(&mut command),
                // Its own process group too, so a stop can kill all of it
                // (#551).
                Self::Unconfined | Self::Sandbox(Backend::Bubblewrap(_)) => {
                    seatbelt::spawn_leading_a_group(&mut command)
                }
            }
        })
        .map_err(|why| NotRun::Runner {
            said: format!("{program} could not be run: {why}"),
        })?;
        let output = collected(&mut child, stop).map_err(|why| NotRun::Runner {
            said: format!("{program} could not be waited on: {why}"),
        })?;
        if let Self::Sandbox(Backend::Seatbelt(runner)) = self {
            runner.forget_ended_groups();
        }

        let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
        // `bwrap` only. Seatbelt's profile was validated at open, so its exit
        // here is the command's own, 65 included.
        if let Self::Sandbox(Backend::Bubblewrap(runner)) = self
            && let Some(said) = runner.setup_failure(&stderr, output.status.code())
        {
            return Err(NotRun::Runner { said });
        }

        let digest = self.policy_of(&confined);
        Ok(Ran {
            argv: argv.to_vec(),
            confined,
            isolation: self.isolation(),
            network: policy.network,
            policy: digest,
            exit: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr,
            stdout_bytes: u64::try_from(output.stdout.len()).unwrap_or(u64::MAX),
            stderr_bytes: u64::try_from(output.stderr.len()).unwrap_or(u64::MAX),
            cancelled: output.cancelled,
        })
    }
}

/// How long the output a call's leader left in its pipes is read for once
/// the leader has exited (#551). A command whose descendants are all gone
/// reaches the end of its output at once; one that left a process holding
/// its pipes returns this long after its leader's exit.
const DRAIN: Duration = Duration::from_millis(200);

/// How often a running call is looked at: whether its leader exited, and
/// whether a stop was asked.
const POLL: Duration = Duration::from_millis(5);

/// What a call printed and how it ended.
struct Collected {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    cancelled: bool,
}

/// One of a call's output streams, read on a thread of its own into a
/// buffer while the call is collected, and drained and discarded after, so
/// a background writer never blocks on a full pipe nobody reads.
struct Drained {
    kept: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    keeping: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ended: std::sync::mpsc::Receiver<()>,
}

impl Drained {
    fn of(mut stream: impl std::io::Read + Send + 'static) -> Self {
        use std::sync::atomic::Ordering;
        let kept = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let keeping = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let (end, ended) = std::sync::mpsc::channel();
        let (into, still) = (
            std::sync::Arc::clone(&kept),
            std::sync::Arc::clone(&keeping),
        );
        std::thread::spawn(move || {
            let mut chunk = [0_u8; 8192];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(read) if still.load(Ordering::SeqCst) => into
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .extend_from_slice(&chunk[..read]),
                    Ok(_) => {}
                }
            }
            let _ = end.send(());
        });
        Self {
            kept,
            keeping,
            ended,
        }
    }

    /// What was read: all of it once the stream ended, or by `until`,
    /// whichever comes first. Nothing read after is kept.
    fn taken(self, until: Instant) -> Vec<u8> {
        let _ = self
            .ended
            .recv_timeout(until.saturating_duration_since(Instant::now()));
        self.keeping
            .store(false, std::sync::atomic::Ordering::SeqCst);
        std::mem::take(
            &mut *self
                .kept
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
    }
}

/// `child` waited on by its leader: its output so far once the leader has
/// exited, or, when `stop` is asked first, its whole process group killed
/// and reaped (#551).
fn collected(
    child: &mut std::process::Child,
    stop: &dyn Fn() -> bool,
) -> std::io::Result<Collected> {
    let stdout = child.stdout.take().map(Drained::of);
    let stderr = child.stderr.take().map(Drained::of);
    let group = child.id();
    let mut cancelled = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if stop() {
            let _ = seatbelt::kill()
                .args(["-KILL", "--", &format!("-{group}")])
                .output();
            let _ = child.kill();
            cancelled = true;
            break child.wait()?;
        }
        std::thread::sleep(POLL);
    };
    let until = Instant::now() + DRAIN;
    Ok(Collected {
        status,
        stdout: stdout
            .map(|drained| drained.taken(until))
            .unwrap_or_default(),
        stderr: stderr
            .map(|drained| drained.taken(until))
            .unwrap_or_default(),
        cancelled,
    })
}

/// The variables of `parent` that `names` lists, in `names`' order: what a
/// command, or the re-executed drive, receives. Nothing else.
pub fn passed(
    names: &[String],
    parent: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
    let parent: Vec<_> = parent.into_iter().collect();
    names
        .iter()
        .filter_map(|name| {
            parent
                .iter()
                .find(|(key, _)| key.as_os_str() == OsStr::new(name))
                .cloned()
        })
        .collect()
}

/// Set in the re-executed drive's environment, so it does not re-execute
/// again. A constant, never a secret.
pub const SCRUBBED: &str = "DIET_DRIVE_SCRUBBED";

/// The command that re-executes the drive `exe` with `args` under ONLY the
/// variables of `parent` that `names` lists, plus [`SCRUBBED`] (#299, ruling
/// 5983673467, H(1)). `KERN_PROCARGS2` hands a sandboxed command the
/// environment and argv of the drive itself, so the drive holds nothing a
/// command is not given anyway: the endpoint's key is never in either -- it
/// is read from a file after the re-exec.
#[must_use]
pub fn scrubbed_drive(
    exe: &Path,
    args: &[std::ffi::OsString],
    names: &[String],
    parent: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Command {
    let mut command = Command::new(exe);
    command.args(args).env_clear();
    command.envs(passed(names, parent));
    command.env(SCRUBBED, "1");
    command
}

/// How long a runner that is still open for writing is waited for (#88).
///
/// Linux refuses to execute a file open for writing (ETXTBSY). A runner
/// written moments before is open for writing for as long as any process
/// holds a descriptor on it, and a `fork` elsewhere in this process copies
/// every descriptor into the child until that child execs. So the refusal
/// is a window, not a verdict: the spawn is retried across it, and past
/// this bound the refusal is reported as before.
///
/// What is retried is the program THIS process executes. Under the sandbox
/// that is the runner, and a busy command inside it is refused by the
/// runner's own exec, which is the command's result (exit and stderr), not
/// a spawn this process can retry.
const BUSY_PATIENCE: Duration = Duration::from_millis(500);

/// `spawn`'s output, retried while the program is busy being written, for
/// at most [`BUSY_PATIENCE`].
fn output_of(spawn: impl Fn() -> std::io::Result<Output>) -> std::io::Result<Output> {
    spawned(spawn)
}

/// `spawn`'s result -- a child, or its output -- retried while the program
/// is busy being written, for at most [`BUSY_PATIENCE`].
fn spawned<T>(spawn: impl Fn() -> std::io::Result<T>) -> std::io::Result<T> {
    let give_up = Instant::now() + BUSY_PATIENCE;
    loop {
        match spawn() {
            Err(why)
                if why.kind() == std::io::ErrorKind::ExecutableFileBusy
                    && Instant::now() < give_up =>
            {
                std::thread::sleep(Duration::from_millis(10));
            }
            done => return done,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::TcpListener;
    use std::os::unix::fs::PermissionsExt as _;
    use std::path::{Path, PathBuf};
    use std::process;

    use super::bwrap::{Bubblewrap, RUNNER};
    use super::policy::{self, Isolation, Network, Policy, PolicyError, Reads};
    use super::seatbelt::{self, Seatbelt};
    use super::{
        Backend, Confinement, Denial, DenialKind, NotRun, Platform, Ran, Unavailable, open_in,
        open_on,
    };
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
    // #551: a call returns when its command does, and a stop kills it
    // -----------------------------------------------------------------------

    /// Whether process group `group` still has a member.
    fn group_alive(group: &str) -> bool {
        process::Command::new("/bin/kill")
            .args(["-0", "--", &format!("-{group}")])
            .stderr(process::Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    fn kill_group(group: &str) {
        let _ = process::Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{group}")])
            .stderr(process::Stdio::null())
            .status();
    }

    /// `script` run unconfined in `tree` by `/bin/sh`, on a thread, with
    /// `stop`: the run, or `None` when it did not return within `within`.
    fn run_on_a_thread(
        tree: &Path,
        script: &str,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        within: std::time::Duration,
    ) -> Option<(Ran, std::time::Duration)> {
        let (tree, script) = (tree.to_path_buf(), script.to_owned());
        let (sent, ran) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let ran = Confinement::Unconfined
                .run_until(
                    &Policy::unconfined(),
                    &tree,
                    &argv(&["/bin/sh", "-c", &script]),
                    &|| stop.load(std::sync::atomic::Ordering::SeqCst),
                )
                .expect("it ran");
            let _ = sent.send((ran, started.elapsed()));
        });
        ran.recv_timeout(within).ok()
    }

    /// The smoke run's shape (#551): a list backgrounded with `&` holds the
    /// call's pipes while its server runs. The call returns once its
    /// foreground command exits, with what it printed, and the server keeps
    /// running in the call's group.
    #[test]
    fn a_call_returns_when_its_command_exits_though_a_background_server_holds_its_pipes() {
        let ground = Ground::make("background-holds-pipes");
        let script = format!(
            "echo $$; cd {} && /usr/bin/nohup /bin/sleep 30 > server.log 2>&1 & /bin/sleep 1; \
             echo started",
            ground.tree.display()
        );
        let never = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let Some((ran, took)) = run_on_a_thread(
            &ground.tree,
            &script,
            never,
            std::time::Duration::from_secs(15),
        ) else {
            panic!("the call did not return while its background server held its pipes");
        };
        let group = ran
            .stdout
            .lines()
            .next()
            .expect("the leader's pid")
            .to_owned();
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
        assert_eq!(ran.exit, Some(0), "{ran:?}");
        assert!(!ran.cancelled);
        assert!(ran.stdout.ends_with("started\n"), "{:?}", ran.stdout);
        assert!(
            group_alive(&group),
            "the background server was killed with the call"
        );
        kill_group(&group);
    }

    /// The same shape under the Mac's own Seatbelt, which T1 runs under: the
    /// call returns, the server survives it, and the session's end kills it.
    /// On Linux `bwrap`'s pid namespace ends a server with its call, so this
    /// is the Mac's row only.
    #[test]
    fn under_seatbelt_a_backgrounded_server_survives_its_call_until_the_session_ends() {
        if Platform::here() != Platform::MacOs {
            return;
        }
        let ground = Ground::make("seatbelt-background");
        let confinement = open_on(&Policy::merged_usr(), Platform::MacOs).expect("Seatbelt");
        let (sent, ran) = std::sync::mpsc::channel();
        let (tree, runner) = (ground.tree.clone(), confinement.clone());
        // A LIST backgrounded, as the smoke run's was: its subshell holds the
        // call's pipes while the server runs.
        let script = format!(
            "echo $$; cd {} && /usr/bin/nohup /bin/sleep 30 > server.log 2>&1 & /bin/sleep 1; \
             echo started",
            ground.tree.display()
        );
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let ran = runner
                .run(
                    &Policy::merged_usr(),
                    &tree,
                    &argv(&["/bin/sh", "-c", &script]),
                )
                .expect("it ran");
            let _ = sent.send((ran, started.elapsed()));
        });
        let (ran, took) = ran
            .recv_timeout(std::time::Duration::from_secs(15))
            .expect("the call returned while its background server held its pipes");
        let group = ran
            .stdout
            .lines()
            .next()
            .expect("the leader's pid")
            .to_owned();
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
        assert!(ran.stdout.ends_with("started\n"), "{:?}", ran.stdout);
        assert!(group_alive(&group), "the server did not survive its call");
        confinement.end_session();
        let gone = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while group_alive(&group) && std::time::Instant::now() < gone {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            !group_alive(&group),
            "the session's end left the server running"
        );
    }

    /// A stop kills a running call's whole process group -- its foreground
    /// command and what it started in the background -- and the run says it
    /// was cancelled, with what it printed before.
    #[test]
    fn a_stop_kills_a_running_calls_whole_group_and_says_so() {
        let ground = Ground::make("stop-kills-group");
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let asked = std::sync::Arc::clone(&stop);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            asked.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let Some((ran, took)) = run_on_a_thread(
            &ground.tree,
            "echo $$; /bin/sleep 600 & /bin/sleep 600",
            stop,
            std::time::Duration::from_secs(15),
        ) else {
            panic!("the stop did not end the call");
        };
        let group = ran
            .stdout
            .lines()
            .next()
            .expect("the leader's pid")
            .to_owned();
        assert!(ran.cancelled, "{ran:?}");
        assert!(took < std::time::Duration::from_secs(5), "{took:?}");
        // The kill is asynchronous to the group's last member: give it a beat.
        let gone = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while group_alive(&group) && std::time::Instant::now() < gone {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            !group_alive(&group),
            "a member of the call's group survived the stop"
        );
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
    fn a_mac_without_seatbelt_refuses_to_start_rather_than_running_unconfined() {
        // A `PATH` with nothing on it, named rather than read, so the refusal
        // is seen on a Mac that HAS `sandbox-exec` as well as on Linux.
        let ground = Ground::make("no-seatbelt");
        let empty = ground.outside.join("empty-path");
        fs::create_dir_all(&empty).expect("an empty directory to search");
        let refusal = open_in(
            &Policy::merged_usr(),
            Platform::MacOs,
            Some(empty.as_os_str()),
        )
        .expect_err("no `sandbox-exec`, so the drive does not start");
        assert_eq!(
            refusal,
            Unavailable::NoRunner {
                mechanism: Isolation::Sandbox,
                looked_for: seatbelt::RUNNER.to_owned(),
            }
        );
        assert!(
            refusal.to_string().contains("sandbox-exec")
                && refusal.to_string().contains("refusing to start"),
            "and it names the runner it looked for: {refusal}"
        );
        assert!(
            matches!(
                open_in(&Policy::merged_usr(), Platform::MacOs, None),
                Err(Unavailable::NoRunner { .. })
            ),
            "no `PATH` at all is the same refusal"
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
        let runner = Confinement::Sandbox(Backend::Bubblewrap(Bubblewrap::at(PathBuf::from(
            "/usr/bin/bwrap",
        ))));
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
                "--tmpfs",
                "/dev/shm",
                "--bind",
                "/work/tree",
                "/work/tree",
                "--remount-ro",
                "/",
                "--remount-ro",
                "/dev",
                "--remount-ro",
                "/dev/shm",
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
        let runner =
            Confinement::Sandbox(Backend::Bubblewrap(Bubblewrap::at(PathBuf::from("bwrap"))));
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
        let runner =
            Confinement::Sandbox(Backend::Bubblewrap(Bubblewrap::at(PathBuf::from("bwrap"))));
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

    /// The number of seeded escapes executed where the mechanism is present.
    ///
    /// CHECKED rather than printed. libtest discards a passing test's output,
    /// so a census in an `eprintln!` is invisible by construction -- this
    /// seat claimed twice on #62 that the census was visible in CI, measured
    /// it, and found it was not. A row deleted from the test below would
    /// leave the count unspoken; asserted, it does not.
    const SEEDED_ESCAPES: usize = 6;

    /// The same, for the macOS rows: child inheritance (two), a write to
    /// `/private/tmp`, a write into a second directory outside the tree, a
    /// secret read there, the two network rows, an undeclared loopback port
    /// beside a declared one, a listener and a bare bind on undeclared ports,
    /// the declared port off this host, a Unix-socket connect under `host`,
    /// an orphan's late write, and the scrubbed drive's absent key. The
    /// orphan's network and another process's environment are measured and
    /// asserted but are not escapes: both are disclosed equipment limits
    /// (#299, 5981374872 and 5983673467). And eight under a permissive policy
    /// (`the_permissive_rows`).
    const SEEDED_ESCAPES_MACOS: usize = 22;

    /// Set on a host where the sandbox is REQUIRED rather than merely welcome.
    ///
    /// Without it the no-runner branch is a correct refusal and nothing more:
    /// on a machine with no `bwrap` the escapes cannot run and the refusal is
    /// the whole result. That is also the "0 of 6 executed" case, and on a
    /// host that installs the runner it is a regression that would otherwise
    /// pass green and silent. Where this is set, a run that proves nothing
    /// about confinement fails instead.
    const REQUIRED: &str = "DIET_REQUIRE_SANDBOX";

    /// A host path of this process's own (#316): a fixed one is shared by every
    /// verify on the machine, so another run's escape would fail this one's.
    fn outside_path() -> String {
        format!("/tmp/outside-{}", std::process::id())
    }

    /// The six seeded escapes, and the three controls without which they
    /// prove nothing.
    ///
    /// One test rather than nine, because the branch matters: on a host with
    /// the runner every escape is executed, and on a host without it the
    /// REFUSAL is what is asserted. A test that quietly skipped where the
    /// runner is absent would be a test that cannot fail, and this module is
    /// the last thing that should have one.
    ///
    /// A control here is the same command under a policy that permits it --
    /// same argv, same target, one declaration different. An earlier version
    /// of the network control ran a *different* command against a *different*
    /// address and exited `0` under either policy, which is a control that
    /// cannot fail and therefore is not one.
    #[test]
    // The Linux rows are kept as they were, inline; the macOS branch is the
    // dozen lines that took this past the limit.
    #[allow(clippy::too_many_lines)]
    fn the_seeded_escapes_are_denied_where_the_mechanism_is_present() {
        let policy = Policy::merged_usr();
        // The Mac's own mechanism on a Mac; the Linux one everywhere else, as
        // before, where a missing `bwrap` is the refusal asserted.
        let (platform, looked_for, expected) = match Platform::here() {
            Platform::MacOs => (Platform::MacOs, seatbelt::RUNNER, SEEDED_ESCAPES_MACOS),
            Platform::Linux | Platform::Other => (Platform::Linux, RUNNER, SEEDED_ESCAPES),
        };
        let confinement = match open_on(&policy, platform) {
            Err(unavailable) => {
                assert_eq!(
                    unavailable,
                    Unavailable::NoRunner {
                        mechanism: Isolation::Sandbox,
                        looked_for: looked_for.to_owned(),
                    },
                    "no runner, so the drive refuses; the escapes are not executed here"
                );
                assert!(
                    std::env::var_os(REQUIRED).is_none(),
                    "`{REQUIRED}` is set, so this is a host where the sandbox is \
                     required, and `{looked_for}` is not on it: 0 of {expected} \
                     seeded escapes were executed. The refusal asserted above is \
                     correct and is not the point -- a run that proves nothing \
                     about confinement must not be a green run here."
                );
                return;
            }
            Ok(confinement) => confinement,
        };

        let ground = Ground::make("escapes");
        if platform == Platform::MacOs {
            assert_eq!(
                the_macos_rows(&confinement, &policy, &ground),
                SEEDED_ESCAPES_MACOS,
                "every macOS row ran against Seatbelt and was denied, judged by its \
                 exit and what reached the host"
            );
            return;
        }
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

        let mut denied = 0_usize;

        // Row two: a write outside the tree, to a host path of our own (#316).
        let outside = run(&["sh", "-c", &format!("echo x > {}", outside_path())]);
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
            !std::path::Path::new(&outside_path()).exists(),
            "and nothing reached the host"
        );
        denied += 1;

        // Rows two-b and two-c: the tmpfs mounts a single `--remount-ro /`
        // does not reach. `--dev` builds a separate mount and `/dev/shm` a
        // further one inside it, so one remount of `/` left BOTH writable
        // while the advisory told the model the working tree was the only
        // writable path. The write succeeded and the file evaporated with
        // the sandbox: this module's own name for the outcome worse than a
        // denial, produced by this module.
        for scratch in ["/dev/pwn", "/dev/shm/pwn"] {
            let line = format!("echo x > {scratch}");
            let wrote = run(&["sh", "-c", &line]);
            assert_ne!(wrote.exit, Some(0), "the write to {scratch} was denied");
            assert!(
                wrote
                    .denials()
                    .iter()
                    .any(|denial| denial.kind == DenialKind::ReadOnly),
                "and recorded as read-only, not invented: {:?}",
                wrote.stderr
            );
            denied += 1;
        }

        // The control those two need. Closing `/dev` by breaking it would
        // pass both rows above and leave every ordinary command unable to
        // open a device it is supposed to have.
        let devices = run(&[
            "sh",
            "-c",
            "echo x > /dev/null && test -c /dev/null && test -d /dev/shm && echo ok",
        ]);
        assert_eq!(devices.exit, Some(0), "{}", devices.stderr);
        assert_eq!(
            devices.stdout.trim(),
            "ok",
            "read-only, not absent: the devices are still there and still usable"
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
        denied += 1;

        denied += the_network_rows(&confinement, &policy, &ground.tree);

        assert_eq!(
            denied, SEEDED_ESCAPES,
            "every seeded escape ran against the real mechanism and was denied. \
             This is the census, and it is an assertion because a printed one is \
             discarded by libtest on the passing run that is the whole point."
        );
    }

    /// A runner that is not a sandbox, so the RECORD can be checked on a host
    /// that has no sandbox.
    ///
    /// It drops everything up to `--` and executes what follows. It confines
    /// nothing and no test here pretends otherwise: what it stands in for is
    /// the `Confinement::Sandbox` *arm*, so that the provenance the record
    /// claims -- the caller's argv, the composed argv, the mechanism, the
    /// declared network, the child's own exit and streams -- is asserted
    /// against a run that actually happened rather than against constants.
    ///
    /// Before this existed, gutting [`Confinement::run`] to write
    /// `Isolation::None`, `Network::Host` and `confined: argv.to_vec()` left
    /// every test in the module passing.
    fn stand_in_runner(at: &std::path::Path) -> Bubblewrap {
        fs::write(
            at,
            "#!/bin/sh\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
        )
        .expect("a stand-in runner");
        fs::set_permissions(at, fs::Permissions::from_mode(0o755)).expect("it is executable");
        Bubblewrap::at(at.to_path_buf())
    }

    #[test]
    fn a_command_the_sandbox_could_not_execute_is_not_the_sandbox_failing() {
        // The mirror of the fabrication above, and just as fabricated. The
        // runner uses the same prefix and the same exit status for a command
        // it could not execute -- AFTER building the sandbox perfectly well --
        // and that happens constantly: the default policy binds `/usr` and
        // `/etc`, so a toolchain in `/opt`, `/usr/local` or a home directory
        // is simply not in there. Read as a setup failure it destroyed the
        // exit status, the output and the real diagnosis, and told the caller
        // the sandbox had not started about a sandbox that had.
        let runner = Bubblewrap::at(PathBuf::from("bwrap"));
        assert_eq!(
            runner.setup_failure("bwrap: execvp cargo: No such file or directory\n", Some(1)),
            None,
            "the sandbox was built; `cargo` is what is missing, and that is the \
             command's result to report"
        );
        assert_eq!(
            runner.setup_failure("bwrap: Can't mkdir /dev: Read-only file system\n", Some(1)),
            Some("bwrap: Can't mkdir /dev: Read-only file system".to_owned()),
            "while a real setup failure is still one: the exclusion is the \
             `execvp` prefix, not the whole discriminator"
        );
    }

    #[test]
    fn a_working_tree_that_cannot_mean_one_thing_is_refused_before_anything_runs() {
        let ground = Ground::make("worktree");
        let plain = Confinement::Unconfined;

        // Resolved once by the harness and again inside the sandbox: a
        // relative `repo` under `/w/repo` mounts `/w/repo/repo` OVER the tree
        // it was meant to bind, and the command writes into a directory that
        // is not the one the record names.
        let relative = plain
            .run(&Policy::unconfined(), Path::new("repo"), &argv(&["true"]))
            .expect_err("a relative working tree is refused");
        assert!(
            matches!(relative, NotRun::Worktree { ref path, why } if path == "repo" && why.contains("absolute")),
            "and refused for BEING relative, not for happening not to exist -- \
             the two arrive at the same variant and mean different things: \
             {relative:?}"
        );
        assert!(
            relative.to_string().contains("repo"),
            "and the refusal names it: {relative}"
        );

        let absent = plain
            .run(
                &Policy::unconfined(),
                &ground.outside.join("not-there"),
                &argv(&["true"]),
            )
            .expect_err("a working tree that is not a directory is refused");
        assert!(matches!(absent, NotRun::Worktree { .. }), "{absent:?}");

        assert!(
            plain
                .run(&Policy::unconfined(), &ground.tree, &argv(&["true"]))
                .is_ok(),
            "and an absolute directory that exists runs, or this is a refusal \
             of everything rather than of the two shapes that cannot mean one \
             thing"
        );
    }

    #[test]
    fn an_empty_command_is_refused_rather_than_running_the_runner_alone() {
        // Under `Sandbox` the COMPOSED vector is never empty, so a guard on
        // the composed one never fired here: the runner ran with no command
        // at all, printed sixty lines of its usage text, exited 1, and that
        // was banked as the command's own standard error and status.
        let ground = Ground::make("empty");
        let runner = Confinement::Sandbox(Backend::Bubblewrap(stand_in_runner(
            &ground.outside.join("stand-in"),
        )));
        let refusal = runner
            .run(&Policy::merged_usr(), &ground.tree, &[])
            .expect_err("no command is not a command");
        assert!(matches!(refusal, NotRun::Nothing), "{refusal:?}");

        assert!(
            Confinement::Unconfined
                .run(&Policy::unconfined(), &ground.tree, &[])
                .is_err(),
            "and unconfined too, where the guard did fire"
        );
    }

    /// The retry across ETXTBSY, on any platform: busy twice and then run
    /// is a run, on the third try; busy throughout is refused within the
    /// bound; anything else is reported at once (#88).
    #[test]
    fn a_busy_runner_is_retried_within_a_bound_and_nothing_else_is() {
        use std::cell::Cell;
        use std::io;
        use std::os::unix::process::ExitStatusExt as _;
        let ran = || {
            Ok(process::Output {
                status: process::ExitStatus::from_raw(0),
                stdout: Vec::new(),
                stderr: Vec::new(),
            })
        };
        let busy = || io::Error::from(io::ErrorKind::ExecutableFileBusy);

        let tries = Cell::new(0);
        let output = super::output_of(|| {
            tries.set(tries.get() + 1);
            if tries.get() < 3 { Err(busy()) } else { ran() }
        });
        assert!(
            output.is_ok() && tries.get() == 3,
            "{output:?} after {}",
            tries.get()
        );

        let started = std::time::Instant::now();
        let refused = super::output_of(|| Err(busy())).expect_err("busy throughout");
        assert_eq!(refused.kind(), io::ErrorKind::ExecutableFileBusy);
        assert!(
            started.elapsed() >= super::BUSY_PATIENCE,
            "it waited the bound"
        );
        assert!(
            started.elapsed() < super::BUSY_PATIENCE * 4,
            "and no longer"
        );

        let tries = Cell::new(0);
        let refused = super::output_of(|| {
            tries.set(tries.get() + 1);
            Err(io::Error::from(io::ErrorKind::NotFound))
        })
        .expect_err("not found");
        assert_eq!((refused.kind(), tries.get()), (io::ErrorKind::NotFound, 1));
    }

    /// #88: Linux refuses to execute a file that is still open for writing
    /// (ETXTBSY), and in a test process another thread's `fork` can hold a
    /// freshly written runner's write descriptor until that child execs. A
    /// runner held open across the run, then closed, must still run.
    /// Linux only: macOS executes a file open for writing, so this cannot
    /// fail there.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_runner_still_open_for_writing_runs_once_it_is_closed() {
        let ground = Ground::make("busy");
        let at = ground.outside.join("stand-in");
        let runner = Confinement::Sandbox(Backend::Bubblewrap(stand_in_runner(&at)));
        let held = fs::OpenOptions::new()
            .write(true)
            .open(&at)
            .expect("held for writing");
        let closer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            drop(held);
        });
        let ran = runner.run(
            &Policy::merged_usr(),
            &ground.tree,
            &argv(&["sh", "-c", "exit 0"]),
        );
        closer.join().expect("the closer");
        assert!(ran.is_ok(), "{ran:?}");
    }

    #[test]
    fn the_record_is_read_from_the_run_and_not_written_down_as_a_constant() {
        let ground = Ground::make("record");
        let runner = Confinement::Sandbox(Backend::Bubblewrap(stand_in_runner(
            &ground.outside.join("stand-in"),
        )));
        let policy = Policy::merged_usr();
        let command = argv(&["sh", "-c", "echo out; echo err >&2; exit 7"]);

        let ran = runner
            .run(&policy, &ground.tree, &command)
            .expect("the stand-in ran");

        assert_eq!(ran.argv, command, "the caller's command, verbatim");
        assert_eq!(
            ran.confined,
            runner.compose(&policy, &ground.tree, &command),
            "and the composed vector beside it, so the record says what was \
             actually executed rather than what was asked for"
        );
        assert_ne!(
            ran.confined, ran.argv,
            "which under a sandbox is never the same vector"
        );
        assert_eq!(
            ran.isolation,
            Isolation::Sandbox,
            "the mechanism is read from the confinement that ran it"
        );
        assert_eq!(
            ran.network,
            Network::None,
            "and the network from the policy it ran under"
        );
        assert_eq!(ran.exit, Some(7), "the child's own status");
        assert_eq!(ran.stdout.trim(), "out");
        assert_eq!(ran.stderr.trim(), "err");

        // The network is READ, not assumed: the same runner under the other
        // declaration banks the other word.
        let shared = runner
            .run(
                &Policy {
                    network: Network::Host,
                    ..policy.clone()
                },
                &ground.tree,
                &argv(&["true"]),
            )
            .expect("the stand-in ran");
        assert_eq!(shared.network, Network::Host);
        assert_eq!(shared.exit, Some(0));

        // And so is the mechanism: unconfined banks `none` and a confined
        // vector equal to the command, because that is what happened.
        let plain = Confinement::Unconfined
            .run(&Policy::unconfined(), &ground.tree, &argv(&["true"]))
            .expect("it ran");
        assert_eq!(plain.isolation, Isolation::None);
        assert_eq!(plain.confined, plain.argv);
    }

    /// The network rows of the seeded-escape test, and their control.
    ///
    /// A function rather than four more inline paragraphs so the one test
    /// above stays readable; it is called from exactly one place and the
    /// branch on the runner's presence is still made there, once.
    fn the_network_rows(
        confinement: &Confinement,
        policy: &Policy,
        tree: &std::path::Path,
    ) -> usize {
        let run = |declared: &Policy, parts: &[String]| {
            confinement
                .run(declared, tree, parts)
                .expect("the sandbox set up")
        };

        // Row three: a network call under `network = none`.
        let reached = run(
            policy,
            &argv(&[
                "python3",
                "-c",
                "import socket; socket.create_connection(('1.1.1.1', 80), 2)",
            ]),
        );
        assert_ne!(reached.exit, Some(0), "the call was denied");
        // Typed under `bwrap` only. Seatbelt's refusal is `EPERM`, which is
        // ambiguous by construction, so the macOS rows are judged by the exit
        // and what reached the host (#299, ruling 8).
        if matches!(confinement, Confinement::Sandbox(Backend::Bubblewrap(_))) {
            assert!(
                reached
                    .denials()
                    .iter()
                    .any(|denial| denial.kind == DenialKind::Network),
                "and recorded as a network denial: {:?}",
                reached.stderr
            );
        }

        // Control two: THE SAME COMMAND, against THE SAME address, with the
        // network declared. It is the only thing that tells "the policy
        // denied it" from "the command cannot reach anything anyway", and it
        // has to be the same command or it tells neither.
        //
        // The address is a listening socket on this host's loopback rather
        // than somewhere on the internet, so the control needs no network of
        // its own to prove that the sandbox has one. Nothing accepts on it:
        // the kernel completes the handshake for a listening socket whether
        // or not anybody calls `accept`, and a completed handshake is the
        // whole claim.
        let door = TcpListener::bind(("127.0.0.1", 0)).expect("a loopback door to knock on");
        // Spelled through the type rather than as a method call on the
        // binding, which the hygiene table reads as an internal hostname. A
        // false positive in the gate, disclosed on the PR rather than patched
        // from here -- the pattern table is not this seat's file.
        let port = TcpListener::local_addr(&door)
            .expect("the door has an address")
            .port();
        let knock = argv(&[
            "python3",
            "-c",
            &format!(
                "import socket; socket.create_connection(('127.0.0.1', {port}), 2); print('reached')"
            ),
        ]);

        let refused = run(policy, &knock);
        assert_ne!(
            refused.exit,
            Some(0),
            "under `network = none` the sandbox's loopback is its own and the \
             host's door is not on it: {:?}",
            refused.stderr
        );

        let declared = Policy {
            network: Network::Host,
            ..policy.clone()
        };
        let shared = run(&declared, &knock);
        assert_eq!(shared.exit, Some(0), "{}", shared.stderr);
        assert_eq!(
            shared.stdout.trim(),
            "reached",
            "and with the network declared the same knock on the same door is \
             answered: {:?}",
            shared.stderr
        );
        drop(door);

        // The two escapes this function executed and saw denied: the call to
        // an address off this host, and the knock on the host's own loopback
        // door. The control immediately above is not one of them -- it is
        // the row that had to SUCCEED, and counting it would inflate the
        // census with a case that proves the opposite thing.
        2
    }

    /// The macOS rows of the seeded-escape test, against Seatbelt.
    ///
    /// Judged by the exit and by what reached the host, never by a typed
    /// denial: Seatbelt's refusal is `EPERM`, which also arises with no
    /// sandbox at all (#299, ruling 8). Child inheritance comes FIRST, because
    /// a profile that held for the command and not for what it started would
    /// make every row after it a statement about one process.
    fn the_macos_rows(confinement: &Confinement, policy: &Policy, ground: &Ground) -> usize {
        let run = |declared: &Policy, parts: &[String]| {
            confinement
                .run(declared, &ground.tree, parts)
                .expect("the sandbox set up")
        };
        // Read once and removed, so a row that fails leaves nothing behind on
        // the host for the next run to trip over.
        let reached_the_host = |path: &Path| {
            let there = path.exists();
            let _ = fs::remove_file(path);
            there
        };
        let outside = format!("/private/tmp/outside-{}", process::id());
        let mut denied = 0_usize;

        // Row one: a grandchild, through `sh`, `python3` and `subprocess`,
        // writing outside the tree. Its exit is passed back up the chain.
        let child = format!("{outside}-child");
        let line = format!(
            "python3 -c \"import subprocess, sys; \
             sys.exit(subprocess.call(['sh', '-c', 'echo x > {child}']))\""
        );
        let grandchild = run(policy, &argv(&["sh", "-c", &line]));
        assert_ne!(
            grandchild.exit,
            Some(0),
            "the grandchild's write was denied: {:?}",
            grandchild.stderr
        );
        assert!(
            !reached_the_host(Path::new(&child)),
            "and nothing reached the host"
        );
        denied += 1;

        // And a double-forked writer that outlives the command that started
        // it. The command itself succeeds; the host is read after a wait.
        let daemon = format!("{outside}-daemon");
        let line = format!("( ( sleep 1; echo x > {daemon} ) & ) & echo spawned");
        let forked = run(policy, &argv(&["sh", "-c", &line]));
        assert_eq!(forked.exit, Some(0), "{}", forked.stderr);
        assert_eq!(forked.stdout.trim(), "spawned");
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert!(
            !reached_the_host(Path::new(&daemon)),
            "the orphan's late write was denied too: {:?}",
            forked.stderr
        );
        denied += 1;

        // Control one. Without it, a profile that denied everything would
        // pass every row.
        let inside = run(
            policy,
            &argv(&[
                "sh",
                "-c",
                "echo written > ./in-tree.txt && cat ./in-tree.txt",
            ]),
        );
        assert_eq!(inside.exit, Some(0), "{}", inside.stderr);
        assert_eq!(inside.stdout.trim(), "written");
        assert!(
            ground.tree.join("in-tree.txt").is_file(),
            "and the write reached the real tree"
        );

        // A write outside the tree.
        let wrote = run(policy, &argv(&["sh", "-c", &format!("echo x > {outside}")]));
        assert_ne!(wrote.exit, Some(0), "the write was denied");
        assert!(
            !reached_the_host(Path::new(&outside)),
            "and nothing reached the host"
        );
        denied += 1;

        // A write into a second directory outside the tree: the tree's
        // sibling under `$TMPDIR`, standing in for a home directory (the
        // real one is not written to prove it cannot be).
        let home = ground.outside.join("escape");
        let wrote = run(
            policy,
            &argv(&["sh", "-c", &format!("echo x > {}", home.display())]),
        );
        assert_ne!(wrote.exit, Some(0), "the write was denied");
        assert!(!reached_the_host(&home), "and nothing reached the host");
        denied += 1;

        // A secret in that second directory.
        let secret = ground.outside.join("secret");
        let read = run(policy, &argv(&["cat", &secret.to_string_lossy()]));
        assert_ne!(read.exit, Some(0), "the secret was not readable");
        assert!(
            !read.stdout.contains("s3cret"),
            "and did not leak: {:?}",
            read.stdout
        );
        assert!(secret.is_file(), "though it is right there on the host");
        denied += 1;

        // 1.1.1.1 under `none`, and the knock on a loopback port nothing
        // declared, with the same knock under `host` as their control.
        denied += the_network_rows(confinement, policy, &ground.tree);

        let counted = denied
            + the_declared_loopback_rows(confinement, policy, &ground.tree)
            + the_declared_listener_rows(confinement, policy, &ground.tree)
            + the_host_network_rows(confinement, policy, &ground.tree)
            + the_orphan_rows(confinement, policy, &ground.tree)
            + the_permissive_rows(confinement, policy, ground);
        confinement.end_session();
        counted
    }

    /// Under `network = "host"`, a Unix-domain socket on the host is NOT
    /// reachable (#299 review, F1: `network*` let the Docker daemon answer).
    /// The socket is one this test listens on, in a directory of its own
    /// under `/private/tmp` (short, because `sun_path` is 104 bytes), outside
    /// the tree. One escape.
    fn the_host_network_rows(
        confinement: &Confinement,
        policy: &Policy,
        tree: &std::path::Path,
    ) -> usize {
        let dir = PathBuf::from(format!("/private/tmp/diet-unix-{}", process::id()));
        fs::create_dir_all(&dir).expect("a directory for the socket");
        let at = dir.join("s");
        let _ = fs::remove_file(&at);
        let door = std::os::unix::net::UnixListener::bind(&at).expect("a host-side socket");
        let host = Policy {
            network: Network::Host,
            ..policy.clone()
        };
        let line = format!(
            "import socket, sys\n\
             s = socket.socket(socket.AF_UNIX)\n\
             try:\n    s.connect('{}')\n\
             except PermissionError:\n    sys.exit(13)\n\
             print('reached')",
            at.display()
        );
        let knocked = confinement
            .run(&host, tree, &argv(&["python3", "-c", &line]))
            .expect("the sandbox set up");
        drop(door);
        let _ = fs::remove_dir_all(&dir);
        assert_eq!(
            knocked.exit,
            Some(13),
            "a Unix socket on the host is refused under `host`: {:?} {:?}",
            knocked.stdout,
            knocked.stderr
        );
        1
    }

    /// The two disclosed equipment limits, measured (#299, 5981374872).
    ///
    /// A double-forked orphan outlives its command: its late write outside
    /// the tree is still REFUSED (one escape), and under a declared port it
    /// can still CONNECT after the command returned (asserted as measured:
    /// `orphan_control = "none"`). And a command reads another process's argv
    /// AND ENVIRONMENT through `sysctl` (`KERN_PROCARGS2`; `ps` itself is
    /// refused): asserted as measured, not an escape, and disclosed as
    /// `process_env_visible = "same_user"`; three profile denies tried
    /// against it failed (`seatbelt.rs`). The drive's own key is not there to
    /// read, because the drive re-executes scrubbed: one escape.
    fn the_orphan_rows(
        confinement: &Confinement,
        policy: &Policy,
        tree: &std::path::Path,
    ) -> usize {
        let door = TcpListener::bind(("127.0.0.1", 0)).expect("a declared door");
        let port = TcpListener::local_addr(&door)
            .expect("the door has an address")
            .port();
        door.set_nonblocking(true)
            .expect("a door that does not block");
        let declared = Policy {
            loopback: vec![port],
            ..policy.clone()
        };
        let late = format!("/private/tmp/outside-{}-orphan", process::id());
        let line = format!(
            "( ( sleep 1; echo x > {late}; \
             python3 -c \"import socket; socket.create_connection(('127.0.0.1', {port}), 2)\" \
             ) > /dev/null 2>&1 & ) & echo spawned"
        );
        let started = confinement
            .run(&declared, tree, &argv(&["sh", "-c", &line]))
            .expect("the sandbox set up");
        assert_eq!(started.exit, Some(0), "{}", started.stderr);
        assert_eq!(
            started.stdout.trim(),
            "spawned",
            "the command returned first"
        );
        let mut connected = false;
        for _ in 0..60 {
            if door.accept().is_ok() {
                connected = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        let wrote = Path::new(&late).exists();
        let _ = fs::remove_file(&late);
        assert!(
            !wrote,
            "the orphan's late write outside the tree was refused"
        );
        assert!(
            connected,
            "measured: the orphan, after its command returned, still reached the \
             declared port -- disclosed as `orphan_control = \"none\"`"
        );

        1 + the_process_table_rows(confinement, policy, tree)
    }

    /// The process table (#299, ruling 5983673467, H), measured. One escape:
    /// the scrubbed drive's key is absent; another process's environment is
    /// readable, asserted as measured and not counted.
    fn the_process_table_rows(
        confinement: &Confinement,
        policy: &Policy,
        tree: &std::path::Path,
    ) -> usize {
        // The process table (#299, ruling 5983673467, H): from inside, a
        // command reads the argv AND THE ENVIRONMENT of a same-user process
        // that is not an Apple platform binary -- measured, and disclosed as
        // `process_env_visible = "same_user"`. And the drive itself, run
        // through the re-exec builder, holds no key there to read.
        let marker = format!("diet-argv-marker-{}", process::id());
        let key = format!("DIET-FAKE-ENDPOINT-KEY-{}", process::id());
        let sleeper = |command: &mut process::Command| {
            command
                .stdin(process::Stdio::null())
                .stdout(process::Stdio::null())
                .stderr(process::Stdio::null())
                .spawn()
                .expect("a process to read")
        };
        let python_sleep = ["-c", "import time; time.sleep(30)", &marker];
        let mut other = sleeper(
            process::Command::new("python3")
                .args(python_sleep)
                .env("DIET_FAKE_ENDPOINT_KEY", &key),
        );
        let mut parent: Vec<_> = std::env::vars_os().collect();
        parent.push(("DIET_FAKE_ENDPOINT_KEY".into(), key.clone().into()));
        let mut drive = sleeper(&mut super::scrubbed_drive(
            Path::new("python3"),
            &python_sleep.map(std::ffi::OsString::from),
            &policy.environment,
            parent,
        ));
        std::thread::sleep(std::time::Duration::from_millis(500));
        let read = |pid: u32| {
            let line = format!(
                "import ctypes\n\
                 libc = ctypes.CDLL(None)\n\
                 mib = (ctypes.c_int * 3)(1, 49, {pid})\n\
                 size = ctypes.c_size_t(262144); buf = ctypes.create_string_buffer(262144)\n\
                 assert libc.sysctl(mib, 3, buf, ctypes.byref(size), None, 0) == 0\n\
                 print(buf.raw[:size.value].replace(b'\\0', b' ').decode(errors='replace'))"
            );
            confinement
                .run(policy, tree, &argv(&["python3", "-c", &line]))
                .expect("the sandbox set up")
        };
        let seen_other = read(other.id());
        let seen_drive = read(drive.id());
        for child in [&mut other, &mut drive] {
            let _ = child.kill();
            let _ = child.wait();
        }
        assert_eq!(seen_other.exit, Some(0), "{}", seen_other.stderr);
        assert!(
            seen_other.stdout.contains(&marker) && seen_other.stdout.contains(&key),
            "measured: another same-user process's argv AND environment are readable \
             from inside -- disclosed as `process_env_visible = \"same_user\"`, not an \
             escape: {:?}",
            seen_other.stdout
        );
        assert_eq!(seen_drive.exit, Some(0), "{}", seen_drive.stderr);
        assert!(
            seen_drive.stdout.contains(&marker) && !seen_drive.stdout.contains(&key),
            "and a drive re-executed through the scrub holds no key to read: {:?}",
            seen_drive.stdout
        );
        1
    }

    /// An undeclared loopback port BESIDE a declared one, and the declared
    /// one as its control, under Seatbelt: the allowance is the port, not the
    /// loopback. One escape.
    fn the_declared_loopback_rows(
        confinement: &Confinement,
        policy: &Policy,
        tree: &std::path::Path,
    ) -> usize {
        let run = |declared: &Policy, parts: &[String]| {
            confinement
                .run(declared, tree, parts)
                .expect("the sandbox set up")
        };
        let door = TcpListener::bind(("127.0.0.1", 0)).expect("a declared door");
        let other = TcpListener::bind(("127.0.0.1", 0)).expect("an undeclared door");
        let port_of = |listener: &TcpListener| {
            TcpListener::local_addr(listener)
                .expect("the door has an address")
                .port()
        };
        let declared = Policy {
            loopback: vec![port_of(&door)],
            ..policy.clone()
        };
        let knock = |port: u16| {
            argv(&[
                "python3",
                "-c",
                &format!(
                    "import socket; socket.create_connection(('127.0.0.1', {port}), 2); print('reached')"
                ),
            ])
        };
        let refused = run(&declared, &knock(port_of(&other)));
        assert_ne!(
            refused.exit,
            Some(0),
            "a port the regimen did not declare is not reachable: {:?}",
            refused.stdout
        );
        let answered = run(&declared, &knock(port_of(&door)));
        assert_eq!(answered.exit, Some(0), "{}", answered.stderr);
        assert_eq!(
            answered.stdout.trim(),
            "reached",
            "and the declared one is answered, under the same policy"
        );

        1
    }

    /// What a declared port admits besides an outbound knock (#299, ruling
    /// 5976914097: bind, inbound and outbound, on `localhost` only), under
    /// Seatbelt. Three escapes and one control.
    ///
    /// **What is NOT a row, and why.** Seatbelt's `localhost` is every
    /// address of this host, and its grammar admits no other host ("host must
    /// be * or localhost", exit 65): a bind of the declared port on `0.0.0.0`
    /// or on the host's LAN address, and a connect to the LAN address, were
    /// measured ADMITTED. A row asserting them refused would fail on every
    /// Mac. Ruled acceptable and disclosed (#299, 5977176035): the equipment
    /// row says `loopback_scope = "host"`, and `NETWORK_LOOPBACK`'s docs say
    /// so.
    fn the_declared_listener_rows(
        confinement: &Confinement,
        policy: &Policy,
        tree: &std::path::Path,
    ) -> usize {
        let free = || {
            let door = TcpListener::bind(("127.0.0.1", 0)).expect("a port to free");
            TcpListener::local_addr(&door)
                .expect("the door has an address")
                .port()
        };
        let (port, undeclared) = (free(), free());
        let declared = Policy {
            loopback: vec![port],
            ..policy.clone()
        };
        let run = |parts: &[String]| {
            confinement
                .run(&declared, tree, parts)
                .expect("the sandbox set up")
        };
        let listen = |at: u16| {
            argv(&[
                "python3",
                "-c",
                &format!(
                    "import socket; s = socket.socket(); \
                     s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1); \
                     s.bind(('127.0.0.1', {at})); s.listen(1); s.settimeout(6); \
                     c, _ = s.accept(); c.sendall(b'hi'); print('accepted')"
                ),
            ])
        };

        // A bare bind, no listen, on a port nothing declared: the bind rule
        // alone, on the real mechanism (#299 review, F9).
        let bare = run(&argv(&[
            "python3",
            "-c",
            &format!(
                "import socket, sys\n\
                 try:\n    socket.socket().bind(('127.0.0.1', {undeclared}))\n\
                 except PermissionError:\n    sys.exit(13)"
            ),
        ]));
        assert_eq!(
            bare.exit,
            Some(13),
            "a bind on an undeclared port is refused at the bind: {:?}",
            bare.stderr
        );

        // A listener on a port nothing declared.
        let bound = run(&listen(undeclared));
        assert_ne!(
            bound.exit,
            Some(0),
            "a listener on an undeclared port is refused: {:?}",
            bound.stdout
        );

        // The declared port, on an address off this host. Exit 13 is the
        // refusal at the connect; anything else (a timeout, an unreachable
        // network) means the profile let the connect leave.
        let off_host = run(&argv(&[
            "python3",
            "-c",
            &format!(
                "import socket, sys\n\
                 try:\n    socket.create_connection(('1.1.1.1', {port}), 2)\n\
                 except PermissionError:\n    sys.exit(13)\n\
                 except OSError:\n    sys.exit(4)"
            ),
        ]));
        assert_eq!(
            off_host.exit,
            Some(13),
            "the declared port is admitted on this host only: {:?}",
            off_host.stderr
        );

        // Control: a listener on the declared port, reached from OUTSIDE.
        let knocker = std::thread::spawn(move || {
            for _ in 0..50 {
                if let Ok(mut door) = std::net::TcpStream::connect(("127.0.0.1", port)) {
                    let mut said = String::new();
                    let _ = std::io::Read::read_to_string(&mut door, &mut said);
                    return said;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            String::new()
        });
        let listened = run(&listen(port));
        let heard = knocker.join().expect("the knocker");
        assert_eq!(listened.exit, Some(0), "{}", listened.stderr);
        assert_eq!(listened.stdout.trim(), "accepted");
        assert_eq!(heard, "hi", "and the host reached the listener inside");

        3
    }

    // -----------------------------------------------------------------------
    // Seatbelt: the profile, the parameters, the digest, the open
    // -----------------------------------------------------------------------

    /// A policy with one of each parameter, so every numbered slot appears.
    fn shaped(network: Network, loopback: Vec<u16>) -> Policy {
        Policy {
            network,
            readable: vec!["/nonexistent-toolchain".to_owned()],
            loopback,
            // One secret, named here, so the composed argv does not depend
            // on this host's `HOME`.
            secrets: vec!["/nonexistent-secret".to_owned()],
            ..Policy::merged_usr()
        }
    }

    /// The `(allow file-write* …)` rule of `profile`, whole.
    fn rule<'a>(profile: &'a str, head: &str) -> Vec<&'a str> {
        profile
            .split("\n(")
            .filter(|rule| rule.trim_start_matches('(').starts_with(head))
            .collect()
    }

    #[test]
    fn the_seatbelt_profile_is_exactly_what_the_policy_declares() {
        let runner = Confinement::Sandbox(Backend::Seatbelt(Seatbelt::at(PathBuf::from(
            "/usr/bin/sandbox-exec",
        ))));
        let composed = runner.compose(
            &Policy {
                writable: vec!["/nonexistent-writable".to_owned()],
                ..shaped(Network::None, vec![18766])
            },
            &PathBuf::from("/work/tree"),
            &argv(&["cargo", "test"]),
        );
        let profile = r#"(version 1)
(deny default)
(define (ancestors path)
  (let loop ((i (- (string-length path) 1)) (found (list (literal "/"))))
    (cond ((<= i 0) (apply require-any found))
          ((char=? (string-ref path i) #\/)
           (loop (- i 1) (cons (literal (substring path 0 i)) found)))
          (else (loop (- i 1) found)))))
(allow process-exec process-fork)
(allow signal (target same-sandbox))
(allow sysctl-read)
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo"))
(allow file-read-metadata
  (literal "/private") (literal "/private/var")
  (subpath "/private/var/select")
  (ancestors (param "WORKTREE"))
  (ancestors (param "READABLE_0"))
  (ancestors (param "WRITABLE_0")))
(allow file-read*
  (literal "/") (literal "/etc") (literal "/var") (literal "/tmp")
  (subpath "/usr") (subpath "/bin") (subpath "/sbin")
  (subpath "/System") (subpath "/Library")
  (subpath "/private/etc") (subpath "/private/var/db/dyld")
  (subpath "/private/var/db/timezone") (literal "/private/var/db/xcode_select_link")
  (subpath "/dev")
  (subpath (param "WORKTREE"))
  (subpath (param "READABLE_0"))
  (subpath (param "WRITABLE_0")))
(allow file-write*
  (subpath (param "WORKTREE"))
  (subpath (param "WRITABLE_0"))
  (literal "/dev/null"))
(allow file-ioctl (literal "/dev/null"))
(allow network-outbound (remote ip (string-append "localhost:" (param "PORT_0"))))
(allow network-bind (local ip (string-append "localhost:" (param "PORT_0"))))
(allow network-inbound (local ip (string-append "localhost:" (param "PORT_0"))))
(deny file-read* file-write*
  (subpath (param "SECRET_0")))
(deny file-read* file-write* (require-all (regex #"/\.env(\.[^/]*)?$") (require-not (regex #"/\.env\.(example|sample|template|dist)$")) (require-not (subpath (param "WORKTREE")))))
"#;
        assert_eq!(
            composed,
            argv(&[
                "/usr/bin/sandbox-exec",
                "-D",
                "WORKTREE=/work/tree",
                "-D",
                "READABLE_0=/nonexistent-toolchain",
                "-D",
                "WRITABLE_0=/nonexistent-writable",
                "-D",
                "PORT_0=18766",
                "-D",
                "SECRET_0=/nonexistent-secret",
                "-p",
                profile,
                "--",
                "cargo",
                "test",
            ])
        );
        assert!(
            !profile.contains("/work/tree") && !profile.contains("18766"),
            "no path and no port in the profile itself: they are the -D values"
        );
    }

    #[test]
    fn the_seatbelt_profile_writes_only_to_the_tree_and_dev_null() {
        for policy in [
            Policy::merged_usr(),
            shaped(Network::None, vec![1, 2]),
            shaped(Network::Host, Vec::new()),
        ] {
            let profile = Seatbelt::profile(&policy);
            assert_eq!(
                rule(&profile, "allow file-write*"),
                ["allow file-write*\n  (subpath (param \"WORKTREE\"))\n  \
                  (literal \"/dev/null\"))"],
                "one write rule, naming the tree and `/dev/null`: {profile}"
            );
            assert_eq!(
                profile.matches("allow file-write").count(),
                1,
                "and nothing else grants a write: {profile}"
            );
            assert!(
                !profile.contains("/dev/tty"),
                "not even the terminal, which a command run from one could write \
                 escape sequences to past its captured output: {profile}"
            );
        }
    }

    #[test]
    fn the_seatbelt_profile_opens_the_network_only_as_declared() {
        let none = Seatbelt::profile(&Policy::merged_usr());
        assert!(
            !none.contains("network"),
            "`none` with nothing declared allows no network at all: {none}"
        );

        let ports = shaped(Network::None, vec![18766, 18767]);
        let declared = Seatbelt::profile(&ports);
        let expected: Vec<String> = (0..2)
            .flat_map(|index| {
                let port = format!("(string-append \"localhost:\" (param \"PORT_{index}\"))");
                [
                    format!("allow network-outbound (remote ip {port}))"),
                    format!("allow network-bind (local ip {port}))"),
                    format!("allow network-inbound (local ip {port}))"),
                ]
            })
            .collect();
        let mut found: Vec<String> = rule(&declared, "allow network")
            .iter()
            .map(|rule| rule.trim_end().to_owned())
            .collect();
        found.sort();
        let mut sorted = expected.clone();
        sorted.sort();
        assert_eq!(
            found, sorted,
            "each declared port admits outbound, bind and inbound, on localhost and on \
             its own parameter, and nothing else is admitted: {declared}"
        );
        assert!(
            !declared.contains("localhost:*") && !declared.contains("\"*:"),
            "no wildcard port and no wildcard host: {declared}"
        );
        let composed = Seatbelt::at(PathBuf::from("sandbox-exec")).compose(
            &ports,
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        assert!(
            composed.contains(&"PORT_0=18766".to_owned())
                && composed.contains(&"PORT_1=18767".to_owned()),
            "and the ports themselves are the parameters: {composed:?}"
        );

        let host = Seatbelt::profile(&shaped(Network::Host, Vec::new()));
        assert_eq!(
            rule(&host, "allow network")
                .iter()
                .map(|rule| rule.trim_end())
                .collect::<Vec<_>>(),
            [
                "allow network-outbound (remote ip \"*:*\"))",
                "allow network-bind (local ip \"*:*\"))",
                "allow network-inbound (local ip \"*:*\"))",
                "allow network-outbound (literal \"/private/var/run/mDNSResponder\"))",
            ],
            "and `host` opens the IP network and DNS's socket: {host}"
        );
        assert!(
            !host.contains("network*")
                && host.matches("literal").count() == declared.matches("literal").count() + 1,
            "and no other Unix-domain socket: `network*` admitted the Docker daemon's: {host}"
        );
    }

    #[test]
    fn the_worktree_enters_seatbelt_canonical() {
        // Seatbelt's `subpath` matches the RESOLVED path, so a tree given
        // through a symlink would be denied to itself.
        let ground = Ground::make("canonical");
        let link = ground.outside.join("link");
        std::os::unix::fs::symlink(&ground.tree, &link).expect("a link to the tree");
        let real = fs::canonicalize(&ground.tree).expect("the tree resolves");
        assert_ne!(link, real, "the link is not already the canonical path");

        let composed = Seatbelt::at(PathBuf::from("sandbox-exec")).compose(
            &Policy::merged_usr(),
            &link,
            &argv(&["true"]),
        );
        assert_eq!(
            composed[1..3],
            ["-D".to_owned(), format!("WORKTREE={}", real.display())],
            "the canonical tree enters, and `confined` records it"
        );
    }

    /// A stand-in `sandbox-exec` in a directory of its own that exits
    /// `code`, for an open that runs anywhere.
    fn stand_in_seatbelt(dir: &Path, code: i32) -> PathBuf {
        fs::create_dir_all(dir).expect("a directory for the stand-in");
        let at = dir.join(seatbelt::RUNNER);
        fs::write(
            &at,
            format!("#!/bin/sh\necho 'sandbox-exec: unbound variable: bogus' >&2\nexit {code}\n"),
        )
        .expect("a stand-in runner");
        fs::set_permissions(&at, fs::Permissions::from_mode(0o755)).expect("it is executable");
        at
    }

    #[test]
    fn a_profile_seatbelt_refuses_at_open_is_unavailable_not_a_run() {
        let ground = Ground::make("validate");
        let refusing = ground.outside.join("refusing");
        stand_in_seatbelt(&refusing, 65);
        let refusal = open_in(
            &Policy::merged_usr(),
            Platform::MacOs,
            Some(refusing.as_os_str()),
        )
        .expect_err("a profile the runner refuses at open does not start the drive");
        assert!(
            matches!(&refusal, Unavailable::ProfileRefused { said } if said.contains("65")),
            "{refusal:?}"
        );
        assert!(
            refusal.to_string().contains("Seatbelt"),
            "and the refusal names Seatbelt: {refusal}"
        );

        // The positive control: the same open, with a runner that accepts.
        let accepting = ground.outside.join("accepting");
        let runner = stand_in_seatbelt(&accepting, 0);
        let opened = open_in(
            &Policy::merged_usr(),
            Platform::MacOs,
            Some(accepting.as_os_str()),
        )
        .expect("a profile the runner accepts opens");
        assert!(
            matches!(&opened, Confinement::Sandbox(Backend::Seatbelt(found)) if found.runner() == runner),
            "{opened:?}"
        );
    }

    #[test]
    fn ending_the_session_kills_what_its_commands_left_running() {
        // A stand-in Seatbelt runner, so this runs anywhere: what is tested is
        // the group each command leads and the kill at session end.
        let ground = Ground::make("session");
        let runner = Confinement::Sandbox(Backend::Seatbelt(Seatbelt::at(
            stand_in_runner(&ground.outside.join("seatbelt-stand-in"))
                .runner()
                .to_path_buf(),
        )));
        let started = runner
            .run(
                &Policy::merged_usr(),
                &ground.tree,
                &argv(&["sh", "-c", "sleep 30 > /dev/null 2>&1 & echo $!"]),
            )
            .expect("the stand-in ran");
        let pid = started.stdout.trim().to_owned();
        let alive = |pid: &str| {
            process::Command::new("kill")
                .args(["-0", pid])
                .stderr(process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        assert!(
            alive(&pid),
            "a backgrounded child is alive after its command returned: a server \
             one command starts must survive into the next"
        );

        runner.end_session();
        let mut gone = false;
        for _ in 0..50 {
            if !alive(&pid) {
                gone = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(gone, "and gone once the session ended (pid {pid})");
        assert!(
            matches!(&runner, Confinement::Sandbox(Backend::Seatbelt(seatbelt)) if seatbelt.groups().is_empty()),
            "and the session's groups are forgotten"
        );
    }

    #[test]
    fn the_seatbelt_profile_denies_secrets_after_every_allow() {
        let declared = Policy {
            reads: Reads::All,
            network: Network::Host,
            writable: vec!["/nonexistent-writable".to_owned()],
            secrets: vec!["/one".to_owned(), "/two".to_owned()],
            ..Policy::merged_usr()
        };
        let profile = Seatbelt::profile(&declared);
        let secrets = "(deny file-read* file-write*\n  (subpath (param \"SECRET_0\"))\n  \
                       (subpath (param \"SECRET_1\")))\n";
        let env_files = "(deny file-read* file-write* (require-all (regex #\"/\\.env(\\.[^/]*)?$\") \
                         (require-not (regex #\"/\\.env\\.(example|sample|template|dist)$\")) \
                         (require-not (subpath (param \"WORKTREE\")))))\n";
        let at = |needle: &str| {
            profile
                .find(needle)
                .unwrap_or_else(|| panic!("`{needle}` is not in {profile}"))
        };
        let last_allow = profile.rfind("(allow").expect("allows");
        assert!(
            at(secrets) > last_allow && at(env_files) > last_allow,
            "both denies, reads and writes alike, come AFTER every allow -- the later \
             rule wins in Seatbelt, so `sandbox_reads = \"all\"`, a readable path and a \
             writable dir cannot open a secret: {profile}"
        );
        assert!(profile.contains("(allow file-read*)\n"), "{profile}");

        // The defaults, and a regimen that can add and never remove.
        let defaults: Vec<String> = policy::DEFAULT_SECRETS
            .iter()
            .map(|path| (*path).to_owned())
            .collect();
        assert_eq!(Policy::merged_usr().secrets, defaults);
        assert_eq!(policy("sandbox_secrets = []\n").secrets, defaults);
        let added = policy("sandbox_secrets = [\"/srv/keys\", \"~/.kube\"]\n");
        assert_eq!(added.secrets[..defaults.len()], defaults[..]);
        assert_eq!(added.secrets[defaults.len()..], ["/srv/keys", "~/.kube"]);
    }

    #[test]
    fn a_writable_dir_enters_canonical_and_binds_writable_under_bwrap() {
        let ground = Ground::make("writable");
        let real = ground.outside.join("cache");
        fs::create_dir_all(&real).expect("a writable dir");
        let link = ground.outside.join("cache-link");
        std::os::unix::fs::symlink(&real, &link).expect("a link to it");
        let declared = Policy {
            writable: vec![link.to_string_lossy().into_owned()],
            ..Policy::merged_usr()
        };
        let composed = Seatbelt::at(PathBuf::from("sandbox-exec")).compose(
            &declared,
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        let canonical = fs::canonicalize(&real).expect("it resolves");
        assert!(
            composed.contains(&format!("WRITABLE_0={}", canonical.display())),
            "the declared dir itself, canonical, and not its parent: {composed:?}"
        );

        let bwrap = Bubblewrap::at(PathBuf::from("bwrap")).compose(
            &Policy {
                writable: vec!["/srv/cache".to_owned()],
                ..Policy::merged_usr()
            },
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        let at = |needle: &[&str]| {
            bwrap
                .windows(needle.len())
                .position(|window| window == needle)
                .unwrap_or_else(|| panic!("{needle:?} is not in {bwrap:?}"))
        };
        assert!(
            at(&["--bind", "/work/tree", "/work/tree"])
                < at(&["--bind", "/srv/cache", "/srv/cache"])
                && at(&["--bind", "/srv/cache", "/srv/cache"]) < at(&["--remount-ro", "/"]),
            "bound like the tree, after it and before the remounts: {bwrap:?}"
        );
    }

    #[test]
    fn sandbox_reads_all_refuses_to_start_on_linux() {
        let declared = policy("sandbox_reads = \"all\"\n");
        assert_eq!(declared.reads, Reads::All);
        let refusal = open_on(&declared, Platform::Linux)
            .expect_err("bwrap binds what it is told and cannot hide a secret set");
        assert_eq!(
            refusal,
            Unavailable::ReadsAllNotImplemented {
                on: Platform::Linux
            }
        );
        assert!(refusal.to_string().contains("sandbox_reads"), "{refusal}");
    }

    #[test]
    fn the_command_receives_only_the_environment_the_policy_names() {
        // `cargo test` gives this process `CARGO_MANIFEST_DIR`, which is in no
        // policy's list: the variable the clear must keep out.
        assert!(
            std::env::var_os("CARGO_MANIFEST_DIR").is_some(),
            "this test needs a variable in its own environment to keep out"
        );
        let ground = Ground::make("environment");
        let env = |policy: &Policy| {
            Confinement::Unconfined
                .run(policy, &ground.tree, &argv(&["env"]))
                .expect("it ran")
                .stdout
        };
        let scrubbed = env(&Policy::unconfined());
        assert!(
            !scrubbed.contains("CARGO_MANIFEST_DIR="),
            "an unlisted variable does not reach the command: {scrubbed}"
        );
        assert!(scrubbed.contains("PATH="), "a listed one does: {scrubbed}");
        let passed = env(&Policy {
            environment: vec!["PATH".to_owned(), "CARGO_MANIFEST_DIR".to_owned()],
            ..Policy::unconfined()
        });
        assert!(
            passed.contains("CARGO_MANIFEST_DIR="),
            "and one the regimen names does: {passed}"
        );

        assert_eq!(
            policy("env_passthrough = [\"NODE_ENV\"]\n").environment,
            ["PATH", "HOME", "LANG", "TERM", "TMPDIR", "NODE_ENV"]
        );
        for bad in [
            "env_passthrough = [\"\"]\n",
            "env_passthrough = [\"1X\"]\n",
            "env_passthrough = [\"A-B\"]\n",
        ] {
            assert!(
                matches!(
                    Policy::from_regimen(&regimen::parse(bad).expect("a regimen")),
                    Err(PolicyError::NotAnEnvironmentName(_))
                ),
                "{bad}"
            );
        }
        assert_eq!(
            Policy::from_regimen(
                &regimen::parse("sandbox_writable = [\"cache\"]\n").expect("a regimen")
            ),
            Err(PolicyError::NotAbsolute("cache".to_owned()))
        );
    }

    /// T1's regimen lines as ratified (#301 5981771715, shape at 5981651636),
    /// with placeholder values: what this module's keys must read them as.
    /// The cwd, the dev server's port and the source's location are the
    /// maintainer's and are not this module's keys; the allowlist and the
    /// pre-seeded set are #298's.
    #[test]
    fn t1s_ratified_regimen_lines_read_as_the_intended_policy() {
        // The maintainer's lines (#301 5981929426): tilde paths written
        // literally; the reference checkout's name is a placeholder here.
        let t1 = policy(
            "cwd = \"~/git/experiments/babylon-lite-minecraft-fps\"\n\
             isolation = \"sandbox\"\n\
             network = \"host\"\n\
             sandbox_reads = \"all\"\n\
             sandbox_writable = [\"~/git/reference/placeholder-repo\"]\n\
             env_passthrough = [\"npm_config_cache\"]\n",
        );
        assert_eq!(
            t1,
            Policy {
                isolation: Isolation::Sandbox,
                network: Network::Host,
                reads: Reads::All,
                writable: vec!["~/git/reference/placeholder-repo".to_owned()],
                loopback: Vec::new(),
                environment: ["PATH", "HOME", "LANG", "TERM", "TMPDIR", "npm_config_cache"]
                    .map(str::to_owned)
                    .to_vec(),
                ..Policy::merged_usr()
            },
            "the default secret set, the reference checkout writable, the default five \
             plus the npm cache's variable, and no loopback declaration: under `host` \
             the dev server's and the devtools' loopback ports are already reachable"
        );
        assert_eq!(
            t1.secrets,
            policy::DEFAULT_SECRETS
                .iter()
                .map(|path| (*path).to_owned())
                .collect::<Vec<_>>()
        );
        let composed = Seatbelt::at(PathBuf::from("sandbox-exec")).compose(
            &t1,
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        let home = std::env::var("HOME").expect("a HOME");
        let expected = format!(
            "WRITABLE_0={}",
            Path::new(&home)
                .join("git/reference/placeholder-repo")
                .display()
        );
        assert!(
            composed.contains(&expected),
            "and the reference checkout enters expanded from HOME exactly, \
             `{expected}`: {composed:?}"
        );
    }

    #[test]
    fn a_tilde_path_is_the_drives_home_and_nothing_else_relative_is_accepted() {
        let home = PathBuf::from(std::env::var_os("HOME").expect("this test needs a HOME"));
        assert_eq!(policy::home_expanded("~"), home);
        assert_eq!(policy::home_expanded("~/src/x"), home.join("src/x"));
        assert_eq!(
            policy::home_expanded("~user/x"),
            PathBuf::from("~user/x"),
            "another user's home is not this drive's"
        );
        assert_eq!(policy::home_expanded("/abs"), PathBuf::from("/abs"));

        let read = |text: &str| Policy::from_regimen(&regimen::parse(text).expect("a regimen"));
        for key in ["sandbox_readable", "sandbox_writable", "sandbox_secrets"] {
            assert!(
                read(&format!("{key} = [\"~\", \"~/src\"]\n")).is_ok(),
                "{key}"
            );
            for bad in ["~user/x", "src", "./src"] {
                assert_eq!(
                    read(&format!("{key} = [\"{bad}\"]\n")),
                    Err(PolicyError::NotAbsolute(bad.to_owned())),
                    "{key} = {bad}"
                );
            }
        }

        // Expanded where it is composed, under both runners.
        let declared = Policy {
            readable: vec!["~/nonexistent-toolchain".to_owned()],
            ..Policy::merged_usr()
        };
        let expected = home
            .join("nonexistent-toolchain")
            .to_string_lossy()
            .into_owned();
        let seatbelt = Seatbelt::at(PathBuf::from("sandbox-exec")).compose(
            &declared,
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        assert!(
            seatbelt.contains(&format!("READABLE_0={expected}")),
            "{seatbelt:?}"
        );
        let bwrap = Bubblewrap::at(PathBuf::from("bwrap")).compose(
            &declared,
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        assert!(
            bwrap
                .windows(3)
                .any(|window| window == ["--ro-bind", expected.as_str(), expected.as_str()]),
            "{bwrap:?}"
        );
    }

    /// A secret inside a writable dir, and a `.env` in one: shapes that no
    /// longer open (#299 review of 3f7cc1e, A and B), so the rename that read
    /// them under `3f7cc1e` (rc 0) cannot happen. Two escapes.
    fn the_refused_shapes_rows(permissive: &Policy, writable: &Path) -> usize {
        let mut denied = 0_usize;
        let parent = writable.join("config");
        fs::create_dir_all(parent.join("gh")).expect("a scratch secret");
        let refused = open_on(
            &Policy {
                secrets: vec![parent.join("gh").to_string_lossy().into_owned()],
                ..permissive.clone()
            },
            Platform::MacOs,
        );
        assert!(
            matches!(refused, Err(Unavailable::SecretUnderWritable { .. })),
            "a writable dir holding a secret's parent does not open: {refused:?}"
        );
        denied += 1;
        let _ = fs::remove_dir_all(&parent);
        fs::create_dir_all(writable.join("deep")).expect("a dir");
        fs::write(writable.join("deep").join(".env"), "TOKEN=w\n").expect("a writable .env");
        let refused = open_on(permissive, Platform::MacOs);
        assert!(
            matches!(refused, Err(Unavailable::EnvFileDeclared { .. })),
            "a writable dir holding a `.env` does not open: {refused:?}"
        );
        denied += 1;
        let _ = fs::remove_dir_all(writable.join("deep"));
        denied
    }

    /// Under a permissive policy -- `sandbox_reads = "all"`, `network =
    /// "host"`, a declared writable dir -- what still holds (#29, planning
    /// 5981447912 and 5981606817). Seven escapes; the home read, the write
    /// into the declared dir and the `.env` inside the tree are controls.
    fn the_permissive_rows(confinement: &Confinement, policy: &Policy, ground: &Ground) -> usize {
        let held = ground.outside.join("held");
        let writable = ground.outside.join("writable");
        let undeclared = ground.outside.join("undeclared");
        for dir in [&held, &writable, &undeclared] {
            fs::create_dir_all(dir).expect("a scratch dir");
        }
        fs::write(held.join("key"), "k3y\n").expect("a scratch secret");
        fs::write(ground.outside.join(".env"), "TOKEN=outside\n").expect("an outside .env");
        fs::write(ground.tree.join(".env"), "TOKEN=inside\n").expect("an inside .env");
        let mut secrets = policy.secrets.clone();
        secrets.push(held.to_string_lossy().into_owned());
        let permissive = Policy {
            reads: Reads::All,
            network: Network::Host,
            writable: vec![writable.to_string_lossy().into_owned()],
            secrets,
            ..policy.clone()
        };
        let run = |parts: &[&str]| {
            confinement
                .run(&permissive, &ground.tree, &argv(parts))
                .expect("the sandbox set up")
        };
        let mut denied = 0_usize;

        let home = run(&["sh", "-c", "ls \"$HOME\" > /dev/null"]);
        assert_eq!(
            home.exit,
            Some(0),
            "measured: a home read is allowed: {}",
            home.stderr
        );

        let secret = run(&["cat", &held.join("key").to_string_lossy()]);
        assert!(
            secret.exit != Some(0) && !secret.stdout.contains("k3y"),
            "a read under a secret path is refused, even under reads = all"
        );
        denied += 1;

        let out = format!("/private/tmp/outside-{}-permissive", process::id());
        let wrote = run(&["sh", "-c", &format!("echo x > {out}")]);
        let reached = Path::new(&out).exists();
        let _ = fs::remove_file(&out);
        assert!(
            wrote.exit != Some(0) && !reached,
            "a write outside the tree and the declared dirs is refused"
        );
        denied += 1;

        denied += the_host_network_rows(confinement, &permissive, &ground.tree);

        let into = writable.join("w.txt");
        let control = run(&["sh", "-c", &format!("echo w > {}", into.display())]);
        assert_eq!(control.exit, Some(0), "{}", control.stderr);
        assert!(
            into.is_file(),
            "a write into the declared writable dir is allowed and lands"
        );

        let sibling = undeclared.join("w.txt");
        let refused = run(&["sh", "-c", &format!("echo w > {}", sibling.display())]);
        assert!(
            refused.exit != Some(0) && !sibling.exists(),
            "a write into a sibling dir nobody declared is refused"
        );
        denied += 1;

        denied += the_refused_shapes_rows(&permissive, &writable);

        let outside_env = run(&["cat", &ground.outside.join(".env").to_string_lossy()]);
        assert!(
            outside_env.exit != Some(0) && !outside_env.stdout.contains("outside"),
            "a `.env` outside the tree is refused"
        );
        denied += 1;
        let inside_env = run(&["cat", "./.env"]);
        assert_eq!(
            inside_env.stdout.trim(),
            "TOKEN=inside",
            "and one inside reads"
        );

        let env = run(&["env"]);
        assert_eq!(env.exit, Some(0), "{}", env.stderr);
        assert!(
            !env.stdout.contains("CARGO_MANIFEST_DIR="),
            "a variable the policy does not name does not reach the command"
        );
        denied += 1;

        denied
    }

    #[test]
    fn a_writable_dir_or_tree_holding_a_secret_refuses_to_start() {
        // Renaming a secret's PARENT moves the secret out from under its
        // deny: measured rc 0 under `sandbox_writable = ["~"]` for
        // `~/.config/gh`. So the shape is refused before anything runs.
        let ground = Ground::make("secret-under");
        let secret = ground.outside.join("config").join("gh");
        fs::create_dir_all(&secret).expect("a scratch secret");
        let declared = Policy {
            writable: vec![ground.outside.to_string_lossy().into_owned()],
            secrets: vec![secret.to_string_lossy().into_owned()],
            ..Policy::merged_usr()
        };
        let empty = ground.outside.join("empty-path");
        fs::create_dir_all(&empty).expect("an empty PATH dir");
        for platform in [Platform::Linux, Platform::MacOs] {
            let refusal = open_in(&declared, platform, Some(empty.as_os_str()))
                .expect_err("a writable dir holding a secret's parent does not start");
            assert_eq!(
                refusal,
                Unavailable::SecretUnderWritable {
                    writable: fs::canonicalize(&ground.outside).expect("it resolves"),
                    secret: fs::canonicalize(&secret).expect("it resolves"),
                },
                "on {platform:?}, naming both"
            );
        }
        // The default set too: `~` writable holds `~/.ssh`'s parent, `~`.
        assert!(matches!(
            open_in(
                &policy("sandbox_writable = [\"~\"]\n"),
                Platform::MacOs,
                Some(empty.as_os_str())
            ),
            Err(Unavailable::SecretUnderWritable { .. })
        ));

        // And the tree itself, at run.
        let runner = Confinement::Sandbox(Backend::Bubblewrap(stand_in_runner(
            &ground.outside.join("stand-in"),
        )));
        let in_tree = ground.tree.join("keys");
        fs::create_dir_all(&in_tree).expect("a secret in the tree");
        let refused = runner
            .run(
                &Policy {
                    secrets: vec![in_tree.to_string_lossy().into_owned()],
                    ..Policy::merged_usr()
                },
                &ground.tree,
                &argv(&["true"]),
            )
            .expect_err("a tree holding a secret runs nothing");
        assert!(
            matches!(refused, NotRun::SecretInTree { .. }),
            "{refused:?}"
        );
    }

    #[test]
    fn a_declared_path_holding_a_dot_env_file_refuses_to_start() {
        let ground = Ground::make("env-declared");
        let empty = ground.outside.join("empty-path");
        fs::create_dir_all(&empty).expect("an empty PATH dir");
        let accepting = ground.outside.join("accepting");
        stand_in_seatbelt(&accepting, 0);
        let opens = |dir: &Path, writable: bool| {
            let path = dir.to_string_lossy().into_owned();
            let declared = if writable {
                Policy {
                    writable: vec![path],
                    ..Policy::merged_usr()
                }
            } else {
                Policy {
                    readable: vec![path],
                    ..Policy::merged_usr()
                }
            };
            open_in(&declared, Platform::MacOs, Some(accepting.as_os_str()))
        };

        let templates = ground.outside.join("templates");
        fs::create_dir_all(templates.join("node_modules").join("pkg")).expect("dirs");
        fs::create_dir_all(templates.join(".git")).expect("dirs");
        for name in super::declared::ENV_TEMPLATES {
            fs::write(templates.join(name), "A=1\n").expect("a template");
        }
        fs::write(
            templates.join("node_modules").join("pkg").join(".env"),
            "A=1\n",
        )
        .expect("a dep's");
        fs::write(templates.join(".git").join(".env"), "A=1\n").expect("git's");
        for writable in [true, false] {
            assert!(
                opens(&templates, writable).is_ok(),
                "only the four template names, and `.env` files under node_modules and .git \
                 (not scanned), so it starts (writable: {writable})"
            );
        }

        let local = ground.outside.join("local").join("deep");
        fs::create_dir_all(&local).expect("a dir");
        fs::write(local.join(".env.production"), "TOKEN=x\n").expect("a .env.production");
        for writable in [true, false] {
            let refusal = opens(&ground.outside.join("local"), writable)
                .expect_err("a declared path holding .env.production does not start");
            assert_eq!(
                refusal,
                Unavailable::EnvFileDeclared {
                    found: fs::canonicalize(local.join(".env.production")).expect("it resolves"),
                },
                "naming it (writable: {writable})"
            );
        }
        assert!(
            matches!(
                open_in(
                    &Policy {
                        writable: vec![ground.outside.join("local").to_string_lossy().into_owned()],
                        ..Policy::merged_usr()
                    },
                    Platform::Linux,
                    Some(empty.as_os_str())
                ),
                Err(Unavailable::EnvFileDeclared { .. })
            ),
            "on Linux too, before the runner is looked for"
        );
    }

    #[test]
    fn the_built_in_system_layout_is_not_scanned_for_dot_env_files() {
        // A stand-in for the defaults: a dir holding a `.env`, named as a
        // built-in default, is not scanned; declared by a regimen, it is.
        let ground = Ground::make("env-defaults");
        fs::write(ground.outside.join(".env"), "TOKEN=x\n").expect("a .env");
        let outside = ground.outside.to_string_lossy().into_owned();
        let declared = Policy {
            readable: vec![outside.clone()],
            ..Policy::merged_usr()
        };
        assert_eq!(
            super::declared::env_file_declared_beyond(&declared, &[outside.as_str()]),
            None,
            "a built-in default path is not the regimen's declaration"
        );
        assert!(
            super::declared::env_file_declared_beyond(&declared, &[]).is_some(),
            "and the same path, declared, is scanned"
        );
        assert_eq!(
            Policy::merged_usr().readable,
            policy::DEFAULT_READABLE
                .iter()
                .map(|path| (*path).to_owned())
                .collect::<Vec<_>>(),
            "the default policy's reads are exactly the built-in list the scan skips"
        );
        let started = std::time::Instant::now();
        assert_eq!(
            super::declared::env_file_declared(&Policy::merged_usr()),
            None
        );
        assert!(
            started.elapsed() < std::time::Duration::from_millis(100),
            "a default policy declares nothing to scan: {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn bwrap_masks_each_secret_inside_a_declared_path() {
        let ground = Ground::make("masks");
        let keys = ground.outside.join("keys");
        fs::create_dir_all(&keys).expect("a secret dir");
        let token = ground.outside.join("token");
        fs::write(&token, "t\n").expect("a secret file");
        let outside = ground.outside.to_string_lossy().into_owned();
        let composed = Bubblewrap::at(PathBuf::from("bwrap")).compose(
            &Policy {
                readable: vec![outside.clone()],
                secrets: vec![
                    keys.to_string_lossy().into_owned(),
                    token.to_string_lossy().into_owned(),
                    "/nonexistent-secret".to_owned(),
                ],
                ..Policy::merged_usr()
            },
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        let real = fs::canonicalize(&ground.outside).expect("it resolves");
        let inside = |path: &Path| {
            Path::new(&outside)
                .join(path.strip_prefix(&real).expect("under the outside dir"))
                .to_string_lossy()
                .into_owned()
        };
        let at = |needle: &[&str]| {
            composed
                .windows(needle.len())
                .position(|window| window == needle)
                .unwrap_or_else(|| panic!("{needle:?} is not in {composed:?}"))
        };
        let dir_mask = at(&["--tmpfs", &inside(&fs::canonicalize(&keys).expect("keys"))]);
        let file_mask = at(&[
            "--ro-bind",
            "/dev/null",
            &inside(&fs::canonicalize(&token).expect("token")),
        ]);
        let bind = at(&["--ro-bind", &outside, &outside]);
        let remount = at(&["--remount-ro", "/"]);
        assert!(
            bind < dir_mask && bind < file_mask && dir_mask < remount && file_mask < remount,
            "masked after the bind that exposes them and before the remounts: {composed:?}"
        );
        assert!(
            !composed.iter().any(|arg| arg == "/nonexistent-secret"),
            "a secret outside every declared path needs no mask"
        );
    }

    #[test]
    fn a_session_that_drops_its_confinement_kills_what_its_commands_left_running() {
        // `serve`'s path: a session owns its confinement handle and ends by
        // dropping it, with no `end_session` call (#299, 5982547482).
        let ground = Ground::make("session-drop");
        let runner = Confinement::Sandbox(Backend::Seatbelt(Seatbelt::at(
            stand_in_runner(&ground.outside.join("seatbelt-stand-in"))
                .runner()
                .to_path_buf(),
        )));
        let started = runner
            .run(
                &Policy::merged_usr(),
                &ground.tree,
                &argv(&["sh", "-c", "sleep 30 > /dev/null 2>&1 & echo $!"]),
            )
            .expect("the stand-in ran");
        let pid = started.stdout.trim().to_owned();
        let alive = |pid: &str| {
            process::Command::new("kill")
                .args(["-0", pid])
                .stderr(process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        };
        let held = runner.clone();
        drop(runner);
        assert!(
            alive(&pid),
            "a clone still holds the session: nothing is killed yet"
        );
        drop(held);
        let mut gone = false;
        for _ in 0..50 {
            if !alive(&pid) {
                gone = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(
            gone,
            "and once the last handle is dropped, the session's groups are killed (pid {pid})"
        );
    }

    #[test]
    fn the_dot_env_scan_ignores_case_as_the_profile_does() {
        use super::declared::is_env_file;
        for name in [
            ".env",
            ".ENV",
            ".Env",
            ".env.production",
            ".Env.Production",
            ".ENV.production",
        ] {
            assert!(
                is_env_file(name),
                "{name} is a `.env` file, as Seatbelt reads it"
            );
        }
        for name in [
            ".env.example",
            ".env.EXAMPLE",
            ".ENV.Template",
            ".Env.Sample",
            ".ENV.DIST",
        ] {
            assert!(
                !is_env_file(name),
                "{name} is a template, exempt in any case"
            );
        }
        for name in ["not.env", ".envrc", "env", ".env "] {
            assert!(!is_env_file(name), "{name} is not a `.env` file");
        }

        // And at open: a writable dir holding `d/.ENV` no longer opens, so
        // the rename-then-read of the review (rc 0) cannot happen.
        let ground = Ground::make("env-case");
        let accepting = ground.outside.join("accepting");
        stand_in_seatbelt(&accepting, 0);
        let writable = ground.outside.join("w");
        fs::create_dir_all(writable.join("d")).expect("a dir");
        fs::write(writable.join("d").join(".ENV"), "TOKEN=x\n").expect("an upper-case .env");
        let refused = open_in(
            &Policy {
                writable: vec![writable.to_string_lossy().into_owned()],
                ..Policy::merged_usr()
            },
            Platform::MacOs,
            Some(accepting.as_os_str()),
        );
        assert!(
            matches!(refused, Err(Unavailable::EnvFileDeclared { .. })),
            "{refused:?}"
        );
    }

    #[test]
    fn a_declared_path_inside_a_secret_refuses_to_start() {
        let ground = Ground::make("inside-secret");
        let secret = ground.outside.join("ssh");
        fs::create_dir_all(secret.join("sub")).expect("a scratch secret");
        fs::write(secret.join("id_rsa"), "k\n").expect("a key");
        let empty = ground.outside.join("empty-path");
        fs::create_dir_all(&empty).expect("an empty PATH dir");
        let real = fs::canonicalize(&secret).expect("it resolves");
        for (readable, writable, inside) in [
            (vec![secret.join("id_rsa")], vec![], real.join("id_rsa")),
            (vec![secret.clone()], vec![], real.clone()),
            (vec![], vec![secret.join("sub")], real.join("sub")),
        ] {
            let declared = Policy {
                readable: readable
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                writable: writable
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                secrets: vec![secret.to_string_lossy().into_owned()],
                ..Policy::merged_usr()
            };
            for platform in [Platform::Linux, Platform::MacOs] {
                assert_eq!(
                    open_in(&declared, platform, Some(empty.as_os_str())).expect_err("refused"),
                    Unavailable::DeclaredInsideSecret {
                        path: inside.clone(),
                        secret: real.clone(),
                    },
                    "{platform:?}, naming both"
                );
            }
        }
    }

    #[test]
    fn a_session_forgets_a_process_group_once_it_has_no_member() {
        let ground = Ground::make("groups");
        let runner = Seatbelt::at(
            stand_in_runner(&ground.outside.join("seatbelt-stand-in"))
                .runner()
                .to_path_buf(),
        );
        let confinement = Confinement::Sandbox(Backend::Seatbelt(runner.clone()));
        confinement
            .run(&Policy::merged_usr(), &ground.tree, &argv(&["true"]))
            .expect("it ran");
        assert!(
            runner.groups().is_empty(),
            "a command that left nothing running leaves no group to kill later, \
             whose id could by then be someone else's"
        );
        let left = confinement
            .run(
                &Policy::merged_usr(),
                &ground.tree,
                &argv(&["sh", "-c", "sleep 30 > /dev/null 2>&1 & echo $!"]),
            )
            .expect("it ran");
        assert_eq!(
            runner.groups().len(),
            1,
            "a group with a live member is kept"
        );
        confinement.end_session();
        let _ = left;
    }

    #[test]
    fn a_secret_that_does_not_exist_yet_resolves_through_its_existing_ancestor() {
        // A symlinked HOME stand-in: `link` -> `real`, and a secret under it
        // that the host has not created yet.
        let ground = Ground::make("canonical-prefix");
        let real = ground.outside.join("real-home");
        fs::create_dir_all(&real).expect("a home");
        let link = ground.outside.join("home-link");
        std::os::unix::fs::symlink(&real, &link).expect("a symlinked home");
        let secret = link.join(".config").join("gh");
        let canonical = fs::canonicalize(&real)
            .expect("it resolves")
            .join(".config")
            .join("gh");
        assert_eq!(policy::canonical_prefix(&secret), canonical);

        // Used by the open-time check...
        let empty = ground.outside.join("empty-path");
        fs::create_dir_all(&empty).expect("an empty PATH dir");
        assert!(matches!(
            open_in(
                &Policy {
                    writable: vec![real.to_string_lossy().into_owned()],
                    secrets: vec![secret.to_string_lossy().into_owned()],
                    ..Policy::merged_usr()
                },
                Platform::MacOs,
                Some(empty.as_os_str())
            ),
            Err(Unavailable::SecretUnderWritable { .. })
        ));
        // ...and by the profile's deny.
        let composed = Seatbelt::at(PathBuf::from("sandbox-exec")).compose(
            &Policy {
                secrets: vec![secret.to_string_lossy().into_owned()],
                ..Policy::merged_usr()
            },
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        assert!(
            composed.contains(&format!("SECRET_0={}", canonical.display())),
            "{composed:?}"
        );
    }

    #[test]
    fn the_re_executed_drive_holds_only_the_listed_environment() {
        let mut parent: Vec<_> = std::env::vars_os().collect();
        parent.push(("DIET_FAKE_ENDPOINT_KEY".into(), "fake-key-value".into()));
        assert!(
            parent.iter().any(|(name, _)| name == "CARGO_MANIFEST_DIR"),
            "this test needs an unlisted variable of its own to keep out"
        );
        let output = super::scrubbed_drive(
            Path::new("env"),
            &[],
            &Policy::merged_usr().environment,
            parent,
        )
        .output()
        .expect("env ran");
        let seen = String::from_utf8_lossy(&output.stdout);
        assert!(
            !seen.contains("fake-key-value") && !seen.contains("CARGO_MANIFEST_DIR="),
            "neither a key nor any other unlisted variable reaches the drive: {seen}"
        );
        assert!(
            seen.contains(&format!("{}=1", super::SCRUBBED)) && seen.contains("PATH="),
            "the marker and the listed variables do: {seen}"
        );
    }

    #[test]
    fn the_drives_key_file_joins_the_secrets_and_no_command_reads_it() {
        // T1's lines, and a key file elsewhere under a home stand-in.
        let ground = Ground::make("key-file");
        let cfg = ground.outside.join("cfg");
        fs::create_dir_all(&cfg).expect("a config dir");
        let key = cfg.join("key");
        fs::write(&key, "DUMMY-KEY\n").expect("a key file");
        let mut t1 = policy(
            "isolation = \"sandbox\"\n\
             network = \"host\"\n\
             sandbox_reads = \"all\"\n\
             env_passthrough = [\"npm_config_cache\"]\n",
        );
        super::declared::add_credential(&mut t1, &key).expect("not in a declared path");
        let canonical = fs::canonicalize(&key).expect("it resolves");
        assert!(
            t1.secrets
                .contains(&canonical.to_string_lossy().into_owned()),
            "the key file is now one of the session's secrets: {:?}",
            t1.secrets
        );
        let composed = Seatbelt::at(PathBuf::from("sandbox-exec")).compose(
            &t1,
            &ground.tree,
            &argv(&["cat", &key.to_string_lossy()]),
        );
        assert!(
            composed
                .iter()
                .any(|arg| arg.starts_with("SECRET_")
                    && arg.ends_with(&*canonical.to_string_lossy())),
            "and the profile denies it, under `sandbox_reads = \"all\"` too: {composed:?}"
        );

        // A regimen that lists the key's directory has declared it open.
        for declared in [
            Policy {
                readable: vec![cfg.to_string_lossy().into_owned()],
                ..t1.clone()
            },
            Policy {
                writable: vec![cfg.to_string_lossy().into_owned()],
                ..t1.clone()
            },
        ] {
            let mut declared = declared;
            declared.secrets.retain(|secret| !secret.ends_with("/key"));
            assert!(
                super::declared::add_credential(&mut declared, &key).is_err(),
                "a key file in a declared path is refused"
            );
        }

        // And on the Mac, measured: a sandboxed `cat <keyfile>` is refused.
        if Platform::here() == Platform::MacOs
            && let Ok(confinement) = open_on(&t1, Platform::MacOs)
        {
            let read = confinement
                .run(&t1, &ground.tree, &argv(&["cat", &key.to_string_lossy()]))
                .expect("the sandbox set up");
            assert!(
                read.exit != Some(0) && !read.stdout.contains("DUMMY-KEY"),
                "the key is not readable from inside: {:?} {:?}",
                read.exit,
                read.stdout
            );
        }
    }

    #[test]
    fn no_helper_the_drive_runs_unconfined_is_looked_up_on_path() {
        // A `kill` planted in a writable dir at the head of `PATH`, which a
        // command can do when the drive's `PATH` names such a dir.
        let ground = Ground::make("planted-kill");
        let planted = ground.tree.join("bin");
        fs::create_dir_all(&planted).expect("a writable PATH dir");
        let marker = ground.outside.join("ESCAPED");
        fs::write(
            planted.join("kill"),
            format!("#!/bin/sh\necho ran > {}\n", marker.display()),
        )
        .expect("a planted kill");
        fs::set_permissions(planted.join("kill"), fs::Permissions::from_mode(0o755))
            .expect("executable");
        let status = super::seatbelt::kill()
            .env("PATH", &planted)
            .args(["-0", &process::id().to_string()])
            .status();
        let _ = status;
        assert!(
            !marker.exists(),
            "the planted `kill` did not run: the drive's helper is `/bin/kill`, by path"
        );
    }

    /// sha256, hex, computed here rather than through `crate::digest`.
    fn sha256_independently(bytes: &[u8]) -> String {
        use sha2::{Digest as _, Sha256};
        Sha256::digest(bytes)
            .iter()
            .fold(String::new(), |mut out, byte| {
                let _ = std::fmt::Write::write_fmt(&mut out, format_args!("{byte:02x}"));
                out
            })
    }

    #[test]
    fn the_policy_is_the_digest_of_exactly_the_profile_or_the_runner_argv() {
        // Seatbelt: the bytes passed with `-p`, and nothing else.
        let seatbelt = Confinement::Sandbox(Backend::Seatbelt(Seatbelt::at(PathBuf::from(
            "/usr/bin/sandbox-exec",
        ))));
        let policy = shaped(Network::None, vec![18766]);
        let confined = seatbelt.compose(&policy, Path::new("/work/tree"), &argv(&["true"]));
        let at = confined
            .iter()
            .position(|arg| arg == "-p")
            .expect("a profile");
        let digest = seatbelt.policy_of(&confined).expect("a policy");
        assert_eq!(digest, sha256_independently(confined[at + 1].as_bytes()));
        assert_eq!(digest.len(), 64);
        assert!(
            digest
                .chars()
                .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
        );
        // One per policy: not the tree, not its depth, not the command.
        for (tree, command) in [
            ("/elsewhere/tree", argv(&["true"])),
            ("/a/much/deeper/working/tree", argv(&["cargo", "test"])),
        ] {
            let other = seatbelt.compose(&policy, Path::new(tree), &command);
            assert_eq!(seatbelt.policy_of(&other), Some(digest.clone()), "{tree}");
        }
        assert_ne!(
            seatbelt.policy_of(&seatbelt.compose(
                &Policy::merged_usr(),
                Path::new("/work/tree"),
                &argv(&["true"])
            )),
            Some(digest.clone()),
            "and a policy of another shape is another digest"
        );

        // bwrap: the composed argv up to `--`, NUL-joined.
        let bwrap = Confinement::Sandbox(Backend::Bubblewrap(Bubblewrap::at(PathBuf::from(
            "/usr/bin/bwrap",
        ))));
        let confined = bwrap.compose(
            &Policy::merged_usr(),
            Path::new("/work/tree"),
            &argv(&["true"]),
        );
        let end = confined
            .iter()
            .position(|arg| arg == "--")
            .expect("a separator");
        let digest = bwrap.policy_of(&confined).expect("a policy");
        assert_eq!(
            digest,
            sha256_independently(confined[..end].join("\0").as_bytes())
        );
        let other = bwrap.compose(
            &Policy::merged_usr(),
            Path::new("/work/tree"),
            &argv(&["cargo", "test"]),
        );
        assert_eq!(
            bwrap.policy_of(&other),
            Some(digest),
            "the command is not the policy"
        );

        assert_eq!(
            Confinement::Unconfined.policy_of(&argv(&["true"])),
            None,
            "unconfined has no policy to hash"
        );
    }

    #[test]
    fn the_record_carries_the_policy_it_ran_under() {
        let ground = Ground::make("policy");
        let command = argv(&["sh", "-c", "exit 3"]);
        for runner in [
            Confinement::Sandbox(Backend::Bubblewrap(stand_in_runner(
                &ground.outside.join("bwrap-stand-in"),
            ))),
            Confinement::Sandbox(Backend::Seatbelt(Seatbelt::at(
                stand_in_runner(&ground.outside.join("seatbelt-stand-in"))
                    .runner()
                    .to_path_buf(),
            ))),
        ] {
            let ran = runner
                .run(&Policy::merged_usr(), &ground.tree, &command)
                .expect("the stand-in ran");
            assert_eq!(ran.exit, Some(3), "the command's own exit");
            assert_eq!(
                ran.policy,
                runner.policy_of(&ran.confined),
                "the digest of what was actually composed"
            );
            assert!(ran.policy.is_some(), "and a confined run always has one");
        }
        let plain = Confinement::Unconfined
            .run(&Policy::unconfined(), &ground.tree, &command)
            .expect("it ran");
        assert_eq!(plain.policy, None);
    }

    #[test]
    fn network_loopback_refuses_to_start_on_linux() {
        let declared = policy("network_loopback = [18766]\n");
        assert_eq!(declared.loopback, [18766]);
        let refusal = open_on(&declared, Platform::Linux)
            .expect_err("bwrap's loopback cannot reach the host's ports");
        assert_eq!(
            refusal,
            Unavailable::LoopbackNotImplemented {
                on: Platform::Linux
            }
        );
        assert!(
            refusal.to_string().contains("network_loopback")
                && refusal.to_string().contains("loopback")
                && refusal.to_string().contains("not built for linux"),
            "and it says so: {refusal}"
        );
    }

    #[test]
    fn a_loopback_declaration_is_read_only_under_no_network() {
        let read = |text: &str| Policy::from_regimen(&regimen::parse(text).expect("a regimen"));
        assert_eq!(
            read("network_loopback = [18766, 18767]\n")
                .expect("a policy")
                .loopback,
            [18766, 18767]
        );
        assert!(
            read("model = \"m\"\n")
                .expect("a policy")
                .loopback
                .is_empty()
        );
        for shared in [
            "network = \"host\"\nnetwork_loopback = [18766]\n",
            "isolation = \"none\"\nnetwork_loopback = [18766]\n",
        ] {
            assert_eq!(
                read(shared),
                Err(PolicyError::LoopbackWithoutNone {
                    network: Network::Host
                }),
                "under `host` every port is already reachable: {shared}"
            );
        }
        for bad in [
            "network_loopback = 18766\n",
            "network_loopback = [\"18766\"]\n",
            "network_loopback = [0]\n",
            "network_loopback = [65536]\n",
            "network_loopback = [-1]\n",
        ] {
            assert_eq!(read(bad), Err(PolicyError::NotAListOfPorts), "{bad}");
        }
    }

    #[test]
    fn an_operation_not_permitted_is_listed_and_never_explained() {
        let said = "cat: ../outside/secret: Operation not permitted";
        let refused = ran(said);
        assert_eq!(
            refused.denials(),
            vec![Denial {
                kind: DenialKind::NotPermitted,
                evidence: said.to_owned(),
            }]
        );
        assert!(
            !DenialKind::NotPermitted.is_unambiguous(),
            "EPERM arises with no sandbox at all"
        );
        assert!(
            !refused
                .as_the_model_sees_it()
                .contains("ran under isolation"),
            "so no policy note is attached: {}",
            refused.as_the_model_sees_it()
        );
        assert!(
            ran("curl: (7) Failed to connect to 127.0.0.1 port 1: Couldn't connect to server")
                .denials()
                .is_empty(),
            "and a refused connection is not evidence of a policy"
        );
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
            policy: None,
            exit: Some(1),
            stdout: String::new(),
            stderr: stderr.to_owned(),
            stdout_bytes: 0,
            stderr_bytes: stderr.len() as u64,
            cancelled: false,
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
            shown.contains("absent (under bwrap) or present but denied (under Seatbelt)"),
            "and it is true under both backends: under Seatbelt the paths are there \
             and refused, so `not present` would be false (#299, 5977176035): {shown}"
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

        // And the distinction is USED at the one site where a false positive
        // reaches the model. The module built it and then, for a while, did
        // not use it: an ordinary missing file inside the working tree came
        // back with a policy explanation attached, which the model believes.
        assert!(
            !missing
                .as_the_model_sees_it()
                .contains("ran under isolation"),
            "an ambiguous denial gets no policy note: {}",
            missing.as_the_model_sees_it()
        );
        let denied = ran("sh: 1: cannot create /etc/hosts: Read-only file system");
        assert!(
            denied
                .as_the_model_sees_it()
                .contains("ran under isolation"),
            "and an unambiguous one still does, or the note is gone rather than \
             narrowed: {}",
            denied.as_the_model_sees_it()
        );
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

        assert_eq!(
            read("isolation = \"none\"\nnetwork = \"none\"\n"),
            Err(PolicyError::Unenforceable {
                key: policy::NETWORK,
                under: Isolation::None,
            }),
            "recorded rather than refused, this is a drive that ran on the \
             operator's network and banked a record saying it did not"
        );
        assert_eq!(
            read("isolation = \"none\"\nnetwork = \"host\"\n")
                .expect("a policy")
                .network,
            Network::Host,
            "and the declaration that agrees with what unconfined means is read, \
             not refused along with it"
        );
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
                "absent_or_outside",
                "not_permitted"
            ]
        );
        assert_eq!(
            Platform::ALL.iter().map(|p| p.tag()).collect::<Vec<_>>(),
            ["linux", "macos", "other"]
        );
    }
}
