# The floor's prefix-cache retention: r2 against r3 (2026-10-12)

#826 saw the floor's trunk hit rate fall to 0.54 above about 120k tokens, with `cache_n` stuck at multiples of 32,768 whenever forks ran beside the trunk. The engine seat read ExLlamaV3 1.5.4's cache code and proposed a config r3 (#826 thread). The maintainer approved this window to measure r3 against r2 under that pattern.

**Short answer:**
- **r3 does what it was built to do.** Under pool pressure, the second-tier KV cache in system RAM leaves no checkpoint stranded, and the larger recurrent store keeps more of them.
- **Its costs are small:** cold prefill +0.4%, the same VRAM, and about 11 GiB less free system RAM at worst.
- **This window did not reproduce #826's trunk misses.** The trunk hit 0.98 on every arm, r2 included. The likely reason is that this harness's trunk turns write about 60 tokens, where T1's write long reasoning (see *What it does not show*).
- **Not run: the canary,** one of the proposal's pass criteria.
- **A separate finding:** a request sized near the pool edge crashes the generator and cancels every in-flight request (see *The pool-edge crash*).

## The three configs (`configs/`, redacted like r2's receipts: the port and the two model directories)

| arm | config | what differs from r2 |
|---|---|---|
| A | `cfg-r2.yml` (live config-floor.yml, live sha256 1e17c49d…; this redacted copy is dd281654…, byte-identical to the 2026-10-05 r2 receipt) | none |
| B | `cfg-r2kv.yml` | `memory.sysmem_kv_cache: 6144` |
| C | `cfg-r3.yml` | + `memory.sysmem_recurrent_cache: 6144` (default 4096) and `model.recurrent_checkpoint_interval_pp: 8192` (default 32768) |

Each arm ran on a fresh server process:
- **launch:** `harness/arm.sh` stops the server by PID and starts the config under `harness/tabby_metrics.py`;
- **instrumentation:** `tabby_metrics.py` runs TabbyAPI's `main.py` unchanged, with a read-only hook that appends ExLlamaV3's own counters (`pagetable.metrics`, `recurrent_cache.metrics`, `cpu_page_cache.metrics`) to `data/box/metrics-<arm>.jsonl` at each job's end and every 10 s;
- **sampling:** nvidia-smi memory, `MemAvailable` and the server's RSS at 1 Hz, in `data/box/samp-<arm>.csv`.

## The load (`harness/drive.py`)

- **Shape.** A trunk with one interview fork always running beside it: #826's `beside: 1`, cut from the trunk's history the way T1's forks are.
- **Step 0.** A cold 130,059-token trunk prompt (llama.cpp C/C++/CUDA source), thinking off, `max_tokens` 4096.
- **Steps 1–7.** Trunk turns 75 s apart, each appending about 2k tokens of new source as a tool result. The trunk grows 130k → 144k.
- **The fork.**
  - It is the trunk's current history plus an interview question.
  - Thinking runs at `reasoning_effort: medium`; the reason is under *Harness lessons*.
  - Its cap is min(32,768 or 8,192, pool − its prompt − 8,192), with `min_tokens` set to the cap.
  - A new fork starts whenever the last one ends.
- **Cold sample.** Each arm ends with one more cold 130,042-token prompt, so each arm gives two cold-prefill samples.
- **Order.** ABCCBA: arms 1–3 with forks capped at 32,768, arms 4–6 at 8,192. One server restart per arm, nothing else changed.
- **Timing.** Arms ran 19:53–22:04Z, and r2 was restored through the stock launcher at 22:04:45Z.

## Results (`data/summary.txt`, from `data/results.jsonl` and the end-of-arm counters)

| arm | trunk hit min / median (7 steps) | cold prefill s | live page evictions | checkpoints stranded | recurrent: held / GiB / live ones evicted by LRU / stranded ones pruned | RAM tier: pushes / restores | peak VRAM MiB | min MemAvailable MiB |
|---|---|---|---|---|---|---|---|---|
| 1 A r2, forks 32k | 0.984 / 0.984 | 167.3, 167.6 | 596 | **31** | 7 / 1.01 / 0 / 26 | – | 22,574 | 20,649 |
| 2 B r2kv, 32k | 0.984 / 0.985 | 167.7, 167.8 | 652 | **0** | 27 / 3.90 / **7** / 0 | 587 / 0 | 22,594 | 11,548 |
| 3 C r3, 32k | 0.985 / 0.985 | 168.5, 168.1 | 634 | **0** | 41 / 5.92 / 4 / 0 | 572 / 0 | 22,606 | 9,488 |
| 4 C r3, 8k | 0.985 / 0.985 | 168.5, 168.1 | 585 | **0** | 41 / 5.92 / 4 / 0 | 541 / 0 | 22,596 | 9,563 |
| 5 B r2kv, 8k | 0.983 / 0.985 | 167.6, 167.9 | 618 | **0** | 27 / 3.90 / 6 / 0 | 565 / 0 | 22,612 | 11,583 |
| 6 A r2, 8k | 0.984 / 0.984 | 167.5, 167.8 | 565 | **32** | 7 / 1.01 / 0 / 23 | – | 22,580 | 20,446 |

What the table shows:
- **The pool was under real pressure in every arm:** 565–652 complete, hashed pages were evicted from VRAM per arm.
- **On r2, eviction strands checkpoints.** About 31 per arm lost the pages under them and became unrestorable; the idle-time `prune_stranded()` then deleted them (23–26).
- **The RAM tier (B, C) ends stranding.** Every evicted page went to pinned system RAM (540–590 pushes), so no checkpoint lost its anchor. None was restored, because the trunk never asked for an evicted page.
- **With nothing stranded, the 4 GiB recurrent store fills.** It held 27 checkpoints and evicted 6–7 live ones by LRU. r3's 6 GiB store at the 8,192 grid held 41 and evicted 4.
- **r3's costs:**
  - cold prefill 168.1–168.5 s against 167.3–167.9 s on r2 and r2kv: **+0.4%** for the denser checkpoint grid;
  - peak VRAM unchanged (22.6 GiB, about 1.96 GiB free of 24);
  - lowest `MemAvailable` 9.5 GiB, against 11.5 on r2kv and 20.4 on r2.
- **A recurrent checkpoint is 148 MiB** (1.549 GB over 10 stashes in pilot 2): 48 GDN layers' fp32 state plus conv state. The DFlash2 drafter's state is not in it.

**r3 against the proposal's pass criteria:**
- trunk hit ≥ 0.97: met (0.983–0.985);
- cold prefill ≤ 3% slower: met (+0.4%);
- MemAvailable ≥ 4 GiB: met (9.5);
- VRAM headroom ≥ 300 MiB: met (1,964);
- canary unchanged: **not run** in this window.

## What it does not show

**#826's trunk misses.** No arm, r2 included, dropped below 0.98.
- **The likely reason.** The eviction policy is tree-shaped: the least-recently-used root goes first, pruned tail-first. Here the pages eaten first were finished forks' outputs and old tails the trunk no longer needed. This harness's trunk writes about 60 tokens per turn. T1's trunk writes long reasoning, so its own freshly finished tail is the unreferenced leaf that eviction reaches, and the checkpoints near the end of its prompt strand with it.
- **Proposed follow-up (needs a window):** the same ABCCBA, with trunk turns generating long output (thinking on at effort medium, `min_tokens` 4,096).

## The pool-edge crash (pilot 4) and the arithmetic behind it

**What happened.**
- In pilot 4, a fork's prompt plus `max_tokens` was 130,140 + 32,768 = 162,908, against the 163,840-token pool.
- About 6 minutes in, the fork had generated about 31k tokens: it runs at about 85 tok/s alone between trunk turns.
- Then the server logged (`pilots/box/pilot4-crash-tabby-1-A-r2-f32768.log`):

  ```
  ERROR:    FATAL ERROR with generation. Attempting to recreate the generator. If this fails, please restart the server.
  WARNING:  Immediately terminating all jobs. Clients will have their requests cancelled.
  ERROR:    Sent to request: Job requires 641 pages (only 640 available) and cannot be enqueued. Total cache allocated is 640 * 256 = 163840 tokens. The request exceeds the available context size.
  ```
- The trunk's in-flight request was cancelled with it.

**Why: TabbyAPI's admission check and ExLlamaV3's reservation differ by the drafter's headroom.** Read at TabbyAPI be74bf0 and ExLlamaV3 1.5.4:
- **TabbyAPI** (`common/errors.py: validate_context_requirements`) admits a job when `ceil((prompt − 1 + 2048) / B) · B ≤ cache_size`. B is the allocation boundary: `recurrent_checkpoint_interval`, 2,048 here. TabbyAPI chunks output at 2,048 tokens per round (`job_max_rq_tokens`).
- **ExLlamaV3** (`generator/job.py: prepare_for_queue`) aligns the round the same way, then reserves `num_draft_tokens` more as `rq_headroom`: 7 for the floor's DFlash2 drafter.
- **The result.** A round whose aligned end lands exactly on 163,840 passes TabbyAPI's check but needs ⌈(163,840 + 7) / 256⌉ = 641 pages.
- **Where it bites.**
  - **At admission.** A request's first round passes TabbyAPI's check and then fails ExLlamaV3's at enqueue, so that one request is refused. This is what T1 run 3 hit: a 161,282-token prompt gives 161,281 + 2,048 → 163,840, i.e. 641 pages.
  - **On a later round.** Later rounds are requeued inside the generator, unchecked, and there the same assertion is the fatal error above. Any round starting above 159,745 tokens (prompt plus output so far) triggers it.
- **The safe rule for serve:** keep **prompt + max_tokens ≤ cache_size − 2·B**, 159,744 on the floor. The same arithmetic holds for the 48 GiB host's line (B 2,048, MTP 3 drafts, cache_size 524,288 → 520,192).
- **Not yet verified on the engine.** A diagnostic that logs the failing job's page counts is staged (`tabby_crashdiag.py`, not run). It needs the maintainer's window.

## Harness lessons (for anyone re-running this)

1. **xhigh shares no prefix with a thinking-off trunk.** The chat template renders `Reasoning effort is set to xhigh…` at the top of the system prompt for xhigh (and low), and nothing for medium or thinking off. A fork at the default xhigh effort therefore shares no prefix with a thinking-off trunk. Checked: a medium fork reused 3,584 of 3,742 tokens, an xhigh fork 0.
2. **Trunk turns back to back, 8 s apart, put no pressure on the pool:** the fork barely grows between them. Pilots 1–3 hit 0.98–0.99 throughout with no live evictions.
3. **TabbyAPI reserves the prompt plus a 2,048-token round, not the whole completion,** so a fork's `max_tokens` costs nothing until it is generated.
4. **`min_tokens` did not hold medium forks to their cap.** Most ended at 2–5k tokens, though one ran to its 17,189-token cap.
5. **The pilots before arm 1** (`pilots/`) are kept with the data:
   - an ssh-pipe hang in my launcher, with the floor idle on r2;
   - the xhigh/thinking-off mismatch;
   - no step gap;
   - short forks;
   - a 116k trunk that never pressured the pool;
   - pilot 4's crash.

## Not measured

- The canary on r3.
- Trunk turns with long outputs (the follow-up above).
- r3 under the vision cell.
- RAM pressure from vision offload combined with r3's 12 GiB of host caches over a long run.
