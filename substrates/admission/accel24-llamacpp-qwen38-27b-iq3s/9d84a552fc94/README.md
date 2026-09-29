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
- The words are the five ruled on #143: `pass`, `fail`, `n/a (reason)`, `unreported`, `unadjudicated`.
- `recompute.sh` re-derives every word that follows from a raw file and refuses one that disagrees. It was seen red on six seeded faults: a flipped word, a tampered raw byte, a misstated divergence position, a raised headroom criterion, and a misstated canary count on each directory.
- `fingerprint.json` gives the recipe and the canonical components. The directory is named by the first 12 hex characters.
- `scrub.json` lists every raw file in which an absolute home path was replaced by `~`, with its pre-scrub digest. This is the only edit to the raw files.

**This is a cells directory, not an admission.** Admission also needs the depth probe and the parity fire on this fingerprint. The admission word is written by hand in `admission.toml` one level up, once those exist; until a checker exists, this README says so.

## The cells

| cell | word | reading |
|---|---|---|
| identity | pass | `llama-server` `865044a2…` at mainline `4ceb171`, the build's and the container's libraries hashed (`raw/engine-manifest.txt`), weights `58fd8267…` |
| kwarg delivery (paired) | pass | thinking off 0 reasoning characters, on 116 |
| rendered effort | unadjudicated | absent = `xhigh`; `low` and `medium` render differently; `high`, `none` and `max` refused (HTTP 500, the template's "Unexpected reasoning effort", text by digest); `reasoning_strength` not read. No word chosen: a reading is before planning on #143 |
| canary | unadjudicated | 36/36, the first draw; Q5's first-draw criterion is unruled |
| output invariance | pass | reading (c): spec-off reproducible within a process and across two; spec-on reproducible with itself; on/off first differs at token 55 |
| headroom | pass | 1,966 MiB free with both slots full, against the floor's 1,208 |
| checkpoint restore | unreported | no instrument; I4b not built |

**Beside the cells:**
- Measured bpw, by D9's method (`raw/bpw.json`): 3.4981 without the MTP layer, 3.5457 with it.
- The canary's decode speed on the same fixture, k = 36: median 76.1 tok/s here, against the floor's 60.0 before and 64.7 after. Single window, single sample per request; the timings are in the raw files.
