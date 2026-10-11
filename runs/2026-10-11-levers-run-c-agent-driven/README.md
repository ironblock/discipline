# Levers run C — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page. Not the maintainer. The maintainer approved this re-run on 2026-10-11. Agent-driven runs count as evidence, labelled as such.

**Purpose:** v0.2.0 evidence for the three levers #747 reworded as not shown:
- interview cadence per class;
- the v4 fork asks' SUPERSEDE;
- the reconcile gate holding under `refuse`.

## Setup

- **Build:** develop `20a2e6f0f5ffdd84b451f5c9e762f1c8afec592b`, which includes #744, #748 (as #752) and #751.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2. Serve declared the engine and reported it warmed. No other serve was running.
- **Regimen:** `regimen.toml` here. It is levers run A's, with:
  - `capture_modality = "fields"` and `fork_asks = "v4"`;
  - `interview_warrant = ["read", "scoping"]` and `interview_cadence = "per_class"`;
  - the audit on (`meditation_audit: on`);
  - `reconcile_gate = "refuse"`;
  - `self_capture = "off"` and `interview_skip_self_recorded = "off"`, so the forks do the writing;
  - run A's budget trigger dropped, so the only meditation is the operator's;
  - no AGENTS.md in the worktree.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`.
- **Workspace:** a fresh, empty git repository. The task was a stdlib Python Markdown link checker.

**The start row** (`record.jsonl` line 1, `levers`) names:
- `fork_asks: v4:bbaac02236b3146c`;
- `capture_modality: fields`;
- `fork_warrant: one-per-gap-gated:read+scoping`;
- `interview_routing_and_cadence: per_class`;
- `self_capture: off`;
- `reconcile_gate: refuse`;
- `meditation_audit: on`.

## The run

1. **Scoping ask:** write `linkcheck.py`, reporting broken links one per line as `file:line: target`, plus three sample docs. The model wrote and ran it. `final`.
   - Five per-class forks fired on its write calls, then the scoping fork (3970) at the turn's end.
   - The scoping fork landed the decision `interview-t1-f3970-0`.
2. **Scoping ask, a change of scope:** report broken links as JSON Lines instead. `final`.
   - Two per-class forks fired on its edit calls, then the scoping fork (4988).
3. **The operator's flag:** `interview-t2-f4988-2`, the current and still-true JSON Lines decision, flagged with a concern that does not hold: "stale?" (`flag` 5552).
4. **The operator's meditation:**
   - The reconcile (audit 5555) answered **KEEP** for the flagged note.
   - One re-ask (audit 9776) over that note alone answered **KEEP** again.
   - The meditation held: `meditation.held` 10034, `unaddressed: ["interview-t2-f4988-2"]`.
5. **An ask while held**, refused: `refused` 10035, `because: meditation-held`. The composer explained it ("not taken: the meditation is held until your changes are addressed …").
6. **Refill anyway,** from the held panel, which named the entry and its text. The meditation line (10038) carries `refilled_anyway: true` and `unaddressed: ["interview-t2-f4988-2"]`.
7. Ended with confirm; serve exited.

## What it showed

| lever | shown | where (log `seq`) |
| --- | --- | --- |
| interview cadence: per class | yes, on edit-class calls | 7 forks with a `call:` trigger: 1346, 1994, 2343, 2946 and 3230 (turn 1's `write` calls) and 4269 and 4705 (turn 2's `edit` calls). Each is `trigger: call:edit:<call id>`, `why: read`, besides the scoping forks at `turn_end` (3970, 4988). **The model called no read-class tool** (`read`, `grep`, `glob`) in this run, so a `call:read:` trigger is not shown here |
| v4 SUPERSEDE | yes | fork 4988's answer ends `SUPERSEDE: interview-t1-f3970-0 — decision: … reports each broken link as one JSON Lines object with keys file, line, target …`. Patch 5550 is `op: supersede`, `supersedes: interview-t1-f3970-0`: the right entry, with the new decision's text |
| reconcile gate: refuse holding | yes | flag 5552 → audit 5555 KEEP → re-ask 9776 KEEP → `meditation.held` 10034 naming the entry → ask refused `meditation-held` 10035 → refill anyway → meditation 10038 with `refilled_anyway: true` |

## What broke

Nothing in the session. Noted:
- **Per class fired on the edit class only.** Turn 1's `bash` calls (`sed`, `mv`, test runs) fired no fork, and no read-class tool was called. A run whose ask makes the model read files is still needed to show a `call:read:` trigger.
- **The flag's send button was clicked by script.** The working-memory list was long enough that the browser pane's element finder could not reach it, so the operator clicked the flag form's own buttons in the page by script. The flag, the meditation and refill anyway were ordinary clicks.

**Counts:** 2 asks, 24 requests, 13 tool calls, 11 forks (9 interview, 2 audit), 35 patches, 1 flag, 1 meditation held then refilled, 1 refusal. Both turns settled `final`.

## Scrubbed

In `session.log` and `record.jsonl`, the home directory is written `~` and the account name `user`. `product.txt` is unchanged. Serve's stdout is not committed.

## Files

`diet check-log session.log` and `diet check-record record.jsonl` both accept these files: 2 turn rows, no repairs, and no `record_refused` on serve's end line. I ran both at the build above.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `c6f7809d79bf74c30dd0ca17d4c900285af175755205049f59ad050dca0582ae` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `59b8ca975967301817940dea10412d7c007a02a1f2f041f985cccaf4ae018de2` |
| `product.txt` | the working memory at end | `924bcb91595a2f73bd04896903eed2e861a1021a200148554a80fdd1317a9160` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `40751ab61e893f81efdcdd76c713b37c49b242a3ce1df44b50a7d38bd676a392` |
| `regimen.toml` | the regimen as it ran | `6517504c2ccfec402fd41e168dbeb2df2e2b268c687519b8fdc577cb5c54d916` |
