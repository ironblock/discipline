# Ground truth for the invariance result: which near-ties, and whether acceptance is exact

`../2026-09-29-ada-invariance/` found spec-on and spec-off greedy outputs first differing at token 65 on one prompt. The maintainer asked, before ruling #142, whether that is a numerical near-tie or an acceptance defect. Three reads answer it. All ran on 2026-09-29 against production of `ada48-llamacpp-qwen38flashnext-q20`, instance `2026-09-28`, **with no downtime**. That directory stays sealed; this one adds to it.

**Answer: the near-tie reading.**

- Every place the two paths disagree, across five prompts and 2,327 positions, is a flip to the spec-off path's **second-ranked** token, at a top-2 margin of **0.007–0.320 logits**.
- Which side of a near-tie spec-on takes depends on its own verify schedule.
- No emitted token outside spec-off's top two was seen.

## Method: spec-off decoding without a spec-off server

This engine has no per-request speculation control (`#if 0` around `speculative.*`, `tools/server/server-schema.cpp` at `e7051ef`). **A request with `n_predict 1` has a draft budget of 0**, because `get_n_draft_max` caps it at `n_remaining − 1`. Its token is sampled by the non-speculative path, with probs filled.

`gt.py scan` walks a token sequence one position at a time on a pinned slot, with `cache_prompt` on. At each position the server evaluates exactly one new token (`prompt_n` 1 in every row but the first), so the cache is built as spec-off decoding builds it, teacher-forced along the given tokens. It records the argmax and the top-two logprobs at every position.

**Validated both ways on the original prompt (p0):**

- **Along the committed spec-off tokens** (`scan-p0-along-off.json`): the argmax equals the given token at **all 1,024 positions**. The method reproduces spec-off decoding exactly.
- **Along the committed spec-on tokens** (`scan-p0-along-on.json`): the first position where the argmax differs is **65**, which matches the invariance test.

So along a spec-on output, the first position where the scan's argmax differs is the first token a spec-off run would emit differently. The scan's p0 inputs are rep 0 of `inv-off.json` and `inv-on.json` in the sealed directory; p1–p4 use rep 0 of `on-p*.json` here.

## Read 1: the verify pass's own logits at position 65: not exposed by this build

- **From the source.** On the speculative path the server never fills per-token probs: the accept loop at `e7051ef` sets `result.prob = 1.0f` with `// TODO: set result.probs` (`tools/server/server-context.cpp`, the loop after the verify step).
- **Measured.** A spec-on run with `n_probs 5` (`on-p0-nprobs.json`) returned probs for **1 of 1,024** tokens: position 0, the non-speculative first token. That run also reproduced the committed spec-on tokens exactly from a different server process, the first reading of spec-on reproducibility across processes.
- **By the source read** posted on #142 (https://github.com/ironblock/discipline/issues/142#issuecomment-5883063891), acceptance at greedy is exact equality with the verify batch's argmax (`common/sampling.cpp:678-706`). Reads 2 and 3 are consistent with that, but neither measures the verify batch's logits.

## Read 2: rejections before 65, and the schedule deciding the flip

`steps-p0.json` holds spec-on runs at `n_predict` 1–70, with `draft_n` and `draft_n_accepted`.

- **The first rejection comes within the first 7 tokens:** `n_predict` 7 gives 4 drafted and 3 accepted. By `n_predict` 64, 26 of 66 drafted tokens had been rejected. So rollbacks (checkpoint restores on this hybrid model) happen well before 65. "None before 65" is ruled out.
- **Truncation changes the draft schedule near the end of a run** (`n_draft_max ≤ n_remaining − 1`), and that changes the tokens.
  - **Runs cut at 66 and 67** emitted **`.` at 65**, the spec-off token, and followed the spec-off tokens exactly from there. The full-length spec-on run emitted ` estimate`.
  - **The run cut at 60** emitted, at 58, the spec-off path's runner-up there (margin 0.045).
- So spec-on lands on either side of a near-tie depending on its own verify schedule.

## Read 3: breadth, five prompts

`breadth-summary.json` summarises it. Each prompt had spec-on at k = 3 (`on-p*.json`), plus the scan along rep 0.

| prompt | tokens | spec-on reps identical | disagreements | first at | margin there | all margins | on token = off's 2nd, every time |
|---|---|---|---|---|---|---|---|
| p0 (the original) | 1,024 | (see the sealed dir) | 14 | 65 | 0.124 | 0.010–0.285 | yes |
| p1 (arithmetic) | 320 | yes | 3 | 95 | 0.117 | 0.014–0.117 | yes |
| p2 (prose) | 181 | yes | 6 | 21 | 0.098 | 0.034–0.320 | yes |
| p3 (explanation) | 623 | yes | 10 | 55 | 0.119 | 0.007–0.156 | yes |
| p4 (list) | 179 | yes | 3 | 101 | 0.014 | 0.014–0.070 | yes |

- **Disagreements are rare but reached on every prompt:** 36 of 2,327 positions (1.5%), with the first between tokens 21 and 101. So a long greedy output is not bit-identical between the two paths.
- **Every disagreement is a near-tie flipped to the runner-up.**
  - Only 8–12% of all positions have a top-2 margin under 0.3.
  - The widest flip (0.320) sits inside the range the cache path alone moved at position 65 (0.021–0.368, `../2026-09-29-ada-invariance/margin-*.json`).
- **Cross-read.** The margin at 65 reads 0.124 here and 0.021–0.368 in the sealed directory's probes: the same logits, read along different cache paths.

## Not covered

- The verify batch's logits (Read 1).
- Concurrency: every request ran alone.
- Any sampler but greedy.
