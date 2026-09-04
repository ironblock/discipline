#!/usr/bin/env python3
"""Every injection in verify.sh must change the tree it is run against.

An injection that changes nothing is a gate that reports RED for no reason,
or -- far worse -- a case the selftest never notices is inert. A verdict is
only worth what the fault behind it cost, so an injection is proven to change
the tree before its verdict counts.

This is the same question the selftest asks, asked in seconds instead of in
the tens of minutes a full run takes, and it is what catches the failure mode
a merge introduces: two branches that each add injections to verify.sh, joined
line by line, leave bodies spliced into each other and anchors pointing at
text that moved. Fourteen injections were silently emptied that way once. The
selftest would have caught it; it would have taken forty minutes to say so,
and the merge had already been pushed.

Each injection runs against its own copy of the tracked tree, in a fresh `git
init` so the ones that build history have a repository to build it in. The
fingerprint is every tracked file's digest plus the repository's refs and
commit subjects, so a change to the worktree and a change to history both
count -- and `.git`'s own internals, which differ run to run, do not.
"""

import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

FUNC = re.compile(r"^(inject_[a-z0-9_]+)\(\) \{", re.M)
# Every helper an injection may call, sourced alongside it. Extracted by name
# rather than by sourcing verify.sh, which would run the gate.
HELPERS = re.compile(r"^(?:seed_commit)\(\) \{\n.*?^\}\n", re.M | re.S)

GIT_ENV = {"GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"}


def tracked_files(root: Path) -> list[str]:
    out = subprocess.run(
        ["git", "-C", str(root), "ls-files", "-z"],
        capture_output=True,
        check=True,
    ).stdout
    return [part.decode() for part in out.split(b"\0") if part]


def populate(box: Path, root: Path, tracked: list[str]) -> None:
    for rel in tracked:
        dest = box / rel
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(root / rel, dest)
    subprocess.run(
        ["git", "-C", str(box), "init", "--quiet"],
        check=True,
        env={**os.environ, **GIT_ENV},
    )


def fingerprint(box: Path) -> str:
    digest = hashlib.sha256()
    for path in sorted(p for p in box.rglob("*") if p.is_file() and ".git" not in p.parts):
        digest.update(str(path.relative_to(box)).encode())
        digest.update(b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    # History counts too: an injection whose whole effect is a commit, or a
    # ref it declines to create, changes no file and is not thereby inert.
    for args in (["show-ref"], ["log", "--all", "--format=%H %s"]):
        history = subprocess.run(
            ["git", "-C", str(box), *args], capture_output=True, text=True
        )
        digest.update(history.stdout.encode())
    return digest.hexdigest()


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    text = (root / "verify.sh").read_text(encoding="utf-8")
    names = [match.group(1) for match in FUNC.finditer(text)]
    if not names:
        print("check-injections: verify.sh defines no injections", file=sys.stderr)
        return 2
    tracked = tracked_files(root)
    helpers = "\n".join(match.group(0) for match in HELPERS.finditer(text))

    inert = []
    for name in names:
        body = re.search(rf"^{name}\(\) \{{\n.*?^\}}\n", text, re.M | re.S)
        if body is None:
            inert.append((name, 2, "its body could not be extracted"))
            continue
        box = Path(tempfile.mkdtemp(prefix="check-injections."))
        try:
            populate(box, root, tracked)
            before = fingerprint(box)
            run = subprocess.run(
                ["bash", "-c", f"set -e\n{helpers}\n{body.group(0)}\ncd {box}\n{name}\n"],
                capture_output=True,
                text=True,
                env={**os.environ, **GIT_ENV},
            )
            if fingerprint(box) == before:
                tail = (run.stderr or "").strip().splitlines()[-1:] or [""]
                inert.append((name, run.returncode, tail[0][:80]))
        finally:
            shutil.rmtree(box, ignore_errors=True)

    print(f"check-injections: {len(names)} injection(s), {len(inert)} change nothing")
    for name, code, err in inert:
        print(f"  {name}  exit={code}  {err}")
    return 1 if inert else 0


if __name__ == "__main__":
    sys.exit(main())
