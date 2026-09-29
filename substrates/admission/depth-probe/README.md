# The depth probe (#143, I4c)

**The question.** After padding that fills a fraction of the rung's declared serving context, does the rung still **apply** the rules stated in a session's first user turn?

This is one of the three results admission requires. It is built to planning's ruling on #143 (2026-09-29, comment 5885161766):

| the ruling | how the probe meets it |
|---|---|
| **Corpus:** the gym's own tree at a pinned commit | `make_corpus.py` writes `corpus/manifest.json`: every `.rs` file under `diet/src/` as a file read, every directory under it as a listing, and the first-parent diffs of `diet/src/` in the last 12 commits, each with the sha256 of the exact bytes read. Pinned now at `1833b8e`: 58 reads (2.7 MB), 15 listings, 12 diffs. The probe refuses any mismatch. |
| **Counter-examples** from a committed script, planted at declared depths, pinned by digest | `counterexamples.py` writes `counterexamples.json`: one Rust file-read turn per constraint, each asserting the constraint's opposite. The four-constraint set is planted at 0.2/0.4/0.6/0.8 of the padding. Deterministic (written twice, byte-identical). Its sha256 is in the manifest. |
| **Ladder:** fractions of the rung's declared `serving_context`, 0.5 / 0.9 / 0.95, plus the zero-pad control | `--serving-context` and `--fractions` (default `0 0.5 0.9 0.95`). A cell's padding is sized by bisection on the server's own token count, and refused if it misses its target by more than `--tolerance`. The run refuses a ladder whose deepest cell plus `max_tokens` exceeds the server's per-slot context. |
| **Five samples per cell**, the rung's supported coding configuration, thinking on | `--samples 5`; `--sampler` is the rung's configuration. **Thinking is checked, not assumed:** a sample with no reasoning is counted as `thinking_off`, and its cell reads `unadjudicated`. |
| **Word:** `no cliff` = every cell's pass count within one sample of the control's | `decide` returns `pass` / `fail` / `unadjudicated` from `summary.json` and a criterion TOML (`within = 1`, `min_samples = 5`, `max_errors = 0`). A cell with a server error, a thinking-off sample or too few samples is `unadjudicated`, never `pass`. A cliff outranks an unadjudicated cell. The control itself must be adjudicable. |
| **The grader is mechanical**, one deterministic check per constraint, a seeded fault per constraint | `CHECKS` in `depth_probe.py`: one check per constraint on the answer's final code block. `fixtures/graders.json` violates every constraint alone, in both sets, and the selftest refuses a constraint without such a fixture. No model grades anything. |
| **The four-constraint set is the admission probe**; the eight-constraint set is a second probe | `--tier supported` (the default) is admission's. `--tier hard` is the second probe. `meta.json` records which, as `admission: true/false`. |

## Where the design comes from

The design is the research program's T2, which the maintainer made available (2026-09-29):
- the constraint texts, the task, and the grader checks are carried over in substance;
- the code is new.

What differs, and why:
- **Prompts are rendered by each rung's own template** through the chat endpoint. The original hand-built one model's chat format.
- **Depth is measured by the server's tokenizer.** The original estimated it from characters, at about 3.3 per token.
- **Every slot is cleared before each cell, and every request is pinned to slot 0.** `evidence/floor-prefill-2026-09-29/` shows why:
  - under `--kv-unified`, another slot's resident prompt made a 150k prefill fail;
  - slot erase is refused on a line without `--slot-save-path`;
  - a pinned tiny uncached request per slot released the cache.
- **The eight-constraint set has no retrieval question,** as in the original. Only the four-constraint set asks one, reported beside the word and not read by it.

What is not carried over: the original's default endpoint and machine-specific crash guard.

## Use

```
python3 depth_probe.py run --endpoint http://HOST:PORT --corpus corpus/manifest.json --tier supported \
    --serving-context 160000 --samples 5 --retrieval \
    --sampler '{"temperature": 0.6, "top_p": 0.95, "top_k": 20, "min_p": 0.0}' --out OUT
python3 depth_probe.py summarise OUT/rows.jsonl > OUT/summary.json
python3 depth_probe.py decide OUT/summary.json criterion.toml
```

- **Per-sample row:** each records:
  - the cell's fraction, target and rendered depth;
  - how many counter-examples are planted in its padding;
  - `prompt_tokens`, `finish` and the reasoning length;
  - every constraint's result;
  - `truncated`, and the code excerpt.
- **`meta.json`** records:
  - the sampler and seed;
  - the corpus manifest's digest;
  - the served template's digest;
  - the per-slot context.
- **`criterion.toml`** is the ruled word: `within = 1`, `min_samples = 5`, `max_errors = 0`.

## Tests

- **`python3 depth_probe.py selftest`** covers:
  - 16 grader fixtures: every constraint violated alone in both sets, a missing code block, the last block graded over an earlier draft;
  - a check that every constraint has its fault;
  - an end-to-end run against a scripted fake server, which checks:
    - per-cell counts and plantings, including one thinking-off sample;
    - the control carries no counter-example, and every deeper request carries all of them;
    - slots cleared, requests pinned, depths reached;
  - 11 `decide` cases.
- **`python3 mutants.py`** seeds 11 mutations and requires the selftest to exit 1 on each. **All 11 are killed:**
  - two grader checks dropped;
  - the first code block graded;
  - counter-examples not planted;
  - slots not cleared;
  - requests not pinned;
  - the cliff threshold off by one;
  - server errors ignored;
  - thinking-off ignored, and never counted;
  - the cliff/unadjudicated order lost.
- **`fixtures/corpus/`** is a synthetic corpus for the tests, not a probe corpus.

## Window estimate (floor, 160,000 per slot)

**The cells:** 0 / 80,000 / 144,000 / 152,000 tokens.
- **Prefill:** one cold prefill per cell, the samples reusing it. The floor's cold rate measured 1,055 tok/s at 40k and 738 at 150k, so about 6–7 minutes.
- **Decode:** 20 samples plus 4 retrievals, up to 4,096 tokens each at depth, about 1–1.5 hours.

**About 1.5 hours, production up;** the probe occupies both slots while it runs. The candidate's cells (at 229,376: 114,688 / 206,438 / 217,907) take somewhat longer.
