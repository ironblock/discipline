//! What the declared paths hold, read at open, on both backends.
//!
//! Two things a profile cannot refuse by path, refused instead before
//! anything runs (#299 review of 3f7cc1e, findings A and B):
//!
//! - **A secret inside a writable path.** The secrets' deny matches a path,
//!   so a command that can write a secret's PARENT renames the parent and
//!   reads the secret at its new name -- measured, rc 0, under
//!   `sandbox_writable = ["~"]` for `~/.config/gh`. So a writable dir (and,
//!   at run, the worktree) that equals or contains a secret refuses to start,
//!   naming both. Default and regimen-added secrets alike.
//! - **A `.env` file inside a declared path.** The `.env` deny exempts the
//!   tree, so a `.env` in a writable dir read at its new name once its
//!   directory was moved into the tree (rc 0, in T1's own shape); and under
//!   `bwrap` the pattern cannot be masked at all. So a declared readable or
//!   writable path holding a `.env` or `.env.*` file refuses to start,
//!   naming it -- only paths the REGIMEN declared, never the policy's
//!   built-in system layout (`/usr`, `/etc`). The scan recurses, does NOT descend into `node_modules` or
//!   `.git`, does not follow symlinks, and exempts by exact name
//!   `.env.example`, `.env.sample`, `.env.template` and `.env.dist` (#299,
//!   5982547482). Names are compared ignoring ASCII case, as the profile's
//!   regex is matched, so the scan and the profile now agree on case and on
//!   the templates (review of 0dec34b, G). The worktree's own `.env` files
//!   are allowed (ruling 5981606817), and the worktree is checked for
//!   secrets (A) but never scanned for `.env`.
//!
//!   Accepted by ruling, and stated: a `.env` under a skipped `node_modules`
//!   or `.git` in a writable dir opens, and is readable once moved into the
//!   tree; and a `.env` the HOST places into a writable dir after open is
//!   not seen by the scan.
//! - **A declared path inside a secret.** `sandbox_readable =
//!   ["~/.ssh/id_rsa"]` or a writable `~/.ssh/sub` would be bound by `bwrap`
//!   with no mask (the masks cover secrets INSIDE a declared path). So a
//!   declared readable or writable path that equals or lies inside a secret
//!   refuses to start, naming both, on both backends (review of 0dec34b, I).

use std::path::{Path, PathBuf};

use super::policy::{DEFAULT_READABLE, Policy, canonical_prefix, resolved};

/// Templates, not secrets: exempt from the `.env` rule by exact name, here
/// and in the Seatbelt profile's deny alike (#299, 5982547482).
pub const ENV_TEMPLATES: &[&str] = &[".env.example", ".env.sample", ".env.template", ".env.dist"];

/// Directories the `.env` scan does not descend into.
const NOT_SCANNED: &[&str] = &["node_modules", ".git"];

/// Whether a file named `name` is a `.env` file the rule covers.
#[must_use]
///
/// **Ignoring ASCII case**, as Seatbelt's regex does (#299 review of
/// 0dec34b, G): `.ENV` and `.Env.Production` are `.env` files there -- and on a
/// case-insensitive volume `dotenv` opens one for `.env` -- so a scan that
/// matched only the lower-case names let a writable dir holding `d/.ENV`
/// open, and `mv d` into the tree read it. The template exemption ignores
/// case the same way, as the profile's does.
pub fn is_env_file(name: &str) -> bool {
    let lowered = name.to_ascii_lowercase();
    (lowered == ".env" || lowered.starts_with(".env."))
        && !ENV_TEMPLATES
            .iter()
            .any(|template| template.eq_ignore_ascii_case(name))
}

/// The first secret that `dir` equals or contains, if any.
fn secret_within(policy: &Policy, dir: &Path) -> Option<PathBuf> {
    policy
        .secrets
        .iter()
        .map(|secret| resolved(secret))
        .find(|secret| secret.starts_with(dir))
}

/// A `sandbox_writable` dir that equals or contains a secret, and the secret.
#[must_use]
pub fn secret_under_writable(policy: &Policy) -> Option<(PathBuf, PathBuf)> {
    policy.writable.iter().find_map(|dir| {
        let dir = resolved(dir);
        secret_within(policy, &dir).map(|secret| (dir, secret))
    })
}

/// A declared readable or writable path that equals or lies INSIDE a secret,
/// and the secret (review of 0dec34b, I). The built-in system layout is not
/// a declaration and is not checked.
#[must_use]
pub fn declared_inside_secret(policy: &Policy) -> Option<(PathBuf, PathBuf)> {
    policy
        .readable
        .iter()
        .filter(|path| !DEFAULT_READABLE.contains(&path.as_str()))
        .chain(&policy.writable)
        .find_map(|path| {
            let path = resolved(path);
            policy
                .secrets
                .iter()
                .map(|secret| resolved(secret))
                .find(|secret| path.starts_with(secret))
                .map(|secret| (path, secret))
        })
}

/// The declared readable or writable path that `file` lies in, if any:
/// where a credential the drive reads must NOT be, since a command can read
/// a declared path (#299, ruling 5983673467, H(1)).
#[must_use]
pub fn declared_path_holding(policy: &Policy, file: &Path) -> Option<PathBuf> {
    let file = canonical_prefix(file);
    policy
        .readable
        .iter()
        .chain(&policy.writable)
        .map(|path| resolved(path))
        .find(|path| file.starts_with(path))
}

/// Add the drive's own credential file to `policy`'s secrets, so no command
/// of the session reads it under any read mode, and no writable dir or tree
/// holding it opens (#299 review of 0706745, O; the regimen may extend the
/// set, and here the drive does).
///
/// # Errors
///
/// The declared path the file lies in: a regimen that lists it readable or
/// writable has declared it open, and the drive refuses rather than shorten
/// the regimen.
pub fn add_credential(policy: &mut Policy, file: &Path) -> Result<(), PathBuf> {
    if let Some(path) = declared_path_holding(policy, file) {
        return Err(path);
    }
    let file = canonical_prefix(file).to_string_lossy().into_owned();
    if !policy.secrets.contains(&file) {
        policy.secrets.push(file);
    }
    Ok(())
}

/// The secret the worktree equals or contains, if any.
#[must_use]
pub fn secret_under_tree(policy: &Policy, tree: &Path) -> Option<PathBuf> {
    let tree = std::fs::canonicalize(tree).unwrap_or_else(|_| tree.to_path_buf());
    secret_within(policy, &tree)
}

/// The first `.env` file in a path the REGIMEN declared readable or
/// writable. The policy's built-in system layout ([`DEFAULT_READABLE`]:
/// `/usr`, `/etc`) is not scanned: the ruling covers "a declared path", the
/// threat is the host's own secrets rather than system files, and scanning
/// `/usr` cost about a second per open.
#[must_use]
pub fn env_file_declared(policy: &Policy) -> Option<PathBuf> {
    env_file_declared_beyond(policy, DEFAULT_READABLE)
}

/// The same, with the built-in defaults named, so a test can stand one in.
#[must_use]
pub fn env_file_declared_beyond(policy: &Policy, defaults: &[&str]) -> Option<PathBuf> {
    policy
        .readable
        .iter()
        .filter(|path| !defaults.contains(&path.as_str()))
        .chain(&policy.writable)
        .find_map(|path| env_file_in(&resolved(path)))
}

/// The first `.env` file at or under `path`.
fn env_file_in(path: &Path) -> Option<PathBuf> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if meta.is_file() {
        let name = path.file_name()?.to_str()?;
        return is_env_file(name).then(|| path.to_path_buf());
    }
    if !meta.is_dir() {
        return None;
    }
    let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
        .ok()?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect();
    entries.sort();
    entries.into_iter().find_map(|entry| {
        let skipped = entry
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| NOT_SCANNED.contains(&name));
        if skipped { None } else { env_file_in(&entry) }
    })
}

/// Under `bwrap`, what masks each secret that lies inside a declared
/// readable or writable path: `--tmpfs` for a directory, `--ro-bind
/// /dev/null` for a file, at the path the secret has INSIDE the sandbox (the
/// bind is at the path as declared). A secret that does not exist needs no
/// mask.
#[must_use]
pub fn masks(policy: &Policy, bound: &[(String, PathBuf)]) -> Vec<Vec<String>> {
    let mut out = Vec::new();
    for secret in policy.secrets.iter().map(|secret| resolved(secret)) {
        let Some(inside) = bound.iter().find_map(|(at, real)| {
            secret
                .strip_prefix(real)
                .ok()
                .map(|rest| Path::new(at).join(rest))
        }) else {
            continue;
        };
        let inside = inside.to_string_lossy().into_owned();
        if secret.is_dir() {
            out.push(vec!["--tmpfs".to_owned(), inside]);
        } else if secret.exists() {
            out.push(vec!["--ro-bind".to_owned(), "/dev/null".to_owned(), inside]);
        }
    }
    out
}
