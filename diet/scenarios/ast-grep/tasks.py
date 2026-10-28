"""The ast-grep tutorial's tasks (#971): one operation each, demonstrated once on a fresh instance.

Each task is one family of `gen.py`'s truth: the lines a right command matches. The answer is the command, through
the answer tool; `check.py` runs it out of the model's sight against a pristine copy of the instance and compares its
matches with the truth, so equivalent spellings pass and a right answer reached any other way fails. The asks fill
`{service.fixture.KEY}` from the instance's params. Tutorial and held-out tasks test the same gaps on other surfaces.

DRAFT: every ask's wording is model-facing, and model-facing text is the maintainer's. Placeholders, provisional.
"""

# The gaps each family exercises (#971, #976), for a reader; the grader never reads them.
GAPS = {
    "return-type": "a function pattern matches only with its return type: `: $RET`",
    "miss": "a miss prints nothing (run, exit 1) or `nothing found` (outline, exit 0): the right command may match nothing",
    "barrel": "a directory's exports are not the package's public surface: the barrel is",
    "stop-by": "`has` and `inside` look one level down unless `stopBy: end`",
    "dollar": "`$name` in double quotes is the shell's; lowercase `$x` in a pattern is literal",
    "outline": "`outline --match` takes a regex: anchor it; and on a directory outline lists only exports unless `--items structure`",
}

ASK = "DRAFT. Give the ast-grep command, run from this directory, that {what}"

# Operations whose gap a command can't show (Dispatch, 2026-10-27): the answer is never run, so a lesson about reading
# the tool's output is invisible in it. The maintainer ruled (2026-10-27) that "no match" is part of mastery: t2 and h2
# answer through `answer-tool-miss.json`, whose structured fields say what the command does on a miss, graded against
# `miss_truth.json`. Their asks (above) are the reshaped ones, DRAFT; this table stays as the record of why.
INVISIBLE = {
    "miss": {"why": "the right command is the same whether or not the model knows a miss prints nothing (run, exit 1) "
                    "or `nothing found` (outline, exit 0): that lesson is about reading output, and no output is read",
             "reshaped": {
                 "t2-zero-calls": "DRAFT. Give the ast-grep command that matches every direct call to `{service.fixture.zero_fn}` "
                                  "under src/, and say what it prints, and its exit status, when there is no such call.",
                 "h2-maybe-defined": "DRAFT. Give the ast-grep outline command that shows the definition of "
                                     "`{service.fixture.maybe}` under src/, and say what it prints, and its exit status, when "
                                     "there is none."}},
}

TASKS = [
    {"id": "t1-ret-decl", "split": "tutorial", "family": "ret-decl", "gap": "return-type",
     "what": "matches every function declared with the `function` keyword under src/ whose declared return type is exactly `{service.fixture.target}`, and nothing else."},
    {"id": "t2-zero-calls", "split": "tutorial", "family": "zero-calls", "gap": "miss",
     "what": "matches every call expression under src/ that calls `{service.fixture.zero_fn}` directly, as `{service.fixture.zero_fn}(...)`, and nothing else. In the answer, also say what that command prints, and its exit status, when nothing matches.", "tool": "answer-tool-miss.json"},
    {"id": "t3-public-surface", "split": "tutorial", "family": "public-surface", "gap": "barrel",
     "what": "shows each export of the package's public surface, the entry point src/index.ts, and nothing else."},
    {"id": "t4-await-any", "split": "tutorial", "family": "await-any", "gap": "stop-by",
     "what": "matches every function declared with the `function` keyword under src/ that contains an `await` expression anywhere in its body, at any depth, and nothing else."},
    {"id": "t5-emit-calls", "split": "tutorial", "family": "emit-calls", "gap": "dollar",
     "what": "matches every call to `$emit(...)` under src/, and nothing else."},
    {"id": "t6-fn-outline", "split": "tutorial", "family": "fn-outline", "gap": "outline",
     "what": "outlines the function `{service.fixture.line_fn}` under src/, and nothing else."},
    {"id": "h1-ret-arrow", "split": "held-out", "family": "ret-arrow", "gap": "return-type",
     "what": "matches every arrow function assigned to a `const` under src/ whose declared return type is exactly `{service.fixture.arrow_target}`, and nothing else."},
    {"id": "h2-maybe-defined", "split": "held-out", "family": "maybe-defined", "gap": "miss",
     "what": "outlines the definition of `{service.fixture.maybe}` under src/, if there is one, and nothing else. In the answer, also say what that command prints, and its exit status, when nothing matches.", "tool": "answer-tool-miss.json"},
    {"id": "h3-public-types", "split": "held-out", "family": "public-types", "gap": "barrel",
     "what": "matches each statement of the package's entry point, src/index.ts, that exports only types, and nothing else."},
    {"id": "h4-log-in-try", "split": "held-out", "family": "log-in-try", "gap": "stop-by",
     "what": "matches every call to `{service.fixture.log_fn}(...)` under src/ that sits inside a `try` block, at any depth, and nothing else."},
    {"id": "h5-track-calls", "split": "held-out", "family": "track-calls", "gap": "dollar",
     "what": "matches every call to `$track(...)` under src/, and nothing else."},
    {"id": "h6-itf-outline", "split": "held-out", "family": "itf-outline", "gap": "outline",
     "what": "outlines the interface `{service.fixture.line_itf}` under src/, and nothing else."},
]
for t in TASKS:
    t["ask"] = ASK.format(what=t["what"])

TUTORIAL = [t for t in TASKS if t["split"] == "tutorial"]
HELD_OUT = [t for t in TASKS if t["split"] == "held-out"]
