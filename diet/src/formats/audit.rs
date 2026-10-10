//! The `audit` format, v0.
//!
//! The grammar at `diet/formats/audit/grammar.pest` is normative. This
//! module implements it and is the **one authorized implementation**: a
//! seam's audit (#504) reads its answer through here.
//!
//! An audit is the whole answer to the dogma's pinned audit ask, which
//! numbers the working notes 1..n and asks for one line per note: `KEEP`,
//! `UPDATE: <note>`, `REMOVE — <reason>` or `REMOVE — dup of #<k>`. There is
//! no `ADD`: the pinned asks offer none. The model judges; the grammar
//! formats; the seam folds. See the grammar for the four rules.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;

use pest::Parser as _;
use pest_derive::Parser;

use crate::formats::record::json::Value;

#[derive(Parser)]
#[grammar = "../formats/audit/grammar.pest"]
#[grammar = "../formats/number.pest"]
struct AuditParser;

/// What the audit said of one note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Judgment {
    /// The note stands.
    Keep,
    /// The note is replaced by this one.
    Update(String),
    /// The note no longer holds, for this reason.
    Remove(String),
    /// The note duplicates note `k` of the same ask.
    DupOf(u32),
}

/// One line of an audit: the note's number in the ask, and its judgment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// The note's number, from 1.
    pub number: u32,
    /// What became of it.
    pub judgment: Judgment,
}

/// Why an answer is not an audit.
#[derive(Debug)]
pub enum ParseError {
    /// The grammar refused it.
    Syntax(Box<pest::error::Error<Rule>>),
    /// The lines are not numbered 1..L, each once (rule 2).
    Numbering(String),
    /// A dup names no other line's note (rule 3).
    Dup(String),
    /// The parse tree was not the grammar's shape.
    Shape(&'static str),
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(err) => write!(f, "not an audit: {err}"),
            Self::Numbering(why) | Self::Dup(why) => write!(f, "not an audit: {why}"),
            Self::Shape(why) => write!(f, "not an audit: {why}"),
        }
    }
}

impl Error for ParseError {}

/// An audit's lines, in the order the answer wrote them.
///
/// # Errors
///
/// [`ParseError`] when the answer is not one verdict per line, its numbers
/// are not 1..L each once, or a dup names no other line's note.
pub fn parse(input: &str) -> Result<Vec<Line>, ParseError> {
    let mut document = AuditParser::parse(Rule::document, input)
        .map_err(|err| ParseError::Syntax(Box::new(err)))?;
    let document = document.next().ok_or(ParseError::Shape("no document"))?;
    let mut lines = Vec::new();
    for line in document.into_inner() {
        match line.as_rule() {
            Rule::line => {}
            Rule::EOI => continue,
            _ => return Err(ParseError::Shape("an unexpected rule inside the document")),
        }
        let mut inner = line.into_inner();
        let number = inner
            .next()
            .ok_or(ParseError::Shape("a line with no number"))?
            .as_str()
            .parse::<u32>()
            .map_err(|_| ParseError::Numbering("a number past u32".to_owned()))?;
        let verdict = inner
            .next()
            .ok_or(ParseError::Shape("a line with no verdict"))?;
        let text = |pair: pest::iterators::Pair<'_, Rule>| {
            pair.into_inner()
                .last()
                .map(|text| text.as_str().trim().to_owned())
                .ok_or(ParseError::Shape("a verdict with no text"))
        };
        let judgment = match verdict.as_rule() {
            Rule::keep => Judgment::Keep,
            Rule::update => Judgment::Update(text(verdict)?),
            Rule::remove => Judgment::Remove(text(verdict)?),
            Rule::dup => Judgment::DupOf(
                text(verdict)?
                    .parse::<u32>()
                    .map_err(|_| ParseError::Dup("a dup's number past u32".to_owned()))?,
            ),
            _ => return Err(ParseError::Shape("a verdict the grammar does not name")),
        };
        lines.push(Line { number, judgment });
    }
    let count = u32::try_from(lines.len()).map_err(|_| ParseError::Shape("too many lines"))?;
    let mut numbers = BTreeSet::new();
    for line in &lines {
        if !numbers.insert(line.number) {
            return Err(ParseError::Numbering(format!(
                "note {} is judged twice",
                line.number
            )));
        }
    }
    if let Some(missing) = (1..=count).find(|n| !numbers.contains(n)) {
        return Err(ParseError::Numbering(format!(
            "its {count} line(s) judge no note {missing}: they are numbered 1..{count}, each once"
        )));
    }
    for line in &lines {
        if let Judgment::DupOf(of) = line.judgment
            && (of == line.number || !numbers.contains(&of))
        {
            return Err(ParseError::Dup(format!(
                "line {} is a dup of #{of}, which is not another line's note",
                line.number
            )));
        }
    }
    Ok(lines)
}

/// The audit as the record's value space: its lines, each a number and a
/// verdict with its note, reason or the note it duplicates.
///
/// # Errors
///
/// As [`parse`], as a sentence.
pub fn project(source: &str) -> Result<Value, String> {
    let lines = parse(source).map_err(|err| err.to_string())?;
    Ok(Value::Object(BTreeMap::from([(
        "lines".to_owned(),
        Value::Array(
            lines
                .into_iter()
                .map(|line| {
                    let mut members =
                        BTreeMap::from([("n".to_owned(), Value::Integer(i64::from(line.number)))]);
                    let (verdict, detail) = match line.judgment {
                        Judgment::Keep => ("keep", None),
                        Judgment::Update(note) => ("update", Some(("note", Value::String(note)))),
                        Judgment::Remove(reason) => {
                            ("remove", Some(("reason", Value::String(reason))))
                        }
                        Judgment::DupOf(of) => ("dup", Some(("of", Value::Integer(i64::from(of))))),
                    };
                    members.insert("verdict".to_owned(), Value::String(verdict.to_owned()));
                    if let Some((key, value)) = detail {
                        members.insert(key.to_owned(), value);
                    }
                    Value::Object(members)
                })
                .collect(),
        ),
    )])))
}

#[cfg(test)]
mod tests {
    use super::{Judgment, parse};

    #[test]
    fn each_verdict_reads_and_case_is_not_significant() {
        let lines = parse(
            "1. KEEP\n2. update: the parser drops blank lines\n3. REMOVE — superseded by turn 4\n4. Remove - dup of #2\n",
        )
        .expect("an audit");
        let judgments: Vec<Judgment> = lines.into_iter().map(|line| line.judgment).collect();
        assert_eq!(
            judgments,
            [
                Judgment::Keep,
                Judgment::Update("the parser drops blank lines".to_owned()),
                Judgment::Remove("superseded by turn 4".to_owned()),
                Judgment::DupOf(2),
            ]
        );
    }

    #[test]
    fn prose_skipped_numbers_and_self_dups_are_refused() {
        for answer in [
            "Here is my audit:\n1. KEEP",
            "1. KEEP\n3. KEEP",
            "1. KEEP\n1. REMOVE - stale",
            "1. REMOVE - dup of #1",
            "1. KEEP\n2. REMOVE - dup of #3",
            "1. UPDATE:",
            "1. ADD: a new note",
            "",
        ] {
            assert!(parse(answer).is_err(), "{answer:?}");
        }
    }
}
