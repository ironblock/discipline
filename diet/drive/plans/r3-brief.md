# Brief: R3, per-request timings and cache telemetry into the log and the record (#117)

This is the goal, the constraints and the definition of done, and nothing else. No preferred approach is included. It is cut from the ruled rows on #117 and the surface's recorded asks; where a sentence is quoted it is the ruling's own.

## The item

**The R3 row, ruled (quoted verbatim):** "Per-request `timings` and cache telemetry into the record via the journal → record projection (each item `client/journal.rs` lists as *unspellable* becomes a demand-driven record bump); streaming consumed by the surface." It serves **DoD 1 (stats)**.

**The re-ranking ruling on the same issue (quoted):** "Record changes (#82, #92) are re-ranked by the DoD: the ask's prose, per-message timings and cache counts, and a slot id are blocking; patches and the seam's reason and phase are needed at steps 3–5; the rest waits for a session that wants it. They are requested on demand, as the projection meets each unspellable item, not filed as a batch."

**The surface's recorded asks for DoD 1 (track five, 2026-09-28), which this increment set is where they land or are refused:**
- per-request `timings` on the response: `prompt_n`, `cache_n`, `prompt_ms`, `predicted_n`, `predicted_ms`;
- prompt progress while prefilling: `total`, `cache`, `processed`, `decoded`;
- `calls_from` on a response: the tokens and milliseconds generated before the first tool-call chunk;
- the whole `response.reasoning` on the response (reasoning deltas stream since I5r; the response line carries text only);
- a `request.failed` reason `context_overflow` (v0's reason set is closed);
- the system prompt's token count (the start line carries the head's text only).

## What exists on `main`

- Log format v0 (`diet/formats/log/`, its grammar, fixtures and one reader; the generated TypeScript bindings), with `session.start`, `ask`, `request`, `settlement`, `refused`, `delta` (text or reasoning), `stop.asked`, `response`, `cancelled`, `request.failed`, `turn.settled`, `idle.gap`; `t` on every line; references by seq.
- The session and server (`diet/src/drive/session.rs`, `serve.rs`), `diet-drive serve`, the streaming transport (`diet/src/client/stream.rs`) with typed pieces, the outbound credential, Basic auth and `--listen`.
- The record format (`diet/formats/record/`) with its own `response` carrying `to_request`, and the record's timing and substrate rows as #82 and #92 describe them; `client/journal.rs` and its list of items it calls unspellable.
- The measured Q10 capture of a thinking turn from the DoD 1 instance, with `timings` in the server's final chunk as the server sends them.

## Constraints

- **The log is a format, and a change to it is a versioned bump**, drafted by track three and applied by track one as a courier patch, never edited in place: grammar, fixtures (valid and invalid, each invalid with a `.reason`), one reader, the generated bindings regenerated, the conformance suite. v0 fixtures keep parsing under the reader that reads the bump, or the bump says why not.
- **Nothing is invented:** every number is the server's own (llama.cpp's `timings` object and its prompt-progress frames as the server emits them), named as the server names it or as the record already names it, with the precedence rule the R2c plan adopted (a ruling, then the record, then the session's tag, then the consumer, then R2a's word). A count the server does not send is absent, never zero.
- **Measured, not computed:** cache counts and timings are read from the server's response, never derived client-side; where the server's field is ambiguous, the Q10 capture is the receipt and a new capture is taken rather than assumed.
- **Ownership:** track three owns `diet/src/drive/`, `diet/src/client/`, `diet/drive/`, `diet/client/`; track one owns `diet/formats/` and `scripts/`; `exercise/` is track five's. A change across a boundary goes by courier.
- **The record projection** (log → record) must be able to carry what this bump adds, or the bump states which record change (#82, #92) it waits on and requests it on demand as the ruling says.
- **The surface consumes the stream;** what it draws from these fields is its own, but the fields' meaning is fixed here, once.
- **Repository rules:** the root and `diet/` AGENTS.md; a test that cannot fail is not a test; every mechanism introduced gets a seeded fault in its lane's `gate.toml`; acceptance is a command and its exit code; no home paths, hostnames or private identifiers.

## Definition of done

1. A driven turn against the DoD 1 instance produces, in the log, per-request timings and cache counts as the server reported them, and the surface can draw a message's footer (tokens, milliseconds, warm versus cold) from them alone.
2. Prompt progress during prefill reaches the surface as events, so a header shows a count, not a sweep.
3. The response line carries what the surface's asks above require, or the ask is refused here with the reason on #117.
4. The log's first bump is conformance-tested, with v0's fixtures still read, and the generated bindings regenerated and checked stale-red.
5. The record projection carries the timings into the record, or names the record change it waits on.

## Not in scope

- The warm-tail fork (R4), patches (R5), the seam (R6), and their event kinds.
- Any change to what a claim measures.
