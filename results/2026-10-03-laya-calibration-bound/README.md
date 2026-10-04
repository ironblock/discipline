+++
hypothesis = "laya-typed-decisions as shipped, with this program's own temperature, agrees with the fresh Sonnet majority at or above the judges' own agreement, and is calibrated within the labels' noise, on both questions."
result = "refuted"
kind = "reproducible-by-config"
figures = "referenced"
product_sha256 = "74da539a97dcc0b97bfdd5d2e6f75735e6fc406d236af672d7cb4adb77b4c604"
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
  "Migrated to referenced figures (#376): every figure in the body is a reference into results.json, rendered by scripts/check-results.py. A reference path cannot step into a key that opens with a digit or carries a date, so analyze.py now also writes a `report` block holding the per-record d under letter keys, the F2 floor at each held-out size under `n<size>` keys, the judges' own agreement (1 - d) and the pre-registered constants. Every value results.json held before is unchanged, which a comparison with the block removed confirms. The product's sha256 moved from e79ed1a6... to 74da539a..., and the record's digests moved with it. Some figures now print at the precision of their reference: the temperatures at two places, and the floors and bound intervals at four, in the calibration table and in the F2 floor-by-size sentence (which the typed text gave at three). The linter's round() and pct() round half-even, where the typed text rounded half-up, so two values on an exact half now render one unit lower. The archive's edit agreement, 0.9925, renders as 0.992 and 99.2% (typed 0.993 and 99.3%, as planning's ruling 5975253990 quotes it). Verdict's fold-B cross-fit ECE, 0.0345 on both paths, renders as 0.034 (typed 0.035). No value in results.json moved.",
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

Planning's text, ruled on #165: laya-typed-decisions as shipped, with this program's own temperature, agrees with
the fresh Sonnet majority at or above the judges' own agreement, and is calibrated within the labels' noise, on
both questions, the verdict (accepts, questions, declines, ignores) and the edit (yes, no).

This is the **zero-shot baseline** of #165's hypothesis, not that hypothesis: #165 names a model fine-tuned on
this program's judged rows, which does not exist yet. This record measures the checkpoint as shipped, the floor
the fine-tune must clear, and the bound it will be held to.

## Test

Under the pre-registration and its six amendments (`pre-registration.md`):

- **The re-judge.** {{product.judges.verdict.n_items}} items drawn from the real items the three Sonnet records judged once
  ([uncited: plan.json's population, 4,712 items]), stratified by record × original verdict (`rejudge/draw.py`, seed
  from its phrase; `plan.json`, `sample.json`, `layout.json` rebuild from the archive). Three passes of twelve
  batches, each batch thirty-three or thirty-four sample items plus four rotating seeded controls, fresh opaque ids,
  the key withheld; one fresh Sonnet instance per batch ([uncited: model claude-sonnet-5]) under prompt v2 and the
  archive's launch form ([uncited: Form 3]), the model read off each transcript (`rejudge/judge/run-log.jsonl`). d is one minus the mean share of agreeing judge pairs, with a
  {{product.report.design.boot}}-resample item bootstrap giving each interval
  ([uncited: 95% intervals, the 2.5th and 97.5th percentiles]); e₃ = 3d² − 2d³ carries d's interval.
- **The laya passes.** Two `/v1/classify` requests per item, one per question, rendered by the translation table
  (`laya/build_requests.py`), with no `calibration` field, so the probabilities are the raw softmax. Served by
  sidekickd v0.7.0 on the laptop's M1 Max in two daemon runs, `cpu_and_ne` (pass A, the word) and `cpu_and_gpu`
  (pass B, beside); laya's fp32 PyTorch forward on the laptop's CPU is the reference (`laya/fp32.jsonl`). The
  Sidekick program built, started, warmed and stopped each daemon and graded the served artifact (`sidekick/`,
  report `7cb25496...`: ANE C, GPU A, no graded flips).
- **The fit and the reading.** A two-fold cross-fit, stratified by the twelve strata: one temperature per question
  in [{{product.report.design.T_low}}, {{product.report.design.T_high}}] by golden-section NLL minimisation on one fold, applied to the other. Top-one ECE ([uncited: top-1, the chosen label's confidence]) with
  {{product.report.design.bins}} equal-width bins on the held-out items with a majority. The bound is e₃ plus the median ECE of a
  perfectly calibrated predictor at the held-out n ({{product.report.design.floor_draws}} draws); below its interval reads `calibrated`,
  above reads `miscalibrated`, inside `inconclusive`.

`analyze.py` derives everything in `results.json`; `recompute.sh` re-derives it byte for byte. Every figure below is
a reference into `results.json`, rendered by `scripts/check-results.py`.

## Results

**The judges** (three fresh judgments per item, {{product.judges.verdict.n_items}} items).

| question | d (interval) | e₃ (interval) | Fleiss κ | no majority | archive vs fresh majority |
| --- | --- | --- | --- | --- | --- |
| verdict | {{round(product.judges.verdict.d, 4)}} ({{round(product.judges.verdict.d_ci95[0], 4)}}–{{round(product.judges.verdict.d_ci95[1], 4)}}) | {{round(product.judges.verdict.e3, 4)}} ({{round(product.judges.verdict.e3_ci95[0], 4)}}–{{round(product.judges.verdict.e3_ci95[1], 4)}}) | {{round(product.judges.verdict.fleiss_kappa, 3)}} | {{product.judges.verdict.no_majority}} | {{round(product.judges.verdict.archive_vs_fresh_majority.rate, 3)}} |
| edit | {{round(product.judges.edit.d, 4)}} ({{round(product.judges.edit.d_ci95[0], 4)}}–{{round(product.judges.edit.d_ci95[1], 4)}}) | {{round(product.judges.edit.e3, 4)}} ({{round(product.judges.edit.e3_ci95[0], 4)}}–{{round(product.judges.edit.e3_ci95[1], 4)}}) | {{round(product.judges.edit.fleiss_kappa, 3)}} | {{product.judges.edit.no_majority}} | {{round(product.judges.edit.archive_vs_fresh_majority.rate, 3)}} |

**laya against the fresh majority** (pass A; pass B within one decision everywhere).

| question | n | agreement | the judges' own agreement | majority-class rate | answered agreement / coverage | T (fit on A / on B / all) |
| --- | --- | --- | --- | --- | --- | --- |
| verdict | {{product.laya.verdict.n_with_majority}} | {{round(product.laya.verdict.passes.A.agreement_with_fresh_majority.rate, 3)}} | {{round(product.report.judges.verdict.agreement, 3)}} | {{round(product.laya.verdict.majority_class_rate, 3)}} | {{round(product.laya.verdict.passes.A.agreement_with_fresh_majority.answered_rate, 3)}} / {{round(product.laya.verdict.passes.A.agreement_with_fresh_majority.coverage, 3)}} | {{round(product.laya.verdict.passes.A.T_fit_on_fold.A, 2)}} / {{round(product.laya.verdict.passes.A.T_fit_on_fold.B, 2)}} / {{round(product.laya.verdict.passes.A.T_deployed_all, 2)}} |
| edit | {{product.laya.edit.n_with_majority}} | {{round(product.laya.edit.passes.A.agreement_with_fresh_majority.rate, 3)}} | {{round(product.report.judges.edit.agreement, 3)}} | {{round(product.laya.edit.majority_class_rate, 3)}} | {{round(product.laya.edit.passes.A.agreement_with_fresh_majority.answered_rate, 3)}} / {{round(product.laya.edit.passes.A.agreement_with_fresh_majority.coverage, 3)}} | {{round(product.laya.edit.passes.A.T_fit_on_fold.A, 2)}} / {{round(product.laya.edit.passes.A.T_fit_on_fold.B, 2)}} / {{round(product.laya.edit.passes.A.T_deployed_all, 2)}} |

laya answers `questions` on {{product.laya.verdict.passes.A.predicted_counts.questions}} of {{product.laya.verdict.n_with_majority}} verdict items and
`ignores` on {{product.laya.verdict.passes.A.predicted_counts.ignores}}, where the majority answers `ignores` on {{product.judges.verdict.majority_counts.ignores}};
on edit it answers `yes` on {{product.laya.edit.passes.A.predicted_counts.yes}} where the majority says `yes` on {{product.judges.edit.majority_counts.yes}}.

**The calibration reading** (pass A's cross-fitted top-one ECE against e₃ + floor at the held-out n; F2 the floor of
record, F1 beside).

| question | ECE, raw softmax | cross-fit ECE | floor F2 (F1) | bound interval F2 (F1) | interval reading | recorded |
| --- | --- | --- | --- | --- | --- | --- |
| verdict | {{round(product.laya.verdict.passes.A.ece_top1_T1, 3)}} | {{round(product.laya.verdict.passes.A.ece_top1_crossfit, 3)}} | {{round(product.rule.verdict.F2_base_rate.floor, 4)}} ({{round(product.rule.verdict.F1_consistency.floor, 4)}}) | {{round(product.rule.verdict.F2_base_rate.bound_interval[0], 4)}}–{{round(product.rule.verdict.F2_base_rate.bound_interval[1], 4)}} ({{round(product.rule.verdict.F1_consistency.bound_interval[0], 4)}}–{{round(product.rule.verdict.F1_consistency.bound_interval[1], 4)}}) | {{product.rule.verdict.F2_base_rate.reading}} | inconclusive (fit at bound) |
| edit | {{round(product.laya.edit.passes.A.ece_top1_T1, 3)}} | {{round(product.laya.edit.passes.A.ece_top1_crossfit, 3)}} | {{round(product.rule.edit.F2_base_rate.floor, 4)}} ({{round(product.rule.edit.F1_consistency.floor, 4)}}) | {{round(product.rule.edit.F2_base_rate.bound_interval[0], 4)}}–{{round(product.rule.edit.F2_base_rate.bound_interval[1], 4)}} ({{round(product.rule.edit.F1_consistency.bound_interval[0], 4)}}–{{round(product.rule.edit.F1_consistency.bound_interval[1], 4)}}) | {{product.rule.edit.F2_base_rate.reading}} | miscalibrated |

**Beside the rule** (pass A where a path applies). Classwise ECE (amendment two): verdict
{{round(product.laya.verdict.passes.A.ece_classwise_T1, 3)}} on the raw softmax and {{round(product.laya.verdict.passes.A.ece_classwise_crossfit, 3)}} cross-fitted,
edit {{round(product.laya.edit.passes.A.ece_classwise_T1, 3)}} and {{round(product.laya.edit.passes.A.ece_classwise_crossfit, 3)}}. Per-record d, for (b)-v2, stage two
and the second substrate: verdict {{round(product.report.judges.verdict.per_record_d.b_v2, 3)}}, {{round(product.report.judges.verdict.per_record_d.stage_2, 3)}} and
{{round(product.report.judges.verdict.per_record_d.second_substrate, 3)}}; edit {{round(product.report.judges.edit.per_record_d.b_v2, 3)}}, {{round(product.report.judges.edit.per_record_d.stage_2, 3)}} and
{{round(product.report.judges.edit.per_record_d.second_substrate, 3)}}. The F2 floor at the five pre-registered held-out sizes, smallest first: verdict
{{round(product.report.floor_F2_by_n.verdict.n100, 4)}}, {{round(product.report.floor_F2_by_n.verdict.n200, 4)}}, {{round(product.report.floor_F2_by_n.verdict.n400, 4)}}, {{round(product.report.floor_F2_by_n.verdict.n800, 4)}}, {{round(product.report.floor_F2_by_n.verdict.n1600, 4)}}; edit {{round(product.report.floor_F2_by_n.edit.n100, 4)}}, {{round(product.report.floor_F2_by_n.edit.n200, 4)}}, {{round(product.report.floor_F2_by_n.edit.n400, 4)}}, {{round(product.report.floor_F2_by_n.edit.n800, 4)}}, {{round(product.report.floor_F2_by_n.edit.n1600, 4)}}. The cross-fit ECE per fold, pass A: verdict
{{round(product.laya.verdict.passes.A.ece_top1_crossfit_by_fold.A, 3)}} and {{round(product.laya.verdict.passes.A.ece_top1_crossfit_by_fold.B, 3)}}, edit
{{round(product.laya.edit.passes.A.ece_top1_crossfit_by_fold.A, 3)}} and {{round(product.laya.edit.passes.A.ece_top1_crossfit_by_fold.B, 3)}}; pass B: verdict
{{round(product.laya.verdict.passes.B.ece_top1_crossfit_by_fold.A, 3)}} and {{round(product.laya.verdict.passes.B.ece_top1_crossfit_by_fold.B, 3)}}, edit
{{round(product.laya.edit.passes.B.ece_top1_crossfit_by_fold.A, 3)}} and {{round(product.laya.edit.passes.B.ece_top1_crossfit_by_fold.B, 3)}}.

**Path parity.** Over all {{product.judges.verdict.n_items}} items per question (verdict, then edit), A and B agree on
{{product.laya.verdict.parity_400.A_vs_B_agree}} and {{product.laya.edit.parity_400.A_vs_B_agree}} decisions, A and fp32 on
{{product.laya.verdict.parity_400.A_vs_fp32_agree}} and {{product.laya.edit.parity_400.A_vs_fp32_agree}}, B and fp32 on
{{product.laya.verdict.parity_400.B_vs_fp32_agree}} and {{product.laya.edit.parity_400.B_vs_fp32_agree}}; the two ANE changes sit
at fp32 margins of {{product.laya.verdict.parity_400.fp32_margins_of_changed.A[0]}} and
{{product.laya.edit.parity_400.fp32_margins_of_changed.A[0]}} logits, under the {{product.report.design.band_floor}} floor. The abstention band
of {{product.laya.verdict.abstention_band_logits}} logits takes {{product.laya.verdict.abstain_n}} verdict and
{{product.laya.edit.abstain_n}} edit items.

## Conclusion

**Refuted, on agreement alone.** laya-typed-decisions as shipped agrees with the fresh Sonnet majority on
{{pct(product.laya.verdict.passes.A.agreement_with_fresh_majority.rate, 1)}} of verdicts and {{pct(product.laya.edit.passes.A.agreement_with_fresh_majority.rate, 1)}} of edits, against the judges' own
{{pct(product.report.judges.verdict.agreement, 1)}} and {{pct(product.report.judges.edit.agreement, 1)}}, and below what always
answering the majority class would score ({{pct(product.laya.verdict.majority_class_rate, 1)}},
{{pct(product.laya.edit.majority_class_rate, 1)}}). Calibration fails too, on edit (`miscalibrated` under both floors), and
verdict's reading is `inconclusive (fit at bound)`. The served paths are faithful to the fp32 forward (pass A agrees
with it on {{product.laya.verdict.parity_400.A_vs_fp32_agree}} and {{product.laya.edit.parity_400.A_vs_fp32_agree}} of {{product.judges.verdict.n_items}}
decisions, pass B on {{product.laya.verdict.parity_400.B_vs_fp32_agree}} and {{product.laya.edit.parity_400.B_vs_fp32_agree}}), so it is the model, not the serving. This is what the checkpoint's own card predicts: near-chance
zero-shot ({{pct(product.laya.verdict.passes.A.agreement_with_fresh_majority.rate, 1)}} four-way, where chance is {{pct(product.report.design.chance_four_way, 0)}}), the
option prior it documents (`questions` on {{product.laya.verdict.passes.A.predicted_counts.questions}} of {{product.laya.verdict.n_with_majority}} items,
`ignores` on {{product.laya.verdict.passes.A.predicted_counts.ignores}} where the majority says `ignores` on {{product.judges.verdict.majority_counts.ignores}}),
and probabilities that must be refit. It is the baseline {{front.claim_issue}}'s fine-tune must clear;
{{front.claim_issue}} stays open.

**Two findings for the program, larger than the word.** First, the Sonnet judge is reliable: a single judge
disagrees with another on {{pct(product.judges.verdict.d, 1)}} of verdicts (κ {{round(product.judges.verdict.fleiss_kappa, 3)}}) and
{{pct(product.judges.edit.d, 1)}} of edits (κ {{round(product.judges.edit.fleiss_kappa, 3)}}). Second, the archive's single-judge labels replicate:
{{pct(product.judges.verdict.archive_vs_fresh_majority.rate, 1)}} and {{pct(product.judges.edit.archive_vs_fresh_majority.rate, 1)}} agreement
with a fresh majority across time, prompt form and instance. Every framing word in `results/` that rests on those
labels, (b)-v2, (b′) both stages and the second-substrate re-fire, rests on labels now shown to hold.
