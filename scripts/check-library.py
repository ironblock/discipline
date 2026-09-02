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
``diet/src/`` may have a string literal anywhere in its pattern**, and no
``matches!`` may either. A string becomes a kind in exactly one place per enum
-- a ``from_tag`` that iterates ``ALL`` and compares against each variant's own
tag -- and that place is caught by the compiler when a variant is added.

How it looks, and why not by line. The first version of this rule was one
regex anchored to the start of a line, which meant it saw ``"A" => 1`` and not
``Some("A") => 1``, ``("A", true) => 1``, ``["A", ..] => 1``, ``&"A" => 1``,
``"A" if turn == 1 => 1`` (the guard's own ``==`` defeated it), or an
or-pattern long enough that ``cargo fmt`` breaks it across lines -- which is
the shape the formatter produces unprompted for any realistic tag vocabulary,
so the rule's coverage depended on how long the identifiers were. It now masks
comments and literal interiors, then walks back from each ``=>`` to its arm
boundary and asks whether a string literal begins anywhere in the pattern.
That sees every shape above, because it does not care what the pattern looks
like -- only where it ends.

Scope, and what it does not cover. ``diet/tests/`` is not scanned: the
conformance harness dispatches on a format NAME, which is not a field kind, a
verdict or an outcome class, and it panics on a name it does not know. That is
a guard rather than a drift, but it is a string match, and the honest
statement is that this script does not look at it. A ``==`` comparison against
a literal is out of reach and stays out of reach on purpose: the library's
eight of them are file-extension filters and test assertions, not predicates
over a vocabulary, and a rule that flagged them would be worse than the one it
replaced. The arm is what this catches.

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

# A `mod foo;` declaration -- the only thing that puts a file in the crate.
MOD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;", re.M)

# `#[path = "elsewhere.rs"] mod name;` -- rustc reads the named file instead of
# the one the module name implies. Rare, and a rule that called it orphaned
# would block legitimate code.
PATH_MOD = re.compile(
    r'#\[\s*path\s*=\s*"([^"]+)"\s*\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+[A-Za-z_][A-Za-z0-9_]*\s*;'
)

CHAR = re.compile(r"'(?:\\.[^']*|[^'\\])'")


def mask(text: str) -> tuple[str, set[int]]:
    """Blank out comments and literal interiors; report where literals start.

    Returns the masked text -- same length, so every index still lines up with
    the original -- and the set of indices at which a string literal begins.
    Structure is read off the masked text so that a `=>` or a brace inside a
    string or a comment cannot be mistaken for the real thing.
    """
    out = list(text)
    starts: set[int] = set()
    i, n = 0, len(text)

    def blank(lo: int, hi: int) -> None:
        for j in range(lo, min(hi, n)):
            if out[j] != "\n":
                out[j] = " "

    while i < n:
        rest = text[i:]
        if rest.startswith("//"):
            end = text.find("\n", i)
            end = n if end == -1 else end
            blank(i, end)
            i = end
            continue
        if rest.startswith("/*"):
            # Rust block comments nest.
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth += 1
                    j += 2
                elif text.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            blank(i, j)
            i = j
            continue
        raw = re.match(r'(b?r)(#*)"', rest)
        if raw:
            hashes = raw.group(2)
            close = '"' + hashes
            end = text.find(close, i + raw.end())
            end = n if end == -1 else end + len(close)
            starts.add(i)
            blank(i, end)
            i = end
            continue
        if rest.startswith('"') or rest.startswith('b"'):
            start = i
            j = i + (2 if rest.startswith('b"') else 1)
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == '"':
                    j += 1
                    break
                j += 1
            starts.add(start)
            blank(start, j)
            i = j
            continue
        if text[i] == "'":
            # A char literal, or a lifetime. Only the first has a closing
            # quote a couple of characters along.
            found = CHAR.match(text, i)
            if found:
                blank(i, found.end())
                i = found.end()
                continue
        i += 1
    return "".join(out), starts


def _guard_start(masked: str, start: int, end: int) -> int:
    """Where a pattern stops and its `if` guard begins, or `end`."""
    depth = 0
    for found in re.finditer(r"\bif\b", masked[start:end]):
        here = start + found.start()
        depth = 0
        for char in masked[start:here]:
            if char in "([{":
                depth += 1
            elif char in ")]}":
                depth -= 1
        if depth == 0:
            return here
    return end


def arm_patterns(masked: str) -> list[tuple[int, int]]:
    """The span of every match arm's pattern, guard excluded.

    Walks each `match` block splitting arms the way rustc does -- at a `,` or
    at the close of a block-bodied arm -- rather than guessing from a line.
    A pattern can hold braces (`Self::X { a } =>`) and an arm body can hold
    string literals, so only the arm structure tells the two apart.
    """
    spans: list[tuple[int, int]] = []
    for keyword in re.finditer(r"\bmatch\b", masked):
        index, depth, brace = keyword.end(), 0, None
        while index < len(masked):
            char = masked[index]
            if char in "([":
                depth += 1
            elif char in ")]":
                depth -= 1
            elif char == "{":
                if depth == 0:
                    brace = index
                    break
                depth += 1
            index += 1
        if brace is None:
            continue
        index, depth, begin, arrow = brace + 1, 0, brace + 1, None
        while index < len(masked):
            char = masked[index]
            if char in "([{":
                depth += 1
            elif char in ")]}":
                if depth == 0 and char == "}":
                    break
                depth -= 1
                if depth == 0 and char == "}" and arrow is not None:
                    spans.append((begin, arrow))
                    arrow = None
                    index += 1
                    while index < len(masked) and masked[index].isspace():
                        index += 1
                    if index < len(masked) and masked[index] == ",":
                        index += 1
                    begin = index
                    continue
            elif char == "," and depth == 0:
                if arrow is not None:
                    spans.append((begin, arrow))
                    arrow = None
                begin = index + 1
            elif depth == 0 and masked.startswith("=>", index) and arrow is None:
                arrow = index
                index += 2
                continue
            index += 1
        if arrow is not None:
            spans.append((begin, arrow))
    return [(begin, _guard_start(masked, begin, arrow)) for begin, arrow in spans]


def matches_patterns(masked: str) -> list[tuple[int, int]]:
    """The pattern half of every `matches!(expr, PAT)` -- after the comma."""
    spans = []
    for call in re.finditer(r"\bmatches!\s*\(", masked):
        depth, i, comma = 1, call.end(), None
        while i < len(masked) and depth:
            char = masked[i]
            if char in "([{":
                depth += 1
            elif char in ")]}":
                depth -= 1
            elif char == "," and depth == 1 and comma is None:
                comma = i
            i += 1
        if comma is not None:
            spans.append((comma, _guard_start(masked, comma, i)))
    return spans


def line_of(text: str, index: int) -> int:
    """The 1-based line `index` falls on."""
    return text.count("\n", 0, index) + 1


def declared_modules(source: pathlib.Path) -> tuple[list[str], list[str]]:
    """The modules `source` declares: by name, and by explicit `#[path]`.

    Read off the masked text, so a declaration inside a comment does not count.
    A `mod` line commented out is exactly how a module goes uncompiled while
    everything stays green -- which is the failure rule two exists for, and it
    would have walked straight past a block comment.
    """
    try:
        text = source.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return [], []
    masked, _ = mask(text)
    # `#[path = "..."]` keeps its literal, which masking blanked; take those
    # from the original text and trust the masked text for the rest.
    paths = [
        found.group(1)
        for found in PATH_MOD.finditer(text)
        if masked[found.start() : found.end()].strip()
    ]
    return MOD.findall(masked), paths


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
        names, paths = declared_modules(source)
        candidates = [base / f"{name}.rs" for name in names]
        candidates += [base / name / "mod.rs" for name in names]
        candidates += [source.parent / path for path in paths]
        for candidate in candidates:
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
        lines_scanned += len(text.splitlines())
        masked, literals = mask(text)
        for start, end in arm_patterns(masked) + matches_patterns(masked):
            caught = sorted(index for index in literals if start <= index < end)
            if not caught:
                continue
            pattern = " ".join(text[start:end].split())
            failures.append(
                f"{source}:{line_of(text, caught[0])}: a match arm on a string "
                f"literal: {pattern[:90]}"
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
