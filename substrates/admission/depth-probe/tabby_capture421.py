#!/usr/bin/env python3
"""#421's capture from a TabbyAPI line (#393 window): the exact requests and every reply as bytes, I0-style, kept raw.
  capture  --endpoint URL --i0-dir DIR --out DIR [--sampler-json JSON]   three POSTs and three GETs
  verify   DIR                                                          re-hash every file against notes.json
POSTs (chat completions, streamed, I0's tool and prompt with a fresh nonce, I0's sampler unless --sampler-json names #301's):
  turn1-off      enable_thinking false;  turn1-on  enable_thinking true;  turn2-openai  the assistant's call re-sent in the openai shape
                 (an assistant message with tool_calls, then a tool message carrying I0's recorded output, tool-output.txt; nothing is executed here)
A request that draws a 4xx is kept as <name>.refused.http with its request, and re-sent without the llama.cpp-only key(s) it names
(return_progress): both are kept, so the refusal is data. GETs: /v1/model, /v1/models, /health, bytes as received.
The bearer key comes from TABBY_API_KEY and is added to the request head as sent; the kept head has that line replaced by
`Authorization: Bearer <redacted>`, and the key appears nowhere else."""
from __future__ import annotations
import argparse, hashlib, json, os, pathlib, socket, sys, time, urllib.parse
LLAMA_ONLY = ("return_progress",)

def sha(b: bytes) -> str: return hashlib.sha256(b).hexdigest()

def exchange(url, method, path, body=None):
    u = urllib.parse.urlparse(url); key = os.environ.get("TABBY_API_KEY")
    def head(auth): 
        h = f"{method} {path} HTTP/1.1\r\nHost: {u.hostname}:{u.port}\r\n" + (f"Authorization: Bearer {auth}\r\n" if key else "")
        h += "Content-Type: application/json\r\n" if body is not None else ""
        h += f"Content-Length: {len(body)}\r\n" if body is not None else ""
        return h + "Connection: close\r\n\r\n"
    s = socket.create_connection((u.hostname, u.port), timeout=1800); s.sendall(head(key).encode() + (body or b"")); buf = b""
    while d := s.recv(65536): buf += d
    s.close(); return head("<redacted>").encode(), buf

def status(buf: bytes) -> int: return int(buf.split(b" ", 2)[1])

def events(buf):
    body = buf.split(b"\r\n\r\n", 1)[1]
    if b"Transfer-Encoding: chunked" in buf.split(b"\r\n\r\n", 1)[0]:
        text = b""
        while body:
            size, _, rest = body.partition(b"\r\n"); n = int(size, 16)
            if n == 0: break
            text += rest[:n]; body = rest[n + 2:]
    else: text = body
    return [l[6:] for l in text.decode().split("\n") if l.startswith("data: ") and l[6:] != "[DONE]"]

def assemble(buf):
    content, reasoning, calls, finish = "", "", {}, None
    for e in map(json.loads, events(buf)):
        for c in e.get("choices", []):
            d = c.get("delta", {}); content += d.get("content") or ""; reasoning += d.get("reasoning_content") or ""
            for tc in d.get("tool_calls") or []:
                k = calls.setdefault(tc.get("index", 0), {"id": None, "name": "", "arguments": ""}); k["id"] = tc.get("id") or k["id"]
                f = tc.get("function") or {}; k["name"] += f.get("name") or ""; k["arguments"] += f.get("arguments") or ""
            finish = c.get("finish_reason") or finish
    return {"content": content, "reasoning_chars": len(reasoning), "tool_calls": [calls[i] for i in sorted(calls)], "finish_reason": finish}

def capture(a) -> int:
    out = pathlib.Path(a.out); out.mkdir(parents=True, exist_ok=True); i0 = pathlib.Path(a.i0_dir); notes = {}
    model = json.loads(exchange(a.endpoint, "GET", "/v1/model")[1].split(b"\r\n\r\n", 1)[1])["id"]
    base = json.loads((i0 / "turn1.request.json").read_text()); nonce = sha(str(time.time_ns()).encode())[:16]; notes["nonce"] = nonce; notes["model"] = model
    base["model"] = model; base["messages"][1]["content"] = base["messages"][1]["content"].replace(base["messages"][1]["content"].split("]")[0] + "]", f"[{nonce}]", 1)
    if a.sampler_json: base.update(json.loads(a.sampler_json)); notes["sampler"] = "named by --sampler-json"
    else: notes["sampler"] = "I0's"
    def post(name, body):
        raw = json.dumps(body, separators=(",", ":"), ensure_ascii=False).encode(); head, buf = exchange(a.endpoint, "POST", "/v1/chat/completions", raw)
        if 400 <= status(buf) < 500:
            keep(name + ".refused", head, raw, buf); body = {k: v for k, v in body.items() if k not in LLAMA_ONLY}
            raw = json.dumps(body, separators=(",", ":"), ensure_ascii=False).encode(); head, buf = exchange(a.endpoint, "POST", "/v1/chat/completions", raw)
        keep(name, head, raw, buf); return buf
    def keep(name, head, raw, buf):
        (out / f"{name}.request.head.txt").write_bytes(head); (out / f"{name}.request.json").write_bytes(raw); (out / f"{name}.http").write_bytes(buf)
        notes[name] = {"request_sha256": sha(raw), "response_sha256": sha(buf), "response_bytes": len(buf), "status": status(buf)}
    off = json.loads(json.dumps(base)); off["chat_template_kwargs"] = {"enable_thinking": False}
    on = json.loads(json.dumps(base)); on["chat_template_kwargs"] = {"enable_thinking": True}
    b1 = post("turn1-off", off); notes["turn1-off_assembled"] = assemble(b1) if status(b1) == 200 else None
    b2 = post("turn1-on", on); notes["turn1-on_assembled"] = assemble(b2) if status(b2) == 200 else None
    calls = (notes["turn1-off_assembled"] or {}).get("tool_calls") or []
    if calls:
        c = calls[0]; t2 = json.loads(json.dumps(off)); t2["messages"] += [
            {"role": "assistant", "content": notes["turn1-off_assembled"]["content"] or None, "tool_calls": [{"id": c["id"], "type": "function", "function": {"name": c["name"], "arguments": c["arguments"]}}]},
            {"role": "tool", "tool_call_id": c["id"], "content": (i0 / "tool-output.txt").read_text()}]
        b3 = post("turn2-openai", t2); notes["turn2-openai_assembled"] = assemble(b3) if status(b3) == 200 else None
    else: notes["turn2-openai_assembled"] = "not sent: turn 1 returned no tool call"
    for name, path in (("get-v1-model", "/v1/model"), ("get-v1-models", "/v1/models"), ("get-health", "/health")):
        head, buf = exchange(a.endpoint, "GET", path); (out / f"{name}.request.head.txt").write_bytes(head); (out / f"{name}.http").write_bytes(buf)
        notes[name] = {"response_sha256": sha(buf), "response_bytes": len(buf), "status": status(buf)}
    (out / "notes.json").write_text(json.dumps(notes, indent=1) + "\n"); print(json.dumps({k: v.get("status") for k, v in notes.items() if isinstance(v, dict) and "status" in v})); return 0

def verify(a) -> int:
    d = pathlib.Path(a.dir); notes = json.loads((d / "notes.json").read_text()); bad = []
    for name, v in notes.items():
        if not isinstance(v, dict) or "response_sha256" not in v: continue
        if sha((d / f"{name}.http").read_bytes()) != v["response_sha256"]: bad.append(f"{name}.http")
        if "request_sha256" in v and sha((d / f"{name}.request.json").read_bytes()) != v["request_sha256"]: bad.append(f"{name}.request.json")
    key = os.environ.get("TABBY_API_KEY")
    for p in d.iterdir():
        if key and key.encode() in p.read_bytes(): bad.append(f"{p.name} contains the key")
    for b in bad: print(f"verify: {b} does not hash as notes.json says")
    if not bad: print("verify: every file hashes as notes.json says")
    return 1 if bad else 0

if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0]); sub = p.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("capture"); c.add_argument("--endpoint", required=True); c.add_argument("--i0-dir", required=True); c.add_argument("--out", required=True); c.add_argument("--sampler-json"); c.set_defaults(fn=capture)
    v = sub.add_parser("verify"); v.add_argument("dir"); v.set_defaults(fn=verify)
    a = p.parse_args(); sys.exit(a.fn(a))
