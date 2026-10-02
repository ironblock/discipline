#!/usr/bin/env python3
"""Classify a pull request's diff as `material` or `chore` (#276, ratified on #25).

    pr-scope.py --base REF [--head REF] [--census DIR]
    pr-scope.py --check          the classifier against its own cases

A diff is MATERIAL when any of these holds, and a CHORE otherwise:

  * #112's map returns a seeded fault for it: a changed file reaches a
    fault's catcher, the selftest machinery, or -- with `--census`, the
    latest `main` selftest's census -- a file a fault's injection touched.
    The map is `scope-selftest.py`'s own, read from there, never copied;
  * it changes `verify.sh`, the gate itself;
  * it touches the protocol: `PROTOCOL` below, declared here and nowhere
    else -- the results and substrates records, the formats, the PR
    template, CONTRIBUTING.md, check-owners.tsv, the gate README, any
    workflow;
  * it touches the trees the gate runs from, `GATE_TREES` (ruled on #276):
    the map names the files a check function names, and a check reaches
    many more; or a root entry other than a Markdown document or a licence.

The branch's name is never read: a `chore/` prefix is a hint, and the diff is
the value. Printed:

    material|chore
    reason: <why, for material>
    checks: <the verify.sh checks whose inputs the diff touches>

`checks` is what a chore owes locally (`./verify.sh --only ...`). `hygiene`
reads every file and `history` every commit, so both are always named. CI
runs every check whatever this prints; the lane changes only what the author
owes locally and on the thread.

Stdlib only. Exit 0 with a verdict printed, 2 when the diff cannot be read.
"""

from __future__ import annotations

import argparse
import importlib.util
import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
VERIFY = ROOT / "verify.sh"
EXIT_BROKEN = 2

# THE PROTOCOL, declared once (#276; planning's refinement 1 on #25): a
# change here changes what a claim is, what a record holds, or what a PR
# owes, whether or not any seeded fault reaches it. A path ending in `/` is
# a tree; any other names one file.
PROTOCOL = (
    "results/",
    "substrates/",
    "diet/src/formats/",
    "diet/formats/",
    ".github/PULL_REQUEST_TEMPLATE.md",
    "CONTRIBUTING.md",
    ".github/check-owners.tsv",
    "tools/gate/README.md",
    ".github/workflows/",
)

# THE TREES THE GATE RUNS FROM, declared once (#280's review; ruled on #276).
# #112's map names the files a check function names; a check also reads
# files it reaches through `${here}`, an import, `include_str!` or `cd` --
# about 280 of them, which read `chore` against the map alone. So a diff
# touching any of these trees or root build files is material whatever the
# map says. A tree, not a per-file list: it names where the gate's code
# lives, and the map still names which faults.
GATE_TREES = (
    "diet/",
    "scripts/",
    "exercise/",
    "tools/",
    "tests/",
    "pages/",
    ".github/",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "verify.sh",
)

# What a file at the root may be and stay a chore: a Markdown document other
# than CONTRIBUTING.md, which is protocol, or a licence. Any other root entry
# -- `.gitattributes`, a new build file -- is material until ruled otherwise.
ROOT_CHORE = re.compile(r"(?!CONTRIBUTING\.md$)[^/]+\.md|LICENSE[^/]*")

# What a check's body says it reads besides `scripts/...`: `cargo` reads the
# Rust workspace, and `cd DIR` the tree it changes into.
CARGO = re.compile(r"\bcargo\b")
RUST_WORKSPACE = {"diet/", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rustfmt.toml"}
CHANGES_INTO = re.compile(r"\bcd ([A-Za-z0-9_][A-Za-z0-9_./-]*)")

# Checks that read everything: the tree scan reads every file, the history
# scan every commit, so a diff of anything touches both.
READS_EVERYTHING = ("hygiene", "history")

# The trees a check reads that its function names by flag, not by script.
TREE_FLAG = re.compile(r"--(?:tree|root)\s+([A-Za-z0-9_./-]+)")


def scope_selftest():
    """#112's map, loaded from its own file."""
    spec = importlib.util.spec_from_file_location("scope_selftest", ROOT / "scripts" / "scope-selftest.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def changed(base: str, head: str) -> list[str]:
    done = subprocess.run(
        ["git", "diff", "--name-only", "--no-renames", "-z", f"{base}...{head}"],
        cwd=ROOT, capture_output=True,
    )
    if done.returncode != 0:
        raise RuntimeError(done.stderr.decode(errors="replace").strip() or f"git diff exited {done.returncode}")
    # NUL-separated, never quoted (#280's review: a non-ASCII, tab or quote in
    # a path came back C-quoted, starting with `"`, and matched no protocol
    # entry).
    return [p.decode("utf-8", errors="surrogateescape") for p in done.stdout.split(b"\0") if p]


def in_protocol(path: str) -> str | None:
    """The protocol entry `path` falls under, if any."""
    return next((entry for entry in PROTOCOL if path == entry or (entry.endswith("/") and path.startswith(entry))), None)


def in_gate_tree(path: str) -> str | None:
    """The gate tree or root build file `path` falls under, if any."""
    return next((entry for entry in GATE_TREES if path == entry or (entry.endswith("/") and path.startswith(entry))), None)


def unknown_root(path: str) -> bool:
    """A root entry that is neither a gate file nor what a chore may touch."""
    return "/" not in path and in_gate_tree(path) is None and not ROOT_CHORE.fullmatch(path)


def check_inputs(scope, text: str) -> dict[str, set[str]]:
    """check -> the files and trees it reads, from verify.sh's own function
    bodies and #112's declared inputs."""
    bodies = scope.functions(text)
    checks = re.search(r"^readonly CHECKS=\(([^)]*)\)", text, re.M)
    inputs: dict[str, set[str]] = {}
    for check in (checks.group(1).split() if checks else []):
        body = bodies.get(f"check_{check}", "")
        reads = scope.check_scripts(bodies, check) | scope.CHECK_INPUTS.get(check, set())
        for match in TREE_FLAG.finditer(body):
            reads.add(match.group(1).rstrip("/") + "/")
        if CARGO.search(body):
            reads |= RUST_WORKSPACE
        for match in CHANGES_INTO.finditer(body):
            reads.add(match.group(1).rstrip("/") + "/")
        inputs[check] = reads
    return inputs


def classify(files: list[str], census: pathlib.Path | None = None) -> tuple[str, str, list[str]]:
    """(verdict, reason, checks) for a diff of `files`."""
    scope = scope_selftest()
    text = VERIFY.read_text(encoding="utf-8")
    changed_set = set(files)
    reason = ""
    protocol = next(((f, entry) for f in files if (entry := in_protocol(f))), None)
    if "verify.sh" in changed_set:
        reason = "it changes verify.sh, the gate itself"
    elif protocol:
        reason = f"{protocol[0]} is protocol ({protocol[1]})"
    elif changed_set & scope.MACHINERY_FILES:
        reason = f"{sorted(changed_set & scope.MACHINERY_FILES)[0]} is the selftest's machinery"
    elif tree := next(((f, entry) for f in files if (entry := in_gate_tree(f))), None):
        reason = f"{tree[0]} is in the gate's tree ({tree[1]})"
    elif root := next((f for f in files if unknown_root(f)), None):
        reason = f"{root} is a root entry a chore may not touch (only *.md other than CONTRIBUTING.md, and LICENSE*)"
    else:
        touched = scope.read_census(census)[1] if census else {}
        for ident, (deps, _units) in sorted(scope.dependencies(ROOT, text).items()):
            hit = scope.reaches(touched.get(ident, set()), changed_set) or scope.reaches(deps, changed_set)
            if hit:
                reason = f"{hit} reaches the seeded fault {ident}"
                break
    inputs = check_inputs(scope, text)
    checks = [c for c, reads in inputs.items() if c in READS_EVERYTHING or scope.reaches(reads, changed_set)]
    return ("material" if reason else "chore"), reason, checks


# The classifier's own cases (#276's acceptance), run by `check_metadata`:
# each a diff, by its changed paths, and the verdict it must get. A protocol
# entry the classifier stopped reading reads `chore` here and is red.
CASES = (
    (("logo/logo-dark.svg", "logo/README.md", "README.md"), "chore"),
    (("verify.sh",), "material"),
    (("results/2026-10-03-x/README.md",), "material"),
    ((".github/PULL_REQUEST_TEMPLATE.md",), "material"),
    (("CONTRIBUTING.md",), "material"),
    ((".github/check-owners.tsv",), "material"),
    (("tools/gate/README.md",), "material"),
    ((".github/workflows/pages.yml",), "material"),
    (("substrates/registry.toml",), "material"),
    (("diet/src/formats/log.rs",), "material"),
    (("scripts/hygiene.sh",), "material"),
    (("scripts/gatelib.py",), "material"),
)


# Each protocol entry, read on its own (#280's review: five entries were
# covered only because the fault map also reached their cases, so dropping
# one from PROTOCOL left `--check` green). A path, and the entry it must fall
# under -- written here, not derived from PROTOCOL.
PROTOCOL_CASES = (
    ("results/2026-10-03-x/README.md", "results/"),
    ("substrates/registry.toml", "substrates/"),
    ("diet/src/formats/log.rs", "diet/src/formats/"),
    ("diet/formats/decline/fixtures/valid/x.txt", "diet/formats/"),
    (".github/PULL_REQUEST_TEMPLATE.md", ".github/PULL_REQUEST_TEMPLATE.md"),
    ("CONTRIBUTING.md", "CONTRIBUTING.md"),
    (".github/check-owners.tsv", ".github/check-owners.tsv"),
    ("tools/gate/README.md", "tools/gate/README.md"),
    (".github/workflows/pages.yml", ".github/workflows/"),
)


# Each gate tree, read on its own, by a path in it -- written here, not
# derived from GATE_TREES -- and the root rule both ways.
GATE_CASES = (
    ("diet/src/capture/router/asks/generic.txt", "diet/"),
    ("scripts/hygiene-exceptions.tsv", "scripts/"),
    ("exercise/src/app.ts", "exercise/"),
    ("tools/gate/faults.toml", "tools/"),
    ("tests/fixtures/results-bad/x.reason", "tests/"),
    ("pages/index.html", "pages/"),
    (".github/CODEOWNERS", ".github/"),
    ("Cargo.toml", "Cargo.toml"),
    ("Cargo.lock", "Cargo.lock"),
    ("rust-toolchain.toml", "rust-toolchain.toml"),
    ("rustfmt.toml", "rustfmt.toml"),
    ("verify.sh", "verify.sh"),
)
ROOT_CASES = (
    ("README.md", False),
    ("AGENTS.md", False),
    ("LICENSE", False),
    ("CONTRIBUTING.md", True),
    (".gitattributes", True),
    ("Makefile", True),
)


def check() -> int:
    """Every case gets its verdict, or the classifier is red."""
    wrong = 0
    for path, entry in PROTOCOL_CASES:
        if in_protocol(path) != entry:
            wrong += 1
            print(f"pr-scope: {path} falls under {in_protocol(path)!r}, not the protocol entry {entry!r}", file=sys.stderr)
    for files, expected in CASES:
        verdict, reason, _checks = classify(list(files))
        if verdict != expected:
            wrong += 1
            print(
                f"pr-scope: {', '.join(files)} classified {verdict}, not {expected}"
                f"{f' ({reason})' if reason else ''}",
                file=sys.stderr,
            )
    for path, entry in GATE_CASES:
        if in_gate_tree(path) != entry:
            wrong += 1
            print(f"pr-scope: {path} falls under {in_gate_tree(path)!r}, not the gate tree {entry!r}", file=sys.stderr)
    for path, unknown in ROOT_CASES:
        if unknown_root(path) != unknown:
            wrong += 1
            print(f"pr-scope: the root entry {path} reads {'unknown' if unknown_root(path) else 'chore-able'}, not as declared", file=sys.stderr)
    total = len(CASES) + len(PROTOCOL_CASES) + len(GATE_CASES) + len(ROOT_CASES)
    if wrong:
        print(f"pr-scope: {wrong} of {total} case(s) misclassified", file=sys.stderr)
        return 1
    print(f"pr-scope: {total} case(s) classified as declared")
    return 0


def main(argv: list[str]) -> int:
    if argv == ["--check"]:
        return check()
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--base", required=True, help="what the PR is measured against")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--census", type=pathlib.Path, help="the latest main selftest's census directory")
    args = parser.parse_args(argv)
    try:
        files = changed(args.base, args.head)
    except RuntimeError as err:
        print(f"pr-scope: the diff {args.base}...{args.head} cannot be read: {err}", file=sys.stderr)
        return EXIT_BROKEN
    if not files:
        print(f"pr-scope: {args.base}...{args.head} changes nothing; there is no diff to classify", file=sys.stderr)
        return EXIT_BROKEN
    verdict, reason, checks = classify(files, args.census)
    print(verdict)
    if reason:
        print(f"reason: {reason}")
    print(f"checks: {', '.join(checks)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
