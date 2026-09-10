//! The scripted three-turn drive, and the server that answers it.
//!
//! One copy, in the library, because the integration lane runs the drive as a
//! *program* and the unit tests run it as a function -- and a lane asserting
//! on a script the tests never ran would be two scripts that can disagree.
//! That is the same rule the rest of this crate is built on, applied to a
//! fixture.
//!
//! # What a canned server is, and is not
//!
//! [`acts`] is a list of replies played in order. It answers whatever it is
//! asked, so it decides nothing about compliance -- it is the floor below
//! #23's battery, not a substitute for it. What it buys is that the whole
//! round trip runs in milliseconds with no weights, no GPU and no network, so
//! a change to a router or a reconciler is answered in the time it takes to
//! run a test rather than in ten minutes.
//!
//! The acts are in **call order**, and the order is a property of
//! [`script`]: turn one's ask and its fork, turn two's ask and its fork and
//! the ratification the declared boundary triggers, turn three's ask. A
//! script that fires a seam somewhere else needs its own acts, and
//! [`the_acts_are_exactly_what_the_script_asks_for`] is what stops the two
//! drifting apart.
//!
//! [`the_acts_are_exactly_what_the_script_asks_for`]: #

use crate::client::stub::Act;

use super::script::{Command, Script, Turn};
use crate::formats::record::Regime;

/// The answer every interview fork is given.
///
/// Prose with no tag, two tagged regions with values, and a decline -- so a
/// drive against this server exercises both sides of the capture census
/// rather than a fold that keeps everything.
pub const FORK_ANSWER: &str = "some prose nobody tagged\n\
                               DECISION: keep the reconciler\n\
                               LEARNED: the seam refills from the object\n\
                               EVIDENCE: (none)";

/// A reply that counts the prompt and not the answer.
///
/// Legal, and some serving stacks do it under some flags. A record's
/// `response.output_tokens` is required and zero is a measurement, so this is
/// the input that has to reach a [`super::Halt`] rather than six fabricated
/// zeroes -- and until it existed, nothing in the lane could tell the
/// difference.
#[must_use]
pub fn reply_counting_only_the_prompt(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!(
        "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\
         \"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":{PROMPT_TOKENS}}},\
         \"generation_settings\":{{}},\"timings\":{{\"prompt_n_cached\":512}}}}"
    )
}

/// A fork answer whose two regions say the SAME thing.
///
/// Two patches, one entry: the second dedupes onto the first. It is the only
/// shape that tells `capture.entries` counted from `Applied::touched` apart
/// from one counted from the patch list, and without it both readings agreed
/// on every fixture in the lane.
pub const FORK_REPEATS_ITSELF: &str = "DECISION: keep the reconciler\n\
                                       DECISION: keep the reconciler";

/// How many prompt tokens the canned server reports for every call.
///
/// A number rather than a silence, because a server that reports no usage
/// cannot produce a record -- see [`super::Halt::Unmeasured`] -- and a canned
/// server that could not be recorded from would make the lane test the halt
/// path forever.
pub const PROMPT_TOKENS: u64 = 600;

/// The scripted three-turn drive.
///
/// Turn two declares the boundary, so the seam lands where the script chose
/// rather than where a cadence happened to put it.
#[must_use]
pub fn script(regime: Regime) -> Script {
    Script {
        regime,
        turns: vec![
            Turn {
                ask: "read the module and say what it exports".to_owned(),
                commands: vec![Command::new("shell", &["sh", "-c", "echo one > one.txt"])],
                fork: Some("what did that establish?".to_owned()),
                boundary: false,
                phase: None,
            },
            Turn {
                ask: "now change it".to_owned(),
                commands: Vec::new(),
                fork: Some("what did that establish?".to_owned()),
                boundary: true,
                phase: None,
            },
            Turn {
                ask: "and run the tests".to_owned(),
                commands: vec![Command::new(
                    "shell",
                    &["sh", "-c", "echo three > three.txt"],
                )],
                fork: None,
                boundary: false,
                phase: None,
            },
        ],
    }
}

/// The replies the canned server plays for [`script`], in call order.
#[must_use]
pub fn acts() -> Vec<Act> {
    [
        "turn one",
        FORK_ANSWER,
        "turn two",
        FORK_ANSWER,
        "DECISION: fold them",
        "turn three",
    ]
    .into_iter()
    .map(|text| Act::Answer(reply(text)))
    .collect()
}

/// The sha256 of the replies this server plays, as the record spells a digest.
///
/// A canned server serves no weights, and that is not the same as serving
/// nothing identifiable: its answers ARE these bytes, in this order. So this
/// is the substrate's weights identity -- COMPUTED from the artifact that
/// decided every reply rather than asserted about it, which is what
/// `Weights::Digest` means. It is also why a canned drive is reproducible by
/// config rather than a historical observation: replay it and the same acts
/// play again, byte for byte.
///
/// What it does NOT cover is how the stub plays them. A change there is a
/// change to this program, not to the substrate, and folding the two into one
/// digest would make every edit to the serving loop look like a different
/// substrate.
///
/// Every variant, and no fallback, for the reason `diet-drive`'s regimen
/// crossing has none: an act this function did not think of would otherwise
/// hash to whatever the acts it replaced hashed to, and a digest that cannot
/// tell two servers apart is not an identity. Each field is written with its
/// own length in front, so no two act lists can render to the same bytes by
/// agreeing across a separator.
#[must_use]
pub fn acts_digest() -> String {
    digest_of(&acts())
}

/// The digest of any act list, which is what makes [`acts_digest`] checkable.
///
/// Split out because a digest that ignored its input would move the record and
/// every test that reads it TOGETHER, and pass: both sides would be asking the
/// same function. Taking the acts as an argument is what lets a test hand it
/// two different lists and require two different answers, which is the only
/// form of "this is an identity" that can fail.
#[must_use]
pub fn digest_of(acts: &[Act]) -> String {
    use std::fmt::Write as _;

    let mut canonical = String::new();
    let piece = |out: &mut String, text: &str| {
        let _ = write!(out, "{}:{text};", text.len());
    };
    for act in acts {
        match act {
            Act::Answer(body) => {
                canonical.push_str("answer;");
                piece(&mut canonical, body);
            }
            Act::Status(code, body) => {
                let _ = write!(canonical, "status;{code};");
                piece(&mut canonical, body);
            }
            Act::Stall(wait, body) => {
                let _ = write!(canonical, "stall;{};", wait.as_nanos());
                piece(&mut canonical, body);
            }
            Act::Chunked(chunks) => {
                let _ = write!(canonical, "chunked;{};", chunks.len());
                for chunk in chunks {
                    piece(&mut canonical, chunk);
                }
            }
            Act::AnswerAndHold(body, held) => {
                let _ = write!(canonical, "answer-and-hold;{};", held.as_nanos());
                piece(&mut canonical, body);
            }
            Act::Undercount(body, announced) => {
                let _ = write!(canonical, "undercount;{announced};");
                piece(&mut canonical, body);
            }
            Act::Hangup => canonical.push_str("hangup;"),
        }
        canonical.push('\n');
    }
    super::digest::sha256_hex(canonical.as_bytes())
}

/// One reply in the shape a llama.cpp-dialect server sends.
///
/// Rendered here rather than read from a fixture file because it is six
/// strings and a usage block; a file would be a seventh thing to keep in
/// step.
#[must_use]
pub fn reply(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    format!(
        "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\
         \"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":{PROMPT_TOKENS},\
         \"completion_tokens\":7}},\"generation_settings\":{{}},\
         \"timings\":{{\"prompt_n_cached\":512}}}}"
    )
}

/// How many calls [`script`] makes: one per turn, one per fork, one per seam.
///
/// Derived rather than written down, so a script that grows a turn cannot
/// leave the count behind.
#[must_use]
pub fn calls(script: &Script) -> usize {
    script
        .turns
        .iter()
        .map(|turn| 1 + usize::from(turn.fork.is_some()) + usize::from(turn.boundary))
        .sum()
}

/// The regimen this lane ships, byte for byte.
///
/// `include_str!` rather than a path read at run time: a test that read the
/// file from the working directory would pass in a checkout and fail in a
/// lane that ran from anywhere else, and a fixture nothing reads is a fixture
/// that rots.
pub const DEV_LOOP: &str = include_str!("../../drive/dev-loop.toml");

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Act, acts, acts_digest, digest_of};

    #[test]
    fn the_digest_the_record_carries_is_the_digest_of_the_acts_that_were_played() {
        assert_eq!(
            acts_digest(),
            digest_of(&acts()),
            "the wiring, and the only reason a caller may take one for the other"
        );
    }

    #[test]
    fn a_digest_that_does_not_move_with_the_acts_is_not_an_identity() {
        // The failure this exists for: `digest_of` returning a constant, or
        // hashing something other than what it was handed. Both leave every
        // other test in this repository passing, because the record and the
        // test that reads it would ask the SAME function and get the same
        // wrong answer.
        let played = acts();
        let mut one_reply_different = played.clone();
        one_reply_different[0] = Act::Answer("something else entirely".to_owned());
        assert_ne!(
            digest_of(&played),
            digest_of(&one_reply_different),
            "a server that answers differently is a different server"
        );

        assert_ne!(
            digest_of(&played),
            digest_of(&[]),
            "and a server with no acts at all is not this one"
        );
    }

    #[test]
    fn the_order_the_acts_are_played_in_is_part_of_the_identity() {
        let played = acts();
        let mut swapped = played.clone();
        swapped.swap(0, 1);
        assert_ne!(
            digest_of(&played),
            digest_of(&swapped),
            "the acts are played IN ORDER, so two orders are two servers"
        );
    }

    #[test]
    fn a_body_that_spells_the_separators_is_still_one_act() {
        // What the length prefix is for, and the case that shows it. An
        // earlier version of this test compared `["ab", "c"]` with
        // `["a", "bc"]`; both survive without the prefix, because the act
        // terminator already falls between them -- it was a guard that could
        // not fire, and dropping the prefix proved it by passing.
        //
        // A body containing the delimiters is the collision. Without the
        // length in front, ONE act whose text spells the end of an act and the
        // start of the next renders to exactly the bytes that TWO acts do.
        assert_ne!(
            digest_of(&[Act::Answer("x;\nanswer;y".to_owned())]),
            digest_of(&[Act::Answer("x".to_owned()), Act::Answer("y".to_owned())]),
            "a server that answers once and a server that answers twice"
        );
        // And across variants, whose payloads are written into the same field
        // positions: a status body and an answer body are different acts.
        assert_ne!(
            digest_of(&[Act::Answer("x".to_owned())]),
            digest_of(&[Act::Status(200, "x".to_owned())]),
        );
        assert_ne!(
            digest_of(&[Act::Stall(Duration::from_secs(1), "x".to_owned())]),
            digest_of(&[Act::Stall(Duration::from_secs(2), "x".to_owned())]),
            "a wait is a property of the act, not decoration on it"
        );
    }
}
