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

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))
try:
    import decoding
except ModuleNotFoundError:  # pragma: no cover -- an operator error, not a bug
    sys.exit("hygiene-decode: decoding.py is not beside this script")

EXIT_BROKEN = 2


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
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            # Not decodable text, so it has no decoded view. The bytes
            # themselves are still scanned by the caller; this is a view
            # beside them, never instead of them.
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
