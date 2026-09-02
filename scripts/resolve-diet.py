#!/usr/bin/env python3
"""Resolve the `diet` binary, and refuse to guess which one you meant.

Anything that shells out to `diet` crosses a boundary, and a boundary with no
provenance is a boundary you cannot audit. Four instruments once produced
banked numbers through a release binary seven days behind its source; they
graded seventy-three controls green without a guard the source had carried all
week, and the documented way to pin a run to a specific build was a silent
no-op. The resolver they used picked the NEWER of two builds. That is the
behaviour this script exists not to have.

Three rules, and each of them is that incident:

  * ``DIET_BIN`` PINS THE BUILD. If it is set, that binary is used and no
    other, and a ``DIET_BIN`` that does not exist is an error rather than a
    fallback. A pin that can be silently ignored is not a pin.
  * TWO CANDIDATES IS A REFUSAL, NOT A CHOICE. With both ``target/debug/diet``
    and ``target/release/diet`` present, this exits 2. Choosing the newer is
    exactly what banked the wrong numbers, and choosing the older would be no
    better -- the caller knows which one it meant and has to say so.
  * A BINARY OLDER THAN ITS SOURCE DOES NOT REFLECT IT. If anything under
    ``diet/`` or the workspace manifests is newer than the binary, this exits
    2 rather than running a build that predates the code it claims to be.

On success it prints one JSON object -- the resolved path, its SHA-256, and
what it was checked against -- so the caller can record what it ran.

Stdlib only. Exit 0 on a resolved binary, 2 on anything else.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import pathlib
import sys
import tomllib

def target_dir() -> pathlib.Path:
    """Where cargo puts a build, asked the way cargo decides it.

    `CARGO_TARGET_DIR` is not the only answer, and assuming it was is how a
    gate ran a program cargo had not built: with `[build] target-dir` set in
    `.cargo/config.toml`, or `CARGO_BUILD_TARGET_DIR` in the environment,
    cargo builds elsewhere and anything left lying at `target/debug/diet` is
    what gets resolved -- with its SHA-256 recorded as provenance for an
    artifact that is not the one under test.

    Cargo's precedence, followed here: the two environment variables (which
    are two spellings of one setting), then `build.target-dir` from the
    nearest `.cargo/config.toml` walking up from the working directory, then
    `target`. A config's relative path is relative to the directory holding
    `.cargo/`, as cargo resolves it.
    """
    for variable in ("CARGO_TARGET_DIR", "CARGO_BUILD_TARGET_DIR"):
        named = os.environ.get(variable)
        if named:
            return pathlib.Path(named)
    here = pathlib.Path.cwd()
    for directory in (here, *here.parents):
        for name in ("config.toml", "config"):
            config = directory / ".cargo" / name
            if not config.is_file():
                continue
            try:
                with config.open("rb") as handle:
                    settings = tomllib.load(handle)
            except (OSError, tomllib.TOMLDecodeError):
                continue
            named = settings.get("build", {}).get("target-dir")
            if named:
                return directory / named
    return pathlib.Path("target")


def candidates() -> tuple[pathlib.Path, ...]:
    """The two paths a build of `diet` lands at."""
    root = target_dir()
    return (root / "debug" / "diet", root / "release" / "diet")

# What the binary is supposed to reflect. A change to any of these that the
# binary predates means the binary is not the code.
SOURCES = (
    pathlib.Path("diet"),
    pathlib.Path("Cargo.toml"),
    pathlib.Path("Cargo.lock"),
    pathlib.Path("rust-toolchain.toml"),
)

EXIT_REFUSED = 2


def refuse(message: str) -> int:
    print(f"resolve-diet: {message}", file=sys.stderr)
    return EXIT_REFUSED


def digest(path: pathlib.Path) -> str:
    sha = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1 << 16), b""):
            sha.update(block)
    return sha.hexdigest()


def newest_source() -> tuple[float, str] | None:
    """The most recently modified source file, and its path."""
    newest: tuple[float, str] | None = None
    for root in SOURCES:
        if root.is_file():
            paths = [root]
        elif root.is_dir():
            # `target/` lives outside these roots, so nothing here is an
            # artifact of the build being checked.
            paths = [p for p in root.rglob("*") if p.is_file()]
        else:
            continue
        for path in paths:
            stamp = path.stat().st_mtime
            if newest is None or stamp > newest[0]:
                newest = (stamp, str(path))
    return newest


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--expect",
        help="fail unless the resolved path is exactly this. The test that a "
        "pinned DIET_BIN is not silently ignored.",
    )
    args = parser.parse_args()

    pinned = os.environ.get("DIET_BIN")
    if pinned:
        path = pathlib.Path(pinned)
        if not path.is_file():
            return refuse(f"DIET_BIN names {pinned}, which is not a file")
        source = "DIET_BIN"
    else:
        present = [path for path in candidates() if path.is_file()]
        if not present:
            return refuse(
                "no diet binary: build one, or set DIET_BIN. Resolving to "
                "nothing and carrying on is how a check of nothing passes"
            )
        if len(present) > 1:
            names = ", ".join(str(path) for path in present)
            return refuse(
                f"two builds present ({names}) and no DIET_BIN. Picking the "
                f"newer is what graded seventy-three controls green through a "
                f"binary seven days behind its source; pick one and say so"
            )
        path = present[0]
        source = "the only build present"

    if not os.access(path, os.X_OK):
        return refuse(
            f"{path} is not executable, so it is not a build of anything; "
            f"resolving it would hand a caller a file it cannot run"
        )

    binary_stamp = path.stat().st_mtime
    newest = newest_source()
    if newest is None:
        # Rule three is stated without conditions, so it cannot quietly not
        # apply. Off the repository root none of SOURCES exists, the scan
        # returned nothing, and a stale binary resolved with
        # `"checked_against": null` and exit 0 -- a check of no files
        # reporting success, which is the one shape this repository refuses.
        return refuse(
            f"no source found under {', '.join(str(root) for root in SOURCES)} "
            f"relative to {pathlib.Path.cwd()}, so nothing establishes whether "
            f"{path} reflects it. Run from the tree the binary belongs to"
        )
    if newest[0] > binary_stamp:
        return refuse(
            f"{path} is older than {newest[1]}, so it does not reflect the "
            f"source it is supposed to be a build of"
        )

    resolved = {
        "path": str(path),
        "sha256": digest(path),
        "resolved_by": source,
        "checked_against": newest[1],
    }
    # Exit 1, not 2: this is the provenance TEST failing, not the resolver
    # refusing. The difference matters to a caller deciding whether to build.
    if args.expect is not None and path.resolve() != pathlib.Path(args.expect).resolve():
        print(
            f"resolve-diet: resolved {path}, but {args.expect} was pinned; "
            f"a pin that can be silently ignored is not a pin",
            file=sys.stderr,
        )
        return 1
    print(json.dumps(resolved, sort_keys=True))
    return 0


if __name__ == "__main__":
    sys.exit(main())
