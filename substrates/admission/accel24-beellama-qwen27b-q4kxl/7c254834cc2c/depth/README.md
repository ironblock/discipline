# The floor's depth cell (#143, I4c)

**Word: `pass` (no cliff).** The strict reading beside it is also met: every sample passed at every cell.

This is the admission depth probe (`substrates/admission/depth-probe/`, #179, merged at 10d866e), run once on this fingerprint's serving process. It ran 2026-09-29 08:02:12–08:19:55Z (17.7 min), production up, announced on #143 before it started.

| cell | target | depth (server tokens) | counter-examples planted | application | retrieval |
|---|---|---|---|---|---|
| control | 0 | 259 | 0 | 5/5 | 1/1 |
| 0.5 | 80,000 | 79,848 | 4 | 5/5 | 1/1 |
| 0.9 | 144,000 | 143,586 | 4 | 5/5 | 1/1 |
| 0.95 | 152,000 | 151,622 | 4 | 5/5 | 1/1 |

**Run configuration:**
- **Ladder:** fractions of the registry's `serving_context`, the 160,000-token pool shared by the two slots, with the other slot cleared before each cell (planning, #143).
- **Sampler:** the registry's sampler card (temperature 0.6, top_k 20, top_p 1.0, min_p 0.0), thinking on as served, `max_tokens` 4096.
- **Corpus:** this repository at `1833b8e` plus tokio at `tokio-1.53.1`, loaded against `corpus/manifest.json` digest by digest.
- **Samples:** no server error, no truncation, and no sample without reasoning. The open question on how a thinking-off sample is read (#179) therefore does not touch this cell.

**Identity:** the probe called the floor's serving process, the same pid as the cells window. Its exe digest, read after the window (`raw/identity-after.txt`), is this directory's `980845d6…`. The card's VRAM and the process's health were unchanged.

**Files:**
- `cell.toml`: the word, reading, criterion, and each raw file's digest.
- `raw/rows.jsonl`: one row per sample. Each row holds the cell, the rendered depth, the counter-examples planted, the finish, the reasoning length, each constraint's result and an excerpt of the code. A retrieval row holds its answer.
- `raw/meta.json`: the run's configuration, the corpus manifest's digest and the served template's digest.
- `raw/summary.json` and `raw/decide.json`: the instrument's `summarise` and `decide` output.
- `raw/console.log` and `raw/run.sh`: the run as launched. `run.sh` reads the box address from a local file; nothing private is in it.
- `recompute.sh`: summarises the rows and decides again through the committed instrument and criterion, requiring both outputs to match byte for byte. It also checks four things:
  - the run's corpus manifest is the committed one;
  - the ladder and tier are admission's;
  - the identity is this directory's exe;
  - `cell.toml`'s word and digests agree.

  It was seen red on three seeded faults: the word flipped to `fail`; one 0.95 sample's grade flipped, with the rows' digest re-pinned; and the committed corpus manifest changed by one byte.

**This is one of admission's three results for this fingerprint.** The cells are in `../cells.toml`, and the parity fire is separate. The admission word is written by hand where #143 says, not here.
