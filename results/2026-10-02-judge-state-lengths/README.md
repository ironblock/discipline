+++
hypothesis = "Under laya's own tokenizer, the 90th percentile of the archive's judge states fits laya-en's state budget under both of #165's questions, so the judge seat's variant is laya-en."
result = "refuted"
kind = "reproducible-by-config"
product_sha256 = "753c5737c89e35d58b272b072492615c249a1a66914363fc1b078a36da6b811d"
controls_run = ["the two heads measured against their caps", "each option against its 48-token cap"]
known_defects = [
  "The translation table the two heads are measured on is the data seat's draft, under planning's review on #165 (comments 5942974019 and 5943124493). If planning changes a question's wording or options, the heads and so the state budgets change, and this directory is re-run with the reviewed table; the state counts do not depend on the table.",
  "The sequence construction is Sidekick's port of laya's builder as relayed by Dispatch (2026-10-02), not read from laya's own code here: each fragment tokenized alone without specials, each option capped at 48 tokens, the question and options capped at head_max_len, the state given max_len minus the head minus the closing [SEP]. The smaller-budget rules Sidekick gives when options crowd the question are not implemented, because neither question's head reaches its cap (heads.json).",
  "An option's text is composed as '<key>: <label>', following laya's rendering of criteria; laya-typed-decisions' own head rules beyond head_max_len 256 were not relayed and are taken to be laya-en's.",
  "The states are composed as translation-table.json composes a control's state (the prompt's labels, ENTRY or NOTE, REASONING, PROSE). The judges saw the same three fields as JSON keys in a batch file, not this composed text; the composition is the translation's, and the count is of the text laya would see.",
  "Unique states only: an item judged in more than one batch counts once, by the sha256 of its composed state; the first record that held it is named in counts.jsonl.",
  "In check-recompute's sandbox the counts are not re-tokenized (no tokenizers library, no archive around the directory): what re-derives there is the report from the committed counts. Where both are present, recompute.sh rebuilds the counts from the archive's batches and requires them byte for byte.",
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
- **Budgets:** for each question of the translation table, the head is measured as laya builds it (`heads.json`), and the state gets the model's `max_len` minus the head minus the closing `[SEP]`.
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

Refuted: laya-en's p90 does not fit under either question, so by #165's procedure the judge seat's variant is **laya-typed-decisions**, which fits the p90 under both and still truncates about 3% of states. Those truncated states are the long reasoning traces, cut from the end. The table's review on #165 can change the heads and so the budgets; the counts stand.
