#!/usr/bin/env python3
"""Recompute both ties from committed files only; exit 1 if either fails.

beellama: every file of the disk read (exe and 29 libraries) equals the same-named regular member of the release
tarball, whose sha256 (beellama-tarball-members.json, hashed on the host) must be the registry's
engine_release_tarball_sha256.
candidate: every file the 2026-09-29 engine manifest hashed (exe and the 8 libraries the server loads, by soname)
equals the disk read's file of that soname.
registry: every substrate's engine_libraries table equals the read it cites, and no input is empty."""
import hashlib, json, pathlib, re, sys, tomllib
here = pathlib.Path(__file__).resolve().parent
root = here.parents[2]
reg = tomllib.loads((root / "substrates/registry.toml").read_text())["substrate"]
ok = True

bee = json.loads((here / "beellama-preview-v0.3.2-disk.json").read_text())
tar = json.loads((here / "beellama-tarball-members.json").read_text())
pinned = reg["accel24-beellama-qwen27b-q4kxl"]["engine_release_tarball_sha256"]
files = dict(bee["libraries"], **{"llama-server": bee["exe"]})
bad = sorted(k for k, v in files.items() if tar["regular_members"].get(k) != v)
# both ways: every shared object the release ships is in the read, so a read that lost a library cannot pass
shipped = {k for k in tar["regular_members"] if re.search(r"\.so(\.\d+)*$", k)}
bad += sorted(f"{k} (shipped, not read)" for k in shipped - set(bee["libraries"]))
# the floor's process read is the same release
proc = json.loads((here / "beellama-floor-process.json").read_text())
if (proc["exe"], proc["libraries"]) != (bee["exe"], bee["libraries"]):
    bad.append("the floor's process read differs from the disk read")
print(f"beellama: tarball {'is' if tar['tarball_sha256'] == pinned else 'IS NOT'} the pinned release; "
      f"{len(files) - len(bad)} of {len(files)} files equal its members{'; differing: ' + ', '.join(bad) if bad else ''}")
ok &= tar["tarball_sha256"] == pinned and not bad

cand = json.loads((here / "accel24-llamacpp-candidate-disk.json").read_text())
manifest = root / "substrates/admission/accel24-llamacpp-qwen38-27b-iq3s/9d84a552fc94/raw/engine-manifest.txt"
# the manifest is the one the candidate's admission pinned: its sha256 is that record's engine component
pin = json.loads(json.loads((manifest.parents[1] / "fingerprint.json").read_text())["canonical"])["engine"]
if hashlib.sha256(manifest.read_bytes()).hexdigest() != pin:
    print("candidate: the engine manifest is not the one the admission record pins"); ok = False
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
ok &= not bad and len(rows) > 1 and any(r[2] == "llama-server" for r in rows)

# Each registry table is the read it cites, so a read edited after the fact (or a table) cannot pass unseen.
cites = {"accel24-beellama-qwen27b-q4kxl": "beellama-floor-process.json",
         "cpu-beellama-qwen3-1p7b-q4km": "beellama-preview-v0.3.2-disk.json",
         "accel24-llamacpp-qwen38-27b-iq3s": "accel24-llamacpp-candidate-disk.json",
         "ada48-llamacpp-qwen38flashnext-q20": "ada48-running-2026-10-01-process.json"}
for name, read in cites.items():
    d = json.loads((here / read).read_text()); r = reg[name]
    same = (r["engine_identity"], r["engine_libraries"], r["engine_fingerprint"], r["engine_libraries_read"]) == \
           (d["exe"], d["libraries"], d["engine_fingerprint"], d["read"]) and d["libraries"]
    print(f"registry: {name} {'equals' if same else 'DOES NOT EQUAL'} {read}")
    ok &= bool(same)
# the substrate's engine fields name the 2026-10-01 build since its instance row (#143); the 2026-09-28 instance keeps
# what it ran, which is the disk read of that build
old = next(i for i in reg["ada48-llamacpp-qwen38flashnext-q20"]["instance"] if i["id"] == "2026-09-28")
dd = json.loads((here / "ada48-2026-09-28-build-disk.json").read_text())
same = (old.get("engine_identity"), old.get("engine_fingerprint")) == (dd["exe"], dd["engine_fingerprint"])
print(f"registry: ada48's 2026-09-28 instance {'equals' if same else 'DOES NOT EQUAL'} ada48-2026-09-28-build-disk.json")
ok &= same
ok &= len(files) > 1 and tar["regular_members"] != {}
sys.exit(0 if ok else 1)
