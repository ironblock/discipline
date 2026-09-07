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
