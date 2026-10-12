# Levers run C, re-fired: agent-driven

**Who drove it:** an agent (Track 1), launching `diet-drive scenario diet/scenarios/levers-run-c.jsonl` live on the floor. Its steps and the operator's actions are the scenario's own; the model's answers are the floor's. Not the maintainer. The assembler's own line was "scripted: scenario levers-run-c, re-fired on accel24-tabbyapi-exl3-qwen38-27b-3p00".

This is evidence only once the maintainer approves it.

**Purpose:** v0.3.0's last piece of evidence. A scenario the canned substrate plays (#760) re-fires through the same runner against a live model, and the levers [run C](../2026-10-11-levers-run-c-agent-driven/README.md) showed by hand show again:
- interview cadence per class;
- the v4 fork asks' SUPERSEDE;
- the reconcile gate holding under `refuse`.

## Setup

- **Build:** `d75d1daac6e1e3ead901782aac66f26facba3549`, as the log's `session.start` names it.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2 (the loaded model, a 163,840-token pool, two slots, no drafter). Serve checked the engine at start. No other session was running on it.
- **Regimen:** `regimen.toml` here. It is run C's, byte for byte, plus one key: `max_output_tokens = 32768`. Run C's serve took that cap on its command line; the scenario runner passes none, so the regimen carries it. The start row's `step_and_output_limits` names it (`output_cap:32768:regimen`).
- **Head:** the scenario's own, `diet/drive/heads/floor.md`, not T1's session-one head that run C was served with.
- **Workspace:** a fresh, empty git repository, as run C's was. The task is the scenario's two asks, the same text run C's operator typed: a stdlib Python Markdown link checker, then a change of scope to JSON Lines.

**The start row** (`session.log` line 1, `levers`) names:
- `approval: none`;
- `archive_recall: literal`;
- `background_commands: on`;
- `bash_timeout: 120000ms`;
- `cache_lifetime: undeclared`;
- `capture_modality: fields`;
- `compaction_depth: tail:2000`;
- `extraction_seat: warm-model`;
- `failed_turns_on_the_trunk: kept`;
- `fork_asks: v4:bbaac02236b3146c`;
- `fork_delivery: advisory`;
- `fork_delivery_site: tail`;
- `fork_input_view: last_turn`;
- `fork_memory_share: tail:output-cap`;
- `fork_warrant: one-per-gap-gated:read+scoping`;
- `instruction_files: on`;
- `interview_role: user`;
- `interview_routing_and_cadence: per_class`;
- `isolation: sandbox`;
- `mechanical_capture: off`;
- `meditation_audit: on`;
- `meditation_trigger: operator-declared+window`;
- `meditation_warm: on`;
- `model_pruning: off`;
- `operator_unilateral: on`;
- `phase_graph: none`;
- `reasoning_state: on:effort:xhigh`;
- `reconcile_gate: refuse`;
- `render_budget: 120:tier`;
- `self_capture: off`;
- `step_and_output_limits: max_steps:none:default:output_cap:32768:regimen:clamped:163840:reported`;
- `stream_deadline: idle:300000ms`;
- `subagent: harness`;
- `substrate_rung: accel24-tabbyapi-exl3-qwen38-27b-3p00`;
- `tangent_closure: operator`;
- `tool_call_text_fallback: off`;
- `tool_output_disposition: cap-on-arrival:2000-lines:51200-bytes+meditation:reference`;
- `tool_surface: standard`;

## The run

| turn | ask (`seq`) | trunk requests | tool calls | forks | settled |
| --- | --- | --- | --- | --- | --- |
| 1 | 1 | 5 | 7 | 5 | `final` |
| 2 | 1478 | 3 | 4 | 6 | `final` |

1. **Scoping ask** (`ask` 1): write `linkcheck.py` and three sample docs, and run it once. The model listed the worktree, wrote four files, and ran the checker twice: once broken (exit 1), once with the target touched (exit 0). `final` (381).
   - Four per-class forks fired on the writes (383, 696, 1042, 1164), each `trigger: call:edit:…`, `why: read`.
   - Then the scoping fork (1366) at `turn_end`, which landed `interview-t1-f1366-0`: "decision: The task is complete …".
2. **Scoping ask, a change of scope** (`ask` 1478): JSON Lines instead. Three `edit` calls and one run. `final` (1586).
   - Three per-class forks fired on the edits (1588, 1795, 1874).
   - Then the scoping fork (2053), whose answer ends `SUPERSEDE: interview-t1-f1366-0 — replaced by this turn's DECISION (output format is JSON Lines, not file:line: target).`
   - Patch 2306 is `op: supersede`, `supersedes: interview-t1-f1366-0`; the new decision itself landed as its own `add`, `interview-t2-f2053-0`.
3. **The operator's flag** (2308): the scenario flags the first live entry holding "as JSON Lines". Here that is `interview-t2-0`, a per-class `learned:` note saying the checker now reports JSON Lines (run C's flag fell on the decision). Its note is "stale?", a concern that does not hold.
4. **The operator's meditation:**
   - The reconcile audit (2310) answered all 18 notes, KEEP for the flagged one, and retired 12 others as duplicates or stale, among them the supersede's stray remark (`interview-t2-2`, removed as a duplicate of the decision).
   - The one re-ask (5411), over the flagged note alone, answered KEEP.
   - The meditation held: `meditation.held` 5469, `unaddressed: ["interview-t2-0"]`.
5. **An ask while held**, refused: `refused` 5470, `because: meditation-held`.
6. **Refill anyway:** meditation 5473 carries `refilled_anyway: true` and `unaddressed: ["interview-t2-0"]`.
7. Ended; serve exited.

The whole session took about four minutes (07:24:46 to 07:28:35 UTC).

## What it showed

| lever | shown | where (log `seq`) | against run C |
| --- | --- | --- | --- |
| interview cadence: per class | yes, on edit-class calls | 7 forks with `trigger: call:edit:<call id>`, `why: read`: 383, 696, 1042, 1164 (turn 1's writes) and 1588, 1795, 1874 (turn 2's edits); the scoping forks at `turn_end` beside them (1366, 2053) | the same: 7 per-class forks (run C split them 5 and 2). The model again called no read-class tool, so a `call:read:` trigger is still not shown |
| v4 SUPERSEDE | yes, mechanically | fork 2053's answer names `interview-t1-f1366-0`; patch 2306 is `op: supersede`, `supersedes: interview-t1-f1366-0`, the right entry | **differs.** Run C's superseding entry carried the new decision's text. Here the model wrote a reason after the id ("— replaced by this turn's DECISION …"), and that reason is what now stands as `interview-t2-2`; the decision is a separate entry |
| reconcile gate: refuse holding | yes | flag 2308 → audit 2310 KEEP → re-ask 5411 KEEP → `meditation.held` 5469 naming the entry → ask refused 5470 → refill anyway → meditation 5473 with `refilled_anyway: true` | the same chain. This audit also retired 12 notes, the supersede's remark among them; run C's kept every note |

## What broke

Nothing in the session: both turns `final`, no failed request, and every expectation in the scenario met (`unmet: []`, `unmatched: []`).

Noted:
- **The SUPERSEDE line's second half.** The v4 ask asks for the id, then the entry that should stand. The model wrote the id, then a dash and a reason, and the parser took the reason as the entry. The supersede still voids the right entry, but the working record's standing text for it is a remark rather than a fact. Whether the parser should refuse a `—` reason there, or the ask should say it more plainly, is a question for its own ticket.
- **Still no read-class trigger.** As in run C, the model wrote and edited files and ran them through bash, but never called `read`, `grep` or `glob`.

**Counts:** 2 asks, 19 requests, 11 tool calls, 11 forks (2 audit, 9 interview), 31 patches, 1 flag, 1 meditation, 1 meditation held, 1 refusal.

| kind | lines |
| --- | --- |
| `ask` | 2 |
| `delta` | 5323 |
| `flag` | 1 |
| `fork` | 11 |
| `fork.settled` | 11 |
| `meditation` | 1 |
| `meditation.held` | 1 |
| `patch` | 31 |
| `progress` | 30 |
| `refused` | 1 |
| `request` | 19 |
| `response` | 19 |
| `session.start` | 1 |
| `settlement` | 11 |
| `tool_call` | 11 |
| `turn.settled` | 2 |

## Scrubbed

- `record.jsonl`: account-name (3), personal-home-path (37).
- `session.log`: account-name (6), personal-home-path (38).

Home paths are written `~`, scratch paths `/tmp/scratch/` and the account name `user`, by `scripts/scrub.py` with the hygiene check's own patterns. The regimen is byte for byte.

## Files

`diet check-log` accepts `session.log` at this build.
`diet check-record` accepts `record.jsonl` at this build.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `76a05bdb0cc08e37d4e535b0d5fb952d3ee758a251085c3b43ccfb26a50e905b` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `2bdf6f7f7207347236420797a90baa7ae49e9aa33211b59bb4c2a6b30df338f6` |
| `product.txt` | the working memory at end | `174982fcfe6402f4e3a04af86e1753eaee91f5884b927f020c8a6cf5cd21d244` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `a45ce9d29a5764fb5506b1358471baaf37fc2d966bb39fef6179082c32980b39` |
| `regimen.toml` | the regimen as it ran | `d0ac73513fd7e5affe7de50906f057c7055a03e2bb37ef39542680dc0bb79e11` |
