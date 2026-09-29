# Cells: `accel24-llamacpp-qwen38-27b-iq3s`, the middle-rung candidate

The candidate's constitutional cells, on the inference seat's recommended line (#143, comment 5883986293).

## The window

The #143 cells window, 2026-09-29, ratified by the maintainer. The step list was posted on #143 before the start, and the account afterwards.

**Downtime:** production was stopped 05:44:56–06:20:14Z, 35 min 18 s. That includes a 15-minute stall (05:56–06:11): the candidate sat idle waiting for its canary, because launching the box script over SSH hung the orchestrator on this seat's side. Nothing else was served meanwhile.

**Restore:** through the machine's provisioning script under an exit trap. Verified four ways:
- the exe digest and command line read identical;
- `fingerprint --check --hash-models` against the 2026-09-20 capture, rc 0;
- the box verification script, PASS;
- canary 36/36 against the pool.

**How to read the files:**
- `cells.toml` holds one table per cell: its word, instrument, reading, and the raw files it cites by digest.
- The words are those ruled on #143: `pass`, `fail`, `n/a (reason)`, `unreported`, `unadjudicated`, and `baseline`, which is a rung's first canary draw, not a test (comment 5885752512).
- `recompute.sh` re-derives every word that follows from a raw file and refuses one that disagrees. It was seen red on six seeded faults: a flipped word, a tampered raw byte, a misstated divergence position, a raised headroom criterion, and a misstated canary count on each directory. It was then seen red on three more: the candidate's first canary draw given a word other than `baseline`, the refusal row read against the wrong line's results, and a changed floor VRAM figure in `raw/refusal-window.log`.
- `fingerprint.json` gives the recipe and the canonical components. The directory is named by the first 12 hex characters.
- `scrub.json` lists every raw file in which an absolute home path was replaced by `~`, or a private build-container name by `<build-container>`, with its pre-scrub digest. These are the only edits to the raw files. `raw/refusal.json` holds both lines' results as `refusal.sh` wrote them, and each directory reads its own line (`refusal_rung`).

**This is a cells directory, not an admission.** Admission also needs the depth probe and the parity fire on this fingerprint. The depth probe's cell is in `depth/`: `pass`, run 2026-09-29. The parity fire is still to come. The admission word will be in `admission.toml` in this directory once the parity fire exists, derived from the three results by `substrates/admission/derive_admission.py` (#183).

## The cells

| cell | word | reading |
|---|---|---|
| identity | pass | `llama-server` `865044a2…` at mainline `4ceb171`, the build's and the container's libraries hashed (`raw/engine-manifest.txt`), weights `58fd8267…` |
| kwarg delivery (paired) | pass | thinking off 0 reasoning characters, on 116; a chat request at each refused level (high, none, max) returns HTTP 500 (`raw/refusal.json`, production up; the candidate CPU-only, by `raw/refusal.sh` and its `raw/refusal-server.log`: no CUDA device visible, `--gpu-layers` ignored, 2 threads, bound to localhost; the floor's pid and VRAM unchanged across it and its health ok, `raw/refusal-window.log`) |
| rendered effort | n/a (a capability fact on the registry) | absent = `xhigh`; `low` and `medium` render differently; `high`, `none` and `max` refused (HTTP 500, the template's "Unexpected reasoning effort", text by digest); `reasoning_strength` not read. Ruled a capability fact (#143): recorded as `effort` on the registry entry |
| canary | baseline | 36/36, the first draw on this line: its baseline, not a test (Q5). The second draw is the first test |
| output invariance | pass | reading (c): spec-off reproducible within a process and across two; spec-on reproducible with itself; on/off first differs at token 55 |
| headroom | pass | 1,966 MiB free with both slots full, against the floor's 1,208 |
| checkpoint restore | unadjudicated | no gym instrument; I4b not built, so the cell never ran. Whether this engine's line restores checkpoints is unmeasured, so the gap is the gym's and the word is not `unreported` (comment 5887164395; #178 had written `unreported`) |

**Beside the cells:**
- Measured bpw, by D9's method (`raw/bpw.json`): 3.4981 without the MTP layer, 3.5457 with it.
- `raw/identity-before.txt` also records `bottom_weights`: the digest of the CPU rung's weights (`cpu-beellama-qwen3-1p7b-q4km`, the registry's `weights_main`), read because that rung was resident beside the candidate during the fill (D8's m7). No cell cites it.
- The canary's decode speed on the same fixture, k = 36: median 76.1 tok/s here, against the floor's 60.0 before and 64.7 after. Single window, single sample per request; the timings are in the raw files.
