#!/usr/bin/env bash
# Gate 0 for a cells directory (#143). Every raw file cells.toml cites must hash as it says; every
# word derivable from a raw file is re-derived here, and a word that disagrees is refused. Words not
# derivable from a raw file (n/a, unreported, unadjudicated) are checked for form only. A rung's first
# canary draw is its baseline (Q5, planning, #143), re-derived here from the single draw's log.
# Exit 0 when all hold, 1 when any fails, 2 when a step cannot run.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
python3 - "$here" <<'PY' || exit 1
import hashlib, itertools, json, math, pathlib, re, sys, tomllib
here = pathlib.Path(sys.argv[1])
cells = tomllib.loads((here / "cells.toml").read_text())
fp = json.loads((here / "fingerprint.json").read_text())
bad = 0
def fail(m):
    global bad; bad += 1; print(f"recompute: {m}")
if hashlib.sha256(fp["canonical"].encode()).hexdigest() != fp["sha256"] or here.name != fp["sha256"][:12]:
    fail("the directory is not named by its fingerprint's first 12 hex characters")
WORDS = ("pass", "fail", "unreported", "unadjudicated")
raw = {}
for name, c in cells.items():
    w = c.get("word", "")
    if not (w in WORDS or re.fullmatch(r"n/a \(.+\)", w) or (w == "baseline" and name == "canary")): fail(f"[{name}] word {w!r} is not one of the ruled words")
    for key, want in (c.get("raw") or {}).items():
        hits = [p for p in (here / "raw").iterdir() if p.name.replace("-", "_").replace(".", "_") == key]
        if len(hits) != 1: fail(f"[{name}] cites {key}, which names no single raw file"); continue
        got = hashlib.sha256(hits[0].read_bytes()).hexdigest()
        if got != want: fail(f"[{name}] {hits[0].name} hashes to {got[:12]}..., cells.toml says {want[:12]}...")
        raw[key] = hits[0]
DERIVED = set()
def derived(name, word):
    DERIVED.add(name)
    if name in cells and cells[name]["word"] != word: fail(f"[{name}] says {cells[name]['word']!r}; its raw files give {word!r}")
# identity: the running exe the window read
ident = (here / "raw/identity-before.txt").read_text()
want_exe = "prod_exe 980845d60ae7a820f5e2a8b7081727a242b35d3ca8a4021a6fb1240f4a0aa3d4" if (here / "raw/canary-pool.json").exists() else "cand_file 865044a2bf2d56856f843b7cf080faf519030e338d8b75d366270b5b9d2b5511 llama-server"
derived("identity", "pass" if want_exe in ident else "fail")
if "kwarg_delivery" in cells:
    kw = json.loads((here / "raw/kw.json").read_text())
    refused = cells["kwarg_delivery"].get("refused_levels")
    ok_ref = True
    if refused is not None:  # the refused-level row (planning, 2026-09-29): each refused level must return 500
        rf = json.loads((here / "raw/refusal.json").read_text())[cells["kwarg_delivery"]["refusal_rung"]]
        wl = (here / "raw/refusal-window.log").read_text()  # production undisturbed: the floor's pid and VRAM unchanged, health ok
        before = re.search(r"floor before: pid=(\d+) vram=(\d+) MiB", wl); after = re.search(r"floor after: pid=(\d+) vram=(\d+) MiB health=\{\"status\":\"ok\"\}", wl)
        if not (before and after and before.groups() == after.groups()): fail("[kwarg_delivery] raw/refusal-window.log does not show the floor undisturbed")
        ok_ref = all(rf[l]["status"] == 500 for l in refused)
        accepted_err = [l for l, v in rf.items() if l not in refused and v["status"] != 200]
        if accepted_err: fail(f"[kwarg_delivery] levels not declared refused did not return 200: {accepted_err}")
    derived("kwarg_delivery", "pass" if kw["thinking_disabled"]["reasoning_chars"] == 0 and kw["thinking_enabled"]["reasoning_chars"] > 0 and ok_ref else "fail")
    if kw["props_template_sha256"] not in cells["rendered_effort"]["reading"]: fail("[rendered_effort] the reading does not name the served template's digest")
    errs = [k for k, v in kw["rendered_effort"].items() if "error" in v]
    if "refusal_text_sha256" in cells["rendered_effort"]:
        e = kw["rendered_effort"]["reasoning_effort=high"].get("error", "")
        if hashlib.sha256(e.encode()).hexdigest() != cells["rendered_effort"]["refusal_text_sha256"]: fail("[rendered_effort] the refusal text's digest does not match raw/kw.json")
    elif errs: fail(f"[rendered_effort] raw/kw.json records refusals {errs} the reading does not")
# output invariance: first differing token per pair, from the token ids
inv = {p.stem[4:]: [r["tokens"] for r in json.loads(p.read_text())["rows"]] for p in (here / "raw").glob("inv-*.json")}
if inv:
    def fd(x, y):
        for i, (a, b) in enumerate(zip(x, y)):
            if a != b: return i
        return None if len(x) == len(y) else min(len(x), len(y))
    offs = sorted(k for k in inv if k.endswith(("-off", "-off2"))); on = next(k for k in inv if k.endswith("-on"))
    off_pairs = {fd(x, y) for a in offs for b in offs for x in inv[a] for y in inv[b] if x is not y}
    onoff = {fd(x, y) for a in offs for x in inv[on] for y in inv[a]}
    stated = int(re.search(r"on/off pair first differs at token (\d+)", cells["output_invariance"]["reading"]).group(1))
    if off_pairs != {None}: fail(f"[output_invariance] spec-off pairs diverge: {sorted(off_pairs, key=str)}")
    if onoff != {stated}: fail(f"[output_invariance] on/off first differences {sorted(onoff, key=str)}, the reading says {stated}")
    onon = {fd(x, y) for x, y in itertools.combinations(inv[on], 2)}
    m = re.search(r"differs from reps 1-4 .* at token (\d+)", cells["output_invariance"]["reading"])
    if (onon != {None}) != bool(m) or (m and onon != {None, int(m.group(1))}): fail(f"[output_invariance] spec-on self-divergence {sorted(onon, key=str)} is not what the reading states")
    derived("output_invariance", "pass")
# canary: hits over n from each draw's log; the floor's draws judged by #139's test against the committed pool
def draw(p):
    m = re.search(r"^rate: (\d+)/(\d+)", p.read_text(), re.M); return int(m.group(1)), int(m.group(2))
pool = here / "raw/canary-pool.json"
if pool.exists():
    P = json.loads(pool.read_text()); rate, z = P["hits"] / P["k"], P["z"]
    def wilson(h, n):
        ph = h / n; den = 1 + z * z / n; c = (ph + z * z / (2 * n)) / den; hw = z * math.sqrt(ph * (1 - ph) / n + z * z / (4 * n * n)) / den
        return c - hw, c + hw
    words = []
    st = re.search(r"before the stop (\d+)/(\d+), after the restore (\d+)/(\d+)", cells["canary"]["reading"])
    if not st: fail("[canary] the reading does not state the before and after draws")
    for i, f in enumerate(("canary-before.log", "canary-after.log")):
        h, n = draw(here / "raw" / f); lo, hi = wilson(h, n); words.append(lo <= rate <= hi)
        if st and (int(st.group(2 * i + 1)), int(st.group(2 * i + 2))) != (h, n): fail(f"[canary] {f} reads {h}/{n}; the reading states {st.group(2*i+1)}/{st.group(2*i+2)}")
    derived("canary", "pass" if all(words) else "fail")
elif (here / "raw/canary.log").exists():
    h, n = draw(here / "raw/canary.log")
    st = re.match(r"(\d+)/(\d+);", cells["canary"]["reading"])
    if not st or (int(st.group(1)), int(st.group(2))) != (h, n): fail(f"[canary] the draw reads {h}/{n}, which is not what the reading states first")
    derived("canary", "baseline")  # a rung's first draw is its baseline; the second draw is the first test (Q5)
# headroom, where measured here: free at peak against the floor's reading cited in the criterion
if (here / "raw/fill.json").exists():
    fl = json.loads((here / "raw/fill.json").read_text())
    ref = int(re.search(r"([\d,]+) MiB", cells["headroom"]["criterion"]).group(1).replace(",", ""))
    derived("headroom", "pass" if fl["vram_free_at_peak_mib"] >= ref and all("error" not in r for r in fl["requests"]) else "fail")
# pass and fail are results: a cell may carry one only if this script re-derived it from a raw file (review of #185)
for name, c in cells.items():
    if isinstance(c, dict) and c.get("word") in ("pass", "fail") and name not in DERIVED: fail(f"[{name}] says {c['word']!r}, but no raw file re-derives it")
if bad: sys.exit(1)
print(f"recompute: {len(cells)} cell(s); every cited raw file hashes as stated and every derivable word re-derives")
PY
