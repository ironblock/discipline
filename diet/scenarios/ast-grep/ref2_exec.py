"""Task set 2 by execution (#971): python3 -I ref2_exec.py [SEEDS], or as `ref.py`'s second half.

For SEEDS fresh instances (20 unless given), per set-2 family: the reference must execute right on every seed; the
naive command wrong on at least one; the over-broad command wrong on EVERY seed (the distractors make it so); a
`--type` filter the ask doesn't name, wrong on at least one. Needs `ast-grep` on PATH and macOS.
"""
import os, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import command, gen, ref2, rubric  # noqa: E402


def right(cmd, root, want):
    try:
        return command.matches(command.checked(cmd), root) == sorted(want)
    except command.Refused:
        return False


def run(seeds=20, verbose=True):
    """Per family: {reference wrong, naive right, over-broad right, kind filter wrong} counts, and the seeds' count."""
    counts = {f: {"ref_wrong": 0, "naive_right": 0, "broad_right": 0, "kind_wrong": 0, "static": 0} for f in ref2.FAMILIES}
    with tempfile.TemporaryDirectory() as d:
        for seed in range(1, seeds + 1):
            root = os.path.join(d, f"s{seed}")
            truth = gen.write(seed * 101, root, os.path.join(d, f"s{seed}.json"))
            p = truth["params"]
            for fam in ref2.FAMILIES:
                want = truth["answers"][fam]
                c = counts[fam]
                # Statically (the verdict): the reference right; the naive, over-broad and kind-filtered forms not.
                judged = {"reference": rubric.judge(ref2.reference(fam, p), fam, p)}
                for name, cmd in (("naive", ref2.naive(fam, p)), ("over-broad", ref2.overbroad(fam, p)), ("kind filter", ref2.kind_filtered(fam, p))):
                    if cmd:
                        judged[name] = rubric.judge(cmd, fam, p)
                for name, (verdict, why) in judged.items():
                    if (verdict == "right") != (name == "reference"):
                        c["static"] += 1
                        if verbose:
                            print(f"ref2: seed {seed * 101} {fam}: the rubric calls the {name} {verdict}: {why}")
                if not right(ref2.reference(fam, p), root, want):
                    c["ref_wrong"] += 1
                    if verbose:
                        got = command.matches(command.checked(ref2.reference(fam, p)), root)
                        print(f"ref2: seed {seed * 101} {fam}: reference {got} != {want}")
                c["naive_right"] += right(ref2.naive(fam, p), root, want)
                broad = ref2.overbroad(fam, p)
                c["broad_right"] += bool(broad) and right(broad, root, want)
                kf = ref2.kind_filtered(fam, p)
                c["kind_wrong"] += bool(kf) and not right(kf, root, want)
    return counts, seeds


def report(counts, seeds):
    bad = 0
    for fam, c in counts.items():
        line = f"ref2: {fam}: reference wrong {c['ref_wrong']}/{seeds}, naive right {c['naive_right']}/{seeds}"
        line += f", over-broad right {c['broad_right']}/{seeds}" if ref2.overbroad(fam, gen.build(1)[1]["params"]) else ""
        line += f", kind filter wrong {c['kind_wrong']}/{seeds}" if fam.startswith("kind-") else ""
        print(line)
        if c["static"]:
            print(f"ref2: {fam}: {c['static']} static misjudgment(s)")
        bad += (c["ref_wrong"] > 0) + (c["naive_right"] == seeds) + (c["broad_right"] > 0) + (c["static"] > 0)
        bad += fam.startswith("kind-") and c["kind_wrong"] == 0
    print(f"ref2: {bad} problem(s)")
    return bad


if __name__ == "__main__":
    sys.exit(1 if report(*run(int(sys.argv[1]) if len(sys.argv) > 1 else 20)) else 0)
