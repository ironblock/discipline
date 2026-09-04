+++
hypothesis = "A generative interview fired at a step where nothing capture-worthy happened confabulates an entry in a majority of calls, in every prompt-language cell, and the harness's own compaction turn does so at a higher rate still."
result = "supported"
product_sha256 = "7b2ff52e12b693a77bcbd50c86bc431c14c18168cbcf05cf94d3b74f7f7a4464"
controls_run = ["sixteen gold steps with a known capture-worthy event, graded alongside the null steps", "the harness-meta null step, the compaction turn itself, scored by two independent criteria that must agree"]
known_defects = ["The asks fired were the campaign's own twenty-four prompt-language variants, carried per row in the artifact, not the pinned dogma templates; dogma_version names the pinned set in force for comparison, not the words sent.", "Every parse rate in this campaign is a floor: answers were graded through the interview parser of that era, before its heading, bold-tag and wrapper-fence defects were fixed, and this campaign was never regraded.", "The grading itself (is_confabulation per row) is consumed, not recomputed: it is the archived grader's verdict, mechanical on the parsed answer, and this directory re-derives the three headline figures from it."]
turns = 0
prefill_tokens_total = 0

[regime]
arm = "prompt-language-factorial-campaign-one"
substrate = "box-qwen36-27b-ud-q4kxl"
dogma_version = 0
+++

# Confabulation on ecological nulls

The first recompute-confirmed row ported from the research program. The
numbers here were first computed in prose; the committed instrument is the
derivation that makes them a gate-0 target, and it must reproduce them
exactly.

## Observation

A capture interview asked at every step answers something at every step.
Steps where nothing capture-worthy happened, the ecological nulls, were
answered with entries anyway, across every wording tried, and the harness's
own summarise-and-replace turn answered with invented entries too.

## Hypothesis

A generative interview fired at a step where nothing capture-worthy happened
confabulates an entry in a majority of calls, in every prompt-language cell,
and the harness's own compaction turn does so at a higher rate still.

## Test

Campaign one of the prompt-language factorial, fired 2026-07-29: 24 steps of
one replayed session (16 gold, 7 primary null, 1 harness-meta null) x 24 cells
(E x A x B x C: two wordings, three presupposition loads, two framings, an
existence gate on or off) x k = 2 draws, 1,152 calls, no errors. The regime is
the record's start row: Qwen3.6-27B UD-Q4_K_XL on the pinned box, temperature
0.6, thinking on. `campaign-one.graded.jsonl` is every call with its graded
verdict; `recompute_headline.py` derives the headline; `headline.json` is
the committed result.

Two collapse conventions are pinned in the instrument because a hand
re-derivation picks one silently: the aggregate is per call (336), and the
per-cell floor is per step with the two draws collapsed by any-of; computing
the floor per call yields 0.357 and reads as a defect that is not there.

Re-run: `bash recompute.sh` exits 0 when every field of `headline.json`
reproduces from the artifact, 1 when any differs. A recompute over an archive
has no session turns, so the summary carries zero.

## Results

Per call, 244 of the 336 primary-null calls confabulated (0.726). Per cell,
under any-of collapse, the floor is 0.571 (4 of 7 steps) and eleven of 24 cells
sit at 1.0. The compaction turn confabulated on 41 of 48 calls (0.854), by two
independent criteria that agree. The three claims in `run.jsonl` state each
number, and `recompute.sh` re-derives all 37 fields of the headline.

## Conclusion

Supported, on one replayed session and one model: an interview fired blind
generates to fill the form. Unknown from here: whether the rate moves on a
second substrate, and what a schema-constrained or tool-mediated modality
does to it. The pre-registered directional comparisons inside the campaign
(presupposition load, framing, the existence gate) did not clear significance
at k = 2 and are not claimed here.

Fired 2026-07-29 in the research program against its inference box; graded
there; the three headline figures were re-derived by the committed instrument
on 2026-08-24 and again here. The artifact is byte-identical to the research
program's except that 20 occurrences of a home-directory path inside
tool-output strings were shortened to `~/` (180 bytes), which touches no
graded field; the instrument differs only in the line that names its input
path and in the two docstring phrases that pointed at that program's files.
