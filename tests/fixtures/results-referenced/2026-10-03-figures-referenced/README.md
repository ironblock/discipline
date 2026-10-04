+++
hypothesis = "the figures a report states are references the linter renders from its product"
result = "supported"
kind = "historical-observation"
figures = "referenced"
fired = 2026-10-03
product_sha256 = "8d143d3448000bf023830f0de49fc0d4ca2dedbf71ebcc7cfafda0b3bf4b5124"
controls_run = []
known_defects = []
turns = 2
prefill_tokens_total = 2048
claim_issue = "63"
absent = { supersedes = "a constructed fixture, nothing superseded", rule_ratified = "a constructed fixture, no decision rule", window_start = "a constructed fixture, no window" }

[regime]
arm = "baseline"
substrates = ["local"]
dogma_version = 0
+++

# A referenced report

A synthetic results directory for `scripts/check-results.py`'s figure lint
(#63): every figure in its body is a reference the linter renders from the
product (`report.json`, found by `product_sha256`), the front-matter or the
record's summary row. It lives under `tests/fixtures/`, not `results/`; the
seeded cases copy it there and break one thing.

## Observation

A report's typed figures could drift from its product with every gate green
[uncited: the count of directories measured on #63, 2 at the time].

## Hypothesis

The figures a report states are references the linter renders from its
product.

## Test

The record holds {{summary.turns}} turns; nothing was fired
[uncited: this directory is constructed, 1 file of product].

## Results

The run recorded {{summary.turns}} turns and {{summary.prefill_tokens_total}}
prefill tokens. The decode rate was {{product.decode_rate}}, or
{{pct(product.decode_rate, 1)}} at one place, {{round(product.decode_rate, 2)}}
at two. Of {{product.of_steps}} steps, {{count(product.null_steps)}} were null;
the first was {{product.null_steps[0]}}.

## Conclusion

The claim is {{front.claim_issue}}'s.
Supported on {{front.fired}}: the figures above are the product's own.
