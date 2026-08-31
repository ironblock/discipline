//! The `regimen` format, v0.
//!
//! The grammar at `diet/formats/regimen/grammar.pest` is normative. This
//! module implements it and adds the two rules a PEG cannot state: a key may
//! appear at most once in a document, and an integer literal must fit in an
//! `i64`.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "../formats/regimen/grammar.pest"]
struct RegimenParser;

/// A scalar a regimen may hold in v0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A double-quoted string. v0 has no escapes, so the bytes between the
    /// quotes are the value.
    String(String),
    /// A signed 64-bit integer.
    Integer(i64),
    /// `true` or `false`.
    Boolean(bool),
}

/// A parsed regimen document.
///
/// Keys are held in sorted order so that two documents differing only in
/// key order are the same regimen. A regimen names a fixed combination of
/// variables; the order they were typed in is not one of them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Regimen {
    entries: BTreeMap<String, Value>,
}

impl Regimen {
    /// The value bound to `key`, if the document binds it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.get(key)
    }

    /// Every binding, in sorted key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// How many keys the document binds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the document binds no keys at all. An empty regimen is
    /// syntactically valid: a file of nothing but comments parses to one.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<'a> IntoIterator for &'a Regimen {
    type Item = (&'a str, &'a Value);
    type IntoIter = Box<dyn Iterator<Item = (&'a str, &'a Value)> + 'a>;

    fn into_iter(self) -> Self::IntoIter {
        Box::new(self.iter())
    }
}

/// Why a document is not a regimen.
#[derive(Debug)]
pub enum ParseError {
    /// The document does not match the grammar.
    Syntax(Box<pest::error::Error<Rule>>),
    /// The document binds the same key more than once.
    DuplicateKey {
        /// The key bound twice.
        key: String,
    },
    /// An integer literal is well-formed but does not fit in an `i64`.
    IntegerOutOfRange {
        /// The key the literal was bound to.
        key: String,
        /// The literal as it appeared in the document.
        literal: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "regimen syntax error: {err}"),
            Self::DuplicateKey { key } => write!(f, "duplicate key `{key}`"),
            Self::IntegerOutOfRange { key, literal } => {
                write!(
                    f,
                    "integer `{literal}` bound to `{key}` does not fit in i64"
                )
            }
        }
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Syntax(err) => Some(err),
            Self::DuplicateKey { .. } | Self::IntegerOutOfRange { .. } => None,
        }
    }
}

/// Parse `input` as a regimen document.
///
/// # Errors
///
/// Returns [`ParseError::Syntax`] if the input does not match the grammar,
/// [`ParseError::DuplicateKey`] if a key is bound twice, and
/// [`ParseError::IntegerOutOfRange`] if an integer literal overflows `i64`.
///
/// # Panics
///
/// Panics if the generated parser disagrees with the grammar file about the
/// shape of a successful parse. That is a build-time inconsistency between
/// `grammar.pest` and this module, not an input the caller can supply.
pub fn parse(input: &str) -> Result<Regimen, ParseError> {
    let file = RegimenParser::parse(Rule::file, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?
        .next()
        .expect("Rule::file yields exactly one pair on success");

    let mut entries = BTreeMap::new();
    for pair in file.into_inner() {
        if pair.as_rule() != Rule::pair {
            continue;
        }
        let (key, value) = binding(pair)?;
        match entries.entry(key) {
            Entry::Vacant(slot) => {
                slot.insert(value);
            }
            Entry::Occupied(slot) => {
                return Err(ParseError::DuplicateKey {
                    key: slot.key().clone(),
                });
            }
        }
    }
    Ok(Regimen { entries })
}

fn binding(pair: Pair<'_, Rule>) -> Result<(String, Value), ParseError> {
    let mut inner = pair.into_inner();
    let key = inner
        .next()
        .expect("a pair always starts with a key")
        .as_str()
        .to_owned();
    let raw = inner.next().expect("a pair always carries a value");

    let value = match raw.as_rule() {
        Rule::string => Value::String(
            raw.into_inner()
                .next()
                .expect("a string always wraps an inner")
                .as_str()
                .to_owned(),
        ),
        Rule::boolean => Value::Boolean(raw.as_str() == "true"),
        Rule::integer => {
            let literal = raw.as_str();
            Value::Integer(literal.parse().map_err(|_| ParseError::IntegerOutOfRange {
                key: key.clone(),
                literal: literal.to_owned(),
            })?)
        }
        other => unreachable!("grammar admits no other value rule: {other:?}"),
    };
    Ok((key, value))
}

#[cfg(test)]
mod tests {
    use super::{ParseError, Value, parse};

    #[test]
    fn parses_the_three_scalar_kinds() {
        let regimen = parse("arm = \"baseline\"\ndogma_version = 0\nreplay = false\n")
            .expect("document is a regimen");
        assert_eq!(regimen.len(), 3);
        assert_eq!(regimen.get("arm"), Some(&Value::String("baseline".into())));
        assert_eq!(regimen.get("dogma_version"), Some(&Value::Integer(0)));
        assert_eq!(regimen.get("replay"), Some(&Value::Boolean(false)));
    }

    #[test]
    fn key_order_is_not_part_of_the_regimen() {
        let one = parse("a = 1\nb = 2\n").expect("document is a regimen");
        let other = parse("b = 2\na = 1\n").expect("document is a regimen");
        assert_eq!(one, other);
    }

    #[test]
    fn a_comments_only_document_is_an_empty_regimen() {
        let regimen = parse("# nothing but this\n").expect("document is a regimen");
        assert!(regimen.is_empty());
    }

    #[test]
    fn a_duplicate_key_is_rejected() {
        let err = parse("arm = \"a\"\narm = \"b\"\n").expect_err("duplicate key");
        assert!(matches!(err, ParseError::DuplicateKey { key } if key == "arm"));
    }

    #[test]
    fn an_oversized_integer_is_rejected() {
        let err = parse("n = 99999999999999999999\n").expect_err("overflows i64");
        assert!(matches!(err, ParseError::IntegerOutOfRange { key, .. } if key == "n"));
    }

    #[test]
    fn v0_admits_no_tables() {
        assert!(matches!(
            parse("[section]\narm = \"a\"\n"),
            Err(ParseError::Syntax(_))
        ));
    }
}
