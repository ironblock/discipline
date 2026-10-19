# Releases

*The program's initiatives, the milestones (epics) inside each, what each open milestone means as an observable, and the evidence that it happened. A definition or row changes only with the maintainer's approval ([`AGENTS.md`](AGENTS.md)). How milestones and tags work is in `CONTRIBUTING.md`, Milestones.*

## Initiatives

The maintainer, 2026-10-09: "Working prototype _with every theorized lever present_ → controls and reproducible tests → rigor, test matrix, data collection → publish." Each is an initiative, run in that order, and holds as many milestones as it needs; nothing in a later initiative is an obstacle to an earlier one.

1. **The prototype:** every theorized lever (`program.md`, The levers) present and working. v0.1.0 is its first milestone, v0.2.0 the remainder of the levers, v0.3.0 sessions an agent can drive end to end, v0.4.0 phased operation (both the maintainer, 2026-10-11), and v0.5.0 the hosted API (approved by the maintainer, 2026-10-10).
2. **Controls and reproducible tests:** v0.8.0, runs re-fire, and v0.9.0, every lever has its control (drafted by Dispatch; approved by the maintainer, 2026-10-14).
3. **Rigor:** the test matrix and data collection.
4. **Publish.**

## v0.1.0 — the pieces work

The prototype's first milestone. Each piece of the harness shown working **once, live, on the floor**. A passing test does not count. Using the pieces together is v0.6.0's.

**Floor:** the 3.8 EXL3 line, config r2, on linux-pc (`accel24-tabbyapi-exl3-qwen38-27b-3p00`, #497), driven through TabbyAPI (#496). The 3.6 is retired.

**Evidence:** four agent-driven runs under `runs/`, each row's approved by the maintainer (2026-10-10). Finishing T1 ([`trajectories/t1.md`](trajectories/t1.md)) moved to v0.6.0: at `xhigh` its script does not fit the floor's window without a seam ([`2026-10-10-t1-run-3-agent-driven`](../runs/2026-10-10-t1-run-3-agent-driven)).

| piece | shown when | evidence |
| --- | --- | --- |
| Ask | an ask streams (reasoning, then the answer) and settles; the log has the response row | [`2026-10-10-t1-run-3-agent-driven`](../runs/2026-10-10-t1-run-3-agent-driven), turns 1–2 |
| Tools, unprompted | the model runs a shell command under Seatbelt without being told to, reads a file outside the worktree (the Babylon Lite source), and writes or edits a file inside it | [`2026-10-10-t1-run-3-agent-driven`](../runs/2026-10-10-t1-run-3-agent-driven): 117 shell commands under Seatbelt in serve's `--worktree`; the reference source read outside it; 10 writes and 25 edits inside it |
| Image | an attached screenshot PNG, and a reply that shows the model saw it | [`2026-10-10-image-smoke-agent-driven`](../runs/2026-10-10-image-smoke-agent-driven): the model read a random number and colour, drawn on a page it never opened, off the screenshot alone |
| Interview fork | a fork in the idle gap lands a patch that shows in the working-memory panel | [`2026-10-10-seam-smoke-run-1-agent-driven`](../runs/2026-10-10-seam-smoke-run-1-agent-driven): the v4 fork's patches after both scoping asks |
| Seam | a declared seam renders, refills from working memory, and one ask after it settles | [`2026-10-10-seam-smoke-run-1-agent-driven`](../runs/2026-10-10-seam-smoke-run-1-agent-driven) |
| Cancel and end | an in-flight call is cancelled; the session ends with confirm and serve exits | [`2026-10-10-t1-run-3-agent-driven`](../runs/2026-10-10-t1-run-3-agent-driven), turn 3: a hung command cancelled, the turn settled `cancelled` |
| Record | the log parses, the record is projected from it and names the engine that ran, the receipt renders, and the run directory is committed, digest-pinned and hygiene-scanned | [`2026-10-10-seam-smoke-run-3-agent-driven`](../runs/2026-10-10-seam-smoke-run-3-agent-driven): 261 rows, 10 of 10 turn rows, `diet check-record` accepts it |

A row's evidence is a run directory under `runs/`, or a line in one, filled from the recording: a driven session kept as evidence that a piece works, not a claim about the program, which belongs in `results/` (the maintainer, 2026-10-10). Its README says truthfully who drove the run, the maintainer or an agent. A row counts once the maintainer approves its evidence. The milestone is done when every row counts.

**Done:** every row counts (the maintainer, 2026-10-10: "Agree to all"). The milestone closes when v0.1.0 is tagged.

## v0.2.0 — every lever live

The prototype's second milestone. Each lever in `program.md` §2 shown working **once, live, on the floor**, its state named on the record's start row. The rows are the levers themselves; a lever counts once the maintainer approves its evidence. Agent-driven runs count, labelled as such (the maintainer, 2026-10-10: "Deltas between what I do and what agents do will be interesting data"). Wording a lever needs is approved by the maintainer before it ships (`AGENTS.md`).

One working state per lever, a non-default one where the default is off, none or total; every state is the test matrix's (initiative 3).

| lever | state shown | evidence |
| --- | --- | --- |
| compaction depth | a tail, `seam_tail_tokens` 2000 | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): seam 4039 carried 1 turn. At seam 2804 it carried none, the newest turn being larger than *N*; the zero-turns warning drawn at two meditations of [`gate-and-tail`](../runs/2026-10-10-gate-and-tail-agent-driven) |
| seam trigger | budget | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): seam 2804, `reason: budget` |
| fork warrant | one per gap, gated on scoping | [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven): forks 860 and 2141 |
| fork asks | v4 | [`levers-run-c`](../runs/2026-10-11-levers-run-c-agent-driven): fork 4988 answered SUPERSEDE, and patch 5550 superseded `interview-t1-f3970-0` with the new decision. Earlier, [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven)'s one SUPERSEDE was misread (3188, #648) |
| self-capture | on, a reminder after 3 silent turns | [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven) and [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven): captures and the reminder |
| capture modality | tools, with ask set t1 | [`levers-run-b`](../runs/2026-10-10-levers-run-b-agent-driven): fork 608 answered through `update_record` |
| model-elected pruning | — | moved to Later — lever candidates (#710; the maintainer, 2026-10-10) |
| fork delivery of results | advisory | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): `delivered` 4042 and 5818 |
| tool-output disposition | cap on arrival; `reference` at the seam | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): call 2592 capped; both seams listed outputs by path |
| isolation | `sandbox` | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven): a Seatbelt refusal at 12815 |
| approval | none | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) |
| reasoning state | on, `xhigh` | [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven) and [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) |
| cache lifetime (hosted) | — | v0.5.0's: every paid call needs the maintainer's approval |
| substrate rung | the floor | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) |
| tool surface | standard | [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven), [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) and [`image-smoke`](../runs/2026-10-10-image-smoke-agent-driven) |
| instruction files | on, an AGENTS.md in the worktree | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): all 8 trunk answers followed it |
| tangent closure | closed, its entry parked | [`levers-run-b`](../runs/2026-10-10-levers-run-b-agent-driven): 4 messages rolled back, the prefix intact (1399) |
| interview routing and cadence | per class, skipping self-recorded turns | [`levers-run-c`](../runs/2026-10-11-levers-run-c-agent-driven): seven forks triggered by the model's edits (`call:edit:…`, 1346 to 4705) beside the turn-end scoping forks; the skip in [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven), `fork.skipped` at 14819 |
| render budget | tier | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): `render_reduced` 1 and 2. The render stayed over its budget after reducing (v0.2.0's own fix) |
| fork memory share | displacement logged | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): forks 2439 and 4952, `may-displace-trunk-cache`. No clamp or refusal was provoked |
| archive recall | literal | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven): `recalled` at 10396 and three more |
| fork input view | last turn | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): forks 2439 and 4952 |
| interview role | user | the start rows of [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven) and [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven). System and developer do not render in the floor's chat template |
| step and output limits | `max_steps` 6 | [`levers-run-b`](../runs/2026-10-10-levers-run-b-agent-driven): turn 3 stopped at `max_steps` (814) |
| failed turns on the trunk | kept | [`levers-run-b`](../runs/2026-10-10-levers-run-b-agent-driven): turn 4 cancelled after 3 steps, and the next request only appended |
| reconcile gate | `refuse` | [`levers-run-c`](../runs/2026-10-11-levers-run-c-agent-driven): a flag on a still-true decision (5552) kept by the audit (5555) and its re-ask (9776), held (`meditation.held` 10034, an ask refused `meditation-held` at 10035), then refilled anyway (10038). Holding needs #748's rule: an audit that keeps a flagged note leaves it unaddressed |
| subagent | — | later |

Every row's evidence is approved (the maintainer, 2026-10-10: "approve all, proceed"). The reconcile gate's row and the zero-turns warning are approved too, as written (the maintainer, 2026-10-10). The extraction seat is struck (#570). The milestone closes with its open issues and the v0.2.0 tag.

**Corrected 2026-10-11.** An adversarial review of the evidence found three rows that did not show what they said: fork asks, interview routing and cadence, and the reconcile gate. Each was reworded to what its log shows (the maintainer approved the rewording, 2026-10-11), and all three were then shown by [`levers-run-c`](../runs/2026-10-11-levers-run-c-agent-driven), driven after the tag on #744's and #748's fixes (the maintainer approved the evidence, 2026-10-11). Pruning, cache lifetime and subagent remain not shown, as deferred.

**v0.2.1** (2026-10-11): the fixes from an adversarial review of the work merged since 2026-10-08:
- the review's ten issues, #728 to #737;
- #748, the maintainer's ruling that an operator's change is addressed only when the model acted on it;
- #749, the surface's follow-up to #748;
- the evidence above, corrected and re-shown (#747, #750, #755).

It also carries what merged after v0.2.0's tag:
- BEGIN on today's floor, with the product on (#720, #722, #723);
- serve serving the page on its own origin (#716);
- sidechains as lanes in the Claude Code adapter (#708);
- a capture row's count (#443).

## v0.3.0 — driveable

An agent drives a served session end to end, and a drive can be run again. The maintainer, 2026-10-11, pulling it forward: his ability to run sessions is always the limiting factor. The work follows the surface seat's friction list from driving every run under `runs/`, and is split by whose concern it is: the session's on diet's side, the operator's page on exercise's.

- **The session's state, readable:** `GET /state`, a `/wait` long-poll and `GET /memory` (#756).
- **A run assembled, not transcribed:** the log names its build; one command scrubs, checks and digests a run directory and writes its README skeleton; the seqs a README cites come from a query (#757).
- **Driven through tools:** an MCP server over serve's operator commands, so a Claude.app session drives a session without a browser (#758).
- **The page operable by keyboard and by an agent** (#759).
- **Scenarios:** a drive written as steps and expectations, run on the stand-in in CI and re-fired on a named endpoint by one command (#760).

**Done when** levers run C's three levers, written as a scenario, pass on the stand-in in CI; and the same scenario, re-fired on the floor through the runner, lands as an assembled run under `runs/`.

**Done:** the scenario passes on the stand-in in CI (`diet/scenarios/levers-run-c.jsonl`, #777), and its re-fire on the floor is [`2026-10-12-levers-run-c-refire-agent-driven`](../runs/2026-10-12-levers-run-c-refire-agent-driven) (#811): every expectation met, assembled and checked. The maintainer approved it on 2026-10-12.

**v0.3.0** (2026-10-12): the milestone's issues, #756–#760, #717, #766, #772, and #780's fix to MCP `events`. It also carries the v0.4.0 work merged so far:
- phases as documents, two keys, the page and MCP (#787–#790);
- capture during the turn (#782–#784);
- what the phased T1 retries found (#797, #806, #813, #814, #821, #822, #826);
- the kitchen-sink scenario (#798, part 1).

v0.4.0 is not done: its done-when, T1 in phases on the floor reaching step 8, is being driven.

## v0.4.0 — phased operation

Structure for an autonomous model, enforced classically: Superpowers-like ground rules without its token weight (the maintainer, 2026-10-11). Agent-driven T1 on the floor died at step 4 when one build turn filled the window with working memory empty (the T1 smoke run, 2026-10-11); phases give that turn a checkpoint, and capture during the turn gives the checkpoint something to refill from.

- **Phases as documents:** TOML in the user's library, each with its ground rules and suggested or allowed transitions; a starting phase by default or by choice, a quick-task phase with none. At a meditation the system prompt becomes the head plus the phase's ground rules, the only time it changes (#787).
- **Two keys:** the model or the operator proposes a move, the other accepts, the operator may move alone; a model's proposal ends its turn; the operator rules meditate-and-move, keep or stay, shown the evidence (#788), on the page (#789) and over MCP (#790).
- **Capture during the turn:** interviews cut at a step run in the spare slot while the trunk goes on (#782); interviews warranted by what a step produced, the predecessor's evidence, trouble and open triggers (#783); mechanical capture of the files each step touched (#784).
- **What the phased retries found** (agent-driven T1 on Flash-Next and the floor, 2026-10-11/12):
  - a window meditation inside a turn splits the running turn, keeping its ask and only its recent steps, as OpenCode 2 and Pi do (#813);
  - a refill fits the window: reference lines bounded, working memory elided to the window's line (#814), and the meditation's audit fitted the same way (#821);
  - a fork's own record-tool calls fold as its answer (#806);
  - a fork cut from the trunk reserves only its new tokens where the server shares prefix pages (#797);
  - the in-turn fork queue is bounded (#822);
  - the kitchen sink shows every lever as a diet scenario (#798).

**Done when** T1, driven on the floor in phases, reaches step 8's feedback loops without the window ending a turn.

## v0.5.0 — the hosted API live

The hosted API kind driven live (#555, #556): the first paid calls, the cache lifetime lever, and the per-fork cache-read share, on Haiku 5.5 (the nearest to the 27B floor) and Sonnet 5.5 (the maintainer, 2026-10-10). Every live call needs the maintainer's approval.

## v0.6.0 — the pieces together (T2)

The pieces used in whatever order the work needs to reach a goal, in one session that ends in a declared compaction. T2 is that session; its trajectory is not yet written. Finishing T1 moved here from v0.1.0 (the maintainer, 2026-10-10); it runs with meditation allowed, automatic or declared (the maintainer, 2026-10-11; [`trajectories/t1.md`](trajectories/t1.md)). This milestone and v0.7.0 predate the initiatives, and follow its lever milestones.

## v0.7.0 — T3, NetHack

Attempting the NetHack 5 WebAssembly port, with a record that says truthfully which arm, regime and model ran it. The comparison is the archived Claude desktop app session of the same work, read by the Claude Code adapter (#500, #501).

## v0.8.0 — runs re-fire

The second initiative's first milestone. Every run committed so far is a historical observation. Its asks were typed in answer to what a temperature-1.0 model did, so the inputs of a re-firing don't exist. A run re-fires when its inputs are a file. v0.3.0's scenarios (#760) are that file and its runner. This milestone derives scenarios from the runs already committed, repeats them, and names a re-fired run reproducible.

| piece | shown when | evidence |
| --- | --- | --- |
| Operator script | every operator move a log records becomes a script: asks and their files, scoping marks, meditations, flags, edits, rulings, cancels, the end. It is derived from the log by a tool and played by `serve` against a live substrate. The script fixes the inputs, not what comes back | — |
| Today's runs as scripts | each committed run under `runs/` has its script, and one of them is re-fired on the floor | — |
| Repeats | one script re-fired *N* times under one regimen. The receipts are a distribution, and the record names the script's digest and the repeat | — |
| Reproducible kind | a re-fired run's README reads reproducible, not historical: its script, regimen and substrate are enough to fire it again | — |

An ask a person types in answer to what the model did can't be scripted faithfully. A scripted run says which asks are fixed, and a driven run stays a driven run.

## v0.9.0 — every lever has its control

For each lever row of v0.2.0, the same script run with the lever at its shown state and at its control: off, the default, or a sham where one is named (`program.md` §2). It runs *N* times each, on the floor. This milestone shows that the pairs run and the receipts compare. Whether a difference is real is the third initiative's.

| piece | shown when | evidence |
| --- | --- | --- |
| Paired runs | one command fires a script under two regimens that differ in one lever, *N* times each, and writes both records | — |
| A control per lever | each v0.2.0 lever has a pair on the floor, or a stated reason it can't, such as the hosted cache lifetime, which is v0.5.0's | — |
| The delta | a receipt-level comparison per pair: tokens, turns, captures, refusals, settles. A table, not a claim | — |
| Maintainer against agent | at least one script driven by the maintainer and re-fired by an agent, as the delta the maintainer asked for ("Deltas between what I do and what agents do will be interesting data") | — |

## Later

Work that serves no open release waits in a `Later —` milestone named for the condition on which it returns (`AGENTS.md`, Tickets). It is sorted at a grooming, not as it is found.

| milestone | holds | returns |
| --- | --- | --- |
| Later — loop correctness | tests and edge cases in the drive loop's own code | at feature complete, or sooner if a trajectory trips one |
| Later — product polish | usability of serve and the surface for a stranger | at feature complete |
| Later — claims publication | proofs of the log and record formats; the results linter | when claims are prepared for publication |
| Later — substrate ladder | admission instruments and substrate work the loop does not need | with the ladder program |
| Later — gate redesign | defects in the gate and its kept checks | with the gate redesign, after feature complete |
| Later — lever candidates | levers proposed but not built: a find tool for working memory (#664), how far back a prune may reach (#665), model-elected pruning shown live (#710) | when a run shows the need, or the feature a candidate extends is turned on |
| Later — rigor | detector and instrument studies that ran ahead of the third initiative: small-model triggers, the tense rule, the stance codebook, closure verification (#907) | with the third initiative, on initiative 2's lever pairs (the maintainer, 2026-10-19) |
