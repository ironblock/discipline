#!/usr/bin/env python3
"""#117 Q10 measurements against the running production llama-server, read-only.
Raw-socket capture of one streamed thinking turn (as llama-server-4df29be-stream.http
was captured), then the prompt_n A/B and the tokenized prefix diff. The api key is
read from the launch file and never printed or written."""
import json, os, re, socket, sys, time, urllib.request, pathlib
out = pathlib.Path(sys.argv[1]); out.mkdir(parents=True, exist_ok=True)
cmd = open(os.path.expanduser('~/setup/prod-cmdline.txt')).read()
host = re.search(r'--host (\S+)', cmd).group(1); port = int(re.search(r'--port (\d+)', cmd).group(1))
key = re.search(r'--api-key (\S+)', cmd).group(1)
Q1 = "A train leaves at 14:05 and arrives at 17:50. How long is the journey in minutes? Answer with the number and one short sentence."
Q2 = "Now convert that to hours and minutes."
def post(path, body):
    req = urllib.request.Request(f"http://{host}:{port}{path}", data=json.dumps(body).encode(),
                                 headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=600) as r: return json.load(r)
# 1. raw capture
body = json.dumps({"messages": [{"role": "user", "content": Q1}], "stream": True,
                   "stream_options": {"include_usage": True}, "max_tokens": 2048,
                   "temperature": 0.6, "top_p": 0.95}).encode()
head = (f"POST /v1/chat/completions HTTP/1.1\r\nHost: {host}:{port}\r\nContent-Type: application/json\r\n"
        f"Authorization: Bearer {key}\r\nContent-Length: {len(body)}\r\nConnection: close\r\n\r\n").encode()
s = socket.create_connection((host, port), timeout=600); s.sendall(head + body)
raw = b""
while True:
    b = s.recv(65536)
    if not b: break
    raw += b
s.close()
(out / "stream.http").write_bytes(raw)
# decode the stream to get the turn's reasoning and content
text = raw.split(b"\r\n\r\n", 1)[1]
dec = b""; i = 0
while True:
    j = text.index(b"\r\n", i); n = int(text[i:j], 16)
    if n == 0: break
    dec += text[j+2:j+2+n]; i = j + 2 + n + 2
reasoning, content, usage = "", "", None
for line in dec.decode().split("\n"):
    if line.startswith("data: ") and line != "data: [DONE]":
        e = json.loads(line[6:])
        for c in e.get("choices", []):
            d = c.get("delta", {}); reasoning += d.get("reasoning_content") or ""; content += d.get("content") or ""
        if e.get("usage"): usage = {"usage": e["usage"], "timings": e.get("timings")}
(out / "turn1.json").write_text(json.dumps({"reasoning_content": reasoning, "content": content, "final": usage}, indent=1, ensure_ascii=False))
# 2. A/B: the same turn-2 history with and without reasoning_content, order with / without / with
def turn2(with_r):
    a = {"role": "assistant", "content": content}
    if with_r: a["reasoning_content"] = reasoning
    return [{"role": "user", "content": Q1}, a, {"role": "user", "content": Q2}]
ab = []
for with_r in (True, False, True):
    r = post("/v1/chat/completions", {"messages": turn2(with_r), "max_tokens": 1, "temperature": 0.6})
    ab.append({"with_reasoning": with_r, "prompt_n": r["timings"]["prompt_n"], "cache_n": r["timings"].get("cache_n"), "prompt_tokens": r["usage"]["prompt_tokens"]})
    time.sleep(1)
(out / "ab.json").write_text(json.dumps(ab, indent=1))
# 3. tokenized diff: turn-1 prompt + the generated stream, against the re-rendered turn-2 prompt
p1 = post("/apply-template", {"messages": [{"role": "user", "content": Q1}], "add_generation_prompt": True})["prompt"]
p2 = post("/apply-template", {"messages": turn2(True), "add_generation_prompt": True})["prompt"]
gen_think = post("/apply-template", {"messages": [{"role": "user", "content": Q1}, {"role": "assistant", "content": content, "reasoning_content": reasoning}], "add_generation_prompt": False})["prompt"]
tok = lambda t: post("/tokenize", {"content": t, "add_special": False, "with_pieces": True})["tokens"]
t_gen = tok(p1) ; t_p2 = tok(p2)
# the generated stream as the model emitted it: the generation prompt, then its thinking and answer
generated = p1 + "<think>\n" + reasoning + "\n</think>\n\n" + content
t_stream = tok(generated)
def first_diff(a, b):
    for k, (x, y) in enumerate(zip(a, b)):
        if (x["id"] if isinstance(x, dict) else x) != (y["id"] if isinstance(y, dict) else y): return k
    return min(len(a), len(b))
k = first_diff(t_stream, t_p2)
(out / "prefix-diff.json").write_text(json.dumps({"stream_tokens": len(t_stream), "rerendered_turn2_tokens": len(t_p2),
    "first_differing_token": k, "stream_is_prefix_of_rerendered": k == len(t_stream),
    "stream_at_diff": t_stream[k:k+6], "rerendered_at_diff": t_p2[k:k+6],
    "stream_text_assumed": "turn-1 generation prompt + '<think>\\n' + reasoning_content + '\\n</think>\\n\\n' + content",
    "rerendered_turn2_text_tail": p2[len(p1)-50:len(p1)+400]}, indent=1, ensure_ascii=False))
print(json.dumps({"turn1_reasoning_chars": len(reasoning), "turn1_content_chars": len(content), "ab": ab, "first_diff": k, "stream_tokens": len(t_stream), "turn2_tokens": len(t_p2)}))
