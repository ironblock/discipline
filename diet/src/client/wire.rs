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

/// What came back, read through the dialect's declared paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Reply {
    /// The answer's text, if the reply carried one. `Some("")` is an answer
    /// of nothing at all, which is a typed outcome and not an absence.
    pub text: Option<String>,
    /// Why the server stopped, as the server spells it.
    pub finish_reason: Option<String>,
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
#[must_use]
pub fn body(shape: &RequestShape) -> String {
    let mut out = String::from("{\"model\":");
    string(&shape.model, &mut out);
    out.push_str(",\"messages\":[");
    for (index, message) in shape.messages.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"role\":");
        string(message.role.tag(), &mut out);
        out.push_str(",\"content\":");
        string(&message.content, &mut out);
        out.push('}');
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

    #[test]
    fn a_declared_path_reads_what_is_there_and_nothing_when_it_is_not() {
        let reply = read(
            &Dialect::llama_cpp(),
            "{\"choices\":[{\"message\":{\"content\":\"hi\"},\"finish_reason\":\"stop\"}],\
             \"usage\":{\"prompt_tokens\":9,\"completion_tokens\":2},\
             \"timings\":{\"prompt_n_cached\":7},\
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
            &Dialect::llama_cpp(),
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
}
