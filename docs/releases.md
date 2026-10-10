# Releases

*The program's initiatives, the milestones (epics) inside each, what each open milestone means as an observable, and the evidence that it happened. A definition or row changes only with the maintainer's approval ([`AGENTS.md`](AGENTS.md)). How milestones and tags work is in `CONTRIBUTING.md`, Milestones.*

## Initiatives

The maintainer, 2026-10-09: "Working prototype _with every theorized lever present_ → controls and reproducible tests → rigor, test matrix, data collection → publish." Each is an initiative, run in that order, and holds as many milestones as it needs; nothing in a later initiative is an obstacle to an earlier one.

1. **The prototype:** every theorized lever (`program.md`, The levers) present and working. v0.1.0 is its first milestone; the milestones after it are the remainder of the levers, proposed once each lever's code is inventoried.
2. **Controls and reproducible tests.**
3. **Rigor:** the test matrix and data collection.
4. **Publish.**

## v0.1.0 — the pieces work

The prototype's first milestone. Each piece of the harness shown working **once, live, on the floor**. A passing test does not count. Using the pieces together is v0.2.0's.

**Floor:** the 3.8 EXL3 line, config r2, on linux-pc (`accel24-tabbyapi-exl3-qwen38-27b-3p00`, #497), driven through TabbyAPI (#496). The 3.6 is retired.

**Main evidence:** T1 ([`trajectories/t1.md`](trajectories/t1.md)). T1 has no seam; a short session of its own is fine for that row (#493).

| piece | shown when | evidence |
| --- | --- | --- |
| Ask | an ask streams (reasoning, then the answer) and settles; the log has the response row | — |
| Tools, unprompted | the model runs a shell command under Seatbelt without being told to, reads a file outside the worktree (the Babylon Lite source), and writes or edits a file inside it | — |
| Approval | a command not in the pre-seeded list (`npm install`) prompts; approving it at workspace scope lets it run | — |
| Image | an attached screenshot PNG, and a reply that shows the model saw it | — |
| Interview fork | a fork in the idle gap lands a patch that shows in the working-memory panel | — |
| Seam | a declared seam renders, refills from working memory, and one ask after it settles | — |
| Cancel and end | an in-flight call is cancelled; the session ends with confirm and serve exits | — |
| Record | the log parses, the record is projected from it and names the engine that ran, the receipt renders, and the results directory is committed, digest-pinned and hygiene-scanned | — |

A row's evidence is a results directory, or a line in one, filled from the recording. Its README says truthfully who drove the run, the maintainer or an agent. A row counts once the maintainer approves its evidence. The milestone is done when every row counts.

## v0.2.0 — the pieces together (T2)

The pieces used in whatever order the work needs to reach a goal, in one session that ends in a declared compaction. T2 is that session; its trajectory is not yet written. This milestone and v0.3.0 predate the initiatives; where they sit among the lever milestones is proposed with those.

## v0.3.0 — T3, NetHack

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
