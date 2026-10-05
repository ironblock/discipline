# Addendum to #335: the TabbyAPI + vision line that cleared every bar (`linux-pc`, 2026-10-04)

**Measurement only.** Recorded under #335 (5985332225). The maintainer ratified this line as the baseline afterwards (#393); its admission is a separate window. This directory follows `2026-10-04-qwen38-27b-dflash2-exl3/`, whose `wf/` and `wg/` hold the two attempts that failed. Registry ids only.

**The window.** The floor (`accel24-beellama-qwen27b-q4kxl`) was down 22:45:22–22:47:42Z, restored with `provision-diet.sh`. After the restore: exe `980845d60ae7a820`, fingerprint `--hash-models` IDENTICAL `4b194a74ff6ab263`, verify-box PASS, canary 35/36 (`checks/`).

**The line** (`logs/tabby-config.yml`): TabbyAPI `be74bf0a` + ExLlamaV3 1.5.4, EXL3 plain `3.00bpw` + the DFlash2 EXL3 drafter, cache `8,8`, `max_batch_size` 2, `vision: true`, `vision_offload: true`. Two changes from `wg/` fixed it:
1. **`draft_cache_mode: "8,8"`.** TabbyAPI defaults the drafter's cache to FP16. With 8,8 it loads at **196,608**, using 22,662 MiB, where `wg/` managed only 172,032 at 23,162.
2. **An overlay model directory** whose `preprocessor_config.json` sets `size.shortest_edge` to 1,048,576 pixels, up from 65,536. ExLlamaV3 reads that field as `min_pixels`. It means at least 1,024 visual tokens, matching the 3.6 floor's `--image-min-tokens 1024`. Every other file is a symlink to the branch (`scripts/w335h.sh`).

**Results:**
- **#373's image test** (`logs/vision/`: the committed image, request, responses and `derive.json`): three 200s, with 1,099 prompt tokens each. The image now reaches the model at about 1,060 visual tokens. All three replies read "NPC7": **`accepted` ×3**. In `wg/`, at about 250 visual tokens, it was 1 of 3. This is a measurement, not the registry's vision word.
- **Text after the images** (`logs/tabby-text.json`): 1,739 prompt tokens decoded at 104.5 tok/s (cold prefill 1,115); 76,828 tokens decoded at 66.5 (cold prefill 932). The server stayed up; in `wg/` it ran out of memory here.
- **VRAM** (`logs/vram-trace.txt`, 0.2 s samples; `logs/marks.txt` has the phase times): 22,662 MiB after load, **peak 23,816 MiB (760 MiB free)** over the image and text requests. That passes the v0.1.0 bar of 300 MiB (#143 5984376546).

**Redaction:** `$HOME`, `<port>`, `<user>`, `<boxhost>`, `<scratch>`; in `scripts/mach.sh` a private run-directory name and a restore-capture path component are `<run-dir>` and `<restore-capture>`. Nothing else was changed, so the scripts do not run as committed. `SHA256SUMS` covers every file.
