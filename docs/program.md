# The program

*The system, its levers, and what is known about each. Read before any session. Short on purpose: a lever gets a paragraph; anything longer is a link to a results directory. Threads are the transcript; this file is the state.*

*Lines marked `[unsettled]` are questions, not facts. Who may change which line is in [`AGENTS.md`](AGENTS.md).*

## 0. The premise

Context is a working set, not a transcript. Locally, prefill is the only cost that scales badly with the working set; decode is flat. So the game is prefix stability and a small, curated working set, with durable artifacts replacing the disposable transcript.

From the frontier side, KV, not compute, decides how many developers a node serves. **The working-set discipline is the admission condition for self-hosting, not an optimisation.**

## 1. The components

- **The trunk.** The session's single prefix: system, tool definitions, the project prefix, the conversation. Frozen except at its tail. The trunk carries settled turns only; a turn that stops at `max_steps` (#29) or fails after its tool steps (#541) keeps the commands it ran on the trunk; a cancelled turn keeps none.
- **Working memory.** A typed object in program memory — goal, constraints, decisions, open questions, next steps, gotchas, files — mutated by classical code from patches. The context is only ever a render of it (`diet/src/seam/render.rs`).
- **Forks.** Disposable one-turn sessions born off the warm trunk in an idle gap; each answers one templated ask (`diet/dogma/templates/`), emits a patch, and is never continued. The dogma pins every ask by digest (`diet/dogma/MANIFEST.tsv`).
- **The seam.** A declared boundary at which the trunk is rebuilt from the head plus the rendered working memory, carrying no turns (#493, #505). **Total compaction by design: the refilled session is a better starting point each time — a hill climb — not an imitation of continuity.** Refused as `nothing-to-seam` when working memory is empty.
- **The record.** The log (`diet/formats/log`, append-only, sequence-numbered) is the source; the record is projected from it; a results directory is the unit of evidence, digest-pinned, hygiene-scanned, recomputable.
- **The regimen.** A TOML file naming the arm, the dogma version, the substrate, the sampler, the isolation, and the approval policy a session is held to. `diet check-regimen` reads one.
- **The substrate.** A served model on a registered machine: engine, weights, template, serving line, reasoning state as rendered. Its served configuration is declared by whoever runs the test, and serve is to corroborate each field the engine can report, refusing only on a contradiction (duty of care, ruled 2026-10-07; not built until #509 lands, so serve still requires llama.cpp's `build_info`). Whether it is fit for a claim is measured, never declared (§4). **Two kinds of server** (the maintainer, 2026-10-09): local, under our governance, which is confirmed up, loaded with the right model, warmed, and running the settings we require; and an API, a server we don't control and can't reason about, where nearly everything is declared. The shapes differ enough that "local" and "API" are an explicit kind wherever they matter. The API kind is wanted, not yet built.
- **`exercise`.** The reference surface: the Claude-app-shaped harness with the curtain pulled back — ask, tools, approval, image, forks, seam, cancel/end, record. v0.1.0 is each of those shown working live on the floor; a passing test does not count (`docs/releases.md`).

## 2. The levers

Each lever has states; the current build sits at one of them; the experiments move them.

| lever | states | current | notes |
|---|---|---|---|
| **compaction depth** | none · one entry · *n* entries · total | total (#505) | `[unsettled]` whether partial compaction is ever better than total; the 0-1-n sweep is the experiment |
| **seam trigger** | operator-declared · model-proposed, operator-ratified · cadence · budget | declared | walk before run; model-proposed is #124 (v0.2.0) |
| **fork warrant** | every gap · one per gap, gated on prior turn · none | one per gap, gated (#374) | a generative interview at a null step confabulates (`results/2026-07-29-confabulation-on-nulls/`) |
| **fork delivery of results** | advisory · imperative · none | — | measured: advisory is ignored (1/99), imperative is obeyed (~50%) and **capability-independent** (#142); the collector-as-nominator is dead in both framings (#17) |
| **tool-output disposition** | keep · replace with reference · capture salient line · evict | keep | the token-carriage metric (#31) is what would say what keeping costs |
| **isolation** | `none` · `sandbox` (Seatbelt on macOS, `bwrap` on Linux) · `vm` (refused by serve) | `sandbox` | "accident containment, not adversarial security" (`diet/src/isolation/mod.rs`); the approval layer is frozen (its hardening tickets closed as not planned in the grooming, #495) |
| **approval** | denylist + prompt (once / session / workspace) · pre-seeded allow set · none | denylist + prompt, over the regimen's pre-seeded set (T1's draft seeds `ls`, `cat`, `rg`, `node`, `npx`, `chrome-devtools`) | the approval trace is a measurement only if the operator's policy is a declared sentence in the regimen (#29) |
| **reasoning state** | thinking on/off · effort as *rendered* · budget | floor's as registered | eligibility is on the rendered state, never the requested one (#143) |
| **cache lifetime (hosted)** | 5 min · 1 h · per-breakpoint decision | — | the hour pays when P(next request > 5 min) > ~0.6; read `cache_ttl` from the response, never the request (#79) |
| **substrate rung** | small CPU model · the 3.8 27B floor · Flash-Next on the Ada box · a laptop engine | the 3.6 (`accel24-beellama-qwen27b-q4kxl`) in the registry; the target is the 3.8 EXL3 line, `accel24-tabbyapi-exl3-qwen38-27b-3p00` (#497) | the floor must be always available; a stronger model on a shared box is a top rung, not a replacement. Rungs, candidates (the admitted candidate `accel24-llamacpp-qwen38-27b-iq3s` among them) and parked engines are in `substrates/LADDER.md` |

## 3. Cross-cutting concerns

- **Prefix stability.** Every mechanism above is judged by whether it leaves the prefix byte-identical; the cache tripwire (`expected_cache_n − cache_n`) is the instrument, so far only in an archived grader (`results/2026-08-10-extraction-acceptance-inverts/`), not in `diet`. Harness compaction, effort switches, hidden injections re-voiced by templates, and reasoning dropped between tool calls are the known prefix-mutation classes (#79).
- **Framing.** Anything delivered to the model as an instruction is acted on about half the time regardless of model capability; anything advisory is ignored. Precision beats recall for everything that reaches the model (#142, #114).
- **Provenance.** Every number carries its regime (`results/AGENTS.md`); a claim's rule ratified after its window opened makes it post-hoc (`scripts/check-results.py`).
- **Hygiene.** Identity is a person or a host, not a shape; a scanner cannot read an image, so images carry an author's declaration (#372).
- **Distribution matching.** The harness presents tools in the shape the model was trained on and never forbids what the model is trained to do first (`git status`). The fenced-code-block era confounds every mimicry number before native tool calls (#20).

## 4. Substrates and admission

The admission rule and its cell words are in `substrates/LADDER.md` (#143), derived by `substrates/admission/derive_admission.py`. The maintainer has ruled that the parity fire is not part of the rule: it reproduces a band taken from the 3.6's own draws, so it bars every successor by construction. Until #498 lands, `derive_admission.py` still reads the parity word.

**What is known about formats:** quant fitness is format × backend. EXL3 at 2.05 bpw beats Q2_0 at a smaller size on a CUDA card, at about half the KLD; pruning (the Coder build) costs prose quality (`substrates/LADDER.md`). That tiered placement frees the same memory without that cost is #199's open claim, not a result.

## 5. Hazards, collocated

- **Seam:** an incongruity between working memory and anything else in context is discoverable only at a seam; operator edits must be surfaced there before the refill (#150). Supersession is rare (14 reversals in 18 sessions: #132; planning on [#24](https://github.com/ironblock/discipline/issues/24#issuecomment-5972991175)) and can only be advisory.
- **Forks:** a fork that sees only turn N reconstructs a thinner plan than the model held — `next_steps` is the leakiest slot. Interview mimicry rates are unmeasured under native tool calls.
- **Record:** three things the log cannot show — a rewritten instance id, a provider's billed TTL from the request side, a transient compute-path fallback — are declared hazards with their detectors named.
- **Serving:** a draft head silently moved to the host by `-fit`; quantized KV disabling sparse attention; a unified-cache concurrency bug fixed 2026-09-30 23:20Z (a fingerprint boundary); a model-scoped cache-retention control without the billed split is a label.
- **Process:** a backlog is the agents' environment and agents optimise it; a thread is a transcript nobody compacts. This file is the compaction.

## 6. Unsettled

- `[unsettled]` Compaction depth: is total ever worse than partial? (the 0-1-n sweep)
- `[unsettled]` The seam's audit grammar and whether v0.1.0 needs it (#504)
- `[unsettled]` Whether `recompute` and `admission` stay as manual commands (#508)
- `[unsettled]` The 3.8 registry entry's hazards line ("no /props or /slots… no draft_n") disagrees with TabbyAPI's source at `be74bf0` as read for #509 (outside this repository): a `/props` route without `build_info`, and `draft_n` in `timings` when a draft ran. `/v1/model` itself reports no version, commit or draft. Which is wrong is not established; the line is the maintainer's to correct.
- `[unsettled]` A laptop rung (Splash): a candidate, unmeasured. (Strata is parked by the maintainer, `substrates/LADDER.md`.)
