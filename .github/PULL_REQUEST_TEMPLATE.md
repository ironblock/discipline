## What changed

<!-- The change, in a sentence or two. Point at the lines that matter.
     Nothing from a private environment, here or anywhere on the thread:
     hostnames, addresses, home paths, internal ticket identifiers -- and
     a person's schedule, habits or whereabouts are private too: a machine's
     availability is a fact about the machine ("reserved", "down", "restored
     at <time>"), never about a person's time. -->

## Why

<!-- The problem this solves, or the claim it serves. Link the issue.
     A chore has no issue: this section is its ticket. -->

## Scope

<!-- REQUIRED. Paste what `python3 scripts/pr-scope.py --base origin/HEAD`
     prints: `material` or `chore`, why, and the checks the diff touches.
     The diff decides -- never the branch name, never a label (#276).

     A CHORE lands something that changes no gate input, no record, no claim
     and no protocol. It owes: CI green on the head; `./verify.sh --only` the
     checks pr-scope names, with their exit codes, below; one fresh-instance
     review posted as a review event, under the chore brief. It does not owe
     an issue's rows, `--selftest`, or ruled disclosures. Its owner merges;
     Dispatch arms nothing. A chore cannot be what pr-scope calls material,
     fail hygiene or history, or land without its review event.

     THE CHORE BRIEF, for its reviewer: hygiene of everything added, a
     person's schedule, habits or whereabouts among it, which no pattern
     catches; the licensing and provenance of every asset -- the tool that
     made it, the tool's inputs, any embedded font or third-party element, and
     the licence the repository may carry it under; and every rendering claim
     either checked on a device or stated as unseen. -->

## Acceptance

<!-- REQUIRED. Commands and the exit codes they produced on this branch.
     Re-execute; do not re-read. A verdict through a grep is not a gate.
     A chore's rows are `./verify.sh --only <each check pr-scope names>`. -->

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
     job of whoever owns the PR. The base branch is the integration branch
     (the repository's default); a release PR, from it into the release
     branch, is held to the same five, and its selftest is the full set, not
     a scoped one (CONTRIBUTING.md, "Branches and releases"). -->

<!-- A chore owes the first two boxes and the hygiene and history checks; the
     rest are a material PR's. Mark the others "n/a: chore". For a chore, the
     second box's deferred findings go under "Notes". -->

- [ ] **CI is green on the head commit, including the `selftest` job.** Not
      "green when I pushed": green on what is about to merge, into the base
      branch this PR names.
- [ ] **A fresh-instance review is recorded on this thread**, and every finding
      it raised is either fixed, refuted with a command and an exit code, or
      listed under *Known defects* with the reason it is deferred.
- [ ] **The acceptance table above cites the issue's own rows**, with the
      command and the exit code it produced here. Re-executed, not re-read.
- [ ] **If this PR touched `verify.sh` or `tools/gate/faults.toml`:**
      `./verify.sh --only injections` exits 0. A merge resolved line by line
      splices injection bodies into each other and empties them silently; the
      resolution is by NAME, and that check is what proves the result.
- [ ] **Every decision this PR disclosed has been ruled on this thread, AND
      the body's disclosure section names the outcome of each.** A question
      asked in a PR body and never answered is a decision made by whoever
      merges, silently. A question that HAS been answered and is still listed
      as open is worse: it reads as waiting when it is late, and a fresh
      reviewer correctly leaves it alone. #69 lost a day to exactly that --
      two rulings sat unimplemented through two review rounds because the body
      still called them open. So a ruled disclosure is EDITED to say where it
      ended up, not left as it was asked.

## Known defects

<!-- What is still wrong after this lands. Empty is a claim.
     A chore heads this section "Notes" instead, and it may be empty. -->
