#!/usr/bin/env python3
"""hygiene.sh stays honest on bash 3.2 (#236).

CI runs bash 5, where an empty array's "${a[@]}" is fine under `set -u`; a
stock Mac runs bash 3.2, where it aborts -- and an EXIT trap reached through
that abort sees $? = 0, so the scan read as clean having scanned nothing. CI
cannot run the old shell, so this reads the script for the three properties
that keep it honest there:

  1. every array expansion is guarded: ${a+"${a[@]}"}, never bare "${a[@]}";
  2. exactly one `trap ... EXIT`, so no second trap replaces the first;
  3. that trap maps an exit 0 to a failure unless `finished=1` ran, and
     `finished=1` is the script's last line.

Exit 0 when all hold, 1 naming each that does not, 2 if the file is unreadable.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent


def problems(text: str) -> list[str]:
    found = []
    for n, line in enumerate(text.splitlines(), 1):
        if line.lstrip().startswith("#"):
            continue
        for m in re.finditer(r'(?<![+])"\$\{([A-Za-z_]+)\[@\]\}"', line):
            found.append(f"line {n}: \"${{{m.group(1)}[@]}}\" is unguarded; bash 3.2 aborts on it under set -u")
    traps = [n for n, line in enumerate(text.splitlines(), 1)
             if re.match(r"\s*trap\s.*\bEXIT\b", line)]
    if len(traps) != 1:
        found.append(f"{len(traps)} EXIT trap(s) at line(s) {traps}; one, or a later one replaces the first")
    if not re.search(r'\[ "\$finished" = 1 \] \|\| \[ "\$rc" -ne 0 \] \|\| rc=', text):
        found.append("the EXIT trap does not map an unfinished exit 0 to a failure")
    last = [line for line in text.splitlines() if line.strip()][-1:]
    if last != ["finished=1"]:
        found.append(f"the last line is {last!r}, not finished=1")
    return found


def main(argv: list[str]) -> int:
    path = pathlib.Path(argv[0]) if argv else ROOT / "scripts" / "hygiene.sh"
    try:
        text = path.read_text(encoding="utf-8")
    except OSError as err:
        print(f"check-hygiene-portable: {err}", file=sys.stderr)
        return 2
    found = problems(text)
    for line in found:
        print(f"check-hygiene-portable: {path.name}: {line}", file=sys.stderr)
    if found:
        return 1
    print(f"check-hygiene-portable: {path.name} is guarded, trapped once, and finishes")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
