#!/usr/bin/env python3
"""Add the selftest's shards back up, and refuse if they do not make a whole.

`verify.sh --selftest --shard K/N` runs every Nth fault starting at the Kth.
That is a division of labour, not a selection of faults -- but the difference
between those two is invisible from inside any one shard, because a shard that
ran nothing and a shard that ran its share both exit 0 and both say `success`.

So each shard writes down which faults it ran, by ordinal, and this script
checks the claim the sharding rests on:

  * every shard reported -- a missing census is a missing job, and a job that
    never started cannot have passed;
  * the shards agree on how many there are and how many faults exist, because
    two shards disagreeing about the total means they partitioned different
    lists;
  * the ordinals they ran, taken together, are exactly 1..total -- none
    skipped, none run twice;
  * and `total` is what the manifest says must go red in the selftest, asked
    of check-fault-manifest.py rather than recomputed here. A census that adds
    up perfectly to the wrong number is the failure this is really for: it is
    what a shard flag would look like if it had quietly stopped enumerating
    part of the manifest.

Requiring every shard's literal `success` is the workflow's job and is a
different claim -- that each shard's faults went red. This script's claim is
that between them they were the whole list. Neither implies the other.

Usage:  check-selftest-census.py DIR

Stdlib only. Exit 0 if the shards make a whole, 1 otherwise, 2 on misuse.
"""

from __future__ import annotations

import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
MANIFEST_READER = ROOT / "scripts" / "check-fault-manifest.py"

SCALARS = ("shard", "shards", "total")


class Census:
    """One shard's report, parsed rather than trusted."""

    def __init__(self, path: pathlib.Path) -> None:
        self.path = path
        self.scalars: dict[str, int] = {}
        self.ordinals: list[int] = []
        self.errors: list[str] = []
        self._parse()

    def _parse(self) -> None:
        try:
            text = self.path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as err:
            self.errors.append(f"{self.path.name}: unreadable: {err}")
            return
        for number, line in enumerate(text.split("\n"), 1):
            if not line.strip():
                continue
            parts = line.split("\t")
            if len(parts) != 2:
                self.errors.append(f"{self.path.name}:{number}: not `key<TAB>value`")
                continue
            key, raw = parts[0].strip(), parts[1].strip()
            try:
                value = int(raw)
            except ValueError:
                self.errors.append(f"{self.path.name}:{number}: `{raw}` is not a number")
                continue
            if key in SCALARS:
                if key in self.scalars:
                    self.errors.append(f"{self.path.name}:{number}: `{key}` given twice")
                self.scalars[key] = value
            elif key == "ordinal":
                self.ordinals.append(value)
            else:
                self.errors.append(f"{self.path.name}:{number}: unknown key `{key}`")
        for key in SCALARS:
            if key not in self.scalars:
                self.errors.append(f"{self.path.name}: no `{key}` line")


def manifest_total(failures: list[str]) -> int | None:
    """What the manifest says must go red in the selftest.

    Asked of the manifest's own reader. Deriving it here -- by parsing
    faults.toml, or by subtracting the per-run kinds from `--count-red` --
    would be a second opinion about a number, which is the defect this
    repository names most often.
    """
    try:
        done = subprocess.run(
            [sys.executable, str(MANIFEST_READER), "--count-selftest-red"],
            capture_output=True,
            text=True,
            check=True,
        )
    except (OSError, subprocess.CalledProcessError) as err:
        failures.append(f"cannot ask {MANIFEST_READER.name} for the count: {err}")
        return None
    try:
        return int(done.stdout.strip())
    except ValueError:
        failures.append(f"{MANIFEST_READER.name} did not answer with a number")
        return None


def main(argv: list[str]) -> int:
    args = [a for a in argv[1:] if not a.startswith("--")]
    for flag in argv[1:]:
        if flag.startswith("--"):
            print(f"unknown flag: {flag}", file=sys.stderr)
            return 2
    if len(args) != 1:
        print(__doc__.strip().split("\n")[-3], file=sys.stderr)
        return 2

    root = pathlib.Path(args[0])
    if not root.is_dir():
        print(f"{root}: not a directory", file=sys.stderr)
        return 2

    failures: list[str] = []

    # Recursive: `actions/download-artifact` gives each artifact a directory of
    # its own, so a flat glob would find nothing and report zero shards -- a
    # wrong answer that looks like an empty run rather than a broken read.
    files = sorted(p for p in root.rglob("*") if p.is_file())
    if not files:
        print(f"{root}: no census files, so no shard reported", file=sys.stderr)
        return 1

    reports = [Census(p) for p in files]
    for report in reports:
        failures.extend(report.errors)
    reports = [r for r in reports if not r.errors]
    if not reports:
        for message in failures:
            print(message, file=sys.stderr)
        print("check-selftest-census: no census could be read", file=sys.stderr)
        return 1

    shard_counts = {r.scalars["shards"] for r in reports}
    totals = {r.scalars["total"] for r in reports}
    if len(shard_counts) != 1:
        failures.append(f"the shards disagree about how many there are: {sorted(shard_counts)}")
    if len(totals) != 1:
        failures.append(
            f"the shards disagree about how many faults exist: {sorted(totals)}; "
            f"they partitioned different lists"
        )

    total = min(totals)
    declared = manifest_total(failures)
    if declared is not None and len(totals) == 1 and declared != total:
        failures.append(
            f"the shards enumerated {total} fault(s); the manifest says "
            f"{declared} must go red in the selftest"
        )

    seen_shards = [r.scalars["shard"] for r in reports]
    if len(shard_counts) == 1:
        # How many shards there are is the shards' own claim, and they must
        # agree on it. The caller does not restate it: the matrix that spawns
        # them is the one place the number is written, and a second copy here
        # would be a number with two readers.
        want = min(shard_counts)
        for number in range(1, want + 1):
            if seen_shards.count(number) == 0:
                failures.append(f"shard {number} of {want} filed no census")
            elif seen_shards.count(number) > 1:
                failures.append(f"shard {number} of {want} filed {seen_shards.count(number)} censuses")
        for number in sorted(set(seen_shards)):
            if number < 1 or number > want:
                failures.append(f"a census claims to be shard {number} of {want}")

    # The partition itself. A count would pass a run in which one shard ran a
    # fault twice while another skipped one -- the arithmetic works and a fault
    # went unproven, which is exactly the trade this issue refuses.
    ran: dict[int, list[int]] = {}
    for report in reports:
        for ordinal in report.ordinals:
            ran.setdefault(ordinal, []).append(report.scalars["shard"])

    if len(totals) == 1:
        missing = [n for n in range(1, total + 1) if n not in ran]
        if missing:
            failures.append(
                f"{len(missing)} fault(s) were run by no shard: "
                + ", ".join(str(n) for n in missing[:20])
                + (" ..." if len(missing) > 20 else "")
            )
    twice = {n: s for n, s in ran.items() if len(s) > 1}
    if twice:
        failures.append(
            f"{len(twice)} fault(s) were run by more than one shard: "
            + ", ".join(f"{n} (shards {sorted(s)})" for n, s in sorted(twice.items())[:10])
        )
    stray = [n for n in ran if len(totals) == 1 and (n < 1 or n > total)]
    if stray:
        failures.append(f"ordinals outside 1..{total} were reported: {sorted(stray)[:10]}")

    for message in failures:
        print(message, file=sys.stderr)
    if failures:
        print(f"check-selftest-census: {len(failures)} failure(s)", file=sys.stderr)
        return 1

    print(
        f"check-selftest-census: {len(reports)} shard(s) ran "
        f"{len(ran)} of {total} fault(s), each exactly once"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
