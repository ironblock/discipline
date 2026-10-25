"""The `skills` service (#971): `python3 -I skills.py start|stop`. The official ast-grep agent skills, the generic-docs control.

The skills (github.com/ast-grep/agent-skill) carry no license, so they are never in this tree: `start` fetches the
repository's archive at PINNED, checks each skill file against its digest here, and writes the skills under the run's
worktree at `.agent-skills/` (`ast-grep/SKILL.md`, `ast-grep/references/rule_reference.md`, `outline/SKILL.md`), where
the model's head points. A digest that differs refuses the run (exit 1). It provides `root`. It leaves no process.
"""
import hashlib, io, json, os, sys, tarfile, urllib.request

PINNED = "f2175aff21f20cfb8e2db30c28febeb3a2d42b61"
URL = "https://codeload.github.com/ast-grep/agent-skill/tar.gz/" + PINNED
FILES = {
    "ast-grep/SKILL.md": "a18b68ab555758b85915937cf252d70b0261f930a07ce52095e7cafbbd304782",
    "ast-grep/references/rule_reference.md": "9f6d2ba2f2dbde059f61f426c262abe943f548984496962c43c7cc6e44813ad6",
    "outline/SKILL.md": "e403fca295ad403bad60bbde1ffa320095cba60f99d09bda5c83a224da8b6613",
}


def start():
    here, worktree = os.environ["DIET_SERVICE_DIR"], os.environ["DIET_WORKTREE"]
    with urllib.request.urlopen(URL, timeout=60) as r:
        archive = tarfile.open(fileobj=io.BytesIO(r.read()), mode="r:gz")
    root = os.path.join(worktree, ".agent-skills")
    prefix = f"agent-skill-{PINNED}/ast-grep/skills/"
    for rel, digest in FILES.items():
        data = archive.extractfile(prefix + rel).read()
        if hashlib.sha256(data).hexdigest() != digest:
            sys.exit(f"skills: {rel} at {PINNED} is not the file pinned here")
        os.makedirs(os.path.dirname(os.path.join(root, rel)), exist_ok=True)
        with open(os.path.join(root, rel), "wb") as f:
            f.write(data)
    open(os.path.join(here, "pids"), "w").close()
    with open(os.path.join(here, "provides.json"), "w") as f:
        json.dump({"root": root, "commit": PINNED}, f)


if __name__ == "__main__":
    if sys.argv[1:] == ["start"]:
        start()
    elif sys.argv[1:] != ["stop"]:
        sys.exit("usage: skills.py start|stop")
