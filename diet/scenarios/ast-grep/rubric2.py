"""Task set 2's static judge (#971, the maintainer 2026-10-29): `rubric.judge` hands it set 2's families.

Each family knows its right forms (each executes right on 20 seeds, `ref2_exec.py`) and names the wrongs the gap is
about; anything else is unrecognized. A reason states an ast-grep fact or the ask's terms, never the fix
(`rubric.py`'s LEAKS test reads these too).
"""
import rubric as R

INDEX = "src/index.ts"
FAMILIES = ("ret-method", "ret-async", "zero-new", "itf-maybe", "public-aliases", "public-values", "throw-in-for",
            "report-in-catch", "store-get", "log-calls", "cls-outline", "alias-outline", "kind-dir", "kind-file",
            "kind-hidden", "init-call", "cond-call", "prop-arrow", "fexpr-await", "gen-decl", "nested-decl",
            "spread-call", "rest-decl", "array-spread", "one-call", "two-params", "none-call")
DIRECT = "`has` looks only at a node's direct children by default"
PARENT = "`inside` looks only at a node's direct parent by default"
ANY_ARGS = "a multi-node metavariable matches any number of arguments"


def rule(f):
    r = R.rule_of(f) if f["sub"] == "scan" else None
    return R.merged_all(r) if r else r


def kind_sel(f):
    return "".join((R.one(f, "--kind") or "").split()) if f["sub"] == "run" and not R.one(f, "--pattern") else None


def outline_in(f, path, name):
    """An outline of PATH alone, anchored on NAME: the kind-file family's."""
    if f["sub"] != "outline" or not R.only(f, path):
        return "wrong", "an outline of that file alone is wanted"
    m = R.one(f, "--match")
    if m != f"^{name}$":
        return "wrong", ("--match is not anchored: it matches names, signatures and first lines" if m and name in m else "--match names another item")
    if R.one(f, "--type") is not None:
        return "wrong", "a --type filter on a kind the ask doesn't name: right only where the item happens to be that kind"
    return "right", "an anchored outline"


def judge(f, family, p):
    t = p["target"]
    if family in ("kind-file",):
        return outline_in(f, p["kind_file"], p["kind_file_item"])
    if family in ("public-aliases", "public-values"):
        if not R.only(f, INDEX):
            return "wrong", "not the entry point alone: a directory's exports are not the surface"
        if kind_sel(f) == "export_statement":
            return "wrong", "every export statement of the entry point, " + ("aliased or not" if family == "public-aliases" else "type-only ones included")
        r = rule(f)
        if not r or r.get("kind") != "export_statement":
            return "unrecognized", "not a rule over export statements the rubric knows"
        if family == "public-values":
            nots = r.get("not")
            return (("right", "export statements that aren't type-only") if isinstance(nots, dict) and set(nots) == {"regex"}
                    and str(nots["regex"]).startswith("^export type") else ("unrecognized", "not a form the rubric knows"))
        if isinstance(r.get("regex"), str) and r["regex"].strip() in (" as ", "as", r"\bas\b", r" as \w"):
            return "right", "export statements with an alias"
        has = r.get("has")
        if isinstance(has, dict) and has.get("kind") == "export_specifier" and isinstance(has.get("has"), dict) and has["has"].get("field") == "alias":
            return ("right", "an aliased export specifier, any depth") if R.deep(has) else ("wrong", DIRECT)
        return "unrecognized", "not a form the rubric knows"
    if not R.whole_project(f):
        if f["sub"] == "outline" and family in ("itf-maybe", "cls-outline", "alias-outline", "kind-dir", "kind-hidden"):
            return "wrong", "an outline of the project is wanted"
        return "wrong", "not the whole project"
    if family == "ret-method":
        r = rule(f)
        if r and r.get("kind") == "method_definition":
            if R.direct_type(r.get("has"), t):
                return "right", "a method return-type rule"
            if R.type_wrong(r.get("has"), t):
                return "wrong", R.type_wrong(r.get("has"), t)
        return "unrecognized", "not a form the rubric knows"
    if family == "ret-async":
        if R.search(f, f"async function $F($$$A): Promise<{t}> {{ $$$B }}"):
            return "right", "the async return-type pattern"
        if R.search(f, f"function $F($$$A): Promise<{t}> {{ $$$B }}"):
            return "wrong", "a function pattern without `async` matches async and plain declarations alike"
        return "unrecognized", "not a form the rubric knows"
    if family == "zero-new":
        c = p["new_cls"]
        if R.search(f, f"new {c}($$$A)"):
            return "right", "the new-expression pattern"
        if R.search(f, f"{c}($$$A)"):
            return "wrong", "a call pattern matches calls; `new C(…)` is a new expression, not a call"
        if R.search(f, "new $C($$$A)"):
            return "wrong", "`$C` matches every class constructed"
        return "unrecognized", "not a form the rubric knows"
    if family == "itf-maybe":
        return R.outline_named(f, p["maybe_itf"], "interface", False)
    if family == "cls-outline":
        return R.outline_named(f, p["line_cls"], "class", False)
    if family == "alias-outline":
        return R.outline_named(f, p["line_alias"], "struct", False)
    if family == "kind-dir":
        return R.outline_named(f, p["kind_dir"], None, False)
    if family == "kind-hidden":
        return R.outline_named(f, p["kind_hidden"], None, True)
    if family in ("throw-in-for", "report-in-catch"):
        r = rule(f)
        if kind_sel(f) == "throw_statement":
            return "wrong", "every throw statement, inside a loop or not"
        if not r:
            return "unrecognized", "not a rule the rubric can read"
        inside = r.get("inside")
        if family == "throw-in-for":
            if r.get("kind") != "throw_statement" or not isinstance(inside, dict):
                return "unrecognized", "not a throw statement inside a loop"
            if inside.get("kind") == "for_in_statement":
                return "wrong", "`for_in_statement` is `for…in` and `for…of`; `for (;;)` is `for_statement`"
            if inside.get("kind") != "for_statement":
                return "unrecognized", "not a loop kind the rubric knows"
            return ("right", "inside for, stopBy end") if R.deep(inside) else ("wrong", PARENT)
        if not R.pattern_is(r, f"{p['report_fn']}($$$A)") or not isinstance(inside, dict):
            return "unrecognized", "not the call inside a catch clause"
        if inside.get("kind") == "try_statement":
            return "wrong", "a try statement holds its try block too, not only its catch"
        if inside.get("kind") != "catch_clause":
            return "unrecognized", "not a kind the rubric knows"
        return ("right", "inside catch, stopBy end") if R.deep(inside) else ("wrong", PARENT)
    if family in ("store-get", "log-calls", "spread-call", "one-call", "none-call"):
        name = {"store-get": "$store.get", "log-calls": "$log", "spread-call": p["spread_fn"], "one-call": p["one_fn"], "none-call": p["none_fn"]}[family]
        right = {"store-get": [f"{name}($$$A)"], "log-calls": [f"{name}($$$A)"], "spread-call": [f"{name}(...$A)"],
                 "one-call": [f"{name}($A)"], "none-call": [f"{name}()"]}[family]
        if R.search(f, *right):
            return "right", "the call pattern"
        if family == "store-get" and R.search(f, "store.get($$$A)"):
            return "wrong", "`store` and `$store` are different names"
        if family == "log-calls" and R.search(f, "log($$$A)"):
            return "wrong", "`log` and `$log` are different names"
        if family in ("spread-call", "one-call", "none-call") and R.search(f, f"{name}($$$A)"):
            return "wrong", ANY_ARGS
        if family == "none-call" and R.search(f, f"{name}($A)"):
            return "wrong", "`$A` matches exactly one argument, and the call has none"
        return "unrecognized", "not a call pattern the rubric knows"
    if family in ("init-call", "cond-call", "prop-arrow"):
        r = R.rule_of(f) if f["sub"] == "scan" else None
        if r and isinstance(r.get("all"), list) and any(isinstance(x, dict) and "field" in x and not ({"has", "inside"} & set(x)) for x in r["all"]):
            return "wrong", "`field` belongs to a relation (`has`, `inside`): an `all:` item of its own matches nothing"
        r = rule(f)
        if family == "init-call" and R.search(f, f"const $V = {p['init_fn']}($$$A)"):
            return "wrong", "a `const` pattern leaves out `let` and `var`"
        want_kind = {"init-call": "variable_declarator", "cond-call": "if_statement", "prop-arrow": "pair"}[family]
        if not r or r.get("kind") != want_kind or not isinstance(r.get("has"), dict):
            return "unrecognized", "not a form the rubric knows"
        has = r["has"]
        if family == "init-call":
            good = has.get("field") == "value" and R.pattern_is(has, f"{p['init_fn']}($$$A)")
        elif family == "prop-arrow":
            good = has.get("field") == "value" and has.get("kind") == "arrow_function"
        else:
            inner = has.get("has")
            good = has.get("field") == "condition" and isinstance(inner, dict) and R.pattern_is(inner, f"{p['cond_fn']}($$$A)") and not R.deep(inner)
        if good and not R.deep(has):
            return "right", "the field's own node"
        if R.deep(has) or (has.get("field") is None and (R.pattern_is(has, f"{p.get('init_fn')}($$$A)", f"{p.get('cond_fn')}($$$A)") or has.get("kind") == "arrow_function")):
            return "wrong", "any depth also reaches what's nested inside the node, not only its own field"
        return "unrecognized", "not a form the rubric knows"
    if family == "fexpr-await":
        if kind_sel(f) == "function_expression:has(await_expression)":
            return "right", "a :has() selector, any depth"
        r = rule(f)
        if r and isinstance(r.get("any"), list) and any(isinstance(x, dict) and x.get("kind") == "arrow_function" for x in r["any"]):
            return "wrong", "`arrow_function` is an arrow, not a function expression"
        if r and r.get("kind") == "function_expression" and isinstance(r.get("has"), dict) and r["has"].get("kind") == "await_expression":
            return ("right", "has await, stopBy end") if R.deep(r["has"]) else ("wrong", DIRECT)
        return "unrecognized", "not a form the rubric knows"
    if family == "gen-decl":
        sel = kind_sel(f)
        if sel == "generator_function_declaration":
            return "right", "the generator declaration kind"
        if sel == "generator_function":
            return "wrong", "`generator_function` is a generator written as a value, not a declaration"
        r = rule(f)
        if r and isinstance(r.get("any"), list) and {x.get("kind") for x in r["any"] if isinstance(x, dict)} >= {"generator_function"}:
            return "wrong", "`generator_function` is a generator written as a value, not a declaration"
        return "unrecognized", "not a form the rubric knows"
    if family == "nested-decl":
        if kind_sel(f) == "function_declaration":
            return "wrong", "every function declaration, top-level ones included"
        r = rule(f)
        inside = r.get("inside") if r else None
        if not r or r.get("kind") != "function_declaration" or not isinstance(inside, dict):
            return "unrecognized", "not a form the rubric knows"
        if inside.get("kind") == "function_declaration":
            return ("right", "inside a declaration, stopBy end") if R.deep(inside) else ("wrong", PARENT)
        if isinstance(inside.get("any"), list):
            return "wrong", "an arrow or another function kind is not a function declaration"
        return "unrecognized", "not a form the rubric knows"
    if family == "rest-decl":
        if R.search(f, "function $F($$$A, ...$R): $T { $$$B }"):
            return "right", "the rest-parameter pattern"
        if R.search(f, "function $F(...$R): $T { $$$B }"):
            return "wrong", "`(...$R)` matches only a rest parameter that stands alone"
        if kind_sel(f) == "rest_pattern":
            return "wrong", "every rest pattern: destructurings' and arrows' too"
        return "unrecognized", "not a form the rubric knows"
    if family == "array-spread":
        if kind_sel(f) == "spread_element":
            return "wrong", "every spread: calls' and objects' too"
        r = rule(f)
        if r and r.get("kind") == "array" and isinstance(r.get("has"), dict) and r["has"].get("kind") == "spread_element" and not R.deep(r["has"]):
            return "right", "an array with a spread element"
        return "unrecognized", "not a form the rubric knows"
    if family == "two-params":
        untyped, typed = "function $F($A, $B) { $$$C }", "function $F($A, $B): $R { $$$C }"
        r = R.rule_of(f) if f["sub"] == "scan" else None
        alts = r.get("any") if r else None
        if isinstance(alts, list) and len(alts) == 2 and all(isinstance(x, dict) and set(x) == {"pattern"} for x in alts):
            got = {R.canon(x["pattern"]) for x in alts}
            if got == {R.canon(untyped), R.canon(typed)}:
                return "right", "both declaration shapes"
        if R.search(f, typed):
            return "wrong", "a function pattern with a return type matches only functions declared with one"
        if R.search(f, untyped):
            return "wrong", "a function pattern without a return type matches only functions declared without one"
        if R.search(f, "function $F($$$A): $R { $$$C }"):
            return "wrong", ANY_ARGS.replace("arguments", "parameters")
        return "unrecognized", "not a form the rubric knows"
    raise KeyError(family)
