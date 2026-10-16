//! The refutation interview's tool (#878): `refute_beliefs`, the one way a
//! refutation fork answers, and the trunk's way to say a belief was wrong.
//!
//! Its definition is one JSON object under `diet/dogma/refutation/`: its
//! name, its description and its parameters' schema, sent as written. The
//! file is pinned in `MANIFEST.tsv` as `refutation/tool`, as the
//! self-capture contracts are pinned (`super::capture`).

use super::digest;

/// The definition, exactly as pinned.
pub const TOOL: &str = include_str!("../../dogma/refutation/tool.json");

/// The tool's name, as the definition gives it.
pub const NAME: &str = "refute_beliefs";

/// The definition's manifest line, as `MANIFEST.tsv` writes it.
#[must_use]
pub fn manifest_line() -> String {
    format!("refutation/tool\t{}\t{}\n", digest(TOOL), TOOL.len())
}

/// The definition as a request declares it.
///
/// # Panics
///
/// If the pinned definition does not read, which this module's tests hold
/// it always does.
#[must_use]
pub fn definition() -> crate::client::shape::ToolDefinition {
    use crate::formats::record::json::Value;
    let mut members =
        crate::formats::record::json::line(TOOL.trim_end()).expect("the pinned definition reads");
    let text = |value: Option<Value>| match value {
        Some(Value::String(text)) => text,
        other => panic!("the pinned definition's text is a string: {other:?}"),
    };
    crate::client::shape::ToolDefinition {
        name: text(members.remove("name")),
        description: Some(text(members.remove("description"))),
        schema: members
            .remove("parameters")
            .expect("the pinned definition has parameters"),
    }
}

#[cfg(test)]
mod tests {
    use super::{NAME, definition, manifest_line};
    use crate::dogma::MANIFEST;

    /// The definition matches its manifest line, and the manifest pins no
    /// other `refutation/` file.
    #[test]
    fn the_definition_matches_its_manifest_line_and_nothing_else_does() {
        let pinned: Vec<String> = MANIFEST
            .lines()
            .filter(|line| line.starts_with("refutation/"))
            .map(|line| format!("{line}\n"))
            .collect();
        assert_eq!(pinned, [manifest_line()]);
    }

    /// Every file on disk under `refutation/` is the definition.
    #[test]
    fn the_definition_is_the_only_file() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("dogma/refutation");
        let files: Vec<String> = std::fs::read_dir(&dir)
            .expect("the directory")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .into_string()
                    .expect("a name")
            })
            .collect();
        assert_eq!(files, ["tool.json"]);
    }

    /// It reads: its name, a description, and an object schema whose one
    /// required key, `refuted`, is a list of items whose `confidence` is an
    /// integer from 1 to 10 and whose required keys are the three every item
    /// carries.
    #[test]
    fn the_definition_reads() {
        use crate::formats::record::json::Value;
        let strings = |keys: &[&str]| {
            Value::Array(
                keys.iter()
                    .map(|key| Value::String((*key).to_owned()))
                    .collect(),
            )
        };
        let tool = definition();
        assert_eq!(tool.name, NAME);
        assert!(tool.description.is_some_and(|text| !text.is_empty()));
        let Value::Object(schema) = &tool.schema else {
            panic!("{:?}", tool.schema);
        };
        assert_eq!(schema.get("required"), Some(&strings(&["refuted"])));
        let Some(Value::Object(properties)) = schema.get("properties") else {
            panic!("{schema:?}");
        };
        let Some(Value::Object(refuted)) = properties.get("refuted") else {
            panic!("{properties:?}");
        };
        assert_eq!(
            refuted.get("type"),
            Some(&Value::String("array".to_owned()))
        );
        let Some(Value::Object(item)) = refuted.get("items") else {
            panic!("{refuted:?}");
        };
        let Some(Value::Object(fields)) = item.get("properties") else {
            panic!("{item:?}");
        };
        let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
        assert_eq!(keys, ["because", "claim", "confidence", "note_id", "now"]);
        let Some(Value::Object(confidence)) = fields.get("confidence") else {
            panic!("{fields:?}");
        };
        assert_eq!(confidence.get("minimum"), Some(&Value::Integer(1)));
        assert_eq!(confidence.get("maximum"), Some(&Value::Integer(10)));
        assert_eq!(
            item.get("required"),
            Some(&strings(&["confidence", "because", "now"]))
        );
    }
}
