# Seam smoke run 3 — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page. Not the maintainer. He approved it as evidence, labelled as such.

**Purpose:** confirm live the fixes for what seam smoke run 2 found. Run 2 (agent-driven) is not committed, because its record was never written. It found three faults:
- the record could not be written: an audit request's prefix change had no name the record format reads back (#656, fixed by #658);
- archive recall cut a long item from its start, so the matched text could fall outside the note (#566, fixed by #659);
- a background job that serve's exit stopped was never logged ended (#614, fixed by #660).

## Setup

- **Build:** develop `75f38534b0372f42089a164e185f0e738db2fecf`, the merge of #660 (it includes #658 and #659).
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2. Serve declared the engine and reported it warmed.
- **Regimen:** `regimen.toml` here, run 2's, unchanged. It is run 1's plus:
  - `seam_audit = "on"` and `seam_warm = "on"`;
  - `archive_recall = "literal"`;
  - `interview_cadence = "per_class"`;
  - `interview_skip_self_recorded = "on"`;
  - `render_budget_tokens = 400`.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`.
- **Workspace:** a fresh scratch directory. The task was a stdlib Python Markdown link checker.

## The run

1. Ask for the whole checker. Settled `timeout`: serve's then-fixed 180 s per-call deadline cut the request while reasoning was still streaming (break 1).
2. Step 1 only: `linkcheck.py`'s 44-line docstring, a stub `main()`, the guard. `final`.
3. Step 2: the checker, plus `heading_slug(text)` defined just above the guard. In the written file, `def heading_slug` sits at character 5,550 of 5,860. `final`.
4. **Scope answer (marked scoping):** local links only, `file:line: target`, exit 1, `#anchor` checks. The v4 fork landed 2 entries. `final`.
5. Ask for a loopback `http.server` as a background job. The self-capture reminder displaced the ask (break 2). `final`.
6. The same ask again. The model started `python3 -m http.server 8765 --bind 127.0.0.1 --directory docs` with `is_background` (job `bg_4aaed683`) and confirmed HTTP 200. `final`.
7. **Seam 1 (operator):**
   - the audit retired 1 entry;
   - the render was 258 tokens;
   - warm was recorded;
   - the refill carried 4 entries.
8. Reference-style links and `--json`. Recall fired. `final`.
9. **More scope (marked scoping):** drop `--check-remote`, add duplicate-heading suffixes. Recall fired. Self-capture recorded the decision, and the scoping fork was skipped as already recorded. `final`.
10. **Seam 2 (operator):**
    - the audit superseded 2 entries;
    - the render was 303 tokens;
    - warm was recorded;
    - the refill carried 5 entries.
11. **Recall ask 1:** "the helper heading_slug from the very first version of linkcheck.py: what did its body do?" `final`.
12. **Recall ask 2,** naming only `heading_slug`. `final`.
13. Ended with confirm, with `bg_4aaed683` still running. Serve exited, and port 8765 was free afterwards.

The operator asked for the server on 127.0.0.1 only, not every interface. Its own tool policy refused to send the first wording.

## What it showed

**The record (#658): held.**
- Written, 261 rows. `diet check-record` accepts it.
- No row needed repair: "repair" appears nowhere in the record or serve's output.
- Seam 2's audit request, which broke run 2, has its prefix-change row.

**Turn rows: 10 of 10.**

| turn | row |
| --- | --- |
| 1 | `{"index":1}`, bare: the timed-out turn; the sidecar says it has no usage |
| 2–6 | prefill 2,699 / 5,925 / 14,847 / 23,395 / 25,294 (from usage) |
| 7 | 4,049 (after seam 1) |
| 8 | 26,012 |
| 9 | 4,107 (after seam 2) |
| 10 | 6,697 |

The fork rows are `f/8355` (interview), `f/9164` and `f/14823` (audit), each with its capture. The seam rows are `s/10393` and `s/15008`.

**The recall window (#659): held, with two gaps.** Four recalls, 3 items each:

| ask | items (score) | windowed? |
| --- | --- | --- |
| turn 8 | seam-6 messages 48, 45, 29 (2) | no; all under 1,200 characters |
| turn 9 | seam-6 messages 48, 45, 35 (1) | message 35 cut at its end |
| turn 11 | seam-8 messages 50, 46, 45 (2) | message 46 cut at its end; message 45 at both ends |
| turn 12 | seam-8 messages 50, 46, 45 (1) | messages 46 and 45 at both ends |

- **The window works.** Seam-8 message 45 is a 9,749-character `read` of `linkcheck.py`.
  - At turn 12 its note started `[…]` at character 4,097 and ended `[…]`.
  - It held `heading_slug` (the call site in `collect_headings`): a match far past 1,200 characters, kept.
- **First gap: the window centres on the first anchor in the text, not the most specific one.**
  - At turn 11 the ask also named `linkcheck.py`. The window centred on that name's first occurrence (character 928, the docstring's usage line), and `heading_slug` (5,814) fell outside.
  - At turn 12 the first `heading_slug` is a call site, which comes before the definition.
  - So the definition was not shown either time.
- **Second gap: ties go to the newest archive.** Turn 3's original write holds `def heading_slug`, in seam 1's archive. It was never offered: every item tied on score, and all three slots went to the most recent archive.
- The model said both times, honestly, that it had never been shown the original.

**The background job's end (#660): held.** `{"kind":"background.ended","job":"bg_4aaed683","status":"cancelled",…}` at seq 15472 came before the `ended` settlement at 15474. Serve copied the job's output.

**The job id across a seam: it did not cross.**
- Neither seam's render holds `bg_4aaed683` or `8765`, because both of the model's attempts to record the job were dropped (break 3).
- No later ask needed the id, and unlike run 2 the model did not try to stop the server.

**Fired as declared:**
- the v4 interview fork;
- the skipped self-recorded fork;
- the audit and pre-warm on both seams.

The render budget did not bite.

## What broke

1. **Serve's fixed 180 s per-call deadline cut a long `xhigh` request mid-stream.**
   - Turn 1 failed with `timeout` after 4,870 deltas, with reasoning still arriving 31 ms before the cut.
   - #663 has since replaced it with an idle deadline between chunks, a regimen value.
2. **The self-capture reminder displaced an ask.** On turn 5 the reminder was the last user message, after a short ask. The model answered only the reminder, ran 4 `update_record`s and no command, and the operator re-sent the ask.
3. **The groundedness gate dropped grounded pointers.** On turn 6 two `update_record` pointers named `bg_4aaed683`, which was in a tool result one step earlier, and both were dropped ("kept nothing of it"). 5 of 10 captures were dropped in all. This is why the id reached no seam.
4. **The recall window centres on the earliest anchor in the item,** as above.
5. **Recall ties go to the newest archive,** as above.

The model also wrote scratch fixtures to `/tmp` first, then moved them inside the worktree and deleted them. This is noted, not a break.

**Counts:** 10 asks, 58 requests, 72 tool calls, 3 forks (1 interview, 2 audits), 1 fork skipped, 9 patches, 10 captures (4 recorded, 5 dropped, 1 judged), 1 reminder, 4 recalls, 2 seams, 1 `background.ended`. Settles: timeout, then 9 × `final`. All 57 responses carry `usage`.

## Scrubbed

In `session.log`, `record.jsonl`, `receipt.json` and `unspellable.json`:
- the session's scratch directory is written `/scratch`;
- the home directory `~`;
- the account name `user`.

Digests inside the log and record were taken over the unscrubbed bytes. `product.txt` is unchanged.

**Not committed:** serve's copies of the job's output (`background/`, `files/`) and serve's stdout. The log names the output file by digest.

## Files

`diet check-log session.log` and `diet check-record record.jsonl` both accept the scrubbed files. I ran them at develop `2f9501a0`.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `e135237ec53d7101727b06940da29af2001176072561851eb27f8b43ed9a256f` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `b5e853df55c41b2af76c4b1abf4f77e8a775be0123934fa513fa2548a64a5ab6` |
| `product.txt` | the working memory at end | `baaa333321476a7c3239bad722be42d60ddc203b5a6a6a936e24465242b7f263` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `a320421a6f708d7477f864210f39070c7d1ddfa3098d1e827ecdb4cc7df54faa` |
| `regimen.toml` | the regimen as it ran | `0c2d0b35a201dce084d6dcc16a5b13d591a84d7715991b46a8b433333ea492f2` |
