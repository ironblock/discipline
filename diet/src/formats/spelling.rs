//! The boundary's two spellings (#583): `seam` through log v7, `meditation`
//! from v8.
//!
//! The reader stays closed: a log reads in the spelling its version declares
//! and in no other. A v7 log's old words are respelled to the current ones at
//! the positions that carry them, before anything else reads the line, and a
//! v7 log already in the new words is refused; a v8 log in the old words is
//! refused too. A projection is written back in the spelling the log
//! declared, so a committed log projects as it always did.
//!
//! The positions are the tagged keys that held "seam" in v7, each on the one
//! kind of line that carries it. Free text is never touched.

use std::collections::BTreeMap;

use super::record::json::Value;

/// The log version whose spelling is `meditation`.
pub const RESPELLED_IN: i64 = 8;

/// A respelled position: the line's kind (as v8 spells it; `None` for the
/// `kind` key itself), the key, and its old and new words.
const LOG: &[(Option<&str>, &str, &str, &str)] = &[
    (None, "kind", "seam", "meditation"),
    (
        Some("refused"),
        "command",
        "declare-seam",
        "declare-meditation",
    ),
    (
        Some("refused"),
        "because",
        "seam-not-built",
        "meditation-not-built",
    ),
    (
        Some("refused"),
        "because",
        "nothing-to-seam",
        "nothing-to-meditate",
    ),
    (Some("idle.gap"), "ended_by", "seam", "meditation"),
    (Some("fork"), "why", "seam", "meditation"),
    (Some("session.start"), "fork_delivery", "seam", "meditation"),
    (Some("phase.ruled"), "choice", "seam", "meditation"),
];

/// The words at `object`'s respelled positions, as (key, word, old, new):
/// the kind as it would read either way.
fn positions(
    object: &BTreeMap<String, Value>,
) -> impl Iterator<Item = (&'static str, &str, &'static str, &'static str)> + '_ {
    let kind = match object.get("kind") {
        Some(Value::String(kind)) => Some(kind.as_str()),
        _ => None,
    };
    let kind = kind.map(|kind| if kind == "seam" { "meditation" } else { kind });
    LOG.iter().filter_map(move |(on, key, old, new)| {
        if on.is_some() && *on != kind {
            return None;
        }
        match object.get(*key) {
            Some(Value::String(word)) if word == old || word == new => {
                Some((*key, word.as_str(), *old, *new))
            }
            _ => None,
        }
    })
}

/// `object`, a line of a log declaring `declared`, in the current spelling.
///
/// # Errors
///
/// A word in the other version's spelling: v8's in a log declaring v7 or
/// earlier, v7's in one declaring v8 or later.
pub fn to_current(object: &mut BTreeMap<String, Value>, declared: i64) -> Result<(), String> {
    let mut respelled = Vec::new();
    for (key, word, old, new) in positions(object) {
        if declared < RESPELLED_IN {
            if word == new {
                return Err(format!(
                    "`{key}` is `{new}`, v{RESPELLED_IN}'s spelling: a v{declared} log says `{old}`"
                ));
            }
            respelled.push((key, new));
        } else if word == old {
            return Err(format!(
                "`{key}` is `{old}`, the spelling before v{RESPELLED_IN}: a v{declared} log says \
                 `{new}`"
            ));
        }
    }
    for (key, new) in respelled {
        object.insert(key.to_owned(), Value::String(new.to_owned()));
    }
    Ok(())
}

/// `object`, a line read in the current spelling, written back as a log
/// declaring `declared` spells it.
pub fn to_declared(object: &mut BTreeMap<String, Value>, declared: i64) {
    if declared >= RESPELLED_IN {
        return;
    }
    let back: Vec<(&str, &str)> = positions(object)
        .filter(|(_, word, _, new)| word == new)
        .map(|(key, _, old, _)| (key, old))
        .collect();
    for (key, old) in back {
        object.insert(key.to_owned(), Value::String(old.to_owned()));
    }
}

/// The version a log's first line declares, read off its raw object: what
/// [`to_current`] is told before the line is read.
#[must_use]
pub fn declared(object: &BTreeMap<String, Value>) -> Option<i64> {
    match (object.get("kind"), object.get("version")) {
        (Some(Value::String(kind)), Some(Value::Integer(version))) if kind == "session.start" => {
            Some(*version)
        }
        _ => None,
    }
}
