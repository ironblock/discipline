# R3.0: the captures #117's R3 plan asked for, on the drive endpoint's server

These are captures C0–C5 of R3's plan (on #117), taken 2026-09-29 from 02:09:43 to 02:09:59Z against the running production server of `ada48-llamacpp-qwen38flashnext-q20`, instance `2026-09-28` (engine `e7051ef`), in `substrates/registry.toml`.

**The window.** It was ratified by the maintainer together with #142's window.

- These captures relaunched nothing.
- Earlier in the same window, production was relaunched three times with its identical line for #142's output-invariance test; its binary digest and command line were re-read identical after each relaunch (see #142).
- Every capture occupied one slot of four. The other slots were idle throughout:
  - none of the 27 `/slots` polls taken during C1 saw another slot busy (`notes.json`, `C1_polls_with_other_busy`);
  - none was busy before C3.
- A per-run nonce (`notes.json`) opens every prompt, so no prefix could be warm from anyone else's work.

**How they were taken.**

- `capture.py` reads the server's address and key from the machine's launch file. It never prints or writes the key.
- The raw files are **server-to-client bytes only**, exactly as received. The request direction carries the key and is not kept.
- For each request, `notes.json` records the digest and size of the request body, the response's status line, digest and size, and the counts named below.

| capture | file | response sha256 |
| --- | --- | --- |
| C0, limits | `limits.json` (key names and counts from `/props` and `/slots`, by GET, occupying no slot) | `f0152cb7…` |
| C1, multi-batch prefill | `diet/client/fixtures/llama-server-e7051ef-prompt-progress-stream.http` | `a966fb3a…` |
| C2, warm turn 2 | `diet/client/fixtures/llama-server-e7051ef-warm-turn2-stream.http` | `1bdd0a94…` |
| C3, overflow | `diet/client/fixtures/llama-server-e7051ef-context-overflow.http` | `634e1ce4…` |
| C4 | not taken: its condition, no progress frames in C1, did not hold | — |
| C5, unstreamed | `diet/client/fixtures/llama-server-e7051ef-unstreamed.http` | `d9d2e92e…` |
| C0b, build id (2026-10-01) | `build-info.json`, `build-info.raw` (one GET `/props`, occupying no slot); `engine-read.json` | `18033075…` |
| C0c, the floor's build id (2026-10-01) | `build-info-floor.json`, `build-info-floor.raw` (one GET `/props` on the floor's production, occupying no slot) | `6ce8fc6d…` |

## C0: the limits

- **Slots:** `total_slots` 4, and `/slots` lists 4.
- **Per-slot context:** `default_generation_settings.n_ctx` is **262,144**. Under `--kv-unified` this is the full serving context, not a quarter of it.

## C1: prefill progress

- **The request:**
  - a cold prompt of 9,276 rendered tokens, at least nine batches of `-b 1024`;
  - `stream: true`, `include_usage`, and **`return_progress: true`** (`tools/server/server-schema.cpp` at `e7051ef`);
  - pinned to slot 1.
- **Progress frames arrive.** Of 81 `data:` events, **13 carry `prompt_progress`**. Each rides a role chunk, `delta: {"role": "assistant", "content": null}`, and none rides a `choices: []` chunk.
- **The frame's keys:** **`total`, `cache`, `processed`, `time_ms`**. The first reads `{9276, 0, 0, 0}` and the last `{9276, 0, 9276, 6086}`.
- **The final chunk** keeps the shape of `llama-server-e7051ef-reasoning-stream.http`: `usage` and `timings` on a `choices: []` chunk.

## C2: a streamed warm turn 2

- **The request:** turn 1 (unstreamed) and turn 2 (streamed) were both pinned with **`id_slot: 2`**, with turn 1's `reasoning_content` re-sent on its assistant turn.
- **`id_slot` is honoured, and the streamed final chunk carries the cache count.**
  - Turn 1: `cache_n` 0, `prompt_n` 102.
  - Turn 2: `timings` reads **`cache_n` 160, `prompt_n` 18**.

## C3: the overflow

- **The prompt:** the smallest prompt found over C0's limit, **262,149 rendered tokens against 262,144**. It was counted by the server's own `/tokenize` on the rendered prompt, and **sent once**, streamed, with the other slots idle.
- **The reply:** **HTTP 400, before any prefill.** The whole request finished inside one second.

  ```
  {"error":{"code":400,"message":"request (262149 tokens) exceeds the available context size (262144 tokens), try increasing it","type":"exceed_context_size_error","n_prompt_tokens":262149,"n_ctx":262144}}
  ```

- **A typed field exists** (`type`, plus `n_prompt_tokens` and `n_ctx`), so `context_overflow` has a server field to map from.
- **No full-cache error was seen.**

## C5: one raw unstreamed reply

`timings` carries:

- **`cache_n` 42**;
- `prompt_n` 30;
- `draft_n` and `draft_n_accepted`.

What the reply does not carry, or carries twice:

- **`prompt_n_cached` is absent**, as #156 found from the earlier captures.
- **There is no `generation_settings`** in the chat reply, so the dialect's `sampler_echo` has nothing to read here.
- `usage.prompt_tokens_details.cached_tokens` (42) duplicates `cache_n`.

## C0b: the build id, taken later (2026-10-01)

One read-only GET `/props` was made at 2026-10-01T03:06:11Z by `capture_c0b.py` against the production server of the same substrate. It relaunched nothing and changed nothing.

- **What is kept.**
  - `build_info`'s JSON token as received is in `build-info.raw`: `"b8-e486f80"`, 12 bytes, sha256 `cdee465ac3cfba84e14389b7d8eb9c22c8707449b9517dd7c7c7b4e70661e366`.
  - Its decoded value `b8-e486f80` is 10 bytes, sha256 `f8a339722b87d1c1e1c77107ed73bfd74da8cbbbea252d6e965f58a5af6a3d17`.
  - The full response is not kept, because it carries the model path. Only its digest and size are recorded in `build-info.json`.
- **No other key carries a build or commit id.** At any depth of `/props`, no key name contains `build` or `commit`, and no string value is commit-shaped.
- **This is not the engine of C0-C5.**
  - C0-C5 ran on instance `2026-09-28`, engine `e7051ef`. The registry records that engine reporting itself as `system_fingerprint` `b7-e7051ef` on the chat reply.
  - At C0b, `/props` `build_info` reads `b8-e486f80`. These are two fields; both read as `b<n>-<short sha>`.
  - Read-only reads on the host minutes later, by hand-run commands rather than a committed script, are in `engine-read.json` (the commands are named in its `how`). No full response or path is kept there.
    - The running binary's sha256 is `f316bc7f…`, not the registry's `41e6591d…`. It was built 2026-09-30T22:19Z and launched 2026-10-01T01:10Z.
    - Its checkout's HEAD `e486f80` is one commit on top of `e7051ef`: a fix to QSA bias indexing when a unified cache holds several sequences, 1 file changed.
    - The binary is a 17,872-byte stub, so the digests of its linked libraries are recorded beside it.
  - C0b is therefore a receipt for the engine running on 2026-10-01. It is not evidence about C0-C5. The registry row (still `current` for `2026-09-28`) is not changed here; the new instance and the stub-pinning recipe are #202.

## C0c: the floor's build id (2026-10-01)

One read-only GET `/props` was made at 2026-10-01T06:47:22Z by `capture_c0c.py`, against the production server of `accel24-beellama-qwen27b-q4kxl`, on the instance #206 registers as `2026-10-01`. It relaunched nothing.

- **What is kept.**
  - `build_info`'s JSON token as received is in `build-info-floor.raw`: `"b0-unknown-dirty"`, 18 bytes, sha256 `dc5729d0d9a8864ab71d6557a1f2ed02ad33c7ccf9b01428d96f9a61c151779d`.
  - Its decoded value is 16 bytes, sha256 `c481b63c378b0d6d8ff78dacd19ee6baa1789192df1af649471b9d9fec72fb5a`.
  - No other key carries a build or commit id.
- **The engine names no commit, and no record fixes one.**
  - The engine is an unpacked prebuilt release. The registry already recorded `b0-unknown-dirty` as its self-report.
  - Upstream's release workflow rebuilt the release's assets on every push through June, and the tag was later re-cut more than once. So the commit the downloaded tarball was built from is unknown. The entry says so (`engine_commit_unknown`) and names the two candidates by upstream's workflow runs.
  - Its identity is the binary's digest, and its libraries' digests once #205 merges.
  - Under #157's ruling, the entry declares `engine_build_info = "b0-unknown-dirty"`. That is the literal a start-time check matches, and the check reports `engine_identity: unreported`.
