//! Tool calls a model wrote as text, recovered.
//!
//! A faithful port of Qwen Code's XML tool-call text fallback,
//! `packages/core/src/core/xml-tool-call-fallback.ts`, and of every case in
//! its test file. Some models answer a tool-enabled request with the call
//! written into the content as markup, in one of two dialects:
//!
//! ```text
//! <invoke name="read_file"><parameter name="file_path">a.ts</parameter></invoke>
//! <function=read_file><parameter=file_path>a.ts</parameter></function>
//! ```
//!
//! [`try_recover`] turns such text back into calls, and refuses whenever the
//! markup looks documented rather than emitted: inside a fenced code block,
//! inside an `<example>` wrapper, quoted inside another call's parameter
//! value, malformed (a block that borrowed a later block's closer), or
//! outweighed by prose (more than 0.8 of the text). Parameterless blocks are
//! never recovered.
//!
//! The crate has no regex engine, so each of the original's regular
//! expressions is a small hand-written scanner whose doc comment quotes the
//! expression it implements. Offsets are bytes; every slice is taken at an
//! ASCII delimiter or at a scanner's match boundary, so it is on a char
//! boundary. `\s` and `trim()` mean JavaScript's whitespace set
//! ([`is_js_space`]), not Rust's.
//!
//! # The one deliberate approximation: the markdown lexer
//!
//! The original runs `marked`'s inline lexer over the prose (the text with
//! parameter values masked to spaces) and counts an `<example>` tag only
//! where the lexer produced an HTML token, so a tag inside an inline code
//! span does not count. This port has no markdown lexer. It masks
//! `CommonMark` inline code spans instead: a backtick run of length N up to
//! the next run of exactly N backticks, with an unmatched run being literal
//! text. A tag that starts inside such a span does not count; every other
//! regex-matched tag does. Where `marked` would refuse a tag for any other
//! reason (an attribute shape that is not valid HTML, a backslash escape, a
//! backtick inside a tag that opens a span `marked` never sees), this port
//! still counts it. Past 64 KiB of UTF-16 the original skips the lexer and
//! takes every regex-matched tag; so does this port, skipping the masking.
//!
//! # Other known differences
//!
//! * Arguments are a [`serde_json::Map`], which in this crate is
//!   `BTreeMap`-backed (`preserve_order` is off): keys iterate sorted, not in
//!   the order first written. A later duplicate overwrites, as in the
//!   original.
//! * Parameter values that look like JSON are parsed with `serde_json`, not
//!   `JSON.parse`. With `arbitrary_precision`, numbers keep their spelling
//!   (`1.0` stays `1.0`, `1e400` is not `Infinity`); an escaped lone
//!   surrogate (`"\ud800"`) or nesting past `serde_json`'s recursion limit
//!   fails to parse here, so the raw string is kept.
//! * The prose-dominance ratio counts `char`s; JavaScript counts UTF-16
//!   units. The two agree on text inside the Basic Multilingual Plane.
//! * The caller assigns call ids; the original minted `xml-recovered-…`.

use serde_json::{Map, Value};

/// Past this many UTF-16 units the original skips its markdown lexer; past
/// it this port skips the code-span masking that stands in for the lexer.
const MAX_LEXER_SCAN_LENGTH: usize = 64 * 1024;

/// One call recovered from text.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovered {
    /// The function's name.
    pub name: String,
    /// Its arguments as a JSON object. The map is `BTreeMap`-backed in this
    /// crate, so keys iterate sorted; a later duplicate overwrites the value.
    pub arguments: Map<String, Value>,
    /// The block's text exactly as it appeared: `text[start..end]`.
    pub source: String,
}

/// What a text holds, recovered.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovery {
    /// The calls, in the order they appear.
    pub calls: Vec<Recovered>,
    /// The text with the recovered blocks (and the envelopes they emptied)
    /// removed, runs of three or more newlines collapsed to two, and trimmed.
    pub remaining: String,
}

/// Whether `text` holds anything shaped like a tool call (`containsXmlToolCalls`).
///
/// This is the bare pattern test: it does not apply the fence, example,
/// quoting or malformation guards.
#[must_use]
pub fn contains_xml_tool_calls(text: &str) -> bool {
    leftmost(text, 0, tool_call_at).is_some()
}

/// The calls `text` holds that pass every guard except prose dominance
/// (`extractXmlToolCalls`).
#[must_use]
pub fn extract_xml_tool_calls(text: &str) -> Vec<Recovered> {
    recoverable_blocks(text)
        .into_iter()
        .map(|found| found.into_recovered(text))
        .collect()
}

/// The calls `text` holds and what is left of it (`tryRecoverXmlToolCalls`).
///
/// `None` where the original reports `recovered: false`: no call passed the
/// guards, or the prose outside every tool-call-shaped block (parameterless
/// ones included) is more than 0.8 of the text.
#[must_use]
pub fn try_recover(text: &str) -> Option<Recovery> {
    let found = recoverable_blocks(text);
    if found.is_empty() {
        return None;
    }

    let without_blocks = collapse_newlines(&remove_all(text, tool_call_at));
    let prose_only = js_trim(&without_blocks);
    // `proseOnly.length / text.length > 0.8`, in integers: p/t > 4/5 iff
    // 5p > 4t. JavaScript counts UTF-16 units; `chars()` agrees on BMP text.
    let prose_len = prose_only.chars().count();
    let text_len = text.chars().count();
    if text_len > 0 && prose_len * 5 > text_len * 4 {
        return None;
    }

    let removed: Vec<(usize, usize)> = found.iter().map(|f| (f.start, f.end)).collect();
    let mut without_calls = String::with_capacity(text.len());
    let mut cursor = 0;
    for &(start, end) in &removed {
        without_calls.push_str(&text[cursor..start]);
        cursor = end;
    }
    without_calls.push_str(&text[cursor..]);
    let stripped = strip_emptied_wrappers(&without_calls, text, &removed);
    let remaining = js_trim(&collapse_newlines(&stripped)).to_owned();

    let calls = found
        .into_iter()
        .map(|found| found.into_recovered(text))
        .collect();
    Some(Recovery { calls, remaining })
}

/// A block that passed every guard, located in the text.
struct Found {
    start: usize,
    end: usize,
    name: String,
    arguments: Map<String, Value>,
}

impl Found {
    fn into_recovered(self, text: &str) -> Recovered {
        Recovered {
            name: self.name,
            arguments: self.arguments,
            source: text[self.start..self.end].to_owned(),
        }
    }
}

/// `recoverableToolCallBlocks`: every accepted block, fence and example
/// filtering applied.
fn recoverable_blocks(text: &str) -> Vec<Found> {
    // Parameter spans are a property of the raw text, not of which blocks are
    // accepted: they mask parameter data out of the prose and fence tracking.
    let parameter_ranges: Vec<(usize, usize)> = all_matches(text, parameter_at)
        .iter()
        .map(|p| (p.start, p.end))
        .collect();
    // Regions a closed parameter value owns: a call inside one is quoted.
    let value_spans = closed_parameter_spans(text);

    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(block) = leftmost(text, from, tool_call_at) {
        from = block.end;
        if within(&value_spans, block.start) {
            continue;
        }
        // A rejected block may have swallowed a complete later block, so the
        // scan resumes just after its open tag; with no derivable tag end it
        // skips the whole block.
        let resume_at = open_tag_end(&text[block.start..block.end])
            .map_or(block.end, |tag_end| block.start + tag_end);
        let outside_parameters = remove_all(block.body, parameter_at);
        if has_call_opener(block.body)
            || has_call_tag(&outside_parameters)
            || has_fence_line(&outside_parameters)
        {
            from = resume_at;
            continue;
        }

        let mut arguments = Map::new();
        for parameter in all_matches(block.body, parameter_at) {
            let value = decode_xml_entities(strip_delimiting_newlines(parameter.body));
            arguments.insert(parameter.name.to_owned(), parse_parameter_value(value));
        }
        if !block.name.is_empty() && !arguments.is_empty() {
            blocks.push(Found {
                start: block.start,
                end: block.end,
                name: block.name.to_owned(),
                arguments,
            });
        }
    }

    let examples = example_ranges(text, &parameter_ranges);
    blocks.retain(|&Found { start, end, .. }| {
        !inside_fence(text, start, &parameter_ranges)
            && !inside_fence(text, end - 1, &parameter_ranges)
            && !within(&examples, start)
            && !within(&examples, end - 1)
    });
    blocks
}

/// Whether `index` lies in any half-open range.
fn within(ranges: &[(usize, usize)], index: usize) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| index >= start && index < end)
}

// ---------------------------------------------------------------------------
// Value handling
// ---------------------------------------------------------------------------

/// `decodeXmlEntities`: the five predefined entities, `&amp;` last so
/// `&amp;lt;` becomes `&lt;`.
fn decode_xml_entities(value: &str) -> String {
    if !value.contains('&') {
        return value.to_owned();
    }
    value
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `parseParameterValue`: JSON only when the trimmed value starts with `{`
/// or `[` and parses; otherwise the raw (untrimmed) string.
fn parse_parameter_value(value: String) -> Value {
    let trimmed = js_trim(&value);
    if (trimmed.starts_with('{') || trimmed.starts_with('['))
        && let Ok(parsed) = serde_json::from_str::<Value>(trimmed)
    {
        return parsed;
    }
    Value::String(value)
}

/// `stripDelimitingNewlines`: one `\n` after the open tag and one before the
/// close tag, nothing else.
fn strip_delimiting_newlines(value: &str) -> &str {
    let value = value.strip_prefix('\n').unwrap_or(value);
    value.strip_suffix('\n').unwrap_or(value)
}

// ---------------------------------------------------------------------------
// Character classes and small text helpers
// ---------------------------------------------------------------------------

/// JavaScript's `\s`, which is also the set `String.prototype.trim` removes:
/// `WhiteSpace` (tab, VT, FF, space, NBSP, ZWNBSP and category Zs) plus
/// `LineTerminator` (LF, CR, LS, PS). Unlike Rust's `char::is_whitespace`
/// it includes U+FEFF and excludes U+0085.
fn is_js_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// `String.prototype.trim`.
fn js_trim(s: &str) -> &str {
    s.trim_matches(is_js_space)
}

/// `/\n{3,}/g` replaced with `\n\n`.
fn collapse_newlines(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut run = 0;
    for c in s.chars() {
        if c == '\n' {
            run += 1;
            if run > 2 {
                continue;
            }
        } else {
            run = 0;
        }
        out.push(c);
    }
    out
}

/// The char starting at byte `i`, if any.
fn char_at(text: &str, i: usize) -> Option<char> {
    text.get(i..)?.chars().next()
}

/// The offset just past `literal` when `text` has it at `i`.
fn lit(text: &str, i: usize, literal: &str) -> Option<usize> {
    text.get(i..)?
        .starts_with(literal)
        .then_some(i + literal.len())
}

/// `\s*` (greedy) from `i`: the offset after the run.
fn skip_spaces(text: &str, i: usize) -> usize {
    let rest = &text[i..];
    i + (rest.len() - rest.trim_start_matches(is_js_space).len())
}

// ---------------------------------------------------------------------------
// Scanners. Every pattern starts with `<`; `leftmost` reproduces a global
// regex's `exec` from `lastIndex` by trying each `<` in order.
// ---------------------------------------------------------------------------

/// A matched element: `<open>body</close>`, with the name its open tag gave.
#[derive(Debug, Clone, Copy)]
struct Element<'t> {
    start: usize,
    end: usize,
    name: &'t str,
    body: &'t str,
}

/// The first match at or after `from`, leftmost-first.
fn leftmost<'t, T>(
    text: &'t str,
    from: usize,
    at: impl Fn(&'t str, usize) -> Option<T>,
) -> Option<T> {
    text[from..]
        .match_indices('<')
        .find_map(|(offset, _)| at(text, from + offset))
}

/// Every non-overlapping match, as a global `exec` loop finds them.
fn all_matches<'t>(
    text: &'t str,
    at: impl Fn(&'t str, usize) -> Option<Element<'t>> + Copy,
) -> Vec<Element<'t>> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(element) = leftmost(text, from, at) {
        from = element.end;
        found.push(element);
    }
    found
}

/// `text.replace(PATTERN, '')` for a global pattern.
fn remove_all<'t>(
    text: &'t str,
    at: impl Fn(&'t str, usize) -> Option<Element<'t>> + Copy,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for element in all_matches(text, at) {
        out.push_str(&text[cursor..element.start]);
        cursor = element.end;
    }
    out.push_str(&text[cursor..]);
    out
}

/// `["']([^"']+)["']` at `i`: the name, and the offset after the closing
/// quote. The two quotes need not be the same character.
fn quoted_name(text: &str, i: usize) -> Option<(&str, usize)> {
    let open = lit(text, i, "\"").or_else(|| lit(text, i, "'"))?;
    let len = text[open..].find(['"', '\''])?;
    (len > 0).then(|| (&text[open..open + len], open + len + 1))
}

/// `([^\s<>]+)` at `i`, greedy: the name, and the offset after it.
fn bare_name(text: &str, i: usize) -> Option<(&str, usize)> {
    let rest = &text[i..];
    let len = rest
        .find(|c: char| is_js_space(c) || c == '<' || c == '>')
        .unwrap_or(rest.len());
    (len > 0).then(|| (&rest[..len], i + len))
}

/// `\s+name=["']([^"']+)["']` at `i`.
fn name_attribute(text: &str, i: usize) -> Option<(&str, usize)> {
    let after_spaces = skip_spaces(text, i);
    if after_spaces == i {
        return None;
    }
    quoted_name(text, lit(text, after_spaces, "name=")?)
}

/// `>([\s\S]*?)CLOSE` from `i`: the lazy body ends at the FIRST `close`.
fn lazy_body<'t>(
    text: &'t str,
    start: usize,
    name: &'t str,
    i: usize,
    close: &str,
) -> Option<Element<'t>> {
    let body_start = lit(text, i, ">")?;
    let len = text[body_start..].find(close)?;
    Some(Element {
        start,
        end: body_start + len + close.len(),
        name,
        body: &text[body_start..body_start + len],
    })
}

/// `TOOL_CALL_PATTERN` at `i`, alternatives in order:
///
/// ```text
/// /<invoke\s+name=["']([^"']+)["']>([\s\S]*?)<\/invoke>|<function=([^\s<>]+)>([\s\S]*?)<\/function>/g
/// ```
fn tool_call_at(text: &str, i: usize) -> Option<Element<'_>> {
    invoke_at(text, i).or_else(|| function_at(text, i))
}

/// `<invoke\s+name=["']([^"']+)["']>([\s\S]*?)<\/invoke>` at `i`.
fn invoke_at(text: &str, i: usize) -> Option<Element<'_>> {
    let (name, after) = name_attribute(text, lit(text, i, "<invoke")?)?;
    lazy_body(text, i, name, after, "</invoke>")
}

/// `<function=([^\s<>]+)>([\s\S]*?)<\/function>` at `i`.
fn function_at(text: &str, i: usize) -> Option<Element<'_>> {
    let (name, after) = bare_name(text, lit(text, i, "<function=")?)?;
    lazy_body(text, i, name, after, "</function>")
}

/// `PARAMETER_PATTERN` at `i`:
///
/// ```text
/// /<parameter(?:\s+name=["']([^"']+)["']|=([^\s<>]+))>([\s\S]*?)<\/parameter>/g
/// ```
fn parameter_at(text: &str, i: usize) -> Option<Element<'_>> {
    let after_tag = lit(text, i, "<parameter")?;
    let (name, after) =
        name_attribute(text, after_tag).or_else(|| bare_name(text, lit(text, after_tag, "=")?))?;
    lazy_body(text, i, name, after, "</parameter>")
}

/// `(?:[^>"']|"[^"]*"|'[^']*')*>` from `i`: the offset just past the first
/// `>` outside a quoted run. A quote with no partner fails the match (the
/// star cannot stop before it, since only `>` may follow the star).
fn quoted_run_to_close(text: &str, mut i: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    while let Some(&byte) = bytes.get(i) {
        match byte {
            b'>' => return Some(i + 1),
            b'"' | b'\'' => {
                let len = text[i + 1..].find(char::from(byte))?;
                i += len + 2;
            }
            _ => i += 1,
        }
    }
    None
}

/// `<\/?NAME(?=[LOOKAHEAD])(?:[^>"']|"[^"]*"|'[^']*')*>` at `i`: the tag's
/// `(start, end)`.
fn tag_at(text: &str, i: usize, name: &str, lookahead: fn(char) -> bool) -> Option<(usize, usize)> {
    let after_open = lit(text, i, "<")?;
    let after_slash = lit(text, after_open, "/").unwrap_or(after_open);
    let after_name = lit(text, after_slash, name)?;
    if !char_at(text, after_name).is_some_and(lookahead) {
        return None;
    }
    Some((i, quoted_run_to_close(text, after_name)?))
}

/// `PARAMETER_TAG_PATTERN` at `i`:
///
/// ```text
/// /<\/?parameter(?=[\s=>])(?:[^>"']|"[^"]*"|'[^']*')*>/g
/// ```
fn parameter_tag_at(text: &str, i: usize) -> Option<(usize, usize)> {
    tag_at(text, i, "parameter", |c| {
        is_js_space(c) || c == '=' || c == '>'
    })
}

/// The example tag pattern at `i`:
///
/// ```text
/// /<\/?example(?=[\s/>])(?:[^>"']|"[^"]*"|'[^']*')*>/g
/// ```
fn example_tag_at(text: &str, i: usize) -> Option<(usize, usize)> {
    tag_at(text, i, "example", |c| {
        is_js_space(c) || c == '/' || c == '>'
    })
}

/// `<` + optional `/` + one of `words`, then `(?:[\s=>]|$)` (`$` without the
/// `m` flag: end of input).
fn word_tag_at(text: &str, i: usize, slash: bool, words: &[&str]) -> bool {
    let Some(mut after) = lit(text, i, "<") else {
        return false;
    };
    if slash {
        after = lit(text, after, "/").unwrap_or(after);
    }
    words.iter().any(|word| {
        lit(text, after, word).is_some_and(|end| {
            char_at(text, end).is_none_or(|c| is_js_space(c) || c == '=' || c == '>')
        })
    })
}

/// `/<(?:function|invoke)(?:[\s=>]|$)/`
fn has_call_opener(text: &str) -> bool {
    leftmost(text, 0, |t, i| {
        word_tag_at(t, i, false, &["function", "invoke"]).then_some(())
    })
    .is_some()
}

/// `/<\/?(?:function|invoke|parameter)(?:[\s=>]|$)/`
fn has_call_tag(text: &str) -> bool {
    leftmost(text, 0, |t, i| {
        word_tag_at(t, i, true, &["function", "invoke", "parameter"]).then_some(())
    })
    .is_some()
}

/// ``^ {0,3}((`{3,})|~{3,})`` against the start of `line`: the delimiter, the
/// run's length, and the length of the whole match. Four leading spaces
/// fail: ` {0,3}` cannot leave a space for the run to start on.
fn fence_opener(line: &str) -> Option<(u8, usize, usize)> {
    let bytes = line.as_bytes();
    let spaces = bytes.iter().take_while(|&&b| b == b' ').count();
    if spaces > 3 {
        return None;
    }
    let delim = *bytes.get(spaces)?;
    if delim != b'`' && delim != b'~' {
        return None;
    }
    let run = bytes[spaces..].iter().take_while(|&&b| b == delim).count();
    (run >= 3).then_some((delim, run, spaces + run))
}

/// ``/^ {0,3}(?:`{3,}|~{3,})/m``: with the `m` flag `^` matches at the start
/// and after every JavaScript line terminator (LF, CR, LS, PS).
fn has_fence_line(text: &str) -> bool {
    fence_opener(text).is_some()
        || text
            .char_indices()
            .filter(|&(_, c)| matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}'))
            .any(|(i, c)| fence_opener(&text[i + c.len_utf8()..]).is_some())
}

/// `positionInsideFence`: whether `index` falls inside an unclosed fenced
/// code block. A fence closes only on the same delimiter, at least as long
/// as the opener, with nothing but whitespace after it (`CommonMark` 4.5).
/// Lines starting inside a parameter range are values, not prose, and are
/// skipped.
fn inside_fence(text: &str, index: usize, parameter_ranges: &[(usize, usize)]) -> bool {
    let mut open: Option<(u8, usize)> = None;
    let mut line_start = 0;
    for line in text[..index].split('\n') {
        let this_line = line_start;
        line_start += line.len() + 1;
        if within(parameter_ranges, this_line) {
            continue;
        }
        let Some((delim, len, matched)) = fence_opener(line) else {
            continue;
        };
        match open {
            None => open = Some((delim, len)),
            Some((open_delim, open_len))
                if open_delim == delim
                    && len >= open_len
                    && js_trim(&line[matched..]).is_empty() =>
            {
                open = None;
            }
            Some(_) => {}
        }
    }
    open.is_some()
}

/// `openTagEnd`: the offset just past the first `>` outside a quoted run in
/// `block`, or `None` when it has none.
fn open_tag_end(block: &str) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (index, &byte) in block.as_bytes().iter().enumerate() {
        match quote {
            Some(q) if byte == q => quote = None,
            None if byte == b'"' || byte == b'\'' => quote = Some(byte),
            None if byte == b'>' => return Some(index + 1),
            Some(_) | None => {}
        }
    }
    None
}

/// `closedParameterSpans`: the spans of parameter elements whose open tag is
/// paired by depth with a later close tag. An open tag never closed owns
/// nothing.
fn closed_parameter_spans(text: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut open_starts = Vec::new();
    let mut from = 0;
    while let Some((start, end)) = leftmost(text, from, parameter_tag_at) {
        from = end;
        if text[start..end].starts_with("</") {
            if let Some(open) = open_starts.pop() {
                spans.push((open, end));
            }
        } else {
            open_starts.push(start);
        }
    }
    spans
}

/// The prose the original hands its lexer: `text` with every parameter range
/// masked to spaces, line breaks kept. Each masked char becomes as many
/// spaces as it has UTF-8 bytes, so prose offsets are text offsets (the
/// original's per-UTF-16-unit mask keeps its offsets aligned the same way).
fn mask_parameters(text: &str, parameter_ranges: &[(usize, usize)]) -> String {
    let mut prose = String::with_capacity(text.len());
    let mut cursor = 0;
    for &(start, end) in parameter_ranges {
        prose.push_str(&text[cursor..start]);
        for c in text[start..end].chars() {
            if c == '\r' || c == '\n' {
                prose.push(c);
            } else {
                prose.extend(std::iter::repeat_n(' ', c.len_utf8()));
            }
        }
        cursor = end;
    }
    prose.push_str(&text[cursor..]);
    prose
}

/// `CommonMark` inline code spans in `prose`, the stand-in for `marked`'s
/// lexer: a backtick run of length N up to the next run of exactly N
/// backticks. A run with no partner is literal and scanning goes on after it.
fn inline_code_spans(prose: &str) -> Vec<(usize, usize)> {
    let bytes = prose.as_bytes();
    let run_at = |i: usize| bytes[i..].iter().take_while(|&&b| b == b'`').count();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'`' {
            i += 1;
            continue;
        }
        let opener = run_at(i);
        let mut j = i + opener;
        let mut closed = None;
        while let Some(offset) = prose[j..].find('`') {
            let at = j + offset;
            let run = run_at(at);
            if run == opener {
                closed = Some(at + run);
                break;
            }
            j = at + run;
        }
        if let Some(end) = closed {
            spans.push((i, end));
            i = end;
        } else {
            i += opener;
        }
    }
    spans
}

/// `computeExampleRanges`: the regions explicit `<example>` wrappers cover,
/// paired by depth; an unclosed opener runs to the end of the text.
fn example_ranges(text: &str, parameter_ranges: &[(usize, usize)]) -> Vec<(usize, usize)> {
    if !text.contains("<example") && !text.contains("</example") {
        return Vec::new();
    }
    // `text.length` is UTF-16 units; so is this.
    let lexed = text.encode_utf16().count() <= MAX_LEXER_SCAN_LENGTH;
    let code_spans = if lexed {
        inline_code_spans(&mask_parameters(text, parameter_ranges))
    } else {
        Vec::new()
    };

    let mut ranges = Vec::new();
    let mut depth = 0_usize;
    let mut start = 0;
    let mut from = 0;
    while let Some((position, end)) = leftmost(text, from, example_tag_at) {
        from = end;
        if within(&code_spans, position)
            || within(parameter_ranges, position)
            || inside_fence(text, position, parameter_ranges)
        {
            continue;
        }
        let tag = &text[position..end];
        // `/\/\s*>$/`: self-closing.
        if tag[..tag.len() - 1]
            .trim_end_matches(is_js_space)
            .ends_with('/')
        {
            continue;
        }
        if tag.starts_with("</") {
            if depth > 0 {
                depth -= 1;
                if depth == 0 {
                    ranges.push((start, end));
                }
            }
        } else {
            if depth == 0 {
                start = position;
            }
            depth += 1;
        }
    }
    if depth > 0 {
        ranges.push((start, text.len()));
    }
    ranges
}

/// `/<tool_call>\s*<\/tool_call>|<function_calls>\s*<\/function_calls>/g`
/// at `i`: the wrapper's `(start, end)`.
fn empty_wrapper_at(text: &str, i: usize) -> Option<(usize, usize)> {
    [
        ("<tool_call>", "</tool_call>"),
        ("<function_calls>", "</function_calls>"),
    ]
    .iter()
    .find_map(|&(open, close)| {
        let inner = skip_spaces(text, lit(text, i, open)?);
        Some((i, lit(text, inner, close)?))
    })
}

/// The empty-wrapper strip: a wrapper left empty in `without_calls` is
/// removed unless, mapped back past the removed ranges, it was already empty
/// in the original `text` (a documented empty envelope survives).
fn strip_emptied_wrappers(without_calls: &str, text: &str, removed: &[(usize, usize)]) -> String {
    let mut out = String::with_capacity(without_calls.len());
    let mut cursor = 0;
    let mut from = 0;
    while let Some((start, end)) = leftmost(without_calls, from, empty_wrapper_at) {
        from = end;
        let wrapper = &without_calls[start..end];
        let mut original = start;
        for &(removed_start, removed_end) in removed {
            if removed_start > original {
                break;
            }
            original += removed_end - removed_start;
        }
        let was_empty = text.get(original..original + wrapper.len()) == Some(wrapper);
        out.push_str(&without_calls[cursor..if was_empty { end } else { start }]);
        cursor = end;
    }
    out.push_str(&without_calls[cursor..]);
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const FUNCTION_BLOCK: &str =
        "<function=read_file><parameter=file_path>a.ts</parameter></function>";
    const READ_BLOCK: &str = "<function=read_file><parameter=file_path>b.ts</parameter></function>";

    fn invoke(name: &str, params: &str) -> String {
        format!("<invoke name=\"{name}\">{params}</invoke>")
    }

    fn param(name: &str, value: &str) -> String {
        format!("<parameter name=\"{name}\">{value}</parameter>")
    }

    /// `extractXmlToolCalls(text)` as `(name, args)` pairs.
    fn calls(text: &str) -> Vec<(String, Value)> {
        extract_xml_tool_calls(text)
            .into_iter()
            .map(|call| (call.name, Value::Object(call.arguments)))
            .collect()
    }

    fn call(name: &str, args: Value) -> (String, Value) {
        (name.to_owned(), args)
    }

    fn recover(text: &str) -> Recovery {
        try_recover(text).unwrap_or_else(|| panic!("expected a recovery from {text:?}"))
    }

    fn names(recovery: &Recovery) -> Vec<&str> {
        recovery.calls.iter().map(|c| c.name.as_str()).collect()
    }

    fn args(call: &Recovered) -> Value {
        Value::Object(call.arguments.clone())
    }

    // --- containsXmlToolCalls ---

    #[test]
    fn detects_an_invoke_block() {
        assert!(contains_xml_tool_calls(&invoke(
            "read_file",
            &param("p", "v")
        )));
    }

    #[test]
    fn returns_false_for_plain_text() {
        assert!(!contains_xml_tool_calls("just some text"));
    }

    #[test]
    fn is_stable_across_repeated_calls_no_last_index_leak() {
        let text = invoke("read_file", &param("p", "v"));
        for _ in 0..3 {
            assert!(contains_xml_tool_calls(&text));
        }
    }

    // --- extractXmlToolCalls ---

    #[test]
    fn extracts_a_single_tool_call() {
        let text = invoke("read_file", &param("file_path", "a.ts"));
        assert_eq!(
            calls(&text),
            [call("read_file", json!({"file_path": "a.ts"}))]
        );
    }

    #[test]
    fn extracts_multiple_tool_calls() {
        let text = invoke("read_file", &param("file_path", "a.ts"))
            + "\n"
            + &invoke("run_shell_command", &param("command", "ls"));
        assert_eq!(
            calls(&text),
            [
                call("read_file", json!({"file_path": "a.ts"})),
                call("run_shell_command", json!({"command": "ls"})),
            ]
        );
    }

    #[test]
    fn extracts_multiple_parameters_for_one_call() {
        let text = invoke(
            "edit",
            &(param("file_path", "a.ts") + &param("old_string", "x")),
        );
        assert_eq!(
            calls(&text),
            [call(
                "edit",
                json!({"file_path": "a.ts", "old_string": "x"})
            )]
        );
    }

    #[test]
    fn skips_invoke_blocks_without_parameters_conservative() {
        let text = invoke("no_params", "some body but no parameters");
        assert_eq!(calls(&text), []);
    }

    #[test]
    fn parses_structured_json_but_preserves_scalar_strings() {
        let params = [
            param("count", "3"),
            param("flag", "true"),
            param("opts", "{\"a\": 1}"),
            param("list", "[1, 2]"),
            param("plain", "hello world"),
            param("nil", "null"),
        ]
        .concat();
        assert_eq!(
            calls(&invoke("tool", &params)),
            [call(
                "tool",
                json!({
                    "count": "3",
                    "flag": "true",
                    "opts": {"a": 1},
                    "list": [1, 2],
                    "plain": "hello world",
                    "nil": "null",
                })
            )]
        );
    }

    #[test]
    fn preserves_raw_string_for_malformed_json_values() {
        let text = invoke(
            "tool",
            &(param("data", "{not valid json") + &param("ok", "yes")),
        );
        assert_eq!(
            calls(&text),
            [call(
                "tool",
                json!({"data": "{not valid json", "ok": "yes"})
            )]
        );
    }

    #[test]
    fn extracts_tool_calls_with_multi_line_parameter_values_issue_8003_shape() {
        let text = invoke(
            "edit",
            &(param("file_path", "/some/path/file.tsx")
                + &param("old_string", "line1,\nline2,\nline3,")),
        );
        assert_eq!(
            calls(&text),
            [call(
                "edit",
                json!({
                    "file_path": "/some/path/file.tsx",
                    "old_string": "line1,\nline2,\nline3,",
                })
            )]
        );
    }

    #[test]
    fn strips_only_delimiting_newlines_preserving_significant_whitespace() {
        let text = invoke("edit", &param("old_string", "\n    return null;\n"));
        assert_eq!(
            calls(&text),
            [call("edit", json!({"old_string": "    return null;"}))]
        );
    }

    #[test]
    fn does_not_crash_on_malformed_or_nested_xml() {
        assert_eq!(calls("<invoke name=\"x\"><invoke"), []);
        assert_eq!(calls("</invoke><invoke>"), []);
        // The original asserts only that an array comes back.
        let _ = calls(&invoke("outer", &invoke("inner", &param("p", "v"))));
    }

    #[test]
    fn returns_consistent_results_across_repeated_calls_no_last_index_leak() {
        let text = invoke("read_file", &param("p", "v"));
        let first = extract_xml_tool_calls(&text);
        let second = extract_xml_tool_calls(&text);
        assert_eq!(second, first);
        assert_eq!(second.len(), 1);
    }

    #[test]
    fn is_safe_against_proto_parameter_names() {
        let text = invoke(
            "tool",
            &(param("__proto__", "{\"polluted\": true}") + &param("safe", "yes")),
        );
        let result = extract_xml_tool_calls(&text);
        assert_eq!(result.len(), 1);
        let args = &result[0].arguments;
        assert_eq!(args["safe"], json!("yes"));
        // `__proto__` is an ordinary key, holding what was written.
        assert_eq!(args["__proto__"], json!({"polluted": true}));
        assert!(!args.contains_key("polluted"));
    }

    #[test]
    fn skips_invoke_blocks_inside_fenced_code_blocks() {
        let text = "```xml\n".to_owned()
            + &invoke("run_shell_command", &param("command", "rm -rf /tmp/x"))
            + "\n```";
        assert_eq!(calls(&text), []);
    }

    #[test]
    fn extracts_non_fenced_invokes_while_skipping_fenced_ones() {
        let real_call = invoke("read_file", &param("file_path", "a.ts"));
        let fenced_example = "```xml\n".to_owned()
            + &invoke("run_shell_command", &param("command", "echo hello"))
            + "\n```";
        let text = real_call + "\n" + &fenced_example;
        assert_eq!(
            calls(&text),
            [call("read_file", json!({"file_path": "a.ts"}))]
        );
    }

    /// The `~~~` fence holding ```` ``` ```` lines both "skips" tests share.
    fn tilde_fence_holding_backticks(intro: &str, command: &str) -> String {
        format!(
            "~~~markdown\n{intro}```xml\n{}\n```\n~~~",
            invoke("run_shell_command", &param("command", command))
        )
    }

    fn longer_fence_holding_shorter(closer: &str) -> String {
        format!(
            "````markdown\n```xml\n{}\n{closer}\n````",
            invoke("run_shell_command", &param("command", "rm -rf /tmp/x"))
        )
    }

    fn closing_fence_with_trailing_text() -> String {
        format!(
            "~~~markdown\n{}\n~~~ end of examples\n~~~",
            invoke("run_shell_command", &param("command", "echo hi"))
        )
    }

    #[test]
    fn skips_invokes_inside_a_tilde_fence_that_contains_backtick_lines() {
        let text = tilde_fence_holding_backticks("Here is an example:\n", "echo hello");
        assert_eq!(calls(&text), []);
    }

    #[test]
    fn treats_a_shorter_same_delimiter_fence_as_content_not_a_close_commonmark_4_5() {
        assert_eq!(calls(&longer_fence_holding_shorter("```")), []);
    }

    #[test]
    fn treats_a_closing_fence_with_an_info_string_as_content_not_a_close_commonmark_4_5() {
        assert_eq!(calls(&longer_fence_holding_shorter("```xml")), []);
    }

    #[test]
    fn treats_a_closing_fence_with_trailing_text_as_content_not_a_close() {
        assert_eq!(calls(&closing_fence_with_trailing_text()), []);
    }

    #[test]
    fn extracts_a_later_invoke_when_an_earlier_parameter_contains_an_unclosed_fence() {
        let edit_with_fence = invoke(
            "edit",
            &(param("file_path", "docs.md") + &param("old_string", "```ts\nconst x = 1;")),
        );
        let read_call = invoke("read_file", &param("file_path", "a.ts"));
        let text = edit_with_fence + "\n" + &read_call;
        assert_eq!(
            calls(&text),
            [
                call(
                    "edit",
                    json!({"file_path": "docs.md", "old_string": "```ts\nconst x = 1;"})
                ),
                call("read_file", json!({"file_path": "a.ts"})),
            ]
        );
    }

    #[test]
    fn extracts_a_later_invoke_when_an_earlier_parameter_contains_a_closed_fence_pair() {
        let edit_with_fence = invoke("edit", &param("old_string", "```\ncode\n```"));
        let read_call = invoke("read_file", &param("file_path", "b.ts"));
        let text = edit_with_fence + "\n" + &read_call;
        assert_eq!(
            calls(&text),
            [
                call("edit", json!({"old_string": "```\ncode\n```"})),
                call("read_file", json!({"file_path": "b.ts"})),
            ]
        );
    }

    #[test]
    fn still_skips_invokes_inside_a_prose_fence_when_parameters_also_contain_fences() {
        let text = "```markdown\n".to_owned()
            + &invoke("edit", &param("old_string", "```\ninner\n```"))
            + "\n```\n"
            + &invoke("read_file", &param("file_path", "c.ts"));
        assert_eq!(
            calls(&text),
            [call("read_file", json!({"file_path": "c.ts"}))]
        );
    }

    #[test]
    fn decodes_xml_entities_in_parameter_values() {
        let text = invoke(
            "edit",
            &(param("old_string", "if (a &lt; b) &amp;&amp; c &gt; d")
                + &param("new_string", "x &apos;y&apos; &quot;z&quot;")),
        );
        assert_eq!(
            calls(&text),
            [call(
                "edit",
                json!({"old_string": "if (a < b) && c > d", "new_string": "x 'y' \"z\""})
            )]
        );
    }

    #[test]
    fn decodes_amp_last_so_amp_lt_becomes_literal_lt() {
        let text = invoke("tool", &param("v", "&amp;lt;"));
        assert_eq!(calls(&text), [call("tool", json!({"v": "&lt;"}))]);
    }

    #[test]
    fn leaves_values_without_entities_unchanged() {
        let text = invoke("tool", &param("v", "plain text"));
        assert_eq!(calls(&text), [call("tool", json!({"v": "plain text"}))]);
    }

    #[test]
    fn supports_single_quoted_attribute_values() {
        let text = "<invoke name='read_file'><parameter name='file_path'>a.ts</parameter></invoke>";
        assert_eq!(
            calls(text),
            [call("read_file", json!({"file_path": "a.ts"}))]
        );
    }

    // --- tryRecoverXmlToolCalls ---

    #[test]
    fn reports_no_recovery_when_there_are_no_tool_calls() {
        assert_eq!(try_recover("plain text only"), None);
    }

    #[test]
    fn recovers_function_call_parts_from_xml_content() {
        let result = recover(&invoke("read_file", &param("file_path", "a.ts")));
        assert_eq!(result.calls.len(), 1);
        assert_eq!(result.calls[0].name, "read_file");
        assert_eq!(args(&result.calls[0]), json!({"file_path": "a.ts"}));
        // The `xml-recovered-` id assertion does not apply: the caller assigns ids.
    }

    #[test]
    fn preserves_short_surrounding_text_in_remaining_text() {
        let text = "Sure.\n".to_owned() + &invoke("read_file", &param("file_path", "a.ts"));
        assert_eq!(recover(&text).remaining, "Sure.");
    }

    #[test]
    fn returns_empty_remaining_text_when_the_content_is_only_xml() {
        let result = recover(&invoke("read_file", &param("file_path", "a.ts")));
        assert_eq!(result.remaining, "");
    }

    #[test]
    fn does_not_recover_when_substantial_prose_surrounds_the_xml() {
        let prose = "Here is how you use the tool. First you open the file, then you read it. \
                     The invoke block below shows the format. Remember to always check the path. \
                     This is a documentation example for the read_file tool call format. \
                     You should never execute these examples directly. They are for illustration \
                     purposes only. The actual tool calls are made through the structured API.";
        let text = prose.to_owned() + "\n" + &invoke("read_file", &param("file_path", "a.ts"));
        assert_eq!(try_recover(&text), None);
    }

    #[test]
    fn recovers_when_reasoning_prose_precedes_the_xml_issue_8003_shape() {
        // ~1400 chars of reasoning prose, then a ~600-byte edit invoke with
        // multi-line params: prose ratio ~0.70, which must pass the 0.8 guard.
        let reasoning = "I need to fix the authentication token validation in the middleware. \
            The current implementation does not check the expiry date, which means \
            expired tokens are still accepted. This is a security vulnerability that \
            could allow unauthorized access. I will update the validateToken function \
            to check the exp claim and reject tokens that have expired. The fix involves \
            adding a date comparison after the signature verification step. I also need \
            to make sure the error message is clear about why the token was rejected. \
            Let me look at the current implementation and make the necessary changes. \
            The file is located in the src/middleware directory. I will use the edit tool \
            to replace the old validation logic with the new one that includes expiry \
            checking. This should be a straightforward change that does not affect other \
            parts of the codebase. The test suite should still pass after this change. \
            I have verified that no other middleware depends on the old behavior. \
            The change is backward compatible because valid tokens will still be accepted.";
        let edit_block = invoke(
            "edit",
            &[
                param("file_path", "/project/src/middleware/auth.ts"),
                param(
                    "old_string",
                    "function validateToken(token: string): boolean {\n  \
                     const decoded = jwt.verify(token, SECRET);\n  \
                     return decoded !== null;\n}",
                ),
                param(
                    "new_string",
                    "function validateToken(token: string): boolean {\n  \
                     const decoded = jwt.verify(token, SECRET);\n  \
                     if (!decoded || !decoded.exp) return false;\n  \
                     return Date.now() < decoded.exp * 1000;\n}",
                ),
            ]
            .concat(),
        );
        let text = reasoning.to_owned() + "\n" + &edit_block;
        let result = recover(&text);
        assert_eq!(result.calls.len(), 1);
        assert_eq!(result.calls[0].name, "edit");
        for key in ["file_path", "old_string", "new_string"] {
            assert!(result.calls[0].arguments.contains_key(key), "{key}");
        }
        assert_eq!(result.remaining, reasoning);
    }

    #[test]
    fn preserves_parameterless_invoke_blocks_as_plain_text() {
        let parameterless = invoke("think", "Let me reason about this problem");
        let parameterized = invoke("read_file", &param("file_path", "a.ts"));
        let text = parameterized + "\n" + &parameterless;
        let result = recover(&text);
        assert_eq!(result.calls.len(), 1);
        assert!(result.remaining.contains(&parameterless));
    }

    #[test]
    fn does_not_recover_an_invoke_example_inside_a_fenced_code_block() {
        let text = "```xml\n".to_owned()
            + &invoke("run_shell_command", &param("command", "rm -rf /tmp/x"))
            + "\n```";
        assert_eq!(try_recover(&text), None);
    }

    #[test]
    fn recovers_a_real_invoke_while_excluding_a_fenced_example_after_it() {
        let real_call = invoke("read_file", &param("file_path", "a.ts"));
        let fenced_example = "```xml\n".to_owned()
            + &invoke("run_shell_command", &param("command", "echo hello"))
            + "\n```";
        let result = recover(&(real_call + "\n" + &fenced_example));
        assert_eq!(names(&result), ["read_file"]);
        assert_eq!(args(&result.calls[0]), json!({"file_path": "a.ts"}));
        assert!(result.remaining.contains("```xml"));
        assert!(result.remaining.contains("echo hello"));
    }

    #[test]
    fn does_not_recover_an_invoke_inside_a_tilde_fence_containing_backtick_lines() {
        let text = tilde_fence_holding_backticks("Example:\n", "echo hi");
        assert_eq!(try_recover(&text), None);
    }

    #[test]
    fn does_not_recover_an_invoke_nested_in_a_longer_same_delimiter_fence() {
        assert_eq!(try_recover(&longer_fence_holding_shorter("```")), None);
    }

    #[test]
    fn does_not_recover_an_invoke_when_the_closing_fence_carries_an_info_string() {
        assert_eq!(try_recover(&longer_fence_holding_shorter("```xml")), None);
    }

    #[test]
    fn does_not_recover_an_invoke_when_the_closing_fence_has_trailing_text() {
        assert_eq!(try_recover(&closing_fence_with_trailing_text()), None);
    }

    #[test]
    fn recovers_both_invokes_when_the_first_has_a_fence_like_parameter_value() {
        let edit_with_fence = invoke("edit", &param("old_string", "```\nunclosed fence"));
        let read_call = invoke("read_file", &param("file_path", "a.ts"));
        let result = recover(&(edit_with_fence + "\n" + &read_call));
        assert_eq!(names(&result), ["edit", "read_file"]);
    }

    #[test]
    fn strips_an_empty_function_calls_wrapper_from_remaining_text() {
        let text = "<function_calls>\n".to_owned()
            + &invoke("read_file", &param("file_path", "a.ts"))
            + "\n</function_calls>";
        assert_eq!(recover(&text).remaining, "");
    }

    // --- complete taught-dialect recovery (#10692) ---

    #[test]
    fn recovers_a_complete_function_block() {
        for text in [
            FUNCTION_BLOCK.to_owned(),
            format!("<tool_call>{FUNCTION_BLOCK}</tool_call>"),
        ] {
            assert!(contains_xml_tool_calls(&text), "{text}");
            let result = recover(&text);
            assert_eq!(names(&result), ["read_file"], "{text}");
            assert_eq!(args(&result.calls[0]), json!({"file_path": "a.ts"}));
            assert_eq!(result.calls[0].source, FUNCTION_BLOCK);
            assert_eq!(result.remaining, "", "{text}");
        }
    }

    #[test]
    fn preserves_explicit_examples_while_recovering_a_following_real_call() {
        let documentation = format!("<example>model:\n{FUNCTION_BLOCK}</example>");
        assert_eq!(try_recover(&documentation), None);
        let result = recover(&format!("{documentation}\n{FUNCTION_BLOCK}"));
        assert_eq!(names(&result), ["read_file"]);
        assert_eq!(result.remaining, documentation);
    }

    #[test]
    fn ignores_an_inline_code_example_mention_before() {
        for call in [
            FUNCTION_BLOCK,
            "<invoke name=\"read_file\"><parameter name=\"file_path\">a.ts</parameter></invoke>",
        ] {
            let prose = "See the `<example>` format.";
            let result = recover(&format!("{prose}\n{call}"));
            assert_eq!(names(&result), ["read_file"], "{call}");
            assert_eq!(result.remaining, prose, "{call}");
        }
    }

    #[test]
    fn keeps_a_genuinely_unclosed_example_inert() {
        let documentation = format!("<example>model:\n{FUNCTION_BLOCK}");
        assert_eq!(try_recover(&documentation), None);
    }

    #[test]
    fn keeps_parameter_backticks_from_masking_a_later_example_opener() {
        let write = "<function=write_file><parameter=file_path>a.ts</parameter>\
                     <parameter=content>`</parameter></function>";
        let documentation = format!("<example>model:\n{FUNCTION_BLOCK}\nclosing `</example>");
        let result = recover(&format!("{write}\n{documentation}"));
        assert_eq!(names(&result), ["write_file"]);
        assert_eq!(
            args(&result.calls[0]),
            json!({"file_path": "a.ts", "content": "`"})
        );
        assert_eq!(result.remaining, documentation);
    }

    #[test]
    fn preserves_example_attributes_and_whitespace() {
        for (open, close) in [
            ("<example id=\"one > two\">", "</example>"),
            ("<example >", "</example >"),
        ] {
            let documentation = format!("{open}{FUNCTION_BLOCK}{close}");
            let result = recover(&format!("{documentation}\n{FUNCTION_BLOCK}"));
            assert_eq!(result.calls.len(), 1, "{open}");
            assert_eq!(result.remaining, documentation, "{open}");
        }
    }

    #[test]
    fn ignores_a_literal_example_opener_in_fenced_documentation() {
        let documentation = "```xml\n<example>\n```";
        let result = recover(&format!("{documentation}\n{FUNCTION_BLOCK}"));
        assert_eq!(result.calls.len(), 1);
        assert_eq!(result.remaining, documentation);
    }

    #[test]
    fn keeps_example_tags_in_parameter_data_from_hiding_a_following_real_call() {
        let write = "<function=write_file><parameter=file_path>a.ts</parameter>\
                     <parameter=content><example>literal data</parameter></function>";
        let found: Vec<String> = calls(&format!("{write}\n{FUNCTION_BLOCK}"))
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        assert_eq!(found, ["write_file", "read_file"]);
    }

    #[test]
    fn preserves_parameter_values_json_structure_and_null_prototype_args() {
        let found = calls(
            "<function=write_file>\
             <parameter=file_path>null</parameter>\
             <parameter=content>\n    a &lt; b &amp;&amp; c\n</parameter>\
             <parameter=options>{\"x\":[1,2]}</parameter>\
             <parameter=__proto__>value</parameter>\
             </function>",
        );
        // `__proto__` is an ordinary key; there is no prototype to check.
        assert_eq!(
            found,
            [call(
                "write_file",
                json!({
                    "file_path": "null",
                    "content": "    a < b && c",
                    "options": {"x": [1, 2]},
                    "__proto__": "value",
                })
            )]
        );
    }

    #[test]
    fn recovers_mixed_dialects_while_retaining_fenced_and_parameterless_blocks() {
        let documented = format!("```xml\n{FUNCTION_BLOCK}\n```");
        let parameterless = "<function=no_params></function>";
        let text = format!(
            "<tool_call>{FUNCTION_BLOCK}</tool_call>\n{}\n{documented}\n{parameterless}",
            invoke("run_shell_command", &param("command", "pwd"))
        );
        let result = recover(&text);
        assert_eq!(names(&result), ["read_file", "run_shell_command"]);
        assert_eq!(result.remaining, format!("{documented}\n{parameterless}"));
    }

    #[test]
    fn preserves_an_originally_empty_envelope_after() {
        let invoked = invoke("read_file", &param("file_path", "a.ts"));
        let enveloped = format!("<tool_call>{FUNCTION_BLOCK}</tool_call>");
        for (call, documentation) in [
            (invoked.as_str(), "<tool_call></tool_call>"),
            (enveloped.as_str(), "<tool_call></tool_call>"),
            (invoked.as_str(), "```xml\n<tool_call></tool_call>\n```"),
            (FUNCTION_BLOCK, "```xml\n<tool_call> \n</tool_call>\n```"),
        ] {
            let result = recover(&format!("{call}\n{documentation}"));
            assert_eq!(result.calls.len(), 1, "{call} / {documentation}");
            assert_eq!(result.remaining, documentation, "{call}");
        }
    }

    #[test]
    fn keeps_parameter_fences_from_hiding_a_later_function_block() {
        let text = format!(
            "<function=edit><parameter=old_string>\n```\n</parameter></function>\n{FUNCTION_BLOCK}"
        );
        let found: Vec<String> = calls(&text).into_iter().map(|(name, _)| name).collect();
        assert_eq!(found, ["edit", "read_file"]);
    }

    #[test]
    fn does_not_join_a_prose_opener_to_fenced_parameters() {
        for text in [
            "The shell format is <function=run_shell_command> with a command:\n\
             ```xml\n<parameter=command>echo example</parameter></function>\n```",
            "<function=run_shell_command>\n```xml\n\
             <parameter=command>echo example</parameter>\n```\n</function>",
        ] {
            assert_eq!(try_recover(text), None, "{text}");
        }
    }

    #[test]
    fn preserves_malformed_blocks_instead_of_dispatching_partial_calls() {
        for text in [
            format!("<function=run_shell_command>\n```xml\n{FUNCTION_BLOCK}\n```"),
            "<tool_call><function=write_file>\
             <parameter=file_path>a.ts</parameter>\
             <parameter=content>before</function>after</parameter></function></tool_call>"
                .to_owned(),
        ] {
            assert_eq!(try_recover(&text), None, "{text}");
        }
    }

    #[test]
    fn recovers_the_intact_call_after_an_envelope_whose_block_never_closed() {
        // The first envelope never closes its function block, so nothing may
        // be dispatched from it, but the rescan still finds the intact second
        // call, and the malformed envelope stays visible.
        let truncated =
            "<tool_call><function=read_file><parameter=file_path>a.ts</parameter></tool_call>";
        let intact = "<tool_call><function=run_shell_command><parameter=command>pwd</parameter>\
                      </function></tool_call>";
        let result = recover(&format!("{truncated}{intact}"));
        assert_eq!(names(&result), ["run_shell_command"]);
        assert_eq!(args(&result.calls[0]), json!({"command": "pwd"}));
        assert_eq!(result.remaining, truncated);
    }

    #[test]
    fn preserves_a_parameterless_block_whose_name_contains_parameter_syntax() {
        let parameterless = "<invoke name='<parameter=x>y</parameter>'></invoke>";
        let result = recover(&format!("{FUNCTION_BLOCK}\n{parameterless}"));
        assert_eq!(result.calls.len(), 1);
        assert_eq!(result.remaining, parameterless);
    }

    #[test]
    fn does_not_recover_documentation_or_incomplete_mismatched_blocks() {
        for text in [
            format!("```xml\n{FUNCTION_BLOCK}\n```"),
            format!("{}{FUNCTION_BLOCK}", "Explanation. ".repeat(80)),
            "<function=read_file><parameter=file_path>a.ts</parameter>".to_owned(),
            "<function=read_file><parameter=file_path>a.ts</function>".to_owned(),
            "<invoke name=\"read_file\"><parameter=file_path>a.ts</parameter></function>"
                .to_owned(),
        ] {
            assert_eq!(try_recover(&text), None, "{text}");
        }
    }

    // --- borrowed closers, lexer cost and rejected-block masking ---

    #[test]
    fn does_not_dispatch_a_truncated_block_that_borrows_the_next_call_closers() {
        let text = "<tool_call>\n<function=write_file>\n\
                    <parameter=file_path>a.txt</parameter>\n\
                    <parameter=content>hello\n\
                    <tool_call>\n<function=run_shell_command>\
                    <parameter=command>pwd</parameter></function>\n</tool_call>";
        let result = recover(text);
        assert_eq!(names(&result), ["run_shell_command"]);
        assert_eq!(args(&result.calls[0]), json!({"command": "pwd"}));
        // The truncated block stays visible instead of being dispatched with
        // the next call's markup as its content.
        assert!(result.remaining.contains("hello"));
        assert!(!result.remaining.contains("</function>"));
    }

    #[test]
    fn leaves_a_truncated_block_inert_when_no_donor_close_follows() {
        let text = "<function=write_file><parameter=file_path>a.txt</parameter>\
                    <parameter=content>hello";
        assert_eq!(try_recover(text), None);
    }

    #[test]
    fn recovers_a_value_that_mentions_the_parameter_syntax_literally() {
        let content = "Each argument is wrapped in <parameter name=\"x\"> tags.";
        let write = invoke(
            "write_file",
            &(param("file_path", "a.txt") + &param("content", content)),
        );
        assert_eq!(
            calls(&write),
            [call(
                "write_file",
                json!({"file_path": "a.txt", "content": content})
            )]
        );
    }

    #[test]
    fn does_not_run_the_markdown_lexer_when_the_text_has_no_example_tag() {
        // Behaviour only: the original also asserts the lexer was not called.
        let text = "[a](".repeat(50) + "\n" + READ_BLOCK;
        assert_eq!(names(&recover(&text)), ["read_file"]);
    }

    #[test]
    fn still_runs_the_markdown_lexer_when_an_example_tag_is_present() {
        // Behaviour only: the original also asserts the lexer was called.
        let documentation = format!("<example>model:\n{READ_BLOCK}</example>");
        assert_eq!(try_recover(&documentation), None);
    }

    #[test]
    fn skips_the_lexer_past_the_length_cap_while_still_honouring_examples() {
        // Behaviour only: the original also asserts whether the lexer ran.
        let over_cap = format!(
            "<example>model:\n{}{}{READ_BLOCK}</example>",
            "*a ".repeat(2000),
            "x".repeat(64 * 1024)
        );
        assert!(over_cap.encode_utf16().count() > MAX_LEXER_SCAN_LENGTH);
        assert_eq!(try_recover(&over_cap), None);

        let under_cap = format!("<example>model:\n{READ_BLOCK}</example>");
        assert!(under_cap.encode_utf16().count() <= MAX_LEXER_SCAN_LENGTH);
        assert_eq!(try_recover(&under_cap), None);
    }

    #[test]
    fn preserves_examples_when_nested_emphasis_overflows_the_markdown_lexer() {
        let prose = "*".repeat(4096) + "nested emphasis" + &"*".repeat(4096);
        let documented_call = invoke(
            "write_file",
            &(param("file_path", "example.txt") + &param("content", &"x".repeat(3000))),
        );
        let documentation = format!("<example>model:\n{documented_call}</example>");
        let text = format!("{prose}\n{documentation}\n{READ_BLOCK}");
        assert!(text.encode_utf16().count() < MAX_LEXER_SCAN_LENGTH);

        let result = recover(&text);
        assert_eq!(names(&result), ["read_file"]);
        assert_eq!(args(&result.calls[0]), json!({"file_path": "b.ts"}));
        assert!(result.remaining.contains(&documentation));
    }

    #[test]
    fn does_not_let_a_rejected_block_parameter_swallow_a_later_valid_call() {
        // The write_file block is rejected (its content parameter borrows the
        // function close tag), yet its parameter data must still be masked
        // out of the prose, or the literal example opener would swallow
        // everything to the end of the text.
        let rejected = "<function=write_file><parameter=file_path>a.txt</parameter>\
                        <parameter=content><example>note</function>tail</parameter></function>";
        let result = recover(&format!("{rejected}\n{READ_BLOCK}"));
        assert_eq!(names(&result), ["read_file"]);
        assert!(result.remaining.contains("<example>note"));
    }

    #[test]
    fn does_not_dispatch_a_complete_call_embedded_in_a_rejected_block_name() {
        // The invoke name admits `>`, so this block's open tag ends after the
        // quoted name, not at the first `>` in it.
        let text = "<invoke name=\"a>b<function=run><parameter=cmd>ls</parameter></function>\">\
                    tail<parameter=x></invoke>";
        assert_eq!(try_recover(text), None);
    }

    #[test]
    fn does_not_dispatch_a_call_quoted_inside_a_parameter_value() {
        let quoted = invoke("run_shell_command", &param("command", "rm -rf /tmp/x"));
        let text = invoke(
            "write_file",
            &(param("file_path", "doc.md") + &param("content", &format!("Usage:\n{quoted}\n"))),
        );
        assert_eq!(calls(&text), []);
        assert_eq!(try_recover(&text), None);
    }

    #[test]
    fn still_dispatches_a_real_call_that_follows_a_value_quoting_one() {
        let quoted = invoke("run_shell_command", &param("command", "rm -rf /tmp/x"));
        let documented = invoke(
            "write_file",
            &param("content", &format!("Usage:\n{quoted}\n")),
        );
        let text = documented + "\n" + &invoke("read_file", &param("p", "b.ts"));
        assert_eq!(calls(&text), [call("read_file", json!({"p": "b.ts"}))]);
    }
}
