#!/usr/bin/env python3
"""Classify a pull request's diff as `material` or `chore` (#276, ratified on #25).

    pr-scope.py --base REF [--head REF] [--census DIR]
    pr-scope.py --check          the classifier against its own cases

A diff is MATERIAL when any of these holds, and a CHORE otherwise:

  * #112's map returns a seeded fault for it -- today every file the map
    reaches also lies in PROTOCOL, GATE_TREES or the root rule (0 of its 158
    inputs outside them, measured on #280's third review), so the map is
    kept as a second reader, not as the rule that decides: a changed file reaches a
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
    many more; or a root entry other than a Markdown document or a licence,
    anything under a root dot-directory, or a top-level Python package or
    `node_modules/`, which the gate's interpreters import from;
  * a symlink or a gitlink is on either side of a change, which the gate's
    sandbox does not copy as a file.

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
import tempfile

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
# lives, and the map still names which faults. `target/` and `_site/` are
# ignored build output, yet `git add -f` tracks a file there and the gate
# reads both (#280's fifth review): `resolve-diet.py` picks a build out of
# `target/`, and `check_site` copies `_site/` whole.
GATE_TREES = (
    "diet/",
    "scripts/",
    "exercise/",
    "tools/",
    "tests/",
    "pages/",
    ".github/",
    "target/",
    "_site/",
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "rustfmt.toml",
    "verify.sh",
)

# What a file at the root may be and stay a chore: a Markdown document other
# than CONTRIBUTING.md, which is protocol, or a licence. Any other root entry
# -- `.gitattributes`, a new build file -- is material until ruled otherwise.
ROOT_CHORE = re.compile(r"(?!CONTRIBUTING\.md$)[^/]+\.md|LICENSE[^/]*", re.IGNORECASE)

# What a check's body says it reads besides `scripts/...`: `cargo` reads the
# Rust workspace, and `cd DIR` the tree it changes into.
CARGO = re.compile(r"\bcargo\b")
RUST_WORKSPACE = {"diet/", "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "rustfmt.toml"}
CHANGES_INTO = re.compile(r"\bcd ([A-Za-z0-9_][A-Za-z0-9_./-]*)")
# ...and a path a check's body names in a tree outside `scripts/`, which the
# map's `scripts/...` reader does not see: `python3 substrates/x.py`,
# `tests/fixtures/results-bad`.
NAMED_PATH = re.compile(r"(?<![\w$./-])((?:substrates|tests|results|pages|exercise|diet|tools)/[A-Za-z0-9_./-]*)")

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


# The modes a diff may not carry and stay a chore (#280's fifth review): a
# symlink or a gitlink, on either side. The gate's sandbox and the injection
# pre-flight copy regular files, and a directory or broken symlink breaks
# both, whatever tree it sits in.
LINK_MODES = {"120000": "a symlink", "160000": "a gitlink"}


def changed(base: str, head: str, repo: pathlib.Path = ROOT) -> tuple[list[str], dict[str, str]]:
    """The diff's paths, and those of them that are a link, with what kind."""
    done = subprocess.run(
        ["git", "diff", "--raw", "--no-renames", "-z", f"{base}...{head}"],
        cwd=repo, capture_output=True,
    )
    if done.returncode != 0:
        raise RuntimeError(done.stderr.decode(errors="replace").strip() or f"git diff exited {done.returncode}")
    # NUL-separated, never quoted (#280's review: a non-ASCII, tab or quote in
    # a path came back C-quoted, starting with `"`, and matched no protocol
    # entry). Without renames each record is `:old new sha sha status` and
    # one path (#280's fifth review: a rename out of `results/` read only its
    # new name, a chore).
    fields = done.stdout.split(b"\0")
    files: list[str] = []
    links: dict[str, str] = {}
    for meta, raw in zip(fields[0::2], fields[1::2]):
        path = raw.decode("utf-8", errors="surrogateescape")
        files.append(path)
        modes = meta.decode().lstrip(":").split()[:2]
        if kind := next((LINK_MODES[m] for m in modes if m in LINK_MODES), None):
            links[path] = kind
    return files, links


def under(path: str, entries: tuple[str, ...]) -> str | None:
    """The entry `path` falls under, compared CASEFOLDED (#280's second
    review): on a case-insensitive checkout `Results/x` is `results/x` and
    `Contributing.md` overwrites `CONTRIBUTING.md`, so a case variant is the
    path it collides with."""
    folded = path.casefold()
    return next(
        (e for e in entries if folded == e.casefold() or (e.endswith("/") and folded.startswith(e.casefold()))),
        None,
    )


def in_protocol(path: str) -> str | None:
    """The protocol entry `path` falls under, if any."""
    return under(path, PROTOCOL)


def in_gate_tree(path: str) -> str | None:
    """The gate tree or root build file `path` falls under, if any."""
    return under(path, GATE_TREES)


def importable(path: str, files: list[str], tree: pathlib.Path = ROOT) -> bool:
    """Whether `path` lies in a top-level Python package -- a directory whose
    `__init__.py` this diff adds or the tree carries -- or in a root
    `node_modules/` (#280's third review, ruled on #276). The gate runs
    `python3 -c` and `python3 -` from the root, where a package named `json`
    or `pathlib` replaces the standard library's; Node falls back to a root
    `node_modules/` from `exercise/`. A plain directory nothing imports, such
    as `logo/`, stays an asset directory."""
    if "/" not in path:
        return False
    first = path.split("/", 1)[0]
    if first.casefold() == "node_modules":
        return True
    # An `__init__` with ANY suffix marks a package (#280's fourth review):
    # Python's finder takes `.py`, a sourceless `.pyc` and every extension
    # suffix (`.so`, `.abi3.so`, `.cpython-312-...so`), which differ by
    # interpreter, so the name is matched, never a list of suffixes.
    init = re.compile(re.escape(first) + r"/__init__\.[^/]+", re.IGNORECASE)
    return any(init.fullmatch(f) for f in files) or any((tree / first).glob("__init__.*"))


def unknown_root(path: str) -> bool:
    """A root entry that is neither a gate file nor what a chore may touch:
    a root file other than a Markdown document or a licence, or anything
    under a root DOT-directory -- `.cargo/config.toml` is read by every
    `cargo` the gate runs, `.config/` by tools (#280's second review). A new
    top-level directory otherwise is an asset directory, as ruled."""
    first = path.split("/", 1)[0]
    if in_gate_tree(path) is not None:
        return False
    if "/" in path:
        return first.startswith(".")
    return not ROOT_CHORE.fullmatch(path)


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
        for match in NAMED_PATH.finditer(body):
            named = match.group(1).rstrip("/.")
            reads.add(named.rsplit("/", 1)[0] + "/" if named.endswith(".py") else named + "/")
        inputs[check] = reads
    return inputs


def classify(
    files: list[str], census: pathlib.Path | None = None, links: dict[str, str] | None = None,
) -> tuple[str, str, list[str]]:
    """(verdict, reason, checks) for a diff of `files`, `links` among them."""
    scope = scope_selftest()
    text = VERIFY.read_text(encoding="utf-8") if VERIFY.is_file() else ""
    changed_set = set(files)
    reason = ""
    protocol = next(((f, entry) for f in files if (entry := in_protocol(f))), None)
    if "verify.sh" in changed_set:
        reason = "it changes verify.sh, the gate itself"
    elif links:
        link = sorted(links)[0]
        reason = f"{link} is {links[link]}, which the gate's sandbox does not copy as a file"
    elif protocol:
        reason = f"{protocol[0]} is protocol ({protocol[1]})"
    elif changed_set & scope.MACHINERY_FILES:
        reason = f"{sorted(changed_set & scope.MACHINERY_FILES)[0]} is the selftest's machinery"
    elif tree := next(((f, entry) for f in files if (entry := in_gate_tree(f))), None):
        reason = f"{tree[0]} is in the gate's tree ({tree[1]})"
    elif root := next((f for f in files if unknown_root(f)), None):
        reason = f"{root} is a root entry a chore may not touch (only *.md other than CONTRIBUTING.md, and LICENSE*)"
    elif package := next((f for f in files if importable(f, files)), None):
        reason = f"{package} is in a top-level package or node_modules/, which the gate's interpreters import from"
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
    # Through classify, by paths the map does not reach (#280's second
    # review: the gate-tree and root branches were held only beside it).
    (("exercise/src/app.ts",), "material"),
    (("tests/fixtures/results-bad/x.reason",), "material"),
    (("clippy.toml",), "material"),
    ((".cargo/config.toml",), "material"),
    ((".config/nextest.toml",), "material"),
    (("Contributing.md",), "material"),
    (("Results/x/README.md",), "material"),
    (("Diet/src/lib.rs",), "material"),
    (("docs/notes.md", "logo/a.svg", "NOTES.MD"), "chore"),
    (("json/__init__.py",), "material"),
    (("json/__init__.pyc",), "material"),
    (("re/__init__.so",), "material"),
    (("json/__init__.cpython-312-x86_64-linux-gnu.so",), "material"),
    (("pathlib/__init__.py", "pathlib/x.py"), "material"),
    (("node_modules/x/index.js",), "material"),
    (("logo/build.py", "logo/lens.py"), "chore"),
    # Python's finder on a case-insensitive filesystem imports `__INIT__.py`
    # (#280's fifth review, measured on macOS), so the match ignores case.
    (("json/__INIT__.py",), "material"),
    # The gate's ignored build trees (#280's fifth review).
    (("target/release/diet",), "material"),
    (("_site/x.html",), "material"),
)

# What a chore owes locally, read through classify: #274's paths name
# exactly hygiene and history (#276's done-when), so a change that stopped
# naming either is red.
CHORE_CHECKS = (("README.md", "logo/logo-dark.svg", "logo/build.py"), ["hygiene", "history"])


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
    ("target/release/diet", "target/"),
    ("_site/ledger/index.html", "_site/"),
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


GIT_CASES = 4


def git_cases() -> list[str]:
    """What `changed()` and the tree half of `importable` must read, on a
    throwaway repository (#280's fifth review: neither was reached by a case,
    so `--no-renames`, `-z` and the tree glob could each be dropped green).
    Returns what was misread."""
    wrong: list[str] = []
    with tempfile.TemporaryDirectory(prefix="pr-scope-") as tmp:
        repo = pathlib.Path(tmp)
        git = lambda *a: subprocess.run(
            ["git", "-c", "user.name=pr-scope", "-c", "user.email=pr-scope@example.invalid", *a],
            cwd=repo, capture_output=True, check=True,
        )
        git("init", "--quiet")
        record = repo / "results" / "x" / "README.md"
        record.parent.mkdir(parents=True)
        record.write_text("a record\n", encoding="utf-8")
        (repo / "logo").mkdir()
        git("add", "--all")
        git("commit", "--quiet", "-m", "base")
        git("tag", "base")
        git("mv", "results/x/README.md", "logo/notes.md")
        (repo / "logo" / "caf\u00e9.svg").write_text("<svg/>\n", encoding="utf-8")
        (repo / "logo" / "dirlink").symlink_to("..")
        git("add", "--all")
        git("commit", "--quiet", "-m", "head")
        files, links = changed("base", "HEAD", repo)
        # A rename is read as both its names, a non-ASCII path as itself.
        expected = ["logo/caf\u00e9.svg", "logo/dirlink", "logo/notes.md", "results/x/README.md"]
        if sorted(files) != expected:
            wrong.append(f"a rename out of results/, a non-ASCII path and a symlink read as {sorted(files)}, not {expected}")
        if links != {"logo/dirlink": "a symlink"}:
            wrong.append(f"the symlink logo/dirlink read as links {links}")
        if classify(["logo/dirlink"], links={"logo/dirlink": "a symlink"})[0] != "material":
            wrong.append("a symlink classified chore")
        (repo / "pkg").mkdir()
        (repo / "pkg" / "__init__.py").write_text("", encoding="utf-8")
        if not importable("pkg/x.txt", ["pkg/x.txt"], tree=repo):
            wrong.append("a package the tree carries, and the diff does not add, read as not importable")
    return wrong


def check() -> int:
    """Every case gets its verdict, or the classifier is red."""
    wrong = 0
    for misread in git_cases():
        wrong += 1
        print(f"pr-scope: {misread}", file=sys.stderr)
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
    # A path's CR printed raw could start a workflow command (#280's fifth review).
    printed = escaped("results/a\r::warning::x\n%")
    if printed != "results/a\\x0d::warning::x\\x0a%":
        wrong += 1
        print(f"pr-scope: a CR and an LF in a path print as {printed!r}, not escaped", file=sys.stderr)
    chore_files, chore_checks = CHORE_CHECKS
    verdict, _reason, named = classify(list(chore_files))
    if verdict != "chore" or named != chore_checks:
        wrong += 1
        print(f"pr-scope: {', '.join(chore_files)} classified {verdict}, naming {named}, not chore naming {chore_checks}", file=sys.stderr)
    for path, entry in GATE_CASES:
        if in_gate_tree(path) != entry:
            wrong += 1
            print(f"pr-scope: {path} falls under {in_gate_tree(path)!r}, not the gate tree {entry!r}", file=sys.stderr)
    for path, unknown in ROOT_CASES:
        if unknown_root(path) != unknown:
            wrong += 1
            print(f"pr-scope: the root entry {path} reads {'unknown' if unknown_root(path) else 'chore-able'}, not as declared", file=sys.stderr)
    total = len(CASES) + len(PROTOCOL_CASES) + len(GATE_CASES) + len(ROOT_CASES) + GIT_CASES + 2
    if wrong:
        print(f"pr-scope: {wrong} of {total} case(s) misclassified", file=sys.stderr)
        return 1
    print(f"pr-scope: {total} case(s) classified as declared")
    return 0


def escaped(text: str) -> str:
    """`text` with every C0 control character and DEL written as `\\xNN`."""
    return re.sub(r"[\x00-\x1f\x7f]", lambda m: f"\\x{ord(m.group()):02x}", text)


def main(argv: list[str]) -> int:
    if argv == ["--check"]:
        return check()
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--base", required=True, help="what the PR is measured against")
    parser.add_argument("--head", default="HEAD")
    parser.add_argument("--census", type=pathlib.Path, help="the latest main selftest's census directory")
    args = parser.parse_args(argv)
    try:
        files, links = changed(args.base, args.head)
    except RuntimeError as err:
        print(f"pr-scope: the diff {args.base}...{args.head} cannot be read: {err}", file=sys.stderr)
        return EXIT_BROKEN
    if not files:
        print(f"pr-scope: {args.base}...{args.head} changes nothing; there is no diff to classify", file=sys.stderr)
        return EXIT_BROKEN
    if not VERIFY.is_file():
        print("pr-scope: verify.sh is not in this tree; there is no gate to classify against", file=sys.stderr)
        return EXIT_BROKEN
    verdict, reason, checks = classify(files, args.census, links)
    print(verdict)
    if reason:
        # A path is printed with its control characters escaped (#280's
        # fifth review): a lone CR ends a line for the Actions runner, so a
        # path carrying one could start a workflow command.
        print(f"reason: {escaped(reason)}")
    print(f"checks: {', '.join(checks)}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
