"""Task set 2's reference, naive and over-broad commands (#971, the maintainer 2026-10-29), per family: `ref.py`
runs them on 20 seeds. Every pattern here was probed on ast-grep 0.44.0 before it was written."""

RULE = lambda body, path="src": "ast-grep scan --inline-rules 'id: q\nlanguage: ts\nrule:\n" + body + "' " + path
INDEX = "src/index.ts"


def reference(family, p):
    t = p["target"]
    return {
        "ret-method": RULE(f"  kind: method_definition\n  has:\n    field: return_type\n    regex: ^:\\s*{t}$"),
        "ret-async": f"ast-grep run -l ts -p 'async function $F($$$A): Promise<{t}> {{ $$$B }}' src",
        "zero-new": f"ast-grep run -l ts -p 'new {p['new_cls']}($$$A)' src",
        "itf-maybe": f"ast-grep outline src --match '^{p['maybe_itf']}$'",
        "public-aliases": RULE("  kind: export_statement\n  has:\n    kind: export_specifier\n    has:\n      field: alias\n"
                               "      kind: identifier\n    stopBy: end", INDEX),
        "public-values": RULE("  kind: export_statement\n  not:\n    regex: ^export type", INDEX),
        "throw-in-for": RULE("  kind: throw_statement\n  inside:\n    kind: for_statement\n    stopBy: end"),
        "report-in-catch": RULE(f"  pattern: {p['report_fn']}($$$A)\n  inside:\n    kind: catch_clause\n    stopBy: end"),
        "store-get": "ast-grep run -l ts -p '$store.get($$$A)' src",
        "log-calls": "ast-grep run -l ts -p '$log($$$A)' src",
        "cls-outline": f"ast-grep outline src --match '^{p['line_cls']}$'",
        "alias-outline": f"ast-grep outline src --match '^{p['line_alias']}$'",
        "kind-dir": f"ast-grep outline src --match '^{p['kind_dir']}$'",
        "kind-file": f"ast-grep outline {p['kind_file']} --match '^{p['kind_file_item']}$'",
        "kind-hidden": f"ast-grep outline src --items structure --match '^{p['kind_hidden']}$'",
        "init-call": RULE(f"  kind: variable_declarator\n  has:\n    field: value\n    pattern: {p['init_fn']}($$$A)"),
        "cond-call": RULE(f"  kind: if_statement\n  has:\n    field: condition\n    has:\n      pattern: {p['cond_fn']}($$$A)"),
        "prop-arrow": RULE("  kind: pair\n  has:\n    field: value\n    kind: arrow_function"),
        "fexpr-await": "ast-grep run -l ts -k 'function_expression:has(await_expression)' src",
        "gen-decl": "ast-grep run -l ts -k generator_function_declaration src",
        "nested-decl": RULE("  kind: function_declaration\n  inside:\n    kind: function_declaration\n    stopBy: end"),
        "spread-call": f"ast-grep run -l ts -p '{p['spread_fn']}(...$A)' src",
        "rest-decl": "ast-grep run -l ts -p 'function $F($$$A, ...$R): $T { $$$B }' src",
        "array-spread": RULE("  kind: array\n  has:\n    kind: spread_element"),
        "one-call": f"ast-grep run -l ts -p '{p['one_fn']}($A)' src",
        # Quoted: `: $R` unquoted is a YAML mapping value.
        "two-params": RULE("  any:\n    - pattern: \"function $F($A, $B) { $$$C }\"\n    - pattern: \"function $F($A, $B): $R { $$$C }\""),
        "none-call": f"ast-grep run -l ts -p '{p['none_fn']}()' src",
    }.get(family)


def naive(family, p):
    """The gap's own mistake: wrong statically, and wrong by execution on at least one seed."""
    t = p["target"]
    return {
        "ret-method": RULE(f"  kind: method_definition\n  has:\n    kind: type_identifier\n    regex: ^{t}$\n    stopBy: end"),
        "ret-async": f"ast-grep run -l ts -p 'function $F($$$A): Promise<{t}> {{ $$$B }}' src",
        "zero-new": f"ast-grep run -l ts -p '{p['new_cls']}($$$A)' src",
        "itf-maybe": f"ast-grep outline src --match '{p['maybe_itf']}'",
        "public-aliases": f"ast-grep run -l ts -k export_statement {INDEX}",
        "public-values": f"ast-grep run -l ts -k export_statement {INDEX}",
        "throw-in-for": RULE("  kind: throw_statement\n  inside:\n    kind: for_statement"),
        "report-in-catch": RULE(f"  pattern: {p['report_fn']}($$$A)\n  inside:\n    kind: catch_clause"),
        "store-get": "ast-grep run -l ts -p 'store.get($$$A)' src",
        "log-calls": "ast-grep run -l ts -p 'log($$$A)' src",
        "cls-outline": f"ast-grep outline src --match '{p['line_cls']}'",
        "alias-outline": f"ast-grep outline src --match '{p['line_alias']}'",
        "kind-dir": f"ast-grep outline src --match '{p['kind_dir']}'",
        "kind-file": f"ast-grep outline src --match '^{p['kind_file_item']}View$'",
        "kind-hidden": f"ast-grep outline src --match '^{p['kind_hidden']}$'",
        "init-call": RULE(f"  all:\n    - kind: variable_declarator\n    - field: value\n      pattern: {p['init_fn']}($$$A)"),
        "cond-call": RULE(f"  kind: if_statement\n  has:\n    pattern: {p['cond_fn']}($$$A)\n    stopBy: end"),
        "prop-arrow": RULE("  kind: pair\n  has:\n    kind: arrow_function\n    stopBy: end"),
        "fexpr-await": RULE("  any:\n    - kind: function_expression\n    - kind: arrow_function\n  has:\n    kind: await_expression\n    stopBy: end"),
        "gen-decl": "ast-grep run -l ts -k generator_function src",
        "nested-decl": "ast-grep run -l ts -k function_declaration src",
        "spread-call": f"ast-grep run -l ts -p '{p['spread_fn']}($$$A)' src",
        "rest-decl": "ast-grep run -l ts -p 'function $F(...$R): $T { $$$B }' src",
        "array-spread": "ast-grep run -l ts -k spread_element src",
        "one-call": f"ast-grep run -l ts -p '{p['one_fn']}($$$A)' src",
        "two-params": "ast-grep run -l ts -p 'function $F($A, $B): $R { $$$C }' src",
        "none-call": f"ast-grep run -l ts -p '{p['none_fn']}($$$A)' src",
    }.get(family)


def overbroad(family, p):
    """Forms that also match what the task excludes: each must execute wrong on EVERY seed (the distractors)."""
    return {
        "zero-new": "ast-grep run -l ts -p 'new $C($$$A)' src",
        "itf-maybe": f"ast-grep outline src --match '{p['maybe_itf']}'",
        "public-aliases": RULE("  kind: export_statement\n  has:\n    kind: export_specifier\n    has:\n      field: alias\n"
                               "      kind: identifier\n    stopBy: end"),
        "public-values": f"ast-grep run -l ts -k export_statement {INDEX}",
        "throw-in-for": "ast-grep run -l ts -k throw_statement src",
        "report-in-catch": RULE(f"  pattern: {p['report_fn']}($$$A)\n  inside:\n    kind: try_statement\n    stopBy: end"),
        "store-get": "ast-grep run -l ts -p '$store.$M($$$A)' src",
        "log-calls": "ast-grep run -l ts -p '$F($$$A)' src",
        "cls-outline": f"ast-grep outline src --match '{p['line_cls']}$'",
        "alias-outline": f"ast-grep outline src --match '{p['line_alias']}$'",
        "kind-dir": f"ast-grep outline src --match '{p['kind_dir']}'",
        "kind-file": f"ast-grep outline {p['kind_file']}",
        "init-call": RULE(f"  kind: variable_declarator\n  has:\n    pattern: {p['init_fn']}($$$A)\n    stopBy: end"),
        "cond-call": RULE(f"  kind: if_statement\n  has:\n    pattern: {p['cond_fn']}($$$A)\n    stopBy: end"),
        "prop-arrow": RULE("  kind: pair\n  has:\n    kind: arrow_function\n    stopBy: end"),
        "fexpr-await": RULE("  any:\n    - kind: function_expression\n    - kind: arrow_function\n  has:\n    kind: await_expression\n    stopBy: end"),
        "gen-decl": RULE("  any:\n    - kind: generator_function_declaration\n    - kind: generator_function"),
        "nested-decl": RULE("  kind: function_declaration\n  inside:\n    any:\n      - kind: function_declaration\n"
                            "      - kind: arrow_function\n    stopBy: end"),
        "spread-call": f"ast-grep run -l ts -p '{p['spread_fn']}($$$A)' src",
        "rest-decl": "ast-grep run -l ts -k rest_pattern src",
        "array-spread": "ast-grep run -l ts -k spread_element src",
        "one-call": f"ast-grep run -l ts -p '{p['one_fn']}($$$A)' src",
        "two-params": "ast-grep run -l ts -p 'function $F($$$A): $R { $$$C }' src",
        "none-call": f"ast-grep run -l ts -p '{p['none_fn']}($$$A)' src",
        "ret-method": RULE(f"  kind: method_definition\n  has:\n    kind: type_identifier\n    regex: ^{p['target']}$\n    stopBy: end"),
        "ret-async": f"ast-grep run -l ts -p 'function $F($$$A): Promise<{p['target']}> {{ $$$B }}' src",
    }.get(family)


def kind_filtered(family, p):
    """A `--type` filter the ask doesn't name, on the kind-varying families: right only where the item happens to be a
    type alias, so wrong on some seeds, never on all (the kind is drawn per seed)."""
    return {"kind-dir": f"ast-grep outline src --type struct --match '^{p['kind_dir']}$'",
            "kind-file": f"ast-grep outline {p['kind_file']} --type struct --match '^{p['kind_file_item']}$'",
            "kind-hidden": f"ast-grep outline src --items structure --type struct --match '^{p['kind_hidden']}$'"}.get(family)


FAMILIES = ("ret-method", "ret-async", "zero-new", "itf-maybe", "public-aliases", "public-values", "throw-in-for",
            "report-in-catch", "store-get", "log-calls", "cls-outline", "alias-outline", "kind-dir", "kind-file",
            "kind-hidden", "init-call", "cond-call", "prop-arrow", "fexpr-await", "gen-decl", "nested-decl",
            "spread-call", "rest-decl", "array-spread", "one-call", "two-params", "none-call")
