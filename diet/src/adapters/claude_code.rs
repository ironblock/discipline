//! The Claude Code session log, read into this crate's events.
//!
//! Claude Code writes one JSON object per line to
//! `~/.claude/projects/<slug>/<session>.jsonl`. The first log this adapter was
//! written against is this repository's own.
//!
//! # Every count below is one measurement, and the file it came from moves
//!
//! That log is the session this adapter was written IN, so it grew while it
//! was being read: the first pass over it saw 5,577 rows and the last saw
//! 6,348. Numbers taken at different moments would quietly disagree with each
//! other, so every figure in this file comes from ONE reading --
//! **6,348 rows, 2026-09-12** -- and is a snapshot rather than a constant.
//! The proportions are what the argument rests on; the absolute counts are
//! there so a reader can check the proportions.
//!
//! At that reading: 9 top-level kinds, of which this schema has a word for
//! two. `assistant` (2,122) and `user` (1,213) map; `attachment` (1,369),
//! `last-prompt` (393), `atis-latch` (364), `custom-title` (362), `mode`
//! (321), `queue-operation` (179) and `system` (25) do not. **3,013 rows --
//! 47% -- have no schema kind**, which is the number this adapter exists to
//! make visible rather than to improve.
//!
//! # The mapping, and the two places it would be easy to lie
//!
//! **A `user` row is not a turn.** Of 1,213 `user` rows, 1,151 -- 95% --
//! carry nothing but `tool_result` blocks: the harness handing a tool's
//! output back to the model, which is not a person saying anything. Mapping
//! every `user` row to a turn would have invented over a thousand turns that
//! never happened, in a record whose whole purpose is that its counts are
//! true. So a `user` row opens a turn only when it carries TEXT; otherwise
//! its `tool_result` blocks are joined to the call they answer.
//!
//! Text, and not "text a person wrote", which is what this said first and is
//! a claim the predicate cannot make. Two of the 62 turns the census counts
//! in the reference log are slash-command envelopes the harness composed
//! (`<command-name>…</command-name>`), and a log written by a subagent
//! carries its parent's prompts in the same position. The row says a person
//! is the author; this adapter reports what the row says and does not
//! second-guess it, and the distance between "the `user` role" and "a human
//! being" is the operator's to judge rather than a heuristic's to erase.
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
//! as dropped content rather than quietly discarded -- 628 of them at that
//! reading. The seven unmapped row kinds are counted as unmapped rows. Both
//! are in the census, and neither is in the record: an adapted record is
//! **lossy by declaration**, a view of a foreign session rather than a
//! transcript of one, and the census is the authority on what was seen and
//! not carried.

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
/// matched not one of 1,153 tool calls and derived nothing at all -- no
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
        for (at, (at_row, row)) in rows.iter().enumerate() {
            run.row(*at_row, row, rows.get(at + 1..).unwrap_or(&[]))?;
        }
        Ok(Adapted {
            events: run.events,
            census: run.census,
        })
    }
}

/// Every non-blank line, as JSON and the FILE line it came from.
///
/// The line number is carried rather than recovered, because it cannot be
/// recovered: blank lines are skipped, so a row's position in this vector is
/// not its position in the file. The first version returned bare values and
/// numbered the refusals by vector index, which meant `NotJson` counted file
/// lines and `MissingField` counted rows -- two numbering systems inside one
/// enum whose whole doc comment is that a refusal says WHERE. One blank line
/// anywhere above the fault was enough to send a reader to the wrong line.
fn parse(log: &str) -> Result<Vec<(usize, serde_json::Value)>, Drift> {
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
        rows.push((at + 1, row));
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
        rest: &[(usize, serde_json::Value)],
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
        rest: &[(usize, serde_json::Value)],
    ) -> Result<(), Drift> {
        let content = content_of(at_row, Row::User.tag(), row)?;
        let mut said = String::new();
        match content {
            serde_json::Value::String(text) => said.push_str(text),
            serde_json::Value::Array(blocks) => {
                for block in blocks {
                    let tag = block_type(block);
                    match tag.and_then(Block::from_tag) {
                        Some(Block::Text) => {
                            // Declared, so read or refused. Shrugging here
                            // drops a person's turn on the floor with a
                            // census that says nothing happened -- which is
                            // the module header's own example of the failure
                            // this adapter exists to refuse.
                            let Some(text) = block.get("text").and_then(serde_json::Value::as_str)
                            else {
                                return Err(Drift::MissingField {
                                    at_row,
                                    kind: Row::User.tag().to_owned(),
                                    field: "content[].text.text".to_owned(),
                                });
                            };
                            said.push_str(text);
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
            //
            // Counted all the same: this is a row whose kind was mapped and
            // which produced no event, and `mapped` alone would report it as
            // carried.
            self.census.no_event_one("user/carried no text of its own");
            return Ok(());
        }

        self.turn += 1;
        let prefill = if let Some(counted) = prefill_after(rest)? {
            counted
        } else {
            // The record's `Turn::prefill_tokens` is a `Count`, not an
            // `Option<Count>`, so there is no way to say "the log did not
            // say". The zero goes in and is declared, which is the only
            // honest option without a change to `diet/formats/`.
            self.census.assumed_one(PREFILL_ASSUMED);
            Count::default()
        };
        self.events.push(Event::Turn {
            index: self.turn,
            prefill_tokens: prefill,
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
                    let Some(text) = block.get("text").and_then(serde_json::Value::as_str) else {
                        return Err(Drift::MissingField {
                            at_row,
                            kind: Row::Assistant.tag().to_owned(),
                            field: "content[].text.text".to_owned(),
                        });
                    };
                    spoke.push_str(text);
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
                kind: Row::Assistant.tag().to_owned(),
                field: "message.usage.output_tokens".to_owned(),
            });
        };

        // EVERY assistant row emits a response, whether or not it said
        // anything in words. This used to return early when there was no
        // text, and a row that made a tool call and spoke not at all is the
        // commonest shape in a real log: 1,824 of 2,173 assistant rows, and
        // with them 80.7% of every token the model generated, left through
        // that early return under a census that read as a clean success.
        // Silence is not absence -- the model ran, the tokens were spent, and
        // `text: None` says exactly that where an omitted event said nothing.
        if self.turn == 0 {
            // Nothing to answer. The record's `Response::to_request` is
            // required and a response naming a request that is not in the
            // stream is a dangling link the record itself would reject; this
            // used to emit `u/1` regardless, inventing turn one in the very
            // field `tool_use` refuses to invent it in sixty lines below.
            self.census
                .no_event_one("assistant/spoke before any turn was opened");
            return Ok(());
        }
        self.events.push(Event::Response {
            id: format!("a/{}", self.events.len()),
            to_request: format!("u/{}", self.turn),
            output_tokens: count_of(at_row, "message.usage.output_tokens", tokens)?,
            text: (!spoke.trim().is_empty()).then_some(spoke),
        });
        Ok(())
    }

    /// A `tool_use` block becomes a call awaiting its answer.
    fn tool_use(&mut self, at_row: usize, block: &serde_json::Value) -> Result<(), Drift> {
        if self.turn == 0 {
            // Nothing to attach it to: the record refuses a row naming a turn
            // that never happened, and inventing turn one would be the same
            // fabrication in a different field.
            // Under `assistant/tool_use`, like any other unhomed block of
            // that type, with the reason in `no_event` where reasons live.
            // Two vocabularies in one map -- `assistant/thinking` beside
            // `assistant/tool_use before any turn` -- gave a reader tallying
            // by block type two keys for one type.
            self.census
                .dropped_one(&format!("assistant/{}", Block::ToolUse.tag()));
            self.census
                .no_event_one("assistant/called a tool before any turn was opened");
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
        // `input` is a field this mapping declares, so it is read or refused.
        // Reading a reshaped one as "no arguments" is the same defect
        // `TOOL_NAMES` exists to prevent, one field across: the call is
        // recognised, the census reads as a success, the translation is even
        // counted, and the capture lane derives nothing from a call it can
        // see. `null` is refused by `value_of` for the same reason.
        let args = match block.get("input") {
            None => {
                return Err(Drift::MissingField {
                    at_row,
                    kind: Row::Assistant.tag().to_owned(),
                    field: "content[].tool_use.input".to_owned(),
                });
            }
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
            Some(other) => {
                return Err(Drift::Unrepresentable {
                    at_row,
                    field: "content[].tool_use.input".to_owned(),
                    why: format!(
                        "it is {}, and a call's arguments are an object",
                        shape_of(other)
                    ),
                });
            }
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
                .dropped_one(&format!("user/{}", Block::ToolResult.tag()));
            self.census
                .no_event_one("user/a tool result naming no call");
            return;
        };
        let Some(at) = self.awaiting.remove(&id) else {
            // An answer to a call this log never showed. Counted rather than
            // attached to whichever call happened to be last.
            self.census
                .dropped_one(&format!("user/{}", Block::ToolResult.tag()));
            self.census
                .no_event_one("user/a tool result answering no known call");
            return;
        };
        let Some(Event::ToolCall { exit, output, .. }) = self.events.get_mut(at) else {
            self.census
                .dropped_one(&format!("user/{}", Block::ToolResult.tag()));
            self.census
                .no_event_one("user/a tool result answering no known call");
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
        // Absent stays absent, for the same reason `is_error` does: "the
        // harness said nothing" and "the tool printed nothing" are different
        // facts about a call, and `Some("")` says the second about both.
        *output = block.get("content").map(|content| flatten(Some(content)));
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

/// What a JSON value is, for a refusal that has to say why it will not fit.
///
/// Named rather than `{:?}`-rendered: a debug rendering of a whole foreign
/// object in an error message is how a log's contents end up in a terminal
/// nobody meant to paste them into.
fn shape_of(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "a boolean",
        serde_json::Value::Number(_) => "a number",
        serde_json::Value::String(_) => "a string",
        serde_json::Value::Array(_) => "a list",
        serde_json::Value::Object(_) => "an object",
    }
}

/// A content block's `type`, if it has one.
fn block_type(block: &serde_json::Value) -> Option<&str> {
    block.get("type").and_then(serde_json::Value::as_str)
}

/// The prefill of the assistant row that answers THIS turn, if one does.
///
/// All three token fields, because all three were fed to the model. See this
/// module's header for why `input_tokens` alone is a lie.
///
/// # Why the scan stops
///
/// It used to run to the next `assistant` row anywhere ahead of it. Two
/// person turns in a row -- a second message queued before the model replied,
/// which happens eight times in the log this was written from -- then gave
/// BOTH turns the same prefill, and `Event::Summary` sums prefill across
/// turns, so the session total counted those tokens twice. The scan now stops
/// at the next row that opens a turn: an assistant row after that answers
/// that turn, not this one.
///
/// `None` means the log affords no answer -- an interrupted session's last
/// turn, or a usage object carrying none of the three keys. The caller turns
/// that into the zero the record gives it no way to avoid, and counts it.
fn prefill_after(rest: &[(usize, serde_json::Value)]) -> Result<Option<Count>, Drift> {
    for (at_row, row) in rest {
        match row.get("type").and_then(serde_json::Value::as_str) {
            Some(kind) if kind == Row::Assistant.tag() => {}
            // A row that opens a turn of its own ends the lookahead: whatever
            // answers it is that turn's prefill and not this one's.
            Some(kind) if kind == Row::User.tag() && opens_a_turn(row) => return Ok(None),
            _ => continue,
        }
        let Some(usage) = row.get("message").and_then(|m| m.get("usage")) else {
            return Ok(None);
        };
        let mut sum = 0_u64;
        let mut said = false;
        for key in PREFILL_KEYS {
            if let Some(part) = usage.get(*key).and_then(serde_json::Value::as_u64) {
                said = true;
                sum = sum
                    .checked_add(part)
                    .ok_or_else(|| Drift::Unrepresentable {
                        at_row: *at_row,
                        field: PREFILL_FIELD.to_owned(),
                        why: "the three counts sum past what a count can hold".to_owned(),
                    })?;
            }
        }
        if !said {
            return Ok(None);
        }
        return count_of(*at_row, PREFILL_FIELD, sum).map(Some);
    }
    Ok(None)
}

/// The three token fields that together are what was fed to the model.
const PREFILL_KEYS: &[&str] = &[
    "input_tokens",
    "cache_creation_input_tokens",
    "cache_read_input_tokens",
];

/// What a refusal calls the field the three counts live in.
const PREFILL_FIELD: &str = "message.usage (the three prefill counts)";

/// What is assumed when the log affords no prefill for a turn.
const PREFILL_ASSUMED: &str = "turn.prefill_tokens = 0 (no assistant row answers this turn)";

/// A count the record can hold, or a refusal naming the one it cannot.
///
/// `Count::new(n).unwrap_or_default()` was here, and it turned a number past
/// the cap into a silent zero -- reinstating, one line at a time, exactly the
/// failure `Count` exists to prevent: a value that reads back different from
/// the one written. A number this record cannot spell is drift, and [`Drift`]
/// already has the word for it.
fn count_of(at_row: usize, field: &str, raw: u64) -> Result<Count, Drift> {
    Count::new(raw).map_err(|_| Drift::Unrepresentable {
        at_row,
        field: field.to_owned(),
        why: format!("the count {raw} is past what the record can hold"),
    })
}

/// Whether a `user` row carries text, and so opens a turn.
///
/// The same predicate the mapping uses, factored out because the prefill
/// lookahead has to agree with it exactly. Two copies that disagreed would
/// give a turn the prefill of a different turn.
fn opens_a_turn(row: &serde_json::Value) -> bool {
    match row
        .get("message")
        .and_then(|message| message.get("content"))
    {
        Some(serde_json::Value::String(text)) => !text.trim().is_empty(),
        Some(serde_json::Value::Array(blocks)) => blocks.iter().any(|block| {
            block_type(block).and_then(Block::from_tag) == Some(Block::Text)
                && block
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|text| !text.trim().is_empty())
        }),
        _ => false,
    }
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

    /// One `user` row carrying text a person wrote.
    fn user_says(text: &str) -> String {
        format!("{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":\"{text}\"}}}}")
    }

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
    /// In the log this adapter was written against, 1,151 of 1,213 `user`
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

    /// THE EIGHTY-PERCENT DROP.
    ///
    /// An assistant row that makes a tool call and says nothing in words is
    /// the commonest shape in a real log -- 1,824 of 2,173 rows -- and it
    /// used to emit no response at all, taking its `output_tokens` with it.
    /// 80.7% of every token the model generated left through that hole under
    /// a census that read as a clean success. Found by a fresh-instance
    /// review measuring the adapter against the log it was written from, not
    /// by any test here, which is why this one exists.
    #[test]
    fn an_assistant_row_that_said_nothing_still_reports_what_it_spent() {
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Read\",\
                 \"input\":{\"file_path\":\"/srv/p/a.rs\"}}]",
                4242,
                [1, 0, 0],
            ),
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log).expect("it adapts");
        let spent: Vec<(u64, Option<&str>)> = adapted
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Response {
                    output_tokens,
                    text,
                    ..
                } => Some((output_tokens.get(), text.as_deref())),
                _ => None,
            })
            .collect();
        assert_eq!(
            spent,
            vec![(4242, None)],
            "the row spent 4,242 tokens saying nothing, and both halves of \
             that are recorded: the count, and `text: None` rather than an \
             omitted event"
        );
        assert_eq!(
            adapted.census.silent_rows(),
            0,
            "and no assistant row is silent: the census would say so if one were"
        );
    }

    /// A response cannot name a request that is not in the stream.
    ///
    /// `to_request` used to be `u/{turn.max(1)}`, which emits `u/1` when no
    /// turn has been opened -- a dangling link the record's own structure
    /// check rejects, invented in the very field `tool_use` refuses to invent
    /// it in sixty lines below. Reachable from any resumed or compacted
    /// session whose first mapped row is an assistant row.
    #[test]
    fn an_assistant_row_before_any_turn_names_no_request_and_is_counted() {
        let log = assistant("[{\"type\":\"text\",\"text\":\"resumed\"}]", 9, [1, 0, 0]);

        let adapted = ClaudeCode.adapt(&log).expect("it adapts");
        assert!(
            !adapted
                .events
                .iter()
                .any(|event| matches!(event, Event::Response { .. })),
            "no response, because there is no request for one to answer"
        );
        assert_eq!(
            adapted.census.no_event["assistant/spoke before any turn was opened"], 1,
            "and the row is counted rather than passed over in silence"
        );
        assert_eq!(
            adapted.census.mapped_rows(),
            1,
            "it was still a kind we map"
        );
    }

    /// Two person turns in a row do not share one prefill.
    ///
    /// The lookahead used to run to the next assistant row wherever it was,
    /// so a message queued before the model replied gave BOTH turns the same
    /// prefill -- and `Event::Summary` sums prefill across turns, so the
    /// session total counted those tokens twice. Eight such pairs exist in
    /// the log this was written from.
    #[test]
    fn two_turns_in_a_row_do_not_both_claim_the_same_prefill() {
        let log = [
            user_says("first"),
            user_says("second, before it answered"),
            assistant("[{\"type\":\"text\",\"text\":\"ok\"}]", 3, [10, 20, 70]),
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log).expect("it adapts");
        let prefills: Vec<u64> = adapted
            .events
            .iter()
            .filter_map(|event| match event {
                Event::Turn { prefill_tokens, .. } => Some(prefill_tokens.get()),
                _ => None,
            })
            .collect();
        assert_eq!(
            prefills,
            vec![0, 100],
            "the assistant row answers the SECOND turn; the first has none, \
             and 100 counted twice would overstate the session by 100"
        );
        assert_eq!(
            adapted.census.assumed["turn.prefill_tokens = 0 (no assistant row answers this turn)"],
            1,
            "and the zero the record forces is declared rather than passed off \
             as a measurement"
        );
    }

    /// A number the record cannot hold is drift, not a zero.
    ///
    /// `Count::new(n).unwrap_or_default()` was here. It turned a count past
    /// the cap into `0` with a success census -- reinstating one line at a
    /// time the exact failure `Count` exists to prevent: a value that reads
    /// back different from the one written.
    #[test]
    fn a_count_past_what_the_record_holds_is_refused_rather_than_zeroed() {
        let huge = u64::MAX;
        let log = [
            user_says("go"),
            assistant("[{\"type\":\"text\",\"text\":\"ok\"}]", huge, [1, 0, 0]),
        ]
        .join("\n");
        assert!(
            matches!(
                ClaudeCode.adapt(&log),
                Err(Drift::Unrepresentable { field, .. }) if field.contains("output_tokens")
            ),
            "an output count past the cap is named, not silently zeroed"
        );

        // And the same for the three prefill counts, which are summed.
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"text\",\"text\":\"ok\"}]",
                1,
                [huge, huge, huge],
            ),
        ]
        .join("\n");
        assert!(
            matches!(ClaudeCode.adapt(&log), Err(Drift::Unrepresentable { .. })),
            "a prefill sum that overflows is a refusal, not a wrap or a panic"
        );
    }

    /// Every field the mapping declares is read or refused -- all of them.
    ///
    /// Three declared fields used to be read with a shrug: a `text` block
    /// with its text renamed dropped a person's turn on the floor, and a
    /// `tool_use` whose `input` was renamed or reshaped became a call with no
    /// arguments -- recognised, counted, translated, and useless to the
    /// capture lane. That is the failure `TOOL_NAMES` exists to prevent,
    /// reproduced one field across.
    #[test]
    fn every_declared_field_is_refused_when_it_moves_not_read_around() {
        let moved: &[(&str, &str)] = &[
            (
                "a user row's text block",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\
                 [{\"type\":\"text\",\"body\":\"please fix the parser\"}]}}",
            ),
            (
                "an assistant row's text block",
                "{\"type\":\"assistant\",\"message\":{\"role\":\"assistant\",\"content\":\
                 [{\"type\":\"text\",\"body\":\"hi\"}],\"usage\":{\"output_tokens\":1}}}",
            ),
        ];
        for (what, row) in moved {
            let log = [user_says("go").as_str(), row].join("\n");
            assert!(
                matches!(ClaudeCode.adapt(&log), Err(Drift::MissingField { .. })),
                "{what}: a declared field that moved is a refusal"
            );
        }

        // `input` renamed away entirely, and `input` reshaped.
        for input in [
            "\"arguments\":{\"file_path\":\"/srv/p/a.rs\"}",
            "\"input\":\"ls -la\"",
        ] {
            let log = [
                user_says("go"),
                assistant(
                    &format!("[{{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Read\",{input}}}]"),
                    1,
                    [1, 0, 0],
                ),
            ]
            .join("\n");
            assert!(
                ClaudeCode.adapt(&log).is_err(),
                "a call whose arguments moved is refused, not read as a call \
                 with none: {input}"
            );
        }
    }

    /// A tool argument the record's value space cannot hold is refused.
    ///
    /// The whole `Drift::Unrepresentable` path had no test, no fixture and no
    /// seeded fault: four of the five refusal variants were never produced by
    /// anything in the suite. Under this repository's own rule, most of the
    /// refusal vocabulary had never been seen at all.
    #[test]
    fn a_tool_argument_the_record_cannot_spell_is_named_rather_than_coerced() {
        for (why, value) in [("null", "null"), ("an exponent", "1e5")] {
            let log = [
                user_says("go"),
                assistant(
                    &format!(
                        "[{{\"type\":\"tool_use\",\"id\":\"t\",\"name\":\"Read\",\
                         \"input\":{{\"file_path\":{value}}}}}]"
                    ),
                    1,
                    [1, 0, 0],
                ),
            ]
            .join("\n");
            assert!(
                matches!(ClaudeCode.adapt(&log), Err(Drift::Unrepresentable { .. })),
                "{why} is refused by name rather than coerced into a string"
            );
        }
    }

    /// A refusal points at the line of the FILE, blank lines and all.
    ///
    /// `parse` numbered by file line and the walk renumbered by position in
    /// the filtered vector, so one blank line anywhere above a fault sent a
    /// reader to the wrong line -- two numbering systems inside one enum
    /// whose whole doc comment is that a refusal says where.
    #[test]
    fn a_refusal_counts_the_files_lines_and_not_the_rows_it_kept() {
        let log = [
            "",
            "",
            "{\"type\":\"assistant\",\"msg\":{\"role\":\"assistant\",\"content\":[]}}",
        ]
        .join("\n");
        assert!(
            matches!(
                ClaudeCode.adapt(&log),
                Err(Drift::MissingField { at_row: 3, .. })
            ),
            "the bad row is on line three of the file, not row one of the vector"
        );
    }

    /// "The harness said nothing" is not "the tool printed nothing", and the
    /// same for "it did not fail".
    ///
    /// The `is_error` half of this had no test, and a seeded fault turning an
    /// absent `is_error` into `Some(0)` went green: the one test that asserts
    /// an absent exit does it on a call that was never ANSWERED, where
    /// `tool_result` never runs and the `None` comes from construction. An
    /// answered call whose `is_error` the harness omitted -- which is every
    /// successful call Claude Code writes -- went through the mutated line
    /// and nothing looked.
    #[test]
    fn a_tool_result_that_said_neither_leaves_both_absent() {
        let log = [
            user_says("go"),
            assistant(
                "[{\"type\":\"tool_use\",\"id\":\"t1\",\"name\":\"Bash\",\
                 \"input\":{\"command\":\"true\"}}]",
                1,
                [1, 0, 0],
            ),
            "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\
             [{\"type\":\"tool_result\",\"tool_use_id\":\"t1\"}]}}"
                .to_owned(),
        ]
        .join("\n");

        let adapted = ClaudeCode.adapt(&log).expect("it adapts");
        let answered = adapted.events.iter().find_map(|event| match event {
            Event::ToolCall { output, exit, .. } => Some((output.clone(), *exit)),
            _ => None,
        });
        let (output, exit) = answered.expect("the call is in the stream");
        assert_eq!(
            output, None,
            "the call was answered and the answer said nothing about output; \
             `Some(\"\")` would claim the tool printed an empty string"
        );
        assert_eq!(
            exit, None,
            "and nothing about failure either; `Some(0)` would claim the \
             harness reported a success it never reported"
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
