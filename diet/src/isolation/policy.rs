//! What the regimen says a command may touch.
//!
//! Two facts from prior work decide the shape. A session's commands must not
//! be able to reach the operator's environment beyond the working tree -- an
//! archived drive includes an agent `rm`-ing outside its directory once,
//! caught by a guard that should not have been the last line. And the
//! mechanism has to be cheap enough to use *by default*, or it becomes the
//! guard nobody enables.
//!
//! So: a lightweight OS-native sandbox as the default, and escalation to a
//! full VM only where a regimen declares it needs one. The cheap sandbox is
//! the floor everyone gets; the VM is the ceiling for the few who ask.
//!
//! Nothing here is a guess about the host. The read-only system paths and the
//! symlinks that make a merged-`/usr` layout work are **declared**, with a
//! shipped default that says which layout it describes. A module that
//! inferred a distribution's layout and got it wrong would produce a sandbox
//! that fails to start, or -- far worse -- one that starts and does not
//! confine.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::client::vocabulary;
use crate::formats::regimen::{Regimen, Value};

vocabulary! {
    /// How confined a session's commands are.
    Isolation {
        /// No confinement, declared on purpose. For replay, and for fixtures
        /// somebody has read. Explicit rather than the absence of a setting:
        /// an unconfined drive is a decision, and a decision nobody wrote
        /// down is one nobody can be asked about.
        None => "none",
        /// The default for a live drive: the working tree read-write,
        /// everything else read-only or absent, network as declared.
        Sandbox => "sandbox",
        /// A full virtual machine, for a regimen that declares it needs one.
        Vm => "vm",
    }
}

vocabulary! {
    /// What a confined command may reach on the network.
    Network {
        /// Nothing, beyond any declared `network_loopback` port. Under `bwrap`
        /// the command gets a namespace with no route out; under Seatbelt the
        /// profile allows no network operation but those ports.
        None => "none",
        /// The host's network, declared.
        Host => "host",
    }
}

vocabulary! {
    /// What a sandboxed command may read.
    Reads {
        /// The system paths, the tree, and each `sandbox_readable` path.
        Listed => "listed",
        /// Everything, except the secrets (Seatbelt only; on Linux it refuses
        /// to start).
        All => "all",
    }
}

/// The secrets every policy denies, reads and writes alike, whatever else it
/// allows (#29, planning 5981447912 and 5981606817). `~` is the drive's
/// `HOME`. A regimen's `sandbox_secrets` ADDS to these and never removes
/// one. `.env` files outside the tree are denied by a rule of their own
/// (`seatbelt.rs`), not by a path here.
pub const DEFAULT_SECRETS: &[&str] = &[
    "~/.ssh",
    "~/.aws",
    "~/.config/gh",
    "~/Library/Keychains",
    "~/.npmrc",
    "~/.netrc",
    "~/.docker/config.json",
    "~/.gnupg",
];

/// The system layout [`Policy::merged_usr`] reads: the policy's built-in
/// default, not a path any regimen declared.
pub const DEFAULT_READABLE: &[&str] = &["/usr", "/etc"];

/// The environment every command receives, if the drive has it: nothing
/// else, unless a regimen's `env_passthrough` names it (#29, planning
/// 5981606817).
pub const DEFAULT_ENVIRONMENT: &[&str] = &["PATH", "HOME", "LANG", "TERM", "TMPDIR"];

/// The keys a regimen carries for this module.
pub const ISOLATION: &str = "isolation";
/// Whether a confined command may reach the network.
pub const NETWORK: &str = "network";
/// The read-only paths the sandbox binds.
pub const SANDBOX_READABLE: &str = "sandbox_readable";
/// The symlinks the sandbox's root carries, as `name = "target"`. A Linux
/// layout key: Seatbelt ignores it, because under it the command sees the
/// host's own filesystem, symlinks and all.
pub const SANDBOX_SYMLINKS: &str = "sandbox_symlinks";
/// The loopback ports a command may use under `network = "none"`.
///
/// **On macOS a declared port is open on every address of this host, not only
/// 127.0.0.1**: Seatbelt's grammar admits only `*` or `localhost` as a host,
/// and its `localhost` is all of the host's addresses, so a bind of
/// `0.0.0.0:<port>` is admitted and reachable from the LAN. Connects to other
/// hosts on the port are refused. Recorded as `loopback_scope = "host"` on the
/// equipment (#299, ruling 5977176035); a regimen that runs a server should
/// bind it to `127.0.0.1` itself. On Linux the key refuses to start.
pub const NETWORK_LOOPBACK: &str = "network_loopback";
/// Whether a sandboxed command reads the listed paths or everything but the
/// secrets: `"listed"` (the default) or `"all"`.
pub const SANDBOX_READS: &str = "sandbox_reads";
/// Paths ADDED to [`DEFAULT_SECRETS`]: absolute, or `~/`-relative.
pub const SANDBOX_SECRETS: &str = "sandbox_secrets";
/// Directories a sandboxed command may write besides the tree: absolute.
pub const SANDBOX_WRITABLE: &str = "sandbox_writable";
/// Environment variable names ADDED to [`DEFAULT_ENVIRONMENT`].
pub const ENV_PASSTHROUGH: &str = "env_passthrough";

/// What a session's commands run under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// The mechanism.
    pub isolation: Isolation,
    /// The network.
    pub network: Network,
    /// Paths bound read-only into the sandbox, in the order declared.
    pub readable: Vec<String>,
    /// Symlinks made in the sandbox's root: name to target. Linux only;
    /// Seatbelt ignores them.
    pub symlinks: BTreeMap<String, String>,
    /// Loopback ports a command may use under `network = "none"`, in the
    /// order declared. On macOS each is open on every address of this host,
    /// not only 127.0.0.1 (see [`NETWORK_LOOPBACK`]). Built under Seatbelt only: on Linux a non-empty
    /// list refuses to start.
    pub loopback: Vec<u16>,
    /// What a sandboxed command may read.
    pub reads: Reads,
    /// The secrets denied whatever else is allowed: [`DEFAULT_SECRETS`] and
    /// then the regimen's additions, as written (`~` is expanded when the
    /// command is composed).
    pub secrets: Vec<String>,
    /// Directories writable besides the tree, in the order declared.
    pub writable: Vec<String>,
    /// The environment variables a command receives, if the drive has them:
    /// [`DEFAULT_ENVIRONMENT`] and then the regimen's additions. Every other
    /// variable is cleared, on every backend.
    pub environment: Vec<String>,
}

impl Policy {
    /// The default for a live drive on a merged-`/usr` host.
    ///
    /// **This is a layout, not a fact about every Linux.** Debian, Ubuntu,
    /// Fedora and Arch put the system under `/usr` and make `/bin`, `/sbin`,
    /// `/lib` and `/lib64` symlinks into it; a host that does not can declare
    /// its own `sandbox_readable` and `sandbox_symlinks`. It is written here
    /// as data rather than inferred at run time, because a module that
    /// guessed the layout would produce either a sandbox that fails to start
    /// or -- far worse -- one that starts and does not confine.
    ///
    /// `/tmp` is deliberately absent, and so is the home directory. A command
    /// that writes to `/tmp` under this policy is denied rather than quietly
    /// writing into a throwaway the caller will never see again, and a
    /// home-directory secret is not there to read.
    #[must_use]
    pub fn merged_usr() -> Self {
        Self {
            isolation: Isolation::Sandbox,
            network: Network::None,
            readable: DEFAULT_READABLE
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
            symlinks: [
                ("bin", "usr/bin"),
                ("sbin", "usr/sbin"),
                ("lib", "usr/lib"),
                ("lib64", "usr/lib64"),
            ]
            .into_iter()
            .map(|(name, target)| (name.to_owned(), target.to_owned()))
            .collect(),
            loopback: Vec::new(),
            reads: Reads::Listed,
            secrets: DEFAULT_SECRETS
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
            writable: Vec::new(),
            environment: DEFAULT_ENVIRONMENT
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        }
    }

    /// No confinement, declared.
    #[must_use]
    pub fn unconfined() -> Self {
        Self {
            isolation: Isolation::None,
            network: Network::Host,
            readable: Vec::new(),
            symlinks: BTreeMap::new(),
            loopback: Vec::new(),
            reads: Reads::Listed,
            secrets: DEFAULT_SECRETS
                .iter()
                .map(|path| (*path).to_owned())
                .collect(),
            writable: Vec::new(),
            environment: DEFAULT_ENVIRONMENT
                .iter()
                .map(|name| (*name).to_owned())
                .collect(),
        }
    }

    /// Read the policy `regimen` declares.
    ///
    /// A regimen that says nothing about isolation gets [`Policy::merged_usr`]
    /// -- the sandbox, not the absence of one. That is the "cheap by default"
    /// ruling in its operative form: the way to run unconfined is to write
    /// `isolation = "none"`, and the way to run confined is to say nothing.
    ///
    /// # Errors
    ///
    /// Returns [`PolicyError`] for a value of the wrong shape or a name
    /// outside the vocabulary.
    pub fn from_regimen(regimen: &Regimen) -> Result<Self, PolicyError> {
        let mut policy = match named(regimen, ISOLATION, Isolation::ALL, Isolation::tag)? {
            Some(Isolation::None) => Self::unconfined(),
            Some(Isolation::Vm) => Self {
                isolation: Isolation::Vm,
                ..Self::merged_usr()
            },
            Some(Isolation::Sandbox) | None => Self::merged_usr(),
        };

        if let Some(network) = named(regimen, NETWORK, Network::ALL, Network::tag)? {
            // Refused rather than recorded. Unconfined means nothing enforces
            // a network policy, so a drive declaring both ran on the
            // operator's network and wrote `network = "none"` into its record
            // -- a result mislabelled by exactly the mechanism this module
            // exists to stop one layer up. The way to declare a run that makes
            // no network calls is to say so where somebody can check it, not
            // in a field nothing enforces.
            if policy.isolation == Isolation::None && network == Network::None {
                return Err(PolicyError::Unenforceable {
                    key: NETWORK,
                    under: Isolation::None,
                });
            }
            policy.network = network;
        }

        if let Some(value) = regimen.get(SANDBOX_READABLE) {
            let Value::Array(items) = value else {
                return Err(PolicyError::NotAListOfPaths(SANDBOX_READABLE));
            };
            let mut readable = Vec::with_capacity(items.len());
            for item in items {
                let Value::String(path) = item else {
                    return Err(PolicyError::NotAListOfPaths(SANDBOX_READABLE));
                };
                if !is_rooted(path) {
                    return Err(PolicyError::NotAbsolute(path.clone()));
                }
                readable.push(path.clone());
            }
            policy.readable = readable;
        }

        if let Some(value) = regimen.get(SANDBOX_SYMLINKS) {
            let Value::Table(table) = value else {
                return Err(PolicyError::NotATableOfSymlinks);
            };
            let mut symlinks = BTreeMap::new();
            for (name, target) in table {
                let Value::String(target) = target else {
                    return Err(PolicyError::NotATableOfSymlinks);
                };
                symlinks.insert(name.clone(), target.clone());
            }
            policy.symlinks = symlinks;
        }

        if let Some(value) = regimen.get(NETWORK_LOOPBACK) {
            let Value::Array(items) = value else {
                return Err(PolicyError::NotAListOfPorts);
            };
            let mut loopback = Vec::with_capacity(items.len());
            for item in items {
                let Value::Integer(port) = item else {
                    return Err(PolicyError::NotAListOfPorts);
                };
                match u16::try_from(*port) {
                    Ok(port) if port != 0 => loopback.push(port),
                    _ => return Err(PolicyError::NotAListOfPorts),
                }
            }
            // A declaration of what `none` lets through. Under `host` every
            // port is already reachable, so a record carrying both would name
            // an allowance that allowed nothing.
            if !loopback.is_empty() && policy.network != Network::None {
                return Err(PolicyError::LoopbackWithoutNone {
                    network: policy.network,
                });
            }
            policy.loopback = loopback;
        }

        if let Some(reads) = named(regimen, SANDBOX_READS, Reads::ALL, Reads::tag)? {
            policy.reads = reads;
        }
        for path in paths(regimen, SANDBOX_SECRETS)? {
            if !is_rooted(&path) {
                return Err(PolicyError::NotAbsolute(path));
            }
            if !policy.secrets.contains(&path) {
                policy.secrets.push(path);
            }
        }
        for path in paths(regimen, SANDBOX_WRITABLE)? {
            if !is_rooted(&path) {
                return Err(PolicyError::NotAbsolute(path));
            }
            policy.writable.push(path);
        }
        for name in paths(regimen, ENV_PASSTHROUGH)? {
            let mut chars = name.chars();
            let named_well = chars
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !named_well {
                return Err(PolicyError::NotAnEnvironmentName(name));
            }
            if !policy.environment.contains(&name) {
                policy.environment.push(name);
            }
        }

        Ok(policy)
    }
}

/// Why a regimen does not describe an isolation policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// A key that must hold one of a closed set of names holds something
    /// else, or a name nobody defined.
    NotAName {
        /// The key.
        key: &'static str,
        /// What was written.
        written: String,
        /// What it could have been.
        allowed: Vec<&'static str>,
    },
    /// A setting nothing under this mechanism can enforce, so a record
    /// carrying it would be a claim about the drive that nothing made true.
    Unenforceable {
        /// The key.
        key: &'static str,
        /// The mechanism it cannot be enforced under.
        under: Isolation,
    },
    /// A key that must hold a list of absolute paths holds something else.
    NotAListOfPaths(&'static str),
    /// A path that is not absolute. A relative path in a sandbox policy is a
    /// path whose meaning depends on where the harness happened to be.
    NotAbsolute(String),
    /// The symlink table is not a table of names to targets.
    NotATableOfSymlinks,
    /// `network_loopback` is not a list of ports, each 1 to 65535.
    NotAListOfPorts,
    /// A key that must hold a list of strings holds something else.
    NotAListOfStrings(&'static str),
    /// An `env_passthrough` entry that is not a variable name.
    NotAnEnvironmentName(String),
    /// `network_loopback` declared under a network other than `none`.
    LoopbackWithoutNone {
        /// The network it was declared under.
        network: Network,
    },
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAName {
                key,
                written,
                allowed,
            } => write!(
                f,
                "`{key} = {written}` is not one of {}",
                allowed.join(", ")
            ),
            Self::Unenforceable { key, under } => write!(
                f,
                "`{key}` cannot be enforced under `isolation = \"{}\"`; a record \
                 carrying it would say something about the drive that nothing made \
                 true",
                under.tag()
            ),
            Self::NotAListOfPaths(key) => {
                write!(f, "`{key}` is not a list of absolute paths")
            }
            Self::NotAbsolute(path) => write!(
                f,
                "`{path}` is neither absolute nor `~`-relative, and a relative path in a \
                 sandbox policy means whatever the harness's working directory happened \
                 to be"
            ),
            Self::NotATableOfSymlinks => {
                write!(f, "`{SANDBOX_SYMLINKS}` is not a table of names to targets")
            }
            Self::NotAListOfStrings(key) => write!(f, "`{key}` is not a list of strings"),
            Self::NotAnEnvironmentName(name) => write!(
                f,
                "`{name}` in `{ENV_PASSTHROUGH}` is not an environment variable name \
                 (`[A-Za-z_][A-Za-z0-9_]*`)"
            ),
            Self::NotAListOfPorts => {
                write!(
                    f,
                    "`{NETWORK_LOOPBACK}` is not a list of ports from 1 to 65535"
                )
            }
            Self::LoopbackWithoutNone { network } => write!(
                f,
                "`{NETWORK_LOOPBACK}` is valid only with `{NETWORK} = \"none\"`; under \
                 `{NETWORK} = \"{}\"` every port is already reachable, and a record \
                 carrying both would name an allowance that allowed nothing",
                network.tag()
            ),
        }
    }
}

impl Error for PolicyError {}

/// Whether a policy accepts `path`: absolute, `~`, or `~/…` (#301,
/// 5981929426: regimens carry tilde paths literally, because the hygiene
/// gate refuses a home path written out). `~user/` and every other relative
/// form are refused.
fn is_rooted(path: &str) -> bool {
    path.starts_with('/') || path == "~" || path.starts_with("~/")
}

/// `path` with a leading `~` made the drive's `HOME` -- the same `HOME`
/// the command is given. With no `HOME` it stays as written, which Seatbelt
/// refuses at open: a drive with no `HOME` does not start rather than leave
/// `~` meaning nothing.
#[must_use]
pub fn home_expanded(path: &str) -> PathBuf {
    let home = std::env::var_os("HOME").filter(|home| !home.is_empty());
    let (Some(rest), Some(home)) = (path.strip_prefix('~'), home) else {
        return PathBuf::from(path);
    };
    if rest.is_empty() {
        PathBuf::from(home)
    } else if let Some(under) = rest.strip_prefix('/') {
        PathBuf::from(home).join(under)
    } else {
        PathBuf::from(path)
    }
}

/// `path` with `~` expanded ([`home_expanded`]) and resolved, or as
/// expanded where it does not resolve.
#[must_use]
pub fn resolved(path: &str) -> PathBuf {
    canonical_prefix(&home_expanded(path))
}

/// `path` with its longest EXISTING ancestor resolved and the rest
/// appended (#299 review of 0dec34b, K). A secret that does not exist yet
/// is then compared, and denied in the profile, at the path it will have:
/// under a symlinked `HOME` the expanded path alone would never match.
///
/// **Limit, stated** (review of 0706745, U): the part that does not exist
/// is compared as written, case-sensitively, while a macOS home volume is
/// usually case-insensitive. A writable `~/.MYSECRET` beside a declared
/// secret `~/.mysecret/tok` that does not exist yet is not refused.
#[must_use]
pub fn canonical_prefix(path: &Path) -> PathBuf {
    let mut rest = Vec::new();
    let mut at = path;
    loop {
        if let Ok(real) = std::fs::canonicalize(at) {
            return rest.iter().rev().fold(real, |done, part| done.join(part));
        }
        match (at.parent(), at.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                at = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The strings under `key`, or none if the regimen does not carry it.
fn paths(regimen: &Regimen, key: &'static str) -> Result<Vec<String>, PolicyError> {
    let Some(value) = regimen.get(key) else {
        return Ok(Vec::new());
    };
    let Value::Array(items) = value else {
        return Err(PolicyError::NotAListOfStrings(key));
    };
    items
        .iter()
        .map(|item| match item {
            Value::String(text) => Ok(text.clone()),
            _ => Err(PolicyError::NotAListOfStrings(key)),
        })
        .collect()
}

/// One of a closed set of names under `key`, if the regimen carries one.
fn named<T: Copy>(
    regimen: &Regimen,
    key: &'static str,
    all: &'static [T],
    tag: impl Fn(T) -> &'static str,
) -> Result<Option<T>, PolicyError> {
    let Some(value) = regimen.get(key) else {
        return Ok(None);
    };
    let allowed: Vec<&'static str> = all.iter().map(|item| tag(*item)).collect();
    let Value::String(written) = value else {
        return Err(PolicyError::NotAName {
            key,
            written: format!("{value:?}"),
            allowed,
        });
    };
    all.iter()
        .copied()
        .find(|item| tag(*item) == written)
        .map(Some)
        .ok_or_else(|| PolicyError::NotAName {
            key,
            written: written.clone(),
            allowed,
        })
}
