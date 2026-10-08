# The program

*The system, its levers, and what is known about each. Read before any session. Short on purpose: a lever gets a paragraph; anything longer is a link to a results directory. Threads are the transcript; this file is the state.*

*Compiled by planning on 2026-10-07 from the threads (#25, #31, #117, #143, #165, #24) and planning's own notes, and adapted by Claude on 2026-10-08. No human has authored this text yet; the maintainer edits before it is relied on. Lines marked `[unsettled]` are questions, not facts; figures marked `[unsourced here]` come from records outside this repository and cannot be checked from it.*

## 0. The premise

Context is a working set, not a transcript. Locally, prefill is the only cost that scales badly with the working set; decode is flat. So the game is prefix stability and a small, curated working set, with durable artifacts replacing the disposable transcript. Measured: an identical 50K resend went 46.2 s cold → 0.10 s hot; a mid-prefix edit costs a full re-prefill, quantized to the server's checkpoints. `[unsourced here]`

The economic case from the frontier side, one developer, 4.2 months: cache reads 52.6% of spend, cache writes 35.8%; median Opus context 214–234K; 22.3% of requests beyond 262K; KV, not compute, decides how many developers a node serves. **The working-set discipline is the admission condition for self-hosting, not an optimisation.** `[unsourced here]`: anonymised aggregates in a tracker outside this repository.

## 1. The components

- **The trunk.** The session's single prefix: system, tool definitions, the project prefix, the conversation. Frozen except at its tail. The trunk carries settled turns only; a `max_steps` turn keeps its ran commands on the trunk (#29).
- **Working memory.** A typed object in program memory — goal, constraints, decisions, open questions, next steps, gotchas, files — mutated by classical code from patches. The context is only ever a render of it (`diet/src/seam/render.rs`).
- **Forks.** Disposable one-turn sessions born off the warm trunk in an idle gap; each answers one templated ask (`diet/dogma/templates/`), emits a patch, and is never continued. The dogma pins every ask by digest (`diet/dogma/MANIFEST.tsv`).
- **The seam.** A declared boundary at which the trunk is rebuilt from the head plus the rendered working memory, carrying no turns (#493, #505). **Total compaction by design: the refilled session is a better starting point each time — a hill climb — not an imitation of continuity.** Refused as `nothing-to-seam` when working memory is empty.
- **The record.** The log (`diet/formats/log`, append-only, sequence-numbered) is the source; the record is projected from it; a results directory is the unit of evidence, digest-pinned, hygiene-scanned, recomputable.
- **The regimen.** A TOML file naming the arm, the dogma version, the substrate, the sampler, the isolation, and the approval policy a session is held to. `diet check-regimen` reads one.
- **The substrate.** A served model on a registered machine: engine, weights, template, serving line, reasoning state as rendered. Its served configuration is declared by whoever runs the test, and serve corroborates each field the engine can report, refusing only on a contradiction (duty of care, #509). Whether it is fit for a claim is measured, never declared (§4).
- **`exercise`.** The reference surface: the Claude-app-shaped harness with the curtain pulled back — ask, tools, approval, image, forks, seam, cancel/end, record. v0.1.0 is each of those shown working live on the floor by the maintainer; a passing test does not count (#177).

## 2. The levers

Each lever has states; the current build sits at one of them; the experiments move them.

| lever | states | current | notes |
|---|---|---|---|
| **compaction depth** | none · one entry · *n* entries · total | total (#505) | `[unsettled]` whether partial compaction is ever better than total; the 0-1-n sweep is the experiment |
| **seam trigger** | operator-declared · model-proposed, operator-ratified · cadence · budget | declared | walk before run; model-proposed is #124 (v0.2.0) |
| **fork warrant** | every gap · one per gap, gated on prior turn · none | one per gap, gated (#374) | the floor's 18.8 side-calls per ask is the failure this lever exists to fix `[unsourced here]` |
| **fork delivery of results** | advisory · imperative · none | — | measured: advisory is ignored (1/99), imperative is obeyed (~50%) and **capability-independent** (#142); the collector-as-nominator is dead in both framings (#17) |
| **tool-output disposition** | keep · replace with reference · capture salient line · evict | keep | the router (#16) waits for the carriage metric to say what it costs |
| **isolation** | `none` · `sandbox` (Seatbelt on macOS, `bwrap` on Linux) · `vm` (refused by serve) | `sandbox` | "accident containment, not adversarial security" (`diet/src/isolation/mod.rs`); the approval layer is frozen |
| **approval** | denylist + prompt (once / session / workspace) · pre-seeded allow set · none | denylist + prompt, over the regimen's pre-seeded set (T1's draft seeds `ls`, `cat`, `rg`, `node`, `npx`, `chrome-devtools`) | the approval trace is a measurement only if the operator's policy is a declared sentence in the regimen (#29) |
| **reasoning state** | thinking on/off · effort as *rendered* · budget | floor's as registered | eligibility is on the rendered state, never the requested one (#143) |
| **cache lifetime (hosted)** | 5 min · 1 h · per-breakpoint decision | — | the hour pays when P(next request > 5 min) > ~0.6; read `cache_ttl` from the response, never the request (#79) |
| **substrate rung** | small CPU model · the 3.8 27B floor · Flash-Next on the Ada box · a laptop engine | the 3.6 (`accel24-beellama-qwen27b-q4kxl`) in the registry; the 3.8 EXL3 line on linux-pc is the target (#497) | the floor must be always available; a stronger model on a shared box is a top rung, not a replacement |

## 3. Cross-cutting concerns

- **Prefix stability.** Every mechanism above is judged by whether it leaves the prefix byte-identical; the cache tripwire (`expected_cache_n − cache_n`) is the instrument. Harness compaction, effort switches, hidden injections re-voiced by templates, and reasoning dropped between tool calls are the known prefix-mutation classes (#79).
- **Framing.** Anything delivered to the model as an instruction is acted on about half the time regardless of model capability; anything advisory is ignored. Precision beats recall for everything that reaches the model (#142, #114).
- **Provenance.** Every number carries its regime; every claim its pre-registered rule, ratified before its data (`rule_ratified` checked against `window_start`); every external number its `provenance: human | agent-assisted | agent-authored | unknown`.
- **Hygiene.** Identity is a person or a host, not a shape; a scanner cannot read an image, so images carry an author's declaration (#372).
- **Distribution matching.** The harness presents tools in the shape the model was trained on and never forbids what the model is trained to do first (`git status`). The fenced-code-block era confounds every mimicry number before native tool calls (#20).

## 4. Substrates and admission

A rung is admitted by three results: the constitutional cells (kwarg delivery with a negative control; the canary from its own `baseline` draw; checkpoint restore including recurrent state on hybrids), the depth probe at fractions of the served context, and a parity fire of the archived `extraction-acceptance-inverts` row. Cells read `pass | fail | n/a | unreported | unadjudicated | baseline`; only `fail` bars; a claim declares the cells it requires and a rung lacking one is ineligible for that claim, admitted for others. *(#143; `substrates/admission/`.)* `[unsettled]` #498: whether the parity fire leaves the admission rule.

**What is known about formats:** quant fitness is format × backend — lookup-table quants turn bandwidth-bound decode into compute-bound decode and flatten hardware differences; EXL3 at 2.05 bpw beats Q2_0 on quality and speed at smaller size on a CUDA card; pruning (the Coder build) costs prose quality for memory that tiered placement frees anyway (#199; the comparison figures are `[unsourced here]`).

## 5. Hazards, collocated

- **Seam:** an incongruity between working memory and anything else in context is discoverable only at a seam; operator edits must be surfaced there before the refill (#150). Supersession is rare (14 reversals in 18 drives `[unsourced here]`) and can only be advisory.
- **Forks:** a fork that sees only turn N reconstructs a thinner plan than the model held — `next_steps` is the leakiest slot. Interview mimicry rates are unmeasured under native tool calls.
- **Record:** three things the log cannot show — a rewritten instance id, a provider's billed TTL from the request side, a transient compute-path fallback — are declared hazards with their detectors named.
- **Serving:** a draft head silently moved to the host by `-fit`; quantized KV disabling sparse attention; a unified-cache concurrency bug fixed 2026-09-30 23:20Z (a fingerprint boundary); a model-scoped cache-retention control without the billed split is a label.
- **Process:** a backlog is the agents' environment and agents optimise it; a thread is a transcript nobody compacts. This file is the compaction.

## 6. Unsettled

- `[unsettled]` Compaction depth: is total ever worse than partial? (the 0-1-n sweep)
- `[unsettled]` The seam's audit grammar and whether v0.1.0 needs it (#504)
- `[unsettled]` Whether `recompute` and `admission` stay as manual commands (#508)
- `[unsettled]` The 3.8 registry entry's hazards line ("no /props or /slots… no draft_n") disagrees with TabbyAPI's source at `be74bf0`, which has a `/props` route without `build_info` and sends `draft_n` when a draft ran. Which is wrong is not established; the line is the maintainer's to correct.
- `[unsettled]` A laptop rung (Splash) and a Strata top rung on owned hardware: candidates, unmeasured
