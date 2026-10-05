# The substrate ladder

Where each rung of the claims ladder stands, what admitted or failed to admit it, and the rulings that shaped it. Distilled from #143, which carried the ladder from 2026-09-28 to 2026-10-04 as a running thread; this page replaces the thread as the place to read the current state. The thread stays the archive, and every ruling below links to the comment that made it.

Registry ids only. `registry.toml` holds each substrate's identity; `admission/<substrate>/<fingerprint>/` holds each admission record; `measurements/` holds characterization, which never admits anything.

## The goal, in one paragraph

Every claim declares which rungs it fires on. A word that changes between rungs is then read as a capability gap with a location, not as noise. Three rungs: a small CPU-capable model at the bottom, an always-available floor on `linux-pc`, and intermittent capacity on `rtx6000ada-host` at the top. Admission to a rung is measured, never declared (#143's body).

## The ladder today (2026-10-04)

| rung | substrate | state | section |
|---|---|---|---|
| top, intermittent | `ada48-tabbyapi-exl3-qwen38flashnext-2p05` | serving since 2026-10-03; **unadmitted** | [Top rung](#top-rung) |
| middle, ratified baseline | TabbyAPI + ExLlamaV3, EXL3 plain `3.00bpw` + DFlash2 with the BF16 vision tower (its registry id comes with its admission) | ratified ([5985589548](https://github.com/ironblock/discipline/issues/393#issuecomment-5985589548)); **unadmitted** until its window runs (#393) | [Engine lines](#engine-lines) |
| middle, admitted candidate | `accel24-llamacpp-qwen38-27b-iq3s` | admitted ([5939594798](https://github.com/ironblock/discipline/issues/143#issuecomment-5939594798)). Planning ruled it the floor ([5954674262](https://github.com/ironblock/discipline/issues/143#issuecomment-5954674262)), but that was not ratified; the maintainer ratified the EXL3 line instead. Launched per window, never the machine's serving line | [Middle rung](#middle-rung) |
| archived substrate of record | `accel24-beellama-qwen27b-q4kxl` | admitted at `7c254834cc2c`; still `linux-pc`'s serving line | [Middle rung](#middle-rung) |
| bottom | none admitted | `cpu-beellama-qwen3-1p7b-q4km` is registered and serves as the parity fire's seat B, independent of its own admission | [Bottom rung](#bottom-rung) |

## What admits a rung

Three results, each its own directory, cited by digest from one admission record that carries the word ([5882266710](https://github.com/ironblock/discipline/issues/143#issuecomment-5882266710)):

1. **The constitutional cells:** checkpoint restore including recurrent state on hybrids, kwarg delivery by the negative control (#94) with a refused-level row, the canary at the declared test, headroom, invariance.
2. **The depth probe** in the rung's supported coding configuration, at fractions of its declared serving context.
3. **The parity fire** of `extraction-acceptance-inverts` against #115's archived band.

An admission belongs to one fingerprint: engine, weights, template, serving line and reasoning state. A change to any of them re-admits; an OS-only change is a new instance and does not ([5884256685](https://github.com/ironblock/discipline/issues/143#issuecomment-5884256685), [5925191280](https://github.com/ironblock/discipline/issues/143#issuecomment-5925191280)). Admission is to the ladder; eligibility is per claim, from the cells its pre-registration requires.

Cell words: `pass`, `fail`, `n/a (reason)`, `unreported`, `unadjudicated`, and `baseline` for a rung's first canary draw. Only `fail` bars; `unadjudicated` waits on a re-run.

## Middle rung

Two admission records sit on `linux-pc`'s 24 GiB accelerator. Each one's word is derived by `admission/derive_admission.py` (#183) from the three results it cites. Each result is pinned by the sha256 of its file manifest, and `verify.sh --only admission` re-derives every word and refuses a record whose word or digests differ.

**`accel24-llamacpp-qwen38-27b-iq3s`, the admitted candidate: admitted at `9d84a552fc94`.** Planning's ruling that it is the floor (5954674262) was never ratified. The maintainer ratified the EXL3 line instead (#393 5985589548).

| result | directory | manifest sha256 | word |
|---|---|---|---|
| cells | `admission/accel24-llamacpp-qwen38-27b-iq3s/9d84a552fc94/` | `199c9bd24ced5d11…` | identity, kwarg delivery, output invariance, headroom, checkpoint restore `pass`; canary `baseline` (the rung's first draw); rendered effort `n/a` (a template fact, carried on the registry entry) |
| depth probe | `…/9d84a552fc94/depth/` | `3a488da76d1923f0…` | `pass` (no cliff) |
| parity fire | `results/2026-10-01-extraction-acceptance-parity-candidate/` | `3d4ac379368f04cc…` | `supported` |

- **The fingerprint:** `9d84a552fc940b7f…`, over engine, weights, template, serving line and reasoning state.
- **Registry lag:** the registry entry's `admission` text still reads "not admitted", from before the depth probe and the parity fire landed. The record's derived word is `admitted`; the correction is a registry row of its own.
- **Characterization beside it,** which admits nothing:
  - the quant ladder: `measurements/2026-10-03-qwen38-27b-quality-ladder/`, SHA256SUMS `ab0aa80e876752ed…`;
  - its size-matched addendum: `…-quality-ladder-size-matched/`, `ea6c20cc6f0a901b…`.

**`accel24-beellama-qwen27b-q4kxl`, the archived substrate of record: admitted at `7c254834cc2c`.**

| result | directory | manifest sha256 | word |
|---|---|---|---|
| cells | `admission/accel24-beellama-qwen27b-q4kxl/7c254834cc2c/` | `edbbedbf275d1299…` | identity, kwarg delivery, canary, output invariance, headroom, checkpoint restore `pass`; rendered effort `n/a` (as above) |
| depth probe | `…/7c254834cc2c/depth/` | `944fdd3729424a89…` | `pass` |
| parity fire | `results/2026-09-25-extraction-acceptance-parity/` | `a615bdefc7939c15…` | `supported` |

- **The fingerprint:** `7c254834cc2c49ac…`.
- **Headroom** passes by construction on the reference line: Q9 as amended (#143, [5889255742](https://github.com/ironblock/discipline/issues/143#issuecomment-5889255742)).
- **OS-only instances** recorded after the admission (2026-09-29), `2026-10-01` and `2026-10-02`, each carry an `admission_note` saying the admission stands, as ruled. The current one is `2026-10-02`, restored with its fingerprint check and canary (`measurements/2026-10-02-floor-restore/`).
- **The vision cell** (#373) reads `accepted` on this line, in `…/7c254834cc2c/vision/`, outside the cells manifest. It is evidence for the registry's `vision` field, not an admission result. It merged in #392, and `verify.sh --only admission` re-derives it against the registry.

## Top rung

**Line:** `ada48-tabbyapi-exl3-qwen38flashnext-2p05`. Qwen3.8-Flash-Next as EXL3 2.05 bpw with its MTP head, served by TabbyAPI + ExLlamaV3: one paged KV pool shared by four concurrent requests, `cache_mode 8,8`, MTP with 3 draft tokens. It took over from `ada48-llamacpp-qwen38flashnext-q20` at 2026-10-03T16:22Z after 23 s of downtime, on the maintainer's go ([5971023545](https://github.com/ironblock/discipline/issues/143#issuecomment-5971023545), `measurements/2026-10-03-ada48-tabbyapi/`). The llama.cpp launch file stays on the host for rollback.

**YaRN ×2, 2026-10-04 (#401, on the maintainer's decision):** production serves the card's YaRN form at factor 2, so `max_seq_len` is 524,288 inside the 606,208 pool.
- **Short-text cost:** KLD 0.0880 / 0.2317 against native's 0.0860 / 0.2297.
- **Retrieval:** three needles found at ~252k and at 480,125 tokens.
- **Decode:** 2–8% slower.

This is a new instance ([#401 5985333953](https://github.com/ironblock/discipline/issues/401#issuecomment-5985333953)).

**Engine upgrade, 2026-10-04 (#401, on the maintainer's decision):** ExLlamaV3 1.5.2 → **1.5.4**, and the pool 262,144 → **606,208 tokens**. `max_seq_len` stays 262,144 per request.
- **Why:** 1.5.3–1.5.4 fix the quantized cache (an off-by-one for unaligned geometries) and MTP prefix-cache hashing, both on this line's paths.
- **Measured in the window ([#401](https://github.com/ironblock/discipline/issues/401)):**
  - KLD identical: 0.0860 / 0.2297.
  - The four-way concurrency check is clean.
  - VRAM after load: 41,238 MiB against 43,762, at the same pool.
  - Identical greedy repeats give identical acceptance and speed on 1.5.4, and drift on 1.5.2.
  - Decode is 2–5% slower at 10k.
- **Pool sizing:** the KV costs 18.9 KiB/token. 638,976 does not load. At 606,208, a full four-request fill leaves 978 MiB free.

This is a new instance; the data seat writes its row.

**State: unadmitted.** No admission cell has run on it. Its identity is the composite over its components (#337). Planning set three prerequisites before admission ([5966413123](https://github.com/ironblock/discipline/issues/143#issuecomment-5966413123)):

| prerequisite | status |
|---|---|
| quantized KV at full context | served at a 606,208-token pool (`max_seq_len` 524,288 since YaRN ×2) with `cache_mode 8,8`, and the four-way concurrency check is clean with it; the cache's effect on quality is **unmeasured**, since the KLD scorer runs without a cache (`measurements/2026-10-03-characterization/` §5) |
| a real server with reasoning and timing fields | met: TabbyAPI serves `reasoning_content` and llama-style `timings` including draft counts (§5) |
| a back-to-back A/B in one window | met: ABAB against llama.cpp on 2026-10-03 (§2) |

Its depth probe runs at the served `max_seq_len` (524,288) once the cache is measured.

**The previous line was never admitted either.** `ada48-llamacpp-qwen38flashnext-q20` carried one claim by ruling, #142's, whose rule never depends on token identity. Whether the host admits claims generally (Q13) was never answered. Its last instance, `2026-10-03` on `b7-e7051ef`, is retired, and every result that cites it keeps citing it.

**Why the engine changed** (`measurements/2026-10-03-characterization/` §1–§2), at the same card and less memory:

| | llama.cpp `b7-e7051ef`, GSQ-RCO Q2_0 | TabbyAPI + EXL3 2.05 bpw |
|---|---|---|
| size on disk | 66 GB | 59 GB |
| KLD against Q8_0, code / prose | 0.1593 / 0.3866 | 0.0860 / 0.2297 |
| decode, 1 stream at 10k / "100k" / "200k" | 96.7–106.4 / 69.8–82.1 / 57.6–62.3 | 129.3–131.3 / 121.9–132.6 / 108.9–124.9 |
| VRAM after load | 46,572 MiB | 43,762 MiB |
| four-way concurrency check | clean | clean |

Cross-engine KLD includes the engines' numerical differences, which can only overstate EXL3's loss. ExLlamaV3's calibration set includes Wikipedia text, a head start on the prose corpus; no corpus window was found in it. MTP draft length was swept in-process: n=3 is best single-stream, n=1 best at four concurrent requests (§3).

**Client-visible differences** from the llama.cpp line, for whoever fires its cells: `reasoning_effort: "high"` is a 400 (the template knows `low`, `medium`, `xhigh`), content may open with a blank line after the reasoning, there is no `/slots` endpoint, and the sampler card is TabbyAPI's defaults unless a request sets its own (§5, the registry entry).

**Admission words: none.** No directory exists under `admission/ada48-tabbyapi-exl3-qwen38flashnext-2p05/`, and the registry's `admission` reads `unadmitted`. Its admission is #396. The receipts that pin the line, which admit nothing, are:
- `measurements/2026-10-03-ada48-tabbyapi/`: versions, the frozen package set, the redacted config and its diff, the per-file weight digests, the API checks and the switch log;
- `measurements/2026-10-03-characterization/`, SHA256SUMS `addeb2e36613bea0…`.

## Bottom rung

**Admission words: none.** No directory exists under `admission/` for a bottom-rung substrate.

`cpu-beellama-qwen3-1p7b-q4km` is registered and serves as the parity fire's seat B. That is a role inside another rung's result, not an admission of its own. No cell, depth probe or parity fire has been run with it as the rung under test.

## Engine lines

What each engine is in the ladder, and what was measured about it. Characterization, not admission.

| engine | where | role |
|---|---|---|
| beellama `preview-v0.3.2` (exe `980845d6…`) | `linux-pc` | the archived floor's engine; also seat B's CPU server |
| llama.cpp mainline `4ceb171` | `linux-pc`, in a build container | the middle rung's engine |
| llama.cpp, ironblock's fork `e7051ef` (`q8-sparse-fa`) | `rtx6000ada-host` | the top rung's previous engine, kept for rollback |
| TabbyAPI + ExLlamaV3 1.5.4 (1.5.2 until 2026-10-04) | `rtx6000ada-host` | the top rung's engine since 2026-10-03 |

**Mainline and beellama are equivalent on the 27B's weights.** KLD against Q8_0 agrees within 0.0002 on code and 0.0014 on prose for every GSQ-RCO quant (`measurements/2026-10-03-qwen38-27b-quality-ladder/`). The engine choice between them carries no measurable quality cost.

**`e486f80` is withdrawn.** It is `e7051ef` plus a cherry-picked upstream fix for per-block QSA bias indexing, and it mixes concurrent requests' contexts under `--kv-unified`. The top rung ran it from about 2026-09-30T23:20Z until the revert at 2026-10-03T06:58:30Z, and multi-stream use in that span carries the hazard ([5966611289](https://github.com/ironblock/discipline/issues/143#issuecomment-5966611289), `measurements/2026-10-03-ada48-concurrency/`, `hazard_regime_boundary` in the registry). Planning's 2026-10-01 text read the boundary backwards; the registry carries the correction.

**Quant format matters more than bits per weight, and the winner depends on the model.**
- On Flash-Next, EXL3 2.05 bpw is smaller than GSQ-RCO Q2_0 and has about half its KLD (table above). Planning's reading: Q2_0's advantage was a format whose unpack is cheap on llama.cpp's CUDA path, and EXL3's trellis format is built for its own kernels ([5966413123](https://github.com/ironblock/discipline/issues/143#issuecomment-5966413123)).
- On the 27B, at matched size, GSQ-RCO IQ3_S beats EXL3. EXL3 2.50 bpw is 1.5% larger than IQ3_S with 1.8× its code KLD; EXL3 3.00 bpw roughly ties it for 11–14% more bytes (`measurements/2026-10-03-qwen38-27b-quality-ladder-size-matched/`).
- Prose costs more than code on every quant measured: 1.1–1.6× on the 27B, and 2.4–2.7× at Flash-Next's full-expert rungs (4.3× for the pruned Coder).

**The middle rung's speed at depth** (`linux-pc`, the candidate's registered line; the profile window of 2026-09-29, [5883986293](https://github.com/ironblock/discipline/issues/143#issuecomment-5883986293); greedy, 256 tokens, single samples):

| depth (prompt tokens) | prefill tok/s | decode, MTP n=2 | decode, spec off |
|---|---|---|---|
| 2,236 | 978 | 68.1 | 45.3 |
| ~98k | 858 | 43.1 | 27.7 |
| ~125k | 474 | 34.6 | 18.3 |
| 226,750 (full) | 586 | 35.5 | — |
| 2 streams × ~112.6k | — | 29.9 aggregate | — |

Draft acceptance rises to 0.92–0.97 at 126k and beyond on this prompt, so the deep MTP rows flatter typical use, and spec-off is the clean reading at depth. MTP n=3 adds nothing over n=2 for 300 MiB more. The 3.6 floor's first matched depth reading is in the #335 table below.

**DFlash2 against MTP, EXL3 against GSQ-RCO, and the 3.6 floor at depth** (`linux-pc`, 2026-10-04, #335; six windows with every arm recorded in `measurements/2026-10-04-qwen38-27b-dflash2-exl3/`, and a seventh, the vision fix-up, in `measurements/2026-10-04-qwen38-27b-tabbyapi-vision/` once it is committed). Measurement only: none of this is a floor decision. The bench is the profile's; decode tok/s is the mean of two warm repeats, and two-stream figures are totals.

| line | KV | pool | 2k | ~100k | deep | 2 streams | free at peak |
|---|---|---|---|---|---|---|---|
| 3.6 floor, beellama, original DFlash | q5_0 / q4_1 | 160,000 | 74.4 | 40.9 | 37.4 (157k) | 30.1 | (1,208, #143) |
| 3.8 IQ3_S, mainline, MTP n=2 | q8_0 | 229,376 | 68.1 | 43.1 | 35.5 (227k) | 29.9 | 1,966 |
| 3.8 IQ3_S, mainline, DFlash2 Q4_K_M (Z-Lab GGUF) | q8_0 | 229,376 | 74.6 | 44.7 | 38.5 (226k) | 32.6 | 1,412 |
| the same, DFlash2 Q3_K_M / Q2_K (Anbeeld) | q8_0 | 229,376 | — | 42.5 / 41.3 | — | — | — |
| EXL3 `SC_3.00bpw_H4`, ExLlamaV3 1.5.4, DFlash2 (EXL3 drafter) | `5,4` | 229,376 | 100.5 | 52.5 | 45.1 (226k) | 55.2 | 3,406 |
| EXL3 `SC_3.00bpw_H4`, the same | `8,8` | 196,608 | 111.1 | 57.9 | 60.6 (194k)\* | 59.0 | 874 |
| EXL3 `SC_2.20bpw_H3`, the same | `8,8` | 229,376 | 142.7 | 60.6 | 35.5 (226k) | 51.7 | 1,778 |
| **EXL3 plain `3.00bpw`, the same** | `8,8` | 196,608 | **120.6** | **64.1** | 43.4 (194k) | **60.0** | 488 |

\* That depth's prompt ends on easier text (draft acceptance 0.53, against 0.33–0.40 elsewhere). Deep rows compare only at matched depths.

- **KLD, the same harness as #334:**

  | quant | size | code / prose KLD |
  |---|---|---|
  | EXL3 `SC_2.20bpw_H3` | 10.80 GB | 0.1226 / 0.1697 |
  | GSQ-RCO IQ3_S | 12.12 GB | 0.0542 / 0.0860 |
  | EXL3 `SC_3.00bpw_H4` | 13.45 GB | 0.0591 / 0.0738 |
  | EXL3 plain `3.00bpw` (#334 addendum) | 13.82 GB | 0.0522 / 0.0731 |
  | EXL3 `3.50bpw` | 15.34 GB | 0.0382 / 0.0421 |
- **Not run:** EXL3 + MTP (dropped at the maintainer's direction); EXL3 `3.50bpw` with the drafter, which did not load at 163,840 or more on 1.5.1. The README records every probe.
- **Vision.** No speed arm above loaded a vision tower. Both EXL3 3.00 quants carry the full BF16 tower.
  - TabbyAPI can keep the tower's weights in system RAM (`vision_offload`). The encode still runs on the GPU; ExLlamaV3 has no CPU path for it.
  - **First attempt** (TabbyAPI defaults): it loaded only at 172,032, read #373's image 1 of 3 at about 250 visual tokens, and ran out of memory on the next request.
  - **With two fixes** — `draft_cache_mode "8,8"`, and an image minimum of 1,024 visual tokens (`size.shortest_edge` 1,048,576, matching the 3.6's `--image-min-tokens 1024`) — it loads at 196,608, reads the image `accepted` ×3, and serves text afterwards at 104.5 tok/s (1.7k) and 66.5 (77k), with 760 MiB free at peak.
  - That is the line the maintainer ratified (`measurements/2026-10-04-qwen38-27b-tabbyapi-vision/` once committed).
- **Headroom:** under the v0.1.0 bar (300 MiB free at peak fill, [5984376546](https://github.com/ironblock/discipline/issues/143#issuecomment-5984376546)), every row passes at the pool it ran.
- **The floor refuses a request that doesn't fit its shared pool, rather than evicting the other slot's resident prompt:** HTTP 500 "Context size has been exceeded". #406.

**Hazards the record carries** ([5922544337](https://github.com/ironblock/discipline/issues/143#issuecomment-5922544337), in the registry entries):
- llama.cpp with `-fit` and no `--spec-draft-ngl 99` silently places the MTP draft on the host.
- q8_0 KV without the `q8-sparse-fa` patch disables the sparse-attention decode path.
- On `linux-pc`'s Ampere card, any quantized-KV batch wider than one query takes a full-cache f16 conversion, which covers the MTP verify batch and two-stream decode.
- `-b` is a regimen field: it sets how long a co-resident stream stalls while another slot prefills.
- Speculation is not bit-exact with the spec-off path on these hybrid models; nothing may be gated on token identity.

## Parked and declined

| option | state | ruling |
|---|---|---|
| Strata (single-stream tiered-placement engine for Flash-Next) | **parked** by the maintainer: not a rung on `linux-pc` (32 GB of RAM forces a floor-displacing low-RAM mode, and its speeds are the author's estimates) and not on `mac-pro-2019` (no CUDA). Its regime facts stand if un-parked: single-stream, its calibration fingerprinted, its speed projection (an output-altering control vector) off | [5962681150](https://github.com/ironblock/discipline/issues/143#issuecomment-5962681150), [5963393133](https://github.com/ironblock/discipline/issues/143#issuecomment-5963393133) |
| GSQ-RCO Coder build of Flash-Next | **not a candidate** for any rung: code KLD at parity, prose KLD +84% and top-1 −7.7 points against Q8_0, and an agent's reasoning is prose | [5922544337](https://github.com/ironblock/discipline/issues/143#issuecomment-5922544337) |
| a laptop rung (Splash) | **candidates, not rungs.** Whether a laptop rung is wanted at all is the maintainer's question, unanswered | [5960078036](https://github.com/ironblock/discipline/issues/143#issuecomment-5960078036), [5971097635](https://github.com/ironblock/discipline/issues/143#issuecomment-5971097635) |

## Decision log

One line per ruling, oldest first. M is the maintainer; P is planning; D is Dispatch ruling a how.

| date | by | ruling | comment |
|---|---|---|---|
| 2026-09-29 | P | A rung is admitted by three results (cells, depth probe, parity fire), one fire each; the 3.6's record is assembled from what measured it | [5882266710](https://github.com/ironblock/discipline/issues/143#issuecomment-5882266710) |
| 2026-09-29 | M | Headroom (Q9) is the production line's own measured headroom, not 1 GiB; `-np 2` is a hard floor | [5882668772](https://github.com/ironblock/discipline/issues/143#issuecomment-5882668772) |
| 2026-09-29 | P | Correction: no record of the 3.6's cells existed, so they run fresh; the June depth probe migrates only if it recomputes | [5883561324](https://github.com/ironblock/discipline/issues/143#issuecomment-5883561324) |
| 2026-09-29 | P | A probe admits only the instance it ran on; admission is per fingerprint; cell words `n/a`, `unreported`, `fail`; eligibility is per claim | [5884256685](https://github.com/ironblock/discipline/issues/143#issuecomment-5884256685) |
| 2026-09-29 | P | A refused effort level is a capability fact on the registry entry, not a cell word; the kwarg control gains a refused-level row | [5884923913](https://github.com/ironblock/discipline/issues/143#issuecomment-5884923913) |
| 2026-09-29 | D | Cells live under `admission/<substrate>/<fingerprint>/`; the passing word is `pass`; invariance is read under (c); the candidate's id | [5884955595](https://github.com/ironblock/discipline/issues/143#issuecomment-5884955595) |
| 2026-09-29 | M | The research repository's design may be reused; its results are not migrated | [5885088019](https://github.com/ironblock/discipline/issues/143#issuecomment-5885088019) |
| 2026-09-29 | P | The depth probe: the gym's own tree as padding with generated counter-examples, depths as fractions of serving context, five samples, `no cliff` = within one sample of the zero-pad control, mechanical grader | [5885161766](https://github.com/ironblock/discipline/issues/143#issuecomment-5885161766) |
| 2026-09-29 | P | Under `--kv-unified` the context is a pool shared by every slot; the strict reading is reported beside the word; `unadjudicated` means the cell did not complete | [5885436821](https://github.com/ironblock/discipline/issues/143#issuecomment-5885436821) |
| 2026-09-29 | P | A rung's first canary draw is its `baseline`; eligibility is on the rendered reasoning state; the floor's effort reads `n/a (template has no levels)` | [5885752512](https://github.com/ironblock/discipline/issues/143#issuecomment-5885752512) |
| 2026-09-29 | D | Checkpoint restore is `unadjudicated` until the gym has an instrument, so the instrument is built | [5887164395](https://github.com/ironblock/discipline/issues/143#issuecomment-5887164395) |
| 2026-09-29 | D | `n/a` cells count green; the reference line passes headroom against itself; the checkpoint tolerance rule as built; the 3.6 admitted at `7c254834cc2c` | [5889255742](https://github.com/ironblock/discipline/issues/143#issuecomment-5889255742) |
| 2026-09-29 | P | The candidate's parity pre-registration: the archived band, seat B held constant, a counted-fork floor, the comparison row against #115 | [5894139110](https://github.com/ironblock/discipline/issues/143#issuecomment-5894139110) |
| 2026-09-30 | P | Its four gaps: pool over the intersection of counted keys; errors as they fall; interview misses void only if they change a later request; the interval reported as a number; seat-A floor 10 | [5921525110](https://github.com/ironblock/discipline/issues/143#issuecomment-5921525110) |
| 2026-10-01 | P | Registry hazards on both rungs; `-b` is a regimen field; the Coder build is not a candidate | [5922544337](https://github.com/ironblock/discipline/issues/143#issuecomment-5922544337) |
| 2026-10-01 | D | An OS-only change is a new instance; the admission stands | [5925191280](https://github.com/ironblock/discipline/issues/143#issuecomment-5925191280) |
| 2026-10-01 | D | The candidate derived `admitted`; the floor's checkpoint tolerance is now GPU-derived | [5939594798](https://github.com/ironblock/discipline/issues/143#issuecomment-5939594798) |
| 2026-10-01 | M | Ratified 5921525110 before the candidate's window opened (given in Dispatch's session 2026-10-01, posted 2026-10-02) | [5946588217](https://github.com/ironblock/discipline/issues/143#issuecomment-5946588217) |
| 2026-10-02 | P | The candidate is the floor for every new claim; the 3.6 is the archived substrate of record; nothing is deleted or re-fired | [5954674262](https://github.com/ironblock/discipline/issues/143#issuecomment-5954674262) |
| 2026-10-02 | P | Splash is a laptop-rung candidate, admitted like any rung if a laptop rung is wanted | [5960078036](https://github.com/ironblock/discipline/issues/143#issuecomment-5960078036) |
| 2026-10-02 | M | Strata parked | [5963393133](https://github.com/ironblock/discipline/issues/143#issuecomment-5963393133) |
| 2026-10-03 | P | EXL3 via TabbyAPI is a candidate engine for the top rung; a Python engine's identity is its resolved environment (form ruled on #337) | [5966413123](https://github.com/ironblock/discipline/issues/143#issuecomment-5966413123) |
| 2026-10-03 | M | The top rung's serving line moves to TabbyAPI + EXL3, a new unadmitted substrate | [5971023545](https://github.com/ironblock/discipline/issues/143#issuecomment-5971023545) |
| 2026-10-03 | P | On a laptop that serves a model, a helper runs on the Neural Engine or it evicts the served model | [5971097635](https://github.com/ironblock/discipline/issues/143#issuecomment-5971097635) |
| 2026-10-04 | D | Distil #143 into this page, re-home its open definition of done, then close it (approved by the maintainer) | [5982627646](https://github.com/ironblock/discipline/issues/143#issuecomment-5982627646) |
| 2026-10-04 | M | The headroom bar for v0.1.0 is 300 MiB free at peak fill, or no OOM at the registered pool under the bench; the 1,208 MiB rule is withdrawn | [5984376546](https://github.com/ironblock/discipline/issues/143#issuecomment-5984376546) |
| 2026-10-04 | M | The top rung moves to ExLlamaV3 1.5.4 with a pool sized to that bar (606,208) | [#401 5984591312](https://github.com/ironblock/discipline/issues/401#issuecomment-5984591312) |
| 2026-10-04 | M | The top rung serves YaRN ×2 (`max_seq_len` 524,288) | [#401 5985333953](https://github.com/ironblock/discipline/issues/401#issuecomment-5985333953) |
| 2026-10-04 | M | Ratified: TabbyAPI + ExLlamaV3 1.5.4 serving EXL3 plain `3.00bpw` + DFlash2 with the offloaded BF16 vision tower becomes `linux-pc`'s baseline, pending admission. An earlier relay that evening, ratifying the llama.cpp DFlash2 line, was retracted and deleted at the maintainer's instruction | [5985589548](https://github.com/ironblock/discipline/issues/393#issuecomment-5985589548) |
| 2026-10-04 | M | The new line's four engine-specific cells: checkpoint restore `n/a (no slot-cache report on this engine)`; output invariance `n/a` under the hazard line; the parity fire's routing check replaced by the instance identity plus `/v1/model`'s id, under a fresh pre-registration; template digest and rendered effort read from the model directory | [#393 5986337117](https://github.com/ironblock/discipline/issues/393#issuecomment-5986337117) |

## Open questions

- **The floor's admission and switch.** On 2026-10-04 the maintainer ratified a different line from the one planning ruled on 2026-10-02: TabbyAPI + ExLlamaV3 serving EXL3 `3.00bpw` + DFlash2 with the BF16 vision tower ([5985589548](https://github.com/ironblock/discipline/issues/393#issuecomment-5985589548); § Engine lines). An earlier relay that evening, ratifying the llama.cpp DFlash2 line, was retracted at the maintainer's instruction. The line is unadmitted until its window runs (cells, depth probe, parity, vision cell). The data seat is building the instruments' TabbyAPI forms, against the four cell rulings at [5986337117](https://github.com/ironblock/discipline/issues/393#issuecomment-5986337117). `linux-pc` serves the 3.6 until then. #393.
- **The three-rung re-fire.** One existing claim, the maintainer's pick, re-fired on all three rungs under a pre-registered rule. Blocked on a bottom rung and an admitted top rung. #394.
- **The record gate.** `check-record` refusing a claim record whose declared rung has no admission directory. #395.
- **The top rung's admission** on the TabbyAPI + EXL3 line, beginning with the cache's quality. #396.
- **The bottom rung.** No candidate (`cpu-beellama-qwen3-1p7b-q4km`, MiniCPM5-2B on mainline, AFM 3 Core through sidekick) has an admission record.
- **A laptop rung,** and whether `rtx6000ada-host` admits claims generally (Q13): both the maintainer's.
