# Qwen3.8 27B on `linux-pc`: DFlash2 against MTP, EXL3 against GSQ-RCO, and the 3.6 floor at depth (#335)

**Characterization, not admission, and not a floor decision.** Measured 2026-10-04 on `linux-pc` (RTX 3090, 24,576 MiB) by the inference seat, in six windows the maintainer approved in the inference seat's session. Dispatch cleared each one on #335.

Every arm run is recorded here, including the ones that failed or ran off the intended profile. Registry ids only.

**Headroom.** The bar cited below is the one ruled for v0.1.0 at #143 5984376546: 300 MiB free at peak fill, or the line measured not to OOM at its registered pool under the bench. It replaces the 1,208 MiB rule (the 3.6 floor's own free VRAM), which the first window still applied.

## Windows

The floor is `accel24-beellama-qwen27b-q4kxl`. Every window restored it with `provision-diet.sh`.

| window | floor down | content | checks after the restore |
|---|---|---|---|
| `w1/` | 19:23:05–19:53:57Z (30 min 52 s); the floor's own reading ran 19:10:37–19:23:03Z with production up | the floor at depth; EXL3 3.50 bpw KLD; arm A probes (skipped, see below); arm C; arm C′ | exe `980845d60ae7a820`; fingerprint `--hash-models` IDENTICAL `4b194a74ff6ab263`; verify-box PASS; canary 36/36 |
| `wc/` | 20:19:27–20:31:53Z (12 min 26 s) | EXL3 `SC_3.00bpw_H4` + DFlash2, cache `5,4`; residency baseline | IDENTICAL `4b194a74ff6ab263`; PASS; 35/36 |
| `wd/` | 20:37:15–20:46:26Z (9 min 11 s) | EXL3 `SC_3.00bpw_H4` + DFlash2, cache `8,8` | IDENTICAL; PASS; 36/36 |
| `we/` | 21:08:39–21:22:04Z (13 min 25 s) | EXL3 `SC_2.20bpw_H3` KLD; + DFlash2, cache `8,8` | IDENTICAL; PASS; 36/36 |
| `wf/` | 21:51:35–22:00:53Z (9 min 18 s) | EXL3 plain `3.00bpw` + DFlash2, cache `8,8` (in-process); TabbyAPI + vision did not load at 196,608 | IDENTICAL; PASS; 36/36 |
| `wg/` | 22:07:30–22:08:51Z (1 min 21 s) | TabbyAPI + plain `3.00bpw` + DFlash2 + BF16 vision tower (offloaded) at 172,032; #373's image test; CUDA OOM on the next text request | IDENTICAL; PASS; 36/36 |

## Bench

**Prompts.** The #143 profile's bench (`scripts/deepb.py`): a contiguous slice of a C/C++ corpus, followed by a code-review question. The corpus is a llama.cpp checkout used only as text, at commit `1deefcca` (`identity.txt`).

**Settings.** Greedy decoding, 256 new tokens. Rep 0 prefills cold; reps 1–2 run warm on the same prompt.

**What the table reports:**
- **Single stream:** the mean of reps 1–2's decode rate.
- **Two streams:** total tokens divided by wall time, rep 2.
- **Rate sources:** llama.cpp arms report the server's `timings.predicted_per_second`. ExLlamaV3 arms report (new tokens − 1) divided by the generator's decode time (`scripts/exl3deep.py`, the same corpus, offsets and question, rendered through the model's own chat template).

## Results: decode tok/s

| line | engine | KV | pool | 2k | ~100k | deep | 2 streams, total | free at peak |
|---|---|---|---|---|---|---|---|---|
| 3.6 floor, original DFlash | beellama `980845d6…` | q5_0 / q4_1 | 160,000 | 74.4 | 40.9 | 37.4 (157k) | 30.1 (2×~77k) | not measured here (1,208 MiB, #143) |
| 3.8 GSQ-RCO IQ3_S + MTP n=2 | mainline `4ceb171` | q8_0 / q8_0 | 229,376 | 68.1 | 43.1 | 35.5 (227k) | 29.9 (2×~113k) | 1,966 (#143 5883986293; not re-run) |
| **arm C:** IQ3_S + DFlash2 Q4_K_M (Z-Lab GGUF) | mainline `4ceb171` | q8_0 / q8_0 | 229,376 | 74.6 | 44.7 | 38.5 (226k) | 32.6 (2×~112k) | 1,412 |
| arm C′: IQ3_S + DFlash2 Q3_K_M (Anbeeld) | same | same | 229,376 | — | 42.5 | — | — | — |
| arm C′: IQ3_S + DFlash2 Q2_K (Anbeeld) | same | same | 229,376 | — | 41.3 | — | — | — |
| EXL3 `SC_3.00bpw_H4` + DFlash2 | ExLlamaV3 1.5.4 | `5,4` | 229,376 | 100.5 | 52.5 | 45.1 (226k) | 55.2 (2×~112k) | 3,406 |
| EXL3 `SC_3.00bpw_H4` + DFlash2 | ExLlamaV3 1.5.4 | `8,8` | 196,608 | 111.1 | 57.9 | 60.6 (194k)\* | 59.0 (2×~96k) | 874 |
| EXL3 `SC_2.20bpw_H3` + DFlash2 | ExLlamaV3 1.5.4 | `8,8` | 229,376 | 142.7 | 60.6 | 35.5 (226k) | 51.7 (2×~112k) | 1,778 |
| EXL3 plain `3.00bpw` + DFlash2 | ExLlamaV3 1.5.4 | `8,8` | 196,608 | 120.6 | 64.1 | 43.4 (194k) | 60.0 (2×~96k) | 488 |

\* **The deep rows' prompts end at different places in the corpus.** Draft acceptance there differs from the shallower rows: `SC_3.00` at `8,8` accepts 0.53 at 194k, against 0.33–0.40 elsewhere. Deep rows are comparable only where the depth matches.

**Acceptance is not comparable across engines.** llama.cpp reports `draft_n_accepted / draft_n`, and ExLlamaV3 reports accepted / (accepted + rejected); the draft lengths differ.
- llama.cpp + DFlash2: 0.57–0.77.
- ExLlamaV3 + DFlash2, which drafts 7 tokens by default: 0.28–0.53.
- The floor's original DFlash: 0.15–0.21, with 15-token drafts.

**Pools, free VRAM and KV cost:**
- **Arm C**'s free figure comes from a two-slot fill to 114,122 tokens each (`w1/logs/fill-C-c229376.json`).
- **EXL3 rows'** free figure is 24,576 minus the peak `nvidia-smi` reading during the bench.
- **KV cost per token, against `wc/`'s residency baseline:** `SC_3.00bpw_H4` + DFlash2 drafter + workspaces at an 8,192-token `5,4` cache is 14,684 MiB. That gives about 26 KiB/token at `5,4` and about 44 KiB/token at `8,8`. The llama.cpp line costs 34 KiB/token at q8_0, from #143's 7,616 MiB at 229,376.
- **Under the 300 MiB bar,** every row above passes at the pool it ran.

## Results: KLD against #334's Q8_0 reference

Same harness, base files (`471fe3d2…`, `1cc5037c…`, verified before each stop), scorer and corpora as #334. ExLlamaV3 1.5.1.

| quant | size | code KLD | code top-1 | prose KLD | prose top-1 | log |
|---|---|---|---|---|---|---|
| GSQ-RCO IQ3_S (#334) | 12.12 GB | 0.0542 | 95.43% | 0.0860 | 89.76% | #334 |
| EXL3 `SC_2.20bpw_H3` | 10.80 GB | 0.1226 | 92.85% | 0.1697 | 84.55% | `we/logs/kld-SC_2.20bpw_H3.log` |
| EXL3 `SC_3.00bpw_H4` (#334 addendum) | 13.45 GB | 0.0591 | 95.21% | 0.0738 | 90.19% | #334 addendum |
| EXL3 `3.50bpw` | 15.34 GB | 0.0382 | 96.48% | 0.0421 | 93.53% | `w1/logs/kld-3.50bpw.log` |

## TabbyAPI with the vision tower (`wf/`, `wg/`)

**The line:** TabbyAPI `be74bf0a` on ExLlamaV3 1.5.4, serving plain `3.00bpw` + the DFlash2 EXL3 drafter, cache `8,8`, `max_batch_size` 2, `vision: true`, `vision_offload: true`. The configs are `wf/logs/tabby-config.yml` and `wg/logs/tabby-config.yml`.
- Both EXL3 3.00 quants carry the full BF16 vision tower (333 tensors, 921 MB); only the `_V*` branches quantize it.
- `vision_offload` keeps its weights in system RAM and runs the encode on the GPU. ExLlamaV3 has no CPU compute path for vision.

**Loading:**
- It did not load at 196,608 (`wf/logs/tabby.log`), 188,416 or 180,224 (`wg/logs/tabby-c*.log`), each time with "Insufficient VRAM in split". It loaded at 172,032, using 23,162 MiB.
- The in-process bench loaded the same weights at 196,608 with 1,182 MiB free. Likely cause, not verified: TabbyAPI's default `draft_cache_mode: FP16` gives the drafter its own full-size FP16 cache.

**#373's image test** (`wg/logs/vision/`: the committed image, `run_cell.py`'s request and responses, and `derive.py`'s output). This is a measurement, not the registry's vision word.
- Three 200s, with 287 prompt tokens each.
- Replies: "NPC?", "NPC7" and "NPC?", which `derive.py` reads as `answered-without-seeing`, `accepted`, `answered-without-seeing`: **1 of 3**.
- The model reads the image but misreads the last glyph in two of three. Likely cause [I]: the image reaches it at about 250 visual tokens. The checkpoint's `preprocessor_config.json` floors images at 65,536 pixels (`size.shortest_edge`), while the 3.6 floor upscales to at least 1,024 visual tokens (`--image-min-tokens 1024`).

**VRAM:** 23,162 MiB after load and 24,106 MiB peak over the image requests (`wg/logs/vram-trace.txt`, 0.2 s samples): 470 MiB free.

**The next request, text only (about 1.7k tokens), failed with CUDA out of memory,** and TabbyAPI unloaded (`wg/logs/tabby.log`, `wg/logs/tabby-text.json`). **This TabbyAPI configuration is not servable as measured.**

## What did not run, and why

- **`w1`'s EXL3 arms A (DFlash2) and B (MTP).**
  - The window gated each ExLlamaV3 load on free VRAM ≥ 1,608 MiB (the old 1,208 bar plus 400). ExLlamaV3's free reading after load includes PyTorch's warm-up allocations and is not monotone in pool size: `3.00bpw` at 196,608 read 1,508 MiB free, but at 163,840 read 642.
  - `3.50bpw` did not load at 229,376, 196,608 or 163,840 with the drafter on 1.5.1 ("Insufficient VRAM in split"). `3.00bpw` loaded at 196,608 and 163,840 but was refused by the gate.
  - All six probe logs are in `w1/logs/A-*.log`.
  - The follow-up windows dropped the gate and ran `SC_3.00bpw_H4`, the self-calibrated quant that matches IQ3_S by both size and KLD (the #334 addendum). The MTP arm was dropped at the maintainer's direction.
- **The `5,4` window's `8,8` run** did not load at 229,376 (`wc/logs/S3-dflash2-cq88.log`); `wd/` re-ran it at the largest pool that loads.
- **`SC_2.20bpw_H3` at 262,144** did not load at `8,8` (`we/logs/P-c262144.log`).

## Findings recorded alongside

**The floor's shared pool refuses rather than evicts (#406).**
- The floor's first cold request at 157k and its first two-stream request both returned HTTP 500. The floor's log reads "Context size has been exceeded" (`w1/logs/deep-floor.log`).
- Cause: under `--kv-unified` the 160,000 pool still held the other slot's previous prompt, because the bench's `/slots?action=erase` did not free it on this engine. The identical retry succeeded.

**Vision was not loaded in any arm.**
- `exl3deep.py` loads only the text component, and no llama.cpp arm passed `--mmproj`. The floor's reading ran against production, which loads its projector on the CPU.
- ExLlamaV3 can hold a vision tower's weights in pinned host memory (`EXL3_VISION_PINNED=1`), with compute still on the GPU. Not tested.

**ExLlamaV3 version.**
- The speed arms in `wc/`, `wd/` and `we/` ran on 1.5.4, which carries the DFlash2 batch > 1 fix (`walk_block` striding). The release wheel is `exllamav3-1.5.4+cu132.torch2.13.0`, sha256 `e95ed898…`, installed into a copy of the 1.5.1 venv with only that package replaced.
- KLD rows ran on 1.5.1, as every ladder row has.

## Identities

**Engines:**
- mainline `4ceb1719…`, `llama-server` `865044a2…`;
- beellama exe `980845d6…`;
- ExLlamaV3 1.5.1 / 1.5.4, both with torch 2.13.0+cu132 (`identity.txt` in each window).

**Drafters** (`w1/identity.txt` has the full digests):

| drafter | size | sha256 prefix |
|---|---|---|
| Z-Lab `Qwen3.8-27B-DFlash2-Q4_K_M.gguf` (`z-lab/Qwen3.8-27B-DFlash2-GGUF`) | 1,143,006,816 bytes | `1a25c568…` |
| Anbeeld Q3_K_M | 916,702,560 | `72809a53…` |
| Anbeeld Q2_K | 705,430,880 | `e3eb7705…` |
| `igor255/Qwen3.8-27B-DFlash2-EXL3-4.00bpw`, revision `cbf4643e` (made with stock ExLlamaV3 1.5.1, default calibration) | 1,156,474,612 | `f2a39784…` |

**Weights:**
- GSQ-RCO IQ3_S-mtp `58fd8267…`;
- `turboderp/Qwen3.8-27B-exl3` branches: `3.50bpw` revision `8351c54e`, `SC_2.20bpw_H3` revision `d6e046da`; `SC_3.00bpw_H4` as in the #334 addendum.

## Files

| path | contents |
|---|---|
| `w1/`, `wc/`, `wd/`, `we/`, `wf/`, `wg/` | `window.log`, `identity.txt`, `logs/` (every bench, server, fill, KLD and probe log), `checks/` (verify-box, fingerprint, canary), `scripts/` (the window script and the Mac-side checks) |
| `w1/scripts/` | also `exl3deep.py`, `deepb.py`, `probe143.py` and `exl3kld.py` (v2, `02617353…`) |

**Redaction.** `$HOME`, `<port>`, `<user>`, `<boxhost>` and `<scratch>` replace the home directory, the serving port, the login, the host address and a local staging path. In the six Mac-side scripts (`mac.sh`, `macc.sh`, `macd.sh`, `mace.sh`, `macf.sh`, `macg.sh`) a private run-directory name and a restore-capture path component are `<run-dir>` and `<restore-capture>`. Nothing else was changed, so the scripts do not run as committed. `SHA256SUMS` covers every file.
