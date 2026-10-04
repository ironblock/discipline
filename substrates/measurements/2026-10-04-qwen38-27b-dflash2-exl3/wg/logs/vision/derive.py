#!/usr/bin/env python3
"""#373's vision cell: the typed outcome, derived mechanically from the raw responses (ruled 5982304569, point 3).

Per request, in this order: `refused` when the status is not 2xx or the 2xx body is an error or carries no completion; `accepted` when the
reply's content holds the planted token (case-folded, whitespace removed); otherwise `answered-without-seeing`.
The cell: `accepted` only if all three are; `refused` if any is; else `answered-without-seeing`.
Usage: derive.py CELL_DIR -> prints JSON {per_request, word}. Stdlib only; the token comes from make_image.py."""
import hashlib, importlib.util, json, pathlib, re, sys

cell = pathlib.Path(sys.argv[1])
spec = importlib.util.spec_from_file_location("mi", cell / "make_image.py"); mi = importlib.util.module_from_spec(spec); spec.loader.exec_module(mi)
token, _ = mi.token_and_origin()
assert hashlib.sha256(token.encode()).hexdigest() == (cell / "token.sha256").read_text().strip()
norm = lambda s: re.sub(r"\s+", "", s).casefold()


def word(rec):
    if not 200 <= rec["status"] < 300:
        return "refused"
    try:
        body = json.loads(rec["body"])
    except ValueError:
        return "refused"
    if not isinstance(body, dict) or "error" in body or not body.get("choices"):
        return "refused"  # a 2xx whose body is an error, or no completion at all (#392's review, L3)
    content = (body.get("choices") or [{}])[0].get("message", {}).get("content") or ""
    return "accepted" if norm(token) in norm(content) else "answered-without-seeing"


per = [word(json.loads((cell / "responses" / f"r{i}.json").read_text())) for i in (1, 2, 3)]
cell_word = "accepted" if all(w == "accepted" for w in per) else "refused" if "refused" in per else "answered-without-seeing"
print(json.dumps({"per_request": per, "word": cell_word}))
