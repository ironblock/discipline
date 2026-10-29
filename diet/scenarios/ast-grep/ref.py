"""The tasks are solvable and their gaps are real (#971): python3 -I ref.py [SEEDS].

For SEEDS fresh instances (20 unless given), grades one reference command per family and one naive command that
walks into the family's gap, through `check.py`'s own path (`command.py`, confined), against `gen.py`'s truth.
Every reference must be right on every seed, and every naive command wrong on at least one. The references are
also the canned replies' answers (`make_scenario.py`). Needs `ast-grep` on PATH and macOS; not part of `./verify.sh`.
"""
import os, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import command, gen, rubric  # noqa: E402

RULE = lambda body: "ast-grep scan --inline-rules 'id: q\nlanguage: ts\nrule:\n" + body + "' src"


def reference(family, p):
    return {
        "ret-decl": f"ast-grep run -l ts -p 'function $F($$$A): {p['target']} {{ $$$B }}' src",
        "ret-arrow": f"ast-grep run -l ts -p 'const $N = ($$$A): {p['arrow_target']} => $B' src",
        "zero-calls": f"ast-grep run -l ts -p '{p['zero_fn']}($$$A)' src",
        "maybe-defined": f"ast-grep outline src --match '^{p['maybe']}$'",
        "public-surface": "ast-grep outline src/index.ts",
        # The type form parses with `from $P` and `from $$$P` alike; the value form, `export { $$$S } from $P`, doesn't
        # parse under -l ts ("Cannot parse query as a valid pattern"), with `$P` or `$$$P` (measured, 0.44.0, 2026-10-29).
        "public-types": "ast-grep run -l ts -p 'export type { $$$S } from $$$P' src/index.ts",
        "await-any": RULE("  kind: function_declaration\n  has:\n    pattern: await $E\n    stopBy: end"),
        "log-in-try": RULE(f"  pattern: {p['log_fn']}($$$A)\n  inside:\n    kind: try_statement\n    stopBy: end"),
        "emit-calls": "ast-grep run -l ts -p '$emit($$$A)' src",
        "track-calls": "ast-grep run -l ts -p '$track($$$A)' src",
        "fn-outline": f"ast-grep outline src --items structure --match '^{p['line_fn']}$'",
        "itf-outline": f"ast-grep outline src --match '^{p['line_itf']}$'",
    }[family]


def naive(family, p):
    return {
        "ret-decl": "ast-grep run -l ts -p 'function $F($$$A) { $$$B }' src",
        "ret-arrow": "ast-grep run -l ts -p 'const $N = ($$$A) => $B' src",
        "zero-calls": f"ast-grep run -l ts -p '{p['zero_fn']}' src",
        "maybe-defined": f"ast-grep outline src --match '{p['maybe'][:5]}'",
        "public-surface": "ast-grep outline src --items exports",
        "public-types": "ast-grep outline src/index.ts",
        "await-any": RULE("  kind: function_declaration\n  has:\n    pattern: await $E"),
        "log-in-try": RULE(f"  pattern: {p['log_fn']}($$$A)\n  inside:\n    kind: try_statement"),
        "emit-calls": 'ast-grep run -l ts -p "$emit($$$A)" src',
        "track-calls": "ast-grep run -l ts -p 'track($$$A)' src",
        "fn-outline": f"ast-grep outline src --match '{p['line_fn'][:4]}'",
        "itf-outline": f"ast-grep outline src --match '{p['line_itf']}'",
    }[family]


def overbroad(family, p):
    """Commands that also match what the task excludes: right by execution only on a fixture with nothing for them to
    wrongly catch. gen.py plants that (a near-name, a second call in a try, a Promise<T> return, a non-const arrow), so
    every one must execute wrong on every seed. None where a family has no over-broad form here."""
    return {
        "fn-outline": f"ast-grep outline src --items structure --match '{p['line_fn']}'",
        "itf-outline": f"ast-grep outline src --type interface --match '{p['line_itf']}'",
        # Anchored at one end only (round 4): a suffix near-name (`<prefix><Name>`) catches it.
        "fn-outline-suffix": f"ast-grep outline src --items structure --match '{p['line_fn']}$'",
        "itf-outline-suffix": f"ast-grep outline src --type interface --match '{p['line_itf']}$'",
        "maybe-defined": f"ast-grep outline src --match '{p['maybe']}'",
        "log-in-try": "ast-grep run -k 'try_statement > statement_block call_expression' -l ts src",
        "ret-arrow": RULE(f"  kind: arrow_function\n  has:\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^{p['arrow_target']}$"),
        "ret-decl": RULE(f"  kind: function_declaration\n  has:\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^{p['target']}$\n      stopBy: end"),
        # Round 5: a two-argument log call in a try, and a barrel line exporting two types at once.
        "log-in-try-onearg": RULE(f"  all:\n    - pattern: {p['log_fn']}($A)\n    - inside:\n        kind: try_statement\n        stopBy: end"),
        "public-types-onename": "ast-grep run -p 'export type { $X }' -l typescript src/index.ts",
    }.get(family)


def alternatives(family, p):
    """Other right forms, from the live answers (rubric review, Dispatch 2026-10-27): each must execute right on every
    seed, distractors included, before the rubric may call it right."""
    return {
        "ret-decl": [RULE(f"  kind: function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: ^:\\s*{p['target']}$"),
                     # `all:` of the kind and the `has` (the floor's gap-fill iter-3 t1).
                     RULE(f"  all:\n    - kind: function_declaration\n    - has:\n        field: return_type\n        regex: \"^: {p['target']}$\"")],
        "ret-arrow": [RULE(f"  kind: arrow_function\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: ^:\\s*{p['arrow_target']}$\n"
                           "  inside:\n    kind: variable_declarator\n    inside:\n      kind: lexical_declaration\n      regex: ^const"),
                      # Round 5, the Flash-Next instance's h1: flow style, `all:` of the return type and the const.
                      "ast-grep scan --inline-rules '{language: typescript, id: q, rule: {kind: arrow_function, all: [{has: {field: return_type, "
                      f"kind: type_annotation, has: {{kind: type_identifier, regex: ^{p['arrow_target']}$}}}}}}, {{inside: {{kind: variable_declarator, "
                      "field: value, inside: {kind: lexical_declaration, regex: ^const}}}]}}' src"],
        # Round 5, the Flash-Next instance's t4: the body, then an await at any depth within it.
        "await-any": [RULE("  kind: function_declaration\n  has:\n    field: body\n    kind: statement_block\n    has:\n      kind: await_expression\n      stopBy: end"),
                      # A `:has()` selector (Dispatch, 2026-10-28): the floor's gap-fill iter-2 t4.
                      "ast-grep run -k 'function_declaration:has(await_expression)' src/",
                      # `any:` of kinds (Dispatch, 2026-10-28): the Flash-Next instance's gap-fill t4.
                      RULE("  any:\n    - kind: function_declaration\n    - kind: generator_function_declaration\n  has:\n    field: body\n    kind: statement_block\n    has:\n      kind: await_expression\n      stopBy: end"),
                      RULE("  any:\n    - kind: function_declaration\n  has:\n    pattern: await $E\n    stopBy: end"),
                      RULE("  all:\n    - kind: function_declaration\n    - has:\n        pattern: await $A\n        stopBy: end")],
        "public-surface": ["ast-grep run -l ts -k export_statement src/index.ts"],
        "public-types": ["ast-grep run -p \"export type { \\$\\$\\$ } from '\\$PATH';\" -l ts src/index.ts"],
        "log-in-try": [RULE(f"  all:\n    - pattern: {p['log_fn']}($$$)\n    - inside:\n        pattern: \"try {{ $$$ }}\"\n        stopBy: end"),
                       RULE(f"  all:\n    - pattern: {p['log_fn']}($$$)\n    - inside:\n        kind: try_statement\n        stopBy: end"),
                       RULE(f"  all:\n    - inside:\n        kind: try_statement\n        stopBy: end\n    - pattern: {p['log_fn']}($$$)")],
    }.get(family, [])


def named_wrongs(family, p):
    """Forms the rubric names wrong (round 4): each must execute wrong on at least one seed."""
    return {
        "public-surface": ["ast-grep run -p 'export $$$' -l ts src/index.ts"],
        "await-any": [RULE("  all:\n    - pattern: function $N($$$) { $$$ }\n    - has:\n        pattern: await $X\n        stopBy: end"),
                      # The floor's gap-fill iter-2 t4 is right: see alternatives.
                      # The floor's gap-fill iter-1 t4: an async function expression is the distractor (Dispatch, 2026-10-28).
                      RULE("  any:\n    - kind: function_declaration\n    - kind: function_expression\n    - kind: generator_function_declaration\n  has:\n    pattern: await $X\n    stopBy: end")],
        # Round 5, the floor's answers: `...` as literal text, and `$A` as one parameter.
        "ret-decl": [f"ast-grep run -p 'function $NAME(...$ARGS): {p['target']} {{}}' src",
                     f"ast-grep run -p 'function $F($A): {p['target']} {{ $$$B }}' -l ts src"],
        "zero-calls": [f"ast-grep run -p '{p['zero_fn']}(...)' src"],
        "ret-arrow": [f"ast-grep run -p 'const $NAME = ($ARG): {p['arrow_target']} => $BODY' --selector arrow_function -l TypeScript src",
                      f"ast-grep run -p 'const $NAME = ($ARG): {p['arrow_target']} => $BODY' -l ts src"],
    }.get(family, [])


def kind_filtered(family, p):
    """A filter on a kind the ask doesn't name: right only on the seeds whose item happens to be that kind, so it must
    execute wrong on at least one seed."""
    return {"maybe-defined": f"ast-grep outline src --items structure --type struct --match '^{p['maybe']}$'"}.get(family)


def right(cmd, root, want):
    try:
        return command.matches(command.checked(cmd), root) == sorted(want)
    except command.Refused:
        return False


ALTERNATIVES = ("ret-decl", "ret-arrow", "public-surface", "public-types", "log-in-try", "await-any")
# Over-broad forms caught by a distractor (round 4's suffix near-name; round 5's two-argument call and two-name export):
# each must execute wrong on every seed.
VARIANTS = {"fn-outline": "suffix", "itf-outline": "suffix", "log-in-try": "onearg", "public-types": "onename"}
OVERBROAD = ("fn-outline", "itf-outline", "maybe-defined", "log-in-try", "ret-arrow", "ret-decl")


def main():
    seeds = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    ref_wrong, naive_right, broad_right, total = {}, {}, {}, 0
    alt_wrong, kind_wrong, named_wrong_wrong = {}, {}, {}
    with tempfile.TemporaryDirectory() as d:
        for seed in range(1, seeds + 1):
            root = os.path.join(d, f"s{seed}")
            truth = gen.write(seed * 101, root, os.path.join(d, f"s{seed}.json"))
            total += 1
            for fam, want in truth["answers"].items():
                # The verdict that counts is the static one (rubric.py): the reference must be right, the naive not.
                static_ref = rubric.judge(reference(fam, truth["params"]), fam, truth["params"])
                if static_ref[0] != "right":
                    ref_wrong[fam] = ref_wrong.get(fam, 0) + 1
                    print(f"ref: seed {seed * 101} {fam}: the rubric calls the reference {static_ref}")
                if rubric.judge(naive(fam, truth["params"]), fam, truth["params"])[0] == "right":
                    naive_right[fam] = naive_right.get(fam, 0) + 1
                    print(f"ref: seed {seed * 101} {fam}: the rubric calls the naive command right")
                for alt in alternatives(fam, truth["params"]):
                    if not right(alt, root, want):
                        alt_wrong[fam] = alt_wrong.get(fam, 0) + 1
                        print(f"ref: seed {seed * 101} {fam}: an alternative executed wrong: {alt!r}")
                kf = kind_filtered(fam, truth["params"])
                if kf and not right(kf, root, want):
                    kind_wrong[fam] = kind_wrong.get(fam, 0) + 1
                for i, nw in enumerate(named_wrongs(fam, truth["params"])):
                    if not right(nw, root, want):
                        named_wrong_wrong[(fam, i)] = named_wrong_wrong.get((fam, i), 0) + 1
                variant = overbroad(f"{fam}-{VARIANTS[fam]}", truth["params"]) if fam in VARIANTS else None
                if variant and right(variant, root, want):
                    broad_right[f"{fam}-{VARIANTS[fam]}"] = broad_right.get(f"{fam}-{VARIANTS[fam]}", 0) + 1
                broad = overbroad(fam, truth["params"])
                if broad and right(broad, root, want):
                    broad_right[fam] = broad_right.get(fam, 0) + 1
                # Execution, the audit's way, checks the same references against the truth.
                if not right(reference(fam, truth["params"]), root, want):
                    ref_wrong[fam] = ref_wrong.get(fam, 0) + 1
                    print(f"ref: seed {seed * 101} {fam}: {command.matches(command.checked(reference(fam, truth['params'])), root)} != {want}")
                if right(naive(fam, truth["params"]), root, want):
                    naive_right[fam] = naive_right.get(fam, 0) + 1
    bad = 0
    for fam in gen.build(1)[1]["answers"]:
        r, n, b = ref_wrong.get(fam, 0), naive_right.get(fam, 0), broad_right.get(fam, 0)
        print(f"ref: {fam}: reference wrong {r}/{total}, naive right {n}/{total}" + (f", over-broad executed right {b}/{total}" if fam in OVERBROAD else ""))
        bad += (r > 0) + (n == total) + (b > 0)
        if fam in ALTERNATIVES:
            print(f"ref: {fam}: alternatives executed wrong {alt_wrong.get(fam, 0)}")
            bad += alt_wrong.get(fam, 0) > 0
        if fam in VARIANTS:
            sb = broad_right.get(f"{fam}-{VARIANTS[fam]}", 0)
            print(f"ref: {fam}: the {VARIANTS[fam]} over-broad form executed right {sb}/{total}")
            bad += sb > 0
        for i, _ in enumerate(named_wrongs(fam, gen.build(1)[1]["params"])):
            nw = named_wrong_wrong.get((fam, i), 0)
            print(f"ref: {fam}: named wrong {i + 1} executed wrong on {nw}/{total} seeds")
            bad += nw == 0
        if fam == "maybe-defined":
            k = kind_wrong.get(fam, 0)
            print(f"ref: {fam}: the kind filter executed wrong on {k}/{total} seeds")
            bad += k == 0
    print(f"ref: {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
