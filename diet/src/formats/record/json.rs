//! The value space a record line may hold.
//!
//! This is not a JSON library and must not become one. It is the subset the
//! `record` grammar defines -- no `null`, no binary floats, no exponents --
//! decoded into a shape the schema layer above can read. The grammar is
//! normative about what is accepted; this file is only about what the accepted
//! text means.

use std::collections::BTreeMap;
use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;

use super::{RecordParser, Rule};

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
    /// A decimal from its digits, checked by asking the grammar to read them.
    ///
    /// The one way to make a decimal outside the parser. A number a caller
    /// computed -- a census's reduction, a metric -- reaches a record through
    /// here or not at all, so a spelling the grammar would refuse to read
    /// back is refused before it is written.
    ///
    /// What counts as a spelling is not restated here. This function ran a
    /// hand-written copy of the rule once, and the copy drifted: it refused
    /// `-0.0` as a second spelling of zero while the grammar still read it,
    /// so the same text was a decimal or not depending on which side of the
    /// format you asked. One format, one implementation -- so this parses
    /// the text with the very rule the reader uses, and takes it only if
    /// that rule consumed all of it.
    #[must_use]
    pub fn new(text: &str) -> Option<Self> {
        let matched = RecordParser::parse(Rule::decimal, text).ok()?.next()?;
        // A rule that matched a prefix has not read this text. `1.5.2` is not
        // a decimal for the same reason `1.5 apples` is not: something is
        // left over, and a decimal is the whole of what it is written as.
        (matched.as_str() == text).then(|| Self(text.to_owned()))
    }

    /// The decimal as written.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A decimal spelled the one way the grammar spells it: an optional sign,
    /// no leading zero, a point, at least one digit on each side of it.
    ///
    /// The only door from a number a program computed to a number a record
    /// holds. It takes text rather than an `f64` so that the caller has to
    /// choose how many digits the record keeps, and so that the values with no
    /// spelling here -- the infinities, and a result that is not a number --
    /// are refused rather than written.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let unsigned = text.strip_prefix('-').unwrap_or(text);
        let (whole, fraction) = unsigned.split_once('.')?;
        let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
        if !digits(whole) || !digits(fraction) {
            return None;
        }
        if whole != "0" && whole.starts_with('0') {
            return None;
        }
        // `-0.0` is a second spelling of `0.0`, and one spelling per value is
        // what makes two records of one run the same bytes.
        if text.starts_with('-') && whole == "0" && fraction.bytes().all(|b| b == b'0') {
            return None;
        }
        Some(Self(text.to_owned()))
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
        // Built directly, and that is not a bypass: this pair is what
        // `Rule::decimal` matched, and `Decimal::new` accepts exactly what
        // `Rule::decimal` matches. Routing it back through the constructor
        // would parse the same digits with the same rule a second time.
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

/// Why a line is not one object of the value space.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineError {
    /// The grammar rejected it.
    Syntax(String),
    /// Nested deeper than a record may be.
    TooDeep {
        /// How deep it went.
        depth: usize,
        /// How deep a record may go.
        limit: usize,
    },
    /// More than one line was handed in. A reader of lines reads one at a
    /// time, and silently taking the first would lose the rest.
    NotOneLine,
    /// The grammar accepted it and the value space did not.
    Value(ValueError),
}

impl fmt::Display for LineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(text) => write!(f, "not an object line: {text}"),
            Self::TooDeep { depth, limit } => write!(
                f,
                "nested {depth} deep where {limit} is the most a record allows"
            ),
            Self::NotOneLine => write!(f, "more than one line where one object was expected"),
            Self::Value(err) => write!(f, "{err}"),
        }
    }
}

impl std::error::Error for LineError {}

/// Decode one line of JSON Lines data -- one object -- through the record
/// grammar.
///
/// The one reader for data files that are not records: sense sets, registers,
/// vector caches. They share the record's value space on purpose, so that a
/// number in any of them is a [`Decimal`] and not a float, and they share its
/// grammar so that there is one parser to be wrong about what a line means.
///
/// # Errors
///
/// Returns [`LineError`] when the text is not exactly one object line of the
/// record grammar, or when its value space is rejected.
pub fn line(text: &str) -> Result<BTreeMap<String, Value>, LineError> {
    let depth = super::nesting_depth(text);
    if depth > super::MAX_DEPTH {
        return Err(LineError::TooDeep {
            depth,
            limit: super::MAX_DEPTH,
        });
    }
    let mut parsed = super::RecordParser::parse(Rule::event_line, text)
        .map_err(|err| LineError::Syntax(err.to_string()))?;
    let event_line = parsed.next().ok_or(LineError::Value(ValueError::Shape(
        "a line with no content",
    )))?;
    if event_line.as_span().end() != text.len() {
        return Err(LineError::NotOneLine);
    }
    let object_pair = event_line
        .into_inner()
        .find(|pair| pair.as_rule() == Rule::object)
        .ok_or(LineError::Value(ValueError::Shape("a line with no object")))?;
    object(&object_pair).map_err(LineError::Value)
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

#[cfg(test)]
mod tests {
    use super::{Decimal, LineError, Value, ValueError, line};
    use crate::formats::record::{Event, parse};

    // One reader for every data file that is not a record. A second reader is
    // a second opinion about what a number is.
    #[test]
    fn a_data_line_is_one_object_of_the_record_grammar() {
        let members = line(r#"{"text":"a b","vector":[0.5000,-0.2500]}"#).expect("one object");
        assert_eq!(members.len(), 2);
        assert_eq!(
            members.get("vector"),
            Some(&Value::Array(vec![
                Value::Decimal(Decimal::new("0.5000").expect("a decimal")),
                Value::Decimal(Decimal::new("-0.2500").expect("a decimal")),
            ]))
        );
        assert!(
            line("  {\"a\":1}  \n").is_ok(),
            "surrounding space and a newline are not content"
        );
        assert!(
            matches!(line("[1]"), Err(LineError::Syntax(_))),
            "a line that is not an object"
        );
        assert!(
            matches!(line("{\"a\":1} x"), Err(LineError::Syntax(_))),
            "trailing text"
        );
        assert_eq!(
            line("{\"a\":1}\n{\"b\":2}\n"),
            Err(LineError::NotOneLine),
            "two lines were read as one, and the second was lost"
        );
        assert_eq!(
            line(r#"{"a":1,"a":2}"#),
            Err(LineError::Value(ValueError::DuplicateKey("a".to_owned())))
        );
        let deep = format!("{}{}", "[".repeat(40), "]".repeat(40));
        assert!(matches!(
            line(&format!("{{\"a\":{deep}}}")),
            Err(LineError::TooDeep { .. })
        ));
        assert!(
            matches!(line("{\"a\":1.5e3}"), Err(LineError::Syntax(_))),
            "no exponents"
        );
    }

    // One spelling per value. A number that can be written two ways is a
    // record that can differ from itself.
    #[test]
    fn a_decimal_is_spelled_one_way() {
        for good in ["0.142", "-3.25", "10.0", "0.000100"] {
            assert_eq!(
                Decimal::new(good).map(|number| number.as_str().to_owned()),
                Some(good.to_owned()),
                "{good} is how the grammar spells a decimal"
            );
        }
        for bad in [
            "1", "01.5", "-0.0", "-0.00", ".5", "1.", "NaN", "inf", "1e3", "+1.0", "", "1.5 ",
        ] {
            assert!(
                Decimal::new(bad).is_none(),
                "{bad:?} was accepted as a decimal"
            );
        }
    }

    /// A record whose sampler carries `text` as a setting, so the grammar
    /// itself is the judge of the spelling.
    fn record_with(text: &str) -> String {
        format!(
            r#"{{"record":"start","regime":{{"arm":"baseline","dogma_version":0,"substrate":{{"name":"local","model":"a-model","quantization":"q4","sampler":{{"temperature":{text}}},"reasoning":"on","hardware":"one-gpu"}}}}}}"#
        )
    }

    // The constructor and the grammar must agree, in both directions: a
    // spelling the constructor accepts is one the grammar reads back as the
    // same digits, and a spelling it refuses is one the grammar refuses too.
    // Otherwise a computed number could be written that no reader accepts.
    #[test]
    fn a_constructed_decimal_is_one_the_grammar_reads_back() {
        for text in [
            "0.0",
            "0.142",
            "1.5",
            "-1.5",
            "12.000",
            "0.407",
            "-0.5",
            "1234567.89",
        ] {
            let made = Decimal::new(text).unwrap_or_else(|| panic!("{text} is a decimal"));
            assert_eq!(made.as_str(), text);
            let parsed = parse(&record_with(text)).unwrap_or_else(|err| panic!("{text}: {err}"));
            let Some(Event::Start { regime }) = parsed.events.first() else {
                panic!("a start row");
            };
            assert_eq!(
                regime.substrate.sampler.get("temperature"),
                Some(&Value::Decimal(made)),
                "{text}: the grammar read back a different value"
            );
        }
    }

    #[test]
    fn a_spelling_the_grammar_refuses_cannot_be_constructed() {
        for text in [
            "01.5", ".5", "1.", "1e5", "abc", "1.5.2", "", "-", "+1.5", "1,5", "-0.0", "-0.00",
            "-0.",
            // The negative branch spells "at least one digit after the point"
            // a second time, and only the positive copy was guarded.
            "-1.", "-10.",
        ] {
            assert!(
                Decimal::new(text).is_none(),
                "{text:?} was constructed, and the grammar would not read it back"
            );
            assert!(
                parse(&record_with(text)).is_err(),
                "{text:?} was refused by the constructor but read by the grammar: the two disagree"
            );
        }
    }

    // Every `Display` here was unexecuted, so a reason could say the opposite
    // of what happened with the gate still green.
    #[test]
    fn a_rejected_line_says_which_layer_rejected_it() {
        assert_eq!(
            LineError::NotOneLine.to_string(),
            "more than one line where one object was expected"
        );
        assert_eq!(
            LineError::TooDeep {
                depth: 40,
                limit: 32
            }
            .to_string(),
            "nested 40 deep where 32 is the most a record allows"
        );
        assert!(
            LineError::Syntax("x".to_owned())
                .to_string()
                .starts_with("not an object line")
        );
        assert_eq!(
            LineError::Value(ValueError::DuplicateKey("a".to_owned())).to_string(),
            ValueError::DuplicateKey("a".to_owned()).to_string()
        );
    }

    // A negative zero is refused, and a negative number that merely looks
    // like one is not: the digit that makes it negative may sit on either
    // side of the point.
    #[test]
    fn a_negative_zero_is_the_zero_that_is_already_spelled() {
        for text in ["-0.5", "-0.05", "-0.0000001", "-1.0", "-10.0"] {
            assert!(
                Decimal::new(text).is_some(),
                "{text} is a negative number, not a second spelling of zero"
            );
            assert!(parse(&record_with(text)).is_ok(), "{text}");
        }
    }
}
