# How this repository is worked

*Who posts here and how to tell them apart, which file holds each rule, and what went wrong before. Most comments in this repository were written by model sessions acting for the maintainer under a prefix; this file says how to read them.*

*Compiled by planning on 2026-10-07 and adapted by Claude on 2026-10-08. No human has authored this text yet; the maintainer edits before it is relied on. Lines marked `[unsettled]` are proposals, not rulings.*

## Who posts

One GitHub identity (the maintainer's) posts for every session. Prefixes tell them apart:

- **`[Planning]`**: a long-lived chat session holding the program's whole context. It compiles drafts for this directory; the maintainer authors what they become (`AGENTS.md`, Documentation).
- **`[Dispatch]`**: an orchestrating Claude Code session. It routes tickets and decides how work is done.
- **Tracks**: builder sessions, each with a lane and a worktree.
- **The maintainer**: ratifies, runs windows on the boxes, drives sessions, and is the only one who sends anything outside the repository.

This describes who wrote the existing threads. It does not settle how many sessions should run at once (see Unsettled).

## Where each rule lives

A rule lives in the file its reader opens, once:

| rule | where |
| --- | --- |
| Principles, tickets, documentation, testing; GitHub issues are not documentation | `AGENTS.md` |
| Milestones, cuts, and backlog aging | `CONTRIBUTING.md`, Milestones |
| Branches, merging, review, hygiene, acceptance | `CONTRIBUTING.md` and `.github/PULL_REQUEST_TEMPLATE.md` |
| What CI runs | `.github/check-owners.tsv` and `verify.sh` (`DEFAULT_CHECKS`) |
| A results directory's contract | `results/_template/`, enforced by `scripts/check-results.py`; `results/AGENTS.md` |
| Claims: words, fields, post-hoc | `.github/ISSUE_TEMPLATE/claim.md`, `scripts/check-results.py`, `results/AGENTS.md` |
| The substrate ladder: rungs, admission, parked engines | `substrates/LADDER.md` |
| The system, its levers, what is known | `docs/program.md` |
| Experiments wanted, run and worded | `docs/experiments.md` |

The releases now open are v0.1.0 *the pieces work* (each piece shown live by the maintainer; a passing test does not count), v0.2.0 *the pieces together* (one session ending in a declared compaction) and v0.3.0 *T3, NetHack*. How milestones work is in `CONTRIBUTING.md`; where work that serves none of them goes is in `AGENTS.md`, Tickets.

## What went wrong before, so it is not repeated

A backlog of rigor outgrew the product three times: in this program's predecessor, in the September rigor era, and in the multi-session orchestration of early October. The mechanism was the same each time:
- a backlog is the agents' environment, and agents optimise it;
- a session cannot feel a two-hour CI run or a week without software;
- the controls' value was assumed, because measuring it was nobody's deliverable.

The cure each time was the same three things: a product definition stated as an observable, a fresh context with no inertia, and permission to say no.

The last instance is on record. On 2026-10-07 the seeded-fault selftest left CI (#506). Over five days it had made about five catches at 15–70 minutes a run, four of them its own upkeep. The conventional checks made about eleven in that time, at minutes a run (planning on #25, [comment 5976788461](https://github.com/ironblock/discipline/issues/25#issuecomment-5976788461)).

## Unsettled

Proposed by planning. None of them is a ruling until the maintainer makes it one.

- `[unsettled]` **Compaction at each cut.** At each cut, planning moves what the threads settled into these documents. (Settled since the draft: a ruling lives in a file where it is read, never only in an issue; see `AGENTS.md`.)
- `[unsettled]` **When orchestration runs.** Only against a milestone whose definition of done is something the maintainer did live:
  - two builders and Dispatch, one open PR per session, no stacked PRs;
  - no machinery tickets while a product milestone is open;
  - planning posts no ruling that moves no box;
  - a week with no box ticked pauses orchestration, and the maintainer runs a piece.
