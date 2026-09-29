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
- The words are those ruled on #143: `pass`, `fail`, `n/a (reason)`, `unreported`, `unadjudicated`, and `baseline`, which is a rung's first canary draw, not a test (comment 5885752512).
- `recompute.sh` re-derives every word that follows from a raw file and refuses one that disagrees. It was seen red on six seeded faults: a flipped word, a tampered raw byte, a misstated divergence position, a raised headroom criterion, and a misstated canary count on each directory. It was then seen red on three more: the candidate's first canary draw given a word other than `baseline`, the refusal row read against the wrong line's results, and a changed floor VRAM figure in `raw/refusal-window.log`.
- `fingerprint.json` gives the recipe and the canonical components. The directory is named by the first 12 hex characters.
- `scrub.json` lists every raw file in which an absolute home path was replaced by `~`, or a private build-container name by `<build-container>`, with its pre-scrub digest. These are the only edits to the raw files. `raw/refusal.json` holds both lines' results as `refusal.sh` wrote them, and each directory reads its own line (`refusal_rung`).

**This is a cells directory, not an admission.** Admission also needs the depth probe and the parity fire on this fingerprint. The depth probe's cell is in `depth/`: `pass`, run 2026-09-29. The parity fire is still to come. The admission record is `admission.toml` in this directory, with `admission-recompute.sh` beside it. It cites the cells, `depth/` and the parity fire (#115) by digest. Its word is written by hand, and for now it is held: `checkpoint_restore` is `unadjudicated` until I4b runs. No checker derives the word yet; that is a filed gate item.

## The cells

| cell | word | reading |
|---|---|---|
| identity | pass | exe `980845d6…` and the exact argv, read before the stop and after the restore |
| kwarg delivery (paired) | pass | thinking off 0 reasoning characters, on 505; refused-level row n/a: no level is refused, every level returns 200 (`raw/refusal.json`, taken by `raw/refusal.sh` on the production line; the floor's pid and VRAM unchanged across it, `raw/refusal-window.log`) |
| rendered effort | n/a (a capability fact on the registry) | the template reads no effort key: every level renders identically |
| canary | pass | 36/36 before, 36/36 after, against the pool of 354/360 at z 2.576 |
| output invariance | pass | reading (c): spec-off reproducible within a process and across two; on/off first differs at token 51; spec-on not reproducible with itself after a restore (token 67), declared on the registry's hazards line |
| headroom | n/a (the reference line) | 1,208 MiB free: the inference seat's reading, not re-taken here |
| checkpoint restore | unadjudicated | no gym instrument; I4b not built, so the cell never ran. The engine exposes checkpoint restore, so the word is not `unreported` (comment 5887164395; #178 had written `unreported`) |

`raw/canary-pool.json` is the pooled baseline with the 2026-09-29 pre-run draw added, which the registry's canary text points to.
