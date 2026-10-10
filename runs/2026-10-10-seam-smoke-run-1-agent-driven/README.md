# Seam smoke run 1 — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page. Not the maintainer. He approved it as evidence, labelled as such.

**It showed:** v0.1.0's Seam row holds live on the floor. A declared seam renders, refills the trunk from working memory, and the ask after it settles using what only working memory still held.

## Setup

- **Build:** develop `805962194ce586d068855bbce5c345ba7d157a18`.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2. Serve declared the engine and reported it warmed.
- **Regimen:** `regimen.toml` here, copied from T1 session one's. Changes from T1's:
  - the arm is `seam-smoke`;
  - `self_capture = "on"` at the default cadence (every 3);
  - `fork_asks = "v4"`;
  - no writable reference (`sandbox_writable = []`), because the task needs none.

  Approvals off, `xhigh` and the sampler are T1's.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`.
- **Workspace:** a fresh scratch directory. The task was a small stdlib Python todo CLI, then extended.

## The run

1. Write `todo.py` (add, list, JSON file). Settled `final`.
2. **Scope answer (marked scoping):** priorities high/normal/low, an optional due date, one file. The v4 fork landed a decision and a plan.
3. Add `done <id>` and sort by priority and due date. `final`.
4. **Changed scope (marked scoping):** drop `low`, and migrate stored `low` tasks to `normal`.
   - The self-capture reminder fired.
   - The model recorded a decision and a follow-up.
   - The fork fired (see SUPERSEDE below).
5. **Seam (operator, no phase):** the refill used `placement: message` and carried 6 entries, 0 turns, with `tool_outputs: evict`.
6. "Which priorities did we settle on, and what's still missing? Add what we discussed."
   - The model answered from working memory: high/normal only, the load-time migration, and `update <id>` plus undo still missing.
   - It built and tested both, then resolved its own follow-up.
7. Ended with confirm; serve exited.

## What it showed

| Seam row part | shown? | where |
| --- | --- | --- |
| a declared seam renders | yes | the `seam` line: reason `operator`; the render is the working set, 6 entries |
| refills from working memory | yes | the first trunk request after it carries the head the seam names as `prefix_hash_after`. The record's diff at that request removes the old turns from index 1 on and leaves the system message (index 0) untouched |
| one ask after it settles | yes | turn 5 settled `final`, using what only working memory held |

**Levers live:**
- **self-capture:** 4 `capture` lines, each with its `lane: self-capture` patch;
- **the reminder:** once, at its cadence;
- **the v4 interview fork:** on both scoping asks.

## What broke

1. **The record holds no turn rows.** On this engine, turn 1's `prefill_tokens` and every response's `output_tokens` read "equality unmeasured", and turn rows must run from 1 without a gap. So the forks, patches, captures and the seam have no rows either.
   - The record predates usage on responses (#654), which fixed this.
   - It is a record all the same, and `diet check-record` accepts it as it is.
2. **The v4 SUPERSEDE grammar misread the model.**
   - The model wrote `SUPERSEDE: interview-t2-0 → r1516/…/decision; interview-t2-1 → r1516/…/followup`: two pairs on one line, each pointing an old entry at one self-capture had already written.
   - The grammar read one supersede of `interview-t2-0`, whose new text was the rest of the line. The second pair was lost, and the reference became entry text.
3. **One `unattributed` prefix change with an empty diff,** at the second fork's request (`q/2142`, interview lane). Recorded as data.

**Counts:** 5 asks, 35 trunk requests, 32 tool calls, 2 forks, 9 patches (4 of them self-capture), 4 captures, 1 reminder, 1 seam. All five turns settled `final`.

## Scrubbed

In `session.log`, `record.jsonl`, `receipt.json` and `unspellable.json`:
- the session's scratch directory is written `/scratch`;
- the home directory `~`;
- the account name `user`.

Digests inside the log and record were taken over the unscrubbed bytes. `product.txt` is unchanged. Serve's stdout is not committed.

## Files

`diet check-log session.log` and `diet check-record record.jsonl` both accept the scrubbed files. I ran them at develop `2f9501a0`.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `957401e036cd349713c75fca274fbd1d242cd45b0d41e30b25372c4f51539df8` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `94c68d73f0bf934a4d3512e593d9f4f2dc4c2af56dfba2f0beb466fcde3d79ac` |
| `product.txt` | the working memory at end | `1e7759873054d516aa64cbb2d8ee0251d25c37b2fa9f049fb5214ee373b3d33c` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `ca075908ba5a590fd1a8f39039b19844e73ac28a950d04451de03a1875e4d28e` |
| `regimen.toml` | the regimen as it ran | `a40a54ec92cf5a99b3b35a7f3f7104e181156ba8dcbf675294a171a7d115ad06` |
