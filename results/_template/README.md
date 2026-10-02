+++
hypothesis = "State the claim being tested, in one sentence, so that it could be wrong."
result = "supported"
kind = "reproducible-by-config"
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

A copy of this template must also give the claim's provenance (#32), each
field at the top level or named in `absent = { field = "why it is absent" }`:
`claim_issue` (the issue's number as a string of digits), `supersedes` (the
64-hex digest of the product this one replaces), `rule_ratified = { comment,
at, digest, of }` (the maintainer's ratifying comment id, its UTC time, and
the sha256 of the file `of` names -- `decision-rule.toml` unless `of` says
otherwise) and `window_start` (UTC), with `window_start_from` naming where
that time was read. `rule_ratified_note` carries a caveat the three values
cannot. Whether the rule is post-hoc is derived -- ratified after the window
opened -- and never written.

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
