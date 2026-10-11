//! Finding lines in a log (#757): `diet log find <log> key=value key~text`,
//! so every seq a run's README cites comes from a query, not from ad-hoc
//! `jq` or a script that reads the format a second way.
//!
//! A filter names a key of the line as [`super::log::project`] writes it --
//! the log's own spelling, so what it finds is what the file says -- a
//! dotted path reaching into an object (`entry.id`). `key=value` holds when
//! the key's value is exactly `value`: a string as written, any other value
//! as its JSON text. `key~text` holds when that text contains `text`.
//! Every filter must hold.

use super::record::json::Value;

/// One filter of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Filter {
    /// `key=value`: the key's value is exactly this.
    Is(String, String),
    /// `key~text`: the key's value contains this.
    Holds(String, String),
}

impl Filter {
    /// A filter from its spelling on the command line.
    ///
    /// # Errors
    ///
    /// A spelling with neither `=` nor `~`, or no key before it.
    pub fn parse(spelled: &str) -> Result<Self, String> {
        let at = spelled
            .find(['=', '~'])
            .ok_or_else(|| format!("`{spelled}` is no filter: write key=value or key~text"))?;
        let (key, rest) = spelled.split_at(at);
        if key.is_empty() {
            return Err(format!("`{spelled}` names no key"));
        }
        let value = rest[1..].to_owned();
        Ok(if rest.starts_with('=') {
            Self::Is(key.to_owned(), value)
        } else {
            Self::Holds(key.to_owned(), value)
        })
    }

    /// Whether `line` -- one projected event -- passes.
    #[must_use]
    pub fn passes(&self, line: &Value) -> bool {
        let (Self::Is(key, _) | Self::Holds(key, _)) = self;
        let Some(found) = key.split('.').try_fold(line, |value, part| match value {
            Value::Object(members) => members.get(part),
            _ => None,
        }) else {
            return false;
        };
        let text = match found {
            Value::String(text) => text.clone(),
            other => {
                let mut out = String::new();
                super::record::json::render(other, &mut out);
                out
            }
        };
        match self {
            Self::Is(_, value) => text == *value,
            Self::Holds(_, value) => text.contains(value.as_str()),
        }
    }
}

/// Every line of the log `source` that passes every filter, in order, as
/// the log's projection writes it.
///
/// # Errors
///
/// When `source` is not a log this reader reads.
pub fn find(source: &str, filters: &[Filter]) -> Result<Vec<Value>, String> {
    let projected = super::log::project(source)?;
    let Value::Object(mut projected) = projected else {
        return Err("a log projects to an object".to_owned());
    };
    let Some(Value::Array(events)) = projected.remove("events") else {
        return Err("a log projects its events".to_owned());
    };
    Ok(events
        .into_iter()
        .filter(|line| filters.iter().all(|filter| filter.passes(line)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{Filter, find};
    use crate::formats::record::json::Value;

    fn seqs(lines: &[Value]) -> Vec<i64> {
        lines
            .iter()
            .filter_map(|line| match line {
                Value::Object(members) => match members.get("seq") {
                    Some(Value::Integer(seq)) => Some(*seq),
                    _ => None,
                },
                _ => None,
            })
            .collect()
    }

    /// The committed levers-run-c log, which a run's README cites by seq.
    const RUN_C: &str =
        include_str!("../../../runs/2026-10-11-levers-run-c-agent-driven/session.log");

    /// #757: the README's cited seqs come back from queries: the forks a
    /// call triggered, the flag, the held meditation, the refusal.
    #[test]
    fn a_query_finds_the_seqs_a_run_cites() {
        let called = find(
            RUN_C,
            &[
                Filter::parse("kind=fork").expect("a filter"),
                Filter::parse("trigger~call:").expect("a filter"),
            ],
        )
        .expect("the log reads");
        assert_eq!(seqs(&called), [1346, 1994, 2343, 2946, 3230, 4269, 4705]);
        let one = |filter: &str| {
            seqs(&find(RUN_C, &[Filter::parse(filter).expect("a filter")]).expect("it reads"))
        };
        assert_eq!(one("kind=flag"), [5552]);
        assert_eq!(one("kind=meditation.held"), [10034]);
        assert_eq!(one("because=meditation-held"), [10035]);
        assert_eq!(one("refilled_anyway=true"), [10038]);
        assert_eq!(
            one("supersedes=interview-t1-f3970-0"),
            [5550],
            "a key reached by name, wherever the line keeps it"
        );
    }

    #[test]
    fn a_filter_reads_its_spelling_or_says_why_not() {
        assert_eq!(
            Filter::parse("kind=fork"),
            Ok(Filter::Is("kind".to_owned(), "fork".to_owned()))
        );
        assert_eq!(
            Filter::parse("entry.id~t1"),
            Ok(Filter::Holds("entry.id".to_owned(), "t1".to_owned()))
        );
        assert!(Filter::parse("kind").is_err());
        assert!(Filter::parse("=fork").is_err());
    }
}
