//! Anthropic's Messages API (#555): the request a [`RequestShape`] becomes,
//! and the stream that comes back, read into the client's own [`Piece`]s.
//!
//! **Where the shapes differ, the kind is explicit.** The session keeps one
//! message model; this module is the one place it is rendered as Anthropic's
//! and read back. The head digest is the session's own
//! ([`super::head::Head`]), never this body's, so a record verifies the same
//! way whichever wire carried it.
//!
//! **Conventions, by the vote** (Pi `42a3497d`, `OpenCode` 2 `055d95bb`,
//! Qwen Code `c0c697c8`; Dispatch's ruling, 2026-10-10):
//! - `system` is a list of text blocks, the last marked for the cache.
//! - Breakpoints: the last system block, the last tool, the last block of
//!   the final user message -- Pi's placement, which Qwen Code shares
//!   (`pi:packages/ai/src/api/anthropic-messages.ts:1169-1191,1608,1483-1509`;
//!   `qc:…/anthropicContentGenerator/converter.ts:447-480,1387-1433`). Each
//!   `{"type":"ephemeral"}`, five minutes; the lifetime lever is #556's.
//! - Thinking: adaptive models get `{"type":"adaptive","display":"summarized"}`
//!   and `output_config.effort`; budget models `{"type":"enabled",
//!   "budget_tokens"}`. The interleaved-thinking beta goes with either.
//! - Replay: a signed thinking block goes back verbatim, every turn, and a
//!   redacted one with its data; reasoning with no signature goes back as
//!   text (Pi, `anthropic-messages.ts:1419-1431`).
//! - Tool results: `tool_result` blocks in one user message, consecutive
//!   results merged, an image inside the result's content.
//! - Usage: read at `message_start`, each count a later `message_delta`
//!   carries winning.
//! - `stop_reason`: `end_turn`, `stop_sequence` and `pause_turn` stop;
//!   `tool_use` is calls; `max_tokens` is capped; `refusal` is refused.

use std::collections::BTreeMap;

use serde_json::{Map, Number, Value, json};

use super::shape::{Message, Pin, RequestShape, Role};
use super::stream::{Ended, Piece};
use super::transport::TransportFailure;

/// The `anthropic-version` every request carries.
pub const VERSION: &str = "2023-06-01";

/// The beta that interleaves thinking with tool calls, sent whenever
/// thinking is on (the vote: `OpenCode` always, Qwen Code with thinking).
pub const INTERLEAVED_THINKING: &str = "interleaved-thinking-2025-05-14";

/// The dialect's name, as `session.start`'s `serving.dialect` records it.
pub const DIALECT: &str = "anthropic-messages";

/// How the model is asked to think, as its registry entry's `thinking`
/// declares and the regimen's `[reasoning]` sets: one way per session, the
/// forks' included.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Thinking {
    /// Not asked.
    #[default]
    Off,
    /// `{"type":"adaptive"}`, at `effort` when the regimen names one.
    Adaptive {
        /// `output_config.effort`.
        effort: Option<String>,
    },
    /// `{"type":"enabled","budget_tokens":…}`.
    Budget {
        /// The budget.
        tokens: u64,
    },
}

/// What a request carries beyond its shape.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    /// How the model thinks.
    pub thinking: Thinking,
    /// How long each breakpoint asks the cache to keep its prefix (#556).
    pub ttl: Ttls,
}

/// How long a breakpoint asks the cache to keep its prefix (#556).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Ttl {
    /// Five minutes: `{"type":"ephemeral"}`, the API's default and the
    /// three harnesses'.
    #[default]
    FiveMinutes,
    /// An hour: `{"type":"ephemeral","ttl":"1h"}`.
    Hour,
}

/// Each breakpoint's lifetime, in the cache's wire order: tools, system,
/// the final user message's tail (#556).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ttls {
    /// The last tool's.
    pub tools: Ttl,
    /// The last system block's.
    pub system: Ttl,
    /// The last block of the final user message's.
    pub tail: Ttl,
}

impl Ttls {
    /// Every breakpoint at `ttl`.
    #[must_use]
    pub fn all(ttl: Ttl) -> Self {
        Self {
            tools: ttl,
            system: ttl,
            tail: ttl,
        }
    }

    /// Each breakpoint as sent: an hour when it, or any breakpoint after it
    /// on the wire, asks for one. Anthropic refuses a longer lifetime after
    /// a shorter one, and Qwen Code promotes the earlier anchors rather than
    /// refusing (`qc:packages/core/src/core/anthropicContentGenerator/
    /// converter.ts:1009-1044`, `resolveCacheRetention`).
    #[must_use]
    pub fn on_the_wire(self) -> Self {
        let hour = |ttls: &[Ttl]| {
            if ttls.contains(&Ttl::Hour) {
                Ttl::Hour
            } else {
                Ttl::FiveMinutes
            }
        };
        Self {
            tools: hour(&[self.tools, self.system, self.tail]),
            system: hour(&[self.system, self.tail]),
            tail: self.tail,
        }
    }

    /// These, a fork's: its tools and system read the trunk's entries at
    /// the trunk's lifetimes, and its own tail is never written for an
    /// hour -- nothing reuses it.
    #[must_use]
    pub fn for_a_fork(self) -> Self {
        Self {
            tail: Ttl::FiveMinutes,
            ..self
        }
    }
}

impl Options {
    /// The `anthropic-beta` header's value, when any beta is asked for.
    #[must_use]
    pub fn beta(&self) -> Option<&'static str> {
        (self.thinking != Thinking::Off).then_some(INTERLEAVED_THINKING)
    }
}

/// A cache breakpoint at `ttl`.
fn breakpoint(ttl: Ttl) -> Value {
    match ttl {
        Ttl::FiveMinutes => json!({"type": "ephemeral"}),
        Ttl::Hour => json!({"type": "ephemeral", "ttl": "1h"}),
    }
}

/// The sampler settings Anthropic has a parameter for, by our name.
const SAMPLER: &[&str] = &["temperature", "top_p", "top_k"];

/// The streamed request `shape` is under `options`.
///
/// # Errors
///
/// A pin Anthropic has no parameter for, template kwargs (a local
/// template's), a grammar, or a call whose arguments are not a JSON object:
/// each is refused, never dropped, so what was declared is what was sent.
pub fn body(shape: &RequestShape, options: &Options) -> Result<String, String> {
    if !shape.template_kwargs.is_empty() {
        return Err("chat template kwargs are a local template's; Anthropic has none".to_owned());
    }
    if shape.grammar.is_some() {
        return Err("a grammar has no Anthropic parameter".to_owned());
    }
    let mut out = Map::new();
    out.insert("model".to_owned(), json!(shape.model));
    out.insert(
        "max_tokens".to_owned(),
        json!(shape.limits.max_output_tokens),
    );
    out.insert("stream".to_owned(), json!(true));
    let head = shape
        .messages
        .iter()
        .take_while(|message| message.role == Role::System)
        .count();
    let mut system: Vec<Value> = shape.messages[..head]
        .iter()
        .map(|message| json!({"type": "text", "text": message.content}))
        .collect();
    let ttl = options.ttl.on_the_wire();
    if let Some(Value::Object(last)) = system.last_mut() {
        last.insert("cache_control".to_owned(), breakpoint(ttl.system));
    }
    if !system.is_empty() {
        out.insert("system".to_owned(), Value::Array(system));
    }
    out.insert(
        "messages".to_owned(),
        Value::Array(messages(&shape.messages[head..], ttl.tail)?),
    );
    let mut tools: Vec<Value> = shape
        .tools
        .iter()
        .map(|tool| {
            let mut entry = Map::new();
            entry.insert("name".to_owned(), json!(tool.name));
            if let Some(description) = &tool.description {
                entry.insert("description".to_owned(), json!(description));
            }
            let mut rendered = String::new();
            crate::formats::record::json::render(&tool.schema, &mut rendered);
            let schema: Value = serde_json::from_str(&rendered).unwrap_or(Value::Null);
            entry.insert("input_schema".to_owned(), schema);
            Value::Object(entry)
        })
        .collect();
    if let Some(Value::Object(last)) = tools.last_mut() {
        last.insert("cache_control".to_owned(), breakpoint(ttl.tools));
    }
    if !tools.is_empty() {
        out.insert("tools".to_owned(), Value::Array(tools));
    }
    for (setting, pin) in shape.sampler.iter() {
        let name = setting.tag();
        if !SAMPLER.contains(&name) {
            return Err(format!("`{name}` has no Anthropic parameter"));
        }
        let digits = match pin {
            Pin::Decimal(value) => value.as_str().to_owned(),
            Pin::Integer(value) => value.to_string(),
        };
        let number: Number = digits
            .parse()
            .map_err(|why| format!("`{name}` = {digits} is not a number: {why}"))?;
        out.insert(name.to_owned(), Value::Number(number));
    }
    match &options.thinking {
        Thinking::Off => {}
        Thinking::Adaptive { effort } => {
            out.insert(
                "thinking".to_owned(),
                json!({"type": "adaptive", "display": "summarized"}),
            );
            if let Some(effort) = effort {
                out.insert("output_config".to_owned(), json!({"effort": effort}));
            }
        }
        Thinking::Budget { tokens } => {
            out.insert(
                "thinking".to_owned(),
                json!({"type": "enabled", "budget_tokens": tokens}),
            );
        }
    }
    Ok(Value::Object(out).to_string())
}

/// The turns after the head as Anthropic's messages: a tool result joins
/// the user message its neighbours make, and the last block of the final
/// user message carries the third breakpoint.
fn messages(turns: &[Message], tail: Ttl) -> Result<Vec<Value>, String> {
    let mut out: Vec<(&'static str, Vec<Value>)> = Vec::new();
    for message in turns {
        let (role, blocks) = match message.role {
            Role::User | Role::System | Role::Developer => ("user", user_blocks(message)),
            Role::Tool => ("user", vec![tool_result(message)]),
            Role::Assistant => ("assistant", assistant_blocks(message)?),
        };
        match out.last_mut() {
            Some((last, held)) if *last == role => held.extend(blocks),
            _ => out.push((role, blocks)),
        }
    }
    if let Some(("user", blocks)) = out.last_mut()
        && let Some(Value::Object(block)) = blocks.last_mut()
    {
        block.insert("cache_control".to_owned(), breakpoint(tail));
    }
    Ok(out
        .into_iter()
        .map(|(role, content)| json!({"role": role, "content": content}))
        .collect())
}

fn image(image: &super::shape::Image) -> Value {
    json!({
        "type": "image",
        "source": {"type": "base64", "media_type": image.media_type(), "data": image.base64()},
    })
}

fn user_blocks(message: &Message) -> Vec<Value> {
    let mut blocks = Vec::new();
    if !message.content.is_empty() || message.images.is_empty() {
        blocks.push(json!({"type": "text", "text": message.content}));
    }
    blocks.extend(message.images.iter().map(image));
    blocks
}

fn tool_result(message: &Message) -> Value {
    let mut content = vec![json!({"type": "text", "text": message.content})];
    content.extend(message.images.iter().map(image));
    let mut block = Map::new();
    block.insert("type".to_owned(), json!("tool_result"));
    block.insert(
        "tool_use_id".to_owned(),
        json!(message.tool_call_id.clone().unwrap_or_default()),
    );
    block.insert("content".to_owned(), Value::Array(content));
    if message.tool_error {
        block.insert("is_error".to_owned(), json!(true));
    }
    Value::Object(block)
}

fn assistant_blocks(message: &Message) -> Result<Vec<Value>, String> {
    let mut blocks = Vec::new();
    match (&message.reasoning, &message.reasoning_signature) {
        (Some(thought), Some(signature)) => blocks.push(json!({
            "type": "thinking", "thinking": thought, "signature": signature,
        })),
        (Some(thought), None) if !thought.is_empty() => {
            blocks.push(json!({"type": "text", "text": thought}));
        }
        _ => {}
    }
    blocks.extend(
        message
            .redacted
            .iter()
            .map(|data| json!({"type": "redacted_thinking", "data": data})),
    );
    if !message.content.is_empty() {
        blocks.push(json!({"type": "text", "text": message.content}));
    }
    for call in &message.tool_calls {
        let input: Value = serde_json::from_str(&call.arguments)
            .ok()
            .filter(Value::is_object)
            .ok_or_else(|| {
                format!(
                    "call `{}`'s arguments are not a JSON object, and a `tool_use` input is one",
                    call.id
                )
            })?;
        blocks.push(json!({"type": "tool_use", "id": call.id, "name": call.name, "input": input}));
    }
    Ok(blocks)
}

/// A response's token counts, as Anthropic reports them (#555, #556):
/// `input_tokens` excludes the cache; the cache reads and writes are beside
/// it, the writes by lifetime when the response breaks them down.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// Prompt tokens neither read from nor written to the cache.
    pub input_tokens: Option<u64>,
    /// Tokens generated.
    pub output_tokens: Option<u64>,
    /// Prompt tokens read from the cache.
    pub cache_read_input_tokens: Option<u64>,
    /// Prompt tokens written to the cache.
    pub cache_creation_input_tokens: Option<u64>,
    /// Of those, written for five minutes.
    pub ephemeral_5m_input_tokens: Option<u64>,
    /// Of those, written for an hour.
    pub ephemeral_1h_input_tokens: Option<u64>,
}

impl Usage {
    /// Each count `object` carries over this one's: `message_start`'s, then
    /// any a `message_delta` sends.
    fn overlay(&mut self, object: &Value) {
        let count = |path: &str| object.pointer(path).and_then(Value::as_u64);
        let set = |slot: &mut Option<u64>, path: &str| {
            if let Some(n) = count(path) {
                *slot = Some(n);
            }
        };
        set(&mut self.input_tokens, "/input_tokens");
        set(&mut self.output_tokens, "/output_tokens");
        set(
            &mut self.cache_read_input_tokens,
            "/cache_read_input_tokens",
        );
        set(
            &mut self.cache_creation_input_tokens,
            "/cache_creation_input_tokens",
        );
        set(
            &mut self.ephemeral_5m_input_tokens,
            "/cache_creation/ephemeral_5m_input_tokens",
        );
        set(
            &mut self.ephemeral_1h_input_tokens,
            "/cache_creation/ephemeral_1h_input_tokens",
        );
    }

    /// The prompt in all: uncached, read and written.
    #[must_use]
    pub fn prompt_tokens(&self) -> Option<u64> {
        self.input_tokens.map(|input| {
            input
                + self.cache_read_input_tokens.unwrap_or(0)
                + self.cache_creation_input_tokens.unwrap_or(0)
        })
    }
}

/// What a content block is, by its `content_block_start`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Block {
    Text,
    Thinking,
    ToolUse,
    Other,
}

/// The stream's events, read into pieces.
#[derive(Debug, Default)]
pub(super) struct Decoder {
    blocks: BTreeMap<u64, Block>,
    usage: Usage,
    stop_reason: Option<String>,
    stopped: bool,
}

impl Decoder {
    /// One event's data. `Some` when it ends the stream.
    pub(super) fn event(
        &mut self,
        data: &str,
        on_delta: &mut dyn FnMut(Piece<'_>),
    ) -> Result<Option<Ended>, TransportFailure> {
        let value: Value = serde_json::from_str(data).map_err(|why| {
            TransportFailure::Framing(format!("an event's data is not JSON ({why}): {data}"))
        })?;
        let index = || value.get("index").and_then(Value::as_u64);
        match value.get("type").and_then(Value::as_str) {
            Some("message_start") => {
                if let Some(usage) = value.pointer("/message/usage") {
                    self.usage.overlay(usage);
                }
            }
            Some("content_block_start") => {
                let index = index().ok_or_else(|| missing("index", data))?;
                let block = value.get("content_block").unwrap_or(&Value::Null);
                let kind = match block.get("type").and_then(Value::as_str) {
                    Some("text") => Block::Text,
                    Some("thinking") => Block::Thinking,
                    Some("tool_use") => {
                        on_delta(Piece::ToolCall {
                            index,
                            id: block.get("id").and_then(Value::as_str),
                            name: block.get("name").and_then(Value::as_str),
                            arguments: "",
                        });
                        Block::ToolUse
                    }
                    Some("redacted_thinking") => {
                        if let Some(data) = block.get("data").and_then(Value::as_str) {
                            on_delta(Piece::Redacted(data));
                        }
                        Block::Other
                    }
                    _ => Block::Other,
                };
                self.blocks.insert(index, kind);
            }
            Some("content_block_delta") => {
                let index = index().ok_or_else(|| missing("index", data))?;
                let delta = value.get("delta").unwrap_or(&Value::Null);
                let text = |key: &str| delta.get(key).and_then(Value::as_str);
                match (
                    delta.get("type").and_then(Value::as_str),
                    self.blocks.get(&index),
                ) {
                    (Some("text_delta"), _) => {
                        if let Some(piece) = text("text").filter(|piece| !piece.is_empty()) {
                            on_delta(Piece::Text(piece));
                        }
                    }
                    (Some("thinking_delta"), _) => {
                        if let Some(piece) = text("thinking").filter(|piece| !piece.is_empty()) {
                            on_delta(Piece::Reasoning(piece));
                        }
                    }
                    (Some("signature_delta"), _) => {
                        if let Some(signature) = text("signature") {
                            on_delta(Piece::Signature(signature));
                        }
                    }
                    (Some("input_json_delta"), Some(Block::ToolUse)) => {
                        on_delta(Piece::ToolCall {
                            index,
                            id: None,
                            name: None,
                            arguments: text("partial_json").unwrap_or_default(),
                        });
                    }
                    _ => {}
                }
            }
            Some("message_delta") => {
                if let Some(reason) = value.pointer("/delta/stop_reason").and_then(Value::as_str) {
                    self.stop_reason = Some(reason.to_owned());
                }
                if let Some(usage) = value.get("usage") {
                    self.usage.overlay(usage);
                }
            }
            Some("message_stop") => {
                self.stopped = true;
                return Ok(Some(self.finished(on_delta)));
            }
            Some("error") => {
                return Ok(Some(Ended::Rejected {
                    status: 200,
                    body: data.to_owned(),
                    class: None,
                }));
            }
            // `ping`, `content_block_stop`, and a type this client does not
            // know: nothing about the answer.
            _ => {}
        }
        Ok(None)
    }

    /// How the stream ended: its usage delivered, then its stop.
    fn finished(&mut self, on_delta: &mut dyn FnMut(Piece<'_>)) -> Ended {
        on_delta(Piece::Usage(&self.usage));
        match self.stop_reason.as_deref() {
            Some("refusal") => Ended::Rejected {
                status: 200,
                body: "{\"stop_reason\":\"refusal\"}".to_owned(),
                class: Some(super::stream::Rejection::Refusal),
            },
            reason => Ended::Finished {
                finish_reason: Some(finish_reason(reason).to_owned()),
                timings: None,
            },
        }
    }

    /// The body ended: a stream with no `message_stop` did not finish.
    pub(super) fn at_close(
        &mut self,
        on_delta: &mut dyn FnMut(Piece<'_>),
    ) -> Result<Ended, TransportFailure> {
        if self.stopped || self.stop_reason.is_some() {
            return Ok(self.finished(on_delta));
        }
        Err(TransportFailure::Framing(
            "the stream ended before the server said it was done".to_owned(),
        ))
    }
}

/// Anthropic's `stop_reason` in the words the session settles by.
fn finish_reason(reason: Option<&str>) -> &'static str {
    match reason {
        Some("tool_use") => "tool_calls",
        Some("max_tokens") => "length",
        _ => "stop",
    }
}

fn missing(key: &str, data: &str) -> TransportFailure {
    TransportFailure::Framing(format!("an event carries no `{key}`: {data}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::client::shape::{Limits, SamplerCard, SamplerSetting, ToolCall, ToolDefinition};
    use crate::client::stream::{Cancel, HttpStream, Streaming as _};
    use crate::client::stub::{Act, Stub};
    use crate::client::transport::Endpoint;

    fn shape(messages: Vec<Message>) -> RequestShape {
        RequestShape {
            model: "a-hosted-model".to_owned(),
            messages,
            sampler: SamplerCard::empty(),
            limits: Limits {
                attempt: Duration::from_secs(5),
                call: Duration::from_secs(5),
                max_output_tokens: 1024,
                retries: 0,
                context_window: None,
                idle: None,
            },
            grammar: None,
            template_kwargs: BTreeMap::new(),
            tools: Vec::new(),
        }
    }

    fn parsed(body: &str) -> Value {
        serde_json::from_str(body).expect("the body is JSON")
    }

    /// The session's messages as Anthropic's (#555, the vote): the head is
    /// `system`, its last block cached; a signed thinking block goes back
    /// verbatim, a redacted one with its data; consecutive tool results are
    /// one user message, an image inside its result; the final user
    /// message's last block and the last tool carry the other breakpoints.
    #[test]
    fn a_shape_becomes_anthropics_messages_with_the_votes_breakpoints() {
        let mut said = Message::new(Role::Assistant, "Reading both.");
        said.reasoning = Some("which files?".to_owned());
        said.reasoning_signature = Some("sig-1".to_owned());
        said.redacted = vec!["opaque".to_owned()];
        said.tool_calls = vec![
            ToolCall {
                id: "toolu_1".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"a.rs"}"#.to_owned(),
            },
            ToolCall {
                id: "toolu_2".to_owned(),
                name: "read".to_owned(),
                arguments: r#"{"path":"b.png"}"#.to_owned(),
            },
        ];
        let first = Message::tool_result("toolu_1".to_owned(), "fn a() {}");
        let mut second = Message::tool_result("toolu_2".to_owned(), "");
        second.tool_error = true;
        let mut request = shape(vec![
            Message::new(Role::System, "you are a coder"),
            Message::new(Role::User, "read a.rs and b.png"),
            said,
            first,
            second,
        ]);
        request.tools = vec![ToolDefinition {
            name: "read".to_owned(),
            description: Some("read a file".to_owned()),
            schema: crate::formats::record::json::Value::Object(
                crate::formats::record::json::line(r#"{"type":"object"}"#).expect("a schema"),
            ),
        }];
        request.sampler = SamplerCard::empty().with(
            SamplerSetting::Temperature,
            Pin::Decimal(crate::formats::record::json::Decimal::new("0.60").expect("digits")),
        );
        let options = Options {
            ttl: Ttls::default(),
            thinking: Thinking::Adaptive {
                effort: Some("medium".to_owned()),
            },
        };
        let body = body(&request, &options).expect("a body");
        assert!(
            body.contains(r#""temperature":0.60"#),
            "digits as written: {body}"
        );
        let value = parsed(&body);
        assert_eq!(
            value["system"],
            json!([{"type": "text", "text": "you are a coder", "cache_control": {"type": "ephemeral"}}])
        );
        assert_eq!(
            value["messages"][1],
            json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": "which files?", "signature": "sig-1"},
                {"type": "redacted_thinking", "data": "opaque"},
                {"type": "text", "text": "Reading both."},
                {"type": "tool_use", "id": "toolu_1", "name": "read", "input": {"path": "a.rs"}},
                {"type": "tool_use", "id": "toolu_2", "name": "read", "input": {"path": "b.png"}},
            ]})
        );
        assert_eq!(
            value["messages"][2],
            json!({"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "toolu_1",
                 "content": [{"type": "text", "text": "fn a() {}"}]},
                {"type": "tool_result", "tool_use_id": "toolu_2", "is_error": true,
                 "content": [{"type": "text", "text": ""}],
                 "cache_control": {"type": "ephemeral"}},
            ]})
        );
        assert_eq!(value["messages"].as_array().map(Vec::len), Some(3));
        assert_eq!(
            value["tools"],
            json!([{"name": "read", "description": "read a file",
                    "input_schema": {"type": "object"}, "cache_control": {"type": "ephemeral"}}])
        );
        assert_eq!(
            (
                &value["thinking"],
                &value["output_config"],
                &value["stream"]
            ),
            (
                &json!({"type": "adaptive", "display": "summarized"}),
                &json!({"effort": "medium"}),
                &json!(true)
            )
        );
        assert_eq!(options.beta(), Some(INTERLEAVED_THINKING));
        assert_eq!(Options::default().beta(), None);
    }

    /// #556: each breakpoint at its lifetime, longer before shorter in the
    /// cache's tools, system, messages order -- an anchor promoted to an hour
    /// when one after it asks for an hour, as Qwen Code resolves it -- and a
    /// fork's tail never written for an hour.
    #[test]
    fn breakpoints_carry_their_lifetimes_longer_before_shorter() {
        let hour = json!({"type": "ephemeral", "ttl": "1h"});
        let five = json!({"type": "ephemeral"});
        let mut request = shape(vec![
            Message::new(Role::System, "you are a coder"),
            Message::new(Role::User, "hi"),
        ]);
        request.tools = vec![ToolDefinition {
            name: "read".to_owned(),
            description: None,
            schema: crate::formats::record::json::Value::Object(
                crate::formats::record::json::line(r#"{"type":"object"}"#).expect("a schema"),
            ),
        }];
        let sent = |ttl: Ttls| {
            let value = parsed(
                &body(
                    &request,
                    &Options {
                        ttl,
                        ..Options::default()
                    },
                )
                .expect("a body"),
            );
            (
                value["tools"][0]["cache_control"].clone(),
                value["system"][0]["cache_control"].clone(),
                value["messages"][0]["content"][0]["cache_control"].clone(),
            )
        };
        assert_eq!(
            sent(Ttls::default()),
            (five.clone(), five.clone(), five.clone())
        );
        let per_breakpoint = Ttls {
            tools: Ttl::Hour,
            system: Ttl::Hour,
            tail: Ttl::FiveMinutes,
        };
        assert_eq!(
            sent(per_breakpoint),
            (hour.clone(), hour.clone(), five.clone())
        );
        assert_eq!(
            sent(Ttls::all(Ttl::Hour)),
            (hour.clone(), hour.clone(), hour.clone())
        );
        // An hour on the system alone promotes the tools before it.
        let system_only = Ttls {
            system: Ttl::Hour,
            ..Ttls::default()
        };
        assert_eq!(
            sent(system_only),
            (hour.clone(), hour.clone(), five.clone())
        );
        // A fork reads the trunk's system and tools, its tail five minutes.
        assert_eq!(
            sent(Ttls::all(Ttl::Hour).for_a_fork()),
            (hour.clone(), hour, five)
        );
    }

    /// What has no Anthropic parameter is refused, never dropped; reasoning
    /// with no signature goes back as text; a budget is `enabled`.
    #[test]
    fn what_anthropic_cannot_carry_is_refused_and_unsigned_reasoning_is_text() {
        let mut request = shape(vec![Message::new(Role::User, "hi")]);
        request.sampler = SamplerCard::empty().with(SamplerSetting::MinP, Pin::Integer(0));
        assert_eq!(
            body(&request, &Options::default()),
            Err("`min_p` has no Anthropic parameter".to_owned())
        );
        let mut request = shape(vec![Message::new(Role::User, "hi")]);
        request.template_kwargs.insert(
            "enable_thinking".to_owned(),
            crate::formats::record::json::Value::Boolean(true),
        );
        assert!(body(&request, &Options::default()).is_err());

        let mut earlier = Message::new(Role::Assistant, "done");
        earlier.reasoning = Some("local thoughts".to_owned());
        let request = shape(vec![
            Message::new(Role::User, "a"),
            earlier,
            Message::new(Role::User, "b"),
        ]);
        let value = parsed(
            &body(
                &request,
                &Options {
                    ttl: Ttls::default(),
                    thinking: Thinking::Budget { tokens: 16_384 },
                },
            )
            .expect("a body"),
        );
        assert_eq!(
            value["messages"][1]["content"],
            json!([{"type": "text", "text": "local thoughts"}, {"type": "text", "text": "done"}])
        );
        assert_eq!(
            value["thinking"],
            json!({"type": "enabled", "budget_tokens": 16_384})
        );
        assert_eq!(value.get("output_config"), None);
    }

    /// One SSE event, as `data:` after its `event:` line.
    fn event(data: &Value) -> String {
        format!(
            "event: {}\ndata: {data}\n\n",
            data["type"].as_str().expect("a type")
        )
    }

    /// A 200 carrying `events`, closed after.
    pub(crate) fn streamed(events: &[Value]) -> Act {
        let body: String = events.iter().map(event).collect();
        Act::Raw(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
            )
            .into_bytes(),
        )
    }

    /// CONSTRUCTED, not captured: a stream in the shapes Anthropic documents
    /// for a thinking turn that calls a tool, with usage at `message_start`
    /// and the output count, cache reads and writes at `message_delta`.
    pub(crate) fn thinking_then_a_call() -> Vec<Value> {
        vec![
            json!({"type": "message_start", "message": {"id": "msg_1", "type": "message",
                "role": "assistant", "content": [], "model": "a-hosted-model",
                "usage": {"input_tokens": 12, "output_tokens": 1,
                          "cache_read_input_tokens": 3000, "cache_creation_input_tokens": 40,
                          "cache_creation": {"ephemeral_5m_input_tokens": 40, "ephemeral_1h_input_tokens": 0}}}}),
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "thinking_delta", "thinking": "look at a.rs"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "signature_delta", "signature": "EqQB"}}),
            json!({"type": "content_block_stop", "index": 0}),
            json!({"type": "ping"}),
            json!({"type": "content_block_start", "index": 1,
                   "content_block": {"type": "redacted_thinking", "data": "opaque"}}),
            json!({"type": "content_block_stop", "index": 1}),
            json!({"type": "content_block_start", "index": 2,
                   "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 2,
                   "delta": {"type": "text_delta", "text": "Reading."}}),
            json!({"type": "content_block_stop", "index": 2}),
            json!({"type": "content_block_start", "index": 3,
                   "content_block": {"type": "tool_use", "id": "toolu_1", "name": "read", "input": {}}}),
            json!({"type": "content_block_delta", "index": 3,
                   "delta": {"type": "input_json_delta", "partial_json": "{\"path\":"}}),
            json!({"type": "content_block_delta", "index": 3,
                   "delta": {"type": "input_json_delta", "partial_json": "\"a.rs\"}"}}),
            json!({"type": "content_block_stop", "index": 3}),
            json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"},
                   "usage": {"output_tokens": 88}}),
            json!({"type": "message_stop"}),
        ]
    }

    /// CONSTRUCTED, not captured: a final answer after signed thinking,
    /// its usage with cache reads and a five-minute write.
    pub(crate) fn a_signed_answer() -> Vec<Value> {
        vec![
            json!({"type": "message_start", "message": {"usage": {
                "input_tokens": 9, "output_tokens": 1,
                "cache_read_input_tokens": 2048, "cache_creation_input_tokens": 64,
                "cache_creation": {"ephemeral_5m_input_tokens": 64, "ephemeral_1h_input_tokens": 0}}}}),
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "thinking", "thinking": "", "signature": ""}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "thinking_delta", "thinking": "brief"}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "signature_delta", "signature": "sig-xyz"}}),
            json!({"type": "content_block_start", "index": 1,
                   "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 1,
                   "delta": {"type": "text_delta", "text": "Done."}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                   "usage": {"output_tokens": 30}}),
            json!({"type": "message_stop"}),
        ]
    }

    /// CONSTRUCTED, not captured: an answer of `text`, its prompt `input`
    /// tokens uncached, `read` read from the cache and `written` written
    /// for five minutes.
    pub(crate) fn answering(text: &str, (input, read, written): (u64, u64, u64)) -> Vec<Value> {
        vec![
            json!({"type": "message_start", "message": {"usage": {
                "input_tokens": input, "output_tokens": 1,
                "cache_read_input_tokens": read, "cache_creation_input_tokens": written,
                "cache_creation": {"ephemeral_5m_input_tokens": written, "ephemeral_1h_input_tokens": 0}}}}),
            json!({"type": "content_block_start", "index": 0,
                   "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 0,
                   "delta": {"type": "text_delta", "text": text}}),
            json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"},
                   "usage": {"output_tokens": 20}}),
            json!({"type": "message_stop"}),
        ]
    }

    /// What a stream delivered, piece by piece, in words.
    fn played(stub: &Stub, options: Options) -> (Vec<String>, Result<Ended, TransportFailure>) {
        let mut endpoint = Endpoint::parse(&stub.url()).expect("an endpoint");
        "/v1/messages".clone_into(&mut endpoint.path);
        let transport = HttpStream::new(endpoint)
            .with_bearer(crate::client::stream::Bearer::new("sk-test-key").expect("a key"))
            .with_anthropic(options);
        let mut pieces = Vec::new();
        let ended = transport.stream(
            &shape(vec![Message::new(Role::User, "read a.rs")]),
            Instant::now() + Duration::from_secs(10),
            &Cancel::new(),
            &mut |piece| {
                pieces.push(match piece {
                    Piece::Text(text) => format!("text {text}"),
                    Piece::Reasoning(text) => format!("reasoning {text}"),
                    Piece::Signature(text) => format!("signature {text}"),
                    Piece::Redacted(text) => format!("redacted {text}"),
                    Piece::ToolCall {
                        index,
                        id,
                        name,
                        arguments,
                    } => {
                        format!("call {index} {id:?} {name:?} {arguments}")
                    }
                    Piece::Usage(usage) => format!("usage {usage:?}"),
                    Piece::Progress(_) => "progress".to_owned(),
                });
            },
        );
        (pieces, ended)
    }

    /// #555: the stream read into the client's pieces -- reasoning, its
    /// signature, the redacted block, the text, the call's fragments under
    /// its block's index -- then the usage, `message_delta`'s counts over
    /// `message_start`'s, and `tool_use` as calls; the request carries the
    /// key as `x-api-key`, the version and the beta, and no `Authorization`.
    #[test]
    fn a_stream_is_read_into_pieces_and_its_usage_and_the_request_carries_the_headers() {
        let stub = Stub::serving(vec![streamed(&thinking_then_a_call())]).expect("loopback");
        let (pieces, ended) = played(
            &stub,
            Options {
                ttl: Ttls::default(),
                thinking: Thinking::Adaptive { effort: None },
            },
        );
        let usage = Usage {
            input_tokens: Some(12),
            output_tokens: Some(88),
            cache_read_input_tokens: Some(3000),
            cache_creation_input_tokens: Some(40),
            ephemeral_5m_input_tokens: Some(40),
            ephemeral_1h_input_tokens: Some(0),
        };
        assert_eq!(
            pieces,
            [
                "reasoning look at a.rs".to_owned(),
                "signature EqQB".to_owned(),
                "redacted opaque".to_owned(),
                "text Reading.".to_owned(),
                "call 3 Some(\"toolu_1\") Some(\"read\") ".to_owned(),
                "call 3 None None {\"path\":".to_owned(),
                "call 3 None None \"a.rs\"}".to_owned(),
                format!("usage {usage:?}"),
            ]
        );
        assert_eq!(usage.prompt_tokens(), Some(3052));
        assert_eq!(
            ended,
            Ok(Ended::Finished {
                finish_reason: Some("tool_calls".to_owned()),
                timings: None,
            })
        );
        let heads = stub.heads();
        let head = heads[0].to_ascii_lowercase();
        assert!(head.starts_with("post /v1/messages http/1.1\r\n"), "{head}");
        for line in [
            "x-api-key: sk-test-key\r\n",
            "anthropic-version: 2023-06-01\r\n",
            "anthropic-beta: interleaved-thinking-2025-05-14\r\n",
        ] {
            assert!(head.contains(line), "{line}: {head}");
        }
        assert!(!head.contains("authorization:"), "{head}");
    }

    /// `refusal` is a refusal, `max_tokens` capped, `pause_turn` a stop; an
    /// `error` event is a rejection; a stream that never says it stopped did
    /// not finish.
    #[test]
    fn stop_reasons_map_to_the_sessions_words_and_an_unfinished_stream_is_a_failure() {
        let ending = |reason: &str| {
            vec![
                json!({"type": "message_start", "message": {"usage": {"input_tokens": 5, "output_tokens": 1}}}),
                json!({"type": "message_delta", "delta": {"stop_reason": reason}, "usage": {"output_tokens": 2}}),
                json!({"type": "message_stop"}),
            ]
        };
        for (reason, finish) in [
            ("end_turn", "stop"),
            ("pause_turn", "stop"),
            ("max_tokens", "length"),
        ] {
            let stub = Stub::serving(vec![streamed(&ending(reason))]).expect("loopback");
            let (_, ended) = played(&stub, Options::default());
            assert_eq!(
                ended,
                Ok(Ended::Finished {
                    finish_reason: Some(finish.to_owned()),
                    timings: None
                }),
                "{reason}"
            );
        }
        let stub = Stub::serving(vec![streamed(&ending("refusal"))]).expect("loopback");
        let (_, ended) = played(&stub, Options::default());
        assert!(
            matches!(
                ended,
                Ok(Ended::Rejected {
                    class: Some(super::super::stream::Rejection::Refusal),
                    ..
                })
            ),
            "{ended:?}"
        );
        let stub = Stub::serving(vec![streamed(&[
            json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
        ])])
        .expect("loopback");
        let (_, ended) = played(&stub, Options::default());
        assert!(
            matches!(
                ended,
                Ok(Ended::Rejected {
                    status: 200,
                    class: None,
                    ..
                })
            ),
            "{ended:?}"
        );
        let stub = Stub::serving(vec![streamed(&ending("end_turn")[..1])]).expect("loopback");
        let (_, ended) = played(&stub, Options::default());
        assert!(
            matches!(ended, Err(TransportFailure::Framing(_))),
            "{ended:?}"
        );
    }
}
