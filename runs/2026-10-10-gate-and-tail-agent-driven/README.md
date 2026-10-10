# Gate and tail run — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page. Not the maintainer. Agent-driven runs are approved as evidence, labelled as such.

**Purpose:** two levers live on the floor:
- #150's reconcile gate, `reconcile_gate = "refuse"`, holding a seam over unaddressed operator changes;
- #693's warning when a compaction tail keeps no turn.

**Findings:**
1. **Across three seams the model addressed every operator change, including one it was explicitly asked to leave out.** The gate was armed throughout and never held: there was never an unaddressed change for it to hold over. The held state remains shown only by #698's fixture and #699's stories.
2. **The tail warning fired**, and the page drew it in the maintainer's words.
3. **Serve wrote a log `diet check-log` refuses**, at seam 3 (below). Fixed by #702.

## Setup

- **Build:** develop `b424ee33`.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2. Serve declared the engine and reported it warmed.
- **Regimen:** `regimen.toml` here. It is seam smoke run 2's, plus `reconcile_gate = "refuse"` and `seam_tail_tokens = 200`, a tail smaller than any real turn. That gives fields-modality forks on ask set v4, the seam audit on and self-capture on.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`.
- **Workspace:** a fresh, empty git repository. The task was a stdlib Python budget CLI.

**The start row** (`record.jsonl` line 1, `levers`) names `reconcile_gate: refuse`, `compaction_depth: tail:200`, `fork_asks: v4:…`, `capture_modality: fields` and `seam_audit: on`.

## The run

1. Write `budget.py` (add, report). `final`.
2. **Scope answer (marked scoping):** cents as integers, lowercase categories, a month filter. The v4 fork (1034) landed `interview-t2-0` and `interview-t2-1`.
3. **Two flags:**
   - `interview-t2-0`: "resetting budget.csv silently lost data -- was that agreed?" (1166);
   - `interview-t2-1`: "not done: there is no test for a negative amount" (1167).
4. **Seam 1 (operator), 1778.** The reconciling audit (1169, `audit_q_human_reconcile`) answered UPDATE for both flagged notes, superseding them. The seam line says `unaddressed: []`.
5. Reject negative amounts, and test it. `final`.
6. **An edit and a flag:**
   - the operator rewrote `audit-1169-1` as `operator-1977`: `patch` 1977, `op: supersede`, `lane: operator`;
   - the operator flagged `audit-1169-2`: "stale: negative amounts are now refused and tested" (1978).
7. **Seam 2 (operator), 2090.** The audit (1980) answered UPDATE for the flagged note and KEEP for the edited one, so `unaddressed: []`.
8. **A contrived flag, labelled operator-contrived.** No change had gone unaddressed, so the operator, with Dispatch's agreement, flagged `audit-1980-1` with a note asking the model to leave it out: "leave this note to me: do not include a line for it in your answer" (2091). This is the same kind of operator move as levers run A's "don't record it yourself".
9. **Seam 3 (operator), 2601, with no turn since seam 2.**
   - The audit (2093) answered "1. KEEP / 2. KEEP". It included a line for the very note it was asked to leave out.
   - The fork settled `decline`, and the seam says `unaddressed: []`. No hold.
10. Ended with confirm; serve exited.

**The natural part** is steps 1–7: two seams, three flags and an edit, every change addressed unprompted. **The contrived part** is steps 8–9.

## What it showed

| lever | shown | where (log `seq`) |
| --- | --- | --- |
| compaction depth: a tail that keeps no turn, with its warning | yes | seam 1778: `tail_tokens: 200`, `carried_turns: 0`, `newest_turn_tokens: 2832`; seam 2090: `newest_turn_tokens: 1278`. The page drew "Zero turns retained: last turn was 2832 against budget 200". Seam 2601 had no turn since seam 2090 to keep, so it carries no `newest_turn_tokens` and no warning |
| reconcile gate: refuse | armed, never held | the start row says `reconcile_gate: refuse`. All three seams had `unaddressed: []`, so no `seam.held` was logged and refill anyway was never needed |
| the reconcile itself | yes | audits 1169, 1980 and 2093 asked `audit_q_human_reconcile`; `operator_changes` on each seam names every flag and edit |

## What broke

**Serve wrote a log its own reader refuses.** `diet check-log session.log` says:

> line 2094: a fork of turn 3 after a seam that followed it: the seam replaced the trunk the fork would be cut from

- Seam 3 was declared with no turn since seam 2.
- Serve admitted it, which is how an operator reconciles edits made after a refill.
- Its audit fork (2093) was logged `of_turn: 3, at: 1908`, cut from the trunk seam 2 had already replaced.

The fix, an audit cut from the refilled trunk, is #702. Under `runs/README.md`, a log serve wrote and diet refuses lands as the run's finding.

**Counts:** 3 asks, 21 requests, 14 tool calls, 4 forks (1 interview, 3 audits), 6 patches (1 by the operator), 4 flags (1 contrived), 1 capture, 3 seams, 0 holds. All three turns settled `final`.

## Scrubbed

In `session.log` and `record.jsonl`, the home directory is written `~`. `product.txt` is unchanged. Serve's stdout is not committed.

## Files

`diet check-record record.jsonl` accepts the record: 3 turn rows, no repairs, and the seam rows omit an empty `unaddressed` (#694). `diet check-log session.log` refuses the log, as quoted above. I ran both at the build above.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `82e5a09fd8581b6c8cb9573a1ede8ef73b3458fed00dd5644e419d1ac06dc0ee` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `772ce2c138dff1c8df50ae73b3610d1bbc22ab4929ed2cc41332f73f28dacd81` |
| `product.txt` | the working memory at end | `1fbb3fd4f472adb4ab3972ed18b80d56858347b0d4b5add2f10e86d33224d1de` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `413e11b5b2867abff4ab885bad77e5c96acb1fe5eadd927c7cf803a3362e1958` |
| `regimen.toml` | the regimen as it ran | `6c1095e901fc7bfbddd6eb9e39bf63fc58d986f8ae282957f7fe0afe857faa49` |
