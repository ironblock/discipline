#!/usr/bin/env python3
"""The depth probe (#143, I4c), as ruled by planning on #143 (2026-09-29): does a rung still APPLY
rules stated in a session's first user turn after padding that fills a fraction of its declared
serving context? The design is the research program's T2 (a multi-demand coding task under competing
constraints, graded mechanically on the emitted code), reimplemented for the gym.

  - Corpus: this repository's source plus tokio, each at a pinned commit (corpus/manifest.json, written
    by make_corpus.py): real file reads, directory listings and diffs, each by digest; plus one generated counter-example
    per constraint (counterexamples.json, written by counterexamples.py), planted at declared depths.
  - Ladder: fractions of the rung's declared serving context (0.5, 0.9, 0.95) and the zero-pad control.
  - Five samples per cell, in the rung's supported coding configuration, thinking on (checked per sample).
  - Word: `pass` ("no cliff") when every cell's pass count is within one sample of the control's;
    `fail` when any cell falls two or more below it; `unadjudicated` when a cell has a server error,
    too few samples, or a kept sample with thinking not engaged. A sample whose reply shows thinking did not
    engage is invalid: discarded, recorded, and re-drawn within criterion.toml's `max_redraws` per cell; two or
    more in a cell is a kwarg-delivery finding beside the word (planning, #143, comment 5921525110).
  - The four-constraint set (`supported`) is the admission probe; the eight-constraint set (`hard`)
    is a second probe admission does not require.

  run | summarise | decide | selftest      (mutants.py seeds faults against the selftest)

Stdlib only. Exit 0 when the command did what it says, 1 when a check it runs fails, 2 when it cannot run.
"""
from __future__ import annotations
import argparse, hashlib, json, pathlib, random, re, subprocess, sys, time, tomllib, urllib.error, urllib.request

HERE = pathlib.Path(__file__).resolve().parent
SYSTEM = "You are a coding assistant working in a Rust repository. Honor the project rules the user states."

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
        "admission": True,
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
        "retrieval": None,
        "dims": ["u16", "sync", "vtx", "buf", "result", "be", "bufconst", "idvalid"],
        "admission": False,
    },
}

# ---- the grader: one deterministic check per constraint on the ANSWER's final code block ----
def final_code(content: str) -> str | None:
    blocks = re.findall(r"```(?:rust)?\s*(.*?)```", content or "", re.S)
    for b in reversed(blocks):
        if re.search(r"\bfn\s+\w+", b):
            return b
    return None

CHECKS = {
    "u16": lambda c, nm: bool(re.search(r"\bu16\b", c)),
    "sync": lambda c, nm: not re.search(r"\basync\s+fn\b|\.await\b|\btokio\b", c, re.I),
    "vtx": lambda c, nm: bool(nm and nm.group(1).lower().startswith("vtx_")),
    "buf": lambda c, nm: "4096" in c,
    "result": lambda c, nm: bool(re.search(r"->\s*Result\s*<", c)) and "VtxErr" in c and not re.search(r"\.unwrap\(|\.expect\(|\bpanic!", c),
    "be": lambda c, nm: bool(re.search(r"from_be_bytes", c)) and not re.search(r"from_le_bytes|from_ne_bytes", c),
    "bufconst": lambda c, nm: bool(re.search(r"const\s+VTX_BUF_LEN", c)) and bool(re.search(r";\s*VTX_BUF_LEN\s*\]", c)),
    "idvalid": lambda c, nm: bool(re.search(r"VTX_MAX_ID", c)) and "1024" in c and bool(re.search(r"IdTooLarge", c)),
}

def grade(tier: str, code: str | None) -> dict:
    dims = TIERS[tier]["dims"]
    if not code:
        d = {k: False for k in dims}; d["complete"] = False; d["ALL"] = False; return d
    nm = re.search(r"pub\s+fn\s+([a-z0-9_]+)", code, re.I)
    d = {k: CHECKS[k](code, nm) for k in dims}; d["complete"] = True; d["ALL"] = all(d[k] for k in dims)
    return d

def grade_retrieval(content: str) -> dict:
    o = (content or "").lower()
    d = {"u16": "u16" in o, "sync": any(w in o for w in ("sync", "async", "await", "tokio")), "vtx": "vtx_" in o, "buf": "4096" in o}
    d["ALL"] = all(d.values()); return d

# ---- the corpus ----
def sha(b: bytes) -> str: return hashlib.sha256(b).hexdigest()

def load_corpus(manifest_path: pathlib.Path, repos: dict | None = None) -> dict:
    """{"reads": [(label, text)], "listings": [...], "diffs": [...]}, every text checked against the
    manifest's digest. source = git (named repositories at pinned commits; `self` is this repository,
    others are mapped to local clones by `repos`) or files (the selftest's fixture corpus)."""
    m = json.loads(manifest_path.read_text()); repos = dict(repos or {})
    repos.setdefault("self", str(HERE.parents[2]))
    out = {"reads": [], "listings": [], "diffs": []}
    if m["source"] == "files":
        for kind in out:
            for e in m.get(kind, []):
                b = (manifest_path.parent / e["file"]).read_bytes()
                if sha(b) != e["sha256"]:
                    raise SystemExit(f"depth_probe: corpus {kind} entry {e['file']} does not hash as the manifest says")
                out[kind].append((e["label"], b.decode("utf-8", "replace")))
    else:
        for src in m["sources"]:
            if src["name"] not in repos:
                raise SystemExit(f"depth_probe: no local clone given for source {src['name']} ({src.get('url')}); pass --repo {src['name']}=PATH")
            def git(*args, _r=repos[src["name"]]):
                return subprocess.run(["git", "-C", _r, *args], capture_output=True, check=True).stdout
            for kind in out:
                for e in src.get(kind, []):
                    b = {"reads": lambda: git("show", f"{src['commit']}:{e['path']}"),
                         "listings": lambda: git("ls-tree", "--name-only", src["commit"], e["path"] + "/"),
                         "diffs": lambda: git("diff", e["commit"] + "^1", e["commit"], "--", e["path"])}[kind]()
                    if sha(b) != e["sha256"]:
                        raise SystemExit(f"depth_probe: {src['name']} {kind} entry {e['path']} does not hash as the manifest says")
                    out[kind].append((e["label"], b.decode("utf-8", "replace")))
    if not out["reads"]:
        raise SystemExit("depth_probe: the corpus manifest names no file reads")
    return out

def load_counterexamples(path: pathlib.Path, tier: str, want_sha: str | None) -> list[dict]:
    b = path.read_bytes()
    if not want_sha:
        raise SystemExit("depth_probe: the corpus manifest pins no counterexamples_sha256")
    if sha(b) != want_sha:
        raise SystemExit("depth_probe: counterexamples.json does not hash as the manifest says")
    ce = json.loads(b)["tiers"][tier]
    if sorted(c["constraint"] for c in ce) != sorted(TIERS[tier]["dims"]):
        raise SystemExit(f"depth_probe: counterexamples for {tier} do not cover exactly its constraints")
    return ce

def padding(corpus: dict, counterexamples: list[dict], n_chunks: int, seed: int, chunk_chars: int) -> list[dict]:
    """n_chunks tool-result / acknowledgement turn pairs chosen by a seeded RNG (file reads 70%, listings
    15%, diffs 15%), with every counter-example planted as a file-read turn at its declared fraction of
    the padding. The same inputs always yield the same turns; no padding, no counter-examples."""
    if n_chunks <= 0:
        return []
    rng = random.Random(seed); turns = []
    kinds = [k for k, w in (("reads", 70), ("listings", 15), ("diffs", 15)) for _ in range(w) if corpus[k]]
    for _ in range(n_chunks):
        kind = kinds[rng.randrange(len(kinds))]
        label, text = corpus[kind][rng.randrange(len(corpus[kind]))]
        take = min(len(text), chunk_chars); off = rng.randrange(max(1, len(text) - take + 1))
        turns.append((label, text[off:off + take]))
    for c in sorted(counterexamples, key=lambda c: c["plant_at"], reverse=True):
        turns.insert(min(len(turns), round(c["plant_at"] * len(turns))), (c["label"], c["text"]))
    msgs = []
    for label, chunk in turns:
        msgs.append({"role": "user", "content": f"Tool result -- `{label}`:\n```\n{chunk}\n```"})
        msgs.append({"role": "assistant", "content": f"Noted; reviewed the `{label}` output ({chunk.count(chr(10))} lines)."})
    return msgs

# ---- the server ----
class Server:
    def __init__(self, base: str, timeout: int = 7200):
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
        """One tiny uncached request pinned to each slot: each replaces that slot's cells, so another
        slot's resident prompt no longer counts against a unified cache (measured on the floor,
        evidence/floor-prefill-2026-09-29/)."""
        for s in range(n):
            self.call("/completion", {"prompt": "hi", "n_predict": 1, "cache_prompt": False, "id_slot": s})

def messages_for(tier, pad, final):
    t = TIERS[tier]
    return ([{"role": "system", "content": SYSTEM}, {"role": "user", "content": t["rules"]},
             {"role": "assistant", "content": t["ack"]}] + pad + [{"role": "user", "content": final}])

def size_padding(srv, tier, corpus, ces, target, seed, chunk_chars, tolerance):
    """The largest chunk count whose rendered prompt stays at or under target, by bisection on the
    server's own token count; refused when it misses target by more than the tolerance."""
    if target <= 0:
        return [], srv.ntokens(messages_for(tier, [], TIERS[tier]["task"]))
    at = lambda k: srv.ntokens(messages_for(tier, padding(corpus, ces, k, seed, chunk_chars), TIERS[tier]["task"]))
    lo, hi = 0, 1
    while at(hi) <= target:
        lo, hi = hi, hi * 2
    while hi - lo > 1:
        mid = (lo + hi) // 2
        lo, hi = (mid, hi) if at(mid) <= target else (lo, mid)
    pad = padding(corpus, ces, lo, seed, chunk_chars); n = srv.ntokens(messages_for(tier, pad, TIERS[tier]["task"]))
    if n < target * (1 - tolerance):
        raise SystemExit(f"depth_probe: padding reaches {n} tokens against a target of {target} (tolerance {tolerance})")
    return pad, n

def cmd_run(a) -> int:
    srv, mpath = Server(a.endpoint), pathlib.Path(a.corpus)
    manifest = json.loads(mpath.read_text()); corpus = load_corpus(mpath, dict(r.split("=", 1) for r in a.repo))
    ces = load_counterexamples(mpath.parent / manifest["counterexamples"], a.tier, manifest.get("counterexamples_sha256"))
    sampler = json.loads(a.sampler)
    out = pathlib.Path(a.out); out.mkdir(parents=True, exist_ok=True)
    props = srv.call("/props"); slots = props.get("total_slots") or 1; nctx = props.get("default_generation_settings", {}).get("n_ctx")
    cells = [(f, round(f * a.serving_context)) for f in a.fractions]
    if nctx and max(t for _, t in cells) + a.max_tokens > nctx:
        raise SystemExit(f"depth_probe: the deepest cell plus max_tokens exceeds the server's per-slot context {nctx}")
    meta = {"tier": a.tier, "admission": TIERS[a.tier]["admission"], "serving_context": a.serving_context, "cells": cells,
            "samples": a.samples, "sampler": sampler, "max_tokens": a.max_tokens, "seed": a.seed, "chunk_chars": a.chunk_chars,
            "instrument_sha256": sha(pathlib.Path(__file__).read_bytes()), "criterion_sha256": sha((HERE / "criterion.toml").read_bytes()),
            "corpus_manifest_sha256": sha(mpath.read_bytes()), "template_sha256": sha((props.get("chat_template") or "").encode()),
            "n_ctx_per_slot": nctx, "total_slots": slots, "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    (out / "meta.json").write_text(json.dumps(meta, indent=1) + "\n")
    rows = open(out / "rows.jsonl", "a")
    for frac, target in cells:
        srv.clear_slots(slots)
        pad, n = size_padding(srv, a.tier, corpus, ces, target, a.seed + target, a.chunk_chars, a.tolerance)
        planted = sum(1 for c in ces if any(c["text"] in m["content"] for m in pad))
        stages = [("application", s) for s in range(a.samples)] + ([("retrieval", 0)] if a.retrieval and TIERS[a.tier]["retrieval"] else [])
        budget = int(tomllib.loads((HERE / "criterion.toml").read_text()).get("max_redraws", 0)); redraws = 0
        queue = list(stages)
        while queue:
            stage, s = queue.pop(0); attempt = 0
            while True:
                row = draw(srv, a, stage, s, attempt, frac, target, n, planted, pad)
                # planning (#143, comment 5921525110): a thinking-on application sample whose reply shows thinking did
                # not engage is invalid -- discarded, recorded, and re-drawn within the criterion's declared budget
                invalid = stage == "application" and row.get("error") is None and not row.get("reasoning_chars")
                if invalid:
                    row.update({"invalid": "thinking not engaged", "discarded": redraws < budget})
                rows.write(json.dumps(row) + "\n"); rows.flush()
                print(f"cell {frac} ({n} tok, {planted} planted) {stage} {s}: ALL={row['dims'].get('ALL')} err={row.get('error')}"
                      + (f" INVALID ({'re-drawn' if row['discarded'] else 'budget spent, kept'})" if invalid else ""), flush=True)
                if invalid and row["discarded"]:
                    redraws += 1; attempt += 1; continue
                break
    return 0

def draw(srv, a, stage, s, attempt, frac, target, n, planted, pad) -> dict:
    """One sample: its request, its reply, its row. A re-draw changes only the seed."""
    if True:
            final = TIERS[a.tier]["task"] if stage == "application" else TIERS[a.tier]["retrieval"]
            body = {"messages": messages_for(a.tier, pad, final), "max_tokens": a.max_tokens, "seed": a.seed * 1000 + s + 100 * attempt,
                    "id_slot": 0, "cache_prompt": True, **json.loads(a.sampler)}
            t0 = time.time(); row = {"fraction": frac, "depth_target": target, "depth_rendered": n, "planted": planted,
                                     "stage": stage, "sample": s}
            if attempt: row["redraw"] = attempt
            try:
                r = srv.call("/v1/chat/completions", body); err = None
            except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError) as e:
                r, err = None, f"{type(e).__name__} {getattr(e, 'code', '')}".strip()
            row.update({"error": err, "wall_s": round(time.time() - t0, 1)})
            if r is not None:
                m = r["choices"][0]["message"]; content = m.get("content") or ""
                row.update({"prompt_tokens": r["usage"]["prompt_tokens"], "completion_tokens": r["usage"]["completion_tokens"],
                            "finish": r["choices"][0].get("finish_reason"), "reasoning_chars": len(m.get("reasoning_content") or ""),
                            "timings": r.get("timings")})
                if stage == "retrieval":
                    row["dims"] = grade_retrieval(content); row["answer"] = content[:600]
                else:
                    code = final_code(content); row["dims"] = grade(a.tier, code)
                    row["truncated"] = row["finish"] == "length" and code is None; row["code"] = (code or "")[:1500]
            else:
                row["dims"] = {"ALL": False}
            return row

def summarise(rows: list[dict]) -> dict:
    out = {}
    for r in rows:
        c = out.setdefault(str(r["fraction"]), {"depth_target": r["depth_target"], "depth_rendered": r["depth_rendered"], "planted": r["planted"],
                                                "application": {"n": 0, "pass": 0, "errors": 0, "thinking_off": 0, "truncated": 0},
                                                "retrieval": {"n": 0, "pass": 0}})
        if r.get("invalid"):
            c["application"]["invalid"] = c["application"].get("invalid", 0) + 1
        if r.get("discarded"):
            continue
        st = c[r["stage"]]; st["n"] += 1; st["pass"] += int(bool(r["dims"].get("ALL")))
        if r["stage"] == "application":
            st["errors"] += int(r.get("error") is not None); st["truncated"] += int(bool(r.get("truncated")))
            st["thinking_off"] += int(r.get("error") is None and not r.get("reasoning_chars"))
    return dict(sorted(out.items(), key=lambda kv: float(kv[0])))

def decide(summary: dict, criterion: dict) -> dict:
    """The ruled word. criterion: within (1), min_samples (5), max_errors (0). The control is the
    fraction-0 cell. A cell with an error, a thinking-off sample, or too few samples is unadjudicated;
    a cell whose pass count falls more than `within` below the control's is a cliff and fails; a
    fail outranks an unadjudicated cell; the control itself must be adjudicable."""
    within, mins, maxe = criterion.get("within", 1), criterion.get("min_samples", 5), criterion.get("max_errors", 0)
    if "0" not in summary and "0.0" not in summary:
        return {"word": "unadjudicated", "why": "no zero-pad control cell"}
    ctrl = summary.get("0", summary.get("0.0"))
    def adjudicable(c):
        a = c["application"]; return a["n"] >= mins and a["errors"] <= maxe and a["thinking_off"] == 0
    per, word = {}, "pass"
    if not adjudicable(ctrl):
        return {"word": "unadjudicated", "why": "the control cell is not adjudicable", "per_cell": {}}
    for f, c in summary.items():
        if c is ctrl: per[f] = "control"; continue
        if not adjudicable(c):
            per[f] = "unadjudicated"; word = "unadjudicated" if word == "pass" else word; continue
        cliff = c["application"]["pass"] < ctrl["application"]["pass"] - within
        per[f] = "fail" if cliff else "pass"
        if cliff: word = "fail"
    strict = all(c["application"]["n"] >= mins and c["application"]["pass"] == c["application"]["n"] and adjudicable(c) for c in summary.values())
    out = {"word": word, "per_cell": per, "control_pass": ctrl["application"]["pass"],
           "criterion": {k: criterion[k] for k in ("within", "min_samples", "max_errors") if k in criterion},
           "strict_beside": "pass" if strict else "not met", "strict_is": "every sample at every cell passed: reported beside the word, never the word (planning, #143)"}
    flagged = {f: c["application"]["invalid"] for f, c in summary.items() if c["application"].get("invalid", 0) >= 2}
    if flagged:  # planning (#143, comment 5921525110): two or more invalid samples in a cell is a kwarg-delivery finding
        out["kwarg_delivery_findings"] = {"cells": flagged, "is": "two or more samples in a cell came back with thinking not engaged though the request asked for it: a finding about how the rung delivers the thinking kwarg, noted on the cell, not the word"}
    return out

def cmd_summarise(a) -> int:
    print(json.dumps(summarise([json.loads(l) for l in open(a.rows) if l.strip()]), indent=1)); return 0

def cmd_decide(a) -> int:
    import tomllib
    print(json.dumps(decide(json.load(open(a.summary)), tomllib.loads(pathlib.Path(a.criterion).read_text())), indent=1)); return 0

# ---- selftest ----
def fake_server(script, n_ctx=100000):
    """A stand-in llama-server: /props, /apply-template (concatenates messages), /tokenize (one token per
    whitespace-separated word), /completion (slot clears), /v1/chat/completions (scripted replies)."""
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    state = {"i": 0, "calls": []}
    class H(BaseHTTPRequestHandler):
        def log_message(self, *a): pass
        def _send(self, obj, code=200):
            b = json.dumps(obj).encode(); self.send_response(code); self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
        def do_GET(self):
            state["calls"].append((self.path, None)); self._send({"chat_template": "fake", "total_slots": 2, "default_generation_settings": {"n_ctx": n_ctx}})
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"]))); state["calls"].append((self.path, body))
            if self.path == "/apply-template":
                self._send({"prompt": "\n".join(m["content"] for m in body["messages"])})
            elif self.path == "/tokenize":
                self._send({"tokens": list(range(len(body["content"].split())))})
            elif self.path == "/completion":
                self._send({"content": "", "timings": {}})
            else:
                retr = body["messages"][-1]["content"].startswith("List this project")
                seq = script["retrieval" if retr else "application"]; reply = seq[state["i"] % len(seq)]
                if not retr: state["i"] += 1
                if reply["kind"] == "error":
                    self._send({"error": {"code": 500}}, 500); return
                n = len("\n".join(m["content"] for m in body["messages"]).split())
                self._send({"choices": [{"message": {"content": reply["content"], "reasoning_content": reply.get("reasoning", "thought")},
                                         "finish_reason": "stop"}], "usage": {"prompt_tokens": n, "completion_tokens": 10}, "timings": {}})
    srv = ThreadingHTTPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
    return srv, state

def cmd_selftest(a) -> int:
    import contextlib, io, shutil, tempfile
    fx = json.loads((HERE / "fixtures/graders.json").read_text()); bad = 0
    def check(ok, label, extra=""):
        nonlocal bad; bad += not ok; print(f"{'ok  ' if ok else 'FAIL'}  {label}" + ("" if ok else f" {extra}"))
    for case in fx["cases"]:
        got = grade(case["tier"], final_code(case["content"]))
        check(all(got.get(k) == v for k, v in case["expect"].items()), f"grader: {case['label']}", f"got {got}")
    for tier in TIERS:  # a seeded fault per constraint: each constraint violated alone must read red
        missing = [d for d in TIERS[tier]["dims"] if not any(c["tier"] == tier and c["expect"].get(d) is False and c["expect"].get("ALL") is False
                                                           for c in fx["cases"])]
        check(not missing, f"grader: every {tier} constraint has a fixture violating it", f"missing {missing}")
    e2e = json.loads((HERE / "fixtures/e2e.json").read_text())
    srv, state = fake_server(e2e["script"])
    with tempfile.TemporaryDirectory() as td:
        ns = argparse.Namespace(endpoint=f"http://127.0.0.1:{srv.server_address[1]}", corpus=str(HERE / "fixtures/corpus/manifest.json"),
                                tier="supported", serving_context=1000, fractions=[0.0, 0.5, 0.9], samples=5, retrieval=True,
                                max_tokens=64, seed=7, chunk_chars=120, tolerance=0.25, sampler='{"temperature": 0.6}', repo=[], out=td)
        with contextlib.redirect_stdout(io.StringIO()):
            cmd_run(ns)
        rows = [json.loads(l) for l in open(pathlib.Path(td) / "rows.jsonl")]
        meta = json.loads((pathlib.Path(td) / "meta.json").read_text())
    check(meta.get("instrument_sha256") == sha(pathlib.Path(__file__).read_bytes()) and meta.get("criterion_sha256") == sha((HERE / "criterion.toml").read_bytes()),
          "end to end: meta.json records the instrument's and the criterion's digests", f"got {meta.get('instrument_sha256')}, {meta.get('criterion_sha256')}")
    srv.shutdown()
    s = summarise(rows)
    got = {f: {"application": c["application"], "retrieval": c["retrieval"], "planted": c["planted"]} for f, c in s.items()}
    check(got == e2e["expect_summary"], "end to end: per-cell counts and plantings", f"got {got}")
    probe_calls = [b for p, b in state["calls"] if p == "/v1/chat/completions"]
    ces = json.loads((HERE / "fixtures/corpus/counterexamples.json").read_text())["tiers"]["supported"]
    ctrl = [b for b in probe_calls if not any(c["text"] in m["content"] for c in ces for m in b["messages"])]
    check(len(ctrl) == 6, "end to end: the control cell's requests carry no counter-example", f"{len(ctrl)} of 6")
    deep = [b for b in probe_calls if all(any(c["text"] in m["content"] for m in b["messages"]) for c in ces)]
    check(len(deep) == 13, "end to end: every deeper request carries every counter-example (12 samples and the one re-draw)", f"{len(deep)} of 13")
    inv = [r for r in rows if r.get("invalid")]
    check(len(inv) == 1 and inv[0]["discarded"] and any(r.get("redraw") == 1 and r["sample"] == inv[0]["sample"] and r["fraction"] == inv[0]["fraction"] for r in rows),
          "end to end: a thinking-off sample is recorded, discarded and re-drawn (planning, #143, 5921525110)", f"{inv}")
    cleared = sum(1 for p, b in state["calls"] if p == "/completion" and b.get("cache_prompt") is False)
    check(cleared == 3 * 2, "end to end: every slot cleared before each cell", f"({cleared})")
    check(all(b.get("id_slot") == 0 for b in probe_calls), "end to end: every probe request pinned to slot 0")
    check(all(r["depth_rendered"] >= r["depth_target"] * 0.75 for r in rows), "end to end: every cell reached within tolerance")
    # a rung that never engages thinking: each cell spends the declared re-draw budget, then keeps the invalid sample, so
    # the cell cannot fill five valid samples and reads unadjudicated, with the kwarg-delivery finding beside the word
    budget = tomllib.loads((HERE / "criterion.toml").read_text())["max_redraws"]
    off = {"application": [dict(r, reasoning="") for r in e2e["script"]["application"]], "retrieval": e2e["script"]["retrieval"]}
    srv3, _ = fake_server(off)
    with tempfile.TemporaryDirectory() as td3:
        ns3 = argparse.Namespace(**{**vars(ns), "endpoint": f"http://127.0.0.1:{srv3.server_address[1]}", "out": td3, "fractions": [0.0]})
        with contextlib.redirect_stdout(io.StringIO()):
            cmd_run(ns3)
        rows3 = [json.loads(l) for l in open(pathlib.Path(td3) / "rows.jsonl")]
    srv3.shutdown()
    app3 = [r for r in rows3 if r["stage"] == "application"]
    check(len(app3) == 5 + budget and sum(1 for r in app3 if r.get("discarded")) == budget,
          f"re-draw: a cell re-draws at most the declared {budget} times, then keeps its invalid samples", f"{len(app3)} rows, {sum(1 for r in app3 if r.get('discarded'))} discarded")
    d3 = decide(summarise(rows3), tomllib.loads((HERE / "criterion.toml").read_text()))
    check(d3["word"] == "unadjudicated", "re-draw: a cell that cannot fill five valid samples within the budget is unadjudicated", f"{d3}")
    base_cell = {"application": {"n": 5, "pass": 5, "errors": 0, "thinking_off": 0, "truncated": 0}, "retrieval": {"n": 1, "pass": 1}}
    two = {"0.0": base_cell, "0.5": {"application": {**base_cell["application"], "invalid": 2}, "retrieval": base_cell["retrieval"]}}
    one = {"0.0": base_cell, "0.5": {"application": {**base_cell["application"], "invalid": 1}, "retrieval": base_cell["retrieval"]}}
    crit = tomllib.loads((HERE / "criterion.toml").read_text())
    d2, d1 = decide(two, crit), decide(one, crit)
    check(d2["word"] == "pass" and d2.get("kwarg_delivery_findings", {}).get("cells") == {"0.5": 2},
          "decide: two invalid samples in a cell are a kwarg-delivery finding beside the word, not the word", f"{d2}")
    check("kwarg_delivery_findings" not in d1, "decide: one invalid sample in a cell is no finding")
    check(set(d2["criterion"]) == {"within", "min_samples", "max_errors"}, "decide: it echoes only the criterion keys it applies", f"{d2['criterion']}")
    # planting at declared depths: each counter-example sits at round(plant_at * n) among the corpus turns
    corpus = load_corpus(HERE / "fixtures/corpus/manifest.json")
    ces = json.loads((HERE / "fixtures/corpus/counterexamples.json").read_text())["tiers"]["supported"]
    for n in (5, 10, 17):
        pad = padding(corpus, ces, n, 3, 80); users = [m["content"] for m in pad if m["role"] == "user"]
        at = {c["constraint"]: next(i for i, u in enumerate(users) if c["text"] in u) for c in ces}
        want, turns = {}, n
        for c in sorted(ces, key=lambda c: c["plant_at"], reverse=True):  # the insertion the code performs, recomputed here
            idx = min(turns, round(c["plant_at"] * turns)); turns += 1
            want = {k: (v + 1 if v >= idx else v) for k, v in want.items()}; want[c["constraint"]] = idx
        check(at == want, f"padding: every counter-example planted at its declared fraction (n={n})", f"at {at}, want {want}")
    # refusals: a padding that cannot reach its target within tolerance; a ladder the server cannot hold
    srv2, _ = fake_server(e2e["script"], n_ctx=900)
    try:
        refused = False
        try: size_padding(Server(f"http://127.0.0.1:{srv2.server_address[1]}"), "supported", corpus, ces, 300, 1, 100000, 0.02)  # one chunk overshoots 300, none falls short of it by only 2%
        except SystemExit: refused = True
        check(refused, "size_padding: a depth missed by more than the tolerance is refused")
        ns2 = argparse.Namespace(**{**vars(ns), "endpoint": f"http://127.0.0.1:{srv2.server_address[1]}"})
        refused = False
        with tempfile.TemporaryDirectory() as td2:
            ns2.out = td2
            try:
                with contextlib.redirect_stdout(io.StringIO()): cmd_run(ns2)
            except SystemExit: refused = True
        check(refused, "run: a ladder whose deepest cell plus max_tokens exceeds the per-slot context is refused")
    finally:
        srv2.shutdown()
    bare = parser().parse_args(["run", "--endpoint", "x", "--corpus", "x", "--serving-context", "1", "--sampler", "{}", "--out", "x"])
    check(bare.retrieval is True, "run: retrieval is on unless --no-retrieval is given (as ruled)")
    # digests: a corpus entry or a counter-examples file that does not hash as the manifest says is refused
    with tempfile.TemporaryDirectory() as td:
        t = pathlib.Path(td) / "corpus"; shutil.copytree(HERE / "fixtures/corpus", t)
        (t / "net.rs").write_bytes((t / "net.rs").read_bytes() + b"// tampered\n")
        refused = False
        try: load_corpus(t / "manifest.json")
        except SystemExit: refused = True
        check(refused, "load_corpus: a tampered file-source entry is refused")
        refused = False
        try: load_counterexamples(HERE / "fixtures/corpus/counterexamples.json", "supported", "0" * 64)
        except SystemExit: refused = True
        check(refused, "load_counterexamples: a counter-examples file that does not hash as pinned is refused")
        g = pathlib.Path(td) / "repo"; g.mkdir(); (g / "src").mkdir(); (g / "src/a.rs").write_text("pub fn a() {}\n")
        run = lambda *a: subprocess.run(["git", "-C", str(g), "-c", "user.name=t", "-c", "user.email=t@t", *a], capture_output=True, check=True).stdout
        run("init", "-q"); run("add", "."); run("commit", "-qm", "a"); head = run("rev-parse", "HEAD").decode().strip()
        entry = {"path": "src/a.rs", "label": "cat src/a.rs", "sha256": sha(b"pub fn a() {}\n")}
        man = {"source": "git", "sources": [{"name": "fx", "commit": head, "root": "src", "reads": [entry], "listings": [], "diffs": []}]}
        (pathlib.Path(td) / "git.json").write_text(json.dumps(man))
        check(len(load_corpus(pathlib.Path(td) / "git.json", {"fx": str(g)})["reads"]) == 1, "load_corpus: a git-source entry that hashes as pinned loads")
        entry["sha256"] = "0" * 64; (pathlib.Path(td) / "git.json").write_text(json.dumps(man))
        refused = False
        try: load_corpus(pathlib.Path(td) / "git.json", {"fx": str(g)})
        except SystemExit: refused = True
        check(refused, "load_corpus: a git-source entry that does not hash as pinned is refused")
    # through run: a manifest whose counter-examples pin is wrong or missing is refused before any request;
    # --no-retrieval runs no retrieval sample
    for label, pin in (("wrong", "0" * 64), ("missing", None)):
        with tempfile.TemporaryDirectory() as td:
            t = pathlib.Path(td) / "corpus"; shutil.copytree(HERE / "fixtures/corpus", t)
            m = json.loads((t / "manifest.json").read_text())
            if pin: m["counterexamples_sha256"] = pin
            else: del m["counterexamples_sha256"]
            (t / "manifest.json").write_text(json.dumps(m))
            srv3, st3 = fake_server(e2e["script"]); refused = False
            try:
                with contextlib.redirect_stdout(io.StringIO()): cmd_run(argparse.Namespace(**{**vars(ns), "corpus": str(t / "manifest.json"), "out": td,
                                                                                          "endpoint": f"http://127.0.0.1:{srv3.server_address[1]}"}))
            except SystemExit: refused = True
            finally: srv3.shutdown()
            check(refused and not st3["calls"], f"run: a {label} counter-examples pin is refused before any request")
    with tempfile.TemporaryDirectory() as td:
        srv4, _ = fake_server(e2e["script"])
        try:
            with contextlib.redirect_stdout(io.StringIO()): cmd_run(argparse.Namespace(**{**vars(ns), "retrieval": False, "out": td, "fractions": [0.0],
                                                                                      "endpoint": f"http://127.0.0.1:{srv4.server_address[1]}"}))
            rows4 = [json.loads(l) for l in open(pathlib.Path(td) / "rows.jsonl")]
        finally: srv4.shutdown()
        check(rows4 and not any(r.get("stage") == "retrieval" for r in rows4), "run: --no-retrieval runs no retrieval sample", f"stages {[r.get('stage') for r in rows4]}")
    # make_corpus: a manifest outside the tree that holds the counter-examples is refused
    sys.path.insert(0, str(HERE)); import make_corpus
    with tempfile.TemporaryDirectory() as td:
        g = pathlib.Path(td) / "repo"; (g / "src").mkdir(parents=True); (g / "src/a.rs").write_text("pub fn a() {}\n")
        (g / "src/NOTES.md").write_text("not Rust\n")
        shutil.copy(HERE / "fixtures/corpus/counterexamples.json", g / "ce.json")
        run = lambda *a: subprocess.run(["git", "-C", str(g), "-c", "user.name=t", "-c", "user.email=t@t", *a], capture_output=True, check=True)
        run("init", "-q"); run("add", "."); run("commit", "-qm", "a")
        (g / "src/a.rs").write_text("pub fn a() -> u8 { 0 }\n"); run("commit", "-qam", "b")  # a parent for the diff
        outside = pathlib.Path(td) / "elsewhere/m.json"; outside.parent.mkdir(); refused = False
        try:
            with contextlib.redirect_stdout(io.StringIO()): make_corpus.main([str(g / "ce.json"), str(outside), f"fx={g}@HEAD:src"])
        except SystemExit: refused = True
        check(refused and not outside.exists(), "make_corpus: a manifest outside the tree that holds the counter-examples is refused")
        with contextlib.redirect_stdout(io.StringIO()): make_corpus.main([str(g / "ce.json"), str(g / "m.json"), f"fx={g}@HEAD:src"])
        mc = json.loads((g / "m.json").read_text())
        check(mc["counterexamples"] == "ce.json", "make_corpus: a manifest inside the tree records a relative path")
        check([e["path"] for e in mc["sources"][0]["reads"]] == ["src/a.rs"], "make_corpus: only Rust files become file reads", f"{mc['sources'][0]['reads']}")
        got = load_corpus(g / "m.json", {"fx": str(g)})  # what make_corpus writes, load_corpus reads back by digest
        check((len(got["reads"]), len(got["listings"]), len(got["diffs"])) == (1, 1, 1), "make_corpus: its manifest loads back, every digest agreeing",
              f"{[(k, len(v)) for k, v in got.items()]}")
    for c in json.loads((HERE / "fixtures/decide.json").read_text())["cases"]:
        dd = decide(c["summary"], c["criterion"]); check(dd["word"] == c["word"], f"decide: {c['label']}", f"got {dd['word']}")
        if "strict" in c: check(dd.get("strict_beside") == c["strict"], f"decide: strict beside -- {c['label']}", f"got {dd.get('strict_beside')}")
    print(f"depth_probe selftest: {'all pass' if not bad else f'{bad} failing'}"); return 1 if bad else 0

def parser() -> argparse.ArgumentParser:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run"); r.add_argument("--endpoint", required=True); r.add_argument("--corpus", required=True)
    r.add_argument("--tier", choices=list(TIERS), default="supported")
    r.add_argument("--serving-context", type=int, required=True, help="the rung's declared serving_context")
    r.add_argument("--fractions", type=float, nargs="+", default=[0.0, 0.5, 0.9, 0.95])
    r.add_argument("--samples", type=int, default=5)
    r.add_argument("--retrieval", action=argparse.BooleanOptionalAction, default=True,
                   help="one retrieval sample per cell, on by default as ruled (#143); the hard tier has no retrieval question")
    r.add_argument("--max-tokens", type=int, default=4096); r.add_argument("--seed", type=int, default=2000)
    r.add_argument("--chunk-chars", type=int, default=4000); r.add_argument("--tolerance", type=float, default=0.02)
    r.add_argument("--sampler", required=True, help='the rung\'s supported coding configuration, JSON, e.g. {"temperature": 0.6, "top_p": 0.95, "top_k": 20, "min_p": 0.0}')
    r.add_argument("--repo", action="append", default=[], help="NAME=PATH: a local clone for a corpus source (self defaults to this repository)")
    r.add_argument("--out", required=True); r.set_defaults(fn=cmd_run)
    s = sub.add_parser("summarise"); s.add_argument("rows"); s.set_defaults(fn=cmd_summarise)
    d = sub.add_parser("decide"); d.add_argument("summary"); d.add_argument("criterion"); d.set_defaults(fn=cmd_decide)
    t = sub.add_parser("selftest"); t.set_defaults(fn=cmd_selftest)
    return ap

def main(argv) -> int:
    a = parser().parse_args(argv); return a.fn(a)

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
