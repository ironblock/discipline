#!/usr/bin/env python3
"""The prose of a text file whose embedded images are blanked (#278).

An SVG's `<feImage href="data:image/png;base64,...">` carries a PNG: binary
bytes in text clothing. The loose hygiene patterns -- `internal-ticket-id`
above all -- find their shapes in base64 at about the rate #233 measured in
random bytes, and #233 ruled that a shape inside bytes names nothing. So the
loose patterns read a PROSE view of such a file, the image payloads blanked
to spaces (line and column numbers unchanged). The credential (`b`) rows are
not this script's business: they still read every original file, whole, as
#240 left them, so no credential is cut in two at a view's edge.

A payload is blanked only when it is an image, by its bytes and not by its
label (#279's review: a made-up `data:` prefix hid readable text that
followed it). All of:

  * `data:image/png|jpeg|gif|webp;base64,` then a run of base64 characters
    that ends at the end of the line, a quote, `)`, `>` or whitespace -- a
    run followed by `-`, `_`, `.`, `@`, `:` or a backslash is not a payload;
  * the run decodes as strict base64;
  * the decoded bytes begin AND end as that format does: PNG's signature
    and IEND chunk, JPEG's SOI and EOI markers, GIF's header and trailer,
    WebP's RIFF header whose size is the payload's.

Anything else is prose, read by every pattern. A payload built to pass all
of this while carrying text is deliberate encoding, which no scan of
committed bytes catches; the gate is for what leaks by accident.

Input: paths on stdin, NUL-separated. Output: for each, in order, the path
to scan with the loose patterns -- a prose view written under `--into`, or
the path itself when nothing in it is blanked -- NUL-separated.

Stdlib only. Exit 0 when every view is written, 2 when one cannot be.
"""

from __future__ import annotations

import argparse
import base64
import binascii
import os
import re
import sys

PAYLOAD = re.compile(
    r"data:image/(png|jpeg|gif|webp)(?:;[A-Za-z0-9.+=-]+)*;base64,([A-Za-z0-9+/]+={0,2})(?=[\"')>\s]|$)"
)


def is_image(kind: str, payload: str) -> bool:
    """Whether `payload` decodes, strictly, to bytes shaped like `kind`."""
    try:
        data = base64.b64decode(payload, validate=True)
    except (binascii.Error, ValueError):
        return False
    if kind == "png":
        return data.startswith(b"\x89PNG\r\n\x1a\n") and data.endswith(b"IEND\xaeB`\x82")
    if kind == "jpeg":
        return data.startswith(b"\xff\xd8\xff") and data.endswith(b"\xff\xd9")
    if kind == "gif":
        return data[:6] in (b"GIF87a", b"GIF89a") and data.endswith(b"\x3b")
    return (
        len(data) >= 12 and data[:4] == b"RIFF" and data[8:12] == b"WEBP"
        and int.from_bytes(data[4:8], "little") + 8 == len(data)
    )


def prose(text: str) -> str | None:
    """`text` with every image payload blanked, or None if it has none."""
    if ";base64," not in text:
        return None
    lines, found = [], False
    for line in text.split("\n"):
        for match in PAYLOAD.finditer(line):
            if is_image(match.group(1), match.group(2)):
                start, end = match.span(2)
                line = line[:start] + " " * (end - start) + line[end:]
                found = True
        lines.append(line)
    return "\n".join(lines) if found else None


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--into", required=True, help="where to write the prose views")
    args = parser.parse_args(argv)
    paths = [p for p in sys.stdin.buffer.read().split(b"\0") if p]
    out = sys.stdout.buffer
    for number, raw in enumerate(paths):
        path = os.fsdecode(raw)
        try:
            with open(path, "rb") as handle:
                text = handle.read().decode("utf-8", errors="surrogateescape")
        except OSError as err:
            print(f"hygiene-datauri: cannot read {path}: {err}", file=sys.stderr)
            return 2
        view = prose(text)
        if view is None:
            out.write(raw + b"\0")
            continue
        target = os.path.join(args.into, f"prose-{number}")
        try:
            with open(target, "wb") as handle:
                handle.write(view.encode("utf-8", errors="surrogateescape"))
        except OSError as err:
            print(f"hygiene-datauri: cannot write the prose of {path}: {err}", file=sys.stderr)
            return 2
        out.write(os.fsencode(target) + b"\0")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
