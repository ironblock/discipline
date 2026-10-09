# The parity fire on the TabbyAPI line, 2026-10-05: void, the engine crashed (#393)

The fire under `parity/PREREGISTRATION-tabbyapi.md` did not complete. The engine process died with a GPU out-of-memory assert
(`exllamav3_ext/graph.cu 51`) during seat A. **No word is read from this fire**; it is void under the pre-registration (a
proxy that saw fewer checks than requests, an engine that stopped answering). Nothing here is an admission result.

## What happened (UTC, one window)

| step | result |
|---|---|
| identity before the first fork | matches the step-1 record, every component |
| `/health` and `/v1/model` before | 200; id `Qwen3.8-27B-exl3-3.00bpw-img1024` |
| rehearsal through proxy A | the harness's one request shape (`chat_template_kwargs` enable/preserve thinking, top_k, top_p, temperature) answered 200 by the engine; the check list gained one entry equal to the pinned id |
| seat B's CPU server pre-fire answer | no think block, no `draft_n` |
| engine canary before | 36/36, PASS against a baseline drawn minutes earlier (`canary-baseline.json`) |
| seat A | started 07:24:22; the engine died at 07:34:39, 10 min 17 s in |
| seat A, seat B | both exited 1 with `server error 503` after the crash |
| canary after | could not run (connection refused) |
| identity after the crash | matches the step-1 record, every component (the install is unchanged; the process is gone) |

## The crash

- The last request the engine logged as started, `#499`, carried **20,482 prompt tokens** (`max_tokens` 4096); it never logged a completion. The assert is the last line of the engine log.
- The window held **one request at a time**: the 60 requests in `crash-log-tail.log` (#379 to #499, prompts 4,922 to 27,154 tokens) have no overlap, each starting after the previous finished. One of the configured two slots was in use.
- Prompt-cache reuse was high (86% to 99% on the requests just before). The crash request was not the largest of the run (27,154 was); nothing in the log says what allocation failed beyond the assert.
- Seat A's proxy recorded **142** `/v1/model` checks, every one equal to the pinned id; seat B's recorded 1 (its first request was the 503).
- GPU memory after the crash: 21 MiB used (the process was gone, not hung).

## Against the headroom cell

Step 2's headroom fill (`raw/fill.json`: two concurrent requests of 97,242 tokens, peak 23,748 MiB, **828 MiB free at peak**, bar 300 MiB) passed. The bar as amended is "flat 300 MiB free or measured no-OOM" (#143, 5984376546). This engine OOMed under a far lighter, single-stream load, so the line does not clear the amended bar as measured: the fill measures free memory at peak under a fixed fill, and the engine's later allocation at a 20k-token request was not covered by it.

## What was not done

No re-fire (it would measure the same crash on the same config). Steps 5 (vision cell) and 6 (the #421 capture) need the engine and did not run. The engine was not restarted by this seat; the floor's restore belongs to Track 6.

Files: `crash-log-tail.log` (the engine log's last 400 lines; the assert's source path is the upstream wheel's build directory, its CI runner prefix written as `<upstream-build>`), `checks-A.json` / `checks-B.json` (the proxies' `/v1/model` ids), `identity-before.json` / `identity-after.json`, `canary-before-1.log`, `canary-baseline.json`. `SHA256SUMS` covers them.

## Addendum, 2026-10-05: the fire alone did not reproduce the crash (#393 5997098752)

A side run on the unchanged floor config replayed seat A's request mix directly against the endpoint, with VRAM sampled at 1 Hz by the engine seat. These are measurements, not admission words.

| arm | passes | requests | VRAM after load | max used | free at max | OOM |
| --- | --- | --- | --- | --- | --- | --- |
| A, as served | 3 complete (13-14 min each), a fourth cut short by the operator | 667 | 22,872 MiB | 23,738 MiB | 838 MiB | none |
| B, `EXL3_BC_ATTN=0` | 1 complete (13 min) | 181 | 22,810 MiB | 23,574 MiB | 1,002 MiB | none |

- Arm A's used memory levelled off from about minute 26. Arm B showed the same curve shape.
- The earlier crash did not reproduce under the fire alone. The engine seat's unverified read is that the window's deep steps (the two-request 97k fill and the 186k depth probe) left extra memory held when the fire began. That is a hypothesis; this run does not test it.
- The sustained, varied headroom cell (#480) is the instrument that would measure it.
- The raw VRAM series stay on the host and are not in this record.
