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

## Merge checklist

<!-- Five things, each of which is VISIBLE on this pull request before it is
     merged. A reviewer should be able to tick every box by reading the thread
     and the checks tab, without asking anyone what happened. Merging is the
     job of whoever owns the PR. -->

- [ ] **CI is green on the head commit, including the `selftest` job.** Not
      "green when I pushed": green on what is about to merge.
- [ ] **A fresh-instance review is recorded on this thread**, and every finding
      it raised is either fixed, refuted with a command and an exit code, or
      listed under *Known defects* with the reason it is deferred.
- [ ] **The acceptance table above cites the issue's own rows**, with the
      command and the exit code it produced here. Re-executed, not re-read.
- [ ] **If this PR touched `verify.sh` or `tools/gate/faults.toml`:**
      `./verify.sh --only injections` exits 0. A merge resolved line by line
      splices injection bodies into each other and empties them silently; the
      resolution is by NAME, and that check is what proves the result.
- [ ] **Every decision this PR disclosed has been ruled on this thread.** A
      question asked in a PR body and never answered is a decision made by
      whoever merges, silently.

## Known defects

<!-- What is still wrong after this lands. Empty is a claim. -->
