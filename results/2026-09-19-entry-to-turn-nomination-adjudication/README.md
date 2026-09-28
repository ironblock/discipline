+++
hypothesis = "An entry recorded in the working object at turn N can be matched against the prose of a later turn sharply enough to nominate supersession at a fixed per-session budget without over-firing on mentions that do not supersede -- in each of the two registers #17 names, intent-to-intent and authored-sense -- and the literal tier's anchors, applied as a pre-gate, do not lower precision at that budget."
result = "refuted"
kind = "reproducible-by-config"
product_sha256 = "74fec8c5500370a8848a6116823a08ab8a6adf86e636a98e0e10c9b2d4c93882"
controls_run = []
known_defects = ["rule v1's supported precision bar (0.80) is above what the register's positive density lets the pooled top-5 attain (0.7667), so that clause is unadjudicated at k = 5 for every cell; a v2 wording is the maintainer's (#17, 5744391614)", "the verb's gate-arm bootstrap is over the mean per-row score difference with rejected rows at the floor, not the precision difference the pre-gate sub-rule names, so the sub-verdict is unadjudicated (fresh review of PR #96, B1)", "a row the anchored gate rejects still fills a drive's budget at the scoring's floor, tie-broken by id, when fewer than k rows are admitted; precision over admitted nominations only is not emitted (fresh review, S1)", "the run directory's README carries the assembler's generic conclusion, which says the pre-registration names no rule; this run's record consumes decision-rule.toml and the rule is applied here (fresh review, S3)"]
targets_checked = 5
targets_matched = 5

[regime]
arm = "entry-to-turn-nomination-four-embedders"
substrates = ["all-MiniLM-L6-v2", "bge-small-en-v1.5", "embeddinggemma-300m", "Qwen3-Embedding-0.6B"]
dogma_version = 0

[derivation]
applier_sha256 = "c12a74b37536871156e008864920a88b999c2da011f4740020bf18b2a317d2ed"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "4fa03345f185b20ed867153c3ce047fb6b65b0bd39f91472ea580bdb43171454"

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
"supported bar at k = 5" = "0.8, above the ceiling: unadjudicated for every cell"
"refuted precision ceiling adjudicable at k" = "1, 3, 5"
+++

# The entry-to-turn nomination run, adjudicated under its pre-registered rule

A derived directory. It cites `2026-09-19-entry-to-turn-nomination` by the digest of that directory's
product, applies the decision rule that run consumed by digest, and writes
the verdict the rule yields. Ratified on #17 as amended: the rule is fixed
for this run and applied mechanically; a different rule later is a new
directory beside this one, never an edit of it.

## Observation

`2026-09-19-entry-to-turn-nomination` reports 37 cells, 11 of them control
failures, and says `unadjudicated`: `diet bakeoff --into` applies no rule.
The rule is in `decision-rule.toml`, which that run consumed by digest before
any score existed, and nothing had applied it.

## Hypothesis

An entry recorded in the working object at turn N can be matched against the prose of a later turn sharply enough to nominate supersession at a fixed per-session budget without over-firing on mentions that do not supersede -- in each of the two registers #17 names, intent-to-intent and authored-sense -- and the literal tier's anchors, applied as a pre-gate, do not lower precision at that budget.

## Test

`recompute.sh`, over `report.json` and `decision-rule.toml` committed here at
the digests the record declares. It re-derives every digest, re-applies the
rule, and refuses if the verdict it derives is not byte-for-byte the
`verdict.json` committed beside it. The applier is the Python inside
`recompute.sh`; there is no other copy.

The rule, as ratified and amended: `supported` needs one contender cell at
budget 5 on the intent register with precision at or above 0.8,
hard-negative over-firing at or below 0.1, and separation at least
0.5 above the matched floor cell's, all on the same cell. `refuted`
needs either no contender cell at any pre-registered budget reaching precision
0.6, or the best-separated contender cell short of its matched floor cell
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

10 contender cells on the intent register at budget 5 (659 rows, 153 pairs excluded for stating no intent; positives 102 planted and 8 mined). 0 reach the precision bound; 0 of those also keep over-firing within its bound; 0 clear all three bounds at once. 4 cells have no margin over the floor because the matched floor cell is a control failure or a separation is undefined; they satisfy neither bound.

`refuted` clause one: 16 contender readings at any budget reach the precision ceiling. Clause two: the best-separated cell is `Qwen3-Embedding-0.6B/ensemble_max/without_gate`, d' 2.2271, margin over its matched floor cell 0.1421, which is short of the small margin.

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
`Qwen3-Embedding-0.6B/ensemble_max` +0.0111 (p 0.0001, p_holm 0.0039, floor 0.0001); `bge-small-en-v1.5/ensemble_max` +0.0777 (p 0.0001, p_holm 0.0039, floor 0.0001); `bge-small-en-v1.5/raw_cosine` +0.3778 (p 0.0001, p_holm 0.0039, floor 0.0001); `embeddinggemma-300m/ensemble_max` +0.0889 (p 0.0001, p_holm 0.0039, floor 0.0001); `embeddinggemma-300m/raw_cosine` +0.3333 (p 0.0001, p_holm 0.0039, floor 0.0001). 4 improved by the margin, 0 were
lower by it; the sub-verdict is `unadjudicated`: a pair clears the margin, and the paired bootstrap the sub-rule names -- of precision at k between the arms -- is not among the bootstraps the verb emits; the gate-arm bootstrap in report.json is over the mean score difference with rejected rows at the floor, which is not that statistic. Had the
emitted bootstrap been the named one, the ruled reading (uncorrected p at the
attainable floor, #17 5743190831) would give
`supported`; it is not, and the sub-verdict does not rest on it.

Attainability (ruled on #17, 5744340773): the register's positive density
lets the pooled top-5 be at most 0.7667 positive, under the
0.8 bar, so the `supported` clause is `unadjudicated` for every cell at k = 5
-- a construction defect of rule v1, disclosed on #17 before the run with two
v2 wordings for the maintainer; the verdict below rests on the separation
clause, which has no arithmetic ceiling. The `refuted` precision clause is
adjudicable at k = 1, 3, 5 and not beyond.

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
(five tool calls, 1,200 characters each), narrower than what the collector
sees at deployment, so the operating point here is a floor for that surface;
the provenance sidecar's anchor counts were taken over the wider build
surface and disagree with the instrument's gate on a few rows for that
reason.

Two readings the instrument fills, found by the fresh-instance review and
carried as known defects rather than corrected after the numbers: a row the
anchored gate rejects sits at the scoring's floor and still fills a drive's
budget when fewer than five rows are admitted, tie-broken by id, so the
with-gate precision and the sub-rule's deltas are over nominations the gate
did not make; and the bootstrap the verb emits between the gate arms is over
the mean per-row score difference, which those floors make negative by
construction, not the precision difference the sub-rule names. Both are
courier items to track one; a v2 adjudication sits beside this one when the
verb emits the named statistics.

The authored-sense register, 14 scored cells at budget 5, reported and
not adjudicated: precision from 0.1556 to 0.2444; the cells are in `verdict.json`.

## Conclusion

`refuted` under the rule as ratified and read here, on the
separation clause: the best-separated contender cell does not clear its
matched floor cell by the small margin. The `supported` clause was not
askable at k = 5 on this register. The pre-gate sub-verdict is `unadjudicated`,
carried beside the verdict and never folded in, as the ratified text says.
The by-source split above says what the pooled figure was carried by: the
planted half, at 1.0 or within one hit of it in every cell, against one or
two of the mined positives in the pooled top-5 -- the mined supersessions
the archive holds are not what an entry-to-turn cosine finds. Labelled
`characterization`.
