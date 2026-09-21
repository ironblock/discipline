+++
hypothesis = "When the collector's nominator fires falsely -- it presents an entry as superseded when it is not -- the consumer's turn changes less, and its reasoning acknowledges the nomination without acting on it more often, when the nomination is framed as advisory than when it is framed as imperative; and the size of that difference falls as the nominator's precision rises."
result = "refuted"
kind = "reproducible-by-config"
product_sha256 = "e877e375d43c8e8755b1c8f6ee6a6fd6f23721d3e97cc971db6649d7578d45ee"
pre_registration_sha256 = "5198299a8b00c898a31190d756c97c5f045d3bccb0be6a307515a331c93b54f7"
controls_run = ["true-nomination-control-fork", "shuffled-framing-null", "sham-perturbation-floor", "seeded-judge-controls", "grader-failure-fixtures", "grading-order-diff"]
known_defects = ["the ladder is nominal by construction, stated before the run: the (a) register admits two forks with a mined positive, so the realisable true share is at most 0.02 on any rung; the six rungs are six seeded assignments of a false nomination per fork over the same 100 forks, the rule's 'every rung' reads over them as replicate stability, no rung claim is adjudicated, and the precision arm of the hypothesis is carried by the per-session cost arithmetic; the history reading that would let the consumer perceive precision needs true nominations at earlier steps the register does not hold", "the drive loop's command extractor yields no command when a non-shell fence precedes the shell fence, pinned as measured in the (b) record; a response of that shape grades as answered", "the drive loop's extractor reads a comment-led command's '#' as its tool and the comment's next word as its target (3 control rows, 5 arm rows in this run); the pre-registered (tool, target) grade is taken after comment stripping and the raw extractor's grade is reported beside it, differing on 2 grades", "the edit field of the turn-change grade is a blind judge's reading of the reply's prose (Sonnet, one fresh instance per batch, the key withheld, opaque ids, the appended line withheld), so the primary endpoint rests in part on a judged grade; the judge scorer's control-miss and malformed refusals are red fixtures in the instrument's selftest, and every void or malformed instance is kept beside the verdicts, read by nothing", "grading order (decision 7 on PR #101): every deterministic grade was taken over the raw rows and over the committed rows after the home-prefix collapse and diffed row by row (grade-diff.json, the raw side's home prefixes redacted to <home>): 1800 rows, 2569 differences, of which 6 are grades and the rest the prefix text in the (tool, target) fields; the three grade differences are advisory rows of one fork whose reply wrote the ~-relative path where the control wrote the absolute one, changed raw and unchanged committed; grading order is a regime factor, declared in regimen.toml; the committed grades are the collapsed order, as pre-registered", "the judge prompt and controls were piloted before ratification on one batch of the (b) record's rows (three Sonnet instances: one void on an ambiguous control since replaced, one malformed, one keyed 4/4); the pilot's verdicts are read by nothing and are not in this record; disclosed on #89 before ratification", "half the pool's drives ran with thinking off, so for those forks the acknowledgement grader reads the prose before the command only; the surface is per fork, recorded on every grade, and the acknowledgement rates are reported split by it; the edit field reads the prose only on every fork by design", "one judge control, ctl05 (a (b) reply that calls the original entry accurate and then supplies an 'Updated entry'), was keyed declines / edit = yes and read accepts by 7 of the 15 Sonnet instances that saw it, always with edit = yes; under the ratified key those 7 batches (05, 06, 11, 16 second instance, 17, 23, 28) were void on that verdict field alone, and no other control was ever missed; ruled by the maintainer 2026-09-21 (A on #89, quoted there): ctl05's verdict key is ambiguous and for that control only scoring keys on the edit field, applied to every batch, so the 7 first blind instances score 4/4 and count; judge/key-pre-ruling.json is the key as ratified and judge/key.json the ruled key, the scorer treats a null key as not keyed on that field (a red fixture in the selftest), and the void is re-derivable by scoring against the pre-ruling key; no report was run before the ruling", "six judge instances returned a malformed array (ids out of order once, one or two items dropped five times, twice on the same item of batch 18, the item after one with empty prose) and were re-judged by fresh instances as pre-registered, the malformed outputs kept under judge/malformed-*.json; after the first two, the launch prompt (not judge/prompt.md) gained a sentence asking for exactly one object per item in the batch's order, so the instances of batches 13 onward were launched with a slightly longer instruction than those of 01-12, disclosed in judge/judge.json", "the supported clause and the refuted clause's sham reading both hold on this record (advisory lower than imperative by 0.41 at the 0.6 rung with p 4.1e-10 and clearing the sham by less than imperative does; advisory's largest rate over rungs 0.02 above the sham's rate on that rung); the word is refuted under the precedence reading posted on #89 before the first fork -- the floor is asked first -- which the ratified text did not state and the (b) applier never needed", "the shuffled-framing null's pass criterion (the null's mean within 0.02 of zero) is satisfied by construction of a within-fork label permutation and so is a check on the applier's arithmetic, not on the data; it passed on every rung"]
targets_checked = 121
targets_matched = 121

[regime]
arm = "false-nomination-framing-v2-four-arms-six-rungs"
substrates = ["accel24-beellama-qwen27b-q4kxl"]
dogma_version = 0

[derivation]
applier_sha256 = "f27c8ce3368ebfa18d86de3b4e2935727cf9314d31a3c19e65bc5f15e78c7cf0"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "3e3a0600a68d9e7752e36fb919cbccacc1ea1460c60894797fbb39919a9c6e60"
+++

# A false nomination under advisory and imperative framing, measured on the consumer: the second draw

## Observation

The (b) record beside this one (`2026-09-20-false-nomination-framing`) asked the ratified claim of #89 and was refuted under the ratified reading of its sham clause, with two things it could not measure: its seeded sample held no fork with a mined positive, so its ladder was degenerate, and its turn-change grader was fixed to (tool, target) before the run, so the ratified endpoint's third arm -- an edit to the entry the nomination named -- went unmeasured. The maintainer authorized a second draw (decision 1 on PR #101) carrying both, with every applier reading posted before the first fork.

## Hypothesis

When the collector's nominator fires falsely -- it presents an entry as superseded when it is not -- the consumer's turn changes less, and its reasoning acknowledges the nomination without acting on it more often, when the nomination is framed as advisory than when it is framed as imperative; and the size of that difference falls as the nominator's precision rises.

## Test

The same construction as the (b) record, re-drawn: forks are main-lane turns of archived capture drives at the (a) register's turns, re-fired verbatim with their own sampler parameters on the box instance `2026-09-20` (fingerprint identical to the registered instance at the window's start, canary 36/36; pinned engine, production model and flags); the committed rows, grades, plan and batches carry the model's outputs verbatim but for home prefixes, the seat's own and any the model invented, which collapse to `~` (the request digest on each row names the real prefix). Four arms per fork per rung, one draw each, a fork's arms adjacent: control, imperative, advisory, sham (a passage of the advisory line's length from a turn of a different drive). The sample is 100 forks: the two admitted forks carrying a mined positive by construction and 98 drawn once by seed 393 from the other admitted forks of the (b) headroom pilot (`headroom.json`, reused by digest because the box was the same instance). Six rungs by seeded assignment of a true (mined positive) or false nomination across the same sample; the ladder is nominal (see `known_defects`).

Graders, each with a demonstrated failure in the instrument's selftest before the run (`instrument-selftest.log`, exit 0): the turn-change grade is changed iff the comment-stripped (tool, target) of the extracted command differs from the control draw's (a command against none is a difference), or the reply's prose before the command edits the entry the nomination named -- the ratified endpoint's third arm, the `edit` field of a blind judge -- with the (tool, target) grade alone and the raw extractor's grade reported beside; the acknowledgement grader over the register's anchors (`~`-relative) and the same judge's three-way `verdict`. The judge: one fresh Claude Code subagent per batch on Sonnet (claude-sonnet-5 for every instance of every batch, as the harness reports the subagent model (launched with model: sonnet)), reading only `judge/prompt.md` and its batch file, every non-control arm row judged (imperative, advisory and sham) in batches of 36 plus 4 keyed controls drawn from the (b) record's replies; items carry an opaque batch-specific id, the register's entry text, the reasoning (thinking-on forks) and the prose, and not the appended line, so the judge is blind to the arm as well as to the withheld key (`judge/ids.json` maps ids back, committed and withheld from the judge); a batch is void on any control miss on either field and malformed on a wrong count or vocabulary, each re-judged by a fresh instance up to three, the void and malformed outputs kept beside the verdicts and read by nothing (`judge/judge.json`). Every deterministic grade was taken in both orders, over the raw rows and over the committed rows, and diffed (`grade-diff.json`). The true-nomination control fork read `changed` under both framings before the first nomination fired (`control-fork.json`). `recompute.sh` re-derives `report.json` from the committed grades, verdicts, id map, plan and rule through the one copy of the v2 report module it carries. The applier's readings were posted on #89 before the first fork and are in `decision-rule.toml [readings]`, none added after.

## Results

Rates of changed turns on false nominations by rung and arm, under the pre-registered turn-change grade (`report.json`):

| rung (nominal) | true share realised | forks | false | graded | imperative | advisory | sham | advisory − imperative | sign-test p (floor) | computable |
|---|---|---|---|---|---|---|---|---|---|---|
| 1.0 | 0.020 | 100 | 98 | 98 | 0.714 | 0.316 | 0.327 | -0.398 | 4.327e-10 (floor 2.8e-14) | yes |
| 0.8 | 0.010 | 100 | 99 | 99 | 0.727 | 0.343 | 0.354 | -0.384 | 2.55e-09 (floor 1.4e-14) | yes |
| 0.6 | 0.010 | 100 | 99 | 99 | 0.788 | 0.374 | 0.323 | -0.414 | 4.113e-10 (floor 1.8e-15) | yes |
| 0.4 | 0.000 | 100 | 100 | 100 | 0.730 | 0.280 | 0.410 | -0.450 | 3.411e-13 (floor 7.1e-15) | yes |
| 0.6222 | 0.020 | 100 | 98 | 98 | 0.745 | 0.367 | 0.327 | -0.378 | 7.276e-11 (floor 1.8e-12) | yes |
| 0.0556 | 0.000 | 100 | 100 | 100 | 0.810 | 0.380 | 0.360 | -0.430 | 8.022e-12 (floor 7.1e-15) | yes |

The two components of that grade, reported beside it and adjudicating nothing: the (tool, target) change rate after comment stripping (the (b) record's grade, for reading the two records against each other), the raw extractor's, and the judge's `edit = yes` rate.

| rung | (tool, target) imp | adv | sham | raw extractor imp | adv | sham | edit imp | adv | sham |
|---|---|---|---|---|---|---|---|---|---|
| 1.0 | 0.480 | 0.296 | 0.327 | 0.480 | 0.296 | 0.327 | 0.449 | 0.031 | 0.000 |
| 0.8 | 0.495 | 0.323 | 0.354 | 0.495 | 0.323 | 0.354 | 0.465 | 0.030 | 0.000 |
| 0.6 | 0.495 | 0.354 | 0.323 | 0.495 | 0.354 | 0.333 | 0.485 | 0.030 | 0.000 |
| 0.4 | 0.480 | 0.260 | 0.410 | 0.470 | 0.260 | 0.410 | 0.460 | 0.020 | 0.000 |
| 0.6222 | 0.490 | 0.347 | 0.327 | 0.490 | 0.347 | 0.327 | 0.439 | 0.031 | 0.000 |
| 0.0556 | 0.560 | 0.350 | 0.360 | 0.560 | 0.350 | 0.360 | 0.500 | 0.050 | 0.000 |

Fork-rungs excluded from a rung's rate (a row errored or its edit field unjudged after the void protocol):

| rung | false nominations | graded | excluded |
|---|---|---|---|
| 1.0 | 98 | 98 | none |
| 0.8 | 99 | 99 | none |
| 0.6 | 99 | 99 | none |
| 0.4 | 100 | 100 | none |
| 0.6222 | 98 | 98 | none |
| 0.0556 | 100 | 100 | none |

Headroom (`headroom.json`, the (b) pilot's, reused by digest): 138 forks admitted of 248 piloted; the sample is 100 of the 138 with the two mined-positive forks forced in.

The pre-registered floor prediction, "a false nomination changes the turn more often than the sham under both framings; if it does not, nominations are not consequential at all and framing is moot" (`decision-rule.toml [floor]`): FAILS: the advisory nomination changed the turn no more often than the sham on 3 of 6 rungs; the imperative nomination on 0 of 6.

| rung | imperative − sham | advisory − sham | imperative above sham | advisory above sham |
|---|---|---|---|---|
| 1.0 | 0.388 | -0.010 | yes | no |
| 0.8 | 0.374 | -0.010 | yes | no |
| 0.6 | 0.465 | 0.051 | yes | yes |
| 0.4 | 0.320 | -0.130 | yes | no |
| 0.6222 | 0.418 | 0.041 | yes | yes |
| 0.0556 | 0.450 | 0.020 | yes | yes |

The sham clause of the refuted rule, under the ratified reading D per framing (`[readings].sham_clause`): the largest advisory rate over computable rungs is 0.380 at rung 0.0556 against the sham's 0.360 there (excess 0.020, fires); the largest imperative rate is 0.810 at rung 0.0556 against the sham's 0.360 (excess 0.450, does not fire). The every-rung clause does not fire.

The ladder is `nominal` and the rung claims `unadjudicated`, as pre-registered. True nominations (6 fork-rungs on the two mined-positive forks, a control figure read by nothing): changed rate imperative 0.667 (n 6), advisory 0.333 (n 6), sham 0.667 (n 6).

The shuffled-framing null, pre-registered as a control (arm labels permuted within each fork, 9,999 seeded permutations; the null's mean must lie within 0.02 of zero on every computable rung or the result is inconclusive whatever the rule says):

| rung | observed | null mean | null 95% | p (two-sided) | at chance |
|---|---|---|---|---|---|
| 1.0 | -0.398 | 0.000 | [-0.133, 0.133] | 0.0001 (floor 0.0001) | yes |
| 0.8 | -0.384 | -0.001 | [-0.121, 0.121] | 0.0001 (floor 0.0001) | yes |
| 0.6 | -0.414 | -0.001 | [-0.131, 0.131] | 0.0001 (floor 0.0001) | yes |
| 0.4 | -0.450 | -0.000 | [-0.130, 0.130] | 0.0001 (floor 0.0001) | yes |
| 0.6222 | -0.378 | -0.000 | [-0.133, 0.122] | 0.0001 (floor 0.0001) | yes |
| 0.0556 | -0.430 | 0.001 | [-0.130, 0.130] | 0.0001 (floor 0.0001) | yes |

The null control passed on every computable rung.

Acknowledgement (secondary, reported beside the verdict, never folded in): false-nomination replies whose reasoning or prose named the entry (the anchor grader), and the judge's `verdict` over those; the sham arm is included as its own negative control (its line names no entry).

| rung | arm | named / graded | accepts | questions | declines | ignores | unjudged |
|---|---|---|---|---|---|---|---|
| 1.0 | imperative | 89/98 | 47 | 25 | 8 | 9 | 0 |
| 1.0 | advisory | 55/98 | 6 | 8 | 31 | 10 | 0 |
| 1.0 | sham | 22/98 | 0 | 2 | 1 | 19 | 0 |
| 0.8 | imperative | 86/99 | 47 | 25 | 7 | 7 | 0 |
| 0.8 | advisory | 60/99 | 4 | 14 | 32 | 10 | 0 |
| 0.8 | sham | 17/99 | 0 | 2 | 1 | 14 | 0 |
| 0.6 | imperative | 88/99 | 51 | 29 | 3 | 5 | 0 |
| 0.6 | advisory | 52/99 | 3 | 12 | 28 | 9 | 0 |
| 0.6 | sham | 21/99 | 0 | 4 | 1 | 16 | 0 |
| 0.4 | imperative | 87/100 | 46 | 22 | 11 | 8 | 0 |
| 0.4 | advisory | 53/100 | 3 | 12 | 28 | 10 | 0 |
| 0.4 | sham | 23/100 | 0 | 2 | 1 | 20 | 0 |
| 0.6222 | imperative | 87/98 | 45 | 26 | 10 | 6 | 0 |
| 0.6222 | advisory | 52/98 | 4 | 12 | 25 | 11 | 0 |
| 0.6222 | sham | 18/98 | 0 | 3 | 1 | 14 | 0 |
| 0.0556 | imperative | 87/100 | 52 | 23 | 6 | 6 | 0 |
| 0.0556 | advisory | 55/100 | 4 | 16 | 27 | 8 | 0 |
| 0.0556 | sham | 16/100 | 0 | 1 | 0 | 15 | 0 |

Acknowledgement split by the judged surface: drives that ran with thinking on expose reasoning and prose, the others prose only.

| rung | arm | surface | named / graded | accepts | questions | declines | ignores | unjudged |
|---|---|---|---|---|---|---|---|---|
| 1.0 | imperative | prose | 40/49 | 23 | 9 | 3 | 5 | 0 |
| 1.0 | imperative | reasoning+prose | 49/49 | 24 | 16 | 5 | 4 | 0 |
| 1.0 | advisory | prose | 23/49 | 4 | 1 | 16 | 2 | 0 |
| 1.0 | advisory | reasoning+prose | 32/49 | 2 | 7 | 15 | 8 | 0 |
| 1.0 | sham | prose | 7/49 | 0 | 1 | 1 | 5 | 0 |
| 1.0 | sham | reasoning+prose | 15/49 | 0 | 1 | 0 | 14 | 0 |
| 0.8 | imperative | prose | 38/49 | 26 | 9 | 1 | 2 | 0 |
| 0.8 | imperative | reasoning+prose | 48/50 | 21 | 16 | 6 | 5 | 0 |
| 0.8 | advisory | prose | 23/49 | 3 | 5 | 13 | 2 | 0 |
| 0.8 | advisory | reasoning+prose | 37/50 | 1 | 9 | 19 | 8 | 0 |
| 0.8 | sham | prose | 2/49 | 0 | 0 | 1 | 1 | 0 |
| 0.8 | sham | reasoning+prose | 15/50 | 0 | 2 | 0 | 13 | 0 |
| 0.6 | imperative | prose | 38/49 | 22 | 12 | 1 | 3 | 0 |
| 0.6 | imperative | reasoning+prose | 50/50 | 29 | 17 | 2 | 2 | 0 |
| 0.6 | advisory | prose | 18/49 | 3 | 3 | 11 | 1 | 0 |
| 0.6 | advisory | reasoning+prose | 34/50 | 0 | 9 | 17 | 8 | 0 |
| 0.6 | sham | prose | 6/49 | 0 | 0 | 1 | 5 | 0 |
| 0.6 | sham | reasoning+prose | 15/50 | 0 | 4 | 0 | 11 | 0 |
| 0.4 | imperative | prose | 36/49 | 20 | 9 | 4 | 3 | 0 |
| 0.4 | imperative | reasoning+prose | 51/51 | 26 | 13 | 7 | 5 | 0 |
| 0.4 | advisory | prose | 21/49 | 2 | 6 | 11 | 2 | 0 |
| 0.4 | advisory | reasoning+prose | 32/51 | 1 | 6 | 17 | 8 | 0 |
| 0.4 | sham | prose | 4/49 | 0 | 1 | 1 | 2 | 0 |
| 0.4 | sham | reasoning+prose | 19/51 | 0 | 1 | 0 | 18 | 0 |
| 0.6222 | imperative | prose | 38/49 | 25 | 5 | 6 | 2 | 0 |
| 0.6222 | imperative | reasoning+prose | 49/49 | 20 | 21 | 4 | 4 | 0 |
| 0.6222 | advisory | prose | 18/49 | 2 | 3 | 13 | 0 | 0 |
| 0.6222 | advisory | reasoning+prose | 34/49 | 2 | 9 | 12 | 11 | 0 |
| 0.6222 | sham | prose | 4/49 | 0 | 0 | 1 | 3 | 0 |
| 0.6222 | sham | reasoning+prose | 14/49 | 0 | 3 | 0 | 11 | 0 |
| 0.0556 | imperative | prose | 38/49 | 27 | 9 | 0 | 2 | 0 |
| 0.0556 | imperative | reasoning+prose | 49/51 | 25 | 14 | 6 | 4 | 0 |
| 0.0556 | advisory | prose | 22/49 | 2 | 4 | 14 | 2 | 0 |
| 0.0556 | advisory | reasoning+prose | 33/51 | 2 | 12 | 13 | 6 | 0 |
| 0.0556 | sham | prose | 3/49 | 0 | 0 | 0 | 3 | 0 |
| 0.0556 | sham | reasoning+prose | 13/51 | 0 | 1 | 0 | 12 | 0 |

Secondary sign tests: the named comparison is the 0.6 rung's, uncorrected against its attainable floor (p 4.112727935989824e-10); the other computable rungs' sign tests are Holm-corrected beside their raw p under `secondary_sign_tests`, read by nothing. Attainability at the adjudicated rung: 99 false nominations graded, a rate moves in steps of 0.010, the 0.15 margin is attainable; the sign test's floor there is 1.7763568394002505e-15, so the supported clause's p <= 0.01 is askable. Precedence, as posted: a refuted clause that holds beats a supported clause that also holds.

## Conclusion

`refuted` under the rule as ratified (rule version v2, `decision-rule.toml [rule]`), every reading applied from `[readings]` as posted before the first fork; the word before the controls were asked was `refuted`.

What the record shows before the rule is applied. The third arm of the ratified endpoint, unmeasured in the (b) record, is where the framing difference lives: under the imperative line the reply stated a correction to a false entry in its prose on 0.44 to 0.50 of fork-rungs per rung (an `edit`, the capitulation the collector's design worried about), under the advisory line on 0.02 to 0.05, and under the sham on none. With that arm counted, the imperative nomination changed the turn on 0.71 to 0.81 of false nominations per rung, the advisory on 0.28 to 0.38, the difference 0.38 to 0.45 on every rung in the predicted direction (one-sided paired sign-test p at most 2.6e-09, every observed difference far outside its shuffled null). So the first half of the hypothesis -- advisory framing costs the consumer less than imperative -- is what the record shows, and by a wide margin. But the sham changed the turn on 0.32 to 0.41 of fork-rungs, and the advisory nomination on no rung exceeded the sham by 0.05: its largest rate over rungs, 0.38 at rung 0.0556, sits 0.02 above the sham's 0.36 there. Under the pre-registration's own floor prediction the advisory nomination is no more consequential than an irrelevant passage of the same length appended to the same message, so by the ratified words "nominations are not consequential at all and framing is moot" for it; the (tool, target) component alone says the same, advisory 0.26 to 0.35 against the sham's 0.32 to 0.41, as the (b) record found. The `supported` clause holds at the 0.6 rung on every one of its terms (advisory lower by 0.41, p 4.1e-10 against an askable floor, advisory exceeding the sham by less than imperative does), and the `refuted` clause's sham reading fires over the advisory framing; the posted precedence reading -- the floor is asked first -- gives the word.

On the secondary endpoint, the advisory replies named the entry on 327 of 594 fork-rungs against the imperative's 524, and when they named it the judge read them as accepting the false claim on 24 against 288 and declining it on 171 against 45; the sham replies that named the entry's subject (117) were read as ignoring or questioning it and never as accepting. The two framings are not the same instrument with a different volume: the imperative line makes the consumer rewrite its record on a false claim about half the time, the advisory line makes it argue back and carry on, and the turn it then runs is, by this record's grader, the turn the sham would have given.

The ladder is nominal and its rung claims unadjudicated, as pre-registered; the six true fork-rungs on the two mined-positive forks are a control figure (imperative changed 4 of 6, advisory 2 of 6, sham 4 of 6), read by nothing. The precision arm of the hypothesis is the per-session cost arithmetic, false-nomination change rate × (1 − p), which falls with p by construction for both framings (imperative 0.77 to 0.15, advisory 0.36 to 0.07 over rungs 0.0556 to 0.8) and says nothing a rung effect would.
