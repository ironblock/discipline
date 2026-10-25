"""The ast-grep tutorial's tasks (#971): one source for the scenarios' asks and the grader's families.

Each task is one family of `gen.py`'s truth, asked of a fresh instance. The asks fill `{service.fixture.KEY}` from
the instance's params, as a scenario fills an ask. The tutorial and held-out tasks test the same gaps on different
surfaces, so a held-out task needs what the tutorial taught, not its answer.

DRAFT: every ask's wording is model-facing, and model-facing text is the maintainer's. These are placeholders.
"""

# The gaps each family exercises (#971, #976), for a reader; the grader never reads them.
GAPS = {
    "return-type": "a function pattern matches only with its return type: `: $RET`",
    "miss": "a miss prints nothing (run, exit 1) or `nothing found` (outline, exit 0): grade the answer, not the exit",
    "barrel": "a directory's exports are not the package's public surface: the barrel is",
    "stop-by": "`has` and `inside` look one level down unless `stopBy: end`",
    "dollar": "`$name` in double quotes is the shell's; lowercase `$x` in a pattern is literal",
    "json-lines": "`--json` is one bare array, and its lines are 0-based",
}

TASKS = [
    # Tutorial.
    {"id": "t1-ret-decl", "split": "tutorial", "family": "ret-decl", "gap": "return-type",
     "ask": "DRAFT. In this directory's TypeScript project, list every function declared with the `function` keyword "
            "under src/ whose declared return type is exactly `{service.fixture.target}`. Answer with their names."},
    {"id": "t2-zero-calls", "split": "tutorial", "family": "zero-calls", "gap": "miss",
     "ask": "DRAFT. How many call expressions under src/ call `{service.fixture.zero_fn}` directly, as "
            "`{service.fixture.zero_fn}(...)`? Answer with the count as the one item."},
    {"id": "t3-public-fns", "split": "tutorial", "family": "public-fns", "gap": "barrel",
     "ask": "DRAFT. Which functions does the package's public surface (what src/index.ts exports) offer? Answer with "
            "the names an importer of the package would use."},
    {"id": "t4-await-any", "split": "tutorial", "family": "await-any", "gap": "stop-by",
     "ask": "DRAFT. List every function declared with the `function` keyword under src/ that contains an `await` "
            "expression anywhere in its body, at any depth. Answer with their names."},
    {"id": "t5-emit-callers", "split": "tutorial", "family": "emit-callers", "gap": "dollar",
     "ask": "DRAFT. List every function declared with the `function` keyword under src/ that calls `$emit(...)`. "
            "Answer with their names."},
    {"id": "t6-fn-line", "split": "tutorial", "family": "fn-line", "gap": "json-lines",
     "ask": "DRAFT. Where is the function `{service.fixture.line_fn}` declared? Answer with two items: its file's path "
            "relative to this directory, then the line its declaration starts on, numbered from 1 as an editor numbers "
            "lines."},
    # Held-out.
    {"id": "h1-ret-arrow", "split": "held-out", "family": "ret-arrow", "gap": "return-type",
     "ask": "DRAFT. List every arrow function assigned to a `const` under src/ whose declared return type is exactly "
            "`{service.fixture.arrow_target}`. Answer with the constants' names."},
    {"id": "h2-maybe-defined", "split": "held-out", "family": "maybe-defined", "gap": "miss",
     "ask": "DRAFT. Is `{service.fixture.maybe}` defined anywhere under src/, as a function, class, interface or type? "
            "Answer with one item: yes or no."},
    {"id": "h3-hidden-itfs", "split": "held-out", "family": "hidden-itfs", "gap": "barrel",
     "ask": "DRAFT. Which interfaces are exported by some file under src/ but are not part of the package's public "
            "surface (what src/index.ts exports)? Answer with their names."},
    {"id": "h4-log-in-try", "split": "held-out", "family": "log-in-try", "gap": "stop-by",
     "ask": "DRAFT. List every function declared with the `function` keyword under src/ that calls "
            "`{service.fixture.log_fn}(...)` somewhere inside a `try` block, at any depth. Answer with their names."},
    {"id": "h5-track-count", "split": "held-out", "family": "track-count", "gap": "dollar",
     "ask": "DRAFT. How many calls to `$track(...)` are there under src/? Answer with the count as the one item."},
    {"id": "h6-itf-line", "split": "held-out", "family": "itf-line", "gap": "json-lines",
     "ask": "DRAFT. Where does the interface `{service.fixture.line_itf}` begin? Answer with two items: its file's path "
            "relative to this directory, then the line it starts on, numbered from 1 as an editor numbers lines."},
]

TUTORIAL = [t for t in TASKS if t["split"] == "tutorial"]
HELD_OUT = [t for t in TASKS if t["split"] == "held-out"]
