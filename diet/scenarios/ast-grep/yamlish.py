"""The YAML an `--inline-rules` answer holds, read for the static grader (#971): `load(text)`, or `--selftest`.

Not a YAML parser, and never a reader of any format of this program's: the subset ast-grep rules are written in, so
the rubric can see a rule's shape (`kind`, `pattern`, `has`, `inside`, `stopBy`, `all`/`any`/`not`). It reads block
mappings and sequences by indentation, flow mappings and sequences (`{a: b, c: [d, e]}`), plain, single- and
double-quoted scalars, and `#` comments. Anything else (anchors, tags, block scalars `|` and `>`, multi-documents)
raises ValueError, and the rubric calls the answer unrecognized rather than guessing.
"""
import re, sys


def scalar(text):
    t = text.strip()
    if len(t) >= 2 and t[0] == t[-1] == "'":
        return t[1:-1].replace("''", "'")
    if len(t) >= 2 and t[0] == t[-1] == '"':
        return bytes(t[1:-1], "utf-8").decode("unicode_escape")
    if t[:1] in "&*!|>%@`":
        raise ValueError(f"unsupported YAML: {t[:20]!r}")
    return t


def strip_comment(line):
    out, q = [], None
    for i, c in enumerate(line):
        if q:
            if c == q:
                q = None
        elif c in "'\"":
            q = c
        elif c == "#" and (i == 0 or line[i - 1] in " \t"):
            break
        out.append(c)
    return "".join(out).rstrip()


def flow(text):
    """A flow node: `{...}`, `[...]` or a scalar."""
    value, rest = _flow(text.strip(), 0)
    if text.strip()[rest:].strip():
        raise ValueError(f"trailing text in flow node: {text[rest:]!r}")
    return value


def _flow(s, i):
    while i < len(s) and s[i] == " ":
        i += 1
    if i < len(s) and s[i] in "{[":
        close = "}" if s[i] == "{" else "]"
        mapping = s[i] == "{"
        out = {} if mapping else []
        i += 1
        while True:
            while i < len(s) and s[i] in " ,":
                i += 1
            if i >= len(s):
                raise ValueError("unclosed flow node")
            if s[i] == close:
                return out, i + 1
            if mapping:
                key, i = _token(s, i, ":")
                if i >= len(s) or s[i] != ":":
                    raise ValueError(f"a flow key without a value: {key!r}")
                value, i = _flow(s, i + 1)
                out[scalar(key)] = value
            else:
                value, i = _flow(s, i)
                out.append(value)
    tok, i = _token(s, i, ",}]")
    return scalar(tok), i


def _token(s, i, stops):
    start, q = i, None
    while i < len(s):
        c = s[i]
        if q:
            if c == q:
                q = None
        elif c in "'\"" and i == start:
            q = c
        elif c in stops and (c != ":" or i + 1 >= len(s) or s[i + 1] in " \t"):
            break
        i += 1
    return s[start:i].strip(), i


def load(text):
    lines = []
    for raw in text.replace("\t", "  ").split("\n"):
        line = strip_comment(raw)
        if line.strip() in ("---", "..."):
            raise ValueError("unsupported YAML: document markers")
        if line.strip():
            lines.append((len(line) - len(line.lstrip(" ")), line.strip()))
    if not lines:
        raise ValueError("empty")
    if len(lines) == 1 and lines[0][1][:1] in "{[":
        return flow(lines[0][1])
    value, i = _block(lines, 0, lines[0][0])
    if i != len(lines):
        raise ValueError(f"unexpected indentation at {lines[i][1]!r}")
    return value


def _block(lines, i, indent):
    if lines[i][1].startswith("- ") or lines[i][1] == "-":
        out = []
        while i < len(lines) and lines[i][0] == indent and (lines[i][1].startswith("- ") or lines[i][1] == "-"):
            item = lines[i][1][1:].strip()
            if not item:
                value, i = _block(lines, i + 1, lines[i + 1][0])
            else:
                # The item's first line sits two to the right of its dash, so the rest of it reads as a mapping there.
                inner = indent + (len(lines[i][1]) - len(lines[i][1][1:].lstrip()))
                sub = [(inner, item)] + []
                j = i + 1
                while j < len(lines) and lines[j][0] > indent:
                    sub.append(lines[j])
                    j += 1
                value = _block(sub, 0, inner)[0] if (":" in item and not item.startswith(("{", "[", "'", '"'))) else flow(item)
                i = j
            out.append(value)
        return out, i
    out = {}
    while i < len(lines) and lines[i][0] == indent:
        key, sep, rest = lines[i][1].partition(":")
        if not sep or (rest and rest[0] not in " \t"):
            raise ValueError(f"not a mapping line: {lines[i][1]!r}")
        rest = rest.strip()
        if rest in ("|", ">", "|-", ">-"):
            raise ValueError("unsupported YAML: block scalars")
        if rest:
            # Only a value that opens with a bracket is flow; a plain scalar may hold braces (`export type { $$$ }`).
            out[scalar(key)] = flow(rest) if rest[:1] in "{[" else scalar(rest)
            i += 1
        elif i + 1 < len(lines) and lines[i + 1][0] > indent:
            out[scalar(key)], i = _block(lines, i + 1, lines[i + 1][0])
        elif i + 1 < len(lines) and lines[i + 1][0] == indent and lines[i + 1][1].startswith("- "):
            out[scalar(key)], i = _block(lines, i + 1, indent)  # a sequence at the key's own indentation
        else:
            out[scalar(key)] = None
            i += 1
    return out, i


CASES = [
    ("block", "id: a\nlanguage: ts\nrule:\n  kind: function_declaration\n  has:\n    pattern: await $E\n    stopBy: end",
     {"id": "a", "language": "ts", "rule": {"kind": "function_declaration", "has": {"pattern": "await $E", "stopBy": "end"}}}),
    ("flow", "{id: f, language: typescript, rule: {kind: function_declaration, has: {field: body, kind: await_expression, stopBy: end}}}",
     {"id": "f", "language": "typescript", "rule": {"kind": "function_declaration", "has": {"field": "body", "kind": "await_expression", "stopBy": "end"}}}),
    ("a sequence of mappings", "rule:\n  all:\n    - has:\n        kind: export_clause\n        stopBy: end\n    - pattern: export type { $$$ }",
     {"rule": {"all": [{"has": {"kind": "export_clause", "stopBy": "end"}}, {"pattern": "export type { $$$ }"}]}}),
    ("quoted values and a comment", "rule:\n  pattern: 'foo($$$A)' # the call\n  regex: \"^bar$\"",
     {"rule": {"pattern": "foo($$$A)", "regex": "^bar$"}}),
    ("a regex with a backslash", "rule:\n  regex: ^const\\b", {"rule": {"regex": "^const\\b"}}),
    ("a flow sequence", "rule:\n  any: [{pattern: a($$$)}, {pattern: b($$$)}]", {"rule": {"any": [{"pattern": "a($$$)"}, {"pattern": "b($$$)"}]}}),
]
REFUSED = [("a block scalar", "rule:\n  pattern: |\n    foo()"), ("an anchor", "rule: &r\n  kind: x"),
           ("document markers", "---\nrule:\n  kind: x"), ("not a mapping", "rule\n  kind x"), ("unclosed flow", "{rule: {kind: x}")]


def selftest():
    bad = 0
    for why, text, want in CASES:
        try:
            got = load(text)
        except ValueError as e:
            got = f"refused: {e}"
        if got != want:
            print(f"yamlish: {why}: {got!r}"); bad += 1
    for why, text in REFUSED:
        try:
            load(text)
            print(f"yamlish: not refused: {why}"); bad += 1
        except ValueError:
            pass
    print(f"yamlish: {len(CASES)} read and {len(REFUSED)} refused, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest())
