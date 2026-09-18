#!/usr/bin/env python3
"""Materialise the decoded, normalised view of every file that has one.

`hygiene.sh` scans files with `grep`, and `grep` reads bytes. The bytes of a
captured log are JSON-escaped, so a pattern that guards prose does not guard
logs -- which is where the artefacts live. This writes a MIRROR: for every
input file whose content carries encoded strings, or whose bytes are not
already what `decoding.normalized` would make of them, a file under `--into`
holding that content, at the same relative path.

TWO THINGS GO IN THE SAME MIRROR, and both are ruled 2026-09-12 on #72:

  * `decoding.decoded_text` -- every string a JSON escape hid, unwelding a
    token that landed right after `\n` the way a captured log writes it.
  * `decoding.normalized` -- NFC composition and the invisible-character
    strip, applied to BOTH the decoded strings above and the file's own
    content, so a shaped literal broken by a zero-width space, or split
    across an accent's two spellings, reaches the pattern half exactly as it
    already reached the digest half's tokeniser.

Before this, the pattern half of the gate -- `hygiene.sh`'s shaped literals,
matched by `grep` -- had NEITHER: it saw only the JSON-unescaped strings, not
the file's own bytes, and neither view was normalised. `decoding.py`'s own
comment named that a declared gap; this closes it, and the file's mirror
picture is written from the same `normalized()` the digest half calls.

The scanner then greps the mirror beside the tree and says `(decoded)` when it
reports a hit, so one pattern table covers both views and no pattern has to be
loosened to reach through an escape or a stray invisible character.

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
            name_bytes = path.read_bytes()
            text = name_bytes.decode("utf-8", errors="replace")
        except OSError:
            # Unreadable is a different thing from undecodable, and stays a
            # skip: there are no bytes to make a view out of.
            continue
        # DECODE FIRST, THEN NORMALISE. An escaped newline still welds a
        # token in an NFC string, so normalising ahead of the JSON unescape
        # would leave it welded; decoding ahead of normalising does not,
        # because by the time a code point can be composed or stripped, the
        # escape that hid it from `grep` is already gone.
        decoded = decoding.decoded_text(text)
        parts = []
        if decoded:
            parts.append(decoding.normalized(decoded))
        # The file's OWN content, normalised -- but ONLY for a file that is
        # TEXT to begin with. A true binary decoded with `errors="replace"`
        # is not prose with one stray byte; it is noise `unicodedata` reads
        # as thousands of code points, some of them, by chance, ignorable or
        # combining. Normalising it anyway manufactured a NEW mirror of
        # scanner-visible "text" out of `/dev/urandom` and an ordinary ELF
        # binary, and one of hygiene-patterns.tsv's loose heuristics found a
        # false positive in it -- caught by the fixture built for exactly
        # this, `an ordinary binary does not false-positive`.
        #
        # The `\0` TEST, not a repeat of `grep -I`'s own heuristic: this
        # runs before hygiene.sh's binary/text split even exists, on every
        # scanned path at once, and a NUL byte is the one signal genuine
        # prose never carries while true binary content almost always does
        # inside any reasonably sized sample -- 64KB of uniform random bytes
        # carries one with probability indistinguishable from 1.
        if b"\0" not in name_bytes:
            raw_normalized = decoding.normalized(text)
            if raw_normalized != text:
                parts.append(raw_normalized)
        if not parts:
            continue
        view = "\n".join(parts)
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
