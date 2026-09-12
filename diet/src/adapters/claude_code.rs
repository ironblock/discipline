//! The Claude Code session log, read into this crate's events.
//!
//! Claude Code writes one JSON object per line to
//! `~/.claude/projects/<slug>/<session>.jsonl`. The first log this adapter was
//! written against is this repository's own: 5,577 rows, nine top-level
//! kinds, of which this schema has a word for two.
//!
//! # The mapping, and the two places it would be easy to lie
//!
//! **A `user` row is not a turn.** Of 1,080 `user` rows in that log, 1,019
//! carry nothing but `tool_result` blocks -- the harness handing a tool's
//! output back to the model, which is not a person saying anything. Mapping
//! every `user` row to a turn would have invented about a thousand turns that
//! never happened, in a record whose whole purpose is that its counts are
//! true. So a `user` row opens a turn only when it carries text a person
//! wrote; otherwise its `tool_result` blocks are joined to the call they
//! answer.
//!
//! **`input_tokens` is not the prefill.** A typical assistant row in that log
//! reads `input_tokens: 2` beside `cache_read_input_tokens: 38639` and
//! `cache_creation_input_tokens: 22468`. Recording the first as the turn's
//! prefill would say a turn prefilled two tokens when it prefilled sixty-one
//! thousand: cached tokens are still tokens that were fed to the model, and
//! the cache is an accounting detail of what they cost, not of whether they
//! were sent. The prefill here is the sum of all three.
//!
//! # What it cannot carry
//!
//! `thinking` blocks have no event kind in this schema, so they are counted
//! as dropped content rather than quietly discarded -- 552 of them in that
//! same log. Six of the nine row kinds (`attachment`, `last-prompt`,
//! `atis-latch`, `custom-title`, `mode`, `queue-operation`) have no mapping
//! either, and are counted as unmapped rows. Together that is 47% of the
//! file, which is the number this adapter exists to make visible rather than
//! to improve.

use std::collections::BTreeMap;

use super::{Adapted, Adapter, Census, Drift};
use crate::formats::record::{Count, Event, json::Value};

/// The Claude Code session-log adapter.
#[derive(Debug, Clone, Copy, Default)]
pub struct ClaudeCode;

/// The row kinds this adapter has a mapping for.
///
/// Kept beside [`Row`] rather than derived from it because `Adapter::maps`
/// hands back a `&'static [&'static str]` and a const cannot walk an enum.
/// `the_two_foreign_vocabularies_have_one_definition_each` holds the two in
/// step, which is the weaker guarantee and is named as such.
const MAPS: &[&str] = &["user", "assistant"];

/// A row kind this adapter maps.
///
/// A type rather than a string, because `diet/src` admits no match arm on a
/// string literal and the reason is this exact situation: the foreign
/// vocabulary is matched in several places, and a sixth kind added as an arm
/// somewhere would compile while the others quietly stopped covering it. A
/// tag becomes a `Row` in ONE place -- [`Row::from_tag`], walking
/// [`Row::ALL`] -- and every use after that is exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    /// A person's turn, or the harness handing back a tool's output.
    User,
    /// What the model said, called, or thought.
    Assistant,
}

impl Row {
    /// Every row kind this adapter maps.
    const ALL: &'static [Self] = &[Self::User, Self::Assistant];

    /// The tag Claude Code writes for it.
    const fn tag(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
        }
    }

    /// The one place a foreign tag becomes a row kind.
    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|row| row.tag() == tag)
    }
}

/// A content block this adapter reads.
///
/// Same reason as [`Row`], and the same shape. A block type that is not one
/// of these is counted as dropped content under its own foreign name, which
/// is why `from_tag` returns an `Option` rather than a fallback variant: "a
/// block this adapter has no word for" is a fact about the census, not a kind
/// of block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    /// Text the model wrote, or a person did.
    Text,
    /// A call the model made.
    ToolUse,
    /// The answer to one.
    ToolResult,
}

impl Block {
    /// Every block type this adapter reads.
    const ALL: &'static [Self] = &[Self::Text, Self::ToolUse, Self::ToolResult];

    /// The tag Claude Code writes for it.
    const fn tag(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::ToolUse => "tool_use",
            Self::ToolResult => "tool_result",
        }
    }

    /// The one place a foreign tag becomes a block type.
    fn from_tag(tag: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|block| block.tag() == tag)
    }
}

/// Claude Code's tool names, in the vocabulary the capture lanes speak.
///
/// **This table is why the adapter exists.** `diet::capture::mechanical`
/// knows `bash`, `read_file` and `edit_file`; Claude Code writes `Bash`,
/// `Read`, `Edit` and `Write`. Pointed at a real log without this, the lane
/// matched not one of 1,021 tool calls and derived nothing at all -- no
/// working directory, no file touched, no failed `cd` -- while every count in
/// the census still read as a success. A translation nobody wrote is a lane
/// that silently observes nothing, which is this issue's own failure mode
/// wearing the library's clothes.
///
/// Only names whose CONTRACT matches are translated. `Grep`, `Glob`, `Task`
/// and every `mcp__*` tool are left as they are: the lane records them as
/// commands it does not interpret, which is true, where guessing at their
/// arguments would be a claim about a contract nobody wrote down.
const TOOL_NAMES: &[(&str, &str)] = &[
    ("Bash", "bash"),
    ("Read", "read_file"),
    ("Edit", "edit_file"),
    ("Write", "write_file"),
];

/// Claude Code's argument for the file a path tool acts on.
const FOREIGN_PATH: &str = "file_path";

/// The argument name the capture lane reads a path from.
const NATIVE_PATH: &str = "path";

/// `name` in the capture lanes' vocabulary, and whether it was translated.
fn native_tool(name: &str) -> (&str, bool) {
    TOOL_NAMES
        .iter()
        .find(|(foreign, _)| *foreign == name)
        .map_or((name, false), |(_, native)| (*native, true))
}

impl Adapter for ClaudeCode {
    fn name(&self) -> &'static str {
        "claude-code"
    }

    fn maps(&self) -> &'static [&'static str] {
        MAPS
    }

    fn adapt(&self, log: &str) -> Result<Adapted, Drift> {
        let rows = parse(log)?;
        let mut run = Run::new(self.name());
        for (at_row, row) in rows.iter().enumerate() {
            run.row(at_row + 1, row, rows.get(at_row + 1..).unwrap_or(&[]))?;
        }
        Ok(Adapted {
            events: run.events,
            census: run.census,
        })
    }
}

/// Every non-blank line, as JSON, or the first line that is not.
fn parse(log: &str) -> Result<Vec<serde_json::Value>, Drift> {
    let mut rows = Vec::new();
    for (at, line) in log.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let row: serde_json::Value = serde_json::from_str(line).map_err(|why| Drift::NotJson {
            at_row: at + 1,
            why: why.to_string(),
        })?;
        if !row.is_object() {
            return Err(Drift::NotAnObject { at_row: at + 1 });
        }
        rows.push(row);
    }
    Ok(rows)
}

/// The walk over one log.
struct Run {
    events: Vec<Event>,
    census: Census,
    /// The turn a person most recently opened. Zero means none yet.
    turn: u32,
    /// `tool_use_id` -> where its [`Event::ToolCall`] sits in `events`, so the
    /// `tool_result` that answers it can be joined to the call it answers
    /// rather than filed as a row of its own.
    awaiting: BTreeMap<String, usize>,
}

impl Run {
    fn new(adapter: &str) -> Self {
        Self {
            events: Vec::new(),
            census: Census {
                adapter: adapter.to_owned(),
                ..Census::default()
            },
            turn: 0,
            awaiting: BTreeMap::new(),
        }
    }

    /// One row. `rest` is what follows it, for the prefill lookahead.
    fn row(
        &mut self,
        at_row: usize,
        row: &serde_json::Value,
        rest: &[serde_json::Value],
    ) -> Result<(), Drift> {
        let Some(kind) = row.get("type").and_then(serde_json::Value::as_str) else {
            return Err(Drift::NoKind { at_row });
        };
        // A kind this adapter has no word for is NEWS, not a failure: the
        // census carries it and the walk goes on.
        let Some(mapped) = Row::from_tag(kind) else {
            self.census.unmapped_one(kind);
            return Ok(());
        };
        self.census.mapped_one(kind);
        match mapped {
            Row::User => self.user(at_row, row, rest),
            Row::Assistant => self.assistant(at_row, row),
        }
    }

    /// A `user` row: a person's turn, or a tool's answer, never both invented.
    fn user(
        &mut self,
        at_row: usize,
        row: &serde_json::Value,
        rest: &[serde_json::Value],
    ) -> Result<(), Drift> {
        let content = content_of(at_row, "user", row)?;
        let mut said = String::new();
        match content {
            serde_json::Value::String(text) => said.push_str(text),
            serde_json::Value::Array(blocks) => {
                for block in blocks {
                    let tag = block_type(block);
                    match tag.and_then(Block::from_tag) {
                        Some(Block::Text) => {
                            if let Some(text) =
                                block.get("text").and_then(serde_json::Value::as_str)
                            {
                                said.push_str(text);
                            }
                        }
                        Some(Block::ToolResult) => self.tool_result(block),
                        // A call inside a `user` row. The mapping reads calls
                        // from `assistant` rows only, so this is content with
                        // no home rather than a call -- counted under its own
                        // name, like any other.
                        Some(known @ Block::ToolUse) => {
                            self.census.dropped_one(&format!("user/{}", known.tag()));
                        }
                        None => match tag {
                            Some(other) => self.census.dropped_one(&format!("user/{other}")),
                            None => self.census.dropped_one("user/<untyped block>"),
                        },
                    }
                }
            }
            _ => {
                return Err(Drift::MissingField {
                    at_row,
                    kind: "user".to_owned(),
                    field: "message.content (a string or a list of blocks)".to_owned(),
                });
            }
        }

        if said.trim().is_empty() {
            // Not a turn. The row carried a tool's answer, which has been
            // joined to the call above, or content with no home, which has
            // been counted. Inventing a turn here is the thousand-turn lie.
            return Ok(());
        }

        self.turn += 1;
        self.events.push(Event::Turn {
            index: self.turn,
            prefill_tokens: prefill_after(rest),
        });
        self.events.push(Event::Request {
            id: format!("u/{}", self.turn),
            lane: "main".to_owned(),
            retry_of: None,
            text: Some(said),
        });
        Ok(())
    }

    /// An `assistant` row: what it said, what it called, what it thought.
    fn assistant(&mut self, at_row: usize, row: &serde_json::Value) -> Result<(), Drift> {
        let content = content_of(at_row, "assistant", row)?;
        let mut spoke = String::new();
        let blocks = match content {
            serde_json::Value::Array(blocks) => blocks.clone(),
            serde_json::Value::String(text) => {
                spoke.push_str(text);
                Vec::new()
            }
            _ => {
                return Err(Drift::MissingField {
                    at_row,
                    kind: "assistant".to_owned(),
                    field: "message.content (a string or a list of blocks)".to_owned(),
                });
            }
        };

        for block in &blocks {
            let tag = block_type(block);
            match tag.and_then(Block::from_tag) {
                Some(Block::Text) => {
                    if let Some(text) = block.get("text").and_then(serde_json::Value::as_str) {
                        spoke.push_str(text);
                    }
                }
                Some(Block::ToolUse) => self.tool_use(at_row, block)?,
                // An answer in the row that asks. The mapping joins answers
                // from `user` rows, so this is content with no home.
                Some(known @ Block::ToolResult) => {
                    self.census
                        .dropped_one(&format!("assistant/{}", known.tag()));
                }
                // Reasoning has no event kind here. Counted, not discarded.
                None => match tag {
                    Some(other) => self.census.dropped_one(&format!("assistant/{other}")),
                    None => self.census.dropped_one("assistant/<untyped block>"),
                },
            }
        }

        if spoke.trim().is_empty() {
            return Ok(());
        }
        // A response names its token count, so the count has to be there. It
        // is the one field of the mapping that cannot be absent without the
        // event becoming a guess.
        let Some(tokens) = row
            .get("message")
            .and_then(|m| m.get("usage"))
            .and_then(|u| u.get("output_tokens"))
            .and_then(serde_json::Value::as_u64)
        else {
            return Err(Drift::MissingField {
                at_row,
                kind: "assistant".to_owned(),
                field: "message.usage.output_tokens".to_owned(),
            });
        };
        let to_request = format!("u/{}", self.turn.max(1));
        self.events.push(Event::Response {
            id: format!("a/{}", self.events.len()),
            to_request,
            output_tokens: Count::new(tokens).unwrap_or_default(),
            text: Some(spoke),
        });
        Ok(())
    }

    /// A `tool_use` block becomes a call awaiting its answer.
    fn tool_use(&mut self, at_row: usize, block: &serde_json::Value) -> Result<(), Drift> {
        if self.turn == 0 {
            // Nothing to attach it to: the record refuses a row naming a turn
            // that never happened, and inventing turn one would be the same
            // fabrication in a different field.
            self.census
                .dropped_one("assistant/tool_use before any turn");
            return Ok(());
        }
        let Some(id) = block.get("id").and_then(serde_json::Value::as_str) else {
            return Err(Drift::MissingField {
                at_row,
                kind: "assistant".to_owned(),
                field: "content[].tool_use.id".to_owned(),
            });
        };
        let Some(tool) = block.get("name").and_then(serde_json::Value::as_str) else {
            return Err(Drift::MissingField {
                at_row,
                kind: "assistant".to_owned(),
                field: "content[].tool_use.name".to_owned(),
            });
        };
        let (native, translated) = native_tool(tool);
        if translated {
            self.census.translated_one(tool, native);
        }
        let args = match block.get("input") {
            Some(serde_json::Value::Object(input)) => {
                let mut args = BTreeMap::new();
                for (key, raw) in input {
                    // The lane reads a path from `path`; Claude Code writes it
                    // as `file_path`. Renamed only for the tools whose
                    // contract this adapter claims to know, so an unknown
                    // tool's `file_path` is carried across untouched rather
                    // than reinterpreted as a contract nobody stated.
                    let key = if translated && key == FOREIGN_PATH {
                        NATIVE_PATH.to_owned()
                    } else {
                        key.clone()
                    };
                    args.insert(key, value_of(at_row, "input", raw)?);
                }
                Some(args)
            }
            _ => None,
        };
        self.awaiting.insert(id.to_owned(), self.events.len());
        self.events.push(Event::ToolCall {
            id: id.to_owned(),
            at_turn: self.turn,
            tool: native.to_owned(),
            args,
            exit: None,
            output: None,
        });
        Ok(())
    }

    /// A `tool_result` block answers the call it names.
    fn tool_result(&mut self, block: &serde_json::Value) {
        let Some(id) = block
            .get("tool_use_id")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
        else {
            self.census
                .dropped_one("user/tool_result without a tool_use_id");
            return;
        };
        let Some(at) = self.awaiting.remove(&id) else {
            // An answer to a call this log never showed. Counted rather than
            // attached to whichever call happened to be last.
            self.census
                .dropped_one("user/tool_result answering no known call");
            return;
        };
        let Some(Event::ToolCall { exit, output, .. }) = self.events.get_mut(at) else {
            self.census
                .dropped_one("user/tool_result answering no known call");
            return;
        };
        // `is_error` is a boolean, not an exit code. True becomes 1 because
        // the record's field is an exit status and one is the honest
        // stand-in; ABSENT stays absent rather than becoming zero, because
        // "the harness did not say" is not the same as "it succeeded".
        *exit = block
            .get("is_error")
            .and_then(serde_json::Value::as_bool)
            .map(i64::from);
        *output = Some(flatten(block.get("content")));
    }
}

/// The `message.content` of a row, or the drift of a mapping that lost it.
fn content_of<'a>(
    at_row: usize,
    kind: &str,
    row: &'a serde_json::Value,
) -> Result<&'a serde_json::Value, Drift> {
    let Some(message) = row.get("message") else {
        return Err(Drift::MissingField {
            at_row,
            kind: kind.to_owned(),
            field: "message".to_owned(),
        });
    };
    message.get("content").ok_or_else(|| Drift::MissingField {
        at_row,
        kind: kind.to_owned(),
        field: "message.content".to_owned(),
    })
}

/// A content block's `type`, if it has one.
fn block_type(block: &serde_json::Value) -> Option<&str> {
    block.get("type").and_then(serde_json::Value::as_str)
}

/// The prefill of the next assistant row, or zero where none follows.
///
/// All three components, because all three were fed to the model. See this
/// module's header for why `input_tokens` alone is a lie.
fn prefill_after(rest: &[serde_json::Value]) -> Count {
    for row in rest {
        if row.get("type").and_then(serde_json::Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = row.get("message").and_then(|m| m.get("usage")) else {
            return Count::default();
        };
        let sum: u64 = [
            "input_tokens",
            "cache_creation_input_tokens",
            "cache_read_input_tokens",
        ]
        .iter()
        .filter_map(|key| usage.get(*key).and_then(serde_json::Value::as_u64))
        .sum();
        return Count::new(sum).unwrap_or_default();
    }
    Count::default()
}

/// A tool result's content as text, however the harness shaped it.
fn flatten(content: Option<&serde_json::Value>) -> String {
    match content {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(serde_json::Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| match block {
                serde_json::Value::String(text) => Some(text.clone()),
                block => block
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
            })
            .collect::<String>(),
        _ => String::new(),
    }
}

/// A foreign value in the record's value space, or a refusal.
///
/// The record admits no `null` and no binary floats. A tool argument that
/// cannot be spelled is refused by name rather than coerced into a string --
/// a number rendered as text is a lie about its type, and this is the layer
/// where that lie would be introduced.
fn value_of(at_row: usize, field: &str, raw: &serde_json::Value) -> Result<Value, Drift> {
    match raw {
        serde_json::Value::Null => Err(Drift::Unrepresentable {
            at_row,
            field: field.to_owned(),
            why: "null".to_owned(),
        }),
        serde_json::Value::Bool(b) => Ok(Value::Boolean(*b)),
        serde_json::Value::String(s) => Ok(Value::String(s.clone())),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                return Ok(Value::Integer(i));
            }
            crate::formats::record::json::Decimal::new(&n.to_string())
                .map(Value::Decimal)
                .ok_or_else(|| Drift::Unrepresentable {
                    at_row,
                    field: field.to_owned(),
                    why: format!("the number {n}"),
                })
        }
        serde_json::Value::Array(items) => items
            .iter()
            .map(|item| value_of(at_row, field, item))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        serde_json::Value::Object(entries) => entries
            .iter()
            .map(|(key, item)| value_of(at_row, field, item).map(|v| (key.clone(), v)))
            .collect::<Result<BTreeMap<_, _>, _>>()
            .map(Value::Object),
    }
}

#[cfg(test)]
mod tests {

    use super::{Block, ClaudeCode, MAPS, Row, native_tool};
    use crate::adapters::{Adapter, Drift};
    use crate::formats::record::Event;

    /// One assistant row, with `content` and `usage` written in full.
    fn assistant(content: &str, output_tokens: u64, prefill: [u64; 3]) -> String {
        format!(
            "{{\"type\":\"assistant\",\"message\":{{\"role\":\"assistant\",\"content\":{content},\
             \"usage\":{{\"input_tokens\":{},\"cache_creation_input_tokens\":{},\
             \"cache_read_input_tokens\":{},\"output_tokens\":{output_tokens}}}}}}}",
            prefill[0], prefill[1], prefill[2]
        )
    }

    /// THE THOUSAND-TURN LIE, refused.
    ///
    /// In the log this adapter was written against, 1,019 of 1,080 `user`
    /// rows carry nothing but a `tool_result` -- the harness handing output
    /// back to the model. Counting those as turns would put a thousand turns
    /// in a record whose entire value is that its counts are true.
    #[test]
    fn a_user_row_carrying_only_a_tool_result_is_not_a_turn() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"do the thing\"}}",
            &assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\
                 \"input\":{\"command\":\"ls\"}}]",
                3,
                [1, 0, 0],
            ),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\
             [{\"type\":\"tool_result\",\"tool_use_id\":\"t1\",\"content\":\"a b\"}]}}",
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log).expect("the fixture adapts");
        let turns = adapted
            .events
            .iter()
            .filter(|e| matches!(e, Event::Turn { .. }))
            .count();
        assert_eq!(
            turns, 1,
            "one person said one thing; the other user row was a tool answering"
        );
        // Both user rows are still MAPPED -- the second one's content went
        // somewhere, onto the call it answers. Mapped and turn are different
        // claims and the census makes only the first.
        assert_eq!(adapted.census.mapped.get("user").copied(), Some(2));
    }

    /// A tool's answer lands on the call it names, not on whichever was last.
    #[test]
    fn a_tool_result_joins_the_call_it_names() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}",
            &assistant(
                "[{\"type\":\"tool_use\",\"id\":\"first\",\"name\":\"Bash\",\
                  \"input\":{\"command\":\"one\"}},\
                  {\"type\":\"tool_use\",\"id\":\"second\",\"name\":\"Bash\",\
                  \"input\":{\"command\":\"two\"}}]",
                3,
                [1, 0, 0],
            ),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\
             [{\"type\":\"tool_result\",\"tool_use_id\":\"second\",\"content\":\"B\",\
             \"is_error\":true}]}}",
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log).expect("the fixture adapts");
        for event in &adapted.events {
            let Event::ToolCall {
                id, output, exit, ..
            } = event
            else {
                continue;
            };
            // `if` rather than a `match` on the id, because `diet/src` admits
            // no match arm on a string literal and a call id is exactly the
            // kind of loose vocabulary that rule is about.
            if id == "second" {
                assert_eq!(output.as_deref(), Some("B"));
                assert_eq!(*exit, Some(1), "`is_error` true is a failure");
            } else if id == "first" {
                assert_eq!(output.as_deref(), None, "nothing answered it");
                assert_eq!(
                    *exit, None,
                    "absent is not zero: the harness did not say it succeeded"
                );
            } else {
                panic!("unexpected call {id}");
            }
        }
    }

    /// `input_tokens` alone is a lie by two orders of magnitude.
    #[test]
    fn the_prefill_is_every_token_that_was_fed_in_not_the_uncached_ones() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}",
            &assistant(
                "[{\"type\":\"text\",\"text\":\"ok\"}]",
                7,
                [2, 22468, 38639],
            ),
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log).expect("the fixture adapts");
        let Some(Event::Turn { prefill_tokens, .. }) = adapted
            .events
            .iter()
            .find(|e| matches!(e, Event::Turn { .. }))
        else {
            panic!("the turn is there")
        };
        assert_eq!(
            prefill_tokens.get(),
            2 + 22468 + 38639,
            "cached tokens were still fed to the model; `input_tokens` alone would \
             record a prefill of 2 for a turn that prefilled sixty-one thousand"
        );
    }

    /// A kind the adapter never heard of is news, and the walk goes on.
    #[test]
    fn a_kind_this_adapter_does_not_map_is_counted_rather_than_dropped() {
        let log = concat!(
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}\n",
            "{\"type\":\"a-kind-nobody-has-seen\",\"payload\":\"x\"}"
        );
        let adapted = ClaudeCode
            .adapt(log)
            .expect("an unknown kind is not a refusal");
        assert_eq!(
            adapted
                .census
                .unmapped
                .get("a-kind-nobody-has-seen")
                .copied(),
            Some(1),
            "silence is the failure this issue names"
        );
        assert_eq!(
            adapted.census.rows(),
            2,
            "every row lands in exactly one bucket"
        );
    }

    /// A field that moved is a refusal, because the alternative is reading
    /// whatever happened to be beside it.
    #[test]
    fn a_renamed_field_on_a_mapped_kind_is_refused_rather_than_guessed_at() {
        let log = "{\"type\":\"user\",\"msg\":{\"role\":\"user\",\"content\":\"go\"}}";
        let refused = ClaudeCode.adapt(log).expect_err("the format moved");
        assert!(
            matches!(&refused, Drift::MissingField { kind, field, at_row }
                if kind == "user" && field == "message" && *at_row == 1),
            "{refused}"
        );
    }

    /// The translation that made the capture lane see anything at all.
    #[test]
    fn claude_codes_tool_names_are_translated_into_the_lanes_vocabulary() {
        // Checked against the lane's own table rather than against a copy of
        // it here: a rename on either side that broke the pairing would make
        // this test the place it shows up.
        for (foreign, native) in super::TOOL_NAMES {
            assert_eq!(native_tool(foreign), (*native, true));
            assert!(
                crate::capture::mechanical::tool_kind(native).is_some(),
                "`{native}` is not a tool the mechanical lane knows, so translating \
                 `{foreign}` into it would still derive nothing"
            );
        }
        assert_eq!(
            native_tool("Grep"),
            ("Grep", false),
            "a tool whose contract this adapter does not claim to know is carried \
             across untouched rather than guessed at"
        );
    }

    /// The rename is declared in the census, because it is an interpretation.
    #[test]
    fn a_translated_tool_name_is_declared_in_the_census() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}",
            &assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Read\",\
                  \"input\":{\"file_path\":\"/srv/x.rs\"}}]",
                3,
                [1, 0, 0],
            ),
        ]
        .join("\n");
        let adapted = ClaudeCode.adapt(&log).expect("the fixture adapts");
        assert_eq!(
            adapted.census.translated.get("Read -> read_file").copied(),
            Some(1)
        );
        // And the path argument is renamed WITH it, or the lane reads a tool
        // it knows and finds no path on it.
        let Some(Event::ToolCall { args, tool, .. }) = adapted
            .events
            .iter()
            .find(|e| matches!(e, Event::ToolCall { .. }))
        else {
            panic!("the call is there")
        };
        assert_eq!(tool, "read_file");
        assert!(
            args.as_ref().is_some_and(|a| a.contains_key("path")),
            "`file_path` is renamed to the `path` the lane reads: {args:?}"
        );
    }

    /// Reasoning has no event kind here, so it is counted rather than lost.
    #[test]
    fn thinking_has_no_home_in_the_schema_and_is_counted_as_dropped() {
        let log = [
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"go\"}}",
            &assistant(
                "[{\"type\":\"thinking\",\"thinking\":\"...\"},\
                  {\"type\":\"text\",\"text\":\"ok\"}]",
                3,
                [1, 0, 0],
            ),
        ]
        .join("\n");
        let adapted = ClaudeCode.adapt(&log).expect("the fixture adapts");
        assert_eq!(
            adapted.census.dropped.get("assistant/thinking").copied(),
            Some(1),
            "a mapped row can still have thrown something away, and the census \
             is where that is said"
        );
    }

    /// `MAPS` and [`Row`] say the same thing, and `Block`'s tags are distinct.
    ///
    /// The weaker half of the vocabulary discipline, named rather than
    /// implied: `Adapter::maps` hands back a `&'static [&'static str]`, which
    /// a const cannot build by walking an enum, so the two definitions are
    /// held in step by this test instead of by the compiler. A kind added to
    /// `Row` and not to `MAPS` would otherwise be mapped while the adapter
    /// declared it was not.
    #[test]
    fn the_two_foreign_vocabularies_have_one_definition_each() {
        let from_type: Vec<&str> = Row::ALL.iter().map(|row| row.tag()).collect();
        assert_eq!(
            MAPS, from_type,
            "the kinds this adapter declares are the kinds it maps"
        );
        for tag in MAPS {
            assert!(
                Row::from_tag(tag).is_some(),
                "`{tag}` is declared and does not parse"
            );
        }

        // Round-tripping is the whole claim of a `from_tag` that walks `ALL`:
        // no two variants answer to one tag, and none answers to none.
        for block in Block::ALL {
            assert_eq!(
                Block::from_tag(block.tag()),
                Some(*block),
                "{block:?} round-trips through its own tag"
            );
        }
        assert_eq!(
            Block::ALL.len(),
            Block::ALL
                .iter()
                .map(|block| block.tag())
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            "and no two block types share a tag"
        );
    }

    /// The corpus is committed, and it is what the tests above are about.
    ///
    /// Table-driven against the DIRECTORY rather than against a list of
    /// `include_str!`s, so a fixture cannot be added without a claim about
    /// what it does. A corpus file nobody reads is a file, not a fixture, and
    /// the first version of this test named three by hand while a fourth sat
    /// beside them unmentioned.
    #[test]
    fn the_committed_corpus_adapts_and_refuses_as_it_says_it_does() {
        /// What one fixture is claimed to do: `(rows, unmapped rows, dropped
        /// content)`, or `None` for a fixture whose whole point is that it is
        /// refused.
        type Claim = Option<(u64, u64, u64)>;
        const EXPECTED: &[(&str, Claim)] = &[
            ("session", Some((9, 3, 1))),
            ("an-unmapped-kind", Some((3, 1, 0))),
            ("a-path-that-resolves-to-nothing", Some((3, 0, 0))),
            ("a-renamed-field", None),
        ];

        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("adapters/fixtures/claude-code");
        let mut on_disk: Vec<String> = std::fs::read_dir(&dir)
            .expect("the committed corpus")
            .filter_map(|entry| {
                let path = entry.ok()?.path();
                (path.extension()? == "jsonl")
                    .then(|| path.file_stem()?.to_str().map(str::to_owned))?
            })
            .collect();
        on_disk.sort();
        let mut claimed: Vec<String> = EXPECTED
            .iter()
            .map(|(name, _)| (*name).to_owned())
            .collect();
        claimed.sort();
        assert_eq!(
            on_disk, claimed,
            "every fixture in the corpus is named here, and nothing here is missing \
             from the corpus"
        );

        for (name, expected) in EXPECTED {
            let log = std::fs::read_to_string(dir.join(format!("{name}.jsonl")))
                .unwrap_or_else(|why| panic!("{name}: {why}"));
            match (ClaudeCode.adapt(&log), expected) {
                (Ok(adapted), Some((rows, unmapped, dropped))) => {
                    assert_eq!(adapted.census.rows(), *rows, "{name}: rows");
                    assert_eq!(
                        adapted.census.unmapped_rows(),
                        *unmapped,
                        "{name}: unmapped rows"
                    );
                    assert_eq!(
                        adapted.census.dropped_content(),
                        *dropped,
                        "{name}: dropped content"
                    );
                }
                (Err(drift), None) => {
                    assert!(
                        matches!(drift, Drift::MissingField { .. }),
                        "{name}: it refuses for the reason it was written for, not \
                         another one: {drift}"
                    );
                }
                (Ok(_), None) => panic!("{name}: it was written to be refused and was not"),
                (Err(drift), Some(_)) => panic!("{name}: it was written to adapt: {drift}"),
            }
        }
    }
}
