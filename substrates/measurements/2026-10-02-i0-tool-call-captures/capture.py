#!/usr/bin/env python3
"""#29 I0: streamed tool-call captures against a local llama-server at e486f80.
Raw server-to-client bytes kept exactly as received; request bodies kept beside them, unedited.
  capture.py turn1 OUT PORT              -> OUT/turn1.request.json, OUT/turn1.http
  capture.py turn2 OUT PORT OUTPUT_FILE  -> both re-send shapes, each request and reply
Bodies come from the crate's wire::streaming_body (examples/i0_bodies, scratch); the openai shape is
spliced into the crate's own turn-2-user body text so its head bytes stay the crate's."""
import hashlib, json, pathlib, socket, subprocess, sys
mode, out, port = sys.argv[1], pathlib.Path(sys.argv[2]), int(sys.argv[3]); out.mkdir(parents=True, exist_ok=True)
EX = pathlib.Path(__file__).resolve().parents[3] / "target/debug/examples/i0_bodies"  # i0_bodies.rs, built as a crate example
MODEL = "qwen3.8-flash-next"
notes_p = out / "notes.json"; notes = json.loads(notes_p.read_text()) if notes_p.exists() else {}
def raw(name, body: bytes):
    (out / f"{name}.request.json").write_bytes(body)
    head = (f"POST /v1/chat/completions HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: application/json\r\n"
            f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n").encode()
    s = socket.create_connection(("127.0.0.1", port), timeout=1800); s.sendall(head + body); buf = b""
    while (d := s.recv(65536)): buf += d
    s.close(); (out / f"{name}.http").write_bytes(buf)
    notes[name] = {"request_sha256": hashlib.sha256(body).hexdigest(), "request_bytes": len(body),
                   "response_sha256": hashlib.sha256(buf).hexdigest(), "response_bytes": len(buf),
                   "status_line": buf.split(b"\r\n", 1)[0].decode()}
    notes_p.write_text(json.dumps(notes, indent=1)); return buf
def events(buf):
    """data: payloads of a chunked SSE reply, in order."""
    body = buf.split(b"\r\n\r\n", 1)[1]; text = b""
    while body:
        size, _, rest = body.partition(b"\r\n"); n = int(size, 16)
        if n == 0: break
        text += rest[:n]; body = rest[n + 2:]
    return [l[6:] for l in text.decode().split("\n") if l.startswith("data: ") and l[6:] != "[DONE]"]
def assemble(buf):
    content, calls, finish = "", {}, None
    for e in map(json.loads, events(buf)):
        for c in e.get("choices", []):
            d = c.get("delta", {}); content += d.get("content") or ""
            for tc in d.get("tool_calls") or []:
                k = calls.setdefault(tc["index"], {"id": None, "name": "", "arguments": ""})
                k["id"] = tc.get("id") or k["id"]; f = tc.get("function") or {}
                k["name"] += f.get("name") or ""; k["arguments"] += f.get("arguments") or ""
            finish = c.get("finish_reason") or finish
    return content, [calls[i] for i in sorted(calls)], finish
if mode == "turn1":
    nonce = hashlib.sha256(str(__import__("time").time_ns()).encode()).hexdigest()[:16]; notes["nonce"] = nonce
    body = subprocess.run([EX, MODEL, nonce], capture_output=True, check=True).stdout
    buf = raw("turn1", body); c, calls, fin = assemble(buf)
    notes["turn1_assembled"] = {"content": c, "tool_calls": calls, "finish_reason": fin}; notes_p.write_text(json.dumps(notes, indent=1))
    print(json.dumps(notes["turn1_assembled"], indent=1))
else:
    output = pathlib.Path(sys.argv[4]).read_text(); t1 = notes["turn1_assembled"]; call = t1["tool_calls"][0]
    cmd = json.loads(call["arguments"])["command"]
    user_result = f"The bash tool ran `{cmd}` and printed:\n{output}"
    b_user = subprocess.run([EX, MODEL, notes["nonce"], t1["content"], user_result], capture_output=True, check=True).stdout
    buf = raw("turn2-user", b_user)
    # openai shape: replace the crate's last two messages with assistant tool_calls + a tool message
    s = b_user.decode(); tail_from = s.index(',{"role":"assistant"')
    end = s.index('],"temperature"') if '],"temperature"' in s else s.index("]", tail_from)
    asst = {"role": "assistant", "content": t1["content"] or None,
            "tool_calls": [{"id": call["id"], "type": "function", "function": {"name": call["name"], "arguments": call["arguments"]}}]}
    tool = {"role": "tool", "tool_call_id": call["id"], "content": output}
    b_oai = (s[:tail_from] + "," + json.dumps(asst, separators=(",", ":"), ensure_ascii=False) + "," +
             json.dumps(tool, separators=(",", ":"), ensure_ascii=False) + s[end:]).encode()
    buf2 = raw("turn2-openai", b_oai)
    for n, b in (("turn2-user", buf), ("turn2-openai", buf2)):
        ev = [json.loads(e) for e in events(b)]; last = ev[-1] if ev else {}
        notes[n + "_receipt"] = {"timings": {k: last.get("timings", {}).get(k) for k in ("cache_n", "prompt_n")}, "assembled": assemble(b)}
    notes_p.write_text(json.dumps(notes, indent=1)); print(json.dumps({k: notes[k] for k in notes if k.endswith("_receipt")}, indent=1))
