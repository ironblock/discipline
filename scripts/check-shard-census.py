#!/usr/bin/env python3
"""Add a sharded check's shards back up from what they RAN, and refuse if they
do not make the whole (#262, ruled on #268).

A listing proves what a check would run under a shard; it cannot prove what
the run did. Four reviews of #268 each found a way for the two to differ --
an edit one statement below the listing's `return`, a wrapper that passes a
different shard when it is not listing -- and every one left a shard green
having run less than its share. So each shard's run records the members its
loop reached, as it reaches them (`gatelib.record_ran`), and this script
checks the claim the split rests on, as check-selftest-census.py does for the
selftest:

  * every declared shard reported, under `members-<check>-<k>/`, and nothing
    else did -- a missing census is a missing job;
  * every row is `ran<TAB>member<TAB>outcome`, written from what the
    member's work returned (under --dry, the local proof, `dry<TAB>member`
    and nothing else);
  * the members the shards ran, together, are exactly the unsplit listing --
    asked of verify.sh itself, not recomputed here -- none skipped, none run
    twice, none the unsplit check does not run.

Usage:  check-shard-census.py DIR [--dry]

Stdlib only. Exit 0 if every sharded check's shards make its whole, 1
otherwise, 2 on misuse.
"""

from __future__ import annotations

import os
import pathlib
import re
import subprocess
import sys

import gatelib

ROOT = pathlib.Path(__file__).resolve().parent.parent
OWNERS = ROOT / ".github" / "check-owners.tsv"
VERIFY = ROOT / "verify.sh"
EXIT_LISTED = 3
# A real run's row names the member and the outcome its work returned; a dry
# run's names the member alone.
ROW = re.compile(r"ran\t([^\t]+)\t([a-z]+)|dry\t([^\t]+)")
ARTIFACT = re.compile(r"members-([a-z0-9-]+)-([1-9][0-9]*)")
RAN = "members-ran.tsv"


def declared_shards() -> dict[str, int]:
    """Each check check-owners.tsv splits, and into how many shards."""
    shards = {}
    for line in OWNERS.read_text(encoding="utf-8").split("\n"):
        parts = line.split("\t")
        if line.startswith("#") or len(parts) < 3 or not parts[2].strip():
            continue
        if re.fullmatch(r"[1-9][0-9]*", parts[2]):
            shards[parts[0]] = int(parts[2])
    return shards


def unsplit(check: str) -> tuple[list[str] | None, str]:
    """The members `check` runs unsplit, as verify.sh lists them."""
    env = {k: v for k, v in os.environ.items() if k not in ("VERIFY_CHECK_SHARD", gatelib.MEMBERS_RAN, gatelib.CENSUS_DRY)}
    env["VERIFY_LIST_MEMBERS"] = "1"
    done = subprocess.run(
        ["bash", str(VERIFY), "--only", check], cwd=ROOT, env=env, capture_output=True, text=True
    )
    members = gatelib.members_listed(done.stdout, check)
    if done.returncode != EXIT_LISTED or members is None:
        return None, (
            f"`{check}`: verify.sh --only {check} under VERIFY_LIST_MEMBERS exited "
            f"{done.returncode}, not {EXIT_LISTED} with its members LISTED: "
            f"{(done.stdout + done.stderr).strip()[-200:]}"
        )
    return members, ""


def main(argv: list[str]) -> int:
    dry = "--dry" in argv
    args = [a for a in argv if a != "--dry"]
    if len(args) != 1 or not pathlib.Path(args[0]).is_dir():
        print(__doc__.split("\n\n")[-2], file=sys.stderr)
        return 2
    root = pathlib.Path(args[0])
    shards = declared_shards()
    failures: list[str] = []
    reported: dict[tuple[str, int], list[str]] = {}
    for entry in sorted(root.iterdir()):
        named = ARTIFACT.fullmatch(entry.name)
        if not named or not entry.is_dir():
            failures.append(f"{entry.name}: not a `members-<check>-<k>` census; nothing else belongs here")
            continue
        check, part = named.group(1), int(named.group(2))
        if check not in shards or part > shards[check]:
            failures.append(f"{entry.name}: `{check}` declares {shards.get(check, 'no')} shard(s); this census is of none of them")
            continue
        path = entry / RAN
        try:
            text = path.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError) as err:
            failures.append(f"{entry.name}: no readable {RAN}: {err}")
            continue
        members = []
        for number, line in enumerate(text.split("\n"), 1):
            if not line:
                continue
            row = ROW.fullmatch(line)
            if not row:
                failures.append(f"{entry.name}/{RAN}:{number}: not `ran<TAB>member<TAB>outcome` or `dry<TAB>member`")
            elif row.group(3) is not None and not dry:
                failures.append(
                    f"{entry.name}/{RAN}:{number}: a DRY row -- this shard recorded its members "
                    f"and ran none of them, which is not a run"
                )
            elif row.group(3) is None and dry:
                # The local proof is of the dry loop's own rows (#268's fifth
                # review): a `ran` row there was written by something else.
                failures.append(f"{entry.name}/{RAN}:{number}: a `ran` row in a dry census")
            elif row.group(1) is not None and row.group(2) not in gatelib.RAN_OUTCOMES.get(check, ()):
                failures.append(
                    f"{entry.name}/{RAN}:{number}: `{row.group(1)}` came back `{row.group(2)}`, which "
                    f"is not an outcome `{check}` runs a member to "
                    f"({', '.join(sorted(gatelib.RAN_OUTCOMES.get(check, ())))}); a member that did not run "
                    f"is not counted as run"
                )
            else:
                members.append(row.group(1) or row.group(3))
        reported[(check, part)] = members
    for check, count in sorted(shards.items()):
        absent = [k for k in range(1, count + 1) if (check, k) not in reported]
        if absent:
            failures.append(
                f"`{check}` declares {count} shards, and shard(s) {', '.join(map(str, absent))} "
                f"reported no census; a shard that never reported cannot have run its share"
            )
            continue
        whole, why = unsplit(check)
        if whole is None:
            failures.append(why)
            continue
        failures += gatelib.split_failures(check, count, whole, [reported[(check, k)] for k in range(1, count + 1)])
        if not failures:
            print(f"check-shard-census: `{check}`: {count} shard(s) ran {len(whole)} member(s), each exactly once")
    for failure in failures:
        print(failure, file=sys.stderr)
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
