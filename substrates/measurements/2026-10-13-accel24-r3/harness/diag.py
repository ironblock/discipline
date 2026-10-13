#!/usr/bin/env python3
"""Pool-edge diagnostic (163,840-token pool, B = 2,048, DFlash2 num_draft_tokens 7), under tabby_crashdiag.py.

B: prompt ~158,700 (TabbyAPI admits: align2048(P-1+2048) = 161,792), min=max_tokens 4,000. Its requeued round
   after ~2,048 tokens starts above 159,745, aligns to 163,840 and needs 641 pages -> expected generator-fatal.
A: prompt ~160,800 (TabbyAPI admits: aligns to exactly 163,840), max_tokens 32,768 -> expected refused at enqueue.
Each request's outcome goes to OUT.jsonl; the hook writes the failing job's page counts to the server's metrics file."""
import json, os, sys, time
sys.argv = [sys.argv[0], "diag", "0", sys.argv[1]]
src = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "drive.py")).read()
g = {"__file__": os.path.abspath(__file__), "__name__": "diag"}
exec(src[:src.index("trunk = [")], g)
OUT = sys.argv[3]

def prompt(target):
    body = g["cut"](0, target - 60)
    return [{"role": "system", "content": g["SYSTEM"]}, {"role": "user", "content": body + "\n\nOK?"}]

def run(name, target, max_tokens, min_tokens=None):
    m = prompt(target)
    t0 = time.time()
    body = {"model": "x", "max_tokens": max_tokens, "messages": m, "chat_template_kwargs": {"enable_thinking": False}}
    if min_tokens:
        body["min_tokens"] = min_tokens
    try:
        _, rec = g["stream"](body)
        rec.pop("_reasoning", None)
    except Exception as e:
        rec = {"error": repr(e)[:400]}
    rec.update(case=name, target=target, max_tokens=max_tokens, min_tokens=min_tokens, t_start=t0, wall=time.time() - t0)
    with open(OUT, "a") as f:
        f.write(json.dumps(rec) + "\n")
    print(json.dumps(rec), flush=True)

run("B-requeue", 158700, 4000, 4000)
time.sleep(5)
run("A-admission", 160800, 32768)
