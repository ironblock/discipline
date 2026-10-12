#!/usr/bin/env python3
"""The step warrants' differential corpus (#783): each input below, judged by
the predecessor's own two patterns with Python's `re`, written to
vectors.jsonl beside this file. diet's hand-written matchers
(`diet::drive::warrants`) must agree with every line.

The patterns are the predecessor's, its intent: capture/templates.py,
`gates_for_step`, at 15f4874 (the file last changed in aabfcf9,
2026-08-09). Copied verbatim; do not edit them here without editing the
matchers, and regenerate:

    python3 -I diet/tests/fixtures/step-warrants/generate.py
"""

import json
import pathlib
import re

SURPRISE_RE = re.compile(
    r"\b(actually|instead|turns out|however|wait[,.]|doesn't support|"
    r"not supported|unsupported|can't|cannot|must (?:be|use|not)|"
    r"expects?|requires?|error|failed|failure|mismatch|deprecated|"
    r"confirmed via|it's called|renamed)\b", re.I)

ERROR_RE = re.compile(r"(error|Error|ERROR|Traceback|failed|FAILED|"
                      r"Cannot find|not found|EEXIST|ENOENT|TS\d{4})")

WORDS = [
    "actually", "instead", "turns out", "however", "doesn't support",
    "not supported", "unsupported", "can't", "cannot", "must be", "must use",
    "must not", "expect", "expects", "require", "requires", "error", "failed",
    "failure", "mismatch", "deprecated", "confirmed via", "it's called",
    "renamed",
]

INPUTS = [
    "",
    "Nothing to see here.",
    "wait, x", "wait,x", "wait.", "wait.x", "Wait, really", "wait ,", "waited,",
    "turns  out", "turnsout", "turns\tout", "must  be", "mustbe", "must bee",
    "expected", "expectsx", "required", "requirement", "errors", "terror",
    "error_code", "_error", "error_", "Error", "ERROR!", "eRrOr",
    "can’t", "doesn’t support", "it’s called", "its called",
    "renamed.", "renamedx", "unsupportedly", "notsupported",
    "café error", "naïve failure", "éerror", "erroré", "1error", "error1",
    "Traceback (most recent call last):", "traceback", "FAILED", "Failed",
    "Cannot find module", "cannot find module", "not found", "Not Found",
    "EEXIST: file exists", "eexist", "ENOENT", "TS2345: type mismatch",
    "TS234", "ts2345", "TS23456", "TS١٢٣٤", "TS12a4",
    "the build failed", "no errors here", "terrorism", "mismatched",
    "confirmed  via", "confirmed via email", "however,", "nevertheless",
]
INPUTS += [w for w in WORDS]
INPUTS += [w.upper() for w in WORDS]
INPUTS += [f"x{w}" for w in WORDS] + [f"{w}x" for w in WORDS]
INPUTS += [f"({w})" for w in WORDS] + [f"a {w}." for w in WORDS]

here = pathlib.Path(__file__).resolve().parent
with open(here / "vectors.jsonl", "w", encoding="utf-8") as out:
    for text in INPUTS:
        line = {
            "text": text,
            "surprise": SURPRISE_RE.search(text) is not None,
            "error": ERROR_RE.search(text) is not None,
        }
        out.write(json.dumps(line, ensure_ascii=False, sort_keys=True) + "\n")
