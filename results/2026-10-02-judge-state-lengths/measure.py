#!/usr/bin/env python3
"""#165: the length of the archive's judge states in laya's own tokens, against each laya model's state budget.

A judge state is what a Sonnet judge saw for one item: the nomination (`entry`, or `note` in prompt v1's record) and
the turn's REASONING and PROSE, composed under the prompt's labels exactly as translation-table.json composes a
control's state. laya's input is [CLS] "<type> question: <instructions>" [SEP] [MASK] " "+opt0 ... [MASK] " "+optN
[SEP] <state> [SEP] (Sidekick's port of laya's sequence builder, relayed by Dispatch 2026-10-02): each fragment
tokenized alone without specials, a literal "[MASK]" in text read as a space, each option capped at 48 tokens, the
question and options capped at head_max_len, and the state given max_len minus the head minus the closing [SEP].

  measure.py build REPO_ROOT    -> writes batches.json (every judge batch consumed, by digest) and counts.jsonl
                                   (one row per unique state: its sha256, its first record, its token count)
  measure.py report             -> writes report.json from counts.jsonl, translation-table.json and tokenizer.json
`build` needs the `tokenizers` library (0.23.2 here) and the repository; `report` needs neither."""
import hashlib, json, pathlib, sys
HERE = pathlib.Path(__file__).resolve().parent
MODELS = {"laya-en": {"max_len": 512, "head_max_len": 192}, "laya-typed-decisions": {"max_len": 1024, "head_max_len": 256}}
OPTION_CAP = 48
sha = lambda b: hashlib.sha256(b).hexdigest()

def state_of(it):
    lab, nom = ("ENTRY", it["entry"]) if "entry" in it else ("NOTE", it.get("note") or "")
    return f"{lab}: {nom}\nREASONING: {it.get('reasoning') or ''}\nPROSE: {it.get('prose') or ''}"

def tokens(tok, s):
    return len(tok.encode(s.replace("[MASK]", " "), add_special_tokens=False).ids)

def heads(tok):
    t = json.loads((HERE / "translation-table.json").read_text())
    out = {}
    for q in t["rows"][0]["questions"]:
        qt = tokens(tok, f'{q["type"]} question: {q["question"]}')
        opts = [min(OPTION_CAP, tokens(tok, " " + f'{o["key"]}: {o["label"]}')) for o in q["options"]]
        out[q["field"]] = {"question_tokens": qt, "option_tokens": opts, "head_part": qt + sum(1 + o for o in opts),
                           "head_total": 1 + qt + 1 + sum(1 + o for o in opts) + 1}
    return out

def build(root):
    from tokenizers import Tokenizer
    tok = Tokenizer.from_file(str(HERE / "tokenizer.json"))
    batches, seen, rows = [], {}, []
    for d in sorted((pathlib.Path(root) / "results").glob("*/judge/batches")):
        for f in sorted(d.glob("batch-*.json")):
            b = f.read_bytes(); batches.append({"path": str(f.relative_to(root)), "sha256": sha(b)})
            for it in json.loads(b).get("items", []):
                s = state_of(it); k = sha(s.encode())
                if k not in seen:
                    seen[k] = 1; rows.append({"state_sha256": k, "record": d.parent.parent.name, "tokens": tokens(tok, s)})
    (HERE / "batches.json").write_text(json.dumps(batches, indent=1) + "\n")
    (HERE / "counts.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
    (HERE / "heads.json").write_text(json.dumps(heads(tok), indent=1) + "\n")

def report():
    rows = [json.loads(l) for l in (HERE / "counts.jsonl").read_text().splitlines() if l.strip()]
    hd = json.loads((HERE / "heads.json").read_text())
    L = sorted(r["tokens"] for r in rows); q = lambda p: L[int(p * (len(L) - 1))]
    out = {"tokenizer_sha256": sha((HERE / "tokenizer.json").read_bytes()), "unique_states": len(L),
           "tokens": {"p50": q(.5), "p90": q(.9), "p95": q(.95), "max": L[-1]}, "heads": hd, "models": {}}
    for m, c in MODELS.items():
        per = {}
        for field, h in hd.items():
            budget = c["max_len"] - h["head_total"] - 1
            over = sum(1 for x in L if x > budget)
            per[field] = {"head_within_cap": h["head_part"] <= c["head_max_len"], "state_budget": budget,
                          "truncated": over, "truncated_share": round(over / len(L), 4), "p90_within": q(.9) <= budget}
        out["models"][m] = {**c, "questions": per, "p90_within_every_question": all(v["p90_within"] for v in per.values())}
    out["variant"] = "laya-en" if out["models"]["laya-en"]["p90_within_every_question"] else "laya-typed-decisions"
    (HERE / "report.json").write_text(json.dumps(out, indent=1) + "\n")
    return out

if __name__ == "__main__":
    if sys.argv[1:2] == ["build"]: build(sys.argv[2])
    elif sys.argv[1:2] == ["report"]: print(json.dumps(report()["variant"]))
    else: raise SystemExit(__doc__)
