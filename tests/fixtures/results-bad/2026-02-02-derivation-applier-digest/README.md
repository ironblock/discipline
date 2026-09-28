+++
hypothesis = "State the claim being tested, in one sentence, so that it could be wrong."
result = "supported"
kind = "historical-observation"
product_sha256 = "9d5746195548e47b0c5d4391b2599c221535b245a74f1461c70fdcce54498baa"
controls_run = []
known_defects = []
turns = 2
prefill_tokens_total = 2048

[regime]
arm = "baseline"
substrates = ["served"]
dogma_version = 0

[derivation]
applier_sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "9d5746195548e47b0c5d4391b2599c221535b245a74f1461c70fdcce54498baa"
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
