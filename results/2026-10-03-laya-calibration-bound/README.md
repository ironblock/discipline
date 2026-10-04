+++
hypothesis = "laya-typed-decisions as shipped, with this program's own temperature, agrees with the fresh Sonnet majority at or above the judges' own agreement, and is calibrated within the labels' noise, on both questions."
result = "refuted"
kind = "reproducible-by-config"
product_sha256 = "e79ed1a6b689579d6fb98305397392d5133a56172acd758196d973d969d49fb3"
controls_run = ["seeded-judge-controls", "void-and-rejudge", "archive-same-version-replication", "fp32-parity-reference", "second-served-path", "abstention-band", "two-floor-constructions", "cross-fit"]
known_defects = [
  "The hypothesis is planning's text (#165, 5975253990, (d)), the pre-registration having stated the bound and the three-way reading but no sentence. This record is the ZERO-SHOT BASELINE of #165's hypothesis, a model fine-tuned on this program's judged rows: laya-typed-decisions as shipped, which the fine-tune must clear. It does not test that hypothesis, and #165 stays open; the claim row's id says so.",
  "The verdict question's temperature fits all land at the pre-registered range's upper bound, T = 20 (fold A, fold B, all 400; both paths). A predictor at that temperature is near-uniform, its top-1 confidence at its own 26.7% accuracy, so its cross-fitted ECE (0.011) is small for a reason the claim does not name. Ruled (5975253990, (a)): a fit at the bound reads `inconclusive (fit at bound)`, never `calibrated`; both ECEs are reported, 0.234 at T = 1 and 0.011 at the bound. The range was not widened after the result.",
  "The floor's construction was not fixed by the text. Ruled (5975253990, (b)): F2, the base-rate predictor (every item predicted at the fresh majority's base rates, the pre-registration's words), is the floor of record; F1, consistency resampling of the served pass's own cross-fitted vectors, is carried beside, being a function of the model under test. 10,000 draws each; the readings agree under both, and recompute.sh refuses the word if they ever disagree.",
  "The size rule as amended (5966195560, line 3) cannot fire, a defect of the amendment's text (5975253990, (c)): it enlarges the held-out set when the floor is at or above the bound, the bound is e3 + floor, so it needs e3 <= 0, and e3 = 3d^2 - 2d^3 > 0 whenever 0 < d < 1.5. Here d is 0.0792 and 0.0100, so e3 is 0.0178 and 0.0003 and the rule reads false for both questions. The rule planning meant, the floor against the label noise alone, is the next pre-registration's.",
  "The abstention band is max(the served path's measured flip margin, 0.05 logits) as 5973863758 reads amendment 6: the grade of record's largest fp32 margin among ANE decision changes is 0.0242, so the band is 0.05. The `threshold the cross-fit derives` in that reading is not a quantity amendment 4's temperature fit produces; the band uses the measured flip margin it names in amendment 6's own text.",
  "Batch 29 is 29a, as ruled (5973836546): its first instance ran `grep` on its own batch file to count ids, a breach of the launch prompt's letter under the data seat's mid-run rule (5970891755) that could not reach the key. 29b and 29c are kept as void with their outputs (rejudge/judge/void-29b.json, void-29c.json), read by nothing; verdicts-29.json is byte-identical to void-29a.json. Batch 05's first instance missed a keyed control and was re-judged (05b); its output is void-05a.json.",
  "The Read tool cuts a line at 2,000 characters, and 88 of the 1,344 judged items here carry a field longer than that; 77-121 of 2,000 per archived record do too. The judges likely never saw the end of those fields. The condition is the same on both sides of the archive comparison; it is a hazard of the instrument, stated, not corrected (5973836546).",
  "Two files are scrubbed before commit (scrub.json): rejudge/check_batch.py names the seat's transcript directory, whose path carries the operator's login, now `<project>/<session>`, so the checker as committed does not run unedited; rejudge/judge/run-log.jsonl quotes three void reasons whose paths carried the operator's home, now `~`. Each original's digest is in scrub.json.",
  "Seven of the Sidekick program's 21 run-record files carried the laptop's home path; they are committed as the program's redacted copies (sidekick/REDACTED.sha256: each redacted digest, its original's MANIFEST digest, the lines changed and the substitution), and recompute.sh chains every file to the manifest (14b034ea...). The originals stay with the Sidekick program. REDACTED.sha256 itself quoted the Sidekick session's scratchpad path in pass-a.warm.txt's substitution column; that one quotation is `<session temp dir>` here, recorded in scrub.json with the file's original digest (9eee37e6...), and no digest the chain checks moved.",
  "analyze.py first ran with numpy (posted on #165, 5974978117); the committed version is stdlib-only so recompute.sh runs anywhere, with each random stream seeded by a string of the pre-registered seed offset. Every reading and every point estimate is unchanged; bootstrap intervals and floors moved by up to 0.0025 (edit's F2 floor at n = 400: 0.0150 to 0.0125; pass B's verdict cross-fit interval: 0.0032-0.0578 to 0.0029-0.0558), corrected on #165 (5975415084). results.json is the stdlib output, byte-identical under Python 3.9 and 3.14.",
  "The 1,024 bucket served 108 of 800 requests, and amendment 6 flags every one `possibly_truncated`; 28 were at the 1,024-token maximum (prompt tokens and fp32's at_max_len agree). The ~440-token 90th percentile that predicted none was the archive's judge states under laya's tokenizer (2026-10-02-judge-state-lengths), not this sample's composed requests.",
  "The pass A daemon's first request to the 1,024 bucket returned 504 during warm-up, before the pass (cold compile past the daemon's 60 s request timeout); the load completed and every bucket then served under 0.5 s (sidekick/pass-a.record.txt). No pass request failed.",
  "The pooled cross-fit ECE is computed over all held-out items together, each scored under the temperature fitted on the other fold; each fold's own ECE is reported beside it (results.json, ece_top1_crossfit_by_fold). On pass A, for verdict the folds read 0.045 and 0.035 and the pool 0.011: the folds' calibration errors fall in different bins and partly cancel when pooled. Amendment 4 reports the pooled figure as the rule's; no reading changes on the fold figures (both sit under verdict's F2 interval's upper end and above its lower, and verdict reads inconclusive (fit at bound) either way; edit's folds, 0.073 and 0.073 on pass A and 0.062 and 0.079 on pass B, are above its interval).",
  "rejudge/judge/run-log.jsonl keeps two superseded rows voiding batches 03 and 07, written by a checker that compared a control's verdict ruled None; the fixed checker applies the archive's rule (a field ruled None is not keyed) and both batches are accepted on their first instance (#165, 5970840575). The superseded rows carry a note.",
  "The judge is hosted (claude-sonnet-5); its verdicts are committed and the record recomputes from them, as the Sonnet records before it. Re-firing the judges is a fresh draw of the same version, not a replay.",
]
targets_checked = 134
targets_matched = 134
claim_issue = "165"
rule_ratified = { comment = "5966450883", at = "2026-10-03T06:42:13Z", digest = "f794b29b458232b358f9bbedaede962a488a4e60acd1477c3f2cd516885d6f03", of = "pre-registration.md" }
rule_ratified_note = "5966450883 ratifies the pre-registration 5962826904 with amendments 5963413444, 5963559758 and 5963641998 as amended by 5966195560; 5970476845 ratifies amendment 5 (5970425814, the Sonnet 5 judge); 5973836546 ratifies amendment 6 (5971096031, the reporting additions) and batch 29's 29a. pre-registration.md holds those texts verbatim with the re-pin 5966284931."
window_start = "2026-10-03T15:53:21Z"
window_start_from = "the first entry of batch 01's judge transcript (instance ad573a7753117a400), the run's first measured act; the laya passes ran 2026-10-04T00:15:14Z-00:16:43Z (A) and 00:18:24Z-00:19:29Z (B), the first and last response times in laya/passA and laya/passB"
absent = { supersedes = "nothing replaced: the first measurement of #165's bound and of laya against the Sonnet majority" }

[regime]
arm = "laya-calibration-bound"
substrates = ["apple32-sidekick-laya-typed-ane", "apple32-sidekick-laya-typed-gpu", "apple32-torch-laya-typed-fp32"]
dogma_version = 0
+++

# #165: the judges' own noise, and laya-typed-decisions against it

## Observation

#165 asks whether a typed decision model can stand in for the Sonnet judge. Its rule needs two things the archive
did not hold: how often Sonnet judges disagree with each other on representative items, which sets the bound a
calibration error is held to, and laya-typed-decisions' own probabilities on the same items. The archive's repeated
judgments are the seeded controls and its easiest recurring items, so neither could set the bound (#165, the ECE
question).

## Hypothesis

Planning's text (5975253990): laya-typed-decisions as shipped, with this program's own temperature, agrees with
the fresh Sonnet majority at or above the judges' own agreement, and is calibrated within the labels' noise, on
both questions, the verdict (accepts, questions, declines, ignores) and the edit (yes, no).

This is the **zero-shot baseline** of #165's hypothesis, not that hypothesis: #165 names a model fine-tuned on
this program's judged rows, which does not exist yet. This record measures the checkpoint as shipped, the floor
the fine-tune must clear, and the bound it will be held to.

## Test

Under pre-registration 5962826904 and amendments 1-6 (`pre-registration.md`):

- **The re-judge.** 400 items drawn from the 4,712 real items the three Sonnet records judged once, stratified by
  record × original verdict (`rejudge/draw.py`, seed from its phrase; `plan.json`, `sample.json`, `layout.json`
  rebuild from the archive). Three passes of 12 batches, each batch 33 or 34 sample items plus four rotating seeded
  controls, fresh opaque ids, the key withheld; one fresh `claude-sonnet-5` instance per batch under prompt v2 and
  launch Form 3, the model read off each transcript (`rejudge/judge/run-log.jsonl`). d is one minus the mean share
  of agreeing judge pairs, with a 9,999-resample item bootstrap; e₃ = 3d² − 2d³ carries its interval.
- **The laya passes.** 800 `/v1/classify` requests, 400 items × two questions, rendered by the translation table
  (`laya/build_requests.py`), with no `calibration` field, so T = 1. Served by sidekickd 0.7.0 on the laptop's
  M1 Max in two daemon runs, `cpu_and_ne` (pass A, the word) and `cpu_and_gpu` (pass B, beside); laya's fp32
  PyTorch forward on the laptop's CPU is the reference (`laya/fp32.jsonl`). The Sidekick program built, started,
  warmed and stopped each daemon and graded the served artifact (`sidekick/`, report `7cb25496...`: ANE C, GPU A,
  0 graded flips).
- **The fit and the reading.** A 2-fold cross-fit, stratified by the twelve strata: one temperature per question in
  [0.05, 20] by golden-section NLL minimisation on one fold, applied to the other. Top-1 ECE with 15 equal-width
  bins on the held-out items with a majority. The bound is e₃ plus the median ECE of a perfectly calibrated predictor
  at the held-out n; below its interval reads `calibrated`, above reads `miscalibrated`, inside `inconclusive`.

`analyze.py` derives everything in `results.json`; `recompute.sh` re-derives it byte for byte.

## Results

**The judges** (three fresh judgments per item, 400 items).

| question | d (95% CI) | e₃ (CI) | Fleiss κ | no majority | archive vs fresh majority |
| --- | --- | --- | --- | --- | --- |
| verdict | 0.0792 (0.0583–0.1017) | 0.0178 (0.0098–0.0289) | 0.886 | 3 | 0.940 |
| edit | 0.0100 (0.0033–0.0183) | 0.0003 (0.0000–0.0010) | 0.969 | 0 | 0.993 |

**laya against the fresh majority** (pass A; pass B within one decision everywhere).

| question | n | agreement | the judges' own (1 − d) | majority-class rate | answered agreement / coverage | T (fit on A / on B / all) |
| --- | --- | --- | --- | --- | --- | --- |
| verdict | 397 | 0.267 | 0.921 | 0.443 | 0.268 / 0.970 | 20.0 / 20.0 / 20.0 |
| edit | 400 | 0.635 | 0.990 | 0.795 | 0.649 / 0.955 | 1.65 / 2.53 / 2.00 |

laya answers `questions` on 298 of 397 verdict items and `ignores` on 4, where the majority answers `ignores` on
176; on edit it answers `yes` on 220 where the majority says `yes` on 82.

**The calibration reading** (pass A's cross-fitted top-1 ECE against e₃ + floor at the held-out n; F2 the floor of
record, F1 beside).

| question | ECE at T = 1 | cross-fit ECE | floor F2 (F1) | bound interval F2 (F1) | interval reading | recorded |
| --- | --- | --- | --- | --- | --- | --- |
| verdict | 0.234 | 0.011 | 0.018 (0.020) | 0.027–0.047 (0.030–0.049) | calibrated | inconclusive (fit at bound) |
| edit | 0.070 | 0.062 | 0.0125 (0.0424) | 0.0125–0.0135 (0.0424–0.0434) | miscalibrated | miscalibrated |

**Beside the rule** (pass A where a path applies). Classwise ECE (amendment 2): verdict 0.191 at T = 1 and 0.103 cross-fitted, edit 0.346 and
0.328. Per-record d: verdict 0.076, 0.084 and 0.077 for (b)-v2, 2026-09-27 and 2026-09-29; edit 0.005, 0.020 and 0.005.
The F2 floor at n = 100, 200, 400, 800 and 1,600: verdict 0.033, 0.023, 0.017, 0.012 and 0.008; edit 0.025, 0.020,
0.0125, 0.010 and 0.007. The cross-fit ECE per fold, pass A: verdict 0.045 and 0.035, edit 0.073 and 0.073 (pass B: 0.045 and 0.035, 0.062 and 0.079).

**Path parity.** Over all 400 per question, A and B agree on 399 decisions, A and fp32 on 399, B and fp32 on 400;
the two ANE changes sit at fp32 margins of 0.0117 and 0.0051 logits, under the 0.05 floor. The abstention band of
0.05 logits takes 12 verdict and 18 edit items.

## Conclusion

**Refuted, on agreement alone.** laya-typed-decisions as shipped agrees with the fresh Sonnet majority on 26.7% of
verdicts and 63.5% of edits, against the judges' own 92.1% and 99.0%, and below what always answering the majority
class would score (44.3%, 79.5%). Calibration fails too, on edit (`miscalibrated` under both floors), and verdict's
reading is `inconclusive (fit at bound)`. The served paths are faithful to the fp32 forward (399 or 400 of 400
decisions per question), so it is the model, not the serving. This is what the checkpoint's own card predicts:
near-chance zero-shot (26.7% four-way, where chance is 25%), the option prior it documents (`questions` on 298 of
397 items, `ignores` on 4 where the majority says `ignores` on 176), and probabilities that must be refit. It is the
baseline #165's fine-tune must clear; #165 stays open.

**Two findings for the program, larger than the word.** First, the Sonnet judge is reliable: a single judge
disagrees with another on 7.9% of verdicts (κ 0.886) and 1.0% of edits (κ 0.969). Second, the archive's single-judge
labels replicate: 94.0% and 99.3% agreement with a fresh majority across time, prompt form and instance. Every
framing word in `results/` that rests on those labels, (b)-v2, (b′) both stages and #142, rests on labels now shown
to hold.
