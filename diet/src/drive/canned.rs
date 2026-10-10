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
    let escaped = escaped(text);
    format!(
        "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\
         \"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":{PROMPT_TOKENS}}},\
         \"generation_settings\":{{}},\"timings\":{{\"cache_n\":512}}}}"
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
///
/// Registered as substrate `canned-cache-n`: these bodies carry the cache
/// count under `timings.cache_n`, the key a llama.cpp server sends (#156).
#[must_use]
pub fn acts() -> Vec<Act> {
    SCRIPTED
        .into_iter()
        .map(|text| Act::Answer(reply(text)))
        .collect()
}

/// The acts as first registered, as substrate `canned`: the same answers,
/// with the cache count under `timings.prompt_n_cached`, a key no server
/// sends (#156). Nothing plays them any more. They are kept, byte for byte,
/// because records name their digest, and a digest is only an identity while
/// the bytes it was taken over can still be hashed.
#[must_use]
pub fn acts_as_first_registered() -> Vec<Act> {
    SCRIPTED
        .into_iter()
        .map(|text| Act::Answer(answer(text, "{\"prompt_n_cached\":512}")))
        .collect()
}

/// What [`script`]'s calls are answered with, in call order.
const SCRIPTED: [&str; 6] = [
    "turn one",
    FORK_ANSWER,
    "turn two",
    FORK_ANSWER,
    "DECISION: fold them",
    "turn three",
];

/// What the canned server is, as the registry's `canned-server` entry type
/// names it: its `serves` field, and the engine name a record carries
/// (`drive::regimen` writes it from here).
pub const SERVES: &str = "diet-drive canned";

/// The registry's `hardware_fingerprint` for a canned server playing acts
/// with digest `acts_sha256`: the sha256 of exactly the entry type's two
/// hardware fields, as canonical JSON (sorted keys, no spaces).
///
/// The rule's home is `substrates/check-fingerprints.py` (`digest_of`); this
/// is a second implementation of it, so it is never checked against itself.
/// Every test compares its result with a fingerprint that script produced
/// and the registry committed.
#[must_use]
pub fn hardware_fingerprint(acts_sha256: &str) -> String {
    crate::digest::sha256_hex(
        format!("{{\"acts_sha256\":\"{acts_sha256}\",\"serves\":\"{SERVES}\"}}").as_bytes(),
    )
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

/// What the canned server answers `GET /props` with as its `build_info`:
/// `canned-` and the digest of the acts it plays. The registry's canned
/// entries declare it as their `engine_build_info`, so the engine check ties
/// a canned regime to the canned server like any other (#219 item 11).
#[must_use]
pub fn build_info() -> String {
    format!("canned-{}", acts_digest())
}

/// The reply the stream-replay substrate plays for every request (#411): a
/// warm second turn captured off llama.cpp `e7051ef`, byte for byte -- an
/// event stream that finishes `stop` and closes on `usage` and `timings`
/// (prompt 18, cached 160, predicted 66;
/// `substrates/measurements/2026-09-29-r30-captures/`, capture C2). So a
/// turn against it settles `final`, and its counts are that capture's.
pub const REPLAYED: &[u8] =
    include_bytes!("../../client/fixtures/llama-server-e7051ef-warm-turn2-stream.http");

/// The acts the stream-replay substrate plays: [`REPLAYED`], once, which
/// [`crate::client::stub::Stub::replaying`] plays for every request.
#[must_use]
pub fn replay_acts() -> Vec<Act> {
    vec![Act::Raw(REPLAYED.to_vec())]
}

/// The digest of [`replay_acts`]: the stream-replay substrate's identity, as
/// [`acts_digest`] is the canned one's.
#[must_use]
pub fn replay_digest() -> String {
    digest_of(&replay_acts())
}

/// What the stream-replay server answers `GET /props` with:
/// `canned-` and [`replay_digest`], the literal its registry entry declares.
#[must_use]
pub fn replay_build_info() -> String {
    format!("canned-{}", replay_digest())
}

/// The streamed tool call the tool-turn replay plays (#411's follow-up): a
/// real llama.cpp `e486f80` server calling `bash` with `ls | wc -l`
/// (`substrates/measurements/2026-10-02-i0-tool-call-captures/`, turn 1).
pub const REPLAYED_CALL: &[u8] =
    include_bytes!("../../../substrates/measurements/2026-10-02-i0-tool-call-captures/turn1.http");

/// The same server's answer once the call's output came back as a `tool`
/// message (the same captures, turn 2 in the `openai` shape): it settles
/// `stop`.
pub const REPLAYED_ANSWER: &[u8] = include_bytes!(
    "../../../substrates/measurements/2026-10-02-i0-tool-call-captures/turn2-openai.http"
);

/// The acts the tool-turn replay plays: [`REPLAYED_CALL`] for a request that
/// does not end in a tool result, [`REPLAYED_ANSWER`] for one that does
/// ([`crate::client::stub::Stub::replaying_tool_turns`]).
#[must_use]
pub fn replay_tool_acts() -> Vec<Act> {
    vec![
        Act::Raw(REPLAYED_CALL.to_vec()),
        Act::Raw(REPLAYED_ANSWER.to_vec()),
    ]
}

/// The digest of [`replay_tool_acts`]: substrate `canned-replay-tools`.
#[must_use]
pub fn replay_tools_digest() -> String {
    digest_of(&replay_tool_acts())
}

/// What the tool-turn replay answers `GET /props` with.
#[must_use]
pub fn replay_tools_build_info() -> String {
    format!("canned-{}", replay_tools_digest())
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
            Act::StreamThenHold(chunks) => {
                let _ = write!(canonical, "stream-then-hold;{};", chunks.len());
                for chunk in chunks {
                    piece(&mut canonical, chunk);
                }
            }
            Act::Trickle(gap, chunks) => {
                let _ = write!(canonical, "trickle;{};{};", gap.as_nanos(), chunks.len());
                for chunk in chunks {
                    piece(&mut canonical, chunk);
                }
            }
            Act::Raw(bytes) => {
                // Hex rather than a lossy decode: two different byte strings
                // that are not UTF-8 must not canonicalise alike.
                let _ = write!(canonical, "raw;{};", bytes.len());
                for byte in bytes {
                    let _ = write!(canonical, "{byte:02x}");
                }
                canonical.push(';');
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
    crate::digest::sha256_hex(canonical.as_bytes())
}

/// One reply in the shape a llama.cpp-dialect server sends.
///
/// Rendered here rather than read from a fixture file because it is six
/// strings and a usage block; a file would be a seventh thing to keep in
/// step.
#[must_use]
pub fn reply(text: &str) -> String {
    answer(text, "{\"cache_n\":512}")
}

/// [`reply`]'s body, with `timings` as given.
fn answer(text: &str, timings: &str) -> String {
    let escaped = escaped(text);
    format!(
        "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{escaped}\"}},\
         \"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":{PROMPT_TOKENS},\
         \"completion_tokens\":7}},\"generation_settings\":{{}},\
         \"timings\":{timings}}}"
    )
}

/// A reply that says what it thought as well as what it answered.
///
/// The field is `reasoning_content`, which is where both dialects this client
/// carries declare it -- see `client::shape::Dialect::reasoning`. NOT folded
/// into `content`: a server that put its thinking in the answer would be a
/// different server, and #94's negative control reads the two apart.
///
/// NOT in [`acts`], and that is load-bearing. `acts_digest()` is the canned
/// substrate's whole identity, it is cited in `substrates/registry.toml` and
/// in a committed record fixture, and a new act in that list would move it --
/// so the reasoning acts are functions a test composes its own list out of.
#[must_use]
pub fn reply_with_reasoning(text: &str, reasoning: &str) -> String {
    format!(
        "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{}\",\
         \"reasoning_content\":\"{}\"}},\"finish_reason\":\"stop\"}}],\
         \"usage\":{{\"prompt_tokens\":{PROMPT_TOKENS},\"completion_tokens\":7}},\
         \"generation_settings\":{{}},\"timings\":{{\"cache_n\":512}}}}",
        escaped(text),
        escaped(reasoning),
    )
}

/// A reply whose whole budget went inside the think block.
///
/// `finish_reason` is `length` -- the server saying it ran out of room --
/// the reasoning is what it got through, and the answer is the empty string:
/// not "the model declined", not "the answer was cut off", but a generation
/// that never reached the answer at all. #94 design point 5's input.
#[must_use]
pub fn reply_exhausted_in_the_think_block(reasoning: &str) -> String {
    format!(
        "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"\",\
         \"reasoning_content\":\"{}\"}},\"finish_reason\":\"length\"}}],\
         \"usage\":{{\"prompt_tokens\":{PROMPT_TOKENS},\"completion_tokens\":128}},\
         \"generation_settings\":{{}},\"timings\":{{\"cache_n\":512}}}}",
        escaped(reasoning),
    )
}

/// One string, as a JSON string literal's interior.
///
/// Four replies here escape the same way. Two of them spelled it out
/// inline, which is two copies of the same four lines: one fix and one
/// survivor.
fn escaped(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
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

    use std::collections::BTreeMap;

    use super::{
        Act, DEV_LOOP, acts, acts_as_first_registered, acts_digest, digest_of, hardware_fingerprint,
    };

    use crate::drive::registry::REGISTRY;

    /// Every `[kind.id]` table of a TOML document, with its one-line string
    /// values: `drive::registry`'s scan, the one copy of it.
    ///
    /// BY RULE (#162), every field read here stays a one-line
    /// `key = "value"` string in the registry and in `dev-loop.toml`:
    /// `entry_type`, `acts_sha256`, `hardware_fingerprint`, `weights_kind`,
    /// `weights_acts_sha256`, `engine_identity`, `equipment`, `substrate`
    /// and `substrate_hardware`. A field written another way is not read,
    /// and the tests fail rather than pass on nothing: `registered` panics
    /// on a missing key, and `every_registered_canned_digest_is_acts_this_crate_keeps`
    /// counts the canned entries it read against the raw text.
    fn tables(document: &str) -> BTreeMap<String, BTreeMap<String, String>> {
        crate::drive::registry::tables(document)
            .into_iter()
            .map(|(name, table)| (name, table.strings))
            .collect()
    }

    /// The registry's value of `key` in table `table`.
    fn registered<'a>(
        registry: &'a BTreeMap<String, BTreeMap<String, String>>,
        table: &str,
        key: &str,
    ) -> &'a str {
        registry
            .get(table)
            .and_then(|fields| fields.get(key))
            .unwrap_or_else(|| panic!("the registry has no `{key}` in `[{table}]`"))
    }

    #[test]
    fn the_first_registered_acts_still_hash_to_their_registered_digest() {
        // D9 (#156): the acts records name are never mutated.
        let registry = tables(REGISTRY);
        let first = digest_of(&acts_as_first_registered());
        assert_eq!(
            registered(&registry, "substrate.canned", "weights_acts_sha256"),
            first
        );
        assert_eq!(
            registered(&registry, "equipment.canned-loopback", "acts_sha256"),
            first
        );
        assert_eq!(
            registered(
                &registry,
                "equipment.canned-loopback",
                "hardware_fingerprint"
            ),
            hardware_fingerprint(&first)
        );
    }

    #[test]
    fn every_registered_canned_digest_is_acts_this_crate_keeps() {
        let registry = tables(REGISTRY);
        let kept = [
            acts_digest(),
            digest_of(&acts_as_first_registered()),
            super::replay_digest(),
            super::replay_tools_digest(),
        ];
        let mut current_is_registered = false;
        let (mut servers, mut substrates) = (0, 0);
        for (table, fields) in &registry {
            if table.starts_with("equipment.")
                && fields.get("entry_type").map(String::as_str) == Some("canned-server")
            {
                servers += 1;
                let acts = registered(&registry, table, "acts_sha256");
                assert!(kept.iter().any(|digest| digest == acts), "[{table}] {acts}");
                assert_eq!(
                    registered(&registry, table, "hardware_fingerprint"),
                    hardware_fingerprint(acts),
                    "[{table}]"
                );
            }
            if table.starts_with("substrate.")
                && fields.get("weights_kind").map(String::as_str) == Some("canned")
            {
                substrates += 1;
                let acts = registered(&registry, table, "weights_acts_sha256");
                assert!(kept.iter().any(|digest| digest == acts), "[{table}] {acts}");
                assert_eq!(
                    registered(&registry, table, "engine_identity"),
                    acts,
                    "[{table}]"
                );
                // The literal its server would report (#219 item 11).
                assert_eq!(
                    registered(&registry, table, "engine_build_info"),
                    format!("canned-{acts}"),
                    "[{table}]"
                );
                current_is_registered |= *acts == acts_digest();
            }
        }
        assert!(
            current_is_registered,
            "no canned substrate is registered for the acts this crate plays, {}",
            acts_digest()
        );
        // Every canned entry the text declares was read: a line the scan
        // cannot parse drops its entry out of this check, and is caught here.
        let declaring = |key: &str, value: &str| {
            REGISTRY
                .lines()
                .filter(|line| line.trim_start().starts_with(key) && line.contains(value))
                .count()
        };
        assert_eq!(servers, declaring("entry_type", "\"canned-server\""));
        assert_eq!(substrates, declaring("weights_kind", "\"canned\""));
        assert!(servers > 0 && substrates > 0, "no canned entry was read");
    }

    #[test]
    fn the_regimen_names_the_substrate_that_plays_these_acts() {
        // `dev-loop.toml` cites a substrate and its hardware; both are now
        // checked against the acts, not only copied.
        let registry = tables(REGISTRY);
        let regimen = &tables(DEV_LOOP)[""];
        let substrate = format!("substrate.{}", regimen["substrate"]);
        assert_eq!(
            registered(&registry, &substrate, "weights_acts_sha256"),
            acts_digest(),
            "[{substrate}]"
        );
        let equipment = format!(
            "equipment.{}",
            registered(&registry, &substrate, "equipment")
        );
        assert_eq!(
            registered(&registry, &equipment, "hardware_fingerprint"),
            regimen["substrate_hardware"]
        );
        assert_eq!(
            regimen["substrate_hardware"],
            hardware_fingerprint(&acts_digest())
        );
    }

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

    /// The rehearsal regimen names the stream-replay substrate and the
    /// fingerprint of the acts the replay plays (#411).
    #[test]
    fn the_rehearsal_regimen_names_the_stream_replay_and_its_acts() {
        const REPLAY: &str = include_str!("../../drive/replay.toml");
        let value = |key: &str| {
            REPLAY
                .lines()
                .find_map(|line| line.strip_prefix(&format!("{key} = \"")))
                .and_then(|rest| rest.strip_suffix('"'))
                .unwrap_or_else(|| panic!("replay.toml declares no {key}"))
        };
        assert_eq!(value("substrate"), "canned-replay-tools");
        assert_eq!(
            value("substrate_hardware"),
            hardware_fingerprint(&super::replay_tools_digest())
        );
    }

    /// The tool-turn replay is cited (`projection::CITED`) because in each
    /// of its captures the server's `usage` equals its `timings`: completion
    /// tokens are `predicted_n`, prompt tokens `prompt_n + cache_n`.
    #[test]
    fn the_tool_turn_replays_usage_is_its_timings() {
        let last = |capture: &str, key: &str| -> u64 {
            let at = capture
                .rfind(&format!("\"{key}\":"))
                .unwrap_or_else(|| panic!("no {key}"));
            capture[at + key.len() + 3..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or_else(|why| panic!("{key}: {why}"))
        };
        for bytes in [super::REPLAYED_CALL, super::REPLAYED_ANSWER] {
            let capture = String::from_utf8_lossy(bytes);
            assert_eq!(
                last(&capture, "completion_tokens"),
                last(&capture, "predicted_n")
            );
            assert_eq!(
                last(&capture, "prompt_tokens"),
                last(&capture, "prompt_n") + last(&capture, "cache_n")
            );
        }
    }
}
