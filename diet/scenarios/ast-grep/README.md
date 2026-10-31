# The ast-grep tutorial (#971)

The first slice of #971: ast-grep (`run`, `outline` and `scan`) on small TypeScript projects. What stands here is the instrument of one pre-registered falsification: do tailored notes beat native ast-grep use on cost, at equal correctness?
- **Native use.** The model works in a terminal where `ast-grep` is the real binary and searches run. Only the fixture's truth is denied to it.
- **The answer is a command,** submitted through its own tool, `submit_answer`, and **run** to grade it: right only when it matches exactly the lines the task asks for.
- **"No match" is part of mastery** (2026-10-27). t2 and h2 also say, in structured fields, what the command does when nothing matches.
- **One operation per fresh instance.** Each task is asked once and graded once, with no feedback.
- **No hard cap on output.**

**The back-out (2026-10-30).** #971's disposition, after an adversarial review of the earlier design, removed from this tree the static rubric, the notes loop and its writer table, the gap-filling round, the search shim and its "no trial" rule, task set 2, and the heads and answer-tool text that framed a pass as a demonstration that runs nothing. That review is the errata for the earlier sections of this README and for the results produced under them. What stayed: the answer tool, secrets v2, the generated fixtures with their decoys, execution grading, the run nonces, and the falsification's harness.

**Everything here is DRAFT and provisional.** In the maintainer's words, building it is "an experiment in itself: strong convictions, weakly held". Model-facing text is his: every ask, both tools' descriptions and results, the heads, and the notes heading each say `DRAFT.` until he edits them.

## Fixtures: `gen.py` and `gen2.py`
`python3 -I gen.py SEED OUT TRUTH` writes one instance: a project under `OUT` (`package.json`, `src/` in three directories, a barrel `src/index.ts`), and its truth in `TRUTH`.
- **Every pass gets a fresh instance.** The same families, with a new layout, new names and new values. A seed resolves to the first usable instance at or after it.
- **The truth is, per family, the lines a right command matches** (`path:line`, lines from 1). It comes from the generator's own construction: it records what it wrote where, never what ast-grep says.
- **Every family but the misses matches something.** The misses may match nothing, on purpose.
- **Distractors** (2026-10-27), so execution separates a right form from an over-broad one. A wrong form that catches them executes wrong, rather than passing because the fixture has nothing for it to catch:
  - a near-name beside each outlined item (`<name>View`), which an unanchored `--match` also catches, and a suffix near-name (`pre<name>`, `Legacy<Name>`), which a `--match` anchored at one end only also catches;
  - a second call inside every `try`, which "any call in a try" also catches;
  - a `let` arrow returning the arrow type, which a rule without its `const` also catches;
  - the maybe-defined name, when it's defined, as one of four kinds (type alias, interface, function, class), so a `--type` filter the ask doesn't name also goes wrong;
  - `Promise<T>` returns beside `T`;
  - in their families' truth: a log call with two arguments inside a `try`, which `log($A)` misses; a barrel line exporting two types at once, which `export type { $X }` misses; and a function and a `const` arrow of the target types with two parameters, which `($A)` misses.
- **Decoy families** (`gen2.py`). `gen.build` adds more content after the task families' draws, with families of its own that no task asks for: more for a task's command to wrongly catch. One of its barrel exports is in t3's public-surface truth. `gen.usable` requires its families too, so the instances, and the seed each resolves to (105 to 106), are the ones the falsification runs on.

## Tasks: `tasks.py`
Twelve operations: six tutorial (t1..t6) and six held-out (h1..h6). Each asks for the command that matches a family's lines "and nothing else". A held-out task tests the same gap on another surface.

| gap | tutorial | held-out |
|---|---|---|
| a function pattern needs its return type, `: $RET` | t1 `function` declarations returning a type | h1 arrow constants returning a type |
| a miss matches nothing: `run` exits 1, `outline` prints `nothing found` and exits 0 | t2 direct calls to a function that is often never called | h2 a name's definition, often only in a comment and a string |
| a directory's exports are not the barrel's public surface | t3 the barrel's exports | h3 the barrel's type-only exports |
| `has` and `inside` look one level down unless `stopBy: end` | t4 functions with an `await` at any depth | h4 calls inside a `try` at any depth |
| `$name` in double quotes is the shell's; lowercase `$x` is literal | t5 calls to `$emit(...)` | h5 calls to `$track(...)` |
| `outline --match` is a regex to anchor, and on a directory outline lists only exports unless `--items structure` | t6 one function's outline | h6 one interface's outline |

## The answer, run: `answer-tool-native.json`, `command.py` and `check.py`
- **The answer** comes through `submit_answer`'s schema (`command`, a string), never parsed from prose. It's a tool of its own, so the model can see it isn't bash.
- **`check.py`** takes the turn's last well-formed answer and runs it, confined, on the pass's pristine copy of the instance. It's right only when the matched lines are exactly the truth's. A command `command.py` refuses is not right, and an answer in prose or through another tool is no answer.
  - Execution is the only verdict: `$DIET_AST_GREP_VERDICT`, when set, must be `execute`, or the check exits non-zero.
  - It also counts the ast-grep searches before the answer (`searches`), never help or docs reads. A count, never a fail.
  - Its selftest stubs the execution, so it needs no ast-grep. Each case was checked against a mutant of the check.
- **The miss** (t2, h2) answers through `answer-tool-miss-native.json`, the same `submit_answer` with `exit_status_when_none` (an integer) and `prints_nothing_when_none` (a boolean), all required.
  - It's right only when the command executes right and both fields match `miss_truth.json` for its subcommand.
  - That truth is measured per ast-grep version. On 0.44.0: `run` prints nothing and exits 1; `outline` prints `nothing found` and exits 0; `scan` prints nothing and exits 0. A version not measured there is not right.
- **`command.py`** stands between the model's text and anything that runs:
  - **Parsed with `shlex`, never a shell.** Refused: any operator (`;`, `|`, `&`, a redirection, a subshell, `$(...)`), a backtick, a `$` inside double quotes (a shell expands it, so the command is wrong as typed), a variable assignment, and a parse error.
  - **ast-grep only.** The first word must be `ast-grep` or `sg`, by name. It runs the `ast-grep` on PATH, never `sg`, which is `newgrp` on Linux.
  - **Searches only.** The subcommand is `run` (or none), `outline` or `scan`, and every option one the filter knows: ast-grep 0.44.0's, per subcommand.
    - Options are read as clap reads them: short flags cluster (`-Uj1` is `-U -j 1`); a valued short flag takes the rest of its token or the next word; an optional value (`--debug-query`) is only its `=` form.
    - Refused: `-U`/`--update-all`; `-i`/`--interactive`; config and rule files (`-c`, `--config`, scan's `-r`, `--rule`, `--outline-rules`; a rule goes in `--inline-rules`); an option the filter doesn't know; and absolute, `~` or `..` paths, after `--` too.
    - run's `-r` is `--rewrite`, which only prints.
  - **Run confined.** No shell; a bare environment; a 20 s timeout; a 4 MB output cap; the answer's own output format dropped (`--json`, `--report-style`, `--color`, `--heading`, `--format`) for `--json=stream`; and on macOS, Seatbelt with no network, no writes, and no reads under the home directory except the copy. The confinement is tested on the profile itself, and goes red without it.
  - Every refusal is a red test, beside searches that must be allowed. In double quotes, `\$` is read as bash reads it, as `$`.
- **`ref.py`** (needs ast-grep and macOS; not in `./verify.sh`) runs, on 20 fresh seeds, one reference and one naive command per task family, an over-broad command for six families, and a kind filter for the maybe-defined one.
  - Every reference must execute right on every seed, every naive command wrong on at least one, every over-broad command wrong on every seed, and the kind filter wrong on at least one.
  - The references are also the canned answers.
  - Two gotchas it found: `export { $$$S } from $P` doesn't parse under `-l ts`, while `export type { $$$S } from $P` matches; and `outline` on a directory lists only exports, so a non-exported function needs `--items structure`.

## Reading the docs: `docs_reads.py`
`reads_of(cmd, skills, installed=...)` parses one bash call's docs reads:
- `help`: `--help`, `-h`, `help`, or the tool run bare;
- `man`: man, tldr or cheat of the tool;
- `web`: a fetch of the site or the repository;
- `installed`: a read under the tool's install prefix, the binary included (`strings $(which ast-grep)` reads its embedded rules); listing it is no read;
- `skill`: a read under the official skills' root.

`docs_reads_fixtures.json` holds the reads and non-reads its selftest checks.

## A pass: `make_scenario.py`, `pass.py` and `run_pass.py`
- **`make_scenario.py`** writes `passes/<task>.jsonl`, and `passes/<task>-skills.jsonl` for the official skills' condition, from `tasks.py`; `--check` runs in verify. Each pass is one session, the task asked once and graded once, with no feedback. The header holds the native condition: `head-native.md` (or `head-native-skills.md`), the native answer tool, the `fixture` service (and `skills`), and the fixture's directory as the one secret.
- **The answer key is denied** (secrets v2: no read, no exec). `probe_key.py` proves the model can't read it, on the real drive and sandbox (macOS only; off macOS it skips and says so).
- **The skills' condition.** `services/skills.py` fetches ast-grep/agent-skill at a pinned commit and checks its digests. It has no license, so it is never in this tree.
- **Regimens.**
  - `bare.toml` runs the production GPU host's registry entry; `bare-floor.toml` runs the floor's 27B line. Both are bash plus the answer tool, with no memory levers.
  - Output has no hard cap, as ruled. `max_output_tokens` is the largest job each pool admits, and the drive cuts each request to what is left of the pool (#826). This value is DRAFT.
  - `canned.toml` is the rehearsal twin.
  - **A shared prefix:** `head_nonce = "off"` (the maintainer, 2026-10-27: "the shared prefix is the important bit"). Passes with the same head share a cached prefix.
- **`pass.py RUN_DIR [--skills]`**, per task:
  - `right`;
  - the par instruments, recorded only: `seconds` from the ask to the answer, `tool_calls`, `completion_tokens_before_answer` and `reasoning_chars_before_answer`;
  - `output_tokens`, and `capped` (responses cut at their cap);
  - `searches`: ast-grep searches before the answer, apart from help and docs reads;
  - `requests`: each response's `prompt_tokens` and `cached_tokens`, since cache state moves the seconds;
  - `docs_reads`, `commands` and `tool_used` (descriptive).

  A pass is clean when every task is right and none was capped. Searches and docs reads are recorded, never a fail. A step waits up to 3600 s (`patience`), as long as the 60-minute guard, never the drive's 900 s default.
- **`run_pass.py TASK --out DIR [--skills] [--notes FILE] [--floor] [--head FILE] [--seed N] [--verdict execute] [-- DRIVE_ARGS]`** writes a self-contained copy of the pass, its head the given head with FILE's notes under the notes heading, and runs it from the tree, since the drive finds its assembler by a relative path. It writes `pass.json`.
  - The notes heading, "DRAFT. Your notes on ast-grep, from earlier demonstrations:", is the falsification's T arm's model-facing text, kept verbatim while that run is under way; its wording is the maintainer's to change at the re-plan.
- **Canned:** `DIET_AST_GREP_SEED=7 python3 -I run_pass.py t1-ret-decl --out DIR` rehearses on the canned substrate. t1's canned answer is the naive command, so it grades wrong; every other task's grades right.

## The falsification: `falsify.py` and `analyze.py`
The maintainer's ruling after the adversarial review (#971, 2026-10-29): one pre-registered falsification asks whether tailored notes beat native ast-grep use on cost, at equal correctness. The pre-registration and its amendments are private run data, frozen before any result was read, and their sha256 goes in the run's manifest, with the build commit the run is reproduced from. DRAFT.
- **Heads** (experiment text): `head-native.md` (a terminal, a TypeScript project, any commands including ast-grep, one ast-grep command through submit_answer); `head-native-skills.md` adds the skills pointer; the T arm is the N head with the notes under the notes heading.
- **An explicit seed** (`run_pass.py --seed`), so the arms are paired: 101..104 and 106..111 (seed 105 resolves to 106).
- **`falsify.py`**: the 12 tasks x 10 seeds x 3 arms (N, S, T), the arms' order shuffled per (task, seed) by one `random.Random(29)`, the idle gate and the 60-minute stop; a void or stopped pass is re-run once on the same seed, then recorded void, not right. Every pass has a cost: before the answer when there is one, else to the end of its task turn (a stopped pass's seconds at least its wall time). `rows.jsonl`, `manifest.json`, resumable.
- **`analyze.py`**: per arm, executed-right and the medians over all 120 passes; paired per-(task, seed) T-N and S-N differences with a 10,000-resample bootstrap at seed 29; a per-task table; the decision, T against N alone: the notes win only if T is at most one executed-right below N and T's median tool calls or seconds is at least 25% below N's. A row without a cost is an error, never a free pass. RED-tested on synthetic rows.

## Declared limits
- **macOS only.** The regimens read all (`sandbox_reads = "all"`), which the drive builds for macOS alone, and the check confines commands there only.
- **Other readable copies of ast-grep's docs.** The classifier counts the ones it knows. The live deny list covers the agents' plugin caches (`~/.claude`). Any other copy, such as a package cache, is neither denied nor counted.
- **Which model, the notes, and the par values** are the maintainer's and Dispatch's.

## Checks
In `./verify.sh`'s scenarios check:
- `gen.py --selftest`;
- `check.py --selftest`;
- `docs_reads.py --selftest`;
- `command.py --selftest`, whose execution cases run on macOS only;
- `make_scenario.py --check`;
- `pass.py --selftest`;
- `run_pass.py --selftest`;
- `falsify.py --selftest` and `analyze.py --selftest`;
- `probe_key.py --selftest`.
