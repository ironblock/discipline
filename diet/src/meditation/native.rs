//! What a native harness carries across a compaction (#936): under
//! `meditation_carry = "native_summary"`, the refill holds a summary the
//! model writes with the vote's compaction prompt, and what Qwen Code puts
//! beside it, in place of the working-memory render.
//!
//! The vote (Pi `v1.3.0`, `OpenCode` 2 `v2.0.32`, Qwen Code `v0.25.3`; Qwen
//! Code breaks a tie) is in `docs/harness-baseline.md`. The three differ on
//! the prompt, how the history is shown, the output cap, tools and what rides
//! beside the summary, so each is Qwen Code's:
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
//! - **Beside it** (`postCompactAttachments.ts:800-882`): the model's
//!   acknowledgement, verbatim; then, when there are any, one user message
//!   holding the files the model's file tools last touched (at most 5,
//!   each embedded whole up to 5,000 tokens, within a 50,000-token budget,
//!   else listed by path) and the last 3 images. Qwen Code's plan-mode
//!   reminder and running-subagent list are not carried: this harness has
//!   neither.

use std::path::Path;

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

/// The model's acknowledgement after the summary
/// (`qc:packages/core/src/services/postCompactAttachments.ts:857`).
pub const ACK: &str = "Got it. Thanks for the additional context!";

/// The summary's output cap (`chatCompressionService.ts:58`).
pub const OUTPUT_CAP: u64 = 20_000;

/// What the cap leaves of the window below the prompt (`:69`).
const OUTPUT_MARGIN: u64 = 1_024;

/// The files restored at most (`postCompactAttachments.ts:27`).
const MAX_FILES: usize = 5;

/// The images restored at most (`:60`).
const MAX_IMAGES: usize = 3;

/// Characters a token stands for (`compactionInputSlimming.ts:31`).
const CHARS_PER_TOKEN: usize = 4;

/// The tokens one file may be embedded in (`postCompactAttachments.ts:58`).
const MAX_TOKENS_PER_FILE: usize = 5_000;

/// The tokens every embedded file shares (`:59`).
const TOKEN_BUDGET: usize = 50_000;

/// What a native summary's refill carries (#936): the summary, its analysis
/// stripped, and the restored files and images to follow the
/// acknowledgement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// The summary, never empty.
    pub text: String,
    /// The user message of restored files and images, when there are any.
    pub restored: Option<Message>,
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

/// What the model's file tools last touched in `history`, newest first, the
/// last call of a step counted first, each once, at most [`MAX_FILES`]: a
/// `read`, `write` or `edit` whose result did not fail, its path inside
/// `worktree` (`postCompactAttachments.ts:96-160,815-817`).
fn touched(history: &[Message], worktree: &Path) -> Vec<String> {
    let failed: std::collections::BTreeSet<&str> = history
        .iter()
        .filter(|message| message.role == Role::Tool && message.tool_error)
        .filter_map(|message| message.tool_call_id.as_deref())
        .collect();
    let mut seen = Vec::new();
    for message in history.iter().rev().filter(|m| m.role == Role::Assistant) {
        for call in message.tool_calls.iter().rev() {
            let key = match call.name.as_str() {
                crate::drive::standard::READ | crate::drive::standard::WRITE => "path",
                crate::drive::standard::EDIT => "file_path",
                _ => continue,
            };
            if failed.contains(call.id.as_str()) {
                continue;
            }
            let Some(path) = serde_json::from_str::<serde_json::Value>(&call.arguments)
                .ok()
                .and_then(|arguments| arguments.get(key)?.as_str().map(str::to_owned))
            else {
                continue;
            };
            let inside = worktree.join(&path).starts_with(worktree)
                && !path.split('/').any(|part| part == "..");
            if !inside || seen.contains(&path) {
                continue;
            }
            seen.push(path);
            if seen.len() >= MAX_FILES {
                return seen;
            }
        }
    }
    seen
}

/// How Qwen Code restores a file (`postCompactAttachments.ts:299-371`).
enum Restore {
    /// Embedded whole: its text.
    Embed(String),
    /// Listed by path: too large to embed.
    Reference,
    /// Left out: missing, or binary.
    Skip,
}

/// How the file at `path` is restored.
fn sized(path: &Path) -> Restore {
    let max_chars = MAX_TOKENS_PER_FILE * CHARS_PER_TOKEN;
    let Ok(metadata) = std::fs::metadata(path) else {
        return Restore::Skip;
    };
    if metadata.len() > (max_chars * 4) as u64 {
        return Restore::Reference;
    }
    let Ok(bytes) = std::fs::read(path) else {
        return Restore::Skip;
    };
    let sample = &bytes[..bytes.len().min(512)];
    let unprintable = sample
        .iter()
        .filter(|byte| {
            !((0x20..=0x7e).contains(*byte)
                || matches!(**byte, 0x09 | 0x0a | 0x0d)
                || **byte >= 0x80)
        })
        .count();
    if !sample.is_empty() && unprintable * 10 > sample.len() * 3 {
        return Restore::Skip;
    }
    let text = String::from_utf8_lossy(&bytes).into_owned();
    if text.chars().count() <= max_chars {
        Restore::Embed(text)
    } else {
        Restore::Reference
    }
}

/// The longest run of backticks in `text`.
fn longest_backticks(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

/// The user message Qwen Code puts after the acknowledgement: the files
/// `history`'s file tools last touched, read from `worktree` now (none
/// without one: a session that runs no commands has no file tools), and its
/// last images; `None` when there are neither
/// (`postCompactAttachments.ts:385-488,837-845`).
#[must_use]
pub fn restored(history: &[Message], worktree: Option<&Path>) -> Option<Message> {
    let mut references = Vec::new();
    let mut embeds = Vec::new();
    let mut used = 0;
    let files = worktree.map(|tree| (tree, touched(history, tree)));
    for (tree, path) in files
        .iter()
        .flat_map(|(tree, paths)| paths.iter().map(move |path| (*tree, path.clone())))
    {
        let worktree = tree;
        match sized(&worktree.join(&path)) {
            Restore::Skip => {}
            Restore::Reference => references.push(path),
            Restore::Embed(content) => {
                let chars = content.chars().count();
                if used + chars > TOKEN_BUDGET * CHARS_PER_TOKEN {
                    references.push(path);
                } else {
                    used += chars;
                    embeds.push((path, content));
                }
            }
        }
    }
    let mut parts = Vec::new();
    if !references.is_empty() {
        let mut lines = vec![
            "The following files were recently accessed before context was compacted. They are \
             listed as reference only because they are large. Use `read_file` to view current \
             content for any file you need:"
                .to_owned(),
            String::new(),
        ];
        lines.extend(references.iter().map(|path| format!("- {}", shown(path))));
        parts.push(lines.join("\n"));
    }
    for (path, content) in &embeds {
        let fence = "`".repeat((longest_backticks(content) + 1).max(3));
        parts.push(format!(
            "Recently accessed file (full current content embedded):\n\n## {}\n\n{fence}\n{content}\n{fence}",
            shown(path)
        ));
    }
    let mut images = Vec::new();
    let mut sources = Vec::new();
    'outer: for (at, message) in history.iter().enumerate().rev() {
        if message.images.is_empty() {
            continue;
        }
        let source = at
            .checked_sub(1)
            .and_then(|before| history.get(before))
            .filter(|before| before.role == Role::Assistant)
            .and_then(|before| before.tool_calls.first());
        for image in message.images.iter().rev() {
            images.insert(0, image.clone());
            sources.insert(
                0,
                source.map_or_else(
                    || format!("- turn {at}: user-provided image"),
                    |call| {
                        let arguments = serde_json::from_str::<serde_json::Value>(&call.arguments)
                            .map_or_else(|_| "{}".to_owned(), |value| value.to_string());
                        format!("- turn {at}: {} args={arguments}", call.name)
                    },
                ),
            );
            if images.len() >= MAX_IMAGES {
                break 'outer;
            }
        }
    }
    if !images.is_empty() {
        let mut lines = vec![
            "Recent visual snapshots preserved from before context was compacted (most recent \
             last). Each image corresponds to a tool result or user-pasted image earlier in the \
             conversation:"
                .to_owned(),
            String::new(),
        ];
        lines.extend(sources);
        parts.push(lines.join("\n"));
    }
    if parts.is_empty() {
        return None;
    }
    let mut message = Message::new(Role::User, parts.join("\n\n"));
    message.images = images;
    Some(message)
}

/// `path` with the characters that would break its line taken out
/// (`postCompactAttachments.ts:55-57`).
fn shown(path: &str) -> String {
    path.chars()
        .filter(|c| !matches!(c, '\r' | '\n' | '\t'))
        .collect()
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

    /// #936: what Qwen Code restores beside the summary -- the files the
    /// model's file tools last touched, newest first, each once, small ones
    /// embedded, a missing one left out; and the last images, with where
    /// each came from.
    #[test]
    fn the_files_a_file_tool_touched_are_restored_and_images_kept() {
        let tree = std::env::temp_dir().join(format!(
            "diet-native-restored-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |since| since.as_nanos())
        ));
        std::fs::create_dir_all(&tree).expect("a tree");
        std::fs::write(tree.join("a.rs"), "fn a() {}\n").expect("a file");
        std::fs::write(tree.join("b.md"), "notes with ``` a fence\n").expect("a file");
        let call = |id: &str, name: &str, arguments: &str| {
            let mut message = Message::new(Role::Assistant, String::new());
            message.tool_calls.push(crate::client::shape::ToolCall {
                id: id.to_owned(),
                name: name.to_owned(),
                arguments: arguments.to_owned(),
            });
            message
        };
        let history = vec![
            Message::new(Role::User, "go".to_owned()),
            call("c1", "read", r#"{"path": "a.rs"}"#),
            Message::new(Role::Tool, "fn a() {}".to_owned()),
            call(
                "c2",
                "edit",
                r#"{"file_path": "b.md", "old_string": "x", "new_string": "y"}"#,
            ),
            Message::new(Role::Tool, "ok".to_owned()),
            call("c3", "read", r#"{"path": "gone.txt"}"#),
            Message::new(Role::Tool, "missing".to_owned()),
            call("c4", "bash", r#"{"command": "cat a.rs"}"#),
            Message::new(Role::Tool, "fn a() {}".to_owned()),
        ];
        assert_eq!(touched(&history, &tree), ["gone.txt", "b.md", "a.rs"]);
        let message = restored(&history, Some(&tree)).expect("files to restore");
        assert_eq!(message.role, Role::User);
        let b = message.content.find("## b.md").expect("b.md embedded");
        let a = message.content.find("## a.rs").expect("a.rs embedded");
        assert!(b < a, "newest first: {}", message.content);
        assert!(
            message
                .content
                .contains("````\nnotes with ``` a fence\n\n````"),
            "{}",
            message.content
        );
        assert!(
            !message.content.contains("gone.txt"),
            "a missing file is left out"
        );
        assert!(message.images.is_empty());
        assert!(
            restored(&history[..1], Some(&tree)).is_none(),
            "nothing to restore"
        );
        assert!(restored(&history, None).is_none(), "no worktree, no files");
        let _ = std::fs::remove_dir_all(&tree);
    }
}
