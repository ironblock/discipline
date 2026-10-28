# The ast-grep tutorial (#971)

The first slice of #971: ast-grep (`run`, `outline` and `scan`) on small TypeScript projects. Following the maintainer's ruling, "The pass" (2026-10-26):
- **The model is told.** It's a technical demonstration of subject mastery: nothing runs, and only the answer's structure is checked.
- **The command is never run** (2026-10-27). It's submitted through its own tool, `submit_answer`, and graded statically against the forms known to be right.
- **No trial searches** (2026-10-27). Help and docs may be read, but an ast-grep search before the answer, against the project or a file the model made, fails the attempt. The DRAFT heads state this as user intent.
- **"No match" is part of mastery** (2026-10-27). t2 and h2 also say, in structured fields, what the command does when nothing matches.
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
- **Distractors** (2026-10-27), so execution separates a right form from an over-broad one. A wrong form that catches them executes wrong, rather than passing because the fixture has nothing for it to catch. None of them is in any family's truth:
  - a near-name beside each outlined item (`<name>View`), which an unanchored `--match` also catches, and a suffix near-name (`pre<name>`, `Legacy<Name>`), which a `--match` anchored at one end only also catches (round 4);
  - a second call inside every `try`, which "any call in a try" also catches;
  - a `let` arrow returning the arrow type, which a rule without its `const` also catches;
  - the maybe-defined name, when it's defined, as one of four kinds (type alias, interface, function, class), so a `--type` filter the ask doesn't name also goes wrong (rubric review, 2026-10-27);
  - `Promise<T>` returns beside `T`, already present;
  - round 5, in their families' truth: a log call with two arguments inside a `try`, which `log($A)` misses; a barrel line exporting two types at once, `export type { A, B }`, which `export type { $X }` misses; and a function and a `const` arrow of the target types with two parameters, which `($A)` misses. Before them, all three forms executed right on all 20 seeds, since every such call, export and arrow had one.

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
  - **"As specific as necessary":** a form that also matches what the task excludes is wrong. Examples: an outline `--match` that isn't `^NAME$` (it matches names, signatures and first lines; ruled wrong by the maintainer, 2026-10-27); an arrow rule without its `const`; `--items` left at exports for a function that isn't exported.
  - **Selftest:** 47 right spellings (references and equivalents: flag order, long and short, `sg`, other metavariable names, other quoting, flow-style YAML, field-based rules, the `=> $$$` body, `from $P`, a capitalised `--lang`) and 46 not right (36 wrong with a named reason, 10 unrecognized), the floor's over-broad answers among them, and 12 named wrongs whose reasons must say why. `ref.py` adds 20 seeds: every reference is right, and no naive command is.
- **The miss** (t2, h2) answers through `answer-tool-miss.json`, the same `submit_answer` with `exit_status_when_none` (an integer) and `prints_nothing_when_none` (a boolean), all required.
  - It's right only when the command passes the rubric and both fields match `miss_truth.json` for its subcommand.
  - That truth is measured per ast-grep version. On 0.44.0: `run` prints nothing and exits 1; `outline` prints `nothing found` and exits 0; `scan` prints nothing and exits 0.
  - A version not measured there is unrecognized. `tasks.py`'s `INVISIBLE` keeps the record of why: a command alone can't show the lesson.
- **No trial searches:** `check.py` counts ast-grep searches among the calls before the answer, never help or docs reads, and a count above 0 fails the attempt. `$DIET_AST_GREP_TRIAL=par` records them without failing.
- **Rubric review, 2026-10-27 (Dispatch; provisional, reported to the maintainer).** Each form below went in RED first, and each executes right on all 20 seeds with distractors (`ref.py`'s alternatives):
  - **Return types:** an anchored regex on the annotation itself, `has: {field: return_type, kind: type_annotation, regex: '^:\s*T$'}`, for functions and, within a `const`, for arrows. Named wrongs: an unanchored annotation regex, and a type identifier at any depth (`stopBy: end`), which also matches `Promise<T>`.
  - **Public surface:** `run -k export_statement src/index.ts`, every export statement of the entry point.
  - **Log in try:** `all: [pattern, inside: {kind: try_statement, stopBy: end}]`, in either order. Without `stopBy` it's wrong.
  - **Rejected:** a `--type` filter on a kind the ask doesn't name. It was right only because the generator always made a type alias; now `ref.py`'s kind filter executes wrong on 7 of 20 seeds.
- **Round 4 (Dispatch, 2026-10-28; provisional).** Each went in RED first and was executed on all 20 seeds:
  - **Named wrongs:** "`export $$$` doesn't parse as a pattern", and "a function pattern without its return type matches no typed function", also inside an `all:` rule. Each executes wrong on 20 of 20 seeds.
  - **Learned:** h3's escaped `"export type { \$\$\$ } from '\$PATH';"`, with its trailing `;`, and `inside: {pattern: "try { $$$ }", stopBy: end}`. Each executes right on all 20.
  - **A `--match` anchored at one end only is wrong**, by the maintainer's unanchored ruling. The suffix near-names make it execute wrong on all 20 seeds; before them it executed right on all 20.
  - **t6 holds the ask's word:** it asks for an outline.
  - **Every wrong's reason is model-facing now** (W1r, below), so it states an ast-grep fact or the ask's terms, never the fix. `rubric.py`'s `LEAKS` test fails if any reason names one (for example `--items structure`, or `stopBy: end` as the remedy).
- **Round 5 (Dispatch, 2026-10-28; provisional).** Each went in RED first and was executed on all 20 seeds (`ref.py`):
  - **Named wrongs, from the floor's unrecognized answers:** "`...` in a pattern is literal text: it matches only code that has `...` there" (t1's `(...$ARGS)`, t2's `name(...)`; executes wrong on 20 of 20 and, for the miss, on the 6 seeds that have calls); "`$A` in a parameter list matches exactly one parameter" (h1, and t1's declaration form; 20 of 20 each); "`$X` matches exactly one export specifier" (h3; 20 of 20); and h4's `log($A)` inside a `try` takes the one-argument reason (20 of 20). The selector forms (t5, h5) stay unrecognized.
  - **Learned, from the Flash-Next instance's answers:** t4's `has: {field: body, kind: statement_block, has: {kind: await_expression, stopBy: end}}` (graded wrong without its inner `stopBy`, as any `has` is), and h1's flow-style `all:` of the return type and the `const`. Each executes right on all 20.
- **Gap-fill coverage (Dispatch, 2026-10-28; provisional).** Both models' first gap-fill t4 answers were unrecognized and executed right, so the rubric learned them (RED first; each executes right on all 20 seeds):
  - an `any:` of bare kinds that includes `function_declaration` and adds at most `generator_function_declaration`: "declared with the `function` keyword" arguably covers `function*`, so either set is right. There is no generator distractor: it would flip every plain `function_declaration` answer, and that reading is the maintainer's.
  - `all:` of the kind and the `has`, which is the one rule: for t4, and for t1's return-type rule (the floor's gap-fill t1).
  - `run -k 'function_declaration:has(await_expression)'`: a `:has()` selector reaches any depth, as `has` with `stopBy: end` does. It had been graded "a relation needs a rule", a false wrong the audit caught (the floor's gap-fill t4). `:has(await $X)` matches nothing in 0.44.0, and a `:has` of another kind stays wrong.
  - **Named wrong:** "`function_expression` is a function written as a value, not a function declaration" (the floor's second t4). Its distractor is an async function expression with an `await`, outside the truth: the form executes wrong on 20 of 20 seeds, and before it executed right on all 20.
- **Unrecognized, never a false wrong:** a form the rubric doesn't know is `unrecognized`, never `wrong`, unless the rubric names why it's wrong (no return type, one argument, unanchored, a kind selector over every call in a try). `--lang` and a rule's `language` are case-insensitive.
- **`audit.py --execute PASS_DIR...`** is the offline audit (ast-grep only, macOS only, never the verdict). It runs each pass's answer, the task turn's last as `check.py` grades it (round 5; it had read the last in the whole log, so a notes-turn resubmission was audited in its place), confined against its pristine instance, compares the result with the static verdict, and prints `disagree` where they differ. It drops the answer's own output format (`--json`, `--report-style`, `--color`, `--heading`, `--format`) before asking for `--json=stream`, since ast-grep refuses `--report-style` beside `--json`. It isn't a general mechanism: a tool with destructive actions gets no execution audit.
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
- **`ref.py`** (needs ast-grep and macOS; not in `./verify.sh`) grades one reference and one naive command per family, statically and by execution, on 20 fresh seeds. It also runs an over-broad command for each of six families: the unanchored outline `--match` (three families), the kind selector over every call in a try, the const-less arrow rule, and a return-type rule that reaches into `Promise<T>`.
  - Every reference is right; no naive command is right statically; and with the distractors, every over-broad command executes wrong on every seed.
  - Before the distractors, three of them executed right on all 20 seeds and one on 10.
  - The references are also the canned answers. It found two gotchas:
  - `export { $$$S } from $P` doesn't parse under `-l ts` ("Multiple AST nodes"), and with the language taken from the file it parses but matches nothing. `export type { $$$S } from $P` matches either way, which the floor's answers showed (corrected 2026-10-27);
  - `outline` on a directory lists only exports, so a non-exported function needs `--items structure`.

## Reading the docs: `docs_reads.py`
`reads_of(cmd, skills, installed=...)` parses one bash call's docs reads:
- `help`: `--help`, `-h`, `help`, or the tool run bare;
- `man`: man, tldr or cheat of the tool;
- `web`: a fetch of the site or the repository;
- `installed`: a read under the tool's install prefix, the binary included (`strings $(which ast-grep)` reads its embedded rules); listing it is no read (rubric review, 2026-10-27);
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
  - `trial_searches`: ast-grep searches against the project before the answer. A count above 0 fails the attempt (ruled 2026-10-27; `--trial par` records them only);
  - `requests`: each response's `prompt_tokens` and `cached_tokens`, since cache state moves the seconds;
  - `docs_reads`, `commands` and `tool_used` (descriptive);
  - then the pass's `notes`.

  A pass is clean when every task is right, none was capped, and none ran a trial search. `--trial par` records trial searches without failing.
  - A docs read costs against par, reported and never failing (the maintainer, 2026-10-27). `--docs-read fail` remains as a switch.
  - A step waits up to 3600 s (`patience`), as long as the 60-minute guard, never the drive's 900 s default: two floor passes voided on it.
- **`run_pass.py TASK --out DIR [--skills] [--notes FILE] [--floor] [-- DRIVE_ARGS]`** writes a self-contained copy of the pass, its head carrying the notes so far, and runs it from the tree, since the drive finds its assembler by a relative path. It writes `pass.json` and `notes.md`, the next pass's `--notes`. Rolling across passes, re-planning after ten failures over par, and choosing note writers are Dispatch's experiments.
- **Canned:** `DIET_AST_GREP_SEED=7 python3 -I run_pass.py t1-ret-decl --out DIR` rehearses on the canned substrate. t1's canned answer is the naive command, so it grades wrong; every other task's grades right.

## The notes loop: `check.py`'s writer map and `loop.py`
The note-writer experiment (Dispatch, 2026-10-27). Its asks are the experiment's text, never the maintainer's wording.
- **The notes turn rides on the grade.** The check names the attempt's failure kind and prints, as `got`, the notes ask for that kind. The grade step sends it as its feedback (`right` and `last` are `{got}`), so the model is sent only the ask, never the verdict or its reason. That is the conditional step, chosen over a fork from carry, and it needs no engine change.
  - The asks come from a writer table, `$DIET_AST_GREP_WRITERS` (a JSON file kept out of this tree: `tail`, `W0`, `W0n`, `W1`, `W2`, `W3`, `W4`). Without it, every kind gets the DRAFT stub.
  - **The map:**
    - rubric → W1, refutation;
    - miss fields → W4: the command was right, but what it was said to print or exit with on a miss wasn't;
    - trial → W3: the command was right, but searches were run;
    - right with a docs read → W2, read-replacement;
    - right and clean → W0;
    - unrecognized and no answer → **W0n**, a neutral ask that never says the command was right or wrong;
    - unrecognized with trial searches → **W3n**, which names the searches and never says right or wrong;
    - a wrong command → W1, whatever its searches. When the rubric names a reason (round 4), W1 becomes **W1r**: "That command was graded wrong: <reason>." followed by the rest of W1. Never for unrecognized. W1 alone moved nothing: the 27B took it four times on t6.

    W0 opens "That command was right", which an unrecognized command must never be told; W1 opens "That command was graded wrong", which a right command with wrong miss fields must never be told.
- **`run_pass.py --writers FILE`** passes the table to the check. **`--no-notes`** takes a pass without its notes turn, as a held-out check.
- **pass.json gains, per task,** `failure` (the kind, re-derived with `check.grade` from the turn's own calls and the pass's truth) and `writer` (which ask followed the grade, by its text).
  - And `resubmitted` (Dispatch, 2026-10-28): `submit_answer` stays available in the notes turn, and a call there is recorded, with `resubmitted_command`, never graded. A descriptor: a sign the reason landed. `loop.jsonl` carries both.
- **`loop.py --out DIR [--writers FILE] [--skills] [--floor] [--attempts N] [--notes FILE] [--before CMD] [--max-seconds S] [-- DRIVE_ARGS]`:**
  - `--floor` takes every pass under the floor's regimen (`bare-floor.toml`), as `run_pass.py --floor` does. Without it, a floor endpoint would run under the production entry's regimen.
  - For t1..t6 in order, it runs a fresh instance with the current notes. An attempt that isn't clean is retried on the same operation, with the notes it left, up to N times (4). Then the loop moves on, carrying the notes.
  - An unrecognized attempt is never retried: its notes turn gives the model no signal (the first notes loop on the Flash-Next instance spent 8 attempts that way). It is recorded `queued_for_review`, for rubric review, and the loop moves on.
  - After t6, h1..h6 run once each with the final notes and no notes turns.
  - Per attempt, `DIR/loop.jsonl` records the operation, attempt, clean, failure kind, writer, and the notes' character lengths before and after.
  - `--before` runs the host's idle gate before each pass. `--max-seconds` (3600) stops a pass's process group and records it `stopped`, never graded.
- **Canned:** `DIET_AST_GREP_SEED=7`. t1's canned answer is wrong, so it takes 4 attempts with W1; t2–t6 are clean with W0; the held-out six run without notes turns. The writer's ask, tail included, is what the model receives, and the notes ride into the next pass's head.

## Gap-filling: `tutor_round.py`
The maintainer's change of direction (2026-10-28): the self-written notes loop is set aside; a stronger model, the tutor, edits a model's notes until its failures disappear. `tutor_round.py` is the instrument it reads. DRAFT.
- **`tutor_round.py --notes FILE --out DIR [--tasks ...] [--held-out] [--floor] [--before CMD] [--max-seconds S] [-- DRIVE_ARGS]`** runs each task once on a fresh instance with the notes FILE and no notes turn (`run_pass.py --no-notes`), under the loop's gates.
- **`DIR/rows.jsonl`**, per task: right, kind and the rubric's reason, the command, the trial searches and their commands, the docs read, and `pass.py`'s par fields.
- **`DIR/failures.md`**, the tutor's input, from the tutorial run (t1..t6 by default) only: per failed task, its ask, command, kind and reason, trial commands, and the last 1,500 characters of reasoning before the answer. A stopped or void pass is listed as not run, never as no failure. Private run data, never committed.
- **`--held-out`** runs h1..h6 and writes no failures.md: the tutor never sees a held-out failure. A task list that mixes the splits is refused.

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
- `loop.py --selftest` and `run_pass.py --selftest`;
- `audit.py --selftest`;
- `tutor_round.py --selftest`;
- `probe_key.py --selftest`.
