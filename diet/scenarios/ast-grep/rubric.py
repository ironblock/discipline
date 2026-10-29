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
import rubric2  # noqa: E402  (it imports this module back, by name)

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
        # A `--selector` beside the pattern changes what matches (it picks the pattern's sub-node): never the plain
        # pattern's search (measured, 0.44.0; the crossover's false right, 2026-10-29).
        if form["opts"].get("--selector"):
            return False
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
    # Measured on 0.44.0 (the sweep, 2026-10-29): the annotation's type identifier has no field name.
    if isinstance(inner, dict) and inner.get("kind") == "type_identifier" and "field" in inner:
        return "a type annotation's type has no field name: a `field` on it matches nothing there"
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
            and inner.get("regex") == f"^{name}$" and "field" not in inner)


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
            return "wrong", "--match is anchored at one end only: it also matches every name that begins or ends with it"
        return ("wrong", "--match is not anchored: it matches names, signatures and first lines") if name in m else ("wrong", "--match names another item")
    types = one(form, "--type")
    if types is not None:
        if kind is None:
            return "wrong", "a --type filter on a kind the ask doesn't name: right only where the item happens to be that kind"
        if kind not in types.split(","):
            return "wrong", f"--type leaves out {kind}"
    items = one(form, "--items")
    if need_structure and items != "structure":
        return "wrong", "on a directory, outline lists only exported items, and this item isn't exported"
    return "right", "an anchored outline"


def patterns_of(form):
    """Every pattern the command gives: `--pattern`, and each `pattern` key in its rule at any depth."""
    found = list(form["opts"].get("--pattern", []))
    stack = [rule_of(form)] if form["sub"] == "scan" else []
    while stack:
        node = stack.pop()
        if isinstance(node, dict):
            found += [v for k, v in node.items() if k == "pattern" and isinstance(v, str)]
            stack += list(node.values())
        elif isinstance(node, list):
            stack += node
    return found


def one_meta(form, rights, inside):
    """A run pattern that would be one of RIGHTS but for a single metavariable inside INSIDE (`(...)`, `{...}`) where
    the right form has `$$$`."""
    pat = one(form, "--pattern")
    if form["sub"] != "run" or pat is None:
        return False
    one_ = inside[0] + "$M" + inside[1]
    return one_ in canon(pat) and canon(pat).replace(one_, inside[0] + "$$$" + inside[1], 1) in {canon(r) for r in rights}


def merged_all(r):
    """A rule whose `all:` items are merged into it, when no key is given twice; else the rule as it is."""
    if not isinstance(r.get("all"), list) or not all(isinstance(x, dict) for x in r["all"]):
        return r
    merged = {k: v for k, v in r.items() if k != "all"}
    for x in r["all"]:
        if set(x) & set(merged):
            return r
        merged.update(x)
    return merged


ONE_PARAM = "`$A` in a parameter list matches exactly one parameter"


def declares_functions(r):
    """A rule's own kind is function_declaration, or an `any:` of bare kinds that includes it and adds at most
    generator_function_declaration (Dispatch, 2026-10-28: "declared with the `function` keyword" arguably covers
    `function*`)."""
    if "kind" in r:
        return "any" not in r and r["kind"] == "function_declaration"
    alts = r.get("any")
    if not isinstance(alts, list) or not all(isinstance(x, dict) and set(x) == {"kind"} for x in alts):
        return False
    kinds = {x["kind"] for x in alts}
    return "function_declaration" in kinds and kinds <= {"function_declaration", "generator_function_declaration"}


CONST = ("^const", "^const\\b", "^const\\s", "^const ")


def const_inside(inside):
    """Whether an arrow rule's `inside` reaches a `const` declaration: a variable_declarator inside a
    lexical_declaration whose regex anchors `const`, or that lexical_declaration itself with `stopBy: end`. (ok, why)
    -- never "const" found anywhere in the text (the crossover's false right, 2026-10-29)."""
    def const_decl(node):
        return isinstance(node, dict) and node.get("kind") == "lexical_declaration" and node.get("regex") in CONST
    if not isinstance(inside, dict):
        return False, "not only const arrows"
    if inside.get("kind") == "variable_declarator" and const_decl(inside.get("inside")):
        return True, ""
    if const_decl(inside):
        return (True, "") if deep(inside) else (False, "`inside` looks only at a node's direct parent by default")
    if isinstance(inside.get("pattern"), str) and inside["pattern"].lstrip().startswith("const") and not deep(inside):
        return False, "`inside` looks only at a node's direct parent by default"
    return False, "not only const arrows"


def untyped_function(node):
    """A rule node whose pattern is a function declaration without its return type."""
    return isinstance(node, dict) and pattern_is(node, "function $F($$$A) { $$$B }")


def judge(cmd, family, p):
    try:
        f = command.parse(cmd)
    except command.Refused as why:
        return "wrong", f"refused: {why}"
    if not langs_ok(f):
        return "wrong", "--lang is not TypeScript"
    # Measured on 0.44.0 (2026-10-29): `run` with both exits 2, "the argument '--pattern <PATTERN>' cannot be used with
    # '--kind <KIND>'".
    if f["sub"] == "run" and f["opts"].get("--pattern") and f["opts"].get("--kind"):
        return "wrong", "ast-grep refuses the command: the argument '--pattern' cannot be used with '--kind'"
    # Round 5 (Dispatch, 2026-10-28): the floor's `(...$ARGS)` and `name(...)`. Not for set 2's spread and rest
    # families, whose `...` is the spread they ask for.
    if family not in ("spread-call", "rest-decl", "array-spread") and any("..." in pat for pat in patterns_of(f)):
        return "wrong", "`...` in a pattern is literal text: it matches only code that has `...` there"
    if family in rubric2.FAMILIES:  # task set 2 (#971, 2026-10-29)
        return rubric2.judge(f, family, p)
    if family == "ret-decl":
        if not whole_project(f):
            return "wrong", "not the whole project"
        if search(f, f"function $F($$$A): {p['target']} {{ $$$B }}"):
            return "right", "the return-type pattern"
        r = rule_of(f) if f["sub"] == "scan" else None
        r = merged_all(r) if r else r  # `all:` of the kind and the `has` is the one rule (the floor's gap-fill t1)
        # Its kind, or an `any:` of function kinds (Dispatch, 2026-10-29, as t4's).
        if r and declares_functions(r) and direct_type(r.get("has"), p["target"]):
            return "right", "a return-type rule"
        if r and declares_functions(r) and type_wrong(r.get("has"), p["target"]):
            return "wrong", type_wrong(r.get("has"), p["target"])
        if one_meta(f, [f"function $F($$$A): {p['target']} {{ $$$B }}"], "()"):
            return "wrong", ONE_PARAM
        if f["sub"] == "run" and canon(one(f, "--pattern") or "") == canon("function $F($$$A) { $$$B }"):
            return "wrong", "a function pattern without a return type matches only functions declared without one"
        return "unrecognized", "not a form the rubric knows"
    if family == "ret-arrow":
        if not whole_project(f):
            return "wrong", "not the whole project"
        if search(f, f"const $N = ($$$A): {p['arrow_target']} => $B", f"const $N = ($$$A): {p['arrow_target']} => $$$B"):
            return "right", "the const arrow pattern"
        if one_meta(f, [f"const $N = ($$$A): {p['arrow_target']} => $B"], "()"):
            return "wrong", ONE_PARAM
        r = rule_of(f) if f["sub"] == "scan" else None
        r = merged_all(r) if r else r  # `all:` of the return type and the const is the one rule (round 5)
        if r and r.get("kind") == "arrow_function" and type_wrong(r.get("has"), p["arrow_target"]):
            return "wrong", type_wrong(r.get("has"), p["arrow_target"])
        if r and r.get("kind") == "arrow_function" and direct_type(r.get("has"), p["arrow_target"]):
            ok, why = const_inside(r.get("inside"))
            return ("right", "an arrow return-type rule within a const") if ok else ("wrong", why)
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
            return "wrong", "`$A` matches exactly one argument, and a call may take more or none"
        return "unrecognized", "not a call pattern the rubric knows"
    if family == "maybe-defined":
        return outline_named(f, p["maybe"], None, False)
    if family == "fn-outline":
        return outline_named(f, p["line_fn"], "function", not p.get("line_fn_exported", True))
    if family == "itf-outline":
        return outline_named(f, p["line_itf"], "interface", False)
    if family in ("public-surface", "public-types") and canon(one(f, "--pattern") or "") == canon("export $$$"):
        return "wrong", "`export $$$` doesn't parse as a pattern"
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
        rights = ("export type { $$$S }", "export type { $$$S } from $$$P", "export type { $$$S } from $P", "export type { $$$S } from '$P'",
                  "export type { $$$S } from '$P';")
        if search(f, *rights):
            return "right", "the type-export pattern"
        if one_meta(f, rights, "{}"):
            return "wrong", "`$X` matches exactly one export specifier"
        return "unrecognized", "not a form the rubric knows"
    if family == "await-any":
        # A `:has()` selector reaches any depth, as `has` with stopBy: end does (Dispatch, 2026-10-28; the floor's
        # gap-fill iter-2 t4). `:has(await $X)` matches nothing in 0.44.0, and another kind inside stays wrong.
        sel = re.sub(r"\s+", "", one(f, "--kind") or "")
        if f["sub"] == "run" and not one(f, "--pattern") and sel.startswith("function_declaration:has("):
            if not whole_project(f):
                return "wrong", "not the whole project"
            if sel == "function_declaration:has(await_expression)":
                return "right", "a :has() selector, any depth"
        r = rule_of(f) if f["sub"] == "scan" else None
        if not r or not whole_project(f):
            return ("wrong", "a relation needs a rule") if f["sub"] != "scan" else ("unrecognized", "a rule the rubric can't read")
        items = r.get("all") if isinstance(r.get("all"), list) else [r]
        if any(untyped_function(x) for x in items):
            return "wrong", "a function pattern without its return type matches no typed function"
        r = merged_all(r)  # `all:` of the kind and the `has` is the one rule
        alts = r.get("any") if isinstance(r.get("any"), list) else []
        if r.get("kind") == "function_expression" or any(isinstance(x, dict) and x.get("kind") == "function_expression" for x in alts):
            return "wrong", "`function_expression` is a function written as a value, not a function declaration"
        has = r.get("has")
        if not declares_functions(r) or not isinstance(has, dict):
            return "unrecognized", "not a function_declaration with `has`"
        inner = has.get("has")
        if has.get("field") == "body" and has.get("kind") == "statement_block" and isinstance(inner, dict) and (
                pattern_is(inner, "await $E") or inner.get("kind") == "await_expression"):
            # Round 5, the Flash-Next instance's t4: the body, then the await within it.
            return ("right", "the body has await, stopBy end") if deep(inner) else ("wrong", "`has` looks only at a node's direct children by default")
        if not (pattern_is(has, "await $E") or has.get("kind") == "await_expression"):
            return "unrecognized", "`has` is not an await"
        return ("right", "has await, stopBy end") if deep(has) else ("wrong", "`has` looks only at a node's direct children by default")
    if family == "log-in-try":
        r = rule_of(f) if f["sub"] == "scan" else None
        if not r or not whole_project(f):
            return ("wrong", "a relation needs a rule") if f["sub"] != "scan" else ("unrecognized", "a rule the rubric can't read")
        r = merged_all(r)  # `all:` of a pattern and an inside, in either order, is the one rule
        inside = r.get("inside")
        # `inside: {pattern: "try { $$$ }"}` is the try statement too (round 4).
        is_try = isinstance(inside, dict) and (inside.get("kind") == "try_statement" or pattern_is(inside, "try { $$$ }"))
        if is_try and pattern_is(r, f"{p['log_fn']}($A)"):
            return "wrong", "`$A` matches exactly one argument, and a call may take more or none"
        if not pattern_is(r, f"{p['log_fn']}($$$A)") or not is_try:
            return "unrecognized", "not the call inside a try_statement"
        return ("right", "inside try, stopBy end") if deep(inside) else ("wrong", "`inside` looks only at a node's direct parent by default")
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
         RULE("  kind: function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: ^:\\s*Relay$"),
         # An `any:` of function kinds (Dispatch, 2026-10-29, as t4's): the Flash-Next instance's crossover t1.
         RULE("  any:\n    - kind: function_declaration\n    - kind: generator_function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    has:\n      kind: type_identifier\n      regex: ^Relay$"),
         # `all:` of the kind and the `has` is the one rule (the floor's gap-fill iter-3 t1).
         RULE("  all:\n    - kind: function_declaration\n    - has:\n        field: return_type\n        regex: \"^: Relay$\"")],
        ["ast-grep run -l ts -p 'function $F($$$A) { $$$B }' src",
         "ast-grep run -p 'export function $F($$$A): Relay { $$$B }' src",
         "ast-grep run -p 'function $F($$$A): Relay { $$$B }' -l TypeScript src/core",
         "ast-grep run -p 'function $F($$$A): Relay { $$$B }' src/core",
         "ast-grep run -p 'function $F($$$A): Relay { $$$B }' -l rust src",
         'ast-grep run -p "function $F($$$A): Relay { $$$B }" src',
         RULE("  kind: function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: Relay"),
         RULE("  all:\n    - kind: function_declaration\n    - has:\n        field: return_type\n        regex: Relay"),
         RULE("  kind: function_declaration\n  has:\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^Relay$\n      stopBy: end")]),
    "ret-arrow": (
        ["ast-grep run -p 'const $N = ($$$A): Socket => $B' src", "ast-grep -p 'const $V = ($$$): Socket => $BODY' -l ts src/",
         "ast-grep run -p 'const $NAME = ($$$): Socket => $$$' -l ts src",
         RULE("  kind: arrow_function\n  has:\n    kind: type_annotation\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^Socket$\n  inside:\n    kind: variable_declarator\n    inside:\n      kind: lexical_declaration\n      regex: ^const\\b"),
         RULE("  kind: arrow_function\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: ^:\\s*Socket$\n  inside:\n    kind: variable_declarator\n    inside:\n      kind: lexical_declaration\n      regex: ^const"),
         # Round 5, the Flash-Next instance's h1: flow style, `all:` of the return type and the const.
         "ast-grep scan --inline-rules '{language: typescript, id: q, rule: {kind: arrow_function, all: [{has: {field: return_type, kind: type_annotation, "
         "has: {kind: type_identifier, regex: ^Socket$}}}, {inside: {kind: variable_declarator, field: value, inside: {kind: lexical_declaration, regex: ^const}}}]}}' src"],
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
                      "ast-grep run -p \"export type { \\$\\$\\$ } from '\\$PATH';\" -l ts src/index.ts",
                      "ast-grep run -p 'export type { $$$S } from $$$P' src/index.ts"],
                     ["ast-grep run -p 'export type { $$$S }' src", "ast-grep outline src/index.ts"]),
    "await-any": ([RULE("  kind: function_declaration\n  has:\n    pattern: await $E\n    stopBy: end"),
                   # Round 5, the Flash-Next instance's t4: the body, then an await at any depth within it.
                   RULE("  kind: function_declaration\n  has:\n    field: body\n    kind: statement_block\n    has:\n      kind: await_expression\n      stopBy: end"),
                   "ast-grep scan --inline-rules '{id: f, language: typescript, rule: {kind: function_declaration, has: {field: body, kind: await_expression, stopBy: end}}}' src/",
                   # `any:` of kinds with function_declaration among them (Dispatch, 2026-10-28): the ask's "declared with the
                   # `function` keyword" arguably covers `function*`, so either set is right.
                   RULE("  any:\n    - kind: function_declaration\n    - kind: generator_function_declaration\n  has:\n    field: body\n    kind: statement_block\n    has:\n      kind: await_expression\n      stopBy: end"),
                   RULE("  any:\n    - kind: function_declaration\n  has:\n    pattern: await $E\n    stopBy: end"),
                   # A `:has()` selector reaches any depth, as `has` with stopBy: end does (the floor's gap-fill iter-2 t4).
                   "ast-grep run -k 'function_declaration:has(await_expression)' src/",
                   # `all:` of the kind and the `has` is the one rule (the floor's gap-fill t4).
                   RULE("  all:\n    - kind: function_declaration\n    - has:\n        pattern: await $A\n        stopBy: end")],
                  [RULE("  kind: function_declaration\n  has:\n    pattern: await $E"), "ast-grep run -p 'await $E' src",
                   RULE("  kind: function_declaration\n  has:\n    field: body\n    kind: statement_block\n    has:\n      kind: await_expression"),
                   RULE("  any:\n    - kind: function_declaration\n    - kind: generator_function_declaration\n  has:\n    pattern: await $E"),
                   RULE("  any:\n    - kind: generator_function_declaration\n  has:\n    pattern: await $E\n    stopBy: end"),
                   "ast-grep run -k 'function_declaration:has(await $X)' src", "ast-grep run -k 'function_declaration:has(call_expression)' src",
                   "ast-grep run -k 'function_expression:has(await_expression)' src",
                   RULE("  any:\n    - kind: function_declaration\n    - kind: arrow_function\n  has:\n    pattern: await $E\n    stopBy: end")]),
    "log-in-try": ([RULE("  pattern: loadMeadow($$$A)\n  inside:\n    kind: try_statement\n    stopBy: end"),
                    RULE("  pattern: loadMeadow($$$)\n  inside:\n    kind: try_statement\n    field: body\n    stopBy: end"),
                    RULE("  all:\n    - pattern: loadMeadow($$$)\n    - inside:\n        kind: try_statement\n        stopBy: end"),
                    RULE("  all:\n    - inside:\n        kind: try_statement\n        stopBy: end\n    - pattern: loadMeadow($$$)"),
                    RULE("  all:\n    - pattern: loadMeadow($$$)\n    - inside:\n        pattern: \"try { $$$ }\"\n        stopBy: end")],
                   [RULE("  pattern: loadMeadow($$$A)\n  inside:\n    kind: try_statement"), "ast-grep run -p 'loadMeadow($$$A)' src",
                    "ast-grep run -k 'try_statement > statement_block call_expression' -l TypeScript src",
                    RULE("  all:\n    - pattern: loadMeadow($$$)\n    - inside:\n        kind: try_statement")]),
}


# Named wrongs (round 4): the verdict must be `wrong`, and its reason must say so.
NAMED = [
    ("public-surface", "ast-grep run -p 'export $$$' -l ts src/index.ts", "doesn't parse"),
    ("await-any", RULE("  all:\n    - pattern: function $N($$$) { $$$ }\n    - has:\n        pattern: await $X\n        stopBy: end"), "without its return type"),
    ("fn-outline", "ast-grep outline src --match 'loadQuota$'", "one end"),
    ("itf-outline", "ast-grep outline src --type interface --match 'GarnetRow$'", "one end"),
    ("t6-run", "ast-grep run -p 'function loadQuota($$$): $$$ { $$$ }' -l ts src", "outline"),
    # Round 5 (Dispatch, 2026-10-28), the floor's answers.
    ("ret-decl", "ast-grep run -p 'function $NAME(...$ARGS): Relay {}' src", "literal text"),
    ("zero-calls", "ast-grep run -p 'probeMeadow(...)' src", "literal text"),
    ("ret-arrow", "ast-grep run -p 'const $NAME = ($ARG): Socket => $BODY' --selector arrow_function -l TypeScript src", "exactly one parameter"),
    ("ret-decl", "ast-grep run -p 'function $F($A): Relay { $$$B }' -l ts src", "exactly one parameter"),
    ("public-types", "ast-grep run -p 'export type { $X }' -l typescript src/index.ts", "exactly one export specifier"),
    ("await-any", RULE("  any:\n    - kind: function_declaration\n    - kind: function_expression\n    - kind: generator_function_declaration\n  has:\n    pattern: await $X\n    stopBy: end"), "written as a value"),
    # False rights the crossover's audit found (2026-10-29): graded right, executed wrong.
    ("track-calls", "ast-grep run -p '$track($$$)' -k call_expression --lang typescript src/", "cannot be used with"),
    ("ret-arrow", RULE("  kind: arrow_function\n  all:\n    - has:\n        field: return_type\n        regex: \"^: Socket$\"\n"
                       "    - inside:\n        pattern: \"const $NAME = $ARROW\"") , "direct parent"),
    # The sweep's (2026-10-29): a gap-fill t1 of the Flash-Next instance's, graded right, executed wrong.
    ("ret-decl", RULE("  kind: function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    has:\n"
                      "      field: type\n      kind: type_identifier\n      regex: ^Relay$"), "no field name"),
    ("log-in-try", RULE("  all:\n    - pattern: loadMeadow($A)\n    - inside:\n        kind: try_statement\n        stopBy: end"), "exactly one argument"),
]
# W1r carries a wrong's reason to the model (round 4): no reason may hand it the fix itself.
LEAKS = ["--items structure", "`$$$`", "stopBy: end looks", "without stopBy"]


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
    reasons = []
    for fam, c, part in NAMED:
        v, why = judge(c, "fn-outline" if fam == "t6-run" else fam, P)
        reasons.append(why)
        if v != "wrong" or part not in why:
            print(f"rubric: named wrong {fam}: {v}: {why!r}"); bad += 1
    for fam, (rights, wrongs) in CASES.items():
        reasons += [judge(c, fam, P)[1] for c in wrongs]
    # Task set 2 (#971, 2026-10-29): on a sample instance, each family's reference is right, and its naive, over-broad
    # and kind-filtered forms are not (ref2_exec.py executes them on 20 seeds); their reasons join the LEAKS test.
    import gen, ref2
    p2 = gen.build(7)[1]["params"]
    n2 = 0
    for fam in rubric2.FAMILIES:
        v, why = judge(ref2.reference(fam, p2), fam, p2)
        if v != "right":
            print(f"rubric: set 2 {fam}: the reference is {v}: {why}"); bad += 1
        for wrong in (ref2.naive(fam, p2), ref2.overbroad(fam, p2), ref2.kind_filtered(fam, p2)):
            if wrong:
                n2 += 1
                v, why = judge(wrong, fam, p2)
                reasons.append(why)
                if v == "right":
                    print(f"rubric: set 2 {fam}: graded right: {wrong!r}"); bad += 1
    leaked = [r for r in reasons if any(l in r for l in LEAKS)]
    if leaked:
        print(f"rubric: reasons that name the fix: {leaked}"); bad += 1
    print(f"rubric: {len(NAMED)} named wrongs, {len(reasons)} reasons checked for the fix;", end=" ")
    print(f"rubric: {n_right} right spellings and {n_wrong} not right ({n_wrong - unknown} wrong, {unknown} unrecognized) over "
          f"{len(CASES)} operations, and set 2's {len(rubric2.FAMILIES)} references and {n2} not right, {bad} misjudged")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest())
