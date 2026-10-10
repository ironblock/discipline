//! A tool output capped as it arrives on the trunk, with a pointer to the
//! whole of it (#554): the one disposition that helps before a seam -- the
//! README's 300 KB file read in turn two, paid for on every request after.
//!
//! By the maintainer's rule, the convention is the other harnesses', by
//! vote: Pi and `OpenCode` 2 where they agree, Qwen Code where they do not
//! (at the commits `docs/harness-baseline.md` reads).
//!
//! * **The limit** -- Pi and `OpenCode` 2 agree: 2000 lines or 50 KiB,
//!   whichever comes first (`pi:packages/coding-agent/src/core/tools/truncate.ts`
//!   11-12; `oc:packages/core/src/tool-output-store.ts` 13-14), and both cap
//!   every tool output, so the cap is on unless a regimen turns it off.
//! * **What is kept** -- they differ (Pi the tail, `OpenCode` 2 head and
//!   tail), so Qwen Code's: the head a fifth of the budget, the tail four
//!   fifths, with its separator (`qc:packages/core/src/tools/truncation.ts`
//!   113-116, 130-137).
//! * **The notice** -- they differ, so Qwen Code's, before the output
//!   (`truncation.ts` 198-204), its third line naming the session's read
//!   tool, or -- with `bash` alone -- reading the file through it. A notice
//!   longer than the output it replaces is not used (`truncation.ts` 209-211).

/// Whether and how a tool output is capped on arrival.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputCap {
    /// Capped at whichever of the two comes first.
    Capped {
        /// The most lines shown.
        max_lines: usize,
        /// The most bytes shown.
        max_bytes: usize,
    },
    /// Kept whole: the cap turned off.
    Keep,
}

impl OutputCap {
    /// The convention's default: 2000 lines or 50 KiB.
    pub const DEFAULT: Self = Self::Capped {
        max_lines: 2000,
        max_bytes: 51_200,
    };
}

impl Default for OutputCap {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Where the separator marks what was cut, Qwen Code's.
pub const SEPARATOR: &str = "\n\n---\n... [CONTENT TRUNCATED] ...\n---\n\n";

/// Where the whole output can be read, for the notice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kept<'a> {
    /// Saved at this absolute path, readable by the session's read tool
    /// when it has one, named here; through `bash` otherwise.
    At {
        /// The file's absolute path.
        path: &'a str,
        /// The session's read tool, by name, when it offers one.
        read_tool: Option<&'a str>,
    },
    /// Saved nowhere: the session keeps no recording.
    Nowhere,
}

/// The notice that goes before a capped output.
fn notice(kept: Kept<'_>) -> String {
    let (saved, reading) = match kept {
        Kept::At {
            path,
            read_tool: Some(tool),
        } => (
            format!("The full output has been saved to: {path}"),
            format!(
                "To read the complete output, use the {tool} tool with the absolute file path above."
            ),
        ),
        Kept::At {
            path,
            read_tool: None,
        } => (
            format!("The full output has been saved to: {path}"),
            "To read the complete output, read the file at the absolute path above with the bash \
             tool (for example sed -n, grep or tail)."
                .to_owned(),
        ),
        Kept::Nowhere => (
            "The full output was not saved.".to_owned(),
            "To see more of it, run the command again with a narrower output (for example \
             sed -n, grep, head or tail)."
                .to_owned(),
        ),
    };
    format!(
        "Tool output was too large and has been truncated.\n{saved}\n{reading}\n\
         The truncated output below shows the beginning and end of the content. The marker \
         '... [CONTENT TRUNCATED] ...' indicates where content was removed.\n\n\
         Truncated part of the output:\n"
    )
}

/// Whether `text` is over `cap`.
#[must_use]
pub fn over(text: &str, cap: OutputCap) -> bool {
    match cap {
        OutputCap::Keep => false,
        OutputCap::Capped {
            max_lines,
            max_bytes,
        } => text.len() > max_bytes || text.lines().count() > max_lines,
    }
}

/// The first `lines` lines of `text`, at most `bytes` long, cut at a
/// character boundary.
fn head(text: &str, lines: usize, bytes: usize) -> &str {
    let by_lines = text
        .match_indices('\n')
        .nth(lines.saturating_sub(1))
        .map_or(text.len(), |(at, _)| at + 1);
    let mut end = by_lines.min(bytes).min(text.len());
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// The last `lines` lines of `text`, at most `bytes` long, cut at a
/// character boundary.
fn tail(text: &str, lines: usize, bytes: usize) -> &str {
    let body = text.strip_suffix('\n').unwrap_or(text);
    let by_lines = if lines == 0 {
        text.len()
    } else {
        body.rmatch_indices('\n')
            .nth(lines - 1)
            .map_or(0, |(at, _)| at + 1)
    };
    let mut start = by_lines.max(text.len().saturating_sub(bytes));
    while !text.is_char_boundary(start) {
        start += 1;
    }
    &text[start..]
}

/// `text` as the model is shown it under `cap`: unchanged when it is within
/// the cap, or when the notice would make it longer; otherwise the notice,
/// the head, the separator and the tail.
#[must_use]
pub fn capped(text: &str, cap: OutputCap, kept: Kept<'_>) -> String {
    let OutputCap::Capped {
        max_lines,
        max_bytes,
    } = cap
    else {
        return text.to_owned();
    };
    if !over(text, cap) {
        return text.to_owned();
    }
    let head_lines = max_lines.div_ceil(5);
    let head_bytes = max_bytes.div_ceil(5);
    let first = head(text, head_lines, head_bytes);
    let rest = &text[first.len()..];
    let last = tail(
        rest,
        max_lines - head_lines.min(max_lines),
        max_bytes - head_bytes.min(max_bytes),
    );
    let shown = format!("{}{first}{SEPARATOR}{last}", notice(kept));
    if shown.len() >= text.len() {
        text.to_owned()
    } else {
        shown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(lines: usize) -> String {
        use std::fmt::Write as _;
        (1..=lines).fold(String::new(), |mut out, n| {
            let _ = writeln!(out, "line {n}");
            out
        })
    }

    /// Within the cap, the output is unchanged; the cap off keeps anything.
    #[test]
    fn an_output_within_the_cap_or_with_the_cap_off_is_unchanged() {
        let small = numbered(10);
        assert_eq!(capped(&small, OutputCap::DEFAULT, Kept::Nowhere), small);
        let big = numbered(5000);
        assert_eq!(capped(&big, OutputCap::Keep, Kept::Nowhere), big);
    }

    /// Over the line cap: Qwen Code's notice, the head a fifth of the
    /// lines, the separator, the tail four fifths.
    #[test]
    fn over_the_line_cap_the_head_is_a_fifth_and_the_tail_four_fifths() {
        let cap = OutputCap::Capped {
            max_lines: 10,
            max_bytes: 1_000_000,
        };
        let shown = capped(
            &numbered(100),
            cap,
            Kept::At {
                path: "/r/files/abc",
                read_tool: None,
            },
        );
        let (notice, body) = shown
            .split_once("Truncated part of the output:\n")
            .expect("the notice");
        assert!(notice.starts_with("Tool output was too large and has been truncated.\n"));
        assert!(notice.contains("The full output has been saved to: /r/files/abc\n"));
        assert!(notice.contains("with the bash tool"));
        let (first, last) = body.split_once(SEPARATOR).expect("the separator");
        assert_eq!(first, "line 1\nline 2\n");
        assert_eq!(
            last,
            numbered(100)
                .split_inclusive('\n')
                .skip(92)
                .collect::<String>()
        );
    }

    /// Over the byte cap with one enormous line: cut at character
    /// boundaries, within the budget.
    #[test]
    fn over_the_byte_cap_a_single_line_is_cut_at_character_boundaries() {
        let cap = OutputCap::Capped {
            max_lines: 2000,
            max_bytes: 500,
        };
        let text = "é".repeat(10_000);
        let shown = capped(&text, cap, Kept::Nowhere);
        let body = shown
            .split_once("Truncated part of the output:\n")
            .expect("the notice")
            .1;
        let (first, last) = body.split_once(SEPARATOR).expect("the separator");
        assert!(
            first.len() <= 100 && last.len() <= 400,
            "{} {}",
            first.len(),
            last.len()
        );
        assert!(first.chars().all(|c| c == 'é') && last.chars().all(|c| c == 'é'));
        assert!(shown.contains("The full output was not saved."));
    }

    /// With a read tool on offer, its name stands in Qwen Code's own line.
    #[test]
    fn a_session_with_a_read_tool_is_told_to_use_it() {
        let shown = capped(
            &numbered(5000),
            OutputCap::DEFAULT,
            Kept::At {
                path: "/r/files/abc",
                read_tool: Some("read_file"),
            },
        );
        assert!(shown.contains(
            "To read the complete output, use the read_file tool with the absolute file path above.\n"
        ));
    }

    /// A notice longer than the output it would replace is not used.
    #[test]
    fn a_notice_longer_than_the_output_is_not_used() {
        let cap = OutputCap::Capped {
            max_lines: 1,
            max_bytes: 1_000_000,
        };
        let text = "a\nb\n";
        assert_eq!(capped(text, cap, Kept::Nowhere), text);
    }
}
