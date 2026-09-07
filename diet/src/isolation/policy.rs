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
        /// Nothing. The command gets a namespace with no route out.
        None => "none",
        /// The host's network, declared.
        Host => "host",
    }
}

/// The keys a regimen carries for this module.
pub const ISOLATION: &str = "isolation";
/// Whether a confined command may reach the network.
pub const NETWORK: &str = "network";
/// The read-only paths the sandbox binds.
pub const SANDBOX_READABLE: &str = "sandbox_readable";
/// The symlinks the sandbox's root carries, as `name = "target"`.
pub const SANDBOX_SYMLINKS: &str = "sandbox_symlinks";

/// What a session's commands run under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// The mechanism.
    pub isolation: Isolation,
    /// The network.
    pub network: Network,
    /// Paths bound read-only into the sandbox, in the order declared.
    pub readable: Vec<String>,
    /// Symlinks made in the sandbox's root: name to target.
    pub symlinks: BTreeMap<String, String>,
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
            readable: vec!["/usr".to_owned(), "/etc".to_owned()],
            symlinks: [
                ("bin", "usr/bin"),
                ("sbin", "usr/sbin"),
                ("lib", "usr/lib"),
                ("lib64", "usr/lib64"),
            ]
            .into_iter()
            .map(|(name, target)| (name.to_owned(), target.to_owned()))
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
                if !path.starts_with('/') {
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
    /// A key that must hold a list of absolute paths holds something else.
    NotAListOfPaths(&'static str),
    /// A path that is not absolute. A relative path in a sandbox policy is a
    /// path whose meaning depends on where the harness happened to be.
    NotAbsolute(String),
    /// The symlink table is not a table of names to targets.
    NotATableOfSymlinks,
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
            Self::NotAListOfPaths(key) => {
                write!(f, "`{key}` is not a list of absolute paths")
            }
            Self::NotAbsolute(path) => write!(
                f,
                "`{path}` is not absolute, and a relative path in a sandbox policy \
                 means whatever the harness's working directory happened to be"
            ),
            Self::NotATableOfSymlinks => {
                write!(f, "`{SANDBOX_SYMLINKS}` is not a table of names to targets")
            }
        }
    }
}

impl Error for PolicyError {}

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
