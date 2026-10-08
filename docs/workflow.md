# How this repository is worked

*Who posts here and how to tell them apart, which file holds each rule, and what went wrong before. Most comments in this repository were written by model sessions acting for the maintainer under a prefix; this file says how to read them.*

*Compiled by planning on 2026-10-07 and adapted by Claude on 2026-10-08. No human has authored this text yet; the maintainer edits before it is relied on. Lines marked `[unsettled]` are proposals, not rulings.*

## Who posts

One GitHub identity (the maintainer's) posts for every session. Prefixes tell them apart:

- **`[Planning]`**: a long-lived chat session holding the program's whole context. It writes the synthesis in this directory.
- **`[Dispatch]`**: an orchestrating Claude Code session on the maintainer's Mac. It routes tickets and decides how work is done.
- **Tracks**: builder sessions, each with a lane and a worktree.
- **The maintainer**: ratifies, runs windows on the boxes, drives sessions, and is the only one who sends anything outside the repository.

This describes who wrote the existing threads. It does not settle how many sessions should run at once (see Unsettled).

## Where each rule lives

A rule lives in the file its reader opens, once:

| rule | where |
| --- | --- |
| Principles, tickets, documentation, testing | `AGENTS.md` |
| Branches, merging, review, hygiene, acceptance | `CONTRIBUTING.md` and `.github/PULL_REQUEST_TEMPLATE.md` |
| What CI runs | `.github/check-owners.tsv` and `verify.sh` (`DEFAULT_CHECKS`) |
| A results directory's contract | `results/_template/`, enforced by `scripts/check-results.py`; `results/AGENTS.md` |
| Claims: pre-registration, ratification, words | `docs/experiments.md`, "Claims" |
| The system, its levers, what is known | `docs/program.md` |
| Experiments wanted, run and worded | `docs/experiments.md` |

**Milestones are releases, in cut order:**
- v0.1.0 *the pieces work*: each piece shown live by the maintainer; a passing test does not count.
- v0.2.0 *the pieces together*: one session ending in a declared compaction.
- v0.3.0 *T3, NetHack*.

Work that serves none of them goes to a `Later —` milestone naming the condition on which it returns (#495). Programs are labels (`program:claims`, `program:dogma`, `program:gate`), never milestones.

## What went wrong before, so it is not repeated

A backlog of rigor outgrew the product three times: in diet-inference, in the September rigor era, and in the multi-session orchestration of early October. The mechanism was the same each time:
- a backlog is the agents' environment, and agents optimise it;
- a session cannot feel a two-hour CI run or a week without software;
- the controls' value was assumed, because measuring it was nobody's deliverable.

The cure each time was the same three things: a product definition stated as an observable, a fresh context with no inertia, and permission to say no.

The last instance is on record. On 2026-10-07 the seeded-fault selftest left CI (#506). Over five days it had made about five catches at 15–70 minutes a run, four of them its own upkeep. The conventional checks made about eleven in that time, at minutes a run (planning on #25).

## Unsettled

Proposed by planning. None of them is a ruling until the maintainer makes it one.

- `[unsettled]` **Where rulings live.** A ruling exists when it is a line in `docs/`; the thread comment points at the line. At each cut, planning compacts the threads that moved into these documents.
- `[unsettled]` **Ticket aging.** Unplaced tickets age out after two cuts. ("Cut" needs a definition first.)
- `[unsettled]` **When orchestration runs.** Only against a milestone whose definition of done is something the maintainer did live:
  - two builders and Dispatch, one open PR per session, no stacked PRs;
  - no machinery tickets while a product milestone is open;
  - planning posts no ruling that moves no box;
  - a week with no box ticked pauses orchestration, and the maintainer runs a piece.
