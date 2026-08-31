#!/usr/bin/env python3
"""Lint the repository metadata that `sync-repo-metadata.sh` applies.

The sync workflow runs only on `main`, so without this check a malformed
`labels.json` would not be noticed until after it had merged. This is the
gate that makes the sync trustworthy:

  * `.github/labels.json` and `.github/milestones.json` parse, carry the
    required keys at the required types, and hold no duplicates.
  * Every label an issue template assigns exists in `labels.json`. A template
    that assigns a label nothing creates fails silently in the GitHub UI, so
    it fails loudly here instead.

Stdlib only. Exit 0 if the metadata is sound, 1 otherwise.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys

GITHUB = pathlib.Path(".github")
LABELS = GITHUB / "labels.json"
MILESTONES = GITHUB / "milestones.json"
TEMPLATES = GITHUB / "ISSUE_TEMPLATE"

COLOR = re.compile(r"[0-9a-f]{6}")
# `sync-repo-metadata.sh` builds `repos/{owner}/{repo}/labels/{name}` by
# interpolation, so a name carrying `/`, `?` or `#` would address a different
# endpoint entirely. Restrict names to what is safe unencoded.
LABEL_NAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")
FRONT_MATTER_LABELS = re.compile(r"^labels:\s*(.+?)\s*$", re.MULTILINE)


def load(path: pathlib.Path, key: str, failures: list[str]) -> list[dict]:
    if not path.is_file():
        failures.append(f"{path}: missing")
        return []
    try:
        document = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as err:
        failures.append(f"{path}: not JSON: {err}")
        return []
    if not isinstance(document, dict) or key not in document:
        failures.append(f"{path}: has no top-level `{key}` key")
        return []
    entries = document[key]
    if not isinstance(entries, list) or not entries:
        failures.append(f"{path}: `{key}` is not a non-empty list")
        return []
    return entries


def check_entries(
    path: pathlib.Path,
    entries: list[dict],
    identity: str,
    extra: dict[str, re.Pattern[str]],
    failures: list[str],
) -> set[str]:
    """Check a list of metadata entries and return the identities it defines."""
    seen: set[str] = set()
    for index, entry in enumerate(entries):
        where = f"{path}[{index}]"
        if not isinstance(entry, dict):
            failures.append(f"{where}: is a {type(entry).__name__}, not an object")
            continue
        for field in dict.fromkeys((identity, "description", *extra)):
            value = entry.get(field)
            if not isinstance(value, str) or not value.strip():
                failures.append(f"{where}: `{field}` is missing or not a non-empty string")
        name = entry.get(identity)
        if isinstance(name, str):
            if name in seen:
                failures.append(f"{where}: duplicate {identity} {name!r}")
            seen.add(name)
        for field, pattern in extra.items():
            value = entry.get(field)
            if isinstance(value, str) and not pattern.fullmatch(value):
                failures.append(f"{where}: `{field}` {value!r} does not match {pattern.pattern}")
    return seen


def template_labels(failures: list[str]) -> dict[str, set[str]]:
    """The labels each issue template assigns, read from its front-matter.

    Only the single `labels:` line is read, which is all the templates use;
    a template that grows a YAML list will need this widened, and will say so
    by reporting a label named `[`.
    """
    assigned: dict[str, set[str]] = {}
    if not TEMPLATES.is_dir():
        failures.append(f"{TEMPLATES}: missing")
        return assigned
    templates = sorted(TEMPLATES.glob("*.md"))
    if not templates:
        failures.append(f"{TEMPLATES}: holds no issue templates")
        return assigned
    for path in templates:
        text = path.read_text(encoding="utf-8")
        if not text.startswith("---\n"):
            failures.append(f"{path}: does not open with a `---` front-matter fence")
            continue
        end = text.find("\n---", 3)
        if end == -1:
            failures.append(f"{path}: front-matter fence is never closed")
            continue
        front = text[4:end]
        found = FRONT_MATTER_LABELS.search(front)
        assigned[str(path)] = (
            {item.strip() for item in found.group(1).split(",") if item.strip()}
            if found
            else set()
        )
    return assigned


def main() -> int:
    failures: list[str] = []

    labels = load(LABELS, "labels", failures)
    milestones = load(MILESTONES, "milestones", failures)

    defined = check_entries(
        LABELS, labels, "name", {"color": COLOR, "name": LABEL_NAME}, failures
    )
    check_entries(MILESTONES, milestones, "title", {}, failures)

    for path, assigned in template_labels(failures).items():
        for label in sorted(assigned - defined):
            failures.append(
                f"{path}: assigns label {label!r}, which {LABELS} does not define"
            )

    for message in failures:
        print(message, file=sys.stderr)
    if failures:
        print(f"check-repo-metadata: {len(failures)} failure(s)", file=sys.stderr)
        return 1
    print(
        f"check-repo-metadata: {len(defined)} label(s) and "
        f"{len(milestones)} milestone(s) are sound"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
