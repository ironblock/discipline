#!/usr/bin/env bash
# Recompute the floor's admission record (#143): each of the three results it cites is re-hashed as a file
# manifest (every file under the result, sorted by path, one "path<TAB>sha256" line each; the cells result
# excludes depth/ and the admission files) and must equal the digest admission.toml states; each result's own
# recompute must exit 0; and each result's word must be the one admission.toml records.
# Exit 0 when all hold, 1 when any fails.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd -- "$here/../../../.." && pwd)"
python3 - "$here" "$repo" <<'PY' || exit 1
import hashlib, json, pathlib, subprocess, sys, tomllib
here, repo = map(pathlib.Path, sys.argv[1:])
adm = tomllib.loads((here / "admission.toml").read_text()); fails = []
def manifest(root, exclude=()):
    files = sorted(p for p in root.rglob("*") if p.is_file() and not any(p.relative_to(root).as_posix().startswith(e) for e in exclude)
                   and "__pycache__" not in p.parts)
    return "".join(f"{p.relative_to(root).as_posix()}\t{hashlib.sha256(p.read_bytes()).hexdigest()}\n" for p in files)
EXCLUDE = {"cells": ("depth/", "admission.toml", "admission-recompute.sh")}
for name, r in adm["results"].items():
    root = repo / r["path"]
    got = hashlib.sha256(manifest(root, EXCLUDE.get(name, ())).encode()).hexdigest()
    if got != r["manifest_sha256"]: fails.append(f"{name}: {r['path']} hashes to {got[:16]}, not {r['manifest_sha256'][:16]}")
    rc = subprocess.run(["bash", str(root / r["recompute"])], capture_output=True).returncode
    if rc != 0: fails.append(f"{name}: its recompute exited {rc}")
    if name == "depth":
        w = json.loads((root / "raw/decide.json").read_text())["word"]
    elif name == "parity":
        w = tomllib.loads((root / "README.md").read_text().split("+++")[1])["result"]
    else:
        w = None
    if w is not None and w != r["word"]: fails.append(f"{name}: the result reads {w!r}; admission.toml records {r['word']!r}")
if "cells" in adm["results"]:
    cells = tomllib.loads((here / "cells.toml").read_text())
    words = {k: v.get("word") for k, v in cells.items() if isinstance(v, dict)}
    if words != adm["results"]["cells"]["words"]: fails.append(f"cells: cells.toml's words {words} are not those admission.toml records")
if "word" not in adm and not adm.get("word_held"): fails.append("admission.toml carries neither a word nor the reason it is held")
for f in fails: print(f"admission-recompute: {f}")
if not fails: print(f"admission-recompute: three results re-hash and recompute; " + (f"word {adm['word']!r} (written by hand)" if "word" in adm else f"word held: {adm['word_held']}"))
sys.exit(1 if fails else 0)
PY
