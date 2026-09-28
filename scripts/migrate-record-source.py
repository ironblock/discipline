#!/usr/bin/env python3
"""Add the now-required `source` to every `start` row in the committed corpora.

`Start.source` was ruled required: an absent field meaning `live` is absence
used as a value. Every record written before that ruling is a record without
it, so they migrate -- and they migrate BY MACHINE, because eighty-odd hand
edits is eighty-odd chances to change a fixture in a way nobody reviews.

TEXTUAL, NOT A PARSE-AND-REDUMP, and that is the whole design. Several
fixtures exist to test bytes rather than structure -- CRLF line endings,
blank lines and indentation, a missing trailing newline, string escapes --
and a script that read them as JSON and wrote them back would silently
normalise away the exact thing each was committed to check. So this inserts
one member after the opening brace of a `start` row and touches nothing else.

Idempotent: a row that already declares a source is left alone.
"""

import re
import sys
from pathlib import Path

MEMBER = '"source":{"kind":"live"},'
# A `start` row, with or without spaces after the colon. Anchored to the
# `record` key so a row merely CONTAINING the word start is not matched.
START = re.compile(r'"record"\s*:\s*"start"')
HAS_SOURCE = re.compile(r'"source"\s*:')


def migrate(path: Path) -> int | None:
    """Insert the member into each start row of `path`.

    Returns the number of rows changed, or None for a file that is not UTF-8.
    The corpus deliberately contains one such file -- a record that is not
    text is one of the things `check-record` refuses -- and a migration that
    tried to rewrite it would either crash or destroy the bytes it exists to
    carry. Skipped, and counted as skipped rather than silently.
    """
    # newline="" so CRLF survives the round trip; a fixture that tests line
    # endings must not have them rewritten by the tool that migrates it.
    try:
        with open(path, encoding="utf-8", newline="") as handle:
            original = handle.read()
    except UnicodeDecodeError:
        return None
    out, changed = [], 0
    for line in original.splitlines(keepends=True):
        if START.search(line) and not HAS_SOURCE.search(line):
            brace = line.index("{")
            line = line[: brace + 1] + MEMBER + line[brace + 1 :]
            changed += 1
        out.append(line)
    if changed:
        with open(path, "w", encoding="utf-8", newline="") as handle:
            handle.write("".join(out))
    return changed


def main(roots: list[str]) -> int:
    files, rows, skipped = 0, 0, []
    for root in roots:
        for path in sorted(Path(root).rglob("*.jsonl")):
            changed = migrate(path)
            if changed is None:
                skipped.append(path)
            elif changed:
                files += 1
                rows += changed
    print(f"{files} files, {rows} start rows, source = live added")
    for path in skipped:
        print(f"skipped, not UTF-8: {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:] or ["diet"]))
