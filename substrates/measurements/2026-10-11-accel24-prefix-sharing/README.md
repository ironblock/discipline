# Does the floor share a fork's prefix pages with the trunk's? (2026-10-11)

**Asked by:** #406 (pool-aware forks) and #782 (interviews during the turn). Planning's recall calls this the predecessor's most load-bearing unverified question.
- If a fork cut from the trunk needs its own copy of the trunk's pages, a mid-turn fork can't fit beside a trunk past about half the pool.
- If the pages are shared, a fork costs only its own new tokens.

**Instance:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2: one paged KV pool of 163,840 tokens, `max_batch_size` 2. Measured over the endpoint only, by Dispatch, with the maintainer's OK.

**Method:** `measure.py ENDPOINT MODEL LINES SEED`:
1. Warm a filler prefix with one request.
2. Send two requests that share it, two seconds apart, each with its own short question.
3. As a control, send a request on the warm prefix beside one on a different prefix of the same size.

Every request streams, at `temperature` 0 with `reasoning_effort` low. The engine's own `timings` are read off each stream's end.

## What it showed

| prefix | request | cache_n | prompt_n | prompt_ms | ran beside the other |
| --- | --- | --- | --- | --- | --- |
| ~70k (3,500 lines, seed 1) | B, same prefix as A | 69,888 | 164 | 740 | yes: first token at 3.5 s, A ended at 5.1 s |
| ~100k (5,000 lines, seed 3) | A | 99,840 | 211 | 930 | yes |
| ~100k | B, same prefix as A | 99,840 | 212 | 930 | yes: first token at 4.1 s, A ended at 5.5 s |
| ~100k | C, a different prefix | 0 | 100,049 | 116,470 | its full prefill |

**At ~100k, two separate copies of the prefix can't fit** in the 163,840-token pool, since 2 × 100k is more than that. Yet A and B ran at the same time, each served 99,840 tokens from cache, and each prefilled only its ~210 new tokens. So concurrent requests on one prefix reference the same pages: **the engine shares them.** A fork cut from the trunk's prefix costs the pool only its new tokens.

**The control doesn't show a refusal.** By the time C's chunked prefill needed the pages, A2 had finished and the warm prefix's pages could be evicted. It shows only an uncached prefill's cost: about 860 tokens/s at ~100k.

Raw output, with the endpoint redacted: `result-70k.txt` and `result-100k.txt`.
