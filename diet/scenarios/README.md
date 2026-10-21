# Scenarios

A scenario (#760) is a drive written down as steps and expectations. `diet-drive scenario` runs it. Every `*.jsonl` here runs in `./verify.sh`'s `scenarios` check on a canned scenario substrate.

`split-turn.jsonl` (#813) runs on the same windowed substrate: a turn whose own steps outgrow the window, split by the window meditation inside it, then a second ask whose request carries the split turn whole: the steps the split compacted rejoin the trunk when the turn settles (#884).

`native-summary.jsonl` (#936) runs the native arm's refill: interviews off, so working memory stays empty, and `meditation_carry = "native_summary"`. A declared meditation asks for a summary on the `summary` lane, and the refill carries it, not a render.

`refutation.jsonl` (#878) runs the refutation interview: turn 1's operator fork, cut as the message arrives (#904), answers `refute_beliefs` with `[]` and settles `decline`; a failed `cat` then refutes, at confidence 9, the note a scoping fork recorded, the same call's second item (a confidence of 11) refused beside it, and the next refill renders the note as it was held. `refutation-nothing.jsonl` asks it once and is answered in prose with no call, so the fork settles `unparseable`; turn 3's trunk calls `refute_beliefs` unprompted and writes its refutation on the `trunk` lane, pointing at the step's request.

`--dry-run` rehearses a scenario under the regimen a live run would take, on `canned-scenario-hosted`, and reports the plan it would spend: requests by lane and fork kind, `sized_tokens` summed, output at `max_tokens`, and the worst case, labelled a bound (`diet/drive/BEGIN.md`, #555).

`background-wait.jsonl` starts two background jobs: one that fails at once, and one that runs longer than its turn. It then waits for the longer job to end, `wait` with `where` waiting past the failed job's line, before the next ask, whose notice reports it completed. Without the wait, or with a `where` ignored, the job has not ended by then, and the run fails.

`hosted.jsonl` (#555) rehearses a hosted API on `canned-scenario-hosted`, the scenario substrate whose `server_kind` is `api`: its replies are served as Anthropic's Messages API streams them, at `/v1/messages`. Turn 1 thinks, signs the thinking, calls `bash` and writes the cache; its second request reads it. Turn 2 is answered `529` with a `retry-after` of 1 s: the session logs `request.retried`, waits, sends the retry and is answered. Turn 3 is answered `429` asking for 120 s, more than the regimen's `api_retry_after_max_seconds`, so it is not retried: `request.failed`, and the turn settles `failed`. A canned hosted run sends a rehearsal key, written beside the run's credential, and withholds `ANTHROPIC_API_KEY` from serve, so no rehearsal can carry an operator's key.

`kitchen-sink.jsonl` (#798) is one session showing every lever that writes a line to the log, each with an `expect`; its log is the page's `kitchen-sink` example. It runs on `canned-scenario-windowed`, the scenario substrate with a declared 37615-token window, so its budget meditation fires at a turn's end and its window meditation inside a later turn. `meditation_at_working_set_bytes` is not shown: serve refuses it (`NotServed`), and only the scripted drive takes it. Its replies are routed by `when` and provisioned one per request, so a request nobody wrote a reply for fails the run rather than taking another's. It gates its step warrants by a classifier (`warrant_gate = "classifier"`, #850), scored by its own `classify` lines, so every step logs `warrant.gated` decisions, fired and not. Its audits ask in chunks of eight notes, two in flight at once (`meditation_audit_concurrency = 2`, #854), each fork carrying its `chunk`. It runs the refutation interview (`refutation_interview`, #878): its failed commands ask `REFUTE_Q` in their turns' gaps, turn 6's ask asked about on arrival, before the turn reads it (#904), in the same turn whose `cat missing.txt` fails and takes the gap's fork, so both triggers show without competing (its operator cooldown of 112 reaches turn 6 and no later ask), and the timed-out build's `refute_beliefs` call quotes a claim never written down, which the next refill renders as it was held. A stopped background job's `background.ended` is placed by the reaper when it sees the job end, concurrent with the step that stopped it, so a regeneration may move it (#896); the example's migration says so. Its background build runs 5 s, longer than its turn, and the step after its ask waits for it to end (`wait` with `where`). Its end then comes in the gap after that turn in every run, and the next ask's notice always reports it: a 1 s build raced the turn, and two runs side by side showed the model different notices. Its published example is stitched in one place, and says so in its migration: each gate's `latency_ms`, wall time the check never compares, is carried from the example a rewrite replaces, matched by lane and place, so a rewrite changes only what the session changed (`exercise/src/drive/kitchen-sink.ts`). Similar notes are merged at write time (`note_dedupe = "on"`): a fork's LEARNED restating a live one is folded into it, its `patch` line carrying `merged` and `cosine`.

A run on the canned substrate shows its model the same bytes on any machine. Its recording and worktree are shown as `/scenario/recording` and `/scenario/worktree` wherever their paths would appear: in tool results, background notices, a capped output's "saved to" and the meditation's "saved at". The whole of a capped output is rewritten before it is kept, so the copies under `files/` and their digests are path-free too. Background job ids are taken from their count, and the model is shown pid 0. Commands still run in the real directories, and a job is stopped by its real pid. The log's metadata (`tool_call.cwd`) stays real, and `session.start` declares `shown_roots` and `scenario_fixed`. Only a canned substrate takes `--shown-root`, so a re-fire on an endpoint is shown where it really ran. One limit: a spilled output, one too large to hold, is copied as it streams, so its kept whole keeps real paths.

The format has no grammar of its own, so it has no directory under `diet/formats/`. It is JSON Lines in the record's value space, read by the record's reader (`diet/formats/record/grammar.pest`, `json::objects`):
- one object per line;
- no `null`;
- no floats;
- no key twice.

`diet::drive::scenario::parse` implements the line schema below. A key a line does not take is refused, naming the line. [`fixtures/minimal.jsonl`](fixtures/minimal.jsonl) has one line of every kind.

## The header (line 1)

| key | | |
|---|---|---|
| `scenario` | required | the name a run is labelled with |
| `regimen` | required | the canned regimen, relative to the file; its substrate is `canned-scenario` |
| `live_regimen` | optional | the regimen a re-fire on an endpoint runs under |
| `head` | required | the trunk's head, relative to the file |
| `approvals` | optional | the `POST /approve` scope (`once`, `session`, `workspace`, `decline`) that answers each prompt; absent, a prompt fails the run |
| `seed` | optional | `{worktree path: file}`: each file, relative to the scenario, copied into the fresh worktree before serve starts (an `AGENTS.md`, say). The regimen must run commands. |
| `fixed_asks` | optional | the asks, by their place among the asks from 0, that are fixed: an operator script (below) lists `[0]`. Every later ask was typed in answer to what the model did, so a scripted run of it is not that answer. A person who knows a later ask was written in advance adds its index. |

## `reply`: what the canned substrate answers

`{"reply": {"when": TEXT, "text": TEXT, "calls": [{"id", "name", "arguments"}]}}`

- `when` (optional) and `calls` (optional) shape how the reply is used.
- A reply with `when` answers the first request whose last message holds that text.
- A reply without `when` answers, in file order, a request no routed reply matched.
- Each reply is used once.
- A request no reply is left for is refused `500` and fails the run, naming its last message.
- `calls` are the tool calls the reply makes; `arguments` is the JSON text the model would write.
- A `$job` anywhere in a reply is the last background job id (`bg_` and eight hex digits) the request carries: what `task_stop` names, which no script can know ahead of the run.
- A `$entry:<call>/<field>` anywhere in a reply is the last record entry id for that call and field the request carries, read from the request's decoded text: `r`, the seq diet built it at, then `/<call>/<field>`, as `resolve_entry` names it. A line logged earlier moves the seq, so a reply names the entry by its call and field instead. With none in the request, it stays as written.
- `prompt_tokens` (optional) is the prompt the reply's usage reports, in tokens; absent, the canned server's 600. A trunk that large is what a budget meditation measures.
- On a hosted substrate (`server_kind = "api"`, #555) a reply may also say what only that wire carries; any other substrate refuses the run:
  - `thinking` (optional): the thinking before the answer, sent as a thinking block with a constructed signature;
  - `cache_read_tokens`, `cache_write_tokens` (optional): of `prompt_tokens`, what the usage reports read from the cache and written to it at the 5-minute lifetime;
  - `status` (optional, 400 to 599): an error in place of an answer, Anthropic's error type for it (`rate_limit_error` for 429, `overloaded_error` for 529, else `api_error`); `retry_after` (optional, only with `status`) is its `retry-after` header in seconds. A reply with a `status` needs no `text`.
- `delay_ms` (optional) holds the reply back that long, its events trickled after the delay (#782). The stub answers each request on its own thread, so a held reply orders a trunk's answer after a fork beside it.
- A re-fire on an endpoint ignores every reply.

## `classify`: what the canned classifier scores

`{"classify": {"when": TEXT, "hypothesis": TEXT, "score": TEXT}}`

- A scenario whose regimen gates by a classifier (`warrant_gate = "classifier"`) is scored on the canned substrate by these rules: `diet-drive scenario` serves `POST /v1/classify` on the substrate's own port and passes serve `--classifier-endpoint`, which only a canned substrate takes. The regimen's `endpoint` is never called.
- A window holding `when` scores a hypothesis holding `hypothesis` at `score`: a decimal from 0 to 1, written as text, since the format has no floats.
- The first rule naming both decides. A pair no rule names scores 0, and an empty `when` and `hypothesis` name every pair.
- Rules are used on every call, never spent.
- The canned classifier reports the model, revision and compute units the regimen declares.
- The rules are part of the replies' digest, after the replies.
- A scenario with `classify` lines whose regimen gates no step by a classifier is refused.
- A re-fire on an endpoint ignores every rule.

## `step`: what the runner drives

| step | keys | |
|---|---|---|
| `ask` | `text`, `scoping`?, `refused`?, `attach`? | posted; with `refused`, it must be refused with that tag; each `attach` path, a PNG in the worktree, is uploaded through `POST /files` and attached by its digest |
| `flag` | `pick`, `note`, `as`? | flags the entry `pick` names and binds it to the name `as` |
| `edit` | `pick`, `text`, `as`? | edits it; the operator's new entry is bound |
| `declare-meditation` | | |
| `refill-anyway` | | |
| `rule-phase` | `call`, `choice` | rules on the model's phase proposal that call `call` made: `meditation`, `keep` (`continue` is read too) or `stay`, posted as written |
| `start-phase` | `phase` | picks the phase the session starts in, before the first ask |
| `propose-phase` | `to`, `meditate`? | proposes a phase move the model accepts or declines; `meditate` absent is serve's default |
| `move-phase` | `to`, `meditate`? | moves the phase on the operator's word alone |
| `open-tangent` | `id` | opens a tangent |
| `close-tangent` | `each` | closes the open tangent, ruling `keep`, `drop` or `park` on every live entry born under it |
| `wait` | `until`, `timeout`?, `where`? | `GET /wait` for `idle` or a line's kind, after the last command; with `where`, as an expectation's, a line of the kind that does not match is waited past |
| `end` | | |

- After each admitted ask, meditation or refill, the runner waits until `GET /state` is `idle`.
- `patience` (optional) on a step it waits after -- an ask to be admitted, `declare-meditation`, `refill-anyway`, `rule-phase`, `start-phase`, `propose-phase` or `move-phase` -- is how long, in whole seconds above 0, that wait may take before the drive fails; absent, 900. A live re-fire whose turns run long names one, and always a number: a hung run still ends. Any other step refuses it.
- `pick` names a live entry, read from `GET /memory`: a number is its place in id order, and text is the first entry whose content holds it.
- `place` (optional, an object) on any step is for a reader. The runner never reads it. An operator script writes it (below).

## `expect`: what the log must show

`{"expect": KIND, "where": {KEY: TEXT, "KEY~": TEXT}, "at_least": N, "name": NAME, "after": NAME}`

- **`expect`:** a line of this kind.
- **`where`:** each field equals the text (`KEY`) or holds it (`KEY~`). A `$name` in the text is the entry bound to that name.
- **`at_least`:** how many lines must match (default 1).
- **`name`:** a name for this expectation's first match.
- **`after`:** a later expectation counts only the lines after the first match of the expectation that `after` names.

## Deriving an operator script from a run's log (#908)

```
diet-drive script --log RUN/session.log --regimen R.toml --out DIR [--name NAME] [--canned SCENARIO] [--recording DIR]
```

This writes a scenario, `DIR/NAME.jsonl` (`script` unless named). It holds every move the run's operator made, in the log's order: asks with their scoping marks and attached files, flags, edits, meditations and refills, phase starts, proposals, moves and rulings, tangents, and the end.

Beside it the command writes:
- the head, `head.md`;
- the regimen, `regimen.toml`: `--regimen`, the one the run ran under, since the log does not hold it;
- each attached file under `files/`, seeded into the replay's worktree.

`diet-drive scenario DIR/NAME.jsonl` plays it. The script fixes the inputs, not what comes back.

- **Each step keeps its place.** Its `place` holds:
  - `turn`: the turn the move belongs to;
  - `between`: always true, since a scenario's steps run once the session has settled;
  - for each ask after the first, `answers`: the `seq` of the trunk response it followed, plus `read_ms` and `compose_ms` from its `idle.gap`.

  The header's `fixed_asks` is `[0]`.
- **Background jobs are waited for.** A job that ended between turns, before the next move, becomes a `wait` for its `background.ended`.
- **The head is what the replay's start begins from.** The instruction files' render (#559) and the fork ask set's priming (#598) come off it, since the replay's start adds them again from its worktree and its regimen. The report names the instruction files the run read, which the replay's worktree must hold.
- **A refused ask is written with no text.** Its log line keeps none, and its refusal turns on the session's state.
- **For a replay on the canned substrate**, `--canned SCENARIO` copies that scenario's `reply` and `classify` lines and its `seed` (under `seed/`).
  - The round trip is tested this way. The kitchen sink's log becomes a script, which replays with the kitchen sink's replies, and its operator lines are the original's apart from `seq` and `t` (`diet/tests/drive_scenario_cli.rs`).
  - A live re-fire takes no replies.
- **A move the format cannot express is refused, naming its line, and never dropped:**
  - a stop;
  - a call the operator moved to the background;
  - approvals answered with different scopes;
  - a tangent closed with more than one ruling;
  - a meditation declared with a phase move;
  - a refused command that is not an ask;
  - a flag or edit whose entry's text does not pick it alone.

## Repeating a scenario (#910)

```
diet-drive scenario FILE --out DIR --repeat N [--stop-file PATH] [--void-cap K]
```

This runs the scenario N times. Each repeat ends before the next starts.
- **Each repeat is its own session.** It gets a fresh worktree, so `--worktree` is refused, and its own outputs in `DIR/repeat-NN/`, from `repeat-01`.
- **Each repeat says which it is.** Serve is told with `--run`: the scenario file's `script_sha256`, the `repeat` and the count, `of`. Its log's `session.start` carries them as `run` (log v17), and so does its record's start (record v3).
- **One JSON line per repeat** as it ends, then the batch's line. A repeat's line holds:
  - `repeat`, `of` and `attempt`;
  - `void`: why it was void, or `null`;
  - `turns`, the `turn.settled` lines, and `settles`, counted by reason;
  - `tokens`, every response's prompt and completion tokens;
  - `captures`, the `patch` lines, and `refusals`, the `refused` lines;
  - `unmet`, the expectations the log did not meet, and `record`, its path.
- **Unmet expectations are data.** A repeat that misses one is still assembled, and the batch still passes. So is a failed turn, a `summary_failed` and the API cap.
- **A void repeat is re-run at its slot.** Void means serve could not run the session to its end for a reason outside the model:
  - serve did not start, crashed or never exited, or the assembler crashed (a refusal is the assembler's answer, not a crash);
  - the worktree, its seed or the run's directories could not be made;
  - a transport failure at any point: a `request.failed` whose reason is `transport` or `crashed`, or `timeout` with nothing arriving.

  An HTTP error the server returned is data. A void attempt's outputs are kept beside the slot, as `repeat-NN.void-M/`. After `--void-cap` re-runs (2 unless given) have voided too, the batch stops, names the slot in `void_cap_reached`, and fails.
- **The batch keeps its lines** in `DIR/runs.jsonl`, one per attempt, void ones included (#914). It is the one source of a run's unmet expectations. A line that cannot be written fails the batch and is named as `runs_failed`.
- **The stop file stops the batch between repeats.** It is `DIR/STOP` unless `--stop-file` names another. It is read before each repeat starts, and the batch's line says where it stopped (`stopped_at`).

## Running a pair (#912)

```
diet-drive pair FILE --a R1 --b R2 --repeat N --out DIR [--endpoint URL --model NAME [--key-file FILE]] [--no-assemble] [--stop-file PATH] [--void-cap K] [--contrast L1,L2,...]
```

This runs the scenario under two regimens, N times each, so their records can be compared.
- **The two regimens must differ in exactly one lever.**
  - The levers are compared as `session.start` names them.
  - Each key the regimens bind differently is also tried alone: `R2` with only that key as `R1` binds it. A key whose change moves no lever, such as a sampler setting, counts as a difference.
  - **`arm` is the one exempt key.** It is a label no request carries: two regimens that differ only in `arm` send byte-identical request bodies (`a_regimens_arm_reaches_no_request`, `diet/tests/drive_serve_cli.rs`).
  - Otherwise the pair is refused before any run. The refusal names the levers that differ (with each state), the keys, and the keys that move no lever.
- **`--contrast L1,L2,...` declares a bundle** (#932: interviews on against native). The pair then differs in exactly what it names, no more and no fewer.
  - A lever is named by its own name, or by a key that moves it.
  - A key that moves no lever is named by its dotted name.
  - A difference left out, or a member that names none, is refused before any run, as `missing` and `extra`.
  - The batch's line names the declared set and each lever's states, as `contrast`. `pair` is the regimens' digest as before, and the two regimens fix the set.
- **The arms interleave ABBA ABBA,** so drift on the box falls on both arms. Each arm's repeats go in `DIR/a/repeat-NN/` and `DIR/b/repeat-NN/`.
- **Each run's `run`** names its `arm` (`a` or `b`), the `pair`, the script's digest and its repeat of its arm, on its log's `session.start` and its record's start. `pair` is the sha256 of the two regimens' sha256s, `R1`'s then `R2`'s, as lowercase hex with nothing between.
- **Each run is a repeat as above.** The same mechanics apply: a fresh worktree, a void run re-run at its slot up to the cap, the stop file read before each run, and one line per run, which names its `arm`. The batch's line names the `pair`, the `lever` with its state under each arm, and how many runs each arm `ran`.

## A pair's table (#914)

```
diet-drive table DIR [--seed S]
```

This reads a `pair` batch and writes `DIR/table.md` and `DIR/table.json`. It is a table, not a claim.
- **What it reads:** the batch's `runs.jsonl`, skipping void attempts, and each run's log at `DIR/<arm>/repeat-NN/session.log`. Each log's `run` must be the run `runs.jsonl` names there. No model is asked.
- **Per arm:** each row's median, range and mean over the arm's runs. The JSON also holds every run's value.
- **Per pair:** two B − A deltas, of the medians and of the means.
  - Each has a 95% percentile interval from the same draws: each arm's runs are resampled with replacement, independently, 10,000 times.
  - The interval's ends are the 250th and 9,750th sorted draws.
  - The seed is 0 unless `--seed` is given, and both files print it.
  - **At small N the interval describes the runs; it is not a test.** A median over five runs is lumpy, and a mean is pulled by one outlier, so both are shown.
- **The rows, and each one's source:**
  - the primary measure (#932): `not declared`, with no numbers, until the maintainer writes it;
  - failed commands: `tool_call` with `name: bash`, `outcome: ran` and `exit` ≠ 0, plus `background.ended` with `exit` ≠ 0;
  - beside them, with no failure dropped: commands ended by a signal (`outcome: ran`, no `exit`), and commands their confinement failed (`outcome: command_failed`);
  - output and prompt tokens, per run and per turn: `response.usage`'s `completion_tokens` and `prompt_tokens`, summed, and per turn divided by the `turn.settled` lines;
  - wall ms per run: the last line's `t`;
  - wall ms per turn: each `turn.settled`'s `t` less its `ask`'s, the run's median;
  - turns (`turn.settled`), forks (`fork`), captures (`patch`), refusals (`refused`), and settles, one row per `reason`;
  - unmet expectations: `runs.jsonl`'s `unmet`.
