+++
hypothesis = "Authored sense-descriptions match concrete transcript sentences sharply enough to prioritise capture without over-firing, and a lexical pre-gate improves precision at a fixed budget."
result = "inconclusive"
kind = "reproducible-by-config"
product_sha256 = "42d0e5a1282283e2d3b74c57141e29ec994c0b8b3b30be454200546f6a7c0ed4"
controls_run = ["declared-floor"]
known_defects = []
targets_checked = 2
targets_matched = 2

[regime]
arm = "sense-bakeoff-four-embedders"
substrates = ["all-MiniLM-L6-v2", "bge-small-en-v1.5", "embeddinggemma-300m", "Qwen3-Embedding-0.6B"]
dogma_version = 0
+++

# The sense bakeoff, adjudicated under its pre-registered rule

A derived directory. It cites `2026-09-17-sense-bakeoff` by the digest of that
directory's product, applies the decision rule that run consumed by
digest, and writes the verdict the rule yields. Ruled on #24: the rule
is fixed for this run and applied mechanically; a different rule later
is a new directory beside this one, never an edit of it.

## Observation

`2026-09-17-sense-bakeoff` reports fifty-eight cells, six of them control
failures, and says `unadjudicated`: `diet bakeoff --into` applies no
rule, because the pre-registration it reads carries none. The rule is
in `decision-rule.toml`, which that run consumed by digest before any
score existed, and nothing had applied it.

## Hypothesis

Authored sense-descriptions match concrete transcript sentences sharply enough to prioritise capture without over-firing, and a lexical pre-gate improves precision at a fixed budget.

## Test

`recompute.sh`, over `report.json` and `decision-rule.toml` committed
here at the digests the record declares. It re-derives every digest,
re-applies the rule, and refuses if the verdict it derives is not
byte-for-byte the `verdict.json` committed beside it. The applier is
the Python inside `recompute.sh`; there is no other copy.

The rule, as ratified: `supported` needs one contender cell at budget
five on the primary register with precision at or above the bound,
hard-negative over-firing at or below its bound, and separation at
least the declared margin above the floor's, all on the same cell.
`refuted` needs either no contender cell at any budget reaching the
precision ceiling, or the best cell's separation short of the floor's
by the smaller margin. `inconclusive` otherwise.

Three readings of the ratified text are taken and written on the
verdict itself under `readings`: the primary register is the
tripped-up register; the floor's separation is the floor's on the
same (set, scoring, gate) cell; a margin that cannot be computed
satisfies neither bound. The pre-gate sub-rule is applied per
contender cell at the same budget.

## Results

`verdict.json`, whose digest is `product_sha256` above and in the
summary row. No cell clears all three `supported` bounds at once: the
cells that reach the precision bound without over-firing do not put
the declared margin between their separation and the floor's on the
same cell. The best-separated cell is `bge-small-en-v1.5/softmax/without_gate`, and it
does clear the smaller `refuted` margin over the floor, while many
cells reach the precision ceiling, so neither `refuted` clause holds.

The pre-gate sub-rule: 0 contender cell(s) improved
by the margin, 7 were lower by it, out of
9 pairs, so the sub-verdict is `refuted`
(no cell improves by the margin and at least one is lower by it).

## Conclusion

`inconclusive` under the rule as ratified: the data did not
decide the primary claim. What is still unknown is stated on the
verdict: the pre-gate sub-verdict is `refuted`, and the
ratified text does not say how the two conjuncts of the hypothesis
combine into one word, so this directory's `result` is the main
rule's verdict and the sub-verdict rides beside it, disclosed rather
than folded in.
