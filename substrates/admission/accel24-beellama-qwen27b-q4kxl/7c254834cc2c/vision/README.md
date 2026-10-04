# The floor's vision cell (#373)

This cell sends one synthetic image through the floor's registered serving line, and reads a typed outcome off a detail planted in the image. Ruled on #373 (5982304569).

- **The image.** `make_image.py` builds it from a fixed seed with the stdlib alone. It draws a four-character token from a 17-glyph alphabet with no look-alike glyphs, and places it at a seeded position on a plain background. The result is `image.png`. The token is carried only as its sha256 (`token.sha256`), and the prompt never contains it.
- **The run.** `run_cell.py` sent three sequential requests to `/v1/chat/completions`. Each was one user message: the image as an `image_url` `data:` part, plus a fixed question asking what text the image shows. The requests used the registered sampler card and `max_tokens` 8192, with reasoning as the line has it. The window was granted by Track 6 on #373 (5982346540). Production was used as it stood: no restart, one client.
- **The preflight** (`preflight/`): `fingerprint.py --check` against the 2026-10-02 capture with `--hash-models` read SUBSTRATE IDENTICAL, digest `4b194a74ff6ab263`. The running exe was read from `/proc`. The window's times are in `preflight/window.txt`.
- **The word.** `derive.py` decides each request in this order:
  1. `refused`, for a non-2xx response, an error body, or a 2xx body that carries no completion;
  2. `accepted`, when the reply's content holds the token (case-folded, whitespace removed);
  3. otherwise `answered-without-seeing`.

  The cell is `accepted` only if all three requests are. It is `refused` if any is, and otherwise `answered-without-seeing`. Here all three are `accepted`.
- **`cell.toml`** records the word, each request's word, n, the image and token digests, `max_tokens`, the sampler, the projector digest (`05353347…`, pinned by `../fingerprint.json` and the registry) and the instance. It also cites every raw file by sha256.
- **`recompute.sh`** re-checks all of it:
  - every cited file hashes as stated;
  - the image rebuilds from its seed;
  - the request carries that image and the registered sampler;
  - the word re-derives from `responses/`;
  - the registry's `vision` for the substrate equals the derived word, or the cell is refused.

  `verify.sh --only admission` runs it.

**Two notes from #392's review:**

- **`derive.py` was revised after the run.** A 2xx body with no completion now reads `refused`, in the ruling's sense of "a 2xx whose body is an error". The three responses each carry a completion, so the word is unchanged. `cell.toml` pins the revised script.
- **The preflight's command** was the one posted in the window ask (#373, 5982339964): `fingerprint.py --check <the 2026-10-02 capture> --hash-models`. The log keeps its output, not the command. It took 18 s; the model files are read on the box's own disk.

This directory sits outside the admission record's cells manifest, as `depth/` does (`../admission-recompute.sh`, `../../../derive_admission.py`). It is the evidence for the registry's `vision` field, not an admission result.
