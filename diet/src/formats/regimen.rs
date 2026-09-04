//! The `regimen` format, v1.
//!
//! The grammar at `diet/formats/regimen/grammar.pest` is normative. This
//! module implements it and adds the three rules a PEG cannot state: a key
//! may appear at most once in its own table, a table header may appear at
//! most once in a document, and an integer literal must fit in an `i64`.
//!
//! A float is held as the record's [`Decimal`] -- the digits as written --
//! and is built through [`Decimal::new`], which checks the text against the
//! record's own rule. That is the anti-drift device: the regimen grammar and
//! the record grammar both spell a decimal, and only one of them enforces it.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry;
use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest::iterators::Pair;
use pest_derive::Parser;

use crate::formats::record::json::Decimal;

#[derive(Parser)]
#[grammar = "../formats/regimen/grammar.pest"]
struct RegimenParser;

/// A value a regimen may hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A double-quoted string. There are no escapes, so the bytes between
    /// the quotes are the value.
    String(String),
    /// A signed 64-bit integer.
    Integer(i64),
    /// A float, kept as the exact decimal it was written as. A temperature
    /// through an `f64` is a different temperature, and a regime is
    /// identified by its settings.
    Float(Decimal),
    /// `true` or `false`.
    Boolean(bool),
    /// A `[header]` and the bindings under it. One level: a table holds
    /// scalars, never another table.
    Table(BTreeMap<String, Value>),
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
    /// The document binds the same key more than once in one table.
    DuplicateKey {
        /// The table the key was bound in, or `None` at the top level.
        table: Option<String>,
        /// The key bound twice.
        key: String,
    },
    /// The document opens the same table twice.
    DuplicateTable {
        /// The header that arrived a second time.
        name: String,
    },
    /// The document opens a table whose name a top-level key already holds.
    ///
    /// Its own variant rather than a second spelling of `DuplicateTable`:
    /// the two are different facts about the document, they are told apart
    /// by different fixtures, and a corpus whose reasons differ while its
    /// messages do not pins one case twice and the other never.
    TableShadowsKey {
        /// The header that collided with a binding.
        name: String,
    },
    /// A float the grammar accepted that the record's decimal constructor
    /// refuses.
    ///
    /// Unreachable while the two rules agree, and the reason it is an error
    /// rather than an `expect`: the regimen grammar spells a decimal and the
    /// record grammar enforces one, so a drift between them has to surface
    /// as a verdict on the document rather than as a panic in the reader.
    FloatNotADecimal {
        /// The key the literal was bound to.
        key: String,
        /// The literal as it appeared in the document.
        literal: String,
    },
    /// An integer literal is well-formed but does not fit in an `i64`.
    IntegerOutOfRange {
        /// The key the literal was bound to.
        key: String,
        /// The literal as it appeared in the document.
        literal: String,
    },
    /// The grammar produced a value rule the reader does not know.
    ///
    /// Unreachable against the grammar as written -- and it used to say so,
    /// with `unreachable!`. But the pairing is maintained by hand: adding a
    /// value rule to the `.pest` file and not here turned a document the
    /// grammar accepts into a panic, which is a crash where the caller asked
    /// for a verdict. Refusing it says the same thing and survives.
    UnexpectedRule {
        /// The key whose value could not be read.
        key: String,
        /// The rule the grammar produced, named as the grammar names it.
        rule: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "regimen syntax error: {err}"),
            Self::DuplicateKey { table: None, key } => write!(f, "duplicate key `{key}`"),
            Self::DuplicateKey {
                table: Some(table),
                key,
            } => write!(f, "duplicate key `{key}` in table `{table}`"),
            Self::DuplicateTable { name } => write!(f, "duplicate table `{name}`"),
            Self::TableShadowsKey { name } => write!(
                f,
                "table `{name}` has the name of a key the document already binds"
            ),
            Self::FloatNotADecimal { key, literal } => write!(
                f,
                "float `{literal}` bound to `{key}` is not a decimal the record \
                 would read back; the regimen grammar and the record grammar \
                 have drifted"
            ),
            Self::IntegerOutOfRange { key, literal } => {
                write!(
                    f,
                    "integer `{literal}` bound to `{key}` does not fit in i64"
                )
            }
            Self::UnexpectedRule { key, rule } => {
                write!(
                    f,
                    "the value bound to `{key}` parsed as `{rule}`, which this \
                     reader does not know how to read"
                )
            }
        }
    }
}

impl Error for ParseError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Syntax(err) => Some(err),
            Self::DuplicateKey { .. }
            | Self::DuplicateTable { .. }
            | Self::TableShadowsKey { .. }
            | Self::FloatNotADecimal { .. }
            | Self::IntegerOutOfRange { .. }
            | Self::UnexpectedRule { .. } => None,
        }
    }
}

/// Parse `input` as a regimen document.
///
/// # Errors
///
/// Returns [`ParseError::Syntax`] if the input does not match the grammar,
/// [`ParseError::DuplicateKey`] if a key is bound twice in one table,
/// [`ParseError::DuplicateTable`] if a header arrives twice,
/// [`ParseError::TableShadowsKey`] if a header takes a top-level key's name,
/// [`ParseError::IntegerOutOfRange`] if an integer literal
/// overflows `i64`, [`ParseError::FloatNotADecimal`] if the record's decimal
/// constructor refuses a float this grammar accepted, and
/// [`ParseError::UnexpectedRule`] if the grammar produced a value rule this
/// reader does not know.
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

    let mut entries: BTreeMap<String, Value> = BTreeMap::new();
    // Which table the pairs now arriving belong to. `None` until the first
    // header, which is the only place a top-level binding may sit -- as in
    // TOML, where a key after `[sampler]` is `sampler`'s and not the
    // document's.
    let mut table: Option<String> = None;
    for pair in file.into_inner() {
        match pair.as_rule() {
            Rule::table_header => {
                let name = pair
                    .into_inner()
                    .next()
                    .expect("a table header always carries a key")
                    .as_str()
                    .to_owned();
                // Either way the header would have to replace something,
                // and a regimen that quietly replaces a binding is a regime
                // that is not the one recorded -- but the two collisions are
                // separate facts and say so separately.
                match entries.get(&name) {
                    Some(Value::Table(_)) => return Err(ParseError::DuplicateTable { name }),
                    Some(_) => return Err(ParseError::TableShadowsKey { name }),
                    None => {}
                }
                entries.insert(name.clone(), Value::Table(BTreeMap::new()));
                table = Some(name);
            }
            Rule::pair => {
                let (key, value) = binding(pair)?;
                let scope = match &table {
                    None => &mut entries,
                    Some(name) => match entries.get_mut(name) {
                        Some(Value::Table(inner)) => inner,
                        _ => unreachable!("the open table was inserted as a table"),
                    },
                };
                match scope.entry(key) {
                    Entry::Vacant(slot) => {
                        slot.insert(value);
                    }
                    Entry::Occupied(slot) => {
                        return Err(ParseError::DuplicateKey {
                            table: table.clone(),
                            key: slot.key().clone(),
                        });
                    }
                }
            }
            _ => {}
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
        // On the rule, not on the text. A string arm compiles fine when a third
        // spelling is added to the grammar and quietly stops covering it; an
        // unknown RULE is refused below by name. The compiler is not the gate
        // here -- `Rule` is generated, so no arm is missing until the grammar
        // grows one -- but the failure is a verdict rather than a default.
        Rule::boolean_true => Value::Boolean(true),
        Rule::boolean_false => Value::Boolean(false),
        Rule::integer => {
            let literal = raw.as_str();
            Value::Integer(literal.parse().map_err(|_| ParseError::IntegerOutOfRange {
                key: key.clone(),
                literal: literal.to_owned(),
            })?)
        }
        // Built through the record's constructor, never from the text
        // directly: that constructor parses with the RECORD's decimal rule,
        // so the two grammars cannot drift without this failing loudly.
        Rule::float => {
            let literal = raw.as_str();
            Value::Float(
                Decimal::new(literal).ok_or_else(|| ParseError::FloatNotADecimal {
                    key: key.clone(),
                    literal: literal.to_owned(),
                })?,
            )
        }
        other => {
            return Err(ParseError::UnexpectedRule {
                key,
                rule: format!("{other:?}"),
            });
        }
    };
    Ok((key, value))
}

/// This document, as the record's value space.
///
/// # Errors
///
/// Returns the reason the text is not a regimen.
pub fn project(source: &str) -> Result<crate::formats::record::json::Value, String> {
    use crate::formats::record::json::Value as Json;
    parse(source)
        .map(|parsed| {
            let mut members = std::collections::BTreeMap::new();
            for (key, value) in parsed.iter() {
                members.insert(key.to_owned(), tagged(value));
            }
            Json::Object(members)
        })
        .map_err(|err| err.to_string())
}

/// One value, tagged with the kind the grammar read it as.
///
/// The tag is the point: a consumer reads `{"integer": 0}` and knows the
/// document said `0` and not `"0"`, which is the whole reason a regime is a
/// format rather than a bag of strings.
fn tagged(value: &Value) -> crate::formats::record::json::Value {
    use crate::formats::record::json::Value as Json;
    let (kind, held) = match value {
        Value::String(text) => ("string", Json::String(text.clone())),
        Value::Integer(number) => ("integer", Json::Integer(*number)),
        Value::Float(number) => ("float", Json::Decimal(number.clone())),
        Value::Boolean(flag) => ("boolean", Json::Boolean(*flag)),
        Value::Table(inner) => (
            "table",
            Json::Object(
                inner
                    .iter()
                    .map(|(key, held)| (key.clone(), tagged(held)))
                    .collect(),
            ),
        ),
    };
    Json::Object(std::collections::BTreeMap::from([(kind.to_owned(), held)]))
}

#[cfg(test)]
mod tests {
    use super::{Decimal, ParseError, Value, parse};

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
        assert!(
            matches!(err, ParseError::DuplicateKey { table: None, key } if key == "arm"),
            "a duplicate at the top level is not a duplicate inside a table"
        );
    }

    #[test]
    fn an_oversized_integer_is_rejected() {
        let err = parse("n = 99999999999999999999\n").expect_err("overflows i64");
        assert!(matches!(err, ParseError::IntegerOutOfRange { key, .. } if key == "n"));
    }

    #[test]
    fn a_table_scopes_the_keys_that_follow_it() {
        // The length first, and with a message: a reader that opened the
        // table and then bound its keys at the top level anyway would leave
        // an empty `sampler` beside a stray `temperature`, which is three
        // bindings where the document names two.
        let one = parse("arm = \"baseline\"\n\n[sampler]\ntemperature = 0.6\n")
            .expect("document is a regimen");
        assert_eq!(
            one.len(),
            2,
            "a table is one binding of the document, and its keys are not the document's"
        );

        let regimen = parse(
            "arm = \"baseline\"\nseed = 1\n\n[sampler]\nseed = 7\ntemperature = 0.6\n\n[limits]\nwall_seconds = 1800\n",
        )
        .expect("document is a regimen");

        // `seed` is bound at the top level and again under `[sampler]`, and
        // the two are different bindings. A reader that flattened the
        // document would have reported a duplicate, or kept one of them.
        assert_eq!(regimen.get("seed"), Some(&Value::Integer(1)));
        let Some(Value::Table(sampler)) = regimen.get("sampler") else {
            panic!("`[sampler]` did not become a table");
        };
        assert_eq!(sampler.get("seed"), Some(&Value::Integer(7)));
        assert_eq!(
            sampler.get("temperature"),
            Some(&Value::Float(
                Decimal::new("0.6").expect("0.6 is a decimal")
            ))
        );
        let Some(Value::Table(limits)) = regimen.get("limits") else {
            panic!("`[limits]` did not become a table");
        };
        assert_eq!(limits.get("wall_seconds"), Some(&Value::Integer(1800)));
        assert_eq!(regimen.len(), 4, "arm, seed, sampler, limits");
    }

    /// A float reaches the value space as a NUMBER. Rendering `0.6` as the
    /// string `"0.6"` loses the one thing the type carries -- a consumer
    /// then cannot tell a temperature from a label that reads like one --
    /// and it is the exact lie the ruling that added floats refused.
    #[test]
    fn a_float_projects_as_a_number_and_not_as_its_digits_in_quotes() {
        use crate::formats::record::json::Value as Json;
        let projected = super::project("temperature = 0.6\n").expect("document is a regimen");
        let Json::Object(members) = &projected else {
            panic!("a regimen projects as an object");
        };
        let Some(Json::Object(tagged)) = members.get("temperature") else {
            panic!("`temperature` is missing from the projection");
        };
        assert!(
            matches!(tagged.get("float"), Some(Json::Decimal(_))),
            "a float projected as something other than a decimal: {tagged:?}"
        );
    }

    #[test]
    fn a_table_may_not_arrive_twice_or_take_a_key_s_name() {
        assert!(
            matches!(
                parse("[sampler]\na = 1\n[sampler]\nb = 2\n"),
                Err(ParseError::DuplicateTable { name }) if name == "sampler"
            ),
            "a table opened twice was not refused"
        );
        assert!(
            matches!(
                parse("sampler = 1\n[sampler]\na = 2\n"),
                Err(ParseError::TableShadowsKey { name }) if name == "sampler"
            ),
            "a table opened twice was not refused: it took a key's name"
        );
        assert!(matches!(
            parse("[sampler]\na = 1\na = 2\n"),
            Err(ParseError::DuplicateKey { table: Some(table), key }) if table == "sampler" && key == "a"
        ));
    }

    /// A temperature is the digits it was written as, through and back.
    ///
    /// This is the whole reason a float is a `Decimal` and not an `f64`:
    /// `0.6` through a double is `0.59999999999999998`, and a regime
    /// identified by its settings would then be a different regime on the
    /// way out than it was on the way in.
    #[test]
    fn a_float_round_trips_byte_exact() {
        for literal in ["0.6", "0.95", "1.0", "0.000", "-0.142", "12.750", "0.1"] {
            let regimen = parse(&format!("t = {literal}\n")).expect("document is a regimen");
            let Some(Value::Float(held)) = regimen.get("t") else {
                panic!("{literal} did not read as a float");
            };
            assert_eq!(held.as_str(), literal, "{literal} did not survive the read");
        }
    }

    /// The regimen grammar spells a decimal and the RECORD's grammar
    /// enforces one. Two rules that must agree, so a table of literals is
    /// driven through both and they must give the same verdict.
    ///
    /// It asks the GRAMMAR, not `parse`. Asking `parse` proves nothing about
    /// the grammar: `Decimal::new` refuses anything the record would not
    /// read, so a widened `float` rule turns into a `FloatNotADecimal` on
    /// the same documents and every corpus fixture stays rejected. That is
    /// the parser covering for the normative file -- the same shape as a
    /// `ParseError::Shape` standing in for a grammar refusal -- and it is
    /// what this test is placed one level down to see.
    #[test]
    fn the_float_rule_and_the_records_decimal_rule_agree() {
        fn the_grammar_reads_a_float(literal: &str) -> bool {
            use pest::Parser as _;
            super::RegimenParser::parse(super::Rule::float, literal)
                .ok()
                .and_then(|mut pairs| pairs.next())
                // A rule that matched a prefix has not read this text.
                .is_some_and(|matched| matched.as_str() == literal)
        }

        for literal in [
            "0.6", "0.0", "-0.1", "10.25", "0.000", "-12.5", "1.0", "0.10",
            // and the spellings both must refuse
            "-0.0", "-0.00", "7e-1", "1.", ".5", "01.5", "+1.5", "1.5.2", "1_0.5", "0x1.5",
        ] {
            assert_eq!(
                the_grammar_reads_a_float(literal),
                Decimal::new(literal).is_some(),
                "`{literal}`: the regimen grammar and the record's decimal disagree"
            );
        }
    }

    /// No document the corpus holds may be refused by
    /// [`ParseError::FloatNotADecimal`]. That error exists so a drift
    /// between the two grammars surfaces instead of panicking, and reaching
    /// it means the drift has already happened.
    #[test]
    fn no_invalid_fixture_is_refused_for_a_drift_between_the_grammars() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("formats/regimen/fixtures/invalid");
        let mut checked = 0;
        for entry in std::fs::read_dir(&dir).expect("the invalid fixture directory") {
            let path = entry.expect("a readable entry").path();
            if path.extension().is_none_or(|ext| ext != "toml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            checked += 1;
            assert!(
                !matches!(parse(&text), Err(ParseError::FloatNotADecimal { .. })),
                "{}: refused because the regimen grammar and the record's decimal \
                 have drifted, not because the document is wrong",
                path.display()
            );
        }
        assert!(checked >= 20, "only {checked} invalid fixtures were read");
    }
}
