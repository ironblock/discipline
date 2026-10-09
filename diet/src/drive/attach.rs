//! The operator's PNG, attached to their ask (#372, the loop half; ruled at
//! 5989411005).
//!
//! When the operator's message names a PNG path, the drive attaches that
//! file to the same user message as an `image_url` part, logs it on the
//! `ask` line by reference, and keeps a copy beside the log. T1's tool list
//! is untouched: the operator attaches, the model does not ask to see.
//!
//! # The path rule
//!
//! A candidate is a whitespace-separated token of the ask, trimmed of
//! surrounding quotes and backticks and of trailing punctuation, that ends
//! in `.png`, any case ([`candidates`]). Its `~` is the drive's `HOME`
//! ([`home_expanded`], the policy's own expansion); a relative path is
//! resolved against the worktree, and refused when there is none. The file
//! must lie inside the regimen's read scope ([`readable`]) -- never a
//! secret -- exist, and begin with the PNG signature: the media type is the
//! bytes', never the extension's. The ask's words are sent and logged as
//! the operator wrote them; nothing is rewritten.
//!
//! # The read
//!
//! The path check is a pre-filter that gives a refusal its reason; the read
//! is the boundary (`read_through`, #461 F1). Under a sandbox the file is
//! read once, by `/bin/cat` run under the session's own confinement, so the
//! kernel judges the open against the profile the model's commands run
//! under: a path swapped for a link to a secret after the check is still
//! not read. Every use of the file -- digest, size, signature, copy, data
//! URI -- is of that one read's bytes. Unconfined (`isolation = "none"`, or
//! a regimen that runs no commands) the file is read directly. A file
//! longer than [`MAX_BYTES`] is refused, and a read that does not finish
//! within its patience (a FIFO) is killed and refused.
//!
//! A named `.png` that fails any of this refuses the whole ask, before
//! anything is logged, copied or sent ([`Unattachable`]): `serve` answers
//! `400` with the path and the check, as it does every command it could not
//! log.
//!
//! # The copy
//!
//! With a recording (`--log` or `--record`), each file is copied to
//! `<recording dir>/files/<sha256>` before the request goes out, and the
//! `ask` line names that path. A copy already there with the same digest is
//! kept; one with other bytes refuses the ask. With no recording the image
//! is attached and sent, nothing is copied, and the line carries no
//! `files`: a reference to a copy nobody kept would name nothing.

use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::client::shape::{Message, Role};
use crate::client::vocabulary;
use crate::digest::sha256_hex;
use crate::formats::log::RecordedFile;
use crate::isolation::Reads;
use crate::isolation::declared::is_env_file;
use crate::isolation::policy::{canonical_prefix, home_expanded, resolved};
use crate::isolation::{Confinement, Policy};

/// The media type every attachment is sent and logged as: the only one
/// sniffed (ruled 5989411005: "names a PNG path").
pub const MEDIA_TYPE: &str = "image/png";

/// The directory, under the recording's, that holds the copies.
pub const FILES: &str = "files";

/// The most bytes an attachment may be: a file longer than this is refused
/// [`Check::TooLarge`], and no more than one byte past it is ever read.
pub const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// How long the read of one attachment may take before its reader is killed
/// and the file refused [`Check::Unreadable`]: a FIFO named `x.png` would
/// otherwise hold the connection open forever (#461 F2).
const READ_PATIENCE: Duration = Duration::from_secs(10);

/// The eight bytes every PNG begins with (the PNG specification, 5.2).
const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";

/// What surrounds a path in prose: trimmed from both ends of a token.
const QUOTES: &[char] = &['"', '\'', '`'];

/// What follows a path in prose: trimmed from the end of a token.
const TRAILING: &[char] = &['"', '\'', '`', '.', ',', ';', ':', '!', '?', ')', ']', '}'];

/// The extension a candidate ends in, compared ignoring ASCII case.
const EXTENSION: &str = ".png";

vocabulary! {
    /// Which check a named `.png` failed.
    Check {
        /// No regimen declares a read scope, so nothing is readable.
        NoReadScope => "no-read-scope",
        /// The path is relative and the session has no worktree to resolve
        /// it against.
        RelativeWithoutWorktree => "relative-without-worktree",
        /// The path is, or lies inside, a secret the policy denies.
        Secret => "secret",
        /// The path lies outside the regimen's read scope.
        OutsideReadScope => "outside-read-scope",
        /// Nothing is there.
        Missing => "missing",
        /// Something is there and could not be read as a file.
        Unreadable => "unreadable",
        /// The bytes do not begin with the PNG signature.
        NotPng => "not-png",
        /// The recording already holds other bytes under this digest's name.
        CopyConflict => "copy-conflict",
        /// The copy could not be written.
        CopyFailed => "copy-failed",
        /// The file is longer than [`MAX_BYTES`].
        TooLarge => "too-large",
        /// The ask names a digest that was never uploaded to this server.
        NotUploaded => "not-uploaded",
    }
}

/// Why an ask's named `.png` cannot be attached. The ask is refused whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unattachable {
    /// The path as the operator wrote it, trimmed.
    pub path: String,
    /// The check it failed.
    pub check: Check,
    /// What the check found.
    pub why: String,
}

impl fmt::Display for Unattachable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` is not attached ({}): {}",
            self.path,
            self.check.tag(),
            self.why
        )
    }
}

impl std::error::Error for Unattachable {}

/// What an ask's attachments are checked against and copied to.
///
/// [`Default`] has no read scope, so every named `.png` is refused: a
/// session with no regimen has declared nothing readable.
#[derive(Debug, Clone, Default)]
pub struct Attaching {
    /// The regimen's policy, whose read scope and secrets decide what may be
    /// attached; `None` with no regimen.
    pub policy: Option<Policy>,
    /// The worktree: in the read scope, and what a relative path is resolved
    /// against.
    pub worktree: Option<PathBuf>,
    /// The confinement the session's commands run under, which reads each
    /// attachment; `None` when the regimen runs no commands.
    pub confinement: Option<Confinement>,
    /// The recording's directory, when the session keeps a log or a record:
    /// the copies go under its [`FILES`], and the `ask` line's paths are
    /// relative to it.
    pub recording: Option<PathBuf>,
}

/// An ask, ready for the session: the user message it sends -- the words
/// and any images -- and the files its `ask` line names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    /// The turn's user message.
    pub message: Message,
    /// The `ask` line's `files`, in the order attached: empty when nothing
    /// was attached or nothing was copied.
    pub files: Vec<RecordedFile>,
}

impl Attached {
    /// An ask with nothing attached.
    #[must_use]
    pub fn plain(text: &str) -> Self {
        Self {
            message: Message::new(Role::User, text),
            files: Vec::new(),
        }
    }
}

/// A PNG posted to `serve`'s `/files` (#513): its reference and its bytes,
/// held for the session so an ask can name it by digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Upload {
    /// The reference an `ask` line carries for it.
    pub file: RecordedFile,
    /// Its bytes, exactly as posted.
    pub bytes: std::sync::Arc<Vec<u8>>,
}

/// `bytes`, posted to `/files`, checked as a named PNG's read is -- the
/// signature, [`MAX_BYTES`] -- and, with a recording, copied to
/// `<recording>/files/<sha256>` as an ask's named PNG is.
///
/// # Errors
///
/// [`Check::NotPng`], [`Check::TooLarge`], or the copy's
/// [`Check::CopyConflict`] or [`Check::CopyFailed`].
pub fn uploaded(bytes: Vec<u8>, recording: Option<&Path>) -> Result<Upload, (Check, String)> {
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES {
        return Err((
            Check::TooLarge,
            format!("the upload is longer than the {MAX_BYTES} bytes an attachment may be"),
        ));
    }
    if !bytes.starts_with(SIGNATURE) {
        return Err((
            Check::NotPng,
            "the upload does not begin with the PNG signature".to_owned(),
        ));
    }
    let file = reference(&bytes);
    if let Some(recording) = recording {
        kept(recording, &file, &bytes)?;
    }
    Ok(Upload {
        file,
        bytes: std::sync::Arc::new(bytes),
    })
}

/// The tokens of `text` that name a PNG, trimmed, in order.
#[must_use]
pub fn candidates(text: &str) -> Vec<&str> {
    text.split_whitespace()
        .map(|token| token.trim_start_matches(QUOTES).trim_end_matches(TRAILING))
        .filter(|token| {
            token.len() > EXTENSION.len()
                && token
                    .get(token.len() - EXTENSION.len()..)
                    .is_some_and(|tail| tail.eq_ignore_ascii_case(EXTENSION))
        })
        .collect()
}

/// `text` as an ask, with each PNG it names attached (see the module's
/// rule), and, with a recording, each copied and named.
///
/// # Errors
///
/// [`Unattachable`] for the first named `.png` that fails a check, or whose
/// copy cannot be kept; then nothing was copied for it, and the ask must not
/// be sent.
pub fn attached(text: &str, attaching: &Attaching) -> Result<Attached, Unattachable> {
    attached_with(text, &[], attaching)
}

/// [`attached`], with `uploads` -- PNGs posted to `/files` and named by the
/// ask's `files` (#513) -- attached first, in their order, then the PNGs
/// `text` names. Each upload is named on the `ask` line before the named
/// files, recording or not; with a recording its copy is kept again (whole,
/// or refused).
///
/// # Errors
///
/// As [`attached`]; an upload's copy that cannot be kept is refused under
/// its digest.
pub fn attached_with(
    text: &str,
    uploads: &[Upload],
    attaching: &Attaching,
) -> Result<Attached, Unattachable> {
    // Every check first, so a refusal copies nothing: not even the files
    // named before the one refused.
    let mut found = Vec::new();
    for named in candidates(text) {
        let refused = |check: Check, why: String| Unattachable {
            path: named.to_owned(),
            check,
            why,
        };
        let (file, bytes) =
            checked(named, attaching).map_err(|(check, why)| refused(check, why))?;
        found.push((named, file, bytes));
    }
    let mut message = Message::new(Role::User, text);
    for upload in uploads {
        message = crate::client::attach(message, &upload.file, &upload.bytes).map_err(|why| {
            Unattachable {
                path: upload.file.sha256.clone(),
                check: Check::Unreadable,
                why: why.to_string(),
            }
        })?;
    }
    for (named, file, bytes) in &found {
        message = crate::client::attach(message, file, bytes).map_err(|why| Unattachable {
            path: (*named).to_owned(),
            check: Check::Unreadable,
            why: why.to_string(),
        })?;
    }
    let mut files = Vec::new();
    // An upload is named on the line with or without a recording: serve
    // holds its bytes for the session and answers them at `GET
    // /files/<sha256>`, so the reference names something either way (#513).
    for upload in uploads {
        if let Some(recording) = &attaching.recording {
            kept(recording, &upload.file, &upload.bytes).map_err(|(check, why)| Unattachable {
                path: upload.file.sha256.clone(),
                check,
                why,
            })?;
        }
        files.push(upload.file.clone());
    }
    if let Some(recording) = &attaching.recording {
        for (named, file, bytes) in &found {
            kept(recording, file, bytes).map_err(|(check, why)| Unattachable {
                path: (*named).to_owned(),
                check,
                why,
            })?;
            files.push(file.clone());
        }
    }
    Ok(Attached { message, files })
}

/// The file `named` names, checked, as the reference the log would carry
/// and its bytes: read once, by [`read_through`], and never reopened.
fn checked(named: &str, attaching: &Attaching) -> Result<(RecordedFile, Vec<u8>), (Check, String)> {
    let Some(policy) = &attaching.policy else {
        return Err((
            Check::NoReadScope,
            "no regimen declares a read scope, so nothing may be attached".to_owned(),
        ));
    };
    let expanded = home_expanded(named);
    let located = if expanded.is_absolute() {
        expanded
    } else if let Some(tree) = &attaching.worktree {
        tree.join(expanded)
    } else {
        return Err((
            Check::RelativeWithoutWorktree,
            "a relative path is resolved against the worktree, and this session has none"
                .to_owned(),
        ));
    };
    // Resolved before it is compared, so a link inside the scope to a file
    // outside it is judged where it leads.
    let file = canonical_prefix(&located);
    readable(policy, attaching.worktree.as_deref(), &file).map_err(|check| {
        let why = match check {
            Check::Secret => format!("{} is a secret the policy denies", file.display()),
            _ => format!("{} is outside the regimen's read scope", file.display()),
        };
        (check, why)
    })?;
    let bytes = read_through(attaching, policy, &file, READ_PATIENCE)?;
    if !bytes.starts_with(SIGNATURE) {
        return Err((
            Check::NotPng,
            format!("{} does not begin with the PNG signature", file.display()),
        ));
    }
    let file = reference(&bytes);
    Ok((file, bytes))
}

/// The reference an `ask` line carries for `bytes`: its copy's path under
/// the recording, its digest, its media type and its length.
fn reference(bytes: &[u8]) -> RecordedFile {
    let sha256 = sha256_hex(bytes);
    RecordedFile {
        path: format!("{FILES}/{sha256}"),
        sha256,
        media_type: MEDIA_TYPE.to_owned(),
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
    }
}

/// `file`'s bytes, read ONCE: the digest, the size, the signature, the copy
/// and the data URI are all taken from what this returns, and the path is
/// never opened again (#461 F1).
///
/// The path check before this is a pre-filter that names a clear reason;
/// this read is the boundary. With a sandbox, `file` is read by [`CAT`] run
/// under the session's own confinement, so the kernel judges the open --
/// symlinks included -- against the same profile the model's commands run
/// under, and a path swapped for a link to a secret after the check is
/// still not read. Unconfined (`isolation = "none"`, or a regimen that runs
/// no commands) it is read directly: `none` declares no boundary, and with no
/// commands nothing the model runs can swap the path.
///
/// At most [`MAX_BYTES`] + 1 bytes are read, and anything that is not a
/// regular file is refused before the read; a read still running after
/// `patience` (a FIFO swapped in after that check) is killed and refused.
fn read_through(
    attaching: &Attaching,
    policy: &Policy,
    file: &Path,
    patience: Duration,
) -> Result<Vec<u8>, (Check, String)> {
    match std::fs::metadata(file) {
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {
            return Err((Check::Missing, format!("{} does not exist", file.display())));
        }
        Err(why) => return Err((Check::Unreadable, format!("{}: {why}", file.display()))),
        Ok(meta) if !meta.is_file() => {
            return Err((
                Check::Unreadable,
                format!("{} is not a regular file", file.display()),
            ));
        }
        Ok(_) => {}
    }
    let bytes = match &attaching.confinement {
        Some(confinement @ Confinement::Sandbox(_)) => {
            let Some(tree) = &attaching.worktree else {
                return Err((
                    Check::Unreadable,
                    format!(
                        "{} is read under the session's confinement, which runs in the \
                         worktree, and this session has none",
                        file.display()
                    ),
                ));
            };
            confined(confinement, policy, tree, file, patience)?
        }
        Some(Confinement::Unconfined) | None => {
            let mut bytes = Vec::new();
            std::fs::File::open(file)
                .and_then(|opened| opened.take(MAX_BYTES + 1).read_to_end(&mut bytes))
                .map_err(|why| (Check::Unreadable, format!("{}: {why}", file.display())))?;
            bytes
        }
    };
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES {
        return Err((
            Check::TooLarge,
            format!(
                "{} is longer than the {MAX_BYTES} bytes an attachment may be",
                file.display()
            ),
        ));
    }
    Ok(bytes)
}

/// The program that reads an attachment under the confinement, by absolute
/// path (as the confinement's own `/bin/kill`), never looked up on a `PATH`
/// a command could have written to. `/bin/cat` is there on macOS and on a
/// merged-`/usr` Linux alike.
const CAT: &str = "/bin/cat";

/// At most this much of the reader's standard error is kept, for the reason.
const STDERR_KEPT: u64 = 4096;

/// `file`'s first [`MAX_BYTES`] + 1 bytes, as [`CAT`] run under
/// `confinement` in `tree` reads them: the environment the policy passes,
/// no standard input, and killed past `patience` or once it has printed more
/// than the cap.
fn confined(
    confinement: &Confinement,
    policy: &Policy,
    tree: &Path,
    file: &Path,
    patience: Duration,
) -> Result<Vec<u8>, (Check, String)> {
    let unreadable = |why: String| (Check::Unreadable, format!("{}: {why}", file.display()));
    let argv = [
        CAT.to_owned(),
        "--".to_owned(),
        file.to_string_lossy().into_owned(),
    ];
    let composed = confinement.compose(policy, tree, &argv);
    let Some((program, rest)) = composed.split_first() else {
        return Err(unreadable("the confinement composed no command".to_owned()));
    };
    let mut child = std::process::Command::new(program)
        .env_clear()
        .envs(crate::isolation::passed(
            &policy.environment,
            std::env::vars_os(),
        ))
        .args(rest)
        .current_dir(tree)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|why| unreadable(format!("{program} could not be run: {why}")))?;
    let deadline = Instant::now() + patience;
    // Each stream on a thread of its own, so a reader that never ends is
    // waited on only until the deadline, then killed.
    let stdout = child.stdout.take().map(|out| bounded(out, MAX_BYTES + 1));
    let stderr = child.stderr.take().map(|err| bounded(err, STDERR_KEPT));
    let read = stdout.and_then(|read| read.recv_timeout(patience).ok());
    let over = read
        .as_ref()
        .is_some_and(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_BYTES);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if read.is_some() && !over && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(2));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    match (read, status) {
        (Some(bytes), _) if over => Ok(bytes),
        (Some(bytes), Some(status)) if status.success() => Ok(bytes),
        (None, _) | (Some(_), None) => Err(unreadable(format!(
            "it was not read within {patience:?}, so its reader was killed"
        ))),
        (Some(_), Some(status)) => {
            let said = stderr
                .and_then(|read| read.recv_timeout(Duration::from_secs(1)).ok())
                .map(|said| String::from_utf8_lossy(&said).trim().to_owned())
                .unwrap_or_default();
            Err(unreadable(format!(
                "the session's confinement did not let it be read ({status}): {said}"
            )))
        }
    }
}

/// At most `limit` bytes of `stream`, read on a thread of its own and
/// delivered once it ends or the limit is reached.
fn bounded(stream: impl Read + Send + 'static, limit: u64) -> mpsc::Receiver<Vec<u8>> {
    let (sender, received) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stream.take(limit).read_to_end(&mut bytes);
        let _ = sender.send(bytes);
    });
    received
}

/// `bytes` kept at `file`'s path under `recording`: written when absent
/// (to a temporary name, then renamed, so a copy is whole or not there),
/// left alone when the same digest is already there.
fn kept(recording: &Path, file: &RecordedFile, bytes: &[u8]) -> Result<(), (Check, String)> {
    let at = recording.join(&file.path);
    let failed = |why: std::io::Error| (Check::CopyFailed, format!("{}: {why}", at.display()));
    match std::fs::read(&at) {
        Ok(there) if sha256_hex(&there) == file.sha256 => return Ok(()),
        Ok(_) => {
            return Err((
                Check::CopyConflict,
                format!(
                    "{} already holds other bytes than the file's {}",
                    at.display(),
                    file.sha256
                ),
            ));
        }
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => {}
        Err(why) => return Err(failed(why)),
    }
    std::fs::create_dir_all(recording.join(FILES)).map_err(failed)?;
    let partial =
        recording
            .join(FILES)
            .join(format!(".{}.partial-{}", file.sha256, std::process::id()));
    std::fs::write(&partial, bytes)
        .and_then(|()| std::fs::rename(&partial, &at))
        .map_err(|why| {
            let _ = std::fs::remove_file(&partial);
            failed(why)
        })
}

/// Whether `file`, resolved, may be read under `policy` with `tree` as the
/// worktree: never a secret, nor a `.env` file outside the tree; otherwise
/// anything under `sandbox_reads = "all"`, and under `"listed"` the tree,
/// each readable path and each writable dir. The scope the sandbox profile
/// grants a command (`isolation::seatbelt`), stated as a predicate over the
/// policy's own resolution of its paths.
///
/// # Errors
///
/// [`Check::Secret`] or [`Check::OutsideReadScope`].
pub fn readable(policy: &Policy, tree: Option<&Path>, file: &Path) -> Result<(), Check> {
    let tree = tree.map(canonical_prefix);
    let in_tree = tree.as_ref().is_some_and(|tree| file.starts_with(tree));
    let secret = policy
        .secrets
        .iter()
        .map(|secret| resolved(secret))
        .any(|secret| file.starts_with(secret));
    let env_outside_the_tree = !in_tree
        && file
            .file_name()
            .is_some_and(|name| is_env_file(&name.to_string_lossy()));
    if secret || env_outside_the_tree {
        return Err(Check::Secret);
    }
    let listed = || {
        policy
            .readable
            .iter()
            .chain(&policy.writable)
            .map(|path| resolved(path))
            .any(|root| file.starts_with(root))
    };
    if in_tree || policy.reads == Reads::All || listed() {
        Ok(())
    } else {
        Err(Check::OutsideReadScope)
    }
}

#[cfg(test)]
pub(in crate::drive) mod tests {
    use std::path::{Path, PathBuf};

    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    use super::{
        Attached, Attaching, Check, FILES, MAX_BYTES, MEDIA_TYPE, attached, candidates,
        read_through,
    };
    use crate::drive::tool_loop::tests::scratch;
    use crate::isolation::{Confinement, Isolation, Policy};

    /// Set on a host where the sandbox is required (as in `isolation`'s
    /// tests): there a confinement that does not open fails the test rather
    /// than skipping what only the sandbox can show.
    const REQUIRED: &str = "DIET_REQUIRE_SANDBOX";

    /// The confinement `serve` would open for `policy`; `None` on a host with
    /// no runner, which a host that sets [`REQUIRED`] does not accept.
    pub(in crate::drive) fn confinement_for(policy: &Policy) -> Option<Confinement> {
        match crate::isolation::open(policy) {
            Ok(confinement) => Some(confinement),
            Err(why) => {
                assert!(
                    std::env::var_os(REQUIRED).is_none(),
                    "`{REQUIRED}` is set and the sandbox did not open: {why}"
                );
                None
            }
        }
    }

    /// Whether `attaching` reads through a sandbox: what the tests that only
    /// the sandbox can pass need, and skip without.
    fn sandboxed(attaching: &Attaching) -> bool {
        attaching
            .confinement
            .as_ref()
            .is_some_and(|confinement| confinement.isolation() == Isolation::Sandbox)
    }

    /// A PNG's signature and a body: a test image, generated at run time and
    /// never committed.
    pub(in crate::drive) fn png(body: &str) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(body.as_bytes());
        bytes
    }

    /// A worktree, a directory outside every scope, a recording, and a
    /// stand-in HOME, each fresh; and the policy T1 runs under, the default
    /// sandbox.
    struct Fixture {
        tree: PathBuf,
        outside: PathBuf,
        recording: PathBuf,
        home: PathBuf,
    }

    impl Fixture {
        /// Each directory by its canonical path, so a test sees what the path
        /// rule does with a link the test made, and not with the platform's
        /// own (macOS's temporary directory is under the `/var` link, which
        /// alone turned symlink mutations red there and nowhere else: #461
        /// F5).
        fn new(name: &str) -> Self {
            let fresh = |part: &str| {
                std::fs::canonicalize(scratch(&format!("attach-{name}-{part}")))
                    .expect("a scratch dir")
            };
            Self {
                tree: fresh("tree"),
                outside: fresh("outside"),
                recording: fresh("recording"),
                home: fresh("home"),
            }
        }

        /// The policy `serve` runs, its confinement opened as `serve` opens it.
        fn attaching(&self, policy: Policy) -> Attaching {
            Attaching {
                confinement: confinement_for(&policy),
                policy: Some(policy),
                worktree: Some(self.tree.clone()),
                recording: Some(self.recording.clone()),
            }
        }

        /// A fake key under the stand-in HOME's `.ssh`, a PNG, and the default
        /// policy with that HOME readable and its `.ssh` a secret: what
        /// `~/.ssh` is under a drive whose HOME this is. The secret is named
        /// by path rather than through `HOME`, which every test in the
        /// process shares. Never a real secret.
        fn with_a_secret(&self) -> (Policy, PathBuf, Vec<u8>) {
            let key = self.home.join(".ssh/k.png");
            let bytes = png("a fake key, never a real one");
            write(&key, &bytes);
            let mut policy = Policy::merged_usr();
            policy
                .readable
                .push(self.home.to_string_lossy().into_owned());
            policy
                .secrets
                .push(self.home.join(".ssh").to_string_lossy().into_owned());
            (policy, key, bytes)
        }

        fn copies(&self) -> Vec<PathBuf> {
            std::fs::read_dir(self.recording.join(FILES)).map_or_else(
                |_| Vec::new(),
                |dir| dir.map(|entry| entry.expect("an entry").path()).collect(),
            )
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            for dir in [&self.tree, &self.outside, &self.recording, &self.home] {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
    }

    fn write(at: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(at.parent().expect("a parent")).expect("a directory");
        std::fs::write(at, bytes).expect("written");
    }

    #[test]
    fn a_candidate_is_a_token_ending_in_png_trimmed_of_quotes_and_trailing_punctuation() {
        assert_eq!(
            candidates(
                "see `./shots/a.PNG`, and 'b.png'. not c.pngx, .png or png; then ~/x.png! \
                 \"/abs/y z.png\""
            ),
            ["./shots/a.PNG", "b.png", "~/x.png", "z.png"]
        );
        assert!(candidates("no picture here.").is_empty());
    }

    #[test]
    fn a_png_in_the_worktree_is_attached_copied_by_digest_and_named_by_reference() {
        let fixture = Fixture::new("attached");
        let bytes = png("one");
        write(&fixture.tree.join("shots/one.png"), &bytes);
        let text = "what is wrong in shots/one.png?";
        let Attached { message, files } =
            attached(text, &fixture.attaching(Policy::merged_usr())).expect("attached");

        let sha256 = crate::digest::sha256_hex(&bytes);
        assert_eq!(message.content, text, "the ask's words are not rewritten");
        assert_eq!(
            message
                .images
                .iter()
                .map(crate::client::shape::Image::data_uri)
                .collect::<Vec<_>>(),
            [format!(
                "data:image/png;base64,{}",
                crate::drive::serve::base64(&bytes)
            )]
        );
        assert_eq!(
            files,
            [crate::formats::log::RecordedFile {
                path: format!("files/{sha256}"),
                sha256: sha256.clone(),
                media_type: MEDIA_TYPE.to_owned(),
                bytes: bytes.len() as u64,
            }]
        );
        assert_eq!(
            std::fs::read(fixture.recording.join(FILES).join(&sha256)).expect("the copy"),
            bytes
        );

        // The same bytes again: the copy is kept, and the ask attaches.
        attached(text, &fixture.attaching(Policy::merged_usr())).expect("attached again");
        assert_eq!(fixture.copies().len(), 1);

        // Other bytes under that name: the recording is not the operator's
        // file any more, and the ask is refused rather than the copy
        // overwritten.
        write(&fixture.recording.join(FILES).join(&sha256), b"tampered");
        let refused = attached(text, &fixture.attaching(Policy::merged_usr()))
            .expect_err("a copy that is other bytes");
        assert_eq!(refused.check, Check::CopyConflict, "{refused}");
        assert_eq!(
            std::fs::read(fixture.recording.join(FILES).join(&sha256)).expect("the copy"),
            b"tampered"
        );
    }

    #[test]
    fn with_no_recording_the_png_is_attached_and_nothing_is_copied_or_named() {
        let fixture = Fixture::new("unrecorded");
        write(&fixture.tree.join("one.png"), &png("one"));
        let attaching = Attaching {
            recording: None,
            ..fixture.attaching(Policy::merged_usr())
        };
        let Attached { message, files } =
            attached("look at one.png", &attaching).expect("attached");
        assert_eq!(message.images.len(), 1);
        assert!(
            files.is_empty(),
            "a reference to a copy nobody kept: {files:?}"
        );
        assert!(fixture.copies().is_empty());
    }

    /// Each check, on its own: the ask is refused naming the path and the
    /// check, and nothing is copied.
    #[test]
    fn a_named_png_that_fails_a_check_refuses_the_ask_and_copies_nothing() {
        let fixture = Fixture::new("refused");
        let readable = fixture.outside.join("readable");
        let secret = readable.join("keys");
        let mut policy = Policy::merged_usr();
        policy
            .readable
            .push(readable.to_string_lossy().into_owned());
        policy.secrets.push(secret.to_string_lossy().into_owned());

        write(&fixture.outside.join("away.png"), &png("away"));
        write(&secret.join("key.png"), &png("key"));
        write(&readable.join("open.png"), &png("open"));
        write(&fixture.tree.join("text.png"), b"not a picture");
        std::fs::create_dir_all(fixture.tree.join("dir.png")).expect("a directory");
        let in_scope = fixture.tree.join("fine.png");
        write(&in_scope, &png("fine"));

        let away = fixture.outside.join("away.png");
        let key = secret.join("key.png");
        let cases: [(String, Attaching, Check); 7] = [
            (
                away.display().to_string(),
                fixture.attaching(policy.clone()),
                Check::OutsideReadScope,
            ),
            (
                key.display().to_string(),
                fixture.attaching(policy.clone()),
                Check::Secret,
            ),
            (
                "gone.png".to_owned(),
                fixture.attaching(policy.clone()),
                Check::Missing,
            ),
            (
                "text.png".to_owned(),
                fixture.attaching(policy.clone()),
                Check::NotPng,
            ),
            (
                "dir.png".to_owned(),
                fixture.attaching(policy.clone()),
                Check::Unreadable,
            ),
            (
                "fine.png".to_owned(),
                Attaching {
                    worktree: None,
                    ..fixture.attaching(policy.clone())
                },
                Check::RelativeWithoutWorktree,
            ),
            (
                in_scope.display().to_string(),
                Attaching {
                    policy: None,
                    ..fixture.attaching(policy.clone())
                },
                Check::NoReadScope,
            ),
        ];
        for (path, attaching, check) in cases {
            // The good one first where it is good, so a refusal that copied
            // what came before it would show.
            let text = if matches!(check, Check::NoReadScope | Check::RelativeWithoutWorktree) {
                path.clone()
            } else {
                format!("compare fine.png with {path}")
            };
            let refused = attached(&text, &attaching).expect_err(&path);
            assert_eq!(
                (refused.path.as_str(), refused.check),
                (path.as_str(), check),
                "{refused}"
            );
            assert!(
                refused.to_string().contains(&path) && refused.to_string().contains(check.tag()),
                "{refused}"
            );
            assert!(
                fixture.copies().is_empty(),
                "{path}: {:?}",
                fixture.copies()
            );
        }

        // A declared readable path that is no secret is in scope.
        let text = format!("and {}", readable.join("open.png").display());
        attached(&text, &fixture.attaching(policy)).expect("a declared readable path");
    }

    /// The copies kept under the recording, as their digests.
    fn digests(fixture: &Fixture) -> Vec<String> {
        fixture
            .copies()
            .iter()
            .filter_map(|copy| copy.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .collect()
    }

    /// A link in the tree is judged where it leads: to a secret, refused and
    /// nothing copied (#461 F1, F5). Without the resolution the link would be
    /// "in the tree", on Linux as on macOS.
    #[test]
    fn a_link_in_the_tree_to_a_secret_is_refused_and_nothing_is_copied() {
        let fixture = Fixture::new("link-secret");
        let (policy, key, _) = fixture.with_a_secret();
        std::os::unix::fs::symlink(&key, fixture.tree.join("shot.png")).expect("a link");
        let refused =
            attached("see shot.png", &fixture.attaching(policy)).expect_err("a link to a secret");
        assert_eq!(refused.check, Check::Secret, "{refused}");
        assert!(fixture.copies().is_empty(), "{:?}", fixture.copies());
    }

    /// The same for a PNG that is no secret but outside the read scope.
    #[test]
    fn a_link_in_the_tree_to_a_png_outside_the_read_scope_is_refused() {
        let fixture = Fixture::new("link-outside");
        let (policy, _, _) = fixture.with_a_secret();
        write(&fixture.outside.join("away.png"), &png("away"));
        std::os::unix::fs::symlink(
            fixture.outside.join("away.png"),
            fixture.tree.join("shot.png"),
        )
        .expect("a link");
        let refused = attached("see shot.png", &fixture.attaching(policy))
            .expect_err("a link outside the scope");
        assert_eq!(refused.check, Check::OutsideReadScope, "{refused}");
        assert!(fixture.copies().is_empty(), "{:?}", fixture.copies());
    }

    /// A link that stays in the tree attaches the PNG it leads to.
    #[test]
    fn a_link_in_the_tree_to_a_png_in_the_tree_attaches_the_png_it_leads_to() {
        let fixture = Fixture::new("link-inside");
        let (policy, _, _) = fixture.with_a_secret();
        let bytes = png("the real one");
        write(&fixture.tree.join("shots/real.png"), &bytes);
        std::os::unix::fs::symlink(
            fixture.tree.join("shots/real.png"),
            fixture.tree.join("alias.png"),
        )
        .expect("a link");
        let Attached { message, files } =
            attached("see alias.png", &fixture.attaching(policy)).expect("attached");
        assert_eq!(message.images.len(), 1);
        assert_eq!(
            message.images[0].data_uri(),
            format!(
                "data:image/png;base64,{}",
                crate::drive::serve::base64(&bytes)
            )
        );
        assert_eq!(digests(&fixture), [crate::digest::sha256_hex(&bytes)]);
        assert_eq!(files.len(), 1);
    }

    /// The read itself, past the path check: under the sandbox the kernel
    /// judges the open, so a link to a secret, or out of the scope, that the
    /// check never saw is still not read; and a PNG in the tree is (#461 F1).
    #[test]
    fn the_confined_read_refuses_a_link_the_path_check_never_saw() {
        let fixture = Fixture::new("confined-read");
        let (policy, key, _) = fixture.with_a_secret();
        let attaching = fixture.attaching(policy.clone());
        if !sandboxed(&attaching) {
            return;
        }
        write(&fixture.outside.join("away.png"), &png("away"));
        let fine = png("fine");
        write(&fixture.tree.join("fine.png"), &fine);
        std::os::unix::fs::symlink(&key, fixture.tree.join("to-key.png")).expect("a link");
        std::os::unix::fs::symlink(
            fixture.outside.join("away.png"),
            fixture.tree.join("to-away.png"),
        )
        .expect("a link");
        let read = |name: &str| {
            read_through(
                &attaching,
                &policy,
                &fixture.tree.join(name),
                super::READ_PATIENCE,
            )
        };
        assert_eq!(read("fine.png").expect("a PNG in the tree"), fine);
        for name in ["to-key.png", "to-away.png"] {
            let (check, why) = read(name).expect_err(name);
            assert_eq!(check, Check::Unreadable, "{name}: {why}");
        }
    }

    /// The race the review measured (#461 F1): a path swapped, again and
    /// again, between a PNG and a link to a secret while it is attached.
    /// The secret's bytes are never attached nor copied, whichever side of
    /// the path check each swap lands on.
    ///
    /// Liveness never rests on the scheduler. That the sandbox attaches the
    /// PNG at this path is shown deterministically, by an attach with nothing
    /// racing it, before the race and after it. That a link to the secret
    /// is refused is shown deterministically by
    /// `the_confined_read_refuses_a_link_the_path_check_never_saw`. The race
    /// asserts only what must hold on every interleaving: the secret is never
    /// attached and never copied, and the swapper swapped between every
    /// two asks. An earlier version required both sides to be met inside the
    /// race, and on a CI runner that met only the link (#461 run
    /// 37293445573, then #471 run 37302234290: "attached 0, refused 2000").
    #[test]
    fn a_path_swapped_for_a_link_to_a_secret_never_attaches_the_secret() {
        const ASKS: usize = 200;
        const RACE_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);
        const HOLD: std::time::Duration = std::time::Duration::from_micros(500);
        let fixture = Fixture::new("swapped");
        let (policy, key, secret) = fixture.with_a_secret();
        let attaching = fixture.attaching(policy);
        if !sandboxed(&attaching) {
            return;
        }
        let benign = png("benign");
        let shot = fixture.tree.join("shot.png");
        let attaches_the_png = |when: &str| {
            write(&shot, &benign);
            let Attached { message, .. } =
                attached("see shot.png", &attaching).unwrap_or_else(|refused| {
                    panic!("{when}, with nothing racing, the PNG was refused: {refused:?}")
                });
            assert_eq!(
                message
                    .images
                    .iter()
                    .map(crate::client::shape::Image::data_uri)
                    .collect::<Vec<_>>(),
                [format!(
                    "data:image/png;base64,{}",
                    crate::drive::serve::base64(&benign)
                )],
                "{when}"
            );
        };
        attaches_the_png("before the race");
        let stop = Arc::new(AtomicBool::new(false));
        let swaps = Arc::new(AtomicUsize::new(0));
        let swapper = {
            let (stop, swaps) = (Arc::clone(&stop), Arc::clone(&swaps));
            let (tree, benign) = (fixture.tree.clone(), benign.clone());
            std::thread::spawn(move || {
                let (link, file, shot) = (tree.join(".l"), tree.join(".f"), tree.join("shot.png"));
                while !stop.load(Ordering::Relaxed) {
                    let _ = std::fs::remove_file(&link);
                    let _ = std::os::unix::fs::symlink(&key, &link);
                    let _ = std::fs::write(&file, &benign);
                    let _ = std::fs::rename(&link, &shot);
                    std::thread::sleep(HOLD);
                    let _ = std::fs::rename(&file, &shot);
                    std::thread::sleep(HOLD);
                    swaps.fetch_add(1, Ordering::Relaxed);
                }
            })
        };
        let secret_uri = format!(
            "data:image/png;base64,{}",
            crate::drive::serve::base64(&secret)
        );
        let before = swaps.load(Ordering::Relaxed);
        let mut outcomes = std::collections::BTreeMap::<String, usize>::new();
        // Each ask waits for a swap the previous one did not see, so the
        // swapper provably runs between asks. An ask refused at the path
        // check returns in microseconds, and 200 of them fit inside one
        // hold: a starved swapper never swapped while they ran (#478's CI).
        // The deadline is generous; past it the race could not be run, and
        // that is a failure, never a pass.
        let deadline = std::time::Instant::now() + RACE_DEADLINE;
        let mut seen = before;
        for asked in 0..ASKS {
            while swaps.load(Ordering::Relaxed) == seen {
                assert!(
                    std::time::Instant::now() < deadline,
                    "could not race: the swapper made no swap before ask {asked} \
                     within {RACE_DEADLINE:?}"
                );
                std::thread::yield_now();
            }
            seen = swaps.load(Ordering::Relaxed);
            let outcome = match attached("see shot.png", &attaching) {
                Ok(Attached { message, .. }) => {
                    assert!(
                        message
                            .images
                            .iter()
                            .all(|image| image.data_uri() != secret_uri),
                        "the secret was attached"
                    );
                    "attached".to_owned()
                }
                Err(refused) => format!("{:?}", refused.check),
            };
            *outcomes.entry(outcome).or_default() += 1;
        }
        let during = swaps.load(Ordering::Relaxed) - before;
        stop.store(true, Ordering::Relaxed);
        swapper.join().expect("the swapper");
        // What the race met, for a reader of a failure; never a verdict.
        eprintln!("swaps during the asks: {during}; outcomes: {outcomes:?}");
        assert!(
            !digests(&fixture).contains(&crate::digest::sha256_hex(&secret)),
            "the secret was copied"
        );
        assert!(during > 0, "the swapper never swapped while the asks ran");
        attaches_the_png("after the race");
    }

    /// A file past [`MAX_BYTES`] is refused and not copied; one at it attaches
    /// (#461 F3).
    #[test]
    fn a_png_longer_than_the_cap_is_refused() {
        let fixture = Fixture::new("over-cap");
        let cap = usize::try_from(MAX_BYTES).expect("the cap fits");
        let mut at_cap = png("");
        at_cap.resize(cap, 0);
        write(&fixture.tree.join("at.png"), &at_cap);
        let mut over = at_cap.clone();
        over.push(0);
        write(&fixture.tree.join("over.png"), &over);
        // The check alone: a failure that printed the attached message would
        // print sixteen megabytes of base64.
        let refused = attached("see over.png", &fixture.attaching(Policy::merged_usr()))
            .err()
            .map(|refused| refused.check);
        assert_eq!(refused, Some(Check::TooLarge));
        assert!(fixture.copies().is_empty(), "{:?}", fixture.copies());
        attached("see at.png", &fixture.attaching(Policy::merged_usr())).expect("at the cap");
    }

    /// A FIFO named `x.png` is refused, and the ask answered, rather than the
    /// read waiting for a writer forever (#461 F2): before the read, and,
    /// under the sandbox, by the read's patience when the path check never
    /// saw it.
    #[test]
    fn a_fifo_named_png_is_refused_rather_than_waited_on() {
        let fixture = Fixture::new("fifo");
        let fifo = fixture.tree.join("pipe.png");
        let made = std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "mkfifo: {made}");
        let attaching = fixture.attaching(Policy::merged_usr());
        // Each call on a thread of its own, so a read that waits forever
        // fails this test rather than hanging the suite.
        let within = |call: Box<dyn FnOnce() -> Option<Check> + Send>| {
            let (sender, received) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = sender.send(call());
            });
            received
                .recv_timeout(Duration::from_secs(5))
                .expect("the read returned within 5 s")
        };
        let asked = attaching.clone();
        let refused = within(Box::new(move || {
            attached("see pipe.png", &asked)
                .err()
                .map(|refused| refused.check)
        }));
        assert_eq!(refused, Some(Check::Unreadable));
        if sandboxed(&attaching) {
            // Past the path check, as a FIFO swapped in after it would be.
            let began = Instant::now();
            let read = within(Box::new(move || {
                let policy = attaching.policy.clone().expect("a policy");
                read_through(&attaching, &policy, &fifo, Duration::from_millis(300))
                    .err()
                    .map(|(check, _)| check)
            }));
            assert_eq!(read, Some(Check::Unreadable));
            assert!(began.elapsed() < Duration::from_secs(5));
        }
    }
}
