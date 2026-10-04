#!/usr/bin/env python3
"""#320 spike, never merged: each `ci` seeded fault proven red as a test of
its checker, check-ci-coverage.py, rather than through `verify.sh --only ci`
in a sandbox.

For each `ci` case gatelib reads out of verify.sh: copy the tracked tree into a
temp dir (git init, add), run the case's own inject_* function (sourced from
verify.sh, as seeded_case runs it), then run the checker twice --
SPIKE_320_SKIP_RUNS=1 (rule 13's dry and real shard runs skipped), and in full
-- and say which form goes red with the case's own signature. A fault red
under the skip needs none of rule 13's runs, so its successor is a rule test
that costs the checker's ~0.3 s; one red only in full needs the runs.

    ci_successors.py OUT.jsonl      (from the tree's root, on Linux)
"""
import json, os, pathlib, re, shutil, subprocess, sys, tempfile, time
sys.path.insert(0, "scripts")
import gatelib  # noqa: E402

root = pathlib.Path.cwd()
text = (root / "verify.sh").read_text()
cases = [c for c in gatelib.seeded_cases(text) if c.check == "ci"]
head = text.split("\n# main\n", 1)[0]
funcs = pathlib.Path(tempfile.mkdtemp()) / "verify-functions.sh"
funcs.write_text(re.sub(r"^ROOT=.*$", f'ROOT="{root}"', head, count=1, flags=re.M))
files = subprocess.run(["git", "ls-files", "-z"], capture_output=True, check=True).stdout

def red(box, skip):
    env = dict(os.environ)
    if skip:
        env["SPIKE_320_SKIP_RUNS"] = "1"
    t = time.time()
    p = subprocess.run([sys.executable, "scripts/check-ci-coverage.py"], cwd=box, env=env, capture_output=True, text=True)
    return time.time() - t, p.returncode, p.stdout + p.stderr

with open(sys.argv[1], "w") as out:
    for c in cases:
        box = pathlib.Path(tempfile.mkdtemp())
        t = time.time()
        subprocess.run(["xargs", "-0", "cp", "-L", "--parents", "-t", str(box)], input=files, check=True)
        subprocess.run(["git", "-C", str(box), "init", "-q"], check=True)
        subprocess.run(["git", "-C", str(box), "add", "-A"], check=True)
        copy = time.time() - t
        t = time.time()
        inj = subprocess.run(["bash", "-c", f'source "{funcs}" && {c.injection}'], cwd=box, capture_output=True, text=True)
        inject = time.time() - t
        row = {"injection": c.injection, "label": c.label, "copy": copy, "inject": inject, "inject_rc": inj.returncode}
        for skip in (True, False):
            secs, rc, log = red(box, skip)
            row["skip" if skip else "full"] = {"seconds": secs, "rc": rc,
                                               "red_for_its_fault": rc != 0 and bool(re.search(c.signature, log))}
        out.write(json.dumps(row) + "\n"); out.flush()
        shutil.rmtree(box, ignore_errors=True)
        print(f"{c.injection}: skip {row['skip']['red_for_its_fault']} {row['skip']['seconds']:.1f}s, "
              f"full {row['full']['red_for_its_fault']} {row['full']['seconds']:.1f}s", flush=True)
