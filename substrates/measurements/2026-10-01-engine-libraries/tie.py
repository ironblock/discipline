#!/usr/bin/env python3
"""Recompute both ties from committed files only; exit 1 if either fails.

beellama: every file of the disk read (exe and 29 libraries) equals the same-named regular member of the release
tarball, whose sha256 (beellama-tarball-members.json, hashed on the host) must be the registry's
engine_release_tarball_sha256.
candidate: every file the 2026-09-29 engine manifest hashed (exe and the 8 libraries the server loads, by soname)
equals the disk read's file of that soname."""
import json, pathlib, sys, tomllib
here = pathlib.Path(__file__).resolve().parent
root = here.parents[2]
reg = tomllib.loads((root / "substrates/registry.toml").read_text())["substrate"]
ok = True

bee = json.loads((here / "beellama-preview-v0.3.2-disk.json").read_text())
tar = json.loads((here / "beellama-tarball-members.json").read_text())
pinned = reg["accel24-beellama-qwen27b-q4kxl"]["engine_release_tarball_sha256"]
files = dict(bee["libraries"], **{"llama-server": bee["exe"]})
bad = sorted(k for k, v in files.items() if tar["regular_members"].get(k) != v)
print(f"beellama: tarball {'is' if tar['tarball_sha256'] == pinned else 'IS NOT'} the pinned release; "
      f"{len(files) - len(bad)} of {len(files)} files equal its members{'; differing: ' + ', '.join(bad) if bad else ''}")
ok &= tar["tarball_sha256"] == pinned and not bad

cand = json.loads((here / "accel24-llamacpp-candidate-disk.json").read_text())
manifest = root / "substrates/admission/accel24-llamacpp-qwen38-27b-iq3s/9d84a552fc94/raw/engine-manifest.txt"
rows = [l.split() for l in manifest.read_text().splitlines() if l.startswith("cand_file ")]
bad, matched = [], set()
for _, digest, name in rows:
    if name == "llama-server":
        bad += [] if digest == cand["exe"] else [name]
        continue
    real = [k for k in cand["libraries"] if k == name or k.startswith(name + ".")]
    if len(real) != 1 or cand["libraries"][real[0]] != digest:
        bad.append(name)
    else:
        matched.add(real[0])
rest = sorted(set(cand["libraries"]) - matched)
print(f"candidate: {len(rows) - len(bad)} of {len(rows)} files of the 2026-09-29 manifest equal the disk read"
      f"{'; differing: ' + ', '.join(bad) if bad else ''}; not in the manifest: {', '.join(rest)}")
ok &= not bad
sys.exit(0 if ok else 1)
