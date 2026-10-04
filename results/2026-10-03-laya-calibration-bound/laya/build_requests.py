#!/usr/bin/env python3
"""#165's laya pass inputs: one /v1/classify request per (sampled item, judge question), rendered by the translation
table (its digest declared on #165 before the pass). T = 1: no `calibration` field. Usage: build_requests.py REPO_ROOT OUT"""
import hashlib, importlib.util, json, pathlib, sys
ROOT, OUT = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2])
RJ = pathlib.Path.home() / "diet-inference-runs/165/rejudge/out"
spec = importlib.util.spec_from_file_location("m", ROOT / "results/2026-10-02-judge-state-lengths/measure.py"); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
table_p = ROOT / "results/2026-10-02-judge-state-lengths/translation-table.json"
table = json.loads(table_p.read_text()); questions = table["rows"][0]["questions"]
sample = json.loads((RJ / "sample.json").read_text())
state_of = {}
for rec in sorted({s["record"] for s in sample}):
    for bf in (ROOT / "results" / rec / "judge" / "batches").glob("batch-*.json"):
        for it in json.loads(bf.read_text())["items"]:
            st = m.state_of(it); state_of.setdefault(m.sha(st.encode()), st)
rows = []
for s in sample:
    st = state_of[s["state_sha256"]]
    for q in questions:
        rows.append({"item": s["state_sha256"], "field": q["field"],
                     "request": {"model": "laya-typed-decisions", "input": st, "question_type": q["type"],
                                 "instructions": q["question"], "candidate_labels": [f'{o["key"]}: {o["label"]}' for o in q["options"]]}})
OUT.write_text("".join(json.dumps(r, ensure_ascii=False) + "\n" for r in rows))
print(len(rows), "requests;", len(sample), "items;", [q["field"] for q in questions],
      "| table sha256", hashlib.sha256(table_p.read_bytes()).hexdigest()[:16], "| requests sha256", hashlib.sha256(OUT.read_bytes()).hexdigest()[:16])
