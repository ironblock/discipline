# calcheck_sc.py: does any 200-character window of the KLD corpora occur verbatim in the self-calibration trace of
# turboderp/Qwen3.8-27B-exl3 (cal_trace.json: token ids of 217 prompt/response rows)? Decodes every row with the
# model's own tokenizer.json, then searches windows every 1,000 characters of each corpus.
import json, sys
from tokenizers import Tokenizer
tok = Tokenizer.from_file(sys.argv[1]); d = json.load(open(sys.argv[2]))
text = "\n".join(tok.decode(r[k], skip_special_tokens=False) for r in d["rows"] for k in ("input_ids", "response_ids"))
print(f"cal_trace rows={len(d['rows'])} meta={d['meta']} decoded_chars={len(text)}")
for c in sys.argv[3:]:
    t = open(c, encoding="utf-8", errors="replace").read()
    w = [t[i:i + 200] for i in range(0, len(t) - 200, 1000)]
    print(f"corpus {c.split('/')[-1]} windows={len(w)} found_in_cal_trace={sum(1 for x in w if x in text)}")
