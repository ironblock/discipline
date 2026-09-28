+++
hypothesis = "A generative interview fired at a step where nothing capture-worthy happened confabulates an entry in a majority of calls, in every prompt-language cell, and the harness's own compaction turn does so at a higher rate still."
result = "supported"
kind = "reproducible-by-config"
product_sha256 = "7b2ff52e12b693a77bcbd50c86bc431c14c18168cbcf05cf94d3b74f7f7a4464"
controls_run = ["sixteen gold steps with a known capture-worthy event, graded alongside the null steps", "the harness-meta null step, the compaction turn itself, scored by two independent criteria that must agree"]
known_defects = ["The asks fired were the campaign's own twenty-four prompt-language variants, carried per row in the artifact, not the pinned dogma templates; dogma_version names the pinned set in force for comparison, not the words sent.", "Every parse rate in this campaign is a floor: answers were graded through the interview parser of that era, before its heading, bold-tag and wrapper-fence defects were fixed, and this campaign was never regraded.", "No substrate capture was taken for this fire, so the substrate INSTANCE is an inference, not a measurement. It is inferred from bracketing: two captures before 2026-07-29 and three after it, on 2026-08-03, all record deployment 44.20260721 on kernel 7.1.3, and the archive dates the next deployment entering service to 2026-08-07. An earlier revision stated the instance flatly and argued it only from above, by reading a deployment version string's digits as an image build date -- which the archive nowhere states.", "The README's PROSE figures are not bound to the product. `recompute.sh` re-derives every number the front matter states and reads nothing else, so the rates and counts in the body -- the numbers a reader actually takes away -- could drift from `headline.json` and every gate would stay green; a review demonstrated exactly that. Closing it needs a declaration this schema does not have: the directory naming which product fields its prose cites, checked the way the front matter now is.", "The grading itself (is_confabulation per row) is consumed, not recomputed: it is the archived grader's verdict, mechanical on the parsed answer, and this directory re-derives the three headline figures from it.", "The substrate's engine is identified by record, not by measurement: `engine.version_or_digest` is the sha256 of the release tarball the serving binary was unpacked from, which the registry carries for this substrate, and the archive's note that the server ran from that unpacked directory is what ties the two. The running binary was not hashed at this fire; its digest was first measured on 2026-09-05, on a later instance of the same substrate, and by the ruling on the registry it does not attach to this one."]
targets_checked = 3
targets_matched = 3

[regime]
arm = "prompt-language-factorial-campaign-one"
substrates = ["accel24-beellama-qwen27b-q4kxl"]
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
the record's start row, which declares one substrate by its registry id:
`accel24-beellama-qwen27b-q4kxl`, the 27B at UD-Q4_K_XL served by the beellama
engine on the accelerator host, its weights by digest, its hardware by the
registry's fingerprint of that host, temperature 0.6, top_k 20, top_p 1.0,
min_p 0.0, reasoning on. The instance is the registry's `2026-07-25`, inferred
rather than captured (see `known_defects`). `campaign-one.graded.jsonl` is every call with its graded
verdict; `recompute_headline.py` derives the headline; `headline.json` is
the committed result.

Two collapse conventions are pinned in the instrument because a hand
re-derivation picks one silently: the aggregate is per call (336), and the
per-cell floor is per step with the two draws collapsed by any-of; computing
the floor per call yields 0.357 and reads as a defect that is not there.

Re-run: `bash recompute.sh` exits 0 when every field of `headline.json`
reproduces from the artifact, 1 when any differs. The record's summary is a
`recompute`, not a drive: three targets checked, three matched, and the three
digests it compared, in path order -- the same three every claim consumes.

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
graded field; the instrument differs in four places, which is the whole diff
against that program's copy -- the line that names its input path, two
docstring phrases that pointed at that program's files, and a comment in
`main()` that named its grader by a private filename.
