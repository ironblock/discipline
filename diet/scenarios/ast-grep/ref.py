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
        # `from $P` doesn't parse ("Multiple AST nodes"); `from $$$P` does.
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
        "maybe-defined": f"ast-grep outline src --match '{p['maybe']}'",
        "log-in-try": "ast-grep run -k 'try_statement > statement_block call_expression' -l ts src",
        "ret-arrow": RULE(f"  kind: arrow_function\n  has:\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^{p['arrow_target']}$"),
        "ret-decl": RULE(f"  kind: function_declaration\n  has:\n    field: return_type\n    has:\n      kind: type_identifier\n      regex: ^{p['target']}$\n      stopBy: end"),
    }.get(family)


def alternatives(family, p):
    """Other right forms, from the live answers (rubric review, Dispatch 2026-10-27): each must execute right on every
    seed, distractors included, before the rubric may call it right."""
    return {
        "ret-decl": [RULE(f"  kind: function_declaration\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: ^:\\s*{p['target']}$")],
        "ret-arrow": [RULE(f"  kind: arrow_function\n  has:\n    field: return_type\n    kind: type_annotation\n    regex: ^:\\s*{p['arrow_target']}$\n"
                           "  inside:\n    kind: variable_declarator\n    inside:\n      kind: lexical_declaration\n      regex: ^const")],
        "public-surface": ["ast-grep run -l ts -k export_statement src/index.ts"],
        "log-in-try": [RULE(f"  all:\n    - pattern: {p['log_fn']}($$$)\n    - inside:\n        kind: try_statement\n        stopBy: end"),
                       RULE(f"  all:\n    - inside:\n        kind: try_statement\n        stopBy: end\n    - pattern: {p['log_fn']}($$$)")],
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


ALTERNATIVES = ("ret-decl", "ret-arrow", "public-surface", "log-in-try")
OVERBROAD = ("fn-outline", "itf-outline", "maybe-defined", "log-in-try", "ret-arrow", "ret-decl")


def main():
    seeds = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    ref_wrong, naive_right, broad_right, total = {}, {}, {}, 0
    alt_wrong, kind_wrong = {}, {}
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
        if fam == "maybe-defined":
            k = kind_wrong.get(fam, 0)
            print(f"ref: {fam}: the kind filter executed wrong on {k}/{total} seeds")
            bad += k == 0
    print(f"ref: {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
