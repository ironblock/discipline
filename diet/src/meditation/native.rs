//! What a native harness carries across a compaction (#936): under
//! `meditation_carry = "native_summary"`, the refill holds a summary the
//! model writes with the vote's compaction prompt, and what Qwen Code puts
//! beside it, in place of the working-memory render.
//!
//! The vote (Pi `v1.3.0`, `OpenCode` 2 `v2.0.32`, Qwen Code `v0.25.3`; Qwen
//! Code breaks a tie) is in `docs/harness-baseline.md`. The three differ on
//! the prompt, how the history is shown, the output cap and tools, so each
//! is Qwen Code's; what rides beside the summary is voted item by item:
//!
//! - **The request:** the compression prompt as the system message
//!   (`qc:packages/core/src/core/prompts.ts:925-971`), the history as
//!   messages, then the one-line directive
//!   (`qc:packages/core/src/services/chatCompressionService.ts:100-101`); no
//!   tools and reasoning off, as its cold path sends it (`:838-880`,
//!   `packages/core/src/utils/sideQuery.ts:96-99`); its output at most
//!   20,000 tokens, fitted to the window less 1,024 (`chatCompressionService.ts:58,69,91-98`).
//! - **The summary:** its `<analysis>` scratchpad stripped, closed or not
//!   (`qc:packages/core/src/services/postCompactAttachments.ts:552-561`).
//!   An empty one is a failure (`:563-571`). It rides in our refill message
//!   (#597): Pi and `OpenCode` 2 agree on a user message in `<summary>`
//!   tags, so Qwen Code's trailer is not added.
//! - **Beside it**, voted item by item (#1009; the earlier vote took it as
//!   one bundle, which the three differed on, so Qwen Code's whole bundle
//!   was carried). Each item at the pinned tags:
//!   - **The acknowledgement: none.** Pi sends none: its rebuilt context is
//!     the summary, then the kept entries (`pi:…/core/session-manager.ts:499-511`).
//!     `OpenCode` 2 sends none: after an automatic compaction the runner goes
//!     on to the next request (`oc:packages/core/src/session/runner/llm.ts:220-225`).
//!     Qwen Code alone adds "Got it. Thanks for the additional context!"
//!     (`qc:…/postCompactAttachments.ts:856-858`). Pi and `OpenCode` 2 agree.
//!   - **The restored files: ruled (#1012), Pi's path lists.** Pi and
//!     `OpenCode` 2 agree on no file contents, only paths: Pi appends
//!     `<read-files>`/`<modified-files>` lists to the summary
//!     (`pi:…/compaction/compaction.ts:1056-1058`, `utils.ts:77-87`), and
//!     `OpenCode` 2's summary template has a "Relevant Files" section of at
//!     most 15 (`oc:packages/core/src/session/compaction.ts:129-131`); Qwen
//!     Code embeds the contents. The maintainer ruled for Pi's lists, so no
//!     restored-files message and no images follow the summary. Pi's
//!     definitions (`utils.ts:47-72`) map onto our file tools: a `read`'s
//!     `path` is read, a `write`'s `path` and an `edit`'s `file_path` are
//!     modified, each as the call named it, failed or not; read lists only
//!     what was not modified; both sorted, each list left out when empty.
//!     They gather over the history summarized and every earlier native
//!     refill's lists, the earlier read as read and the earlier modified as
//!     modified (`compaction.ts:60-89,917-923`).
//!   - **The kept recent turns: ruled (#1012), as built.** Pi keeps them as
//!     messages, 20,000 tokens by default
//!     (`pi:…/compaction/compaction.ts:126-130,802-829`); `OpenCode` 2 as
//!     text inside its checkpoint, 15,000 by default
//!     (`oc:packages/core/src/session/compaction.ts:197,704-811`); Qwen Code
//!     keeps none (`qc:…/chatCompressionService.ts:570-574`). The maintainer
//!     ruled that the compaction depth lever (`meditation_tail_tokens`)
//!     decides, as built, total by default.
//!
//!   Qwen Code's plan-mode reminder and running-subagent list are not
//!   carried: this harness has neither. The start line's `meditation_carry`
//!   lever names the layout, `native_summary:summary+paths`; runs between
//!   #1009 and #1012 read `native_summary:summary+restored`, Qwen Code's
//!   restored files after the summary, and runs before #1009 read
//!   `native_summary` alone, with the acknowledgement between.

use std::collections::BTreeSet;

use crate::client::shape::{Message, RequestShape, Role};
use crate::formats::record::json::Value;

/// Qwen Code's compression prompt, verbatim
/// (`qc:packages/core/src/core/prompts.ts:927-969` at `v0.25.3`), pinned in
/// the dogma (`crate::dogma::compaction`).
pub const PROMPT: &str = crate::dogma::compaction::QWEN_CODE;

/// The directive the history is followed by
/// (`qc:packages/core/src/services/chatCompressionService.ts:100-101`).
pub const DIRECTIVE: &str =
    "First, reason in your <analysis> block. Then, produce the <state_snapshot> XML.";

/// The summary's output cap (`chatCompressionService.ts:58`).
pub const OUTPUT_CAP: u64 = 20_000;

/// What the cap leaves of the window below the prompt (`:69`).
const OUTPUT_MARGIN: u64 = 1_024;

/// What a native summary's refill carries (#936): the summary, its analysis
/// stripped, then Pi's path lists (#1012).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The summary, never empty, the lists of `files` appended.
    pub text: String,
    /// The files it lists: what the next native refill gathers from.
    pub files: Files,
}

/// The files a native refill names (#1012): Pi's read and modified lists
/// (`pi:…/compaction/utils.ts:67-72`), read holding none of modified.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Files {
    /// Read and not modified.
    pub read: BTreeSet<String>,
    /// Written or edited.
    pub modified: BTreeSet<String>,
}

/// The summary request: `history` -- the trunk past its head, and what rides
/// after it -- under the compression prompt, then the directive; no tools,
/// reasoning off, and its output cap fitted to `template`'s window.
#[must_use]
pub fn request(template: &RequestShape, history: &[Message]) -> RequestShape {
    let mut shape = template.clone();
    shape.messages = std::iter::once(Message::new(Role::System, PROMPT.trim().to_owned()))
        .chain(
            history
                .iter()
                .filter(|message| message.role != Role::System)
                .cloned(),
        )
        .chain(std::iter::once(Message::new(
            Role::User,
            DIRECTIVE.to_owned(),
        )))
        .collect();
    shape.tools.clear();
    if shape.template_kwargs.contains_key("enable_thinking") {
        shape
            .template_kwargs
            .insert("enable_thinking".to_owned(), Value::Boolean(false));
    }
    shape.template_kwargs.remove("reasoning_effort");
    let input = shape
        .messages
        .iter()
        .map(crate::meditation::render::estimated_tokens)
        .sum::<u64>();
    let cap = template.limits.context_window.map_or(OUTPUT_CAP, |window| {
        OUTPUT_CAP.min(window.saturating_sub(input).saturating_sub(OUTPUT_MARGIN))
    });
    shape.limits.max_output_tokens = u32::try_from(cap.max(1)).unwrap_or(u32::MAX);
    shape
}

/// `raw`, the model's answer, with its `<analysis>` blocks taken out --
/// each closed one, then one left open at the end -- and trimmed.
#[must_use]
pub fn stripped(raw: &str) -> String {
    let mut text = raw.to_owned();
    while let Some(start) = text.find("<analysis>") {
        let Some(end) = text[start..].find("</analysis>") else {
            text.truncate(start);
            break;
        };
        let mut after = start + end + "</analysis>".len();
        while text[after..].starts_with(char::is_whitespace) {
            after += text[after..].chars().next().map_or(1, char::len_utf8);
        }
        text.replace_range(start..after, "");
    }
    text.trim().to_owned()
}

/// The files `history`'s file tools named, with `earlier`'s, as Pi
/// gathers them (`pi:…/compaction/utils.ts:47-72`, `compaction.ts:60-89`):
/// a `read`'s `path` read, a `write`'s `path` and an `edit`'s `file_path`
/// modified, each as the call named it, whether or not it failed; read
/// keeps only what was not modified.
#[must_use]
pub fn files(history: &[Message], earlier: &Files) -> Files {
    let mut read = earlier.read.clone();
    let mut modified = earlier.modified.clone();
    for call in history
        .iter()
        .filter(|message| message.role == Role::Assistant)
        .flat_map(|message| &message.tool_calls)
    {
        let (key, into) = match call.name.as_str() {
            crate::drive::standard::READ => ("path", &mut read),
            crate::drive::standard::WRITE => ("path", &mut modified),
            crate::drive::standard::EDIT => ("file_path", &mut modified),
            _ => continue,
        };
        if let Some(path) = serde_json::from_str::<serde_json::Value>(&call.arguments)
            .ok()
            .and_then(|arguments| arguments.get(key)?.as_str().map(str::to_owned))
            .filter(|path| !path.is_empty())
        {
            into.insert(path);
        }
    }
    read.retain(|path| !modified.contains(path));
    Files { read, modified }
}

/// `files` as Pi appends them to its summary
/// (`pi:…/compaction/utils.ts:77-87`): a `<read-files>` and a
/// `<modified-files>` section, a path a line, each left out when empty;
/// empty when both are.
#[must_use]
pub fn listed(files: &Files) -> String {
    let mut sections = Vec::new();
    for (tag, paths) in [
        ("read-files", &files.read),
        ("modified-files", &files.modified),
    ] {
        if !paths.is_empty() {
            let lines: Vec<&str> = paths.iter().map(String::as_str).collect();
            sections.push(format!("<{tag}>\n{}\n</{tag}>", lines.join("\n")));
        }
    }
    if sections.is_empty() {
        return String::new();
    }
    format!("\n\n{}", sections.join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #936: the analysis comes out, closed or left open, and what is left
    /// is trimmed; an answer that is all analysis is empty.
    #[test]
    fn the_analysis_scratchpad_is_stripped_closed_or_open() {
        assert_eq!(
            stripped("<analysis>think</analysis>\n\n<state_snapshot>x</state_snapshot>\n"),
            "<state_snapshot>x</state_snapshot>"
        );
        assert_eq!(
            stripped("<state_snapshot>x</state_snapshot>\n<analysis>cut off"),
            "<state_snapshot>x</state_snapshot>"
        );
        assert_eq!(stripped("<analysis>only</analysis>"), "");
        assert_eq!(stripped("plain"), "plain");
    }

    /// #936: the request is the compression prompt, the history without
    /// its system messages, then the directive; no tools, thinking off, and
    /// the cap at most 20,000, fitted to the window.
    #[test]
    fn the_summary_request_is_qwen_codes_cold_path() {
        let mut template = RequestShape {
            model: "a-model".to_owned(),
            cache_prefix: None,
            messages: vec![Message::new(Role::System, "you are the trunk".to_owned())],
            sampler: crate::client::shape::SamplerCard::empty(),
            limits: crate::client::shape::Limits {
                attempt: std::time::Duration::from_secs(5),
                call: std::time::Duration::from_secs(5),
                max_output_tokens: 64,
                retries: 0,
                context_window: Some(100_000),
                idle: None,
            },
            grammar: None,
            template_kwargs: std::collections::BTreeMap::new(),
            tools: Vec::new(),
        };
        template.tools = crate::drive::tool_loop::ToolSurface::Bash.tools_timed(false, None);
        template
            .template_kwargs
            .insert("enable_thinking".to_owned(), Value::Boolean(true));
        template.template_kwargs.insert(
            "reasoning_effort".to_owned(),
            Value::String("low".to_owned()),
        );
        let history = vec![
            Message::new(Role::System, "head".to_owned()),
            Message::new(Role::User, "build it".to_owned()),
            Message::new(Role::Assistant, "built".to_owned()),
        ];
        let shape = request(&template, &history);
        assert_eq!(shape.messages[0].role, Role::System);
        assert!(
            shape.messages[0]
                .content
                .starts_with("You are the component that summarizes")
        );
        assert!(shape.messages[0].content.ends_with("</state_snapshot>"));
        assert_eq!(shape.messages[1].content, "build it");
        assert_eq!(
            shape.messages.last().map(|m| m.content.as_str()),
            Some(DIRECTIVE)
        );
        assert_eq!(
            shape.messages.len(),
            4,
            "the head's system message is not sent"
        );
        assert!(shape.tools.is_empty());
        assert_eq!(
            shape.template_kwargs.get("enable_thinking"),
            Some(&Value::Boolean(false))
        );
        assert!(!shape.template_kwargs.contains_key("reasoning_effort"));
        assert_eq!(shape.limits.max_output_tokens, 20_000);
        template.limits.context_window = Some(4_000);
        let small = request(&template, &history);
        assert!(
            small.limits.max_output_tokens < 3_000,
            "fitted to the window"
        );
    }

    /// An assistant message calling `name` with `arguments`.
    fn call(id: &str, name: &str, arguments: &str) -> Message {
        let mut message = Message::new(Role::Assistant, String::new());
        message.tool_calls.push(crate::client::shape::ToolCall {
            id: id.to_owned(),
            name: name.to_owned(),
            arguments: arguments.to_owned(),
        });
        message
    }

    /// #1012: Pi's definitions on our file tools -- a `read`'s `path` is
    /// read, a `write`'s `path` and an `edit`'s `file_path` modified, a
    /// failed call counted too, a shell command never; read keeps only what
    /// was not modified, both sorted; and an earlier refill's lists gather
    /// in, its read as read and its modified as modified.
    #[test]
    fn the_file_tools_name_pis_read_and_modified_files() {
        let mut failed = Message::new(Role::Tool, "no such file".to_owned());
        failed.tool_call_id = Some("c3".to_owned());
        failed.tool_error = true;
        let history = vec![
            Message::new(Role::User, "go".to_owned()),
            call("c1", "read", r#"{"path": "src/z.rs"}"#),
            Message::new(Role::Tool, "fn z() {}".to_owned()),
            call(
                "c2",
                "edit",
                r#"{"file_path": "b.md", "old_string": "x", "new_string": "y"}"#,
            ),
            Message::new(Role::Tool, "ok".to_owned()),
            call("c3", "read", r#"{"path": "gone.txt"}"#),
            failed,
            call("c4", "read", r#"{"path": "b.md"}"#),
            Message::new(Role::Tool, "y".to_owned()),
            call("c5", "write", r#"{"path": "out/new.txt", "content": "n"}"#),
            Message::new(Role::Tool, "ok".to_owned()),
            call("c6", "bash", r#"{"command": "cat a.rs > c.rs"}"#),
            Message::new(Role::Tool, String::new()),
        ];
        let found = files(&history, &Files::default());
        assert_eq!(
            found.read.iter().collect::<Vec<_>>(),
            ["gone.txt", "src/z.rs"]
        );
        assert_eq!(
            found.modified.iter().collect::<Vec<_>>(),
            ["b.md", "out/new.txt"]
        );
        let earlier = Files {
            read: ["old.rs".to_owned(), "out/new.txt".to_owned()].into(),
            modified: ["kept.md".to_owned()].into(),
        };
        let gathered = files(&history, &earlier);
        assert_eq!(
            gathered.read.iter().collect::<Vec<_>>(),
            ["gone.txt", "old.rs", "src/z.rs"]
        );
        assert_eq!(
            gathered.modified.iter().collect::<Vec<_>>(),
            ["b.md", "kept.md", "out/new.txt"]
        );
    }

    /// #1012: the lists ride as Pi appends them -- after a blank line, a
    /// `<read-files>` then a `<modified-files>` section, a path a line --
    /// and a list with no file is left out; with neither, nothing is
    /// appended, as Pi emits none.
    #[test]
    fn the_lists_take_pis_tags_and_an_empty_one_is_left_out() {
        let both = Files {
            read: ["a.rs".to_owned(), "c.rs".to_owned()].into(),
            modified: ["b.md".to_owned()].into(),
        };
        assert_eq!(
            listed(&both),
            "\n\n<read-files>\na.rs\nc.rs\n</read-files>\n\n<modified-files>\nb.md\n</modified-files>"
        );
        let modified = Files {
            read: BTreeSet::new(),
            modified: ["b.md".to_owned()].into(),
        };
        assert_eq!(
            listed(&modified),
            "\n\n<modified-files>\nb.md\n</modified-files>"
        );
        let read = Files {
            read: ["a.rs".to_owned()].into(),
            modified: BTreeSet::new(),
        };
        assert_eq!(listed(&read), "\n\n<read-files>\na.rs\n</read-files>");
        assert_eq!(listed(&Files::default()), "");
        assert_eq!(
            listed(&files(
                &[call("c1", "bash", r#"{"command": "ls"}"#)],
                &Files::default()
            )),
            "",
            "no file tool, no lists"
        );
    }
}
