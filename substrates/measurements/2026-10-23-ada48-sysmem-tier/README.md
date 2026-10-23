# 2026-10-23: rtx6000ada-host's TabbyAPI line gains the host-memory KV tier

The registry's `ada48-tabbyapi-exl3-qwen38flashnext-2p05` has a new instance, `2026-10-23`. It is instance 2026-10-12b with three keys added:
- `model.recurrent_checkpoint_interval_pp 8192`;
- `memory.sysmem_kv_cache 32768`;
- `memory.sysmem_recurrent_cache 16384`.

The maintainer's condition was "on unless it's broken". The tier restores bit-identically, so it is on.

## Files
- `config-redacted.yml`: the live config, with the four replacements listed in `config-redaction.diff`. Against 2026-10-12b's redacted config it differs in exactly the three keys above.
- `identity.json`: `substrates/admission/depth-probe/tabby_identity.py`, run on the host with the serving venv. The composite is `6c51e914…`. Against 2026-10-12b:
  - the driver went from 615.71.09 to 615.71.10, the host's package upgrade of 2026-10-22, which this line had not yet recorded;
  - the config and its redaction note changed;
  - nothing else differs.
- `kvtier.jsonl`: twelve requests in three arms. Each row carries token counts, prefill time and a sha256 of the output. The output text is not committed.

## The tier works
P is a 278,500-token prompt cut from a public source tree, with a one-paragraph question after it. E1 (245,107 tokens) and E2 (268,041) are disjoint prompts cut from the same tree. Each arm sends P, E1, E2, then P again, one at a time. Every request is greedy and returns 64 tokens.

| arm | P, cold | P after E1 and E2 |
|---|---|---|
| tier off (2026-10-12b's config) | 278,500 new, 98.0 s | 278,500 new, 0 cached, 98.6 s |
| tier on, disposable restart | 278,500 new, 105.0 s | 228 new, **278,272 cached**, 0.15 s |
| tier on, production (instance 2026-10-23) | 278,500 new, 102.1 s | 228 new, **278,272 cached**, 0.15 s |

- **Correctness:** P's output digest is the same in all six P rows. That is restored against recomputed, and each against the cold runs.
- **Cost:** cold prefill is 4–7% slower with this config (two runs against one).
- **Sizing:** one recurrent checkpoint of this model is about 113 MB, so a 300k prefix at interval 8192 needs about 4.2 GB of checkpoints. The default `sysmem_recurrent_cache` of 4096 MB would not hold one.
- **Host memory:** after the tier filled, 150 GB of 251 was still available.

## Throughput under batching (why the grading config stays at four slots)
A grading-shaped config was tried and rejected; nothing from it is served:
- max_batch_size 16 at MTP 3 with a 262,144-token pool;
- max_batch_size 32 with drafting off.

Max_batch_size 32 at MTP 3 does not load. The recurrent state is fp32 per slot and per draft-history step: 36 layers × 4 × 48 × 128 × 128 × 4 B ≈ 453 MB per slot.

The load was single requests of about 8.4k prompt tokens each, at xhigh. In every arm:
- requests per second stayed at 0.15–0.21 from concurrency 1 to 32;
- per-request decode was about 166/c with MTP, and 90/c with drafting off.

The card draws its 300 W power limit at 80–87% utilisation even at concurrency 1, so more slots divide a fixed budget. MTP acceptance held between 0.58 and 0.71 at every level.

## The pool edge, re-measured
Tested on a disposable 32,768-token pool (chunk 2048, MTP 3), with `min_tokens` forcing full length (`ban_eos_token` is ignored by this backend).
- A single job whose prompt + max_tokens was cache_size − 2,060 ran to length.
- The same at cache_size − 1,548 and − 1,036 was admitted, then failed with "FATAL ERROR with generation … Immediately terminating all jobs" mid-generation. This is the registry's `under_pressure_is` case.
- Two concurrent jobs whose combined reservations reached cache_size − 1,024 both ran to length.
- After a fatal, `/health` answers 503 while requests still succeed.

The entry's `served_cache_size` (524288) and `served_chunk_size` (2048) are unchanged, so diet's existing clamp still applies: prompt + max_tokens at or under cache_size − 2 × chunk_size.
