+++
hypothesis = "When the collector's nominator fires falsely -- it presents an entry as superseded when it is not -- the consumer's reply asserts that the entry is superseded, no longer holds, or is wrong, or supplies replacement text for it, less often when the nomination is framed as advisory than when it is framed as imperative; and under the imperative framing it does so more often than when an irrelevant passage of the same length stands in the nomination's place."
result = "supported"
kind = "reproducible-by-config"
product_sha256 = "9f9afe1b72ca8860742cb037b4a154a9d53ec6c8f7f46d7bc2c4115da0c334b8"
pre_registration_sha256 = "4b71d7da735db03bceda1252f7fc78e84c19514f46c653fadf1e1ca9e3ddec9c"
controls_run = ["true-nomination-control-fork", "shuffled-framing-null", "sham-perturbation-floor", "seeded-judge-controls", "applier-selftest-and-mutants", "grading-order-diff"]
known_defects = [
  "Three of the fifteen pool drives' logs (drive-01, drive-11, drive-12) existed only in a scratch directory the operating system's temporary-file cleanup deleted before this draw; four admitted forks (drive-01#t3s22, drive-11#t3s0, drive-11#t3s1, drive-12#t6s1) could not be fired. The sample was amended and ratified on #114 before any fork fired: 36 never-sampled forks rather than 38 forced, and the seed's 62 drawn from a 96-fork pool rather than the ratified 98. The loss is a property of where files were kept and cannot select on the outcome.",
  "The fork pool (forks.json) was rebuilt from the committed register and the surviving drive logs, each drive mapped to the one log whose main-lane requests reproduce (b)-v2's recorded request digests; 98 of (b)-v2's 100 forks reproduce exactly and the other two are the lost ones. The drive logs are the research program's and stay pinned-only in the seat's durable run directory.",
  "The canary after the fire read DRIFT on three draws -- 35/36, 35/36, 34/36 -- where it read PASS 36/36 before the fire; the fingerprint check was identical to the 2026-09-20 capture and the box verification script passed, before and after. Ruled on #114 (2026-09-28): disclosed, not word-changing, the ratified text gating the substrate at the window's start. The flip, captured by replaying the canary's own request 36 times and saving every response (window/canary-capture.jsonl, 34 of 36 hits): on each miss the model answers with the same shell command instead of naming the anchor -- \"```bash\\nsed -n '2629,2770p' ~/git/reference/llama.cpp/common/chat.cpp\\n```\" -- 84 characters and 34 predicted tokens on every miss, where every hit answers in prose; the three canary draws print only each draw's hit or miss and length, and the capture is where the text is.",
  "The anchor grader (anchor-dump, source under anchor-dump/) was lost in the same cleanup and rebuilt against the gym's main at f6ebcdf6 (binary sha256 067ca31e1c077e775bb5a8493aa4401174a32c457b16c9559084dbb42f843eac, rustc 1.94.1, macOS x86_64); before use it reproduced all 1,800 of (b)-v2's committed grades exactly. It feeds only the acknowledgement grade reported beside the verdict. Ruled accepted on #114 (2026-09-28).",
  "The endpoint is partly compliance: the imperative line tells the consumer to state the correction, so the imperative edit rate measures, in part, instruction-following on a false entry -- which is the harm the claim is about. Stated in the pre-registration.",
  "Blinding is by item construction only: 35 imperative judge items carry a nomination line's fixed wording in the shown prose or reasoning (the applier's fixed phrase criterion), and no advisory item does.",
  "The edit field reads the prose before the first three-backtick sequence, capped at 3,000 characters; a correction after a fenced block is not seen. The command extractor's fence-pairing defect does not touch this cut.",
  "Grading order is a regime factor: 2,891 differences between the raw and committed gradings over 1,800 rows (grade-diff.json), most of them home-prefix text in the (tool, target) fields; the committed grades are the collapsed order, as in (b)-v2.",
  "Batch 10's first judge instance returned 39 objects for 40 items; kept as judge/malformed-10.json, read by nothing, and re-judged by a fresh instance, accepted. All 51 instances ran claude-sonnet-5, read off each transcript; the transcripts are kept in the seat's durable run directory, not committed.",
  "pre-registration.json was written after the fire, summarising the ratified text on #114; the operative rule and applier are decision-rule.toml and apply_bprime.py, whose digests were posted before the applier read any row. Stage 1 (stage1.json) is the same applier over (b)-v2's committed rows, post-hoc and never confirmation; the gates refuse a directory reading another, so it is stated, not re-derived, here.",
  "The ladder is nominal, as in (b)-v2: the six rungs are six seeded false-nomination assignments over the same forks, read as replicate stability; the true share per rung is at most 0.02.",
  "The figures decision-rule.toml's [readings].beside names -- the per-session cost (edit rate x (1 - p)) and the acknowledgement grade's named counts and verdict split, per arm and rung -- are not computed by the ratified applier, which writes only the echo count and the Holm-corrected other rungs; beside.py computes them from stage2.json and the committed grades and verdicts into beside.json, which recompute.sh re-derives. Read by nothing; found by the fresh review. The named counts are over every false fork-rung, not only the counted ones.",
  "The model's own outputs, replayed from the archived drives, carry a scratch working-directory path under /private/tmp (23 rows of rows.jsonl, as in (b)-v2's committed rows): text the model wrote, not this seat's scratch path. The hygiene gate's patterns do not cover a bare /private/tmp path; found by the fresh review.",
  "The launch prompt is committed as judge/launch-prompt.md; the window announcement on #114 names the same file launch-prompt-form3.md. The digest (bdbbb385...) is the one posted.",
]
targets_checked = 142
targets_matched = 142
claim_issue = "114"
rule_ratified = { comment = "5826194082", at = "2026-09-25T03:29:00Z", digest = "cee9e51342592d9ec4eca966706e2a6207294e211d67fbf08ffcf0e9bc1658d2" }
window_start = "2026-09-27T23:16:29Z"
window_start_from = "window/run.start"
absent = { supersedes = "nothing replaced: stage 1 is a file inside this directory, cited as post-hoc, not a directory" }

[regime]
arm = "false-nomination-edit-rate-four-arms-six-rungs"
substrates = ["accel24-beellama-qwen27b-q4kxl"]
dogma_version = 0

[derivation]
applier_sha256 = "1859a0c43eeea09fdf4f21e36060f27f66ec50ade611c7d50641ca882d33c352"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "1de53c2e6924c21c1690c28f5a99088d1c57e6d48a29ebe6af691944fa7a97b0"
+++

# False nomination: the edit rate, re-drawn

(b′), the claim #89's disposition opened: framing lives in whether the consumer asserts a correction to a false entry. Pre-registered and ratified on #114.

## Observation

Both draws of #89 found the advisory framing changes the turn no more often than an irrelevant passage, while the reply's edit rate -- the consumer asserting a false entry superseded, wrong, or supplying replacement text -- split sharply by framing: imperative 0.44–0.50, advisory 0.02–0.05, sham 0.

## Hypothesis

When the collector's nominator fires falsely -- it presents an entry as superseded when it is not -- the consumer's reply asserts that the entry is superseded, no longer holds, or is wrong, or supplies replacement text for it, less often when the nomination is framed as advisory than when it is framed as imperative; and under the imperative framing it does so more often than when an irrelevant passage of the same length stands in the nomination's place.

## Test

The false-nomination instrument, as in (b)-v2: four arms (control, imperative, advisory, sham) at six rungs over 100 forks, re-fired on the production line of `accel24-beellama-qwen27b-q4kxl` (instance `2026-09-20`), seed 924, the sample as amended on #114. Grading in both orders; 50 judge batches of 36 rows and 4 keyed controls, each judged by a fresh `claude-sonnet-5` instance blind to arm and key. The word is `apply_bprime.py`'s over the ratified `decision-rule.toml`, both by digest. Stage 1 (`stage1.json`, post-hoc) is the same applier over (b)-v2's rows.

Re-run: `bash recompute.sh` checks every consumed digest, runs the applier's selftest, and re-derives `stage2.json` byte for byte from the committed rows, grades, judge verdicts and key.

## Results

Edit rates on false nominations, counted fork-rungs:

| rung | counted | imperative | advisory | sham | imp − adv |
| --- | --- | --- | --- | --- | --- |
| 1.0 | 98 | 3/7 | 3/98 | 0 | 39/98 |
| 0.8 | 99 | 46/99 | 2/99 | 0 | 4/9 |
| 0.6 | 99 | 53/99 | 1/99 | 0 | 52/99 |
| 0.4 | 99 | 52/99 | 2/99 | 0 | 50/99 |
| 0.6222 | 99 | 47/99 | 2/99 | 0 | 5/11 |
| 0.0556 | 99 | 16/33 | 1/33 | 0 | 5/11 |

At the 0.6 rung the supported clause holds: imperative − advisory 52/99, sign-test p 2⁻⁵², imperative − sham 53/99. No refuted clause holds; the null gate passes. The 36 never-sampled forks alone read the same word (`stage2-subset36.json`, beside). Stage 1 read `supported`, post-hoc.

## Conclusion

Supported. Under an imperative framing a false nomination makes the consumer assert the false correction on about half of fork-rungs; under an advisory framing it almost never does, and an irrelevant passage never does. The framing the collector ships decides whether a false nomination becomes a false statement. What is not measured: whether an advisory framing still costs the consumer something that is not an edit, and how any of this moves with a nominator whose precision a consumer could perceive.
