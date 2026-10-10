# The program

*The system, its levers, and what is known about each. Read before any session. Short on purpose: a lever gets a paragraph; anything longer is a link to a results directory. Threads are the transcript; this file is the state.*

*Lines marked `[unsettled]` are questions, not facts. Who may change which line is in [`AGENTS.md`](AGENTS.md).*

## 0. The premise

Context is a working set, not a transcript. Locally, prefill is the only cost that scales badly with the working set; decode is flat. So the game is prefix stability and a small, curated working set, with durable artifacts replacing the disposable transcript.

From the frontier side, KV, not compute, decides how many developers a node serves. **The working-set discipline is the admission condition for self-hosting, not an optimisation.**

**The hypothesis** (the maintainer, 2026-10-09): "a small model with limited context can 'hill climb' a difficult problem by creating a better and better starting point, with git as the status and the summary as the 'continually improving brief' that each compacted session starts from."

## 1. The components

- **The trunk.** The session's single prefix: system, tool definitions, the project prefix, the conversation. Frozen except at its tail. **The system prompt is written once, at session start, and never mutated** (the maintainer, 2026-10-09: "System prompt mutation is forbidden."). It says up front how the program works: that the harness forks the session and the forks interview about certain events ("prime up front, mutate system prompt never"; #598). The trunk carries settled turns only; a turn that stops at `max_steps` (#29) or fails after its tool steps (#541) keeps the commands it ran on the trunk; a cancelled turn keeps its finished steps and answers its open calls (#577).
- **Working memory.** A typed object in program memory — goal, constraints, decisions, open questions, next steps, gotchas, files — mutated by classical code from patches. As built, an entry names its kind only by the interview tag that produced it (`decision: `, `constraint: `, …): there are no typed sections and no goal kind yet (`diet/src/object.rs`, `fold` in `diet/src/drive/mod.rs`). The context is only ever a render of it (`diet/src/seam/render.rs`).
- **Forks.** Disposable one-turn sessions born off the warm trunk in an idle gap; each answers one templated ask (`diet/dogma/templates/`), emits a patch, and is never continued. The dogma pins its templates by digest (`diet/dogma/MANIFEST.tsv`); the asks serve's forks actually send (`diet/src/capture/router/asks/`) are not pinned.
- **The seam.** A declared boundary at which the trunk is rebuilt from the head plus the rendered working memory, carrying no turns by default (#493, #505); the render goes in a `<summary>` user message after the unchanged system prompt, as Pi and OpenCode 2 carry a compaction summary (#597, #604); compaction depth can keep a tail of recent whole turns (#579). **Total compaction by design: the refilled session is a better starting point each time — a hill climb — not an imitation of continuity.** Refused as `nothing-to-seam` when working memory is empty.
- **The record.** The log (`diet/formats/log`, append-only, sequence-numbered) is the source; the record is projected from it; a results directory is the unit of evidence, digest-pinned, hygiene-scanned, recomputable.
- **The regimen.** A TOML file naming the arm, the dogma version, the substrate, the sampler, the isolation, and the approval policy a session is held to. `diet check-regimen` reads one.
- **The substrate.** A served model on a registered machine: engine, weights, template, serving line, reasoning state as rendered. Its served configuration is declared by whoever runs the test, and serve is to corroborate each field the engine can report, refusing only on a contradiction (duty of care, ruled 2026-10-07; built for llama.cpp and TabbyAPI, #546). Whether it is fit for a claim is measured, never declared (§4). **Two kinds of server** (the maintainer, 2026-10-09): local, under our governance, which is confirmed up, loaded with the right model, warmed, and running the settings we require; and an API, a server we don't control and can't reason about, where nearly everything is declared. The shapes differ enough that "local" and "API" are an explicit kind wherever they matter. The API kind is wanted, not yet built.
- **`exercise`.** The reference surface: the Claude-app-shaped harness with the curtain pulled back — ask, tools, image, forks, seam, cancel/end, record. v0.1.0 is each of those shown working live on the floor; a passing test does not count (`docs/releases.md`).

## 2. The levers

Each lever has states; the current build sits at one of them; the experiments move them.

| lever | states | current | notes |
|---|---|---|---|
| **compaction depth** | none (never seam) · a tail of *N* tokens kept after the refill · total (*N* = 0) | total by default; `seam_tail_tokens` keeps whole turns, newest first, up to *N* (#579) | depth is measured in tokens kept (chars/4, as all three other harnesses estimate). The default stays 0, because Pi (~20,000) and OpenCode 2 (~8,000) differ and Qwen Code keeps none. A tail starts at a user message, since Qwen's template needs one. `[unsettled]` whether partial compaction is ever better than total; the 0-1-n sweep is the experiment |
| **seam trigger** | operator-declared · model-proposed, operator-ratified · cadence · budget | declared, cadence (operator turns) and budget (a share of the context window) (#520) | derived seams fire on their own; model-proposed is #124; operator-chosen phases are #563 |
| **fork warrant** | every gap · one per gap, gated on prior turn · none | one per gap, gated (#374) | the predecessor reported that a generative interview at a null step confabulates (`results/2026-07-29-confabulation-on-nulls/`); its intent, not a result here |
| **fork asks** | named sets of asks, pinned by digest; a regimen picks one | one unpinned set, the router's asks (`diet/src/capture/router/asks/`), additions only | the maintainer, 2026-10-09: "treat all of this as somewhat speculative and be prepared to change the list, allow different asks in different regimens". Pinning the asks serve sends, and a first new set adding SUPERSEDE to the scoping ask, approved the same day; resolve, retire and park are not asked for yet (#595, #562) |
| **self-capture** | off · on, with a reminder after N silent turns | off: serve never offers the tools (`diet/src/capture/tools.rs`, built 2026-09-04, unwired) | the model updates working memory through tools offered from session start (`update_record`, `resolve_entry`, `propose_phase_transition`), through the grounding check. The maintainer, 2026-10-09: a tool call is the in-distribution way for any model to emit schema-shaped markup, its description explains the program in context, and self-elected updates give a cadence to measure; #609 |
| **capture modality** | prose fields parsed from text · tool calls | prose fields | how an interview fork answers. The fields approach needed heavy parsing because every model formats a little differently; the template already parses tool calls; #610 |
| **model-elected pruning** | off · a tool the model calls to prune a large tool result | off | the whole stays saved by digest (#578); the mechanism behind #149's claim; #612 |
| **fork delivery of results** | at the seam · advisory · imperative | at the seam; advisory and imperative are built (#550) | measured on *false* nominations (#114, #142): imperative framing made the model accept the false nomination 53/99, advisory 1/99, sham 0, and that held on a second substrate. Advisory replies engaged the nominated entry 327/594 times and declined it 171. How often either framing is followed on a *true* nomination is unmeasured. New entries reach the trunk only at a seam (#550, the maintainer 2026-10-09); forks emit only `add` until an ask asks for more (#562, #595); the collector-as-nominator is dead in both framings (#17) |
| **tool-output disposition** | keep · replace with reference · capture salient line · evict, each at the seam · cap on arrival | cap on arrival, by default: 2,000 lines / 50 KiB, the head fifth and tail four-fifths kept, the whole output saved by its digest (#578) | the cap's limit, shape and notice follow the other harnesses by vote (Pi and OpenCode agree on the limit; Qwen Code breaks the ties); the seam states act on the outputs a seam compacts away, deciding what the refill carries of each (#553); the token-carriage metric (#31) is what would say what keeping costs |
| **isolation** | `none` · `sandbox` (Seatbelt on macOS, `bwrap` on Linux) · `vm` (refused by serve) | `sandbox` | "accident containment, not adversarial security" (`diet/src/isolation/mod.rs`); the approval layer is frozen (its hardening tickets closed as not planned in the grooming, #495) |
| **approval** | denylist + prompt (once / session / workspace) · pre-seeded allow set · none | none for T1 (#544, #545) | the layer is frozen and its removal is filed for after publication (#540); with none, every command runs, and the record says approvals were off |
| **reasoning state** | thinking on/off · effort (xhigh · medium · low for Qwen3.8) · budget | on, xhigh for T1 (#547) | one effort per session; a fork inherits it, since effort rewrites the system prompt. Reasoning in history follows the model's convention and is not a lever (§3). No Qwen3.8 template has a budget variable, so a declared budget is recorded as not sent (#531) |
| **cache lifetime (hosted)** | 5 min · 1 h · per-breakpoint decision | — | built last, after the API server kind (#555, #556); read `cache_ttl` from the response, never the request (#79); the ~0.6 break-even in earlier drafts has no source in the tree |
| **substrate rung** | small CPU model · the 3.8 27B floor · Flash-Next on the Ada box · a laptop engine | the 3.8 EXL3 line, `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2 (#517); the 3.6 is retired | the floor must be always available; a stronger model on a shared box is a top rung, not a replacement. Rungs, candidates (the admitted candidate `accel24-llamacpp-qwen38-27b-iq3s` among them) and parked engines are in `substrates/LADDER.md` |
| **tool surface** | bash only · the standard set (edit, write, read with images, grep, glob) | bash, read, write and edit (#580) | every other harness offers the standard set (`docs/harness-baseline.md`); #557 |
| **instruction files** | AGENTS.md discovered and injected · off | off | the other harnesses inject them; #559 |
| **tangent closure** | keep · drop · park, per entry | — | #22 |
| **capture modality** | prose · schema · tool | — | #18 |
| **interview routing and cadence** | per tool-output class · per call · at the turn boundary · an evidence threshold · skip what the model just recorded itself (#611) | one per gap, gated | the predecessor noted capture as cliff-shaped, a 1.5× threshold change moving capture 7× (#16); its intent, not a result here; #564 |
| **render budget** | none · tier or elide old pointers past a size | none | #565 |
| **fork memory share** | a main slot, with forks borrowing a bounded tail | — | #406 |
| **archive recall** | literal · embedding · off | off | embedding added nothing in the bakeoff; #566 |
| **fork input view** | the whole warm trunk · the last turn · the last *N* turns | the whole warm trunk | a narrower view pays a prefill; #567 |
| **interview role** | user · system · developer, where the served template supports it | user | the maintainer, 2026-10-09: "consider asking the interview question as something other than 'user' role when supported". The floor's Qwen3.8 template allows a system message only at the beginning and has no developer role, so only user runs there. Replaces the fork delivery site lever (#568), whose position-0 state appended to the system message. the predecessor's #21 kept the ask last and moved only an instruction: at the end of the ask it scored +0.222 (p 0.0023); a priming paragraph in the system message scored +0.056, a sham's score, unless the ask was also wrapped in `<interview>` tags (+0.222). Its intent, not a result here; #599 |
| **step and output limits** | `max_steps` · the output cap, as regimen settings | flags | an 8,192-token cap killed an xhigh write step on the 3.8; T1 runs 32,768; #569 |
| **extraction seat** | the warm model · a small offboard model · an encoder | the warm model | #570, and #499's claim |
| **failed turns on the trunk** | the commands a failed turn ran are kept · dropped | kept (#541) | a cancelled turn keeps its finished steps (#577) |
| **subagent** (later) | fired by the harness (the interview fork) · called by the model | the harness | Qwen Code exposes an `agent` tool; #561 |

## 3. Cross-cutting concerns

- **Prefix stability.** Every mechanism above is judged by whether it leaves the prefix byte-identical; the cache tripwire (`expected_cache_n − cache_n`) is the instrument, so far only in an archived grader (`results/2026-08-10-extraction-acceptance-inverts/`), not in `diet`. Harness compaction, effort switches, hidden injections re-voiced by templates, and reasoning dropped between tool calls are the known prefix-mutation classes (#79).
- **Framing.** On false nominations, an instruction was accepted about half the time on both substrates measured, and an advisory note almost never, though advisory replies did engage with the entry (#114, #142). How either framing is followed on a true nomination is unmeasured. Precision beats recall for everything that reaches the model.
- **Provenance.** Every number carries its regime (`results/AGENTS.md`); a claim's rule ratified after its window opened makes it post-hoc (`scripts/check-results.py`).
- **Hygiene.** Identity is a person or a host, not a shape; a scanner cannot read an image, so images carry an author's declaration (#372).
- **Distribution matching.** The harness presents tools in the shape the model was trained on and never forbids what the model is trained to do first (`git status`). The fenced-code-block era confounds every mimicry number before native tool calls (#20). **Reasoning follows the model's own convention, and is not a lever** (the maintainer, 2026-10-09). Qwen keeps its reasoning in history (`preserve_thinking`), and 90–95% of its tokens are chain of thought, not prose. Without the reasoning preserved it can loop: if its last thought was "next, B", it attends back to that after the tool result and thinks "now, B" even when the result *was* B, a discontinuity in its trajectory.

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
