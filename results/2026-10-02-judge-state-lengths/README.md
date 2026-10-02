+++
hypothesis = "Under laya's own tokenizer, the 90th percentile of the archive's judge states fits laya-en's state budget under both of #165's questions, so the judge seat's variant is laya-en."
result = "refuted"
kind = "reproducible-by-config"
product_sha256 = "052005d6323d23cde63e8edee80685629c10bbe4f6f95101918721dab805cb2d"
controls_run = ["the two heads measured against their caps", "each option against its 48-token cap"]
known_defects = [
  "The translation table the two heads are measured on is the data seat's draft, under planning's review on #165 (comments 5942974019 and 5943124493). If planning changes a question's wording or options, the heads and so the state budgets change, and this directory is re-run with the reviewed table; the state counts do not depend on the table.",
  "The sequence rules are laya's build_sequence (rl_common.py, lines 47-77, at 55cf4c4e), read against measure.py: each fragment tokenized alone without specials, each option a [MASK] plus at most 48 tokens of ' <key>: <label>', the question cut to head_max_len less the options, the state given max_len minus the head minus the closing [SEP] and cut from its end; max_len and head_max_len are laya's configs (512/192, 1024/256). The option-shrinking branch (options leaving the question under 16 tokens) is not implemented, because neither question's options come near it (heads.json). laya serializes a state its own way (serialize_state); the text counted here is the translation's composition.",
  "The states are composed as translation-table.json composes a control's state (the prompt's labels, ENTRY or NOTE, REASONING, PROSE). The judges saw the same three fields as JSON keys in a batch file, not this composed text; the composition is the translation's, and the count is of the text laya would see.",
  "The percentile is over unique states: an item judged in more than one batch counts once, by the sha256 of its composed state; the first record that held it is named in counts.jsonl, with how many judged items carried it. Over all 6976 judged items the p90 is 401, which fits laya-en's edit budget (412) but not its verdict budget (350), so the variant is the same under either count (report.json, p90_all_items_within). The translation table's 14 distinct states (the noul-neutral row reuses ctl01's) are among the counted states.",
  "Percentiles are the lower order statistic, index floor(p*(n-1)), without interpolation; nearest-rank gives a p90 one token higher, and no budget is that close.",
  "The glob reads results/*/judge/batches only; the nested copies under 2026-09-29-false-nomination-edit-rate-second-substrate/stage2-record/judge/batches are not read, and every state in them is already counted from the top-level records (fresh review of #247).",
  "In check-recompute's sandbox the counts are not re-tokenized (no tokenizers library, no archive around the directory): what re-derives there is the report from the committed counts. Where both are present, recompute.sh rebuilds the counts from the archive's batches and requires them byte for byte.",
  "recompute.sh checks the front matter and report.json, not the figures written in the README's body: a changed body number stays green there. The body is generated from report.json; the figures to trust are report.json's.",
  "The choice of laya-typed-decisions at 1,024 tokens on this measurement is planning's ruling on #165 (comment 5944195901, 2026-10-02T02:00Z), applying #165's procedure to these numbers; this directory measures, the ruling selects.",
  "The hypothesis is the procedure's first branch stated as a claim so that the record can carry the word: refuted means laya-en's p90 does not fit, which is what selects laya-typed-decisions; it is not a finding against laya-en.",
]
claim_issue = "165"
absent = { supersedes = "nothing replaced: the first measurement of #165", rule_ratified = "#165's procedure is planning's design answer on the claim stub, not a ratified decision rule", window_start = "the archive's judge states were read, not drawn; no window" }
targets_checked = 7
targets_matched = 7

[regime]
arm = "judge-state-lengths"
substrates = ["laya-tokenizer"]
dogma_version = 0
+++

# Judge state lengths, in laya's tokens

#165's procedure chooses the judge seat's laya variant from the states it will read: laya-en if the 90th percentile of the judge states fits, laya-typed-decisions at 1,024 if not.

## Observation

The archive's Sonnet judges graded 5732 unique states across four judged records. Two proxy tokenizers put the 90th percentile at 431–447 tokens (#165, comment 5942974019), close enough to laya-en's budget that the proxy could not decide.

## Hypothesis

Under laya's own tokenizer, the 90th percentile of the archive's judge states fits laya-en's state budget under both of #165's questions, so the judge seat's variant is laya-en.

## Test

- **States:** every item in every `results/*/judge/batches/batch-*.json` (consumed by digest in `batches.json`), composed as the translation table composes a control's state.
- **Tokens:** counted by laya's own tokenizer, `tokenizer.json`, sha256 `6c8aaa9a542084f2457eab775d4eeb51f92a70c0fd9de28d5edb0ddec3c08d30`, from `convaiinnovations/laya` at `55cf4c4e…` (Apache-2.0; byte-identical in laya-typed-decisions), registered as `laya-tokenizer`.
- **Budgets:** for each of the two judge questions (verdict and edit, as the table's control rows ask them; the noul-neutral row's questions are not judge questions and are not measured), the head is measured as laya builds it (`heads.json`), and the state gets the model's `max_len` minus the head minus the closing `[SEP]`.
- **Re-run:** `measure.py build <repo>` (needs the `tokenizers` library), then `measure.py report`.

## Results

The states, in laya's tokens: p50 142, p90 **439**, p95 714, max 2641.

| model | question | head | state budget | states truncated | p90 fits |
| --- | --- | --- | --- | --- | --- |
| laya-en | verdict | 161 | 350 | 809 (14.1%) | no |
| laya-en | edit | 99 | 412 | 625 (10.9%) | no |
| laya-typed-decisions | verdict | 161 | 862 | 189 (3.3%) | yes |
| laya-typed-decisions | edit | 99 | 924 | 141 (2.5%) | yes |

Both heads are within their caps (verdict 158 and edit 96 tokens of question and options, against 192 and 256), and no option reached the 48-token cap.

## Conclusion

Refuted: laya-en's p90 does not fit under either question, so by #165's procedure the judge seat's variant is **laya-typed-decisions**, which fits the p90 under both and still truncates about 3% of states. Those truncated states are the long reasoning traces, cut from the end. Planning ruled the selection on #165 (comment 5944195901): laya-typed-decisions at 1,024 tokens. The table's review on #165 can change the heads and so the budgets; the counts stand.
