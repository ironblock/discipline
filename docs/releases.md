# Releases

*The program's initiatives, the milestones (epics) inside each, what each open milestone means as an observable, and the evidence that it happened. A definition or row changes only with the maintainer's approval ([`AGENTS.md`](AGENTS.md)). How milestones and tags work is in `CONTRIBUTING.md`, Milestones.*

## Initiatives

The maintainer, 2026-10-09: "Working prototype _with every theorized lever present_ → controls and reproducible tests → rigor, test matrix, data collection → publish." Each is an initiative, run in that order, and holds as many milestones as it needs; nothing in a later initiative is an obstacle to an earlier one.

1. **The prototype:** every theorized lever (`program.md`, The levers) present and working. v0.1.0 is its first milestone, v0.2.0 the remainder of the levers, and v0.3.0 the hosted API (approved by the maintainer, 2026-10-10).
2. **Controls and reproducible tests.**
3. **Rigor:** the test matrix and data collection.
4. **Publish.**

## v0.1.0 — the pieces work

The prototype's first milestone. Each piece of the harness shown working **once, live, on the floor**. A passing test does not count. Using the pieces together is v0.4.0's.

**Floor:** the 3.8 EXL3 line, config r2, on linux-pc (`accel24-tabbyapi-exl3-qwen38-27b-3p00`, #497), driven through TabbyAPI (#496). The 3.6 is retired.

**Evidence:** four agent-driven runs under `runs/`, each row's approved by the maintainer (2026-10-10). Finishing T1 ([`trajectories/t1.md`](trajectories/t1.md)) moved to v0.4.0: at `xhigh` its script does not fit the floor's window without a seam ([`2026-10-10-t1-run-3-agent-driven`](../runs/2026-10-10-t1-run-3-agent-driven)).

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
| fork asks | v4 | [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven): a SUPERSEDE patch at 3188; [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) |
| self-capture | on, a reminder after 3 silent turns | [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven) and [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven): captures and the reminder |
| capture modality | tools, with ask set t1 | [`levers-run-b`](../runs/2026-10-10-levers-run-b-agent-driven): fork 608 answered through `update_record` |
| model-elected pruning | — | not run: its description awaits the maintainer |
| fork delivery of results | advisory | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): `delivered` 4042 and 5818 |
| tool-output disposition | cap on arrival; `reference` at the seam | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): call 2592 capped; both seams listed outputs by path |
| isolation | `sandbox` | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven): a Seatbelt refusal at 12815 |
| approval | none | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) |
| reasoning state | on, `xhigh` | [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven) and [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) |
| cache lifetime (hosted) | — | v0.3.0's: every paid call needs the maintainer's approval |
| substrate rung | the floor | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) |
| tool surface | standard | [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven), [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven) and [`image-smoke`](../runs/2026-10-10-image-smoke-agent-driven) |
| instruction files | on, an AGENTS.md in the worktree | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): all 8 trunk answers followed it |
| tangent closure | closed, its entry parked | [`levers-run-b`](../runs/2026-10-10-levers-run-b-agent-driven): 4 messages rolled back, the prefix intact (1399) |
| interview routing and cadence | per class, skipping self-recorded turns | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven): a scoping fork at 8355, `fork.skipped` at 14819 |
| render budget | tier | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): `render_reduced` 1 and 2. The render stayed over its budget after reducing (v0.2.0's own fix) |
| fork memory share | displacement logged | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): forks 2439 and 4952, `may-displace-trunk-cache`. No clamp or refusal was provoked |
| archive recall | literal | [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven): `recalled` at 10396 and three more |
| fork input view | last turn | [`levers-run-a`](../runs/2026-10-10-levers-run-a-agent-driven): forks 2439 and 4952 |
| interview role | user | the start rows of [`seam-smoke-run-1`](../runs/2026-10-10-seam-smoke-run-1-agent-driven) and [`seam-smoke-run-3`](../runs/2026-10-10-seam-smoke-run-3-agent-driven). System and developer do not render in the floor's chat template |
| step and output limits | `max_steps` 6 | [`levers-run-b`](../runs/2026-10-10-levers-run-b-agent-driven): turn 3 stopped at `max_steps` (814) |
| failed turns on the trunk | kept | [`levers-run-b`](../runs/2026-10-10-levers-run-b-agent-driven): turn 4 cancelled after 3 steps, and the next request only appended |
| reconcile gate | `refuse` | [`gate-and-tail`](../runs/2026-10-10-gate-and-tail-agent-driven): set, never held. Across three meditations the model addressed every operator change, including one a contrived flag asked it to leave out. The held state is covered by #698's tests and #699's stories |
| subagent | — | later |

Every row's evidence is approved (the maintainer, 2026-10-10: "approve all, proceed"). The reconcile gate's row and the zero-turns warning await him. The extraction seat is struck (#570). The milestone closes with its open issues and the v0.2.0 tag.

## v0.3.0 — the hosted API live

The hosted API kind driven live (#555, #556): the first paid calls, the cache lifetime lever, and the per-fork cache-read share, on Haiku 5.5 (the nearest to the 27B floor) and Sonnet 5.5 (the maintainer, 2026-10-10). Every live call needs the maintainer's approval.

## v0.4.0 — the pieces together (T2)

The pieces used in whatever order the work needs to reach a goal, in one session that ends in a declared compaction. T2 is that session; its trajectory is not yet written. Finishing T1 moved here from v0.1.0 (the maintainer, 2026-10-10); how it runs, with a seam or without, is still the maintainer's to decide. This milestone and v0.5.0 predate the initiatives, and follow its lever milestones.

## v0.5.0 — T3, NetHack

Attempting the NetHack 5 WebAssembly port, with a record that says truthfully which arm, regime and model ran it. The comparison is the archived Claude desktop app session of the same work, read by the Claude Code adapter (#500, #501).

## Later

Work that serves no open release waits in a `Later —` milestone named for the condition on which it returns (`AGENTS.md`, Tickets). It is sorted at a grooming, not as it is found.

| milestone | holds | returns |
| --- | --- | --- |
| Later — loop correctness | tests and edge cases in the drive loop's own code | at feature complete, or sooner if a trajectory trips one |
| Later — product polish | usability of serve and the surface for a stranger | at feature complete |
| Later — claims publication | proofs of the log and record formats; the results linter | when claims are prepared for publication |
| Later — substrate ladder | admission instruments and substrate work the loop does not need | with the ladder program |
| Later — gate redesign | defects in the gate and its kept checks | with the gate redesign, after feature complete |
| Later — lever candidates | levers proposed but not built: a find tool for working memory (#664), how far back a prune may reach (#665) | when a run shows the need, or the feature a candidate extends is turned on |
