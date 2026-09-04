# The register

Sentences an AI coding agent wrote during real sessions, labelled against the
sense sets in `../sets.jsonl`, for the embedding bakeoff and the collector's
calibration. Rows are `{"id","text","label","source"}` and nothing else.

`mistake.jsonl` is the authored register: written by hand for the
instrument's own tests, every row `source: authored`. It is not the corpus.

`mined-mistake.jsonl` and `mined-reversal.jsonl` are the corpus, every row
`source: mined`. `mined.provenance.jsonl` carries one row per corpus row
with the archive it came from; `mined.judge-prompt.md` is the instrument
that labelled it, pinned.

## Provenance, as content

**Where the sentences come from.** The research program's archived drives,
every one conducted by the same model family on the same task family: 26
scripted or human-driven sessions auditing the llama.cpp server source
(native calibration run 2, 2026-07-31 to 2026-08-08; the cache-ram sweep,
2026-08-05 to 2026-08-08; the human-driven drives, 2026-08-10 to
2026-08-15; the Apple-silicon floor suite), the 630 interview answers of the
framing experiment over those drives (2026-08-19), the Pi-harness live
drives (2026-07), and the replay substrate session (2026-06-30, a Minecraft
clone against an unfamiliar rendering library). The speaker is Qwen3.6-27B
throughout, on the program's inference box or on Apple silicon; both are
named per row in the provenance file. Sentences were taken from the model's
visible replies, its reasoning, and its answers to interview asks, split at
sentence boundaries, 30 to 320 characters, code blocks removed.

**How they were chosen.** Two lexical passes: a narrow seed set (the
program's own corrections: "turns out", "wait,", "I was wrong", "not
actually", "misread", "I assumed", and the like), then a broad one
("actually" anywhere, "I forgot/missed/overlooked", "was wrong", "in
fact", "no longer", "out of date", "earlier I said"). Mistake-adjacent
seeds ("pitfall", "could go wrong", "if I were wrong", "false positive")
mined hard-negative candidates, and a 2% random sample of unmatched
sentences from the same files supplied routine negatives. 1,949 candidates
were judged.

**How they were labelled.** By a judge model under seeded controls, the
human auditing samples only. The judge saw each sentence with the pinned
prompt (`mined.judge-prompt.md`, version 1) and labelled it against both
classes independently, precision-first. Every batch of 45 carried four
controls with withheld answers, indistinguishable from the rows; a batch
whose controls did not all score would have been re-judged. Round one: 96 of
96 controls; round two: 80 of 80. Two control answers in the author's first
key disagreed with the prompt's own definitions (a limitation restated as
still holding is `hard_negative`, not `negative`; a reversal may also be a
concrete false belief, since the classes are labelled independently) and
were corrected before scoring; no judge verdict was changed. The judge was
Claude Fable 5.1, in fresh sessions with no access to the key.

**How the rows were picked.** Every judged positive; matched negatives drawn
from the same archives in the same proportions; hard negatives capped at 50
per set. Duplicate texts were removed at mining.

| set | positive | matched negative | hard negative |
| --- | --- | --- | --- |
| `mistake` | 91 | 91 | 50 |
| `reversal` | 17 | 17 | 50 |

The counts are the archive's own rates under a precision-first judge, not a
target met: of 1,949 candidates the judge called 91 concrete mistakes and 17
reversals. A register wanting more positives needs more sessions, not a
looser judge.

**Scrubbing.** Home-directory paths inside sentences were shortened to
`~/`; no private address, hostname, ticket identifier or employer content
appears, and the hygiene gate is the check.
