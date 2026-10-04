import json, re, sys, threading, urllib.request
PORT, KEY = int(sys.argv[1]), sys.argv[2]
text = re.sub(r"\n+", " ", open("$HOME/setup/wikitext-2-raw/wiki.test.raw", errors="replace").read())
H = {"Content-Type": "application/json", "Authorization": f"Bearer {KEY}"}
def post(path, payload):
    return json.load(urllib.request.urlopen(urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", data=json.dumps(payload).encode(), headers=H), timeout=1800))
try:
    cpt = len(text[:40000]) / len(post("/tokenize", {"content": text[:40000]})["tokens"])
except Exception:
    cpt = len(text[:40000]) / len(post("/v1/token/encode", {"text": text[:40000], "add_bos_token": False})["tokens"])
P = "\n\nContinue the passage above in the same register for several paragraphs."
def ask(i, out):
    d = post("/v1/chat/completions", {"messages": [{"role": "user", "content": text[i*60000:i*60000+int(10000*cpt)] + P}], "max_tokens": 120, "temperature": 0, "chat_template_kwargs": {"enable_thinking": False}})
    t = d.get("timings", {}); out[i] = (d["choices"][0]["message"].get("content") or "", round(t["draft_n_accepted"]/t["draft_n"],2) if t.get("draft_n") else None)
for mode in ("solo", "conc"):
    out = {}
    if mode == "solo":
        for i in range(4): ask(i, out)
    else:
        th = [threading.Thread(target=ask, args=(i, out)) for i in range(4)]; [x.start() for x in th]; [x.join() for x in th]
    for i in range(4): print(mode, i, "a=%s" % out[i][1], repr(out[i][0][:150]))
