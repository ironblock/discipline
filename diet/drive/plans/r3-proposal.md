# R3 proposal: per-request timings, prompt progress and cache telemetry, into the log and the record

This is the proposing session's blind proposal, round 2, dated 2026-09-28. It is based on `main` at `3ada4cb`. I read no issue, PR or comment, and used no web access. The rulings quoted here come from the brief alone, plus the coordinator's four rulings on the round-1 critique's majors, which are cited where they apply. I read the consumer's shape on the unmerged `origin/feat/exercise-r1-surface` (`0381fb2`) with local `git show`, because the surface's asks are written there as types. **No llama-server ran.** The experiments ran in the scratchpad and are described in §5.

Round 1 is kept as `r3-proposal.round1.md`. The revision log at the end answers each of the critique's 20 findings.

**In short.**
- **Timings go on `response`, as a `timings` object.** It holds the server's keys, as the server spells them and with the digits it wrote. The server sends them only in the stream's last data chunk, and the transport drops that chunk today.
- **Prompt progress becomes a new log kind, `progress`, one line per server frame.** Neither capture contains a progress frame, so the field names wait on a new capture from the DoD 1 instance.
  - DoD 2 is not refusable. If the stream has no frames, the nearest measured source the server offers carries it: `/slots`, polled, and its accuracy cost is stated.
  - Only if no measured source exists does planning get asked to amend the item. Until then R3 stays open.
- **`response.reasoning` is carried.** It adds about 5% to a thinking turn's log.
- **`calls_from` and the system prompt's token count are refused for R3.** The reasons go on #117. Because the consumer claims `calls_from` was ruled into scope, that refusal also goes to the maintainer.
- **`context_overflow` joins the closed reason set only if a capture shows a typed server field to map from.** It is never read from message prose.
- **The log goes to v1.**
  - `line()` reads v0 and v1.
  - The whole-log check refuses v1 content in a log that declares itself v0.
- **The record gets the timings through the journal.**
  - A llama.cpp dialect path that the server never sends gets fixed.
  - The corrected canned server becomes a **new** canned instance with its own digest. The old instance is never mutated.
  - A new test reads the record's own reader, so the unspellable list really does turn red when the record grows.
- **Every mechanism has a fault, and the existing lane faults the change touches are re-anchored in the same patch.**

---

## 1. What exists

### What the server sends (the captures)

- **Timings arrive in exactly one place: the last data chunk before `[DONE]`,** a chunk with `"choices":[]` that carries `usage` and `timings`.
  - `diet/client/fixtures/llama-server-e7051ef-reasoning-stream.http:1250` (the DoD 1 instance, cold) sends `timings: {cache_n 0, prompt_n 89, prompt_ms 297.198, prompt_per_token_ms, prompt_per_second, predicted_n 312, predicted_ms 2591.561, predicted_per_token_ms, predicted_per_second, draft_n 312, draft_n_accepted 207}`. Beside it: `usage: {completion_tokens 312, prompt_tokens 89, total_tokens 401, prompt_tokens_details.cached_tokens 0}`.
  - `llama-server-4df29be-stream.http:38` (the reference build, warm) sends the same keys without the `draft_*` pair: `cache_n 28, prompt_n 1`, with `usage.prompt_tokens 29` and `cached_tokens 28`.
  - `[DONE]` follows, at `:1254` and `:42` respectively.
- **The semantics are measured on the DoD 1 instance, unstreamed.** In `substrates/measurements/2026-09-28-q10-reasoning/ab.json`, three turn-2 requests satisfy `prompt_tokens = prompt_n + cache_n` in every row (420 = 20 + 400; 125 = 40 + 85; 420 = 335 + 85). `capture.py:54` read them from `r["timings"]`.
  - So `prompt_n` is the prompt tokens that were prefilled, and `cache_n` is the tokens reused from the slot.
  - The streamed warm capture on `4df29be` agrees (29 = 1 + 28).
  - No **streamed warm** turn exists for `e7051ef`.
- **`draft_n` and `draft_n_accepted` appear because the instance decodes speculatively.** `substrates/registry.toml:509` lists `--spec-type draft-mtp`, and `:511` records that `draft_n` varies run to run.
- **Neither capture contains a prompt-progress frame, a `prompt_progress` key, per-token timings, a tool-call chunk or an error body.**
  - The request that produced the `e7051ef` capture asked only for `stream` and `include_usage` (`capture.py`, section 1).
  - Nothing the server sends during prefill appears in either capture: the first event after the headers is the role chunk.
- **The server has `/apply-template` and `/tokenize`.** Q10's prefix diff used both (`capture.py`, section 3).
- **The DoD 1 instance is a fork.** `registry.toml:496` says "llama.cpp (ironblock's fork)". It serves 4 slots (`-np 4`) with `-b 1024 -ub 512`, and requires a key (`:509`).

### What the transport parses and drops

- **The request asks for usage.** `wire::streaming_body` appends `"stream":true,"stream_options":{"include_usage":true}` after the body (`diet/src/client/wire.rs:186-192`). The head hash is unaffected (`:170-176`).
- **The usage-and-timings chunk is dropped.** `Reading::event` reads only `choices.first()` (`diet/src/client/stream.rs:568-592`), so the `choices: []` chunk is parsed as JSON and discarded. `[DONE]` then returns `Ended::Finished { finish_reason }` (`:554-557`). No token count, cache count or timing leaves the streamed transport.
- **A refusal is kept as text.** `Ended::Rejected { status, body }` keeps the server's body verbatim (`:138-143`, `:562-566`), and nothing classifies it.
- **The digits survive as written.** `serde_json` is built with `arbitrary_precision` (`diet/Cargo.toml:24`), so a number's text survives the parse (measured; see §5).
- **The unstreamed path reads a cached-token path that no capture contains.** For the llama.cpp dialect it reads `timings.prompt_n_cached` (`diet/src/client/shape.rs:302`, read at `wire.rs:336-340`).
  - Every capture and `ab.json` show `timings.cache_n`.
  - The canned server plays the unmeasured key back (`diet/src/drive/canned.rs:55, 232, 253, 271`).
  - So against the real server, the journal's `Cache.cached_tokens` is `None`. That is inferred from the captures; I did not run it.
- **The canned server's bodies are its identity.**
  - `canned::acts_digest()` hashes the acts, including the bodies that carry `prompt_n_cached`.
  - That digest is the canned substrate's `engine.version_or_digest` and its `weights.acts_sha256` (`drive/regimen.rs:136-150`).
  - It is pinned in `substrates/registry.toml:245-255` (`acts_sha256 = 8947e695…`, and `hardware_fingerprint`), in `diet/drive/dev-loop.toml:51`, and in the record fixture `valid/canned-substrate.jsonl`.
  - `dev-loop.toml:40-50` says that nothing checks the registry against the code.
  - The critique measured the digest after the key change: `188c4004…`.
- **The lane faults on the lines R3 touches:**
  - `client.cache-sniffed` (`diet/client/gate.toml:179-195`): its mutation sniffs `timings.prompt_n_cached`;
  - `client.streaming-body-drops-usage` (`:786-797`): its anchor is `wire.rs:191`;
  - `drive.session-reasoning-left-off-the-trunk` and `drive.session-reasoning-trimmed` (`diet/drive/gate.toml:1480-1503`): both anchor on `session.rs`'s `answer.reasoning = …` line.

  The lanes declare `faults = 58` and `faults = 124`.

### What the log's response line and the record's response row carry

- **Log v0 `response`** carries `to_request`, `text` and an optional `finish_reason` (`diet/src/formats/log.rs:884-891`). Nothing else is allowed on it (`:604-612`).
  - The session holds the whole reasoning at settle time (`diet/src/drive/session.rs:1052-1053`) and puts it only on the trunk.
  - `line_of` maps `Answered` to `response` (`:861-870`).
  - The module doc says outright that R3 is absent (`:44-45`).
- **Log v0 has no decimal.** `Holds` is `Count | Text | Version | Head | Tag` (`log.rs:795-806`). The bindings header says "a number here is an integer" (`log.rs:978-983`, `log.ts:3-4`).
- **The record's `response`** carries `id`, `to_request`, `output_tokens` (required) and `text` (`diet/src/formats/record/mod.rs:1152-1165`). `turn` carries `prefill_tokens` (`:1049-1054`).
  - The record's value space already has an exact `Decimal` (`record/json.rs:34-45`, `formats/number.pest`, no exponent).
  - It has no timing or cache field. It also has no clock: `client/cache.rs:46-49` and `drive/mod.rs:514-517` call a timestamp field "#82's territory".

### What `journal.rs` lists as unspellable, and why

The journal exists because "record v0 cannot spell most of what happens here" (`diet/src/client/journal.rs:5-8`). `lost()` (`:290-330`) is the list of what does not fit. The list is written as the request for a schema change, so that when the record grows, a test turns red:

| Entry | What is lost (the list's own words, abridged) |
|---|---|
| `serving.declared` | `start.serving.concurrency` |
| `regime.mismatch`, `regime.unverified`, `regime.stripped` | event kinds for pins that were contradicted, unchecked or stripped |
| `outcome.timeout` | a timeout as a typed outcome (deliberately projected to nothing) |
| `outcome.capped` | `response.capped_at` |
| `outcome.refused` | `response.refused` with its status |
| `outcome.failed` | `response.failed` with its class |
| `outcome.unreadable` | `response.unreadable` |
| **`cache.observed`** | **"cache telemetry per request: `response.cache`, with the prompt and reused token counts and the path they were read from"** (`:320-323`) |

Per-row losses are noted inline in `project`:
- `request.sampler`;
- `request.retry_reason`;
- the difference between issued and arrived (`:386-404`);
- an unreported `output_tokens`, and `response.outcome` (`:420-433`).

Two more facts shape the rest of this proposal:
- **The journal records no timings at all.**
- **The interactive session builds no journal and writes no record.** `session::call` drives `Streaming::stream` directly (`session.rs:1009-1044`). Only the unstreamed `Client` pushes journal entries (`client/mod.rs:674-716`), and only the gym projects them (`drive/mod.rs:1258`).

### How a bump is versioned and applied

- **The version.** `log::VERSION = 0` (`log.rs:38`). `session.start` must state exactly that version (`:615-620`), and the bindings carry the literal type `version: 0` (`log.ts:86`, from `log.rs:946`).
- **The rule.** "A later kind is a versioned bump" (`log.rs:10-12`). R2c D4 adds: "each bump is a courier round trip: grammar, fixtures, reader and the consumer's types. There is no migration while logs are not persisted (W4)" (`diet/drive/plans/r2c-proposal.md:175`).
- **The courier.**
  - Track three drafts the patch for `diet/formats/log/**` and `diet/src/formats/log.rs`, and runs its faults to write their `catches`.
  - Track one applies it, and wires the injections into `verify.sh` (the log's are at `:1446-1510`) and `tools/gate/faults.toml` (`:5591-5630`).
  - Lanes declare their own faults in `diet/client/gate.toml` and `diet/drive/gate.toml`, which track one wires (`diet/client/gate.toml:1-22`).
- **Conformance.**
  - `tests/conformance.rs` parses every valid fixture to its `.expected.json` (`:259-292`).
  - It refuses every invalid one, and fails when two invalid fixtures are rejected by the identical message (`:294-338`).
  - `log` is registered at `diet/src/formats/mod.rs:69-73`.
- **Two existing invalid fixtures are sensitive to this bump:**
  - `fixtures/invalid/an-unknown-version.jsonl` declares `"version":1`. Under a v1 reader it stops being invalid.
  - `an-unknown-kind.jsonl` uses `"kind":"progress"`, which is the consumer's name for the progress kind (see D2).

### How the bindings regenerate

- `log::typescript()` generates `formats/log/log.ts` from `schema()` and the vocabularies (`log.rs:974-1039`).
- `the_checked_in_bindings_are_current` fails when they are stale (`:1692-1701`).
- `write_the_bindings` is the ignored test that rewrites them: `cargo test -p discipline-diet --lib formats::log::tests::write_the_bindings -- --ignored` (`:1703-1708`).
- The stale-red fault `test.log_bindings_stale` exists (`verify.sh:1446-1462`, `faults.toml:5591-5599`). Its anchor, `Self::GapEnd => "GapEnd",`, survives the changes proposed here.
- The generator knows flat keys and one exclusivity rule (`exactly_one`, `:926-932`), and it hard-codes one nested interface, `HeadMessage` (`:997`). That interface is the precedent for `Timings`. The generator has no non-integer number.
- `the_schema_is_what_every_kind_writes` walks top-level keys only (`:1454-1527`).
- `log::line` is the reader for a resumed stream and never sees `session.start` (`:342-352`). `serve.rs:1273` and `tests/drive_serve_cli.rs` call it per event.

### How the unspellable list is asserted today

`client::tests::the_record_projection_names_every_fact_record_v0_cannot_carry` (`client/mod.rs:1726-1760`) compares the kinds `lost()` emits against a hard-coded list. It never reads the record's schema, so a record that learns to spell an item leaves the test green.

The record keeps no key table. Each row's reader takes its known members and refuses the leftover as `SchemaError::UnknownField` (exercised at `record/mod.rs:5019-5029`). That refusal is the record's own statement of what it cannot spell.

`journal.rs` has no tests of its own.

### The consumer's asks, as its types say them (unmerged branch)

These are all in `exercise/src/drive/log.ts` on the consumer branch:
- **`response.timings?: Timings`** with `prompt_n, cache_n, prompt_ms, predicted_n, predicted_ms` (`:57-67`, `:126-127`).
- **`response.calls_from?: {predicted_n?, predicted_ms}`** (`:128-134`).
- **`response.reasoning?`**, described as "the whole reasoning, as the deltas streamed it" (`:135-136`).
- **`session.start.system_tokens?`** (`:102-103`).
- **A `progress` kind:** `{request, prompt: {total, cache, processed}, decoded}`. It is called "Transient", and sourced "from llama.cpp's stream if it carries prompt progress, else its `/slots`, polled once a second" (`:147-160`). The comment says this was ruled on 2026-09-26; I could not check that.
- **`FailReason | 'context_overflow'`** (`:47-50`).

The consumer's recorded fixtures write `0` for an absent timing (`exercise/scripts/migrate-recorded.py:73-76`). That is its own lane, but it contradicts "absent, never zero" (Q12).

---

## 2. Decisions

### D1. Where timings go, and which fields

**Options.**
- **(a) A `timings` object on `response`.** Its keys are the server's own, each optional, and the object is absent when the server sent none. The server calls the object `timings` and so does the consumer, so the precedence rule (a ruling, then the record, then the session's tag, then the consumer, then R2a) and the server agree. The cost: the schema table and the generator gain a nested object (`Holds::Timings` with its own field list) and a non-integer number.
- **(b) The same keys flat on `response`.** This is less generator work. But it loses the server's grouping, and a later `usage` or `draft` key sits in one namespace with `text`.
- **(c) On `request`.** This is not possible. A `request` line is written before the call (`session.rs` order: request, deltas, terminal), the numbers arrive in the stream's last chunk, and the log is append-only.
- **(d) A new kind, `timings {request, …}`, after the terminal line.** This keeps `response` unchanged, but it needs its own cross-line rules (once per request, only after a `response`) and a join in the surface. The server sends timings only when the call finished (a cancel closes the socket and a refusal has none), so a separate line buys nothing.

**Recommendation: (a).** This is decided by the adopted precedence rule rather than asked: the consumer names the object `timings`, and the record, the rulings and the session name nothing.

**Which chunk.** The transport reads `timings` from whichever data chunk carries it, and the last one wins. It delivers them at `[DONE]` or at the close. With `include_usage` they ride the `choices: []` chunk (both captures). Without it they ride the last chunk that still has `choices` (`wire.rs:177-180`). C1 records whether progress frames carry either.

**Fields,** named as the server names them:
- **`prompt_n`, `cache_n`, `predicted_n`** are counts.
- **`prompt_ms`, `predicted_ms`** are **non-negative** numbers carried as the digits the server wrote: a record `Decimal`, or an `Integer` if the server writes one. `number.pest` admits negative fractions, so the non-negativity is a reader rule with its own fixture.
- **`draft_n`, `draft_n_accepted`** are carried only if Q3 says so. They are the in-band evidence that a speculative path produced the answer, which `diet/AGENTS.md` (GRADING, "speculative or optimization path") makes a regime fact.
- **Not carried:** `*_per_token_ms` and `*_per_second`. They are the server's arithmetic on the carried numbers, and nobody asked for them. If a rate is ever wanted, carry the server's rate rather than let the surface divide.
- **Not carried: `usage`.** `usage.prompt_tokens` and `cached_tokens` duplicate `prompt_n + cache_n` and `cache_n` on this server (the `ab.json` rows and both captures).

**Absence.**
- A key the server did not send is absent. A call that did not finish has no `timings`.
- A number the record's value space cannot spell, such as an exponent (`number.pest`: "No exponent"), is also absent. The transport's test pins that behaviour (§5, risk 2).

**The footer DoD 1 asks for** comes from this object alone:
- tokens from `predicted_n`, `prompt_n` and `cache_n`, each as the server sent it;
- milliseconds from `prompt_ms` and `predicted_ms`;
- warm versus cold from `cache_n`.

What the surface draws from these is its own. Their meaning is fixed here as the `ab.json` receipt fixes it: `prompt_n` is prefilled, `cache_n` is reused, and the two are disjoint.

This plan gives no guidance to add numbers together. A prompt total would be the client computing a number the server already sends (`usage.prompt_tokens`). Whether to carry that number is new Q7.

### D2. Prompt progress

**Options.**
- **(a) One log line per server progress frame.** The kind is named `progress`, by precedence (the consumer's word; the record, the rulings and the session name none). Its fields are named as the server's frame names them, and capture C1 decides what those are. The line references its `request` by seq, and is refused after that request has ended (the `delta` rule, `log.rs:572-577`).
- **(b) The surface derives progress from deltas.** This is not possible for prefill: no delta arrives until prefill is over (the first event in both captures is the role chunk). Any count derived that way is computed rather than measured.
- **(c) A transient SSE event outside the log.** This is a second format on a stream whose every `data:` is a log line (R2c D7, `serve.rs:419-452`). It would not be replayed on resume, and could not be conformance-tested.
- **(d) The drive polls the server's `/slots` while a request is busy and logs what it reads, as the same kind.** These are the server's numbers, sampled at times the client picks. They cost a second connection per poll against a keyed server with 4 slots. This is unmeasured, and only worth it if C1 shows the fork emits no frames.

**Recommendation: (a), sourced from the stream.** The kind's name, `progress`, is decided by the precedence rule (the consumer's word). The fields are the server's.

**DoD 2 is not refusable. The sources, in order of accuracy:**
1. **Stream frames.** The server's own count at each step it reports, in-band with the request and in order with its deltas.
2. **`/slots`, polled by the drive while the request is busy, and logged as the same kind** (option (d)). This is the nearest measured source if C1 shows no frames. It costs accuracy in three ways, which the plan states on the line rather than hides:
   - **It is sampled.** A frame reflects the slot at the poll instant, so the count lags by up to one poll period. At the consumer's once a second, a prefill shorter than a second shows at most one mid-prefill count.
   - **It needs the slot identified.** With 4 slots the drive must know which slot is its own. Either the request pins a slot (the slot id is blocking by the re-ranking ruling, and arrives with R4), or the poll matches the slot by task. C4 shows whether `/slots` carries either.
   - **It costs a keyed request per poll** on a shared server.
3. **If neither exists** (the fork emits no frames, and `/slots` is disabled or carries no prefill count), no measured source is known to me. `/metrics` is not on the serving line (`registry.toml:509`) and would need a relaunch this program does not control. Only then does planning get asked to amend DoD 2 (Q5). **R3 is not done until DoD 2 is met or amended by ruling.** A partial result is recorded as partial, not as negative.

**Costs and fields.**
- **Cost to the log's size.** One line is about 97 bytes (computed for a four-field frame). The Q10 turn's 309 delta lines are 20,350 bytes. How many frames a prefill produces is unmeasured: one per `-ub 512` or one per `-b 1024` both seem possible to me, and C1 decides. A 32k-token cold head would then be 32 to 64 lines, or 3 to 6 KB. A warm turn is one or two lines.
- **`decoded`.** I can tie it to no field in any capture. Counting deltas is computed, and under speculative decoding a chunk is not shown to be one token. It is carried only if a capture shows the server sending it: in a stream frame, or as a per-slot count on `/slots` (Q6).
- **Asking for progress.** It must be requested. Upstream llama.cpp names the request flag `return_progress` and the frame object `prompt_progress` (`total`, `cache`, `processed`, `time_ms`), but that is **my recollection, not verified**, and the instance is a fork. C1 is the receipt.

### D3. `calls_from`

- **Server-derived.** The server's `timings` object has no such field in either capture.
- **Client-derived.** The alternative is to count deltas, or read the clock, before the first `delta.tool_calls` chunk. That is a computed split, so it cannot exist under "measured, not computed".
- **It also has no referent yet.**
  - The trunk has no tool loop: `max_steps` is "here because the ruling names it" (`session.rs:118-120`).
  - The transport does not read `delta.tool_calls` (`stream.rs:568-592`).
- **The only measured route I can see** is the server's per-chunk timings, read at the chunk that carries the first tool call: `predicted_n` and `predicted_ms` so far. Upstream calls the request flag `timings_per_token`, but that too is recollection, not a capture.

**Recommendation: refuse it in R3, on #117, with that reason. Re-raise it at the tool loop (DoD 2) with a capture that has per-chunk timings.**

The consumer's type says `calls_from` was "ruled into R3's scope on #117" (`exercise/src/drive/log.ts:128-134` at `0381fb2`). I cannot read that ruling. DoD 3 allows an ask to be refused with the reason on #117, but refusing something a ruling put in scope is the maintainer's call, not this plan's. So the refusal is put to the maintainer as Q7b, with the reasons above. If the ruling stands, R3 carries the server's per-chunk numbers, never a client split.

### D4. The whole reasoning on the response line

- **(a) Add an optional `reasoning` to `response`.** It is the concatenation of that request's reasoning deltas, byte for byte and untrimmed: the same string the trunk re-sends (`session.rs:1052-1053`). It is absent when none arrived, following the `arrived` rule (`session.rs:938-942`).
- **(b) Keep deltas only, and let the surface concatenate.**

**Recommendation: (a).**
- **The precedent.** `response.text` already duplicates the text deltas.
- **The need.** A reader that resumed mid-turn, or a projection that skips deltas, otherwise has no whole reasoning.
- **The cost.** 1,096 bytes (JSON-escaped) on the Q10 turn's 20,350 bytes of deltas, about 5%.
- **What the reader does not check.** The v0 reader does not check `text` against its deltas, and this proposal adds no such check for `reasoning` (risk 6).

### D5. `context_overflow`

- **What maps to it.** Only a **typed** field the server sends: an error `type` or `code` in the refusal body, before the stream or as an in-stream `error` event.
- **What never maps to it.** The `message`. `request.failed.message` is prose by ruling (#140, pinned at `log.rs:1604-1608`).
- **What upstream sends** is, by my unverified recollection, HTTP 400 with `error.type = "exceed_context_size_error"`. Capture C3 settles what the fork sends.
- **The mapping.**
  - `Ended::Rejected` gains the classified kind, and the transport classifies the body it already keeps (`stream.rs:138-143`).
  - The session maps it to `request.failed { reason: context_overflow, status }`. `turn.settled` stays `failed`.
- **What is not an overflow.** A generation that runs out of context mid-answer and stops with `finish_reason: length` is a capped generation, a typed outcome of its own (`diet/AGENTS.md`, GRADING).

**Recommendation:**
- Add `context_overflow` to `FailReason`, named by precedence (the consumer's word), **only if C3 shows a typed field**.
- Otherwise refuse it on #117: the server's overflow is `server` with its status, and nothing typed distinguishes it.

### D6. The system prompt's token count

- **The server does not report it.** Its only token counts are per request (`prompt_n`, `cache_n`), and a cold turn 1's `prompt_n` includes the ask and the template.
- **The server can count it.** Q10 used `/apply-template` and `/tokenize` on the DoD instance.
- **Options.**
  - **(a) Refuse it.**
  - **(b) At open, the drive asks the server for the token count of `/apply-template(head)`, and `session.start` carries it as `system_tokens`** (the consumer's name). This is valid only after a capture shows the rendered head is a token prefix of turn 1's prompt, as `prefix-diff.json` showed for the turn. It adds two keyed requests at open.
  - **(c) The surface uses turn 1's `prompt_n + cache_n`.** That is a different number.

**Recommendation: (a) for R3, with the reason on #117.** (b) is its own measured increment, if planning wants it (Q8).

### D7. The log's bump, and what changes for v0 readers

- **The version.** The bump is **v1**.
  - **The reader** reads `version` 0 and 1.
  - **The writer** writes 1.
- **Scoping.**
  - Each kind, key and tag in the schema table gains the version it arrived in.
  - **Where it is enforced.** `line()` reads the union of v0 and v1, because it is the resumed-stream reader and never sees `session.start` (`log.rs:342-352`). The scoping rule lives in `check`: a whole log that declares 0 and carries a v1 kind, key or tag is refused. The v0-scoping fixtures prove it through `parse`.
  - **What a resuming surface can enforce.** A surface that resumes mid-log cannot enforce scoping on the lines it reads. It knows the version from its first connection's `session.start`.
  - Without the rule, "version 0" says nothing.
- **Alternatives, both refused.**
  - A lenient reader, which accepts v1 content under 0.
  - A second, v0-only reader, which is two implementations of one format (`diet/AGENTS.md`, CONTENT).
- **Fixtures.**
  - Every v0 valid fixture stays byte-identical and must still parse to its expected value (DoD 4).
  - `an-unknown-version` moves to `"version":2`.
  - `an-unknown-kind` moves to a kind no version names, because `progress` becomes a v1 kind.
  - Reason texts that say "v0" are re-read. The same goes for the reader's "which v0 does not name" (`log.rs:1228`).
- **The bindings** gain:
  - `export const VERSION = 1` (what the writer writes) beside `export const READS = [0, 1] as const` (what the reader reads);
  - `version: 0 | 1`;
  - a `Timings` interface;
  - `ProgressLine`;
  - `context_overflow`, if D5 lands.

  The "a number here is an integer" header changes, because `prompt_ms` is not one.
- **What changes for a v0 reader.** A v0-typed surface refuses a v1 log at its first line (`version: 0` is a literal type). The surface moves with the bump (R3.6). The Rust reader reads both versions. `diet/wasm` exports no log reader (`diet/wasm/src/lib.rs`), so nothing there changes. No log is persisted, so nothing migrates.

### D8. What the record projection needs

**Recommendation.**
- **The interactive session keeps writing no record in R3 (Q11).** DoD 5's "or names the record change it waits on" is met through the journal, which is where the ruling puts the projection.
- **Fix the llama.cpp dialect's cached path** to `timings.cache_n` (`shape.rs:302`). On the DoD instance the unstreamed reply carries `cache_n` (`ab.json`), and no capture carries `prompt_n_cached`. C5 adds a raw unstreamed reply, so the dialect test replays real bytes. The canned server's bodies move with the fix, under D9.
- **The journal records the server's timings object** from unstreamed replies, as a new entry kind beside `cache.observed`. That vocabulary is track three's.
- **`lost()` names per-request timings** in the words the record request will use. Each item also names, as data, the record row and key (or kind) it asks for: here `("response", "timings")`.
- **The mechanism that makes "the list goes red when the record grows" true** (new).
  - `every_unspellable_item_is_one_the_record_refuses` probes the **record's own reader**. For each item's `(row, key)`, it parses a minimal valid record whose row of that kind carries the key, and requires `SchemaError::UnknownField` naming that key. An item that asks for a new kind requires the record's unknown-kind refusal instead.
  - When track one teaches the record `response.timings`, the refusal stops and the test fails with "`lost()` names `response.timings`, which the record now spells". The projection must then write it, and the item leaves the list.
  - This replaces reliance on `the_record_projection_names_every_fact_record_v0_cannot_carry`, which compares `lost()` with itself.
- **Which record field carries the reuse count.** It is carried once.
  - For llama.cpp, `response.timings.cache_n` carries it, beside `prompt_n`, which is the count the server itself reports.
  - The existing `response.cache` item stays on the list, reworded to the dialects without `timings` (the openai-compatible path, `usage.prompt_tokens_details.cached_tokens`, with its path). It is **not** demanded now, because no session on such a dialect wants it.
  - So the one record bump demanded now is `response.timings`, the same object as the log's, with the record's exact `Decimal` for milliseconds (Q10).
- **What waits.**
  - The **slot id**, also blocking, arrives with R4 (`an-unknown-key.reason`: "slot arrives with R4").
  - Everything else on the list waits for a session that wants it.

### D9. The canned server's identity when its bodies change (new; ruled by the coordinator)

The dialect fix changes the canned server's answer bodies, and with them `acts_digest()`, which is a pinned substrate's identity (§1).

**Decision: never mutate a pinned substrate.**
- **Two canned instances, not one.**
  - The corrected acts are a **new** canned instance with their own digest.
  - The current acts stay in the code unchanged, as the old instance's acts, so that its replay stays byte-identical.
  - Each instance is identified by its own digest. Nothing is renamed.
- **The registry edit is routed to the data seat.** The data seat adds a new canned entry and names it (its id is not invented here). The existing `canned-loopback` entry (`acts_sha256 = 8947e695…`) is left as it is.
- **What moves to the new instance, in the same PR as the dialect fix:**
  - the gym and its tests;
  - `diet-drive canned`;
  - `dev-loop.toml:51`'s `substrate_hardware`.
- **What keeps the old digest.** Old records, including the record fixture `valid/canned-substrate.jsonl` (track one's), keep pointing at the old instance. Their bytes and their digest are still true of the old acts.
- **The citation becomes checked.**
  - `every_registered_canned_digest_is_acts_this_crate_plays` reads the registry's canned entries and requires each `acts_sha256` to equal the digest of one acts set the code keeps.
  - A second test pins that the old acts still hash to the registry's old value.
  - This closes the "nothing checks" gap that `dev-loop.toml:40-50` names, for canned entries only.

---

## 3. The plan

| # | Work | Where | Owner | Waits on | Unblocks |
|---|---|---|---|---|---|
| R3.0 | captures C1–C5 on the DoD 1 instance, in a window the data seat coordinates | a capture script beside Q10's under `substrates/measurements/`, and the raw bytes under `diet/client/fixtures/` | the data seat (Q13); fixtures handed to three | nothing | D2, D5, DoD 2, 3 |
| R3.1 | the transport keeps the timings; the dialect path is fixed; the new canned instance; the affected faults are re-anchored | `client/stream.rs`, `shape.rs`, `wire.rs`, `drive/canned.rs`, `drive/regimen.rs`, `dev-loop.toml`, both `gate.toml` | three; the registry entry by the data seat; wiring by one | C5 for the dialect test's bytes | DoD 1, 5 |
| R3.2 | log v1 (a courier patch) | `diet/formats/log/**`, `diet/src/formats/log.rs`, `verify.sh`, `faults.toml` | one, from three | R3.0 for `progress` and `context_overflow` (Q1) | DoD 1–4 |
| R3.3 | progress asked for and parsed; the refusal classified | `client/stream.rs`, `client/wire.rs`, `diet/client/gate.toml` | three; wiring by one | R3.0 | DoD 2, 3 |
| R3.4 | the session logs timings, reasoning, progress (or `/slots` samples) and overflow | `drive/session.rs`, `drive/serve.rs` (doc), `diet/drive/gate.toml` | three; wiring by one | R3.1, R3.3; R3.2 for `line_of` | DoD 1–3 |
| R3.5 | the journal records timings; `lost()` names them; the record probe; the record bump is requested | `client/wire.rs`, `client/mod.rs`, `client/journal.rs`, `diet/client/gate.toml` | three; wiring and the record bump by one | R3.1 | DoD 5 |
| R3.6 | the surface reads v1 | `exercise/` | five | R3.2 (types); R3.4 to run live | DoD 1, 2 |
| R3.7 | live acceptance: two turns against the DoD 1 instance | a relay tap in the capture script; fixtures under `diet/drive/fixtures/`; a test in `diet/src/drive/serve.rs` | three; the run in the data seat's window | R3.4; R3.6 for the surface's half | DoD 1, 2 |

**Rules for every increment.**
- **Tests.** Every test is written RED first, against the new signature with the behaviour absent.
- **Faults.** Every fault changes the tree and is seen red on its own signature. Its acceptance is the lane command exiting 101 with the fault applied and 0 without it. Its `catches` is written by running it.
- **Wiring.** Every lane fault is declared in its lane's `gate.toml`, with the `[package] faults` count updated, and **track one wires it** into `verify.sh` and `tools/gate/faults.toml` in a courier round trip for that increment (`client/gate.toml:1-7`).
- **Names.** Fault and test names below are proposals.

### R3.0: the captures (in a window the data seat coordinates)

**These captures are not read-only in effect, and the plan says so.**
- Every capture occupies one of the 4 slots, and prefills into a `--kv-unified` cache shared with live users. A long cold prefill can evict another user's warm prefix. Q10's `ab.json` requests did the same.
- So R3.0 runs in a window the data seat coordinates. The server is never stopped or relaunched.

**They need a script, not a session.** The session's transport cannot:
- send a flag it does not yet send;
- keep raw bytes;
- pin a slot;
- read `/slots`.

So the captures are taken by a `capture.py`-style script that reads the key as Q10's did and never prints or writes it.

- **Before anything is sent: read the limits.** Read the server's own per-slot context limit and its slot count from its properties endpoint (upstream `/props`, by recollection; C0). This is a read that occupies no slot. `serving_context = 262144` is the launch value; what each slot is allowed under `--kv-unified` is exactly what C0 reads.
- **C1: a prefill long enough to take several batches.**
  - A streamed turn whose prompt spans at least three `-b 1024` batches, made cold by a unique prefix so the cache cannot warm it, with progress requested (upstream's flag, `return_progress`, then whatever the fork documents).
  - It records whether frames come, where they sit (with `choices: []`, with `timings`, or neither), their keys, and how many per prefill (per `-ub` or per `-b`).
  - It also records whether the final chunk is unchanged.
- **C2: a streamed warm turn 2.**
  - It records `cache_n > 0` in the streamed final chunk on `e7051ef`. `ab.json` is unstreamed.
  - It pins turn 2 to turn 1's slot, if the fork honours a slot-pinning request field (upstream `id_slot`, by recollection).
  - Otherwise it relies on the server's own slot choice, and **the receipt is whatever `cache_n` it shows**, with the other slots' activity noted. A cold result is a result, not a failed capture.
- **C3: the overflow.**
  - It is designed against the **per-slot limit C0 reads**, at the smallest prompt that exceeds it: that limit plus a few tokens, counted by the server's `/tokenize` before sending.
  - It is sent **once**, **inside the coordinated window**, and **not** as a 262k-token probe fired at an arbitrary time.
  - Both conditions apply, and here is why. Whether the fork refuses a too-long prompt before it writes any KV is unverified. And under `--kv-unified`, a prompt below the per-slot limit can still fail when other slots fill the shared cache, which would be a different error type. C3 is therefore run with the other slots idle, as far as the window allows, so that the error it records is the per-slot overflow and not the full-cache one.
  - If the full-cache error is seen, it is recorded as its own observation and **not** mapped to `context_overflow`.
- **C4, only if C1 shows no frames:** `/slots` read while C1's request is busy.
  - **Redaction.** `/slots` may carry other sessions' prompts. So C4 commits **only the key names, and the counts for the capturing request's own slot**. Its README says so.
  - This is the one capture that is not "nothing edited", by design.
- **C5: one raw unstreamed reply.** It records what the dialect reads, including `timings.cache_n`, whether `prompt_n_cached` is present, and whether `generation_settings` (the dialect's `sampler_echo`) is present.
- **Receipts.**
  - C1, C2, C3 and C5 are committed as raw socket bytes with nothing edited.
  - C0 and C4 are committed as recorded key names and counts.
  - A README states how each was taken and what the window was, as `2026-09-28-q10-reasoning/README.md` does.

### R3.1: the transport keeps what the server measured; the dialect and the canned instance (track three)

**The change.**
- `Reading::event` reads `timings` from any chunk that carries it, with the last one winning (D1). `Ended::Finished` gains `timings: Option<Timings>`. Each field is an `Option`, and the milliseconds are a record `Decimal` built from the number's own text (`Decimal::new`, `record/json.rs:47-62`). A number that `Decimal::new` refuses is absent.
- `Canned` gains a step that plays timings.
- `Dialect::llama_cpp().cached_tokens` becomes `timings.cache_n`.
- The new canned instance (D9): the acts become two sets, and the gym moves to the new one.

**Tests.**
- `the_real_servers_timings_reach_the_caller_as_written` replays both captures, whole and one byte at a time (the `stream.rs:1154-1200` pattern). It asserts `prompt_ms == "297.198"`, `cache_n == Some(0)` and `draft_n == Some(312)`, and on `4df29be` `cache_n == Some(28)` and `draft_n == None`.
- `timings_on_the_last_chunk_with_choices_are_read_too` covers the no-`include_usage` shape.
- `a_timing_the_server_did_not_send_is_absent_not_zero`.
- `a_timings_digits_survive_the_transport`: a seeded `"prompt_ms":1.10` reads back `"1.10"`. The captures' digits would survive an `f64` round trip by accident (§5).
- `an_exponent_in_a_timing_is_absent`: `2.9e2`, which re-renders as `2.9e+2` and which `Decimal::new` refuses (critique, measured).
- `timings_that_arrived_before_a_cancel_are_not_delivered`. It replaces round 1's `a_cancelled_turn_has_no_timings`, which could not fail. A canned stream sends a timings chunk, and a cancel lands before `[DONE]`. The result must be `Cancelled`, and a caller-side recorder must see no timings.
- `the_llama_cpp_dialect_reads_the_cache_count_the_server_sends` replays C5's raw bytes.
- `every_registered_canned_digest_is_acts_this_crate_plays`, and `the_old_canned_acts_still_hash_to_their_registered_digest` (D9).

**Faults (new, `diet/client/gate.toml`):**

| Fault | What it changes | Caught by |
|---|---|---|
| `client.stream-drops-the-timings-chunk` | the timings chunk is skipped again | the first test |
| `client.stream-reads-an-absent-timing-as-zero` | `unwrap_or(0)` on a missing key | `…absent_not_zero` |
| `client.stream-rounds-a-timing-through-a-float` | the digits go through `as_f64` then `format!` | `…digits_survive…` |
| `client.stream-spells-an-unspellable-timing` | on a refused `Decimal::new`, fall back to the `f64`'s display (`290`, read as an integer) | `…exponent…is_absent` |
| `client.stream-delivers-timings-after-a-cancel` | timings handed on at the cancel | `…before_a_cancel…` |
| `client.dialect-reads-an-unsent-cache-path` | the path reverted to `timings.prompt_n_cached` | the C5 dialect test |
| `drive.canned-old-acts-edited` (in `diet/drive/gate.toml`) | one byte of an old act changed | `the_old_canned_acts_still_hash…` |

**Existing faults re-anchored in the same patch.**
- **`client.cache-sniffed`.** The mutation becomes a sniff of `timings.cache_n` then `usage.prompt_tokens_details.cached_tokens`, the keys the fixed code and the test bodies actually carry. Under the openai-compatible dialect it then yields a number where `cache_telemetry_is_read_from_the_path_the_dialect_declares_and_nowhere_else` requires `None`, so it is red again. It is never left pointing at a key nothing reads.
- **The test bodies** at `client/mod.rs:854, 1361` and `wire.rs:641` move to `cache_n`.
- **All faults re-run.** Every `catches` in both lanes is regenerated by running.

**Acceptance.** `cargo test -p discipline-diet -- client` and `cargo test -p discipline-diet -- drive` both exit 0, and each new or re-anchored fault exits 101 on its signature.

### R3.2: log v1 (track one; courier patch from track three)

**The schema.**
- `response` gains `timings?` (D1) and `reasoning?` (D4).
- A `progress` kind is added (D2, with the fields C1 shows).
- `FailReason` gains `context_overflow` if D5 lands.
- `Holds` gains a non-negative number and the nested `timings`, following the `HeadMessage` precedent (`log.rs:997`).
- Every kind, key and tag gains the version it arrived in. `check` enforces it, and `line()` reads the union (D7).
- The generator emits the nested interface, `VERSION` and `READS`, and `version: 0 | 1`.
- `grammar.pest`'s header says v1 and v0.

**Valid fixtures (v1):**
- an answered turn with timings;
- a warm turn (`cache_n > 0`);
- a response whose server sent no `cache_n`;
- a thinking turn whose response carries its reasoning;
- a turn with at least two progress lines before its first delta;
- a context overflow, if D5 lands.

Every v0 fixture is kept byte-identical.

**Invalid fixtures,** each with a `.reason` and a distinct rejection:
- a v0 log carrying `timings`;
- a v0 log carrying `progress`;
- `progress` after its request ended;
- `progress` citing a line that is not a request;
- `timings.prompt_ms` as a string;
- a negative `prompt_n`;
- a negative `prompt_ms`;
- an unknown key inside `timings`;
- `context_overflow` in a v0 log, if D5 lands;
- `an-unknown-version` at 2;
- `an-unknown-kind` retargeted to a kind no version names.

**Tests.**
- `the_schema_is_what_every_kind_writes` is extended into nested fields. Every optional key inside `timings` must be written somewhere and omitted somewhere in the corpus, and hold its declared type.
- `every_event` gains one of each new shape.

**Seeded faults (verify.sh injections, each changing the tree and seen red on its own signature):**

| Fault | What it disables |
|---|---|
| `test.log_v1_key_read_in_a_v0_log` | version scoping in `check` |
| `test.log_progress_after_its_request_read` | `progress` removed from the ended-request arm |
| `test.log_progress_citing_a_non_request_read` | `progress` removed from the reference arm |
| `test.log_timings_negative_count_read` | a timings count read as an integer, not as a count |
| `test.log_timings_negative_ms_read` | the non-negative check on ms |
| `test.log_timings_ms_as_text_read` | the ms reader accepting a string |
| `test.log_timings_unknown_key_read` | the nested key check |
| `test.log_nested_schema_unchecked` | the schema test's nested walk skipped |

`test.log_bindings_stale` keeps its anchor.

**Acceptance.** `cargo test -p discipline-diet --test conformance -- formats::log` and `cargo test -p discipline-diet --lib formats::log` both exit 0, and each injection above turns its own test red.

### R3.3: progress and the refusal's type, in the transport (track three)

**The change.**
- `streaming_body` asks for progress, using the name C1 shows. The request is appended after the head, so `head_sha256` is unchanged.
- The transport delivers progress frames as a new variant beside `Piece` (it is not answer text).
- `Ended::Rejected` gains the classified kind, read from the body's typed field.
- If the fallback applies (D2, source 2), a `/slots` poller is added. It samples the request's own slot while it is busy and delivers the same frames, each marked with its source.

**Tests.**
- `a_streaming_body_asks_for_usage_and_progress_and_keeps_the_head`, modelled on `a_streaming_body_is_the_body_with_streaming_asked_for_and_the_same_head` (`wire.rs:710`). It parses the body and requires both flags and the unchanged head. It is what catches a dropped flag, because the capture-replay tests do not depend on what was sent.
- C1 replayed, whole and byte by byte: the frames arrive in order, before the first reasoning piece.
- C3 replayed: classified as an overflow.
- A body whose *message* mentions context but whose type does not is not classified.
- If the poller is added: `a_slots_poll_reads_only_the_requests_own_slot`, against C4's recorded shape.

**Faults.**

| Fault | What it changes |
|---|---|
| `client.streaming-body-drops-progress` | the progress flag removed |
| `client.stream-drops-progress` | frames not delivered |
| `client.refusal-classified-from-its-message` | the class read from the message |
| `client.slots-poll-reads-another-slot` | the poller reads another slot (if the poller is added) |

**Re-anchored.** `client.streaming-body-drops-usage`'s anchor is the rewritten `format!` line at `wire.rs:191`. It is re-anchored on the new line, and still removes only `include_usage`.

**Acceptance.** `cargo test -p discipline-diet -- client` exits 0, and each fault exits 101 on its signature.

### R3.4: the session (track three)

**The change.**
- `Answered` gains `timings` and `reasoning`.
- A new `Progress` event is added.
- `Rejected` carries the class.
- `line_of` maps each (`session.rs:809-900`).
- **The reasoning is bound once.** In `call()`, one binding feeds both the trunk message and the `Answered` event, so the two cannot differ.
- The R3 disclaimer at `session.rs:44-45` is updated, and so is the stale "I1 is not built yet" at `serve.rs:36-38`.

**Tests.**
- `a_finished_turns_response_carries_the_servers_timings` (Canned).
- `the_responses_reasoning_is_its_deltas_byte_for_byte`.
- `progress_is_logged_before_the_first_delta_and_never_after_the_end`.
- `an_overflow_is_request_failed_context_overflow_and_the_turn_failed`.
- The existing `every_event_the_session_logs_is_a_line_the_log_format_reads` extends to the new variants through its exhaustive match.

**Faults (new):**

| Fault | What it changes |
|---|---|
| `drive.session-drops-timings` | `Answered` built without the timings |
| `drive.session-response-reasoning-dropped` | the response's reasoning `None` while the trunk keeps it |
| `drive.session-logs-progress-after-the-end` | a progress frame pushed after the terminal event |
| `drive.session-overflow-logged-as-server` | the class ignored, so the reason is `server` |

**Re-anchored.** `drive.session-reasoning-left-off-the-trunk` and `drive.session-reasoning-trimmed` anchor on `answer.reasoning = (!reasoning.is_empty()).then_some(reasoning);`, which this increment rewrites. Both are re-anchored on the one binding.
- A trim applied there now reaches both the trunk and the response. So `-trimmed` catches both the existing trunk test and the new response test.
- Round 1's proposed `drive.session-trims-the-responses-reasoning` duplicated it, and is dropped.

**Acceptance.** `cargo test -p discipline-diet -- drive` and `cargo test -p discipline-diet --test drive_serve_cli` exit 0, and each fault exits 101 on its signature.

### R3.5: the journal, the record probe and the record request (track three; the bump is track one's)

**The change.**
- `wire::parse` reads the `timings` object with its digits.
- `Client` pushes a timings entry.
- `lost()` gains the timings item with its `(row, key)`, and rewords `response.cache` (D8).

**Tests.**
- `a_llama_cpp_reply_journals_its_timings` (in `client::tests`), on C5's bytes.
- `every_unspellable_item_is_one_the_record_refuses` (D8), which reads `formats::record`'s own reader.

**Faults.**

| Fault | What it changes | Where declared |
|---|---|---|
| `client.journal-drops-timings` | `Client` pushes no timings entry | `diet/client/gate.toml` |
| `client.unspellable-item-the-record-spells` | **the record's reader** is taught to take a `timings` member on `response` (`record/mod.rs`, in the `Response` arm), which is exactly the event this probe guards. Seen red as "`lost()` names `response.timings`, which the record now spells". | `diet/client/gate.toml`, targeting track one's file; the target is flagged to track one when it is wired |

**The record bump itself** (record `response.timings`) is requested on the issue Q10 names, and lands by courier when ruled. It is not part of R3's acceptance, but R3.5's probe fails the day it lands until the projection writes it.

**Acceptance.** `cargo test -p discipline-diet -- client` exits 0. `client::journal` matches no test, so round 1's filter is replaced. Each fault exits 101 on its signature.

### R3.6: the surface (track five)

The surface does three things:
- **Adopts `log.ts` v1.**
- **Draws the footer from `response.timings`, and the header count from `progress`.**
- **Reads `context_overflow`.**

It also drops any zero it writes for an absent timing (Q12). What it draws, and how, is its own.

### R3.7: live acceptance (track three; the run is in the data seat's window)

**The run.** `diet-drive serve` drives two turns against the DoD 1 instance:
- a cold turn 1, whose head spans at least three `-b 1024` batches and is made cold by a unique prefix;
- then a warm turn 2.

**The tap.** `diet-drive`'s endpoint points at a recording relay in the capture script. The relay forwards both ways and writes **only the server-to-client bytes**, because the other direction carries the key.

**The receipts** go under `diet/drive/fixtures/`, which is track three's. They are not placed under `diet/formats/log/fixtures/`, so the conformance suite and track one are not involved.
- the SSE `data:` lines as a log;
- the relay's server bytes.

**The gate.** `the_dod1_log_carries_timings_progress_and_a_warm_turn`, in `serve.rs`'s tests, reads the committed log and requires:
- that it parses as v1 through `log::parse`;
- timings on both responses;
- `cache_n > 0` on turn 2;
- **at least two** `progress` lines before turn 1's first delta, with `processed` strictly rising, so a single sweep-shaped frame fails.

**What this is.** It is a **receipt check**: it proves that the committed run met DoD 1 and DoD 2. No later session regression turns it red; R3.4's tests are what do that.

**Acceptance.** `diet check-log diet/drive/fixtures/<the log>` exits 0, and that test exits 0.

---

## 4. Questions for planning and the maintainer

Round 1's Q2 (nested or flat) and Q4 (`progress` or `prompt_progress`) are removed. The adopted precedence rule decides both, and D1 and D2 say so. The numbering is otherwise kept.

1. **One bump, or two?** v1 could carry timings and reasoning now, with v2 carrying progress and overflow after the captures. *Recommended: one bump, taken after R3.0, if the captures land before R3.1 and R3.4 are done. Two only if R3.0 stalls, because every bump is a courier round trip and a surface migration.*
3. **Carry `draft_n` and `draft_n_accepted`?** *Recommended: yes, both optional. They are the server's in-band evidence that the answer came off a speculative path, and the registry already records their run-to-run variation (`registry.toml:511`).*
5. **If neither stream frames nor `/slots` carry a prefill count on the fork, does planning amend DoD 2, or does R3 stay open?** *Recommended: R3 stays open, and DoD 2 is recorded as partial with the sources tried. Planning amends the item only if no measured source exists. A sweep is never drawn in its place.*
6. **`decoded`?** *Recommended: refused unless a capture shows a server field carrying it. Counting deltas is computed.*
7. **Carry the server's `usage.prompt_tokens`, so a footer's prompt total is the server's number rather than a sum?** *Recommended: no, unless the surface asks for a total. The footer guidance no longer sums anything, and `prompt_n` and `cache_n` are the server's parts.*
7b. **The consumer says `calls_from` was "ruled into R3's scope" on #117. Does the refusal stand?** *Recommended: yes, as refused for now. There is no tool loop, the transport reads no `tool_calls`, and no per-chunk timing has been captured. If the ruling stands, it is carried at the tool loop as the server's per-chunk numbers, never as a client split.*
8. **Refuse the system prompt's token count for R3?** *Recommended: yes. If it is wanted, it is its own increment: `/apply-template` and `/tokenize` at open, valid only after a capture shows the rendered head is a token prefix of turn 1's prompt.*
9. **Refuse v1 content in a log that declares 0?** *Recommended: yes, in `check`. `line()` reads the union.*
10. **Which record issue carries the `response.timings` bump (#82 or #92)?** *Recommended: planning's call. The code cites #82 for a record clock (`client/cache.rs:46-49`) and #92 for prefix reasons (`client/head.rs:64`); I could read neither issue. The record carries the reuse count once, in `timings.cache_n`, for llama.cpp (D8).*
11. **Does R3 make the interactive session write a record?** *Recommended: no. DoD 5 is met by the journal, the probe and the named record bump.*
12. **The consumer writes `0` for absent timings in its recorded fixtures (`migrate-recorded.py:73-76`, `canned.ts:180`, unmerged).** *Recommended: track five makes them absent when it adopts v1.*
13. **Who takes C0–C5, and who owns the window?** *Recommended: the data seat, as with Q10. It coordinates the window on the shared server, and reads the key the way `capture.py` does, never printing it.*
14. **Who adds the new canned registry entry, and who owns `diet/tests/`?** *Recommended: the data seat adds the entry (the coordinator's ruling). No test in this plan lands in `diet/tests/` except extensions to `drive_serve_cli.rs`, which R2c's I5 already put with track three.*
15. **Is C5 in scope?** *Recommended: yes. It is one cheap request, and without it the dialect test runs on composed bytes.*

---

## 5. Risks, and what I did not check

**No llama-server ran.** Every claim about the server rests on the two committed captures and on `ab.json` and `turn1.json` from Q10.

- **Rests on the captures alone:**
  - the placement of `timings`;
  - its key names on `e7051ef` and `4df29be`;
  - `draft_*` appearing only when speculating;
  - the absence of any progress frame when progress is not asked for.
- **Rests on `ab.json` (unstreamed, three rows, one slot, one sequence):** `prompt_n` and `cache_n` are disjoint and sum to `prompt_tokens`.
- **Recollection, not checked:** upstream llama.cpp's `return_progress`, `prompt_progress {total, cache, processed, time_ms}`, `timings_per_token`, `exceed_context_size_error`, `/props`, `id_slot`, the contents of `/slots`, how often frames come, and how the unified KV cache fails. The instance is a fork, so R3.0 exists for exactly these.

**What I measured.** In the scratchpad, a small binary parsed every `timings` object in both captures with `serde_json` + `arbitrary_precision`, and checked that each number re-renders as the text the server wrote.
- **With the feature:** all 20 numbers matched, and the probe exited 0.
- **Without the feature:** the captures' digits matched too. A seeded `1.10` did not match (exit 1), while with the feature it matched (exit 0).
- **So:** the feature, already on in `diet/Cargo.toml:24`, is what guarantees digits as written, and R3.1's test must use a seeded value.
- **Reproduced by the critique,** with exit codes read directly. The critique also measured `2.9e2 → 2.9e+2`, which `Decimal::new` refuses.
- **Byte counts:** the log-size figures in D2 and D4 were computed by a script that rendered the capture's 309 pieces as v0 lines. They are not measured from a running drive.

**Not re-run in round 2.** The critique's canned-digest measurement (`8947e695…` before the change, `188c4004…` after) is cited, not reproduced.

**Risks.**
1. **No measured prefill source may exist.** The fork may emit no progress frames, and `/slots` may carry no prefill count. Then R3 stays open on DoD 2 (Q5). The overflow may also carry no typed field, in which case `context_overflow` is refused.
2. **An exponent from the server cannot be carried** (`number.pest`), so such a timing is absent. The server's values in the captures are nowhere near an exponent's range, but nothing guarantees it.
3. **The model is hybrid recurrent.** "warm reuse after a divergence depends on where checkpoints land" (`registry.toml:511`), so `cache_n` is the server's truth about reuse, and a warm-versus-cold reading of it is the surface's interpretation.
4. **`predicted_n == usage.completion_tokens` in both captures** (312, 6), but that equality is not a rule. The record's `output_tokens` comes from `usage.completion_tokens` (`wire.rs:330`), and I propose no mapping between the two.
5. **The captures disturb a shared server.** Slot occupancy and KV eviction mean C1–C3 and R3.7 affect other users, which is why they run in the data seat's window. C3's error could be the full-cache kind rather than the per-slot kind. It is recorded apart if so.
6. **The reader checks neither `response.text` nor `response.reasoning` against their deltas.** A writer that trims one would pass the format. Only R3.4's session test and its fault catch it.
7. **The canned change moves a pinned identity.** D9 contains it. If the data seat's registry entry lags the PR, `every_registered_canned_digest_is_acts_this_crate_plays` fails. That failure is the intended signal, but it couples the PR to the data seat's edit.
8. **One fault targets another track's file.** `client.unspellable-item-the-record-spells` mutates `record/mod.rs`, which is track one's. Track one must agree to wire a lane fault whose target is its own file.
9. **The consumer branch I read is unmerged and may have moved** since `0381fb2`.

**Not checked:**
- the text of #82, #92, #117 and #140;
- how many progress frames a real prefill emits;
- whether sending a progress flag changes anything else in the server's reply;
- the per-slot context limit under `--kv-unified`, which is C0;
- whether the consumer's TypeScript compiles against the regenerated bindings;
- whether `drive_cli.rs`'s pinned cache-census numbers change after the gym moves to the new canned instance. They should not, because the bodies move with the reader; not run.

---

## Revision log (round 2)

The coordinator's rulings on M1–M6 are adopted as stated.

| # | Finding | Response | What changed | Evidence if rebutted |
|---|---|---|---|---|
| 1 | Major: the dialect fix silently moves the canned substrate's identity | accepted (with the ruling) | New D9: the corrected acts are a new canned instance; the old acts are kept and never mutated; the registry edit goes to the data seat; the gym, `diet-drive canned` and `dev-loop.toml:51` move in the same PR; old records and `canned-substrate.jsonl` keep the old digest; two tests make the registry citation checked, with a fault on the old acts; risk 7; Q14 | — |
| 2 | Major: the "goes red when the record grows" mechanism does not exist, and the acceptance filter matched 0 tests | accepted (built) | D8 and R3.5: `every_unspellable_item_is_one_the_record_refuses` probes the record's own reader for `UnknownField` per item, with `(row, key)` declared as data in `lost()`; fault `client.unspellable-item-the-record-spells` mutates the record reader; acceptance is now `-- client`; §1 describes today's self-comparing test | — |
| 3 | Major: the fallback refuses a definition-of-done item | accepted (with the ruling) | D2 gives the source order: stream frames, then `/slots` with its accuracy cost stated, then a request to planning to amend, with R3 open until then; Q5 reworded; risk 1; "In short" | — |
| 4 | Major: mechanisms without faults, one without a test | accepted | Progress-flag test and fault (R3.3); session progress-ordering and overflow faults (R3.4); injections for a non-request citation, negative count, negative ms, ms as text and the nested walk (R3.2); `client.journal-drops-timings`; `client.stream-spells-an-unspellable-timing`; faults for the cancel test, the poller and the old canned acts | — |
| 5 | Major: the plan breaks existing lane faults | accepted | `client.cache-sniffed` re-anchored to sniff `cache_n` (so it is red again, never pointing at a dead key); `client.streaming-body-drops-usage` re-anchored on the new `format!` line; both reasoning faults re-anchored on one binding, and the near-duplicate dropped; `faults =` counts updated; all `catches` regenerated by running; wiring rows for track one | — |
| 6 | Major: C3 does not fit the instance, and C4 could commit another session's text | accepted (with the ruling) | R3.0 rewritten: C0 reads the per-slot limit first; C3 is the smallest prompt over that limit, sent once in a window the data seat coordinates, with the reasons for both; the full-cache error is recorded apart; slot and KV disturbance stated; C2's slot pinning or recorded outcome; C4 redacted to key names and own-slot counts; the need for a script stated | — |
| 7 | Minor: version scoping cannot live in `line()` | accepted | D7: `line()` reads the union, scoping lives in `check`, the fixtures prove it through `parse`, and a resuming surface's limits are stated; Q9 | — |
| 8 | Minor: dropping `usage` leaves the footer to compute a total | partly | The footer guidance no longer sums anything; carrying `usage.prompt_tokens` is new Q7. It is not carried by default, because nothing asks for a total | — |
| 9 | Minor: timings are not tied to the `choices: []` chunk | accepted | D1 "Which chunk": any chunk, the last one wins, delivered at `[DONE]`; a test for the no-`include_usage` shape; C1 records where frames sit | — |
| 10 | Minor: `a_cancelled_turn_has_no_timings` cannot fail | accepted | Replaced by a transport test (a timings chunk, then a cancel before `[DONE]`) with its fault | — |
| 11 | Minor: the DoD 2 gate passes on a single frame | accepted | R3.7: at least two frames with strictly rising `processed`, on a head spanning three or more `-b 1024` batches made cold by a unique prefix; stated as a receipt check; fixtures under `diet/drive/fixtures/`; the tap is a recording relay that writes only the server's direction | — |
| 12 | Minor: the schema test does not reach nested keys | accepted | R3.2 extends the test into nested fields, with an injection; the `HeadMessage` precedent is cited, and §1's wrong statement is corrected | — |
| 13 | Minor: ms has no invalid-value fixture | accepted | ms is non-negative (D1); a negative-`prompt_ms` fixture and injection | — |
| 14 | Minor: the record would spell the reuse count twice | accepted | D8: carried once, in `timings.cache_n`, for llama.cpp; `response.cache` reworded to other dialects and not demanded now | — |
| 15 | Minor: the dialect test runs on composed bytes | accepted | C5 (a raw unstreamed reply) added; the dialect and journal tests replay it; Q15 | — |
| 16 | Minor: the refusal of `calls_from` does not address a claimed ruling | accepted | D3 names the claimed ruling; new Q7b to the maintainer | — |
| 17 | Minor: the table omits track one's wiring | accepted | "Wiring by one" in each row; the rule stated once above the increments | — |
| 18 | Nit: `diet/wasm` has no log reader | accepted | Clause dropped in D7 | — |
| 19 | Nit: the frame-count estimate may be off by half | accepted | D2 gives both (`-ub` and `-b`), and C1 decides | — |
| 20 | Nit: `VERSION` needs to split | accepted | `VERSION` (writes) and `READS` (reads) in D7 and R3.2 | — |
| Q+/Q− | The critique's question list | accepted | Q2 and Q4 removed as decided by the rule; added Q5 (reworded), Q7, Q7b, Q13 (window), Q14 and Q15, plus the record-field choice folded into Q10 | — |
