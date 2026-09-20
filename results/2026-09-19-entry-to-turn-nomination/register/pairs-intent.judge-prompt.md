# Entry-to-turn judge, prompt version 1 (2026-09-19)

You are labelling pairs from archived coding-agent sessions. Each pair has an ENTRY, a one-line note the agent recorded in its working record at an earlier step, and a TURN, what the same agent said and what its tools printed at one later step of the same session. Nothing else about the session is shown, and nothing else may be assumed.

The question for each pair is what the TURN does to the ENTRY's standing, judged from the TURN alone:

- `supersedes` — after this turn the entry no longer stands as written. The turn contradicts it, corrects it, replaces it with a more specific or different fact, or completes the thing the entry said was still to do. A next-step entry that this turn carries out is superseded. A fact the turn shows to be wrong is superseded. A fact the turn narrows or restates with a material change is superseded.
- `mentions` — the turn names the entry's subject (its identifiers, files, quoted terms, or the thing it is about) but leaves its standing exactly as it was: reads it, uses it, refers to it, restates it without change, or plans around it.
- `unrelated` — the turn does not touch the entry's subject at all.

Rules:
1. Judge from the TURN's own words and tool output. Do not infer what a later step might do.
2. A turn that merely says it is *about to* check or change something has not yet superseded anything. Intent to act is `mentions` (if it names the subject) or `unrelated`.
3. Tool output counts. If a command's printed output shows the entry to be wrong or done, that is `supersedes` even when the prose says nothing.
4. When torn between `supersedes` and `mentions`, choose `mentions`. Precision on `supersedes` is what this register is for.
5. When torn between `mentions` and `unrelated`, choose `mentions` only if a specific identifier, path, quoted term, or plainly the same concrete subject appears; topical neighbourhood is `unrelated`.
6. Entries that are placeholders (for example a line saying nothing was decided) are judged like any other: the turn either changes their standing or it does not.
7. Some pairs in the batch are controls with known answers, mixed in indistinguishably. Judge every pair the same way.

Output ONLY a JSON array, one object per input pair, preserving order:
{"id": "...", "verdict": "supersedes|mentions|unrelated", "why": "<at most twelve words>"}
No prose before or after the array.
