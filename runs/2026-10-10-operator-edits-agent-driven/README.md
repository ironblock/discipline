# Operator edits run — agent-driven

**Who drove it:** an agent (the surface seat), acting as operator through the exercise surface's `?drive` page. Not the maintainer. Agent-driven runs are approved as evidence, labelled as such.

**Purpose:** #150, the operator's edits and flags on working memory (#655, #657, #671), live on the floor:
- an edit from the surface;
- a flag with a note;
- the model's write over the edited entry refused;
- a seam that reconciles both.

**Record:** none. Serve refused its own record (break 1). `record.refused.jsonl` is the projection serve kept beside the empty record.

## Setup

- **Build:** develop `2972e890763902a111dfadd969acb2435261fa1e`.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2. Serve declared the engine and reported it warmed.
- **Regimen:** `regimen.toml` here, which is seam smoke run 2's with the arm renamed `operator-edits`. That gives:
  - fields-modality forks on ask set v4;
  - the seam audit on;
  - self-capture on.
- **Serve:** `diet-drive serve` with T1's session-one head, a 32768-token output cap, `--log` and `--record`.
- **Workspace:** a fresh, empty git repository. The task was a stdlib Python notes CLI.

## The run

1. Write `notes.py` (add, show). Settled `final`.
2. **Scope answer (marked scoping):** dated lines, newest first, plus `count`. The v4 fork landed `interview-t2-0` (decision) and `interview-t2-1` (plan).
3. Add `search WORD`. `final`.
4. **Edit, from the working-memory drawer:** the operator rewrote `interview-t2-0`.
   - Log `patch` 547: `op: supersede`, `lane: operator`, superseding `interview-t2-0` as `operator-547`.
   - **Operator slip:** the drawer's select-all did not take, so the operator's text went into the model's sentence instead of replacing it. The entry reads "decision: decision (operator): … must not change format.Implemented the scope decisions …".
5. **Flag:** the operator flagged `interview-t2-1` with "not true: search has no test for an empty notes.txt" (log `flag` 548).
6. **The overwrite (marked scoping):** "dates are now stored as YYYY-MM-DD HH:MM … the decision entry operator-547 … is out of date: resolve it as superseded with resolve_entry and record the new date format with update_record".
   - The model made the change.
   - It called `resolve_entry` on `operator-547` (capture 729, `judged`).
   - It called `update_record` (capture 731, `recorded`), whose supersede of `operator-547` was refused: `patch.refused` 732, `op: supersede`, `lane: self-capture`, `reason: operator-entry`.
   - No patch adds the model's new entry, so the new date format reached working memory nowhere.
   - The scoping fork was skipped because self-capture had already recorded the decision (`fork.skipped` 796).
7. **Seam (operator), seq 1079:** the reconcile, below.
8. Ended with confirm; serve exited.

## The seam's reconcile

- **The ask:** the audit fork (800) asked `audit_q_human_reconcile`. It explains the markers and asks for every marked note to be addressed: keep an edited note as written, and keep, update or remove a flagged one.
- **The render's markers:** the working set marks `operator-547` `[edited by the operator]`. The audit's list marks the plan `[flagged: not true: search has no test for an empty notes.txt]`.
- **`operator_changes`:** `[{entry: operator-547, kind: edit}, {entry: interview-t2-1, kind: flag}]`.
- **`unaddressed`:** `[]`.
- **The model addressed both:**
  - `1. UPDATE: plan: Implementation complete; add/show/count/search verified against a populated notes.txt, but search against an empty notes.txt is untested.` This is `patch` 1077, superseding `interview-t2-1`. The flag's concern held, and the plan now says so.
  - `2. KEEP`, the edited note, as the ask instructs.
- **As data:** the kept note is stale. The operator's text says dates "must not change format", and turn 4 changed them, at the operator's own ask. The reconcile keeps an edited note as written. Today an unaddressed change does not stop the refill; that is a question with the maintainer, and here nothing was unaddressed.
- The refill carried 2 entries: `operator-547` and the audit's updated plan.

## What broke

1. **Serve refused its own record because `unaddressed` was empty.**
   - At session end serve reported `record_refused: Schema(WrongType { of: "seam", field: "unaddressed", want: "a non-empty list of strings" })`.
   - The projection's seam row carries `"unaddressed": []`, and the record reader requires a non-empty list. So the success case, every change addressed, empties the record.
   - `diet check-record record.refused.jsonl` says the same: "a `seam` row's `unaddressed` is not a non-empty list of strings".
   - The log is whole, and `diet check-log` accepts it.
2. **A refused supersede also loses the write's new content.** The model's `update_record` was refused for superseding the operator's entry, and its new entry (the new date format) was not added beside it. Capture 731 still says `recorded`.
3. **The open working-memory drawer covers the composer's right-hand buttons** (scope answer, send) at the browser pane's width. The operator closed it to mark the ask.

**Counts:** 4 asks, 16 requests, 12 tool calls, 2 forks (1 interview, 1 audit), 1 fork skipped, 4 patches (1 by the operator), 1 refused patch, 1 flag, 2 captures, 1 reminder, 1 seam. All four turns settled `final`.

## Scrubbed

In `session.log` and `record.refused.jsonl`, the home directory is written `~`. `product.txt` is unchanged. Serve's stdout is not committed.

## Files

`diet check-log session.log` accepts the log. I ran it at the build above. `record.refused.jsonl` is refused by `diet check-record`, as break 1 says.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `b3d097f155d77d2f1ca914d4cc66a0365abf234a24e23e8ee47ffa270060a927` |
| `record.refused.jsonl` | the projection serve kept beside the empty record, scrubbed | `e6b6114efaca7d7b900de534d003801f718258e01c08e95a82d62b363bb235ba` |
| `product.txt` | the working memory at end | `f6919b137b352cd9348f728b0ac8faf7e3dd5b06d1a0fec24713f8cfc7ffb6fb` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the projection could not spell | `1df93041661b857c158e3f650276e0e5da079935cb4b829dbe57c9b48af1a08c` |
| `regimen.toml` | the regimen as it ran | `b0478e4ad81d546df086bad4fcac7e09a935ae62ac450098b1d083d047a4e509` |
