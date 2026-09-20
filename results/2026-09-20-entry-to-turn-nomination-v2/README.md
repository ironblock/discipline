+++
hypothesis = "An entry recorded in the working object at turn N can be matched against the prose of a later turn sharply enough to nominate supersession at a fixed per-session budget without over-firing on mentions that do not supersede -- in each of the two registers #17 names, intent-to-intent and authored-sense -- and the literal tier's anchors, applied as a pre-gate, do not lower precision at that budget."
result = "unadjudicated"
kind = "reproducible-by-config"
product_sha256 = "764bcec70f94bad02635c8c9983aa84824ac24d58a68b0fee0fb72679dd4bce5"
pre_registration_sha256 = "0b3ac3bb381b5bb823f3d858259e5618d7eabd543c5d1e2e6000181f88bf6e9a"
controls_run = ["scoring-extremes", "shuffled-label-null"]
known_defects = []
targets_checked = 19

[regime]
arm = "entry-to-turn-nomination-four-embedders"
substrates = ["all-MiniLM-L6-v2", "bge-small-en-v1.5", "embeddinggemma-300m", "Qwen3-Embedding-0.6B"]
dogma_version = 0

[pre_registration]
primary = "precision at a fixed nomination budget of k = 5 per drive, the top k of each drive pooled across drives, on the intent register, per embedder, scoring and gate"
separation = "the area under the curve and the standardised separation, per cell, positives against every other row"
over_firing = "the share of hard-negative rows -- later turns that name the entry's anchors without superseding it -- nominated within the pooled per-drive budget"
by_source = "precision at every budget broken out by source, planted against mined, beside the pooled figure"
correction = "paired bootstrap across embedders within a cell and between the two gate arms within an (embedder, scoring), Holm-corrected together, the attainable p floor printed beside every p"
+++

# The entry-to-turn nomination run, over the caches this record consumed

Written by `diet bakeoff --into`. The numbers are in `report.json`;
this file is what makes them checkable.

## Observation

A record declared a pairs register and the caches that place its texts,
with a digest for each, and nothing had turned them into numbers.

## Hypothesis

An entry recorded in the working object at turn N can be matched against the prose of a later turn sharply enough to nominate supersession at a fixed per-session budget without over-firing on mentions that do not supersede -- in each of the two registers #17 names, intent-to-intent and authored-sense -- and the literal tier's anchors, applied as a pre-gate, do not lower precision at that budget.

## Test

`diet bakeoff run.jsonl --into <this directory>`, over the artefacts
committed here. Every cache is read at the digest the record declares
for it; a cache whose bytes are not those bytes stops the run rather
than scoring something else. The pre-registered endpoints are in the
`[pre_registration]` table above and the settings they run under --
the budget ladder, the resamples, the shuffles, the attainable p floor
-- are in `report.json`.

## Results

19 artefact(s) checked, 19 matched. The cells, the
comparisons and the null are in `report.json`, whose digest is
`product_sha256` above and in the summary row.

## Conclusion

`unadjudicated`, which is not a verdict and does not pretend to be.
Ruled 2026-09-13: `inconclusive` is a SCIENTIFIC verdict -- the data
did not decide -- and a directory whose numbers are decisive while
its front-matter says `inconclusive` states a falsehood a reader has
to catch. `unadjudicated` states what is true: no one has applied a
decision rule to these numbers yet.

A pairs claim is pre-registered alongside a `decision-rule.toml`,
written before any score existed and pinned by digest. Applying that
rule to the numbers this directory assembled is a separate
adjudication step -- this verb writes the numbers; an adjudication
reads them against the rule and is recorded beside this directory.
Until that adjudication exists, the answer here is `unadjudicated`.

Every input is committed beside this file at the digest the record consumed, so the same command over the same bytes produces the same numbers.
