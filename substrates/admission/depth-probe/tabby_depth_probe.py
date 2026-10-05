#!/usr/bin/env python3
"""The depth probe against a TabbyAPI endpoint (#393). depth_probe.py is untouched: its digest is pinned by every recorded depth
cell, and `summarise` / `decide` are its own. This file swaps only the server layer:
  - `ntokens`: the model directory's template rendered here with jinja2 (add_generation_prompt on), the string POSTed to /v1/token/encode.
    The endpoint's own message-list path was measured on 2026-10-04 to refuse (422) when the config names no api_servers and, when it
    works, to force add_generation_prompt off (TabbyAPI be74bf0a, endpoints/core/router.py): neither is the depth a chat request sees;
  - no slot clearing (the engine has no slots) and no `id_slot` / `cache_prompt` in the request;
  - the per-request context and the pool come from GET /v1/model; the template digest from the model directory's template file
    (ruled at #393 5986337117: the digest comes from the model directory, the rendering is a capability fact).
Every request's own `usage.prompt_tokens` is checked against the depth the encode endpoint gave: a cell that misses by more than
--ntokens-tolerance is an error row, which `decide` reads as unadjudicated. The key, when the endpoint has one, comes from
TABBY_API_KEY and is never written anywhere."""
from __future__ import annotations
import argparse, json, os, pathlib, sys, urllib.request
HERE = pathlib.Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import depth_probe as dp  # noqa: E402

class TabbyServer(dp.Server):
    def __init__(self, base: str, timeout: int = 7200):
        super().__init__(base, timeout)
        self.key = os.environ.get("TABBY_API_KEY"); self.template_text = ""; self.model = {}; self._tpl = None
    def _req(self, path, body=None):
        h = {"Content-Type": "application/json"}
        if self.key: h["Authorization"] = f"Bearer {self.key}"
        req = urllib.request.Request(self.base + path, data=json.dumps(body).encode() if body is not None else None, headers=h)
        with urllib.request.urlopen(req, timeout=self.timeout) as r:
            return json.load(r)
    def call(self, path, body=None):
        if path == "/props":
            self.model = self._req("/v1/model"); p = self.model.get("parameters", {})
            return {"total_slots": 1, "chat_template": self.template_text,
                    "default_generation_settings": {"n_ctx": p.get("max_seq_len")}}
        if path == "/v1/chat/completions" and body is not None:
            body = {k: v for k, v in body.items() if k not in ("id_slot", "cache_prompt")}
        return self._req(path, body)
    def render(self, messages) -> str:
        if self._tpl is None:
            from jinja2.sandbox import ImmutableSandboxedEnvironment
            def raise_exception(m): raise ValueError(m)
            env = ImmutableSandboxedEnvironment(trim_blocks=True, lstrip_blocks=True); env.globals["raise_exception"] = raise_exception
            self._tpl = env.from_string(self.template_text)
        return self._tpl.render(messages=messages, add_generation_prompt=True)
    def ntokens(self, messages) -> int:
        return len(self._req("/v1/token/encode", {"text": self.render(messages), "add_bos_token": False})["tokens"])
    def clear_slots(self, n: int):
        return None

def cmd_run(a) -> int:
    tpl = pathlib.Path(a.template_file).read_text()
    class S(TabbyServer):
        def __init__(self, base, timeout=7200):
            super().__init__(base, timeout); self.template_text = tpl
    orig_server, orig_draw = dp.Server, dp.draw
    def draw(srv, aa, stage, s, attempt, frac, target, n, planted, pad):
        row = orig_draw(srv, aa, stage, s, attempt, frac, target, n, planted, pad)
        pt = row.get("prompt_tokens")
        if pt is not None and abs(pt - n) > max(8, a.ntokens_tolerance * n):
            row["error"] = f"ntokens-mismatch: encode {n}, usage {pt}"; row["dims"] = {"ALL": False}
        return row
    dp.Server, dp.draw = S, draw
    try:
        rc = dp.cmd_run(a)
    finally:
        dp.Server, dp.draw = orig_server, orig_draw
    mp = pathlib.Path(a.out) / "meta.json"; meta = json.loads(mp.read_text())
    srv = S(a.endpoint); srv.call("/props")
    meta.update({"engine": "tabbyapi", "wrapper": "tabby_depth_probe.py", "wrapper_sha256": dp.sha(pathlib.Path(__file__).read_bytes()),
                 "model_id": srv.model.get("id"), "model_parameters": srv.model.get("parameters"),
                 "template_source": f"model directory file {pathlib.Path(a.template_file).name}", "ntokens_tolerance": a.ntokens_tolerance})
    mp.write_text(json.dumps(meta, indent=1) + "\n")
    return rc

def cmd_selftest(a) -> int:
    import threading
    from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
    seen = []
    class H(BaseHTTPRequestHandler):
        def log_message(self, *x): pass
        def _send(self, o):
            b = json.dumps(o).encode(); self.send_response(200); self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(b))); self.end_headers(); self.wfile.write(b)
        def do_GET(self):
            seen.append(("GET", self.path, None, self.headers.get("Authorization"))); self._send({"id": "m", "parameters": {"max_seq_len": 1234}})
        def do_POST(self):
            b = json.loads(self.rfile.read(int(self.headers["Content-Length"]))); seen.append(("POST", self.path, b, self.headers.get("Authorization")))
            self._send({"tokens": list(range(len(b["text"].split())))})
    srv = ThreadingHTTPServer(("127.0.0.1", 0), H); threading.Thread(target=srv.serve_forever, daemon=True).start()
    os.environ["TABBY_API_KEY"] = "selftest-key-0"
    s = TabbyServer(f"http://127.0.0.1:{srv.server_address[1]}"); s.template_text = "{% for m in messages %}{{ m.content }} {% endfor %}{% if add_generation_prompt %}GEN{% endif %}"
    ok = True
    def check(name, cond):
        nonlocal ok; print(("ok   " if cond else "FAIL ") + name); ok &= bool(cond)
    p = s.call("/props")
    check("props: context from /v1/model, one slot, template from the file", p["default_generation_settings"]["n_ctx"] == 1234 and p["total_slots"] == 1 and p["chat_template"].startswith("{% for"))
    check("ntokens: the template rendered here, the string encoded, generation prompt counted", s.ntokens([{"role": "user", "content": "a b c"}, {"role": "assistant", "content": "d"}]) == 5 and isinstance(seen[-1][2]["text"], str) and seen[-1][2]["text"].endswith("GEN"))
    check("the key is sent as a bearer and never in a body", all(x[3] == "Bearer selftest-key-0" for x in seen) and "selftest-key-0" not in json.dumps([x[2] for x in seen]))
    check("clear_slots makes no request", (lambda n: (s.clear_slots(2), len(seen) == n)[1])(len(seen)))
    srv.shutdown(); return 0 if ok else 1

def parser() -> argparse.ArgumentParser:
    p = dp.parser(); sub = next(x for x in p._actions if isinstance(x, argparse._SubParsersAction))
    r = sub.choices["run"]; r.add_argument("--template-file", required=True, help="the model directory's chat template file, whose bytes' digest is recorded")
    r.add_argument("--ntokens-tolerance", type=float, default=0.01); r.set_defaults(fn=cmd_run)
    sub.choices["selftest"].set_defaults(fn=cmd_selftest)
    return p

if __name__ == "__main__":
    a = parser().parse_args(sys.argv[1:]); sys.exit(a.fn(a))
