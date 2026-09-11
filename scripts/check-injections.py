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
FUNC_BODY = re.compile(r"^(inject_[a-z0-9_]+)\(\) \{\n(.*?)^\}\n", re.M | re.S)
# The injection a seeded case names. A case naming one that is not defined is
# the failure this script could not see until it looked: it enumerates
# DEFINITIONS, so a definition deleted outright is invisible to it -- there is
# nothing to run and report inert. The selftest catches it, eventually, as
# "changed nothing"; that is forty minutes away and this is not.
# Every helper an injection may call, sourced alongside it. Extracted by name
# rather than by sourcing verify.sh, which would run the gate.
HELPERS = re.compile(
    r"^(?:seed_commit|strip_substrates)\(\) \{\n.*?^\}\n", re.M | re.S
)

GIT_ENV = {"GIT_CONFIG_GLOBAL": "/dev/null", "GIT_CONFIG_SYSTEM": "/dev/null"}

# A field line inside a struct body or a struct-like enum variant: an optional
# visibility, a name, a colon, a type. Attributes, doc comments and blank
# lines are not fields and are skipped by not matching.
FIELD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?([a-z_][a-z0-9_]*)\s*:\s*[^,]", re.M)
TYPE_DECL = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(struct|enum)\s+([A-Z][A-Za-z0-9_]*)\s*\{", re.M)
VARIANT = re.compile(r"^\s{4,}([A-Z][A-Za-z0-9_]*)\s*\{", re.M)


def brace_span(text: str, opened: int) -> int | None:
    """Where the block opened by the `{` at `opened` closes, or None.

    None is a real answer: an injection may replace a PREFIX of a literal,
    leaving the file to supply the rest. That is not a whole literal, so it
    cannot be missing a field, and the matcher running off the end is what
    says so.
    """
    depth, cursor = 0, opened
    while cursor < len(text):
        if text[cursor] == "{":
            depth += 1
        elif text[cursor] == "}":
            depth -= 1
            if depth == 0:
                return cursor
        cursor += 1
    return None


USE_LINE = re.compile(r"^\s*(?:pub\s+)?use\s+([A-Za-z_][A-Za-z0-9_:]*)::(?:\{([^}]*)\}|([A-Za-z_][A-Za-z0-9_]*))", re.M)
# Both roots, because a resolver that cannot SEE a declaration is a resolver
# that places a literal against the wrong one. `Ground` is declared in
# diet/tests/drive_cli.rs beside two diet/src modules, so reading only src
# meant the scan knew two of the three and could not know it was short.
EDITS = re.compile(r"\b(diet/(?:src|tests)/[A-Za-z0-9_/]+\.rs)\b")


def module_file(root: Path, path: str) -> Path | None:
    """The file a `use crate::a::b` path names, if it is one."""
    parts = [p for p in path.split("::") if p not in ("crate", "self", "super")]
    if not parts:
        return None
    for candidate in (root.joinpath(*parts).with_suffix(".rs"),
                      root.joinpath(*parts) / "mod.rs"):
        if candidate.is_file():
            return candidate
    return None


def declared_types(*roots: Path) -> dict[str, list[str]]:
    """Every braced type in the crate, by the name a literal spells, with the
    fields a literal of it must name.

    Structs AND struct-like enum variants. The variants matter: the record's
    `Event::Summary { .. }` is built inside an injection, and a scan that read
    only `pub struct` would have watched #47's items 3 and 4 rewrite exactly
    that variant without a word.

    A variant is keyed both ways -- `Event::Summary` and `Summary` -- because
    an injection may spell either, and both are the same obligation.

    Keyed by name AND by the file that declares it, because TWENTY-THREE
    names here are declared in more than one place -- measured, after a
    disclosure claimed five and was never re-asked -- and one of them is
    `Provenance` -- the type whose growth invalidated an injection on #43 and
    the reason this scan exists. A map from bare names to one field list would
    have compared that injection against the wrong `Provenance` and reported
    nine failures on a clean tree, which is how a scan gets switched off.
    """
    found: dict[str, dict[Path, list[str]]] = {}
    for path in sorted(p for root in roots for p in root.rglob("*.rs")):
        text = path.read_text(encoding="utf-8", errors="replace")
        for decl in TYPE_DECL.finditer(text):
            kind, name = decl.group(1), decl.group(2)
            opened = text.index("{", decl.start())
            end = brace_span(text, opened)
            if end is None:
                continue
            body = text[opened + 1 : end]
            if kind == "struct":
                found.setdefault(name, {})[path] = FIELD.findall(body)
                continue
            for variant in VARIANT.finditer(body):
                inner_end = brace_span(body, variant.end() - 1)
                if inner_end is None:
                    continue
                fields = FIELD.findall(body[variant.end() : inner_end])
                found.setdefault(f"{name}::{variant.group(1)}", {})[path] = fields
                found.setdefault(variant.group(1), {})[path] = fields
    return found


def incomplete_literals(
    source: str, types: dict, crate: Path, repo: Path
) -> list[str]:
    """Injections whose replacement text builds a literal missing a field.

    THE COMPILER CANNOT SEE INSIDE AN INJECTION. Its replacement text is a
    string in verify.sh, so a merge that adds a field to a struct silently
    invalidates every injection that writes a whole literal of it, and the
    tree still builds. The selftest catches it -- as "red for the wrong
    reason", which is the only way it is catchable there at all -- forty
    minutes into CI. This asks the same question in a second.

    Written from a demonstration rather than from a theory. The first version
    of this scan skipped any injection body containing `..`, meaning to skip
    functional updates like `..other.provenance`. But the very injection that
    had gone red on CI contains `..Effect::default()` -- a functional update
    of a DIFFERENT struct, several lines away -- so the scan skipped it and
    reported all clear. `..` is therefore honoured only INSIDE the literal it
    belongs to, which is why the span is brace-matched rather than guessed.
    """
    stale: list[str] = []
    unresolved: list[str] = []
    for match in FUNC_BODY.finditer(source):
        name, body = match.group(1), match.group(2)
        # Repository-relative, because an injection may now edit diet/tests as
        # well as diet/src and the two do not share a prefix to strip.
        edits = [repo / p for p in EDITS.findall(body)]
        for spelling, where in types.items():
            for opened in literal_starts(body, spelling):
                brace = opened + len(spelling) + 1
                end = brace_span(body, brace)
                if end is None:
                    continue  # a prefix of a literal, not a whole one
                inner = body[brace + 1 : end]
                if ".." in inner:
                    continue  # this literal's own functional update supplies the rest
                fields, why = resolve(spelling, where, edits, crate)
                if fields is None:
                    unresolved.append(f"{name}  builds a {spelling}: {why}")
                    continue
                if not fields:
                    continue
                # `Word { text, literal }` names its fields by shorthand and
                # there is no colon to find. A scan that wanted one reported
                # every shorthand literal as missing everything.
                missing = [
                    f for f in fields
                    if not re.search(rf"\b{f}\s*(?::|,|\}}|$)", inner)
                ]
                if missing:
                    stale.append(
                        f"{name}  builds a {spelling} without {', '.join(missing)}"
                    )
    return stale + [f"{line}" for line in unresolved]


def literal_starts(body: str, spelling: str):
    """Where `spelling {` begins in `body`, as a whole word.

    Whole-word so that `Provenance {` does not also match inside
    `object::Provenance {` -- the qualified spelling is a key of its own and
    reporting both would name one literal twice.
    """
    at = 0
    needle = spelling + " {"
    while (opened := body.find(needle, at)) >= 0:
        at = opened + len(needle)
        before = body[opened - 1] if opened else " "
        if before.isalnum() or before in "_:":
            continue
        yield opened


def resolve(spelling: str, where: dict, edits: list, root: Path):
    """Which declaration of `spelling` a literal in this injection means.

    One name, several declarations, is the ordinary case in a crate of any
    size, and picking the first is how a scan compares an injection against a
    type it has never heard of. So: the file the injection EDITS, then what
    that file imports, then a tree-wide answer only if there is exactly one.
    Anything else is unresolved and SAID so -- a scan that guesses here fails
    on a clean tree, and a scan that fails on a clean tree gets deleted.
    """
    if len(where) == 1:
        return next(iter(where.values())), ""
    for edited in edits:
        if edited in where:
            return where[edited], ""
    bare = spelling.split("::")[-1]
    for edited in edits:
        if not edited.is_file():
            continue
        for imported in USE_LINE.finditer(edited.read_text(encoding="utf-8", errors="replace")):
            names = (imported.group(2) or imported.group(3) or "")
            if bare not in {n.strip().split(" as ")[0] for n in names.split(",")}:
                continue
            target = module_file(root, imported.group(1))
            if target in where:
                return where[target], ""
    return None, (
        f"declared in {len(where)} places with different fields and the "
        f"injection names none of them"
    )


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

    # The third question, and the cheapest: does every struct literal an
    # injection writes still name every field its type declares? Asked
    # statically, before a single box is built, because the answer is in two
    # texts and needs no tree at all.
    crate_root = root / "diet" / "src"
    tests_root = root / "diet" / "tests"
    stale = incomplete_literals(
        text, declared_types(crate_root, tests_root), crate_root, root
    )
    if stale:
        print(
            f"check-injections: {len(stale)} injection(s) build a literal the "
            f"compiler never sees, and it no longer names every field:",
            file=sys.stderr,
        )
        for line in stale:
            print(f"  {line}", file=sys.stderr)
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
            # TWO WAYS AN INJECTION FAILS TO BE A FAULT, and only one of them
            # used to be read. The fingerprint answers "did anything change";
            # the exit status answers "did what it meant to do happen". An
            # injection that copies a directory and THEN raises on a field the
            # schema renamed satisfies the first and fails the second -- and
            # was reported as fine here, while the selftest graded the check
            # against the half-made tree and called it a gate that did not
            # fire. Found by running it, after item 3 renamed `substrate`.
            if fingerprint(box) == before or run.returncode != 0:
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

    print(
        f"check-injections: {len(names)} injection(s), {len(inert)} that change "
        f"nothing or do not finish; every struct literal inside one names every "
        f"field its type declares"
    )
    for name, code, err in inert:
        print(f"  {name}  exit={code}  {err}")
    return 1 if inert else 0


if __name__ == "__main__":
    sys.exit(main())
