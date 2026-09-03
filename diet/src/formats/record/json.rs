//! The value space a record line may hold.
//!
//! This is not a JSON library and must not become one. It is the subset the
//! `record` grammar defines -- no `null`, no binary floats, no exponents --
//! decoded into a shape the schema layer above can read. The grammar is
//! normative about what is accepted; this file is only about what the accepted
//! text means.

use std::collections::BTreeMap;
use std::fmt;

use pest::iterators::Pair;

use super::Rule;

/// A value a record may carry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A nested object, keys in sorted order.
    Object(BTreeMap<String, Value>),
    /// A list, in the order written.
    Array(Vec<Value>),
    /// Text, with JSON escapes decoded.
    String(String),
    /// A signed 64-bit integer.
    Integer(i64),
    /// An exact decimal, kept as the digits that were written.
    ///
    /// Not an `f64`. A banked number should read back as the number that was
    /// computed, and 0.142 through a double is 0.14199999999999999 -- a record
    /// that does that has lost the thing it was storing.
    Decimal(Decimal),
    /// `true` or `false`.
    Boolean(bool),
}

/// An exact decimal: the sign, the digits before the point, the digits after.
///
/// Comparison and rendering are textual on purpose. Two records of the same
/// run are the same bytes, and a value that has never been through a float
/// cannot have drifted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Decimal(String);

impl Decimal {
    /// The decimal as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a line's value space could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    /// The same key appears twice in one object. Which one wins is a question
    /// no reader should have to answer.
    DuplicateKey(String),
    /// An integer literal that does not fit in an `i64`.
    IntegerOutOfRange(String),
    /// A `\u` escape naming half a surrogate pair, or no character at all.
    BadEscape(String),
    /// The grammar produced a shape this file does not expect, which means
    /// the two have drifted apart.
    Shape(&'static str),
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateKey(key) => {
                write!(f, "the key `{key}` is bound twice in one object")
            }
            Self::IntegerOutOfRange(text) => {
                write!(f, "the integer {text} does not fit in an i64")
            }
            Self::BadEscape(text) => write!(f, "the escape {text} names no character"),
            Self::Shape(what) => write!(f, "grammar and parser disagree: {what}"),
        }
    }
}

/// Decode one `value` pair.
///
/// # Errors
///
/// Returns [`ValueError`] for the rules a PEG cannot state: duplicate keys,
/// integers outside `i64`, and `\u` escapes that name no character.
pub fn value(pair: &Pair<'_, Rule>) -> Result<Value, ValueError> {
    let inner = pair
        .clone()
        .into_inner()
        .next()
        .ok_or(ValueError::Shape("a value with no content"))?;
    match inner.as_rule() {
        Rule::object => object(&inner).map(Value::Object),
        Rule::array => inner
            .into_inner()
            .map(|item| value(&item))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Rule::string => string(&inner).map(Value::String),
        Rule::integer => inner
            .as_str()
            .parse::<i64>()
            .map(Value::Integer)
            .map_err(|_| ValueError::IntegerOutOfRange(inner.as_str().to_owned())),
        Rule::decimal => Ok(Value::Decimal(Decimal(inner.as_str().to_owned()))),
        Rule::boolean_true => Ok(Value::Boolean(true)),
        Rule::boolean_false => Ok(Value::Boolean(false)),
        _ => Err(ValueError::Shape("an unexpected rule inside a value")),
    }
}

/// Decode one `object` pair into its members.
///
/// # Errors
///
/// Returns [`ValueError::DuplicateKey`] if a key is bound twice, and whatever
/// [`value`] returns for its members.
pub fn object(pair: &Pair<'_, Rule>) -> Result<BTreeMap<String, Value>, ValueError> {
    let mut map = BTreeMap::new();
    for member in pair.clone().into_inner() {
        if member.as_rule() != Rule::member {
            return Err(ValueError::Shape("an unexpected rule inside an object"));
        }
        let mut parts = member.into_inner();
        let key = parts
            .next()
            .filter(|part| part.as_rule() == Rule::string)
            .ok_or(ValueError::Shape("a member with no key"))?;
        let key = string(&key)?;
        let val = parts
            .next()
            .filter(|part| part.as_rule() == Rule::value)
            .ok_or(ValueError::Shape("a member with no value"))?;
        // The grammar cannot say this: a PEG has no memory of what it already
        // matched. Two bindings of one key make the object's meaning depend on
        // which reader you ask.
        if map.insert(key.clone(), value(&val)?).is_some() {
            return Err(ValueError::DuplicateKey(key));
        }
    }
    Ok(map)
}

/// Decode a `string` pair, resolving escapes.
fn string(pair: &Pair<'_, Rule>) -> Result<String, ValueError> {
    let raw = pair
        .clone()
        .into_inner()
        .next()
        .filter(|inner| inner.as_rule() == Rule::string_inner)
        .ok_or(ValueError::Shape("a string with no inner run"))?;
    unescape(raw.as_str())
}

/// JSON string escapes, surrogate pairs included.
fn unescape(raw: &str) -> Result<String, ValueError> {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let code = chars
            .next()
            .ok_or(ValueError::Shape("a trailing backslash"))?;
        match code {
            '"' => out.push('"'),
            '\\' => out.push('\\'),
            '/' => out.push('/'),
            'b' => out.push('\u{8}'),
            'f' => out.push('\u{c}'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            'u' => out.push(code_point(&mut chars)?),
            _ => {
                return Err(ValueError::Shape(
                    "an escape the grammar should have rejected",
                ));
            }
        }
    }
    Ok(out)
}

/// One `\uXXXX` escape, joining a surrogate pair when it finds one.
fn code_point(chars: &mut std::str::Chars<'_>) -> Result<char, ValueError> {
    let high = hex4(chars)?;
    // A lone high surrogate is not a character. Joining the pair here rather
    // than letting one through keeps every `String` in a record valid UTF-8 by
    // construction.
    if (0xD800..0xDC00).contains(&high) {
        let mut lookahead = chars.clone();
        if lookahead.next() == Some('\\') && lookahead.next() == Some('u') {
            let low = hex4(&mut lookahead)?;
            if (0xDC00..0xE000).contains(&low) {
                *chars = lookahead;
                let joined = 0x1_0000 + ((high - 0xD800) << 10) + (low - 0xDC00);
                return char::from_u32(joined)
                    .ok_or_else(|| ValueError::BadEscape(format!("\\u{high:04X}\\u{low:04X}")));
            }
        }
        return Err(ValueError::BadEscape(format!("\\u{high:04X}")));
    }
    char::from_u32(high).ok_or_else(|| ValueError::BadEscape(format!("\\u{high:04X}")))
}

/// Exactly four hex digits, as a number.
fn hex4(chars: &mut std::str::Chars<'_>) -> Result<u32, ValueError> {
    let digits: String = chars.by_ref().take(4).collect();
    u32::from_str_radix(&digits, 16).map_err(|_| ValueError::BadEscape(format!("\\u{digits}")))
}

/// Render a value back to the record's own spelling.
///
/// One spelling per value, so that a record read and written back is the same
/// bytes. Keys come out sorted because [`Value::Object`] holds them sorted:
/// two records of the same run must not differ by the order a map iterated.
pub fn render(value: &Value, out: &mut String) {
    match value {
        Value::Object(members) => {
            out.push('{');
            for (index, (key, member)) in members.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                render_string(key, out);
                out.push(':');
                render(member, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                render(item, out);
            }
            out.push(']');
        }
        Value::String(text) => render_string(text, out),
        Value::Integer(number) => out.push_str(&number.to_string()),
        Value::Decimal(number) => out.push_str(number.as_str()),
        Value::Boolean(flag) => out.push_str(if *flag { "true" } else { "false" }),
    }
}

/// Escape a string the way the grammar accepts it back.
fn render_string(text: &str, out: &mut String) {
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            control if control < ' ' => {
                let code = control as u32;
                out.push_str("\\u00");
                out.push(char::from_digit(code >> 4, 16).unwrap_or('0'));
                out.push(char::from_digit(code & 0xf, 16).unwrap_or('0'));
            }
            other => out.push(other),
        }
    }
    out.push('"');
}
