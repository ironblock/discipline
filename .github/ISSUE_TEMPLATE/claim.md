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

<!-- Nothing from a private environment: the hygiene gate rejects hostnames,
     addresses, home paths and internal ticket identifiers for a reason, and
     a person's schedule, habits or whereabouts are private too: a machine's
     availability is a fact about the machine ("reserved", "down", "restored
     at <time>"), never about a person's time. -->

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
