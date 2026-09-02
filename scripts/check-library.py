#!/usr/bin/env python3
"""Two rules about the `diet` library saying what it says.

RULE ONE: no stringly predicate.

Field kinds, verdicts and outcome classes were matched as strings in more than
one place in the prior system, and the places drifted. The repair is typed
predicates with exhaustive matches: adding a variant then fails to compile
everywhere that has to care, where a string comparison compiles and quietly
stops covering the new case.

The compiler enforces exhaustiveness once the predicate is an enum. It cannot
stop somebody writing a new ``match text { "DECISION" => ... }``, so this does.
The rule it enforces is narrow and mechanical: **no match arm in
``diet/src/`` may have a string literal for its pattern.** A string becomes a
kind in exactly one place per enum -- a ``from_tag`` that iterates ``ALL`` and
compares against each variant's own tag -- and that place is caught by the
compiler when a variant is added.

Scope, and what it does not cover. ``diet/tests/`` is not scanned: the
conformance harness dispatches on a format NAME, which is not a field kind, a
verdict or an outcome class, and it panics on a name it does not know. That is
a guard rather than a drift, but it is a string match, and the honest
statement is that this script does not look at it. A ``==`` comparison against
a literal is likewise out of reach; the arm is what this catches.

RULE TWO: no source file the library does not compile.

A `.rs` file under ``diet/src/`` that no ``mod`` declaration reaches is a file
rustc never sees: its tests do not run, its lints do not fire, and
``cargo test -- <its module>`` matches nothing and exits 0. This rule exists
because that happened -- a whole module, 500 lines and eleven tests, sat on
disk uncompiled while the gate stayed green, and the only symptom was a test
filter quietly selecting nothing.

Stdlib only. Exit 0 if both rules hold, 1 otherwise.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path("diet/src")

# A match arm whose pattern is one or more string literals: `"a" => ...`,
# `"a" | "b" => ...`, `"a" if cond => ...`. Byte and raw strings included,
# because a stringly predicate spelled `b"DECISION"` is the same predicate.
STRING = r'(?:b?r?#*"(?:[^"\\]|\\.)*"#*)'
ARM = re.compile(rf"^\s*(?:{STRING}\s*\|\s*)*{STRING}\s*(?:if\b[^=]*?)?=>")

# A `mod foo;` declaration -- the only thing that puts a file in the crate.
MOD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", re.M)


def declared_modules(source: pathlib.Path) -> list[str]:
    """The module names `source` declares."""
    try:
        text = source.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return []
    return MOD.findall(text)


def reachable(entry: pathlib.Path) -> set[pathlib.Path]:
    """Every file rustc reaches from `entry` through `mod` declarations."""
    seen = {entry.resolve()}
    frontier = [entry]
    while frontier:
        source = frontier.pop()
        # A file named `foo.rs` holds the module `foo`, whose children live in
        # `foo/`; `mod.rs` and `lib.rs` hold theirs beside them.
        base = (
            source.parent
            if source.name in ("lib.rs", "main.rs", "mod.rs")
            else source.parent / source.stem
        )
        for name in declared_modules(source):
            for candidate in (base / f"{name}.rs", base / name / "mod.rs"):
                if candidate.is_file() and candidate.resolve() not in seen:
                    seen.add(candidate.resolve())
                    frontier.append(candidate)
    return seen


def main() -> int:
    if not ROOT.is_dir():
        print(f"{ROOT}: missing", file=sys.stderr)
        return 1

    sources = sorted(ROOT.rglob("*.rs"))
    if not sources:
        print(
            f"{ROOT}: holds no Rust source; a scan of nothing is not a pass",
            file=sys.stderr,
        )
        return 1

    failures: list[str] = []

    # Rule two, first: an orphaned file's contents are moot, but its existence
    # is the finding.
    roots = [ROOT / "lib.rs", *sorted((ROOT / "bin").glob("*.rs"))]
    compiled: set[pathlib.Path] = set()
    for root in roots:
        if root.is_file():
            compiled |= reachable(root)
    if not compiled:
        print(f"{ROOT}: no crate root found", file=sys.stderr)
        return 1
    for source in sources:
        if source.resolve() not in compiled:
            failures.append(
                f"{source}: no `mod` declaration reaches it, so rustc never "
                f"sees it: its tests do not run and its lints do not fire"
            )

    lines_scanned = 0
    for source in sources:
        try:
            text = source.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as err:
            failures.append(f"{source}: cannot read: {err}")
            continue
        for number, line in enumerate(text.splitlines(), start=1):
            lines_scanned += 1
            if line.lstrip().startswith("//"):
                continue
            if ARM.match(line):
                failures.append(
                    f"{source}:{number}: a match arm on a string literal: "
                    f"{line.strip()}"
                )

    if not lines_scanned:
        print(f"{ROOT}: scanned no lines", file=sys.stderr)
        return 1

    for message in failures:
        print(message, file=sys.stderr)
    if failures:
        print(
            f"check-library: {len(failures)} failure(s)",
            file=sys.stderr,
        )
        return 1
    print(
        f"check-library: {len(sources)} source file(s), {lines_scanned} line(s); "
        f"all compiled, no match arm on a string literal"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
