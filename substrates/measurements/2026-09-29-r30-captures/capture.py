#!/usr/bin/env python3
"""#117 R3.0 captures C0-C5 against the running production server (never stopped or
relaunched by this script). Raw bytes kept are SERVER-TO-CLIENT only; the request
direction carries the key, which is read from the launch file and never printed or
written. /slots (C4) keeps key names and the capturing slot's numeric fields only.
Usage: r30.py OUTDIR"""
import hashlib, json, os, re, socket, sys, threading, time, pathlib, urllib.request
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
cmd = open(os.path.expanduser('~/setup/prod-cmdline.txt')).read()
HOST, PORT = "127.0.0.1", int(re.search(r'--port (\d+)', cmd).group(1))
KEY = re.search(r'--api-key (\S+)', cmd).group(1)
NONCE = hashlib.sha256(str(time.time_ns()).encode()).hexdigest()[:16]
notes = {"nonce": NONCE, "port": PORT, "started": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}

def get(path):
    req = urllib.request.Request(f"http://{HOST}:{PORT}{path}", headers={"Authorization": f"Bearer {KEY}"})
    with urllib.request.urlopen(req, timeout=60) as r: return json.load(r)
def post(path, body):
    req = urllib.request.Request(f"http://{HOST}:{PORT}{path}", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json", "Authorization": f"Bearer {KEY}"})
    with urllib.request.urlopen(req, timeout=900) as r: return json.load(r)
def raw(path, body, name):
    """One request over a raw socket; returns and writes the response bytes only."""
    b = json.dumps(body).encode()
    head = (f"POST {path} HTTP/1.1\r\nHost: {HOST}:{PORT}\r\nContent-Type: application/json\r\n"
            f"Authorization: Bearer {KEY}\r\nContent-Length: {len(b)}\r\nConnection: close\r\n\r\n").encode()
    s = socket.create_connection((HOST, PORT), timeout=900); s.sendall(head + b)
    buf = b""
    while (d := s.recv(65536)):
        buf += d
    s.close()
    (out / f"{name}.http").write_bytes(buf)
    notes[name] = {"response_bytes": len(buf), "response_sha256": hashlib.sha256(buf).hexdigest(),
                   "status_line": buf.split(b"\r\n", 1)[0].decode(errors="replace"),
                   "request_body_sha256": hashlib.sha256(b).hexdigest(), "request_body_bytes": len(b)}
    return buf
def ntok(text):
    return len(post("/tokenize", {"content": text})["tokens"])
def rendered_len(messages):
    return ntok(post("/apply-template", {"messages": messages, "add_generation_prompt": True})["prompt"])

# C0: the limits, by GET (occupies no slot)
props = get("/props")
dgs = props.get("default_generation_settings") or {}
c0 = {"props_keys": sorted(props), "default_generation_settings_keys": sorted(dgs),
      "total_slots": props.get("total_slots"), "n_ctx_per_slot": dgs.get("n_ctx"),
      "slots_count": len(get("/slots"))}
(out / "C0-limits.json").write_text(json.dumps(c0, indent=1))
limit = c0["n_ctx_per_slot"]
print("C0", c0["total_slots"], limit, flush=True)

# C1: cold multi-batch prefill, streamed, progress asked for; /slots polled beside it (C4 material)
filler = "\n".join(f"Record {i:05d}: the inventory count for bay {i % 97} was checked and logged." for i in range(420))
c1_msgs = [{"role": "user", "content": f"[{NONCE}] Below is a log.\n{filler}\nHow many records are there? Answer in one word."}]
notes["C1_prompt_tokens_rendered"] = rendered_len(c1_msgs)
polls, stop = [], threading.Event()
def poll():
    while not stop.is_set():
        try:
            polls.append((time.time(), get("/slots")))
        except Exception as e:
            polls.append((time.time(), repr(e)))
        time.sleep(0.25)
th = threading.Thread(target=poll); th.start()
c1 = raw("/v1/chat/completions", {"messages": c1_msgs, "stream": True, "stream_options": {"include_usage": True},
                                  "return_progress": True, "max_tokens": 64, "temperature": 0.6, "id_slot": 1}, "C1-progress")
stop.set(); th.join()
frames = c1.count(b"prompt_progress")
notes["C1_prompt_progress_occurrences"] = frames
print("C1", notes["C1-progress"]["status_line"], "progress frames:", frames, flush=True)
# C4 only when C1 shows no frames: key names, and the busy slot's numeric fields
busy = [[x for x in s if isinstance(x, dict) and x.get("is_processing")] for _, s in polls if isinstance(s, list)]
if frames == 0:
    keys = sorted({k for _, s in polls if isinstance(s, list) for x in s for k in x})
    mine = [{k: v for k, v in b[0].items() if isinstance(v, (int, float, bool))} for b in busy if len(b) == 1]
    (out / "C4-slots.json").write_text(json.dumps({"slot_keys": keys, "polls": len(polls),
        "polls_with_exactly_one_busy_slot": len(mine), "capturing_slot_numeric_fields": mine}, indent=1))
notes["C1_polls"] = len(polls); notes["C1_polls_with_other_busy"] = sum(1 for b in busy if len(b) > 1)

# C2: a streamed warm turn 2, both turns pinned to slot 2 by id_slot
q1 = f"[{NONCE}] A train leaves at 14:05 and arrives at 17:50. How long is the journey in minutes? Answer with the number."
t1 = post("/v1/chat/completions", {"messages": [{"role": "user", "content": q1}], "max_tokens": 1024,
                                   "temperature": 0.6, "id_slot": 2})
m1 = t1["choices"][0]["message"]
notes["C2_turn1_timings"] = t1.get("timings")
a1 = {"role": "assistant", "content": m1.get("content") or ""}
if m1.get("reasoning_content") is not None: a1["reasoning_content"] = m1["reasoning_content"]
raw("/v1/chat/completions", {"messages": [{"role": "user", "content": q1}, a1, {"role": "user", "content": "Now in hours and minutes."}],
                             "stream": True, "stream_options": {"include_usage": True}, "max_tokens": 1024,
                             "temperature": 0.6, "id_slot": 2}, "C2-warm-turn2")
print("C2", notes["C2-warm-turn2"]["status_line"], flush=True)

# C5: one raw unstreamed reply
raw("/v1/chat/completions", {"messages": [{"role": "user", "content": f"[{NONCE}] Say OK."}], "stream": False,
                             "max_tokens": 256, "temperature": 0.6}, "C5-unstreamed")
print("C5", notes["C5-unstreamed"]["status_line"], flush=True)

# C3: the smallest prompt over the per-slot limit, counted by /tokenize on the rendered prompt, sent ONCE
unit = "The quick brown fox jumps over the lazy dog near the riverbank. "
per = ntok(unit * 200) / 200
def msgs(n, extra=""):
    return [{"role": "user", "content": f"[{NONCE}] " + unit * n + extra}]
n = int((limit - rendered_len(msgs(0))) / per)
L = rendered_len(msgs(n))
while L <= limit:
    n += max(1, int((limit - L) / per)); L = rendered_len(msgs(n))
while True:
    L2 = rendered_len(msgs(n - 1))
    if L2 > limit: n, L = n - 1, L2
    else: break
pad = ""
for w in ("a ", "b ", "c ", "d ", "e ", "f ", "g ", "h "):
    if rendered_len(msgs(n - 1, pad + w)) > limit:
        break
    pad += w
if rendered_len(msgs(n - 1, pad)) > limit:
    c3_msgs = msgs(n - 1, pad)
else:
    c3_msgs = msgs(n)
notes["C3_rendered_tokens"] = rendered_len(c3_msgs); notes["C3_limit"] = limit
notes["C3_other_slots_busy_before"] = [x.get("id") for x in get("/slots") if x.get("is_processing")]
print("C3 sending", notes["C3_rendered_tokens"], "tokens against", limit, flush=True)
raw("/v1/chat/completions", {"messages": c3_msgs, "stream": True, "stream_options": {"include_usage": True},
                             "max_tokens": 16, "temperature": 0.6}, "C3-overflow")
print("C3", notes["C3-overflow"]["status_line"], flush=True)
notes["finished"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
(out / "notes.json").write_text(json.dumps(notes, indent=1))
