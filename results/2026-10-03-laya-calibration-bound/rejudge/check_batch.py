#!/usr/bin/env python3
"""Check one judged batch: the model off the instance's transcript, the tools it used, the output's shape, the keyed
controls. Appends a row to judge/run-log.jsonl. Usage: check_batch.py NN AGENT_ID"""
import json, pathlib, sys, datetime, hashlib
nn, agent = sys.argv[1], sys.argv[2]
W = pathlib.Path.home() / "diet-inference-runs/165/rejudge/work/judge"
T = pathlib.Path.home() / ".claude/projects/<project>/<session>/subagents" / f"agent-{agent}.jsonl"
models, tools = set(), []
for line in T.read_text().splitlines():
    try: d = json.loads(line)
    except Exception: continue
    msg = d.get("message") or {}
    if msg.get("model"): models.add(msg["model"])
    for c in msg.get("content") or []:
        if isinstance(c, dict) and c.get("type") == "tool_use":
            inp = c.get("input", {}); tools.append((c["name"], inp.get("file_path") or inp.get("path") or inp.get("command", "")[:60]))
allowed = {str(W / "prompt.md"), str(W / f"batches/batch-{nn}.json"), str(W / f"verdicts-{nn}.json")}
stray = [t for t in tools if t[0] not in ("Read", "Write") or t[1] not in allowed]
batch = json.loads((W / f"batches/batch-{nn}.json").read_text())["items"]
key = json.loads((W / "key.json").read_text())[str(int(nn))]
reasons = []
if models != {"claude-sonnet-5"}: reasons.append(f"model {sorted(models)}")
if stray: reasons.append(f"stray tool use {stray}")
vf = W / f"verdicts-{nn}.json"
try:
    v = json.loads(vf.read_text())
    if not isinstance(v, list) or len(v) != len(batch): reasons.append(f"length {len(v) if isinstance(v, list) else type(v).__name__} != {len(batch)}")
    elif [x.get("id") for x in v] != [it["id"] for it in batch]: reasons.append("ids not in batch order")
    else:
        got = {x["id"]: x for x in v}
        miss = [cid for cid, ans in key.items() for f in ("verdict", "edit") if ans.get(f) is not None and got[cid].get(f) != ans.get(f)]  # a field ruled None is not keyed
        if miss: reasons.append(f"keyed control missed: {miss}")
except Exception as e:
    reasons.append(f"malformed: {e}")
row = {"batch": nn, "agent": agent, "models": sorted(models), "tools": len(tools), "status": "void" if reasons else "accepted",
       "reasons": reasons, "verdicts_sha256": hashlib.sha256(vf.read_bytes()).hexdigest() if vf.exists() else None,
       "checked": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}
with open(W / "run-log.jsonl", "a") as f: f.write(json.dumps(row) + "\n")
print(json.dumps(row))
