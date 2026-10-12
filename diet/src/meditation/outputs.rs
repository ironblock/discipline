//! What a meditation's refill carries of the tool outputs it compacts away (#553):
//! the outputs in the turns before the kept tail (#552), under the
//! regimen's [`MeditationToolOutputs`] state.
//!
//! The section follows the render, in the refill's system message, as Pi
//! appends its file lists to its summary and `OpenCode` 2's summary carries
//! its "Relevant Files": both keep tool work inside the compacted context.
//! They differ on its shape (Pi's tagged lists, `OpenCode`'s model-written
//! section), so Qwen Code's post-compaction block breaks the tie: a sentence
//! saying what follows, then one `- turn N: <tool> args=<json>` line per
//! call. A result is labelled `[Tool result]:`, the label Pi's and
//! `OpenCode` 2's compaction both serialize one with.
//!
//! The section is logged on the meditation line as sent, as the render is, so
//! the projection rebuilds the refill from the log whatever the state.

use crate::formats::log::MeditationToolOutputs;

/// One output a meditation compacts away, as the session gathered it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The turn its call was made in.
    pub turn: u32,
    /// The tool's name.
    pub name: String,
    /// Its arguments, as the model wrote them.
    pub arguments: String,
    /// What the trunk carried as its result: the output after the cap.
    pub shown: String,
    /// The media types of the images its result carried (#557).
    pub images: Vec<String>,
    /// Its whole, saved by digest: for `reference`.
    pub saved: Option<Saved>,
    /// The verbatim excerpts the read fork quoted from it: for `salient`.
    pub excerpts: Vec<String>,
    /// Its reference line, when the model pruned it (#612): carried in
    /// its place whatever the state.
    pub pruned: Option<String>,
}

/// An output's whole, kept by digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// Its length in bytes.
    pub bytes: u64,
    /// Its sha256.
    pub sha256: String,
    /// Where it was saved, when the session keeps a recording.
    pub path: Option<String>,
}

/// The section `state` carries of `outputs`, in call order, and how many
/// outputs it carried; `None` when it carries none -- always under `evict`,
/// and under `salient` when no fork quoted any of them.
#[must_use]
pub fn section(state: MeditationToolOutputs, outputs: &[Output]) -> Option<(String, u64)> {
    // A pruned output is its reference line whatever the state (#612).
    let entries: Vec<String> = outputs
        .iter()
        .filter_map(|output| match (&output.pruned, state) {
            (Some(line), _) => Some(line.clone()),
            (None, MeditationToolOutputs::Evict) => None,
            (None, MeditationToolOutputs::Keep) => Some(kept(output)),
            (None, MeditationToolOutputs::Reference) => Some(reference_line(output)),
            (None, MeditationToolOutputs::Salient) => salient(output),
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    // Every entry a reference line -- the state's, or a prune's under
    // `evict` -- is introduced as reference.
    let referenced_only = state == MeditationToolOutputs::Reference
        || (state == MeditationToolOutputs::Evict && outputs.iter().any(|o| o.pruned.is_some()));
    let sentence = match state {
        _ if referenced_only => {
            "The following tool results were produced before context was compacted. They are \
             listed as reference only: each was saved whole, and can be read at its path."
        }
        MeditationToolOutputs::Salient => {
            "The following excerpts were quoted from tool results produced before context was \
             compacted, in call order:"
        }
        _ => {
            "The following tool results were produced before context was compacted, in call order:"
        }
    };
    let count = entries.len() as u64;
    Some((
        format!("\n{HEADER}{sentence}\n\n{}", entries.concat()),
        count,
    ))
}

/// The section's heading, in the render's own register.
pub const HEADER: &str = "# tool outputs\n";

/// `- turn N: <tool> args=<json>`, Qwen Code's line for a call, its
/// arguments kept to one line.
fn call_line(output: &Output) -> String {
    format!(
        "- turn {}: {} args={}",
        output.turn,
        output.name,
        super::render::one_line(&output.arguments)
    )
}

/// `keep`: the call, then its result as the trunk had it; an image as
/// Qwen Code's placeholder for one.
fn kept(output: &Output) -> String {
    let mut result = output.shown.clone();
    for media_type in &output.images {
        if !result.is_empty() && !result.ends_with('\n') {
            result.push('\n');
        }
        result.push_str("[image: ");
        result.push_str(media_type);
        result.push(']');
    }
    if !result.ends_with('\n') {
        result.push('\n');
    }
    format!("{}\n[Tool result]: {result}", call_line(output))
}

/// The most characters of a call's arguments a reference line carries
/// (#814), Qwen Code's cut for a task's description, as a background
/// command's list cuts one (#614).
pub const REFERENCE_ARGS_CHARS: usize = 200;

/// The arguments that name a call (#814): its file -- `path` or
/// `file_path` -- or its command's first line, as a one-line object; the
/// whole arguments when they hold neither.
fn identifying(arguments: &str) -> String {
    let Ok(serde_json::Value::Object(args)) = serde_json::from_str::<serde_json::Value>(arguments)
    else {
        return super::render::one_line(arguments);
    };
    let mut named = serde_json::Map::new();
    for key in ["path", "file_path"] {
        if let Some(value) = args.get(key) {
            named.insert(key.to_owned(), value.clone());
        }
    }
    if let Some(serde_json::Value::String(command)) = args.get("command") {
        let first = command.lines().next().unwrap_or_default();
        named.insert("command".to_owned(), serde_json::Value::from(first));
    }
    if named.is_empty() {
        return super::render::one_line(arguments);
    }
    serde_json::Value::Object(named).to_string()
}

/// `- turn N: <tool> args=<json>`, the call named by its identifying
/// arguments within [`REFERENCE_ARGS_CHARS`], the rest elided and counted
/// (#814): a reference points at an output, and need not carry its input
/// -- a heredoc that writes a whole file is in the log's `tool_call` line,
/// whole.
fn named_call(output: &Output) -> String {
    let args = identifying(&output.arguments);
    let total = args.chars().count();
    let args = if total > REFERENCE_ARGS_CHARS {
        let cut: String = args.chars().take(REFERENCE_ARGS_CHARS).collect();
        format!(
            "{cut}\u{2026} ({} more characters)",
            total - REFERENCE_ARGS_CHARS
        )
    } else {
        args
    };
    format!("- turn {}: {} args={args}", output.turn, output.name)
}

/// `reference`: the call, named by its identifying arguments (#814), its
/// whole's size and sha256, and where it was saved. What a pruned output
/// is carried as (#612).
#[must_use]
pub fn reference_line(output: &Output) -> String {
    let Some(saved) = &output.saved else {
        return format!("{}: not saved\n", named_call(output));
    };
    let at = saved
        .path
        .as_ref()
        .map_or_else(|| "not saved".to_owned(), |path| format!("saved at {path}"));
    format!(
        "{}: {} bytes, sha256 {}, {at}\n",
        named_call(output),
        saved.bytes,
        saved.sha256
    )
}

/// `salient`: the call, then each excerpt the read fork quoted from it;
/// nothing for an output no fork quoted.
fn salient(output: &Output) -> Option<String> {
    if output.excerpts.is_empty() {
        return None;
    }
    let mut entry = format!("{}\n", call_line(output));
    for excerpt in &output.excerpts {
        entry.push_str("[Tool result excerpt]: ");
        entry.push_str(excerpt);
        if !excerpt.ends_with('\n') {
            entry.push('\n');
        }
    }
    Some(entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn output(turn: u32, shown: &str) -> Output {
        Output {
            turn,
            name: "bash".to_owned(),
            arguments: "{\"command\":\"ls\"}".to_owned(),
            shown: shown.to_owned(),
            images: Vec::new(),
            saved: None,
            excerpts: Vec::new(),
            pruned: None,
        }
    }

    /// #814: a reference line names its call by the arguments that identify
    /// it, within the bound: a `bash` heredoc that writes a whole file by its
    /// command's first line, a `write` by its path; a call with neither is
    /// cut, the rest counted.
    #[test]
    fn a_reference_line_names_its_call_within_the_bound() {
        let saved = Some(Saved {
            bytes: 14_522,
            sha256: "a".repeat(64),
            path: Some("/r/files/aaa".to_owned()),
        });
        let body = "print('x')\n".repeat(1_400);
        let heredoc = Output {
            arguments: serde_json::json!({"command": format!("cat > big.py <<'EOF'\n{body}EOF")})
                .to_string(),
            saved: saved.clone(),
            ..output(3, "")
        };
        let line = reference_line(&heredoc);
        assert!(
            line.starts_with(
                "- turn 3: bash args={\"command\":\"cat > big.py <<'EOF'\"}: 14522 bytes"
            ),
            "{line}"
        );
        let write = Output {
            name: "write".to_owned(),
            arguments: serde_json::json!({"path": "src/big.py", "content": body}).to_string(),
            saved: saved.clone(),
            ..output(4, "")
        };
        let line = reference_line(&write);
        assert!(
            line.starts_with("- turn 4: write args={\"path\":\"src/big.py\"}: 14522 bytes"),
            "{line}"
        );
        let long = Output {
            name: "grep".to_owned(),
            arguments: serde_json::json!({"pattern": "x".repeat(500)}).to_string(),
            saved,
            ..output(5, "")
        };
        let line = reference_line(&long);
        let args = line
            .split_once(" args=")
            .and_then(|(_, rest)| rest.split_once(": 14522 bytes"))
            .map(|(args, _)| args)
            .expect("the args");
        assert!(
            args.ends_with(&format!("\u{2026} ({} more characters)", 514 - 200)),
            "{args}"
        );
        assert!(args.chars().count() < REFERENCE_ARGS_CHARS + 40, "{args}");
    }

    /// #612: a pruned output is its reference line whatever the state; under
    /// `evict` it alone is carried, introduced as reference.
    #[test]
    fn a_pruned_output_is_its_line_under_every_state() {
        let line = "- turn 1: bash args={\"command\":\"ls\"}: 4 bytes, sha256 x, not saved\n";
        let pruned = Output {
            pruned: Some(line.to_owned()),
            ..output(1, "a\nb\n")
        };
        let outputs = [pruned, output(2, "c")];
        let (evicted, count) = section(MeditationToolOutputs::Evict, &outputs).expect("the prune");
        assert_eq!(count, 1);
        assert!(evicted.contains("listed as reference only"), "{evicted}");
        assert!(evicted.ends_with(line), "{evicted}");
        let (kept, count) = section(MeditationToolOutputs::Keep, &outputs).expect("both");
        assert_eq!(count, 2);
        assert!(
            kept.contains(line) && !kept.contains("[Tool result]: a\nb"),
            "{kept}"
        );
        assert!(kept.contains("[Tool result]: c"), "{kept}");
    }

    #[test]
    fn evict_carries_nothing_and_keep_carries_each_result_in_call_order() {
        let outputs = [output(1, "a\nb\n"), output(2, "c")];
        assert_eq!(section(MeditationToolOutputs::Evict, &outputs), None);
        assert_eq!(
            section(MeditationToolOutputs::Keep, &outputs),
            Some((
                "\n# tool outputs\nThe following tool results were produced before context was \
                 compacted, in call order:\n\n- turn 1: bash args={\"command\":\"ls\"}\n\
                 [Tool result]: a\nb\n- turn 2: bash args={\"command\":\"ls\"}\n\
                 [Tool result]: c\n"
                    .to_owned(),
                2
            ))
        );
        assert_eq!(section(MeditationToolOutputs::Keep, &[]), None);
    }

    #[test]
    fn reference_names_size_digest_and_path_and_salient_only_quoted_outputs() {
        let mut saved = output(3, "whole");
        saved.saved = Some(Saved {
            bytes: 5,
            sha256: "ab".to_owned(),
            path: Some("/r/files/ab".to_owned()),
        });
        let (text, n) = section(MeditationToolOutputs::Reference, &[saved]).expect("carried");
        assert!(
            text.ends_with(
                "- turn 3: bash args={\"command\":\"ls\"}: 5 bytes, sha256 ab, saved at /r/files/ab\n"
            ),
            "{text}"
        );
        assert_eq!(n, 1);
        let mut quoted = output(4, "x\ny\n");
        quoted.excerpts = vec!["y".to_owned()];
        let (text, n) =
            section(MeditationToolOutputs::Salient, &[output(3, "z"), quoted]).expect("carried");
        assert!(
            text.ends_with(
                "in call order:\n\n- turn 4: bash args={\"command\":\"ls\"}\n\
                 [Tool result excerpt]: y\n"
            ),
            "{text}"
        );
        assert_eq!(n, 1);
        assert_eq!(
            section(MeditationToolOutputs::Salient, &[output(3, "z")]),
            None
        );
    }
}
