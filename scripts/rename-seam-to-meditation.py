#!/usr/bin/env python3
"""The mechanical half of #583: "seam" becomes "meditation".

Run from the repository root on a clean tree. It renames identifiers, the
module path, CSS, labels, comments and current docs, and leaves alone what
must keep the old word:

* history: results/, runs/, substrates/, docs/releases.md, diet/drive/plans/;
* committed inputs that declare their version: every format's fixtures, the
  exercise's recorded runs, its authored examples (both pinned by an
  admission) and the script that migrated the runs;
* names of things on disk that stay: run directories, fixture files;
* the archive's reference key `seam-<turn>/message-<n>`, which the model
  reads (held for the maintainer's ruling);
* other senses: "Test seam" (a code seam), "the whole seam claim";
* research-format names: `seam.begin`, `seam.end`, `seam.render`,
  `seam-add`;
* the canned acts' fork answer, which the registry pins by digest.

The hand-made half -- the log and record v8 version gate, the regimen and
HTTP aliases, the v8 fixture counterparts, program.md section 2 -- is
committed on top of this script's output and replayed after a re-run.

After the text pass it moves the renamed paths, regenerates the TS log
bindings and formats.
"""

import pathlib
import re
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

EXCLUDED = [
    r"^results/",
    r"^runs/",
    r"^substrates/",
    r"^docs/releases\.md$",
    r"^diet/drive/plans/",
    r"^diet/formats/[^/]+/fixtures/",
    r"^exercise/src/drive/recorded/",
    r"^exercise/src/drive/examples/",
    r"^exercise/scripts/migrate-recorded\.py$",
    r"^logo/",
    r"^scripts/rename-seam-to-meditation\.py$",
]

# Spans the text pass must not touch, found before it runs.
PROTECTED = [
    r"\d{4}-\d{2}-\d{2}-[a-z0-9-]*seam[a-z0-9-]*",  # run directories
    r"seam-smoke-run-\d+",
    r"seam-(?:\d+|\{[^}]*\})/message",  # the archive key, held
    r"seam-1/message",
    r"Test seam",
    r"the whole seam claim",
    r"seam\.(?:begin|end|render)",
    r"seam-add",
    r"the seam refills",  # the canned acts' fork answer: registered by digest
]

# Ordered: the longest and most specific first.
WORDS = [
    # The verb, where the text uses one.
    ("overflow seams once", "overflow meditates once"),
    ("overflow_seams_once", "overflow_meditates_once"),
    ("window_seams_first", "window_meditates_first"),
    ("never seam", "never meditate"),
    ("nothing-to-seam", "nothing-to-meditate"),
    ("nothing_to_seam", "nothing_to_meditate"),
    ("NothingToSeam", "NothingToMeditate"),
    ("SEAMED", "MEDITATED"),
    ("Seamed", "Meditated"),
    ("seamed", "meditated"),
    ("SEAMING", "MEDITATING"),
    ("Seaming", "Meditating"),
    ("seaming", "meditating"),
    ("SEAMS", "MEDITATIONS"),
    ("Seams", "Meditations"),
    ("seams", "meditations"),
    ("SEAM", "MEDITATION"),
    ("Seam", "Meditation"),
    ("seam", "meditation"),
]

MOVES = [
    ("diet/src/seam", "diet/src/meditation"),
    ("exercise/src/ui/Seam.tsx", "exercise/src/ui/Meditation.tsx"),
    ("exercise/src/ui/seam.css", "exercise/src/ui/meditation.css"),
]


def fixture_stems():
    """Every fixture file's stem holding the word: names on disk that stay."""
    stems = set()
    for path in (ROOT / "diet/formats").glob("*/fixtures/*/*"):
        stem = path.name.split(".")[0]
        if "seam" in stem.lower():
            stems.add(stem)
    return sorted(stems, key=len, reverse=True)


def tracked():
    out = subprocess.run(
        ["git", "ls-files"], cwd=ROOT, check=True, capture_output=True, text=True
    ).stdout
    excluded = [re.compile(pattern) for pattern in EXCLUDED]
    return [p for p in out.splitlines() if not any(e.search(p) for e in excluded)]


def rename_text(text, protected):
    held = []

    def hold(match):
        held.append(match.group(0))
        return f"\x00{len(held) - 1}\x00"

    text = protected.sub(hold, text)
    for old, new in WORDS:
        text = text.replace(old, new)
    return re.sub(r"\x00(\d+)\x00", lambda m: held[int(m.group(1))], text)


def main():
    status = subprocess.run(
        ["git", "status", "--porcelain"], cwd=ROOT, capture_output=True, text=True
    ).stdout
    if status.strip() and "--dirty" not in sys.argv:
        sys.exit("the tree is not clean")
    protected = re.compile(
        "|".join(
            [re.escape(stem) for stem in fixture_stems()] + PROTECTED
        )
    )
    changed = 0
    for rel in tracked():
        path = ROOT / rel
        if not path.is_file():
            continue
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        if "seam" not in text.lower():
            continue
        renamed = rename_text(text, protected)
        if renamed != text:
            path.write_text(renamed, encoding="utf-8")
            changed += 1
    for old, new in MOVES:
        if (ROOT / old).exists():
            subprocess.run(["git", "mv", old, new], cwd=ROOT, check=True)
    print(f"{changed} files rewritten, {len(MOVES)} paths moved")
    if "--no-post" in sys.argv:
        return
    subprocess.run(["cargo", "fmt", "--all"], cwd=ROOT, check=True)
    subprocess.run(
        [
            "cargo", "test", "-q", "-p", "discipline-diet", "--lib",
            "formats::log::tests::write_the_bindings", "--", "--ignored",
        ],
        cwd=ROOT,
        check=True,
    )


if __name__ == "__main__":
    main()
