+++
hypothesis = "precision at a fixed nomination budget, the top k of the register, per embedder, scoring and gate"
result = "unadjudicated"
kind = "reproducible-by-config"
product_sha256 = "eac8640d9238471698a848a4c594b26e7e3f23bada875cf19475d9c5017f6789"
pre_registration_sha256 = "82b03174df245e6111299b382f3e0519e61cc041c3febac5b973eed7c49383a4"
controls_run = ["scoring-extremes", "shuffled-label-null"]
known_defects = ["v1 declared the shuffled-label null under controls_run and the runner that wrote this directory did not execute it: report.json carries the null's parameters and no band. Ruled on #17 (5743190831, 2026-09-19): corrected by this superseding note, not by an edit of the numbers; the assembler's correction, to declare only the nulls it runs, is track one's and is not on main as of a7d2148."]
targets_checked = 12

[regime]
arm = "sense-bakeoff-four-embedders"
substrates = ["all-MiniLM-L6-v2", "bge-small-en-v1.5", "embeddinggemma-300m", "Qwen3-Embedding-0.6B"]
dogma_version = 0

[pre_registration]
primary = "precision at a fixed nomination budget, the top k of the register, per embedder, scoring and gate"
separation = "the area under the curve and the standardised separation, per cell"
over_firing = "the share of hard-negative rows nominated within the budget"
comparator = "an entailment cross-encoder as the accuracy ceiling, so the gap between it and an embedder is priced rather than assumed"
correction = "paired bootstrap across embedders, Holm-corrected across cells, the attainable p floor printed beside every p"
+++

# The sense bakeoff, over the caches this record consumed

Written by `diet bakeoff --into`. The numbers are in `report.json`;
this file is what makes them checkable.

## Observation

A record declared the caches it consumed, with a digest for each, and
nothing had turned those caches into numbers.

## Hypothesis

precision at a fixed nomination budget, the top k of the register, per embedder, scoring and gate

## Test

`diet bakeoff run.jsonl --into <this directory>`, over the artefacts
committed here. Every cache is read at the digest the record declares
for it; a cache whose bytes are not those bytes stops the run rather
than scoring something else. The pre-registered endpoints are in the
`[pre_registration]` table above and the settings they run under --
the budget ladder, the resamples, the shuffles, the attainable p floor
-- are in `report.json`.

## Results

12 artefact(s) checked, 12 matched. The cells, the
comparisons and the null are in `report.json`, whose digest is
`product_sha256` above and in the summary row.

## Conclusion

`unadjudicated`, which is not a verdict and does not pretend to be.
Ruled 2026-09-13: `inconclusive` is a SCIENTIFIC verdict -- the data
did not decide -- and a directory whose numbers are decisive while
its front-matter says `inconclusive` states a falsehood a reader has
to catch. `unadjudicated` states what is true: no one has applied a
decision rule.

And the reason none has been applied is a gap in the PRE-REGISTRATION,
not in this verb: it names endpoints and no rule that turns them into
a verdict. The rule going forward is that a pre-registration carries
its decision rule, written before the data like every other endpoint;
when it does, this assembler applies it mechanically and writes
`supported`, `refuted` or `inconclusive`, because applying a
PRE-REGISTERED rule after seeing numbers is not choosing after seeing
them. Until this claim's pre-registration carries one, the answer is
`unadjudicated`.

Every input is committed beside this file at the digest the record consumed, so the same command over the same bytes produces the same numbers.
