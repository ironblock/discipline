# The register

Sentences an AI coding agent wrote during real sessions, labelled against the
sense sets in `../sets.jsonl`, for the embedding bakeoff and the collector's
calibration. Rows are `{"id","text","label","source"}` and nothing else.

`authored-mistake.jsonl` is the authored register: written by hand for the
instrument's own tests, every row `source: authored`. It is not the corpus.

`mined-mistake.jsonl` and `mined-reversal.jsonl` are the corpus, every row
`source: mined`. `mined.provenance.jsonl` carries one row per corpus row
with the archive it came from; `mined.judge-prompt.md` is the instrument
that labelled it, pinned; `mined.controls.json` is the fourteen controls with
the answers the judge never saw, and `mined.judge-controls.py` scores a
verdict file against them: exit 0 when every control a batch carried is
answered as keyed, 1 naming the first miss, 2 for a batch carrying none.

## Provenance, as content

**Where the sentences come from.** The research program's archived drives,
every one conducted by the same model family on the same task family: 26
scripted or human-driven sessions auditing the llama.cpp server source (and
beellama, the program's own fork of it, whose server binary the box runs)
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
were judged, in batches of 45 rows plus 4 controls.

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

**One rule the judge applied that the prompt does not spell out:** when the
sentence is a supersession verdict about the working record ("the earlier
record is wrong"), the record counts as the speaker's own belief, so a
record found wrong is a positive and a record found right is a hard
negative. The judge applied it consistently across both rounds; the pinned
prompt is not edited in place, and a version 2 would state it.

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

**What a reader should know before trusting the counts.** The positives are
few episodes restated. A fresh-instance audit of the rows counted, among the
91 mistake positives, 21 that are one path mistake (a source tree read at
the wrong location), 12 that are one discovery (two files believed unique to
the fork exist upstream too), and 16 that are supersession verdicts over the
record; among the 17 reversal positives, 7 are that same file discovery and
6 are record verdicts. Rows are not independent events: the effective number
of reversal episodes is about five. The instrument's paired bootstrap and its
p-value floor treat rows as independent, so read them against this.

The same audit judged 40 sampled mistake positives (one firm disagreement, a
next-action sentence read as a mistake, and two fragments), 40 sampled hard
negatives (none firm, about four borderline, the speaker misreading its own
ask), and all 17 reversal positives (all defensible). The labels are the
judge's; the audit is recorded rather than applied, since a row relabelled
by hand is a row the controls never covered.

**Archives are named by experiment and drive** in the provenance file, never
by path: a path into a private repository is a pointer a reader cannot open.

**Scrubbing.** Home-directory paths inside sentences were shortened to
`~/`; no private address, hostname, ticket identifier or employer content
appears, and the hygiene gate is the check.
