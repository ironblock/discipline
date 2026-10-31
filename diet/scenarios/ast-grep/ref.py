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
        # The type form parses with `from $P` and `from $$$P` alike; the value form, `export { $$$S } from $P`, doesn't
        # parse under -l ts ("Cannot parse query as a valid pattern"), with `$P` or `$$$P` (measured, 0.44.0, 2026-10-29).
        "public-types": "ast-grep run -l ts -p 'export type { $$$S } from $$$P' src/index.ts",
        "await-any": RULE("  kind: function_declaration\n  has:\n    pattern: await $E\n    stopBy: end"),
        "log-in-try": RULE(f"  pattern: {p['log_fn']}($$$A)\n  inside:\n    kind: try_statement\n    stopBy: end"),
        "emit-calls": "ast-grep run -l ts -p '$emit($$$A)' src",
        "track-calls": "ast-grep run -l ts -p '$track($$$A)' src",
        "fn-outline": f"ast-grep outline src --items structure --match '^{p['line_fn']}$'",
        "itf-outline": f"ast-grep outline src --match '^{p['line_itf']}$'",
    }.get(family)


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
    }.get(family)


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


def kind_filtered(family, p):
    """A filter on a kind the ask doesn't name: right only on the seeds whose item happens to be that kind, so it must
    execute wrong on at least one seed."""
    return {"maybe-defined": f"ast-grep outline src --items structure --type struct --match '^{p['maybe']}$'"}.get(family)


def right(cmd, root, want):
    try:
        return command.matches(command.checked(cmd), root) == sorted(want)
    except command.Refused:
        return False


# The tasks' families; the fixture's other families are its decoys (gen2.py), with no task.
FAMILIES = ("ret-decl", "ret-arrow", "zero-calls", "maybe-defined", "public-surface", "public-types", "await-any", "log-in-try",
            "emit-calls", "track-calls", "fn-outline", "itf-outline")
# Over-broad forms caught by a distractor (a suffix near-name, a two-argument call, a two-name export): each must
# execute wrong on every seed.
VARIANTS = {"fn-outline": "suffix", "itf-outline": "suffix", "log-in-try": "onearg", "public-types": "onename"}
OVERBROAD = ("fn-outline", "itf-outline", "maybe-defined", "log-in-try", "ret-arrow", "ret-decl")


def main():
    seeds = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    ref_wrong, naive_right, broad_right, kind_wrong, total = {}, {}, {}, {}, 0
    with tempfile.TemporaryDirectory() as d:
        for seed in range(1, seeds + 1):
            root = os.path.join(d, f"s{seed}")
            truth = gen.write(seed * 101, root, os.path.join(d, f"s{seed}.json"))
            total += 1
            for fam in FAMILIES:
                want, p = truth["answers"][fam], truth["params"]
                if not right(reference(fam, p), root, want):
                    ref_wrong[fam] = ref_wrong.get(fam, 0) + 1
                    print(f"ref: seed {seed * 101} {fam}: {command.matches(command.checked(reference(fam, p)), root)} != {want}")
                if right(naive(fam, p), root, want):
                    naive_right[fam] = naive_right.get(fam, 0) + 1
                kf = kind_filtered(fam, p)
                if kf and not right(kf, root, want):
                    kind_wrong[fam] = kind_wrong.get(fam, 0) + 1
                for key in [fam] + ([f"{fam}-{VARIANTS[fam]}"] if fam in VARIANTS else []):
                    broad = overbroad(key, p)
                    if broad and right(broad, root, want):
                        broad_right[key] = broad_right.get(key, 0) + 1
    bad = 0
    for fam in FAMILIES:
        r, n, b = ref_wrong.get(fam, 0), naive_right.get(fam, 0), broad_right.get(fam, 0)
        print(f"ref: {fam}: reference wrong {r}/{total}, naive right {n}/{total}" + (f", over-broad executed right {b}/{total}" if fam in OVERBROAD else ""))
        bad += (r > 0) + (n == total) + (b > 0)
        if fam in VARIANTS:
            sb = broad_right.get(f"{fam}-{VARIANTS[fam]}", 0)
            print(f"ref: {fam}: the {VARIANTS[fam]} over-broad form executed right {sb}/{total}")
            bad += sb > 0
        if fam == "maybe-defined":
            k = kind_wrong.get(fam, 0)
            print(f"ref: {fam}: the kind filter executed wrong on {k}/{total} seeds")
            bad += k == 0
    print(f"ref: {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
