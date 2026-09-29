# The candidate's depth cell (#143, I4c)

**Word: `pass` (no cliff).** The strict reading beside it is also met: every sample passed at every cell.

This is the admission depth probe (`substrates/admission/depth-probe/`, #179; the instrument is unchanged since 10d866e, and the run's head was 05db8f5). It ran once on this fingerprint, the candidate launched fresh on its cells-window line, from 2026-09-29 08:25:35 to 09:00:26Z, with production down. The window was announced on #143 before it started.

| cell | target | depth (server tokens) | counter-examples planted | application | retrieval |
|---|---|---|---|---|---|
| control | 0 | 297 | 0 | 5/5 | 1/1 |
| 0.5 | 114,688 | 113,831 | 4 | 5/5 | 1/1 |
| 0.9 | 206,438 | 206,097 | 4 | 5/5 | 1/1 |
| 0.95 | 217,907 | 217,720 | 4 | 5/5 | 1/1 |

**Run configuration:**
- **Ladder:** fractions of the registry's `serving_context`, the 229,376-token pool shared by the two slots, with the other slot cleared before each cell (planning, #143).
- **Sampler, declared, not measured:** this rung's entry had no sampler card, so the probe used the floor's (temperature 0.6, top_k 20, top_p 1.0, min_p 0.0). That keeps the two rungs' depth cells comparable. The entry now carries it as `sampler_card_declared`. If a sampler is ruled for this rung, the cell is re-run under it.
- **Effort and thinking:** effort absent, which renders `xhigh` on this template; thinking on as served; `max_tokens` 4096.
- **Corpus:** this repository at `1833b8e` plus tokio at `tokio-1.53.1`, loaded against `corpus/manifest.json` digest by digest.
- **Samples:** no server error, no truncation, and no sample without reasoning; the smallest reasoning length is 633 characters. Every sample finished with `stop`.

**Identity:**
- **What was read in this window:** the running server's exe (`865044a2…`) is this fingerprint's `llama-server` (`../raw/engine-manifest.txt`), and its weights (`58fd8267…`) are this fingerprint's. The source tree is mainline `4ceb171`, clean (`raw/identity-before.txt`).
- **What was not re-hashed:** the build's other files and the container's libraries. The fingerprint's engine component is the digest of the whole manifest, so this window confirms its binary and weights, not every library.

**Downtime and restore:**
- **Downtime:** production was stopped at 08:25:28Z and restored at 09:00:52Z by the provisioning script, 35 min 24 s in all (`raw/window.log`). The restore brought back the floor's own exe and an identical command line.
- **Checks after the restore:** all passed (`raw/restore-checks.txt`, `raw/mac.log`):
  - `fingerprint --check --hash-models`: SUBSTRATE IDENTICAL;
  - the box verification script: PASS;
  - the canary against the pool: 36/36, PASS.

**A first launch sent no requests.** The Mac-side driver was not executable, so it exited before copying anything to the box. It was fixed and launched again. Everything recorded here is from the second launch.

**Files:**
- `cell.toml`: the word, reading, criterion, sampler, identity, downtime and each raw file's digest.
- `raw/rows.jsonl`, `raw/meta.json`, `raw/summary.json`, `raw/decide.json`: the rows, the run's configuration (with the corpus manifest's and served template's digests), and the instrument's `summarise` and `decide` output.
- `raw/console.log`: the probe's per-sample lines.
- `raw/mac.log`: the Mac side's step log.
- `raw/window.log` and `raw/identity-before.txt`: the box side's step log and the identity it read, both written by the box script.
- `raw/restore-checks.txt`: the verdict lines of the three post-restore checks, extracted by grep. Their full logs stay local, because they name the box's devices.
- `raw/mac.sh` and `raw/d143c.sh`: the two scripts as run. `d143c.sh` has one private build-container name scrubbed, recorded in `scrub.json` with its pre-scrub digest.
- `recompute.sh`: summarises the rows and decides again through the committed instrument and criterion, requiring both outputs to match byte for byte. It also checks:
  - the corpus manifest is the committed one, and the served template is this fingerprint's;
  - the tier is admission's, and the ladder is the ruled fractions of the registry's `serving_context`, every cell present and full, each target reached within 2% without exceeding it and carrying its counter-examples;
  - the sampler is the registry's declared card and `max_tokens` is 4096;
  - the running exe and weights are this fingerprint's, the window log's candidate is that process, and production was restored on its own exe and line;
  - the probe, the fingerprint check, the verification and the canary are recorded as passing;
  - every row regrades to its stored grade, every cell's samples are numbered 0 to 4 once each, and the server's prompt count equals the depth on every application row;
  - the console log is the rows';
  - `cell.toml`'s word, whole reading and digests agree, and every file in `raw/` is pinned.

  It was seen red on eleven seeded faults: the 0.95 cell dropped; the temperature changed; the 0.95 cell made shallow and unplanted; a retrieval failure; a sample's code swapped with its grade kept; the running exe, the weights, and the window's candidate pid each changed; production restored on another line; the canary failed; the fingerprint differed. Each was re-pinned.

**This is one of admission's three results for this fingerprint.** The cells are in `../cells.toml`. The parity fire waits on I5 and its pre-registration. The candidate's admission waits on all three.
