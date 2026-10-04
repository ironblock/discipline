#!/usr/bin/env bash
# The vision cell's own check (#373, ruled 5982304569). Every file cell.toml cites must hash as it says; the image and
# its token digest must rebuild byte for byte from make_image.py's seed; the request must carry exactly that image,
# the registered sampler and cell.toml's max_tokens, and never the token; the word must re-derive from responses/ by
# derive.py, request by request; the projector must be the one this directory's fingerprint and the registry pin; and
# the registry's `vision` for the substrate must equal the derived word. Stdlib only.
# Exit 0 when all hold, 1 when any fails, 2 when a step cannot run.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd -- "$here/../../../../.." && pwd)"
tmp="$(mktemp -d)" || exit 2
trap 'rm -rf "$tmp"' EXIT
python3 -B "$here/make_image.py" "$tmp" >/dev/null || { echo "vision: make_image.py failed"; exit 2; }
derived="$(cd "$here" && python3 -B derive.py .)" || { echo "vision: derive.py failed"; exit 2; }
python3 -B - "$here" "$root" "$tmp" "$derived" <<'PY' || exit 1
import base64, hashlib, importlib.util, json, pathlib, sys, tomllib
here, root, tmp, derived = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]), json.loads(sys.argv[4])
cell = tomllib.loads((here / "cell.toml").read_text())
bad = 0
def fail(m):
    global bad; bad += 1; print(f"vision: {m}")
sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
for path, want in sorted(cell["raw"].items()):
    p = here / path
    if not p.is_file(): fail(f"{path} is cited and absent"); continue
    if sha(p) != want: fail(f"{path} hashes to {sha(p)[:12]}..., cell.toml says {want[:12]}...")
for name in ("image.png", "token.sha256"):
    if (tmp / name).read_bytes() != (here / name).read_bytes(): fail(f"{name} does not rebuild from make_image.py's seed")
if sha(here / "image.png") != cell["image_sha256"]: fail("image.png is not cell.toml's image_sha256")
if (here / "token.sha256").read_text().strip() != cell["token_sha256"]: fail("token.sha256 is not cell.toml's token_sha256")
req = json.loads((here / "request.json").read_text())
# the request's whole shape, not only its first message: exactly these keys, one user message of exactly an image part
# and the fixed prompt, so no second message, stop string or grammar can carry the token (#392's review, M1)
spec_rc = importlib.util.spec_from_file_location("run_cell", here / "run_cell.py"); rc = importlib.util.module_from_spec(spec_rc); spec_rc.loader.exec_module(rc)
want_keys = {"messages", "max_tokens", "stream", *rc.SAMPLER}
if set(req) != want_keys: fail(f"request.json's keys are {sorted(req)}, not exactly {sorted(want_keys)}")
msgs = req.get("messages") or [{}]
parts = msgs[0].get("content") or [{}, {}]
if len(msgs) != 1 or msgs[0].get("role") != "user" or len(parts) != 2 or parts[0].get("type") != "image_url" \
        or parts[1] != {"type": "text", "text": rc.PROMPT} or req.get("stream") is not False:
    fail("request.json is not one user message of exactly the image part and run_cell.py's prompt, unstreamed")
# and byte for byte: the request run_cell.py builds from image.png, rebuilt here, must be request.json's exact bytes,
# so no key inside the message or the image part can ride along (#392's delta review, L5)
rebuilt = {"messages": [{"role": "user", "content": [
    {"type": "image_url", "image_url": {"url": "data:image/png;base64," + base64.b64encode((here / "image.png").read_bytes()).decode()}},
    {"type": "text", "text": rc.PROMPT}]}], "max_tokens": rc.MAX_TOKENS, "stream": False, **rc.SAMPLER}
if json.dumps(rebuilt, separators=(",", ":")).encode() != (here / "request.json").read_bytes():
    fail("request.json is not byte for byte the request run_cell.py builds from image.png")
if parts[0]["image_url"]["url"] != "data:image/png;base64," + base64.b64encode((here / "image.png").read_bytes()).decode():
    fail("request.json does not carry image.png as its data: URI")
if req.get("max_tokens") != cell["max_tokens"]: fail(f"request.json's max_tokens {req.get('max_tokens')} is not cell.toml's {cell['max_tokens']}")
sub = tomllib.loads((root / "substrates/registry.toml").read_text())["substrate"][cell["substrate"]]
card = dict(kv.strip().split(" ") for kv in sub["sampler_card"].split(","))
for k, v in card.items():
    if float(req.get(k, "nan")) != float(v): fail(f"request.json's {k} is {req.get(k)!r}, the registered sampler_card says {v}")
if "chat_template_kwargs" in req or "model" in req: fail("request.json sets chat_template_kwargs or a model; the line's own reasoning and model are the regime")
import re
spec = importlib.util.spec_from_file_location("make_image", here / "make_image.py"); mi = importlib.util.module_from_spec(spec); spec.loader.exec_module(mi)
token, _ = mi.token_and_origin()
if hashlib.sha256(token.encode()).hexdigest() != cell["token_sha256"]: fail("make_image.py's token is not cell.toml's token_sha256")
norm = lambda s: re.sub(r"\s+", "", s).casefold()
if norm(token) in norm(json.dumps({k: v for k, v in req.items() if k != "messages"}) + json.dumps(parts[1:])):
    fail("the request carries the planted token outside the image")
# the cell's file set is closed: every file under vision/ is cited by cell.toml or is the cell's own text, and
# responses/ holds exactly n responses (#392's review, L2)
own = {"cell.toml", "recompute.sh", "README.md"}
present = {p.relative_to(here).as_posix() for p in here.rglob("*") if p.is_file() and "__pycache__" not in p.parts}
if present != set(cell["raw"]) | own: fail(f"files under vision/ not cited by cell.toml: {sorted(present - set(cell['raw']) - own)}; cited and absent: {sorted(set(cell['raw']) - present)}")
if sorted(p.name for p in (here / "responses").iterdir() if p.is_file()) != [f"r{i}.json" for i in range(1, cell["n"] + 1)]:
    fail(f"responses/ does not hold exactly r1..r{cell['n']}")
if derived["per_request"] != cell["per_request"] or derived["word"] != cell["word"]:
    fail(f"cell.toml says {cell['word']!r} {cell['per_request']}; responses/ derive {derived['word']!r} {derived['per_request']}")
if len(derived["per_request"]) != cell["n"]: fail(f"n is {cell['n']}, responses/ hold {len(derived['per_request'])}")
fp = json.loads(json.loads((here.parent / "fingerprint.json").read_text())["canonical"])
if not (cell["projector_sha256"] == fp["weights"]["projector"] == sub["weights_projector"]):
    fail("the projector is not the one this directory's fingerprint and the registry pin")
if sub.get("vision") != derived["word"]:
    fail(f"the registry's vision for {cell['substrate']} is {sub.get('vision')!r}; the cell derives {derived['word']!r}")
if bad: sys.exit(1)
print(f"vision: {len(cell['raw'])} cited file(s) hash as cell.toml says, the image rebuilds from its seed, and the word "
      f"{derived['word']!r} re-derives from {cell['n']} responses and equals the registry's vision")
PY
