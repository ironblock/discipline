+++
hypothesis = "State the claim being tested, in one sentence, so that it could be wrong."
result = "supported"
kind = "reproducible-by-config"
figures = "referenced"
product_sha256 = "b491f615ad737993319517a59c5d450c320ea7d20358e4787e41526baae05182"
controls_run = ["null-regimen"]
known_defects = []
turns = 2
prefill_tokens_total = 2048

[regime]
arm = "baseline"
substrates = ["local"]
dogma_version = 0
+++

# Template

Copy this directory to `results/YYYY-MM-DD-<slug>/`, where `<slug>` is the
claim's slug in the claim ledger. `scripts/check-results.py` lints it; the
copy must pass before it lands.

Every number in the front-matter, outside `[regime]`, must appear in
`run.jsonl`'s summary record. `[regime]` must agree with `regimen.toml`.
That is the whole point: prose is verified against data, never merely written.

Every figure in the body is a reference, not digits (#63): `{{product.<path>}}`
(the one file here whose sha256 is `product_sha256`), `{{front.<key>}}` or
`{{summary.<path>}}`, or `count()`, `round(…, n)` or `pct(…, n)` of one. The
linter renders them from the data at check time; `--render DIR` prints the
result. Results and Conclusion carry no typed figure; Observation, Hypothesis
and Test may carry one only inside `[uncited: <reason>]`.

## Observation

What was seen that prompted the test. No interpretation.

## Hypothesis

The claim, stated so that a run could falsify it. Repeat the front-matter
`hypothesis` verbatim.

## Test

The regimen, the arms, the controls, and the command that produces
`run.jsonl`. Someone else must be able to re-run it from this section alone.

## Results

What the run recorded, every figure a reference to the field that holds it.

## Conclusion

Whether the hypothesis survived, and what is still unknown. Brief by design.
