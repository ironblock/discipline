# The substrate ladder

Where each rung of the claims ladder stands, what admitted or failed to admit it, and the rulings that shaped it. Distilled from #143, which carried the ladder from 2026-09-28 to 2026-10-04 as a running thread; this page replaces the thread as the place to read the current state. The thread stays the archive, and every ruling below links to the comment that made it.

Registry ids only. `registry.toml` holds each substrate's identity; `admission/<substrate>/<fingerprint>/` holds each admission record; `measurements/` holds characterization, which never admits anything.

## The goal, in one paragraph

Every claim declares which rungs it fires on. A word that changes between rungs is then read as a capability gap with a location, not as noise. Three rungs: a small CPU-capable model at the bottom, an always-available floor on `linux-pc`, and intermittent capacity on `rtx6000ada-host` at the top. Admission to a rung is measured, never declared (#143's body).

## The ladder today (2026-10-04)

| rung | substrate | state | section |
|---|---|---|---|
| top, intermittent | `ada48-tabbyapi-exl3-qwen38flashnext-2p05` | serving since 2026-10-03; **unadmitted** | [Top rung](#top-rung) |
| middle, the floor for new claims | `accel24-llamacpp-qwen38-27b-iq3s` | admitted ([5939594798](https://github.com/ironblock/discipline/issues/143#issuecomment-5939594798)); ruled the floor ([5954674262](https://github.com/ironblock/discipline/issues/143#issuecomment-5954674262)); launched per window, not yet the machine's serving line | [Middle rung](#middle-rung) |
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

<!-- Written by the data seat (Track 4): each record's admission words, with the measurement and admission directories by digest. -->

*The data seat writes this section.*

## Top rung

**Line:** `ada48-tabbyapi-exl3-qwen38flashnext-2p05`. Qwen3.8-Flash-Next as EXL3 2.05 bpw with its MTP head, served by TabbyAPI + ExLlamaV3 1.5.2: one 262,144-token paged pool, four concurrent requests, `cache_mode 8,8`, MTP with 3 draft tokens. It took over from `ada48-llamacpp-qwen38flashnext-q20` at 2026-10-03T16:22Z after 23 s of downtime, on the maintainer's go ([5971023545](https://github.com/ironblock/discipline/issues/143#issuecomment-5971023545), `measurements/2026-10-03-ada48-tabbyapi/`). The llama.cpp launch file stays on the host for rollback.

**State: unadmitted.** No admission cell has run on it. Its identity is the composite over its components (#337). Planning set three prerequisites before admission ([5966413123](https://github.com/ironblock/discipline/issues/143#issuecomment-5966413123)):

| prerequisite | status |
|---|---|
| quantized KV at full context | served at 262,144 with `cache_mode 8,8`, and the four-way concurrency check is clean with it; the cache's effect on quality is **unmeasured**, since the KLD scorer runs without a cache (`measurements/2026-10-03-characterization/` §5) |
| a real server with reasoning and timing fields | met: TabbyAPI serves `reasoning_content` and llama-style `timings` including draft counts (§5) |
| a back-to-back A/B in one window | met: ABAB against llama.cpp on 2026-10-03 (§2) |

Its depth probe runs at the served 262,144 once the cache is measured.

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

<!-- Written by the data seat (Track 4): the top rung's admission words (none yet), with directories by digest. -->

## Bottom rung

<!-- Written by the data seat (Track 4): admission words, if any, with directories by digest. -->

*The data seat writes this section.*

## Engine lines

What each engine is in the ladder, and what was measured about it. Characterization, not admission.

| engine | where | role |
|---|---|---|
| beellama `preview-v0.3.2` (exe `980845d6…`) | `linux-pc` | the archived floor's engine; also seat B's CPU server |
| llama.cpp mainline `4ceb171` | `linux-pc`, in a build container | the middle rung's engine |
| llama.cpp, ironblock's fork `e7051ef` (`q8-sparse-fa`) | `rtx6000ada-host` | the top rung's previous engine, kept for rollback |
| TabbyAPI + ExLlamaV3 1.5.2 | `rtx6000ada-host` | the top rung's engine since 2026-10-03 |

**Mainline and beellama are equivalent on the 27B's weights.** KLD against Q8_0 agrees within 0.0002 on code and 0.0014 on prose for every GSQ-RCO quant (`measurements/2026-10-03-qwen38-27b-quality-ladder/`). The engine choice between them carries no measurable quality cost.

**`e486f80` is withdrawn.** It is `e7051ef` plus a cherry-picked upstream fix for per-block QSA bias indexing, and it mixes concurrent requests' contexts under `--kv-unified`. The top rung ran it from about 2026-09-30T23:20Z until the revert at 2026-10-03T06:58:30Z, and multi-stream use in that span carries the hazard ([5966611289](https://github.com/ironblock/discipline/issues/143#issuecomment-5966611289), `measurements/2026-10-03-ada48-concurrency/`, `hazard_regime_boundary` in the registry). Planning's 2026-10-01 text read the boundary backwards; the registry carries the correction.

**Quant format matters more than bits per weight, and the winner depends on the model.**
- On Flash-Next, EXL3 2.05 bpw is smaller than GSQ-RCO Q2_0 and has about half its KLD (table above). Planning's reading: Q2_0's advantage was a format whose unpack is cheap on llama.cpp's CUDA path, and EXL3's trellis format is built for its own kernels ([5966413123](https://github.com/ironblock/discipline/issues/143#issuecomment-5966413123)).
- On the 27B, at matched size, GSQ-RCO IQ3_S beats EXL3. EXL3 2.50 bpw is 1.5% larger than IQ3_S with 1.8× its code KLD; EXL3 3.00 bpw roughly ties it for 11–14% more bytes (`measurements/2026-10-03-qwen38-27b-quality-ladder-size-matched/`).
- Prose costs more than code at every rung of both ladders: 1.3–1.6× on the 27B, 2.4–2.7× on Flash-Next.

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
| a laptop rung (Splash, oMLX, or AFM 3 Core through sidekick) | **candidates, not rungs.** Whether a laptop rung is wanted at all is the maintainer's question, unanswered | [5960078036](https://github.com/ironblock/discipline/issues/143#issuecomment-5960078036), [5879482684](https://github.com/ironblock/discipline/issues/143#issuecomment-5879482684), [5971097635](https://github.com/ironblock/discipline/issues/143#issuecomment-5971097635) |

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

## Open questions

- **The floor's ratification and switch.** Planning ruled the candidate the floor on 2026-10-02. The definition of done also needs the maintainer's ratification of it, and `linux-pc`'s serving line is still the 3.6. Successor: DoD 2.
- **The three-rung re-fire.** One existing claim, the maintainer's pick, re-fired on all three rungs under a pre-registered rule. Blocked on a bottom rung and an admitted top rung. Successor: DoD 3.
- **The record gate.** `check-record` refusing a claim record whose declared rung has no admission directory. Successor: DoD 4.
- **The top rung's admission** on the TabbyAPI + EXL3 line, beginning with the cache's quality. Successor: top rung.
- **The bottom rung.** No candidate (`cpu-beellama-qwen3-1p7b-q4km`, MiniCPM5-2B on mainline, AFM 3 Core through sidekick) has an admission record.
- **A laptop rung,** and whether `rtx6000ada-host` admits claims generally (Q13): both the maintainer's.
