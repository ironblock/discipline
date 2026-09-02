---
name: Claim
about: A hypothesis, the regime it was tested under, and how to reproduce it.
title: "claim: "
labels: claim
---

## Hypothesis

<!-- One sentence, stated so that a run could falsify it. -->

## Result

<!-- supported | refuted | inconclusive -- and nothing else on this line. -->

## Regime

| field           | value |
| --------------- | ----- |
| `arm`           |       |
| `substrate`     |       |
| `dogma_version` |       |

## Controls run

<!-- Every control this claim was run against. "none" is an answer, and a
     costly one. -->

## Reproduction

```
# The commands, in order, that produce run.jsonl from a clean checkout.
```

## Results directory

<!-- results/YYYY-MM-DD-<slug>/, where <slug> matches this issue's ledger row.
     `python3 scripts/check-results.py --root results` must exit 0. -->

## Known defects

<!-- What is wrong with this claim that you already know about. Empty is a
     claim; empty and untrue is a defect. -->
