+++
hypothesis = "When the collector's nominator fires falsely -- it presents an entry as superseded when it is not -- the consumer's turn changes less, and its reasoning acknowledges the nomination without acting on it more often, when the nomination is framed as advisory than when it is framed as imperative; and the size of that difference falls as the nominator's precision rises."
result = "refuted"
kind = "reproducible-by-config"
product_sha256 = "3d5f136b8b9c280327cb414308fa1760499c02e71423155c5162a4d018618f6f"
pre_registration_sha256 = "0c70a82b339a4ea90ff229d237b65626ef0d9c31edff38977ffc2599872a045c"
controls_run = ["true-nomination-control-fork", "shuffled-framing-null", "sham-perturbation-floor", "seeded-judge-controls", "grader-failure-fixtures"]
known_defects = ["under the population reading of the ladder the consumer sees no history of earlier nominations within a re-fired turn, so a per-nomination rung effect is not expected and the precision arm of the hypothesis is carried by the per-session cost arithmetic; the history reading needs true nominations at earlier steps the register does not hold", "the drive loop's command extractor yields no command when a non-shell fence precedes the shell fence, pinned as measured; a response of that shape grades as answered", "planted positives are true by label and not in the archived observation the fork re-fires, so true nominations come from forks with a mined positive only: two of the 138 admitted forks in headroom.json carry one, and none of the seeded 100-fork sample", "the refuted clause's 'the sham's' was read by the applier as the sham's advantage on the rung where advisory's advantage is largest; no reading of that phrase was posted before the run, the applier's wording is from 2026-09-20T09:12Z with the run near its end, and the verdict pivots on it at one rung: the two other literal readings are computed beside it on the verdict under sham_clause_readings, one of which would make the verdict inconclusive; the maintainer's ruling decides", "the ratified endpoint's third arm, 'an edit to the entry the nomination named', is not measured: the design fixed the turn-change grader to (tool, target) before the run, and a reply that states the correction and repeats the control's command grades unchanged", "grades were first taken over the raw rows and the judge batches drawn from that grading; the committed grades are re-taken over the committed rows after the home-prefix collapse, which names two more advisory rows (their replies wrote the absolute prefix the register's ~-relative anchor could not match before), so those two named rows sit outside every batch and are unjudged", "the drive loop's extractor reads a comment-led command's '#' as its tool and the comment's next word as its target (36 control rows, 16 arm rows); changed_nocomment re-grades with comments stripped and differs on 11 grades, with no rung rate moving by more than 0.01; the pre-registered grade is the extractor's", "the instrument's selftest carries a red fixture for every grader but the judge scorer, whose control-miss refusal is exercised by batch 22 alone", "one judge batch of 25 (batch 22, 36 rows) is void: its fresh instance judged a keyed control accepts where the key says declines; its verdicts are kept under judge/void-22.json, read by nothing, and its 36 rows count as unjudged in the acknowledgement tallies; whether a second fresh instance may re-judge that batch is disclosed on the PR, not decided here", "the supported clause's p-below-the-attainable-floor requirement is met only when every discordant fork goes the same way, since the floor is 0.5 to the discordant count; at the 0.6 rung 26 forks were discordant and 3 went the advisory-changed way, so p = 4.4e-05 against a floor of 1.5e-08 and the clause fails on p alone whatever the margin -- a construction of the ratified rule, applied as written", "the ladder did not realise: the seeded sample holds no fork with a mined positive, so plan.json assigns a false nomination to every fork at every rung and the realised true share is 0.0 on all six rungs, nominal 1.0 included; the six rungs are six seeded draws of a false nomination per fork from the same 100 forks, the claim is adjudicated on those, and the record says nothing about a rung effect; this was readable from plan.json before the run and was not read"]
targets_checked = 62
targets_matched = 62

[regime]
arm = "false-nomination-framing-four-arms-six-rungs"
substrates = ["accel24-beellama-qwen27b-q4kxl"]
dogma_version = 0

[derivation]
applier_sha256 = "eb05013c1b74202420e4dc0a020fa0a4203d71db6d17b517b252ebb0559d1ea4"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "196ca4bd707ab5940ca4570d7b633ba9df9a2e6082a433a632487ad48bc21c66"
+++

# A false nomination under advisory and imperative framing, measured on the consumer

## Observation

The collector's design treats advisory framing and a precision gate as load-bearing because "imperative nominations caused capitulation on false positives", and the research record held no measurement behind that sentence (#89). No arm had ever framed a nomination advisorily against imperatively, and no false nomination had ever been injected.

## Hypothesis

When the collector's nominator fires falsely -- it presents an entry as superseded when it is not -- the consumer's turn changes less, and its reasoning acknowledges the nomination without acting on it more often, when the nomination is framed as advisory than when it is framed as imperative; and the size of that difference falls as the nominator's precision rises.

## Test

Forks are main-lane turns of archived capture drives at the (a) register's turns, re-fired verbatim with their own sampler parameters on the box instance `2026-09-20`; the committed rows, grades, plan and headroom carry the model's outputs verbatim but for home prefixes, the seat's own and any the model invented, which collapse to `~` (the request digest on each row names the real prefix) (pinned engine, production model and flags). Four arms per fork per rung, one draw each: control, imperative, advisory, sham (a passage of the advisory line's length from a turn of a different drive). Six rungs by seeded assignment of true (mined positives only) or false nominations across the same fork sample. Headroom decided before firing by two no-nomination draws. Graders: the turn-change grader over the drive loop's own command extractor (tool = first word, target = first path-like argument, else the second word; unchanged iff equal to the control draw's; the ratified text's third arm, an edit to the entry named, is not measured, see `known_defects`); the acknowledgement grader over the register's anchors (`~`-relative) and a blind judge: one fresh instance per batch of `claude-fable-5-1` as the harness reports it, launched as a Claude Code subagent reading only `judge/prompt.md` and its batch file (committed under `judge/batches/`, home prefixes collapsed), the key withheld (`judge/judge.json`). Grading ran over the committed rows, after the home-prefix collapse, so `grades.jsonl` re-derives from `rows.jsonl`; the batches were drawn from an earlier grading over the raw rows (see `known_defects`). The instrument's selftest ran on the workstation before assembly (`instrument-selftest.log`, exit 0); every grader's demonstrated failure is in it but the judge scorer's; the true-nomination control fork read `changed` under both framings before the run (control-fork.json). `recompute.sh` re-derives `report.json` from the committed grades, verdicts, plan and rule through the one copy of the report module it carries.

## Results

Rates of changed turns on false nominations by rung and arm (`report.json`):

| rung (nominal) | true share realised | forks | false | graded | imperative | advisory | sham | advisory − imperative | sign-test p (floor) | computable |
|---|---|---|---|---|---|---|---|---|---|---|
| 1.0 | 0.000 | 100 | 100 | 100 | 0.520 | 0.390 | 0.420 | -0.130 | 0.009579 (floor 7.5e-09) | yes |
| 0.8 | 0.000 | 100 | 100 | 100 | 0.540 | 0.430 | 0.440 | -0.110 | 0.01734 (floor 1.2e-07) | yes |
| 0.6 | 0.000 | 100 | 100 | 100 | 0.540 | 0.340 | 0.450 | -0.200 | 4.399e-05 (floor 1.5e-08) | yes |
| 0.4 | 0.000 | 100 | 100 | 100 | 0.530 | 0.350 | 0.440 | -0.180 | 0.0004561 (floor 3.7e-09) | yes |
| 0.6222 | 0.000 | 100 | 100 | 100 | 0.540 | 0.360 | 0.460 | -0.180 | 0.0007155 (floor 9.3e-10) | yes |
| 0.0556 | 0.000 | 100 | 100 | 100 | 0.570 | 0.320 | 0.320 | -0.250 | 1.118e-05 (floor 2.9e-11) | yes |

Headroom (`headroom.json`): 138 forks admitted of 248 piloted, 110 excluded because a no-nomination draw emitted no command or the two draws disagreed on (tool, target); the seeded sample is 100 of the 138.

The pre-registered floor prediction, "a false nomination changes the turn more often than the sham under both framings; if it does not, nominations are not consequential at all and framing is moot" (`decision-rule.toml [floor]`): FAILS: the advisory nomination changed the turn no more often than the sham on 6 of 6 rungs, while the imperative nomination changed it more often than the sham on 6 of 6. Under the pre-registration's own reading, an advisory nomination is no more consequential than an irrelevant passage of the same length.

| rung | imperative − sham | advisory − sham | imperative above sham | advisory above sham |
|---|---|---|---|---|
| 1.0 | 0.100 | -0.030 | yes | no |
| 0.8 | 0.100 | -0.010 | yes | no |
| 0.6 | 0.090 | -0.110 | yes | no |
| 0.4 | 0.090 | -0.090 | yes | no |
| 0.6222 | 0.080 | -0.100 | yes | no |
| 0.0556 | 0.250 | 0.000 | yes | no |

The ladder's nominal rungs are the seeded probability of a true nomination per fork; the realised column is what the seed drew. Every rung realised a true share of 0.0: the seeded sample holds no fork with a mined positive, so all six rungs are false nominations only, six seeded draws from the same forks, and no rung effect can be read from this record (see `known_defects`).

True nominations (0 fork-rungs): changed rate imperative undefined, advisory undefined, sham undefined.

The acknowledgement endpoint, reported beside the verdict and never folded in, is per rung and arm under `acknowledgement` in `report.json`: how many false-nomination replies named the entry, and the judge's accepts / questions / declines / ignores over those.

The shuffled-framing null (pre-registered control): arm labels permuted within each fork, 9,999 permutations, the advisory − imperative difference recomputed each time; the null must sit at chance.

| rung | observed | null mean | null 95% | p (two-sided) | at chance |
|---|---|---|---|---|---|
| 1.0 | -0.130 | 0.001 | [-0.110, 0.110] | 0.0192 (floor 0.0001) | yes |
| 0.8 | -0.110 | -0.000 | [-0.100, 0.100] | 0.042 (floor 0.0001) | yes |
| 0.6 | -0.200 | 0.000 | [-0.110, 0.110] | 0.0006 (floor 0.0001) | yes |
| 0.4 | -0.180 | 0.000 | [-0.110, 0.110] | 0.0014 (floor 0.0001) | yes |
| 0.6222 | -0.180 | 0.001 | [-0.100, 0.100] | 0.0007 (floor 0.0001) | yes |
| 0.0556 | -0.250 | 0.001 | [-0.110, 0.110] | 0.0001 (floor 0.0001) | yes |

Acknowledgement (secondary, reported beside the verdict, never folded in): false-nomination replies whose reasoning or prose named the entry, and the blind judge's reading of those. `unjudged` rows sit in a void batch (see `known_defects`).

| rung | arm | named / graded | accepts | questions | declines | ignores | unjudged |
|---|---|---|---|---|---|---|---|
| 1.0 | imperative | 89/100 | 41 | 30 | 9 | 3 | 6 |
| 1.0 | advisory | 59/100 | 4 | 15 | 28 | 9 | 3 |
| 0.8 | imperative | 88/100 | 56 | 27 | 3 | 1 | 1 |
| 0.8 | advisory | 60/100 | 4 | 17 | 25 | 11 | 3 |
| 0.6 | imperative | 89/100 | 56 | 23 | 3 | 3 | 4 |
| 0.6 | advisory | 60/100 | 3 | 18 | 26 | 10 | 3 |
| 0.4 | imperative | 88/100 | 52 | 17 | 12 | 4 | 3 |
| 0.4 | advisory | 53/100 | 3 | 16 | 27 | 6 | 1 |
| 0.6222 | imperative | 93/100 | 48 | 22 | 11 | 7 | 5 |
| 0.6222 | advisory | 59/100 | 5 | 13 | 28 | 9 | 4 |
| 0.0556 | imperative | 83/100 | 47 | 20 | 10 | 2 | 4 |
| 0.0556 | advisory | 57/100 | 6 | 15 | 26 | 9 | 1 |

Acknowledgement split by the judged surface (design correction 5747611166): drives that ran with thinking on expose reasoning and prose, the others prose only.

| rung | arm | surface | named / graded | accepts | questions | declines | ignores | unjudged |
|---|---|---|---|---|---|---|---|---|
| 1.0 | imperative | prose | 42/50 | 23 | 10 | 3 | 3 | 3 |
| 1.0 | imperative | reasoning+prose | 47/50 | 18 | 20 | 6 | 0 | 3 |
| 1.0 | advisory | prose | 24/50 | 4 | 2 | 15 | 1 | 2 |
| 1.0 | advisory | reasoning+prose | 35/50 | 0 | 13 | 13 | 8 | 1 |
| 0.8 | imperative | prose | 39/50 | 25 | 10 | 2 | 1 | 1 |
| 0.8 | imperative | reasoning+prose | 49/50 | 31 | 17 | 1 | 0 | 0 |
| 0.8 | advisory | prose | 23/50 | 3 | 7 | 12 | 0 | 1 |
| 0.8 | advisory | reasoning+prose | 37/50 | 1 | 10 | 13 | 11 | 2 |
| 0.6 | imperative | prose | 40/50 | 31 | 6 | 0 | 3 | 0 |
| 0.6 | imperative | reasoning+prose | 49/50 | 25 | 17 | 3 | 0 | 4 |
| 0.6 | advisory | prose | 24/50 | 3 | 7 | 12 | 2 | 0 |
| 0.6 | advisory | reasoning+prose | 36/50 | 0 | 11 | 14 | 8 | 3 |
| 0.4 | imperative | prose | 39/50 | 23 | 5 | 6 | 2 | 3 |
| 0.4 | imperative | reasoning+prose | 49/50 | 29 | 12 | 6 | 2 | 0 |
| 0.4 | advisory | prose | 19/50 | 3 | 5 | 8 | 2 | 1 |
| 0.4 | advisory | reasoning+prose | 34/50 | 0 | 11 | 19 | 4 | 0 |
| 0.6222 | imperative | prose | 43/50 | 22 | 10 | 5 | 5 | 1 |
| 0.6222 | imperative | reasoning+prose | 50/50 | 26 | 12 | 6 | 2 | 4 |
| 0.6222 | advisory | prose | 24/50 | 3 | 4 | 12 | 3 | 2 |
| 0.6222 | advisory | reasoning+prose | 35/50 | 2 | 9 | 16 | 6 | 2 |
| 0.0556 | imperative | prose | 36/50 | 22 | 7 | 4 | 1 | 2 |
| 0.0556 | imperative | reasoning+prose | 47/50 | 25 | 13 | 6 | 1 | 2 |
| 0.0556 | advisory | prose | 20/50 | 5 | 4 | 10 | 1 | 0 |
| 0.0556 | advisory | reasoning+prose | 37/50 | 1 | 11 | 16 | 8 | 1 |

The refuted clause's sham reading, three literal readings of "the maximum difference over all rungs does not exceed the sham's by >= 0.05" (none posted before the run; see `known_defects`): (A, applied) advisory's largest advantage over imperative (0.250 at rung 0.0556) against the sham's advantage on that rung (0.250): fires = True; (B) against the sham's own largest advantage over all rungs (0.250): fires = True, verdict `refuted`; (C) the largest over rungs of advisory's excess over the sham on the same rung (1.0: 0.030, 0.8: 0.010, 0.6: 0.110, 0.4: 0.090, 0.6222: 0.100, 0.0556: -0.000; largest 0.110): fires = False, verdict `inconclusive`.

Notes on the statistics: the paired sign test is one-sided toward advisory changing less, one per rung, uncorrected, its floor 0.5 to the discordant count; the shuffled-framing null's `at chance` criterion (0 inside the null's 95% interval) is satisfied by any permutation null, so the null's mean and spread are the check that matters (means within 0.002 of 0 on every rung); the extractor's comment-led commands and `changed_nocomment` are under `known_defects`.

Attainability: at the adjudicated rung (0.6), 100 false nominations were graded, so a rate moves in steps of 0.010; the 0.15 margin is attainable.

## Conclusion

`refuted` under the rule as ratified and read here (the readings are on the verdict under `readings`).

What the record shows before the rule is applied: on every computable rung the advisory framing changed fewer false-nomination turns than the imperative, by 0.110 to 0.250 (one-sided paired sign-test p at most 0.01734; every observed difference outside its shuffled-framing null's 95% interval), and the sham perturbation also changed fewer turns than the imperative, by 0.080 to 0.250. So part of what advisory framing buys is what any perturbation of the last message buys, and the rule asks whether the rest clears 0.05 at the rung where advisory's advantage is largest. But the pre-registered floor prediction failed for the advisory framing on every rung: the advisory nomination changed the turn no more often than the sham, so by the pre-registration's own words framing is moot for it, and what the advisory arm measures is the perturbation, not the nomination. On the secondary endpoint, over the 600 fork-rungs per arm (six seeded draws over the same 100 forks, not 600 independent nominations), advisory replies named the entry less often (348 against 530 for imperative) and, when they named it, the judge read them as accepting it far less (25 against 300) and questioning or declining it far more (254 against 187); 38 named rows are unjudged (the void batch and the two rows named only after the collapse).

The verdict word is under reading (A) of the sham clause; reading (B) gives the same word and reading (C) gives `inconclusive`. None was posted before the run. The maintainer's ruling on the reading decides which word stands; the record carries all three. The supported clause does not hold at the adjudicated rung. Refuted-clause readings: every computable rung not lower by the margin: False; advisory's largest advantage (0.250 at rung 0.0556) against the sham's there (0.250): no better than the sham: True.
