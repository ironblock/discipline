# How this repository is worked

*Who posts here and how to tell them apart, which file holds each rule, and what went wrong before. Most comments in this repository were written by model sessions acting for the maintainer under a prefix; this file says how to read them.*

*Who may change which line is in [`AGENTS.md`](AGENTS.md).*

## Who posts

One GitHub identity (the maintainer's) posts for every session until the agents have their own account, which the maintainer approved on 2026-10-09. Prefixes tell the sessions apart:

- **`[Dispatch]`**: the orchestrating Claude Code session. It decides how work is done and who does it, carries questions and results to the maintainer, and writes his answers where they are read.
- **Builders** (`[Track …]`): sessions that each hold a worktree and one pull request at a time, and follow Dispatch's lead.
- **The maintainer** approves every significant decision and result, in any form, and is otherwise not hands-on. He runs windows on the boxes and is the only one who sends anything outside the repository.

**Planning is off GitHub and off the code** from 2026-10-08. It is the maintainer's ideation chat; what comes of it reaches this repository only through him. `[Planning]` comments before that date were written by it.

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

**The maintainer, 2026-10-09:**
- "Any significant decision or result needs my approval in some form." Knowing the program runs this way, any ratified outcome came through him. Nothing is done in his name.
- "The priority now, above all else: get a working version of the system that exercises the core idea. No constraints, restrictions, or 'correctness' obstacles." The initiatives are in `releases.md`.
- Sessions that know why a punted item mattered give him the data and the perspective; they do not put it back on the map themselves.
- "All questions like this should follow the conventions of other harnesses, such as they may exist." A question about how the harness behaves (what a cancel keeps, how a tool is shaped, what an instruction file does) follows the other harnesses' convention, from `docs/harness-baseline.md` and their sources, without asking him. It is decided by a vote: where OpenCode 2 and Pi agree, that is the default; where they disagree, Qwen Code breaks the tie (the maintainer, 2026-10-09). Only where none of them has a convention is it his question.

**Dispatch's practice, which it may change:**
- Work comes from the earliest open milestone of the prototype initiative, and every session may take some. A blank in what a lever means goes to the maintainer as a question; work that depends on it waits, and other work goes on.
- A question to the maintainer is answerable from a phone in seconds: the recommendation, why, and what yes and no each do. Silence is not a yes.
- One open pull request per session, branched from `develop`. No stacked pull requests.
- A finding on no milestone's path is not worked; where it goes is in `CONTRIBUTING.md` (review).
