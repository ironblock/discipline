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
use std::path::{Path, PathBuf};

use crate::client::shape::{Message, Role};
use crate::client::vocabulary;
use crate::digest::sha256_hex;
use crate::formats::log::RecordedFile;
use crate::isolation::Policy;
use crate::isolation::Reads;
use crate::isolation::declared::is_env_file;
use crate::isolation::policy::{canonical_prefix, home_expanded, resolved};

/// The media type every attachment is sent and logged as: the only one
/// sniffed (ruled 5989411005: "names a PNG path").
pub const MEDIA_TYPE: &str = "image/png";

/// The directory, under the recording's, that holds the copies.
pub const FILES: &str = "files";

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attaching {
    /// The regimen's policy, whose read scope and secrets decide what may be
    /// attached; `None` with no regimen.
    pub policy: Option<Policy>,
    /// The worktree: in the read scope, and what a relative path is resolved
    /// against.
    pub worktree: Option<PathBuf>,
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
    for (named, file, bytes) in &found {
        message = crate::client::attach(message, file, bytes).map_err(|why| Unattachable {
            path: (*named).to_owned(),
            check: Check::Unreadable,
            why: why.to_string(),
        })?;
    }
    let mut files = Vec::new();
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
/// and its bytes.
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
    let bytes = std::fs::read(&file).map_err(|why| match why.kind() {
        std::io::ErrorKind::NotFound => {
            (Check::Missing, format!("{} does not exist", file.display()))
        }
        _ => (Check::Unreadable, format!("{}: {why}", file.display())),
    })?;
    if !bytes.starts_with(SIGNATURE) {
        return Err((
            Check::NotPng,
            format!("{} does not begin with the PNG signature", file.display()),
        ));
    }
    let sha256 = sha256_hex(&bytes);
    let file = RecordedFile {
        path: format!("{FILES}/{sha256}"),
        sha256,
        media_type: MEDIA_TYPE.to_owned(),
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
    };
    Ok((file, bytes))
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

    use super::{Attached, Attaching, Check, FILES, MEDIA_TYPE, attached, candidates};
    use crate::drive::tool_loop::tests::scratch;
    use crate::isolation::Policy;

    /// A PNG's signature and a body: a test image, generated at run time and
    /// never committed.
    pub(in crate::drive) fn png(body: &str) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend_from_slice(body.as_bytes());
        bytes
    }

    /// A worktree, a directory outside every scope, and a recording, each
    /// fresh; and the policy T1 runs under, the default sandbox.
    struct Fixture {
        tree: PathBuf,
        outside: PathBuf,
        recording: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            Self {
                tree: scratch(&format!("attach-{name}-tree")),
                outside: scratch(&format!("attach-{name}-outside")),
                recording: scratch(&format!("attach-{name}-recording")),
            }
        }

        fn attaching(&self, policy: Policy) -> Attaching {
            Attaching {
                policy: Some(policy),
                worktree: Some(self.tree.clone()),
                recording: Some(self.recording.clone()),
            }
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
            for dir in [&self.tree, &self.outside, &self.recording] {
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
}
