import json, sys, re, time, threading, urllib.request
PORT=int(sys.argv[1]); TAG=sys.argv[2]
text = re.sub(r"\n+", " ", open("$HOME/setup/wikitext-2-raw/wiki.test.raw", errors="replace").read())
def post(path, payload, timeout=3600):
    req = urllib.request.Request(f"http://127.0.0.1:{PORT}{path}", data=json.dumps(payload).encode(),
                                 headers={"Content-Type": "application/json"})
    return json.load(urllib.request.urlopen(req, timeout=timeout))
try:
    cpt = len(text[:40000]) / len(post("/tokenize", {"content": text[:40000]})["tokens"])
except Exception:
    cpt = len(text[:40000]) / len(post("/v1/token/encode", {"text": text[:40000], "add_bos_token": False})["tokens"])
PROMPT = "\n\nContinue the passage above in the same register for several paragraphs."
def erase():
    for s in range(4):
        try: post(f"/slots/{s}?action=erase", {})
        except Exception as e: pass
def one(i, depth, out):
    off = i*60000
    body={"messages":[{"role":"user","content":text[off:off+int(depth*cpt)]+PROMPT}],"max_tokens":256,"temperature":0}
    try:
        t=post("/v1/chat/completions", body).get("timings",{})
        out[i]={"tok_s":round(t.get("predicted_per_second",0),2),"n":t.get("predicted_n"),"dn":t.get("draft_n"),
                "da":t.get("draft_n_accepted"),"pn":t.get("prompt_n"),"pp":round(t.get("prompt_per_second",0))}
    except Exception as e: out[i]={"error":str(e)[:120]}
import os
PH=[tuple(int(x) for x in p.split(":")) for p in os.environ.get("PHASES","200000:1,100000:1,100000:2").split(",")]
for depth, c in PH:
    if c==1: erase()
    for rep in range(3):   # rep 0 = warm-up (prefill), reps 1-2 = cache-warm decode
        out={}; t0=time.time()
        th=[threading.Thread(target=one,args=(i,depth,out)) for i in range(c)]
        [x.start() for x in th]; [x.join() for x in th]
        wall=time.time()-t0; ok=[o for o in out.values() if "tok_s" in o]
        dn=sum(o["dn"] or 0 for o in ok)
        print(json.dumps({"tag":TAG,"depth":depth,"conc":c,"rep":rep,"wall_s":round(wall,1),
            "per_req":[o["tok_s"] for o in ok],"agg_wall":round(sum(o["n"] or 0 for o in ok)/wall,1),
            "pn":[o["pn"] for o in ok],"pp":[o["pp"] for o in ok],
            "a":round(sum(o["da"] or 0 for o in ok)/dn,3) if dn else None,
            "errors":[o["error"] for o in out.values() if "error" in o]}), flush=True)
