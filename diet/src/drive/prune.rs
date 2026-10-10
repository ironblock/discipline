//! Model-elected pruning (#612): a tool the model calls to have an earlier
//! tool result it no longer needs replaced, at a seam, by a pointer to its
//! whole.
//!
//! **No harness convention.** Pi, `OpenCode` 2 and Qwen Code give the model
//! no such tool; `OpenCode`'s prune and Qwen Code's microcompaction clear
//! old results themselves, rewriting earlier messages mid-conversation. So
//! the lever is this program's own, off by default, and its model-facing
//! words wait on the maintainer's approval before anyone turns it on.
//!
//! **The prefix rule holds** (#553): the trunk is never rewritten between
//! seams. The tool answers at once; the replacement is made at the next
//! seam (`prune_seam = "next"`, the default), or at a seam derived as soon
//! as the turn settles (`"turn_end"`), refused or deferred as any seam is.
//! At that seam a pruned output is carried as #596's `reference` line,
//! verbatim, whatever `seam_tool_outputs` says -- in the section when its
//! turn is compacted away, in place of the result when it sits in the kept
//! tail.
//!
//! **The handle is what the model wrote.** A chat template renders a call
//! as its name and arguments, not its id, so the model names the call by a
//! string argument it gave it -- `bash`'s command, `read`'s path -- matched
//! exactly; the most recent unpruned match is the one pruned.

use std::collections::BTreeMap;

use crate::client::shape::ToolDefinition;
use crate::formats::record::json::Value;
use crate::formats::regimen::{self, Regimen};

/// The tool's name.
pub const PRUNE_OUTPUT: &str = "prune_output";

/// The regimen key that offers the tool: `on` or `off` (the default).
pub const MODEL_PRUNING: &str = "model_pruning";

/// The regimen key for when a prune is applied: `next` (the default) or
/// `turn_end`.
pub const PRUNE_SEAM: &str = "prune_seam";

/// The tool's description. Draft model-facing text: nobody turns the lever
/// on until the maintainer approves it.
pub const DESCRIPTION: &str = "Remove an earlier tool result you no longer need from your \
     context. Its whole output is already saved; at the next context compaction it is replaced \
     by a one-line pointer giving its size, sha256 and path, which you can read again if you \
     need it. Use this for a large result whose content you are finished with. Name the call by \
     the command or path you gave it.";

/// The description of its one parameter, `call`.
pub const CALL_DESCRIPTION: &str = "The command or file path of the earlier call whose result \
     to remove, exactly as you wrote it. If several calls match, the most recent is pruned.";

/// When a prune is applied.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PruneSeam {
    /// At the next seam, whenever it fires.
    #[default]
    Next,
    /// At a seam derived as soon as the turn settles.
    TurnEnd,
}

impl PruneSeam {
    /// The regimen's word for it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::TurnEnd => "turn_end",
        }
    }

    /// What the tool answers a prune it took.
    #[must_use]
    pub fn answer(self) -> &'static str {
        match self {
            Self::Next => "pruned; replaced at the next context compaction",
            Self::TurnEnd => "pruned; replaced when this turn ends",
        }
    }
}

/// Whether `regimen` offers the tool, and when its prunes are applied:
/// `None` unless `model_pruning = "on"`. Read leniently: any other value of
/// either key is its default.
#[must_use]
pub fn of(regimen: &Regimen) -> Option<PruneSeam> {
    let word = |key: &str| match regimen.get(key) {
        Some(regimen::Value::String(word)) => Some(word.as_str()),
        _ => None,
    };
    (word(MODEL_PRUNING) == Some("on")).then(|| match word(PRUNE_SEAM) {
        Some("turn_end") => PruneSeam::TurnEnd,
        _ => PruneSeam::Next,
    })
}

/// The start row's word for the lever: `off`, or `on:next` / `on:turn_end`.
#[must_use]
pub fn lever(regimen: &Regimen) -> String {
    of(regimen).map_or_else(|| "off".to_owned(), |seam| format!("on:{}", seam.word()))
}

/// The tool, as a request declares it.
#[must_use]
pub fn definition() -> ToolDefinition {
    let text = |s: &str| Value::String(s.to_owned());
    let call = Value::Object(BTreeMap::from([
        ("description".to_owned(), text(CALL_DESCRIPTION)),
        ("type".to_owned(), text("string")),
    ]));
    ToolDefinition {
        name: PRUNE_OUTPUT.to_owned(),
        description: Some(DESCRIPTION.to_owned()),
        schema: Value::Object(BTreeMap::from([
            (
                "properties".to_owned(),
                Value::Object(BTreeMap::from([("call".to_owned(), call)])),
            ),
            ("required".to_owned(), Value::Array(vec![text("call")])),
            ("type".to_owned(), text("object")),
        ])),
    }
}

/// The `call` a prune's arguments name, when they parse and name one.
#[must_use]
pub fn target_of(arguments: &str) -> Option<String> {
    let parsed: serde_json::Value = serde_json::from_str(arguments).ok()?;
    let call = parsed.get("call")?.as_str()?;
    (!call.is_empty()).then(|| call.to_owned())
}

/// Whether an earlier call's `arguments` hold `call` as one of its string
/// values, exactly.
#[must_use]
pub fn names(arguments: &str, call: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(arguments)
        .ok()
        .and_then(|parsed| {
            parsed
                .as_object()
                .map(|fields| fields.values().any(|value| value.as_str() == Some(call)))
        })
        .unwrap_or(false)
}

/// The refusal for a prune naming no call, or one no earlier call matches.
#[must_use]
pub fn unmatched(call: &str) -> String {
    format!("{PRUNE_OUTPUT}: no earlier call matches `{call}`")
}

/// The refusal for a prune whose target a seam already compacted away.
pub const ALREADY_COMPACTED: &str = "prune_output: that result was already compacted away";

/// The refusal for a prune whose target is already pruned.
pub const ALREADY_PRUNED: &str = "prune_output: that result is already pruned";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lever_reads_leniently_and_is_off_by_default() {
        let read = |text: &str| {
            let parsed = regimen::parse(text).expect("a regimen");
            (of(&parsed), lever(&parsed))
        };
        assert_eq!(read("arm = \"a\"\n"), (None, "off".to_owned()));
        assert_eq!(read("model_pruning = \"yes\"\n"), (None, "off".to_owned()));
        assert_eq!(
            read("model_pruning = \"on\"\n"),
            (Some(PruneSeam::Next), "on:next".to_owned())
        );
        assert_eq!(
            read("model_pruning = \"on\"\nprune_seam = \"turn_end\"\n"),
            (Some(PruneSeam::TurnEnd), "on:turn_end".to_owned())
        );
        assert_eq!(
            read("model_pruning = \"on\"\nprune_seam = \"soon\"\n"),
            (Some(PruneSeam::Next), "on:next".to_owned())
        );
    }

    #[test]
    fn a_call_is_named_by_a_string_argument_exactly() {
        assert!(names(r#"{"command":"cat big.log"}"#, "cat big.log"));
        assert!(names(r#"{"path":"src/a.rs","limit":20}"#, "src/a.rs"));
        assert!(!names(r#"{"command":"cat big.log | head"}"#, "cat big.log"));
        assert!(!names(r#"{"limit":20}"#, "20"));
        assert!(!names("not json", "x"));
        assert_eq!(
            target_of(r#"{"call":"cat big.log"}"#).as_deref(),
            Some("cat big.log")
        );
        assert_eq!(target_of(r#"{"call":""}"#), None);
        assert_eq!(target_of(r#"{"other":"x"}"#), None);
    }
}
