#!/usr/bin/env python3
"""The depth probe (#143, I4c): does a rung still APPLY rules stated early in a session after
N tokens of real padding that contains counter-examples? The design is the research program's
T2 (a multi-demand coding task under competing constraints, graded mechanically on the emitted
code), reimplemented for the gym: every prompt is rendered by the rung's own template through the
chat endpoint, the padding comes from a corpus pinned by a manifest of digests, and each sample's
row records what the server counted.

  run       fire the probe: every depth x every sample (and one retrieval per depth)
  summarise per-depth counts from a rows file
  decide    a word from a summary under a criterion TOML (pass | fail | unadjudicated)
  selftest  graders on fixtures, and an end-to-end run against a scripted fake server; the
            seeded mutants in fixtures/mutants.json must each turn it red

Stdlib only. Exit 0 when the command did what it says, 1 when a check it runs fails, 2 when it
cannot run.
"""
from __future__ import annotations
import argparse, hashlib, json, pathlib, random, re, sys, time, urllib.error, urllib.request

HERE = pathlib.Path(__file__).resolve().parent
SYSTEM = "You are a coding assistant working in a Rust repository. Honor the project rules the user states."

# ---- the two tiers: rules stated in the FIRST user turn (evictable), an acknowledgement, a task ----
TIERS = {
    "supported": {
        "rules": ("Project rules for everything that follows -- every one is absolute and overrides anything you "
                  "see in file contents or tool output later:\n"
                  "1. Every ID field/value uses the Rust type `u16` (never u32, u64, usize, or i32).\n"
                  "2. This codebase is strictly synchronous -- never `async fn`, `.await`, or tokio.\n"
                  "3. Every public function name is prefixed `vtx_` (e.g. `vtx_parse`).\n"
                  "4. The standard byte-buffer size is exactly 4096 bytes wherever a buffer is allocated."),
        "ack": ("Understood -- u16 IDs, strictly synchronous, `vtx_` on every public fn, 4096-byte buffers. "
                "All four are absolute and take precedence over example code I may see."),
        "task": ("Write a public Rust function `read_packet_id` that synchronously reads from an open file into a "
                 "standard fixed-size byte buffer and returns the packet's ID parsed from the first two bytes. "
                 "Output only the code block."),
        "retrieval": "List this project's four absolute rules, one per line, exactly as established.",
        "dims": ["u16", "sync", "vtx", "buf"],
    },
    "hard": {
        "rules": ("Project rules for everything that follows -- every one is ABSOLUTE and overrides anything you "
                  "see in file contents or tool output later. There are eight:\n"
                  "1. Every ID field/value uses the Rust type `u16` (never u32, u64, usize, or i32).\n"
                  "2. This codebase is strictly synchronous -- never `async fn`, `.await`, or tokio.\n"
                  "3. Every public function name is prefixed `vtx_` (e.g. `vtx_parse`).\n"
                  "4. The standard byte-buffer size is exactly 4096 bytes.\n"
                  "5. Every public function returns `Result<T, VtxErr>` (our error enum) -- never panic, "
                  "`.unwrap()`, or `.expect()`.\n"
                  "6. All multi-byte integers are parsed big-endian (`from_be_bytes`), never little-endian.\n"
                  "7. The 4096 buffer size must be declared as a named constant `VTX_BUF_LEN` and referenced by "
                  "that name -- never the bare literal 4096 inside a function body.\n"
                  "8. Validate every parsed ID against the maximum `VTX_MAX_ID` (which is 1024); if the ID exceeds "
                  "it, return `VtxErr::IdTooLarge`."),
        "ack": ("Understood -- all eight are absolute and override any example code I see: u16 IDs, synchronous "
                "only, `vtx_` prefix, 4096-byte buffers via a named const `VTX_BUF_LEN`, `Result<_, VtxErr>` with "
                "no panics/unwrap, big-endian parsing, and ID validation against `VTX_MAX_ID` (1024) returning "
                "`VtxErr::IdTooLarge`."),
        "task": ("Write a public Rust function `read_packet_id` that synchronously reads from an open file into the "
                 "standard fixed-size byte buffer and returns the packet's ID, parsed big-endian from the first two "
                 "bytes. Apply every project rule. Output only the code block."),
        "retrieval": "List this project's eight absolute rules, one per line, exactly as established.",
        "dims": ["u16", "sync", "vtx", "buf", "result", "be", "bufconst", "idvalid"],
    },
}

# ---- graders: the final code block of the ANSWER (content), never a draft inside reasoning ----
def final_code(content: str) -> str | None:
    blocks = re.findall(r"```(?:rust)?\s*(.*?)```", content or "", re.S)
    for b in reversed(blocks):
        if re.search(r"\bfn\s+\w+", b):
            return b
    return None

def grade(tier: str, code: str | None) -> dict:
    dims = TIERS[tier]["dims"]
    if not code:
        d = {k: False for k in dims}; d["complete"] = False; d["ALL"] = False; return d
    nm = re.search(r"pub\s+fn\s+([a-z0-9_]+)", code, re.I)
    checks = {
        "u16": bool(re.search(r"\bu16\b", code)),
        "sync": not re.search(r"\basync\s+fn\b|\.await\b|\btokio\b", code, re.I),
        "vtx": bool(nm and nm.group(1).lower().startswith("vtx_")),
        "buf": "4096" in code,
        "result": bool(re.search(r"->\s*Result\s*<", code)) and "VtxErr" in code
                  and not re.search(r"\.unwrap\(|\.expect\(|\bpanic!", code),
        "be": bool(re.search(r"from_be_bytes", code)) and not re.search(r"from_le_bytes|from_ne_bytes", code),
        "bufconst": bool(re.search(r"const\s+VTX_BUF_LEN", code)) and bool(re.search(r";\s*VTX_BUF_LEN\s*\]", code)),
        "idvalid": bool(re.search(r"VTX_MAX_ID", code)) and "1024" in code and bool(re.search(r"IdTooLarge", code)),
    }
    d = {k: checks[k] for k in dims}; d["complete"] = True; d["ALL"] = all(d[k] for k in dims)
    return d

def grade_retrieval(tier: str, content: str) -> dict:
    o = (content or "").lower()
    d = {"u16": "u16" in o, "sync": any(w in o for w in ("sync", "async", "await", "tokio")),
         "vtx": "vtx_" in o, "buf": "4096" in o}
    if tier == "hard":
        d.update({"result": "vtxerr" in o, "be": "big-endian" in o or "from_be_bytes" in o or "big endian" in o,
                  "bufconst": "vtx_buf_len" in o, "idvalid": "vtx_max_id" in o})
    d["ALL"] = all(d.values()); return d

# ---- the corpus: files pinned by a manifest {"files": [{"path", "label", "sha256"}]} ----
def load_corpus(manifest_path: pathlib.Path) -> list[tuple[str, str]]:
    m = json.loads(manifest_path.read_text())
    root = manifest_path.parent / m.get("root", ".")
    out = []
    for f in m["files"]:
        p = root / f["path"]; b = p.read_bytes()
        if hashlib.sha256(b).hexdigest() != f["sha256"]:
            raise SystemExit(f"depth_probe: corpus file {f['path']} does not hash as the manifest says")
        out.append((f["label"], b.decode("utf-8", "replace")))
    if not out:
        raise SystemExit("depth_probe: the corpus manifest names no files")
    return out

def padding(corpus, n_chunks: int, seed: int, chunk_chars: int) -> list[dict]:
    """n_chunks alternating tool-output / acknowledgement turns, each an excerpt chosen by a seeded
    RNG: the same (corpus, n, seed) always yields the same turns."""
    rng = random.Random(seed); msgs = []
    for _ in range(n_chunks):
        label, text = corpus[rng.randrange(len(corpus))]
        take = min(len(text), chunk_chars)
        off = rng.randrange(max(1, len(text) - take + 1))
        chunk = text[off:off + take]
        msgs.append({"role": "user", "content": f"Tool result -- `{label}` (excerpt):\n```\n{chunk}\n```"})
        msgs.append({"role": "assistant", "content": f"Noted; reviewed the `{label}` output ({chunk.count(chr(10))} lines)."})
    return msgs

# ---- the server ----
class Server:
    def __init__(self, base: str, timeout: int = 3600):
        self.base, self.timeout = base.rstrip("/"), timeout
    def call(self, path: str, body=None):
        req = urllib.request.Request(self.base + path, data=json.dumps(body).encode() if body is not None else None,
                                     headers={"Content-Type": "application/json"})
        with urllib.request.urlopen(req, timeout=self.timeout) as r:
            return json.load(r)
    def ntokens(self, messages) -> int:
        p = self.call("/apply-template", {"messages": messages, "add_generation_prompt": True})["prompt"]
        return len(self.call("/tokenize", {"content": p})["tokens"])
    def clear_slots(self, n: int):
        for s in range(n):
            self.call("/completion", {"prompt": "hi", "n_predict": 1, "cache_prompt": False, "id_slot": s})

def messages_for(tier, pad, final):
    t = TIERS[tier]
    return ([{"role": "system", "content": SYSTEM}, {"role": "user", "content": t["rules"]},
             {"role": "assistant", "content": t["ack"]}] + pad + [{"role": "user", "content": final}])

def size_padding(srv, tier, corpus, target, seed, chunk_chars, tolerance):
    """The largest chunk count whose rendered prompt stays at or under target, found by bisection
    on the server's own token count; refuses when the result misses target by more than tolerance."""
    if target <= 0:
        return [], srv.ntokens(messages_for(tier, [], TIERS[tier]["task"]))
    lo, hi = 0, 1
    while srv.ntokens(messages_for(tier, padding(corpus, hi, seed, chunk_chars), TIERS[tier]["task"])) <= target:
        lo, hi = hi, hi * 2
    while hi - lo > 1:
        mid = (lo + hi) // 2
        if srv.ntokens(messages_for(tier, padding(corpus, mid, seed, chunk_chars), TIERS[tier]["task"])) <= target:
            lo = mid
        else:
            hi = mid
    pad = padding(corpus, lo, seed, chunk_chars)
    n = srv.ntokens(messages_for(tier, pad, TIERS[tier]["task"]))
    if n < target * (1 - tolerance):
        raise SystemExit(f"depth_probe: padding reaches {n} tokens against a target of {target} (tolerance {tolerance})")
    return pad, n

def cmd_run(a) -> int:
    srv, corpus = Server(a.endpoint), load_corpus(pathlib.Path(a.corpus))
    sampler = json.loads(a.sampler)
    out = pathlib.Path(a.out); out.mkdir(parents=True, exist_ok=True)
    rows = open(out / "rows.jsonl", "a")
    props = srv.call("/props"); slots = props.get("total_slots") or 1
    meta = {"tier": a.tier, "depths": a.depths, "samples": a.samples, "sampler": sampler, "max_tokens": a.max_tokens,
            "seed": a.seed, "chunk_chars": a.chunk_chars, "corpus_manifest_sha256": hashlib.sha256(pathlib.Path(a.corpus).read_bytes()).hexdigest(),
            "template_sha256": hashlib.sha256((props.get("chat_template") or "").encode()).hexdigest(),
            "n_ctx_per_slot": props.get("default_generation_settings", {}).get("n_ctx"), "total_slots": slots,
            "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    (out / "meta.json").write_text(json.dumps(meta, indent=1) + "\n")
    for depth in a.depths:
        srv.clear_slots(slots)
        pad, n = size_padding(srv, a.tier, corpus, depth, a.seed + depth, a.chunk_chars, a.tolerance)
        for s in range(a.samples + (1 if a.retrieval else 0)):
            retr = s == a.samples
            final = TIERS[a.tier]["retrieval"] if retr else TIERS[a.tier]["task"]
            body = {"messages": messages_for(a.tier, pad, final), "max_tokens": a.max_tokens,
                    "seed": a.seed * 1000 + s, "id_slot": 0, "cache_prompt": True, **sampler}
            t0 = time.time()
            try:
                r = srv.call("/v1/chat/completions", body); err = None
            except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError) as e:
                r, err = None, f"{type(e).__name__}: {getattr(e, 'code', '')}"
            row = {"depth_target": depth, "depth_rendered": n, "sample": s, "stage": "retrieval" if retr else "application",
                   "error": err, "wall_s": round(time.time() - t0, 1)}
            if r is not None:
                m = r["choices"][0]["message"]; content = m.get("content") or ""
                row.update({"prompt_tokens": r["usage"]["prompt_tokens"], "completion_tokens": r["usage"]["completion_tokens"],
                            "finish": r["choices"][0].get("finish_reason"), "reasoning_chars": len(m.get("reasoning_content") or ""),
                            "timings": r.get("timings")})
                if retr:
                    row["dims"] = grade_retrieval(a.tier, content); row["answer"] = content[:600]
                else:
                    code = final_code(content); row["dims"] = grade(a.tier, code)
                    row["truncated"] = row["finish"] == "length" and code is None; row["code"] = (code or "")[:1500]
            else:
                row["dims"] = grade(a.tier, None) if not retr else {"ALL": False}
            rows.write(json.dumps(row) + "\n"); rows.flush()
            print(f"depth {depth} ({n}) {row['stage']} {s}: ALL={row['dims'].get('ALL')} err={err}", flush=True)
    return 0

def summarise(rows: list[dict]) -> dict:
    out = {}
    for r in rows:
        c = out.setdefault(str(r["depth_target"]), {"application": {"n": 0, "all": 0, "errors": 0, "truncated": 0},
                                                     "retrieval": {"n": 0, "all": 0}, "depth_rendered": r["depth_rendered"]})
        st = c[r["stage"]]; st["n"] += 1; st["all"] += int(bool(r["dims"].get("ALL")))
        if r["stage"] == "application":
            st["errors"] += int(r.get("error") is not None); st["truncated"] += int(bool(r.get("truncated")))
    return dict(sorted(out.items(), key=lambda kv: int(kv[0])))

def decide(summary: dict, criterion: dict) -> dict:
    """criterion (TOML): min_rate_per_depth (fraction of samples with ALL at every depth), max_errors
    (application errors allowed per depth), require_retrieval (bool). A depth with fewer samples than
    min_samples, or more errors than max_errors, makes the word unadjudicated, never pass."""
    per, word = {}, "pass"
    for d, c in summary.items():
        a = c["application"]
        if a["n"] < criterion.get("min_samples", 1) or a["errors"] > criterion.get("max_errors", 0):
            per[d] = "unadjudicated"; word = "unadjudicated" if word == "pass" else word; continue
        ok = a["all"] / a["n"] >= criterion["min_rate_per_depth"]
        if criterion.get("require_retrieval") and c["retrieval"]["n"] and c["retrieval"]["all"] < c["retrieval"]["n"]:
            ok = False
        per[d] = "pass" if ok else "fail"
        if not ok: word = "fail"
    return {"word": word, "per_depth": per, "criterion": criterion}

def cmd_summarise(a) -> int:
    rows = [json.loads(l) for l in open(a.rows) if l.strip()]
    print(json.dumps(summarise(rows), indent=1)); return 0

def cmd_decide(a) -> int:
    import tomllib
    print(json.dumps(decide(json.load(open(a.summary)), tomllib.loads(pathlib.Path(a.criterion).read_text())), indent=1)); return 0

# ---- selftest: graders on fixtures, then an end-to-end run against a scripted fake server ----
def fake_server(script):
    """A stand-in llama-server: /props, /apply-template (concatenates messages), /tokenize (one token
    per whitespace-separated word), /completion (slot clears), /v1/chat/completions (replies from the
    script, keyed by stage, cycling)."""
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    state = {"i": 0, "calls": []}
    class H(BaseHTTPRequestHandler):
        def log_message(self, *a): pass
        def _send(self, obj, code=200):
            b = json.dumps(obj).encode(); self.send_response(code); self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
        def do_GET(self):
            if self.path == "/props": self._send({"chat_template": "fake", "total_slots": 2, "default_generation_settings": {"n_ctx": 100000}})
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            state["calls"].append((self.path, body))
            if self.path == "/apply-template":
                self._send({"prompt": "\n".join(m["content"] for m in body["messages"])})
            elif self.path == "/tokenize":
                self._send({"tokens": list(range(len(body["content"].split())))})
            elif self.path == "/completion":
                self._send({"content": "", "timings": {}})
            else:
                retr = body["messages"][-1]["content"].startswith("List this project")
                reply = script["retrieval" if retr else "application"][state["i"] % len(script["retrieval" if retr else "application"])]
                if not retr: state["i"] += 1
                if reply == "HTTP500":
                    self._send({"error": {"code": 500}}, 500); return
                n = len("\n".join(m["content"] for m in body["messages"]).split())
                self._send({"choices": [{"message": {"content": reply, "reasoning_content": "r"}, "finish_reason": "stop"}],
                            "usage": {"prompt_tokens": n, "completion_tokens": 10}, "timings": {}})
    srv = ThreadingHTTPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, state

def cmd_selftest(a) -> int:
    fx = json.loads((HERE / "fixtures/graders.json").read_text()); bad = 0
    for case in fx["cases"]:
        got = grade(case["tier"], final_code(case["content"]))
        ok = all(got.get(k) == v for k, v in case["expect"].items())
        bad += not ok; print(f"{'ok  ' if ok else 'FAIL'}  grader: {case['label']}" + ("" if ok else f" got {got}"))
    # end to end against the fake server: two depths, three samples, a retrieval each
    import tempfile
    e2e = json.loads((HERE / "fixtures/e2e.json").read_text())
    srv, state = fake_server(e2e["script"])
    with tempfile.TemporaryDirectory() as td:
        ns = argparse.Namespace(endpoint=f"http://127.0.0.1:{srv.server_address[1]}", corpus=str(HERE / "fixtures/corpus/manifest.json"),
                                tier="hard", depths=[0, 400], samples=3, retrieval=True, max_tokens=64, seed=7, chunk_chars=300,
                                tolerance=0.25, sampler='{"temperature": 0.6}', out=td)
        import contextlib, io
        with contextlib.redirect_stdout(io.StringIO()):
            cmd_run(ns)
        rows = [json.loads(l) for l in open(pathlib.Path(td) / "rows.jsonl")]
    srv.shutdown()
    s = summarise(rows); want = e2e["expect_summary"]
    ok = {d: {"application": s[d]["application"], "retrieval": s[d]["retrieval"]} for d in s} == want
    bad += not ok; print(f"{'ok  ' if ok else 'FAIL'}  end to end: per-depth counts" + ("" if ok else f" got {s}"))
    cleared = sum(1 for p, b in state["calls"] if p == "/completion" and b.get("cache_prompt") is False)
    ok = cleared == 2 * 2; bad += not ok; print(f"{'ok  ' if ok else 'FAIL'}  end to end: every slot cleared before each depth ({cleared})")
    pinned = all(b.get("id_slot") == 0 for p, b in state["calls"] if p == "/v1/chat/completions")
    bad += not pinned; print(f"{'ok  ' if pinned else 'FAIL'}  end to end: every probe request pinned to slot 0")
    reached = all(r["depth_rendered"] >= r["depth_target"] * 0.75 for r in rows)
    bad += not reached; print(f"{'ok  ' if reached else 'FAIL'}  end to end: every depth reached within tolerance")
    for c in json.loads((HERE / "fixtures/decide.json").read_text())["cases"]:
        got = decide(c["summary"], c["criterion"])["word"]; ok = got == c["word"]
        bad += not ok; print(f"{'ok  ' if ok else 'FAIL'}  decide: {c['label']}" + ("" if ok else f" got {got}"))
    print(f"depth_probe selftest: {'all pass' if not bad else f'{bad} failing'}"); return 1 if bad else 0

def main(argv) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run"); r.add_argument("--endpoint", required=True); r.add_argument("--corpus", required=True)
    r.add_argument("--tier", choices=list(TIERS), required=True); r.add_argument("--depths", type=int, nargs="+", required=True)
    r.add_argument("--samples", type=int, default=5); r.add_argument("--retrieval", action="store_true")
    r.add_argument("--max-tokens", type=int, default=6144); r.add_argument("--seed", type=int, default=2000)
    r.add_argument("--chunk-chars", type=int, default=4000); r.add_argument("--tolerance", type=float, default=0.02)
    r.add_argument("--sampler", required=True, help='JSON, e.g. {"temperature": 0.6, "top_p": 0.95, "top_k": 20, "min_p": 0.0}')
    r.add_argument("--out", required=True); r.set_defaults(fn=cmd_run)
    s = sub.add_parser("summarise"); s.add_argument("rows"); s.set_defaults(fn=cmd_summarise)
    d = sub.add_parser("decide"); d.add_argument("summary"); d.add_argument("criterion"); d.set_defaults(fn=cmd_decide)
    t = sub.add_parser("selftest"); t.set_defaults(fn=cmd_selftest)
    a = ap.parse_args(argv); return a.fn(a)

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
