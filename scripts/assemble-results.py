#!/usr/bin/env python3
"""Assemble a served session's results directory from what `serve` wrote.

    python3 scripts/assemble-results.py --log FILE --record FILE --regimen FILE \\
        --out results/YYYY-MM-DD-<slug> [--receipt FILE] [--home DIR]

`serve --log L --record R --regimen G` leaves the log, the record, the record's
sidecar (R.unspellable.json), the receipt (R.receipt.json, or L.receipt.json
with no record) and every attached file under the log's directory. This copies
them into OUT under the names a results directory reads:

    log.jsonl         the session's log
    run.jsonl         its record
    unspellable.json  what the record could not spell
    product.txt       the working memory at the end (R.product.txt), the
                      summary's product
    receipt.json      the receipt, when the session ran commands
    regimen.toml      the regimen, byte for byte
    files/<sha256>    every file a log line names, checked against its digest
    digests.json      every file's sha256 and size, and what was scrubbed
    README.md         a skeleton whose front-matter is filled from the data

A relative --out is taken from the repository root. The directory is built
beside OUT and moved there only once it passes, so a refusal leaves nothing.

and refuses (exit 1) when:
- the regimen's bytes do not hash to the record's `regimen_sha256`;
- a named file is missing or is not its digest;
- `diet check-log`, `check-record` or `check-regimen` refuses a copy;
- the copy is not hygiene-clean;
- the record or the log is empty (a session that never ended).

SCRUB. A log carries the operator's absolute paths (a call's `cwd`, a
receipt's reference checkouts). Each occurrence of the home directory (--home,
default $HOME) is rewritten to `~`, the spelling regimens already use. Only the
log, record, sidecar and receipt are rewritten. digests.json keeps each
rewritten file's original sha256 beside the copy's, so the copy can be told
from what serve wrote. The regimen is never rewritten: its digest is the
record's claim, so a regimen carrying the home directory is refused instead.

HYGIENE runs as `scripts/hygiene.sh --tree OUT` under LC_ALL=C (#467: under a
UTF-8 locale on macOS its binary scan passes what it should flag).

`scripts/check-results.py OUT` is run last and its verdict reported. It does
not decide the exit: the README is the author's to write.

Prints one JSON object on stdout. Exit 0 assembled; 1 refused; 2 usage.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile
import tomllib

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCRUBBED = ("log.jsonl", "run.jsonl", "unspellable.json", "receipt.json")


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


class Refused(Exception):
    pass


def lines(data: bytes) -> list[str]:
    """JSON Lines, split on `\n` only: `str.splitlines` also splits on
    U+2028, U+2029 and U+0085, which the writer leaves raw inside a string."""
    return [line for line in data.decode("utf-8").split("\n") if line.strip()]


def named_files(log: bytes) -> dict[str, dict]:
    """Every `files` entry of an `ask` or `tool_call` line, by digest."""
    named: dict[str, dict] = {}
    for number, line in enumerate(lines(log), 1):
        try:
            event = json.loads(line)
        except json.JSONDecodeError as err:
            raise Refused(f"log line {number} is not JSON: {err}") from err
        if event.get("kind") in ("ask", "tool_call") and isinstance(event.get("files"), list):
            for entry in event["files"]:
                named.setdefault(entry["sha256"], entry)
    return named


def scrub(data: bytes, home: str) -> bytes:
    """`home` rewritten to `~` wherever the name it ends in ends: the
    boundary hygiene's personal-home-path reads, so whatever follows -- a
    quote, a backtick, a paren, a colon -- the path is scrubbed."""
    pattern = re.compile(re.escape(home.rstrip("/").encode()) + rb"(?![A-Za-z0-9._-])")
    return pattern.sub(b"~", data)


def diet() -> str:
    resolved = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "resolve-diet.py")],
        capture_output=True, text=True, cwd=ROOT,
    )
    if resolved.returncode != 0:
        raise Refused(f"no diet binary: {resolved.stderr.strip()}")
    # resolve-diet.py names the binary relative to the repository.
    return str(ROOT / json.loads(resolved.stdout)["path"])


def checked(binary: str, command: str, path: pathlib.Path) -> None:
    ran = subprocess.run([binary, command, str(path)], capture_output=True, text=True)
    if ran.returncode != 0:
        raise Refused(f"diet {command} {path.name} exits {ran.returncode}: {(ran.stdout + ran.stderr).strip()[:2000]}")


def front_matter(regimen: dict, record_digest: str, summary: dict, opened: int) -> str:
    substrate = regimen.get("substrate")
    window = datetime.datetime.fromtimestamp(opened / 1000, datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    lines = [
        "+++",
        f'product_sha256 = "{summary["product_sha256"]}"',
        f"turns = {summary['turns']}",
        f"prefill_tokens_total = {summary['prefill_tokens_total']}",
        'hypothesis = "[unwritten: the author states the claim this session tests]"',
        'result = "inconclusive"',
        'kind = "historical-observation"',
        'historical_reason = "a served session: an operator drove it live, and a re-run is a new session"',
        'figures = "referenced"',
        'absent = { claim_issue = "[unwritten: the claim this session bears on]", '
        'supersedes = "a session supersedes nothing", '
        'rule_ratified = "no decision rule: a session is an observation" }',
        f'window_start = "{window}"',
        "window_start_from = \"log.jsonl's session.start `opened`\"",
        "controls_run = []",
        "known_defects = []",
        "",
        "[regime]",
        f"arm = {json.dumps(regimen.get('arm', ''))}",
        f"substrates = [{json.dumps(substrate)}]" if substrate else "substrates = []",
        f"dogma_version = {int(regimen.get('dogma_version', 0))}",
        "+++",
        "",
        "# " + str(regimen.get("arm", "a served session")),
        "",
        "Assembled by `scripts/assemble-results.py` from the session's log and record",
        f"(the record as written hashes to `{record_digest}`; `digests.json` has every file).",
        "",
        "## Observation",
        "",
        "## Hypothesis",
        "",
        "## Test",
        "",
        "## Results",
        "",
        "## Conclusion",
        "",
    ]
    return "\n".join(lines)


def assemble(args: argparse.Namespace) -> dict:
    # A relative --out is the repository's `results/...`, wherever this runs.
    out = ROOT / args.out
    if out.exists() and any(out.iterdir()):
        raise Refused(f"{out} exists and is not empty")
    log_path, record_path = pathlib.Path(args.log), pathlib.Path(args.record)
    receipt_path = pathlib.Path(args.receipt) if args.receipt else next(
        (p for p in (pathlib.Path(f"{record_path}.receipt.json"), pathlib.Path(f"{log_path}.receipt.json")) if p.is_file()),
        None,
    )
    sidecar_path = pathlib.Path(f"{record_path}.unspellable.json")
    recording = log_path.parent

    log = log_path.read_bytes()
    record = record_path.read_bytes()
    regimen_bytes = pathlib.Path(args.regimen).read_bytes()
    rows = [json.loads(line) for line in lines(record)]
    if not rows:
        raise Refused(f"{record_path} is empty: the session never ended, so serve never wrote its record")
    if not lines(log):
        raise Refused(f"{log_path} is empty")
    start = rows[0]
    claimed = start.get("regimen_sha256")
    regimen_digest = sha256(regimen_bytes)
    if claimed is None:
        raise Refused("the record's start carries no regimen_sha256: it predates the digest, or was not written by serve")
    if claimed != regimen_digest:
        raise Refused(f"{args.regimen} hashes to {regimen_digest}, not the record's regimen_sha256 {claimed}")
    if args.home.rstrip("/").encode() in regimen_bytes:
        raise Refused(f"{args.regimen} carries the home directory; its digest is the record's claim, so it cannot be scrubbed")

    files: dict[str, bytes] = {}
    for digest, entry in sorted(named_files(log).items()):
        source = recording / entry["path"]
        if not source.is_file():
            raise Refused(f"{source}: named by the log and not there")
        data = source.read_bytes()
        if sha256(data) != digest:
            raise Refused(f"{source}: hashes to {sha256(data)}, not {digest}")
        files[f"files/{digest}"] = data

    written: dict[str, bytes] = {"log.jsonl": log, "run.jsonl": record, "regimen.toml": regimen_bytes}
    if sidecar_path.is_file():
        written["unspellable.json"] = sidecar_path.read_bytes()
    summary = next((row for row in rows if row.get("record") == "summary"), None)
    if summary is None:
        raise Refused("the record has no summary row: it was written before serve wrote one")
    product = pathlib.Path(f"{record_path}.product.txt").read_bytes()
    if sha256(product) != summary["product_sha256"]:
        raise Refused(f"{record_path}.product.txt hashes to {sha256(product)}, not the summary's product_sha256")
    written["product.txt"] = product
    if receipt_path is not None:
        written["receipt.json"] = receipt_path.read_bytes()

    # Assembled beside OUT under OUT's own name, and moved there only once it
    # passes: a refusal leaves nothing behind.
    out.parent.mkdir(parents=True, exist_ok=True)
    holding = pathlib.Path(tempfile.mkdtemp(prefix=".assembling-", dir=out.parent))
    try:
        built = build(holding / out.name, written, files, args.home, regimen_bytes, record, log, summary)
        if out.exists():
            out.rmdir()
        built.rename(out)
    finally:
        shutil.rmtree(holding, ignore_errors=True)

    results = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "check-results.py"), str(out)],
        capture_output=True, text=True, cwd=ROOT,
    )
    digests = json.loads((out / "digests.json").read_text())["files"]
    return {
        "out": str(out),
        "regimen_sha256": regimen_digest,
        "files": len(digests),
        "scrubbed": sorted(n for n, row in digests.items() if "as_written_sha256" in row),
        "hygiene": "clean",
        "check_results": {"exit": results.returncode, "says": (results.stdout + results.stderr).strip().split("\n")[:20]},
    }


def build(out: pathlib.Path, written: dict[str, bytes], files: dict[str, bytes], home: str,
          regimen_bytes: bytes, record: bytes, log: bytes, summary: dict) -> pathlib.Path:
    """The directory at OUT, written, checked and hygiene-clean, or a refusal."""
    out.mkdir()
    digests: dict[str, dict] = {}
    for name, data in {**written, **files}.items():
        copy = scrub(data, home) if name in SCRUBBED else data
        (out / name).parent.mkdir(parents=True, exist_ok=True)
        (out / name).write_bytes(copy)
        row = {"sha256": sha256(copy), "bytes": len(copy)}
        if copy != data:
            row["as_written_sha256"] = sha256(data)
        digests[name] = row
    regimen = tomllib.loads(regimen_bytes.decode("utf-8"))
    (out / "README.md").write_text(front_matter(regimen, sha256(record), summary, json.loads(lines(log)[0])["opened"]))
    (out / "digests.json").write_text(json.dumps({
        "regimen_sha256": sha256(regimen_bytes),
        "home_scrubbed_to": "~",
        "files": dict(sorted(digests.items())),
    }, indent=2, sort_keys=True) + "\n")

    binary = diet()
    checked(binary, "check-log", out / "log.jsonl")
    checked(binary, "check-record", out / "run.jsonl")
    checked(binary, "check-regimen", out / "regimen.toml")

    hygiene = subprocess.run(
        ["bash", str(ROOT / "scripts" / "hygiene.sh"), "--tree", str(out)],
        capture_output=True, text=True, cwd=ROOT, env={**os.environ, "LC_ALL": "C"},
    )
    if hygiene.returncode != 0:
        raise Refused(f"hygiene.sh exits {hygiene.returncode} on the assembled copy:\n{(hygiene.stdout + hygiene.stderr).strip()[:4000]}")
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--log", required=True)
    parser.add_argument("--record", required=True)
    parser.add_argument("--regimen", required=True)
    parser.add_argument("--out", required=True)
    parser.add_argument("--receipt")
    parser.add_argument("--home", default=os.path.expanduser("~"))
    args = parser.parse_args()
    try:
        report = assemble(args)
    except (Refused, OSError, ValueError, KeyError) as err:
        print(json.dumps({"refused": str(err)}))
        return 1
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
