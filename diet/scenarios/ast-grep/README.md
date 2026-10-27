# The ast-grep tutorial (#971)

The first slice of #971: ast-grep (`run`, `outline` and `scan`) on small TypeScript projects. Following the maintainer's ruling, "The pass" (2026-10-26):
- **The model is told.** It's a technical demonstration of subject mastery: nothing runs, and only the answer's structure is checked.
- **Each operation is demonstrated once, in isolation.** Help and docs are allowed.
- **The answer is a command.** A right answer reached any other way fails.
- **A notes update** follows each demonstration.
- **No hard cap on output.** Length is graded instead.

**Everything here is DRAFT and provisional.** In the maintainer's words, building it is "an experiment in itself: strong convictions, weakly held". Model-facing text is his: every ask, both tools' descriptions and results, the heads, and the notes prompt and heading each say `DRAFT.` until he edits them.

## Fixtures: `gen.py`
`python3 -I gen.py SEED OUT TRUTH` writes one instance: a project under `OUT` (`package.json`, `src/` in three directories, a barrel `src/index.ts`), and its truth in `TRUTH`.
- **Every pass gets a fresh instance.** The same families, with a new layout, new names and new values.
- **The truth is, per family, the lines a right command matches** (`path:line`, lines from 1). It comes from the generator's own construction: it records what it wrote where, never what ast-grep says.
- **Every family but the two misses matches something.** The misses, `zero-calls` and `maybe-defined`, may match nothing, on purpose.

## Tasks: `tasks.py`
Twelve operations: six tutorial and six held-out. Each asks for the command that matches a family's lines "and nothing else". A held-out task tests the same gap on another surface:

| gap | tutorial | held-out |
|---|---|---|
| a function pattern needs its return type, `: $RET` | t1 `function` declarations returning a type | h1 arrow constants returning a type |
| a miss matches nothing: `run` exits 1, `outline` prints `nothing found` and exits 0 | t2 direct calls to a function that is often never called | h2 a name's definition, often only in a comment and a string |
| a directory's exports are not the barrel's public surface | t3 the barrel's exports | h3 the barrel's type-only exports |
| `has` and `inside` look one level down unless `stopBy: end` | t4 functions with an `await` at any depth | h4 calls inside a `try` at any depth |
| `$name` in double quotes is the shell's; lowercase `$x` is literal | t5 calls to `$emit(...)` | h5 calls to `$track(...)` |
| `outline --match` is a regex to anchor, and on a directory outline lists only exports unless `--items structure` | t6 one function's outline | h6 one interface's outline |

## The answer is a command: `answer-tool.json`, `command.py` and `check.py`
- **The answer** comes through the `answer` tool's schema (`command`, a string), never parsed from prose.
- **`check.py`** grades the turn's last well-formed answer:
  - it runs the command out of the model's sight against a pristine copy of the instance (the fixture service's `pristine`, never the worktree, which the model could change);
  - it's right when the matched lines are exactly the truth's, so equivalent spellings pass;
  - `got` reports counts only, never what the command printed;
  - commands run on macOS only, where the confinement is built.
- **`command.py`** stands between the model's text and anything that runs:
  - **Parsed with `shlex`, never a shell.** Refused: any operator (`;`, `|`, `&`, a redirection, a subshell, `$(...)`), a backtick, a `$` inside double quotes (a shell expands it, so the command is wrong as typed), a variable assignment, and a parse error.
  - **ast-grep only.** The first word must be `ast-grep` or `sg`, by name. It runs the `ast-grep` on PATH, never `sg`, which is `newgrp` on Linux.
  - **Searches only.** The subcommand is `run` (or none), `outline` or `scan`. Refused: `-U`/`--update-all`, `-i`/`--interactive`, config and rule files (`-c`, `--config`, `-r`, `--rule`; a rule goes in `--inline-rules`), and absolute, `~` or `..` paths.
  - **Run confined.** No shell; a bare environment; a 20 s timeout; a 4 MB output cap; `--json=stream` in place of any `--json` it gave; and on macOS, Seatbelt with no network, no writes, and no reads under the home directory except the copy.
  - Every refusal is a red test (30), beside 10 searches that must be allowed. The confinement is tested on the profile itself, and goes red without it.
- **`ref.py`** (needs ast-grep and macOS; not in `./verify.sh`) grades one reference and one naive command per family through the same path, on 20 fresh seeds. Every reference is right on every seed, and every naive command is wrong on at least one. The references are also the canned answers. It found two gotchas:
  - `export { $$$S } from $P` doesn't parse ("Multiple AST nodes"), but `from $$$P` does;
  - `outline` on a directory lists only exports, so a non-exported function needs `--items structure`.

## Reading the docs: `docs_reads.py`
`reads_of(cmd, skills, installed=...)` parses one bash call's docs reads:
- `help`: `--help`, `-h`, `help`, or the tool run bare;
- `man`: man, tldr or cheat of the tool;
- `web`: a fetch of the site or the repository;
- `installed`: a read under the tool's install prefix, but the binary;
- `skill`: a read under the official skills' root.

`docs_reads_fixtures.json` holds 27 reads and 20 non-reads.

## A pass: `make_scenario.py`, `pass.py` and `run_pass.py`
- **One operation per fresh instance** is Dispatch's reading of "in isolation", unconfirmed. `make_scenario.py` writes `passes/<task>.jsonl`, and `passes/<task>-skills.jsonl` for the official skills' condition, from `tasks.py`; `--check` runs in verify. Each pass is one session:
  1. the task, asked once and graded once, with no feedback;
  2. then a notes update through the `notes` tool (`notes-tool.json`). The prompt is a DRAFT stub, and Dispatch will supply experimental ones.
- **Two supplied tools.** The header's `answer_tool` lists both (#971's engine PR).
- **The answer key is denied.** The fixture's directory is the header's `sandbox_secrets`, and `probe_key.py` proves the model can't read it (macOS only; off macOS it skips and says so).
- **The skills' condition.** `services/skills.py` fetches ast-grep/agent-skill at a pinned commit and checks its digests. It has no license, so it is never in this tree.
- **Regimens.**
  - `bare.toml` runs the production GPU host's registry entry; `bare-floor.toml` runs the floor's 27B line. Both are bash plus the two tools, with no memory levers.
  - Output has no hard cap, as ruled. `max_output_tokens` is the largest job each pool admits (cache_size less two 2048-token chunks: 520192 and 159744), and the drive cuts each request to what is left of the pool (#826). This value is DRAFT.
  - `canned.toml` is the rehearsal twin.
- **`pass.py RUN_DIR [--skills] [--docs-read par|fail]`**, per task:
  - `right`;
  - the par instruments, recorded only: `seconds` from the ask to the answer, `tool_calls`, `completion_tokens_before_answer` and `reasoning_chars_before_answer`;
  - `output_tokens`, and `capped` (responses cut at their cap);
  - `docs_reads`, `commands` and `tool_used` (descriptive);
  - then the pass's `notes`.

  A pass is clean when every task is right and none was capped. A docs read costs against par by default, reported and never failing; that's Dispatch's reading, unconfirmed. `--docs-read fail` makes one fail.
- **`run_pass.py TASK --out DIR [--skills] [--notes FILE] [--floor] [-- DRIVE_ARGS]`** writes a self-contained copy of the pass, its head carrying the notes so far, and runs it. It writes `pass.json` and `notes.md`, the next pass's `--notes`. Rolling across passes, re-planning after ten failures over par, and choosing note writers are Dispatch's experiments.
- **Canned:** `DIET_AST_GREP_SEED=7 python3 -I run_pass.py t1-ret-decl --out DIR` rehearses on the canned substrate. t1's canned answer is the naive command, so it grades wrong; every other task's grades right.

## Declared limits
- **macOS only.** The regimens read all (`sandbox_reads = "all"`), which the drive builds for macOS alone, and the check confines commands there only.
- **Other readable copies of ast-grep's docs.** The classifier counts the ones it knows. The live deny list covers the agents' plugin caches (`~/.claude`). Any other copy, such as a package cache, is neither denied nor counted.
- **Which model, the note writers, and the par values** are the maintainer's and Dispatch's.

## Checks
In `./verify.sh`'s scenarios check:
- `gen.py --selftest`;
- `check.py --selftest`, whose command cases run on macOS only;
- `docs_reads.py --selftest`;
- `command.py --selftest`;
- `make_scenario.py --check`;
- `pass.py --selftest`;
- `probe_key.py --selftest`.
