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

/// The working object rendered as the prompt's prefix.
///
/// The regime is carried at the top, because a prompt whose record cannot say
/// which substrate and which dogma it was built for is a prompt that does not
/// transfer.
#[must_use]
pub fn render(object: &WorkingObject, phase: Option<&str>) -> String {
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

    for entry in object.live() {
        out.push_str(&one_line(entry.id.as_str()));
        out.push('\t');
        out.push_str(&one_line(&entry.content));
        out.push('\n');
    }

    out
}

/// The trunk a served session continues from after a seam (#493): its
/// `head`, untouched, then [`refill_message`] carrying `render`, and nothing
/// of the old trunk.
///
/// A TOTAL compaction, by the maintainer's intent on #493: the refill is not
/// a summary of the old turns with the later ones kept, and it does not try
/// to make the model believe it is continuing the session. Each seam hands it
/// a better starting point, built from working memory alone.
///
/// The system message is never changed (#597, the maintainer: "System
/// prompt mutation is forbidden"): after a seam it is byte-identical to the
/// session's first. The session and the record's projection both rebuild
/// the trunk through this one function, so the projection's head check
/// holds the session to it.
#[must_use]
pub fn refill(head: &[Message], render: &str) -> Vec<Message> {
    let mut trunk = head.to_vec();
    trunk.push(refill_message(render));
    trunk
}

/// The message a seam's refill rides in (#597), shaped as Pi and `OpenCode`
/// 2 carry a compaction summary: a user message, the text inside
/// `<summary>` tags, opened and closed on lines of their own (Pi
/// `core/messages.ts:11-17`, `OpenCode` 2 `session/runner/to-llm-message.ts:
/// 147-158`). Their lead sentences differ and Qwen Code's summary message
/// has none, so it has none; no harness but Qwen Code adds a trailer, so it
/// has none.
#[must_use]
pub fn refill_message(render: &str) -> Message {
    Message::new(Role::User, format!("<summary>\n{render}\n</summary>"))
}

/// The trunk after a seam logged before #597: `render` appended to the
/// head's system message after a blank line, or a system message of its
/// own when the head has none. What those logs' heads were sent, so they
/// still rebuild.
#[must_use]
pub fn refill_in_the_system_message(head: &[Message], render: &str) -> Vec<Message> {
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
mod refill_tests {
    use super::*;

    /// The head is never changed (#597): the render rides in a user
    /// message after it, inside `<summary>` tags on lines of their own.
    #[test]
    fn a_refill_leaves_the_head_alone_and_carries_the_render_in_a_user_message() {
        let head = vec![Message::new(Role::System, "you are the trunk")];
        let trunk = refill(&head, "# regime\n");
        assert_eq!(trunk[..1], head[..]);
        assert_eq!(trunk.len(), 2);
        assert_eq!(trunk[1].role, Role::User);
        assert_eq!(trunk[1].content, "<summary>\n# regime\n\n</summary>");
        // Before #597, appended to the system message.
        let before = refill_in_the_system_message(&head, "# regime\n");
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].content, "you are the trunk\n\n# regime\n");
    }
}

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
