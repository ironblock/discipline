+++
hypothesis = "Authored sense-descriptions match concrete transcript sentences sharply enough to prioritise capture without over-firing, and a lexical pre-gate improves precision at a fixed budget."
result = "inconclusive"
kind = "reproducible-by-config"
product_sha256 = "813afcc5f8093005db283d6d9e98b534d902e3cfc25fd8cbd1b4c81152cf2a92"
controls_run = []
known_defects = []
targets_checked = 2
targets_matched = 2

[regime]
arm = "sense-bakeoff-four-embedders"
substrates = ["all-MiniLM-L6-v2", "bge-small-en-v1.5", "embeddinggemma-300m", "Qwen3-Embedding-0.6B"]
dogma_version = 0

[derivation]
applier_sha256 = "475cef5b87bc3fd7d2e897cebe585ba7c3de01336cb109d910071948ee99b1a0"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "eac8640d9238471698a848a4c594b26e7e3f23bada875cf19475d9c5017f6789"
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

Seven readings of the ratified text are taken and written on the
verdict itself under `readings`: which register is primary; whose
cell the floor's separation is read from; what a margin that cannot
be computed satisfies; which cell is "the best cell"; what a cell is;
the pre-gate sub-rule's scope; and its quantifier. Where a reading
turns the verdict, the verdict under the other reading is computed
and carried under `alternatives`, so a ruling is a measured delta.

The arithmetic's environment is the `[derivation]` block in the
front-matter, not this prose (ruled on #84): the applier's digest,
which is `recompute.sh`'s own; the runtime that applied the rule; the
substrate it ran on, by registry id; and the original, cited by the
digest of its product, which is the digest the claim row consumes.
The record's start row stays the run being adjudicated, per the same
ruling: the regime is what was measured, and an adjudication measures
nothing.

## Results

`verdict.json`, whose digest is `product_sha256` above and in the
summary row. No cell clears all three `supported` bounds at once: the
cells that reach the precision bound without over-firing do not put
the declared margin between their separation and the floor's on the
same cell. The best-separated cell is `bge-small-en-v1.5/softmax/without_gate`, and it
does clear the smaller `refuted` margin over the floor, while many
cells reach the precision ceiling, so neither `refuted` clause holds
under the reading that "the best cell" is the best-separated one.
Under the reading that it is the best cell on the primary endpoint,
two cells tie at the top precision and one of them sits under the
floor: if every tied cell must clear the margin the verdict is
`refuted`, and if the tie is broken by separation it is
`inconclusive`. Ruled on #84: "the best cell" in a clause about
separation is the best-separated cell, the reading that needs no rule
beyond the text; both readings stay on the verdict, and rule v2 is
worded as the maximum separation over all scored cells.

The pre-gate sub-rule: 0 contender pair(s) improved
by the margin, 7 were lower by it, out of
9 pairs, so the sub-verdict is `refuted`
(no cell improves by the margin and at least one is lower by it); under the reading that every pair must be lower
it is `inconclusive`.

## Conclusion

`inconclusive` under the rule as read here: the data did not
decide the primary claim; the reading of "the best cell" under which
it would be `refuted` is carried, and the ruling on #84 records that
the claim is not supported under either. What is still unknown is stated on the
verdict: the pre-gate sub-verdict is `refuted`, and the
ratified text does not say how the two conjuncts of the hypothesis
combine into one word, so this directory's `result` is the main
rule's verdict and the sub-verdict rides beside it, disclosed rather
than folded in.
