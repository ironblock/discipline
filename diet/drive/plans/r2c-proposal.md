# R2c proposal: the HTTP + SSE surface of the interactive drive loop

This is a blind proposal from the proposing session, dated 2026-09-27. It is based on `origin/main` at `178c012`. I read the consumer's current shape on `origin/feat/exercise-r1-surface`, which is unmerged. I read no issues, PRs, comments or other branches, and used no web access.

**In short.** The plan serves one R2a `Session` over HTTP/1.1 built on the standard library alone:

- `GET /events` replays the log from a sequence number and then tails it as SSE, with one log line per `data:` field.
- `POST /commands` carries the commands the session already has. A refused command gets a `409` carrying the refusal's tag, and the refusal also stays in the log, as R2a logs it now.
- Every SSE payload is a line of a new format, `diet/formats/log` v0. One implementation in `diet/src/formats/` reads and renders it. That format is track one's, applied from a courier patch.
- The session gains the parts that the rulings place on the surface: a turn counter that blocks a stale cancel, `turn.settled {turn, reason}`, a `session.start` line that carries the version, and a time stamp on every line.
- No new dependency. A throwaway prototype of this HTTP layer over the real `Session` is 313 lines and passes clippy pedantic. Its seeded fault fails the test.
- Everything below unblocks DoD step 1 only. DoD steps 2 to 5 need nothing from R2c beyond a stream that does not care which event kinds it carries. Auth, the idle-gap intake and even cancel wait, going strictly by the DoD (Q4, Q6, Q7).

---

## 1. What exists

**R2a: `diet/src/drive/session.rs`**

- **Vocabularies.** The settlement (`awaiting|turn|capture|ended`), the command kinds (`ask|cancel|declare-seam|end`) and the refusals (`in-flight|ended|nothing-in-flight|seam-not-built`) are tagged `vocabulary!` enums: :61-101. The client's macro generates `ALL` and `tag` but no `from_tag` (`diet/src/client/mod.rs:115-138`).
- **The log.** `Event` has 10 variants, and each is stored as `Logged {seq, event}`: :103-183. `seq` is the event's position in the log and has no gaps: `State::push` :193-197. There is no time stamp, no turn number and no header line.
- **`ask`.** It refuses during `turn` and during `capture` with `in-flight`, logs the refusal and never queues the ask: :284-298. It returns the seq of the `Asked` event: :299, :336.
- **`cancel()`.** It takes no argument and stops whatever turn is in flight: :345-364. During `capture`, `state.cancel` is `None` (:481), so a cancel there is refused as `nothing-in-flight` (:359). Consequence: a cancel meant for turn N that arrives after turn N+1 was admitted stops turn N+1. Nothing in-process reaches this today. Over HTTP, network latency or a second tab can.
- **Other commands.** `declare_seam` is always refused with `seam-not-built` (:372-383). `end` is at :391-406.
- **Reading the log.** `events_from` and `wait_from(first, patience)` return owned `Vec` copies and wait on the session's condvar (:420-446). `from` returns nothing for a start past the end (:460-463). Readers never hold the session's lock, because `Shared::lock` is private (:223-227). `wait_from` is the SSE tail primitive.
- **Capture.** `capture` is passed straight through (:492-495), so no test can hold a session in `capture` until R4 exists.
- **How a turn ends.** The five terminal outcomes are Answered, Cancelled, Rejected, Failed and Crashed (:138-173, set at :482-513 and :519-528). A cancelled call never joins the trunk and is never an answer (:29-33).
- **Serialization.** The log exists only in memory: "how it is serialised ... is an open question on #117" (:49-50).
- **Seeded faults.** Ten `drive.session-*` faults guard this code: `diet/drive/gate.toml:833-981`.

**R2b: `diet/src/client/stream.rs`** (this plan leaves it unchanged)

- **Cancel.** `Cancel` is a flag plus stoppers, and a stopper registered after the cancel runs at once: :51-121.
- **`HttpStream`.** It was measured against llama-server `4df29be`. Its cancel shuts the socket, which is the only thing that frees the server slot: :170-193 and :261-277. It reads only `delta.content` (:514-518), so reasoning deltas are never delivered.
- **Test doubles.** `Canned` and `Gate` drive a session with no server and no sleep: :667-820.
- **Captured reply.** `diet/client/fixtures/llama-server-4df29be-stream.http`, whose pieces are pinned at :931-937.

**Formats (track one)**

- **The convention.** One grammar; `fixtures/valid|invalid` with an `.expected.json` or a `.reason` beside each case; one implementation; an entry in `FORMATS`; a name in the harness's `per_format!`. Sources: `diet/src/formats/mod.rs:3-20` and `:57-93`, `diet/tests/conformance.rs:1-25`, `diet/AGENTS.md:13`.
- **Versioning.** A version is the grammar's header line (for example `diet/formats/record/grammar.pest:1`, "v0"), and a format changes only by a versioned bump (`diet/AGENTS.md`, GRADING).
- **The record's value space.** A JSON subset with no null, no binary floats and no exponents (`record/grammar.pest:1-24`).
  - `json::line` is "the one reader for data files that are not records" (`diet/src/formats/record/json.rs:362-395`).
  - `render` sorts keys and escapes `\n` and `\r` (`json.rs:402-452`), so a rendered line never contains a raw line break.
- **A schema grammar over another format's terminals.** Precedent: `operating_points/grammar.pest:1-7`. Rules that span rows are checked in Rust, under a name: `:38-43`.
- **The record.** It splits syntax (the grammar) from schema (Rust): `record/mod.rs:1-5`. It states its regime once, in `start`: `:13-18`. Its `rejected` kind means a groundedness-floor rejection (`record/mod.rs:528-529`).
- **Readers outside the library.** Only CLI verbs (`diet/src/bin/diet.rs:73-87`) or wasm pass-throughs (`diet/wasm/src/lib.rs:1-13`, `:53-62`). "A foreign reader" is forbidden (`diet/AGENTS.md:14`).

**Gates**

- **Lane faults.** Each lane keeps them in `diet/<lane>/gate.toml`, registered in `tools/gate/lanes.toml` (drive and client are registered). `check_lanes` applies them (`verify.sh:327-342`), and selftest cases are generated from the manifest, so a new fault needs no edit to `verify.sh`.
  - A lane PR registers its own fault ids in `tools/gate/faults.toml` (entry shape: `:5868-5873`).
  - The drive lane's command filters tests by `-- drive` (`diet/drive/gate.toml:25-29`), so an integration test's name must contain `drive` (as `diet/tests/drive_cli.rs:86` does).
- **Lints.** `cargo clippy --workspace --all-targets -- -D warnings` (`verify.sh:113`) with pedantic on (`Cargo.toml:16`) and `unsafe_code = "forbid"` (`:12`).
- **No string-literal match arms.** No match arm anywhere in `diet/src` may have a string literal in its pattern (`scripts/check-library.py:4-19`). Routing therefore has to be a table.
- **A hygiene false positive.** The hygiene table reads the method-call form of `local_addr` on a listener as a hostname; write `TcpListener::local_addr(&listener)` instead (`diet/src/client/stub.rs:102-108`).
- **Dependencies.** Today diet depends on `pest`, `serde_json` (for foreign formats only) and `sha2` (`diet/Cargo.toml:18-39`). `Cargo.lock` lists 37 packages.

**Entry points**

- `diet-drive` runs only the scripted gym. `regime_of` refuses every real endpoint, because a substrate's identity cannot be resolved yet (`diet/src/drive/regimen.rs:55-64`). The gym's request-shape defaults are at `diet/src/bin/drive.rs:388-417`.
- A loopback stub server can serve raw bytes, hold a stream open and record hang-ups (`diet/src/client/stub.rs:1-16`, `:54`, `:61`).

**The consumer: `origin/feat/exercise-r1-surface`** (unmerged and provisional)

- **The interface** (`exercise/src/drive/transport.ts:19-58`):
  - `DriveTransport {subscribe, dispatch, watchLink?}`.
  - `Command = ask{text} | cancel | seam{to}`.
  - `Refusal` is an open set: `busy|ended|nothing-to-seam|nothing-to-cancel|off-script|recording`.
  - `Ack` and `Link`.
- **The events** (`exercise/src/drive/events.ts`):
  - every event has `At {seq, t}`: :56-61;
  - `session.start {arm, model, slots, trunk_slot, phase, system}`: :63-74;
  - `ask {turn, text}`: :77-81;
  - `delta {request, text?, reasoning?}`, "never recorded": :93-99;
  - `response {id, to_request, text, stop, timings}`: :118-126;
  - `request.failed {request, reason, message}`: :135-144;
  - `turn.settled {turn, reason}`: :166-172;
  - `session.end`: :266-268.
  - The file tags delta and streaming as R3 (:21, :300).
- **Its canned cancel** emits a `response` with `stop: 'cancelled'` (`exercise/src/drive/canned.ts:147-169`).
- **Session state is inferred from events, not read from the log:** `connecting|awaiting|turn|capture|ratify|ended` (`exercise/src/session/fold.ts:187`, `:592-600`). A turn closes on `turn.settled` (`:392`). Unknown kinds are counted (`:459`).
- **`useSession`** appends every event without dedupe (`exercise/src/session/useSession.ts:9-19`).
- **Its own rule:** "No second parser: anything that reads a `diet` format goes through `diet`" (`exercise/AGENTS.md:10`).
- **`capture.cancelled`.** The recorded `cancelled-capture` session carries `capture.cancelled {id, turn}` once, after the last `patch` (`exercise/src/drive/recorded/cancelled-capture.json`; see also `exercise/src/drive/recorded.ts:41`).

---

## 2. Decisions

### D1. The HTTP layer: add a dependency or not

| | std only (`std::net`, one thread per connection) | `tiny_http` 0.12 (sync) | `hyper` / `axum` + `tokio` |
|---|---|---|---|
| Build | nothing added | +1.83 s for a clean debug build; 4 new packages (`log` is already in the lock) | not measured: not in the offline cache, and web access was excluded |
| Code to audit | ours, about 300 to 500 lines | 3.7k lines in tiny_http (which forbids unsafe code); 14.7k lines across its 5 crates; `unsafe` in `ascii`, `httpdate` and `log` | the largest of the three, plus an async runtime |
| Lints | pedantic applies to all of it | pedantic was clean on a 50-line SSE sketch (only our code is linted) | not measured |
| What it saves | nothing | reading the request line, headers and body: about 70 of the prototype's 313 lines | the same, but the sync `Session` (a condvar, `session.rs:430-446`) would have to be bridged into the runtime |
| What it does not do | n/a | the hard part: tailing on the session's condvar, heartbeats, noticing that a reader has gone, mapping refusals, auth | the same |

**Measured.** I built a throwaway std-only surface over the real `Session` and `Canned` (in `scratchpad/exp/sse-std/`). It covers:

- replay then tail, and resuming with `Last-Event-ID`;
- a cancel over HTTP reaching a call held at a `Gate`;
- a `409` for an ask sent mid-turn;
- Basic auth that hashes both sides and then folds;
- heartbeats.

Results:

- **Size.** 313 lines of code and 154 lines of tests.
- **Lints.** `cargo clippy --offline --all-targets -- -D warnings`, with the workspace's lint table copied in, exited 0. A planted lint was caught, so the check really ran.
- **Tests.** All 3 pass.
- **Seeded fault.** Replacing the cancel handler with a no-op exits 101. That required first fixing a hang in the test reader; see §5.
- **A reader that leaves.** A reader that closed its connection was released 201 ms after it closed, at a 100 ms heartbeat. That is two heartbeats, because the first write after the close still succeeds.

**Recommendation: std only.** The accepted v1 default stands. A dependency would buy only request parsing, and the request surface here is small: loopback, one request per connection, `Connection: close`, and size caps. Revisit this if a second server-side need appears, such as TLS or HTTP/2.

### D2. Where the log's schema lives

- **(a) Make R2a's `Event` the format's type** by moving it into `diet/src/formats/log.rs`. There would be one enum. But the format would then depend on `client::transport::TransportFailure` (`session.rs:58`, `:168-173`), or else lose the typed failure that R2a's tests assert (`session.rs:816-820`). Every later change to R2a would also become a courier patch.
- **(b) Give the format its own line type** with `render` and `parse`, and have `drive` convert `Logged` into a line with one exhaustive `match`. A session variant with no line then fails to compile, and a round-trip test proves every variant can be read back.

**Recommendation: (b).** Formats stay a leaf module and R2a keeps its types. The conversion is track three's code; the reader is track one's. Precedent: `drive` builds `formats::record` values and parses its own output back (`diet/src/drive/mod.rs:114`, `:1193`).

### D3. The log's syntax

- **(a) The record's value space**, one object per line, read with `json::line`. `diet/formats/log/grammar.pest` becomes a schema grammar concatenated after the record's grammar (the operating_points precedent). The schema, and the rules that span rows (seq has no gaps; a settlement edge's `from` equals the previous `to`), are checked in Rust.
- **(b) The log's own copy** of the JSON-subset rules. That is two texts for one syntax.
- **(c) `serde_json`.** Its manifest entry reserves it for foreign formats (`diet/Cargo.toml:20-24`).

**Recommendation: (a).** This is track one's decision.

### D4. The vocabulary

- **(a) Derive the tags mechanically** from R2a's variant names (`asked`, `settled`, `stop_asked`, `answered`, `rejected`, and so on). This is the smallest change. But `rejected` collides with the record's `rejected`, which means something else, and `settled` would sit next to the ruled `turn.settled`.
- **(b) Take the consumer's provisional names wholesale.** Its `response` for a cancelled call contradicts R2a (`session.rs:29-33`). It also has no settlement edges and no refusals, which the ruled state machine requires and R2a already logs.
- **(c) Apply one precedence rule, always keeping R2a's semantics.** Take the name from the first of these that has one:
  1. a ruling;
  2. the record, where its concept means the same thing;
  3. an existing tag in `session.rs`;
  4. the consumer's provisional names, where the concept means the same thing;
  5. R2a's own word.

**Recommendation: (c).** Applied to R2a's events, the rule gives the draft below for the courier patch. Every line also carries `seq`, `kind` and `t`.

| R2a event (`session.rs`) | log v0 `kind` | fields | where the name comes from |
|---|---|---|---|
| none; new, pushed when the session opens | `session.start` | `version`, `model`, `head` (`[{role, content}]`) | the consumer; the record's `start` means "carries the regime" (`record/mod.rs:13-18`) |
| `Asked` :107 | `ask` | `turn`, `text` | the existing tag `CommandKind::Ask => "ask"` :79 |
| `Settled` :112 | `settlement` | `from`, `to` | the noun in the ruling ("a settlement state machine"); see Q1 |
| `Refused` :119 | `refused` | `command`, `because`, `during` | R2a |
| `Delta` :128 | `delta` | `text` | R2a and the consumer agree |
| `StopAsked` :137 | `stop.asked` | `turn` | R2a |
| `Answered` :139 | `response` | `text`, `finish_reason?` | the record (`record/mod.rs:515`) |
| `Cancelled` :146 | `cancelled` | `partial` | R2a; the consumer's cancelled `response` contradicts `session.rs:29-33` |
| `Rejected` :152 | `request.failed` | `reason: server`, `status`, `message`, `partial` | the consumer; the record's `rejected` means something else |
| `Failed` :168 | `request.failed` | `reason: timeout \| transport`, `message`, `partial` | the consumer |
| `Crashed` :163 | `request.failed` | `reason: crashed`, `message` | the consumer |
| none; new | `turn.settled` | `turn`, `reason` (`final \| cancelled \| max_steps \| timeout \| failed`) | ruled |
| none; new, intake waits (W2) | `idle.gap` | `notice`, `read`, `compose`, `away` | ruled |

`turn.settled.reason` is derived as follows:

- a `response` settles the turn as `final`;
- `cancelled` as `cancelled`;
- `request.failed` with reason `timeout` as `timeout`;
- `request.failed` with reason `server`, `transport` or `crashed` as `failed`.

`max_steps` is in the vocabulary because a ruling names it, but only the tool loop will emit it. Within a turn the order is: the terminal event, then `turn.settled`, then the settlement edge.

### D5. Where the version lives

- **(a) In `session.start` at seq 0**, as a `version` field. This follows the record, which states its regime once (`record/mod.rs:13-18`). A reader refuses a version it does not implement. A reader that resumes mid-log has already read line 0.
- **(b) On every line.** That is redundancy that can disagree with itself, which is the record's own argument against it.
- **(c) Outside the log**, in the HTTP path or a header. A log file on disk would then not carry its version.

**Recommendation: (a).** The cost: every R2a seq moves up by one. Tests that assert `ask` returns `Ok(0)` change, and the `catches` lists of the ten `drive.session-*` faults are regenerated.

### D6. Stale cancel (ruled: "blocked by an admission counter")

- **(a) A 1-based `turn` counter.** It increments on each admitted ask and is carried on `ask`, `stop.asked` and `turn.settled`; the command becomes `cancel(turn)`. It matches the record's `turn.index` and the consumer's `turn`.
- **(b) Cancel by the seq of the targeted ask.** `ask` already returns it (`session.rs:299`), so no new counter is needed. But `turn.settled` still needs a turn to name.

**Recommendation: (a).** `Session::cancel` takes the turn as an argument, so there is no second path that skips the check. A cancel is then handled by which turn it names:

- a turn older than the one in flight: refused with a new tag, `stale` (the ruling's own word);
- the latest turn, with nothing in flight: refused with `nothing-in-flight`;
- a turn that was never admitted: a `400`, because it is not a valid command.

The client must take the turn from the log, not from its own reply, because a second tab may have asked since.

### D7. SSE framing and resume

- **Framing.** Each event is `id: <seq>` plus exactly one `data:` line, which is one log line; `render` guarantees it contains no line break. `seq` also appears inside the data, because it is the format's key and a log file on disk has no SSE framing.
- **No `event:` field.** With a per-kind `event:` name, `EventSource` silently drops any kind the page registered no listener for. The consumer counts unknown kinds (`fold.ts:459`), so it needs them delivered.
- **Resume.**
  - `Last-Event-ID: k` resumes from k+1; this is what `EventSource` sends when it reconnects on its own.
  - `?from=n` starts at n; this is what a first connection can say.
  - If both are present, the header wins. A start past the end of the log waits there, like a tail.
- **Heartbeat.** A comment line (`:`) every 15 s, configurable for tests. Only a write reveals that a reader has left; measured at two heartbeats.
- **Options rejected.** A per-kind `event:` field (above). Resuming by query string only: `EventSource`'s own reconnects would then replay from 0 and rely on dedupe alone.

### D8. Commands

- **(a) One route, `POST /commands`**, with the body `{"kind": <CommandKind tag>, ...}`. This mirrors the consumer's `dispatch(command)` and R2a's `CommandKind` (`session.rs:75-87`). The kind is found by a lookup over `ALL`.
- **(b) One route per command.** The cost is the same.

**Recommendation: (a).** Replies:

| Case | Status and body |
|---|---|
| `ask` accepted | `200 {"seq", "turn"}` |
| any other command accepted | `200 {}` |
| any `Refusal` | `409 {"refused": tag}`, and the refusal is logged, as R2a logs it now |
| malformed body, unknown kind or unknown key | `400`; it is not a command, so it is not logged |
| missing or wrong credential | `401` |
| no such route (the table is keyed by method and path) | `404` |
| head over 16 KiB or body over 1 MiB | `413` |
| connection cap reached | `503` |

Every refusal gets the same status because the tag already carries the reason. The ruled `409` for a prompt sent during capture follows from that.

### D9. How the browser reaches it

- **(a) Vite's dev proxy** (`server.proxy`). The page and the server share an origin, so diet needs no CORS code. This is track five's configuration.
- **(b) CORS headers in diet** for a configured origin. JSON POSTs trigger a preflight, and credentials add rules of their own.
- **(c) diet serves the built SPA itself.**

**Recommendation: (a) now.** (b) and (c) wait.

### D10. Auth and bind address (ruled: loopback by default, optional constant-time Basic auth)

- **Bind.** Listen on `127.0.0.1` unless `--listen` says otherwise.
- **Credential source.** A file containing `user:password`, not an argument, which `ps` would show. An environment variable is the alternative. Recommendation: a file.
- **Comparison.** Hash both sides with `diet::digest::sha256` (`diet/src/digest.rs:87`), then fold the 32 bytes with no early exit. Compare the scheme case-insensitively, since it is not secret, and hash only the token. The expected token is base64-encoded once at start. Only encoding is needed; test it against the RFC 4648 §10 vectors. The alternative, the `subtle` and `base64` crates, is available offline but not needed.
- **Off loopback without a credential.** Either refuse to start (fail closed) or allow it (the ruling says auth is "optional"). Recommendation: refuse. See Q7.
- **Remote screens.** Basic auth over plain HTTP between hosts sends the credential in cleartext. For a screen on another host, an SSH tunnel keeps diet on loopback and makes auth unnecessary.

### D11. Concurrency and limits

- **(a) One thread per connection** (this is what I measured), with:
  - a connection cap (for example 32; past it, `503`);
  - a 10 s read timeout per request;
  - a 10 s write timeout on each stream, so a reader that stops reading is dropped. It can never stall the session, because readers get owned copies (`session.rs:430-446`).
- **(b) A fixed thread pool.** A stuck SSE reader would hold one of its workers.
- **(c) Non-blocking I/O.** This needs mio or a hand-rolled poll loop.

**Recommendation: (a).**

### D12. The entry point

- **(a) A subcommand in `diet/src/bin/drive.rs`**: `diet-drive serve --endpoint URL --model NAME --head FILE [--listen ADDR]`. No change to `Cargo.toml`.
- **(b) A new `[[bin]]`.** This touches `diet/Cargo.toml`.
- **(c) Configure it from a regimen, as the gym does.** Today that path refuses every endpoint (`regimen.rs:55-64`).

**Recommendation: (a).** The head comes from a plain file, and the limits are the gym's defaults (`bin/drive.rs:402-407`). The first thing the binary prints is the address it bound. Who owns the binary is Q3.

### D13. The `idle.gap` intake (ruled; waits)

- **(a) A field on the `ask` command.** The gap is complete at the moment the ask is sent. It is appended under the same lock, immediately before the `ask` event, and only if the ask is admitted.
- **(b) A POST of its own.** Its order relative to the ask then depends on the client waiting for the first POST before sending the second.

**Recommendation: (a).** Put the kind into log v0 now, which avoids a later version bump; the intake itself waits (W2). The units and meanings are Q6.

### D14. `t` on every line

- **(a) Milliseconds since the session opened**, from a monotonic clock, stamped together with `seq` under one lock, so `t` never decreases in seq order.
- **(b) No time stamp until R3.** The consumer requires `t` (`events.ts:56-61`).

**Recommendation: (a).** One field now is cheaper than a version bump later. See Q5.

### D15. How the surface reads a line

- **(a) A wasm pass-through, `check_log_line`,** beside `check_record`. It is conformance-equal to the native reader by #78's gate.
- **(b) A TypeScript reader** that passes the shared corpus.
- **(c) `JSON.parse` plus generated types.** That is a foreign reader (`diet/AGENTS.md:14`; `exercise/AGENTS.md:10`).

**Recommendation: (a).** Track five and the owner of the wasm crate decide. R2c's obligation either way: every `data:` field is exactly one line that `formats::log` accepts (tested in I4).

### D16. `capture.cancelled`

- **(a) Pin it as invalid in log v0.** A stop during capture is `stop.asked {turn}` followed by `settlement capture→awaiting`. How each fork ended is for R4 to say.
- **(b) Add it as a kind.** It would repeat what the settlement edge already says.

**Recommendation: (a)**, as an invalid fixture with a `.reason`. See Q8.

### Where this plan and the consumer disagree

1. **Refusal names.** The consumer's `busy` is diet's `in-flight`, and its `nothing-to-cancel` is diet's `nothing-in-flight`. Its `nothing-to-seam` ("no turn has settled") is not the same as diet's `seam-not-built`. This plan adds `stale`.
2. **The seam command.** The consumer sends `seam {to}`; diet has `declare-seam` with no argument until R6.
3. **Cancel.** The consumer's `cancel` names nothing. The plan requires `{turn}`: the stale-cancel guard is ruled.
4. **`end`.** diet has an `end` command; the consumer lists it as "not yet here".
5. **Refusals in the log.** The consumer carries refusals only in the `Ack`. diet also logs them as `refused` events, which the consumer will count as an unknown kind.
6. **Session state.** The consumer infers it from events and includes a `ratify` state (`fold.ts:187`, `:592-600`). diet logs `settlement {from, to}` explicitly over the ruled states, and in the ruled vocabulary `ratify` is a lane, not a state.
7. **A cancelled call.** The consumer's canned transport emits `response {stop: 'cancelled'}`. diet never makes a cancelled call a response: it logs `cancelled {partial}` followed by `turn.settled {reason: cancelled}`.
8. **`delta`.** The consumer's has a required `request` id and an optional `reasoning`. v0 has neither: there are no request ids until the tool loop, and `HttpStream` does not read reasoning (`stream.rs:514-518`).
9. **`session.start`.** The consumer's requires `arm`, `slots`, `trunk_slot`, `phase` and `system`. v0 carries `version`, `model` and `head`. None of arm, slots or phase can be stated honestly in R2c: `regime_of` refuses an endpoint (`regimen.rs:55-64`), slots arrive with R4 and phases with R6.
10. **`response`.** v0's has no `id`, `to_request`, `timings` or `stop`; it has `finish_reason`. `request` events and `progress` frames wait for R3 or the tool loop.
11. **`request.failed` reasons.** The consumer's are `server|context_overflow|timeout|disconnected`; v0's are `server|timeout|transport|crashed`.
12. **`session.end`.** The consumer has this kind; v0 expresses the same thing as `settlement → ended`.
13. **Dedupe.** `useSession` does not dedupe (`useSession.ts:14-17`). The ruling says clients dedupe by sequence, so `HttpTransport` must drop any event whose seq it has already seen before it calls a listener.
14. **`idle.gap`.** It is ruled, but it is absent from `events.ts`. The surface has to send it.
15. **`capture.cancelled`.** The consumer carries it as a kind of its own name; v0 rejects it (D16).
16. **Deltas in the record.** The consumer calls deltas "never recorded". In v0 they are in the log and are replayed on resume; the record projection drops them (Q11).

---

## 3. The plan

**Order.** I1 and I2 run in parallel; then I3, I4, I5 and I6 follow in that order. The table ranks every piece by the DoD step it unblocks.

| # | Work | Where | Track | Waits on | Unblocks |
|---|---|---|---|---|---|
| I1 | log format v0 | `diet/formats/log/`, `diet/src/formats/log.rs`, `formats/mod.rs:57`, `tests/conformance.rs`, `bin/diet.rs:73` | one (courier patch drafted by three) | Q1–Q3, Q5, Q8, Q9 | DoD 1 |
| I2 | session: start line, turn counter and stale refusal, `turn.settled`, `t` | `diet/src/drive/session.rs`, `diet/drive/gate.toml`, `tools/gate/faults.toml` | three | nothing to start; Q4 and Q5 to finish | DoD 1 |
| I3 | convert `Logged` to a log line, with a round-trip test | `diet/src/drive/session.rs` | three | I1, I2 | DoD 1 |
| I4 | the HTTP + SSE server | `diet/src/drive/serve.rs`, `drive/mod.rs` | three | I3 | DoD 1 |
| I5 | `diet-drive serve` and its CLI test | `diet/src/bin/drive.rs`, `diet/tests/drive_serve_cli.rs` | three (see Q3) | I4 | DoD 1 |
| I6 | `HttpTransport`, `events.ts` from v0, the Vite proxy; `check_log_line` in wasm | `exercise/`; `diet/wasm/` | five; wasm owner | I1 for the types; I5 to run live | DoD 1 |
| W1–W7 | below | | | | none |

**DoD 2 to 5.** R2c does no further work for these. Their events travel through the same stream: `serve.rs` never matches on an event kind, and I4's round-trip test streams every current variant. The events themselves come from the tool loop (DoD 2), R4 and R5 (DoD 3) and R6 (DoD 4 and 5).

**How every test is written.** Each test below is written RED first: it compiles against the new signature, with the behaviour absent, and fails on its assertion rather than on compilation. Every SSE test reader gives up on an overall deadline, never on a per-read timeout (see §5).

### I1: the log format v0 (track one; courier patch drafted by track three)

- **API.**
  - `line(&str) -> Result<Line, _>`: reads one line; used for a stream that resumes mid-log.
  - `parse(&str)`: reads a whole log and checks the rules that span rows.
  - `render(&Line) -> String`.
  - `project`: for `FORMATS` and the CLI verb `check-log`.
- **Valid fixtures:** a log that is only a header; one answered turn; one cancelled turn; one per `request.failed` reason; refusals, including `stale`; `idle.gap`; an ended session.
- **Invalid fixtures, each with a `.reason`:**
  - a gap in seq; a duplicated seq; a seq that does not start at 0;
  - a first line that is not `session.start`; an unknown version; an unknown kind; `capture.cancelled`;
  - a `settlement` whose `from` is not the previous `to`;
  - a `turn` that does not increase by 1 per `ask`; a `turn.settled` naming a turn never asked;
  - a reason outside the vocabulary; a `null`; a float.
- **RED.** Add `log` to `FORMATS` and to `per_format!` before the reader exists. The harness's empty-bucket and pairing assertions then fail.
- **Seeded faults.** These are track one's, registered the way track one registers them (`verify.sh` injections). Example: remove the no-gaps check, so the seq-gap fixture is accepted.
- **Acceptance.** `cargo test -p discipline-diet --test conformance -- formats::log` exits 0.

### I2: the session carries what the surface needs (track three)

- **Changes.**
  - `session.start` is pushed when the session opens (model and head).
  - A turn counter is added to `Asked` and `StopAsked`; `cancel` becomes `cancel(turn)`; `Refusal` gains `Stale => "stale"`.
  - `TurnSettled {turn, reason}` is added, with a `vocabulary! SettleReason`.
  - `Logged` gains `t`.
  - The vocabulary test (`session.rs:1010-1035`) pins the new words. Rust-internal names do not depend on Q1; only I3 does.
- **Tests, each with its seeded fault** (in `diet/drive/gate.toml`):

| Test | Setup and assertion | Seeded fault |
|---|---|---|
| `a_cancel_naming_a_turn_that_already_settled_does_not_stop_the_next_one` | Hold turn 2 at a `Gate`; `cancel(1)` returns `Err(stale)`; turn 2 then answers | `drive.session-a-stale-cancel-stops-the-next-turn`: the turn comparison becomes `true` |
| `every_way_a_turn_ends_settles_it_once_with_its_reason` | A table over `Canned` outcomes: finish; cancel at a `Gate`; `Fail(Timeout)`; `Fail(Connect)`; `Reject`; `Panics`. Each ask gets exactly one `TurnSettled`, with the right turn and reason, before the settlement edge | `drive.session-a-timeout-settles-as-failed`; `drive.session-a-crash-settles-no-turn` |
| `the_log_begins_with_the_session_it_describes` | seq 0 is the start line, carrying the template's head and model | `drive.session-the-start-omits-the-head` |
| `each_event_is_stamped_when_it_was_logged` | `t` never decreases. A `Gate` that the test holds for at least 50 ms shows up as a gap of at least 50 ms. The bound is only a lower one, so the test cannot flake | `drive.session-the-clock-stands-still` |

- **Existing tests.** Move expected seqs up by one. Rerun every `drive.session-*` fault and regenerate its `catches`.
- **Acceptance.** Both of these exit 0:
  - `cargo test -p discipline-diet -- drive --skip every_seeded_fault_still_names_source_that_is_there`
  - `python3 scripts/apply-lane-faults.py --verify`
- **Why this unblocks DoD 1.** The consumer's fold closes a turn on `turn.settled` (`fold.ts:392`), and the start line carries the version.

### I3: from `Logged` to a log line (track three; waits on I1 and I2)

- **Test** `every_event_the_session_logs_is_a_line_the_log_format_reads`:
  - Make one instance of every variant, using a helper whose `match` is exhaustive, so a new variant fails to compile.
  - Render each instance, read it back with `formats::log::line`, and compare field by field.
  - Also read a real session's whole log with `formats::log::parse`.
- **Seeded fault:** `drive.session-a-line-drops-the-partial` (the conversion drops `partial` from `cancelled`).

### I4: `diet/src/drive/serve.rs` (track three; waits on I3)

`serve(listener, Arc<Session<S>>, Config)` is generic over `Streaming`. Routing is a table: `GET /events` and `POST /commands`. Tests use `Canned` and `Gate` and no sleeps.

| Test | What it checks | Seeded fault |
|---|---|---|
| `an_ask_over_http_streams_its_answer_to_a_reader_already_listening` | DoD 1 in miniature | `drive.serve-an-ask-never-reaches-the-session` |
| `a_late_reader_replays_from_zero_then_tails` | a reader that connects after events exist still gets all of them, then the tail | `drive.serve-from-is-ignored` |
| `a_reader_resuming_after_an_id_starts_at_the_next_and_misses_nothing` | both `Last-Event-ID` and `?from=`; the header wins | `drive.serve-a-resume-repeats-the-last-event` (the +1 removed) |
| `every_data_line_is_one_log_line_and_its_id_is_its_seq` | over sessions driven through finish, cancel, fail, reject, crash and refusals: every data line is accepted by `formats::log::line`, and its `seq` equals its `id` | `drive.serve-the-id-is-not-the-seq` |
| `a_refused_command_is_409_with_its_tag_and_is_in_the_log` | an ask mid-turn gets `in-flight`; `declare-seam` gets `seam-not-built`; any command after `end` gets `ended` | `drive.serve-a-refusal-is-200` |
| `a_cancel_over_http_reaches_a_call_blocked_mid_answer` | see Q4 | `drive.serve-cancel-reaches-nothing` |
| `a_reader_that_leaves_is_let_go` | the stream's thread exits within 10 s at a 100 ms heartbeat (measured: 201 ms) | `drive.serve-no-heartbeat` (the thread never exits) |
| `a_reader_that_never_reads_does_not_stall_the_session` | the session keeps going while a reader reads nothing | none: `serve.rs` cannot take the session's lock (`session.rs:223-227`); the property belongs to `wait_from` |
| `the_default_listen_address_is_loopback` | with no `--listen`, the bound address is loopback | `drive.serve-listens-everywhere-by-default` |
| `a_request_past_its_cap_is_413_before_it_is_read_whole` | the body cap | `drive.serve-no-body-cap` |
| `a_connection_past_the_cap_is_503` | the connection cap | `drive.serve-no-connection-cap` |
| `a_malformed_command_is_400_and_not_logged` | a bad body is not a command | none |

The capture half of the ruled 409 uses the same mapping. It gets an end-to-end test with R4, because capture is passed straight through today (`session.rs:492-495`).

### I5: `diet-drive serve` (track three; ownership is Q3; waits on I4)

- **Test** `a_served_drive_streams_a_real_servers_answer_over_sse`:
  - Run the binary against `Stub::serving(Act::Raw(<the captured llama-server stream>))`.
  - Open an SSE reader at `from=0`, then POST an ask.
  - Assert more than one `delta` arrives, their concatenation equals `response.text`, `turn.settled` has reason `final`, and the settlement returns to `awaiting`.
  - The assertions are about the relation between events, not the six captured pieces, which are already pinned once (`stream.rs:937`).
  - Also assert that the start line's head equals the `--head` file.
- **Seeded fault:** `drive.serve-bin-drops-the-head`.
- **Manual check** (not in CI; llama.cpp only):
  1. Start llama-server, then `diet-drive serve --endpoint …/v1/chat/completions --model M --head head.txt`.
  2. In one terminal: `curl -N http://127.0.0.1:PORT/events`.
  3. In another: `curl -d '{"kind":"ask","text":"hi"}' http://127.0.0.1:PORT/commands`.
- **Acceptance, the server half of DoD 1:** `cargo test -p discipline-diet --test drive_serve_cli` exits 0.

### I6: the consumer (track five, and the wasm crate's owner)

- **In `exercise/`:**
  - An `HttpTransport`: `EventSource` on `/events?from=0`; drops any event whose seq it has already seen before calling a listener; derives `Link` from `readyState`; sends commands as `POST /commands` and maps the reply to an `Ack`.
  - `events.ts` regenerated from log v0; its own header plans for this (`events.ts:8-10`).
  - The Vite proxy.
- **In `diet/wasm/`:** a `check_log_line` pass-through and its conformance entry, if D15 (a) is chosen.
- **Can start early.** Work can begin against I1's fixtures before I5 exists.
- **Acceptance.** Track five's `pnpm verify`. DoD 1 end to end is the manual run from I5 with the page open.

### Waiting: none of these unblocks a DoD step

- **W1: auth and binding off loopback** (D10).
  - Tests: `a_wrong_credential_is_401_on_every_route`, which walks the route table so a new route cannot skip auth; `base64_matches_rfc4648`; `off_loopback_without_a_credential_refuses_to_start`.
  - Seeded faults: `drive.serve-auth-compares-nothing`; `drive.serve-a-route-skips-auth`.
  - Constant time comes from the construction, not from a timing test; a timing test would be flaky.
- **W2: `idle.gap` intake** (D13).
  - Test: the gap is logged immediately before the ask it ended, and not at all if the ask is refused.
  - Seeded fault: the gap is dropped.
- **W3: the seam's `to`.** This is R6's, and reaches DoD 4 and 5 only through R6.
- **W4:** writing the log to a file on disk.
- **W5:** a session identity that survives a restart (§5).
- **W6:** CORS, or diet serving the SPA.
- **W7:** more than one session per process.
- **Cancel over HTTP and the stale guard,** if the answer to Q4 is to wait.

---

## 4. Questions for planning and the maintainer

Each question ends with my recommended answer.

1. **Q1. Vocabulary.** Should the precedence rule in D4 and its draft table stand? In particular: `settlement` as the name of the state-machine edge (so it is not confused with `turn.settled`); Rejected, Failed and Crashed merged into `request.failed {reason}`; `cancelled` and `stop.asked` as kinds of their own. *Recommendation: yes.*
2. **Q2. Authorship of the courier patch.** *Recommendation: track three drafts I1, because it knows R2a's events, and track one rules on it and applies it.*
3. **Q3. Ownership the brief does not state.** Who owns the format reader under `diet/src/formats/`, `diet/tests/conformance.rs`, `diet/src/bin/diet.rs`, `diet/src/bin/drive.rs`, `diet/tests/drive_*.rs` and `diet/wasm/`? *Recommendation: the first three and wasm go to track one; the drive binary and its tests go to track three.*
4. **Q4. Cancel.** Going strictly by the DoD, cancel over HTTP unblocks nothing, but the R2 row rules cancel in. *Recommendation: land the stale guard in I2 and the route in I4; the marginal cost is one table entry.*
5. **Q5. `t` in v0.** *Recommendation: yes (D14).*
6. **Q6. `idle.gap`.** What do `notice`, `read`, `compose` and `away` each measure, and in what units? Should the gap ride on the `ask`? *Recommendation: non-negative integers in milliseconds, defined in `exercise/` where they are measured, attached to the `ask`. The kind goes in v0 and the intake waits.*
7. **Q7. Off loopback, and where diet runs.** Should diet refuse to listen off loopback without a credential? Will diet run on the same host as the browser? *Recommendation: refuse; use an SSH tunnel for a remote screen. Auth waits unless diet must run on a different host from the browser.*
8. **Q8. `capture.cancelled`.** *Recommendation: pin it as invalid (D16).*
9. **Q9. What `session.start` carries in v0.** *Recommendation: `version`, `model` and `head` only. Arm, slots and phase wait until a registry, R4 and R6 can state them.*
10. **Q10. Does the surface read lines through wasm (D15)?** *Recommendation: yes; track five and the wasm owner decide.*
11. **Q11. Deltas.** *Recommendation: keep them in the log, because a reader that reconnects mid-answer needs them; the record projection drops them.*
12. **Q12. Limits for the interactive trunk.** The gym's defaults are 512 output tokens and a 180 s call. A capped answer shows in the log as `finish_reason: length`, a typed outcome. *Recommendation: keep the defaults and add flags only if the DoD run hits them.*
13. **Q13. Is streaming R2 or R3?** The R2 row includes streaming; the consumer tags it R3 (`events.ts:21`). *Recommendation: `delta` is R2, since R2a already emits it; R3 adds timings and progress.*

---

## 5. Risks, and what I did not check

**What I measured, and how.** Everything ran on 4 cores, Linux 6.18, Rust 1.94.1, with no network; scratch directories were under `scratchpad/exp/`.

- **Prototype build.** `cargo build --offline --tests` on the std prototype, which depends on `diet` by path: 30.0 s clean. Nearly all of that is diet and its existing dependencies; the surface adds no crates.
- **Prototype lints.** `cargo clippy --offline --all-targets -- -D warnings` with the workspace's lint table exited 0. I confirmed clippy really ran by planting two lints (`&Vec<u8>` as a parameter, `usize as u32`); it reported both and I removed them.
- **Prototype tests.** 3 passed, exit 0. Then I seeded a fault by editing the handler with `sed`, ran the tests, and restored the file:
  - **First run: it hung.** Heartbeats every 200 ms kept resetting the reader's 10 s per-read timeout, so the test never failed. `timeout 120` killed it with exit 124. This is exactly the hang that `session.rs:231-235` warns about.
  - **Second run,** after adding an overall deadline to the reader: exit 101 after 10.15 s.
  - **After restoring** the file: exit 0.
  - In every run I read cargo's own exit status directly, not the status of a `grep` after it.
- **A reader that leaves** was released 201 ms after closing, at a 100 ms heartbeat.
- **tiny_http:** `cargo build --offline` of a crate using `tiny_http = "0.12"` took 1.83 s from clean and added 5 crates. Clippy was clean on the SSE sketch. I counted lines and `unsafe` occurrences in the extracted `.crate` sources.

**What I did not check**

- `hyper`, `axum` and `tokio` are not priced: they are not in the offline cache, and web access was excluded.
- No real llama-server ran end to end. The CLI test in the plan uses the captured stream.
- In the browser, I checked none of these:
  - whether Vite's proxy buffers an SSE response that ends when the connection closes;
  - whether a `401` on a `fetch` or an `EventSource` makes the browser prompt for Basic credentials;
  - what `EventSource` does when it reconnects through the proxy.
- Detecting that a reader left was measured on Linux only.
- I did not read the issues (#117, #31). I know only the rulings quoted in the brief; for example, I do not know whether R6's seam command takes a `to`.

**Risks**

- **A restart reuses seqs.** A restarted diet numbers its log from 0 again. A client that resumes with `Last-Event-ID` against the new process waits, then receives a different session's events as if they continued its own. v0 carries no session identity (W5). For DoD 1, the page reloads when its link is `lost`.
- **The log grows without bound.** It lives in memory and grows with every delta. The cost of replaying a long session from 0 is not measured.
- **The seq shift touches existing faults.** Adding `session.start` moves every seq up by one, which touches ten existing `drive.session-*` faults; their `catches` must be regenerated by running them, not edited by hand.
- **Repository gates that will bite the implementer.** Routing must be a table (check-library rule one). `local_addr` must be spelled around the hygiene false positive. Integration test names must contain `drive`, or the lane never runs them.
- **"Constant-time" is a claim about construction:** both sides are hashed to fixed length and compared with no early exit. No test observes the timing.
- **The consumer's branch is unmerged.** Its shape may move before I6.
