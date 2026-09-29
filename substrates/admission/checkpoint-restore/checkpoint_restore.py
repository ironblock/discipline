#!/usr/bin/env python3
"""Checkpoint restore (#143, I4b; the plan's D12 and Q4 as ruled): when a server reuses a cached prefix --
from its KV cache, or on a hybrid model from a context checkpoint -- is the first token it generates the
same distribution it would generate cold?

  1. Send P + X (primes the slot).
  2. Send P + Y: the server reuses P (warm). Its timings report cache_n, the tokens it reused.
  3. Send P + Y with cache_prompt false, k times (cold).

The cell reads `pass` when the warm call reused (cache_n > 0) and its first token's top-k log-probabilities
agree with the cold call's within a tolerance; `fail` when it reused and they do not. The tolerance comes
from the cold path's own floor and from a known-good restore, never from warm against warm (N3): it is the
largest of cold-against-cold on the rung, cold-against-cold on the reference, and warm-against-cold on the
reference, at a warm continuation matched in length to the rung's (the reference's prime drops lines from P;
the distance a known-good restore shows depends on that length, measured 2026-09-29). The reference is a dense model (its GGUF header has no recurrent keys) on the same engine
binary as the rung, because restore is engine behaviour; without one the cell is `unadjudicated`. An
attempt with no reuse reads `unadjudicated` and is retried (3 attempts, as ruled); the cell is
`unadjudicated` if every attempt is. Generation requests only, one token each: no restart, but each call
occupies a slot and writes the server's prompt cache (N9).

  checkpoint_restore.py measure --endpoint URL --role rung|reference --out FILE [--attempts 3] [--cold 3]
                            [--prime-drops-lines N]   (reference: match the rung's warm continuation length)
  checkpoint_restore.py gguf PATH                   the header's architecture and recurrent keys, as JSON
  checkpoint_restore.py decide RUNG REFERENCE IDENTITY CRITERION
  checkpoint_restore.py selftest                    (mutants.py seeds faults against it)

Distance: over the union of each side's top-k tokens (k from the criterion), the largest absolute
difference in log-probability; a token absent from the other side's n_probs list makes the distance
infinite. Exit codes: measure/gguf 0; decide 0 whatever the word, 2 on malformed input; selftest 0 or 1."""
import argparse, hashlib, itertools, json, math, pathlib, re, struct, sys, urllib.request

HERE = pathlib.Path(__file__).resolve().parent
LINES = 120
P = "".join(f"Line {i}: the quick brown fox jumps over the lazy dog while counting to {i * 7}.\n" for i in range(LINES))
X = "Question: what colour is the fox? Answer:"
Y = "Question: what animal is lazy? Answer:"
SAMPLER = {"temperature": 1.0, "top_k": 0, "top_p": 1.0, "min_p": 0.0}  # the full distribution, unsampled

def sha(b: bytes) -> str: return hashlib.sha256(b).hexdigest()

def call(endpoint: str, prompt: str, cache: bool, slot: int, n_probs: int) -> dict:
    body = {"prompt": prompt, "n_predict": 1, "n_probs": n_probs, "cache_prompt": cache, "id_slot": slot, **SAMPLER}
    r = urllib.request.Request(endpoint.rstrip("/") + "/completion", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    d = json.load(urllib.request.urlopen(r, timeout=900))
    top = d["completion_probabilities"][0]["top_logprobs"]
    return {"cache_n": d["timings"]["cache_n"], "prompt_n": d["timings"]["prompt_n"],
            "top": [[t["id"], t["logprob"]] for t in top]}

def prefix(drop: int) -> str:
    """P with its last `drop` lines removed: priming with it makes the warm call reprocess those lines too, so a
    dense reference's warm continuation can be matched in length to a hybrid rung's, whose reuse stops at a checkpoint."""
    return "".join(P.splitlines(keepends=True)[:LINES - drop])

def measure(endpoint: str, attempts: int, cold: int, slot: int, n_probs: int, drop: int = 0, warm_draws: int = 3) -> dict:
    out = {"prompts": {"P_sha256": sha(P.encode()), "X": X, "Y": Y, "lines": LINES, "prime_drops_lines": drop}, "sampler": SAMPLER, "warm_draws": warm_draws,
           "n_probs": n_probs, "id_slot": slot, "attempts": []}
    for _ in range(attempts):
        prime = call(endpoint, prefix(drop) + X, True, slot, n_probs)
        warm = call(endpoint, P + Y, True, slot, n_probs)
        colds = [call(endpoint, P + Y, False, slot, n_probs) for _ in range(cold)]
        more = []
        if warm["cache_n"] > 0:
            for _ in range(warm_draws - 1):  # re-prime and draw warm again: the cached prefix is recomputed each time
                call(endpoint, prefix(drop) + X, True, slot, n_probs); more.append(call(endpoint, P + Y, True, slot, n_probs))
        out["attempts"].append({"prime": prime, "warm": warm, "warm_more": more, "cold": colds})
        if warm["cache_n"] > 0: break  # a reusing attempt is the measurement; retries are only for no reuse
    return out

def dist(a: list, b: list, k: int) -> float:
    """Largest |logprob difference| over the union of each side's top-k; a token missing from the other side's
    list makes it infinite."""
    A, B = dict(map(tuple, a)), dict(map(tuple, b))
    top = lambda m: sorted(m, key=m.get, reverse=True)[:k]
    return max((abs(A[t] - B[t]) if t in A and t in B else math.inf) for t in set(top(A)) | set(top(B)))

def reusing(m: dict):
    return next((a for a in m["attempts"] if a["warm"]["cache_n"] > 0), None)

def warms(att: dict) -> list:
    """Every warm draw of an attempt, each of which must have reused."""
    return [att["warm"]] + att.get("warm_more", [])

RECURRENT = (".ssm.", "ssm_", "recurrent", "full_attention_interval", ".wkv.", "rwkv", "time_mix", ".shortconv.", "mamba")
HEX64 = re.compile(r"[0-9a-f]{64}")

def cold_floor(att: dict, k: int) -> float:
    return max((dist(x["top"], y["top"], k) for x, y in itertools.combinations(att["cold"], 2)), default=0.0)

def decide(rung: dict, ref: dict, ident: dict, crit: dict) -> dict:
    k, need = crit["top_k"], crit["attempts"]
    res = {"criterion": crit}
    hdr = ident.get("reference_header") or {}
    recurrent = [key for key in hdr.get("keys", []) if any(m in key for m in RECURRENT)] + ([hdr["architecture"]] if any(m in str(hdr.get("architecture", "")) for m in ("rwkv", "mamba", "lfm")) else [])
    if not (HEX64.fullmatch(str(ident.get("rung_engine", ""))) and HEX64.fullmatch(str(ident.get("reference_engine", "")))):
        return {**res, "word": "unadjudicated", "reason": "an engine binary's digest is missing: the same-engine requirement is unverified"}
    if ident.get("reference_engine") != ident.get("rung_engine"):
        return {**res, "word": "unadjudicated", "reason": "the reference does not run on the rung's engine binary"}
    if not hdr.get("architecture") or recurrent:
        return {**res, "word": "unadjudicated", "reason": f"no dense reference on the rung's engine (recurrent keys {recurrent})" if recurrent else "the reference's GGUF header was not read"}
    if len(rung["attempts"]) > need or len(ref["attempts"]) > need:
        return {**res, "word": "unadjudicated", "reason": f"more than the declared {need} attempts"}
    # the two measurements must be of one procedure: prompt, sampler, a list deep enough to find every compared token, roles
    if rung.get("prompts", {}).get("P_sha256") != ref.get("prompts", {}).get("P_sha256") or rung.get("sampler") != ref.get("sampler") \
       or min(rung.get("n_probs", 0), ref.get("n_probs", 0)) < 2 * k or (rung.get("role"), ref.get("role")) != ("rung", "reference"):
        return {**res, "word": "unadjudicated", "reason": "the rung's and reference's measurements are not of one procedure (prompt, sampler, n_probs of at least twice top_k, roles)"}
    ra, fa = reusing(ref), reusing(rung)
    if ra is None:
        return {**res, "word": "unadjudicated", "reason": f"the reference reused nothing in {len(ref['attempts'])} attempt(s)"}
    if fa is None and len(rung["attempts"]) < need:
        return {**res, "word": "unadjudicated", "reason": f"the rung reused nothing, and stopped at {len(rung['attempts'])} of the declared {need} attempts"}
    if len(ra["cold"]) < 2 or (fa and len(fa["cold"]) < 2):
        return {**res, "word": "unadjudicated", "reason": "fewer than two cold calls: the cold path's floor is unmeasured"}
    if any(w["cache_n"] == 0 for w in warms(ra)) or (fa and any(w["cache_n"] == 0 for w in warms(fa))):
        return {**res, "word": "unadjudicated", "reason": "a repeated warm draw reused nothing"}
    tol = {"cold_cold_reference": cold_floor(ra, k), "warm_cold_reference": max(dist(w["top"], ra["cold"][0]["top"], k) for w in warms(ra))}
    if fa is None:
        return {**res, "word": "unadjudicated", "reason": f"the rung reused nothing in {len(rung['attempts'])} attempt(s)", "tolerance_parts": tol}
    # the tolerance is only a bound for a warm continuation of the rung's length: the reference's must match (N3)
    rn, fn = ra["warm"]["prompt_n"], fa["warm"]["prompt_n"]
    if abs(rn - fn) > crit["shape_match"] * fn:
        return {**res, "word": "unadjudicated", "reason": f"the reference's warm continuation ({rn} tokens) does not match the rung's ({fn}) within {crit['shape_match']:.0%}",
                "tolerance_parts": tol}
    tol["cold_cold_rung"] = cold_floor(fa, k)
    tau = max(tol.values())
    if not math.isfinite(tau):  # a token-set wobble on a bounding path bounds nothing: never a pass (review of #186)
        return {**res, "word": "unadjudicated", "reason": "the tolerance is infinite: a compared token is missing from a bounding call's list", "tolerance_parts": tol}
    ds = [dist(w["top"], fa["cold"][0]["top"], k) for w in warms(fa)]; d = max(ds)
    return {**res, "word": "pass" if d <= tau else "fail", "distance": d, "distances": ds, "tolerance": tau, "tolerance_parts": tol,
            "rung_cache_n": fa["warm"]["cache_n"], "reference_cache_n": ra["warm"]["cache_n"],
            "rung_warm_prompt_n": fn, "reference_warm_prompt_n": rn,
            "rung_attempts": len(rung["attempts"]), "reference_attempts": len(ref["attempts"]),
            "top1_same": fa["warm"]["top"][0][0] == fa["cold"][0]["top"][0][0]}

def gguf(path: str) -> dict:
    F = {0: "<B", 1: "<b", 2: "<H", 3: "<h", 4: "<I", 5: "<i", 6: "<f", 7: "<?", 10: "<Q", 11: "<q", 12: "<d"}
    f = open(path, "rb")
    if f.read(4) != b"GGUF": raise SystemExit("checkpoint_restore: not a GGUF file")
    f.read(4); _, nkv = struct.unpack("<QQ", f.read(16))
    def rs():
        n, = struct.unpack("<Q", f.read(8)); return f.read(n).decode("utf-8", "replace")
    def val(t):
        if t in F: return struct.unpack(F[t], f.read(struct.calcsize(F[t])))[0]
        if t == 8: return rs()
        at, n = struct.unpack("<IQ", f.read(12))
        if at in F: f.seek(n * struct.calcsize(F[at]), 1); return None
        for _ in range(n): val(at)
        return None
    keys, arch = [], None
    for _ in range(nkv):
        key = rs(); t, = struct.unpack("<I", f.read(4)); v = val(t); keys.append(key)
        if key == "general.architecture": arch = v
    return {"architecture": arch, "keys": sorted(k for k in keys if not k.startswith("tokenizer."))}

# ---- selftest ----
def fake_server(script):
    """A stand-in /completion: replies are scripted per call, in order, as {cache_n, top} for warm (cache_prompt
    true on P + Y) and cold calls; the prime is answered with cache_n 0."""
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    state = {"calls": [], "warm": 0, "cold": 0}
    class H(BaseHTTPRequestHandler):
        def log_message(self, *a): pass
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"]))); state["calls"].append(body)
            if body["prompt"].endswith(X): rep = {"cache_n": 0, "top": script["cold"][0]}; state.setdefault("primes", []).append(body["prompt"])
            elif body["cache_prompt"]: rep = script["warm"][min(state["warm"], len(script["warm"]) - 1)]; state["warm"] += 1  # the last reply repeats
            else: rep = {"cache_n": 0, "top": script["cold"][state["cold"] % len(script["cold"])]}; state["cold"] += 1
            d = {"completion_probabilities": [{"top_logprobs": [{"id": i, "logprob": lp} for i, lp in rep["top"]]}],
                 "timings": {"cache_n": rep["cache_n"], "prompt_n": rep.get("prompt_n", 100 - rep["cache_n"])}}
            b = json.dumps(d).encode(); self.send_response(200); self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
    srv = ThreadingHTTPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, state

def cmd_selftest(a) -> int:
    import tomllib
    fx = json.loads((HERE / "fixtures/cases.json").read_text()); crit = tomllib.loads((HERE / "criterion.toml").read_text()); bad = 0
    def check(ok, label, extra=""):
        nonlocal bad; bad += not ok; print(f"{'ok  ' if ok else 'FAIL'}  {label}" + ("" if ok else f" {extra}"))
    for case in fx["measure"]:  # the procedure against a scripted server: requests, retries, and the word
        srv, st = fake_server(case["script"]); url = f"http://127.0.0.1:{srv.server_address[1]}"
        rung = {"role": "rung", **measure(url, crit["attempts"], 3, 0, 20)}; srv.shutdown()
        srv, _ = fake_server(fx["reference_script"]); ref = {"role": "reference", **measure(f"http://127.0.0.1:{srv.server_address[1]}", crit["attempts"], 3, 0, 20)}; srv.shutdown()
        got = decide(rung, ref, fx["identity"], crit)
        check(got["word"] == case["word"], f"measure+decide: {case['label']}", f"got {got['word']} ({got.get('reason', got.get('distance'))})")
        check(len(rung["attempts"]) == case["attempts"], f"measure: {case['label']} took {case['attempts']} attempt(s)", f"took {len(rung['attempts'])}")
        check(all(c["n_predict"] == 1 and c["id_slot"] == 0 for c in st["calls"]), f"measure: {case['label']}: one token per request, pinned to the slot")
        colds = [c for c in st["calls"] if not c["cache_prompt"]]
        check(all(c["prompt"] == P + Y for c in colds) and len(colds) == 3 * case["attempts"], f"measure: {case['label']}: every cold call is P + Y uncached")
    srv, st = fake_server(fx["reference_script"]); measure(f"http://127.0.0.1:{srv.server_address[1]}", 1, 2, 0, 20, drop=23); srv.shutdown()
    check(st.get("primes") == [prefix(23) + X] * 3 and prefix(23).count("\n") == LINES - 23 and P.startswith(prefix(23)),
          "measure: a reference primed short of 23 lines, so its warm call reprocesses them")
    for case in fx["decide"]:
        got = decide(case["rung"], case["reference"], case.get("identity", fx["identity"]), crit)
        check(got["word"] == case["word"] and case.get("reason_has", "") in got.get("reason", ""), f"decide: {case['label']}", f"got {got['word']} ({got.get('reason', got.get('distance'))})")
    check(dist([[1, -0.1], [2, -2.0]], [[1, -0.1], [3, -2.0]], 2) == math.inf, "distance: a top token missing from the other side is infinite")
    check(abs(dist([[1, -0.5], [2, -1.0]], [[1, -0.25], [2, -1.0]], 2) - 0.25) < 1e-12, "distance: the largest log-probability difference")
    print(f"checkpoint_restore selftest: {'all pass' if not bad else f'{bad} failing'}"); return 1 if bad else 0

def main(argv) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    m = sub.add_parser("measure"); m.add_argument("--endpoint", required=True); m.add_argument("--role", choices=["rung", "reference"], required=True)
    m.add_argument("--out", required=True); m.add_argument("--attempts", type=int, default=3); m.add_argument("--cold", type=int, default=3)
    m.add_argument("--id-slot", type=int, default=0); m.add_argument("--n-probs", type=int, default=20)
    m.add_argument("--warm-draws", type=int, default=3, help="warm draws per reusing attempt, each after a fresh prime")
    m.add_argument("--prime-drops-lines", type=int, default=0, help="reference only: prime with P short of this many lines, to match the rung's warm continuation")
    g = sub.add_parser("gguf"); g.add_argument("path")
    d = sub.add_parser("decide"); [d.add_argument(x) for x in ("rung", "reference", "identity", "criterion")]
    sub.add_parser("selftest")
    a = ap.parse_args(argv)
    if a.cmd == "measure":
        r = {"role": a.role, **measure(a.endpoint, a.attempts, a.cold, a.id_slot, a.n_probs, a.prime_drops_lines, a.warm_draws)}
        pathlib.Path(a.out).write_text(json.dumps(r, indent=1) + "\n"); return 0
    if a.cmd == "gguf":
        print(json.dumps(gguf(a.path), indent=1)); return 0
    if a.cmd == "decide":
        import tomllib
        try:
            rd = [json.loads(pathlib.Path(p).read_text()) for p in (a.rung, a.reference, a.identity)]
            crit = tomllib.loads(pathlib.Path(a.criterion).read_text())
        except (OSError, ValueError) as e:
            print(f"checkpoint_restore: {e}", file=sys.stderr); return 2
        print(json.dumps(decide(*rd, crit), indent=1)); return 0
    return cmd_selftest(a)

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
