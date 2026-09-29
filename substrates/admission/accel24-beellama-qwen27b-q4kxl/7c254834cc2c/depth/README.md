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

**Identity:** the probe called the floor's serving process, the same pid as the cells window. Its exe digest, read after the window (`raw/identity-after.txt`), is this fingerprint's engine `980845d6…`. The card's VRAM (23,684 MiB) and the process's health were the same before and after; those two reads are transcribed from the operator's terminal in `raw/window-readings.txt`, labelled as such, not written by a script.

**A first launch sent no requests.** At 08:01Z the runner tried an ssh forward to the floor's port; the forward was refused, and the runner was stopped by pid while still in its health-check loop, before the probe started. The run recorded here called the floor's port directly. `raw/run.sh` is that second runner. It reads the box's address from a local file that sits in another window's directory; the file holds this box's address, not another machine's.

**Files:**
- `cell.toml`: the word, reading, criterion, and each raw file's digest.
- `raw/rows.jsonl`: one row per sample. Each row holds the cell, the rendered depth, the counter-examples planted, the finish, the reasoning length, each constraint's result and an excerpt of the code. A retrieval row holds its answer.
- `raw/meta.json`: the run's configuration, the corpus manifest's digest and the served template's digest.
- `raw/summary.json` and `raw/decide.json`: the instrument's `summarise` and `decide` output.
- `raw/window-readings.txt`: the before and after reads, transcribed, and how `identity-after.txt` was read.
- `raw/identity-after.txt`: the floor's pid, exe digest and start time, read after the window.
- `raw/console.log` and `raw/run.sh`: the run as launched.
- `recompute.sh`: summarises the rows and decides again through the committed instrument and criterion, requiring both outputs to match byte for byte. It also checks:
  - the run's corpus manifest is the committed one;
  - the tier is admission's, and the ladder is the ruled fractions of the registry's `serving_context`, with every cell present and full;
  - the sampler is the registry's card and `max_tokens` is 4096;
  - the identity is this fingerprint's engine and the served template is this fingerprint's (both read from `../fingerprint.json`);
  - every cell's target is its fraction of the pool, its rendered depth reached the target within 2% without exceeding it, and every deeper cell carries all four counter-examples, in the summary and in every row;
  - the pid after the window is the pid read before and after it;
  - every row regrades to its stored grade through the committed grader (application code and retrieval answers alike; an excerpt at its cap would be refused), every cell's samples are numbered 0 to 4 once each, and the server's prompt count equals the rendered depth on every application row;
  - the console log's per-sample lines are the rows';
  - `cell.toml`'s word, whole reading (retrieval and defect counts included) and digests agree, and every file in `raw/` is pinned.

  It was seen red on sixteen seeded faults: the word flipped to `fail`; one 0.95 sample's grade flipped, with the rows' digest re-pinned; the committed corpus manifest changed by one byte; the 0.95 cell dropped with the summary and decision regenerated and re-pinned; the temperature, `max_tokens` and template digest each changed and re-pinned; the 0.95 cell made shallow and unplanted, regenerated and re-pinned; an unpinned raw file edited; the pid changed; the reading misstated; a retrieval failure regenerated; a sample's code swapped with its grade kept; a duplicated sample; the prompt count changed; a console line changed.

**This is one of admission's three results for this fingerprint.** The cells are in `../cells.toml`, and the parity fire is separate. The admission word is in `../admission.toml`, derived from the three results by `substrates/admission/derive_admission.py` (#183).
