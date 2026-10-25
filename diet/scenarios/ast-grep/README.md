# The ast-grep tutorial (#971)

The first slice of #971: one model and ast-grep (`run` and `outline`) on small TypeScript projects. This directory holds what the tutorial is graded with, and the scenarios that run a pass of it.

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

Option values are never flags, `cd` is followed, `$(...)` counts as a command, and a heredoc's body counts only when it is fed to a shell. The tool run bare prints its whole help, so it is a read, but a subcommand given nothing is not (0.44.0: `run` prints only its usage error, and `outline` outlines the working directory). `docs_reads_fixtures.json` holds 25 reads and 18 non-reads.

## Solvable, and the gaps are real: `ref.py`
`python3 -I ref.py [SEEDS]` needs `ast-grep` and runs outside `./verify.sh`. On each fresh instance it runs one reference solution per family and one naive try that walks into the family's gap, and grades both against the truth.
- The references only show the tasks can be answered; they never feed the truth.
- With ast-grep 0.44.0 on 20 seeds:
  - every reference is right on every seed;
  - every naive try is wrong on every seed, except h2's text search, which is right whenever the name really is defined (11 of 20).
- One more gap it found: `export { $$$S } from $P` doesn't parse as a pattern ("Multiple AST nodes"). The barrel is read by `kind: export_statement`.

## Running a pass
`make_scenario.py` writes four scenarios from `tasks.py`, one pass each: `tutorial.jsonl` and `held-out.jsonl`, bare, and their `-skills` twins, the official skills' condition. `--check` says when one is stale.
- **A pass is one session on a fresh instance.** The `fixture` service (`services/fixture.py`) writes a `gen.py` instance into the run's worktree before serve starts, and its truth into the service's own directory, where the model can't read it.
  - Its seed is `$DIET_AST_GREP_SEED`, else drawn fresh.
  - The asks fill each instance's names from it (`{service.fixture.target}`).
- **Each task is asked once and graded once** (`attempts` 1), on its `answer` call (`check.py`), with no feedback inside the pass. The header's `answer_tool` hands serve `answer-tool.json`.
- **The bare regimen** (`bare.toml`) runs bash and the answer tool, with no memory levers. `canned.toml` is its rehearsal twin.
  - Its `max_output_tokens`, 8192 per response, is a DRAFT: the cap isn't ruled.
- **The official skills** (github.com/ast-grep/agent-skill) carry no license, so they are never in this tree. The `skills` service fetches the repository's archive at a pinned commit and checks each file against its digest. It then writes the files under the worktree's `.agent-skills/`, where `head-skills.md` points.
- **`pass.py RUN_DIR [--skills]`** gives the pass's verdict, from the log:
  - each task's grade;
  - each bash call's docs reads (`docs_reads.py`), with `.agent-skills` under the call's working directory as the skills' root under `--skills`.

  A pass is clean when every task is right and nothing read the docs.
  - Each task also reports `tool_used`, which is descriptive only: whether a clean pass must use ast-grep isn't ruled.
    - `ast-grep` when its calls searched with `run`, `outline` or `scan` (an option-led call runs `run`);
    - `other` when they ran only something else (rg, grep, a script; help alone counts here);
    - `none` when they ran nothing. The tutorial ends on a clean pass and rolls back from any other.
- **The canned replies** are a real ast-grep command and then an answer call, for the instance at `CANNED_SEED` (7). The first task's answer is wrong on purpose, the return-type miss. So `DIET_AST_GREP_SEED=7 diet-drive scenario tutorial.jsonl --out DIR` grades both ways: t1 wrong, t2–t6 right, and the pass unclean on t1. The skills and held-out scenarios do the same.

## Checks
`gen.py --selftest`, `check.py --selftest`, `docs_reads.py --selftest`, `make_scenario.py --check` and `pass.py --selftest` run in `./verify.sh`'s scenarios check, with no ast-grep needed. Each was seen to go red on a mutant:
- the generator, its line numbers off by one;
- the grader, a refused call counted;
- the classifier, its option values read as flags.
