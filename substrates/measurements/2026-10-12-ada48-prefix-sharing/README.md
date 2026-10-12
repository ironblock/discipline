# Does Flash-Next's server share a fork's prefix pages with the trunk's? (2026-10-12)

**Asked by:** the agent-driven T1 retry on this instance. In its build turn, 22 interview forks cut during the turn were refused `pool`. The entry declared no `served_prefix_shared`, so each fork beside a ~198k-token trunk was counted as a second ~198k copy (#797). The floor's server shares (`../2026-10-11-accel24-prefix-sharing/`, #796), and this is the same engine. This measures whether this instance does too.

**Instance:** `ada48-tabbyapi-exl3-qwen38flashnext-2p05`: one paged KV pool of 606,208 tokens (`cache_size`), `max_seq_len` 524,288, `max_batch_size` 4. Measured over the endpoint only, by Dispatch, while the instance was otherwise idle.

**Method:** the floor's `measure.py`, run as `KEY_FILE=<file> measure.py ENDPOINT MODEL 16000 3`. Its one change is that it sends the endpoint's key, read from the file `KEY_FILE` names, as a bearer credential.
1. Warm a ~320k-token filler prefix with one request.
2. Send two requests that share it, two seconds apart, each with its own short question.
3. As a control, send a request on the warm prefix beside one on a different prefix of the same size.

Every request streams, at `temperature` 0 with `reasoning_effort` low. The engine's own `timings` are read off each stream's end.

## What it showed

| request | cache_n | prompt_n | prompt_ms | ran beside the other |
| --- | --- | --- | --- | --- |
| warm | 0 | 320,048 | 104,990 | alone |
| A | 320,000 | 51 | 90 | yes |
| B, same prefix as A | 320,000 | 52 | 90 | yes: first token at 4.2 s, A ended at 5.8 s |
| C, a different prefix | 0 | 320,049 | 107,610 | its full prefill |

**Two separate copies of a ~320k prefix can't fit** in the 606,208-token pool. Yet A and B ran at the same time, each served 320,000 tokens from cache, and each prefilled only its ~51 new tokens. So concurrent requests on one prefix reference the same pages: **the engine shares them.** A fork cut from the trunk's prefix costs the pool only its new tokens.

**The control doesn't show a refusal.** A2 finished before C's chunked prefill needed the pages, as on the floor. It shows only an uncached prefill's cost: about 3,000 tokens/s at ~320k.

Raw output: `result-320k.txt`.
