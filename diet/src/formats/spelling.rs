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
    (None, "kind", "seam.held", "meditation.held"),
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
    (Some("refused"), "because", "seam-held", "meditation-held"),
    (Some("idle.gap"), "ended_by", "seam", "meditation"),
    (Some("fork"), "why", "seam", "meditation"),
    (Some("session.start"), "fork_delivery", "seam", "meditation"),
    (Some("phase.ruled"), "choice", "seam", "meditation"),
];

/// A renamed key (#705's, after the inventory): the line's kind, the key's
/// name before v8 and from it.
const LOG_KEYS: &[(&str, &str, &str)] = &[("fork", "after_seam", "after_meditation")];

/// The kind a line names, as v8 spells it.
fn current_kind(object: &BTreeMap<String, Value>) -> Option<&str> {
    let kind = match object.get("kind") {
        Some(Value::String(kind)) => kind.as_str(),
        _ => return None,
    };
    Some(
        LOG.iter()
            .find(|(on, key, old, _)| on.is_none() && *key == "kind" && *old == kind)
            .map_or(kind, |(_, _, _, new)| new),
    )
}

/// The words at `object`'s respelled positions, as (key, word, old, new):
/// the kind as it would read either way.
fn positions(
    object: &BTreeMap<String, Value>,
) -> impl Iterator<Item = (&'static str, &str, &'static str, &'static str)> + '_ {
    let kind = current_kind(object);
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

/// The levers whose names held the boundary's word, old and new (#737).
const LEVER_NAMES: &[(&str, &str)] = &[
    ("seam_trigger", "meditation_trigger"),
    ("seam_audit", "meditation_audit"),
    ("seam_warm", "meditation_warm"),
];

/// The lever words that held it: a lever's whole word, or a part of one.
const LEVER_WORDS: &[(&str, &str, &str)] = &[
    ("fork_delivery", "seam", "meditation"),
    ("tool_output_disposition", "+seam:", "+meditation:"),
];

/// Whether `word`, the lever `lever`'s state, holds `part` as its spelling
/// does: the whole word for `fork_delivery`, a part of it otherwise.
fn holds(lever: &str, word: &str, part: &str) -> bool {
    if lever == "fork_delivery" {
        word == part
    } else {
        word.contains(part)
    }
}

/// The `levers` of a start row or line, in the spelling `old` (true) or
/// current (false), refusing the other one when `strict`: a lever map is
/// read in its version's spelling like any other position (#737). `what`
/// names the version for a refusal.
fn levers_in(
    levers: &mut BTreeMap<String, Value>,
    from_old: bool,
    strict: Option<&str>,
) -> Result<(), String> {
    for (old, new) in LEVER_NAMES {
        let (from, to) = if from_old { (old, new) } else { (new, old) };
        if let Some(what) = strict
            && levers.contains_key(*to)
        {
            return Err(format!("its levers name `{to}`, which {what}"));
        }
        if let Some(value) = levers.remove(*from) {
            levers.insert((*to).to_owned(), value);
        }
    }
    for (lever, old, new) in LEVER_WORDS {
        let (from, to) = if from_old { (old, new) } else { (new, old) };
        if let Some(Value::String(word)) = levers.get_mut(*lever) {
            if let Some(what) = strict
                && holds(lever, word, to)
            {
                return Err(format!("its lever `{lever}` is `{word}`, which {what}"));
            }
            if holds(lever, word, from) {
                *word = word.replace(from, to);
            }
        }
    }
    Ok(())
}

/// A start's `levers`, read from a version spelling the old way or not
/// (#737): respelled to the current words when `old`, each refusing the
/// other version's form.
fn levers_to_current(
    object: &mut BTreeMap<String, Value>,
    old: bool,
    version: &str,
) -> Result<(), String> {
    let Some(Value::Object(levers)) = object.get_mut("levers") else {
        return Ok(());
    };
    if old {
        levers_in(
            levers,
            true,
            Some(&format!(
                "is the later spelling: {version} says the earlier"
            )),
        )
    } else {
        // In the current spelling already: refuse an old form, change nothing.
        let mut probe = levers.clone();
        levers_in(
            &mut probe,
            false,
            Some(&format!(
                "is the earlier spelling: {version} says the later"
            )),
        )
    }
}

/// A start's `levers`, written back in the old spelling (#737).
fn levers_to_declared(object: &mut BTreeMap<String, Value>) {
    if let Some(Value::Object(levers)) = object.get_mut("levers") {
        let _ = levers_in(levers, false, None);
    }
}

/// `object`, a line of a log declaring `declared`, in the current spelling.
///
/// # Errors
///
/// A word in the other version's spelling: v8's in a log declaring v7 or
/// earlier, v7's in one declaring v8 or later.
pub fn to_current(object: &mut BTreeMap<String, Value>, declared: i64) -> Result<(), String> {
    let kind = current_kind(object).map(str::to_owned);
    if kind.as_deref() == Some("session.start") {
        levers_to_current(
            object,
            declared < RESPELLED_IN,
            &format!("a v{declared} log"),
        )?;
    }
    for (on, old, new) in LOG_KEYS {
        if kind.as_deref() != Some(*on) {
            continue;
        }
        if declared < RESPELLED_IN {
            if object.contains_key(*new) {
                return Err(format!(
                    "`{on}` carries `{new}`, v{RESPELLED_IN}'s name: a v{declared} log names it `{old}`"
                ));
            }
            if let Some(value) = object.remove(*old) {
                object.insert((*new).to_owned(), value);
            }
        } else if object.contains_key(*old) {
            return Err(format!(
                "`{on}` carries `{old}`, the name before v{RESPELLED_IN}: a v{declared} log names it \
                 `{new}`"
            ));
        }
    }
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

/// `object`, a line read alone with no version to say how it spells the
/// boundary (#737), in the current spelling: each old word or key respelled,
/// a current one kept, nothing refused.
pub fn to_current_either(object: &mut BTreeMap<String, Value>) {
    let kind = current_kind(object).map(str::to_owned);
    for (on, old, new) in LOG_KEYS {
        if kind.as_deref() == Some(*on)
            && !object.contains_key(*new)
            && let Some(value) = object.remove(*old)
        {
            object.insert((*new).to_owned(), value);
        }
    }
    let respelled: Vec<(&str, &str)> = positions(object)
        .filter(|(_, word, old, _)| word == old)
        .map(|(key, _, _, new)| (key, new))
        .collect();
    for (key, new) in respelled {
        object.insert(key.to_owned(), Value::String(new.to_owned()));
    }
}

/// `object`, a line read in the current spelling, written back as a log
/// declaring `declared` spells it.
pub fn to_declared(object: &mut BTreeMap<String, Value>, declared: i64) {
    if declared >= RESPELLED_IN {
        return;
    }
    if current_kind(object) == Some("session.start") {
        levers_to_declared(object);
    }
    let kind = current_kind(object).map(str::to_owned);
    for (on, old, new) in LOG_KEYS {
        if kind.as_deref() == Some(*on)
            && let Some(value) = object.remove(*new)
        {
            object.insert((*old).to_owned(), value);
        }
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

/// The record version whose spelling is `meditation`: the first a record's
/// `start` names. A start naming none is version 1.
pub const RECORD_RESPELLED_IN: i64 = 2;

/// A record's respelled positions, as [`LOG`]'s: the row's kind (as v2
/// spells it; `None` for the `record` key itself), the key, the words.
const RECORD: &[(Option<&str>, &str, &str, &str)] = &[
    (None, "record", "seam", "meditation"),
    (Some("start"), "fork_delivery", "seam", "meditation"),
];

/// The version a record's row declares: its `start`'s `version`, or 1.
#[must_use]
pub fn record_declared(row: &BTreeMap<String, Value>) -> i64 {
    match row.get("version") {
        Some(Value::Integer(version)) => *version,
        _ => 1,
    }
}

/// The words at a record row's respelled positions, as [`positions`].
fn record_positions(
    row: &BTreeMap<String, Value>,
) -> impl Iterator<Item = (&'static str, &str, &'static str, &'static str)> + '_ {
    let kind = match row.get("record") {
        Some(Value::String(kind)) if kind == "seam" => Some("meditation"),
        Some(Value::String(kind)) => Some(kind.as_str()),
        _ => None,
    };
    RECORD.iter().filter_map(move |(on, key, old, new)| {
        if on.is_some() && *on != kind {
            return None;
        }
        match row.get(*key) {
            Some(Value::String(word)) if word == old || word == new => {
                Some((*key, word.as_str(), *old, *new))
            }
            _ => None,
        }
    })
}

/// `row`, of a record declaring `declared`, in the current spelling.
///
/// # Errors
///
/// A word in the other version's spelling.
pub fn record_to_current(row: &mut BTreeMap<String, Value>, declared: i64) -> Result<(), String> {
    if matches!(row.get("record"), Some(Value::String(kind)) if kind == "start") {
        levers_to_current(
            row,
            declared < RECORD_RESPELLED_IN,
            &format!("a version {declared} record"),
        )?;
    }
    let mut respelled = Vec::new();
    for (key, word, old, new) in record_positions(row) {
        if declared < RECORD_RESPELLED_IN {
            if word == new {
                return Err(format!(
                    "`{key}` is `{new}`, version {RECORD_RESPELLED_IN}'s spelling: a record of \
                     version {declared} says `{old}`"
                ));
            }
            respelled.push((key, new));
        } else if word == old {
            return Err(format!(
                "`{key}` is `{old}`, the spelling before version {RECORD_RESPELLED_IN}: a record \
                 of version {declared} says `{new}`"
            ));
        }
    }
    for (key, new) in respelled {
        row.insert(key.to_owned(), Value::String(new.to_owned()));
    }
    Ok(())
}

/// `row`, in the current spelling, written as a record of `declared` spells
/// it.
pub fn record_to_declared(row: &mut BTreeMap<String, Value>, declared: i64) {
    if declared >= RECORD_RESPELLED_IN {
        return;
    }
    if matches!(row.get("record"), Some(Value::String(kind)) if kind == "start") {
        levers_to_declared(row);
    }
    let back: Vec<(&str, &str)> = record_positions(row)
        .filter(|(_, word, _, new)| word == new)
        .map(|(key, _, old, _)| (key, old))
        .collect();
    for (key, old) in back {
        row.insert(key.to_owned(), Value::String(old.to_owned()));
    }
}

/// A record kind's tag as a record of `declared` spells it.
#[must_use]
pub fn record_kind_as(tag: &'static str, declared: i64) -> &'static str {
    if declared < RECORD_RESPELLED_IN && tag == "meditation" {
        "seam"
    } else {
        tag
    }
}
