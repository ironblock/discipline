+++
hypothesis = "Fired once per seat on the candidate rung, under the configuration the archived row declares, the extraction-seat bakeoff reproduces the archived row's result: seat B, the small extractor answering a self-contained ask off the accelerator, has the larger deduped gate-accept rate than seat A, the candidate extracting off the warm session prefix, by an amount within the band derived from the archived row's committed tallies."
result = "supported"
kind = "reproducible-by-config"
product_sha256 = "f912a6ac86451a3d94e0f96a43862569c2d57b64e31011faf9e21548d0201a40"
pre_registration_sha256 = "b36fb625380b428f5500560450f6bd4d35bb7a7d911f0c893dfec60fea77a872"
controls_run = ["the routing fingerprint: every seat-B extraction answer without draft_n, every seat-A extraction answer and every interview answer of both seats with it", "the seat-B canary before the first fork: no think block and no draft_n", "the candidate's canary before and after the fire, each against its baseline draw", "the plan check: every planned fork ran with its planned request"]
known_defects = [
  "What the word weighs, stated before the run (#115's reading, carried): the band is wide (0.024877 to 0.21789 around the archived 0.091469), so `supported` says the inversion held on the candidate rung at a size anywhere in that range. The fire's own 95% interval, [-0.008232, 0.112803] over the 31 paired forks, straddles zero: the word rests on a point estimate whose own interval reaches the sign the rule would call refuted. That is the one sentence the ruled rule writes beside the word (#143, comment 5921525110).",
  "The comparison row is inconclusive, and its sign is negative: the candidate's effect (0.037095) is 0.051377 below the floor's (0.088472), just past the 0.05 the rule reads as independent and short of the 0.15 it reads as dependent. The directional prediction (independent) is not met, by 0.001377. No paired p was computed: below 0.15 the dependent word cannot hold whatever p is.",
  "A disclosed regime difference, not a parameter to equalise: the candidate's template renders `Reasoning effort is set to xhigh` by default (window/rendered-head.json), and the floor's renders no level. The fire ran at the rung's rendered state, as pre-registered. Extraction forks ran with thinking off (enable_thinking false), as #115's; the start row's `reasoning = \"off\"` names that, and the interview forks' state is the template's.",
  "This is a parity fire of a retired lane, as #115's was: the research program removed the length-triggered extraction gate from its harness on 2026-08-16, and the fire ran that program's harness at 0e14292, rebuilt --locked at #115's build path, byte-identical to #115's binary (f5682886...). Its offline rehearsal planned 87 forks per seat; every one ran here with its planned request (apply.py --check-plan).",
  "The source drive (the human-driven drive of 2026-08-10) and the seat arms are consumed pinned-only, as #115's were, so `kind` is `reproducible-by-config` on that precedent: re-firing needs the research program's private drive.",
  "The fired logs were scrubbed as #115's were: the private alias map, then home prefixes collapsed to `~`; the planned requests passed the same two steps, which is what lets their digests match. The map is private; the raw logs are not committed.",
  "Interview forks are answer-dependent beyond the 56 planned: the applier requires none of the extras and reads nothing from them; a planned fork that did not run would be a reported miss (none here).",
  "`box.json`'s instance id is transcribed, not derivable: the fingerprint logs name the capture's digest (7117e838a6657c55, identical before and after, which recompute.sh checks), and the registry's 2026-10-01 instance (#206) is the one pinned by that capture, whose file is private.",
  "The candidate's canary before and after the fire were each one draw (36 of 36), read against the candidate's baseline draw by the instrument's rule; the canary files here are those draws' logs. The floor's canary after the restore (35 of 36, PASS against its pool) is in window/canary-floor-after.log and is not part of box.json, whose canary fields are the candidate's.",
  "`apply.py`'s selftest runs under #115's config (extraction-acceptance-inverts-115.json, carried here), the config its fixtures were written and proven against (#191); under the candidate config it stops on a fixture written for #115's switches. The verdict itself is re-derived under the candidate config.",
  "`pre-registration.json` and `decision-rule.toml` were written as files after the fire, transcribing the ratified text on #143 (comments 5894139110 and 5921525110); the operative rule is apply.py and the candidate config, merged in #191 before the first fork.",
  "The README's prose figures are not bound to the product, the class #115's record discloses: recompute.sh re-derives the band, the grader's report, the verdict, the interval, the plan check, the comparison and the front matter's numbers, and reads nothing else.",
  "The window's raw outputs under window/ are the Mac driver's and the box script's logs, scrubbed as the seat logs were; server logs are not committed (they carry paths). mac.log and box-window.log give each step's time and exit code; #143 carries the same timeline, posted after the applier ran.",
]
targets_checked = 54
targets_matched = 54

[regime]
arm = "extraction-seat-parity-refire"
substrates = ["accel24-llamacpp-qwen38-27b-iq3s", "cpu-beellama-qwen3-1p7b-q4km"]
dogma_version = 0

[derivation]
applier_sha256 = "c4ec0983bbc4ca84bf29a140348a0cdf0ee9da9356896086b8d003d8687e6b84"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "e32eae7aab07590e33c3f25012ff32f83de4fe4db13e61ba1d2f524cf34c6cf6"
+++

# Extraction acceptance inverts, fired on the candidate rung

The candidate rung's parity fire (#143): `results/2026-08-10-extraction-acceptance-inverts` fired on `accel24-llamacpp-qwen38-27b-iq3s` and compared with the archived row's band, as #115 compared the floor.

## Observation

#115 re-fired the archived row on the floor and the inversion held (0.088472). A rung is admitted only if the extraction contract's inversion holds on it too (#143, I5), so the candidate rung needed the same fire, with seat B held constant and seat A the candidate.

## Hypothesis

Fired once per seat on the candidate rung, under the configuration the archived row declares, the extraction-seat bakeoff reproduces the archived row's result: seat B, the small extractor answering a self-contained ask off the accelerator, has the larger deduped gate-accept rate than seat A, the candidate extracting off the warm session prefix, by an amount within the band derived from the archived row's committed tallies.

## Test

The pre-registration is planning's #143 comment 5894139110, as ruled in 5921525110 and ratified by the maintainer; it is committed here as `pre-registration.json` and `decision-rule.toml`, and the applier and its config were merged in #191 before the first fork.

- **The band** is the archived row's, #115's `band.json`, derived by `band.py` from the archived tallies.
- **The fire.** Seats A and B were fired once each on 2026-10-01 through #115's harness (`0e14292`, byte-identical binary), replaying #115's source drive: seat A on the candidate, seat B's extraction lane on the same CPU substrate as #115's.
- **The plan.** The offline rehearsal's 87 planned forks per seat; every one ran with its planned request (`apply.py --check-plan`), so no interview miss could void the fire.
- **The box record.** `box.json`, one field per check, re-derived from the raw outputs under `window/`.
- **The word.** `apply.py` with `extraction-acceptance-inverts-candidate.json`: the unadjudicated checks first, then the effect against the band; the fire's own interval written beside it (`interval.json`).
- **The comparison.** The effect against the floor's (#115) under `comparison-rule.toml`.

The window, 2026-10-01 (UTC): production down 14:51:06–16:07:46; preflight (the floor's fingerprint identical to its 2026-10-01 capture, verify-box PASS); the candidate up (exe `865044a2…`) and seat B's server up (exe `980845d6…`); the live rehearsal; the candidate's canary before, 36/36; seat A 14:55–15:13 and seat B 15:13–16:02, each rc 0; the candidate's canary after, 36/36; the checkpoint arm and the GPU reference (their verdicts are in the admission tree); the restore, same exe and command line, the fingerprint check and verify-box after both passing, the floor's canary 35/36 PASS. `build_info` as each server reported it: the candidate `b1-4ceb171`, seat B `b0-unknown-dirty` (`window/build-info-*`).

Re-run: `bash recompute.sh` re-derives each consumed digest, the band, the grader's report over the fired logs, the verdict and the interval under the candidate config, the plan check, `box.json` from the window's raw outputs, the start row's engines and weights from the identity read, the comparison row and the front matter's numbers.

## Results

Every unadjudicated check passed: counted forks 31 of 31 on each seat, 31 in common, no misses, no interview misses; seat A accepted 52, above the floor of 10.

| | offered | accepted, deduped | rate |
| --- | --- | --- | --- |
| seat A, the candidate | 1177 | 52 | 0.044180 |
| seat B, the 1.7B on CPU | 2227 | 181 | 0.081275 |

The effect, seat B minus seat A pooled over the 31 paired forks, is **0.037095**, inside the band [0.024877, 0.21789]: **supported**. Beside the word: **The fire's own 95% interval [-0.008232, 0.112803] straddles zero.**

The comparison row: the candidate's effect against the floor's 0.088472 differs by **-0.051377** -- not under 0.05 (independent), not reaching 0.15 (dependent): **inconclusive**, sign **negative**; the prediction, independent, is not met.

## Conclusion

Supported: the inversion holds on the candidate rung, at about two fifths of the floor's size. The weight is low twice over -- the band is wide, and the fire's own interval reaches below zero -- and the comparison with the floor does not decide whether the inversion's size depends on the rung: the difference sits just past the independence margin. What is still unknown is the archived row's own open question: whether what either seat accepts is true of the mechanism.
