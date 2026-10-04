//! The macOS sandbox: a Seatbelt profile, through `sandbox-exec`.
//!
//! **Default-deny.** The profile starts from `(deny default)` and allows back
//! only what step 0 on #299 measured `sh`, `python3` and `curl` needing to
//! start, plus the working tree. A write outside the tree, a read under the
//! home directory, an outbound connection and a DNS lookup are each refused
//! at the syscall, and the refusal reaches the command as `EPERM`
//! ("Operation not permitted"), exit 1 -- there is no signal of its own.
//!
//! # One profile per policy
//!
//! The profile is generated from the [`Policy`] and holds **no path and no
//! port**. Every value -- the working tree, each `sandbox_readable` path,
//! each declared loopback port -- enters through a `-D` parameter
//! (`(param "WORKTREE")`, `READABLE_<n>`, `WRITABLE_<n>`, `PORT_<n>`,
//! `SECRET_<n>`), and the values are
//! recorded in the composed argv (`Ran::confined`).
//!
//! **What the digest is a function of, exactly:** the policy's network
//! (`none` or `host`), its read mode (`sandbox_reads`), the NUMBER of
//! `sandbox_readable` paths, of `sandbox_writable` dirs and of secrets, and,
//! under `network = "none"`, the NUMBER of declared loopback ports. Nothing
//! else: not the worktree, not its depth, not `HOME`, not the paths or ports
//! themselves.
//!
//! The worktree's depth does not enter because its ancestors are computed
//! INSIDE the profile, from `(param "WORKTREE")`, by the `ancestors` function
//! below. They are needed: with no metadata allow at all `sh` cannot resolve
//! `getcwd`, and with a global one `stat` of a file under `$HOME` succeeds
//! while its read is denied -- existence and size leak (step 0, measured).
//! So metadata is allowed on the system paths, on the tree, and on each
//! literal ancestor of the tree and of each readable path, and the profile
//! text stays one per policy.
//!
//! # What the profile allows, and why
//!
//! - `process-exec`, `process-fork`, `sysctl-read`: nothing starts without
//!   them. `signal` only to the same sandbox, so a command cannot signal the
//!   operator's other processes (measured: `kill -0` of an outside pid is
//!   refused, a child's own `kill` of its sibling works).
//!   **`sysctl-read` exposes the host's process table**: through
//!   `KERN_PROCARGS2` a command reads the argv AND THE ENVIRONMENT of every
//!   same-user process that is not an Apple platform binary -- the drive's
//!   own included -- and through other calls its cwd and open files
//!   (review of 0dec34b, H; the first disclosure said argv and cwd, which
//!   was wrong). The census measures it. Neither `(deny sysctl-read
//!   (sysctl-name "kern.procargs2"))`, a `kern.proc` prefix deny, nor
//!   `(deny process-info* (target others))` stopped it. `ps` itself is
//!   refused (setuid; exit 71). Ruled at #299 5983673467: disclosed as
//!   `process_env_visible = "same_user"` on the equipment, and the drive
//!   re-executes itself scrubbed so its own environment holds no key.
//! - `mach-lookup`, **narrowed to one service**,
//!   `com.apple.system.opendirectoryd.libinfo` (user and group names). With
//!   no `mach-lookup` at all, `sh`, `python3`, `curl`, the loopback knock,
//!   and DNS plus HTTPS under `network = "host"` all still worked on the Mac
//!   Pro (macOS 26.6.2); what failed was naming: `id` and `ls -l` printed
//!   numeric ids. That one service is what remains.
//! - Reads: `/`, `/usr`, `/bin`, `/sbin`, `/System`, `/Library`,
//!   `/private/etc`, `/private/var/db/dyld`, `/private/var/db/timezone`,
//!   `/private/var/db/xcode_select_link`, `/dev`, the `/etc`, `/var` and
//!   `/tmp` symlinks themselves, the tree, and each `sandbox_readable` path.
//!   Homebrew's `/usr/local` is inside `/usr`. Nothing under `/Users`.
//! - `sandbox_reads = "all"`: `file-read*` everywhere, in place of the list
//!   above. The secrets below still deny.
//! - `sandbox_writable`: each dir is writable (and readable, and its
//!   ancestors' metadata) like the tree, through `WRITABLE_<n>`.
//! - **Secrets, LAST**: `deny file-read* file-write*` on every
//!   `sandbox_secrets` path (`SECRET_<n>`: the defaults, `~/.ssh`, `~/.aws`,
//!   `~/.config/gh`, `~/Library/Keychains`, `~/.npmrc`, `~/.netrc`,
//!   `~/.docker/config.json`, `~/.gnupg`, plus the regimen's additions, `~`
//!   being the drive's `HOME`), and on every `.env` or `.env.*` file outside
//!   the tree. The later rule wins in Seatbelt, so these beat `reads =
//!   "all"`, a readable path and a writable dir alike.
//! - The environment is not the profile's: `Confinement::run` clears it and
//!   passes only `PATH`, `HOME`, `LANG`, `TERM`, `TMPDIR` and the regimen's
//!   `env_passthrough`, on every backend.
//! - Writes: the tree and `/dev/null`. Nothing else. **Not `/dev/tty`**:
//!   with it, a command run from a terminal wrote raw escape sequences to the
//!   operator's terminal past its captured output (#299 review, F3). `bwrap`
//!   prevents that with `--new-session`; here the equivalent, `setsid` in a
//!   `pre_exec`, needs `unsafe`, which the workspace forbids, so the device
//!   is simply not writable. Nothing measured needs it: `sh`, `python3`,
//!   `curl`, `node` and `npm` run with their streams captured.
//! - A hard link that the HOST made inside the tree to a file outside it can
//!   be written through (the write is to an inode the tree names); the
//!   sandbox cannot create one (`ln` is refused). Do not hand a drive a tree
//!   whose files are hard-linked outside it -- `git clone --local` hard-links
//!   `.git/objects`.
//! - Network: under `none`, each declared `network_loopback` port admits
//!   `network-outbound`, `network-bind` and `network-inbound`, on
//!   `localhost:<port>` only (#299, ruling 5976914097: the control is the
//!   host, not the direction), and nothing else -- not DNS, not another
//!   port, and not the declared port on another host. Under `host`: IP
//!   outbound, bind and inbound on any address and port, and the one
//!   Unix-domain socket DNS needs, `/private/var/run/mDNSResponder`. **Not
//!   `network*`**, which also admits every Unix-domain socket: the Docker
//!   daemon answered `GET /_ping` from inside the sandbox through it, and so
//!   did the user's ssh-agent (#299 review, F1).
//!
//!   **`localhost` here is every address of this host, not only 127.0.0.1.**
//!   Seatbelt's grammar admits only `*` or `localhost` as a host (anything
//!   else is exit 65, "host must be * or localhost"), so a bind of the
//!   declared port on `0.0.0.0` or the LAN address is admitted and reachable
//!   from the LAN, while a connect to another host on it is refused
//!   (measured). Disclosed as `loopback_scope = "host"` on the equipment
//!   (#299, ruling 5977176035).
//!
//! `sandbox_symlinks` is a Linux layout key and is **ignored here**: the
//! command sees the host's own filesystem, symlinks and all.
//!
//! # Known consequence, stated
//!
//! A read of a home-directory file is `EPERM`, not `ENOENT` as under
//! `bwrap`. Tools that treat a missing `~/.gitconfig` as fine treat a
//! refused one as fatal: `git` exits 128 under the default policy unless
//! the regimen makes its configuration readable.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex, PoisonError};

use super::Unavailable;
use super::policy::{Isolation, Network, Policy, Reads, canonical_prefix, home_expanded};
use crate::digest::sha256_hex;

/// The program this module composes for.
pub const RUNNER: &str = "sandbox-exec";

/// The flag the profile text follows in the composed argv.
const PROFILE_FLAG: &str = "-p";

/// The worktree the profile is validated against at open: a system
/// directory every macOS has, empty and owned by root. The validation runs
/// `/usr/bin/true`, so nothing is ever written here.
const VALIDATION_TREE: &str = "/private/var/empty";

/// The profile's fixed head: deny by default, then what every command needs
/// to start at all.
const HEAD: &str = r#"(version 1)
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
"#;

/// The system paths a command reads.
const SYSTEM_READS: &str = r#"  (literal "/") (literal "/etc") (literal "/var") (literal "/tmp")
  (subpath "/usr") (subpath "/bin") (subpath "/sbin")
  (subpath "/System") (subpath "/Library")
  (subpath "/private/etc") (subpath "/private/var/db/dyld")
  (subpath "/private/var/db/timezone") (literal "/private/var/db/xcode_select_link")
  (subpath "/dev")
"#;

/// Where a command may write: the tree, each `sandbox_writable` dir (added
/// between these two), and `/dev/null`.
const WRITES_HEAD: &str = r#"(allow file-write*
  (subpath (param "WORKTREE"))"#;

/// The end of the write rule.
const WRITES_TAIL: &str = r#"
  (literal "/dev/null"))
(allow file-ioctl (literal "/dev/null"))
"#;

/// Reads of everything, for `sandbox_reads = "all"`; the secrets are denied
/// after it.
const ALL_READS: &str = "(allow file-read*)\n";

/// `.env` files outside the tree: any path whose last component is `.env` or
/// starts `.env.`, unless it is under the worktree or is one of the four
/// template names `declared::ENV_TEMPLATES` exempts (#299, 5982547482).
/// Seatbelt matches the regex ignoring case, and the open-time scan now
/// compares names ignoring ASCII case too, so the two agree on `.ENV`,
/// `.Env.Production` and `.env.EXAMPLE` alike (review of 0dec34b, G). Measured:
/// `.env` and `.env.production` outside are refused (exit 1); `.env.example`,
/// `.env.template` and `not.env` outside, and `.env` inside, read (exit 0).
const ENV_FILES: &str = r#"(deny file-read* file-write* (require-all (regex #"/\.env(\.[^/]*)?$") (require-not (regex #"/\.env\.(example|sample|template|dist)$")) (require-not (subpath (param "WORKTREE")))))
"#;

/// The host's IP network, declared, and DNS's socket: nothing else that
/// `network*` would admit, because that includes every Unix-domain socket.
const HOST_NETWORK: &str = r#"(allow network-outbound (remote ip "*:*"))
(allow network-bind (local ip "*:*"))
(allow network-inbound (local ip "*:*"))
(allow network-outbound (literal "/private/var/run/mDNSResponder"))
"#;

/// The only host a declared loopback port is admitted on.
const LOOPBACK: &str = "localhost";

/// A found runner, and the process groups its commands have led.
///
/// **Each command leads its own process group**, and every group is killed
/// at SESSION end ([`Seatbelt::end_session`]), not after each command: a dev
/// server a regimen starts in one command must survive into the next. Under
/// Seatbelt nothing else bounds a command's lifetime -- a double-forked
/// orphan is reparented to launchd and outlives the drive, still confined and
/// still networked (#299 review, F2; disclosed as `orphan_control = "none"`
/// on the equipment). `bwrap`'s pid namespace has no such orphan.
///
/// **Escapable, and stated:** a child that calls `setsid` (or `setpgid`)
/// itself leaves the group and survives the kill. The group is made with
/// `process_group(0)` rather than `setsid` in a `pre_exec`, because
/// `pre_exec` is `unsafe` and the workspace forbids `unsafe`. And a group
/// id is a pid: after each command returns, every recorded group with no
/// member left is forgotten ([`Seatbelt::forget_ended_groups`]), so the
/// only id the session-end kill can find reused is one whose group emptied
/// after the last command returned.
///
/// **Ctrl-C reaches only the drive.** Because each command leads its own
/// group, it is outside the terminal's foreground group: an interrupt at the
/// operator's terminal stops the drive, while a running command and its
/// children keep going, and on that path the session-end kill does not run
/// (#299 review of 3f7cc1e, D).
#[derive(Debug, Clone)]
pub struct Seatbelt {
    runner: PathBuf,
    groups: Arc<Groups>,
}

impl Seatbelt {
    /// Find the runner on `PATH`.
    ///
    /// # Errors
    ///
    /// Returns [`Unavailable::NoRunner`] if it is not there. Never a fallback.
    pub fn found() -> Result<Self, Unavailable> {
        Self::found_in(std::env::var_os("PATH").as_deref())
    }

    /// Find the runner on a `PATH` named rather than read, so a test can
    /// name one that lacks it on a host that has it.
    ///
    /// # Errors
    ///
    /// As [`Seatbelt::found`].
    pub fn found_in(path: Option<&OsStr>) -> Result<Self, Unavailable> {
        path.and_then(|path| {
            std::env::split_paths(path)
                .map(|dir| dir.join(RUNNER))
                .find(|candidate| candidate.is_file())
        })
        .map(|runner| Self {
            runner,
            groups: Arc::default(),
        })
        .ok_or_else(|| Unavailable::NoRunner {
            mechanism: Isolation::Sandbox,
            looked_for: RUNNER.to_owned(),
        })
    }

    /// A runner at a named path, for a caller that has one.
    #[must_use]
    pub fn at(runner: PathBuf) -> Self {
        Self {
            runner,
            groups: Arc::default(),
        }
    }

    /// Where the runner is.
    #[must_use]
    pub fn runner(&self) -> &Path {
        &self.runner
    }

    /// Spawn `command` leading a process group of its own, and remember the
    /// group for [`Seatbelt::end_session`] ([`spawn_leading_a_group`]).
    pub(super) fn spawn(&self, command: &mut Command) -> std::io::Result<Child> {
        let child = spawn_leading_a_group(command)?;
        self.groups
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(child.id());
        Ok(child)
    }

    /// Forget every recorded group that no longer has a member, after each
    /// command returns (#299 review of 0dec34b, J). A group id is protected
    /// from reuse only while the group has a member, so an emptied one kept
    /// until session end could name someone else's group by then and take
    /// its `SIGKILL`. Checked with `kill -0 -- -<pgid>`, a subprocess, since
    /// the workspace forbids `unsafe` and no safe signal wrapper is a
    /// dependency: exit 0 means the group still has a member.
    pub fn forget_ended_groups(&self) {
        let mut groups = self.groups.0.lock().unwrap_or_else(PoisonError::into_inner);
        groups.retain(|group| {
            kill()
                .args(["-0", "--", &format!("-{group}")])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
        });
    }

    /// The process groups this runner's commands led, still to be ended.
    #[must_use]
    pub fn groups(&self) -> Vec<u32> {
        self.groups
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Kill every process group a command of this session led, and forget
    /// them. Called once, when the session ends.
    pub fn end_session(&self) {
        self.groups.end();
    }

    /// The profile `policy` runs under: one fixed text per policy, holding
    /// no path and no port (see the module documentation for exactly what
    /// it is a function of).
    #[must_use]
    pub fn profile(policy: &Policy) -> String {
        let mut out = String::from(HEAD);

        out.push_str("(allow file-read-metadata\n");
        out.push_str("  (literal \"/private\") (literal \"/private/var\")\n");
        out.push_str("  (subpath \"/private/var/select\")\n");
        out.push_str("  (ancestors (param \"WORKTREE\"))");
        for index in 0..policy.readable.len() {
            let _ = write!(out, "\n  (ancestors (param \"READABLE_{index}\"))");
        }
        for index in 0..policy.writable.len() {
            let _ = write!(out, "\n  (ancestors (param \"WRITABLE_{index}\"))");
        }
        out.push_str(")\n");

        if policy.reads == Reads::All {
            out.push_str(ALL_READS);
        } else {
            out.push_str("(allow file-read*\n");
            out.push_str(SYSTEM_READS);
            out.push_str("  (subpath (param \"WORKTREE\"))");
            for index in 0..policy.readable.len() {
                let _ = write!(out, "\n  (subpath (param \"READABLE_{index}\"))");
            }
            for index in 0..policy.writable.len() {
                let _ = write!(out, "\n  (subpath (param \"WRITABLE_{index}\"))");
            }
            out.push_str(")\n");
        }

        out.push_str(WRITES_HEAD);
        for index in 0..policy.writable.len() {
            let _ = write!(out, "\n  (subpath (param \"WRITABLE_{index}\"))");
        }
        out.push_str(WRITES_TAIL);

        if policy.network == Network::Host {
            out.push_str(HOST_NETWORK);
        } else {
            for index in 0..policy.loopback.len() {
                let port = format!("(string-append \"{LOOPBACK}:\" (param \"PORT_{index}\"))");
                let _ = writeln!(out, "(allow network-outbound (remote ip {port}))");
                let _ = writeln!(out, "(allow network-bind (local ip {port}))");
                let _ = writeln!(out, "(allow network-inbound (local ip {port}))");
            }
        }

        let denies = Self::secrets(policy);
        // The denies LAST. In a Seatbelt profile the later matching rule
        // wins, so a deny after every allow beats `sandbox_reads = "all"`,
        // every `sandbox_readable` path and every `sandbox_writable` dir.
        format!("{out}{denies}")
    }

    /// The secrets' denies: reads (metadata included) and writes, of each
    /// secret path and of `.env` files outside the tree.
    fn secrets(policy: &Policy) -> String {
        let mut out = String::from("(deny file-read* file-write*");
        for index in 0..policy.secrets.len() {
            let _ = write!(out, "\n  (subpath (param \"SECRET_{index}\"))");
        }
        out.push_str(")\n");
        out.push_str(ENV_FILES);
        out
    }

    /// The `-D` values `policy` and `worktree` give the profile, in the
    /// order the profile names them.
    fn parameters(policy: &Policy, tree: &str) -> Vec<String> {
        let mut out = vec![format!("WORKTREE={tree}")];
        for (index, path) in policy.readable.iter().enumerate() {
            out.push(format!(
                "READABLE_{index}={}",
                canonical(&home_expanded(path))
            ));
        }
        for (index, dir) in policy.writable.iter().enumerate() {
            out.push(format!(
                "WRITABLE_{index}={}",
                canonical(&home_expanded(dir))
            ));
        }
        if policy.network == Network::None {
            for (index, port) in policy.loopback.iter().enumerate() {
                out.push(format!("PORT_{index}={port}"));
            }
        }
        for (index, secret) in policy.secrets.iter().enumerate() {
            out.push(format!(
                "SECRET_{index}={}",
                canonical(&home_expanded(secret))
            ));
        }
        out
    }

    /// The argv that runs `argv` under `policy` in `worktree`:
    /// `sandbox-exec -D KEY=value … -p <profile> -- argv…`.
    ///
    /// **The worktree is canonicalised first.** Seatbelt's `subpath` matches
    /// the RESOLVED path, so a tree given as `/tmp/x` -- which is
    /// `/private/tmp/x` -- would be denied to itself. `confined` records the
    /// canonical path. A `sandbox_readable` path that exists is canonicalised
    /// the same way (`/etc` is `/private/etc`); one that does not is passed
    /// as declared, where it allows nothing.
    ///
    /// `sandbox-exec` parses its options with `getopt`, so `--` ends them:
    /// measured, `sandbox-exec -p … -- -x` executes `-x` rather than reading
    /// it as an option.
    #[must_use]
    pub fn compose(&self, policy: &Policy, worktree: &Path, argv: &[String]) -> Vec<String> {
        let tree = canonical(worktree);
        let mut out = vec![self.runner.to_string_lossy().into_owned()];
        for parameter in Self::parameters(policy, &tree) {
            out.push("-D".to_owned());
            out.push(parameter);
        }
        out.push(PROFILE_FLAG.to_owned());
        out.push(Self::profile(policy));
        out.push("--".to_owned());
        out.extend(argv.iter().cloned());
        out
    }

    /// The `policy` a run under this runner carries: the sha256 of exactly
    /// the bytes passed with `-p`, read back out of the composed argv.
    #[must_use]
    pub fn policy_of(confined: &[String]) -> Option<String> {
        let end = confined
            .iter()
            .position(|arg| arg == "--")
            .unwrap_or(confined.len());
        confined[..end]
            .iter()
            .position(|arg| arg == PROFILE_FLAG)
            .and_then(|at| confined.get(at + 1))
            .map(|profile| sha256_hex(profile.as_bytes()))
    }

    /// Run the profile once, against `/usr/bin/true`, before any command.
    ///
    /// `sandbox-exec` exits 65 for a profile it cannot compile and so can a
    /// command (`EX_DATAERR`), so a failure is told apart HERE, at open, and
    /// never by reading a command's standard error: at run time a 65 is the
    /// command's own.
    ///
    /// # Errors
    ///
    /// Returns [`Unavailable::ProfileRefused`] if the runner does not run
    /// `/usr/bin/true` under the profile and exit 0.
    pub fn validated(self, policy: &Policy) -> Result<Self, Unavailable> {
        let composed = self.compose(
            policy,
            Path::new(VALIDATION_TREE),
            &["/usr/bin/true".to_owned()],
        );
        let Some((program, rest)) = composed.split_first() else {
            return Err(Unavailable::ProfileRefused {
                said: "nothing was composed".to_owned(),
            });
        };
        let output =
            super::output_of(|| Command::new(program).args(rest).output()).map_err(|why| {
                Unavailable::ProfileRefused {
                    said: format!("{program} could not be run: {why}"),
                }
            })?;
        if output.status.success() {
            return Ok(self);
        }
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first = stderr.lines().next().unwrap_or_default();
        Err(Unavailable::ProfileRefused {
            said: match output.status.code() {
                Some(code) => format!("exit {code}: {first}"),
                None => format!("ended by a signal: {first}"),
            },
        })
    }
}

/// `path` resolved, or as given where it does not resolve.
fn canonical(path: &Path) -> String {
    canonical_prefix(path).to_string_lossy().into_owned()
}

/// The process groups a session's commands led, killed when the session
/// ends: by [`Seatbelt::end_session`], or when the last handle on the
/// confinement is DROPPED. The drop is the one place every session owner
/// reaches -- the gym's `diet-drive` and `serve`'s sessions alike -- without
/// either having to remember a call (#299, ruling 5982355781 (b), placed on
/// the confinement handle by 5982547482).
#[derive(Debug, Default)]
pub struct Groups(Mutex<Vec<u32>>);

impl Groups {
    /// Kill every recorded group, once.
    fn end(&self) {
        let groups = std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner));
        for group in groups {
            // A group whose members are all gone answers "no such process",
            // which is the outcome wanted; nothing to report.
            let _ = kill().args(["-KILL", "--", &format!("-{group}")]).output();
        }
    }
}

impl Drop for Groups {
    fn drop(&mut self) {
        self.end();
    }
}

/// The `kill` this module runs UNCONFINED, by absolute path, never looked up
/// on `PATH` (#299 review of 0706745, P): the command knows the drive's
/// `PATH`, and a `kill` it planted in a writable entry would otherwise run
/// outside the sandbox right after the command that planted it. `/bin/kill`
/// is there on macOS and on a merged-`/usr` Linux alike. It is the only
/// helper this module spawns by name; the runner itself is found once, at
/// open, before any command has run.
const KILL: &str = "/bin/kill";

/// A `kill` command, by absolute path.
pub(super) fn kill() -> Command {
    Command::new(KILL)
}

/// `command` spawned leading a process group of its own.
#[cfg(unix)]
fn spawn_leading_a_group(command: &mut Command) -> std::io::Result<Child> {
    use std::os::unix::process::CommandExt as _;
    command.process_group(0).spawn()
}

/// Process groups are a Unix notion; on any other target (the wasm32 read
/// side compiles this module) Seatbelt refuses rather than spawn a command
/// outside a group it could later kill.
#[cfg(not(unix))]
fn spawn_leading_a_group(_command: &mut Command) -> std::io::Result<Child> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Seatbelt leads each command in a process group, which exists only on Unix",
    ))
}
