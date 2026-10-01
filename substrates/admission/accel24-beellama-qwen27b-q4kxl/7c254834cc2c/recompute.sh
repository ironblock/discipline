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
# headroom on the reference line: it passes by construction, and the figure it states is the one the candidate's
# criterion compares against (Dispatch, #143, comment 5889255742)
if cells.get("headroom", {}).get("reference_line"):
    cand = here.parents[1] / "accel24-llamacpp-qwen38-27b-iq3s/9d84a552fc94/cells.toml"
    ref_c = re.search(r"([\d,]+) MiB", tomllib.loads(cand.read_text())["headroom"]["criterion"]) if cand.exists() else None
    own = re.search(r"([\d,]+) MiB", cells["headroom"]["reading"])
    derived("headroom", "pass" if ref_c and own and ref_c.group(1) == own.group(1) else "fail")
# headroom, where measured here: free at peak against the floor's reading cited in the criterion
if (here / "raw/fill.json").exists():
    fl = json.loads((here / "raw/fill.json").read_text())
    ref = int(re.search(r"([\d,]+) MiB", cells["headroom"]["criterion"]).group(1).replace(",", ""))
    derived("headroom", "pass" if fl["vram_free_at_peak_mib"] >= ref and all("error" not in r for r in fl["requests"]) else "fail")
# checkpoint restore (#143, I4b): the committed instrument's decide over the raw measurements must reproduce the
# committed decide output (so the instrument and criterion it ran are pinned by what they gave); the reading's
# figures are that output's; the rung header is the hybrid the reading names; the window log's exes are the
# identity's and this fingerprint's, and production was undisturbed; the measured floor process is the one the
# 09:00Z restore brought back, whose fingerprint check read SUBSTRATE IDENTICAL (#184's depth/raw)
if (here / "raw/checkpoint-rung.json").exists():
    import subprocess
    inst = here.parents[1] / "checkpoint-restore"
    r = subprocess.run([sys.executable, "-B", str(inst / "checkpoint_restore.py"), "decide", str(here / "raw/checkpoint-rung.json"), str(here / "raw/checkpoint-reference.json"),
                        str(here / "raw/checkpoint-identity.json"), str(inst / "criterion.toml")], capture_output=True, text=True)
    committed = json.loads((here / "raw/checkpoint-decide.json").read_text())
    if r.returncode != 0: fail(f"[checkpoint_restore] the instrument's decide exited {r.returncode}")
    elif json.loads(r.stdout) != committed: fail("[checkpoint_restore] the instrument now decides differently from the committed raw/checkpoint-decide.json")
    derived("checkpoint_restore", committed["word"])
    cp = cells.get("checkpoint_restore", {}); rd = cp.get("reading", "")
    unm = json.loads((here / "raw/checkpoint-reference-unmatched.json").read_text())
    sys.path.insert(0, str(inst)); import checkpoint_restore as cr
    ua = cr.reusing(unm); unmatched = max(cr.dist(w["top"], ua["cold"][0]["top"], committed["criterion"]["top_k"]) for w in cr.warms(ua)) if ua else None
    for fig in (f"reused {committed['rung_cache_n']} tokens", f"warm against cold {committed['distance']:.4f}", f"Tolerance {committed['tolerance']:.4f}",
                f"({committed['reference_warm_prompt_n']} tokens against the rung's {committed['rung_warm_prompt_n']})", f"reads {unmatched:.4f}" if unmatched is not None else "unmatched"):
        if fig not in rd: fail(f"[checkpoint_restore] the reading does not state {fig!r}")
    # the reading's other claims, each from the decide output or the raw measurements
    ra = cr.reusing(json.loads((here / "raw/checkpoint-reference.json").read_text())); fa = cr.reusing(json.loads((here / "raw/checkpoint-rung.json").read_text()))
    k = committed["criterion"]["top_k"]; draws = [cr.dist(w["top"], ra["cold"][0]["top"], k) for w in cr.warms(ra)]
    claims = {"; cold against cold 0. ": committed["tolerance_parts"]["cold_cold_rung"] == 0, "the same top token": committed["top1_same"],
              "on the first attempt": committed["rung_attempts"] == 1, "its three draws are identical": len(draws) == 3 and len(set(draws)) == 1,
              "the reference's cold against cold 0": committed["tolerance_parts"]["cold_cold_reference"] == 0}
    for phrase, holds in claims.items():
        if (phrase in rd) != holds: fail(f"[checkpoint_restore] the reading {'states' if phrase in rd else 'omits'} {phrase!r}, and the raw files say {holds}")
    # the reference model: its weights are the bottom rung's registered file, and its server ran CPU-only on that file
    reg = tomllib.loads((here.parents[2] / "registry.toml").read_text())["substrate"]["cpu-beellama-qwen3-1p7b-q4km"]
    rw = re.search(r"reference_weights ([0-9a-f]{64})", (here / "raw/checkpoint-reference-weights.txt").read_text())
    slog = (here / "raw/checkpoint-reference-server.log").read_text()
    if not rw or rw.group(1) != reg["weights_main"]: fail("[checkpoint_restore] the reference's weights are not the bottom rung's registered file")
    if "no CUDA-capable device is detected" not in slog or f"loading model '~/Models/{reg['weights_main_file']}'" not in slog: fail("[checkpoint_restore] the reference server's log does not show it CPU-only on the registered file")
    hdr = json.loads((here / "raw/checkpoint-rung-header.json").read_text())
    hybrid = any(any(m in k for m in cr.RECURRENT) for k in hdr.get("keys", []))
    if ("hybrid" in rd) != hybrid: fail(f"[checkpoint_restore] the reading's hybrid claim does not match the rung header ({hdr.get('architecture')})")
    ident = json.loads((here / "raw/checkpoint-identity.json").read_text())
    if ident["reference_header"] != json.loads((here / "raw/checkpoint-reference-header.json").read_text()): fail("[checkpoint_restore] the identity's reference header is not the one read")
    wl = (here / "raw/checkpoint-window.log").read_text()
    bf = re.search(r"floor pid=(\d+) exe=([0-9a-f]{64}) vram=(\d+) MiB health=\{\"status\":\"ok\"\}", wl)
    af = re.search(r"floor after: pid=(\d+) vram=(\d+) MiB health=\{\"status\":\"ok\"\}", wl)
    rf = re.search(r"reference pid=\d+ exe=([0-9a-f]{64})", wl)
    eng = json.loads(json.loads((here / "fingerprint.json").read_text())["canonical"])["engine"]
    if not (bf and af and rf): fail("[checkpoint_restore] the window log does not record the floor before and after and the reference")
    else:
        if (bf.group(1), bf.group(3)) != af.groups(): fail("[checkpoint_restore] the floor's pid or VRAM changed across the window")
        if not (bf.group(2) == ident["rung_engine"] == eng and rf.group(1) == ident["reference_engine"]): fail("[checkpoint_restore] the window log's exes are not the identity's and this fingerprint's")
        rest = here.parents[1] / "accel24-llamacpp-qwen38-27b-iq3s/9d84a552fc94/depth/raw"
        tie = (rest / "window.log").exists() and re.search(rf"restored: pid={bf.group(1)} exe={eng[:16]} cmdline_same=yes", (rest / "window.log").read_text()) \
              and "SUBSTRATE IDENTICAL" in (rest / "restore-checks.txt").read_text()
        if not tie: fail("[checkpoint_restore] the measured floor process is not the one the recorded restore brought back with its fingerprint identical")
# the GPU-derived tolerance (#143, as ruled: it replaces the CPU-derived one in this cell): the floor's own rung
# measurement against the same dense 1.7B on this rung's binary with the card free, taken in the candidate's parity
# window of 2026-10-01; the instrument's decide over it must reproduce the committed output, and this cell's word is
# that output's. The reference ran on the GPU (its log names the card; the window log reads its VRAM), on the
# registered file, on this fingerprint's engine
if (here / "raw/checkpoint-gpu-decide.json").exists():
    import subprocess
    inst = here.parents[1] / "checkpoint-restore"
    r = subprocess.run([sys.executable, "-B", str(inst / "checkpoint_restore.py"), "decide", str(here / "raw/checkpoint-rung.json"), str(here / "raw/checkpoint-gpu-reference.json"),
                        str(here / "raw/checkpoint-gpu-identity.json"), str(inst / "criterion.toml")], capture_output=True, text=True)
    gd = json.loads((here / "raw/checkpoint-gpu-decide.json").read_text())
    if r.returncode != 0: fail(f"[checkpoint_restore] the instrument's decide over the GPU reference exited {r.returncode}")
    elif json.loads(r.stdout) != gd: fail("[checkpoint_restore] the instrument now decides differently from the committed raw/checkpoint-gpu-decide.json")
    derived("checkpoint_restore", gd["word"])
    rd = cells.get("checkpoint_restore", {}).get("reading", "")
    for fig in (f"GPU-derived tolerance {gd['tolerance']:.4f}", f"warm against cold {gd['distance']:.4f}", f"({gd['reference_warm_prompt_n']} tokens against the rung's {gd['rung_warm_prompt_n']})"):
        if fig not in rd: fail(f"[checkpoint_restore] the reading does not state {fig!r}")
    gl = (here / "raw/checkpoint-gpu-reference-server.log").read_text(); gw = (here / "raw/checkpoint-gpu-window.log").read_text()
    gi = json.loads((here / "raw/checkpoint-gpu-identity.json").read_text())
    regs = tomllib.loads((here.parents[2] / "registry.toml").read_text())["substrate"]
    gu = re.search(r"GPU reference up pid=\d+ exe=([0-9a-f]{64}) vram=(\d+) MiB", gw)
    if "CUDA0" not in gl or "no CUDA-capable device" in gl or not gu or int(gu.group(2)) < 1000: fail("[checkpoint_restore] the GPU reference's logs do not show it on the card")
    elif not (gu.group(1) == gi["reference_engine"] == gi["rung_engine"] == regs["accel24-beellama-qwen27b-q4kxl"]["engine_identity"]): fail("[checkpoint_restore] the GPU reference's exe is not this rung's engine")
    if f"loading model '~/Models/{regs['cpu-beellama-qwen3-1p7b-q4km']['weights_main_file']}'" not in gl: fail("[checkpoint_restore] the GPU reference did not load the bottom rung's registered file")
    gwid = {l.split()[0]: l.split()[1] for l in (here / "raw/checkpoint-gpu-window-identity.txt").read_text().splitlines() if len(l.split()) > 1}
    if gwid.get("seatb_weights") != regs["cpu-beellama-qwen3-1p7b-q4km"]["weights_main"]: fail("[checkpoint_restore] the GPU reference's weights are not the bottom rung's registered file")
    if gi["reference_header"] != json.loads((here / "raw/checkpoint-reference-header.json").read_text()): fail("[checkpoint_restore] the GPU identity's reference header is not the one read")
# pass and fail are results: a cell may carry one only if this script re-derived it from a raw file (review of #185)
for name, c in cells.items():
    if isinstance(c, dict) and c.get("word") in ("pass", "fail") and name not in DERIVED: fail(f"[{name}] says {c['word']!r}, but no raw file re-derives it")
if bad: sys.exit(1)
print(f"recompute: {len(cells)} cell(s); every cited raw file hashes as stated and every derivable word re-derives")
PY
