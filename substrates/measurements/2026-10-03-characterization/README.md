# Engine and quant characterization: Flash-Next quant ladder, TabbyAPI + EXL3 against llama.cpp, MTP draft length, 27B short-prompt speed (#336)

**Characterization, not admission.** Nothing here admits a rung or changes a registry entry. It records what the inference seat measured, so that later work starts from data instead of from a conversation. Registry ids only: `rtx6000ada-host` (RTX 6000 Ada 48 GB) and `linux-pc` (RTX 3090 24 GB).

**Revision 3 (2026-10-03).** This corrects the first courier (SHA256SUMS `6f06808a…`) and revision 2 (`ba3f230f…`) after the two reviews on #340. Every number below is now traced to a committed file.

**What changed from revision 2:**
- §2: llama.cpp's "100k" prefill is described per run. Run 1 prefilled the whole prompt; run 2 reused a ~9.5k prefix.
- §3: the repeatability figure is corrected to 2.5 tok/s.
- §3: the llama.cpp acceptance comparison is withdrawn.
- The KLD tool's receipt now names its tree, and the separate-build claim now compares `llama-server` with `llama-server`.
- New receipt `rcpt/quantizer-versions.txt`.
- `rcpt/trunk-sha256.txt` lists all three paths of the rebuilt BF16 shard.
- "Ada host" is replaced by `rtx6000ada-host` in `versions.txt` and `config-redacted.yml`.

**What changed from revision 1 to revision 2:**
- **Prefill figures (§2):** corrected, and explained by prefix reuse.
- **§3's method:** in-process ExLlamaV3, not TabbyAPI.
- **The KLD tool's identity:** now has a receipt (`rcpt/kld-tool-identity.txt`).
- **Claims withdrawn:**
  - "`cache_mode 8,8` held quality": no committed measurement exercises the KV cache.
  - "first request ~14% slow": the log shows 12.6%.
- **Units:** VRAM is now stated in the units each log prints.
- **Receipts added:**
  - the calibration-overlap scan;
  - a TabbyAPI probe (effort refusal, `/slots`, leading blank line, `timings`);
  - per-host provenance (dates, ExLlamaV3 version);
  - the `exl3win.sh` launcher;
  - host hashes of the Flash-Next GGUF trunks, including the BF16-PLE variants;
  - the freeze and weight-hash files that `versions.txt` names.
- **`versions.txt`:** it now says three redacted lines, matching `config-redaction.diff`, and its 1.5.2 build string matches the README.

**How the paths read.** Paths in §1–§3 and §5 are relative to `rtx6000ada-host/`, except those under `rcpt/`, which is at the top level. Paths in §4 name their host directory. File modification times in the receipts are in host local time (UTC−07:00). Dates below are in UTC.

**Redaction, applied before hashing.**
- Home directories became `$HOME`.
- The API key became `<api-key>`.
- The serving ports became `<port>`, `<side-port>` and `<side-port-2>`.
- Nothing else in a log or script was changed.

Because of the redaction, **the scripts do not run as committed**: restore the port, the key and the paths first. `rtx6000ada-host/config-redaction.diff` describes the redaction of the TabbyAPI config. The prose prompt in `exl3bench.py` names a workstation product, `mac-pro-2019`'s model, as essay subject matter; it is not a host reference.

All files are byte-identical to the digests in `SHA256SUMS`.

## 1. Flash-Next quant ladder: KLD against Q8_0 (`rtx6000ada-host`, 2026-10-01 and 2026-10-03 UTC)

**Tool.** `llama-perplexity` from the development tree `$HOME/src/llama-q8sparse`, the path that `kld/kldwin.sh`, `kld/ref.sh` and `kld/udq4.sh` call. Its HEAD is `e7051ef` (`rcpt/kld-tool-identity.txt`):
- The reflog shows HEAD at `e7051ef` since 2026-09-25 17:18 (host time), and the tree is clean.
- Every binary and library is dated 2026-09-25 17:09–17:16, before every KLD log. The exe is `633b467b…`.
- The binaries were built from the working tree that was committed, a minute and a half later, as `e7051ef`.
- It is a separate build from the serving line's. The tree's own `llama-server` launcher hashes to `7990b3ad…`; the serving `b7-e7051ef` `llama-server`'s is `41e6591d…`. Both are thin launchers, so the difference shows a different build directory, not different code.
- Lines in `kld/window.log` that name another build refer to the production restore, not to the scoring tool.

**Reference.** Qwen3.8-Flash-Next Q8_0 (unsloth, revision `38bb39ee`; digests in `weights-digests.txt`). It was run CPU-only (`kld/ref.sh`, `kld/ref.log`) over two corpora:
- `code.txt`: C++ source, sha256 `a23ee696…`
- `prose.txt`: wikitext-2 `wiki.test.raw`, sha256 `173c87a5…`

Each corpus is 12 × 4,096-token chunks. The second half of each chunk is scored: 24,564 positions per corpus.

The reference logits are `base-code.kld` (`7153dc35…`) and `base-prose.kld` (`663eb63d…`). They are too large to commit, and the inference seat keeps them.

**How each row was scored:**
- **GGUF rows:** `llama-perplexity --kl-divergence` with the same tool.
  - Q2_0 and Coder: `kld/kldwin.sh`, all on the GPU.
  - UD-Q4_K_XL: `kld/udq4.sh`, CPU-only.
- **EXL3 rows:** `exl3kld.py`, which runs the EXL3 model with **no KV cache** (`cache=False`) and applies llama.cpp's `kl_divergence()` arithmetic to the same base files. Its PPL(base), read from the base files alone, equals llama.cpp's PPL(base) in the KLD logs: 1.346347 code, 2.950894 prose.

| build | size on disk | code KLD | code top-1 | code PPL | prose KLD | prose top-1 | prose PPL | log |
|---|---|---|---|---|---|---|---|---|
| Q8_0 reference, PPL(base) over the scored positions | — | — | — | 1.346 | — | — | 2.951 | any KLD log's `PPL(base)` |
| GSQ-RCO Q2_0 | 66 GB | 0.1593 | 93.58% | 1.416 | 0.3866 | 79.89% | 3.540 | `kld/kld-q2_iq4nl-*.log` |
| GSQ-RCO Coder (IQ1_M; 256 experts, pruned) | 58 GB | 0.1640 | 93.62% | 1.425 | 0.7126 | 72.16% | 4.652 | `kld/kld-coder_iq4nl-*.log` |
| EXL3 2.05 bpw (`2.05bpw_h4_ng4`), ExLlamaV3 1.5.1 | 59 GB | 0.0865 | 95.62% | 1.368 | 0.2308 | 84.88% | 3.202 | `logs/exl3kld-205.log` |
| EXL3 2.05 bpw, rerun on ExLlamaV3 1.5.2 | 59 GB | 0.0860 | 95.61% | 1.367 | 0.2297 | 84.86% | 3.200 | `logs/exl3kld-205-v152.log` |
| EXL3 3.05 bpw (`3.05bpw_h5_ng5`), ExLlamaV3 1.5.1 | 80 GB | 0.0333 | 97.22% | 1.349 | 0.0895 | 90.52% | 3.014 | `logs/exl3kld-305.log` |
| Unsloth UD-Q4_K_XL | 111 GB | 0.0168 | 98.12% | 1.351 | 0.0437 | 93.33% | 2.973 | `kld/kld-udq4-*.log` |

`kld-results.txt` collects the EXL3 RESULT lines.

**Notes:**
- **Two reference PPLs.** `kld/ref.log` prints llama-perplexity's own final estimates from the unquantized reference pass, 1.3470 and 2.9522. The table's 1.346 and 2.951 are PPL(base) recomputed from the base files' 16-bit log-probabilities, as every KLD log reports them.
- **The PLE table.** `*_iq4nl` is each GGUF's PLE table as shipped. The `*_bf16` logs are the same trunks with the table re-stored at BF16. Q2_0 moves by ≤ 0.005 and the Coder by ≤ 0.004, so the table's precision is not where the loss is.
  - The Q2_0 and the Coder share one second shard on the host: the same file, one inode, which holds the shipped table. The publisher's two shard-2 files are byte-identical (both sha256 `316b46f3…`).
  - The two BF16 variants share one rebuilt second shard.
  - Host hashes: `rcpt/trunk-sha256.txt`.
- **Prose costs more than code.** At every full-expert rung, prose KLD is 2.4–2.7× code KLD; the pruned Coder's is 4.3×.
- **Cross-engine KLD** also counts any numerical difference between engines, so the EXL3 rows can only overstate EXL3's loss.
- **Calibration overlap** (`rcpt/calcheck.py`, `rcpt/calcheck-exl3-1.5.1.txt`, `rcpt/calcheck-exl3-1.5.2.txt`):
  - Method: 200-character windows of each corpus, one every 1,000 characters, searched verbatim in ExLlamaV3's bundled calibration files.
  - Result: 0 of 1,500 code windows and 0 of 1,289 prose windows found, against the calibration sets of both installed versions.
  - Not checked: the sets of the versions that made the quants (ExLlamaV3 1.4.4 for both Flash-Next quants and 1.4.2 for the 27B quants, per each quant's `quantization_config.json`; see `rcpt/quantizer-versions.txt`).
  - The bundled set includes a 2.1 M-character `wiki.utf8`, so EXL3 has a domain head start on prose.

## 2. Flash-Next serving: TabbyAPI + EXL3 2.05 bpw against llama.cpp `b7-e7051ef` Q2_0 (`rtx6000ada-host`, 2026-10-03 07:24–07:41 UTC)

`abwin/abwin2.sh`, log `logs/abwin2-1003.log`.
- **Order.** ABAB: llama, Tabby, llama, Tabby. Each arm is started fresh on a side port while production is down.
- **Configs.** Both arms are as served:
  - llama.cpp: 262k context, q8_0 KV, `-np 4`, MTP n=3.
  - Tabby: a 262,144-token pool at `cache_mode 8,8`, `max_batch_size 4`, MTP n=3.
- **Bench.** `abwin/deep2.py`: wikitext-2 at the stated depth, followed by "Continue the passage above in the same register for several paragraphs.", greedy, 256 new tokens, over the OpenAI-compatible API. Concurrent request *i* starts *i* × 60,000 characters into the text.
- **Reps.** Rep 0 includes prefill. Reps 1 and 2 are cache-warm, and **the decode figures below are reps 1–2 only.**

**Concurrency check** (`abwin/contcheck.py`): four ~10k-token passages, each run alone and then all four at once, 120 greedy tokens each.
- **Criterion for "clean":** no concurrent continuation follows another request's passage.
- **Result:** clean for both engines, in all four arms.
- Solo and concurrent outputs are not identical on either engine: they diverge, within their own passage, on 1–3 of 4 requests (the first 150 characters of each are in the log).

| test | llama.cpp e7051ef | TabbyAPI + EXL3 2.05 |
|---|---|---|
| concurrency check | clean | clean |
| 10k, 1 stream, decode tok/s | 96.7–106.4 | 129.3–131.3 |
| 10k, 4 streams, aggregate tok/s | 146.4–152.0 | 153.9–160.1 |
| "100k", 1 stream, decode | 69.8–82.1 | 121.9–132.6 |
| "200k", 1 stream, decode | 57.6–62.3 | 108.9–124.9 |
| prefill tok/s, rep 0 at "100k" | 1,169 (run 1: 101,622 new tokens); 1,143 (run 2: 92,098 new tokens) | 2,928–2,941 (91,680 new tokens) |
| prefill tok/s, rep 0 at "200k" | 902–904 (99,374 new tokens) | 2,871–2,876 (98,890 new tokens) |
| VRAM, `nvidia-smi` after load | 46,572 MiB | 43,762 MiB |

**Prefix reuse explains the "new tokens" counts.** The "200k" prompt extends the "100k" prompt, and the "100k" prompt extends the 10k prompt.
- Both engines reused the cached prefix of the previous phase at "200k": the prompt is ~200k tokens deep, but only its last ~99k were prefilled. `deep2.py` erases llama.cpp's slots before each single-stream phase, yet llama.cpp still reused the ~101k prefix there.
- At "100k", llama.cpp prefilled the whole prompt in run 1 (101,622 tokens), but in run 2 it reused a ~9.5k prefix (92,098 new tokens). TabbyAPI reused its cached 10k prefix in both runs (TabbyAPI has no slot-erase endpoint).
- So the prefill figures are throughput over the new tokens, at the depths those tokens start from. They are not cold whole-prompt prefill, and the two engines' "100k" figures cover different token spans.
- Decode at each depth is unaffected.

Without speculation, EXL3 decode barely depends on depth: 90.5 tok/s at 10k, 88 at 100k.
- Source: `logs/exl3win-1002.log`, run by `rcpt/exl3win.sh`.
- Setup: `-cs 131072`, no `-cq`, so the cache is ExLlamaV3's FP16 default; ExLlamaV3 1.5.1 (`rcpt/rtx6000ada-host-provenance.txt`).

## 3. MTP draft length on in-process ExLlamaV3 1.5.2 (`rtx6000ada-host`, 2026-10-03 16:43–16:58 UTC)

**Method** (`abwin/mtpsweep.sh`, log `logs/mtpsweep-1003.log`):
- The serving TabbyAPI is stopped. Each arm then runs `exl3spd.py` directly in TabbyAPI's venv (ExLlamaV3 1.5.2), one process per arm.
- Each arm uses the serving model and cache configuration: `-cs 262144 -cq 8,8 -ngr`.
- TabbyAPI itself is not in the loop.
- `exl3spd.py` sends the same prompts as `deep2.py` through ExLlamaV3's own generator: the same text and offsets, the chat template rendered locally, greedy, 256 new tokens.
- Reps 1–2 shown. "a" is the MTP draft acceptance rate.

| n | 10k, 1 stream | 10k, 4 streams (aggregate) | 100k, 1 stream |
|---|---|---|---|
| 1 | 117.5–119.1 (a 0.75–0.78) | **193.4–196.0** | 116.2–121.8 |
| 2 | 128.9–132.3 | 159.2–159.3 | 132.8–145.4 |
| **3** (run 1) | **128.1–140.4** (a 0.50–0.58) | 162.3–164.7 | **161.3–163.6** (a 0.76–0.78) |
| **3** (run 2, the last arm, drift control) | 126.9–139.2 (a 0.50–0.58) | 159.8–162.9 | 161.7–163.7 (a 0.76–0.78) |
| 4 | 117.3–121.0 | 147.9–151.2 | 127.7–150.2 |
| 6 | 91.3–102.3 | 132.2–134.0 | 109.2–109.3 |
| 8 | 52.7–66.6 (a 0.23–0.25) | 116.7–122.3 | 75.0–93.5 |
| dynamic, cap 8 | 126.1–130.6 | 149.6–150.3 | 117.8–126.5 |

- **Findings.** n=3 is the best single-stream setting. n=1 is best when four requests run at once.
- **Repeatability.** The two n=3 runs give identical acceptance (greedy) and agree on speed within 2.5 tok/s (the largest gap is the 10k four-stream aggregate, 162.3 against 159.8).
- **VRAM** (the log's `vram_used_gib`): 43.8 GiB at n ≤ 4 and in both n=3 runs, 44.6 at n=6, 45.5 at n=8 and at dynamic.

**Why 100k is faster than 10k at n=3: the text, not the depth.** This is a hypothesis.
- Each depth's prompt ends in a different article, and the 100k continuation is easier to predict: acceptance 0.76–0.78 against 0.50–0.58.
- On this engine a decode step costs about the same at either depth (§2's no-speculation figures).
- The llama.cpp arms of §2 do not settle this. Run 1 accepted more at "100k" than at 10k (0.62–0.65 against 0.50–0.57), but run 2 did not (0.50–0.62 against 0.55–0.62).
- Not confirmed with the final passage held fixed.

## 4. Qwen3.8 27B, short-prompt speed (2026-09-22 UTC)

**Method.** Single stream, greedy, 320 new tokens. Prompt lengths:
- 53–98 tokens on a cold prompt;
- 4–5 where the repeated code prompt hit the prompt cache.

**Nothing was run at depth.** The first EXL3 prompt after load ran 37.6 tok/s against 43.0 for the identical second prompt, 12.6% slower. Dates and engine versions: `rcpt/linux-pc-provenance.txt` and `rcpt/rtx6000ada-host-provenance.txt` (ExLlamaV3 1.5.1, torch 2.13.0+cu132, on both hosts).

**`linux-pc`** (`linux-pc/logs/rco27b.log` from `rco27b-matrix.sh`; `linux-pc/logs/exl3-run.log` and `exl3-dfl.log` from `exl3-run.sh`, `exl3-dfl.sh` and `exl3bench.py`):

| engine, weights | tok/s | VRAM, as logged |
|---|---|---|
| mainline `4ceb171`, GSQ-RCO IQ2_XS / IQ2_S / IQ3_XXS / IQ3_S (`-mtp` files), plain | 50.0 / 48.7 / 47.4 / 46.2 (the three prompts within 0.2 of each) | 9,590 / 10,264 / 11,060 / 12,660 MiB |
| the same, MTP n_max=2 (code / code2 / prose ranges) | 62.6–73.1 across the four quants | 10,470–13,540 MiB |
| ik_llama.cpp (`rcpt/linux-pc-provenance.txt`), the same four, plain | 49.7 / 48.5 / 47.5 / 46.5 | 9,898–12,968 MiB |
| ExLlamaV3 1.5.1, EXL3 4.00 / 5.00 bpw (`-cs 32768`), 6.00 bpw (`-cs 8192`), warm prompts | 43.0 / 35.4 / 33.2 | 17.9 / 20.7 / 22.0 GiB used |
| EXL3 4.00 + DFlash2 (HF-format drafter), code / code2 / prose | 74.1 / 93.5 / 80.1 | 23.1 GiB used |
| EXL3 5.00 + DFlash2 | fails to load: "Insufficient VRAM in split for model and cache" | — |

**`rtx6000ada-host`** (`rtx6000ada-host/logs/exl3-recheck.log`):
- EXL3 4.00: 49.9 / 50.0 / 49.9 tok/s, 18.1 GiB used.
- With DFlash2: 84.2 / 92.8 / 82.2, 23.3 GiB used.

**No quality measurement of any 27B quant on any engine is committed here.** #334 tracks it.

## 5. Runtime facts

- **ExLlamaV3 CPU-MoE offload (`-mcl N`).** It moves the experts of the first N MoE layers to a spawned CPU worker.
  - Cost on Flash-Next 3.05 bpw (`logs/exl3-fn-mcl{10,14,20}.log`): 41.0–47.2 tok/s at 10 layers, 36.9–41.2 at 14, 29.7–34.0 at 20. That is about 0.7–1.3 ms per token per added offloaded layer.
  - A script that loads with it needs an `if __name__ == "__main__":` guard.
- **The PLE (n-gram) table is quantized with its trunk.** EXL3 calibrates the weights against the quantized table, so a table and its trunk stay paired. This is a property of ExLlamaV3's converter, read from its source and not measured here.
- **The EXL3 cache is a paged, unified pool** shared by concurrent requests.
  - `cache_mode 8,8` has **no quality measurement** here, because the KLD rows run without a cache.
  - What is measured: the four-way concurrency check is clean with it (§2).
- **TabbyAPI**, probed 2026-10-03 on the serving instance (`rcpt/apiprobe.py`, `rcpt/apiprobe-2026-10-03.jsonl`):
  - `reasoning_effort: "high"` returns HTTP 400 "TemplateError: Unexpected reasoning effort high. Supported types are xhigh (default), medium, and low."
  - `GET /slots` returns 404.
  - Message content starts with a blank line (`"\n\n391"`).
  - Responses carry llama-style `timings`, including `draft_n` and `draft_n_accepted`.
  - The earlier `apicheck-results.jsonl` (from `apicheck.py`) records the same refusal as a bare HTTP 400.
- **The quants carry an MTP head.** The EXL3 quants of both models include it (`text_config.mtp_num_hidden_layers = 1`), and ExLlamaV3 ≥ 1.5.1 drafts from it with `--mtp`; §3 uses it.
- **ExLlamaV3 can't load the EXL3-converted DFlash2 drafter.** It fails with "Required tensor candidate_selector.predecessor_codebook not found" (`linux-pc/logs/exl3-run.log`).

## Weights

`weights-digests.txt` lists the sha256 and Hugging Face revision of each publisher weight file used above, read from each host's Hugging Face download metadata. Those files were not re-hashed on the host, except as follows:
- **EXL3 2.05 bpw:** every file hashed on `rtx6000ada-host` (`rtx6000ada-host/exl3-205bpw-sha256.txt`).
- **The Flash-Next GGUF trunks used in §1, including the locally rebuilt BF16-PLE second shard:** hashed on `rtx6000ada-host` (`rcpt/trunk-sha256.txt`).

The GSQ-RCO 27B IQ3_S digest (`58fd8267…`) equals the registered `weights_main` of `accel24-llamacpp-qwen38-27b-iq3s`.

## Engines

- **llama.cpp on `rtx6000ada-host`:** the KLD tool, §1 (`rcpt/kld-tool-identity.txt`); the serving `b7-e7051ef`, §2.
- **ExLlamaV3:**
  - 1.5.1 with torch 2.13.0+cu132, for the 1.5.1 KLD rows, §2's no-speculation figures and §4;
  - 1.5.2, wheel `+cu132.torch2.11.0` on torch 2.11.0+cu130, for TabbyAPI, §3 and the 1.5.2 rerun.
- **TabbyAPI:** git `be74bf0`.
- **Version and freeze files:** `rtx6000ada-host/versions.txt`, `rtx6000ada-host/tabby-venv-freeze-2026-10-03.txt`.
- **On `linux-pc`:** mainline `4ceb171` (the candidate rung's engine) and ik_llama.cpp `c5b5773`.
- **Registry row:** the engine fingerprint for a Python/torch line is #337's ruling.
