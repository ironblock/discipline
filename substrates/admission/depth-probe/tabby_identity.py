#!/usr/bin/env python3
"""A TabbyAPI + ExLlamaV3 line's identity, re-read from the host (#393, #337): the ten components of the registry's
python-torch-exllamav3 form in the form's order, and their composite (check-fingerprints.py's composite_identity). Run with the SERVING
venv's python, on the host. Digests of files the host holds (wheel, config copies, freeze file) are taken here; the installed set is
cross-checked against the freeze file name by name, so a freeze file that does not describe this venv is refused, and the live compiled
extension is hashed where it sits. Prints one JSON object; with --expect, exits 1 and names every component that differs."""
from __future__ import annotations
import argparse, re, hashlib, importlib.metadata as md, json, pathlib, subprocess, sys

def sha(p) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""): h.update(b)
    return h.hexdigest()

def composite(order, comps) -> str:
    return hashlib.sha256(json.dumps([[n, comps[n]] for n in order], separators=(",", ":"), ensure_ascii=False).encode()).hexdigest()

def norm(n: str) -> str:
    return re.sub(r"[-_.]+", "-", n.strip().lower())

def freeze_mismatch(freeze_file: str) -> list[str]:
    """`name==version` lines must match the installed version; `name @ url` lines (installed from a wheel or URL) must be installed;
    `-e` lines (the editable checkout) are skipped, as the serving commit is its own component."""
    want, direct = {}, set()
    for l in pathlib.Path(freeze_file).read_text().splitlines():
        if l.startswith("-e") or not l.strip() or l.startswith("#"): continue
        if "==" in l: n, v = l.split("==", 1); want[norm(n)] = v.strip()
        elif " @ " in l: direct.add(norm(l.split(" @ ", 1)[0]))
    have = {norm(d.metadata["Name"]): d.version for d in md.distributions() if "_vendor" not in str(getattr(d, "_path", ""))}
    bad = [f"{n}: file {v}, installed {have.get(n)}" for n, v in want.items() if have.get(n) != v]
    bad += [f"{n}: in the file by URL, not installed" for n in direct if n not in have]
    bad += [f"{n}: installed {v}, not in the file" for n, v in have.items() if n not in want and n not in direct and not n.startswith("tabbyapi")]
    return bad

def read(a, order) -> dict:
    import torch
    site = pathlib.Path(__import__("exllamav3").__file__).resolve().parent.parent
    ext = sorted(site.glob("exllamav3_ext*.so"))
    if len(ext) != 1: raise SystemExit(f"tabby_identity: expected one compiled extension in {site}, found {len(ext)}")
    drv = subprocess.check_output(["nvidia-smi", "--query-gpu=driver_version", "--format=csv,noheader"], text=True).split()[0]
    bad = freeze_mismatch(a.freeze)
    if bad: raise SystemExit("tabby_identity: the freeze file does not describe this venv:\n  " + "\n  ".join(bad[:10]))
    return {"serving_commit": subprocess.check_output(["git", "-C", a.tabby_dir, "rev-parse", "HEAD"], text=True).strip(),
            "exllamav3_wheel_sha256": sha(a.wheel), "exllamav3_extension_sha256": sha(ext[0]),
            "torch": torch.__version__, "cuda_runtime": torch.version.cuda, "nvidia_driver": drv,
            "python": ".".join(map(str, sys.version_info[:3])), "package_set_sha256": sha(a.freeze),
            "config_redacted_sha256": sha(a.config_redacted), "config_redaction_diff_sha256": sha(a.config_redaction_diff)}

def main() -> int:
    ap = argparse.ArgumentParser()
    for n in ("tabby-dir", "wheel", "freeze", "config-redacted", "config-redaction-diff", "registry"): ap.add_argument("--" + n, required=True)
    ap.add_argument("--expect", help="a JSON object of component -> value (and optionally engine_identity) to compare against")
    a = ap.parse_args()
    import tomllib
    order = tomllib.loads(pathlib.Path(a.registry).read_text())["engine_component_form"]["python-torch-exllamav3"]["components"]
    comps = read(a, order); out = {"components": comps, "engine_identity": composite(order, comps)}
    print(json.dumps(out, indent=1))
    if a.expect:
        want = json.loads(pathlib.Path(a.expect).read_text()); diff = [n for n in order if n in want and want[n] != comps[n]]
        if "engine_identity" in want and want["engine_identity"] != out["engine_identity"]: diff.append("engine_identity")
        if diff: print("DIFFERS: " + ", ".join(diff), file=sys.stderr); return 1
    return 0

if __name__ == "__main__":
    sys.exit(main())
