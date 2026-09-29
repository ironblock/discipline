#!/usr/bin/env python3
"""Derive a rung's admission word from its admission directory (#183), instead of trusting the hand-written one.

For every `substrates/admission/<substrate-id>/<fingerprint-prefix>/admission.toml` in the tree:
  1. its own `admission-recompute.sh` must exit 0: each cited result re-hashes to the digest stated, and each
     result's own recompute passes;
  2. the word is derived from the results' words, by planning's rule (#143):
       - any cell reading `fail`, a depth cell reading `fail`, or a parity fire reading `refuted` or
         `inconclusive`: `not admitted (<the first failing result>)` (comment 5882266710);
       - else any cell reading `unadjudicated`: no word; the admission is held, and waits on a re-run
         (comment 5885436821);
       - else, every cell `pass`, `n/a (reason)`, `unreported` or, on the canary, `baseline` (comments
         5884256685, 5885752512), the depth cell `pass` and the parity fire `supported`: `admitted`;
       - any other word anywhere is refused;
  3. the record's word must be the derived one: `word = "admitted"`, `word = "not admitted (...)"` naming the
     derived failing result, or `word_held` for a held admission.

Where #183's text maps an unadjudicated cell to `not admitted`, this follows planning's later ruling
(5885436821), which #183 did not cite: an unadjudicated cell neither admits nor bars.

Usage: derive_admission.py --all [--root DIR] | --selftest
Exit 0 every record's word is its derived word; 1 any is not, or a record fails its own recompute;
2 the tree cannot be read. Each failure is one line `admission: FAIL <class>: <detail>`.
"""
import json, pathlib, re, subprocess, sys, tomllib

CELL_OK = re.compile(r"pass|unreported|n/a \(.+\)")
CELL_WORDS = re.compile(r"pass|fail|unreported|unadjudicated|baseline|n/a \(.+\)")

def derive(cells: dict, depth: str, parity: str):
    """(word, reason): word is 'admitted', 'not admitted (<result>)', or None for a held admission."""
    for name, w in cells.items():
        if not CELL_WORDS.fullmatch(w) or (w == "baseline" and name != "canary"):
            raise ValueError(f"cell {name} reads {w!r}, not one of the ruled words")
    if depth not in ("pass", "fail", "unadjudicated"):
        raise ValueError(f"the depth cell reads {depth!r}")
    if parity not in ("supported", "refuted", "inconclusive", "unadjudicated"):
        raise ValueError(f"the parity fire reads {parity!r}")
    failing = [name for name, w in cells.items() if w == "fail"]
    if failing:
        return f"not admitted ({failing[0]})", f"cell {failing[0]} reads fail"
    if depth == "fail":
        return "not admitted (depth)", "the depth cell reads fail"
    if parity in ("refuted", "inconclusive"):
        return "not admitted (parity)", f"the parity fire reads {parity}"
    held = [name for name, w in cells.items() if w == "unadjudicated"] + (["depth"] if depth == "unadjudicated" else []) + (["parity"] if parity == "unadjudicated" else [])
    if held:
        return None, f"{', '.join(held)} unadjudicated: the admission waits on a re-run"
    if all(CELL_OK.fullmatch(w) or (name == "canary" and w == "baseline") for name, w in cells.items()):
        return "admitted", "every result passes"
    raise ValueError("no rule applies")  # unreachable: every ruled word is handled above

def read_results(adm_dir: pathlib.Path, root: pathlib.Path):
    adm = tomllib.loads((adm_dir / "admission.toml").read_text(encoding="utf-8"))
    cells = {k: v["word"] for k, v in tomllib.loads((adm_dir / "cells.toml").read_text(encoding="utf-8")).items() if isinstance(v, dict)}
    depth = json.loads((adm_dir / "depth/raw/decide.json").read_text(encoding="utf-8"))["word"]
    pp = root / adm["results"]["parity"]["path"] / "README.md"
    parity = tomllib.loads(pp.read_text(encoding="utf-8").split("+++")[1])["result"]
    return adm, cells, depth, parity

def names_same(written, word) -> bool:
    """The written word is the derived one; a `not admitted (...)` may add detail after the failing result's name."""
    if written == word:
        return True
    m, n = re.fullmatch(r"not admitted \((\w+)\b[^)]*\)", str(written)), re.fullmatch(r"not admitted \((\w+)\)", word)
    return bool(m and n and m.group(1) == n.group(1))

def check(adm_dir: pathlib.Path, root: pathlib.Path) -> list[str]:
    fails = []
    rc = subprocess.run(["bash", str(adm_dir / "admission-recompute.sh")], capture_output=True, text=True).returncode
    if rc != 0:
        fails.append(f"admission.record-does-not-recompute: {adm_dir.relative_to(root)}'s admission-recompute.sh exited {rc}")
    try:
        adm, cells, depth, parity = read_results(adm_dir, root)
        word, why = derive(cells, depth, parity)
    except (OSError, KeyError, ValueError, IndexError, tomllib.TOMLDecodeError, json.JSONDecodeError) as e:
        return fails + [f"admission.results-unreadable: {adm_dir.relative_to(root)}: {e}"]
    written = adm.get("word")
    if word is None and (written is not None or not str(adm.get("word_held", "")).strip()):
        fails.append(f"admission.word-not-derived: {adm_dir.relative_to(root)} writes {written!r}; the results derive a held admission ({why})")
    elif word is not None and "word_held" in adm:
        fails.append(f"admission.word-not-derived: {adm_dir.relative_to(root)} holds a word the results derive as {word!r}")
    elif word is not None and not names_same(written, word):
        fails.append(f"admission.word-not-derived: {adm_dir.relative_to(root)} writes {written!r}; the results derive {word!r} ({why})")

    return fails

def selftest() -> int:
    ok_cells = {"identity": "pass", "kwarg_delivery": "pass", "rendered_effort": "n/a (no levels)", "canary": "pass", "checkpoint_restore": "pass"}
    cases = [
        ("every result passing", ok_cells, "pass", "supported", "admitted"),
        ("an n/a cell with its reason", ok_cells, "pass", "supported", "admitted"),
        ("an unreported cell", {**ok_cells, "canary": "unreported"}, "pass", "supported", "admitted"),
        ("a first canary draw", {**ok_cells, "canary": "baseline"}, "pass", "supported", "admitted"),
        ("an unadjudicated cell holds", {**ok_cells, "checkpoint_restore": "unadjudicated"}, "pass", "supported", None),
        ("an unadjudicated depth cell holds", ok_cells, "unadjudicated", "supported", None),
        ("an unadjudicated parity fire holds", ok_cells, "pass", "unadjudicated", None),
        ("a failing cell bars, named", {**ok_cells, "kwarg_delivery": "fail"}, "pass", "supported", "not admitted (kwarg_delivery)"),
        ("a failing cell bars even beside an unadjudicated one", {**ok_cells, "kwarg_delivery": "fail", "checkpoint_restore": "unadjudicated"}, "pass", "supported", "not admitted (kwarg_delivery)"),
        ("a depth cliff bars", ok_cells, "fail", "supported", "not admitted (depth)"),
        ("a refuted parity fire bars", ok_cells, "pass", "refuted", "not admitted (parity)"),
        ("an inconclusive parity fire bars", ok_cells, "pass", "inconclusive", "not admitted (parity)"),
    ]
    refused = [
        ("baseline on a cell other than the canary", {**ok_cells, "identity": "baseline"}, "pass", "supported"),
        ("a word no ruling names", {**ok_cells, "identity": "ok"}, "pass", "supported"),
        ("an n/a without its reason", {**ok_cells, "rendered_effort": "n/a"}, "pass", "supported"),
        ("a parity word no rule names", ok_cells, "pass", "passed"),
        ("a depth word no rule names", ok_cells, "no cliff", "supported"),
    ]
    bad = 0
    for label, c, d, p, want in cases:
        got = derive(c, d, p)[0]; good = got == want; bad += not good
        print(f"{'ok  ' if good else 'FAIL'}  derive: {label}: {got!r}" + ("" if good else f" (expected {want!r})"))
    for label, c, d, p in refused:
        try:
            derive(c, d, p); good = False
        except ValueError:
            good = True
        bad += not good
        print(f"{'ok  ' if good else 'FAIL'}  derive refuses {label}")
    print(f"derive_admission selftest: {'all pass' if not bad else f'{bad} failing'}"); return 1 if bad else 0

def main(argv) -> int:
    if argv == ["--selftest"]:
        return selftest()
    if not argv or argv[0] != "--all":
        print("usage: derive_admission.py --all [--root DIR] | --selftest", file=sys.stderr); return 2
    root = pathlib.Path(argv[2]) if len(argv) == 3 and argv[1] == "--root" else pathlib.Path(__file__).resolve().parents[2]
    dirs = sorted(p.parent for p in (root / "substrates/admission").glob("*/*/admission.toml"))
    fails = [f for d in dirs for f in check(d, root)]
    for f in fails: print(f"admission: FAIL {f}")
    print(f"admission: {len(dirs)} record(s); " + ("every word is its derived word" if not fails else f"{len(fails)} failure(s)"))
    return 1 if fails else 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
