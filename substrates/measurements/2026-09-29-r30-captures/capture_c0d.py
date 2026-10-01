#!/usr/bin/env python3
"""C0d: two chat replies from the floor's production server, read-only, each carrying usage and timings, so the
record can see whether llama.cpp's usage counts equal its timing counts on this engine (#157): one unstreamed, one
streamed with stream_options.include_usage. Raw SERVER-TO-CLIENT bytes are kept as received; the request bodies are
kept too (they carry no key: this server takes none, and the prompt is a fixed short sentence). Writes
C0d-unstreamed.http, C0d-streamed.http, the two request bodies, and notes-c0d.json with digests and the equality read.
Usage: capture_c0d.py BASE_URL OUTDIR"""
import hashlib, json, pathlib, socket, sys, time, urllib.parse
url, out = sys.argv[1].rstrip("/"), pathlib.Path(sys.argv[2]); out.mkdir(parents=True, exist_ok=True)
u = urllib.parse.urlparse(url); HOST, PORT = u.hostname, u.port or 80
sha = lambda b: hashlib.sha256(b).hexdigest()
NONCE = sha(str(time.time_ns()).encode())[:12]
MSGS = [{"role": "user", "content": f"[{NONCE}] Name one prime number between 10 and 20. Answer in one line."}]
def raw(body, name):
    b = json.dumps(body).encode()
    head = (f"POST /v1/chat/completions HTTP/1.1\r\nHost: llama-server\r\nContent-Type: application/json\r\n"
            f"Content-Length: {len(b)}\r\nConnection: close\r\n\r\n").encode()
    s = socket.create_connection((HOST, PORT), timeout=300); s.sendall(head + b); buf = b""
    while (d := s.recv(65536)):
        buf += d
    s.close()
    (out / f"{name}.http").write_bytes(buf); (out / f"{name}.request.json").write_bytes(b)
    return buf, b
def body_of(http):
    head, _, rest = http.partition(b"\r\n\r\n")
    if b"chunked" in head.lower():  # de-chunk
        data, i = b"", 0
        while True:
            j = rest.index(b"\r\n", i); n = int(rest[i:j], 16)
            if n == 0: break
            data += rest[j + 2:j + 2 + n]; i = j + 2 + n + 2
        return data
    return rest
def equality(usage, t):
    return {"prompt_tokens == prompt_n + cache_n": usage.get("prompt_tokens") == t.get("prompt_n", 0) + t.get("cache_n", 0),
            "completion_tokens == predicted_n": usage.get("completion_tokens") == t.get("predicted_n"),
            "cached_tokens == cache_n": (usage.get("prompt_tokens_details") or {}).get("cached_tokens") == t.get("cache_n")}
notes = {"nonce": NONCE, "taken": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
base = {"messages": MSGS, "max_tokens": 32, "temperature": 0.6, "top_k": 20, "top_p": 1.0,
        "chat_template_kwargs": {"enable_thinking": False}}
h1, r1 = raw(dict(base), "C0d-unstreamed")
j1 = json.loads(body_of(h1)); u1, t1 = j1.get("usage") or {}, j1.get("timings") or {}
notes["unstreamed"] = {"status_line": h1.split(b"\r\n", 1)[0].decode(), "response_bytes": len(h1), "response_sha256": sha(h1),
                       "request_sha256": sha(r1), "usage": u1, "timings": {k: t1.get(k) for k in ("prompt_n", "cache_n", "predicted_n")},
                       "equal": equality(u1, t1)}
h2, r2 = raw(dict(base, stream=True, stream_options={"include_usage": True}), "C0d-streamed")
events = [json.loads(l[6:]) for l in body_of(h2).decode().splitlines() if l.startswith("data: {")]
final = [e for e in events if e.get("usage")]
u2 = final[-1]["usage"] if final else {}; t2 = (final[-1].get("timings") if final else None) or {}
notes["streamed"] = {"status_line": h2.split(b"\r\n", 1)[0].decode(), "response_bytes": len(h2), "response_sha256": sha(h2),
                     "request_sha256": sha(r2), "data_events": len(events), "usage_chunks": len(final),
                     "final_chunk_has_timings": bool(final and final[-1].get("timings")), "usage": u2,
                     "timings": {k: t2.get(k) for k in ("prompt_n", "cache_n", "predicted_n")}, "equal": equality(u2, t2)}
(out / "notes-c0d.json").write_text(json.dumps(notes, indent=1) + "\n")
print(json.dumps(notes, indent=1))
