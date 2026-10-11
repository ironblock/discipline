# Levers run B — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page. Not the maintainer. Agent-driven runs are approved as evidence, labelled as such.

**Purpose:** v0.2.0, every lever live. This run shows the levers not yet shown in a committed run that run A's settings could not carry:
- capture modality (tools, on ask set t1);
- step and output limits (a `max_steps` that stops a turn);
- seam trigger (model-proposed, operator-ruled);
- tangent closure;
- failed turns on the trunk (a turn cancelled after it took steps, with the state on the start row).

## Setup

- **Build:** develop `22da91a43e4a1fe8bd4bf55ebf2fcb2b9dd7a2a6`.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2. Serve declared the engine and reported it warmed.
- **Regimen:** `regimen.toml` here. It is seam smoke run 2's, plus:
  - `fork_asks = "t1"` and `capture_modality = "tools"`;
  - `max_steps = 6`;
  - `phases = ["plan", "build"]` with `[phase_transitions] plan = ["build"]`.

  Self-capture, the scoping warrant, approvals off, `xhigh` and the standard tool surface are run 2's.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`.
- **Workspace:** a fresh, empty git repository. The task was a stdlib Python todo CLI.

**The start row names every target state** (`record.jsonl` line 1, `levers`):

| lever | start row |
| --- | --- |
| capture modality | `tools`, with `fork_asks: t1:49c5eb0e8fb55db1` |
| step and output limits | `max_steps:6:regimen:output_cap:32768:…` |
| seam trigger | `operator-declared+window+model-proposed`; `phase_graph: phases:plan,build;transitions:plan>build` |
| tangent closure | `operator` |
| failed turns on the trunk | `kept` |

The log's `session.start` opens in phase `plan`.

## The run

1. **Plan phase:** plan a todo CLI, write no code, and propose moving to build when ready.
   - The model called `propose_phase_transition` (plan → build).
   - The operator ruled **seam**, and serve refused it: `nothing-to-seam`, because working memory was empty (break 1).
   - The operator then ruled **continue**, and the phase became `build`.
2. **Scope answer (marked scoping):** add, list and done, with `todo.json`; write `todo.py` in one write. The interview fork ran and answered through `update_record`.
3. "Smoke-test it, one command per tool call", with eight commands. The turn stopped after six.
4. "Finish the smoke test …". **The operator cancelled the turn after it had run three steps.**
5. **Tangent:** opened before turn 5, which asked a side question (should a `remove` command come later?) and asked for the answer to be recorded as a follow-up. The tangent was closed with that entry **parked**.
6. Ended with confirm; serve exited.

## What it showed

| lever | shown | where (log `seq`) |
| --- | --- | --- |
| capture modality: tools | yes | fork 608 (scoping) settled `value` (671); its `capture` lines 672 and 674 carry `fork: 608` and `tool: update_record`, each recorded |
| step limit | yes | turn 3 settled `max_steps` (814) after 6 tool calls of the 8 asked |
| seam trigger: model-proposed, operator-ruled | yes | `capture` 322 `propose_phase_transition` plan → build, `outcome: proposed`; `phase.ruled` 355, `choice: continue` |
| tangent closure | yes | `tangent.open` 1202; `tangent.close` 1399: `parked` the follow-up, `rolled_back: 4`, `prefix_intact: true` |
| failed turns on the trunk: kept | yes | turn 4 settled `cancelled` (1200) after 3 tool calls ran (1081, 1082, 1182). The record's prefix change at the next trunk request (`q/1206`) only adds a message (the third step's result); nothing is removed |

Also fired: the self-capture reminder.

## What broke

1. **The composer offers "seam" on a proposal when there is nothing to seam.** With empty working memory, serve refused the ruling (`refused`, `because: nothing-to-seam`, seq 354), and the operator had to rule again. The proposal's buttons don't know whether a seam is possible.
2. **The plan-phase answer held no plan.** The model proposed the move and answered only "Plan is proposed — I'll start building once the phase moves to build." A one-line summary of the plan rode only in the proposal's `reason`.
3. **One self-capture dropped by the groundedness gate** (1080).

**Counts:** 5 asks, 17 requests, 13 tool calls, 1 fork, 3 patches, 5 captures (3 recorded, 1 proposed, 1 dropped), 1 reminder, 1 refusal, 1 cancel, 1 tangent. Settles: final, final, `max_steps`, `cancelled`, final.

## Scrubbed

In `session.log` and `record.jsonl`, the home directory is written `~` and the account name `user`. `product.txt` is unchanged. Serve's stdout is not committed.

## Files

`diet check-log session.log` and `diet check-record record.jsonl` both accept these files. I ran them at the build above. "repair" appears nowhere in the record or serve's output.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `5c4a90b9adab8d99826f6bb250a0cb1d2e0dbf2cac483637a445fdc01c235130` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `c1a1d162c84611ca8ab37985a13b9a8fc8709f9773211a71c40a8d7e08c8eb9f` |
| `product.txt` | the working memory at end | `b748acda855dffffd15d9da45c9434be314e90cb2db88222cdc9e69f80a7b4eb` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `c62092e3974a02d586a246e7925f11ffd14ab7c1c04e1ff1f541a695601cf562` |
| `regimen.toml` | the regimen as it ran | `09cb509422c05086fa41e0921a69d533ba7f3fb5eeb40712cd21e33c3a0c692f` |
