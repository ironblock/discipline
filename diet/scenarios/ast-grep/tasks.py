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
    # Task set 2 (the maintainer, 2026-10-29): the gaps the gap-fill rounds' failures showed.
    "kind-filter": "an outline `--type` the ask doesn't name is right only where the item happens to be that kind",
    "field": "`field` belongs to a relation: `has: {field: …}` matches a node's own field, `stopBy: end` reaches past it",
    "fn-kind": "a function written as a value (`function_expression`, `generator_function`) is not a declaration",
    "spread": "`...` in a pattern is literal: it matches a spread or a rest, and nothing else",
    "exactly-one": "`$A` matches exactly one node: one argument or parameter, never none and never several",
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
     # DRAFT (Dispatch, 2026-10-29, as t4's): "declared with the `function` keyword" also read as `const f = function`.
     "what": "matches every function declaration (a `function name(…) { … }` statement) under src/ whose declared return type is exactly `{service.fixture.target}`, and nothing else."},
    {"id": "t2-zero-calls", "split": "tutorial", "family": "zero-calls", "gap": "miss",
     "what": "matches every call expression under src/ that calls `{service.fixture.zero_fn}` directly, as `{service.fixture.zero_fn}(...)`, and nothing else. In the answer, also say what that command prints, and its exit status, when nothing matches.", "tool": "answer-tool-miss.json"},
    {"id": "t3-public-surface", "split": "tutorial", "family": "public-surface", "gap": "barrel",
     "what": "shows each export of the package's public surface, the entry point src/index.ts, and nothing else."},
    {"id": "t4-await-any", "split": "tutorial", "family": "await-any", "gap": "stop-by",
     # DRAFT (the maintainer, 2026-10-29): "declared with the `function` keyword" also read as `const f = async function`.
     "what": "matches every function declaration (a `function name(…) { … }` statement) under src/ that contains an `await` anywhere in its body, and nothing else."},
    {"id": "t5-emit-calls", "split": "tutorial", "family": "emit-calls", "gap": "dollar",
     "what": "matches every call to `$emit(...)` under src/, and nothing else."},
    {"id": "t6-fn-outline", "split": "tutorial", "family": "fn-outline", "gap": "outline",
     "what": "outlines the function `{service.fixture.line_fn}` under src/, and nothing else."},
    {"id": "h1-ret-arrow", "split": "held-out-1", "now_taught": ['field'], "family": "ret-arrow", "gap": "return-type",
     "what": "matches every arrow function assigned to a `const` under src/ whose declared return type is exactly `{service.fixture.arrow_target}`, and nothing else."},
    {"id": "h2-maybe-defined", "split": "held-out-1", "now_taught": ['kind-filter'], "family": "maybe-defined", "gap": "miss",
     "what": "outlines the definition of `{service.fixture.maybe}` under src/, if there is one, and nothing else. In the answer, also say what that command prints, and its exit status, when nothing matches.", "tool": "answer-tool-miss.json"},
    {"id": "h3-public-types", "split": "held-out-1", "now_taught": ['exactly-one'], "family": "public-types", "gap": "barrel",
     "what": "matches each statement of the package's entry point, src/index.ts, that exports only types, and nothing else."},
    {"id": "h4-log-in-try", "split": "held-out-1", "now_taught": ['exactly-one'], "family": "log-in-try", "gap": "stop-by",
     "what": "matches every call to `{service.fixture.log_fn}(...)` under src/ that sits inside a `try` block, at any depth, and nothing else."},
    {"id": "h5-track-calls", "split": "held-out-1", "family": "track-calls", "gap": "dollar",
     "what": "matches every call to `$track(...)` under src/, and nothing else."},
    {"id": "h6-itf-outline", "split": "held-out-1", "family": "itf-outline", "gap": "outline",
     "what": "outlines the interface `{service.fixture.line_itf}` under src/, and nothing else."},
    # Task set 2 (the maintainer, 2026-10-29): two tutorial operations per gap, and a fresh held-out set on other surfaces.
    # DRAFT asks.
    {"id": 't7-ret-method', "split": 'tutorial', "family": 'ret-method', "gap": 'return-type', "set": 2,
     "what": 'matches every method, in a class under src/, whose declared return type is exactly `{service.fixture.target}`, and nothing else.'},
    {"id": 't8-zero-new', "split": 'tutorial', "family": 'zero-new', "gap": 'miss', "set": 2,
     "what": 'matches every `new {service.fixture.new_cls}(...)` expression under src/, and nothing else. In the answer, also say what that command prints, and its exit status, when nothing matches.', "tool": 'answer-tool-miss.json'},
    {"id": 't9-public-aliases', "split": 'tutorial', "family": 'public-aliases', "gap": 'barrel', "set": 2,
     "what": "matches each statement of the package's entry point, src/index.ts, that re-exports a name under another name (`export { a as b } …`), and nothing else."},
    {"id": 't10-throw-in-for', "split": 'tutorial', "family": 'throw-in-for', "gap": 'stop-by', "set": 2,
     "what": 'matches every `throw` statement under src/ that sits inside a `for (init; test; update)` loop, at any depth, and nothing else.'},
    {"id": 't11-store-get', "split": 'tutorial', "family": 'store-get', "gap": 'dollar', "set": 2,
     "what": 'matches every call to `$store.get(...)` under src/, and nothing else.'},
    {"id": 't12-cls-outline', "split": 'tutorial', "family": 'cls-outline', "gap": 'outline', "set": 2,
     "what": 'outlines the class `{service.fixture.line_cls}` under src/, and nothing else.'},
    {"id": 't13-kind-dir', "split": 'tutorial', "family": 'kind-dir', "gap": 'kind-filter', "set": 2,
     "what": 'outlines the item named `{service.fixture.kind_dir}` under src/, and nothing else.'},
    {"id": 't14-kind-file', "split": 'tutorial', "family": 'kind-file', "gap": 'kind-filter', "set": 2,
     "what": 'outlines the item named `{service.fixture.kind_file_item}` in `{service.fixture.kind_file}`, and nothing else.'},
    {"id": 't15-init-call', "split": 'tutorial', "family": 'init-call', "gap": 'field', "set": 2,
     "what": 'matches every variable declarator under src/ whose initializer is itself a call to `{service.fixture.init_fn}(...)`, not a call that wraps one, and nothing else.'},
    {"id": 't16-cond-call', "split": 'tutorial', "family": 'cond-call', "gap": 'field', "set": 2,
     "what": 'matches every `if` statement under src/ whose condition is a call to `{service.fixture.cond_fn}(...)`, and nothing else.'},
    {"id": 't17-fexpr-await', "split": 'tutorial', "family": 'fexpr-await', "gap": 'fn-kind', "set": 2,
     "what": 'matches every function expression (a `function` written as a value) under src/ that contains an `await` anywhere in its body, and nothing else.'},
    {"id": 't18-gen-decl', "split": 'tutorial', "family": 'gen-decl', "gap": 'fn-kind', "set": 2,
     "what": 'matches every generator function declaration (a `function* name(…) { … }` statement) under src/, and nothing else.'},
    {"id": 't19-spread-call', "split": 'tutorial', "family": 'spread-call', "gap": 'spread', "set": 2,
     "what": 'matches every call to `{service.fixture.spread_fn}(...)` under src/ whose argument is a spread, as `{service.fixture.spread_fn}(...xs)`, and nothing else.'},
    {"id": 't20-rest-decl', "split": 'tutorial', "family": 'rest-decl', "gap": 'spread', "set": 2,
     "what": 'matches every function declaration under src/ that has a rest parameter (`...name`), and nothing else.'},
    {"id": 't21-one-call', "split": 'tutorial', "family": 'one-call', "gap": 'exactly-one', "set": 2,
     "what": 'matches every call to `{service.fixture.one_fn}(...)` under src/ with exactly one argument, and nothing else.'},
    {"id": 't22-two-params', "split": 'tutorial', "family": 'two-params', "gap": 'exactly-one', "set": 2,
     "what": 'matches every function declaration under src/ with exactly two parameters, and nothing else.'},
    {"id": 'h7-ret-async', "split": 'held-out', "family": 'ret-async', "gap": 'return-type', "set": 2,
     "what": 'matches every `async` function declaration under src/ whose declared return type is exactly `Promise<{service.fixture.target}>`, and nothing else.'},
    {"id": 'h8-itf-maybe', "split": 'held-out', "family": 'itf-maybe', "gap": 'miss', "set": 2,
     "what": 'outlines the interface `{service.fixture.maybe_itf}` under src/, if there is one, and nothing else. In the answer, also say what that command prints, and its exit status, when nothing matches.', "tool": 'answer-tool-miss.json'},
    {"id": 'h9-public-values', "split": 'held-out', "family": 'public-values', "gap": 'barrel', "set": 2,
     "what": "matches each statement of the package's entry point, src/index.ts, that exports values (not only types), and nothing else."},
    {"id": 'h10-report-in-catch', "split": 'held-out', "family": 'report-in-catch', "gap": 'stop-by', "set": 2,
     "what": 'matches every call to `{service.fixture.report_fn}(...)` under src/ that sits inside a `catch` clause, at any depth, and nothing else.'},
    {"id": 'h11-log-calls', "split": 'held-out', "family": 'log-calls', "gap": 'dollar', "set": 2,
     "what": 'matches every call to `$log(...)` under src/, and nothing else.'},
    {"id": 'h12-alias-outline', "split": 'held-out', "family": 'alias-outline', "gap": 'outline', "set": 2,
     "what": 'outlines the type alias `{service.fixture.line_alias}` under src/, and nothing else.'},
    {"id": 'h13-kind-hidden', "split": 'held-out', "family": 'kind-hidden', "gap": 'kind-filter', "set": 2,
     "what": "outlines the item named `{service.fixture.kind_hidden}` under src/, which isn't exported, and nothing else."},
    {"id": 'h14-prop-arrow', "split": 'held-out', "family": 'prop-arrow', "gap": 'field', "set": 2,
     "what": 'matches every object property under src/ whose value is an arrow function, and nothing else.'},
    {"id": 'h15-nested-decl', "split": 'held-out', "family": 'nested-decl', "gap": 'fn-kind', "set": 2,
     "what": 'matches every function declaration under src/ that sits inside another function declaration, and nothing else.'},
    {"id": 'h16-array-spread', "split": 'held-out', "family": 'array-spread', "gap": 'spread', "set": 2,
     "what": 'matches every array literal under src/ that contains a spread (`...xs`), and nothing else.'},
    {"id": 'h17-none-call', "split": 'held-out', "family": 'none-call', "gap": 'exactly-one', "set": 2,
     "what": 'matches every call to `{service.fixture.none_fn}(...)` under src/ with no arguments, and nothing else.'},
]
for t in TASKS:
    t["ask"] = ASK.format(what=t["what"])

TUTORIAL = [t for t in TASKS if t["split"] == "tutorial"]
# Held-out set 2 is the held-out set; set 1's (h1..h6) stay runnable as "held-out set 1" (the gap-fill rounds' results).
HELD_OUT = [t for t in TASKS if t["split"] == "held-out"]
HELD_OUT_1 = [t for t in TASKS if t["split"] == "held-out-1"]
