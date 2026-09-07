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

Two questions, because one is not enough. First: does every seeded case name
an injection that EXISTS? A merge can delete a definition outright, and this
script enumerates definitions -- so the deleted one leaves nothing to run and
nothing to report, while the case that names it goes on claiming coverage.
Then: does every injection change the tree?

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

import gatelib
import sys
import tempfile
from pathlib import Path

FUNC = re.compile(r"^(inject_[a-z0-9_]+)\(\) \{", re.M)
# The injection a seeded case names. A case naming one that is not defined is
# the failure this script could not see until it looked: it enumerates
# DEFINITIONS, so a definition deleted outright is invisible to it -- there is
# nothing to run and report inert. The selftest catches it, eventually, as
# "changed nothing"; that is forty minutes away and this is not.
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
    # One `cp`, not a thousand `shutil.copy2` calls. Measured: 0.70s the old
    # way, 0.56s this way, for the same thousand files. A fifth, not an order
    # of magnitude -- the copy itself is most of what a populate costs either
    # way, which is why the caller now populates ONCE and restores.
    #
    # `--preserve=mode` and not `-p`: the executable bit is load-bearing (a
    # results fixture's recompute.sh is run by name), and the modification
    # times deliberately are not preserved -- a copy whose sources look older
    # than some artifact is how a stale binary gets read as fresh, which is a
    # mistake this repository has already paid for once.
    subprocess.run(
        ["cp", "-L", "--parents", "--preserve=mode", "-t", str(box), *tracked],
        cwd=root,
        check=True,
    )
    subprocess.run(
        ["git", "-C", str(box), "init", "--quiet"],
        check=True,
        env={**os.environ, **GIT_ENV},
    )


def digests(box: Path) -> dict[str, str]:
    """Every entry in the box by path, with what it is and what it holds.

    Content AND mode AND link target, because the box is now reused between
    injections and a fresh box no longer hides what an injection leaves
    behind. A `chmod` and a symlink are both changes to the tree -- the
    question this script asks -- and both used to be invisible here: the mode
    was never read, and a symlink pointing at a DIRECTORY is not `is_file()`,
    so it appeared in no walk and would have survived every restore.

    Symlinks are not followed. A link is recorded as the path it names, so a
    link that is retargeted is a change even when both targets read the same.

    One thing this still cannot see: an EMPTY directory. git does not record
    them, so the box never starts with one and an injection whose only effect
    is `mkdir` would read as inert. That was true before the box was reused
    and is unchanged by it; restore() removes them regardless, so the blind
    spot cannot leak from one injection into the next. It is recorded here
    rather than left to be rediscovered.
    """
    found: dict[str, str] = {}
    for where, dirs, files in os.walk(box, followlinks=False):
        here = Path(where)
        if ".git" in here.relative_to(box).parts:
            dirs[:] = []
            continue
        dirs[:] = [d for d in dirs if not (here == box and d == ".git")]
        for name in list(dirs) + files:
            path = here / name
            rel = str(path.relative_to(box))
            if path.is_symlink():
                found[rel] = "link:" + os.readlink(path)
            elif path.is_file():
                found[rel] = (
                    f"file:{path.stat().st_mode & 0o7777:04o}:"
                    + hashlib.sha256(path.read_bytes()).hexdigest()
                )
    return found


def fingerprint(box: Path, files: dict[str, str] | None = None) -> str:
    per_file = digests(box) if files is None else files
    digest = hashlib.sha256()
    for rel in sorted(per_file):
        digest.update(rel.encode())
        digest.update(b"\0")
        digest.update(per_file[rel].encode())
        digest.update(b"\0")
    # History counts too: an injection whose whole effect is a commit, or a
    # ref it declines to create, changes no file and is not thereby inert.
    for args in (["show-ref"], ["log", "--all", "--format=%H %s"]):
        history = subprocess.run(
            ["git", "-C", str(box), *args], capture_output=True, text=True
        )
        digest.update(history.stdout.encode())
    return digest.hexdigest()


def restore(box: Path, root: Path, pristine: dict[str, str]) -> None:
    """Put the box back to the tree it was populated from.

    Populating costs 0.56s and there are two hundred injections, so the box is
    made once and put back between them. What is put back is only what moved:
    an injection changes one file, or two, out of a thousand. Measured on this
    tree, back to back, same verdict both ways (215 injections, 0 inert):
    2m23s a box each, 49s one box restored -- and the system time, which is
    what a thousand file creations and deletions actually costs, fell from
    1m41s to 15s.

    `.git` is rebuilt outright rather than diffed. Two injections seed history
    rather than files, and a fresh `git init` -- five milliseconds against a
    directory of a dozen files -- is cheaper to do than to reason about.

    THE RESTORE IS NOT TRUSTED. The caller re-fingerprints afterwards and
    refuses the run if the box is not byte-for-byte what it started as. A
    reused box that quietly keeps the previous injection's edit would make the
    NEXT injection look like it changed nothing -- reporting an inert gate
    that is fine, which is a false red, or worse, hiding one that is not.
    """
    now = digests(box)
    for rel, want in pristine.items():
        if now.get(rel) != want:
            dest = box / rel
            if dest.is_symlink() or dest.is_file():
                dest.unlink()
            dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(root / rel, dest)
    for rel in sorted(now, key=len, reverse=True):
        if rel in pristine:
            continue
        stray = box / rel
        if stray.is_symlink() or stray.is_file():
            stray.unlink()
        elif stray.is_dir():
            shutil.rmtree(stray, ignore_errors=True)
    # An unlinked file can leave the directory that held it, which no
    # fingerprint sees and an injection might.
    for path in sorted(box.rglob("*"), key=lambda p: len(p.parts), reverse=True):
        if path.is_dir() and ".git" not in path.parts and not any(path.iterdir()):
            path.rmdir()
    shutil.rmtree(box / ".git", ignore_errors=True)
    subprocess.run(
        ["git", "-C", str(box), "init", "--quiet"],
        check=True,
        env={**os.environ, **GIT_ENV},
    )


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    text = (root / "verify.sh").read_text(encoding="utf-8")
    names = [match.group(1) for match in FUNC.finditer(text)]
    if not names:
        print("check-injections: verify.sh defines no injections", file=sys.stderr)
        return 2
    named = {case.injection for case in gatelib.seeded_cases(text)}
    undefined = sorted(named - set(names))
    if undefined:
        print(
            f"check-injections: {len(undefined)} seeded case(s) name an injection "
            f"that is not defined",
            file=sys.stderr,
        )
        for name in undefined:
            print(f"  {name}  named by a seeded case, defined nowhere", file=sys.stderr)
        return 1

    tracked = tracked_files(root)
    helpers = "\n".join(match.group(0) for match in HELPERS.finditer(text))

    inert = []
    box = Path(tempfile.mkdtemp(prefix="check-injections."))
    try:
        populate(box, root, tracked)
        pristine = digests(box)
        before = fingerprint(box, pristine)
        for name in names:
            body = re.search(rf"^{name}\(\) \{{\n.*?^\}}\n", text, re.M | re.S)
            if body is None:
                inert.append((name, 2, "its body could not be extracted"))
                continue
            run = subprocess.run(
                ["bash", "-c", f"set -e\n{helpers}\n{body.group(0)}\ncd {box}\n{name}\n"],
                capture_output=True,
                text=True,
                env={**os.environ, **GIT_ENV},
            )
            if fingerprint(box) == before:
                tail = (run.stderr or "").strip().splitlines()[-1:] or [""]
                inert.append((name, run.returncode, tail[0][:80]))
            restore(box, root, pristine)
            # The box is shared now, so its cleanliness is a precondition of
            # every case after this one rather than a detail of this one. An
            # unrestored edit would make the next injection look inert, and
            # "this gate never fires" is the one verdict that must never be
            # reached by accident.
            if fingerprint(box) != before:
                print(
                    f"check-injections: the sandbox could not be put back after "
                    f"{name}, so nothing after it can be trusted",
                    file=sys.stderr,
                )
                return 2
    finally:
        shutil.rmtree(box, ignore_errors=True)

    print(f"check-injections: {len(names)} injection(s), {len(inert)} change nothing")
    for name, code, err in inert:
        print(f"  {name}  exit={code}  {err}")
    return 1 if inert else 0


if __name__ == "__main__":
    sys.exit(main())
