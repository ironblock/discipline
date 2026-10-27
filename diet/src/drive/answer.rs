//! A scenario's answer tool (#971): a task answered through a tool with a
//! schema, never parsed out of prose.
//!
//! `serve --answer-tool FILE` reads one JSON object, `{"name",
//! "description", "parameters", "result"}`: the tool as the model is told
//! it, and the text a well-formed call is shown. Its words are the file's;
//! the engine writes only the reason a malformed call is refused. The tool
//! runs nothing: a call is checked against the schema's top level -- its
//! `required` keys present, each given key of the type its property names
//! -- and its arguments, on its `tool_call` line, are the answer a grade's
//! check reads.

use std::collections::BTreeMap;

use crate::client::shape::{RequestShape, ToolDefinition};
use crate::formats::record::json::{self, Value};

/// An answer tool, read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnswerTool {
    /// The tool as its requests declare it.
    pub definition: ToolDefinition,
    /// What the model is shown of a well-formed call.
    pub result: String,
}

impl AnswerTool {
    /// The tool `text` defines.
    ///
    /// # Errors
    ///
    /// Text that is not one JSON object of exactly `name`, `description`,
    /// `parameters` (an object) and `result`, each text but `parameters`; or
    /// one outside the record's value space (a `null`, an exponent).
    pub fn read(text: &str) -> Result<Self, String> {
        let parsed: serde_json::Value =
            serde_json::from_str(text).map_err(|why| format!("not JSON: {why}"))?;
        let mut members = json::line(&parsed.to_string()).map_err(|why| why.to_string())?;
        let mut text_of = |key: &str| match members.remove(key) {
            Some(Value::String(text)) if !text.is_empty() => Ok(text),
            _ => Err(format!("`{key}` is not text")),
        };
        let (name, description, result) = (
            text_of("name")?,
            text_of("description")?,
            text_of("result")?,
        );
        let Some(schema @ Value::Object(_)) = members.remove("parameters") else {
            return Err("`parameters` is not an object".to_owned());
        };
        if let Some(key) = members.keys().next() {
            return Err(format!("`{key}` is not a key an answer tool takes"));
        }
        Ok(Self {
            definition: ToolDefinition {
                name,
                description: Some(description),
                schema,
            },
            result,
        })
    }

    /// Why a call's `arguments` do not answer through this tool, in the words
    /// the model is shown; `None` when they do.
    #[must_use]
    pub fn fault(&self, arguments: &str) -> Option<String> {
        let refused = |why: &str| {
            Some(format!(
                "(the drive did not take this call: {why}, as the `{}` tool's parameters say.)\n",
                self.definition.name
            ))
        };
        let Ok(serde_json::Value::Object(given)) =
            serde_json::from_str::<serde_json::Value>(arguments)
        else {
            return refused("its arguments are not a JSON object");
        };
        let Value::Object(schema) = &self.definition.schema else {
            return None;
        };
        let empty = BTreeMap::new();
        let properties = match schema.get("properties") {
            Some(Value::Object(properties)) => properties,
            _ => &empty,
        };
        if let Some(Value::Array(required)) = schema.get("required") {
            for key in required {
                if let Value::String(key) = key
                    && !given.contains_key(key)
                {
                    return refused(&format!("it has no `{key}`"));
                }
            }
        }
        for (key, value) in &given {
            let declared = properties.get(key).and_then(|property| match property {
                Value::Object(property) => match property.get("type") {
                    Some(Value::String(kind)) => Some(kind.as_str()),
                    _ => None,
                },
                _ => None,
            });
            if let Some(kind) = declared
                && is_of(kind, value) == Some(false)
            {
                return refused(&format!("its `{key}` is not {}", article(kind)));
            }
        }
        None
    }
}

/// Whether `value` is of the schema's `kind`; `None` for a kind this check
/// does not know, which it leaves unchecked.
fn is_of(kind: &str, value: &serde_json::Value) -> Option<bool> {
    Some(match kind {
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        _ => return None,
    })
}

/// A schema's type word with its article: "a string", "an integer".
fn article(kind: &str) -> String {
    match kind {
        "integer" | "array" | "object" => format!("an {kind}"),
        _ => format!("a {kind}"),
    }
}

/// `shape`'s tools with `tool` after them, declared from the first request
/// and never changed.
///
/// # Errors
///
/// A tool already declared by the name it takes, named.
pub fn declare(shape: &mut RequestShape, tool: &AnswerTool) -> Result<(), String> {
    let name = &tool.definition.name;
    if shape.tools.iter().any(|declared| declared.name == *name) {
        return Err(format!(
            "the answer tool's name `{name}` is a tool the session already declares"
        ));
    }
    shape.tools.push(tool.definition.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOOL: &str = r#"{
  "name": "answer",
  "description": "Give your answer.",
  "parameters": {
    "type": "object",
    "properties": {
      "rule": {"type": "string"},
      "matches": {"type": "integer"},
      "files": {"type": "array"}
    },
    "required": ["rule", "matches"]
  },
  "result": "Answer recorded."
}"#;

    /// The file reads, whitespace and all, to its definition and result;
    /// a missing key, a stray one or a non-object schema is refused.
    #[test]
    fn an_answer_tool_reads_from_its_file_and_refuses_a_wrong_one() {
        let tool = AnswerTool::read(TOOL).expect("it reads");
        assert_eq!(tool.definition.name, "answer");
        assert_eq!(
            tool.definition.description.as_deref(),
            Some("Give your answer.")
        );
        assert_eq!(tool.result, "Answer recorded.");
        let Value::Object(schema) = &tool.definition.schema else {
            panic!("an object schema");
        };
        assert!(schema.contains_key("required"));
        for (broken, names) in [
            (
                r#"{"name":"a","description":"d","parameters":{}}"#,
                "result",
            ),
            (
                r#"{"name":"a","description":"d","parameters":[],"result":"r"}"#,
                "parameters",
            ),
            (
                r#"{"name":"a","description":"d","parameters":{},"result":"r","x":1}"#,
                "`x`",
            ),
            ("not json", "JSON"),
        ] {
            let why = AnswerTool::read(broken).expect_err(broken);
            assert!(why.contains(names), "{broken}: {why}");
        }
    }

    /// A call holding every required key, each of its type, is well formed;
    /// a missing or mistyped key, or arguments that are no object, is
    /// refused naming what is wrong.
    #[test]
    fn a_call_is_checked_against_the_schemas_top_level() {
        let tool = AnswerTool::read(TOOL).expect("it reads");
        assert_eq!(tool.fault(r#"{"rule":"x","matches":3}"#), None);
        assert_eq!(
            tool.fault(r#"{"rule":"x","matches":3,"files":["a"],"extra":true}"#),
            None
        );
        for (wrong, names) in [
            (r#"{"rule":"x"}"#, "no `matches`"),
            (
                r#"{"rule":"x","matches":"3"}"#,
                "`matches` is not an integer",
            ),
            (
                r#"{"rule":"x","matches":3.5}"#,
                "`matches` is not an integer",
            ),
            (r#"{"rule":7,"matches":3}"#, "`rule` is not a string"),
            (
                r#"{"rule":"x","matches":3,"files":"a"}"#,
                "`files` is not an array",
            ),
            ("[1]", "not a JSON object"),
            ("{", "not a JSON object"),
        ] {
            let why = tool.fault(wrong).expect(wrong);
            assert!(why.contains(names), "{wrong}: {why}");
        }
    }

    /// Each tool goes after the tools already declared, in the order given;
    /// a name one of them has -- another supplied tool's among them -- is
    /// refused, naming it.
    #[test]
    fn the_answer_tool_is_declared_last_and_never_over_another() {
        let tool = AnswerTool::read(TOOL).expect("it reads");
        let mut shape = crate::drive::session::tests::template();
        shape.tools.push(crate::drive::prune::definition());
        declare(&mut shape, &tool).expect("declared");
        let mut notes = tool.clone();
        notes.definition.name = "notes".to_owned();
        declare(&mut shape, &notes).expect("a second, by another name");
        assert_eq!(
            shape
                .tools
                .iter()
                .map(|tool| tool.name.as_str())
                .collect::<Vec<_>>(),
            ["prune_output", "answer", "notes"]
        );
        let why = declare(&mut shape, &tool).expect_err("a second `answer`");
        assert!(why.contains("`answer`"), "{why}");
    }
}
