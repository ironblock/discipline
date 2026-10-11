#!/usr/bin/env python3
"""Assemble a driven session's run directory from what `serve` wrote (#757).

    python3 scripts/assemble-run.py --log FILE --record FILE --regimen FILE \\
        --out runs/YYYY-MM-DD-<name> [--home DIR]

`serve --log L --record R` leaves the log, the record and, beside the record,
its sidecar (R.unspellable.json), the working memory at the end
(R.product.txt) and the receipt (R.receipt.json, else L.receipt.json). This
copies them into OUT under the names `runs/README.md` lists:

    session.log       the session's log, scrubbed
    record.jsonl      the record, scrubbed
    product.txt       the working memory at the end, scrubbed
    receipt.json      the receipt, scrubbed (when the session ran commands)
    unspellable.json  the record's sidecar, scrubbed
    regimen.toml      the regimen, byte for byte
    README.md         a skeleton: what the files say, filled; every narrative
                      claim `[unwritten: ...]` for whoever drove it to write

SCRUB, with the hygiene check's own patterns (scripts/scrub.py): a home path
to `~`, a scratch path to `/tmp/scratch/`, the account name (--home's last
part, default $HOME's) to `user`. A file still matching any other hygiene
row -- a LAN address, a hostname, a credential -- is refused, never
rewritten. The regimen is not scrubbed: its sha256 is the record's claim, so
one carrying anything to scrub is refused.

CHECKED at this build: `diet check-log` and `diet check-record` over the
scrubbed copies. A refusal is the run's finding, quoted in the README, per
`runs/README.md`; it does not stop the assembly. The tree-wide hygiene scan
(scripts/hygiene.sh) then runs over the whole directory, and a hit refuses.

COUNTED through `diet`: the README's counts and turn table come from `diet
check-log`'s own projection of the log, and its build from the log's
`session.start` (v8's `build_commit`), never typed. The directory is built
beside OUT and moved there only once it passes, so a refusal leaves nothing.

Prints one JSON line: {"assembled": OUT, "scrubbed": {...}, "checks": {...},
"counts": {...}} on success (exit 0), {"refused": WHY} otherwise (exit 1).
"""

import argparse
import collections
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "scripts"))
import scrub  # noqa: E402

# The files a run holds, in the order its README's table lists them, with
# what each one is. The regimen is copied byte for byte; every other text is
# scrubbed.
FILES = [
    ("session.log", "the session's log, scrubbed"),
    ("record.jsonl", "the record serve wrote at end, scrubbed"),
    ("product.txt", "the working memory at end"),
    ("receipt.json", "the receipt"),
    ("unspellable.json", "what the record could not spell"),
    ("regimen.toml", "the regimen as it ran"),
]
UNSCRUBBED = {"regimen.toml"}


class Refused(Exception):
    pass


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def diet() -> str:
    resolved = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "resolve-diet.py")],
        capture_output=True, text=True, cwd=ROOT,
    )
    if resolved.returncode != 0:
        raise Refused(f"no diet binary: {resolved.stderr.strip()}")
    return str(ROOT / json.loads(resolved.stdout)["path"])


def check(binary: str, command: str, path: pathlib.Path) -> tuple[bool, dict]:
    """`diet COMMAND PATH`: whether it accepted the file, and what it said."""
    ran = subprocess.run([binary, command, str(path)], capture_output=True, text=True)
    try:
        said = json.loads(ran.stdout)
    except json.JSONDecodeError:
        said = {"error": (ran.stdout + ran.stderr).strip()[:2000]}
    return ran.returncode == 0, said


def inputs(args: argparse.Namespace) -> dict[str, pathlib.Path | None]:
    """Where serve left each file, by the name the run gives it."""
    record = pathlib.Path(args.record)
    log = pathlib.Path(args.log)
    receipt = next(
        (p for p in (pathlib.Path(f"{record}.receipt.json"), pathlib.Path(f"{log}.receipt.json")) if p.is_file()),
        None,
    )
    return {
        "session.log": log,
        "record.jsonl": record,
        "product.txt": pathlib.Path(f"{record}.product.txt"),
        "receipt.json": receipt,
        "unspellable.json": pathlib.Path(f"{record}.unspellable.json"),
        "regimen.toml": pathlib.Path(args.regimen),
    }


def counts_of(events: list[dict]) -> dict:
    """Lines by kind, requests and forks by lane, and each turn's row."""
    kinds = collections.Counter(event["kind"] for event in events)
    lanes = {
        kind: dict(sorted(collections.Counter(e.get("lane", "-") for e in events if e["kind"] == kind).items()))
        for kind in ("request", "fork")
    }
    turns: dict[int, dict] = {}
    for event in events:
        turn = event.get("turn")
        if event["kind"] == "ask":
            turns[turn] = {"ask": event["seq"], "requests": 0, "tool_calls": 0, "forks": 0, "settled": None}
        elif event["kind"] == "request" and event.get("lane") == "trunk" and turn in turns:
            turns[turn]["requests"] += 1
        elif event["kind"] == "tool_call" and turn in turns:
            turns[turn]["tool_calls"] += 1
        elif event["kind"] == "fork" and event.get("of_turn") in turns:
            turns[event["of_turn"]]["forks"] += 1
        elif event["kind"] == "turn.settled" and turn in turns:
            turns[turn]["settled"] = event["reason"]
    return {"kinds": dict(sorted(kinds.items())), "lanes": lanes, "turns": turns}


def plural(count: int, one: str, many: str) -> str:
    return f"{count} {one if count == 1 else many}"


def readme(name: str, start: dict, counts: dict, checks: dict, scrubbed: dict, digests: dict) -> str:
    kinds, lanes, turns = counts["kinds"], counts["lanes"], counts["turns"]
    build = (
        f"`{start['build_commit']}`"
        + (", built from a dirty tree" if start.get("build_dirty") else "")
        + ", as the log's `session.start` names it"
        if "build_commit" in start
        else "[unwritten: the build; the log names none, so serve predates #757]"
    )
    forks = lanes["fork"]
    fork_lanes = ", ".join(f"{n} {lane}" for lane, n in forks.items())
    summary = [
        plural(kinds.get("ask", 0), "ask", "asks"),
        plural(kinds.get("request", 0), "request", "requests"),
        plural(kinds.get("tool_call", 0), "tool call", "tool calls"),
        plural(kinds.get("fork", 0), "fork", "forks") + (f" ({fork_lanes})" if forks else ""),
        plural(kinds.get("patch", 0), "patch", "patches"),
        plural(kinds.get("flag", 0), "flag", "flags"),
        plural(kinds.get("meditation", 0), "meditation", "meditations"),
        plural(kinds.get("meditation.held", 0), "meditation held", "meditations held"),
        plural(kinds.get("refused", 0), "refusal", "refusals"),
    ]
    out = [
        f"# {name}",
        "",
        "**Who drove it:** [unwritten: who drove it, and whether the maintainer approved it]",
        "",
        "**Purpose:** [unwritten: what this run is evidence for]",
        "",
        "## Setup",
        "",
        f"- **Build:** {build}.",
        "- **Floor:** [unwritten: the substrate and whether it was warmed]",
        "- **Regimen:** `regimen.toml` here. [unwritten: what it changes, and why]",
        "- **Workspace:** [unwritten: the task and its worktree]",
        "",
        "**The start row** (`session.log` line 1, `levers`) names:",
    ]
    out += [f"- `{lever}: {state}`;" for lever, state in sorted(start.get("levers", {}).items())]
    out += [
        "",
        "## The run",
        "",
        "| turn | ask (`seq`) | trunk requests | tool calls | forks | settled |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for turn, row in sorted(turns.items()):
        out.append(
            f"| {turn} | {row['ask']} | {row['requests']} | {row['tool_calls']} | {row['forks']} | "
            f"`{row['settled'] or 'unsettled'}` |"
        )
    out += [
        "",
        "[unwritten: what happened, turn by turn, each seq from `diet log find`]",
        "",
        "## What it showed",
        "",
        "[unwritten: which rows or levers it shows, each with the seqs a `diet log find` query returned]",
        "",
        "## What broke",
        "",
        "[unwritten: what broke, or that nothing did]",
        "",
        f"**Counts:** {', '.join(summary)}.",
        "",
        "| kind | lines |",
        "| --- | --- |",
    ]
    out += [f"| `{kind}` | {n} |" for kind, n in kinds.items()]
    out += ["", "## Scrubbed", ""]
    if scrubbed:
        for name, made in sorted(scrubbed.items()):
            out.append(f"- `{name}`: " + ", ".join(f"{label} ({n})" for label, n in sorted(made.items())) + ".")
        out.append("")
    out += [
        "Home paths are written `~`, scratch paths `/tmp/scratch/` and the account name `user`, "
        "by `scripts/scrub.py` with the hygiene check's own patterns. The regimen is byte for byte.",
        "",
        "## Files",
        "",
    ]
    for command, (ok, said) in checks.items():
        out.append(
            f"`diet {command}` accepts `{said['file']}` at this build."
            if ok
            else f"`diet {command}` refuses `{said['file']}` at this build: `{said['why']}`. "
            "[unwritten: the issue that fixes it]"
        )
    out += ["", "| file | what it is | sha256 |", "| --- | --- | --- |"]
    out += [f"| `{name}` | {what} | `{digests[name]}` |" for name, what in FILES if name in digests]
    return "\n".join(out) + "\n"


def assemble(args: argparse.Namespace) -> dict:
    out = pathlib.Path(args.out)
    if not out.is_absolute():
        out = ROOT / out
    if out.exists():
        raise Refused(f"{out} exists")
    account = pathlib.Path(args.home).name
    sources = inputs(args)
    for name in ("session.log", "record.jsonl", "regimen.toml"):
        if not sources[name] or not sources[name].is_file():
            raise Refused(f"no {name} at {sources[name]}")
    written: dict[str, bytes] = {}
    scrubbed: dict[str, dict] = {}
    for name, source in sources.items():
        if source is None or not source.is_file():
            continue
        data = source.read_bytes()
        if name in UNSCRUBBED:
            text = data.decode("utf-8")
            if scrub.scrub(text, account)[1] or scrub.unscrubbable(text):
                raise Refused(f"{source} carries what a scrub would rewrite; its digest is the record's claim")
            written[name] = data
            continue
        text, made = scrub.scrub(data.decode("utf-8"), account)
        left = scrub.unscrubbable(text)
        if left:
            raise Refused(f"{source} matches hygiene rows no scrub rewrites: {', '.join(left)}")
        if made:
            scrubbed[name] = made
        written[name] = text.encode("utf-8")
    holding = out.parent / f".{out.name}.assembling"
    if holding.exists():
        shutil.rmtree(holding)
    holding.mkdir(parents=True)
    try:
        for name, data in written.items():
            (holding / name).write_bytes(data)
        binary = diet()
        checks = {}
        events = []
        for command, name in (("check-log", "session.log"), ("check-record", "record.jsonl")):
            ok, said = check(binary, command, holding / name)
            checks[command] = (ok, {"file": name, "why": said.get("error", "")})
            if command == "check-log" and ok:
                events = said["value"]["events"]
        if not events:
            raise Refused(f"diet check-log refuses the log: {checks['check-log'][1]['why']}")
        counts = counts_of(events)
        digests = {name: sha256(data) for name, data in written.items()}
        (holding / "README.md").write_text(readme(out.name, events[0], counts, checks, scrubbed, digests))
        hygiene = subprocess.run(
            ["bash", str(ROOT / "scripts" / "hygiene.sh"), "--tree", str(holding)],
            capture_output=True, text=True, cwd=ROOT,
        )
        if hygiene.returncode != 0:
            raise Refused(f"the hygiene check refuses the assembled run: {(hygiene.stdout + hygiene.stderr).strip()[:2000]}")
        holding.rename(out)
    finally:
        if holding.exists():
            shutil.rmtree(holding)
    return {
        "assembled": str(out),
        "scrubbed": scrubbed,
        "checks": {command: ok for command, (ok, _) in checks.items()},
        "counts": {"kinds": counts["kinds"], "lanes": counts["lanes"]},
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--log", required=True)
    parser.add_argument("--record", required=True)
    parser.add_argument("--regimen", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--home", default=os.path.expanduser("~"))
    args = parser.parse_args()
    try:
        print(json.dumps(assemble(args), sort_keys=True))
    except Refused as why:
        print(json.dumps({"refused": str(why)}))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
