//! The render: the working object turned into the prompt a session continues
//! from.
//!
//! This is the architecture's single deliberate prefill event, and its
//! economics are the reason everything else exists. A 50K-token prefix
//! re-sent identically costs 0.10 s warm against 46 s cold -- 444x -- and any
//! edit to the prefix forces a full re-prefill, because reuse snaps to the
//! engine's checkpoint boundaries. (One model, one engine, one card;
//! re-measure per substrate.) So the prefix stays byte-identical during a
//! phase, state accumulates outside the context, and this function runs only
//! at a seam.
//!
//! It is a **pure function** of the object and the phase. Nothing here reads
//! a clock, a counter, or anything that moves: two renders of the same object
//! in the same phase are the same bytes, and the byte-identity claim the seam
//! controller makes is only checkable because that is true.

use std::fmt::Write as _;

use crate::client::shape::{Message, Role};
use crate::object::WorkingObject;

/// Which frame this module renders with.
///
/// **A placeholder, and it says so in the render itself.** The frame is
/// prompt text the model reads, and prompt text in this program is
/// judgment-as-data: it is authored by the maintainer, pinned, and
/// byte-guarded like every other template. This one is not. Naming it in the
/// prompt means a drive under the placeholder is distinguishable from a drive
/// under the authored frame -- which matters because they are different
/// regimes, and a comparison across them that could not tell would be the
/// incomparable-regime class again.
pub const FRAME_VERSION: &str = "placeholder";

/// What the render does past its budget (#565).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverBudget {
    /// An older entry keeps its first line, cut at [`TIER_CHARS`].
    Tier,
    /// An older entry keeps only its id.
    Elide,
}

/// The render's budget (#565): estimated tokens (chars/4, as #552's depth),
/// and what becomes of the older entries past it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    /// The estimated tokens the whole render may run to.
    pub tokens: u64,
    /// What becomes of an entry past it.
    pub over: OverBudget,
}

/// A render, and what its budget did to it (#565).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The render.
    pub text: String,
    /// Its estimated tokens.
    pub tokens: u64,
    /// How many entries it shortened or elided.
    pub reduced: u64,
}

/// The most characters a tiered entry keeps of its first line.
pub const TIER_CHARS: usize = 80;

/// The entry kinds that are the brief and always render in full (#565):
/// the goal and the constraints, by the fold's `<kind>: ` prefix.
const BRIEF: &[&str] = &["goal: ", "constraint: "];

/// The working object rendered as the prompt's prefix.
///
/// The regime is carried at the top, because a prompt whose record cannot say
/// which substrate and which dogma it was built for is a prompt that does not
/// transfer.
#[must_use]
pub fn render(object: &WorkingObject, phase: Option<&str>) -> String {
    rendered(object, phase, None).text
}

/// [`render`] under a `budget` (#565), still a pure function of the object,
/// the phase and the budget. With none, or with everything inside it, the
/// bytes are [`render`]'s. Past it, the goal and the constraints still
/// render whole; every other entry is kept whole newest first -- newest by
/// its latest patch, the greatest of its provenances -- until the next would
/// pass the budget, and that one and every older one is tiered or elided in
/// place, with one line after the working set counting them.
#[must_use]
pub fn rendered(object: &WorkingObject, phase: Option<&str>, budget: Option<Budget>) -> Rendered {
    let mut out = head(object, phase);
    let line = |id: &str, content: &str| format!("{}\t{}\n", one_line(id), one_line(content));
    let brief = |content: &str| BRIEF.iter().any(|kind| content.starts_with(kind));
    // Which entries are reduced: none without a budget; with one, the
    // header and the brief are spent first, then the rest newest first.
    let mut reduced = std::collections::BTreeSet::new();
    if let Some(budget) = budget {
        let mut spent = estimate(&out)
            + object
                .live()
                .filter(|entry| brief(&entry.content))
                .map(|entry| estimate(&line(entry.id.as_str(), &entry.content)))
                .sum::<u64>();
        let mut rest: Vec<_> = object
            .live()
            .filter(|entry| !brief(&entry.content))
            .collect();
        rest.sort_by(|a, b| {
            b.provenances
                .iter()
                .max()
                .cmp(&a.provenances.iter().max())
                .then_with(|| b.id.cmp(&a.id))
        });
        let mut over = false;
        for entry in rest {
            let cost = estimate(&line(entry.id.as_str(), &entry.content));
            over = over || spent + cost > budget.tokens;
            if over {
                reduced.insert(entry.id.clone());
            } else {
                spent += cost;
            }
        }
    }
    let over = budget.map_or(OverBudget::Tier, |budget| budget.over);
    let mut count = 0_u64;
    for entry in object.live() {
        if !reduced.contains(&entry.id) {
            out.push_str(&line(entry.id.as_str(), &entry.content));
            continue;
        }
        match over {
            OverBudget::Tier => {
                let first = entry.content.split('\n').next().unwrap_or_default();
                let mut cut: String = first.chars().take(TIER_CHARS).collect();
                // An entry already one short line is left as it is, and not
                // counted: tiering did nothing to it.
                if cut.chars().count() < entry.content.chars().count() {
                    cut.push('…');
                    count += 1;
                }
                out.push_str(&line(entry.id.as_str(), &cut));
            }
            OverBudget::Elide => {
                out.push_str(&one_line(entry.id.as_str()));
                out.push('\n');
                count += 1;
            }
        }
    }
    if count > 0 {
        let what = match over {
            OverBudget::Tier => "shortened",
            OverBudget::Elide => "elided",
        };
        let _ = writeln!(out, "{count} older entries {what}");
    }
    Rendered {
        tokens: estimate(&out),
        text: out,
        reduced: count,
    }
}

/// Estimated tokens of `text`: its characters over four, rounded up, as
/// [`estimated_tokens`] counts a message.
fn estimate(text: &str) -> u64 {
    (text.chars().count() as u64).div_ceil(4)
}

/// The render's header: the regime, the phase, the frame, and the working
/// set's heading.
fn head(object: &WorkingObject, phase: Option<&str>) -> String {
    let regime = object.regime();
    let mut out = String::new();

    out.push_str("# regime\n");
    out.push_str("arm: ");
    out.push_str(&regime.arm);
    out.push_str("\nsubstrate: ");
    // THE DECLARED IDS, in declaration order, and all of them. A regime can
    // now carry several -- a drive whose interview fork runs on a small local
    // model while the main lane runs a large one is the arrangement this
    // repository exists to measure -- so a frame naming one of them would be
    // naming whichever happened to be first.
    //
    // This CHANGES THE RENDERED BYTES of every seam, and there was no option
    // that did not: the field this line used to read, `substrate.name`, no
    // longer exists. Prose identity was what the digest replaced. The frame's
    // shape is unchanged -- one line, one label, one value -- which is what
    // the byte-identity claim below is about.
    out.push_str(
        &regime
            .substrates
            .iter()
            .map(|s| s.id.as_str())
            .collect::<Vec<_>>()
            .join(", "),
    );
    out.push_str("\ndogma_version: ");
    out.push_str(&regime.dogma_version.to_string());
    out.push_str("\nphase: ");
    // A session with no phase graph renders `-` rather than omitting the
    // line. A field that disappears when it is empty makes two prompts differ
    // in shape as well as in content, and the byte-identity claim is about
    // shape as much as anything.
    out.push_str(phase.unwrap_or("-"));
    out.push_str("\nframe_version: ");
    out.push_str(FRAME_VERSION);
    out.push_str("\n\n");
    out.push_str(WORKING_SET_HEADER);
    out
}

/// The trunk a served session continues from after a seam (#493): its
/// `head`, with `render` after the head's standing instruction, and nothing
/// of the old trunk.
///
/// A TOTAL compaction, by the maintainer's intent on #493: the refill is not
/// a summary of the old turns with the later ones kept, and it does not try
/// to make the model believe it is continuing the session. Each seam hands it
/// a better starting point, built from working memory alone.
///
/// The render joins the first message when that message is the system one,
/// separated by a blank line, rather than arriving as a message of its own: a
/// second system message, or two user messages in a row, is a shape some chat
/// templates refuse. A head with no system message gets one, holding the
/// render. The session and the record's projection both rebuild the trunk
/// through this one function, so the projection's head check holds the
/// session to it.
#[must_use]
pub fn refill(head: &[Message], render: &str) -> Vec<Message> {
    let mut trunk = head.to_vec();
    match trunk.first_mut() {
        Some(first) if first.role == Role::System => {
            first.content.push_str("\n\n");
            first.content.push_str(render);
        }
        _ => trunk.insert(0, Message::new(Role::System, render)),
    }
    trunk
}

/// A message's estimated tokens: its characters -- content, reasoning, each
/// tool call's name and arguments, each image's encoding -- divided by four,
/// rounded up. The harnesses compared estimate the same way, none with a
/// tokenizer (#552; Pi `compaction.ts` 298-349, `OpenCode` 2 `util/token.ts`
/// 3-5); Pi's rounding up, which never undercounts.
#[must_use]
pub fn estimated_tokens(message: &Message) -> u64 {
    let chars = message.content.chars().count()
        + message.reasoning.as_ref().map_or(0, |r| r.chars().count())
        + message
            .tool_calls
            .iter()
            .map(|call| call.name.chars().count() + call.arguments.chars().count())
            .sum::<usize>()
        + message
            .images
            .iter()
            .map(|image| image.data_uri().len())
            .sum::<usize>();
    u64::try_from(chars.div_ceil(4)).unwrap_or(u64::MAX)
}

/// The recent tail of `turns` -- the trunk after its head -- a seam keeps
/// after the refill (#552): whole turns, from the newest back, while their
/// estimated tokens stay within `budget`. A turn starts at its user message:
/// the tail never starts at an assistant reply or a tool result, so no call
/// is parted from its result, and the chat template driven here refuses a
/// conversation with no user message after the system one. The turn that
/// would cross the budget is not kept (the budget is a ceiling); with no
/// whole turn within it, or a budget of 0, the tail is empty -- the total
/// refill (#505).
#[must_use]
pub fn tail(turns: &[Message], budget: u64) -> &[Message] {
    let mut start = turns.len();
    let mut spent = 0_u64;
    let mut since = 0_u64;
    for (at, message) in turns.iter().enumerate().rev() {
        since = since.saturating_add(estimated_tokens(message));
        if message.role == Role::User {
            if spent.saturating_add(since) > budget {
                break;
            }
            spent += since;
            since = 0;
            start = at;
        }
    }
    &turns[start..]
}

/// The last `n` whole turns of `turns` (#567), each starting at its user
/// message as [`tail`]'s do, so no call is parted from its result; all of
/// them when there are fewer, none when `n` is 0.
#[must_use]
pub fn last_turns(turns: &[Message], n: usize) -> &[Message] {
    if n == 0 {
        return &turns[turns.len()..];
    }
    let mut seen = 0;
    for (at, message) in turns.iter().enumerate().rev() {
        if message.role == Role::User {
            seen += 1;
            if seen == n {
                return &turns[at..];
            }
        }
    }
    let first = turns
        .iter()
        .position(|message| message.role == Role::User)
        .unwrap_or(turns.len());
    &turns[first..]
}

/// `text` with everything that could forge a row escaped out of it.
///
/// **The render owns the grammar of the prompt it emits, and it used to have
/// none.** A working-set row is `id \t content \n`, and entry content comes
/// from model output through the capture lanes -- so a single note containing
/// a newline, a tab and an identifier of the model's choosing wrote an extra
/// row into the next prefix, with whatever id it liked. Two different objects
/// rendered to the same bytes, which also means the object could move without
/// the prefix moving: the byte-identity claim with a hole in the other
/// direction.
///
/// It matters twice over, because the same text is numbered into the audit
/// ask, and all three pinned templates say *"reply with EXACTLY one line,
/// each starting with its number ... Output {n} lines and nothing else"*. An
/// unescaped newline there put an unnumbered line in the middle of the list,
/// and a ratifier folding answer line *k* onto item *k* then wrote a verdict
/// onto the wrong entry.
///
/// Escaped rather than refused: the content is already in the object by the
/// time it gets here, and a render that refused to draw an entry would be a
/// prompt silently missing a fact.
#[must_use]
pub fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// How many bytes of working set the render would carry.
///
/// The budget trigger measures THIS rather than a count of entries, because
/// the thing approaching a ceiling is the prompt, and an entry is not a fixed
/// size. Deriving it from the object rather than taking it as an argument
/// means no caller can pass a number that disagrees with what would actually
/// be sent.
#[must_use]
pub fn working_set_bytes(object: &WorkingObject) -> u64 {
    let mut total: u64 = 0;
    for entry in object.live() {
        // Measured through the same escaping the render uses, and counting
        // the same separators, so the two cannot disagree. They used to
        // differ by any proportional amount without a test noticing, because
        // the only bounds asserted were "more than zero" and "less than the
        // whole render" -- and the whole render carries a regime header worth
        // a hundred bytes of slack.
        //
        // A count that saturates is a count that stops being a measurement.
        // Nothing here can reach it -- an object of eighteen exabytes is not
        // a thing -- and saturating is still the right floor for a number
        // that decides a trigger.
        total = total
            .saturating_add(one_line(entry.id.as_str()).len() as u64)
            .saturating_add(one_line(&entry.content).len() as u64)
            .saturating_add(2);
    }
    total
}

/// Where the working set begins in a render.
///
/// Public so a caller -- and the test that pins
/// [`working_set_bytes`] against what is actually emitted -- can find the
/// section without re-deriving the header's shape.
pub const WORKING_SET_HEADER: &str = "# working set\n";

#[cfg(test)]
mod tail_tests {
    use super::{Message, Role, estimated_tokens, tail};
    use crate::client::shape::ToolCall;

    fn user(text: &str) -> Message {
        Message::new(Role::User, text)
    }

    fn turn(ask: &str, chars: usize) -> Vec<Message> {
        let mut call = Message::new(Role::Assistant, "");
        call.tool_calls = vec![ToolCall {
            id: "c".to_owned(),
            name: "bash".to_owned(),
            arguments: "{}".to_owned(),
        }];
        vec![
            user(ask),
            call,
            Message::tool_result("c".to_owned(), "x".repeat(chars)),
            Message::new(Role::Assistant, "done"),
        ]
    }

    fn tokens(messages: &[Message]) -> u64 {
        messages.iter().map(estimated_tokens).sum()
    }

    /// A message's estimate is its characters over four, rounded up, its
    /// tool calls' names and arguments counted.
    #[test]
    fn a_messages_tokens_are_its_characters_over_four_rounded_up() {
        assert_eq!(estimated_tokens(&user("abcde")), 2);
        let mut call = Message::new(Role::Assistant, "");
        call.tool_calls = vec![ToolCall {
            id: "c".to_owned(),
            name: "bash".to_owned(),
            arguments: "{\"command\":\"ls\"}".to_owned(),
        }];
        assert_eq!(estimated_tokens(&call), 5);
    }

    /// Whole turns from the newest back, within the budget; the turn that
    /// would cross it is not kept; a budget of 0, or one no whole turn fits,
    /// keeps nothing.
    #[test]
    fn the_tail_is_whole_turns_from_the_newest_back_within_the_budget() {
        let turns: Vec<Message> = [turn("one", 400), turn("two", 400), turn("three", 400)].concat();
        let one_turn = tokens(&turn("three", 400));
        assert!(tail(&turns, 0).is_empty());
        assert!(
            tail(&turns, one_turn - 1).is_empty(),
            "the crossing turn is not kept"
        );
        let kept = tail(&turns, one_turn);
        assert_eq!(kept.len(), 4);
        assert_eq!(kept[0], user("three"));
        let kept = tail(&turns, one_turn * 2 + 5);
        assert_eq!(kept.len(), 8);
        assert_eq!(kept[0], user("two"));
        assert_eq!(tail(&turns, u64::MAX).len(), turns.len());
    }

    /// Turns that start mid-way (a trunk whose first kept message is not an
    /// ask) are never cut into: the tail starts only at a user message.
    #[test]
    fn the_tail_never_starts_at_a_reply_or_a_tool_result() {
        let mut turns = turn("one", 10);
        turns.remove(0);
        assert!(
            tail(&turns, u64::MAX).is_empty(),
            "{:?}",
            tail(&turns, u64::MAX)
        );
    }
}
