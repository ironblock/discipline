#!/usr/bin/env python3
"""Write the depth probe's corpus manifest (#143, as ruled): this repository's own tree at a pinned
commit -- every Rust file under diet/src/ as a file read, every directory under it as a listing, and
the diffs of diet/src/ in the commit's last N first-parent commits -- each entry with the sha256 of
the exact bytes the probe will read, so the corpus recomputes by digest. The counter-examples file is
pinned alongside. Usage: make_corpus.py REPO COMMIT COUNTEREXAMPLES_JSON OUT [--diffs N]"""
import argparse, hashlib, json, pathlib, subprocess

def git(repo, *args):
    return subprocess.run(["git", "-C", repo, *args], capture_output=True, check=True).stdout

def main():
    ap = argparse.ArgumentParser(); ap.add_argument("repo"); ap.add_argument("commit"); ap.add_argument("counterexamples")
    ap.add_argument("out"); ap.add_argument("--diffs", type=int, default=12); ap.add_argument("--root", default="diet/src")
    a = ap.parse_args()
    commit = git(a.repo, "rev-parse", a.commit).decode().strip()
    paths = [p for p in git(a.repo, "ls-tree", "-r", "--name-only", commit, a.root + "/").decode().splitlines() if p.endswith(".rs")]
    sha = lambda b: hashlib.sha256(b).hexdigest()
    reads = [{"path": p, "label": f"cat {p}", "sha256": sha(git(a.repo, "show", f"{commit}:{p}"))} for p in paths]
    dirs = sorted({str(pathlib.PurePosixPath(p).parent) for p in paths})
    listings = [{"path": d, "label": f"ls {d}", "sha256": sha(git(a.repo, "ls-tree", "--name-only", commit, d + "/"))} for d in dirs]
    shas = git(a.repo, "rev-list", "--first-parent", f"-{a.diffs}", commit, "--", a.root).decode().split()
    diffs = []
    for c in shas:
        b = git(a.repo, "diff", f"{c}^1", c, "--", a.root)  # the first-parent difference: a merge's own change
        if b.strip():
            diffs.append({"commit": c, "path": a.root, "label": f"git diff {c[:7]}^1 {c[:7]} -- {a.root}", "sha256": sha(b)})
    out = pathlib.Path(a.out)
    ce = pathlib.Path(a.counterexamples)
    manifest = {"source": "git", "repo": str(pathlib.Path(a.repo).resolve().relative_to(out.parent.resolve(), walk_up=True)),
                "commit": commit, "reads": reads, "listings": listings, "diffs": diffs,
                "counterexamples": str(ce.resolve().relative_to(out.parent.resolve(), walk_up=True)),
                "counterexamples_sha256": sha(ce.read_bytes()), "written_by": "make_corpus.py"}
    out.write_text(json.dumps(manifest, indent=1) + "\n")
    print(f"{len(reads)} reads, {len(listings)} listings, {len(diffs)} diffs at {commit[:12]}")

if __name__ == "__main__":
    main()
