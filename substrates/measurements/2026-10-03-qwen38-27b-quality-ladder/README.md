# Qwen3.8 27B quality ladder on `linux-pc`: KLD against Q8_0 across the candidate's engine, the floor's engine and EXL3 (#334)

**Characterization, not admission.** Measured 2026-10-03 on `linux-pc` (RTX 3090 24 GB) by the inference seat. Registry ids only.

**Approval and downtime.**
- The maintainer granted the reference download and the floor window directly to the inference seat at 2026-10-03T20:13Z, in the seat's own session. There is no GitHub comment to cite.
- The window was announced before it started, in #334 comment 5973088811.
- The floor (`accel24-beellama-qwen27b-q4kxl`) was down 20:32:14Z–20:55:06Z (22 m 52 s), restored by `provision-diet.sh`.
- After the restore, all checks passed (`checks/`, `mac.log`):
  - the floor's exe (`980845d6…`);
  - `fingerprint.py --check --hash-models` against the current baseline (the 2026-10-02 post-restore capture): SUBSTRATE IDENTICAL;
  - `verify-box`: PASS, before and after;
  - the canary: 36/36, PASS.

## Result

**Method.** 12 × 4,096-token chunks per corpus, scoring the second half of each chunk: 24,564 positions per corpus. "Top-1" is llama.cpp's "Same top p": the share of positions where the quant's most likely token matches the reference's. Size is the file on disk.

| weights | size | engine | code KLD | code top-1 | prose KLD | prose top-1 |
|---|---|---|---|---|---|---|
| GSQ-RCO IQ2_XS-mtp | 8.77 GB | mainline `4ceb171` | 0.1533 | 91.83% | 0.2421 | 81.27% |
| | | BeeLlama `preview-v0.3.2` | 0.1533 | 91.88% | 0.2418 | 81.35% |
| GSQ-RCO IQ2_S-mtp | 9.61 GB | mainline | 0.1175 | 93.01% | 0.1772 | 84.19% |
| | | BeeLlama | 0.1175 | 92.94% | 0.1761 | 84.25% |
| GSQ-RCO IQ3_XXS-mtp | 10.44 GB | mainline | 0.0990 | 93.92% | 0.1252 | 86.77% |
| | | BeeLlama | 0.0988 | 93.83% | 0.1264 | 86.72% |
| **GSQ-RCO IQ3_S-mtp** (the candidate rung's file) | 12.12 GB | mainline | **0.0542** | 95.43% | **0.0860** | 89.76% |
| | | BeeLlama | 0.0543 | 95.46% | 0.0847 | 89.73% |
| **EXL3 4.00 bpw** | 16.86 GB | ExLlamaV3 1.5.1 | **0.0172** | 97.53% | **0.0223** | 95.43% |
| EXL3 5.00 bpw | 19.90 GB | ExLlamaV3 1.5.1 | 0.0056 | 98.53% | 0.0090 | 97.53% |
| EXL3 6.00 bpw | 22.94 GB | ExLlamaV3 1.5.1 | not measured: the scorer's load failed with "Insufficient VRAM in split for model and cache" (`logs/exl3-6.00.log`) | | | |

**Reference** (Q8_0, PPL over the scored positions as each KLD log reports it): code 1.6487, prose 6.3317. llama-perplexity's own final estimates in `logs/ref-*.log` are 1.6489 and 6.3432.

### What it shows

1. **The two llama.cpp-family engines are equivalent on these weights.**
   - Mainline and the floor's BeeLlama release agree within 0.0002 on code KLD for every quant, and within 0.0013 (≤ 1.5%) on prose.
   - The differences sit inside one standard error, about 0.003–0.005 on prose.
   - So the engine choice between these two carries no measurable quality cost on this model.
2. **Precision costs more on prose than on code.** Each GSQ-RCO quant gives up 1.3–1.6× more on prose than on code; EXL3 gives up 1.3–1.6×. This matches the Flash-Next ladder (#336), where prose is also where quantization costs most.
3. **The quants are not size-matched.**
   - EXL3 4.00 bpw has about a third of IQ3_S's code KLD and a quarter of its prose KLD, but it is 39% larger (16.9 GB against 12.1 GB).
   - This table therefore does not separate the quantizer from the bit budget.
   - The EXL3 repository also publishes 3.00 and 3.50 bpw branches, near IQ3_S's size. They are the size-matched comparison and were not run here.
4. **Speed for the same files** (#336 §4, short prompts, single stream):
   - IQ3_S with MTP n=2: 63–70 tok/s in 13.5 GB.
   - EXL3 4.00 with the DFlash2 drafter: 74–93 tok/s in 23.1 GB of 24.
   - EXL3 4.00 without a drafter: 43 tok/s.

## How it was measured

**Corpora.** The same as the Flash-Next ladder (#336 §1):
- `code.txt`: sha256 `a23ee6968f5cb387…`
- `prose.txt`: `173c87a53759e020…`

The 27B's tokenizer is byte-identical to Flash-Next's (`pre=qwen35`, n_vocab 248,320), so the scored token streams are the same.

**Reference logits.**
- Weights: `unsloth/Qwen3.8-27B-GGUF` `Qwen3.8-27B-Q8_0.gguf`, revision `4ca72078`. sha256 `a680f44a06920e5d689774823782006aa3acc8db95750323373b24139b67e348`, re-hashed on the host before the floor stopped (`identity.txt`).
- Run with mainline `llama-perplexity` with 52 of the model's layers on the GPU:

  `-m Q8_0 -ngl 52 -t 8 -fa on -c 4096 -b 4096 -ub 512 --chunks 12 -f <corpus> --kl-divergence-base base-<corpus>.kld`
- The base files are 12,199,858,100 bytes each, with sha256 prefixes `471fe3d21ee07cc5` (code) and `1cc5037cb35395a9` (prose). They are too large to commit, and the inference seat keeps them.

**GGUF rows.** The same flags with `-ngl 99 --kl-divergence-base base-<corpus>.kld --kl-divergence`:
- Mainline: run in the `llmbuild` build container that built it.
- BeeLlama: its release `llama-perplexity`, run read-only from the release directory beside the floor's `llama-server`.

**EXL3 rows.** `scripts/exl3kld.py` (v2, sha256 `02617353…`). It applies llama.cpp's `kl_divergence()` arithmetic to the same base files, with no KV cache.
- v2 adds `--last-only`, which computes logits only for the scored half of each chunk (ExLlamaV3's `last_tokens_only`), to fit 24 GB.
- **Equivalence check.** On the first two code chunks of the 4.00 bpw model, v2 with and without `--last-only` gives identical results to every printed digit: KLD 0.008502, PPL(Q) 2.097876, top-1 97.777% (`logs/exl3-eq-full.log`, `logs/exl3-eq-last.log`).
- v2 without `--last-only` runs the same arithmetic as v1, which reproduced llama.cpp's PPL(base) exactly on Flash-Next (#336).

**Engines** (`identity.txt`):

| engine | identity |
|---|---|
| mainline | `4ceb1719101f32637b841206c172f3f058ffc182`, clean tree; `llama-perplexity` `06e6e140…` |
| BeeLlama | `llama-perplexity` `385640dd…`, in the release directory whose `llama-server` is the floor's registered exe `980845d6…` |
| ExLlamaV3 | 1.5.1 with torch 2.13.0+cu132 |

**Weights** (publisher digests, from the host's Hugging Face download metadata, listed in #336's `weights-digests.txt`):

| file | sha256 |
|---|---|
| GSQ-RCO IQ2_XS-mtp | `f3369f8d…` |
| GSQ-RCO IQ2_S-mtp | `e6406238…` |
| GSQ-RCO IQ3_XXS-mtp | `63f29a21…` |
| GSQ-RCO IQ3_S-mtp | `58fd8267…`, equal to the candidate rung's registered `weights_main` |
| EXL3 4.00 bpw | revision `113cf7ab` |
| EXL3 5.00 bpw | revision `a35e75a7` |
| EXL3 6.00 bpw | revision `d32ba0bb` |

**Files.**
- `window.log`: every row's summary, with timestamps.
- `logs/`: the full output of every run.
- `scripts/w334.sh`: the window. `scripts/mac.sh`: the checks before and after.
- `checks/`: the verification outputs.
- In the scripts, `<port>`, `<user>` and `$HOME` replace the serving port, the login and the home directory; nothing else was changed. `SHA256SUMS` covers every file.
