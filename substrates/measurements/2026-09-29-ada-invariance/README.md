# Output invariance under MTP speculation on the drive endpoint's server, measured

This is the on/off/off test that #143's plan (I4d) specifies and the grading statute (`diet/AGENTS.md:21`) asks a speculative path to pass before it is graded on. It ran on 2026-09-29 in the window the maintainer ratified for #142, against `ada48-llamacpp-qwen38flashnext-q20`, instance `2026-09-28` (engine `e7051ef`). The results were posted on #142.

**Result: speculation changes the output.** On one greedy prompt:

- spec-off is bitwise reproducible over 1,024 tokens, within one process and across two separately launched processes;
- spec-on is reproducible within its process;
- every spec-on output first differs from every spec-off output at **token 65**.

## The test

**Why production was relaunched.** This engine compiles out per-request speculation control: at `e7051ef`, the `speculative.*` request fields in `tools/server/server-schema.cpp` sit inside `#if 0`. So a spec-off arm needs a server launched without the draft. `inv.sh` does this:

1. stops production;
2. launches each arm as a fresh server on another port, with production's own serving line and the port changed;
   - the off arms also drop the five `--spec-*` flags;
3. restores production through an exit trap, re-reading its binary digest and command line.

Production was down for about 4.5 minutes in all, over three restores:

- 02:03:39–02:05:50Z;
- 02:06:18–02:07:19Z;
- 02:07:46–02:09:01Z.

After each restore the digest (`41e6591d…`) and the command line read identical (`inv*.log`).

**What each arm sent** (`inv.py`, one fixed prompt):

- rendered by the server's own `/apply-template`, with sha256 `e174b1ec…` in every arm;
- sent to `/completion` with `temperature 0`, `top_k 1`, `cache_prompt: false`, `seed 924` and `n_predict 1024`;
- with `return_tokens`, so the generated token ids are compared, not text.

| file | arm | process | `draft_n` per rep | sha256 |
|---|---|---|---|---|
| `inv-off.json` | spec off | 1 | none | `f2d3d155…` |
| `inv-off-b.json` | spec off (see the defect below) | 1 | none | `2dcb8d87…` |
| `inv-off2.json` | spec off | 2 | none | `1943036a…` |
| `inv-on.json` | MTP, `n_max` 3, `p_min` 0.0 | 3 | 1037, 1028, 1028, 1028, 1028 | `118cffee…` |
| `compare.json` | the first differing token for every pair | — | — | `92a0d252…` |

| pairs | n | first differing token |
|---|---|---|
| off/off within process 1 | 45 | none in 1,024 |
| off2/off2 within process 2 | 10 | none in 1,024 |
| off/off2, across processes | 25 | none in 1,024 |
| on/on within process 3 | 10 | none in 1,024 |
| on/off, against each off set | 75 | **65** in every pair |

Rep 0 of the on arm drafted 1037 tokens and the others 1028, so its accept/reject trajectory differed while its output did not.

**What this does not cover:**

- concurrency, since every request ran alone;
- other prompts;
- spec-on across processes;
- anything past the first divergence.

## The defect, disclosed

- **What went wrong.** The first pass took `$!` from a `cd && cmd &` subshell, not from the server. Stopping the spec-off server killed only the subshell, the server kept its port, and the spec-on launch failed to bind.
- **What the first pass's "on" reps really are.** They were served by the spec-off process. They are kept as `inv-off-b.json`: `draft_n` is null in all five, and `inv.log` records the sequence.
- **The fix.**
  - Launches `exec` inside the subshell.
  - A free-port check was added before each launch.
  - Stopping goes by `pgrep -x` and the port on the command line.
- **The re-runs.** Spec-on (process 3, `inv-on.log`) and a second spec-off launch (process 2, `inv-off2.log`) were run after the fix. The defect had shown that a cross-process off/off control was missing.
- **Which script is committed.** `inv.sh` is the last version that ran, and it is the version the process 2 run used.

## At the divergence: the target's own distribution

**Why these reads could be taken with production up.** At position 65 the arms share tokens 0–64. The spec-off token is `.` (id 13), and the spec-on token is ` estimate` (id 15572). Both reads below were taken against production with no downtime, and neither involves a draft: the first predicted token after a prefill comes from the prefill's own logits.

**`margin-prefill.json`** (`margin-prefill.py`): the prompt plus the 65 shared tokens, prefilled cold in one batch and read three times, identical each time.

| rank | token | logprob |
|---|---|---|
| 1 | `.` | −1.5901 |
| 2 | `?` | −1.8776 |
| 3 | ` estimate` | −1.9581 |

- The top-two logit margin is 0.288.
- The spec-on token ranks third, 0.368 logits below the top.

**`margin-batch-shape.json`** (`margin-batch-shape.py`): the same position on a pinned slot warmed so that exactly 1 or 4 tokens are evaluated in the final batch. Four tokens is the size of an `n_max` 3 verify batch.

| final batch | order | `.` | ` estimate` | margin |
|---|---|---|---|---|
| 1 token | first | −1.5941 | −1.8579 | 0.264 |
| 4 tokens | first | −1.5817 | −1.7530 | 0.171 |
| 1 token | after the 4-token probe | −1.5894 | −1.6102 | **0.021** |
| 4 tokens | again | −1.5817 | −1.7530 | 0.171 |

**What this shows:**

- **The logits at one position depend on how the cache was built.**
  - The same 1-token batch read 0.264 or 0.021, depending on what the slot held before.
  - The 4-token read repeated exactly.
- **The spec-off token was first in every read.** The gap between the two emitted tokens ranged from 0.021 to 0.368 logits.
- **Speculation changes exactly that path.** Verify batches and rollbacks alter how the cache is built, and on this hybrid recurrent model how the recurrent state is chunked.

**What it does not show:**

- The verify batch's own logits at position 65 were not read.
- So whether speculation flipped a near-tie, or emitted a token its own verify pass did not rank first, is **not established**.
