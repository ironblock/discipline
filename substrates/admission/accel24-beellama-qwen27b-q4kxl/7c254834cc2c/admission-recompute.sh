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
import hashlib, json, pathlib, re, subprocess, sys, tomllib
here, repo = map(pathlib.Path, sys.argv[1:])
adm = tomllib.loads((here / "admission.toml").read_text()); fails = []
def manifest(root, exclude=(), exact=()):
    files = sorted(p for p in root.rglob("*") if p.is_file() and not any(p.relative_to(root).as_posix().startswith(e) for e in exclude)
                   and p.relative_to(root).as_posix() not in exact and "__pycache__" not in p.parts)
    return "".join(f"{p.relative_to(root).as_posix()}\t{hashlib.sha256(p.read_bytes()).hexdigest()}\n" for p in files)
EXCLUDE = {"cells": ("depth/",)}
EXACT = {"cells": {"admission.toml", "admission-recompute.sh"}}
# the structure is the rule's, not the record's: exactly these three results, each run through its own recompute.sh
if sorted(adm["results"]) != ["cells", "depth", "parity"]: fails.append(f"the results are {sorted(adm['results'])}, not cells, depth and parity")
# the record's identity and its own results are this directory's: the substrate is its parent's name, the
# fingerprint is fingerprint.json's (and names this directory), the cells are this directory, the depth cell its
# depth/, and the parity record declares this substrate in its regime
fpj = json.loads((here / "fingerprint.json").read_text())["sha256"]
if adm.get("substrate") != here.parent.name: fails.append(f"the substrate {adm.get('substrate')!r} is not this directory's {here.parent.name!r}")
if adm.get("fingerprint") != fpj or not fpj.startswith(here.name): fails.append("the fingerprint is not this directory's fingerprint.json")
rel = here.relative_to(repo).as_posix()
if adm["results"].get("cells", {}).get("path") != rel: fails.append("the cells cited are not this directory")
if adm["results"].get("depth", {}).get("path") != rel + "/depth": fails.append("the depth cell cited is not this directory's depth/")
pp = adm["results"].get("parity", {}).get("path", "")
reg = tomllib.loads((repo / pp / "README.md").read_text().split("+++")[1]).get("regime", {}) if pp.startswith("results/") and (repo / pp / "README.md").exists() else {}
if reg.get("arm") != "extraction-seat-parity-refire" or here.parent.name not in reg.get("substrates", []):
    fails.append("the parity record cited is not a parity fire of extraction-acceptance-inverts (arm extraction-seat-parity-refire) whose regime names this substrate")
# the constitutional cells planning named must be present (5882266710)
for need in ("checkpoint_restore", "kwarg_delivery", "canary"):
    if need not in tomllib.loads((here / "cells.toml").read_text()): fails.append(f"cells.toml has no {need} cell")
for name, r in adm["results"].items():
    root = repo / r["path"]
    if r["recompute"] != "recompute.sh": fails.append(f"{name}: its recompute is {r['recompute']!r}, not its own recompute.sh"); continue
    got = hashlib.sha256(manifest(root, EXCLUDE.get(name, ()), EXACT.get(name, ())).encode()).hexdigest()
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
if ("word" in adm) == bool(str(adm.get("word_held", "")).strip()): fails.append("admission.toml must carry exactly one of a word and the reason it is held")
cellnames = [k for k, v in tomllib.loads((here / "cells.toml").read_text()).items() if isinstance(v, dict)]
named_re = r"not admitted \((cells|depth|parity|" + "|".join(map(re.escape, cellnames)) + r")\b[^)]*\)"
if "word" in adm and not (adm["word"] == "admitted" or re.fullmatch(named_re, adm["word"])):
    fails.append(f"the word {adm['word']!r} is not admitted, or not admitted with the failing result named")
# not admitted must name a result that is actually failing: a cell reading fail, a depth word fail, or a parity
# word refuted or inconclusive; an unadjudicated cell alone neither admits nor bars, so the word is held (5885436821)
if str(adm.get("word", "")).startswith("not admitted ("):
    named = re.match(named_re, adm["word"]).group(1)
    cw = adm["results"].get("cells", {}).get("words", {})
    failing = cw.get(named) == "fail" if named in cellnames else {"cells": any(w == "fail" for w in cw.values()), "depth": adm["results"].get("depth", {}).get("word") == "fail",
               "parity": adm["results"].get("parity", {}).get("word") in ("refuted", "inconclusive")}[named]
    if not failing: fails.append(f"not admitted names {named}, which is not failing")
if adm.get("word") == "admitted":
    words = adm["results"].get("cells", {}).get("words", {})
    unpassing = [k for k, w in words.items() if not (w in ("pass", "unreported") or re.fullmatch(r"n/a \(.+\)", w) or (k == "canary" and w == "baseline"))]  # unreported admits (5884256685)
    if unpassing or adm["results"].get("depth", {}).get("word") != "pass" or adm["results"].get("parity", {}).get("word") != "supported":
        fails.append(f"admitted is written while a result is not passing: cells {unpassing}, depth {adm['results'].get('depth', {}).get('word')!r}, parity {adm['results'].get('parity', {}).get('word')!r}")
for f in fails: print(f"admission-recompute: {f}")
if not fails: print(f"admission-recompute: three results re-hash and recompute; " + (f"word {adm['word']!r} (derived by derive_admission.py, #183)" if "word" in adm else f"word held: {adm['word_held']}"))
sys.exit(1 if fails else 0)
PY
