# T1 in phases on the floor — agent-driven

**Who drove it:** an agent (the operator seat), acting as operator through `diet-drive`'s MCP tools against `serve`. Not the maintainer. The operator's screenshot helper was Dispatch's tooling. This run is evidence only once the maintainer approves it.

**Purpose:** v0.4.0's done-when (`docs/releases.md`): "T1, driven on the floor in phases, reaches step 8's feedback loops without the window ending a turn."

**The answer: yes, with the confounds below.**
- T1 reached step 8's feedback rounds (turns 9–17) in phases, over 17 turns and about 3 h 27 min.
- Every turn settled `final`, so no turn ended on the window.
- Seven window meditations refilled the trunk instead, two of them inside the 130-step build turn.

It did not show a working feature. The section **What it did not show** says why, and what of that belongs to the harness rather than the model.

## Setup

- **Build:** `033d06bbd22b4bf713e88e3eec4f7f67d5e7d7f9`, as the log's `session.start` names it.
- **Floor:** `accel24-tabbyapi-exl3-qwen38-27b-3p00`, config r2: a 163,840-token pool, two slots, vision on. Serve checked the engine and its served configuration at start (the start row's `served`). The trunk ran under a 32,768-token output cap, clamped to the window (`step_and_output_limits`).
- **Regimen:** `regimen.toml` here.
  - **Phases:** the starter library, starting in `orient`.
  - **Capture:** self-capture on, mechanical capture on, and interviews during the turn under the scoping, evidence, trouble and open warrants.
  - **Meditation:** a 4,000-token tail, tool outputs carried by reference, and a 40,000-token render budget. The audit, the warm and literal archive recall are on.
  - **Reconcile gate:** `carry`.
  - **Sandbox:** writes are confined to the worktree and `~/git/reference/Babylon`, the writable reference.
- **Workspace:** T1's (`docs/trajectories/t1.md`): an empty worktree, with Babylon.js, Babylon Lite and their docs as references beside it. The script asks the model to turn Babylon Lite's example into a Quake-style FPS with destructible terrain, then plays the feedback loop of step 8.

**The start row** (`session.log` line 1, `levers`) names:
- `approval: none`;
- `archive_recall: literal`;
- `background_commands: on`;
- `bash_timeout: 120000ms`;
- `cache_lifetime: undeclared`;
- `capture_modality: fields`;
- `compaction_depth: tail:4000`;
- `extraction_seat: warm-model`;
- `failed_turns_on_the_trunk: kept`;
- `fork_asks: v4:bbaac02236b3146c`;
- `fork_backlog: queued:3:default`;
- `fork_delivery: meditation`;
- `fork_delivery_site: tail`;
- `fork_input_view: trunk`;
- `fork_memory_share: tail:8192:default`;
- `fork_warrant: one-per-gap-gated:scoping+evidence:3000-chars:2-per-step+trouble:cooldown-3-steps+open:every-10-steps`;
- `instruction_files: on`;
- `interview_role: user`;
- `interview_routing_and_cadence: gap:during-turn`;
- `isolation: sandbox`;
- `mechanical_capture: on`;
- `meditation_audit: on`;
- `meditation_trigger: operator-declared+window+model-proposed`;
- `meditation_warm: on`;
- `model_pruning: off`;
- `operator_unilateral: on`;
- `phase_graph: library:starter;start:orient;phases:orient,build,plan,quick,test;transitions:orient>plan,build>plan,build>test,plan>build,plan>orient,test>build`;
- `reasoning_state: on:effort:xhigh`;
- `reconcile_gate: carry`;
- `render_budget: 40000:tier`;
- `self_capture: on:every:3:c1:1726eab1c3bd4722`;
- `step_and_output_limits: max_steps:none:default:output_cap:32768:flag:clamped:163840:reported`;
- `stream_deadline: idle:300000ms`;
- `subagent: harness`;
- `substrate_rung: accel24-tabbyapi-exl3-qwen38-27b-3p00`;
- `tangent_closure: operator`;
- `tool_call_text_fallback: off`;
- `tool_output_disposition: cap-on-arrival:2000-lines:51200-bytes+meditation:reference`;
- `tool_surface: standard`;

## The run

| turn | ask (`seq`) | trunk requests | tool calls | forks | settled |
| --- | --- | --- | --- | --- | --- |
| 1 | 1 | 2 | 1 | 3 | `final` |
| 2 | 643 | 6 | 9 | 4 | `final` |
| 3 | 4325 | 8 | 15 | 6 | `final` |
| 4 | 10588 | 3 | 7 | 5 | `final` |
| 5 | 19452 | 7 | 16 | 8 | `final` |
| 6 | 33412 | 130 | 161 | 56 | `final` |
| 7 | 96944 | 3 | 2 | 3 | `final` |
| 8 | 97598 | 1 | 0 | 1 | `final` |
| 9 | 99287 | 30 | 30 | 19 | `final` |
| 10 | 115016 | 39 | 38 | 21 | `final` |
| 11 | 136683 | 5 | 4 | 6 | `final` |
| 12 | 142604 | 11 | 11 | 8 | `final` |
| 13 | 148455 | 13 | 12 | 12 | `final` |
| 14 | 157145 | 16 | 15 | 12 | `final` |
| 15 | 171620 | 8 | 11 | 11 | `final` |
| 16 | 185705 | 6 | 11 | 8 | `final` |
| 17 | 197946 | 9 | 10 | 5 | `final` |

Seqs from `diet log find session.log kind=<kind>`.

**orient** (turns 1–4)
- Turn 1: the model is asked whether it knows Babylon.
- Turn 2: it reads Babylon Lite's docs and states how Lite differs.
- Turn 3: the FPS ask. The model proposes `orient → plan` (`phase.proposed` 9651: 16 steps, 42,752 tokens). The operator rules **stay** (`phase.ruled` 10587), because the model's own scope question was still open.
- Turn 4: the scope (noise terrain, bots first, multiplayer later, a scoping ask). The model records it with `update_record` unprompted, then proposes again (`phase.proposed` 12441: 19 steps, 47,815 tokens). The operator rules **meditation** (`phase.ruled` 14667), and the move goes through `meditation` 19451 (`reason: phase`).

**plan** (turn 5)
- The model plans tile-removal destruction, records the plan, and proposes `plan → build` (`phase.proposed` 26287: 7 steps, 28,089 tokens).
- Ruled **meditation** (`phase.ruled` 28767). The move goes through `meditation` 33411.

**build** (turn 6, one ask, 130 trunk requests)
- The model writes the game: a lab scene and its terrain, input and audio modules. It lints, typechecks, and starts the lab's dev server.
- Two **window meditations** fire inside the turn (66143, 91930), and the turn goes on.
- It proposes `build → test` (`phase.proposed` 94730: 130 steps, 34,933 tokens). Ruled **meditation** (`phase.ruled` 96940), the move through `meditation` 96943.

**test** (turns 7–17)
- Turn 7: the model is asked to start its dev server.
- Turn 8: "ready to test", with a screenshot attached.
- Turns 9–17: step 8's **feedback rounds**. Each ask reports what the operator's screenshot showed: the scene stuck on "Initializing Babylon Lite", then the init markers where it stopped. The model debugs inside `test`, as its ground rules allow ("fix what fails"), and proposes no move:
  - stage markers, a navmesh fix, parallel wasm loading;
  - a production build and an on-page progress overlay;
  - a `WebAssembly.instantiateStreaming` patch, test pages;
  - a Vite middleware serving the Recast build raw.
- Five more **window meditations** fire (turns 9, 10, 11, 13 and 15).
- The operator stopped after turn 17: four rounds in a row (14–17) with no visible change on screen.

## What it showed

**No turn ended on the window.**
- All 17 turns settled `final`: `turn.settled` 640, 2886, 9652, 12442, 26288, 94731, 97285, 98065, 113545, 135186, 142578, 146416, 154617, 167745, 182972, 196050 and 202896.
- r1 on the floor died in its build turn on the window; here the build turn ran 130 steps and settled.

**The phase moves**, all four proposed by the model, unprompted (`kind=phase.proposed`, `kind=phase.ruled`):

| # | move | proposed (`seq`) | steps / tokens | ruling (`seq`) | meditation |
| --- | --- | --- | --- | --- | --- |
| 1 | orient → plan | 9651 | 16 / 42,752 | stay (10587) | none |
| 2 | orient → plan | 12441 | 19 / 47,815 | meditation (14667) | 19451 |
| 3 | plan → build | 26287 | 7 / 28,089 | meditation (28767) | 33411 |
| 4 | build → test | 94730 | 130 / 34,933 | meditation (96940) | 96943 |

**The window meditations** (`kind=meditation reason=window`, window 163,840):
- There were **seven, not five as the operator's notes count**. Two fired inside the build turn, which the notes missed.
- Each fired before a trunk request whose sized prompt crossed the window's line.
- Under #813 the running turn is split: its ask, and its most recent steps, kept; its earlier steps compacted.

| `seq` | turn | sized prompt | turn steps kept | turn steps compacted |
| --- | --- | --- | --- | --- |
| 66143 | 6 | 133,344 | 16 | 44 |
| 91930 | 6 | 131,076 | 24 | 47 |
| 109836 | 9 | 133,273 | 9 | 3 |
| 117142 | 10 | 135,534 | 4 | 5 |
| 137509 | 11 | 146,825 | 1 | 0 |
| 151336 | 13 | 131,765 | 3 | 0 |
| 171624 | 15 | 132,346 | — | — |

The last one fired at turn 15's ask (171620), before the turn's first step, so it had no turn steps to keep or compact.

**The largest prompt was sized, not sent.**
- **Sized:** 146,825 of 163,840 is the prompt serve sized before turn 11's next request. The window meditation at 137509 fired on it, and the request went out on the refilled trunk.
- **Sent:** the largest prompt sent and measured was 131,290, at `response` 137504, which the engine prefilled whole (`cache_n` 0).

**The trunk's cache above 120k.**
- 52 trunk calls carried a prompt over 120,000 tokens.
- Of their 6,517,000 prompt tokens, 6,327,040 came from the cache: **97.1%**.
- Across all 297 trunk calls the rate is 96.3%.

**Self-capture, unprompted:**
- `update_record` in 11 of the 17 turns (2, 4–6, 9, 10 and 13–17; `kind=capture tool=update_record`). The operator's notes date the first call to turn 4; the log has one in turn 2.
- `resolve_entry` in turns 5 and 6;
- `propose_phase_transition` four times (turns 3, 4, 5 and 6).

## What broke

## What it did not show

- **No feature was seen running.** Nothing of the FPS was ever seen rendering. The code exists: a lab scene with noise terrain and tile removal, a Havok character controller, hitscan, Recast navmesh bots, a GPU-text HUD and Web Audio. It typechecks and lints. The confound: the operator's screenshot helper (Dispatch's tooling) captured after a fixed 8 seconds, and in that window it could not load even stock lab scenes from the dev server (scene2, scene40 and scene170 all stayed on the spinner). So the rounds' verdict, the Recast glue module never loading, says as much about the test browser as about the model. A rerun with a longer capture window would tell them apart.
- **The example was not found.** The model read "their example" as the getting-started doc's example and a few lab scenes, and never found Babylon Lite's Minecraft example. Its own worktree's name was a hint it never used.
- **The model built inside the reference checkout,** not its worktree:
  - **Writes:** every file of the game; two test pages; edits to `lab/vite.config.ts`; and temporary markers in `packages/babylon-lite/src/navigation/navigation.ts`, against its own "zero engine changes" decision.
  - **Commands it ran there:** `lint:fix` repo-wide, and `vite build`.
  - **Left behind:** scratch directories.
  - The regimen made the reference writable, so the sandbox allowed all of it. The receipt's `reference_modified` is `{}`, which is wrong: the writable path is a directory of checkouts, which it did not judge (#830). The changes themselves are kept privately (a patch and a tarball) and are not committed.
- **The images.** Whether the model saw turn 8's attached screenshot is inconclusive. Its turn-9 reply reasons from that turn's attachment (the spinner, no error box), so it read that one.

## Earlier attempts

Not committed, and cited by name only:
- the Flash-Next instance's attempt 1 (`ada48-tabbyapi-exl3-qwen38flashnext-2p05`), contaminated by a sibling worktree;
- the Flash-Next instance's image non-op;
- floor run 1, which ended on the window;
- floor run 2, stopped for #826.

## What broke

Nothing in the session: every turn settled `final`, and no window meditation was held (the reconcile gate is `carry`). One ask was refused `400 unattachable`: the operator had named a placeholder digest, and nothing was sent. The session ended at the operator's stop.

**Counts:** 17 asks, 479 requests, 353 tool calls, 188 forks (2 audit, 186 interview), 877 patches, 0 flags, 10 meditations, 0 meditations held, 0 refusals.

| kind | lines |
| --- | --- |
| `ask` | 17 |
| `background.ended` | 13 |
| `capture` | 42 |
| `delta` | 200616 |
| `fork` | 188 |
| `fork.settled` | 188 |
| `fork.skipped` | 246 |
| `meditation` | 10 |
| `notice` | 7 |
| `patch` | 877 |
| `phase.noted` | 1 |
| `phase.proposed` | 4 |
| `phase.ruled` | 4 |
| `progress` | 1266 |
| `recalled` | 8 |
| `reminded` | 3 |
| `request` | 479 |
| `response` | 479 |
| `session.start` | 1 |
| `settlement` | 58 |
| `tool_call` | 353 |
| `turn.settled` | 17 |

## Scrubbed

- `product.txt`: personal-home-path (79).
- `record.jsonl`: account-name (45), personal-home-path (3513).
- `session.log`: account-name (108), personal-home-path (5635).

Home paths are written `~`, scratch paths `/tmp/scratch/` and the account name `user`, by `scripts/scrub.py` with the hygiene check's own patterns. The regimen is byte for byte.

## Files

`diet check-log` accepts `session.log` at this build.
`diet check-record` accepts `record.jsonl` at this build.

| file | what it is | sha256 |
| --- | --- | --- |
| `session.log` | the session's log, scrubbed | `1f4adf99ad2c332deb7d3c5d292f097e4d4325e6071002412edb2f484f2fd3a0` |
| `record.jsonl` | the record serve wrote at end, scrubbed | `e2d55e263fd85f5920d18e452617d8ad4b82cef7258a931d6d7428d5ab62bb43` |
| `product.txt` | the working memory at end | `9e522da20120ed056dcf8122387e607f110821b2f0920563569de65b44ce066b` |
| `receipt.json` | the receipt | `b87deeb93882740bd8eaa62c7bdfbe67f482c075d59966d726b1f3509879bd3e` |
| `unspellable.json` | what the record could not spell | `2fb854f773f37bda11b1d8bed3b2e462cc82a02a5d57569dc62fa68f16deb846` |
| `regimen.toml` | the regimen as it ran | `db3e41f63113cd114bad5a773fe13b0419319136d02b2e457a5a1ff3688ee62e` |
