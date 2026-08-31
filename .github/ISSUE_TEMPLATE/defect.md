---
name: Defect
about: Something is wrong. Acceptance is a command and its exit code.
title: "defect: "
labels: defect
---

## Reproduction

```
# The shortest command sequence, from a clean checkout, that shows the defect.
```

## Expected

<!-- What should have happened. -->

## Observed

<!-- What happened instead. Paste the output and the exit code, not a summary
     of them. -->

| | |
| --- | --- |
| command | `` |
| exit code | |

## Acceptance

<!-- REQUIRED. A command and the exit code it must produce once this is fixed.
     "The tests pass" is not an acceptance criterion; a command is.

     Example:
       ./verify.sh --only results        exit 0
       python3 scripts/check-results.py tests/fixtures/results-bad/<case>  exit 1
-->

| | |
| --- | --- |
| command | `` |
| required exit code | |

## Environment

<!-- Only what bears on the defect. Nothing from a private environment: the
     hygiene gate rejects hostnames, addresses and home paths for a reason. -->
