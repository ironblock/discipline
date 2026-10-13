# The floor on r3: follow-up window, canary, and the pool-edge counts (2026-10-13)

The maintainer approved a follow-up window on the floor, relayed by Dispatch and confirmed in the engine seat's session. It had three parts:
1. ABCCBA again, with long trunk outputs.
2. The canary on r3.
3. The pool-edge diagnostic.

**The end-state rule:** leave the floor on r3 if the canary is unchanged and r3 meets every pass criterion, otherwise restore r2. **r3 met them, so the floor serves r3.** It is instance `r3-2026-10-13` of `accel24-tabbyapi-exl3-qwen38-27b-3p00`.

**Window timing:** the endpoint went down at 00:59:59Z (r2 stopped). r3 has served through the stock launcher (`provision-exl3-27b.sh`, `config-floor.yml`) since 01:56:22Z.

## r3

`config-redacted.yml` is the live `config-floor.yml` (sha256 612743fc…) with three non-serving lines redacted (`config-redaction.diff`). It is r2 plus:

```yaml
model:
  recurrent_checkpoint_interval_pp: 8192   # default 32768
memory:
  sysmem_recurrent_cache: 6144             # default 4096
  sysmem_kv_cache: 6144                    # default 0: a second-tier KV cache in pinned system RAM
```

The reasons and the first window's measurements are in `substrates/measurements/2026-10-12-accel24-r3-window/`. `identity.json` is `substrates/admission/depth-probe/tabby_identity.py`'s output on the host. It gives engine identity `6d5aa379…`. Every component other than the config and its redaction note equals r2's.

## Pass criteria (the proposal's, as the maintainer set them)

| criterion | r3 | met |
|---|---|---|
| trunk hit ≥ 0.97 | min 0.973 (this window, long trunk outputs); 0.985 (2026-10-12) | yes |
| cold prefill ≤ +3% | 147.5 and 148.0 s against r2/r2kv's 145.3–146.8 s at 118k (+1.1%); +0.4% at 130k on 2026-10-12 | yes |
| MemAvailable ≥ 4 GiB | 9.5 GiB at its lowest (both windows) | yes |
| VRAM headroom ≥ 300 MiB | peak 22,606 MiB of 24,576: 1,970 free | yes |
| canary unchanged | **36/36, PASS** (`canary-r3.log`), against r2's 36/36 baseline; run on r3 as finally served, after the stock launcher brought it up | yes |

**The four confirmations on r3:**
- up: health 200;
- right model: `/v1/model` reports `Qwen3.8-27B-exl3-3.00bpw-img1024`, 163840/163840, 8,8, batch 2, vision on;
- warmed: warmup finished in 3.9 s, and a short completion answered "ready";
- required settings: the process runs `main.py --config config-floor.yml`, which holds r3's bytes (612743fc…) with the three r3 keys present.

## The long-trunk arms (`data/`)

**The setup:**
- **The trunk** starts at 118k and thinks at effort medium, with `max_tokens` 4,096. Its reasoning is kept in history: the floor renders it back (`preserve_thinking`), as T1's trunk does.
- **The forks** also think at effort medium, so trunk and forks share a prefix.
- 6 steps, no gap, ABCCBA.
- The harness is in `harness/`: `drive.py` with `R3_TRUNK_MODE=long`.

| arm | trunk hit min / median | longest queue wait s | trunk output tokens | live evictions | stranded | recurrent held / LRU-evicted | RAM tier push / restore | peak VRAM | min MemAvailable MiB |
|---|---|---|---|---|---|---|---|---|---|
| L1 r2, f32k | 0.972 / 0.979 | 1.4 | 8,756 | 0 | 0 | 23 / 0 | – | 22,566 | 21,110 |
| L2 r2kv, f32k | 0.970 / 0.977 | **161.3** | 24,659 | 197 | 0 | 27 / 19 | 189 / **65** | 22,598 | 11,563 |
| L3 r3, f32k | 0.973 / 0.980 | 1.4 | 7,950 | 0 | 0 | 33 / 0 | 0 / 0 | 22,590 | 10,768 |
| L4 r3, f8k | 0.975 / 0.978 | 1.4 | 28,672 | 116 | 0 | 41 / 14 | 108 / **9** | 22,582 | 9,534 |
| L5 r2kv, f8k | 0.969 / 0.978 | 1.4 | 9,059 | 0 | 0 | 23 / 0 | 0 / 0 | 22,582 | 12,247 |
| L6 r2, f8k | 0.974 / 0.978 | 1.4 | 7,155 | 0 | 0 | 21 / 0 | – | 22,598 | 21,538 |

**What the arms show:**
- **Output length varied by arm.** `min_tokens` did not hold the trunk to 4,096, and the model stopped at 1–2k in four of six arms. So pressure varied too: only L2 and L4, whose trunks wrote 25–29k tokens, evicted live pages.
- **Where pages were evicted, the RAM tier gave some back.** It restored them for later requests: 65 pages in L2, 9 in L4. A re-prefill would have rebuilt them otherwise.
- **One long queue wait.** In L2 the trunk's last request waited 161 s in the queue (`ttft` minus prefill time) while a 32k fork held the pages it needed. #826's "trunk requests took about 3 minutes" may be partly this, not only re-prefill.
- **No arm reproduced #826's 32k-grid fallback.** Trunk hits stayed at 0.97–0.98, where the missing 2–3% is the newest turn plus the reasoning re-render.
- **Uncontrolled factor.** Trunk output length is set by the model here, not the harness, so this window does not resolve r2 against r3 on hit rate.

## The pool-edge diagnostic (`diagnostic/`)

**The setup.** `harness/tabby_crashdiag.py` wraps ExLlamaV3's `Job.prepare_for_queue` and records the failing job's page inputs (`diagnostic/page-counts.jsonl`). It ran on r2, with B = 2,048, `num_draft_tokens` 7 (DFlash2) and 640 pages.

| case | `rq` | input | tokens left | aligned round (`max_rq_tokens`) | headroom | pages | outcome |
|---|---|---|---|---|---|---|---|
| admission | false | 160,783 | 32,768 | 3,057 (ends at 163,840) | 7 | ⌈163,847/256⌉ = 641 | that request refused; the generator carried on |
| requeue | **true** | 161,793 | 891 | 2,047 (ends at 163,840) | 7 | 641 | **`FATAL ERROR with generation` … `Immediately terminating all jobs`** |

**What the counts settle:**
- **Every round aligns to B.** It reserves the whole aligned round plus the draft headroom, even when fewer tokens remain: the requeued job had 891 left and reserved 2,047.
- **TabbyAPI checks only the first round, and without the headroom.** The requeue case's job totalled 162,684 tokens, under cache_size, and still took the generator down.
- **The bound.** A round starting at x is safe iff ceil((x − 1 + 2048)/B)·B + D ≤ cache_size, i.e. x ≤ 159,745 here. So **prompt + max_tokens ≤ cache_size − 2B** (159,744) is sufficient: serve's two-chunk rule.
- `requeue` here is `diagnostic/diag.jsonl`'s `B2-requeue`. Its first attempt stopped at 1,047 tokens, before any requeue, so a second attempt asked for a long list.

## Not measured
- Trunk hit under long outputs with output length controlled, which needs a way to hold `min_tokens` or a fixed long reply.
- r3 under the vision cell.
- r3 over a long T1 run's RAM.
