# T1 run 3 — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page, following `docs/trajectories/t1.md`. Not the maintainer. He approved it as evidence, labelled as such.

**This is not a finished T1.** The run stopped during step 7.

**Finding:** T1's script at `xhigh` does not fit the floor's 163,840-token window without a seam.

## Setup

- **Build:** develop `1bb5309a`, with #576–#582 and #584.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2. Serve declared the engine and reported it warmed.
- **Regimen:** `regimen.toml` here. It is T1 session one's regimen as it ran, plus `tool_surface = "standard"`:
  - approvals off;
  - reasoning `xhigh`;
  - the sampler card 1.0 / 20 / 0.95 / 0.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`.
- **Workspace:** a scratch copy of the experiments directory, in place of `~/git/experiments/babylon-lite-minecraft-fps`.

## What it showed

| T1 row | shown? | where |
| --- | --- | --- |
| Ask | yes | turns 1–2: reasoning, then an answer, settled `final` |
| Tools, unprompted | yes | 117 bash calls under Seatbelt; reads of the reference source outside the worktree; 13 writes and 25 edits inside it; grep and glob |
| Image | yes | turn 5: a PNG of the model's own game, named by path in the ask, attached and drawn. The model's reasoning describes the terrain it shows |
| Interview fork | yes, off-content | after the operator's marked scope answer, the fork landed 2 patches about the session's directory and servers, not the operator's three scope decisions |
| Cancel and end | yes | turn 3: a hung call was cancelled, and the turn settled `cancelled` 16 ms after the stop. Ended with confirm, and serve exited |
| Record | partly | written, with no turn rows (see below) |
| Seam | n/a | T1 has no seam |

The model did not find the reference's own example. It asked no scope questions before building.

## What broke

1. **The context window filled, and the run stopped there.**
   - By turn 5 the trunk's prompt was 159,289 tokens. The floor serves a 163,840-token window.
   - The engine refused the next job. Its console said: "Job requires 641 pages (only 640 available) and cannot be enqueued. Total cache allocated is 640 * 256 = 163840 tokens". TabbyAPI then answered with its catch-all "Chat completion aborted."
   - Serve logged `request.failed`, reason `server`, not `context_overflow`.
   - A failed turn keeps its steps, so the next ask failed the same way 0.6 s after it was sent.
   - Step 8's feedback loops cannot run without a seam or some other way to shrink the trunk.
2. **A hung command had no timeout.** `pnpm exec playwright install chromium` hung for 1h46m with no connection, until the operator's cancel ended it. The 79 finished steps were kept.
3. **The record holds no turn rows.** On this engine, turn 1's `prefill_tokens` read "equality unmeasured", and turn rows must run from 1 without a gap. Usage on responses (#654) has fixed this since.
4. **Model behaviour, as data:**
   - it believed it could not write to the worktree, though every write landed;
   - it left dev servers running, which serve's exit cleaned up.

**Counts:** 137 requests, 160 tool calls, 1 fork, 2 patches. Settles: final, final, cancelled, final, failed, failed.

## Scrubbed

In `session.log`, `record.jsonl`, `receipt.json` and `unspellable.json`:
- the session's scratch directory is written `/scratch`;
- the home directory `~`;
- a LAN address (Vite's "Network:" line) `LAN-ADDRESS`;
- the account name `user`.

Digests inside the log and record (`head_sha256` and the like) were taken over the unscrubbed bytes. `product.txt` is unchanged.

**Not committed:** the operator's step-7 screenshot and serve's copy of it, and serve's stdout. The log names the screenshot by digest.

## Files

`diet check-log session.log` and `diet check-record record.jsonl` both accept the scrubbed files. I ran them at develop `2f9501a0`.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `bc47c9272f180632168ffdf2728ad2873b9a9e51616d1648c65baeb1aa442172` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `3f18b28ff597f38515f1316bfd223dfed8ff97316428727f4bfc1b058e822db9` |
| `product.txt` | the working memory at end | `2febf4f29ad42995dbb13d43693801229024de36c57dd436b845588dd948f148` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `cb5d487785d2ab55454714c3a8460cce8098f24ef369b4909e5f79c31552e193` |
| `regimen.toml` | the regimen as it ran | `7158d03aa59d2fdc53675b599f9884ddd91ed73e80082719ac6f76a808bbf1e6` |
