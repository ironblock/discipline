# The depth probe (#143, I4c)

**The question.** After N tokens of real padding that contains counter-examples, does a rung still **apply** the rules stated in a session's first user turn?

This is one of the three results admission requires (planning's ruling on #143: "the depth probe in the rung's supported coding configuration ... the T2 design with counter-example padding").

## Where the design comes from, and why it is reimplemented

**The source.** The design is the research program's T2, whose instrument the maintainer made available for this (2026-09-29).

- **A multi-demand coding task under competing constraints.** The rules are stated early, and the task needs all of them at once.
- **Padding of alternating tool-output and acknowledgement turns,** cut from a code corpus that uses the forbidden forms.
- **Mechanical grading of the answer's final code block.**
- **Two tiers:**
  - `supported`: four rules;
  - `hard`: eight rules, two of them multi-hop.

The tier texts and the grader checks are carried over in substance.

**Reimplemented, not ported.** Admission needs four things the original does not do:
- **Prompts rendered by the rung's own template.** The original hand-builds one model's chat format; this probe sends messages to the chat endpoint, and each rung renders them.
- **Depth sized by the server's own token count.** The original estimated depth from characters. This probe bisects the padding's chunk count against `/apply-template` then `/tokenize`, and refuses a depth that misses its target by more than `--tolerance`.
- **Slots cleared before each depth.** On a unified cache another slot's resident cells count against the same context: the floor's 150k prefill failed with "Context size has been exceeded" while a 125k prompt sat in the other slot (measured 2026-09-29). Slot erase needs `--slot-save-path`, which the floor's line does not have, so the probe sends one tiny uncached request pinned to every slot, then pins all probe requests to slot 0 so a depth's samples share its prefix.
- **A corpus pinned by a manifest of digests** (`--corpus`), refused on any mismatch.

**What the original had that this leaves out:** its default endpoint and crash guard, which were tied to one machine's history.

## Use

```
python3 depth_probe.py run --endpoint http://HOST:PORT --corpus CORPUS/manifest.json --tier hard \
    --depths 0 40000 75000 125000 150000 --samples 5 --retrieval \
    --sampler '{"temperature": 0.6, "top_p": 0.95, "top_k": 20, "min_p": 0.0}' --out OUT
python3 depth_probe.py summarise OUT/rows.jsonl > OUT/summary.json
python3 depth_probe.py decide OUT/summary.json CRITERION.toml
```

- **Per-sample row:** each carries the target and rendered depth, `prompt_tokens`, `finish`, the reasoning length, every rule's result, `truncated` (hit `max_tokens` with no code block), and the code excerpt.
- **`meta.json`** records:
  - the sampler, seed and chunk size;
  - the corpus manifest's digest;
  - the served template's digest;
  - the per-slot context.

**How `decide` reads a summary** under a criterion TOML (`min_rate_per_depth`, `min_samples`, `max_errors`, `require_retrieval`):
- A depth with a server error or too few samples reads `unadjudicated`, never `pass`.
- A failing depth outranks an unadjudicated one.

## Tests

- **`python3 depth_probe.py selftest`** covers:
  - 12 grader fixtures, one per rule failing alone, plus: a missing code block; the last block graded over an earlier draft; the supported tier;
  - an end-to-end run against a scripted fake server: per-depth counts, every slot cleared before each depth, every request pinned, every depth reached;
  - 7 `decide` cases.
- **`python3 mutants.py`** applies each of 7 seeded mutations to a scratch copy and requires the selftest to exit 1:
  - the sync check dropped;
  - the first code block graded instead of the last;
  - slots not cleared;
  - requests not pinned;
  - server errors ignored;
  - `bufconst` accepting the bare literal;
  - the fail-over-unadjudicated order lost.
  All 7 are killed.
- **`fixtures/corpus/`** is a small synthetic corpus written for the tests. It is **not a probe corpus.**

## Proposals for planning: not decisions

The probe cannot run for admission until planning rules the corpus and the criterion (#143).

**1. Corpus.** A code corpus that uses the forbidden forms (u32/usize ids, async and `.await`, buffers other than 4096, little-endian parsing, `.unwrap()`), pinned by commit and manifest.

| option | licence (GitHub) | for | against |
|---|---|---|---|
| (a) this repository's own Rust tree, `diet/src/` at a pinned commit | Apache-2.0 | no third-party licence question; already public here | synchronous: few async counter-examples |
| (b) `tokio-rs/tokio` at a pinned commit | MIT | async counter-examples throughout; u32/usize; other buffer sizes | third-party; an attribution file under the corpus |
| (c) `BurntSushi/ripgrep` at a pinned commit | Unlicense | synchronous I/O and buffer handling, with sizes other than 4096 | few async forms |
| (d) (a) plus (b) mixed | Apache-2.0 and MIT | both kinds of counter-example | two pins to maintain |

- **Recommendation:** (d). Or (a) alone if planning wants no third-party text in the tree.
- **The original's corpus** was a third-party tree plus a build log and a directory listing. It is not proposed.

**2. Depth ladder**, from the floor's cold prefill measured on production (2026-09-29, single sample each): 40k 1,055 tok/s (38 s), 75k 733 (102 s), 125k 805 (155 s), 150k 738 (204 s).
- **Floor, 160,000 per slot:** 0 / 40k / 75k / 125k / 150k.
- **Candidate, 229,376 per slot:** 0 / 40k / 75k / 125k / 175k / 220k. Its prefill at depth was measured by the inference seat (#143): about 586 tok/s at 226,750.

**3. Samples per cell and the tier.** 5 samples per depth, and one retrieval per depth, on the `hard` tier. The original used 5 per cell, and its hard tier is the stricter test. Sampler: the rung's supported coding configuration, thinking on.

**4. Criterion**, one of:
- (i) `min_rate_per_depth = 1.0`: every sample at every depth honours every rule. This is the original's reading of "no cliff".
- (ii) `min_rate_per_depth = 0.8`: at most one miss per depth.
- (iii) A cliff defined relative to the zero-pad cell: fail when the deepest cell's rate falls below the zero-pad cell's by 2 samples or more. This needs a `decide` extension; not built.

The recommendation is (i), with `min_samples = 5`, `max_errors = 0`, `require_retrieval = true`, because it is the reading the original result was stated in.

**5. Window estimate** (floor, the ladder above, 5 samples plus 1 retrieval):
- **Prefill:** about 8 minutes, one cold prefill per depth, the samples reusing it.
- **Decode:** 30 generations of up to 6,144 tokens at 20–60 tok/s at depth, about 1–2 hours.
- **Floor window:** about 1.5–2.5 hours. The candidate's, with its deeper ladder, somewhat longer.
- **Production stays up throughout;** the probe occupies both slots while it runs.
