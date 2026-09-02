+++
hypothesis = "State the claim being tested, in one sentence, so that it could be wrong."
result = "supported"
product_sha256 = "a4c8f7287a11a48f011777a9d8d7c9e446e08fbb2092c9ac7e7247057719f3a2"
controls_run = ["null-regimen"]
known_defects = []
turns = 2
prefill_tokens_total = 2048

[regime]
arm = "treatment"
substrate = "local"
dogma_version = 0
+++

# Template

Copy this directory to `results/YYYY-MM-DD-<slug>/`, where `<slug>` is the
claim's slug in the claim ledger. `scripts/check-results.py` lints it; the
copy must pass before it lands.

Every number in the front-matter, outside `[regime]`, must appear in
`run.jsonl`'s summary record. `[regime]` must agree with `regimen.toml`.
That is the whole point: prose is verified against data, never merely written.

## Observation

What was seen that prompted the test. No interpretation.

## Hypothesis

The claim, stated so that a run could falsify it. Repeat the front-matter
`hypothesis` verbatim.

## Test

The regimen, the arms, the controls, and the command that produces
`run.jsonl`. Someone else must be able to re-run it from this section alone.

## Results

What the run recorded. Numbers here must be the numbers in `run.jsonl`.

## Conclusion

Whether the hypothesis survived, and what is still unknown. Brief by design.
