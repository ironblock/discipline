+++
hypothesis = "An entry recorded in the working object at turn N can be matched against the prose of a later turn sharply enough to nominate supersession at a fixed per-session budget without over-firing on mentions that do not supersede -- in each of the two registers #17 names, intent-to-intent and authored-sense -- and the literal tier's anchors, applied as a pre-gate, do not lower precision at that budget."
result = "refuted"
kind = "reproducible-by-config"
product_sha256 = "1c46f51f2273c289dd29b0d238aafebb3d096e33d38d3fb9f6eb57e208dff852"
controls_run = []
known_defects = ["a row the anchored gate rejects still fills a drive's budget at the scoring's floor when fewer than k rows are admitted; the pooled figure is adjudicated as ratified and the admitted-only figure the verb emits is reported beside it, not adjudicated", "the register's turn surface is the judged view: the step's one tool call with its output cut to 1,200 characters, narrower than the collector's deployment surface", "rule v2's wording was fixed by the maintainer after the v1 verdict's numbers existed, as a reading of the ratified text; the separation clause that decides both verdicts is untouched by it"]
targets_checked = 5
targets_matched = 5

[regime]
arm = "entry-to-turn-nomination-four-embedders"
substrates = ["all-MiniLM-L6-v2", "bge-small-en-v1.5", "embeddinggemma-300m", "Qwen3-Embedding-0.6B"]
dogma_version = 0

[derivation]
applier_sha256 = "c37cb1158fb87f82dba0fba44c84f5131b9966e26f6266c5834e28edbf819580"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "764bcec70f94bad02635c8c9983aa84824ac24d58a68b0fee0fb72679dd4bce5"

[envelope]
label = "characterization"
"budget ladder" = "[1, 3, 5, 10, 25, 50], k = 5 adjudicated -- the collector's per-session nomination budget, ratified on #24/#69; the ladder is the design's own (within)"
"anchored pre-gate" = "two distinct anchors must recur -- the ratified pre-registration's; the shipped tier 0 fires on one, carried as a reading (within)"
"bge-small-en-v1.5 query instruction" = "not used -- the card recommends its query instruction for short-query retrieval; declared unused on #24's regime (OUTSIDE)"
"embeddinggemma-300m prompts" = "documented document and retrieval-query prompts, byte for byte -- the model card's (within)"
"Qwen3-Embedding-0.6B instruction byte form" = "`Instruct: {task}\\nQuery: ` with a space after `Query:` -- the card emits no space; the space is the ruling's byte form declared on #24 (OUTSIDE)"
"Qwen3-Embedding-0.6B document prefix" = "none -- the card: documents need no instruction (within)"
"pooling per embedder" = "the model's shipped modules (mean; CLS; the graph's head; last token) -- each model's documented pooling (within)"
"all-MiniLM-L6-v2 sequence length" = "max_seq_length 256, silent truncation -- 44 of 1566 cached texts exceed 256 tokens, listed by digest in the meta (OUTSIDE)"
"bge-small-en-v1.5 sequence length" = "max_seq_length 512, silent truncation -- 25 of 1566 cached texts exceed 512 tokens, listed by digest in the meta (OUTSIDE)"
"embeddinggemma-300m context" = "2048 positions, 3 documents truncated to the context less the closing token -- the model's documented context; the truncated documents are listed by digest in the meta (OUTSIDE)"

[attainability]
"precision ceiling at k = 1" = "1.0"
"precision ceiling at k = 3" = "0.8333"
"precision ceiling at k = 5" = "0.7667"
"precision ceiling at k = 10" = "0.5251"
"precision ceiling at k = 25" = "0.3005"
"precision ceiling at k = 50" = "0.2124"
"supported bar at k = 5" = "0.6133, attainable"
"refuted precision ceiling adjudicable at k" = "1, 3, 5, 10, 25, 50"
+++

# The entry-to-turn nomination run, adjudicated under rule v2: the bars scaled to what the register lets a budget attain

A derived directory, the second adjudication of the same run. It cites
`2026-09-20-entry-to-turn-nomination-v2` (the run re-assembled by the verb after #99, which emits the
gate-arm precision bootstrap and the admitted-only precision) by the digest
of its product, applies rule v2 -- the ratified rule with its precision bars
scaled to the rung's attainable ceiling, worded by the maintainer on #17
(5747481065) after the v1 adjudication found the 0.80 bar above what this
register lets k = 5 attain -- and writes the verdict it yields. It sits
beside the v1 adjudication; both rules and both verdicts are in the ledger.

## Observation

`2026-09-20-entry-to-turn-nomination-v2` reports 37 cells, 11 of them control
failures, and says `unadjudicated`: `diet bakeoff --into` applies no rule.
That run consumed rule v1 (`decision-rule.toml`) by digest before any score
existed; this directory consumes `decision-rule-v2.toml`, the same tables plus
the maintainer's `[rule.v2]` wording of 2026-09-20, fixed after the v1 numbers
existed and disclosed as such under `known_defects`.

## Hypothesis

An entry recorded in the working object at turn N can be matched against the prose of a later turn sharply enough to nominate supersession at a fixed per-session budget without over-firing on mentions that do not supersede -- in each of the two registers #17 names, intent-to-intent and authored-sense -- and the literal tier's anchors, applied as a pre-gate, do not lower precision at that budget.

## Test

`recompute.sh`, over `report.json` and `decision-rule-v2.toml` committed here at
the digests the record declares. It re-derives every digest, re-applies the
rule, and refuses if the verdict it derives is not byte-for-byte the
`verdict.json` committed beside it. The applier is the Python inside
`recompute.sh`; there is no other copy.

Rule v2: `supported` needs one contender cell at budget 5 on the intent
register with precision at or above 0.80 x the rung's attainable ceiling
(0.6133 here, the ceiling being 0.7667),
hard-negative over-firing at or below 0.1, and separation at least
0.5 above the matched floor cell's, all on the same cell. `refuted`
needs either no contender cell at any pre-registered budget reaching 0.60 x
that budget's ceiling, or the best-separated contender cell short of its matched floor cell
by the 0.2 margin. `inconclusive` otherwise. A margin that cannot be
computed satisfies neither bound. The pre-gate sub-rule compares the anchored
arm's precision at budget 5 with the ungated arm's per contender
(embedder, scoring), with the gate-arm paired bootstrap the verb emits.

The readings this applier still takes are written on the verdict under
`readings`: the primary-register scope of "over all scored contender cells";
the gate-arm p read uncorrected at the attainable floor, as ruled, with the
Holm-corrected readings carried under `alternatives`; a cell as a (scoring, gate) pair per embedder.
The authored-sense register is scored and reported beside the verdict, not
adjudicated. The arithmetic's environment is the `[derivation]` block in the
front-matter (ruled on #84); the record's start row stays the run being
adjudicated.

## Results

`verdict.json`, whose digest is `product_sha256` above and in the summary row.

10 contender cells on the intent register at budget 5 (659 rows, 153 pairs excluded for stating no intent; positives 102 planted and 8 mined). 4 reach the scaled precision bar (0.6133): `Qwen3-Embedding-0.6B/ensemble_max/with_gate`, `bge-small-en-v1.5/ensemble_max/with_gate`, `embeddinggemma-300m/ensemble_max/with_gate`, `Qwen3-Embedding-0.6B/ensemble_max/without_gate`; 0 of those also keep over-firing within its bound; 0 clear all three bounds at once. 4 cells have no margin over the floor because the matched floor cell is a control failure or a separation is undefined; they satisfy neither bound.

`refuted` clause one: 50 contender readings at any budget reach the precision ceiling. Clause two: the best-separated cell is `Qwen3-Embedding-0.6B/ensemble_max/without_gate`, d' 2.2271, margin over its matched floor cell 0.1421, which is short of the small margin.

Precision at budget 5 by source, beside the pooled figure (amendment 4):
- `Qwen3-Embedding-0.6B/ensemble_max/with_gate`: pooled 0.6333, planted 56/57, mined 1/33
- `bge-small-en-v1.5/ensemble_max/with_gate`: pooled 0.6444, planted 57/58, mined 1/32
- `embeddinggemma-300m/ensemble_max/with_gate`: pooled 0.6333, planted 56/57, mined 1/33
- `Qwen3-Embedding-0.6B/ensemble_max/without_gate`: pooled 0.6222, planted 54/54, mined 2/36
- `bge-small-en-v1.5/ensemble_max/without_gate`: pooled 0.5667, planted 49/49, mined 2/41
- `embeddinggemma-300m/ensemble_max/without_gate`: pooled 0.5444, planted 47/47, mined 2/43
- `bge-small-en-v1.5/raw_cosine/with_gate`: pooled 0.5778, planted 51/52, mined 1/38
- `embeddinggemma-300m/raw_cosine/with_gate`: pooled 0.5889, planted 52/53, mined 1/37
- `bge-small-en-v1.5/raw_cosine/without_gate`: pooled 0.2000, planted 16/19, mined 2/71
- `embeddinggemma-300m/raw_cosine/without_gate`: pooled 0.2556, planted 21/23, mined 2/67

The pre-gate sub-rule over 5 contender (embedder, scoring) pairs:
`Qwen3-Embedding-0.6B/ensemble_max` +0.0111 (p 0.4474, p_holm 1.0000, floor 0.0001); `bge-small-en-v1.5/ensemble_max` +0.0778 (p 0.0271, p_holm 1.0000, floor 0.0001); `bge-small-en-v1.5/raw_cosine` +0.3778 (p 0.0001, p_holm 0.0117, floor 0.0001); `embeddinggemma-300m/ensemble_max` +0.0889 (p 0.0148, p_holm 0.8140, floor 0.0001); `embeddinggemma-300m/raw_cosine` +0.3333 (p 0.0001, p_holm 0.0117, floor 0.0001). 4 improved by the margin, 0 were
lower by it; the sub-verdict is `supported`: a pair clears the margin with its precision bootstrap's uncorrected p at the attainable floor. The
statistic is the one the sub-rule names, the drive-resampled paired bootstrap
of pooled precision at budget 5 between the arms, which the verb emits since
#99; its own p is read uncorrected at the attainable floor, as ruled on #17
(5743190831), and the Holm readings ride beside it:
`inconclusive` with the corrected p at the floor and
`supported` with the corrected p below 0.05.
The Holm family is every comparison the verb emits at once, and it grew from
39 to 117 when #99 folded the 78 precision bootstraps in, so every `p_holm`
here differs from v1's; the uncorrected p the rule reads is unchanged.

Beside every cell, the precision over admitted nominations only (the verb's
`admitted_only`, since #99): a row the anchored gate rejects still fills a
drive's budget at the floor when fewer than five are admitted, so the pooled
with-gate figure counts nominations the gate did not make; the pooled figure is
what the rule adjudicates, and the admitted-only figure says how much of it
the filling carried.

Of the four known defects the v1 adjudication carried, two are closed by #99
(the gate-arm statistic and the admitted-only figure) and one by this rule
(the bar above the ceiling); the run README's conclusion is per kind since
#99. The turn surface remains the judged view, as before.


Attainability (ruled on #17, 5744340773) under rule v2: the register's positive
density lets the pooled top-5 be at most 0.7667 positive, and the
supported bar is 0.80 of that, 0.6133, attainable by construction; the
refuted clause's precision floor is 0.60 of each rung's ceiling. So the
`supported` clause is asked here, and it fails on the other two bounds: every
cell that reaches the bar over-fires above 0.10, and no cell's separation
clears its matched floor cell by 0.5.

Envelope: 5 parameters sit outside their components' recommended
range (bge-small-en-v1.5 query instruction; Qwen3-Embedding-0.6B instruction byte form; all-MiniLM-L6-v2 sequence length; bge-small-en-v1.5 sequence length; embeddinggemma-300m context), cited in the front-matter, so this verdict carries the
`characterization` label the ruling assigns: a result about this configuration, not a claim
about the mechanism.

The floor: `all-MiniLM-L6-v2` was predicted to separate entry-to-turn pairs
worse than the instruction-tuned cells. Of the 6 contender cells with a
computable margin, 4 sit below their matched floor cell and none clears it by the
small margin; the prediction did not hold on this register.

Register, as the report counts it: 812 pairs, 659 of them rows of the
intent register (153 stated no intent), with 102 planted and
8 mined positives; fewer positives than the text's "roughly 150",
ruled to run as is (#17, 5743269467). The turn surface is the judged view
(the step's one tool call, its output cut to 1,200 characters), narrower than what the collector
sees at deployment, so the operating point here is a floor for that surface;
the provenance sidecar's anchor counts were taken over the wider build
surface and disagree with the instrument's gate on a few rows for that
reason. The register carries the step's one tool call (the harness runs one
command per step), its output cut to 1,200 characters.

The authored-sense register, 14 scored cells at budget 5, reported and
not adjudicated: precision from 0.1556 to 0.2444; the cells are in `verdict.json`.

## Conclusion

`refuted` under rule v2, on the separation clause: the
best-separated contender cell does not clear its matched floor cell by the
small margin. The `supported` clause was asked at k = 5 and fails on
over-firing and separation, not on precision. The pre-gate sub-verdict is `supported`,
carried beside the verdict and never folded in, as the ratified text says.
The by-source split above says what the pooled figure was carried by: the
planted half, at 1.0 or within one hit of it in every cell, against one or
two of the mined positives in the pooled top-5 -- the mined supersessions
the archive holds are not what an entry-to-turn cosine finds. Labelled
`characterization`.
