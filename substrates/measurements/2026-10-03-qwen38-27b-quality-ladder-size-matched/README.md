# Addendum to the Qwen3.8 27B quality ladder (#334): size-matched EXL3 rungs on `linux-pc`

**Characterization, not admission.** This was measured 2026-10-03 on `linux-pc` (RTX 3090 24 GB) by the inference seat, using the harness, Q8_0 reference logits and scorer of the main #334 measurement (`substrates/measurements/2026-10-03-qwen38-27b-quality-ladder/`, PR #344). Registry ids only.

**Why.** The main ladder's EXL3 rows (4.00 and 5.00 bpw) are 39–64% larger than GSQ-RCO IQ3_S, so they could not separate the quantizer from the bit budget.
- The maintainer asked for EXL3 rungs matched either by size or by the model card's stated KLD.
- They chose `2.50bpw`, `3.00bpw` and `SC_3.00bpw_H4` from `turboderp/Qwen3.8-27B-exl3`.
- They dropped `SC_3.00bpw_H4_V4`, which differs from `SC_3.00bpw_H4` only by a quantized vision tower.

**Approval and downtime.**
- The maintainer granted the window and the ~40 GB download in the inference seat's session, 2026-10-03 at about 22:30Z. There is no GitHub comment to cite.
- The window was announced in #334 comment 5974173716.
- The floor (`accel24-beellama-qwen27b-q4kxl`) was down 22:35:43Z–22:42:36Z (6 m 53 s).
- After the restore, all checks passed (`checks/`, `mac.log`):
  - `window.log`'s restore line shows exe prefix `980845d60ae7a820`;
  - fingerprint `--check --hash-models`: SUBSTRATE IDENTICAL;
  - `verify-box`: PASS, before and after;
  - the canary: 36/36, PASS.

## Result, with the main ladder's rows for comparison

KLD against the same Q8_0 reference, over 24,564 scored positions per corpus. Size is the model's safetensors shards in bytes / 10⁹ (`rcpt/weights.txt`; #334's `rcpt/weights.txt` for the comparison rows).

| weights | size | engine | code KLD | code top-1 | prose KLD | prose top-1 | source |
|---|---|---|---|---|---|---|---|
| GSQ-RCO IQ3_S-mtp | 12.12 GB | mainline `4ceb171` | 0.0542 | 95.43% | 0.0860 | 89.76% | #334 |
| **EXL3 `2.50bpw`** | 12.30 GB | ExLlamaV3 1.5.1 | **0.0973** | 93.87% | **0.1057** | 87.93% | `logs/exl3-2.50bpw.log` |
| **EXL3 `SC_3.00bpw_H4`** | 13.45 GB | ExLlamaV3 1.5.1 | **0.0591** | 95.21% | **0.0738** | 90.19% | `logs/exl3-SC_3.00bpw_H4.log` |
| **EXL3 `3.00bpw`** | 13.82 GB | ExLlamaV3 1.5.1 | **0.0522** | 95.77% | **0.0731** | 91.05% | `logs/exl3-3.00bpw.log` |
| EXL3 `4.00bpw` | 16.86 GB | ExLlamaV3 1.5.1 | 0.0172 | 97.53% | 0.0223 | 95.43% | #334 |

**Standard errors of the mean**, from the RESULT lines:
- New rows: 0.0016–0.0025 (code) and 0.0034–0.0037 (prose).
- IQ3_S, from #334: 0.0015 (code) and 0.0037 (prose).
- These are each run's own standard errors, not the paired difference's.

### What it shows

1. **At matched size, GSQ-RCO IQ3_S beats EXL3.**
   - EXL3 `2.50bpw` is 1.5% larger than IQ3_S, yet it has 1.8× IQ3_S's code KLD and 1.2× its prose KLD, with lower top-1 on both.
   - The main ladder's EXL3 advantage therefore came from the larger bit budget, not from the quantizer.
2. **The 3.00 tier is roughly level with IQ3_S, for 11–14% more bytes.**
   - `3.00bpw` matches IQ3_S on code (0.052 against 0.054) and is better on prose (0.073 against 0.086).
   - `SC_3.00bpw_H4` is a little worse than IQ3_S on code (0.059) and better on prose (0.074).
3. **Self-calibration doesn't carry over to independent text here.**
   - At the same 3.00 bpw, the self-calibrated `SC_3.00bpw_H4` is no better than plain `3.00bpw` on prose (0.0738 against 0.0731) and worse on code (0.0591 against 0.0522). It is 3% smaller.
   - The model card ranks it the other way, at 0.0257 against 0.0332. The card measures on a self-generated trace of the same kind as the SC calibration data.
   - Our corpora don't overlap that calibration data (`rcpt/calcheck-sc.txt`).
4. **The card's scale and ours differ.**
   - On our corpora, against Q8_0, the rows are 1.6–2.9× the card's figures against FP: 0.052–0.073 against 0.0332 for `3.00bpw`, and 0.017–0.022 against 0.0082 for `4.00bpw`.
   - Card figures should therefore not be compared directly with these.

## How it was measured

**Reference logits.** The main #334 files, reused as they were. Before the floor stopped, the window checked their sha256 prefixes against #334's `window.log` (`471fe3d21ee07cc5` code, `1cc5037cb35395a9` prose), and it aborts on a mismatch (`scripts/w334b.sh`).

**Scorer.** `scripts/exl3kld.py` v2 (sha256 `02617353…`, the same file as #334) with `--last-only`. #334's equivalence check showed it identical to full logits at every printed digit. Command per rung:

`exl3kld.py -m <rung> --tag exl3-<rung> --last-only --base base-code.kld --base base-prose.kld`

**Engine** (`identity.txt`): ExLlamaV3 1.5.1 with torch 2.13.0+cu132. The scorer runs without a KV cache.

**Weights:** `rcpt/weights.txt` gives the publisher sha256, HF revision and size for every file. Each rung's `quantization_config.json` sha256 prefix is in `identity.txt`. The SC branch also ships its 4 MB `cal_trace.safetensors`, which is not counted in its size.

**Calibration overlap** (`rcpt/calcheck_sc.py`, `rcpt/calcheck-sc.txt`):
- The self-calibration trace (`cal_trace.json` on the repo's main branch, sha256 `7a3436f0…`, 217 rows of token ids) was decoded with the model's tokenizer, giving 2,122,166 characters.
- 0 of 1,500 code windows and 0 of 1,289 prose windows (200 characters, one every 1,000) occur in it.

**Files.**
- `window.log`, `logs/`, `identity.txt`.
- `scripts/w334b.sh`: the window. `scripts/mac.sh`: the checks before and after.
- `checks/`, `rcpt/`.
- In the scripts, `<port>`, `<user>` and `$HOME` replace the serving port, the login and the home directory; nothing else was changed. `SHA256SUMS` covers every file.
