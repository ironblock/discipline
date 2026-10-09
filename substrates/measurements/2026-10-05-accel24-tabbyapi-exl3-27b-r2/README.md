# Config r2 of the 3.8 EXL3 line on linux-pc, 2026-10-05 (#393, #497)

Track 6's retune r2 of `accel24-tabbyapi-exl3-qwen38-27b-3p00` changes three things from the ratified 2026-10-04 config:
`max_seq_len` and `cache_size` 196608 → 163840, `warmup: true`, and `sampling.override_preset: qwen38_thinking`
(fallbacks only; the model's generation_config values). The live file is sha256 `1e17c49d…`; the redacted copy and the
redaction diff here are byte-identical to #393 5998974930, three non-serving lines redacted (the port and the two model
directories).

These are measurements. The r2 instance row in `substrates/registry.toml` cites them.

## The window (`window/`), steps 1 to 4

| step | result |
|---|---|
| 1 identity | the components read on the host match the registry's r2 components; composite `61b380f2…` (`identity.json`, `raw/identity.json`) |
| 2 kwarg delivery | `raw/kw.json`, `raw/refusal.json` |
| 2 canary | 36/36, this config's baseline draw (`raw/canary.log`, `canary-baseline.json`) |
| 2 fingerprint | `fingerprint.json`, sha256 starting `3060052d6cb9` |
| 3 depth probe | 24 rows at 0, 81,920, 147,456 and 155,648 tokens of depth, 0 server errors; word `pass` (`depth/`) |
| 4 parity fire | ran clean: both seats exited 0, canary 36/36 before and after, identity before and after byte-identical to step 1, 178 and 148 `/v1/model` checks all equal to the served id, no OOM (`parity/`) |

**The parity word is `inconclusive`.** All unadjudicated checks passed, with 31 of 31 forks paired. The effect is +0.0032 against the
band [0.0249, 0.2179]. It keeps the reference sign but lands below the band, and the fire's own 95% interval [-0.0512, 0.0725]
straddles zero (`parity/verdict.json`, `parity/interval.json`).

**The maintainer's ruling (#393 6000878147):** parity is not a bar on the 3.8 line. It tests the 3.6's behaviour against
itself. The redesign is #491. The word is kept as measured.

To re-derive the verdict from the repository root, with the candidate fire's archived row and band:

```
r=results/2026-10-01-extraction-acceptance-parity-candidate
d=substrates/measurements/2026-10-05-accel24-tabbyapi-exl3-27b-r2/window/parity
python3 -B substrates/admission/parity/apply.py substrates/admission/parity/configs/extraction-acceptance-inverts-tabby.json \
  $r/archived $r/band.json $d $d/box.json --interval-out interval.json
```

The printed verdict and the written interval are byte-identical to `parity/verdict.json` and `parity/interval.json`.

The seat logs went through the same export as the candidate fire's: the private alias map, then home prefixes collapsed to `~`.
The routing is `identity`, read from the proxies' check lists in `box.json`. The box's `verify_*` and `fingerprint_*` fields are
the identity re-read before and after (exit 0, byte-identical to step 1).

**Not run on r2:** the after-load headroom cell (#480), the vision cell and the #421 capture. Production was restored before
them. Vision was measured `accepted` ×3 on these weights under the earlier config
(`substrates/measurements/2026-10-04-qwen38-27b-tabbyapi-vision/`).

## Side runs (`side-runs/`), earlier the same day

Diagnosis on r2 before the window. No admission words come from them.

- **Headroom fill:** two concurrent requests sized to the 163,840 pool, peak 22,442 MiB with 2,134 MiB free (Track 6's reading).
- **Depth probe:** `side-runs/depth/`, the same shapes as the window's step 3.
- **The parity fire's mix replayed four times:** every pass exited 0 (`side-runs/fire-loop.log`). Track 6 sampled VRAM at 1 Hz and
  read a peak of 22,804 MiB with 1,772 MiB free and no OOM. The VRAM series stay on the host.

The ratified config's void run and its side run are in
`substrates/admission/accel24-tabbyapi-exl3-qwen38-27b-3p00/6a96d5696231/parity-void-2026-10-05/`. They record the engine's
OOM 10 min into seat A, and that the fire alone did not reproduce it.

`SHA256SUMS` covers every file here.
