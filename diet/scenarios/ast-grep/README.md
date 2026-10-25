# The ast-grep tutorial (#971)

The first slice of #971: one model and ast-grep (`run` and `outline`) on small TypeScript projects. This directory holds what the tutorial is graded with. The scenarios that run it come with the answer tool (#971's runner PR).

**Model-facing text here is a DRAFT.** That covers every ask in `tasks.py`, the answer tool's description and result in `answer-tool.json`, and any framing. The wording is the maintainer's. Each such text says `DRAFT.` until he settles it.

## Fixtures: `gen.py`
`python3 -I gen.py SEED OUT TRUTH` writes one instance: a project under `OUT` (`package.json`, `src/` in three directories, a barrel `src/index.ts`) and the answer to every task family in `TRUTH`.
- **Every pass gets a fresh instance.** It has the same families, with a new layout, new names (verb + noun functions, noun interfaces) and new values (return types, call counts, which names the barrel re-exports and under what alias).
- **The truth comes from the construction, never from ast-grep.** The generator places each function, call, export and line itself and records where. Grading the tool under test with itself would be circular.
- **Every instance has an answer for every family.** A seed whose draw leaves a list empty moves on to the next seed, so no answer is "none" by construction. The counts may be 0, on purpose.

## Tasks: `tasks.py`
Six tutorial and six held-out tasks. Each is one truth family and one of the known gaps (`GAPS`). A held-out task tests the same gap on another surface:

| gap | tutorial | held-out |
|---|---|---|
| a function pattern needs its return type, `: $RET` | t1 `function` declarations returning a type | h1 arrow constants returning a type |
| a miss: `run` prints nothing and exits 1, `outline` prints `nothing found` and exits 0 | t2 a call count that is often 0, with comment and string decoys | h2 is a name defined (often only in a comment and a string)? |
| a directory's exports are not the barrel's public surface | t3 the public functions, aliases included | h3 the interfaces exported but not public |
| `has` and `inside` look one level down unless `stopBy: end` | t4 functions with an `await` at any depth | h4 functions calling the log function inside a `try` |
| `$name` in double quotes is the shell's; lowercase `$x` in a pattern is literal | t5 callers of `$emit(...)` | h5 the count of `$track(...)` |
| `--json` is one bare array, and its lines are 0-based | t6 a function's file and 1-based line | h6 an interface's file and 1-based line |

## Answers: `answer-tool.json` and `check.py`
- Answers come through a tool with a schema (the structured-answers rule), never parsed from prose: `answer`, with `task` and `answer`, a list of strings.
- `check.py` is the `grade` step's check. It grades the last well-formed `answer` call naming the task, in the turn the grade reads (`DIET_GRADE_TURN`), against the instance's truth (`DIET_SERVICE_FIXTURE_TRUTH`). It never grades an exit status.
  - A list is graded as a set: no item missing, none extra, none twice.
  - A count is digits; a yes or no is one word; a location is a path, then a 1-based line.
  - Names may carry backticks or `()`, and a path may lead with `./`.
- An answer given in prose, with no call, is wrong.

## Reading the docs: `docs_reads.py`
A pass that reads the docs rolls back, so what counts as a read is parsed, never matched as text. `reads_of(cmd, skills)` returns one bash call's reads:
- `help`: `--help`, `-h` or `help` to `ast-grep` or `sg`, on any subcommand;
- `man`: `man`, `tldr` or `cheat` of the tool;
- `web`: a fetch of ast-grep's site or repository;
- `skill`: any reading command on a path under the official skills' root, when the condition gives them.

Option values are never flags, `cd` is followed, `$(...)` counts as a command, and a heredoc's body counts only when it is fed to a shell. `docs_reads_fixtures.json` holds 23 reads and 16 non-reads.

## Solvable, and the gaps are real: `ref.py`
`python3 -I ref.py [SEEDS]` needs `ast-grep` and runs outside `./verify.sh`. On each fresh instance it runs one reference solution per family and one naive try that walks into the family's gap, and grades both against the truth.
- The references only show the tasks can be answered; they never feed the truth.
- With ast-grep 0.44.0 on 20 seeds:
  - every reference is right on every seed;
  - every naive try is wrong on every seed, except h2's text search, which is right whenever the name really is defined (11 of 20).
- One more gap it found: `export { $$$S } from $P` doesn't parse as a pattern ("Multiple AST nodes"). The barrel is read by `kind: export_statement`.

## Checks
`gen.py --selftest`, `check.py --selftest` and `docs_reads.py --selftest` run in `./verify.sh`'s scenarios check, with no ast-grep needed. Each was seen to go red on a mutant:
- the generator, its line numbers off by one;
- the grader, a refused call counted;
- the classifier, its option values read as flags.
