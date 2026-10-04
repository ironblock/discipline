#!/usr/bin/env python3
"""#165's fp32 path (amendment 6): laya-typed-decisions' own PyTorch forward on the laptop's CPU for every request
in requests.jsonl, through sidekick v0.6.0's reference tooling (tools/classifier_reference.py's laya_build and
laya_logits, convert_laya's loaders), so the inputs are built exactly as sidekick's oracle builds them. Per case:
the fp32 logits ("torch") and sidekick's ideal-fp16 oracle ("fp16"), the token count, the top-two margin, and
at_max_len (1,024 tokens: possibly truncated).
Usage: fp32_pass.py SIDEKICK_DIR MODEL_DIR SNAPSHOT_DIR LAYA_COMMON REQUESTS OUT [--limit N]"""
import json, sys, pathlib, hashlib, tomllib
SK, MODEL, SNAP, LAYA, REQ, OUT = map(pathlib.Path, sys.argv[1:7])
limit = int(sys.argv[sys.argv.index("--limit") + 1]) if "--limit" in sys.argv else None
sys.path.insert(0, str(SK / "tools"))
import classifier_reference as cr, convert_laya as cl
from transformers import AutoTokenizer
from sidekick_convert.backbones import modernbert
manifest = tomllib.loads((MODEL / "classifier.toml").read_text())
rl = cl.load_rl_common(SNAP, manifest["id"], LAYA)
cfg = json.loads((SNAP / "rl_agent_config.json").read_text())
assert cr.sha256(SNAP / "tokenizer" / "tokenizer.json") == cr.sha256(MODEL / "tokenizer.json"), "tokenizer differs"
tok = AutoTokenizer.from_pretrained(SNAP / "tokenizer")
reqs = [json.loads(l) for l in REQ.read_text().splitlines() if l.strip()][:limit]
cases = [{"id": f'{r["item"][:16]}:{r["field"]}', "tags": [], "input": r["request"]["input"],
          "candidate_labels": r["request"]["candidate_labels"], "question_type": r["request"]["question_type"],
          "instructions": r["request"]["instructions"], "gold": None, "head": None} for r in reqs]
cls = manifest["classify"]
cases = cr.laya_build(cases, rl, tok, cfg, cls["laya"]["default_instructions"], cls["max_labels"])
modernbert.install_patches()
dm = cl.load_decision_model(SNAP, rl, cfg)
logits = cr.laya_logits(dm, cases, cls["max_labels"])
with OUT.open("w") as f:
    for i, (r, c) in enumerate(zip(reqs, cases)):
        row = {"item": r["item"], "field": r["field"], "n_tokens": len(c["ids"]), "at_max_len": len(c["ids"]) == cfg["max_len"]}
        for name, L in logits.items():
            v = [float(x) for x in L[i, : c["k"]]]; s = sorted(v, reverse=True)
            row[name] = {"logits": v, "margin": s[0] - s[1]}
        f.write(json.dumps(row) + "\n")
print(len(cases), "cases ->", OUT, "| oracles:", sorted(logits), "| max_len", cfg["max_len"])
