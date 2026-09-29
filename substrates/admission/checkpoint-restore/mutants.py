#!/usr/bin/env python3
"""Seeded mutants for checkpoint_restore.py: each mutation in fixtures/mutants.json is applied to a scratch
copy, and the selftest must exit 1 on it. Exit 0 only when every mutant is killed and the unmutated copy
passes; 1 otherwise; 2 when a mutation's text is not found (a stale mutant)."""
import json, pathlib, shutil, subprocess, sys, tempfile
here = pathlib.Path(__file__).resolve().parent
FILES = ("checkpoint_restore.py", "criterion.toml")
muts = json.loads((here / "fixtures/mutants.json").read_text())["mutants"]
def run(override):
    with tempfile.TemporaryDirectory() as td:
        shutil.copytree(here / "fixtures", pathlib.Path(td) / "fixtures")
        for f in FILES: (pathlib.Path(td) / f).write_text(override.get(f) or (here / f).read_text())
        return subprocess.run([sys.executable, "-B", str(pathlib.Path(td) / "checkpoint_restore.py"), "selftest"], capture_output=True, text=True).returncode
base = run({}); bad = int(base != 0); print(f"{'ok  ' if base == 0 else 'FAIL'}  unmutated: selftest rc={base}")
for m in muts:
    f = m.get("file", "checkpoint_restore.py"); src = (here / f).read_text()
    if src.count(m["old"]) != 1:
        print(f"STALE {m['label']}: the mutation's text occurs {src.count(m['old'])} times in {f}"); sys.exit(2)
    rc = run({f: src.replace(m["old"], m["new"], 1)}); killed = rc == 1; bad += not killed
    print(f"{'ok  ' if killed else 'FAIL'}  mutant: {m['label']} -> selftest rc={rc}")
print(f"checkpoint_restore mutants: {len(muts) - (bad - int(base != 0))} of {len(muts)} killed"); sys.exit(1 if bad else 0)
