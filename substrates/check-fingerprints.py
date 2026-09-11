#!/usr/bin/env python3
"""Every equipment entry's `hardware_fingerprint` covers everything its type declares.

WHY THIS IS A CHECK AND NOT A CONVENTION. The fingerprint is a digest over an
entry's declared hardware fields. The first version of it used one fixed field
list across every entry and silently covered TWO of twelve fields on the two
Apple machines, because their entries name `chip` and `model_identifier` where
the accelerator host names `accelerator` and `board`. It produced a
sixty-four-character digest and looked exactly like a working one.

That is the failure this file exists to make impossible to repeat: a fingerprint
covering fewer fields than its type declares is a FAILURE, not a smaller
fingerprint. The same mistake made silently a second time is indistinguishable
from the first, so it gets a test.

THE RULES, all four enforced rather than remembered:

  1. Every entry names an `entry_type` the registry declares.
  2. Every field that type declares is PRESENT on the entry.
  3. `hardware_fingerprint` is the sha256 of exactly those fields, canonically
     serialised -- so a reader recomputes it from published data.
  4. A declared field may not be an instance field (`os`, or anything naming a
     deployment) and may not be marked inferred. A hardware fingerprint that
     moves on an operating-system update is not one, and an unverified claim
     baked into an identifier is one nobody can correct later without changing
     the identity of every record that cites it.

Stdlib only. Exit 0 when every entry checks out, 1 when one does not, 2 when the
check cannot run at all.
"""

from __future__ import annotations

import hashlib
import json
import pathlib
import sys
import tomllib

REGISTRY = pathlib.Path(__file__).resolve().parent / "registry.toml"

EXIT_BAD = 1
EXIT_BROKEN = 2

# Field names that describe the INSTANCE rather than the equipment. Kept as a
# prefix list rather than an exact one so `os`, `os_deployment_version` and
# anything else spelled that way are all refused by the same rule.
INSTANCE_PREFIXES = ("os", "kernel", "deployment", "engine")


def digest_of(entry: dict, fields: list[str]) -> str:
    covered = {f: entry[f] for f in fields}
    return hashlib.sha256(
        json.dumps(covered, sort_keys=True, separators=(",", ":")).encode("utf-8")
    ).hexdigest()


def main(argv: list[str]) -> int:
    path = pathlib.Path(argv[0]) if argv else REGISTRY
    try:
        registry = tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError) as err:
        print(f"check-fingerprints: {path} cannot be read as TOML: {err}", file=sys.stderr)
        return EXIT_BROKEN

    types = registry.get("entry_type")
    machines = registry.get("equipment")
    if not types or not machines:
        print("check-fingerprints: the registry declares no entry types or no equipment; "
              "an empty registry and a working one must not report the same", file=sys.stderr)
        return EXIT_BROKEN

    bad = 0
    for name, entry in sorted(machines.items()):
        etype = entry.get("entry_type")
        if etype not in types:
            print(f"  {name}: entry_type {etype!r} is not declared"); bad += 1
            continue
        fields = types[etype].get("hardware_fields") or []
        if not fields:
            print(f"  {name}: entry type {etype!r} declares no hardware fields"); bad += 1
            continue

        for field in fields:
            if field.startswith(INSTANCE_PREFIXES):
                print(f"  {name}: {etype!r} declares `{field}`, which describes the "
                      f"instance rather than the machine"); bad += 1
            if entry.get(f"{field}_inferred") is True:
                print(f"  {name}: `{field}` is marked inferred and cannot be part of "
                      f"an identifier"); bad += 1

        missing = [f for f in fields if f not in entry]
        if missing:
            print(f"  {name}: {etype!r} declares {len(fields)} hardware field(s) and "
                  f"{len(missing)} are absent: {', '.join(missing)}"); bad += 1
            continue

        stated = entry.get("hardware_fingerprint")
        want = digest_of(entry, fields)
        if stated != want:
            print(f"  {name}: hardware_fingerprint is {stated}, the {len(fields)} declared "
                  f"field(s) hash to {want}"); bad += 1
            continue
        print(f"  {name}: {len(fields)} declared field(s), all present, digest agrees")

    print(f"check-fingerprints: {len(machines)} entr(ies), {bad} problem(s)")
    return EXIT_BAD if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
