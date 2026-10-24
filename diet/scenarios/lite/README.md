# The Babylon Lite tasks (#932, #942)

Twenty tasks on Babylon Lite, a WebGPU engine the model has not seen: eight tutorial tasks, then twelve held-out tasks, pre-registered. Lite's docs contradict its source in places, and the tasks are built on those contradictions: the tutorial lets the model find each one, and the held-out tasks need what it found. The pilot asks whether interview forks carry what the tutorial taught across a refill better than the native harness does (#932).

Lite itself is not in this tree. It is the pinned reference checkout, `~/git/reference/Babylon/Babylon-Lite` at `a9cfda1b`, or `$LITE_REPO`, with its dependencies installed. `services/lite.py` refuses a run on any other commit or on a changed tracked file.

## Running it

```
diet-drive arms lite.jsonl --arms ARMS.toml --repeat N --out DIR
diet-drive table DIR
```
- **`lite.jsonl`:** t1–t8, a declared meditation, then h1–h12, in one session. Every arm crosses the refill. Golden, which never meditates, acknowledges it as skipped.
- **`ARMS.toml`:** the six arms, each contrasting with the native baseline:
  - `native`, the baseline;
  - `interviews`: the interview levers, and a rendered refill;
  - `golden`: compaction depth `none`;
  - `reference`: native whose held-out half runs in a session that never saw the tutorial (`refill_session = "blank"`), the floor;
  - `sealed-native` and `sealed-interviews`: native and interviews whose held-out half runs in a new session from the refill that cannot read Lite's source or its docs (`refill_session = "sealed"`, `sealed = ["source", "docs"]`). `sealed-native-src` and `sealed-interviews-src` are the same with the docs readable (`sealed = ["source"]`): the docs, which are wrong, stay. What the refill carried is all the model knows of the source.
    - **The source part is every top-level entry of the checkout but `docs`:** its packages and lab, its `.git` (which holds every source object), and its root files, `README.md`, `AGENTS.md` and `GUIDANCE.md` among them. So a sealed session loses Lite's own instruction files, which an unsealed one can follow (the delivery screen saw a probe follow AGENTS.md to GUIDANCE.md).
    - **The page still runs Lite:** in a sealed session the `lite` service builds a bundle (`services/bundle.py`) from a `git archive` copy, with nine trap internals renamed, esbuild-minified with names kept, no sourcemap, chunks named by hash; the copy is deleted once built. model-vite serves the bundle in the source's place and allows nothing of the checkout. The grader's server is unchanged.
    - **It is told:** each sealed regimen's `sealed_notice` opens the first held-out ask, as the user's intent. The asks still say where the source is and that `babylon-lite` resolves to it; the notice reconciles that.
    - **Probed warm:** the `seal-probe` service, before serve, loads a page importing `babylon-lite` in a browser config-identical to the model's, and checks what model-vite serves (no sealed file over `/@fs/`, no trap name in any loaded module), where that browser cannot go (the sealed file over `file://`, `chrome://`, grader-vite, an outside origin), and that the model's own browser starts on a fresh profile. It removes its page and its browser, so nothing of it reaches the model.
    - **Compliance is measured, never gated:** `process_trap.compliance` counts a sealed session's attempts on a sealed part, blocked and clean, its `/@fs/` fetches, its reads of the bundle, and lists its permission asks for hand-reading. A native refill restores only files under the worktree that the model's file tools read, and Lite's checkout is outside it, so a sealed arm's refill restores no Lite file by construction and the table's `carried sealed source` row reads 0: a guard. What the summary or the working memory says of the source in its own words is what memory is for, not a leak; the seal is against re-reading the source after the refill.
- **Every regimen sets `meditation_empty = "refill"`** (`../../README.md`): an interviews run that captured nothing by t8 still refills, carrying nothing, and its held-out half is graded. Without it that run would stop at the refill, and only the runs whose interviews captured something would have held-out scores.
- **Every regimen sets `head_nonce = "per_run"`:** each run's head starts with a nonce drawn from the run, so the table labels cached prefill carryover-free. A carried run's second session has the same nonce, drawn from the same run; under `fresh` and `sealed` it is the one in the carried trunk.
- **`lite-empty.jsonl`:** `lite.jsonl` whose canned interviews capture nothing, so a rehearsal of the interviews arm crosses an empty refill. In `lite.jsonl` the first task's canned answer reads its stub and the interview that read warrants records what it learned, so its rehearsal crosses a refill that carries it.
- **A live run needs the deny list** (`../README.md`, "A live experiment needs the deny list"). The model's sandbox reads everything but the secrets, and this machine holds the tasks' private solutions, its graders and every agent's transcripts. `--dry-run` rehearses each arm on the local canned substrate, with real services and real graders. Rehearse with the live run's flags, assembly on: a dry run with `--no-assemble` skips the step the first live batch failed at.
- **`make_scenario.py`** writes both scenarios from `grader/tasks_trap.py`, the prompts' one source; `--check` says when they are stale.

## What a run starts

The header's `services` (`../README.md`, "Services"), outside the sandbox, per run:
- `lite`: the checkout, checked at its commit. It names the checkout's path, which the prompts give the model.
- `model-vite` and `grader-vite`: two dev servers on the run's worktree, Lite's source aliased in read-only. The grader's carries `vite/lite-trace-plugin.mjs`, wrappers that record Lite's lifecycle calls into `window.__LT` without changing an argument or a result. The model's server never carries them.
- `grader-browser`: a headless chrome-devtools daemon with real input (`--experimentalVision`), allowed both dev servers. The model's sandbox cannot reach it.
- `model-browser`: the model's own daemon, allowed only its dev server, reached through one Unix socket (`sandbox_unix_sockets`) and its session id (`serve_env`). The regimens pass the session id on by name.

## Grading

`check.py` grades each attempt in the grader's browser, against the grader's dev server.
- **Tier a, what the page does:** state dumps and screenshot pixels over the task's timeline. Pixel positions come from a hand-written camera model.
- **Tier b, how it does it:** the mechanism the task's constraint states, read from the recorder at runtime and never from the model's source.
- **Right** means both tiers pass.
- **What the model is told is observations only:** each failing tier a check, expected against seen. A tier b failure is told only as the task's stated constraint, unmet. Never a cause: a cause would hand the model the fact the tutorial lets it find and the interviews capture (ruled 2026-10-21). Every check of both tiers, the dumps and the screenshots go to the attempt's `DIET_GRADE_OUT`.
- **Tier c, the process:** defined in `grader/process_trap.py` and computed later from the session log. It records, per task window (from the task's first ask to its last grade), the reads the task made of the Lite checkout and whether one re-derived the task's fact from the source. A read is parsed, not matched: heredoc writes, an output redirection's target, `node_modules` tooling and the model's own files are no read; a relative path after `cd`, and a page script fetching `/@fs/` source, are. A home directory reads as `~` on both sides, the command's paths and the checkout's: an assembled log writes home paths that way (`scripts/scrub.py`), a raw one spells them out, and the grade never depends on the grading machine's `$HOME`. `reads_fixtures.json` holds known reads and non-reads, and `--selftest` reports both numbers.

The graders were verified against 159 cases: each task's stub, its reference solution twice, and every wrong or alternative solution, each graded as expected. The references and alternatives stay private, outside this tree: worked solutions to pre-registered held-out tasks would teach any model trained on this tree.

## A first-move probe (#948)

`diet-drive scenario lite.jsonl --from-carry LOG --only-task TASK --regimen R` runs one held-out task's ask from a recorded refill, and nothing after it, under a regimen whose `[limits] max_steps` is the probe's K. `python3 -I grader/first_move.py LOG ROOT K` measures it from tool calls alone. The first relevant action is the first call that writes the task's `main.ts` or reads the Lite checkout (tier c's classifier); its outcome is `applied` (the write's code applies every fact the detectors test), `wrote without`, `read first`, or `none` within K.

The scan goes on past a read to the first write of `main.ts` within K, and says what was read before it. It distinguishes Lite (tier c's reads) from the model's own past code: a file in the directory of a tutorial task, one the carry ran before its refill, such as `cat t6-pick-dpr/main.ts`. The probe's own directory and a held-out task's hold stubs, and a guessed directory of a task's shape holds nothing. The worktree's own files are no read of Lite, but before a write they are a check, and copying a past solution is not taking up a note. A planted probe, `--planted FACT`, is the wrong-note control: the carry's notes are wrong about FACT. Its outcome is `followed` (the first write misses FACT), `resisted after checking Lite`, `resisted after checking own past code`, `resisted after checking both`, `resisted outright` (nothing was read before the write), or `none`.

The detectors are static heuristics of each fact's mechanism, one per fact, never a grade. `first_move_fixtures.json` holds code that applies each fact and code that misses it the ways the traps invite, and `--selftest` reports both numbers. Validated privately against the tasks' reference and alternative solutions: every right solution's facts detected, and every solution failing its constraint caught.

## Outcomes

`python3 -I outcomes.py DIR` writes `outcomes.json` beside each run's log, and `diet-drive table DIR` then counts each outcome per arm, as `outcome: <task>: <outcome>` rows. It covers the scene-update family, the held-out tasks whose facts are `late-add` or `late-family` (h1, h2, h3, h5). Each task's outcome is derived from its grade lines, its last attempt's diagnosis, and the reads it made between the refill and its last grade. A run whose held-out half ran in a second session is read from that session, `next/`, every read of it after the refill:
- **sufficient:** right, with no read of the Lite source or docs after the refill.
- **recoverable:** right after reading the source.
- **failed toward the docs:** wrong after reading only the docs, with a tier b attractor signature: it never made the change (declared it impossible), added before registration, rebuilt the scene, or replaced the mesh. In a sealed arm the docs are all that is left to read, and they are wrong.
- **mechanism lost:** made the change the way the task asks, a late add or a swap, and tier a still fails.
- **other:** anything else, a right answer after a docs-only read among them, with the tiers' results kept beside it.

`python3 -I check.py --selftest`, `python3 -I grader/process_trap.py --selftest`, `python3 -I grader/first_move.py --selftest`, `python3 -I services/bundle.py --selftest`, `python3 -I services/seal_probe.py --selftest` and `python3 -I outcomes.py --selftest` run without a browser, in `./verify.sh`'s scenarios check. The last builds a case for each outcome from the graders' own checks.
