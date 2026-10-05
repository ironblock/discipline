# `rtx6000ada-host`: ExLlamaV3 1.5.2 → 1.5.4, the KV pool sized to the 300 MiB bar, and YaRN ×2 (#401)

**Two new instances of `ada48-tabbyapi-exl3-qwen38flashnext-2p05`,** both measured and switched on 2026-10-04 by the inference seat. The maintainer approved each step in the inference seat's session, and Dispatch cleared each window. The record is on #401: 5984372939 (the window), 5984591312 (results), 5984646282 (the 1.5.4 switch), 5984938409 (the YaRN trial) and 5985333953 (the YaRN switch). Registry ids only.

## Instance A: ExLlamaV3 1.5.4 with a 606,208-token pool (from 21:36:18Z)

**The window** (`window/`): the endpoint was down 21:05:28–21:16:38Z, then restored on 1.5.2.
- **KLD on 1.5.4:** the same scorer, flags and base files as #336 §1 (`window/kld-205-v154.log`). 0.0860 code / 0.2297 prose, identical to 1.5.2's #336 row.
- **ABAB of TabbyAPI** on a side port with the live config: 1.5.2, 1.5.4, 1.5.2, 1.5.4.
  - **Concurrency:** each arm ran the four-way check (`cont-*.log`), and all were clean.
  - **Bench:** 10k at 1 and 4 streams, then ~100k (`deep-*.log`).
  - **Speed:** 1.5.4 is deterministic across identical greedy repeats and 2–5% slower at 10k (125.5 against a mean of 128.1 single-stream; 147 against 154 total at four streams). At ~100k it holds a steady 132.0; 1.5.2 drifts between repeats (acceptance 0.46–0.55 at 10k and 0.56–0.79 at ~100k).
  - **VRAM after load:** 41,238 MiB on 1.5.4, against 43,762.

**Sizing and the switch** (`sizing/`): the endpoint was down 21:30:12–21:36:18Z.
- **KV cost** (`size401.py`): load readings of 41,238 MiB at 262,144 and 46,078 at 524,288 give 18.9 KiB/token.
- **Largest pool:** 638,976 does not load (`tabby-cs638976.log`, "Insufficient VRAM in split"). 606,208 loads at 47,598 MiB.
- **Full fill at 606,208:** four concurrent requests of 150,438 tokens each peaked at 48,162 MiB, leaving **978 MiB free** (`size401.json`). That passes the v0.1.0 bar of 300 MiB (#143 5984376546).
- **Production then started** on 1.5.4 at that pool. The four-way check on the production endpoint was clean (`cont-prod154.log`).

**Components that changed from instance `2026-10-03`:**

| component | 1.5.2 (`2026-10-03`) | 1.5.4 (instance A) |
|---|---|---|
| ExLlamaV3 wheel | 1.5.2 | `exllamav3-1.5.4+cu132.torch2.11.0-cp312`, sha256 `c810dc4ad13e105cc62da2354405ae3d967c143359d683f323149a45c25fc702` |
| compiled extension | as registered | `9157a0af8dc1135f…` (`window/identity.txt` has the full digest) |
| venv | the serving venv | a copy of it with only `exllamav3` replaced; frozen set in `configs/tabby154-freeze.txt` (90 packages) |
| live config | `e52b13bb…` | `64d7ca56…`; only `cache_size` changed, 262144 → 606208 |

Unchanged: TabbyAPI `be74bf0a`, torch 2.11.0+cu130, Python 3.12.14, driver 615.71.09.

## Instance B: YaRN ×2 (from 22:46:21Z)

**The trial** (`yarn-trial/`): the endpoint was down 21:52:43–22:01:56Z.

**The overlay directory:** symlinks to the serving files, plus one edited `config.json` (`configs/yarn2-config.json`, sha256 `47807d665e9aa57b…`). The edit sets `rope_type: yarn`, `factor: 2.0`, `original_max_position_embeddings: 262144` and `max_position_embeddings: 524288`. This is the model card's YaRN form, at factor 2.

| | native | YaRN ×2 |
|---|---|---|
| KLD, code / prose (`kld-yarn2.log`) | 0.0860 / 0.2297 | 0.0880 / 0.2317 |
| three needles at ~252k (`needle-*-250k.json`) | 3/3 | 3/3 |
| three needles at 480,125 tokens (`needle-yarn2-480k.json`) | — | 3/3 |
| decode, 10k / ~100k (`deep-yarn2.log`) | 125.5 / 132.0 | 122.5 / 121.3 |

**The switch** (`yarn-prod/`, `scripts/yarnprod.sh`): the endpoint was down 22:45:39–22:46:21Z.
- The served name `qwen3.8-flash-next` is a symlink in the models directory, repointed from the native 2.05 bpw directory to the overlay.
- The live config's `max_seq_len` went 262144 → 524288, and nothing else changed: `configs/config-post-yarn.yml`, live sha256 `b5db18d5…`.
- `/v1/model` reports `max_seq_len` 524288 and `cache_size` 606208; the four-way check is clean (`cont-prod-yarn.log`).
- **Client-visible:** the model id reported by `/v1/model` is now the overlay directory's name.

## Rollback

- **To instance A:** repoint the symlink to the native directory, then restore `config.yml.pre-yarn` (`configs/config-post401.yml`).
- **To 1.5.2:** also restore `config.yml.pre401b` (`configs/config-pre401.yml`) and the 1.5.2 launcher.

All of these stay on the host.

## Files and redaction

| path | contents |
|---|---|
| `window/` | the ABAB window: logs, KLD, identity |
| `sizing/` | pool sizing and the 1.5.4 switch |
| `yarn-trial/` | the YaRN trial |
| `yarn-prod/` | the YaRN switch |
| `configs/` | the three live configs, the overlay's `config.json`, the frozen package set |
| `scripts/` | every script that ran |

**Redaction:** `$HOME`, `<user>`, `<host>`, `<port>` (the serving port), `<side-port>` and `<api-key>`. The scripts read the key from the server's token file at run time; no key appears here. The configs' digests above are of the live files. The copies here differ only in the redacted port, so they don't reproduce those digests. `SHA256SUMS` covers every file.
