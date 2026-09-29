+++
hypothesis = "When the collector's nominator fires falsely -- it presents an entry as superseded when it is not -- the consumer's reply asserts that the entry is superseded, no longer holds, or is wrong, or supplies replacement text for it, less often when the nomination is framed as advisory than when it is framed as imperative; and under the imperative framing it does so more often than when an irrelevant passage of the same length stands in the nomination's place."
result = "supported"
kind = "reproducible-by-config"
product_sha256 = "b1c59e704f3249cba6708f636a543f1ca2b24dea02942c0202e2aaee0735d7ee"
pre_registration_sha256 = "b8b20faba4e5dd149b41fa59a115a5c2764bef5870c8c8bf2cfb0a514b864d50"
controls_run = ["true-nomination-control-fork", "shuffled-framing-null", "sham-perturbation-floor", "seeded-judge-controls", "applier-selftest-and-mutants", "comparison-selftest", "grading-order-diff", "headroom-recheck"]
known_defects = [
  "Headroom was re-checked on this substrate over the 100 sampled forks (two no-nomination draws each, 200 requests); 28 failed it and, as comparison-rule.toml's headroom reading states, are excluded from both endpoints: their 504 grade rows are removed from grades.jsonl and kept in grades-headroom-excluded.jsonl, read by nothing. The run's gate fork, drive-08#t9s24, is one of the 28: it changed the turn under both framings, as the run requires, and its rows are excluded like the others'.",
  "Endpoint 2 has no claim row: comparison-rule.toml's words are not the claim vocabulary. It is carried by a comparison row (#181's row kind), with the rule's own word, the pre-registered prediction and the counts over the 72 shared forks, beside the claim; its word is comparison.json's, re-derived byte for byte by recompute.sh from this record and stage 2's committed files, which also checks the row against it.",
  "Served as declared, the MTP draft on (instance 2026-09-28), under reading (c) of the grading statute as ruled on #142. Speculation on this substrate is not bit-exact with the spec-off path: every measured divergence a near-tie flip to the second-ranked token, 36 of 2,327 positions over five prompts (#164, substrates/measurements/2026-09-29-ada-invariance-ground-truth/). The rule has no token-identity check.",
  "Stage 2's side of the comparison is copied under stage2-record/, each file at the bytes and digest committed in results/2026-09-27-false-nomination-edit-rate/ (grades, plan, ids, ruled key, batches, verdicts), so the comparison recomputes inside this directory.",
  "Batches 29 and 41: each first judge instance returned 40 objects whose ids were the batch's but out of order; kept as judge/malformed-29.json and judge/malformed-41.json, read by nothing, and re-judged by fresh instances (29b, 41b), accepted. All 52 instances ran claude-sonnet-5, read off each transcript; the transcripts stay in the seat's durable run directory.",
  "ctl05's verdict key is ruled ambiguous (#89, 2026-09-21) and scored on the edit field only, as in stage 2: judge/key.json is the generated key with that control's verdict null in the 12 batches it sits in, and judge/key-pre-ruling.json the key as generated; every batch scores 4/4 on both.",
  "Blinding is by item construction only: 105 imperative and 4 advisory judge items carry a nomination line's fixed wording in the shown prose or reasoning (word.json's echo_items), against 35 and 0 in stage 2.",
  "The edit field reads the prose before the first three-backtick sequence, capped at 3,000 characters, as in stage 2.",
  "Grading order is a regime factor: 2,898 differences between the raw and collapsed gradings over 1,800 rows (grade-diff.json), against stage 2's 2,891; the committed grades are the collapsed order.",
  "The model's commands on this substrate carry the operator's absolute home path (the archived drives' context names it): in window/run.log, window/headroom.log, headroom-ada-rows.jsonl and control-fork.json it is replaced by ~, as it is in window/batches.log, this seat's own log. rows.jsonl carries none.",
  "One model reply quotes two of the research program's ticket ids from the archived context; in rows.jsonl and in the same reply as shown to the judge (judge/batches/batch-11.json) each is replaced by [ticket id withheld]. The judge saw the unredacted text; no applier and not the comparison reads reply text, and recompute.sh re-derives both products from the scrubbed files. scrub.json records every scrubbed file's pre-scrub sha256 and substitution counts.",
  "The request's model field is the server's alias, qwen3.8-flash-next, where stage 2 sent Qwen3.6-27B; every other field of each request is the fork's archived parameters, as in stage 2.",
  "The start row's weights digest is the first of the model's two main-weight shards; both are listed in its sampler card.",
  "The research program's instrument ran from its checkout at 15f4874 rather than the declared 406f835; nomination_framing.py is byte-identical (4e6ce98f...), and between the two commits only canary.py and canary_pool.py changed, neither of which the instrument imports.",
  "beside.py is stage 2's with its input renamed from stage2.json to word.json and nothing else; beside.json is read by nothing.",
  "Monitors beside the run, gating nothing: /slots polled 5,996 times over the run, never with two slots busy, so no other client's work overlapped a fork; per-request attribution is not possible, the rows carrying no timestamps. nvidia-smi every 5 s: median SM clock 2,505 MHz, maximum 90 C, the throttle reasons mostly software thermal slowdown and the software power cap (window/smi.csv).",
  "pre-registration.json was written after the fire, summarising the text on #142; the operative rules and appliers are the four files whose digests were posted on #142 before any fork fired.",
  "The ladder is nominal, as in stage 2: the six rungs are six seeded false-nomination assignments over the same forks, read as replicate stability.",
  "The endpoint is partly compliance: the imperative line tells the consumer to state the correction, so the imperative edit rate measures, in part, instruction-following on a false entry -- which is the harm the claim is about.",
]
targets_checked = 252
targets_matched = 252

[regime]
arm = "false-nomination-edit-rate-four-arms-six-rungs"
substrates = ["ada48-llamacpp-qwen38flashnext-q20"]
dogma_version = 0

[derivation]
applier_sha256 = "82e32760e3ff57af900bfe5fa6c804ed9752cfff20778b2c5b9730f282cdd7fa"
runtime = "Python 3.14.6"
substrate_id = "mac-pro-2019"
derived_from = "bbbcfc3061beb3524dddefac4721f8d62db3ae86bb07e663c1121a04a78f3cca"
+++

# False nomination: the edit rate, on a second substrate

(b′)-S2 (#142): (b′)'s pre-registered rule, re-fired on `ada48-llamacpp-qwen38flashnext-q20` with stage 2's plan, reported beside stage 2's word, with a pre-registered comparison of the imperative edit rate by fork.

## Observation

Stage 2 of #114 (`results/2026-09-27-false-nomination-edit-rate`) found, on the 27B floor, that an imperative framing makes the consumer assert a false correction on about half of fork-rungs, an advisory framing almost never, and an irrelevant passage never. Whether the effect's size depends on the model's capability was open.

## Hypothesis

When the collector's nominator fires falsely -- it presents an entry as superseded when it is not -- the consumer's reply asserts that the entry is superseded, no longer holds, or is wrong, or supplies replacement text for it, less often when the nomination is framed as advisory than when it is framed as imperative; and under the imperative framing it does so more often than when an irrelevant passage of the same length stands in the nomination's place.

## Test

- **Instrument and plan:** stage 2's false-nomination instrument and plan unchanged: four arms at six rungs over the same 100 forks, seed 924, the same forced forks. `plan.json` equals stage 2's in sample, assignments and headroom digest.
- **Substrate:** fired at `--concurrency 1` on the production line of `ada48-llamacpp-qwen38flashnext-q20` (instance `2026-09-28`, MTP draft on), prompts re-rendered through this model's template from the same (drive, step). The kwarg-delivery negative control passed at the window's start (`window/kwarg-start.json`).
- **Headroom:** re-checked on this substrate over the sampled forks; the 28 that failed are excluded from both endpoints.
- **Grading:** in both orders; 50 judge batches of 36 rows and 4 keyed controls, each judged by a fresh `claude-sonnet-5` instance blind to arm and key.
- **Endpoint 1:** `apply_bprime.py`'s word over `decision-rule.toml`, both at the digests stage 2 used (`word.json`, the product).
- **Endpoint 2:** `compare_s2.py` over `comparison-rule.toml` against stage 2's committed record (`comparison.json`).

Re-run: `bash recompute.sh` checks every consumed digest, runs both appliers' selftests, and re-derives `word.json` and `comparison.json` byte for byte.

## Results

Endpoint 1: edit rates on false nominations, counted fork-rungs (72 forks after the headroom exclusion):

| rung | counted | imperative | advisory | sham | imp − adv |
| --- | --- | --- | --- | --- | --- |
| 1.0 | 71 | 48/71 | 2/71 | 0 | 46/71 |
| 0.8 | 71 | 44/71 | 0 | 0 | 44/71 |
| 0.6 | 72 | 11/18 | 0 | 0 | 11/18 |
| 0.4 | 71 | 36/71 | 3/71 | 0 | 33/71 |
| 0.6222 | 71 | 44/71 | 2/71 | 0 | 42/71 |
| 0.0556 | 71 | 35/71 | 1/71 | 0 | 34/71 |

**At the 0.6 rung the supported clause holds:**
- imperative − advisory 11/18, sign-test p 2⁻⁴⁴;
- imperative − sham 11/18;
- no refuted clause holds, and the null gate passes.

**The word is `supported`, beside stage 2's `supported`.** At the same rung stage 2 read imperative 53/99 over 99 forks.

Endpoint 2, the imperative rate at 0.6 over the 72 shared counted forks:

| | 27B (stage 2) | this substrate | difference | sign test |
| --- | --- | --- | --- | --- |
| rate | 7/12 | 11/18 | -1/36 (unpredicted) | 16 of 30 discordant, p 759852347/1073741824 |

**The comparison rule's word is `substrate_independent`:** the rates differ by less than 0.05. The sham rate is 0 on every rung, so `control_failed` does not trigger.

## Conclusion

**Supported on the second substrate.** On a more capable model the imperative framing still makes the consumer assert a false correction on about three fork-rungs in five, the advisory framing never at the asked rung, and an irrelevant passage never.

**The pre-registered comparison reads `substrate_independent`.** The predicted drop with capability does not appear: the imperative edit rate at 0.6 is within 0.05 of the 27B's over the same forks. This is the alternative the pre-registration named: imperative framing overrides capability, and the harm of a false fire does not shrink as models improve, across these two models.

**What is not measured:**
- a third model;
- the forks this substrate's headroom excluded;
- whether the comparison word should read as a claim verdict (asked on #142).
