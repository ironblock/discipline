# The ast-grep tutorial (#971)

The first slice of #971: ast-grep (`run`, `outline` and `scan`) on small TypeScript projects. Following the maintainer's ruling, "The pass" (2026-10-26):
- **The model is told.** It's a technical demonstration of subject mastery: nothing runs, and only the answer's structure is checked.
- **The command is never run** (2026-10-27). It's submitted through its own tool, `submit_answer`, and graded statically against the forms known to be right.
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

## The answer is a command, never run: `answer-tool.json`, `command.py`, `rubric.py` and `check.py`
- **The answer** comes through `submit_answer`'s schema (`command`, a string), never parsed from prose. It's a tool of its own, so the model can see it isn't bash.
- **`check.py`** judges the turn's last well-formed answer statically (`rubric.py`), with the instance's names.
  - It's right only when the rubric says right. `unrecognized` and a refusal are wrong, and `got` says which and why.
  - It runs nothing, so it works off macOS too.
- **`rubric.py`** reads the command with `command.py`'s `parse`: the subcommand, options by long name with their values, and normalised paths. Then it applies a rubric per operation:
  - **Patterns** are compared with metavariable names and whitespace dropped, so `$F`/`$NAME`/`$_` and `$$$A`/`$$$` are the same, and a reflow is the same. A literal `$emit` stays literal.
  - **`--inline-rules`** is read through `yamlish.py` (block and flow YAML, the subset rules use; anything else is unrecognized). The rubric checks `kind`, `pattern`, `regex`, `field`, and that a relation which must reach any depth says `stopBy: end`.
  - **Paths:** the whole project is `src`, `.` or none; the barrel is `src/index.ts` alone. `--lang`, if given, is TypeScript.
  - **"As specific as necessary":** a form that also matches what the task excludes is wrong. Examples: an outline `--match` that isn't `^NAME$` (it matches names, signatures and first lines); an arrow rule without its `const`; `--items` left at exports for a function that isn't exported.
  - **Selftest:** 30 right spellings (references and equivalents: flag order, long and short, `sg`, other metavariable names, other quoting, flow-style YAML, field-based rules) and 32 wrong ones. `ref.py` adds 20 seeds: every reference is right, and no naive command is.
- **Operations whose gap the command can't show** (`tasks.py`'s `INVISIBLE`): the miss. A right `Z($$$)` reads the same whether or not the model knows a miss prints nothing or `nothing found`. Each carries a reshaped DRAFT ask, unused until the maintainer rules.
- **`audit.py --execute PASS_DIR...`** is the offline audit (ast-grep only, macOS only, never the verdict). It runs each pass's answer confined against its pristine instance, compares the result with the static verdict, and prints `disagree` where they differ. It isn't a general mechanism: a tool with destructive actions gets no execution audit.
  - On the first 24 live answers (the Flash-Next instance) it found 4 disagreements:
    - three unanchored outline `--match` answers that execution passed, because their fixtures had no near-name;
    - one type-only-export rule built from `all`/`any`/`not`, which the rubric doesn't know.
- **`command.py`** stands between the model's text and anything that runs:
  - **Parsed with `shlex`, never a shell.** Refused: any operator (`;`, `|`, `&`, a redirection, a subshell, `$(...)`), a backtick, a `$` inside double quotes (a shell expands it, so the command is wrong as typed), a variable assignment, and a parse error.
  - **ast-grep only.** The first word must be `ast-grep` or `sg`, by name. It runs the `ast-grep` on PATH, never `sg`, which is `newgrp` on Linux.
  - **Searches only.** The subcommand is `run` (or none), `outline` or `scan`, and every option one the filter knows: ast-grep 0.44.0's, per subcommand.
    - Options are read as clap reads them: short flags cluster (`-Uj1` is `-U -j 1`); a valued short flag takes the rest of its token or the next word; an optional value (`--debug-query`) is only its `=` form.
    - Refused: `-U`/`--update-all`; `-i`/`--interactive`; config and rule files (`-c`, `--config`, scan's `-r`, `--rule`, `--outline-rules`; a rule goes in `--inline-rules`); an option the filter doesn't know; and absolute, `~` or `..` paths, after `--` too.
    - run's `-r` is `--rewrite`, which only prints.
  - **Run confined.** No shell; a bare environment; a 20 s timeout; a 4 MB output cap; `--json=stream` in place of any `--json` it gave; and on macOS, Seatbelt with no network, no writes, and no reads under the home directory except the copy.
  - Every refusal is a red test (39), beside 16 searches that must be allowed. In double quotes, `\$` is read as bash reads it, as `$`.
  - The confined execution below is now the audit's, never the verdict's. The confinement is tested on the profile itself, and goes red without it.
- **`ref.py`** (needs ast-grep and macOS; not in `./verify.sh`) grades one reference and one naive command per family, statically and by execution, on 20 fresh seeds. Statically, every reference is right and no naive command is. By execution, the naive outlines pass where a fixture has no near-name. The references are also the canned answers. It found two gotchas:
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
  - **A shared prefix:** `head_nonce = "off"` (the maintainer, 2026-10-27: "the shared prefix is the important bit"). Passes with the same head and notes share a cached prefix, and a new notes version is a new prefix by its own text.
- **`pass.py RUN_DIR [--skills] [--docs-read par|fail]`**, per task:
  - `right`;
  - the par instruments, recorded only: `seconds` from the ask to the answer, `tool_calls`, `completion_tokens_before_answer` and `reasoning_chars_before_answer`;
  - `output_tokens`, and `capped` (responses cut at their cap);
  - `trial_searches`: ast-grep searches against the project before the answer, recorded only (whether trial is allowed now is the maintainer's open question);
  - `requests`: each response's `prompt_tokens` and `cached_tokens`, since cache state moves the seconds;
  - `docs_reads`, `commands` and `tool_used` (descriptive);
  - then the pass's `notes`.

  A pass is clean when every task is right and none was capped. A docs read costs against par, reported and never failing: confirmed by the maintainer on 2026-10-27. `--docs-read fail` remains as a switch.
- **`run_pass.py TASK --out DIR [--skills] [--notes FILE] [--floor] [-- DRIVE_ARGS]`** writes a self-contained copy of the pass, its head carrying the notes so far, and runs it from the tree, since the drive finds its assembler by a relative path. It writes `pass.json` and `notes.md`, the next pass's `--notes`. Rolling across passes, re-planning after ten failures over par, and choosing note writers are Dispatch's experiments.
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
- `yamlish.py`;
- `rubric.py`;
- `make_scenario.py --check`;
- `pass.py --selftest`;
- `probe_key.py --selftest`.
