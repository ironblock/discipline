"""The tasks are solvable and their gaps are real (#971): python3 -I ref.py [SEEDS].

For SEEDS fresh instances (20 unless given), runs one reference ast-grep solution per family and one naive try that
walks into the family's gap, and grades both against `gen.py`'s truth. The truth never comes from here: this checks
the tasks, not the grader. Every reference must be right on every seed, and every naive try wrong on at least one.
Needs `ast-grep` on PATH (0.44.0 when written); not part of `./verify.sh`.
"""
import json, os, re, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gen  # noqa: E402


def sg(repo, *args, shell=None):
    if shell:
        out = subprocess.run(["bash", "-c", shell], cwd=repo, capture_output=True, text=True)
    else:
        out = subprocess.run(["ast-grep", *args], cwd=repo, capture_output=True, text=True)
    return out.stdout


def matches(repo, *args, shell=None):
    text = sg(repo, shell=shell + " --json") if shell else sg(repo, *args, "--json")
    return json.loads(text) if text.strip().startswith("[") else []


def rule(body):
    return ["scan", "--inline-rules", "id: q\nlanguage: ts\nrule:\n" + body, "src"]


def fn_names(ms):
    return sorted({re.search(r"function (\w+)", m["text"]).group(1) for m in ms})


def solve(repo, p, naive):
    """Each family's answer items, by the reference solution or the naive try."""
    t, u, z, m, log = p["target"], p["arrow_target"], p["zero_fn"], p["maybe"], p["log_fn"]
    a = {}
    pat = f"function $F($$$A): {t} {{ $$$B }}" if not naive else "function $F($$$A) { $$$B }"
    a["ret-decl"] = sorted({x["metaVariables"]["single"]["F"]["text"] for x in matches(repo, "run", "-l", "ts", "-p", pat, "src")})
    pat = f"const $N = ($$$A): {u} => $B" if not naive else "const $N = ($$$A) => $B"
    a["ret-arrow"] = sorted({x["metaVariables"]["single"]["N"]["text"] for x in matches(repo, "run", "-l", "ts", "-p", pat, "src")})
    if naive:
        a["zero-calls"] = [str(sg(repo, shell=f"grep -rn '{z}(' src | wc -l").strip())]
        a["maybe-defined"] = ["yes" if sg(repo, shell=f"grep -rl '{m}' src").strip() else "no"]
    else:
        a["zero-calls"] = [str(len(matches(repo, "run", "-l", "ts", "-p", f"{z}($$$A)", "src")))]
        a["maybe-defined"] = ["no" if "nothing found" in sg(repo, "outline", "src", "--match", f"^{m}$") else "yes"]
    # `export { $$$S } from $P` does not parse as a pattern ("Multiple AST nodes"): the statement's kind does.
    lines = [x["text"] for x in matches(repo, "scan", "--inline-rules", "id: q\nlanguage: ts\nrule:\n  kind: export_statement", "src/index.ts")]
    barrel = [l for l in lines if not l.startswith("export type")]
    typed = [l for l in lines if l.startswith("export type")]
    spec = lambda line: [s.split(" as ")[-1].strip() for s in re.search(r"\{(.*)\}", line).group(1).split(",")]
    if naive:  # every function some file under src/ exports
        out = sg(repo, "outline", "src", "--items", "exports")
        a["public-fns"] = sorted({n.strip() for l in out.splitlines() if l.startswith("function:") for n in l[9:].split(",")})
    else:
        a["public-fns"] = sorted({n for l in barrel for n in spec(l)})
    itfs = {x["metaVariables"]["single"]["I"]["text"] for x in matches(repo, "run", "-l", "ts", "-p", "export interface $I { $$$B }", "src")}
    public = set() if naive else {n for l in typed for n in spec(l)}
    a["hidden-itfs"] = sorted(itfs - public) if not naive else sorted(itfs - {n for l in barrel for n in spec(l)})
    stop = "" if naive else "\n    stopBy: end"
    a["await-any"] = fn_names(matches(repo, *rule(f"  kind: function_declaration\n  has:\n    pattern: await $E{stop}")))
    a["log-in-try"] = fn_names(matches(repo, *rule(
        f"  kind: function_declaration\n  has:{stop}\n    pattern: {log}($$$A)\n    inside:\n      kind: try_statement{stop.replace('    ', '      ')}")))
    if naive:  # the pattern in double quotes: the shell takes `$emit` and `$track`
        a["emit-callers"] = fn_names(matches(repo, shell='ast-grep run -l ts -p "$emit($$$A)" src'))
        a["track-count"] = [str(len(matches(repo, shell='ast-grep run -l ts -p "$track($$$A)" src')))]
    else:
        a["emit-callers"] = fn_names(matches(repo, *rule("  kind: function_declaration\n  has:\n    pattern: $emit($$$A)\n    stopBy: end")))
        a["track-count"] = [str(len(matches(repo, "run", "-l", "ts", "-p", "$track($$$A)", "src")))]
    for fam, kind, name in (("fn-line", "function_declaration", p["line_fn"]), ("itf-line", "interface_declaration", p["line_itf"])):
        ms = matches(repo, *rule(f"  kind: {kind}\n  has:\n    field: name\n    regex: ^{name}$"))
        a[fam] = [ms[0]["file"], str(ms[0]["range"]["start"]["line"] + (0 if naive else 1))] if ms else ["", ""]
    return a


def main():
    seeds = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    ref_wrong, naive_right, total = {}, {}, 0
    with tempfile.TemporaryDirectory() as d:
        for seed in range(1, seeds + 1):
            repo = os.path.join(d, f"s{seed}")
            truth = gen.write(seed * 101, repo, os.path.join(d, f"s{seed}.json"))
            total += 1
            for naive, tally in ((False, ref_wrong), (True, naive_right)):
                got = solve(repo, truth["params"], naive)
                for fam, want in truth["answers"].items():
                    right = sorted(got[fam]) == sorted(want["items"]) if want["kind"] == "set" else got[fam] == want["items"]
                    if right == naive:
                        tally[fam] = tally.get(fam, 0) + 1
                        if not naive:
                            print(f"ref: seed {seed * 101} {fam}: got {got[fam]}, want {want['items']}")
    bad = 0
    for fam in gen.build(1)[1]["answers"]:
        r, n = ref_wrong.get(fam, 0), naive_right.get(fam, 0)
        print(f"ref: {fam}: reference wrong {r}/{total}, naive right {n}/{total}")
        bad += (r > 0) + (n == total)
    print(f"ref: {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
