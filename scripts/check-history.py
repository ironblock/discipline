#!/usr/bin/env python3
"""Scan a range's commit messages AND ITS PATCH TEXT -- and a pull request's
title and body -- for the shapes scripts/hygiene-patterns.tsv forbids.

`hygiene.sh` scans files: the tree as it stands. This scanned messages. Between
them was a hole neither could see: **content added in one commit and removed in
a later one is scanned by nothing, ever**, and stays recoverable from history
for as long as the repository exists. Measured on merged `main` -- 32
occurrences of a private tracker token in patch text, zero in the tree, the
gate green over both. `git log -p` hands them to anyone.

So the patch text of the range is scanned with the same table. A token that
enters a diff fails before it enters history. Ruled 2026-09-08 on #54: history
is the write a fix cannot reach.

A file carrying a private hostname or an internal ticket identifier can be
fixed with a commit; a commit MESSAGE, or a diff, carrying one is permanent,
so the ungated path is the more damaging of the two.

There is exactly one pattern definition and exactly one scanner. This script
determines what to read and materialises it, then hands the result to
`hygiene.sh --tree`. Adding a forbidden shape means editing the table, and it
applies to files and history at once.

Determining the range:

  pull_request  the event payload's base.sha..head.sha, plus title and body
  push          the event payload's before..after; on a new branch, where
                `before` is all zeros, the merge base with the default branch
  otherwise     origin/<default>..HEAD -- the history you have not published

**A base that cannot be determined is a failure, never an empty scan.** That
is the whole point: silently scanning nothing is how a gate reports success
over history it never read. An empty RANGE is different and is fine locally --
you simply have no unpublished commits -- but under GITHUB_ACTIONS a push or
pull_request always carries commits, so an empty range there is also a failure.

Stdlib only. Exit 0 if history is clean, 1 if it is not, 2 if the scan could
not be set up.
"""

from __future__ import annotations

import argparse
import json
import os
import pathlib
import subprocess
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
HYGIENE = ROOT / "scripts" / "hygiene.sh"

EXIT_DIRTY = 1
EXIT_BROKEN = 2

ZERO = "0" * 40


class Undeterminable(Exception):
    """No trustworthy base. Never downgraded to an empty scan."""


def git(*args: str, check: bool = True) -> str:
    done = subprocess.run(
        ["git", "-C", str(ROOT), *args], capture_output=True, text=True
    )
    if check and done.returncode != 0:
        raise Undeterminable(f"`git {' '.join(args)}` failed: {done.stderr.strip()}")
    return done.stdout.strip()


def event() -> dict:
    path = os.environ.get("GITHUB_EVENT_PATH")
    if not path or not pathlib.Path(path).is_file():
        return {}
    try:
        return json.loads(pathlib.Path(path).read_text(encoding="utf-8"))
    except (ValueError, OSError) as err:
        raise Undeterminable(f"the event payload is unreadable: {err}") from err


def default_branch() -> str:
    for candidate in ("origin/HEAD", "origin/main", "origin/master"):
        ref = git("rev-parse", "--verify", "--quiet", candidate, check=False)
        if ref:
            return candidate
    raise Undeterminable(
        "no origin/HEAD, origin/main or origin/master to compare against; "
        "pass --range explicitly"
    )


def reachable(name: str) -> bool:
    """Whether `name` is a commit this checkout holds.

    Separate from [`resolve`] because the two questions have different
    answers on a force-push: the pre-rewrite head is a perfectly good sha
    that this clone does not have, and asking "is it here" is not the same
    as demanding it be.
    """
    return bool(git("rev-parse", "--verify", "--quiet", f"{name}^{{commit}}", check=False))


def resolve(name: str) -> str:
    sha = git("rev-parse", "--verify", "--quiet", f"{name}^{{commit}}", check=False)
    if not sha:
        raise Undeterminable(f"`{name}` does not resolve to a commit in this checkout")
    return sha


def determine() -> tuple[str, str, str, list[tuple[str, str]]]:
    """Return (base, head, how, extra_texts)."""
    payload = event()
    name = os.environ.get("GITHUB_EVENT_NAME", "")

    if name == "pull_request":
        pr = payload.get("pull_request") or {}
        base = ((pr.get("base") or {}).get("sha") or "").strip()
        head = ((pr.get("head") or {}).get("sha") or "").strip()
        if not base or not head:
            raise Undeterminable("the pull_request payload carries no base.sha/head.sha")
        extra = []
        if pr.get("title"):
            extra.append(("pull-request-title", str(pr["title"])))
        if pr.get("body"):
            extra.append(("pull-request-body", str(pr["body"])))
        return resolve(base), resolve(head), "pull_request base..head", extra

    if name == "push":
        before = (payload.get("before") or "").strip()
        after = (payload.get("after") or "").strip() or os.environ.get("GITHUB_SHA", "")
        if not after:
            raise Undeterminable("the push payload carries no after sha")
        if before and before != ZERO and reachable(before):
            return resolve(before), resolve(after), "push before..after", []
        # Two ways to arrive here, and they want the same scan.
        #
        # A NEW BRANCH: `before` is the zero sha, nothing was there, so the
        # trunk is what to compare against.
        #
        # A FORCE-PUSH: `before` names the pre-rewrite head, which the rewrite
        # ORPHANED -- so it is absent from CI's fresh checkout and resolves to
        # nothing. That is not a broken payload; it is the expected shape of a
        # legitimate operation, and this repository's own rules require it: the
        # merge protocol mandates stack-order rebases and #54's ruling mandates
        # rebuilding a branch when identity has entered it. Both are
        # force-pushes, so the check was guaranteed to redden the lane that
        # gates the merge, on a branch whose content is fine.
        #
        # The merge base with the trunk is a SUPERSET of the new commits, which
        # is safe for a check whose job is to find what should not be in a
        # message: scanning more than the push introduced can only find more.
        # `undeterminable` is kept for the case where even that fails.
        how = "push, new branch" if not before or before == ZERO else "push, force-push"
        merge_base = git("merge-base", default_branch(), after, check=False)
        if not merge_base:
            raise Undeterminable(
                f"{how}: no merge base against the default branch"
            )
        return merge_base, resolve(after), f"{how}: merge-base..after", []

    base = default_branch()
    merge_base = git("merge-base", base, "HEAD", check=False)
    if not merge_base:
        raise Undeterminable(f"no merge base between {base} and HEAD")
    return merge_base, resolve("HEAD"), f"local {base}..HEAD", []


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--range", metavar="A..B",
                        help="scan this range instead of deriving one from the event")
    args = parser.parse_args(argv)

    extra: list[tuple[str, str]] = []
    try:
        if args.range:
            if ".." not in args.range:
                print("check-history: --range must look like A..B", file=sys.stderr)
                return EXIT_BROKEN
            left, right = args.range.split("..", 1)
            base, head = resolve(left), resolve(right or "HEAD")
            how = f"--range {args.range}"
        else:
            base, head, how, extra = determine()
    except Undeterminable as err:
        print(f"check-history: cannot determine what to scan: {err}", file=sys.stderr)
        print("check-history: an undeterminable base is a failure, not an empty scan",
              file=sys.stderr)
        return EXIT_BROKEN

    shas = [s for s in git("rev-list", f"{base}..{head}").split("\n") if s]

    in_ci = os.environ.get("GITHUB_ACTIONS") == "true"
    if not shas and not extra:
        if in_ci and os.environ.get("GITHUB_EVENT_NAME") in {"push", "pull_request"}:
            print(f"check-history: {how} selected no commits, which cannot happen for "
                  f"a real push or pull request", file=sys.stderr)
            return EXIT_BROKEN
        print(f"check-history: {how} -> 0 commits; no unpublished history to scan")
        return 0

    with tempfile.TemporaryDirectory() as work:
        out = pathlib.Path(work)
        patches = 0
        for sha in shas:
            body = git("log", "-1", "--format=%B%n%an <%ae>", sha)
            (out / f"commit-{sha[:12]}.txt").write_text(body + "\n", encoding="utf-8")
            # `--format=` so the message is not scanned twice, and no `-m`, so
            # a merge commit contributes no patch: whatever it brings in is
            # already in the commits it merges, and every one of those is in
            # this range. A merge shown against each parent would double the
            # text and report each hit twice.
            patch = git("show", "--format=", "--patch", sha, check=False)
            if patch:
                (out / f"patch-{sha[:12]}.txt").write_text(patch + "\n", encoding="utf-8")
                patches += 1
        for label, text in extra:
            (out / f"{label}.txt").write_text(text + "\n", encoding="utf-8")

        print(f"check-history: {how}")
        print(f"check-history: {len(shas)} commit message(s)"
              f" and {patches} patch(es)"
              f"{' + ' + ', '.join(l for l, _ in extra) if extra else ''}"
              f", scanned with the same table as the file gate")

        done = subprocess.run(["bash", str(HYGIENE), "--tree", str(out)])

    if done.returncode == 0:
        return 0
    if done.returncode == EXIT_DIRTY:
        print("check-history: history cannot be edited after it is pushed; "
              "rewrite the offending commits before merging. A hit in a "
              "`patch-*` file is content the diff ADDS OR REMOVES: removing it "
              "in a later commit does not remove it from history, so the "
              "commit that introduced it is the one to rewrite.",
              file=sys.stderr)
        return EXIT_DIRTY
    return EXIT_BROKEN


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
