# Drive plans

These are implementation plans for #117 (R2c to R6), written under the mediated-Socratic protocol ([#25](https://github.com/ironblock/discipline/pull/25#issuecomment-5826738930)). Each plan is made of three files:

- **`<item>-brief.md`**: the goal and the constraints, and nothing else. It is the only input the proposer and the critic were given.
- **`<item>-proposal.md`**: written by a subagent in a clean context, blind to anyone's preferred approach. Options with tradeoffs come first, then a recommendation, then the questions it could not settle.
- **`<item>-critique.md`**: an adversarial review by a second clean-context subagent, which also measures the claims it can.

## How a plan evolves

Each round is a commit, so `git log -p -- diet/drive/plans/` shows what every critique changed.

- A critique goes back to the proposer, who accepts or rebuts each finding and appends a revision log.
- The critic then reviews the revision.
- This repeats until neither side has a critical objection.
- Planning then ruled on the questions, on #117 (before 2026-10-08, when planning left GitHub).

The documents call earlier rounds `*.round1.md`. Those are the files as they stood at the round-1 commit, not separate copies.

One edit was made to the subagents' text. The proposal quoted a listener's `local_addr` in method-call form, which the hygiene table reads as a hostname; it now says "the method-call form of `local_addr` on a listener".

The prototypes and probe scripts the documents cite ran in a session scratchpad, and are not kept. The documents say how each measurement was made, and give its exit status where there is one.

## Status

- **R2c:** ruled on #117 and built: I1 to I7, the `idle.gap` intake and the reasoning courier (#128, #136, #137, #140, #146, #148, #151, #154).
- **R3:** two rounds; the closing critique left no critical or major finding. Ruled and routed on [#117](https://github.com/ironblock/discipline/issues/117#issuecomment-5881602023). The three files were posted there as the record and are committed here with R3.1, its first increment. Round 1's proposal was not posted, so `r3-proposal.round1.md`, which the proposal and critique cite, is not in this directory.
