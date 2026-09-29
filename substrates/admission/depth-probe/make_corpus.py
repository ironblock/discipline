#!/usr/bin/env python3
"""Write the depth probe's corpus manifest (#143, as ruled and reconciled): named repositories, each at a
pinned commit -- every Rust file under its root as a file read, every directory holding one as a listing,
and the first-parent diffs of its root in the last N commits -- each entry with the sha256 of the exact
bytes the probe will read, so the corpus recomputes by digest. The counter-examples file is pinned
alongside. A source is NAME=PATH@COMMIT:ROOT[=URL]; the manifest records the name, URL and commit, never
a local path (the runner maps names to local clones with --repo NAME=PATH).
Usage: make_corpus.py COUNTEREXAMPLES_JSON OUT SOURCE... [--diffs N]"""
import argparse, hashlib, json, pathlib, subprocess

def git(repo, *args):
    return subprocess.run(["git", "-C", repo, *args], capture_output=True, check=True).stdout

def source(spec, ndiffs):
    name, rest = spec.split("=", 1)
    url = None
    if "=" in rest: rest, url = rest.split("=", 1)
    path, rest = rest.rsplit("@", 1); commit, root = rest.split(":", 1)
    commit = git(path, "rev-parse", commit + "^{commit}").decode().strip()
    sha = lambda b: hashlib.sha256(b).hexdigest()
    paths = [p for p in git(path, "ls-tree", "-r", "--name-only", commit, root + "/").decode().splitlines() if p.endswith(".rs")]
    reads = [{"path": p, "label": f"cat {p}", "sha256": sha(git(path, "show", f"{commit}:{p}"))} for p in paths]
    dirs = sorted({str(pathlib.PurePosixPath(p).parent) for p in paths})
    listings = [{"path": d, "label": f"ls {d}", "sha256": sha(git(path, "ls-tree", "--name-only", commit, d + "/"))} for d in dirs]
    diffs = []
    # a root commit has no ^1, so it gives no diff
    for c in git(path, "rev-list", "--first-parent", "--min-parents=1", f"-{ndiffs}", commit, "--", root).decode().split():
        b = git(path, "diff", f"{c}^1", c, "--", root)  # the first-parent difference: a merge's own change
        if b.strip():
            diffs.append({"commit": c, "path": root, "label": f"git diff {c[:7]}^1 {c[:7]} -- {root}", "sha256": sha(b)})
    print(f"{name}: {len(reads)} reads, {len(listings)} listings, {len(diffs)} diffs at {commit[:12]}")
    return {"name": name, "url": url, "commit": commit, "root": root, "reads": reads, "listings": listings, "diffs": diffs}

def main(argv=None):
    ap = argparse.ArgumentParser(); ap.add_argument("counterexamples"); ap.add_argument("out"); ap.add_argument("sources", nargs="+")
    ap.add_argument("--diffs", type=int, default=12); a = ap.parse_args(argv)
    out, ce = pathlib.Path(a.out), pathlib.Path(a.counterexamples)
    try: top = pathlib.Path(git(str(ce.resolve().parent), "rev-parse", "--show-toplevel").decode().strip()).resolve()
    except subprocess.CalledProcessError: raise SystemExit(f"make_corpus: {ce} is not inside a git repository")
    if not out.parent.resolve().is_relative_to(top):  # else the relative path would climb through the local tree
        raise SystemExit(f"make_corpus: write the manifest inside the repository that holds {ce.name}, so no local path enters it")
    manifest = {"source": "git", "sources": [source(s, a.diffs) for s in a.sources],
                "counterexamples": str(ce.resolve().relative_to(out.parent.resolve(), walk_up=True)),
                "counterexamples_sha256": hashlib.sha256(ce.read_bytes()).hexdigest(), "written_by": "make_corpus.py"}
    out.write_text(json.dumps(manifest, indent=1) + "\n")

if __name__ == "__main__":
    main()
