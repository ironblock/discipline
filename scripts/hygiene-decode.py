#!/usr/bin/env python3
"""Materialise the decoded view of every file that has one.

`hygiene.sh` scans files with `grep`, and `grep` reads bytes. The bytes of a
captured log are JSON-escaped, so a pattern that guards prose does not guard
logs -- which is where the artefacts live. This writes a MIRROR: for every
input file whose content carries encoded strings, a file under `--into`
holding those strings decoded, at the same relative path.

The scanner then greps the mirror beside the tree and says `(decoded)` when it
reports a hit, so one pattern table covers both views and no pattern has to be
loosened to reach through an escape.

Reads NUL-separated paths on stdin, so a filename may hold anything a
filename may hold. Prints how many views it wrote.

Stdlib only. Exit 0 when the mirror is written, 2 when it cannot be.
"""

from __future__ import annotations

import argparse
import pathlib
import sys

EXIT_BROKEN = 2

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
try:
    import decoding
except ModuleNotFoundError:  # pragma: no cover -- an operator error, not a bug
    # EXIT_BROKEN, not `sys.exit(message)`, which exits one. This script's
    # caller reads a nonzero exit as "the decode phase broke", so the number
    # matters less here than it does in check-hashes.py -- but the two scripts
    # spell the same failure the same way or the next reader has to check.
    print("hygiene-decode: decoding.py is not beside this script", file=sys.stderr)
    sys.exit(EXIT_BROKEN)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--into", required=True, help="where to write the mirror")
    args = parser.parse_args(argv)

    into = pathlib.Path(args.into)
    try:
        into.mkdir(parents=True, exist_ok=True)
    except OSError as err:
        print(f"hygiene-decode: cannot make {into}: {err}", file=sys.stderr)
        return EXIT_BROKEN

    written = 0
    for name in sys.stdin.buffer.read().split(b"\0"):
        if not name:
            continue
        path = pathlib.Path(name.decode("utf-8", "surrogateescape"))
        try:
            # `errors="replace"`, and NOT a skip on UnicodeDecodeError -- the
            # same correction `check-hashes.py` already carries, for the same
            # reason, in the same tree. THE SKIP DROPPED THE WHOLE FILE OVER
            # ONE BYTE: a cp1252 curly quote anywhere in a JSON log meant no
            # decoded view for any of it, and a token welded to an escape one
            # line away went unseen while the gate printed `1 file(s) clean`.
            #
            # The justification written here was that the caller still scans
            # the bytes. That is the one thing that cannot rescue this: a
            # token welded to `\n` is exactly what the bytes DO NOT show, and
            # undoing that weld is this view's whole job. Measured -- same
            # token, same file, one byte's difference:
            #
            #   pure UTF-8       internal-ticket-id: (decoded) ...:3   exit 1
            #   + one cp1252     1 file(s) clean, 0 decoded view(s)    exit 0
            #
            # U+FFFD is not alphanumeric, so it separates like any other
            # non-token byte: it can split a token that spanned the bad byte,
            # and it cannot invent one that was not there.
            #
            # SIX TRACKED FILES in this repository are already undecodable,
            # one of them a JSONL record fixture, so this was not hypothetical.
            # Over `git ls-files` on this branch the skip cost one view:
            # 409 before, 410 after, the difference being that fixture --
            # `diet/formats/record/fixtures/invalid/not-utf8.jsonl`, the file
            # whose whole purpose is to hold bytes a reader must survive.
            text = path.read_text(encoding="utf-8", errors="replace")
        except OSError:
            # Unreadable is a different thing from undecodable, and stays a
            # skip: there are no bytes to make a view out of.
            continue
        view = decoding.decoded_text(text)
        if not view:
            continue
        # A relative path keeps its shape under the mirror so a report can
        # name the file it came from by stripping one prefix.
        mirror = into / str(path).lstrip("/")
        try:
            mirror.parent.mkdir(parents=True, exist_ok=True)
            # `errors="replace"`: a JSON string may hold a LONE SURROGATE --
            # one committed fixture does, on purpose -- and that is not UTF-8,
            # so an exact write raises and takes the whole mirror with it. The
            # replacement character cannot hide a forbidden token, since every
            # shape the tables look for is ASCII and survives beside it.
            with open(mirror, "w", encoding="utf-8", errors="replace") as out:
                out.write(view)
        except OSError as err:
            print(f"hygiene-decode: cannot write {mirror}: {err}", file=sys.stderr)
            return EXIT_BROKEN
        written += 1

    print(written)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
