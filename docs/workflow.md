# How this repository is worked

*Who posts here and how to tell them apart, which file holds each rule, and what went wrong before. Most comments in this repository were written by model sessions acting for the maintainer under a prefix; this file says how to read them.*

*A line marked `[unsettled]` is a proposal, not a ruling. Who may change which line is in [`AGENTS.md`](AGENTS.md).*

## Who posts

One GitHub identity (the maintainer's) posts for every session. Prefixes tell them apart:

- **`[Dispatch]`**: an orchestrating Claude Code session. It routes work, decides how it is done, and writes the maintainer's rulings into the file where they are read.
- **Builders** (`[Track …]` in older threads): sessions that each hold a worktree and one pull request.
- **The maintainer**: names what a release means, ratifies thesis lines (`docs/AGENTS.md`), runs windows on the boxes, drives sessions, and is the only one who sends anything outside the repository.

**Planning is off GitHub and off the code** from 2026-10-08. It is the maintainer's ideation chat; what comes of it reaches this repository only as the maintainer's words, in a file. `[Planning]` comments before that date were written by it while it posted here.

## Where each rule lives

A rule lives in the file its reader opens, once:

| rule | where |
| --- | --- |
| Principles, tickets, documentation, testing; GitHub issues are not documentation | `AGENTS.md` |
| Milestones, cuts, and backlog aging | `CONTRIBUTING.md`, Milestones |
| What each open release means, and its evidence | `docs/releases.md` |
| A trajectory's script | `docs/trajectories/` |
| Who may change which line of these documents | `docs/AGENTS.md` |
| Branches, merging, review, hygiene, acceptance | `CONTRIBUTING.md` and `.github/PULL_REQUEST_TEMPLATE.md` |
| What CI runs | `.github/check-owners.tsv` and `verify.sh` (`DEFAULT_CHECKS`) |
| A results directory's contract | `results/_template/`, enforced by `scripts/check-results.py`; `results/AGENTS.md` |
| Claims: words, fields, post-hoc | `.github/ISSUE_TEMPLATE/claim.md`, `scripts/check-results.py`, `results/AGENTS.md` |
| The substrate ladder: rungs, admission, parked engines | `substrates/LADDER.md` |
| The system, its levers, what is known | `docs/program.md` |
| Experiments wanted, run and worded | `docs/experiments.md` |

## What went wrong before, so it is not repeated

A backlog of rigor outgrew the product three times: in this program's predecessor, in the September rigor era, and in the multi-session orchestration of early October. The mechanism was the same each time:
- a backlog is the agents' environment, and agents optimise it;
- a session cannot feel a two-hour CI run or a week without software;
- the controls' value was assumed, because measuring it was nobody's deliverable.

The cure each time was the same three things: a product definition stated as an observable, a fresh context with no inertia, and permission to say no.

The last instance is on record. On 2026-10-07 the seeded-fault selftest left CI (#506). Over five days it had made about five catches at 15–70 minutes a run, four of them its own upkeep. The conventional checks made about eleven in that time, at minutes a run (planning on #25, [comment 5976788461](https://github.com/ironblock/discipline/issues/25#issuecomment-5976788461)).

## How work is orchestrated

Ruled by the maintainer on 2026-10-08:

- **Work comes from the earliest open release** (now v0.1.0). A builder is given a ticket only if it is on the path to a row of that release in `docs/releases.md` with no evidence yet. A ticket in its milestone that is on no row's path waits until no ticket on a path is open.
- **At most two builders and Dispatch.** One open pull request per builder. No stacked pull requests.
- **A finding that is on no row's path is not worked.** Where it goes is in `CONTRIBUTING.md` (review). No session files tickets speculatively.
- **A ruling lives in a file.** One made in chat or in a comment is written into the file where it is read, by the next pull request that touches that file or by one of its own, which the maintainer merges (`AGENTS.md`).

## Unsettled

- `[unsettled]` A week in which no row gains evidence pauses orchestration, and the maintainer runs a piece. (Proposed by planning.)
