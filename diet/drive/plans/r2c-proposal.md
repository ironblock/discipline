# R2c proposal: the HTTP + SSE surface of the interactive drive loop

This is the proposing session's blind proposal, round 2, dated 2026-09-27. It is based on `origin/main` at `178c012`. I read the consumer's current shape on `origin/feat/exercise-r1-surface`, which is unmerged. I read no issues, PRs, comments or other branches, and used no web access. Round 1 is kept as `r2c-proposal.round1.md`. The round-2 answers to the critique are in the revision log at the end.

**In short.** The plan serves one R2a `Session` over HTTP/1.1 built on the standard library alone.

- **The stream.** `GET /events` replays the log from a sequence number and then tails it as SSE. Each event carries one log line in its `data:` field. Each SSE id is `<opened>-<seq>`. A resume that names another process's log gets `410`, which tells the page's `EventSource` to stop reconnecting.
- **Commands.** `POST /commands` carries the session's commands. A refused command gets `409` with the refusal's tag, and the refusal is logged too.
- **The boundary.** Loopback is not a boundary against the author's own browser, and this was measured. Every request is therefore checked for its `Host`, its `Origin` and, for a POST, a JSON content type.
- **The format.** Every SSE payload is a line of `diet/formats/log` v0, and one implementation reads and renders it. Track one owns the format and applies it from a courier patch.
- **The session gains** what the rulings put on the surface: a turn counter that blocks a stale cancel, a `request` event whose seq the call's other events cite, `turn.settled {turn, reason}`, a `session.start` line carrying the version and the time it opened, and `t` on every line.
- **No new dependency.** The std-only prototype with the boundary checks and the stream identity is 360 lines and passes clippy pedantic with `-D warnings`. Its 8 tests pass, and each of its 5 seeded faults makes the suite exit 101.
- **Ranking.** Cancel and auth are ruled parts of R2, and the R2 row ranks itself at DoD 1 and 2. So everything here ranks DoD 1, or DoD 1 and 2. What waits: the `idle.gap` intake (Q4), the seam's target phase (R6), a log on disk, CORS, and more than one session.

---

## 1. What exists

**R2a: `diet/src/drive/session.rs`**

- **Vocabularies.** The settlement states (`awaiting|turn|capture|ended`), the command kinds (`ask|cancel|declare-seam|end`) and the refusals (`in-flight|ended|nothing-in-flight|seam-not-built`) are tagged `vocabulary!` enums (:61-101). The client's macro has `ALL` and `tag` but no `from_tag` (`diet/src/client/mod.rs:115-138`).
- **The log.** `Event` has 10 variants, each held as `Logged {seq, event}` (:103-183). `seq` is the position in the log and has no gaps (`State::push`, :193-197). There is no time stamp, no turn number, no request identity and no header line.
- **`ask`.** It refuses during `turn` and during `capture` with `in-flight`, logs the refusal, and never queues (:284-298). It returns the seq of the `Asked` event (:299, :336).
- **`cancel()`.** It takes no argument and stops whatever turn is in flight (:345-364).
  - During `capture`, `state.cancel` is `None` (:481), so a cancel there is refused `nothing-in-flight` (:359).
  - A cancel sent for turn N that arrives after turn N+1 was admitted therefore stops turn N+1.
- **Other commands.** `declare_seam` is always refused `seam-not-built` (:372-383). `end` is at :391-406.
- **Reading the log.** `events_from` and `wait_from(first, patience)` return owned `Vec` copies and wait on the session's condvar (:420-446). `from` treats a start past the end as empty (:460-463). `Shared::lock` is private (:223-227).
- **Capture** is a pass-through (:492-495), so no test can hold a session in `capture` until R4.
- **How a turn ends.** There are five terminal outcomes: Answered, Cancelled, Rejected, Failed and Crashed (:138-173, set at :482-513 and :519-528). A cancelled call is never an answer and never joins the trunk (:29-33).
- **Serialization.** The log exists only in memory, and how it is serialized is an open question (:49-50).
- **Seeded faults.** There are ten `drive.session-*` faults (`diet/drive/gate.toml:833-981`). The `Panics` transport lives in the tests (`session.rs:854-871`).

**R2b: `diet/src/client/stream.rs`** (changed only by I5r)

- `Cancel` has stoppers, and a stopper registered after the cancel runs at once (:51-121).
- `HttpStream` was measured against llama-server `4df29be`. Its cancel shuts the socket, and that is what frees the server slot (:170-193, :261-277). The dialect rule is: "MEASURED, not read from documentation" (:172).
- It reads only `delta.content` (:514-518).
- `Canned` and `Gate` are test doubles (:667-820). The captured reply is at `diet/client/fixtures/llama-server-4df29be-stream.http` (:931-937).

**Formats (track one)**

- **The convention** is one grammar, fixtures under `valid/` and `invalid/`, and one implementation, listed in `FORMATS` and the harness's `per_format!` (`diet/src/formats/mod.rs:3-20`, `:57-93`; `diet/tests/conformance.rs:1-25`; `diet/AGENTS.md:13`). A `Format` literal requires a `project` function (`formats/mod.rs:38-43`).
- **Versioning** is the grammar header ("v0", for example `record/grammar.pest:1`), and changes are by versioned bump.
- **The record's value space.** `json::line` is "the one reader for data files that are not records" (`record/json.rs:362-395`). `render` sorts keys and escapes every control character (`json.rs:402-452`), so a rendered line never breaks SSE framing.
- **Schema grammar over another format's terminals:** `operating_points/grammar.pest:1-7`. Rules that span rows are checked in Rust (`:38-43`).
- **The record** splits syntax from schema (`record/mod.rs:1-5`). It states its regime once, in `start` (`:13-18`). Every `response` names its request (`:19-22`, `:515`). Its `rejected` is a groundedness-floor rejection (`:528-529`). It keeps foreign rows it has no kind for as `unknown`, only for foreign logs (`:533-545`).
- **Outside readers** use CLI verbs (`diet/src/bin/diet.rs:73-87`) or wasm pass-throughs (`diet/wasm/src/lib.rs:1-13`, `:53-62`). A second implementation is forbidden (`diet/AGENTS.md:14`; `exercise/AGENTS.md:10`).

**Gates**

- **Lane faults** live in `diet/<lane>/gate.toml`, are applied by `check_lanes` (`verify.sh:327-342`), and are registered in `tools/gate/faults.toml` (`:5868-5873`). The drive lane filters tests by `-- drive` (`diet/drive/gate.toml:25-29`).
- **Lints:** `clippy --all-targets -D warnings` (`verify.sh:113`) with pedantic on (`Cargo.toml:16`), and `unsafe_code = "forbid"` (`:12`).
- **No match arm may have a string-literal pattern** (`scripts/check-library.py:4-19`).
- **A hygiene false positive:** the method-call form of `local_addr` on a listener has to be written `TcpListener::local_addr(&listener)` (`client/stub.rs:102-108`).
- **Dependencies:** `pest`, `serde_json` (for foreign formats only) and `sha2` (`diet/Cargo.toml:18-39`). `Cargo.lock` has 37 packages.

**Entry points and the dogma**

- **`diet-drive` runs only the scripted gym.** `regime_of` refuses every real endpoint because the substrate's identity cannot be resolved (`drive/regimen.rs:55-64`). The gym's request shape asks for at most 512 output tokens (`bin/drive.rs:388-417`, `:405`).
- **Stub server:** `client/stub.rs:1-16`, `:54`, `:61`.
- **Every dogma operating point keeps the trunk thinking.** `nothink_ops` lists no trunk operation on any model (`diet/dogma/operating-points.toml:32`, `:50`, `:63`, `:75`), and "Everything not listed keeps thinking" (`:27-28`). The non-streaming dialect reads `reasoning_content` (`client/shape.rs:281`, marked unverified).

**The consumer: `origin/feat/exercise-r1-surface`** (unmerged)

- **Transport:** `transport.ts:19-58` has `subscribe`, `dispatch` and `watchLink`; `Command = ask{text} | cancel | seam{to}`; and an open `Refusal` set. Cancel stops "the trunk's call or a fork's" (`:23`).
- **Events** (`events.ts`):
  - `At {seq, t}` (:56-61);
  - `session.start {arm, model, slots, trunk_slot, phase, system}` (:63-74);
  - `ask {turn, text}` (:77-81);
  - `request {id, lane, slot, turn, fork?}` (:83-91);
  - `delta {request, text?, reasoning?}` (:93-99);
  - `response {id, to_request, text, stop, timings}` (:118-126);
  - `request.failed` (:135-144);
  - `turn.settled` (:166-172);
  - `session.end` (:266-268).
- **Canned cancel** emits `response {stop:'cancelled'}` and settles open forks as cancelled (`canned.ts:147-169`).
- **State** is inferred from events (`fold.ts:187`, `:592-600`). A turn closes on `turn.settled` (`:392`), and unknown kinds are counted (`:459`).
- **No dedupe** in `useSession` (`useSession.ts:9-19`).
- **`capture.cancelled` in a recording.** The recorded `cancelled-capture` session carries `capture.cancelled` once (`recorded.ts:41`). The page draws chain-of-thought (`exercise/README.md`, "chain-of-thought italic").

---

## 2. Decisions

### D1. The HTTP layer: dependency or not *(unchanged recommendation; re-measured)*

| | std only (thread per connection) | `tiny_http` 0.12 (sync) | `hyper`/`axum` + `tokio` |
|---|---|---|---|
| Build | nothing added | +1.83 s clean debug; 4 new packages | not measured (not in the offline cache) |
| Audit | ours: the prototype is 360 lines | 3.7k lines (forbids unsafe); 14.7k across 5 crates | largest; async runtime |
| Saves | nothing | request-line and header reading, about 70 lines | the same, and adds bridging the condvar `Session` (`session.rs:430-446`) into async |
| Does not do | n/a | tail, heartbeat, leaver detection, Host/Origin/Content-Type checks, stream identity, refusal mapping, auth | the same |

**Measured.**
- **Round 1.** Replay then tail, resume, a cancel over HTTP reaching a call held at a `Gate`, 409, and hash-then-fold Basic auth: 313 lines.
- **Round 2.** Add the D17 checks and the D7 identity: 360 lines (`scratchpad/exp/sse-std2/`).
  - `cargo clippy --offline --all-targets -- -D warnings` exits 0.
  - `cargo test` exits 0, with 8 passing.
  - I applied 5 seeded faults one at a time with a script, and restored the file after each. Each exits 101 and is caught by its own test. The file was restored every time (`cmp` 0).

**Recommendation: std only.** The v1 default stands. What a crate would buy is the part that is easy; what it leaves undone is where the risk is.

### D2. Where the log's schema lives *(unchanged)*

- **(a)** Make R2a's `Event` the format's type. The format would then depend on `TransportFailure` (`session.rs:58`, `:168-173`), and every change to R2a would need a courier patch.
- **(b)** The format owns a line type with `render` and `parse`. `drive` converts `Logged` to that type in one exhaustive `match`.

**Recommendation: (b).** The precedent is `drive/mod.rs:114` and `:1193`.

### D3. The log's syntax *(unchanged)*

- **(a)** The record's value space, read with `json::line`. `log/grammar.pest` is a schema grammar concatenated after the record's, and the cross-row rules live in Rust (the operating_points precedent).
- **(b)** A second copy of the syntax rules.
- **(c)** `serde_json`, which the manifest reserves for foreign formats (`diet/Cargo.toml:20-24`).

**Recommendation: (a).** Track one decides.

### D4. The vocabulary *(amended: request identity, the discriminator key)*

**Option (a), derived mechanically from the Rust variant names.** The names collide with the record's `rejected` and put `settled` beside `turn.settled`.

**Option (b), the consumer's names wholesale.** Its `response` for a cancelled call contradicts `session.rs:29-33`, and it has no settlement edges and no refusals.

**Option (c), a precedence rule, always keeping R2a's semantics.** Take each name from the first source that has one:
1. a ruling;
2. the record, where it has the same meaning;
3. a `session.rs` tag;
4. the consumer, where it has the same meaning;
5. R2a's own word.

**Recommendation: (c).**

**The discriminator key is `kind`.** The record's key, `record` (`{"record":"start",…}`), names the record format itself. In the log it would mean "a record row", which is not the same meaning, so the rule falls through to the consumer's `kind`.

**Request identity is new in round 2** (finding 4).
- (i) None in v0, as in round 1. DoD 2 needs per-call identity, because a turn will be several calls around `bash`, so v0 would take a breaking bump at DoD 2.
- (ii) A `request {turn, lane}` event per call. The call's other events cite it by its **seq**, an id the log issues rather than one we invent.
- (iii) A string id like the record's `"q1"`, which would be an invented identifier.

**Recommended: (ii).**

The draft for the courier patch follows. Every line carries `seq`, `kind` and `t`.

| R2a event | v0 `kind` | fields | name from |
|---|---|---|---|
| new, at open | `session.start` | `version`, `opened` (unix ms, measured), `model` (the name as sent: not an identity), `head` | the consumer |
| `Asked` | `ask` | `turn`, `text` | `CommandKind::Ask` (:79) |
| new, per call | `request` | `turn`, `lane` (`trunk`) | the record (`record/mod.rs:513`) |
| `Settled` | `settlement` | `from`, `to` | the ruling's noun (Q1) |
| `Refused` | `refused` | `command`, `because`, `during` | R2a |
| `Delta` | `delta` | `request`, one of `text` or `reasoning` (the second only with I5r) | the consumer |
| `StopAsked` | `stop.asked` | `turn` | R2a |
| `Answered` | `response` | `to_request`, `text`, `finish_reason?` | the record |
| `Cancelled` | `cancelled` | `request`, `partial` | R2a |
| `Rejected`, `Failed`, `Crashed` | `request.failed` | `request`, `reason` (`server\|timeout\|transport\|crashed`), `message`, `status?`, `partial?` | the consumer (the record's `rejected` means something else) |
| new | `turn.settled` | `turn`, `reason` (`final\|cancelled\|max_steps\|timeout\|failed`) | ruled |
| new | `idle.gap` | as Q4 decides | ruled; only if Q4 is answered before I1 |

**How `turn.settled.reason` is derived:**
- a `response` gives `final`;
- `cancelled` gives `cancelled`;
- a `timeout` failure gives `timeout`;
- any other `request.failed` gives `failed`.

`max_steps` comes with the tool loop.

**Order within a turn:** `ask`, then the settlement edge into `turn`, then `request`, then `delta`s, then the terminal event, then `turn.settled`, then the settlement edge out.

**What a bump costs later.** v0 is closed; an open vocabulary would carry lines that cannot be conformance-tested, and the record allows `unknown` only for foreign logs (`record/mod.rs:533-545`). So each later DoD step's kinds are a versioned bump through track one:
- tool events at DoD 2;
- fork, fork outcome and patch at DoD 3;
- seam at DoD 4 and 5.

Each bump is a courier round trip: grammar, fixtures, reader and the consumer's types. There is no migration while logs are not persisted (W4).

### D5. Where the version lives *(unchanged; `opened` added)*

- **(a) In `session.start` at seq 0, once**, as the record does in `record/mod.rs:13-18`.
- **(b) On every line.**
- **(c) Outside the log.**

**Recommendation: (a).** Every R2a seq moves up by one, and the ten `drive.session-*` faults get their `catches` regenerated.

### D6. Stale cancel (ruled: blocked by an admission counter) *(amended: a third option, and capture)*

- **(a) A 1-based `turn`, carried on `ask`, `stop.asked` and `turn.settled`,** with `cancel(turn)`. This matches the record's `turn.index` and the consumer's `turn`.
- **(b) Cancel by the ask's seq.** No new counter is needed, but `turn.settled` still needs a turn to name.

**Recommendation: (a).** `Session::cancel` takes the turn, so no path skips the check. What a cancel for turn N gets:

| When the cancel for turn N arrives | Today (R2c) | With R4 |
|---|---|---|
| N is older than the turn in flight | refused as a new `stale` tag, or reuses `nothing-in-flight` (see below) | the same |
| N is the latest turn, and it is in `turn` | the stop reaches the call | the same |
| N is the latest turn, and it is in `capture` | `nothing-in-flight`, truthfully, because capture is a pass-through (`session.rs:492-495`) | Q6 (recommended: the stop reaches capture's calls) |
| N is the latest turn, and the session is `awaiting` | `nothing-in-flight` | the same |
| N was never admitted | `400`, not a command | the same |

**Option (c) (finding 9): reuse `nothing-in-flight` for a stale cancel.** This names nothing new. But the log line would read `refused {cancel, nothing-in-flight, during: turn}`, which contradicts itself.

**Recommended: `stale`**, which is the ruling's own word. The client takes the turn from the log, not from its reply.

### D7. SSE framing, resume, and which process's log *(amended: stream identity, findings 2 and 16)*

- **Framing.** Each event is `id: <opened>-<seq>` plus exactly one `data:` line, which is one log line. `seq` inside the data is the format's key. There is no `event:` field, because `EventSource` drops kinds that have no listener, and the consumer counts unknown kinds (`fold.ts:459`).
- **Resume.**
  - `Last-Event-ID: <opened>-<k>` from this process resumes from k+1.
  - `?from=n` is what a first connection can say; the header wins over it.
  - A start past the end of the log waits there.
- **Heartbeat.** A `:` comment every 15 s, configurable. A reader that left is noticed within two heartbeats (201 ms measured at 100 ms).
- **Restart (finding 2).** A restarted diet numbers its log from 0 again. `EventSource` reconnects on its own with the old `Last-Event-ID`, and a network error never takes it to `CLOSED`, so the page never sees `lost`.
  - **The critique's probe, re-run** against the round-1 prototype: a reader resuming with `Last-Event-ID: 20` on a fresh process got **0 data lines and 13 heartbeats in 4 s**, while a fresh reader got 6. curl exited 28 on its own `--max-time`.
  - **Option (a): an identity on the stream.** A Last-Event-ID from another `opened` gets `410`, and a malformed one gets `400`. By the WHATWG processing model, a non-200 fails the connection, so `readyState` becomes `CLOSED`, the page's link becomes `lost`, and it rebuilds from 0. This is spec-argued, not run in a browser.
  - **Option (b): a non-200 for a Last-Event-ID at or past the log's end.** No identity is needed, but it misses a restarted log that has already grown past k, which is then served as a continuation.
  - **Option (c): nothing (round 1).** The page is silent, measured above.
  - **Recommendation: (a).** `opened` is the wall-clock time the session opened, in unix milliseconds. It is measured once and is also carried in `session.start`. Two processes that open in the same millisecond share it; only tests can do that.
  - **Measured on the amended prototype:** another process's id returns `410` and a bare `20` returns `400`. The page's own resume works. The seeded fault `serve-resumes-another-processs-log` makes the suite exit 101.

### D8. Commands *(amended: 415, 403)*

- **(a) One `POST /commands` taking `{"kind": <CommandKind tag>, …}`.** This mirrors `dispatch(command)` and `CommandKind` (`session.rs:75-87`).
- **(b) One route per command.**

**Recommendation: (a).**

| Case | Response |
|---|---|
| `ask` accepted | `200 {"seq","turn"}` |
| any other command accepted | `200 {}` |
| any `Refusal` | `409 {"refused": tag}`, and the refusal is logged |
| malformed body, unknown kind or unknown key | `400`, not logged |
| POST whose content type is not `application/json` | `415` |
| `Host` or `Origin` not allowed | `403` |
| wrong credential | `401` |
| no such method and path | `404` (so a CORS preflight fails) |
| head over 16 KiB or body over 1 MiB | `413` |
| connection cap reached | `503` |

### D9. How the browser reaches diet *(amended premise)*

- **(a) Vite's dev proxy.** Same origin, no CORS code.
- **(b) CORS headers in diet.**
- **(c) diet serves the SPA.**

**Recommendation: (a) now.** A cross-site POST needs a preflight only because D17 refuses every other content type. Round 1 claimed the preflight without that check (finding 1).

### D10. Auth and bind *(amended: what auth is for, and when `--listen` arrives)*

- **What auth defends against.** Basic auth guards against other local users and processes and against hosts other than this one. It does not guard against web pages; D17 does that.
- **Bind.** I5 binds loopback only, with `--port`. `--listen` arrives with I7, together with the check that refuses to bind off loopback without a credential (finding 6).
- **Credential.** It comes from a file (`ps` shows arguments). Both sides are hashed with `diet::digest::sha256` (`digest.rs:87`) and the 32 bytes are compared by a fold with no early exit. The scheme is compared case-insensitively. The base64 encoding is checked against the RFC 4648 §10 vectors.
- **A remote screen.** An SSH tunnel keeps diet on loopback. D17's checks still apply to the forwarded port. Auth matters there only if other users can reach that port (Q5).

### D11. Concurrency and limits *(amended: every limit tested)*

- **(a) A thread per connection,** with a connection cap (`503`), a 10 s read timeout per request and a 10 s write timeout per stream. All three are configurable for tests.
- **(b) A thread pool.** A stuck reader holds a worker.
- **(c) Non-blocking I/O.** Needs mio or a hand-written poll loop.

**Recommendation: (a).**

### D12. The entry point *(amended)*

- **(a)** `diet-drive serve --endpoint URL --model NAME --head FILE [--port N] [--allow-origin URL]... [--max-output-tokens N]` in `bin/drive.rs`.
- **(b)** A new `[[bin]]`.
- **(c)** Regimen-driven, like the gym, which today refuses endpoints (`regimen.rs:55-64`).

**Recommendation: (a).** It binds loopback only. The cap defaults to the gym's 512 (D18). It prints the bound address and `opened` first. Q2 covers who owns the binary.

### D13. The `idle.gap` intake (ruled) *(recommendation changed, finding 8)*

- **(a) A field on `ask`, logged only if the ask is admitted.** This drops the gap of every ask refused during capture. That is exactly the sample where the author finished before capture did, so the ruled instrument would be biased. It also drops gaps that end in `declare-seam` or `end`.
- **(b) A POST of its own.** Its order relative to the command it precedes depends on the client.
- **(c) An optional `idle_gap` on any command.** It is appended immediately before that command's outcome, under the same lock, whether the command was accepted or refused.

**Recommendation now: (c)**, because (a) biases the instrument. The intake still waits (Q4).

### D14. `t` on every line *(unchanged)*

- **(a) Monotonic milliseconds since open**, stamped under the same lock as `seq`.
- **(b) None until R3.**

**Recommendation: (a)** (Q3).

### D15. How the surface reads a line *(amended, finding 7)*

- **(a) A wasm pass-through, `check_log_line`.**
- **(b) A TypeScript reader. Excluded:** `diet/AGENTS.md:14` forbids a second implementation, and `exercise/AGENTS.md:10` forbids a second parser.
- **(c) `JSON.parse`. Excluded** for the same reason.

**Recommendation: (a).** Q8 covers who builds it and when. R2c's own obligation is tested in I4: every `data:` is exactly one v0 line.

### D16. `capture.cancelled` *(rationale rewritten, finding 3)*

- **(a) Invalid in v0,** because the brief records that it has no place in the ruled vocabulary, and v0's vocabulary is closed.
- **(b) Add it as a kind.**

**Recommendation: (a)**, as an invalid fixture with a `.reason`. Round 1's rationale described a stop during capture that R2a refuses (`session.rs:359`, `:481`) and that nothing plans. What a cancel does during capture is Q6. Whatever R4 decides, it says so with settlement edges and fork outcomes, not with this kind.

### D17. Who may drive diet: loopback is not a boundary against the author's own browser *(new, finding 1)*

**Re-measured.** I ran the critique's probes against the round-1 prototype (`scratchpad/exp/rerun/probe-boundary.sh`):
- **A cross-site POST** as a browser sends it with no preflight (`Content-Type: text/plain`, `Origin: http://evil.example`) returned `200 {"seq":0}` (curl exit 0), and the ask was logged.
- **A GET carrying a rebound host name** (`Host: rebind.evil.example:PORT`) streamed 6 `data:` lines, including that ask (curl exit 28, on its own `--max-time`).

**What that means.** Once the tool loop runs on this server, any page the author visits can drive `bash` inside the confinement. It can already send asks and `end`, and read the log.

**Options.**
- **(a) Three std-only checks on every request:**
  - the `Host` must be one diet answers to: `127.0.0.1:PORT`, `localhost:PORT`, and each `--allow-origin`'s host and port. Otherwise `403`;
  - an `Origin`, if present, must be on the list. Otherwise `403`;
  - a POST must be `application/json`. Otherwise `415`, so a cross-site POST needs a preflight, and the preflight has no route.
- **(b) Basic auth as the defence.** A browser attaches cached Basic credentials to requests aimed at that origin, cross-site ones included, and auth is ruled optional. This is spec-argued, not run.
- **(c) A per-launch secret on every request.** It would stop pages and local users alike. But `EventSource` cannot set headers, so the secret would ride in URLs, and it is a credential mechanism no ruling names.

**Recommendation: (a), in I4.**
- **Measured on the amended prototype** (`probe-amended.sh`):
  - the foreign POST got 403;
  - the same POST with no `Origin` got 415;
  - the rebound `Host` got 403, with 0 data lines;
  - the page's own ask got 200.
- **Seeded faults.** Removing the content-type check, the host check or the origin check each makes the suite exit 101, each caught by its own test.
- **What it does not stop:** another local process that sends correct headers. That is I7's job (Q5).
- **Coupling with track five.** Vite forwards the page's `Host` (`localhost:5173`) unless `changeOrigin` is set, and forwards the page's `Origin` either way. Dev therefore runs `--allow-origin http://localhost:5173`.

### D18. The trunk's reasoning *(new, finding 5)*

The trunk thinks on every model the dogma lists (§1). `HttpStream` reads only `delta.content`. With the gym's 512-token cap:
- nothing streams while the model thinks;
- a think longer than the cap ends with an empty `response.text` and `finish_reason: length`.

That outcome is typed, but it is not DoD 1.

**Options.**
- **(a) Stream reasoning.** `HttpStream` delivers typed pieces (text or reasoning), and `delta` carries exactly one of the two (the consumer's shape, `events.ts:94-99`). The dialect rule (`stream.rs:172`) gates this on a captured stream from llama-server running a thinking model.
- **(b) Suppress thinking on the trunk** through the operating point's kwarg. This contradicts the dogma, where the trunk is in no `nothink_ops`, and it changes the regime.
- **(c) A non-thinking model.** The dogma lists none.

**Recommendation: (a),** as increment I5r, conditional on Q10. The cap becomes a flag, with its value measured on the DoD model. This is unmeasured here; no llama-server ran.

### Where this plan and the consumer disagree

1. **Refusal names.** `busy` is diet's `in-flight`, `nothing-to-cancel` is `nothing-in-flight`, and `nothing-to-seam` means something other than `seam-not-built`. The plan adds `stale`.
2. **The seam command.** The consumer sends `seam {to}`; diet has `declare-seam` with no argument until R6.
3. **Cancel** must carry `{turn}`.
4. **`end`** exists in diet; the consumer does not have it yet.
5. **Refusals** are logged as `refused`; the consumer carries them only in the `Ack`.
6. **State.** The consumer infers state and includes a `ratify` state. diet logs `settlement` edges over the ruled states, and `ratify` is a lane.
7. **A cancelled call** is `cancelled {request, partial}` in diet, never a `response {stop: cancelled}`.
8. **References.** `delta.request`, `response.to_request` and the rest are **integer seqs**, not string ids. `request` has no `slot` until R4. `reasoning` arrives only with I5r.
9. **`session.start`** is `version, opened, model, head`, with no arm, slots or phase (Q7).
10. **`response`** has no `id` (its seq is its id), no `timings` and no `stop`; it has `finish_reason`.
11. **`request.failed` reasons** are `server|timeout|transport|crashed`.
12. **`session.end`** is expressed as `settlement → ended`.
13. **Dedupe** is by seq within one `opened`. On `410` or `lost`, the page must rebuild the transport from 0 with an empty dedupe set.
14. **`idle.gap`** must be sent by the surface (D13, Q4).
15. **`capture.cancelled`** is invalid in v0.
16. **Deltas** are in the log and are replayed.
17. **The SSE id** is `<opened>-<seq>`, not a bare seq. The page must read `seq` from the data.
18. **Headers.** Every POST must send `Content-Type: application/json`. The dev origin must be passed to diet as `--allow-origin`.

---

## 3. The plan

The order is: I1 and I2 in parallel, then I3 and I4 in parallel, then I5, then I6. I7 comes last. I5r waits on a measured capture and on Q10.

| # | Work | Where | Track | Waits on | DoD |
|---|---|---|---|---|---|
| I1 | the log format v0 | `diet/formats/log/`, `diet/src/formats/log.rs`, `formats/mod.rs:57`, `tests/conformance.rs`, `bin/diet.rs:73` | one, by courier from three | Q1, Q2, Q3, Q6, Q7; Q4 for `idle.gap`; Q10 for `reasoning` | 1; its request references serve 2 |
| I2 | session: start line, turn counter and `stale`, `request`, `turn.settled`, `t` | `drive/session.rs`, `diet/drive/gate.toml`, `tools/gate/faults.toml` | three | nothing | 1, 2 |
| I3 | convert `Logged` to a line, with a round-trip test | `drive/session.rs` | three | I1, I2 | 1 |
| I4 | `serve.rs`: routes, SSE with identity, D17 checks, limits, cancel | `drive/serve.rs`, `drive/mod.rs` | three | I2; one test waits on I1 and I3 | 1, 2 |
| I5 | `diet-drive serve`: loopback only, `--allow-origin`, cap flag | `bin/drive.rs`, `tests/drive_serve_cli.rs` | three (Q2) | I3, I4 | 1 |
| I5r | reasoning streamed (D18) | `client/stream.rs`, `drive/session.rs` | three | a captured thinking-model stream; Q10 | 1, if the DoD model thinks |
| I6 | `HttpTransport`, v0 types, dedupe, Vite proxy; `check_log_line` | `exercise/`; `diet/wasm/` | five; wasm owner (Q8) | I1; I5 to run live | 1 |
| I7 | Basic auth, `--listen`, fail-closed off loopback | `drive/serve.rs`, `bin/drive.rs` | three | I4, I5 | 1, 2 by the R2 row; last because it is off by default |

**How tests are written.** Each test is written RED first. It compiles against the new signature with the behaviour absent, and fails on its assertion.
- **The reader helper.** Every HTTP test reader lives in one helper in `serve.rs`'s tests and gives up on an overall deadline. Heartbeats reset a per-read timeout, and this hung twice (§5).
- **Fault names** are proposals. Each fault's `catches` is written by running the fault.

### I1: the log format v0 (track one; courier patch from track three)

- **API.**
  - `line(&str)`: one line, for a resumed stream.
  - `parse(&str)`: a whole log, including the cross-row rules.
  - `render(&Line)`.
  - `project`, which the CLI verb `check-log` uses.
- **Valid fixtures:**
  - a header only;
  - an answered turn;
  - a cancelled turn;
  - each `request.failed` reason;
  - refusals, including `stale`;
  - an ended session;
  - `idle.gap`, only if Q4 is answered.
- **Invalid fixtures, each with a `.reason`:**
  - seq: a gap, a duplicate, or a first seq other than 0;
  - the first line is not `session.start`;
  - an unknown version;
  - an unknown kind;
  - `capture.cancelled`;
  - a settlement whose `from` is not the previous `to`;
  - a `turn` that does not increase by 1;
  - a `turn.settled` that names an unasked turn;
  - a `delta`, `response` or `request.failed` whose reference is not an earlier `request`'s seq;
  - a `delta` with both `text` and `reasoning`, or with neither;
  - a reason outside the vocabulary;
  - `null`;
  - a float.
- **RED.** Register `log` with a stub `project` that returns `Err` (a `Format` needs one; `formats/mod.rs:38-43`). The valid fixtures then fail on assertion.
- **Seeded faults** are track one's (`verify.sh` injections). An example: the check for gaps in seq removed.
- **Acceptance:** `cargo test -p discipline-diet --test conformance -- formats::log` exits 0.

### I2: the session carries what the surface needs (track three)

**Changes.**
- `session.start` is pushed at open, with `opened`, the model and the head.
- A turn counter: `cancel(turn)`, and a `Stale` refusal.
- A `Requested {turn, lane}` event per call. `Delta` and the terminal events carry its seq.
- `TurnSettled {turn, reason}`, with a new `SettleReason` vocabulary.
- `t` on `Logged`.
- The words test (`session.rs:1010-1035`) pins the new tags.

**Tests and faults:**

| Test | Assertion | Seeded fault |
|---|---|---|
| `a_cancel_naming_a_turn_that_already_settled_does_not_stop_the_next_one` | turn 2 is held at a `Gate`; `cancel(1)` is `Err(stale)`; turn 2 then answers | `drive.session-a-stale-cancel-stops-the-next-turn` |
| `every_way_a_turn_ends_settles_it_once_with_its_reason` | `Canned` finish, a cancel at a `Gate`, `Fail(Timeout)`, `Fail(Connect)` and `Reject`, plus the `Panics` transport (`session.rs:854-871`): exactly one `TurnSettled` each, with the right turn and reason, before the settlement edge | `drive.session-a-timeout-settles-as-failed`; `drive.session-a-crash-settles-no-turn` |
| `every_call_is_a_request_and_what_it_produced_names_it` | every `Delta` and terminal event cites the preceding `Requested` seq | `drive.session-a-delta-names-the-ask` |
| `the_log_begins_with_the_session_it_describes` | seq 0 carries the head, the model, and an `opened` between the wall-clock bounds the test took | `drive.session-the-start-omits-the-head`; `drive.session-opened-is-a-constant` |
| `each_event_is_stamped_when_it_was_logged` | `t` never decreases, and a gate held at least 50 ms shows a gap of at least 50 ms (a lower bound only, so it cannot flake) | `drive.session-the-clock-stands-still` |

- **Existing tests** move their expected seqs up by one, and the ten faults' `catches` are regenerated.
- **Acceptance:** `cargo test -p discipline-diet -- drive --skip every_seeded_fault_still_names_source_that_is_there` exits 0, and `python3 scripts/apply-lane-faults.py --verify` exits 0.

### I3: from `Logged` to a log line (track three; waits on I1 and I2)

- **Test** `every_event_the_session_logs_is_a_line_the_log_format_reads`. One instance of every variant comes from a helper with an exhaustive `match`. Each is rendered, read back with `formats::log::line`, and must be equal. A real session's whole log must pass `formats::log::parse`.
- **Fault:** `drive.session-a-line-drops-the-partial`.

### I4: `diet/src/drive/serve.rs` (track three; waits on I2)

- **Shape.** `serve(listener, Arc<Session<S>>, Config, render: fn(&Logged) -> String)`, where `Config` carries the heartbeat, the timeouts, the caps, the allowed hosts and origins, and `opened`.
- **Why `render` is a parameter** (finding 11): only one test needs `formats::log`. Everything else lands before I1, rendering through a test renderer.
- The `✓` in the table marks what the round-2 prototype already measures.

| Test | Checks | Seeded fault |
|---|---|---|
| `an_ask_over_http_streams_its_answer_to_a_reader_already_listening` | DoD 1 in miniature | `drive.serve-an-ask-never-reaches-the-session` |
| `a_late_reader_replays_from_zero_then_tails` | replay | `drive.serve-from-is-ignored` |
| `a_reader_resuming_after_an_id_of_this_process_starts_at_the_next` ✓ | resume; the header wins over the query | `drive.serve-a-resume-repeats-the-last-event` |
| `a_resume_from_another_processs_log_is_410` ✓; `a_malformed_last_event_id_is_400` | D7 | `drive.serve-resumes-another-processs-log` ✓ |
| `a_cross_site_simple_post_is_415_and_never_reaches_the_log` ✓ | D17 | `drive.serve-accepts-a-simple-post` ✓ |
| `a_request_for_a_rebound_host_name_is_403` ✓ | D17 | `drive.serve-answers-any-host` ✓ |
| `a_foreign_origin_is_403_and_never_reaches_the_log` ✓ | D17 | `drive.serve-answers-any-origin` ✓ |
| `every_data_line_is_one_log_line_and_its_id_is_opened_and_seq` (waits on I1 and I3) | sessions driven through every outcome and refusal; every data line passes `formats::log::line` | `drive.serve-the-id-is-not-the-seq` |
| `a_refused_command_is_409_with_its_tag_and_is_in_the_log` | `in-flight`, `seam-not-built`, `ended` | `drive.serve-a-refusal-is-200` |
| `a_cancel_over_http_reaches_a_call_blocked_mid_answer` ✓ | the ruled cancel | `drive.serve-cancel-reaches-nothing` ✓ |
| `a_reader_that_leaves_is_let_go` ✓ | the heartbeat; a 10 s bound (201 ms measured) | `drive.serve-no-heartbeat` |
| `a_reader_that_reads_nothing_does_not_keep_another_clients_ask_from_being_answered` ✓ | a thread per connection (round 1 had no fault for this; finding 10) | `drive.serve-one-connection-at-a-time` ✓ |
| `a_reader_that_stops_reading_is_let_go_within_the_write_timeout` | fills the socket buffer with a canned stream of large deltas | `drive.serve-no-write-timeout` |
| `a_client_that_sends_half_a_request_is_let_go_within_the_read_timeout` | the read timeout | `drive.serve-no-read-timeout` |
| `a_request_past_its_cap_is_413_before_it_is_read_whole` | the body cap | `drive.serve-no-body-cap` |
| `a_connection_past_the_cap_is_503` | the connection cap | `drive.serve-no-connection-cap` |
| `a_malformed_command_is_400_and_not_logged` | not a command | `drive.serve-a-malformed-ask-is-an-empty-ask` |

The capture half of the ruled 409 uses the same mapping. It gets its end-to-end test with R4.

### I5: `diet-drive serve` (track three, Q2; waits on I3 and I4)

- **Test** `a_served_drive_streams_a_real_servers_answer_over_sse`. The binary runs against `Stub::serving(Act::Raw(<captured stream>))`. The test asserts:
  - more than one `delta`, all citing one `request`;
  - their concatenation equals `response.text`;
  - `turn.settled` has reason `final`;
  - the settlement ends in `awaiting`;
  - the start line's head equals the `--head` file.
- **Test** `the_binary_binds_loopback_only`. It asserts the printed address. This moved here from I4 (finding 16).
- **Test** `an_allowed_origin_is_answered_and_its_host_accepted`.
- **Faults:** `drive.serve-bin-drops-the-head`, `drive.serve-bin-binds-everywhere`, `drive.serve-bin-ignores-allow-origin`.
- **Manual check** (llama.cpp only): start llama-server, then `diet-drive serve …`, then run `curl -N -H 'Host: 127.0.0.1:PORT' …/events` alongside `curl -H 'Content-Type: application/json' -d '{"kind":"ask","text":"hi"}' …/commands`.
- **Acceptance:** `cargo test -p discipline-diet --test drive_serve_cli` exits 0.

### I5r: the trunk's reasoning streamed (track three; conditional on Q10)

- **Needs first:** a person with a GPU captures llama-server streaming the DoD model, the same way `llama-server-4df29be-stream.http` was captured.
- **Then:** `Streaming::stream` delivers typed pieces, and `Delta` records which kind each piece is.
- **Tests:** the capture replayed byte by byte, and one byte at a time (the `stream.rs:947-996` pattern). Reasoning and text arrive in order and are labelled.
- **Fault:** `client.stream-reads-reasoning-as-text`.
- **The cap flag's value** is measured on the same model.

### I6: the consumer (track five; the wasm owner for `check_log_line`)

- `HttpTransport` (D7, D17). On `410`, or when `readyState` is `CLOSED`, the link is `lost` and the page rebuilds from 0.
- `events.ts` regenerated from v0, and the Vite proxy.
- **Tests track five owns, with faults in its gate:**
  - `a_replayed_range_reaches_listeners_once` (fault: the dedupe removed);
  - `a_410_marks_the_link_lost` (fault: a 410 read as a reconnect).
- **Acceptance:** `pnpm verify`. DoD 1 end to end is the I5 manual run with the page open.

### I7: auth, `--listen`, and fail-closed (track three)

- **Tests:**
  - `a_wrong_credential_is_401_on_every_route` walks the route table (measured in the prototype);
  - `base64_matches_rfc4648`;
  - `off_loopback_without_a_credential_refuses_to_start`.
- **Faults:** `drive.serve-auth-compares-nothing`, `drive.serve-a-route-skips-auth`, `drive.serve-bin-listens-openly-without-a-credential`.
- **Constant time** holds by construction. A timing test would flake.

### Waiting (unblocks no DoD step)

- **The `idle.gap` intake (D13).** It moves to DoD 3 if Q4 says R4 reads it.
- **The seam's target phase.** It belongs to R6 and reaches DoD 4 and 5 through R6.
- **A log file on disk.**
- **CORS, or diet serving the SPA.**
- **More than one session per process.**

---

## 4. Questions for planning and the maintainer

The recommended answer is in italics. Q2, Q4 and Q13 of round 1 are dropped, because the brief settles them (finding 7).

1. **Q1. The vocabulary (D4).**
   - Does the precedence rule stand?
   - Is `settlement` the right name for the edge?
   - Should Rejected, Failed and Crashed merge into `request.failed`?
   - Should a stale cancel get `stale`, or reuse `nothing-in-flight`?
   - Is the discriminator key `kind`?
   - Should references be the cited event's seq?

   *Yes, with `stale`.*
2. **Q2. Ownership the brief leaves open:** `diet/src/formats/`, `tests/conformance.rs`, `bin/diet.rs`, `bin/drive.rs`, `tests/drive_*.rs`, `diet/wasm/`. *The first three and wasm are track one's; the drive binary and its tests are track three's.*
3. **Q3. Should `t` be in v0?** *Yes.*
4. **Q4. `idle.gap`.**
   - What do `notice`, `read`, `compose` and `away` each measure, and in what units?
   - Is a gap kept when its command is refused?
   - Does a gap end at any command?
   - Does R4 read it? (If so, the intake ranks DoD 3.)

   *Integer milliseconds, defined in `exercise/`; kept when refused; ends at any command (D13 (c)). If this is unanswered when I1 lands, `idle.gap` waits for a bump.*
5. **Q5. Who may reach diet (re-asked on a corrected premise).**
   - Loopback does not protect against the author's browser; D17 does.
   - Is a machine with other local users in scope? If so, turn auth on by default.
   - Which origins may drive diet, and does Vite set `changeOrigin`?

   *A single-user host; `--allow-origin` set to the dev origin; off-host use only through an SSH tunnel.*
6. **Q6. Does cancel reach a capture that is in flight?** Asks are refused during capture (the ruled 409). If cancel is refused too, the author is locked out for as long as an interview runs, and that is DoD 3. *Yes: the R2 row's "cancel that reaches an in-flight call" covers a fork's call. R4 implements it. `capture.cancelled` stays invalid either way (D16).*
7. **Q7. Must a v0 log be projectable into a record, and if not, which version must be?** *No. v0 has no usage (R3) and no substrate identity (`regime_of` refuses one). The version that R3 and the registry make possible must be, and the record-projection item should name it.*
8. **Q8. Who builds `check_log_line`, and when?** *The wasm owner, alongside I6.*
9. **Q9. Should deltas stay in the log?** *Yes. A reader that reconnects mid-answer needs them, and the record projection drops them.*
10. **Q10. Which model does DoD 1 run on, and is its reasoning streamed or suppressed?** *A dogma model, with its reasoning streamed (D18, I5r) and the cap measured on it.*
11. **Q11. Is `opened` the identity of the stream (D7)?** *Yes. It is a measured wall-clock value, and nothing is invented.*
12. **Q12. Is the command wire a format under `diet/formats/`, or an HTTP detail of `serve.rs`?** *An HTTP detail, owned by track three. By the brief's own test ("two consumers make it a format"), each direction of it has one consumer: diet reads the bodies and the page reads the replies. A contract test (I6 running against I5) guards it.*

---

## 5. Risks, and what I did not check

**Measured in round 2.** Everything ran with no network, in `scratchpad/exp/`. The prototype is in `sse-std2/`; the logs and scripts are in `rerun/`.

- **The critique's probes, re-run** against the round-1 prototype:
  - the boundary probe gave `200 {"seq":0}` (curl exit 0) for the cross-site POST, and 6 data lines for the rebound host (curl exit 28, on its own `--max-time`);
  - the restart probe gave 0 data lines and 13 heartbeats in 4 s (curl exit 28), and a fresh reader got 6.
- **The same probes against the amended prototype** (`probe-amended.sh`): 403 for the foreign POST, 415 for the same POST without `Origin`, 403 and 0 data lines for the rebound host, 410 for another process's id, 400 for a bare id, and 200 for the page's own ask.
- **Build and lint.** `cargo build --offline --examples` exits 0. `cargo clippy --offline --all-targets -- -D warnings` exits 0 with no findings. `cargo test` exits 0 with 8 passing.
- **Seeded faults** (`rerun/seed.py`). The script applies each fault, runs `cargo test` with a 120 s bound, and restores the file in a `finally`.
  - `serve-accepts-a-simple-post`, `serve-answers-any-host`, `serve-answers-any-origin`, `serve-one-connection-at-a-time` and `serve-resumes-another-processs-log` each exit 101 and are each caught by their own test.
  - After every run the file is restored, and `cmp` exits 0.
- **A second hang, from the same cause.** My first fault run hung for 300 s and left the file mutated; I restored it by hand (`cmp` 0). My new status helper read a stream until 200 ms of silence, and 100 ms heartbeats never allow that. This is the same hazard round 1 found, which is why §3 wants one reader helper, with an overall deadline, for the whole lane.
- **Round 1's measurements stand:** the tiny_http build, the leaver noticed in 201 ms, and the no-op cancel fault exiting 101 (the critique reproduced this).

**What I did not check**

- **No browser ran.** The following are argued from the specification or from defaults:
  - that a `410` closes an `EventSource`;
  - what the Vite proxy forwards as `Host` and `Origin`;
  - whether the Vite dev server itself refuses rebound hosts (that is track five's).
- **No llama-server ran.** D18's premise that reasoning streams as its own field is inferred from the non-streaming dialect, and I5r measures it first.
- **`hyper`, `axum` and `tokio` are not priced** (not in the offline cache).
- **The issues were not read.**

**Risks**

- **What D17 does not cover.** It does not stop other local processes. That is I7, and Q5.
- **Two processes that open in the same millisecond** share an identity. Only tests can do this.
- **The in-memory log** grows with every delta, and a full replay's cost is not measured.
- **The seq shift** touches ten existing faults.
- **Repository gates:** routing must be a table, `local_addr` must be spelled around the hygiene false positive, and test names must contain `drive`.
- **The consumer branch** is unmerged.

---

## Revision log (round 2)

| finding # | accepted / rebutted / partly | what changed in the proposal (section) | evidence if rebutted |
|---|---|---|---|
| 1 (critical) | accepted | New D17 (Host, Origin and JSON content-type checks) in I4, with three tests and faults; D8's table (403, 415); D9's premise corrected; D10 says what auth is for; Q5 re-asked; ranked DoD 1 and 2 | Re-measured: 200 and 6 data lines before the fix; 403, 415, 403 and 0 data lines after; each fault exits 101 |
| 2 | accepted | D7: identity `<opened>-<seq>` and 410, with options; `opened` in `session.start`; I4 tests; round 1's W5 absorbed; disagreements 13 and 17; I6 handles 410 | Re-measured: 0 data lines and 13 heartbeats before; 410 and 400 after; the fault exits 101 |
| 3 | accepted | D16's rationale rewritten; D6's table states the capture case; Q6 added | — |
| 4 | partly | D4: `request` event and seq references (option ii), and the bump cost per DoD step; Q7 added. Not accepted: that `session.start.model` is a prose identity | v0 names it "the name as sent", and nothing claims it identifies the weights. `regime_of`'s refusal (`regimen.rs:55-64`) is about claiming identity, which v0 does not |
| 5 | accepted | New D18; I5r; Q10 replaces round 1's Q12; the cap flag in D12. Added dogma evidence that the trunk thinks on every listed model | — |
| 6 | accepted | `--listen` removed from I5; it lands in I7 with the fail-closed check (D10, D12) | — |
| 7 | accepted | Round 1's Q2, Q4 and Q13 dropped; Q10 narrowed to Q8; D15's (b) marked excluded | — |
| 8 | accepted | D13's recommendation changed to (c), on any command, kept when refused; Q4 extended; I1 waits on Q4 for `idle.gap` | — |
| 9 | partly | Reuse `nothing-in-flight` added as D6 option (c) and in Q1; `stale` kept | With reuse, the log line would read `refused {cancel, nothing-in-flight, during: turn}`, which contradicts itself |
| 10 | partly | Write timeout, read timeout and 400 each get a test and a fault; the dedupe and 410 tests are named for track five. The "cannot fail" test is re-aimed at other clients, with a fault | Such a test can fail: handling connections on the accept thread (`serve-one-connection-at-a-time`) made it fail, exit 101, measured |
| 11 | accepted | I4 takes `render` as a parameter and waits on I2 only; one test waits on I1 and I3 | — |
| 12 | accepted | The cost of a bump per DoD step is stated (D4); cancel and auth ranked DoD 1 and 2 by the R2 row (I4, I7); D17 ranked 1 and 2 | — |
| 13 | partly | Q12 added | By the brief's own definition (two consumers), each direction of the command wire has one consumer, so the recommendation is not a format, guarded by a contract test |
| 14 | partly | D4 names the discriminator key; Q1 includes it | The rule does apply, and it yields `kind`: the record's `record` names the record format itself, so it does not mean the same thing in the log |
| 15 | accepted | I1's RED uses a stub `project` that returns `Err` | — |
| 16 | accepted | I2 names the `Panics` transport; the loopback test moved to I5 | — |
