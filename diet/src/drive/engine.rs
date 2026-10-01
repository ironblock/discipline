//! The engine check, at start (#157; ruled on #204 and by Dispatch,
//! 2026-10-01).
//!
//! A regimen names a substrate, and the registry says which engine that
//! substrate is. Before `diet-drive serve --regimen` binds, the server it is
//! about to drive is asked for its build (`GET /props`, llama.cpp's
//! `build_info`), and the build's commit must be the one the registry pins
//! as the substrate's `engine_commit`. Otherwise the record would name an
//! engine by declaration while another one answered -- which is what a
//! registered production did on 2026-10-01, rebuilt one commit past its pin
//! (C0b, #203).
//!
//! **An engine that reports no commit** (a prebuilt release reports
//! `b0-unknown-dirty`) is declared by the literal it reports, as the entry's
//! `engine_build_info`. Where one is declared it is checked FIRST, exactly,
//! and the entry's `engine_commit`, if any, is not compared: the literal is
//! the entry's statement that its commit cannot be checked. The engine's
//! identity is then `unreported`, and the announcement says so (amended on
//! #157, 2026-10-01).
//!
//! **Fail closed.** A check that cannot be made refuses, naming what is
//! missing: neither an `engine_build_info` nor an `engine_commit` registered,
//! a malformed one, a server that does not answer `/props`, a `build_info`
//! that names no commit, or one that names a commit and something after it
//! (a dirty build is not the registered commit). A canned substrate is not
//! checked: the canned server has no `/props`, and tying a canned regime to
//! a canned server is its own item (#204, disclosure 4).

use crate::client::stream::HttpStream;
use crate::client::transport::{HttpReply, TransportFailure};
use crate::formats::record::Weights;

use super::registry::{self, Identity};

/// How long the check waits for `/props`.
pub const PROPS_DEADLINE: std::time::Duration = std::time::Duration::from_secs(10);

/// The fewest hex digits a `build_info` commit may have: git's short form.
const SHORTEST_COMMIT: usize = 7;

/// The commit a llama.cpp `build_info` names. It is written
/// `b<build>-<commit>` (`b8-e486f80`, C0b on #203); the build number is not
/// the engine's identity, and is not read.
///
/// # Errors
///
/// When there is no `-`, when fewer than seven lowercase hex digits follow
/// it, or when anything follows the hex digits.
pub fn build_commit(build_info: &str) -> Result<&str, String> {
    let Some((_, after)) = build_info.split_once('-') else {
        return Err(format!(
            "`build_info` \"{build_info}\" names no commit: it is not `b<build>-<commit>`"
        ));
    };
    let hex = after.len() - after.trim_start_matches(is_hex).len();
    let (commit, suffix) = after.split_at(hex);
    if commit.is_empty() {
        return Err(format!(
            "`build_info` \"{build_info}\" names no hex commit after its `-`"
        ));
    }
    if !suffix.is_empty() {
        return Err(format!(
            "`build_info` \"{build_info}\" carries \"{suffix}\" after its commit: a build that \
             is not exactly a commit (a dirty tree, a local patch) is not the registered one"
        ));
    }
    if commit.len() < SHORTEST_COMMIT {
        return Err(format!(
            "`build_info` \"{build_info}\" names a commit of {} hex digits, fewer than {SHORTEST_COMMIT}",
            commit.len()
        ));
    }
    Ok(commit)
}

/// The `build_info` string in a `/props` body.
///
/// # Errors
///
/// When the body is not a JSON object with a string `build_info`.
pub fn build_info_of(props: &str) -> Result<String, String> {
    serde_json::from_str::<serde_json::Value>(props)
        .ok()
        .as_ref()
        .and_then(|props| props.get("build_info"))
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| "`GET /props` answered with no string `build_info`".to_owned())
}

/// How the engine's identity was established, as the announcement names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineIdentity {
    /// The reported commit is the registered one.
    Checked,
    /// The engine reports no commit; its `build_info` is the literal the
    /// registry declares for it.
    Unreported,
}

impl EngineIdentity {
    /// The words the announcement carries.
    #[must_use]
    pub fn tag(self) -> &'static str {
        match self {
            Self::Checked => "checked (commit)",
            Self::Unreported => "unreported (literal matched)",
        }
    }
}

/// A check that passed: the `build_info` as the server reported it, and how
/// the engine's identity was established.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passed {
    /// `build_info`, as read.
    pub build_info: String,
    /// Which comparison passed.
    pub identity: EngineIdentity,
}

/// How much of a refusing `/props` body a refusal echoes: enough to read,
/// and never the 64 MiB a reply may hold, nor a reflected header far down it.
const ECHOED_BODY_CHARS: usize = 200;

/// What the registry says the server must report.
enum Expected<'a> {
    /// Exactly this `build_info`.
    Literal(&'a str),
    /// A `build_info` whose commit is a prefix of this one.
    Commit(&'a str),
}

/// Whether the server reporting `build_info` runs the engine the registry
/// declares for substrate `id`, and how that was established.
///
/// # Errors
///
/// What [`expected`] refuses; a `build_info` other than a declared literal;
/// and, on the commit path, one that names no usable commit
/// ([`build_commit`]) or another commit.
pub fn matches(id: &str, identity: &Identity, build_info: &str) -> Result<EngineIdentity, String> {
    match expected(id, identity)? {
        Expected::Literal(literal) => {
            if build_info != literal {
                return Err(format!(
                    "the server reports `build_info` \"{build_info}\"; the registry declares \
                     that `{id}`'s engine reports exactly \"{literal}\""
                ));
            }
            Ok(EngineIdentity::Unreported)
        }
        Expected::Commit(registered) => {
            let reported = build_commit(build_info)?;
            if !registered.starts_with(reported) {
                return Err(format!(
                    "the server reports `build_info` \"{build_info}\", commit {reported}; the \
                     registry pins `{id}` to {registered}"
                ));
            }
            Ok(EngineIdentity::Checked)
        }
    }
}

/// What the registry declares substrate `id`'s server must report: its
/// `engine_build_info` literal when it declares one, else its
/// `engine_commit`.
///
/// # Errors
///
/// When it declares neither, an empty literal, or a commit that is not 40
/// lowercase hex digits.
impl Expected<'_> {
    /// What a refusal says was expected.
    fn describes(&self) -> String {
        match self {
            Self::Literal(literal) => format!("`build_info` exactly \"{literal}\""),
            Self::Commit(registered) => format!("a build of commit {registered}"),
        }
    }
}

/// The first [`ECHOED_BODY_CHARS`] characters of `body`, marked when cut.
fn excerpt(body: &str) -> String {
    body.char_indices()
        .nth(ECHOED_BODY_CHARS)
        .map_or_else(|| body.to_owned(), |(cut, _)| format!("{}…", &body[..cut]))
}

fn expected<'a>(id: &str, identity: &'a Identity) -> Result<Expected<'a>, String> {
    if let Some(literal) = identity.engine_build_info.as_deref() {
        if literal.is_empty() {
            return Err(format!(
                "the registry's `engine_build_info` for `{id}` is empty: the entry is malformed"
            ));
        }
        return Ok(Expected::Literal(literal));
    }
    let Some(registered) = identity.engine_commit.as_deref() else {
        return Err(format!(
            "the registry records neither an `engine_build_info` nor an `engine_commit` for \
             `{id}`, so the engine check cannot be made, and a check that cannot be made refuses"
        ));
    };
    if registered.len() != 40 || !registered.chars().all(is_hex) {
        return Err(format!(
            "the registry's `engine_commit` for `{id}` is \"{registered}\", not a commit's 40 \
             lowercase hex digits: the entry is malformed"
        ));
    }
    Ok(Expected::Commit(registered))
}

/// The engine check for substrate `id` in `document`, asking the server
/// through `props`: what passed, or `None` for a canned substrate, which is
/// not checked.
///
/// # Errors
///
/// What [`registry::identity`] refuses; a `/props` that did not answer, or
/// did not answer `200`; and what [`build_info_of`] and [`matches`] refuse.
pub fn check(
    document: &str,
    id: &str,
    props: impl FnOnce() -> Result<HttpReply, TransportFailure>,
) -> Result<Option<Passed>, String> {
    let identity = registry::identity(document, id)?;
    if matches!(identity.weights, Weights::Canned { .. }) {
        return Ok(None);
    }
    // Before the server is asked: a check the registry cannot support is
    // refused without touching the server.
    let wants = expected(id, &identity)?;
    // Both values in every refusal (Q3): what the registry expects, and what
    // the server did.
    let refuse = |why: String| format!("`{id}` expects {}: {why}", wants.describes());
    let reply = props().map_err(|why| {
        refuse(format!(
            "the engine check asked `GET /props` and got no reply: {why}"
        ))
    })?;
    if reply.status != 200 {
        return Err(refuse(format!(
            "the engine check asked `GET /props` and was answered {}: {}",
            reply.status,
            excerpt(&reply.body)
        )));
    }
    let build_info = build_info_of(&reply.body).map_err(refuse)?;
    let identity = matches(id, &identity, &build_info)?;
    Ok(Some(Passed {
        build_info,
        identity,
    }))
}

/// [`check`] against the server `transport` drives, with the registry this
/// program was built with.
///
/// # Errors
///
/// As [`check`].
pub fn check_served(transport: &HttpStream, id: &str) -> Result<Option<Passed>, String> {
    check(registry::REGISTRY, id, || {
        transport.props(std::time::Instant::now() + PROPS_DEADLINE)
    })
}

/// A lowercase hex digit.
fn is_hex(c: char) -> bool {
    c.is_ascii_digit() || ('a'..='f').contains(&c)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::registry::{REGISTRY, identity};

    /// A substrate with a registered commit; its binary self-reports build 1,
    /// commit 4ceb171.
    const PINNED: &str = "accel24-llamacpp-qwen38-27b-iq3s";

    fn answered(body: &str) -> HttpReply {
        HttpReply {
            status: 200,
            body: body.to_owned(),
        }
    }

    #[test]
    fn a_build_info_names_the_commit_after_its_build_number() {
        assert_eq!(build_commit("b8-e486f80"), Ok("e486f80"));
        assert_eq!(build_commit("b7-e7051efc8002"), Ok("e7051efc8002"));
        for refused in [
            "e486f80",
            "b8-e486f",
            "b8-",
            "b8-e486f80-dirty",
            "b8-E486F80",
        ] {
            assert!(build_commit(refused).is_err(), "{refused}");
        }
        let dirty = build_commit("b8-e486f80-dirty").expect_err("a suffix");
        assert!(dirty.contains("\"-dirty\""), "{dirty}");
        // A prebuilt release names no commit at all: said so, not called a suffix.
        let prebuilt = build_commit("b0-unknown-dirty").expect_err("no hex");
        assert!(prebuilt.contains("names no hex commit"), "{prebuilt}");
    }

    #[test]
    fn a_server_on_the_registered_commit_passes_and_reports_its_build() {
        let found = identity(REGISTRY, PINNED).expect("registered");
        let commit = found.engine_commit.expect("an engine_commit");
        let build_info = format!("b1-{}", &commit[..7]);
        assert_eq!(
            check(REGISTRY, PINNED, || Ok(answered(&format!(
                "{{\"build_info\":\"{build_info}\"}}"
            )))),
            Ok(Some(Passed {
                build_info,
                identity: EngineIdentity::Checked
            }))
        );
    }

    #[test]
    fn a_server_on_another_commit_is_refused_naming_both() {
        // C0b's build (`b8-e486f80`), which is not this substrate's commit.
        let refused = check(REGISTRY, PINNED, || {
            Ok(answered("{\"build_info\":\"b8-e486f80\"}"))
        })
        .expect_err("another commit");
        assert!(
            refused.contains("e486f80") && refused.contains("4ceb1719"),
            "{refused}"
        );
    }

    #[test]
    fn a_substrate_with_no_registered_engine_identity_is_refused_and_the_server_is_not_asked() {
        let refused = check(REGISTRY, "all-MiniLM-L6-v2", || -> Result<HttpReply, _> {
            panic!("a check that cannot be made does not ask the server")
        })
        .expect_err("neither key");
        assert!(
            refused.contains("neither an `engine_build_info` nor an `engine_commit`"),
            "{refused}"
        );
    }

    #[test]
    fn a_declared_literal_is_matched_exactly_before_any_commit() {
        // The floor's shape (#209): a commit by record, and the literal its
        // prebuilt engine reports. The literal governs; the commit is not
        // compared, so the build the commit would pass is refused.
        let mut found = identity(REGISTRY, PINNED).expect("registered");
        found.engine_build_info = Some("b0-unknown-dirty".to_owned());
        assert_eq!(
            matches(PINNED, &found, "b0-unknown-dirty"),
            Ok(EngineIdentity::Unreported)
        );
        let refused = matches(PINNED, &found, "b1-4ceb171").expect_err("not the literal");
        assert!(
            refused.contains("exactly \"b0-unknown-dirty\""),
            "{refused}"
        );
        found.engine_build_info = Some(String::new());
        let refused = matches(PINNED, &found, "").expect_err("an empty literal");
        assert!(refused.contains("malformed"), "{refused}");
    }

    /// The floor's entry shape after #209: the literal, `engine_commit_unknown`
    /// with its account, and no `engine_commit`.
    #[test]
    fn a_prebuilt_engine_passes_through_the_check_as_unreported() {
        let registry = format!(
            "[equipment.e]\nhardware_fingerprint = \"{}\"\n\
             [substrate.prebuilt]\nequipment = \"e\"\nengine_name = \"n\"\n\
             engine_identity = \"i\"\nweights_main = \"{}\"\n\
             engine_commit_unknown = \"the tarball names no commit\"\n\
             engine_build_info = \"b0-unknown-dirty\"\n",
            "a".repeat(64),
            "b".repeat(64)
        );
        assert_eq!(
            check(&registry, "prebuilt", || Ok(answered(
                "{\"build_info\":\"b0-unknown-dirty\"}"
            ))),
            Ok(Some(Passed {
                build_info: "b0-unknown-dirty".to_owned(),
                identity: EngineIdentity::Unreported
            }))
        );
        assert_eq!(
            [EngineIdentity::Checked, EngineIdentity::Unreported].map(EngineIdentity::tag),
            ["checked (commit)", "unreported (literal matched)"]
        );
    }

    #[test]
    fn a_malformed_registered_commit_is_refused() {
        let mut found = identity(REGISTRY, PINNED).expect("registered");
        for malformed in [
            "4ceb171".to_owned(),
            "g".repeat(40),
            "4CEB1719101F32637B841206C172F3F058FFC182".to_owned(),
        ] {
            found.engine_commit = Some(malformed.clone());
            let refused = matches(PINNED, &found, "b1-4ceb171").expect_err(&malformed);
            assert!(refused.contains("malformed"), "{refused}");
        }
    }

    #[test]
    fn a_props_that_cannot_answer_is_a_refusal_not_a_pass() {
        for (props, says) in [
            (
                Err(TransportFailure::Connect("refused".to_owned())),
                "no reply",
            ),
            (
                Ok(HttpReply {
                    status: 401,
                    body: "unauthorized".to_owned(),
                }),
                "answered 401",
            ),
            (
                Ok(answered("{\"build\":\"b1-4ceb171\"}")),
                "no string `build_info`",
            ),
            (Ok(answered("not json")), "no string `build_info`"),
        ] {
            let refused = check(REGISTRY, PINNED, || props).expect_err(says);
            // Both values: the server's side, and what the registry expects.
            assert!(
                refused.contains(says)
                    && refused.contains(PINNED)
                    && refused.contains("a build of commit 4ceb1719"),
                "{refused}"
            );
        }
        // A refusing body is echoed only in part.
        let refused = check(REGISTRY, PINNED, || {
            Ok(HttpReply {
                status: 500,
                body: "x".repeat(100_000),
            })
        })
        .expect_err("a 500");
        assert!(
            refused.len() < 1_000 && refused.ends_with('…'),
            "{} bytes",
            refused.len()
        );
    }

    #[test]
    fn a_canned_substrate_is_not_checked() {
        assert_eq!(
            check(REGISTRY, "canned-cache-n", || -> Result<HttpReply, _> {
                panic!("a canned substrate's server is not asked")
            }),
            Ok(None)
        );
    }
}
