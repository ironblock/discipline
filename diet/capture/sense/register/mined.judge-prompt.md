# Register judge prompt, version 1 (2026-09-03)

You are labelling sentences for two registers used to calibrate an embedding-based nominator. Every sentence was written by an AI coding agent (the speaker) during a real session: its visible reply, its reasoning, or an answer to an interview question about the step it just took. Label EACH sentence against BOTH event classes below, independently. Judge only what the sentence itself says; do not infer from what the speaker probably meant.

## Class 1: `mistake`
- `positive`: this sentence reports or reveals a CONCRETE mistake the speaker made or a specific belief the speaker was acting on that turned out to be false — a realization, a correction of itself, a discovery that something it assumed (a path, a directory, a function, a field, a behaviour) is not so. "The ls output showed `server` but it's not actually a directory" is positive. "I was reading the wrong file" is positive.
- `hard_negative`: mistake-ADJACENT but not an instance: abstract discussion of how errors or mistakes happen; hypotheticals about what could go wrong; warnings or pitfalls stated in general; descriptions of error-handling code, error messages, or failure paths in the software being read; questions about whether something might be wrong with no conclusion; the speaker describing someone else's or the code's bug rather than its own false belief.
- `negative`: ordinary prose: routine steps, plans, findings about the code that do not correct a prior belief of the speaker's, descriptions, next actions.

## Class 2: `reversal`
- `positive`: the speaker discovers that a previously recorded, stated, or believed LIMITATION or ABSENCE no longer applies: something it thought was missing, unsupported, impossible, or removed is available after all; an earlier note or record is now out of date; a constraint it had recorded has been lifted. "Oh, I see now, X is available as a property of Y" is positive. "The earlier record was accurate for its time but the flag now exists" is positive.
- `hard_negative`: a limitation or constraint restated as STILL holding; a mention of an old constraint with no change; a wish that a limitation would go away; descriptions of software that restores, reuses, or reloads something (cache restore, checkpoint restore) that are about the code's mechanics rather than the speaker's beliefs; a supersession deliberation that concludes nothing changed.
- `negative`: everything else.

## Rules
- Exactly one label per class per sentence. When torn between `positive` and `hard_negative`, choose `hard_negative`: this register is precision-first, and a false positive costs more than a miss.
- Some sentences are controls with known answers, mixed in and indistinguishable. Label every sentence the same way.
- Output ONLY a JSON array, one object per input row, preserving order: `{"id": "...", "mistake": "positive|negative|hard_negative", "reversal": "positive|negative|hard_negative", "why": "<at most twelve words>"}`. No prose before or after the array.
