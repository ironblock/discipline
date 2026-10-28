"""The static grader (#971, the maintainer 2026-10-27): the answer is never run. It is read (`command.py`'s
clap-faithful `parse`) into a canonical form and judged against a rubric per operation: the forms known to be right.

`judge(command, family, params)` is `(verdict, why)`, the verdict `right`, `wrong` or `unrecognized` (a form the
rubric doesn't know, graded wrong and reported, so the audit can show whether the rubric should learn it). Read:
- a pattern with its metavariables' names and all whitespace dropped (`$A`, `$NAME` and `$_` are one; `$$$` and
  `$$$ARGS` one), so a rename or a reflow is the same pattern; a literal `$emit` is not a metavariable;
- an `--inline-rules` rule through `yamlish.py`, for its `kind`, `pattern`, `regex` and relational keys; a relation
  that must reach any depth must say `stopBy: end`;
- paths normalised (`./src/` is `src`); a search of the whole project is `src`, `.` or no path; `--lang`, when
  given, `ts` or `typescript`.
"As specific as necessary": a form that also matches what the task excludes is wrong (an unanchored outline
`--match`, which matches names, signatures and first lines, ruled wrong by the maintainer 2026-10-27; an arrow rule
without its `const`). DRAFT and
provisional: the rubric is a judgment compiled into data, and the audit (`audit.py`) says where it disagrees.
"""
import re, sys

sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
import command, yamlish  # noqa: E402

META = re.compile(r"\$\$\$[A-Z0-9_]*|\$[A-Z_][A-Z0-9_]*")


def canon(pattern):
    """A pattern with metavariable names and whitespace dropped."""
    return re.sub(r"\s+", "", META.sub(lambda m: "$$$" if m.group().startswith("$$$") else "$M", pattern))


def langs_ok(form):
    return all(v.lower() in ("ts", "typescript") for v in form["opts"].get("--lang", []))


def whole_project(form):
    return all(p in ("src", ".") for p in form["paths"])


def only(form, path):
    return form["paths"] == [path]


def one(form, name):
    vals = form["opts"].get(name, [])
    return vals[0] if len(vals) == 1 else None


def rule_of(form):
    """The scan's rule mapping, or None when it has none it can read."""
    text = one(form, "--inline-rules")
    if text is None:
        return None
    try:
        doc = yamlish.load(text)
    except ValueError:
        return None
    if not isinstance(doc, dict) or str(doc.get("language", "")).lower() not in ("ts", "typescript"):
        return None
    return doc.get("rule") if isinstance(doc.get("rule"), dict) else None


def deep(rel):
    return isinstance(rel, dict) and rel.get("stopBy") == "end"


def pattern_is(node, *forms):
    return isinstance(node, dict) and isinstance(node.get("pattern"), str) and canon(node["pattern"]) in {canon(f) for f in forms}


def search(form, *patterns):
    """A run whose pattern is one of PATTERNS, or a scan whose rule is only that pattern."""
    if form["sub"] == "run":
        p = one(form, "--pattern")
        return p is not None and canon(p) in {canon(x) for x in patterns}
    if form["sub"] == "scan":
        r = rule_of(form)
        return r is not None and set(r) == {"pattern"} and pattern_is(r, *patterns)
    return False


ANNOTATION = ("^:\\s*{}$", "^: {}$", "^:\\s+{}$", "^:\\s?{}$")


def type_wrong(node, name):
    """Why a return-type relation is wrong by name, or None: an annotation regex not anchored at both ends, or a type
    identifier reached at any depth (`stopBy: end`), which also matches `Promise<NAME>`."""
    if not isinstance(node, dict) or not (node.get("field") == "return_type" or node.get("kind") == "type_annotation"):
        return None
    rx = node.get("regex")
    if isinstance(rx, str) and name in rx and rx not in {a.format(name) for a in ANNOTATION}:
        return "the annotation's regex isn't anchored to the whole annotation: it also matches what only contains the type"
    inner = node.get("has")
    if isinstance(inner, dict) and deep(inner) and inner.get("kind") == "type_identifier":
        return "the type identifier at any depth (stopBy: end) also matches Promise<T> and other wrapped types"
    return None


def direct_type(node, name):
    """A `has` reaching the return type, whose own `has` is the type identifier NAME, one level down (so a
    `Promise<NAME>` doesn't match)."""
    if not isinstance(node, dict) or not (node.get("field") == "return_type" or node.get("kind") == "type_annotation"):
        return False
    if pattern_is(node, f": {name}"):
        return True
    # The annotation itself, its colon included, matched whole (the live answers' form, rubric review 2026-10-27).
    if not deep(node) and node.get("regex") in {a.format(name) for a in ANNOTATION}:
        return True
    inner = node.get("has")
    return (isinstance(inner, dict) and not deep(inner) and inner.get("kind") == "type_identifier"
            and inner.get("regex") == f"^{name}$")


def outline_named(form, name, kind, need_structure):
    m = one(form, "--match")
    if form["sub"] != "outline" or not whole_project(form) or not langs_ok(form):
        return "wrong", "an outline of the project is wanted"
    if m is None:
        return "wrong", "no --match: every item is outlined"
    if m != f"^{name}$":
        if m == name:
            return "wrong", "--match is not anchored: it matches names, signatures and first lines"
        if m.strip("^$") == name:
            return "wrong", "--match is anchored at one end only"
        return ("wrong", "--match is not anchored: it matches names, signatures and first lines") if name in m else ("wrong", "--match names another item")
    types = one(form, "--type")
    if types is not None:
        if kind is None:
            return "wrong", "a --type filter on a kind the ask doesn't name: right only where the item happens to be that kind"
        if kind not in types.split(","):
            return "wrong", f"--type leaves out {kind}"
    items = one(form, "--items")
    if need_structure and items != "structure":
        return "wrong", "on a directory outline lists only exports; this item isn't exported (--items structure)"
    return "right", "an anchored outline"


def judge(cmd, family, p):
    try:
        f = command.parse(cmd)
    except command.Refused as why:
        return "wrong", f"refused: {why}"
    if not langs_ok(f):
        return "wrong", "--lang is not TypeScript"
    if family == "ret-decl":
        if not whole_project(f):
            return "wrong", "not the whole project"
        if search(f, f"function $F($$$A): {p['target']} {{ $$$B }}"):
            return "right", "the return-type pattern"
        r = rule_of(f) if f["sub"] == "scan" else None
        if r and r.get("kind") == "function_declaration" and direct_type(r.get("has"), p["target"]):
            return "right", "a return-type rule"
        if r and r.get("kind") == "function_declaration" and type_wrong(r.get("has"), p["target"]):
            return "wrong", type_wrong(r.get("has"), p["target"])
        if f["sub"] == "run" and canon(one(f, "--pattern") or "") == canon("function $F($$$A) { $$$B }"):
            return "wrong", "no return type: it matches untyped functions instead"
        return "unrecognized", "not a form the rubric knows"
    if family == "ret-arrow":
        if not whole_project(f):
            return "wrong", "not the whole project"
        if search(f, f"const $N = ($$$A): {p['arrow_target']} => $B", f"const $N = ($$$A): {p['arrow_target']} => $$$B"):
            return "right", "the const arrow pattern"
        r = rule_of(f) if f["sub"] == "scan" else None
        if r and r.get("kind") == "arrow_function" and type_wrong(r.get("has"), p["arrow_target"]):
            return "wrong", type_wrong(r.get("has"), p["arrow_target"])
        if r and r.get("kind") == "arrow_function" and direct_type(r.get("has"), p["arrow_target"]):
            consts = str(r.get("inside", "")).count("const")
            return ("right", "an arrow return-type rule within a const") if consts else ("wrong", "not only const arrows")
        return "unrecognized", "not a form the rubric knows"
    if family == "log-in-try" and f["sub"] == "run" and "try_statement" in (one(f, "--kind") or ""):
        return "wrong", "a kind selector over every call in a try: it also matches calls that aren't the log function's"
    if family in ("zero-calls", "emit-calls", "track-calls"):
        name = {"zero-calls": p["zero_fn"], "emit-calls": "$emit", "track-calls": "$track"}[family]
        if not whole_project(f):
            return "wrong", "not the whole project"
        if search(f, f"{name}($$$A)"):
            return "right", "the call pattern"
        pat = one(f, "--pattern") or ""
        if canon(pat) == canon(f"{name}($A)"):
            return "wrong", "one argument only: `$$$` takes any number"
        return "unrecognized", "not a call pattern the rubric knows"
    if family == "maybe-defined":
        return outline_named(f, p["maybe"], None, False)
    if family == "fn-outline":
        return outline_named(f, p["line_fn"], "function", not p.get("line_fn_exported", True))
    if family == "itf-outline":
        return outline_named(f, p["line_itf"], "interface", False)
    if family == "public-surface":
        if f["sub"] == "run" and one(f, "--kind") == "export_statement" and not one(f, "--pattern"):
            if not only(f, "src/index.ts"):
                return "wrong", "not the entry point alone: a directory's exports are not the surface"
            return "right", "every export statement of the entry point"
        if f["sub"] != "outline":
            return "unrecognized", "not an outline"
        if not only(f, "src/index.ts"):
            return "wrong", "not the entry point alone: a directory's exports are not the surface"
        if one(f, "--match") or one(f, "--type"):
            return "wrong", "a filter leaves exports out"
        return "right", "the entry point's outline"
    if family == "public-types":
        if not only(f, "src/index.ts"):
            return "wrong", "not the entry point alone"
        if search(f, "export type { $$$S }", "export type { $$$S } from $$$P", "export type { $$$S } from $P", "export type { $$$S } from '$P'"):
            return "right", "the type-export pattern"
        return "unrecognized", "not a form the rubric knows"
    if family == "await-any":
        r = rule_of(f) if f["sub"] == "scan" else None
        if not r or not whole_project(f):
            return ("wrong", "a relation needs a rule") if f["sub"] != "scan" else ("unrecognized", "a rule the rubric can't read")
        has = r.get("has")
        if r.get("kind") != "function_declaration" or not isinstance(has, dict):
            return "unrecognized", "not a function_declaration with `has`"
        if not (pattern_is(has, "await $E") or has.get("kind") == "await_expression"):
            return "unrecognized", "`has` is not an await"
        return ("right", "has await, stopBy end") if deep(has) else ("wrong", "`has` without stopBy: end looks one level down")
    if family == "log-in-try":
        r = rule_of(f) if f["sub"] == "scan" else None
        if not r or not whole_project(f):
            return ("wrong", "a relation needs a rule") if f["sub"] != "scan" else ("unrecognized", "a rule the rubric can't read")
        if set(r) == {"all"} and isinstance(r["all"], list) and all(isinstance(x, dict) for x in r["all"]):
            merged = {}
            for x in r["all"]:  # `all:` of a pattern and an inside, in either order, is the one rule
                if set(x) & set(merged):
                    merged = None
                    break
                merged.update(x)
            r = merged or r
        inside = r.get("inside")
        if not pattern_is(r, f"{p['log_fn']}($$$A)") or not isinstance(inside, dict) or inside.get("kind") != "try_statement":
            return "unrecognized", "not the call inside a try_statement"
        return ("right", "inside try, stopBy end") if deep(inside) else ("wrong", "`inside` without stopBy: end looks one level up")
    raise KeyError(family)


P = {"target": "Relay", "arrow_target": "Socket", "zero_fn": "probeMeadow", "maybe": "VertexSpec", "log_fn": "loadMeadow",
     "line_fn": "loadQuota", "line_fn_exported": True, "line_itf": "GarnetRow"}
RULE = lambda body: "ast-grep scan --inline-rules 'id: q\nlanguage: ts\nrule:\n" + body + "' src"
# Per operation: right spellings (references and their equivalents), and wrong ones (naive tries, near-misses).
CASES = {
    "ret-decl": (
        ["ast-grep run -l ts -p 'function $F($$$A): Relay { $$$B }' src",
         "ast-grep run -l TypeScript -p 'function $F($$$A): Relay { $$$B }' src",
         "sg -p 'function $NAME($$$ARGS): Relay {$$$BODY}' --lang typescript ./src/",
         "ast-grep --pattern 'function $_($$$): Relay { $$$ }' src",
         'ast-grep run -p "function \\$F(\\$\\$\\$A): Relay { \\$\\$\\$B }" src',
         RULE("  kind: function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    has:\n      kind: type_identifier\n      regex: ^Relay$"),
         # The live answers' form (rubric review, 2026-10-27): an anchored regex on the annotation, its colon included.
         RULE("  kind: function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: ^:\\s*Relay$")],
        ["ast-grep run -l ts -p 'function $F($$$A) { $$$B }' src",
         "ast-grep run -p 'export function $F($$$A): Relay { $$$B }' src",
         "ast-grep run -p 'function $F($$$A): Relay { $$$B }' -l TypeScript src/core",
         "ast-grep run -p 'function $F($$$A): Relay { $$$B }' src/core",
         "ast-grep run -p 'function $F($$$A): Relay { $$$B }' -l rust src",
         'ast-grep run -p "function $F($$$A): Relay { $$$B }" src',
         RULE("  kind: function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: Relay"),
         RULE("  kind: function_declaration\n  has:\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^Relay$\n      stopBy: end")]),
    "ret-arrow": (
        ["ast-grep run -p 'const $N = ($$$A): Socket => $B' src", "ast-grep -p 'const $V = ($$$): Socket => $BODY' -l ts src/",
         "ast-grep run -p 'const $NAME = ($$$): Socket => $$$' -l ts src",
         RULE("  kind: arrow_function\n  has:\n    kind: type_annotation\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^Socket$\n  inside:\n    kind: variable_declarator\n    inside:\n      kind: lexical_declaration\n      regex: ^const\\b"),
         RULE("  kind: arrow_function\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: ^:\\s*Socket$\n  inside:\n    kind: variable_declarator\n    inside:\n      kind: lexical_declaration\n      regex: ^const")],
        ["ast-grep run -p 'const $N = ($$$A) => $B' src", "ast-grep run -p 'let $N = ($$$A): Socket => $B' src",
         RULE("  kind: arrow_function\n  has:\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^Socket$")]),
    "zero-calls": (["ast-grep run -p 'probeMeadow($$$A)' src", "ast-grep --pattern 'probeMeadow($$$)' --lang ts .", RULE("  pattern: probeMeadow($$$)")],
                   ["ast-grep run -p 'probeMeadow' src", "ast-grep run -p 'probeMeadow($A)' src", "rg -n 'probeMeadow(' src"]),
    "emit-calls": (["ast-grep run -p '$emit($$$A)' src", "ast-grep run --pattern '$emit($$$)' -l ts src/"],
                   ['ast-grep run -p "$emit($$$A)" src', "ast-grep run -p 'emit($$$A)' src", "ast-grep run -p '$E($$$A)' src"]),
    "track-calls": (["ast-grep run -p '$track($$$ARGS)' -l typescript src"], ["ast-grep run -p 'track($$$A)' src"]),
    "maybe-defined": (["ast-grep outline src --match '^VertexSpec$'", "ast-grep outline --items structure --match '^VertexSpec$' src"],
                      ["ast-grep outline src --match VertexSpec", "ast-grep outline src --match 'Vertex'", "ast-grep outline src",
                       # A kind filter the ask doesn't name (rubric review, 2026-10-27): right only where the item is that kind.
                       "ast-grep outline --items structure --type struct --match '^VertexSpec$' -l ts src"]),
    "fn-outline": (["ast-grep outline src --match '^loadQuota$'", "ast-grep outline src --type function --match '^loadQuota$'",
                    "ast-grep outline src --items structure --match '^loadQuota$' --view signatures"],
                   ["ast-grep outline src --match loadQuota", "ast-grep outline src --match '^loadQuota'",
                    "ast-grep outline src --type interface --match '^loadQuota$'"]),
    "itf-outline": (["ast-grep outline src --match '^GarnetRow$'", "ast-grep outline src --type interface --match '^GarnetRow$'"],
                    # h6's live answer: unanchored; right by execution on its fixture, wrong here (no near-name there).
                    ["ast-grep outline src --type interface --match GarnetRow", "ast-grep outline src/index.ts --match '^GarnetRow$'"]),
    "public-surface": (["ast-grep outline src/index.ts", "ast-grep outline --items exports src/index.ts", "ast-grep outline ./src/index.ts --items structure",
                        "ast-grep run -l ts -k export_statement src/index.ts"],
                       ["ast-grep outline src --items exports", "ast-grep outline src", "ast-grep outline src/index.ts --match Relay"]),
    "public-types": (["ast-grep run -l typescript -p 'export type { $$$ }' src/index.ts",
                      "ast-grep run --pattern 'export type { $$$NAMES } from $MODULE' --lang typescript src/index.ts",
                      "ast-grep run -p 'export type { $$$S } from $$$P' src/index.ts"],
                     ["ast-grep run -p 'export type { $$$S }' src", "ast-grep outline src/index.ts"]),
    "await-any": ([RULE("  kind: function_declaration\n  has:\n    pattern: await $E\n    stopBy: end"),
                   "ast-grep scan --inline-rules '{id: f, language: typescript, rule: {kind: function_declaration, has: {field: body, kind: await_expression, stopBy: end}}}' src/"],
                  [RULE("  kind: function_declaration\n  has:\n    pattern: await $E"), "ast-grep run -p 'await $E' src"]),
    "log-in-try": ([RULE("  pattern: loadMeadow($$$A)\n  inside:\n    kind: try_statement\n    stopBy: end"),
                    RULE("  pattern: loadMeadow($$$)\n  inside:\n    kind: try_statement\n    field: body\n    stopBy: end"),
                    RULE("  all:\n    - pattern: loadMeadow($$$)\n    - inside:\n        kind: try_statement\n        stopBy: end"),
                    RULE("  all:\n    - inside:\n        kind: try_statement\n        stopBy: end\n    - pattern: loadMeadow($$$)")],
                   [RULE("  pattern: loadMeadow($$$A)\n  inside:\n    kind: try_statement"), "ast-grep run -p 'loadMeadow($$$A)' src",
                    "ast-grep run -k 'try_statement > statement_block call_expression' -l TypeScript src",
                    RULE("  all:\n    - pattern: loadMeadow($$$)\n    - inside:\n        kind: try_statement")]),
}


def selftest():
    bad = n_right = n_wrong = unknown = 0
    for fam, (rights, wrongs) in CASES.items():
        for c in rights:
            n_right += 1
            v, why = judge(c, fam, P)
            if v != "right":
                print(f"rubric: {fam}: not right ({v}: {why}): {c!r}"); bad += 1
        for c in wrongs:
            n_wrong += 1
            v, why = judge(c, fam, P)
            if v == "right":
                print(f"rubric: {fam}: graded right: {c!r}"); bad += 1
            elif v == "unrecognized":
                unknown += 1
    print(f"rubric: {n_right} right spellings and {n_wrong} not right ({n_wrong - unknown} wrong, {unknown} unrecognized) over "
          f"{len(CASES)} operations, {bad} misjudged")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest())
