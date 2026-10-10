# Levers run A — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page. Not the maintainer. Agent-driven runs are approved as evidence, labelled as such.

**Purpose:** v0.2.0, every lever live. This run shows the seam-side levers not yet shown in a committed run:
- a seam trigger other than declared (budget);
- compaction depth (a tail);
- a tool-output disposition other than evict (reference), plus the cap on arrival;
- a render budget that bites;
- fork input view (last turn);
- fork memory share (a logged cache displacement);
- fork delivery (advisory);
- instruction files (an AGENTS.md the model follows).

## Setup

- **Build:** develop `22da91a43e4a1fe8bd4bf55ebf2fcb2b9dd7a2a6`.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2. Serve declared the engine and reported it warmed. Its registry entry's `serving_context` is 163,840 tokens.
- **Regimen:** `regimen.toml` here. It is seam smoke run 2's, plus:
  - `seam_at_context_fraction = 0.12`, a budget seam at about 19,661 tokens;
  - `seam_tail_tokens = 2000`;
  - `seam_tool_outputs = "reference"`;
  - `render_budget_tokens = 120`, down from 400;
  - `fork_view = "last_turn"`;
  - `fork_delivery = "advisory"`.

  Self-capture, v4 asks, the audit and pre-warm, literal recall, the per-class cadence with its self-recorded skip, approvals off, `xhigh` and the standard tool surface are run 2's.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`.
- **Workspace:** a fresh git repository holding only `AGENTS.md`, copied here as `worktree-AGENTS.md`. It asks the model to end every final answer with the line `-- checked against AGENTS.md`. The task was a stdlib Python Markdown link checker.

**The start row names every target state** (`record.jsonl` line 1, `levers`):

| lever | start row |
| --- | --- |
| compaction depth | `tail:2000` |
| seam trigger | `operator-declared+budget+window` |
| tool-output disposition | `cap-on-arrival:2000-lines:51200-bytes+seam:reference` |
| render budget | `120:tier` |
| fork input view | `last_turn` |
| fork memory share | `tail:output-cap` |
| fork delivery | `advisory` |
| instruction files | `on`; the log's `session.start` lists `AGENTS.md` by sha256 |

## The run

1. Write `linkcheck.py` and three sample docs. The answer ends `-- checked against AGENTS.md`.
2. **Scope answer (marked scoping):** local links only, plus `#anchor` checks. The interview fork ran with `view: last_turn`.
3. "Run `seq 1 6000` …". The output went past the cap.
4. Ignore fenced headings, and add `--quiet`. **The budget seam fired before this turn's work.**
5. A one-line question, which makes a small turn for the tail to carry.
6. **Seam (operator):** the audit ran; the tail carried that small turn.
7. **Scope change (marked scoping):** remove anchor checks. Self-capture recorded the decision, so the scoping fork was skipped.
8. **Scope change (marked scoping):** drop `--quiet`. The operator asked the model not to record anything itself this turn, so the fork ran and superseded the earlier `--quiet` entry.
9. A one-line question. The advisory note of that supersede was delivered with it.
10. Ended with confirm; serve exited.

All eight turns settled `final`.

## What it showed

| lever | shown | where (log `seq`) |
| --- | --- | --- |
| seam trigger: budget | yes | seam 2804, `reason: budget`. The prompt was 25,520 tokens before it and 10,279 after |
| compaction depth: tail | yes | seam 4039: `tail_tokens: 2000`, `carried_turns: 1`, `carried_tokens: 118` (see break 2 for seam 2804) |
| tool-output disposition: reference | yes | seams 2804 and 4039: `tool_outputs: reference`, with 23 and 13 outputs listed by path in the refill instead of evicted |
| cap on arrival | yes | tool call 2592 (`seq 1 6000`): the model saw lines 1–400 and 4401–6000 with `[CONTENT TRUNCATED]`. It read the saved whole and confirmed 6,000 lines |
| render budget | yes | seam 2804: `render_reduced: 1`; seam 4039: `render_reduced: 2` (break 3) |
| fork input view: last turn | yes | forks 2439 and 4952: `view: last_turn` |
| fork memory share | yes, a displacement | forks 2439 and 4952: `hazard: may-displace-trunk-cache`. A last-turn fork on the trunk's own server is off the trunk's prefix, so it may push the trunk's cache out. No clamp or refusal happened |
| fork delivery: advisory | yes | `delivered` 4042 and 5818, `framing: advisory`. 5818 is fork 4952's supersede of the `--quiet` entry, sent with the next ask |
| instruction files: on | yes | `session.start.instruction_files` lists `AGENTS.md`. All 8 trunk answers end `-- checked against AGENTS.md`, and so do both interview fork answers (break 1); the audit fork's does not |

**Also fired:**
- the audit at the operator seam (fork 3646);
- the pre-warm on both seams;
- literal recall (4 times);
- the self-capture reminder;
- the self-recorded skip (turn 7).

## What broke

1. **The AGENTS.md sign-off leaks into fork answers, and the v4 grammar records it as an entry.**
   - Turn 2's fork answer ended with the sign-off. The grammar added `interview-t2-2` as "supersede: NONE … -- checked against AGENTS.md".
   - Turn 8's added `interview-t7-2-2` the same way.
   - An instruction meant for the operator-facing answer applies to the fork too, and the grammar reads what follows its last field as entries.
2. **The tail kept nothing at the budget seam.** Seam 2804 says `tail_tokens: 2000` but `carried_turns: 0`. The newest whole turn (the `seq 1 6000` one) was larger than 2,000 tokens, and the tail keeps whole turns only. It worked at seam 4039, after a small turn.
3. **The render stayed over its budget after reducing:** 131 and 137 tokens against 120, after one and two reductions.
4. **The v4 SUPERSEDE grammar split one multi-line supersede into three entries** (`interview-t7-2`, `-2-1`, `-2-2`), as in seam smoke run 1.
5. **Advisory delivery needs a fork that runs.** With the self-recorded skip on, the first scope change (turn 7) skipped its fork. The operator had to ask the model not to record anything itself before a fork ran and superseded.

**Counts:** 8 asks, 42 requests, 45 tool calls, 3 forks (2 interview, 1 audit), 1 fork skipped, 14 patches, 7 captures (3 recorded, 3 judged, 1 refused), 1 reminder, 4 recalls, 2 seams, 2 deliveries.

## Scrubbed

In `session.log` and `record.jsonl`, the home directory is written `~`. Nothing else needed scrubbing. `product.txt` is unchanged.

**Not committed:**
- serve's saved copies of whole tool outputs (`files/`, named by digest in the log);
- serve's stdout.

## Files

`diet check-log session.log` and `diet check-record record.jsonl` both accept these files. I ran them at the build above. "repair" appears nowhere in the record or serve's output.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `6b1754027f8a82f8ab6fd4f79395246d246b917e818cbbcdbcf2de1879efda9e` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `50c33debb82bb8fefdf864f03158f79473c37b0171a4106f26a7397b382469f1` |
| `product.txt` | the working memory at end | `b7874047dd9fbc2d0c41275fe37125f5ac636ddf6d8a1f52d666307e49daccbd` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `399fce752d8c0924e548200781a3c13f446ea48120494561e848907a603c5aaf` |
| `regimen.toml` | the regimen as it ran | `6a405a6d9b787439e061b887cc3e349109aeb90b78a50473f712735c9cf4f53f` |
| `worktree-AGENTS.md` | the worktree's instruction file, as injected | `daf2bf78a7966e20c015edb6293fc8936390ee583e9db4f57680c98198421295` |
