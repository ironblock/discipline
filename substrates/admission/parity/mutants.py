#!/usr/bin/env python3
"""Seeded mutants for the parity instrument (#143, I5): each mutation in fixtures/mutants.json is applied to a
scratch copy of band.py or apply.py, and selftest.sh must exit 1 on it. Exit 0 only when every mutant is killed
and the unmutated copy passes; 1 otherwise; 2 when a mutation's text is not found (a stale mutant)."""
import json, os, pathlib, shutil, subprocess, sys, tempfile
here = pathlib.Path(__file__).resolve().parent; root = here.parents[2]
muts = json.loads((here / "fixtures/mutants.json").read_text())["mutants"]
def run(override):
    with tempfile.TemporaryDirectory() as td:
        d = pathlib.Path(td) / "parity"; shutil.copytree(here, d, ignore=shutil.ignore_patterns("__pycache__"))
        for f, text in override.items(): (d / f).write_text(text)
        return subprocess.run(["bash", str(d / "selftest.sh")], capture_output=True, text=True, env={**os.environ, "PARITY_ROOT": str(root)}).returncode
base = run({}); bad = int(base != 0); print(f"{'ok  ' if base == 0 else 'FAIL'}  unmutated: selftest rc={base}")
for m in muts:
    src = (here / m["file"]).read_text()
    if src.count(m["old"]) != 1:
        print(f"STALE {m['label']}: the mutation's text occurs {src.count(m['old'])} times in {m['file']}"); sys.exit(2)
    rc = run({m["file"]: src.replace(m["old"], m["new"], 1)}); killed = rc == 1; bad += not killed
    print(f"{'ok  ' if killed else 'FAIL'}  mutant: {m['label']} -> selftest rc={rc}")
print(f"parity mutants: {len(muts) - (bad - int(base != 0))} of {len(muts)} killed"); sys.exit(1 if bad else 0)
