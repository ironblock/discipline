#!/usr/bin/env python3
"""Copy a list of a tree's paths into another directory, as the gate's
sandboxes need them (#380).

    copy-tree.py SRC DEST     the paths, relative to SRC, NUL-separated on stdin

Each path's parent directories are made under DEST. The file is copied with
its links followed, and its mode is kept. Its modification time is NOT
kept: the copy is new.

That last part is the point. A sandbox's sources must look newer than any
artifact a previous case left in the shared cargo target, or cargo declares
the target fresh and runs the wrong binary (verify.sh's `sandbox()`). The
mode matters too, because a copied script is run by name. So this is
`copyfile` and then the source's mode, never `copy2`.

It replaces `cp -L --parents`, which BSD `cp` refuses, in verify.sh's
sandbox and in check-injections.py, which calls `copy_tree` in-process.
It runs in one process for the whole list, as the single batched `cp` did.

Stdlib only. Exit 0 with every path copied; 1 if one cannot be, naming it;
2 on misuse, or on an empty list -- a copy of nothing is not a sandbox.
"""

from __future__ import annotations

import os
import pathlib
import shutil
import stat
import sys

EXIT_FAILED = 1
EXIT_MISUSE = 2


def copy_tree(src: pathlib.Path, dest: pathlib.Path, paths: list[str]) -> int:
    """Copy each path under `src` to the same path under `dest`, following
    links, keeping the mode, with a new mtime. Returns how many were copied.
    An `OSError` names its path."""
    # Each directory made once and each source stated once: the per-file
    # cost is the copy itself, as it was for one batched `cp`.
    # Plain strings, not Path objects: per file, the copy is the cost.
    root, into = os.fspath(src), os.fspath(dest)
    made: set[str] = set()
    for path in paths:
        source, target = os.path.join(root, path), os.path.join(into, path)
        parent = os.path.dirname(target)
        if parent not in made:
            os.makedirs(parent, exist_ok=True)
            made.add(parent)
        mode = os.stat(source).st_mode
        shutil.copyfile(source, target)
        os.chmod(target, stat.S_IMODE(mode))
    return len(paths)


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print(__doc__.strip().split("\n\n")[1], file=sys.stderr)
        return EXIT_MISUSE
    src, dest = pathlib.Path(argv[0]), pathlib.Path(argv[1])
    paths = [p for p in sys.stdin.buffer.read().decode("utf-8").split("\0") if p]
    if not paths:
        print("copy-tree: no paths given; a copy of nothing is not a sandbox", file=sys.stderr)
        return EXIT_MISUSE
    try:
        copy_tree(src, dest, paths)
    except OSError as err:
        print(f"copy-tree: {err}", file=sys.stderr)
        return EXIT_FAILED
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
