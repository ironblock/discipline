//! The standard tool surface (#557): `read`, `write`, `edit`, `grep` and
//! `glob` beside `bash`, as the harnesses compared offer them.
//!
//! By the maintainer's rule each aspect follows Pi and `OpenCode` 2 where
//! they agree and Qwen Code where they do not (at the commits
//! `docs/harness-baseline.md` reads):
//!
//! * **names** -- `read`, `write`, `edit`, `grep`: Pi and `OpenCode` 2
//!   agree. File-name search they name differently (`find`, `glob`), so
//!   Qwen Code's `glob`.
//! * **parameters** -- `write`'s `path` and `content`, and `read`'s `path`,
//!   `offset` (a 1-based line) and `limit`: they agree. `edit`'s differ, so
//!   Qwen Code's: `file_path`, `old_string`, `new_string`, `replace_all`;
//!   `grep`'s too: `pattern`, `glob`, `path`, `limit`. `glob` takes
//!   `pattern`, `path` and `limit`, as both do.
//! * **rules** -- neither requires a read before an edit or an absolute
//!   path, so neither does this; a relative path resolves against the
//!   worktree. Both cap a read at 2000 lines or 50 KiB. Where they differ,
//!   Qwen Code's: a search always case-insensitive, capped at 1000 lines or
//!   25,000 characters; file names newest first, at most 100.
//! * **descriptions and results** -- they differ, so Qwen Code's
//!   (`qc:packages/core/src/tools/edit.ts`, `write-file.ts`,
//!   `read-file.ts`), its tool names swapped for these and the sentences
//!   stating rules this surface does not enforce removed.
//!
//! **Containment:** every tool runs as a helper command through the
//! session's own confinement ([`crate::isolation::Confinement::run_with_input`]), so the
//! kernel judges each path as it judges a `bash` command's. No approval
//! gate decides them (the approval layer is frozen); a cancel stops them as
//! it stops `bash`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::client::shape::ToolDefinition;
use crate::formats::record::json::Value;

/// A call's arguments: the model's output, a foreign format, read with
/// `serde_json` (a `null` for an optional parameter is omitted).
type Args = serde_json::Map<String, serde_json::Value>;

use super::tool_loop::Tools;

/// The read tool's name.
pub const READ: &str = "read";
/// The write tool's name.
pub const WRITE: &str = "write";
/// The edit tool's name.
pub const EDIT: &str = "edit";
/// The content search tool's name.
pub const GREP: &str = "grep";
/// The file-name search tool's name.
pub const GLOB: &str = "glob";

/// The most lines a read shows (Pi and `OpenCode` 2).
const READ_MAX_LINES: usize = 2000;
/// The most bytes a read shows (Pi and `OpenCode` 2).
const READ_MAX_BYTES: usize = 51_200;
/// Lines of context around an edit's snippet (Qwen Code `editHelper.ts`).
const SNIPPET_CONTEXT_LINES: usize = 4;
/// A changed region longer than this shows no snippet.
const SNIPPET_MAX_LINES: usize = 1000;

/// Whether `name` is a standard tool this module runs.
#[must_use]
pub fn is_standard(name: &str) -> bool {
    [READ, WRITE, EDIT, GREP, GLOB].contains(&name)
}

fn text(s: &str) -> Value {
    Value::String(s.to_owned())
}

pub(crate) fn property(kind: &str, description: &str) -> Value {
    Value::Object(BTreeMap::from([
        ("description".to_owned(), text(description)),
        ("type".to_owned(), text(kind)),
    ]))
}

pub(crate) fn definition(
    name: &str,
    description: &str,
    properties: &[(&str, Value)],
    required: &[&str],
) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        description: Some(description.to_owned()),
        schema: Value::Object(BTreeMap::from([
            (
                "properties".to_owned(),
                Value::Object(
                    properties
                        .iter()
                        .map(|(key, value)| ((*key).to_owned(), value.clone()))
                        .collect(),
                ),
            ),
            (
                "required".to_owned(),
                Value::Array(required.iter().map(|key| text(key)).collect()),
            ),
            ("type".to_owned(), text("object")),
        ])),
    }
}

/// The standard tools, in the order a request declares them after `bash`.
#[must_use]
pub fn definitions() -> Vec<ToolDefinition> {
    vec![
        read_tool(),
        write_tool(),
        edit_tool(),
        grep_tool(),
        glob_tool(),
    ]
}

/// `read`: Qwen Code's description, its rules this surface does not
/// enforce (an absolute path) and what it cannot read (PDFs, notebooks,
/// media) removed; Pi's and `OpenCode` 2's parameters.
#[must_use]
pub fn read_tool() -> ToolDefinition {
    definition(
        READ,
        "Reads and returns the content of a specified file. If the file is large, the content \
         will be truncated. For text files, the tool's response will clearly indicate if \
         truncation has occurred and will provide details on how to read more of the file \
         using the 'offset' and 'limit' parameters. Handles text and images (PNG, JPG, GIF, \
         WEBP). For text files, it can read specific line ranges.",
        &[
            (
                "path",
                property(
                    "string",
                    "The path to the file to read, absolute or relative to the working directory.",
                ),
            ),
            (
                "offset",
                property(
                    "integer",
                    "Optional: the 1-based line number to start reading from. Use with 'limit' \
                     to paginate through large files.",
                ),
            ),
            (
                "limit",
                property(
                    "integer",
                    "Optional: maximum number of lines to read. Use with 'offset' to paginate \
                     through large files. If omitted, reads the entire file (if feasible, up to \
                     a default limit).",
                ),
            ),
        ],
        &["path"],
    )
}

/// `write`: Qwen Code's description, its read-first and absolute-path rules
/// removed; Pi's and `OpenCode` 2's parameters.
#[must_use]
pub fn write_tool() -> ToolDefinition {
    definition(
        WRITE,
        "Writes content to a specified file in the local filesystem. If the file does not \
         exist it is created, with its parent directories; if it exists it is overwritten.",
        &[
            (
                "path",
                property(
                    "string",
                    "The path to the file to write to, absolute or relative to the working \
                     directory.",
                ),
            ),
            (
                "content",
                property("string", "The content to write to the file."),
            ),
        ],
        &["path", "content"],
    )
}

/// `edit`: Qwen Code's description and parameters, its read-first and
/// absolute-path rules and its user-modification note removed.
#[must_use]
pub fn edit_tool() -> ToolDefinition {
    definition(
        EDIT,
        "Replaces text within a file. By default, replaces a single occurrence. Set \
         `replace_all` to true when you intend to modify every instance of `old_string`. This \
         tool requires providing significant context around the change to ensure precise \
         targeting.\n\nExpectation for required parameters:\n1. `old_string` MUST be the exact \
         literal text to replace (including all whitespace, indentation, newlines, and \
         surrounding code etc.).\n2. `new_string` MUST be the exact literal text to replace \
         `old_string` with (also including all whitespace, indentation, newlines, and \
         surrounding code etc.). Ensure the resulting code is correct and idiomatic.\n3. NEVER \
         escape `old_string` or `new_string`, that would break the exact literal text \
         requirement.\n**Important:** If ANY of the above are not satisfied, the tool will \
         fail. CRITICAL for `old_string`: Must uniquely identify the single instance to change. \
         Include at least 3 lines of context BEFORE and AFTER the target text, matching \
         whitespace and indentation precisely. If this string matches multiple locations, or \
         does not match exactly, the tool will fail.",
        &[
            (
                "file_path",
                property(
                    "string",
                    "The path to the file to modify, absolute or relative to the working \
                     directory.",
                ),
            ),
            (
                "old_string",
                property(
                    "string",
                    "The exact literal text to replace, preferably unescaped. For single \
                     replacements (default), include at least 3 lines of context BEFORE and \
                     AFTER the target text, matching whitespace and indentation precisely. If \
                     this string is not the exact literal text (i.e. you escaped it) or does not \
                     match exactly, the tool will fail.",
                ),
            ),
            (
                "new_string",
                property(
                    "string",
                    "The exact literal text to replace `old_string` with, preferably unescaped. \
                     Provide the EXACT text. Ensure the resulting code is correct and idiomatic.",
                ),
            ),
            (
                "replace_all",
                property(
                    "boolean",
                    "Replace all occurrences of old_string (default false).",
                ),
            ),
        ],
        &["file_path", "old_string", "new_string"],
    )
}

/// `grep`: Qwen Code's description, its tool name ours; Qwen Code's
/// parameters (Pi and `OpenCode` 2 differ).
#[must_use]
pub fn grep_tool() -> ToolDefinition {
    definition(
        GREP,
        "A powerful search tool built on ripgrep\n\n  Usage:\n  - ALWAYS use the grep tool for \
         search tasks. NEVER invoke `grep` or `rg` as a Bash command. The grep tool has been \
         optimized for correct permissions and access.\n  - Supports full regex syntax (e.g., \
         \"log.*Error\", \"function\\s+\\w+\")\n  - Filter files with glob parameter (e.g., \
         \"*.js\", \"**/*.tsx\")\n  - Pattern syntax: Uses ripgrep (not grep) - special regex \
         characters need escaping (use `interface\\{\\}` to find `interface{}` in Go code)\n",
        &[
            (
                "pattern",
                property(
                    "string",
                    "The regular expression pattern to search for in file contents",
                ),
            ),
            (
                "glob",
                property(
                    "string",
                    "Glob pattern to filter files (e.g. \"*.js\", \"*.{ts,tsx}\") - maps to rg --glob",
                ),
            ),
            (
                "path",
                property(
                    "string",
                    "File or directory to search in (rg PATH). Defaults to current working \
                     directory.",
                ),
            ),
            (
                "limit",
                property(
                    "integer",
                    "Limit output to first N lines/entries. Must be a positive integer. Optional \
                     - shows all matches if not specified.",
                ),
            ),
        ],
        &["pattern"],
    )
}

/// `glob`: Qwen Code's name and description; `pattern`, `path` and
/// `limit` as Pi and `OpenCode` 2 both take them, the descriptions Qwen
/// Code's where it has them.
#[must_use]
pub fn glob_tool() -> ToolDefinition {
    definition(
        GLOB,
        "Fast file pattern matching tool that works with any codebase size\n- Supports glob \
         patterns like \"**/*.js\" or \"src/**/*.ts\"\n- Returns matching file paths sorted by \
         modification time\n- Use this tool when you need to find files by name patterns",
        &[
            (
                "pattern",
                property("string", "The glob pattern to match files against"),
            ),
            (
                "path",
                property(
                    "string",
                    "The directory to search in. If not specified, the current working \
                     directory will be used. IMPORTANT: Omit this field to use the default \
                     directory. DO NOT enter \"undefined\" or \"null\" - simply omit it for the \
                     default behavior. Must be a valid directory path if provided.",
                ),
            ),
            (
                "limit",
                property(
                    "integer",
                    "Optional: the most file paths to return (at most 100).",
                ),
            ),
        ],
        &["pattern"],
    )
}

/// The most match lines a grep shows (Qwen Code's
/// `DEFAULT_TRUNCATE_TOOL_OUTPUT_LINES`).
const GREP_MAX_LINES: usize = 1000;
/// The most characters a grep shows (Qwen Code's
/// `DEFAULT_TRUNCATE_TOOL_OUTPUT_THRESHOLD`).
const GREP_MAX_CHARS: usize = 25_000;
/// The most paths a glob shows (Qwen Code's `MAX_FILE_COUNT`).
const GLOB_MAX_FILES: usize = 100;

/// ripgrep, found once on the drive's own `PATH` and run by its absolute
/// path, so a command can plant no `rg` in front of it; `None` when the
/// machine has none.
fn ripgrep() -> Option<&'static Path> {
    static FOUND: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    if cfg!(test) && std::env::var_os("DIET_TEST_NO_RIPGREP").is_some() {
        return None;
    }
    FOUND
        .get_or_init(|| {
            std::env::var_os("PATH").and_then(|path| {
                std::env::split_paths(&path)
                    .map(|dir| dir.join("rg"))
                    .find(|candidate| candidate.is_file())
            })
        })
        .as_deref()
}

/// How a standard tool's call ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Done {
    /// It returned this result, an error's text included.
    Shown(String),
    /// `read` returned an image (#557): its bytes, delivered to the model as
    /// an image part of the tool's result, as Pi and `OpenCode` 2 deliver it.
    Image {
        /// Its media type, from its signature.
        media_type: &'static str,
        /// Its bytes.
        bytes: Vec<u8>,
    },
    /// A stop killed its helper, after it had printed this.
    Cancelled(String),
}

/// A call's arguments, as an object.
fn arguments(raw: &str) -> Result<Args, String> {
    match serde_json::from_str::<serde_json::Value>(raw.trim()) {
        Ok(serde_json::Value::Object(args)) => Ok(args),
        Ok(_) => Err("the arguments are not a JSON object".to_owned()),
        Err(why) => Err(format!("the arguments are not a JSON object: {why}")),
    }
}

fn required<'a>(args: &'a Args, key: &str) -> Result<&'a str, String> {
    match args.get(key) {
        Some(serde_json::Value::String(value)) => Ok(value),
        Some(serde_json::Value::Null) | None => {
            Err(format!("params must have required property '{key}'"))
        }
        Some(_) => Err(format!("params/{key} must be string")),
    }
}

fn optional_count(args: &Args, key: &str) -> Result<Option<usize>, String> {
    match args.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .filter(|n| *n >= 1)
            .map(|n| Some(usize::try_from(n).unwrap_or(usize::MAX)))
            .ok_or_else(|| format!("params/{key} must be a positive integer")),
    }
}

/// `path` as the helper opens it: absolute as given, relative against the
/// worktree.
fn located(tools: &Tools, path: &str) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        tools.worktree.join(path)
    }
}

/// A helper's run under the session's confinement: its output, or why it
/// failed; `Err(None)` when a stop killed it.
fn confined(
    tools: &Tools,
    argv: &[&str],
    input: Option<&[u8]>,
    stop: &dyn Fn() -> bool,
) -> Result<crate::isolation::Ran, Option<String>> {
    let argv: Vec<String> = argv.iter().map(|part| (*part).to_owned()).collect();
    let ran = tools
        .confinement
        .run_with_input(&tools.policy, &tools.worktree, &argv, input, stop)
        .map_err(|why| Some(why.to_string()))?;
    if ran.cancelled {
        return Err(None);
    }
    Ok(ran)
}

/// The text a failed helper said.
fn said(ran: &crate::isolation::Ran) -> String {
    let stderr = ran.stderr.trim();
    if stderr.is_empty() {
        format!(
            "exit {}",
            ran.exit
                .map_or("by signal".to_owned(), |code| code.to_string())
        )
    } else {
        stderr.to_owned()
    }
}

/// A file's bytes through a confined `cat`.
fn read_bytes(
    tools: &Tools,
    at: &Path,
    stop: &dyn Fn() -> bool,
) -> Result<Vec<u8>, Option<String>> {
    let ran = confined(
        tools,
        &["/bin/cat", "--", &at.to_string_lossy()],
        None,
        stop,
    )?;
    if ran.exit != Some(0) {
        return Err(Some(said(&ran)));
    }
    Ok(ran.raw_stdout)
}

/// A file's text through a confined `cat`.
fn read_text(tools: &Tools, at: &Path, stop: &dyn Fn() -> bool) -> Result<String, Option<String>> {
    let bytes = read_bytes(tools, at, stop)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// The image formats Pi and `OpenCode` 2 both read (JPEG, PNG, GIF, WebP),
/// by signature, never by extension.
fn image_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

/// `content` written to `at` through a confined helper: whether the file
/// existed before.
fn write_text(
    tools: &Tools,
    at: &Path,
    content: &str,
    stop: &dyn Fn() -> bool,
) -> Result<bool, Option<String>> {
    let ran = confined(
        tools,
        &[
            "/bin/sh",
            "-c",
            "if [ -e \"$1\" ]; then echo existed; fi; mkdir -p -- \"$(dirname -- \"$1\")\" && \
             cat > \"$1\"",
            "sh",
            &at.to_string_lossy(),
        ],
        Some(content.as_bytes()),
        stop,
    )?;
    if ran.exit != Some(0) {
        return Err(Some(said(&ran)));
    }
    Ok(ran.stdout.trim() == "existed")
}

/// Run the standard tool `name` with `raw` arguments under `tools`.
#[must_use]
pub fn run(name: &str, raw: &str, tools: &Tools, stop: &dyn Fn() -> bool) -> Done {
    let result = arguments(raw).and_then(|args| match name {
        READ => Ok(read(&args, tools, stop)),
        WRITE => Ok(write(&args, tools, stop)),
        EDIT => Ok(edit(&args, tools, stop)),
        GREP => Ok(grep(&args, tools, stop)),
        GLOB => Ok(glob(&args, tools, stop)),
        _ => Err(format!("`{name}` is not a standard tool")),
    });
    match result {
        Ok(done) => done,
        Err(why) => Done::Shown(why),
    }
}

/// A helper's failure as the call's result, or the cancel.
fn failed(why: Option<String>, describe: impl FnOnce(String) -> String) -> Done {
    why.map_or_else(
        || Done::Cancelled(String::new()),
        |why| Done::Shown(describe(why)),
    )
}

fn read(args: &Args, tools: &Tools, stop: &dyn Fn() -> bool) -> Done {
    let (path, offset, limit) = match (
        required(args, "path"),
        optional_count(args, "offset"),
        optional_count(args, "limit"),
    ) {
        (Ok(path), Ok(offset), Ok(limit)) => (path, offset, limit),
        (Err(why), _, _) | (_, Err(why), _) | (_, _, Err(why)) => return Done::Shown(why),
    };
    let bytes = match read_bytes(tools, &located(tools, path), stop) {
        Ok(bytes) => bytes,
        Err(why) => return failed(why, |why| format!("Error reading file {path}: {why}")),
    };
    if let Some(media_type) = image_type(&bytes) {
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > super::attach::MAX_BYTES {
            return Done::Shown(format!(
                "Cannot read image file {path}: it is larger than {} bytes",
                super::attach::MAX_BYTES
            ));
        }
        return Done::Image { media_type, bytes };
    }
    let Ok(content) = String::from_utf8(bytes) else {
        return Done::Shown(format!("Cannot read binary file: {path}"));
    };
    let lines: Vec<&str> = content.split_inclusive('\n').collect();
    let total = lines.len();
    let start = offset.unwrap_or(1) - 1;
    if start >= total && total > 0 {
        return Done::Shown(format!(
            "Offset {} is beyond end of file ({total} lines total)",
            start + 1
        ));
    }
    let wanted = limit.unwrap_or(READ_MAX_LINES).min(READ_MAX_LINES);
    let mut shown = String::new();
    let mut end = start;
    for line in lines.iter().skip(start).take(wanted) {
        if shown.len() + line.len() > READ_MAX_BYTES && end > start {
            break;
        }
        shown.push_str(line);
        end += 1;
    }
    if start == 0 && end == total {
        Done::Shown(shown)
    } else {
        Done::Shown(format!(
            "Showing lines {}-{end} of {total} total lines.\n\n---\n\n{shown}",
            start + 1
        ))
    }
}

fn write(args: &Args, tools: &Tools, stop: &dyn Fn() -> bool) -> Done {
    let (path, content) = match (required(args, "path"), required(args, "content")) {
        (Ok(path), Ok(content)) => (path, content),
        (Err(why), _) | (_, Err(why)) => return Done::Shown(why),
    };
    match write_text(tools, &located(tools, path), content, stop) {
        Ok(true) => Done::Shown(format!("Successfully overwrote file: {path}.")),
        Ok(false) => Done::Shown(format!(
            "Successfully created and wrote to new file: {path}."
        )),
        Err(why) => failed(why, |why| format!("Error writing to file: {why}")),
    }
}

fn edit(args: &Args, tools: &Tools, stop: &dyn Fn() -> bool) -> Done {
    let (path, old, new) = match (
        required(args, "file_path"),
        required(args, "old_string"),
        required(args, "new_string"),
    ) {
        (Ok(path), Ok(old), Ok(new)) => (path, old, new),
        (Err(why), _, _) | (_, Err(why), _) | (_, _, Err(why)) => return Done::Shown(why),
    };
    let replace_all = match args.get("replace_all") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(all)) => *all,
        Some(_) => return Done::Shown("params/replace_all must be boolean".to_owned()),
    };
    // Pi and `OpenCode` 2 both refuse an empty `old_string` (Qwen Code would
    // create a file); the refusal's words are `OpenCode` 2's.
    if old.is_empty() {
        return Done::Shown(
            "old_string must not be empty. Use write to create or overwrite a file.".to_owned(),
        );
    }
    let at = located(tools, path);
    let current = match read_text(tools, &at, stop) {
        Ok(current) => current,
        Err(why) => return failed(why, |why| format!("Error executing edit: {why}")),
    };
    let occurrences = current.matches(old).count();
    if occurrences == 0 {
        return Done::Shown(format!(
            "Failed to edit, 0 occurrences found for old_string in {path}. No edits made. The \
             exact text in old_string was not found. Ensure you're not escaping content \
             incorrectly and check whitespace, indentation, and context. Use {READ} tool to \
             verify."
        ));
    }
    if !replace_all && occurrences > 1 {
        return Done::Shown(format!(
            "Failed to edit. Found {occurrences} occurrences for old_string in {path} but \
             replace_all was not enabled."
        ));
    }
    if old == new {
        return Done::Shown(format!(
            "No changes to apply. The old_string and new_string are identical in file: {path}"
        ));
    }
    let updated = if replace_all {
        current.replace(old, new)
    } else {
        current.replacen(old, new, 1)
    };
    if let Err(why) = write_text(tools, &at, &updated, stop) {
        return failed(why, |why| format!("Error executing edit: {why}"));
    }
    let mut message = format!("The file: {path} has been updated.");
    if let Some(snippet) = snippet(&current, &updated) {
        message.push(' ');
        message.push_str(&snippet);
    }
    Done::Shown(message)
}

/// Qwen Code's snippet of an edited file (`editHelper.ts`
/// `extractEditSnippet`): the changed lines and four around them, or
/// nothing when the change spans more than a thousand lines.
fn snippet(old: &str, new: &str) -> Option<String> {
    if old == new || new.is_empty() {
        return None;
    }
    let old_lines: Vec<&str> = old.split('\n').collect();
    let new_lines: Vec<&str> = new.split('\n').collect();
    let total = new_lines.len();
    let mut first = 0;
    while first < old_lines.len().min(total) && old_lines[first] == new_lines[first] {
        first += 1;
    }
    let (mut old_end, mut new_end) = (old_lines.len(), total);
    while old_end > first && new_end > first && old_lines[old_end - 1] == new_lines[new_end - 1] {
        old_end -= 1;
        new_end -= 1;
    }
    let (change_start, change_end) = (first + 1, new_end.max(first + 1));
    if change_end - change_start > SNIPPET_MAX_LINES {
        return None;
    }
    let start = change_start.saturating_sub(SNIPPET_CONTEXT_LINES).max(1);
    let end = (change_end + SNIPPET_CONTEXT_LINES).min(total);
    Some(format!(
        "Showing lines {start}-{end} of {total} from the edited file:\n\n---\n\n{}",
        new_lines[start - 1..end].join("\n")
    ))
}

/// Where a search ran, in Qwen Code's words.
fn searched_in(path: Option<&str>, word: &str) -> String {
    path.map_or_else(
        || "in the workspace directory".to_owned(),
        |path| format!("{word} \"{path}\""),
    )
}

fn optional_text<'a>(args: &'a Args, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(format!("params/{key} must be string")),
    }
}

/// The search a `grep` call runs: ripgrep when the machine has it, always
/// case-insensitive as Qwen Code's own (`--ignore-case`); otherwise the
/// system `grep`, the filter as its `--include`.
fn grep_argv(rg: Option<&Path>, pattern: &str, filter: Option<&str>, target: &str) -> Vec<String> {
    let mut searched: Vec<String> = match rg {
        Some(rg) => [
            rg.to_string_lossy().as_ref(),
            "--line-number",
            "--with-filename",
            "--no-heading",
            "--color",
            "never",
            "--ignore-case",
        ]
        .iter()
        .map(|part| (*part).to_owned())
        .collect(),
        None => vec!["/usr/bin/grep".to_owned(), "-rnHIiE".to_owned()],
    };
    match (rg, filter) {
        (Some(_), Some(filter)) => searched.extend(["--glob".to_owned(), filter.to_owned()]),
        (None, Some(filter)) => searched.push(format!("--include={filter}")),
        (_, None) => {}
    }
    if rg.is_some() {
        searched.push("--regexp".to_owned());
        searched.push(pattern.to_owned());
        searched.extend(["--".to_owned(), target.to_owned()]);
    } else {
        searched.extend(["--".to_owned(), pattern.to_owned(), target.to_owned()]);
    }
    searched
}

fn grep(args: &Args, tools: &Tools, stop: &dyn Fn() -> bool) -> Done {
    let (pattern, filter, path, limit) = match (
        required(args, "pattern"),
        optional_text(args, "glob"),
        optional_text(args, "path"),
        optional_count(args, "limit"),
    ) {
        (Ok(pattern), Ok(filter), Ok(path), Ok(limit)) => (pattern, filter, path, limit),
        (Err(why), ..) | (_, Err(why), ..) | (_, _, Err(why), _) | (.., Err(why)) => {
            return Done::Shown(why);
        }
    };
    let target = path.map_or_else(
        || ".".to_owned(),
        |path| located(tools, path).to_string_lossy().into_owned(),
    );
    let searched = grep_argv(ripgrep(), pattern, filter, &target);
    let searched: Vec<&str> = searched.iter().map(String::as_str).collect();
    let ran = confined(tools, &searched, None, stop);
    let ran = match ran {
        Ok(ran) => ran,
        Err(why) => {
            return failed(why, |why| {
                format!("Error during grep search operation: {why}")
            });
        }
    };
    let location = searched_in(path, "in path");
    let filtered = filter.map_or_else(String::new, |filter| format!(" (filter: \"{filter}\")"));
    match ran.exit {
        Some(0) => {}
        Some(1) => {
            return Done::Shown(format!(
                "No matches found for pattern \"{pattern}\" {location}{filtered}."
            ));
        }
        _ => {
            return Done::Shown(format!(
                "Error during grep search operation: {}",
                said(&ran)
            ));
        }
    }
    // `grep -r .` names files `./a`; ripgrep names them `a`.
    let matches: Vec<&str> = ran
        .stdout
        .lines()
        .map(|line| line.strip_prefix("./").unwrap_or(line))
        .collect();
    let total = matches.len();
    let term = if total == 1 { "match" } else { "matches" };
    let mut out =
        format!("Found {total} {term} for pattern \"{pattern}\" {location}{filtered}:\n---\n");
    let wanted = limit.unwrap_or(GREP_MAX_LINES).min(GREP_MAX_LINES);
    let mut included = 0;
    for line in matches.iter().take(wanted) {
        if out.len() + line.len() + 1 > GREP_MAX_CHARS && included > 0 {
            break;
        }
        if included > 0 {
            out.push('\n');
        }
        out.push_str(line);
        included += 1;
    }
    if included < total {
        let omitted = total - included;
        let lines = if omitted == 1 { "line" } else { "lines" };
        let _ = write!(out, "\n---\n[{omitted} {lines} truncated] ...");
    }
    Done::Shown(out)
}

/// Qwen Code's order (`glob.ts` `sortFileEntries`): files changed in the
/// last day newest first, then the rest by path.
fn newest_first(found: &mut [(PathBuf, Option<std::time::SystemTime>)]) {
    let now = std::time::SystemTime::now();
    let day = std::time::Duration::from_secs(24 * 60 * 60);
    let recent = |modified: Option<std::time::SystemTime>| {
        modified.is_some_and(|at| now.duration_since(at).is_ok_and(|age| age < day))
    };
    found.sort_by(|(a, at), (b, bt)| match (recent(*at), recent(*bt)) {
        (true, true) => bt.cmp(at),
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        (false, false) => a.cmp(b),
    });
}

fn glob(args: &Args, tools: &Tools, stop: &dyn Fn() -> bool) -> Done {
    let (pattern, path, limit) = match (
        required(args, "pattern"),
        optional_text(args, "path"),
        optional_count(args, "limit"),
    ) {
        (Ok(pattern), Ok(path), Ok(limit)) => (pattern, path, limit),
        (Err(why), ..) | (_, Err(why), _) | (.., Err(why)) => return Done::Shown(why),
    };
    let root = path.map_or_else(|| tools.worktree.clone(), |path| located(tools, path));
    let root_text = root.to_string_lossy().into_owned();
    let ran = match ripgrep() {
        Some(rg) => confined(
            tools,
            &[
                &rg.to_string_lossy(),
                "--files",
                "--glob",
                pattern,
                "--",
                &root_text,
            ],
            None,
            stop,
        ),
        None => confined(
            tools,
            &[
                "/usr/bin/find",
                &root_text,
                "-type",
                "f",
                "-path",
                &format!("*{pattern}"),
            ],
            None,
            stop,
        ),
    };
    let ran = match ran {
        Ok(ran) => ran,
        Err(why) => {
            return failed(why, |why| {
                format!("Error during glob search operation: {why}")
            });
        }
    };
    let location = path.map_or_else(
        || "in the workspace directory".to_owned(),
        |_| format!("within {root_text}"),
    );
    if ran.exit.is_some_and(|code| code > 1) {
        return Done::Shown(format!(
            "Error during glob search operation: {}",
            said(&ran)
        ));
    }
    let mut found: Vec<(PathBuf, Option<std::time::SystemTime>)> = ran
        .stdout
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let file = Path::new(line);
            let file = if file.is_absolute() {
                file.to_path_buf()
            } else {
                tools.worktree.join(file)
            };
            let modified = std::fs::metadata(&file)
                .and_then(|meta| meta.modified())
                .ok();
            (file, modified)
        })
        .collect();
    if found.is_empty() {
        return Done::Shown(format!(
            "No files found matching pattern \"{pattern}\" {location}"
        ));
    }
    newest_first(&mut found);
    let total = found.len();
    let shown = limit.unwrap_or(GLOB_MAX_FILES).min(GLOB_MAX_FILES);
    let listed: Vec<String> = found
        .iter()
        .take(shown)
        .map(|(file, _)| file.to_string_lossy().into_owned())
        .collect();
    let mut out = format!(
        "Found {total} file(s) matching \"{pattern}\" {location}, sorted by modification time \
         (newest first):\n---\n{}",
        listed.join("\n")
    );
    if total > shown {
        let omitted = total - shown;
        let files = if omitted == 1 { "file" } else { "files" };
        let _ = write!(out, "\n---\n[{omitted} {files} truncated] ...");
    }
    Done::Shown(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drive::session::tests::tools;
    use crate::drive::tool_loop::Decider;
    use crate::drive::tool_loop::tests::scratch;
    use crate::isolation::Confinement;

    fn lines_of(from: usize, to: usize, prefix: &str) -> String {
        use std::fmt::Write as _;
        (from..=to).fold(String::new(), |mut out, n| {
            let _ = writeln!(out, "{prefix}{n}");
            out
        })
    }

    fn unconfined(tree: &Path) -> Tools {
        tools(Confinement::Unconfined, tree, &[], None, Decider::Decline)
    }

    fn shown(done: Done) -> String {
        match done {
            Done::Shown(text) => text,
            Done::Cancelled(_) => panic!("cancelled"),
            Done::Image { media_type, .. } => panic!("an image: {media_type}"),
        }
    }

    fn call(name: &str, args: &serde_json::Value, tools: &Tools) -> String {
        shown(run(name, &args.to_string(), tools, &|| false))
    }

    /// `write` creates a file and its parent directories, then overwrites
    /// it, saying which in Qwen Code's words; a relative path resolves
    /// against the worktree.
    #[test]
    fn write_creates_then_overwrites_relative_to_the_worktree() {
        let tree = scratch("std-write");
        let tools = unconfined(&tree);
        let args = serde_json::json!({"path": "a/b/c.txt", "content": "one\n"});
        assert_eq!(
            call(WRITE, &args, &tools),
            "Successfully created and wrote to new file: a/b/c.txt."
        );
        assert_eq!(
            std::fs::read_to_string(tree.join("a/b/c.txt")).expect("written"),
            "one\n"
        );
        let args = serde_json::json!({"path": "a/b/c.txt", "content": "two\n"});
        assert_eq!(
            call(WRITE, &args, &tools),
            "Successfully overwrote file: a/b/c.txt."
        );
        assert_eq!(
            std::fs::read_to_string(tree.join("a/b/c.txt")).expect("written"),
            "two\n"
        );
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// `read` returns a file whole, a range with Qwen Code's header, refuses
    /// an offset past the end and a binary file, and names a missing
    /// parameter.
    #[test]
    fn read_returns_the_whole_a_range_with_its_header_or_the_refusal() {
        let tree = scratch("std-read");
        let tools = unconfined(&tree);
        let numbered: String = lines_of(1, 10, "");
        std::fs::write(tree.join("n.txt"), &numbered).expect("written");
        std::fs::write(tree.join("bin"), [0xff_u8, 0xfe, 0x00, 0x01]).expect("written");
        assert_eq!(
            call(READ, &serde_json::json!({"path": "n.txt"}), &tools),
            numbered
        );
        assert_eq!(
            call(
                READ,
                &serde_json::json!({"path": "n.txt", "offset": 3, "limit": 2}),
                &tools
            ),
            "Showing lines 3-4 of 10 total lines.\n\n---\n\n3\n4\n"
        );
        assert_eq!(
            call(
                READ,
                &serde_json::json!({"path": "n.txt", "offset": 11}),
                &tools
            ),
            "Offset 11 is beyond end of file (10 lines total)"
        );
        assert_eq!(
            call(READ, &serde_json::json!({"path": "bin"}), &tools),
            "Cannot read binary file: bin"
        );
        assert_eq!(
            call(READ, &serde_json::json!({"offset": null}), &tools),
            "params must have required property 'path'"
        );
        assert!(
            call(READ, &serde_json::json!({"path": "gone"}), &tools)
                .starts_with("Error reading file gone: ")
        );
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// An image is read as its bytes and its type, by signature and not by
    /// name, for the result message (#557); a file that only looks like one
    /// by name is read as what it is.
    #[test]
    fn read_returns_an_image_by_its_signature() {
        let tree = scratch("std-read-image");
        let tools = unconfined(&tree);
        let png = b"\x89PNG\r\n\x1a\nnot really a picture".to_vec();
        let jpeg = [0xff_u8, 0xd8, 0xff, 0xe0, 0x00];
        std::fs::write(tree.join("shot.bin"), &png).expect("written");
        std::fs::write(tree.join("photo"), jpeg).expect("written");
        std::fs::write(tree.join("fake.png"), "words\n").expect("written");
        let read = |path: &str| {
            run(
                READ,
                &serde_json::json!({"path": path}).to_string(),
                &tools,
                &|| false,
            )
        };
        assert_eq!(
            read("shot.bin"),
            Done::Image {
                media_type: "image/png",
                bytes: png
            }
        );
        assert_eq!(
            read("photo"),
            Done::Image {
                media_type: "image/jpeg",
                bytes: jpeg.to_vec()
            }
        );
        assert_eq!(shown(read("fake.png")), "words\n");
        assert_eq!(image_type(b"GIF89a.."), Some("image/gif"));
        assert_eq!(image_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_type(b"RIFF\0\0\0\0WAVE"), None);
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// A read is capped at 2000 lines, with the header saying where it
    /// stopped.
    #[test]
    fn read_stops_at_two_thousand_lines() {
        let tree = scratch("std-read-cap");
        let tools = unconfined(&tree);
        let lines: String = lines_of(1, 2500, "");
        std::fs::write(tree.join("long.txt"), lines).expect("written");
        let shown = call(READ, &serde_json::json!({"path": "long.txt"}), &tools);
        assert!(
            shown.starts_with("Showing lines 1-2000 of 2500 total lines.\n\n---\n\n1\n"),
            "{}",
            &shown[..80]
        );
        assert!(shown.ends_with("\n2000\n"));
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// `edit` replaces one occurrence and shows Qwen Code's snippet; refuses,
    /// in Qwen Code's words, text not found, text found more than once
    /// without `replace_all`, and identical strings; replaces every one with
    /// `replace_all`; and refuses an empty `old_string`, as Pi and
    /// `OpenCode` 2 do.
    #[test]
    fn edit_replaces_exactly_or_refuses_in_qwen_codes_words() {
        let tree = scratch("std-edit");
        let tools = unconfined(&tree);
        let body: String = lines_of(1, 20, "line ") + "x\nx\n";
        std::fs::write(tree.join("f.txt"), &body).expect("written");
        let edit = |old: &str, new: &str, all: bool| {
            call(
                EDIT,
                &serde_json::json!({"file_path": "f.txt", "old_string": old, "new_string": new, "replace_all": all}),
                &tools,
            )
        };
        let done = edit("line 10\n", "line ten\n", false);
        assert!(done.starts_with("The file: f.txt has been updated. Showing lines 6-14 of 23 from the edited file:\n\n---\n\nline 6\n"), "{done}");
        assert!(done.contains("line ten\n"));
        assert!(
            std::fs::read_to_string(tree.join("f.txt"))
                .expect("read")
                .contains("line ten\n")
        );
        assert!(edit("nowhere", "x", false).starts_with(
            "Failed to edit, 0 occurrences found for old_string in f.txt. No edits made."
        ));
        assert!(edit("read tool", "x", false).ends_with("Use read tool to verify."));
        assert_eq!(
            edit("x\n", "y\n", false),
            "Failed to edit. Found 2 occurrences for old_string in f.txt but replace_all was not enabled."
        );
        assert_eq!(
            edit("line 1\n", "line 1\n", false),
            "No changes to apply. The old_string and new_string are identical in file: f.txt"
        );
        assert!(edit("x\n", "y\n", true).starts_with("The file: f.txt has been updated."));
        assert!(
            std::fs::read_to_string(tree.join("f.txt"))
                .expect("read")
                .ends_with("y\ny\n")
        );
        assert_eq!(
            edit("", "z", false),
            "old_string must not be empty. Use write to create or overwrite a file."
        );
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// `grep`, case-insensitive as Qwen Code's always is: Qwen Code's
    /// header and `path:line:text` lines, its glob filter, its no-match
    /// sentence, and its truncation notice at a limit.
    #[test]
    fn grep_finds_matches_in_qwen_codes_shape() {
        let tree = scratch("std-grep");
        let tools = unconfined(&tree);
        std::fs::create_dir_all(tree.join("src")).expect("a dir");
        std::fs::write(tree.join("src/a.rs"), "fn main() {}\nlet Needle = 1;\n").expect("written");
        std::fs::write(tree.join("src/b.txt"), "a needle here\n").expect("written");
        let found = call(GREP, &serde_json::json!({"pattern": "needle"}), &tools);
        assert!(
            found.starts_with(
                "Found 2 matches for pattern \"needle\" in the workspace directory:\n---\n"
            ),
            "{found}"
        );
        assert!(found.contains("src/a.rs:2:let Needle = 1;"), "{found}");
        assert!(found.contains("src/b.txt:1:a needle here"), "{found}");
        let filtered = call(
            GREP,
            &serde_json::json!({"pattern": "needle", "glob": "*.rs"}),
            &tools,
        );
        assert!(filtered.starts_with("Found 1 match for pattern \"needle\" in the workspace directory (filter: \"*.rs\"):"), "{filtered}");
        assert_eq!(
            call(GREP, &serde_json::json!({"pattern": "haystack"}), &tools),
            "No matches found for pattern \"haystack\" in the workspace directory."
        );
        let limited = call(
            GREP,
            &serde_json::json!({"pattern": "needle", "limit": 1}),
            &tools,
        );
        assert!(
            limited.ends_with("\n---\n[1 line truncated] ..."),
            "{limited}"
        );
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// `glob`: absolute paths, files changed in the last day newest first
    /// and the rest by path, Qwen Code's header, its no-match sentence and
    /// its truncation notice.
    #[test]
    fn glob_lists_files_newest_first_in_qwen_codes_shape() {
        let tree = scratch("std-glob");
        let tools = unconfined(&tree);
        std::fs::write(tree.join("old.md"), "a").expect("written");
        std::thread::sleep(std::time::Duration::from_millis(1100));
        std::fs::write(tree.join("new.md"), "b").expect("written");
        std::fs::write(tree.join("skip.txt"), "c").expect("written");
        let found = call(GLOB, &serde_json::json!({"pattern": "*.md"}), &tools);
        let (head, list) = found.split_once("\n---\n").expect("the header");
        assert_eq!(
            head,
            "Found 2 file(s) matching \"*.md\" in the workspace directory, sorted by modification time (newest first):"
        );
        let listed: Vec<String> = list.lines().map(str::to_owned).collect();
        assert_eq!(
            listed,
            [
                tree.join("new.md").to_string_lossy().into_owned(),
                tree.join("old.md").to_string_lossy().into_owned()
            ]
        );
        assert_eq!(
            call(GLOB, &serde_json::json!({"pattern": "*.rs"}), &tools),
            "No files found matching pattern \"*.rs\" in the workspace directory"
        );
        let limited = call(
            GLOB,
            &serde_json::json!({"pattern": "*.md", "limit": 1}),
            &tools,
        );
        assert!(
            limited.ends_with("\n---\n[1 file truncated] ..."),
            "{limited}"
        );
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// A stop kills a standard tool's helper as it kills `bash` (#551).
    #[test]
    fn a_stop_cancels_a_standard_tool() {
        let tree = scratch("std-stop");
        let tools = unconfined(&tree);
        let done = run(READ, r#"{"path":"x"}"#, &tools, &|| true);
        assert!(matches!(done, Done::Cancelled(_)), "{done:?}");
        let _ = std::fs::remove_dir_all(&tree);
    }

    /// Every definition carries Qwen Code's description, adapted: no rule
    /// this surface does not enforce, no other harness's tool name.
    #[test]
    fn the_descriptions_name_no_rule_or_tool_this_surface_lacks() {
        for tool in definitions() {
            let said = tool.description.expect("a description");
            for absent in [
                "absolute path",
                "read_file",
                "MUST use the",
                "has not been read",
            ] {
                assert!(!said.contains(absent), "{}: {absent}", tool.name);
            }
        }
    }

    /// Containment (#557): under the Mac's own Seatbelt, a standard tool is
    /// held to the session's policy as `bash` is -- a write inside the
    /// worktree lands, one outside it is refused by the kernel and comes
    /// back as the tool's error, and nothing is written there.
    #[test]
    fn under_seatbelt_a_write_outside_the_worktree_is_refused() {
        if crate::isolation::Platform::here() != crate::isolation::Platform::MacOs {
            return;
        }
        let tree = scratch("std-seatbelt-tree");
        let outside = scratch("std-seatbelt-outside");
        let policy = crate::isolation::Policy::merged_usr();
        let mut sandboxed = unconfined(&tree);
        sandboxed.confinement =
            crate::isolation::open_on(&policy, crate::isolation::Platform::MacOs)
                .expect("Seatbelt");
        sandboxed.policy = policy;
        let inside = call(
            WRITE,
            &serde_json::json!({"path": "in.txt", "content": "ok"}),
            &sandboxed,
        );
        assert_eq!(
            inside,
            "Successfully created and wrote to new file: in.txt."
        );
        let away = outside.join("away.txt");
        let refused = call(
            WRITE,
            &serde_json::json!({"path": away.to_string_lossy(), "content": "no"}),
            &sandboxed,
        );
        assert!(refused.starts_with("Error writing to file: "), "{refused}");
        assert!(!away.exists(), "the write escaped the sandbox");
        // A search runs under it too, and finds what the tree holds.
        let found = call(GREP, &serde_json::json!({"pattern": "ok"}), &sandboxed);
        assert!(found.contains("in.txt:1:ok"), "{found}");
        let _ = std::fs::remove_dir_all(&tree);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
