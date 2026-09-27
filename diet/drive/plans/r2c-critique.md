# R2c critique: an adversarial review of `r2c-proposal.md`

This is the reviewing session's critique, dated 2026-09-27. It was written against `origin/main` at `178c012` and the consumer branch `origin/feat/exercise-r1-surface`, under the same limits as the proposer: no issues, PRs or comments, no web access, and no other branch.

**Measurements.** All experiments ran in `scratchpad/critique-r2c/`, which is my own directory. I used a private copy of the proposer's std prototype (`scratchpad/exp/sse-std/src/lib.rs`, unmodified), pointed at this worktree's `diet`, and added an example binary that serves a `Canned` session so that curl could probe it. The build (`cargo build --offline --examples`) exited 0. The scripts are `probe-boundary.sh` and `probe-restart.sh` beside the build.

**Severity counts.** 1 critical, 5 major, 7 minor, 3 notes.

---

## Findings

| # | severity | where | finding | evidence | what would resolve it |
|---|---|---|---|---|---|
| 1 | **critical** | D9, D10 ("an SSH tunnel keeps diet on loopback and makes auth unnecessary"), W1, Q7 | **Loopback is not a boundary against the author's own browser.** The plan treats binding to 127.0.0.1 as sufficient, and defers auth on that premise. But any web page the author visits can reach 127.0.0.1. D9's "JSON POSTs trigger a preflight" holds only if the server *refuses* other content types, and the plan never says it will. No Host or Origin check appears anywhere, so a DNS-rebound page reads the log as same-origin. What such a page can do: send asks; send `end`, which ends the session in front of the author; and read the whole log. Once the tool loop rides this server (DoD 2), it can make the model run `bash`. Basic auth does not close this. Through the Vite proxy the browser attaches cached credentials to a cross-site form or `fetch` POST, and directly on the diet port there are no credentials to require. | **Measured** (`probe-boundary.sh`, against the prototype). E1: a POST with `Content-Type: text/plain;charset=UTF-8`, `Origin: http://evil.example` and the body `{"text":"sent by another site"}` returned `http_code=200`, `{"seq":0}` (curl exit 0), and the ask is in the log. A browser sends this as a CORS "simple request", with no preflight. E2: `GET /events?from=0` with `Host: rebind.evil.example:PORT` streamed 6 `data:` lines, including that ask. curl exited 28 on its own `--max-time`, which is expected on an open stream. | Put this in **I4**, not W1. All three checks are std-only, a few lines each: <br>• require `Content-Type: application/json` on every POST (`415` otherwise), so that a cross-site POST needs a preflight, which the 404 on OPTIONS then fails; <br>• refuse a `Host` that is not the bound address or a configured allow-list (the Vite proxy forwards `Host: localhost:5173` unless `changeOrigin` is set, and that is a coupling with track five); <br>• refuse a present `Origin` that is not on the list. <br>Give each a test and a seeded fault (for example `drive.serve-accepts-a-simple-post` and `drive.serve-answers-any-host`). Re-put Q7 to planning with the correct premise. |
| 2 | major | §5 Risks ("For DoD 1, the page reloads when its link is `lost`"), W5, D7 | **After diet restarts, an open page silently shows nothing.** When the connection drops, `EventSource` re-establishes it with the same URL plus `Last-Event-ID` of the *old* log. By the WHATWG rule, a network error or the end of a stream means reconnect, and only a non-200 status or a wrong MIME type fails the connection. The derived `Link` therefore goes `reconnecting → live` and never reaches `lost`, so the claimed mitigation never fires. D7 then says "a start past the end of the log waits there", so the new process holds the page until its own log passes the old id. The ruled dedupe makes the other path worse too: a fresh `?from=0` stream on the same transport drops the new session's seqs 0..k as "already seen". W5's deferral rests on this false mitigation. | **Measured** (`probe-restart.sh`). On a fresh process, a reader sent `?from=0` with `Last-Event-ID: 20`, and then an ask was POSTed (200, `{"seq":0}`). Over 4 s the reader got **0 `data:` lines and 13 heartbeats**, while a fresh reader from 0 got all 6 events of the answered turn. The browser half is argued from the EventSource processing model, not run in a browser. | Give each process an identity on the stream: for example `id: <boot>-<seq>`, with the bare `seq` staying the key inside `data:`. Answer an id from another process with a non-200, so that EventSource closes, `Link` becomes `lost` and the page reloads. The cheapest partial fix is a non-200 for a `Last-Event-ID` at or past the log's end. Either way, add a test and a seeded fault in I4, and restate D7's "its id is its seq". |
| 3 | major | D16 (rationale), D6 (refusal table), §2 "disagree" item 3 | **What cancel does during capture is decided silently, and D16 rests on a claim the plan contradicts.** D16 says "a stop during capture is `stop.asked {turn}` followed by `settlement capture→awaiting`". But §1 (correctly, `session.rs:481`, `:359`) and D6's table ("the latest turn, with nothing in flight: `nothing-in-flight`") both refuse a cancel during capture. The consumer's contract is that cancel stops "the trunk's call or a fork's" (`transport.ts:23`); its canned cancel settles open forks as cancelled (`canned.ts:147-169`); and the recorded `cancelled-capture` session is a person cancelling a capture round. Asks are refused during capture (the ruled 409), so if cancel cannot reach capture either, the author is locked out for as long as an interview runs. That is DoD 3, in front of a person. | The pointers above, and the plan's own text in §1 and D6 against D16. | Add this as a question for planning: "Does cancel reach a capture in flight?" Make D6 state what `cancel(latest turn)` does during capture. Rewrite D16's rationale: `capture.cancelled` is invalid in v0 because the brief records it as having no place in the ruled vocabulary, not because of a mechanism that neither exists nor is planned. |
| 4 | major | D4 table (`response`, `delta`), §2 "disagree" items 8 and 10, Q9, Q11 | **v0 drops request identity, which DoD 2, the record projection and the consumer all need.** `response` takes the record's name (`record/mod.rs:515`) but drops the record's defining link: a record response is `{"record":"response","id":"a1","to_request":"q1",…}`, and "every `response` names the request it answers" (`record/mod.rs:19-22`). DoD 2 (a turn is several model calls around `bash`) needs per-call identity, so v0's `delta` and `response` shapes would take a *breaking* bump at DoD 2. The consumer must rewrite its delta-to-generation fold (`events.ts:93-99`, `:118-126`) for DoD 1 and again for DoD 2. The plan's own argument for `t` (D14) and for `idle.gap` (D13), "one field now is cheaper than a version bump later", applies here and is not applied. More broadly, the log-as-format ruling names the record projection as the second consumer, but v0 is checked only against the surface. Its `session.start.model` is a prose wire name, which is exactly what `regime_of` refuses to turn into an identity (`regimen.rs:55-64`), and the plan does not say whether a v0 log must be projectable into a record. | The pointers above. Record fixture: `diet/formats/record/fixtures/valid/archive-rows.jsonl`. | Offer the option to planning, with a recommendation: add a `request {turn}` event per call, and have `delta.request`, `response.to_request` and `request.failed.request` refer to it by that event's **seq**. That is an id issued by the log, not an invented one. Add a question: "Must a v0 log be projectable into a record, and if not, which version must be?" |
| 5 | major | D12, I5 (the gym's limits), Q12; §1 on `stream.rs:514-518` | **DoD 1 may show nothing with the models the dogma lists.** Every entry in `diet/dogma/operating-points.toml` (qwen3.6, qwen3.5, qwen3, gemma) thinks, and "everything not listed [in `nothink_ops`] keeps thinking" (line 27-28), so the trunk thinks. `HttpStream` reads only `delta.content` (`stream.rs:514-518`), while the non-streaming shape reads `reasoning_content` (`shape.rs:281`). `serve` takes the gym's cap of 512 output tokens (`bin/drive.rs:402-407`) and empty `template_kwargs`. In front of a person: nothing streams while the model thinks, and a long think ends at the cap with an empty `response.text` and `finish_reason: length`. That is typed, but it is not "the trunk answers, streamed". The plan notes the dropped reasoning and asks about limits (Q12), but never connects the two to DoD 1. | The pointers above. **Not measured**: no llama-server ran here. The claim that thinking arrives as `delta.reasoning_content` rests on the repository's own non-streaming shape reading that field. The captured stream is from a random-weights model and cannot show it. | Add a question: "Which model does DoD 1 run on, and is its reasoning shown or suppressed?" Then either stream reasoning (`diet/src/client/` is track three's) and add `delta.reasoning` to v0, or apply the operating point's thinking control on the serve path. Size the cap to match. |
| 6 | major | D12 (`[--listen ADDR]` in I5), D10, W1, Q7 | **I5 ships `--listen` without the fail-closed check the plan recommends.** D10 and Q7 recommend refusing to bind off loopback without a credential, but `off_loopback_without_a_credential_refuses_to_start` sits in W1, which waits. As sequenced, `diet-drive serve --listen 0.0.0.0:…` binds an unauthenticated server on every interface, and that server later runs `bash`. | Compare the D12 command line with the tests W1 lists. | Move the fail-closed check, its test and its fault into I5, or drop `--listen` until W1 lands. |
| 7 | minor | Q2, Q4, Q10, Q13 | **Four questions are already settled.** <br>• Q2: the brief says a change to `diet/formats/` "goes as a courier patch that track one applies". <br>• Q4: the R2 row rules "cancel that reaches an in-flight call ... served over HTTP + SSE" and says the row unblocks DoD 1 and 2, and the tool-loop row rules the stale guard. <br>• Q13: R2b, "a streaming llama.cpp transport", is on `main` as part of R2, and DoD 1 says "streamed". <br>• Q10: `diet/AGENTS.md:14` forbids a second implementation, so D15 (b), a TypeScript reader, is as forbidden as (c), and only who builds `check_log_line` is open. | The brief's text; `diet/AGENTS.md:14`; `exercise/AGENTS.md:10`. | Drop Q2, Q4 and Q13. Reduce Q10 to "who builds `check_log_line`, and when", and remove D15 (b) as an option. |
| 8 | minor | D13, W2, I1's "waits on" column | **The `idle.gap` intake silently decides which gaps are lost.** Riding on `ask`, the gap is dropped when the ask is refused. A refusal during capture (the ruled 409) is exactly the case where the author finished before capture did, so dropping those samples biases the ruled instrument. Gaps that end in a seam or in `end` are lost as well. I1 also puts an `idle.gap` fixture in v0 but does not list Q6 (its units and meanings) among what it waits on. Nor does the plan ask whether R4's interview (DoD 3, "runs in the idle gap while the author reads") consumes the measurement. If it does, W2 is ranked wrongly. | D13 (a), "only if the ask is admitted"; the I1 row of the plan table. | Add to Q6: whether a refused ask's gap is kept, whether gaps end at any command, and whether R4 reads `idle.gap`. Add Q6 to I1's dependencies, or leave `idle.gap` out of v0 until Q6 is answered. |
| 9 | minor | D6 (`stale`) | **A new refusal tag, with no option offered.** `nothing-in-flight` already describes a cancel that names a turn which has settled: nothing of that turn is in flight. The brief says "Nothing else is named until it has to be." Keeping two tags may be worth it (a person can be told "your stop arrived late"), but the choice deserved options. | D6; `session.rs:96-97`. | Put "reuse `nothing-in-flight`" beside `stale` in Q1. |
| 10 | minor | I4 tests; I6 | **Some tests cannot fail, and some mechanisms have no fault.** <br>• `a_reader_that_never_reads_does_not_stall_the_session` cannot fail under any mutation of `serve.rs`, by the plan's own argument. A test that cannot fail is not a test. <br>• `a_malformed_command_is_400_and_not_logged` guards a mechanism the plan introduces, but has no seeded fault. <br>• D11's write timeout and read timeout have neither a test nor a fault. <br>• The ruled "clients dedupe by sequence" has no named test anywhere: I6's acceptance is only `pnpm verify`. | The "Seeded fault" column of I4, where it reads "none"; I6. | Re-aim the first test at the write timeout ("a reader that stops reading is let go within the write timeout"; fault: no write timeout). Give the 400 path a fault (for example, a malformed body is logged). Name a dedupe test and a fault for track five. |
| 11 | minor | §3 order and table ("I4 waits on I3") | **The HTTP server waits on track one for no reason.** I4 queues behind I1, the courier round trip and six questions (Q1–Q3, Q5, Q8, Q9), although `serve.rs` "never matches on an event kind". Only one test, `every_data_line_is_one_log_line…`, needs `formats::log`. This puts track one's ruling on the critical path to DoD 1. | Plan table; §3, "DoD 2 to 5". | Build I4 with rendering passed in as a parameter, and let only that one test wait on I1 and I3. |
| 12 | minor | §3, "DoD 2 to 5 ... need nothing from R2c beyond a stream that does not care which event kinds it carries" | **The DoD reading is too loose on the format and too literal on the surface.** v0 is closed: "an unknown kind" is an invalid fixture, and I4 asserts that every data line is a v0 line. So each of DoD 2–5 needs a versioned bump through track one. That is not R2c's work, but it is a cost the ranking hides. In the other direction, cancel and auth are ruled parts of R2 and R2c, and the R2 row ranks itself at DoD 1 and 2. Finding 1's defence is a precondition of DoD 2 on this server. | The brief's R2 and R2c rows; the plan's invalid-fixture list. | State the cost of a bump per DoD step. Rank finding 1's checks under DoD 2 as well as DoD 1. |
| 13 | minor | D8, D15 | **The command wire has two parties, just as the log does, and the plan does not ask whether it is a format.** It consists of the `POST /commands` bodies, `{"refused": tag}` and `{"seq","turn"}`. The consumer will read the replies with `JSON.parse`, and `exercise/AGENTS.md:10` says anything that reads a `diet` format goes through `diet`. Whether it is a format also decides ownership (track one or track three). | The brief's reason the log is a format: "Two consumers ... make it a format by definition". | Add a question: "Is the command wire a format under `diet/formats/`, or an HTTP detail of `serve.rs`?" |
| 14 | note | D4 ("Every line also carries `seq`, `kind` and `t`") | The precedence rule is not applied to the discriminator key. The record, which ranks above the consumer, uses `record` (`{"record":"start",…}`). `kind` is the consumer's word. | A record fixture. | Name the key in Q1. |
| 15 | note | I1 ("RED") | Adding `log` to `FORMATS` before the reader exists fails to *compile*: a `Format` literal needs its `project` (`formats/mod.rs:38-43`). That contradicts "fails on its assertion rather than on compilation". | `formats/mod.rs:38-43`. | Add a stub `project` that returns `Err`. |
| 16 | note | I2, `every_way_a_turn_ends…`; I4, `the_default_listen_address_is_loopback` | `Panics` is its own transport (`session.rs:854-871`), not a `Canned` step. `serve` takes an already bound listener, so the default address lives in I5's binary, not in I4. | The pointers above. | Wording only. An implementer would get both right. |

---

## Checked and found sound

- **Every `path:line` pointer in §1 and §2 that a decision rests on:**
  - `session.rs`: :49-50, :61-101, :103-183, :193-197, :223-227, :231-235, :284-299, :336, :345-364, :372-383, :391-406, :420-446, :460-463, :481, :492-495, :816-820, :1010-1035;
  - `client/mod.rs:115-138`, where the macro has no `from_tag`;
  - `formats/mod.rs:3-20`, `:57-93`; `tests/conformance.rs:1-25`; `diet/AGENTS.md:13-14`;
  - `record/grammar.pest:1-24`; `json.rs:362-395`, `:402-452`; `operating_points/grammar.pest:1-7`, `:38-43`; `record/mod.rs:1-5`, `:13-18`, `:515`, `:528-529`;
  - `bin/diet.rs:73-87`; `wasm/src/lib.rs:1-13`, `:53-62`; `verify.sh:113`, `:327-342`; `Cargo.toml:12`, `:16`; `check-library.py:4-19`; `stub.rs:102-111`; `diet/Cargo.toml:18-39`;
  - `Cargo.lock` (37 packages; `log` is present); `regimen.rs:55-64`; `bin/drive.rs:388-417`; `drive/gate.toml:25-29`, and `:833-981` (exactly ten `drive.session-*` faults); `drive_cli.rs:86`; `drive/mod.rs:114`, `:1193`; `digest.rs:87`;
  - `stream.rs`: :51, :170-193, :261-277, :514-518, :726-735, :931-937;
  - the consumer's `transport.ts:19-58`, `events.ts:56-61`, `:63-74`, `:93-99`, `:118-126`, `:135-144`, `:166-172`, `:266-268`, `:300`, `fold.ts:187`, `:392`, `:459`, `:592-600`, `useSession.ts:9-19`, `canned.ts:147-169`, `recorded.ts:41`, `exercise/AGENTS.md:10`.

  All say what the plan says. I found no wrong pointer.
- **One log line per `data:` field.** `render_string` escapes every C0 control, including CR and LF, as `\uXXXX` or a short escape (`json.rs:434-455`). The grammar rejects raw controls (`record/grammar.pest:78-81`). So a rendered line cannot break SSE framing. `Value::Integer` is an `i64`, which covers `seq` and `t` in practice.
- **D7 framing and resume, within one process.**
  - With no `event:` field, every kind reaches `onmessage`.
  - The header wins over `?from=`, which matches how EventSource reconnects (same URL plus the header).
  - An event cut off mid-write is not dispatched and does not advance `lastEventId`, so a resume cannot skip one.
- **Readers cannot stall the session.** `wait_from` copies under the lock and returns owned `Vec`s. `Shared::lock` is private to `drive::session`, so `serve.rs` cannot hold it.
- **D1, std only.** It is consistent with the no-dependency default and with the terms on which it may be revisited. I reproduced the proposer's prototype measurements in my own copy:
  - the green run exited 0, with 3 passed;
  - a reader was released 201 ms after closing, at a 100 ms heartbeat;
  - replacing the cancel route with `Ok(())` exited 101 after 10.13 s, failing on the overall deadline;
  - after restoring the file, `cmp` exited 0.
- **The ruled rows reach the surface.**
  - The `turn.settled` mapping covers all five R2a terminal outcomes and reserves `max_steps`.
  - Every refusal maps to 409, so the capture 409 follows. The plan honestly defers its end-to-end test, because capture is pass-through today (`session.rs:492-495`).
  - The admission counter is D6 and I2.
  - `t` matches the consumer's definition, "Milliseconds since `session.start`" (`events.ts:59`).
  - The 1-based turn matches the record's `{"record":"turn","index":1}`.
- **The I2 seeded faults are caught.**
  - With the turn comparison forced true, `cancel(1)` stops held turn 2 and returns `Ok`, so `Err(stale)` fails.
  - Mapping a timeout to `failed` fails the `Fail(Timeout)` row.
  - Letting `crashed()` settle no turn fails the `Panics` row.
  - The `t` lower bound cannot flake: the floor of the difference is at least 50 whenever the real gap is at least 50 ms.
- **Ownership.**
  - Every increment edits inside track three's list, goes by courier to track one (I1), goes to track five (I6), or is flagged in Q3 (`bin/drive.rs`, `tests/drive_*.rs`, `diet/wasm/`, `src/formats/`).
  - Gate files are touched only for `tools/gate/faults.toml` registrations.
  - A new lane fault needs no `verify.sh` or `shards.tsv` edit, because unplanned faults are hashed to a shard (`verify.sh:698-714`).
- **Test filters.** The drive lane's `-- drive` filter matches `drive::serve::tests::*` and the proposed CLI test name.
- **Vocabulary.** `slot`, `trunk`, `ratify` and `extraction` are used as ruled, and disagreement 6 correctly treats `ratify` as a lane rather than a state.

## Questions I would add or remove

**Add:**
1. Which hosts and origins may reach diet: the Vite proxy's origin, and anything else? Is loopback treated as a boundary against the author's own browser? (Finding 1.)
2. What should an open page see after diet restarts, and does the stream carry a per-process identity? (Finding 2.)
3. Does cancel reach a capture in flight, and what does `cancel(latest turn)` do during capture? This replaces Q8's framing. (Finding 3.)
4. Must a v0 log be projectable into a record? Does v0 carry request identity, as seq references? (Finding 4; this absorbs Q9.)
5. Which model does DoD 1 run on, and is the trunk's reasoning streamed or suppressed? (Finding 5; this absorbs Q12.)
6. Is the command wire a format (track one) or an HTTP detail (track three)? (Finding 13.)
7. As part of Q6: are gaps kept when the ask is refused, do they end at any command, and does R4 read `idle.gap`? (Finding 8.)

**Remove:**
- Q2, Q4 and Q13 are settled by the brief.
- Q10 shrinks to "who builds `check_log_line`, and when".
- Q7 should be re-asked with a correct premise, since loopback alone does not make auth unnecessary.

## What I did not check

- No browser ran. The EventSource reconnect behaviour and the Vite proxy's forwarding of `Host` and `Origin` are argued from the specification and from configuration defaults.
- No llama-server ran, so finding 5's reasoning stream is inferred.
- A probe of Node's `localhost`-to-127.0.0.1 resolution was inconclusive on this host (it resolves only to IPv4), so it is not a finding.

---

## Round 2

This round reviews the revised `r2c-proposal.md` (616 lines) against round 1 (`r2c-proposal.round1.md`), on `origin/main` at `178c012`. The limits are unchanged: no GitHub, no web, and no branch but the consumer's.

**Outcome.** 15 findings resolved, 1 changed severity, 0 stand. Five new findings: 3 minor and 2 notes. No critical or major finding remains.

**Measured in round 2.** Everything below ran against my own copy of the proposer's amended prototype. Before I started, `diff -r` against `scratchpad/exp/sse-std2/src` exited 0. I built it in my own target directory (`scratchpad/critique-r2c/target2`), and `cargo build --offline --examples --tests` exited 0.

- **The boundary probe** (`probe-boundary-r2.sh`): my round-1 requests, unchanged except for the binary path and the address line. The added cases are marked "added".
  - E1, a `text/plain` POST with a foreign `Origin`: **403**, curl exit 0.
  - E1b (added), the same POST with no `Origin`: **415**, curl exit 0.
  - E2, a rebound `Host`: **403**, curl exit 0, **0** data lines.
  - E3 (added), a reader from 0 on the same host: 0 data lines. Nothing from E1 or E1b reached the log. curl exited 28 on its own `--max-time`.
- **The restart probe** (`probe-restart-r2.sh`):
  - my round-1 request, a bare `Last-Event-ID: 20`: **400**, curl exit 0, 0 data lines and 0 heartbeats;
  - the author's ask on the new process: 200 `{"seq":0}`;
  - (added) a well-formed id from another process, `<opened-1>-20`: **410**, curl exit 0;
  - (added) this process's own id `<opened>-2`: 200, and the first id delivered is `<opened>-3`;
  - a fresh reader from 0: 6 data lines, starting at `<opened>-0`.
- **The suite and lints.** `cargo test --lib` exited 0, with 8 passed. `cargo clippy --all-targets -- -D warnings` exited 0.
- **The seeded faults.** I ran the proposer's `seed.py`, re-pointed at my copy and reading the proposer's file only as the pristine reference. Every fault changed the tree, and every one exited **101**:

  | Fault | Test that failed |
  |---|---|
  | `serve-accepts-a-simple-post` | its 415 test |
  | `serve-answers-any-host` | its 403 test |
  | `serve-answers-any-origin` | its 403 test |
  | `serve-one-connection-at-a-time` | its re-aimed test, and the replay/cancel test |
  | `serve-resumes-another-processs-log` | its 410 test |

  The file was restored after each run. At the end, `cmp` against the proposer's file exited 0.
- **The revision's new pointers** all say what the plan says: `record/mod.rs:513`, `:533-545`; `operating-points.toml:27-28`, `:32`, `:50`, `:63`, `:75`; `bin/drive.rs:405`; `stream.rs:172`, `:971`; `shape.rs:281`; `session.rs:854-871`; `events.ts:83-91`, `:94-99`.

### Round-1 findings

| finding # | resolved / stands / changed severity | why |
|---|---|---|
| 1 (critical) | **resolved** | D17 adds the Host, Origin and JSON content-type checks to I4, each with a test and a seeded fault. D9's premise is corrected, D10 says what auth is and is not for, and Q5 is re-asked on the corrected premise. My probe now gets 403, 415 and 403, and nothing reaches the log; each fault exits 101 (above). What is left is stated, not silent: the Vite `changeOrigin` coupling and Vite's own host check are unchecked, and Q5 asks. |
| 2 | **resolved** | D7 adds the `<opened>-<seq>` identity, with 410 for a foreign id and 400 for a malformed one. `opened` is carried in `session.start`, and I6 handles the 410. Measured: 400, 410, and the page's own resume starting at the next seq. The browser half is spec-argued and says so. New finding 17 is a consequence of this design, not a reopening. |
| 3 | **resolved** | D16's rationale now rests on the brief's observation. D6's table states the capture case truthfully, and Q6 asks what R4 should do. |
| 4 (partly) | **resolved** | Request identity is option (ii): a `request` event, cited by its **seq**, which the log issues and nobody invents. The cost of a bump per DoD step is stated, and Q7 asks whether a v0 log must be projectable into a record. The part the proposer did not accept is a fair limit: v0 labels `model` "the name as sent", and nothing claims it identifies the weights. |
| 5 | **resolved** | D18, I5r and Q10 connect the thinking trunk, the dropped reasoning and the cap to DoD 1. I5r is gated on a measured capture, as `stream.rs:172` requires. Two gaps remain, recorded as new findings 19 and 20. |
| 6 | **resolved** | `--listen` moves to I7 together with the fail-closed check and its fault. I5 binds loopback only and tests it (`the_binary_binds_loopback_only`, fault `drive.serve-bin-binds-everywhere`). |
| 7 | **resolved** | Round 1's Q2, Q4 and Q13 are dropped. Q10 is narrowed (now Q8), and D15 (b) is marked excluded. |
| 8 | **resolved** | D13 (c) takes a gap on any command and keeps it when the command is refused. Q4 carries the sub-questions, including whether R4 reads the gap. I1 waits on Q4 for `idle.gap`. |
| 9 (partly) | **resolved** | Reuse of `nothing-in-flight` is now offered, as D6 (c) and in Q1. The argument for keeping `stale` holds: `refused {cancel, nothing-in-flight, during: turn}` contradicts itself. It is a fair choice, and planning now sees both. |
| 10 (partly) | **resolved** | The write timeout, the read timeout and the 400 path each get a test and a fault, and the dedupe and 410 tests are named for track five with faults. The re-aimed test can fail: I measured `serve-one-connection-at-a-time` at exit 101, caught by it. Note that `a_410_marks_the_link_lost` is affected by new finding 17. |
| 11 | **resolved** | I4 takes `render` as a parameter and waits only on I2. One test waits on I1 and I3. |
| 12 | **resolved** | D4 states the cost of a bump per DoD step. Cancel, auth and D17 are ranked DoD 1 and 2 by the R2 row. |
| 13 (partly) | **changed severity: note** | The rebuttal is a fair reading of the brief's test: each direction has one consumer. It is now put to planning as Q12. What remains is that the contract test Q12 relies on ("I6 running against I5") is named nowhere in I6's test list and has no fault. |
| 14 (partly) | **resolved** | The rebuttal holds: the record's key `record` names the record format itself, so the precedence rule does fall through to `kind`. |
| 15 | **resolved** | I1's RED step uses a stub `project` that returns `Err`. |
| 16 | **resolved** | I2 names the `Panics` transport, and the loopback test moved to I5. |

### New findings

| # | severity | where | finding | evidence | what would resolve it |
|---|---|---|---|---|---|
| 17 | minor | I6 ("On `410`, or when `readyState` is `CLOSED`, the link is `lost` and the page rebuilds from 0"); D7; D8 | **The page cannot tell a 410 from any other refusal.** `EventSource` exposes neither the status nor the reason, and any non-200 closes it the same way. The revision adds 403 and 415 (D17), I7 adds 401, and the connection cap gives 503. A rebuild from 0 is right only for 410. Against a persistent 403 or 401 the rebuild loops, where `EventSource` itself had stopped, and the author sees a page that keeps resetting and never says why. A plausible trigger: opening the page at `http://127.0.0.1:5173` when diet was started with `--allow-origin http://localhost:5173`. The named test `a_410_marks_the_link_lost` can pass only against a double that exposes the status, which the real transport never has. | The WHATWG EventSource processing model: a non-200 fails the connection, and the `error` event carries no status. D8's table of statuses. Not run in a browser. | On `CLOSED`, learn the status with one `fetch` of the same URL, or read the stream with `fetch` streaming, which does see the status. Rebuild automatically only on 410. Show any other status to the author, with backoff. Write the track-five test against that path. |
| 18 | note | I4 (`Config` carries … `opened`); I2 (`session.start` carries `opened`); D7 ("measured once") | **`opened` has two inputs.** If `serve` is handed a separately measured `opened`, the stream's identity can disagree with the log's own header by a millisecond. That is the record's own argument against stating a value twice. | The I4 shape and the I2 change list, side by side. | Have `serve` read `opened` from the session's seq 0, or add a test that the id's `opened` equals `session.start.opened`. |
| 19 | minor | I6 ("DoD 1 end to end is the I5 manual run with the page open"); I5r; §3 order | **DoD 1's acceptance leaves out I5r, which the plan's own premises put on DoD 1's path.** §1 says every dogma model thinks, and Q10 recommends a dogma model with its reasoning streamed. Yet the stated DoD 1 acceptance is the I5 run, which with such a model streams nothing while it thinks (D18). The capture that I5r needs first ("a person with a GPU") has no owner and no place in the order. Declaring DoD 1 done on the I5 run would be a partial result presented as done. | D18; Q10's recommendation; I6's acceptance line. | When Q10 names a thinking model, make I5r part of DoD 1's acceptance. Name who captures the stream, and when. |
| 20 | minor | D18 (a), I5r | **D18 decides what the page sees of reasoning, but not what the trunk carries.** qwen3.6's operating point sets `preserve_thinking = true`, whose receipt reads "with it off, the prefix a fork re-sends is not byte-identical to what the session sent, and the fork prefills cold" (`operating-points.toml:34`, `:45`). `Message` has only `role` and `content` (`client/shape.rs:43-48`), and the session puts only the answer text on the trunk (`session.rs:484-487`). So even with I5r, each turn re-sends a trunk without the reasoning the server generated. Whether the warm tail survives is what DoD 3's forks depend on. This is inferred from the receipt; nothing here was measured. | The pointers above. | Add to Q10: "When reasoning streams, does it join the trunk's assistant message, as `preserve_thinking` requires?" If it does, I5r extends `Message` (in `diet/src/client/`, which is track three's), and the trunk test covers the reasoning. |
| 21 | note | I2 (`Requested {turn, lane}` per call); D4 (`request.failed` requires `request`) | **Where `Requested` is pushed is unstated.** v0 makes `request.failed` cite an earlier `request`. The path where the turn's thread cannot start (`session.rs:327-335`) settles a turn whose thread never ran. If `Requested` is pushed where the call begins, in the thread, that path writes a `request.failed` that cites nothing. That is an invalid v0 line, on a path no test can reach. D4's order already implies the right place. | The pointers above. | State that `Requested` is pushed in `ask`, under the lock that admits the ask. |

### Checked in round 2 and found sound

- **`<opened>-<seq>` against the brief.**
  - The log line's primary key is still `seq`, inside `data:`. `opened` is a measured field of `session.start`. The composite exists only in SSE framing, as the resume cursor.
  - Replay is still "from a sequence" (`?from=n`, or the seq half of the id), and clients still dedupe by `seq` read from the data.
  - Nothing is invented: both halves are issued or measured. The record has no start-time field that `opened` would shadow.
  - The page never parses the id; `EventSource` echoes it back.
- **The new checks against the Vite-proxy path that D9 recommends.** `--allow-origin http://localhost:5173` admits the page's forwarded `Origin`. It also admits the forwarded `Host` (`localhost:5173`, when `changeOrigin` is off); with `changeOrigin` on, `Host` becomes diet's own, which is also admitted. A same-origin `EventSource` GET carries no `Origin`, and `is_none_or` admits it. A cross-site preflight is refused (403 or 404). With `changeOrigin` on, rebinding through the proxy is left to Vite's own host check. D17 says so, and Q5 asks.
- **The `request` event.**
  - It takes the record's name (`record/mod.rs:513`) and the ruled lane `trunk`, not the record's legacy `main`.
  - References are seqs.
  - `slot` is left out until R4, as the vocabulary ruling requires.
- **D17's order of checks.** Host and Origin come before content type, and content type before auth. A cross-site JSON POST needs a preflight, and the preflight meets 403 or 404. This holds whether a browser sends `Origin` or not.
- **`--listen` in I7.** I5 cannot bind off loopback, and I7 lands the flag, the credential and the fail-closed check together.
- **The rebuttals to findings 4, 9, 13 and 14.** Each is a fair, stated limit, not a silent gap. For 13, see the note above.

### Questions I would add

- **To Q10:** does streamed reasoning join the trunk's assistant message, as `preserve_thinking` requires? (Finding 20.)
- **To Q5 or I6:** how does the page learn why its stream closed? (Finding 17.)

### What I did not check in round 2

- No browser ran. How `EventSource` handles 410, and what the Vite proxy forwards, are spec-argued.
- No llama-server ran, so finding 20 is inferred from the operating point's receipt.
