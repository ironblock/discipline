# R2c brief: goal and constraints only

This is the brief given to the blind proposer for R2c, under the mediated-Socratic protocol (#25). It holds the goal and the constraints and nothing else; no preferred approach is included.

## The item: R2c, the HTTP + SSE surface of the interactive drive loop

**The goal, from the ruled rows (quoted verbatim):**
- **R2 row:** "Interactive drive: an ask from a person; the trunk appended until a seam; a settlement state machine (`awaiting | turn | capture | ended`, a prompt refused while work is in flight); cancel that reaches an in-flight call; served over HTTP + SSE with an append-only event log as the source of truth. llama.cpp only." It unblocks DoD 1 and 2.
- **R2c, ruled 2026-09-26:** "HTTP + SSE: replay from a sequence then tail; clients dedupe by sequence; loopback by default, optional constant-time Basic auth."
- **The model-driven tool loop row, ruled 2026-09-26** (a separate item, but these parts reach the surface): "`turn.settled` carries a reason (`final | cancelled | max_steps | timeout | failed`); a prompt during capture is refused (409), never queued; a stale cancel is blocked by an admission counter".
- **Ruled 2026-09-26:** "The idle-gap instrument becomes a log event: `idle.gap { notice, read, compose, away }` from the surface, into the log."

**What already exists on `main`:**
- R2a: `diet/src/drive/session.rs`, the session with its settlement state machine, its in-memory event log, the trunk and cancel.
- R2b: `diet/src/client/stream.rs`, a streaming llama.cpp transport whose cancel shuts the socket.
- Today the log is in-memory `Event` / `Logged` values, and nothing serializes it.

## Constraints

- **Definition of done** (one screen, one session; the author's, ratified):
  1. The author types an ask. The trunk answers, streamed.
  2. The model runs bash. The tool call and its output are visible, collapsed by default.
  3. An interview runs in the idle gap while the author reads, and a patch lands in working memory where it can be seen.
  4. A phase transition happens: the author declares it.
  5. The author sees the trunk refill from working memory, drawn as the one deliberate prefill event.

  "Every requirement on `exercise` and on `diet` is ranked by which of these steps it unblocks. Anything that unblocks none of them waits."
- **The log is a format (ruled 2026-09-26):** "The session event log is a format, `diet/formats/log`: append-only, a sequence number as primary key, versioned and conformance-tested like every other format. Two consumers (the record projection and the surface) make it a format by definition; the record stays the durable artifact derived from it."
  - `diet/AGENTS.md` and the existing formats under `diet/formats/` show what "a format" means in this repository: grammar, conformance corpus and reader.
  - One more observation has been recorded against the log format: the predecessor's recordings contain a `capture.cancelled` kind that has no place in the ruled vocabulary.
- **Vocabulary rulings:**
  - `slot` means a server slot and nothing else.
  - The canonical session is the `trunk`, which is also the lane name (not `main`).
  - `extraction` is a lane; the ratify step is `ratify`.
  - The tool is `bash`, taking `{command}`.
  - `provenance` is *position* (turn, lane, fork, tangent, index). `authority` is *how it was known* (`stated | extracted | observed | arm`).
  - Nothing else is named until it has to be. Do not invent identifiers where a semantic description will do.
- **Decisions already ruled:**
  - The loop lives in Rust, in `diet/src/drive/`, served over HTTP + SSE.
  - `exercise/` (a React SPA) is its surface.
  - llama.cpp only.
- **An accepted v1 default:** R2a and R2b use no new dependency and no async runtime. That default is to be "revisited only if R2c's blind proposal argues the HTTP layer needs one."
- **Ownership: track three implements this plan.**
  - Track three owns `diet/src/drive/`, `diet/src/client/`, `diet/src/seam/`, `diet/drive/`, `diet/client/`, `diet/seam/`, `diet/isolation/` and `diet/adapters/`.
  - It may **not** edit these: `diet/formats/`, `diet/capture/`, `diet/object/`, and the gate files (`verify.sh`, `scripts/`, and `tools/gate/*`, except a lane PR's own registrations in `tools/gate/faults.toml`).
  - `diet/formats/` belongs to track one, and a change there goes as a courier patch that track one applies. `exercise/` belongs to track five.
- **The consumer:** track five's R1 surface is built against "a narrow drive interface (send an ask, cancel, declare a seam, a stream of events) over a canned in-memory transport". Its current, **unmerged** shape is on branch `origin/feat/exercise-r1-surface` (`exercise/src/drive/transport.ts`, `exercise/src/drive/events.ts`, `exercise/README.md`, and what they import). It is the consumer's current expectation, not a ruling.
- **Repository rules:**
  - The root `AGENTS.md`, `diet/AGENTS.md`, `CONTRIBUTING.md` and the lane gates (`diet/<lane>/gate.toml`, used by `verify.sh`) govern the work.
  - A test that cannot fail is not a test.
  - A mechanism the plan introduces gets a seeded fault in its lane's `gate.toml`.

## What the proposer was told to produce

The proposal was to have these five parts:
1. **What exists**, with pointers.
2. **Decisions:** options and tradeoffs first, then a recommendation.
3. **The plan:** increments in order, each with what lands where and under which track, its tests and seeded faults, what it waits on, and which DoD step it unblocks.
4. **Questions for planning / the maintainer**, for everything the brief does not settle.
5. **Risks, and what it did not check.**
