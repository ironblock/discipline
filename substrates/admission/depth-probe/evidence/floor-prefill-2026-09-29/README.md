# The floor's cold prefill rate and the unified cache's two slots, 2026-09-29

Measured with production up, on `accel24-beellama-qwen27b-q4kxl`: `-np 2 --kv-unified`, 160,000 tokens per slot (`/props`). Each request was one cold prompt (`cache_prompt: false`, `n_predict 1`), its length counted by the server's `/tokenize`. There was one sample per depth.

| depth (prompt_n) | prefill tok/s | wall s | file |
|---|---|---|---|
| 39,962 | 1,055.4 | 38.4 | `console.log` (the run's JSON was never written: it stopped at the first 125k error) |
| 74,962 | 732.9 | 103.0 | `console.log`, as above |
| 124,962 | 805.4 | 155.4 | `prefill-floor-125.json` |
| 149,962 | 737.7 | 203.5 | `prefill-floor-150.json` |

**The sequence, and which file holds each step:**
1. The 40k and 75k requests succeeded (`console.log`).
2. The first 125k request, sent while the 75k prompt was resident, returned HTTP 500. Its client traceback went to stderr, which that run did not capture; the server's own line is the first "Context size has been exceeded" in `server-errors.txt`.
3. Erasing either slot with `POST /slots/N?action=erase` was refused with 501, because the line was started without `--slot-save-path`. This is in `slot-erase-refusal.txt`, transcribed from the terminal and labelled as such.
4. A second 125k attempt succeeded (`prefill-floor-125.json`).
5. A 150k attempt failed the same way while the 125k prompt sat in the other slot. The end of its client traceback is `console.log` line 8, and the server's line is the second in `server-errors.txt`.
6. After one tiny uncached request pinned to each slot (`id_slot` 0 and 1, `cache_prompt: false`), 150k succeeded (`console.log`, `prefill-floor-150.json`).

**The reading:** under `--kv-unified`, another slot's resident prompt counts against the shared cache, and a pinned tiny uncached request per slot releases it. This is what `depth_probe.py`'s `clear_slots` does before every cell.
