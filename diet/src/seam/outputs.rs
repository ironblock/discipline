//! What a seam's refill carries of the tool outputs it compacts away (#553):
//! the outputs in the turns before the kept tail (#552), under the
//! regimen's [`SeamToolOutputs`] state.
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
//! The section is logged on the seam line as sent, as the render is, so
//! the projection rebuilds the refill from the log whatever the state.

use crate::formats::log::SeamToolOutputs;

/// One output a seam compacts away, as the session gathered it.
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
pub fn section(state: SeamToolOutputs, outputs: &[Output]) -> Option<(String, u64)> {
    let entries: Vec<String> = match state {
        SeamToolOutputs::Evict => return None,
        SeamToolOutputs::Keep => outputs.iter().map(kept).collect(),
        SeamToolOutputs::Reference => outputs.iter().map(referenced).collect(),
        SeamToolOutputs::Salient => outputs.iter().filter_map(salient).collect(),
    };
    if entries.is_empty() {
        return None;
    }
    let sentence = match state {
        SeamToolOutputs::Reference => {
            "The following tool results were produced before context was compacted. They are \
             listed as reference only: each was saved whole, and can be read at its path."
        }
        SeamToolOutputs::Salient => {
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

/// `reference`: the call, its whole's size and sha256, and where it was
/// saved.
fn referenced(output: &Output) -> String {
    let Some(saved) = &output.saved else {
        return format!("{}: not saved\n", call_line(output));
    };
    let at = saved
        .path
        .as_ref()
        .map_or_else(|| "not saved".to_owned(), |path| format!("saved at {path}"));
    format!(
        "{}: {} bytes, sha256 {}, {at}\n",
        call_line(output),
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
        }
    }

    #[test]
    fn evict_carries_nothing_and_keep_carries_each_result_in_call_order() {
        let outputs = [output(1, "a\nb\n"), output(2, "c")];
        assert_eq!(section(SeamToolOutputs::Evict, &outputs), None);
        assert_eq!(
            section(SeamToolOutputs::Keep, &outputs),
            Some((
                "\n# tool outputs\nThe following tool results were produced before context was \
                 compacted, in call order:\n\n- turn 1: bash args={\"command\":\"ls\"}\n\
                 [Tool result]: a\nb\n- turn 2: bash args={\"command\":\"ls\"}\n\
                 [Tool result]: c\n"
                    .to_owned(),
                2
            ))
        );
        assert_eq!(section(SeamToolOutputs::Keep, &[]), None);
    }

    #[test]
    fn reference_names_size_digest_and_path_and_salient_only_quoted_outputs() {
        let mut saved = output(3, "whole");
        saved.saved = Some(Saved {
            bytes: 5,
            sha256: "ab".to_owned(),
            path: Some("/r/files/ab".to_owned()),
        });
        let (text, n) = section(SeamToolOutputs::Reference, &[saved]).expect("carried");
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
            section(SeamToolOutputs::Salient, &[output(3, "z"), quoted]).expect("carried");
        assert!(
            text.ends_with(
                "in call order:\n\n- turn 4: bash args={\"command\":\"ls\"}\n\
                 [Tool result excerpt]: y\n"
            ),
            "{text}"
        );
        assert_eq!(n, 1);
        assert_eq!(section(SeamToolOutputs::Salient, &[output(3, "z")]), None);
    }
}
