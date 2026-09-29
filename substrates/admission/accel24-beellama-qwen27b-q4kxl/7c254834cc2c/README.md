# Cells: `accel24-beellama-qwen27b-q4kxl`, the floor as served

The floor's constitutional cells, run once on production as served, as planning's corrected ruling on #143 requires. No freeze record of them existed.

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
| identity | pass | exe `980845d6…` and the exact argv, read before the stop and after the restore |
| kwarg delivery (paired) | pass | thinking off 0 reasoning characters, on 505 |
| rendered effort | unadjudicated | the template reads no effort key: every level renders identically |
| canary | pass | 36/36 before, 36/36 after, against the pool of 354/360 at z 2.576 |
| output invariance | pass | reading (c): spec-off reproducible within a process and across two; on/off first differs at token 51; spec-on not reproducible with itself after a restore (token 67), declared on the registry's hazards line |
| headroom | n/a (the reference line) | 1,208 MiB free: the inference seat's reading, not re-taken here |
| checkpoint restore | unreported | no instrument; I4b not built |

`raw/canary-pool.json` is the pooled baseline with the 2026-09-29 pre-run draw added, which the registry's canary text points to.
