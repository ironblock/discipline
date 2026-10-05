# Admitting a TabbyAPI + ExLlamaV3 line (#393)

The instruments in this directory prefixed `tabby_` take the admission pipeline's three results (cells, depth, parity) to a
Python/torch engine that has no `/props`, no slots and no slot-cache report. `depth_probe.py` is untouched: its digest is
pinned by every recorded depth cell, and the wrappers swap only the server layer.

**The maintainer's reason for the rulings below** (#393, 5986337117): "this is part of the hazard of running the fastest
engine, and building the knowledge is the long-term benefit."

## The four rulings, as the instruments read them

1. **Checkpoint restore** is `n/a (no slot-cache report on this engine: ...)`. The word may read `admitted`.
2. **Output invariance** is `n/a` under the registry's hazard line: nothing is gated on token identity.
3. **The parity fire's routing check** is the instance identity plus `/v1/model`'s id checked at every request
   (`tabby_route_proxy.py`, `apply.py`'s `routing = "identity"`), pre-registered in `../parity/PREREGISTRATION-tabbyapi.md`
   before the fire. A fire whose routing check cannot run is void.
4. **Template digest and rendered effort** come from the model directory on the host. Rendering is a capability fact.

Both `n/a` readings must carry the maintainer's sentence above verbatim; `tabby_cells.py check` refuses a record whose
reading lacks it.

## The instruments

| file | does | selftest |
|---|---|---|
| `tabby_identity.py` | on the host, with the serving venv's python: recomputes the ten components and `engine_identity` in the registry form's order; `--expect` names every drifting component | none (runs on the host with the serving venv) |
| `tabby_kwarg.py` | `raw/kw.json`, `raw/refusal.json`: the paired control (thinking off, on, absent), the refused levels (`high none max`, expect 400) with a `low` control, the model-directory template rendered per level | in `tabby_cells_selftest.py` |
| `tabby_canary.py` | diet-inference's `canary.py` through a bearer wrapper; the first draw of a rung is `baseline` | none (needs the endpoint) |
| `tabby_depth_probe.py` | `depth_probe.py` against `/v1/chat/completions`; depth by rendering the model-directory template locally and encoding the string at `/v1/token/encode`; each request's `usage.prompt_tokens` is checked against it | `selftest` |
| `tabby_headroom_fill.py` | concurrent requests at the pool's size with `nvidia-smi` sampled; `passes_bar` against the 300 MiB bar | none (needs the endpoint) |
| `tabby_fingerprint.py` | `fingerprint.json` and the 12-character directory name; `--check DIR` re-derives it | in `tabby_cells_selftest.py` |
| `tabby_cells.py` | `init` writes `cells.toml` and the recompute scripts from `raw/`; `check` re-derives every cell | `tabby_cells_selftest.py`: a control green, seven seeded faults each red with its signature |
| `tabby_route_proxy.py` | the parity fire's per-request routing check | `selftest` |
| `tabby_capture421.py` | #421's capture: three POSTs and three GETs as raw bytes, digests in `notes.json`; `verify` re-hashes and fails if the key appears in any kept file | `tabby_capture421_selftest.py` |

The API key, when the endpoint has one, comes from `TABBY_API_KEY` and is never written to a file; the kept request heads
redact the Authorization line.

## The window, in order

One window of about two to two and a half hours of downtime for the production endpoint. Nothing below runs until the
inference seat says the floor is free and the rulings above are on the thread. The sequence stops at the first step
that cannot run.

1. **Identity.** `tabby_identity.py` on the host against the baseline line's components. A drifting component stops the window.
2. **Cells.** `tabby_kwarg.py`, `tabby_canary.py` (its first draw is the baseline), `tabby_headroom_fill.py`; then
   `tabby_fingerprint.py` and `tabby_cells.py init` / `check` in the new record directory `<substrate-id>/<fp12>/`.
3. **Depth probe.** `tabby_depth_probe.py run` at the declared serving context, then the unchanged `depth_probe.py` summarise/decide.
4. **Parity fire**, under `../parity/PREREGISTRATION-tabbyapi.md`: one `tabby_route_proxy.py` per seat in front of the endpoint,
   then the band, the fire, and `apply.py` with `routing = "identity"`.
5. **The vision cell**, with #373's script.
6. **#421's capture**, `tabby_capture421.py capture`, committed under the record's admission directory with digests.
7. **Switch or restore.** The switch happens only if every cell word is `admitted` and the vision cell is `accepted`;
   otherwise the llama.cpp line is restored with `provision-diet.sh`. Then #301's regimen names the new registry id.

`admission.toml` cannot exist before step 4's result; `admission-recompute.sh` writes the word from the results and
refuses a word not supported by them. This PR carries the instruments, the registry entry and the pre-registration only.
