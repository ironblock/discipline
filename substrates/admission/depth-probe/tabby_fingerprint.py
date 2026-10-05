#!/usr/bin/env python3
"""The admission fingerprint for a TabbyAPI line (#393). Writes fingerprint.json beside the cells: the canonical JSON (sorted keys, no
spaces) of engine, reasoning_state, serving_line, template and weights, its sha256, and the recipe sentence the floor record carries.
`engine` is the `engine_identity` tabby_identity.py computed from the engine_components form (not a llama.cpp manifest digest);
`template` is the model directory template file's digest; `weights` are the digests of the quantized shards and the drafter. Prints the
directory name (the first 12 hex characters). `--check DIR` recomputes and exits 1 on any difference."""
import argparse, hashlib, json, pathlib, sys
RECIPE = "sha256 of the canonical JSON below (sorted keys, no spaces); the directory is named by its first 12 hex characters"

def build(engine, reasoning_state, serving_line, template, weights):
    canon = json.dumps({"engine": engine, "reasoning_state": reasoning_state, "serving_line": serving_line, "template": template, "weights": weights},
                       sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return {"recipe": RECIPE, "canonical": canon, "sha256": hashlib.sha256(canon.encode()).hexdigest()}

def main(argv) -> int:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--identity", help="tabby_identity.py's JSON output"); p.add_argument("--reasoning-state"); p.add_argument("--serving-line", help="the redacted config's serving lines, as one sentence")
    p.add_argument("--kw", help="raw/kw.json (props_template_sha256)"); p.add_argument("--weights", nargs="+", metavar="NAME=SHA256"); p.add_argument("--out")
    p.add_argument("--check"); a = p.parse_args(argv)
    if a.check:
        d = pathlib.Path(a.check); fp = json.loads((d / "fingerprint.json").read_text()); ok = hashlib.sha256(fp["canonical"].encode()).hexdigest() == fp["sha256"] and d.name == fp["sha256"][:12]
        print("ok" if ok else "FAIL: fingerprint.json's sha256 or the directory name does not match"); return 0 if ok else 1
    ident = json.loads(pathlib.Path(a.identity).read_text()); kw = json.loads(pathlib.Path(a.kw).read_text())
    fp = build(ident["engine_identity"], a.reasoning_state, a.serving_line, kw["props_template_sha256"], dict(w.split("=", 1) for w in a.weights))
    pathlib.Path(a.out).write_text(json.dumps(fp, indent=1) + "\n"); print(fp["sha256"][:12]); return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
