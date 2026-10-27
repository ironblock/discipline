"""The tasks are solvable and their gaps are real (#971): python3 -I ref.py [SEEDS].

For SEEDS fresh instances (20 unless given), grades one reference command per family and one naive command that
walks into the family's gap, through `check.py`'s own path (`command.py`, confined), against `gen.py`'s truth.
Every reference must be right on every seed, and every naive command wrong on at least one. The references are
also the canned replies' answers (`make_scenario.py`). Needs `ast-grep` on PATH and macOS; not part of `./verify.sh`.
"""
import os, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import command, gen  # noqa: E402

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


def right(cmd, root, want):
    try:
        return command.matches(command.checked(cmd), root) == sorted(want)
    except command.Refused:
        return False


def main():
    seeds = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    ref_wrong, naive_right, total = {}, {}, 0
    with tempfile.TemporaryDirectory() as d:
        for seed in range(1, seeds + 1):
            root = os.path.join(d, f"s{seed}")
            truth = gen.write(seed * 101, root, os.path.join(d, f"s{seed}.json"))
            total += 1
            for fam, want in truth["answers"].items():
                if not right(reference(fam, truth["params"]), root, want):
                    ref_wrong[fam] = ref_wrong.get(fam, 0) + 1
                    print(f"ref: seed {seed * 101} {fam}: {command.matches(command.checked(reference(fam, truth['params'])), root)} != {want}")
                if right(naive(fam, truth["params"]), root, want):
                    naive_right[fam] = naive_right.get(fam, 0) + 1
    bad = 0
    for fam in gen.build(1)[1]["answers"]:
        r, n = ref_wrong.get(fam, 0), naive_right.get(fam, 0)
        print(f"ref: {fam}: reference wrong {r}/{total}, naive right {n}/{total}")
        bad += (r > 0) + (n == total)
    print(f"ref: {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
