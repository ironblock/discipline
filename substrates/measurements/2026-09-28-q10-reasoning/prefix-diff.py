#!/usr/bin/env python3
"""#117 Q10 item 3, corrected framing: the stream as generated is the turn-1
generation prompt (which already opens <think>) + reasoning_content + the closing
tag + content; compared token by token with the re-rendered turn-2 prompt."""
import json, os, re, sys, urllib.request, pathlib
out = pathlib.Path(sys.argv[1]); cmd = open(os.path.expanduser('~/setup/prod-cmdline.txt')).read()
host = re.search(r'--host (\S+)', cmd).group(1); port = int(re.search(r'--port (\d+)', cmd).group(1)); key = re.search(r'--api-key (\S+)', cmd).group(1)
def post(path, body):
    req = urllib.request.Request(f"http://{host}:{port}{path}", data=json.dumps(body).encode(), headers={"Content-Type": "application/json", "Authorization": f"Bearer {key}"})
    with urllib.request.urlopen(req, timeout=600) as r: return json.load(r)
t1 = json.load(open(out / "turn1.json")); R, C = t1["reasoning_content"], t1["content"]
Q1 = "A train leaves at 14:05 and arrives at 17:50. How long is the journey in minutes? Answer with the number and one short sentence."
Q2 = "Now convert that to hours and minutes."
p1 = post("/apply-template", {"messages": [{"role": "user", "content": Q1}], "add_generation_prompt": True})["prompt"]
p2 = post("/apply-template", {"messages": [{"role": "user", "content": Q1}, {"role": "assistant", "content": C, "reasoning_content": R}, {"role": "user", "content": Q2}], "add_generation_prompt": True})["prompt"]
tok = lambda t: post("/tokenize", {"content": t, "add_special": False, "with_pieces": True})["tokens"]
opens = p1.endswith("<think>\n")
candidates = {"closing '\\n</think>\\n\\n'": p1 + R + "\n</think>\n\n" + C, "closing '</think>\\n\\n'": p1 + R + "</think>\n\n" + C}
res = {"generation_prompt_opens_think": opens, "turn2_tokens": len(tok(p2))}
t2 = tok(p2)
for name, s in candidates.items():
    ts = tok(s); k = next((i for i, (a, b) in enumerate(zip(ts, t2)) if a["id"] != b["id"]), min(len(ts), len(t2)))
    res[name] = {"stream_tokens": len(ts), "first_differing_token": k, "stream_is_prefix_of_rerendered": k == len(ts),
                 "stream_at_diff": ts[k:k+5], "rerendered_at_diff": t2[k:k+5]}
(out / "prefix-diff.json").write_text(json.dumps(res, indent=1, ensure_ascii=False)); print(json.dumps(res, ensure_ascii=False)[:1500])
