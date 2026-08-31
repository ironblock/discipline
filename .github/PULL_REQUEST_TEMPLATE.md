## What changed

<!-- The change, in a sentence or two. Point at the lines that matter. -->

## Why

<!-- The problem this solves, or the claim it serves. Link the issue. -->

## Acceptance

<!-- REQUIRED. Commands and the exit codes they produced on this branch.
     Re-execute; do not re-read. A verdict through a grep is not a gate. -->

| command | exit code |
| ------- | --------- |
| `./verify.sh` | |
| `./verify.sh --selftest` | |

## Gates touched

<!-- If this PR changes a gate, say which, and how you saw the new gate red.
     A gate that has never been seen red is not a gate. -->

- [ ] This PR adds or changes a gate, and `./verify.sh --selftest` covers it.
- [ ] This PR adds a results directory, and `check-results.py` exits 0 on it.
- [ ] Neither.

## Known defects

<!-- What is still wrong after this lands. Empty is a claim. -->
