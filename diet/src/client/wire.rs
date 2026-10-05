//! The OpenAI-compatible surface: what goes out, and how what comes back is
//! read.
//!
//! Two asymmetries decide this module's shape.
//!
//! The request body is **hand-rendered**. A pinned `0.6` that went out
//! through a binary float would be a different regime from the one the
//! regimen declares, and it would be a different one silently; the text that
//! was pinned is the text that is sent.
//!
//! The response body is read with a real JSON reader, because it is the
//! server's format and not this program's. `crate::formats::record::json` is
//! deliberately not a JSON library -- it has no `null`, no exponents, no
//! binary floats -- and every one of those appears in a live server's reply.
//! Pointing the record's reader at a foreign format would be the eleven
//! divergent decline-detectors again, one level down.

use std::collections::BTreeMap;
use std::error::Error;
use std::fmt::{self, Write as _};

use serde_json::Value;

use super::shape::{Dialect, Pin, RequestShape};
use super::stream::Timings;

/// What came back, read through the dialect's declared paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    /// The answer's text, if the reply carried one. `Some("")` is an answer
    /// of nothing at all, which is a typed outcome and not an absence.
    pub text: Option<String>,
    /// Why the server stopped, as the server spells it.
    pub finish_reason: Option<String>,
    /// The reasoning the server reports it did, where the dialect declares a
    /// place for it.
    ///
    /// `None` means nobody looked, or the declared path reached nothing;
    /// `Some("")` means the field was there and empty, which is the server
    /// saying it thought about nothing. Not collapsed, because they are
    /// different facts about the reply -- though #94's negative control,
    /// which counts characters, reads both as none.
    pub reasoning: Option<String>,
    /// How many tokens came back.
    pub output_tokens: Option<u64>,
    /// How many prompt tokens the server read.
    pub prompt_tokens: Option<u64>,
    /// How many of them it reused from its cache -- the direct measurement of
    /// prefix stability, which is what the seam controller's claims are
    /// checked against.
    pub cached_tokens: Option<u64>,
    /// The sampler settings the server reports it used, by the names it used.
    /// Empty when the dialect declares no echo site, or the site was absent.
    pub sampler_echo: BTreeMap<String, Reported>,
    /// Whether the dialect declared an echo site at all. Without this, "the
    /// server reported nothing" and "nobody asked it to" are the same empty
    /// map, and they are not the same fact.
    pub echo_site_declared: bool,
    /// Whether the declared echo site was present in the reply.
    pub echo_site_present: bool,
    /// What the server measured of the request, read at the dialect's
    /// declared `timings` path, when it is there and an object.
    pub timings: Option<Timings>,
}

/// A value the server reported for one of its settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reported {
    /// A number, as the server spelled it.
    Number(String),
    /// Text.
    Text(String),
    /// `true` or `false`.
    Boolean(bool),
    /// Something this client does not compare: an object, an array, or a
    /// `null`. Kept as its shape's name rather than dropped, because a
    /// setting reported as `null` is a server saying something.
    Other(&'static str),
}

impl fmt::Display for Reported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Number(text) | Self::Text(text) => f.write_str(text),
            Self::Boolean(value) => write!(f, "{value}"),
            Self::Other(shape) => f.write_str(shape),
        }
    }
}

/// Why a reply could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// The body is not JSON.
    NotJson(String),
    /// The body is JSON but not an object, so no declared path can reach
    /// anything in it.
    NotAnObject,
    /// A `200` carrying no answer where an answer goes. Not an empty answer:
    /// `""` is a thing a model said and this is a reply that does not have
    /// the field. Telling those apart is the whole job.
    NoAnswer,
    /// A token count above what the record's value space can spell, so it
    /// cannot be banked as the number it is.
    CountTooLarge(u64),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJson(why) => write!(f, "the reply is not JSON: {why}"),
            Self::NotAnObject => f.write_str("the reply is JSON but not an object"),
            Self::NoAnswer => {
                f.write_str("the reply carries no answer at `choices.0.message.content`")
            }
            Self::CountTooLarge(count) => {
                write!(
                    f,
                    "the reply reports {count} tokens, which no record can spell"
                )
            }
        }
    }
}

impl Error for WireError {}

/// The request body for `shape`, rendered as JSON.
///
/// Every pinned decimal appears as the digits that were pinned.
///
/// **IT STARTS FROM THE HEAD**, so the bytes [`head`] hashes are the bytes
/// that go out. A second renderer here -- one that happened to agree today --
/// would make `request.head_sha256` a digest of something nobody sent, which
/// is the class of defect a fingerprint exists to close rather than open.
/// `client::head::tests::the_head_is_the_bytes_the_body_starts_with` is the
/// drift guard: it asserts `body(shape).starts_with(&head(shape))`.
#[must_use]
pub fn body(shape: &RequestShape) -> String {
    let mut out = head(shape);
    // The MUTABLE TAIL: the one message the head deliberately leaves out.
    // `head` ends mid-array, so this closes it.
    if let Some(last) = shape.messages.last() {
        if shape.messages.len() > 1 {
            out.push(',');
        }
        message(last, &mut out);
    }
    out.push(']');

    for (setting, pin) in shape.sampler.iter() {
        out.push(',');
        string(setting.tag(), &mut out);
        out.push(':');
        match pin {
            Pin::Decimal(value) => out.push_str(value.as_str()),
            Pin::Integer(value) => out.push_str(&value.to_string()),
        }
    }

    out.push_str(",\"max_tokens\":");
    out.push_str(&shape.limits.max_output_tokens.to_string());

    if let Some(grammar) = &shape.grammar {
        out.push_str(",\"grammar\":");
        string(grammar, &mut out);
    }

    out.push('}');
    out
}

/// The streaming form of [`body`]: the same bytes, then `"stream":true` and
/// `"stream_options":{"include_usage":true}` before the closing brace.
///
/// Appended at the END so the head is untouched: [`head`] is still the
/// opening of what goes out, and a streamed request hashes to the same
/// `request.head_sha256` as the same request unstreamed -- the prefix a
/// server caches does not depend on how the answer is delivered.
///
/// `include_usage` is not decoration. Measured against llama-server
/// (`4df29be`, #117): a streamed reply without it carries `timings` on its
/// last chunk and NO `usage` at all, and a record's token counts are
/// required; with it, one extra chunk carries both.
///
/// `return_progress` asks for the server's prefill progress: measured on the
/// drive endpoint (`e7051ef`, #117 R3.0's C1), it then streams a
/// `prompt_progress` object on the chunks before the answer, and the reply
/// is otherwise unchanged.
///
/// # Panics
///
/// Never: [`body`] closes its object as the last thing it writes.
#[must_use]
pub fn streaming_body(shape: &RequestShape) -> String {
    let whole = body(shape);
    let open = whole
        .strip_suffix('}')
        .expect("`body` closes its object as the last thing it writes");
    format!(
        "{open},\"stream\":true,\"stream_options\":{{\"include_usage\":true}},\
         \"return_progress\":true}}"
    )
}

/// The FROZEN HEAD of `shape`: everything a server can reuse from its cache.
///
/// The opening of [`body`], byte for byte, ending mid-array just before the
/// mutable tail. What a request hashes into `request.head_sha256` is exactly
/// this string, through [`crate::digest::sha256_hex`].
///
/// **What is in, and why each:**
///
/// * the **model**, because a different model is a different cache;
/// * the **tool definitions, in declaration order**, because the order is
///   what #79's MCP specimen moves;
/// * the **chat-template arguments**, because on a server-side template they
///   are the only thing a request carries about an effort level -- #79's
///   third live-incident class is a level that rewrites the head, and a hash
///   that excluded them would hash two different rendered heads alike and
///   push a real mutation into the residual;
/// * **every message but the last**, which is the prefix the seam controller
///   claims stays byte-identical.
///
/// **What is out:** the sampler pins, the output cap, the grammar, and the
/// last message. A temperature change is a regime change and not a prefix
/// mutation; a server prefills the same bytes either way.
#[must_use]
pub fn head(shape: &RequestShape) -> String {
    let mut out = String::from("{\"model\":");
    string(&shape.model, &mut out);

    // No key at all when nothing is declared -- the rule
    // `chat_template_kwargs` follows just below, and for the same reason.
    if !shape.tools.is_empty() {
        out.push_str(",\"tools\":[");
        for (index, tool) in shape.tools.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"type\":\"function\",\"function\":{\"name\":");
            string(&tool.name, &mut out);
            out.push_str(",\"parameters\":");
            crate::formats::record::json::render(&tool.schema, &mut out);
            out.push_str("}}");
        }
        out.push(']');
    }

    // THE TEMPLATE'S ARGUMENTS, not the sampler's, and they go in their own
    // object because that is where a chat template reads them from. Rendered
    // through the RECORD's renderer rather than spelled again here: a kwarg
    // is carried into the archive of the request that sent it, and two
    // renderings of one value space is the drift the hand-rendered sampler
    // pins already exist to avoid one level down.
    //
    // No key at all when nothing is set. An empty `chat_template_kwargs` is
    // a field this program added to a request that had nothing to say with
    // it.
    if !shape.template_kwargs.is_empty() {
        out.push_str(",\"chat_template_kwargs\":");
        crate::formats::record::json::render(
            &crate::formats::record::json::Value::Object(shape.template_kwargs.clone()),
            &mut out,
        );
    }

    out.push_str(",\"messages\":[");
    let frozen = shape.messages.len().saturating_sub(1);
    for (index, message_of_the_head) in shape.messages.iter().take(frozen).enumerate() {
        if index > 0 {
            out.push(',');
        }
        message(message_of_the_head, &mut out);
    }
    out
}

/// Append one message as the wire carries it.
///
/// A tool call goes back in `OpenAI`'s shape (#29 Q9, ruled): the assistant
/// message that made it carries `tool_calls`, with `content: null` when it
/// said nothing else, and the result is a `tool` message naming the call by
/// `tool_call_id` before its `content` -- the key order I0's capture sent
/// (`turn2-openai.request.json`), which T12 holds byte for byte. A message
/// with images sends `content` as an array of parts, images first; one with
/// none sends the plain string it always did, so no head moves.
fn message(message: &super::shape::Message, out: &mut String) {
    out.push_str("{\"role\":");
    string(message.role.tag(), out);
    if let Some(id) = &message.tool_call_id {
        out.push_str(",\"tool_call_id\":");
        string(id, out);
    }
    out.push_str(",\"content\":");
    if message.content.is_empty() && !message.tool_calls.is_empty() {
        out.push_str("null");
    } else if message.images.is_empty() {
        string(&message.content, out);
    } else {
        // The operator's attachments (#372): each image as an `image_url`
        // part with its data URI, in order, then the words as a `text` part
        // -- the shape #373's vision cell sent and the engine accepted.
        out.push('[');
        for image in &message.images {
            out.push_str("{\"type\":\"image_url\",\"image_url\":{\"url\":");
            string(&image.data_uri(), out);
            out.push_str("}},");
        }
        out.push_str("{\"type\":\"text\",\"text\":");
        string(&message.content, out);
        out.push_str("}]");
    }
    if let Some(reasoning) = &message.reasoning {
        out.push_str(",\"reasoning_content\":");
        string(reasoning, out);
    }
    if !message.tool_calls.is_empty() {
        out.push_str(",\"tool_calls\":[");
        for (index, call) in message.tool_calls.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str("{\"id\":");
            string(&call.id, out);
            out.push_str(",\"type\":\"function\",\"function\":{\"name\":");
            string(&call.name, out);
            out.push_str(",\"arguments\":");
            string(&call.arguments, out);
            out.push_str("}}");
        }
        out.push(']');
    }
    out.push('}');
}

/// Append `text` as a JSON string literal.
fn string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        if ch == '"' || ch == '\\' {
            out.push('\\');
            out.push(ch);
        } else if ch == '\n' {
            out.push_str("\\n");
        } else if ch == '\r' {
            out.push_str("\\r");
        } else if ch == '\t' {
            out.push_str("\\t");
        } else if (ch as u32) < 0x20 {
            // Infallible: the target is a String.
            let _ = write!(out, "\\u{:04x}", ch as u32);
        } else {
            out.push(ch);
        }
    }
    out.push('"');
}

/// Read `text` as a reply, through `dialect`'s declared paths.
///
/// # Errors
///
/// Returns [`WireError`] when the body is not a JSON object. Everything else
/// is absence, and absence is reported as absence rather than as an error:
/// a server that omits a field the dialect names is a fact this module
/// carries, not a failure it raises.
pub fn read(dialect: &Dialect, text: &str) -> Result<Reply, WireError> {
    let root: Value =
        serde_json::from_str(text).map_err(|why| WireError::NotJson(why.to_string()))?;
    if !root.is_object() {
        return Err(WireError::NotAnObject);
    }

    let mut reply = Reply {
        text: at(&root, "choices.0.message.content").and_then(as_text),
        finish_reason: dialect
            .finish_reason
            .as_deref()
            .and_then(|path| at(&root, path))
            .and_then(as_text),
        reasoning: dialect
            .reasoning
            .as_deref()
            .and_then(|path| at(&root, path))
            .and_then(as_text),
        timings: dialect
            .timings
            .as_deref()
            .and_then(|path| at(&root, path))
            .filter(|timings| timings.is_object())
            .map(Timings::read),
        output_tokens: at(&root, "usage.completion_tokens").and_then(as_count),
        prompt_tokens: dialect
            .prompt_tokens
            .as_deref()
            .and_then(|path| at(&root, path))
            .and_then(as_count),
        cached_tokens: dialect
            .cached_tokens
            .as_deref()
            .and_then(|path| at(&root, path))
            .and_then(as_count),
        ..Reply::default()
    };

    if let Some(path) = dialect.sampler_echo.as_deref() {
        reply.echo_site_declared = true;
        let site = if path.is_empty() {
            Some(&root)
        } else {
            at(&root, path)
        };
        if let Some(Value::Object(map)) = site {
            reply.echo_site_present = true;
            for (key, value) in map {
                reply.sampler_echo.insert(key.clone(), reported(value));
            }
        }
    }

    Ok(reply)
}

/// The value at a dotted path, where a numeric segment indexes an array.
fn at<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut here = root;
    for segment in path.split('.') {
        here = match here {
            Value::Object(map) => map.get(segment)?,
            Value::Array(items) => items.get(segment.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(here)
}

fn as_text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn as_count(value: &Value) -> Option<u64> {
    value.as_u64()
}

fn reported(value: &Value) -> Reported {
    match value {
        Value::Number(number) => Reported::Number(number.to_string()),
        Value::String(text) => Reported::Text(text.clone()),
        Value::Bool(flag) => Reported::Boolean(*flag),
        Value::Null => Reported::Other("null"),
        Value::Array(_) => Reported::Other("array"),
        Value::Object(_) => Reported::Other("object"),
    }
}

/// One spelling for a number, so that `0.60` and `0.6` are the same pin and
/// `0.6` and `0.7` are not.
///
/// Textual, never through a float. Returns `None` for a spelling this client
/// does not compare -- an exponent, a hex literal, anything that is not
/// digits with at most one point -- because "I could not read what the server
/// said" is a different answer from "the server disagreed", and treating the
/// first as the second would manufacture mismatches out of formatting.
#[must_use]
pub fn normalize(text: &str) -> Option<String> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };

    let (int_part, frac_part) = match digits.split_once('.') {
        Some((int_part, frac_part)) => (int_part, frac_part),
        None => (digits, ""),
    };
    if int_part.is_empty() && frac_part.is_empty() {
        return None;
    }
    if !int_part.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    if !frac_part.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }

    let int_trimmed = int_part.trim_start_matches('0');
    let frac_trimmed = frac_part.trim_end_matches('0');
    let int_out = if int_trimmed.is_empty() {
        "0"
    } else {
        int_trimmed
    };

    let zero = int_out == "0" && frac_trimmed.is_empty();
    let sign = if negative && !zero { "-" } else { "" };
    if frac_trimmed.is_empty() {
        Some(format!("{sign}{int_out}"))
    } else {
        Some(format!("{sign}{int_out}.{frac_trimmed}"))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::super::shape::{
        Dialect, Limits, Message, Pin, RequestShape, Role, SamplerCard, SamplerSetting,
    };
    use super::{Reported, WireError, body, normalize, read};

    fn shape(sampler: SamplerCard) -> RequestShape {
        RequestShape {
            model: "a-model".to_owned(),
            messages: vec![Message::new(Role::User, "hello")],
            sampler,
            limits: Limits {
                attempt: Duration::from_secs(1),
                call: Duration::from_secs(1),
                max_output_tokens: 64,
                retries: 0,
            },
            grammar: None,
            template_kwargs: std::collections::BTreeMap::new(),
            tools: Vec::new(),
        }
    }

    /// The pin that goes out is the digits that were pinned.
    ///
    /// The test that fails if anything on this path ever becomes an `f64`:
    /// `0.1` plus `0.2` is the famous case, but the one that matters here is
    /// quieter -- a regime pinned at `0.30000000000000004` is a different
    /// regime from one pinned at `0.3`, and nobody would ever see it happen.
    #[test]
    fn a_pinned_decimal_is_sent_as_the_digits_that_were_pinned() {
        let card = SamplerCard::empty()
            .with_decimal(SamplerSetting::Temperature, "0.6")
            .expect("0.6 is a decimal")
            .with_decimal(SamplerSetting::MinP, "0.05000")
            .expect("0.05000 is a decimal")
            .with(SamplerSetting::Seed, Pin::Integer(1_234_567));
        let rendered = body(&shape(card));

        assert!(rendered.contains("\"temperature\":0.6"), "{rendered}");
        assert!(
            rendered.contains("\"min_p\":0.05000"),
            "trailing zeros are part of the spelling that was pinned: {rendered}"
        );
        assert!(rendered.contains("\"seed\":1234567"), "{rendered}");
        assert!(
            !rendered.contains("0.6000"),
            "nothing on this path went through a float: {rendered}"
        );
    }

    #[test]
    fn an_assistant_messages_reasoning_goes_on_the_wire_as_it_came() {
        let mut answered = Message::new(Role::Assistant, "225.");
        // Untrimmed, trailing newline and all: the server's template renders
        // it back byte for byte, and a changed byte is a cold prefix (#117,
        // Q10).
        answered.reasoning = Some("We need the difference.\n".to_owned());
        let rendered = body(&RequestShape {
            messages: vec![
                Message::new(Role::User, "how long?"),
                answered,
                Message::new(Role::User, "and back?"),
            ],
            ..shape(SamplerCard::empty())
        });
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("the body is JSON");
        assert_eq!(
            parsed["messages"][1]["reasoning_content"],
            serde_json::json!("We need the difference.\n")
        );
        assert_eq!(parsed["messages"][1]["content"], serde_json::json!("225."));
        assert!(
            parsed["messages"][0].get("reasoning_content").is_none(),
            "a message with no reasoning carries no reasoning field: {rendered}"
        );
    }

    #[test]
    fn a_message_with_a_quote_or_a_newline_survives_the_round_trip() {
        let rendered = body(&RequestShape {
            messages: vec![Message::new(Role::User, "say \"hi\"\nthen stop\ttidily")],
            ..shape(SamplerCard::empty())
        });
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("the body is JSON");
        assert_eq!(
            parsed["messages"][0]["content"],
            serde_json::json!("say \"hi\"\nthen stop\ttidily")
        );
    }

    /// A template kwarg goes where a chat template reads it, and a sampler
    /// pin does not go there. #94 design point 4's other half: the control is
    /// a claim about what the server sent back, and this is the claim about
    /// what it was sent.
    #[test]
    fn a_template_kwarg_is_sent_in_the_object_the_template_reads_and_not_beside_the_sampler() {
        use crate::formats::record::json::Value;

        let nothing = body(&shape(SamplerCard::empty()));
        assert!(
            !nothing.contains("chat_template_kwargs"),
            "a request with nothing to say to the template says nothing: {nothing}"
        );

        let mut kwargs = std::collections::BTreeMap::new();
        kwargs.insert("enable_thinking".to_owned(), Value::Boolean(false));
        let rendered = body(&RequestShape {
            template_kwargs: kwargs,
            ..shape(SamplerCard::empty())
        });
        let parsed: serde_json::Value = serde_json::from_str(&rendered).expect("the body is JSON");
        assert_eq!(
            parsed["chat_template_kwargs"]["enable_thinking"],
            serde_json::json!(false)
        );
        // NOT at the top level, where the sampler pins live. A server reads
        // the two from different places, and a kwarg that landed beside
        // `temperature` would be a kwarg the template never sees -- which is
        // the exact delivery failure the negative control exists to catch.
        assert!(
            parsed.get("enable_thinking").is_none(),
            "a template kwarg is not a sampler pin: {rendered}"
        );
    }

    /// Reasoning is read from the path the dialect declares, and from nowhere
    /// else.
    ///
    /// The three answers are different facts: the field was there and said
    /// something, the field was there and said nothing, and nobody declared
    /// anywhere to look. A control that could not tell the third from the
    /// second would pass on any server whose reasoning field this client
    /// cannot find.
    #[test]
    fn reasoning_is_read_from_the_declared_path_and_absence_is_not_silence() {
        let reply = concat!(
            "{\"choices\":[{\"message\":{\"content\":\"ok\",",
            "\"reasoning_content\":\"weighing it up\"},\"finish_reason\":\"stop\"}]}"
        );
        let read_out = read(&Dialect::llama_cpp(), reply).expect("a reply");
        assert_eq!(read_out.reasoning.as_deref(), Some("weighing it up"));

        let silent = concat!(
            "{\"choices\":[{\"message\":{\"content\":\"ok\",",
            "\"reasoning_content\":\"\"},\"finish_reason\":\"stop\"}]}"
        );
        assert_eq!(
            read(&Dialect::llama_cpp(), silent)
                .expect("a reply")
                .reasoning
                .as_deref(),
            Some(""),
            "a server saying it thought about nothing is not a server that said nothing"
        );

        let undeclared = Dialect {
            reasoning: None,
            ..Dialect::llama_cpp()
        };
        assert_eq!(
            read(&undeclared, reply).expect("a reply").reasoning,
            None,
            "nobody looked, which is not the same as nothing being there"
        );
    }

    #[test]
    fn one_spelling_per_number_and_no_spelling_for_the_ones_we_do_not_compare() {
        assert_eq!(normalize("0.60").as_deref(), Some("0.6"));
        assert_eq!(normalize("0.6").as_deref(), Some("0.6"));
        assert_eq!(normalize("40.0").as_deref(), Some("40"));
        assert_eq!(normalize("040").as_deref(), Some("40"));
        assert_eq!(normalize("-0.0").as_deref(), Some("0"), "zero has no sign");
        assert_eq!(normalize("-0.5").as_deref(), Some("-0.5"));
        assert_eq!(normalize("+1.5").as_deref(), Some("1.5"));
        assert_eq!(normalize(".5").as_deref(), Some("0.5"));

        assert_ne!(
            normalize("0.6"),
            normalize("0.7"),
            "and it still discriminates"
        );

        // Not comparable, which is a different answer from disagreeing.
        assert_eq!(normalize("1e-3"), None);
        assert_eq!(normalize("warm"), None);
        assert_eq!(normalize(""), None);
        assert_eq!(normalize("0x10"), None);
        assert_eq!(normalize("1.2.3"), None);
    }

    /// One raw unstreamed reply from the `DoD` 1 instance (R3.0's C5,
    /// `e7051ef`, 2026-09-29): server-to-client bytes, nothing edited.
    const UNSTREAMED: &str =
        include_str!("../../client/fixtures/llama-server-e7051ef-unstreamed.http");

    #[test]
    fn the_llama_cpp_dialect_reads_the_cache_count_the_server_sends() {
        // #156, on the server's own bytes: `timings.cache_n` is there and
        // `timings.prompt_n_cached` is not.
        assert_eq!(
            crate::digest::sha256_hex(UNSTREAMED.as_bytes()),
            "d9d2e92ee5c067736d60da5baf56edc71e14c3de4a5965e1e7a89eaa895b4727",
            "the capture is the one R3.0 took, not one edited since"
        );
        let (_, body) = UNSTREAMED
            .split_once("\r\n\r\n")
            .expect("headers, then the body");
        assert!(!body.contains("prompt_n_cached"));
        let reply = read(&Dialect::llama_cpp(), body).expect("a readable reply");
        assert_eq!(reply.cached_tokens, Some(42));
        assert_eq!(reply.prompt_tokens, Some(72));
    }

    #[test]
    fn the_llama_cpp_chat_reply_has_no_sampler_echo_site() {
        // #159, on the server's own bytes (C5, pinned by digest in the
        // test above): the chat endpoint's reply carries no
        // `generation_settings`, so the dialect declares no echo site, and
        // a pin is "not reported by any site" rather than "absent from one".
        let (_, body) = UNSTREAMED
            .split_once("\r\n\r\n")
            .expect("headers, then the body");
        assert!(!body.contains("generation_settings"), "C5 carries no echo");
        let reply = read(&Dialect::llama_cpp(), body).expect("a readable reply");
        assert!(
            !reply.echo_site_declared,
            "the dialect claims an echo site C5 does not have"
        );
        assert!(reply.sampler_echo.is_empty());
    }

    #[test]
    fn a_declared_path_reads_what_is_there_and_nothing_when_it_is_not() {
        let reply = read(
            &Dialect::echoing(),
            "{\"choices\":[{\"message\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}],\
             \"usage\":{\"prompt_tokens\":9,\"completion_tokens\":2},\
             \"timings\":{\"cache_n\":7},\
             \"generation_settings\":{\"temperature\":0.6,\"grammar\":null}}",
        )
        .expect("a readable reply");

        assert_eq!(reply.text.as_deref(), Some("hi"));
        assert_eq!(reply.finish_reason.as_deref(), Some("stop"));
        assert_eq!(reply.prompt_tokens, Some(9));
        assert_eq!(reply.output_tokens, Some(2));
        assert_eq!(reply.cached_tokens, Some(7));
        assert!(reply.echo_site_declared && reply.echo_site_present);
        assert_eq!(
            reply.sampler_echo.get("temperature"),
            Some(&Reported::Number("0.6".to_owned())),
            "the number is kept as the server spelled it"
        );
        assert_eq!(
            reply.sampler_echo.get("grammar"),
            Some(&Reported::Other("null")),
            "a setting reported as null is a server saying something, not an absence"
        );
    }

    #[test]
    fn a_declared_echo_site_that_is_absent_is_declared_and_not_present() {
        let reply = read(
            &Dialect::echoing(),
            "{\"choices\":[{\"message\":{\"content\":\"hi\"}}]}",
        )
        .expect("a readable reply");

        assert!(
            reply.echo_site_declared,
            "the dialect names a place to look"
        );
        assert!(
            !reply.echo_site_present,
            "and the server did not put one there"
        );
        assert!(reply.sampler_echo.is_empty());
        assert_eq!(reply.cached_tokens, None);
    }

    #[test]
    fn a_body_that_is_not_a_json_object_is_an_error_rather_than_an_empty_reply() {
        assert!(matches!(
            read(&Dialect::llama_cpp(), "not json"),
            Err(WireError::NotJson(_))
        ));
        assert_eq!(
            read(&Dialect::llama_cpp(), "[1, 2]"),
            Err(WireError::NotAnObject)
        );
    }

    #[test]
    fn a_streaming_body_is_the_body_with_streaming_asked_for_and_the_same_head() {
        let shape = shape(SamplerCard::empty());
        let streaming = super::streaming_body(&shape);
        assert!(
            streaming.starts_with(&super::head(&shape)),
            "streaming moved the head, so a streamed request hashes as another prefix"
        );
        let plain = body(&shape);
        assert!(streaming.starts_with(plain.strip_suffix('}').unwrap()));
        let parsed: serde_json::Value =
            serde_json::from_str(&streaming).expect("the streaming body is JSON");
        assert_eq!(parsed["stream"], serde_json::Value::Bool(true));
        assert_eq!(
            parsed["stream_options"]["include_usage"],
            serde_json::Value::Bool(true)
        );
        // Prefill progress, asked for by the flag R3.0's C1 sent: the
        // capture-replay tests cannot notice a dropped flag, because what
        // they replay does not depend on what was asked.
        assert_eq!(parsed["return_progress"], serde_json::Value::Bool(true));
    }

    // -----------------------------------------------------------------------
    // the operator's attached image (#372, the loop half)
    // -----------------------------------------------------------------------

    /// The eight bytes every PNG begins with: enough for a test image, and
    /// generated here rather than committed.
    const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

    fn png_file(bytes: &[u8]) -> crate::formats::log::RecordedFile {
        let sha256 = crate::digest::sha256_hex(bytes);
        crate::formats::log::RecordedFile {
            path: format!("files/{sha256}"),
            sha256,
            media_type: "image/png".to_owned(),
            bytes: bytes.len() as u64,
        }
    }

    fn attached(text: &str, bytes: &[u8]) -> Message {
        crate::client::attach(Message::new(Role::User, text), &png_file(bytes), bytes)
            .expect("the bytes are the file's")
    }

    /// One image, exactly: `content` becomes an array, the image part first
    /// and the text part after it -- #373's accepted `[image_url, text]` --
    /// and nothing else in the body moves.
    #[test]
    fn an_attached_image_goes_out_as_an_image_part_then_the_text_byte_for_byte() {
        let rendered = body(&RequestShape {
            messages: vec![
                Message::new(Role::System, "sys"),
                attached("look", PNG_MAGIC),
            ],
            ..shape(SamplerCard::empty())
        });
        assert_eq!(
            rendered,
            concat!(
                r#"{"model":"a-model","messages":[{"role":"system","content":"sys"},"#,
                r#"{"role":"user","content":[{"type":"image_url","image_url":"#,
                r#"{"url":"data:image/png;base64,iVBORw0KGgo="}},"#,
                r#"{"type":"text","text":"look"}]}],"max_tokens":64}"#,
            )
        );
    }

    /// The head and the body go through one renderer: an attached image in
    /// the frozen part of the trunk is in the head, and so in
    /// `request.head_sha256`.
    #[test]
    fn an_attached_image_in_the_trunk_is_part_of_the_head() {
        let shape = RequestShape {
            messages: vec![
                Message::new(Role::System, "sys"),
                attached("look", PNG_MAGIC),
                Message::new(Role::Assistant, "a header"),
                Message::new(Role::User, "and?"),
            ],
            ..shape(SamplerCard::empty())
        };
        let head = super::head(&shape);
        assert!(body(&shape).starts_with(&head));
        assert!(
            head.contains("data:image/png;base64,iVBORw0KGgo="),
            "the image is not in the head: {head}"
        );
    }

    /// With no image, nothing changes: the bytes are what this renderer sent
    /// before images existed, pinned here, so every existing head digest
    /// still holds.
    #[test]
    fn a_message_with_no_image_renders_its_content_as_the_plain_string_it_always_was() {
        let rendered = body(&RequestShape {
            messages: vec![
                Message::new(Role::System, "sys"),
                Message::new(Role::User, "look"),
            ],
            ..shape(SamplerCard::empty())
        });
        assert_eq!(
            rendered,
            r#"{"model":"a-model","messages":[{"role":"system","content":"sys"},{"role":"user","content":"look"}],"max_tokens":64}"#
        );
    }

    /// The message #373's vision cell sent, and the engine accepted three
    /// times on the `TabbyAPI` line, is the message this renderer sends for
    /// the same image and the same words: byte for byte, as a substring of
    /// the request that went out.
    #[test]
    fn an_attached_message_is_the_one_373s_vision_cell_sent() {
        const IMAGE: &[u8] = include_bytes!(
            "../../../substrates/admission/accel24-beellama-qwen27b-q4kxl/7c254834cc2c/vision/image.png"
        );
        const SENT: &str = include_str!(
            "../../../substrates/admission/accel24-beellama-qwen27b-q4kxl/7c254834cc2c/vision/request.json"
        );
        let rendered = body(&RequestShape {
            messages: vec![attached(
                "What text appears in this image? Answer with the text only.",
                IMAGE,
            )],
            ..shape(SamplerCard::empty())
        });
        let message = rendered
            .strip_prefix(r#"{"model":"a-model","messages":["#)
            .and_then(|rest| rest.strip_suffix(r#"],"max_tokens":64}"#))
            .expect("one message");
        assert!(
            SENT.contains(&format!(r#""messages":[{message}]"#)),
            "not #373's message: {message}"
        );
    }
}
