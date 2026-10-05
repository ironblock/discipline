#!/usr/bin/env python3
"""A TabbyAPI line's cells directory (#393): `init` writes cells.toml, recompute.sh and admission-recompute.sh from raw/; `check` re-derives
every word that follows from a raw file and refuses one that disagrees (the shim recompute.sh calls it). Cells and where each word comes from:
  identity            raw/identity.json (tabby_identity.py): engine_identity equals the registry entry's; raw/weights-sha256.txt lists every
                      digest the registry's weights_main names; raw/model.json is /v1/model's bytes, its id recorded
  kwarg_delivery      raw/kw.json controls (thinking off 0 reasoning characters, on more than 0) and raw/refusal.json (each refused level 400,
                      the accepted level 200)
  rendered_effort     n/a (ruling 4, #393 5986337117): the template digest is the model directory file's, equal to raw/kw.json's
  canary              baseline: raw/canary.log's first draw, "rate: h/n" (Q5)
  headroom            raw/fill.json: free at peak no less than bar_mib (300) and no request errored; a capped dry run never passes
  checkpoint_restore  n/a (no slot-cache report on this engine; ruling 1)
  output_invariance   n/a (the registry's hazard line: nothing is gated on token identity; ruling 2)
The n/a reasons are the maintainer's rulings and are required verbatim in the reading. admission-recompute.sh is the floor record's, byte for byte."""
from __future__ import annotations
import argparse, hashlib, json, pathlib, re, shutil, sys, tomllib
HERE = pathlib.Path(__file__).resolve().parent; REPO = HERE.parents[2]
FLOOR = REPO / "substrates/admission/accel24-llamacpp-qwen38-27b-iq3s/9d84a552fc94"
BAR_MIB = 300
NA_CHECKPOINT = "n/a (no slot-cache report on this engine: TabbyAPI + ExLlamaV3 reports no per-slot cache state to compare, ruled #393 5986337117)"
NA_INVARIANCE = "n/a (the registry's hazard line: nothing is gated on token identity, ruled #393 5986337117)"
NA_EFFORT = "n/a (a capability fact of the template, recorded as data on the registry entry's effort fields, not a cell word: planning on #143; the template digest and rendering come from the model directory, ruled #393 5986337117)"
HAZARD = "this is part of the hazard of running the fastest engine, and building the knowledge is the long-term benefit"
SHIM = '#!/usr/bin/env bash\n# Gate 0 for a TabbyAPI cells directory (#393): the re-derivation is substrates/admission/depth-probe/tabby_cells.py check.\nset -uo pipefail\nhere="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"\nexec python3 -B "$here/../../depth-probe/tabby_cells.py" check "$here"\n'

def sha(b: bytes) -> str: return hashlib.sha256(b).hexdigest()
def key(name: str) -> str: return name.replace("-", "_").replace(".", "_")
def j(p): return json.loads(p.read_text())
def reg_entry(substrate):
    return tomllib.loads((REPO / "substrates/registry.toml").read_text())["substrate"][substrate]

def derive(d: pathlib.Path) -> dict:
    """word, reading and raw files per cell, from raw/ alone; raises KeyError/FileNotFoundError on a missing file."""
    raw, sub = d / "raw", d.parent.name; reg = reg_entry(sub); out = {}
    ident = j(raw / "identity.json"); model = j(raw / "model.json"); wl = (raw / "weights-sha256.txt").read_text()
    want = reg["weights_main"] if isinstance(reg["weights_main"], list) else [reg["weights_main"]]
    ok = ident["engine_identity"] == reg["engine_identity"] and all(w in wl for w in want)
    out["identity"] = ("pass" if ok else "fail", f"engine identity {ident['engine_identity'][:12]}... recomputed from the live components (tabby_identity.py), the registry's {reg['engine_identity'][:12]}...; /v1/model id {model.get('id')!r}; {len(want)} registered weight digests all in raw/weights-sha256.txt", ["identity.json", "model.json", "weights-sha256.txt"])
    kw = j(raw / "kw.json"); rf = j(raw / "refusal.json")["candidate"]
    levels = [l for l in rf if rf[l]["status"] != 200]; accepted = [l for l in rf if rf[l]["status"] == 200]
    kok = kw["thinking_disabled"].get("reasoning_chars") == 0 and kw["thinking_enabled"].get("reasoning_chars", 0) > 0 and bool(levels) and all(rf[l]["status"] == 400 for l in levels) and bool(accepted)
    out["kwarg_delivery"] = ("pass" if kok else "fail", f"enable_thinking false: {kw['thinking_disabled'].get('reasoning_chars')} reasoning characters; true: {kw['thinking_enabled'].get('reasoning_chars')}; default: {kw['default'].get('reasoning_chars')}; refused levels {sorted(levels)} each HTTP 400, accepted {sorted(accepted)} HTTP 200", ["kw.json", "refusal.json"])
    out["rendered_effort"] = (NA_EFFORT, f"the model directory's template file sha256 is {kw['props_template_sha256']}; rendering per level in raw/kw.json", ["kw.json"])
    m = re.search(r"^rate: (\d+)/(\d+)", (raw / "canary.log").read_text(), re.M)
    out["canary"] = ("baseline", f"{m.group(1)}/{m.group(2)}; the first draw on this line, recorded as its baseline, not a test (Q5)", ["canary.log"])
    fl = j(raw / "fill.json")
    hok = fl.get("passes_bar") is True and not fl.get("capped_dry_run") and fl["vram_free_at_peak_mib"] >= fl["bar_mib"] >= BAR_MIB and all("error" not in r for r in fl["requests"])
    out["headroom"] = ("pass" if hok else "fail", f"{fl['np']} requests of {fl['per_slot_target']} tokens fired concurrently against a {fl['n_ctx']}-token pool; peak {fl['vram_peak_mib']} of {fl['vram_total_mib']} MiB, {fl['vram_free_at_peak_mib']} MiB free against the {fl['bar_mib']} MiB bar", ["fill.json"])
    out["checkpoint_restore"] = (NA_CHECKPOINT, f"no instrument applies; the maintainer's reason: {HAZARD}", [])
    out["output_invariance"] = (NA_INVARIANCE, f"no instrument applies; the maintainer's reason: {HAZARD}", [])
    return out

def toml_str(s: str) -> str: return json.dumps(s, ensure_ascii=False)

def init(d: pathlib.Path) -> int:
    cells = derive(d); lines = ["# Cells for a TabbyAPI line (#393). One table per cell; recompute.sh re-derives every word derivable from a raw file.\n"]
    for name, (word, reading, files) in cells.items():
        lines.append(f"\n[{name}]\nword = {toml_str(word)}\nreading = {toml_str(reading)}\n")
        if files: lines.append("raw = { " + ", ".join(f"{key(f)} = \"{sha((d / 'raw' / f).read_bytes())}\"" for f in files) + " }\n")
    (d / "cells.toml").write_text("".join(lines)); (d / "recompute.sh").write_text(SHIM); (d / "recompute.sh").chmod(0o755)
    shutil.copyfile(FLOOR / "admission-recompute.sh", d / "admission-recompute.sh"); (d / "admission-recompute.sh").chmod(0o755)
    print(f"wrote {len(cells)} cells"); return 0

def check(d: pathlib.Path) -> int:
    bad = []; cells = tomllib.loads((d / "cells.toml").read_text()); got = derive(d)
    fp = j(d / "fingerprint.json")
    if sha(fp["canonical"].encode()) != fp["sha256"] or d.name != fp["sha256"][:12]: bad.append("the directory is not named by its fingerprint's first 12 hex characters")
    for name in ("checkpoint_restore", "kwarg_delivery", "canary"):
        if name not in cells: bad.append(f"cells.toml has no {name} cell")
    for name, (word, _, files) in got.items():
        c = cells.get(name)
        if c is None: bad.append(f"cells.toml has no {name} cell"); continue
        if c["word"] != word: bad.append(f"[{name}] says {c['word']!r}; its raw files give {word!r}")
        for f in files:
            if (c.get("raw") or {}).get(key(f)) != sha((d / "raw" / f).read_bytes()): bad.append(f"[{name}] raw/{f} does not hash as cells.toml says")
    for name in cells:
        if name not in got: bad.append(f"[{name}] is not a cell this instrument derives")
    if cells.get("rendered_effort", {}).get("reading", "").find(j(d / "raw/kw.json")["props_template_sha256"]) < 0: bad.append("[rendered_effort] the reading does not name the template digest")
    for n in ("checkpoint_restore", "output_invariance"):
        if HAZARD not in cells.get(n, {}).get("reading", ""): bad.append(f"[{n}] the reading omits the maintainer's reason")
    for b in bad: print(f"recompute: {b}")
    if not bad: print(f"recompute: {len(cells)} cell(s); every cited raw file hashes as stated and every derivable word re-derives")
    return 1 if bad else 0

if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0]); p.add_argument("mode", choices=["init", "check"]); p.add_argument("dir")
    a = p.parse_args(); sys.exit((init if a.mode == "init" else check)(pathlib.Path(a.dir).resolve()))
