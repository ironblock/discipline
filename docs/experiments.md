# Experiments

*Every test anyone wants to run, as a row: what lever it moves, what instrument measures it, and where it stands. The gap this file fills: until now these lived in four people's and sessions' memories. A row with a result links its directory; a wanted row names the piece it waits on.*

*Compiled by planning on 2026-10-07 and adapted by Claude on 2026-10-08. No human has authored this text yet; the maintainer edits before it is relied on.*

## Two kinds of row

**Explorations** are tried to learn what is worth claiming. A builder or the maintainer runs one when its piece exists. It needs no pre-registration or ratification, only a record of what ran, on what regime, and what came out, so a later claim can cite it as the reason it was asked. Most wanted rows start here. The compaction-depth sweep is one.

**Claims** are what the program publishes. Claims publication is a `Later —` milestone (#495), so no claim is owed before feature complete. When a row becomes a claim, these rules apply:
- **The rule comes first.** It is fixed by digest and ratified before any data: `rule_ratified.at` precedes `window_start`, or the row reads post-hoc.
- **Bars are checked against the instrument.** Each bar is compared with the instrument's attainable ceiling before a verdict is read.
- **Out-of-range parameters downgrade the row.** Parameters outside the recommended range make the row a characterisation.
- **Controls are seen red before the run.** A control bounds noise; it cannot find a confound that scales with the treatment.
- **One fire per row, the first.**

The words are `supported | refuted | inconclusive | unadjudicated`. A `comparison` row carries its own words. The contract for a results directory is `results/_template/`.

## With a result

| row | lever | word | where |
| --- | --- | --- | --- |
| sense bakeoff (#24) | embedding tier | inconclusive: MiniLM within margin of instruction-tuned models | `results/2026-09-17-sense-bakeoff-adjudication/` |
| entry-to-turn nomination (#17) | fork delivery · supersession | refuted, twice | `results/2026-09-19-entry-to-turn-nomination-adjudication/`, `results/2026-09-20-entry-to-turn-nomination-v2-adjudication/` |
| false-nomination framing (b) and (b)-v2 (#89) | fork delivery | refuted, both draws: advisory ≈ sham on turn change | `results/2026-09-20-false-nomination-framing/`, `results/2026-09-21-false-nomination-framing-v2/` |
| (b′) edit rate (#114) | fork delivery | supported: imperative 53/99, advisory 1/99, sham 0 | `results/2026-09-27-false-nomination-edit-rate/` |
| (b′) on Flash-Next (#142) | fork delivery × substrate | supported; comparison `substrate_independent`: imperative framing overrides capability | `results/2026-09-29-false-nomination-edit-rate-second-substrate/` |
| parity fire `extraction-acceptance-inverts` (#115) | admission | supported, inside the band | `results/2026-09-25-extraction-acceptance-parity/` |
| the same parity fire on the candidate rung | admission × substrate | supported; comparison inconclusive, its sign negative | `results/2026-10-01-extraction-acceptance-parity-candidate/` |
| judge seat, zero-shot (#165 → #499) | decision model | refuted for the zero-shot model (26.7% four-way, near chance); the Sonnet judges agree (Fleiss κ 0.886 and 0.969) | `results/2026-10-03-laya-calibration-bound/` |
| depth probe on the 3.6 floor (#143) | substrate | `pass`: no cliff at 0.5, 0.9 or 0.95 of the 160,000-token pool (deepest 151,622 tokens) | `substrates/admission/accel24-beellama-qwen27b-q4kxl/7c254834cc2c/depth/` |

## Pre-registered or in flight

| row | lever | waits on |
| --- | --- | --- |
| placement vs pruning at equal VRAM (#199) | substrate format | a routing profile from the archive's drives |
| the 27B quality ladder on the floor (#394) | substrate | the floor window |

## Wanted, by the piece each waits on

| row | lever(s) | instrument | waits on |
| --- | --- | --- | --- |
| **compaction depth 0-1-n** | compaction depth | the same trajectory refilled at none / one / n / total; the second half's success and the receipt | the seam (#493, landed) and the v0.2.0 session |
| supersession at the seam (#132) | seam · supersession | the ratify ask over the register's mined reversals; recall vs the collector's ~0 | the seam's audit (#504) |
| model-elected purge (#149) | tool-output disposition | a true advisory nudge in the tool-result envelope vs policy-forced vs none vs sham | the tool loop (#298) and forks |
| "how long" (tool-call duration) | approval · cache lifetime | a typed score from (command, repo, history); labels free from the logged timings | timings in committed results |
| token carriage as a receipt number (#31) | tool-output disposition | sum over tool outputs of size × later requests carrying it | T1's recording |
| the fork economics simulation | cache lifetime | fork reads at measured prefix sizes × window compliance, over the archived message history | no box; a notebook |
| the judge seat, fine-tuned (#499) | decision model | agreement with the Sonnet majority vs the zero-shot baseline; calibration within the bound | real trajectories with the decisions in them |
| the embedding shortlist vs MiniLM | embedding tier | the sense register; pairs/STS on short technical English | after the first drive; no box |
| mimicry and capture under native tool calls (#20, #21) | forks | the first driven sessions' typed outcomes | T1 |
| the tool-shape census (#80) | distribution matching | the same operation under several signatures; well-formed-call rate | the tool loop |
| T1 vs T3 across substrates | substrate | the same trajectory on the floor and in the archived Sonnet 5 session | the adapter (#500, #501) |
| the ladder's three-rung comparison (#143) | substrate | one claim on all three rungs under a three-rung rule | the bottom rung's admission |
| the fleet runoff (V100 / MI100 / PVC) | substrate | admission within a declared tuning window; loaded cost per successful task | the cards |

None of the wanted rows is a ticket. A row becomes one when its piece exists and someone means to run it.

## The problems, for anyone who knows a method we do not

#24 (second edition) states eleven problems, each as input → output, with its labels, its budget and what was refuted. The constraints that govern all of them:
- nothing runs on the executor's GPU;
- each decision in a session takes at most about 100 ms;
- the text is English, code-mixed;
- anything that reaches the model needs precision over recall;
- mechanical rules come first, and a model handles the residual.
