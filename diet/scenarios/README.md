# Scenarios

A scenario (#760) is a drive written down as steps and expectations. `diet-drive scenario` runs it. Every `*.jsonl` here runs in `./verify.sh`'s `scenarios` check on a canned scenario substrate.

`split-turn.jsonl` (#813) runs on the same windowed substrate: a turn whose own steps outgrow the window, split by the window meditation inside it, then a second ask whose request carries the split turn whole: the steps the split compacted rejoin the trunk when the turn settles (#884).

`native-summary.jsonl` (#936) runs the native arm's refill: interviews off, so working memory stays empty, and `meditation_carry = "native_summary"`. A declared meditation asks for a summary on the `summary` lane, and the refill carries it, not a render.

`refutation.jsonl` (#878) runs the refutation interview: turn 1's operator fork, cut as the message arrives (#904), answers `refute_beliefs` with `[]` and settles `decline`; a failed `cat` then refutes, at confidence 9, the note a scoping fork recorded, the same call's second item (a confidence of 11) refused beside it, and the next refill renders the note as it was held. `refutation-nothing.jsonl` asks it once and is answered in prose with no call, so the fork settles `unparseable`; turn 3's trunk calls `refute_beliefs` unprompted and writes its refutation on the `trunk` lane, pointing at the step's request.

`--dry-run` rehearses a scenario under the regimen a live run would take, on `canned-scenario-hosted` under the rung's own declarations (`--rehearse-as`, #926), and reports the plan it would spend: requests by lane and fork kind, `sized_tokens` summed, output at `max_tokens`, and the worst case, labelled a bound. A drive that fails partway still plans what its log holds, `"complete": false` (`diet/drive/BEGIN.md`, #555).

A local rung's dry run (#942) rehearses on `canned-scenario`, its substrate alone swapped and no `--rehearse-as`. Its plan says `"rung_kind": "local"` and carries no limits. Its bound is the rehearsal's requests at the rung's window plus the widest `max_tokens` they asked, labelled a bound: a local rung spends no money. A graded dry run's plan counts its grades and labels them `canned`: the graders graded canned replies, so every attempt ran.

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
| `ports` | optional | names of loopback ports reserved for each run (below) |
| `services` | optional | what each run starts beside serve, outside the sandbox (below) |
| `sandbox_unix_sockets` | optional | services' values, each `service.NAME.KEY`, passed to serve as `--sandbox-unix-socket`: sockets the model's commands may connect to |
| `serve_env` | optional | `{VAR: TEXT}`: variables added to serve's environment alone, each text filled as an ask's is |
| `seal` | optional | services' values, each `service.NAME.KEY`: paths a sealed next session cannot read (below) |
| `seal_probe` | optional | texts filled as an ask's are, each a file under the seal that the probe asks a sealed session to read |

## Services (#942)

A run may need things beside serve: a dev server, a browser daemon, a grader's own browser. The header's `services` starts them for each run, outside the model's sandbox, before serve starts. They stop after serve exits, even when the run fails.
```json
"ports": ["model_vite"],
"services": [
  {"name": "model-vite", "run": "exec ./vite.sh", "ready": {"tcp": "model_vite"}},
  {"name": "model-browser", "start": "./browser.sh start", "stop": "./browser.sh stop", "ready": {"command": "./browser.sh status"}}
],
"sandbox_unix_sockets": ["service.model-browser.socket"]
```
- **Ports:** each named port is reserved for the run on loopback, so runs side by side never collide.
- **A service is one of two kinds:**
  - `run`: a command that stays in the foreground. Its process group is stopped with TERM, then KILL after 10 s.
  - `start` and `stop`: a daemon that detaches. `start` writes the PIDs it leaves, one per word, to `$DIET_SERVICE_DIR/pids`. After the run the runner runs `stop`, then ends any of those PIDs still alive, TERM then KILL. Nothing is stopped by a pattern.
- **Ready:** `{"tcp": PORT}` (the port accepts a connection) or `{"command": TEXT}` (polled until it exits 0), within `ready_timeout_s` (60 unless given). A service not ready in time, or one that exits first, voids the run as `services`, and serve never starts.
- **Values:** a service may write `$DIET_SERVICE_DIR/provides.json`, an object of strings: a socket's path, a session id.
- **What each is told,** beside the runner's environment:
  - `DIET_PORT_<KEY>` for each port;
  - `DIET_SERVICE_<NAME>_<KEY>` for each value provided by a service started before it, upper-cased with `-` as `_`;
  - `DIET_SERVICE_DIR`, its own directory;
  - `DIET_WORKTREE`, the run's worktree, when it has one.

  Its commands run under `bash -c` in the scenario's directory, each leading a process group of its own, their output appended to `DIR/services/<name>/out.log`.
- **Where the values go:**
  - serve's environment, which passes on to a command only what the regimen's `env_passthrough` names;
  - every check's environment;
  - each ask's text and each grade's `wrong`, `last` and `right`, as `{port.KEY}` and `{service.NAME.KEY}`. Other braces are text. A placeholder naming a port or service the header does not declare is refused when the scenario is read.
- **Sockets:** `sandbox_unix_sockets` names the values serve is given as `--sandbox-unix-socket`. A socket's path differs per run, so it is a run's fact, as the worktree is, and the regimen's bytes stay the arm's. The receipt names it beside the regimen's own.
- **`serve_env`** adds variables to serve's environment alone, such as a daemon's session id that a command-line tool finds it by: `{"CD4A_INTERNAL_DAEMON_SESSION_ID": "{service.model-browser.session}"}`. Each text is filled as an ask's is. A command still sees only what the regimen's `env_passthrough` names. The run's line names each variable with its source text, never its value.
- **What the run says:** its line names each service's `ready_ms` and what its stop did: `killed` for a foreground one that needed the KILL; `stop_exit` and `pids_left_after_stop` for a daemon.

## A live experiment needs the deny list (#942)

Under `sandbox_reads = "all"` the model's commands read everything but the secrets. A machine that runs experiments holds what encodes them, and a live run's model must read none of it. The standard entries:
- the private runs, with their solutions and notes;
- the checkout container: the whole bare-repo container, with its graders and planning, but not the reference checkouts a task reads;
- the agents' transcripts and memory (`~/.claude`);
- every agent session's scratchpad (`/private/tmp/claude-<uid>`), which is outside `$HOME`;
- the desktop app's data (`~/Library/Application Support/Claude`, `~/Library/Caches/claude-cli-nodejs`, `~/Library/Caches/com.anthropic.claudefordesktop*`);
- the operator's keys and configuration (`~/.config/discipline`, git's credentials).
- the operator's personal folders: `~/Documents`, `~/Desktop`, `~/Downloads`, `~/Pictures`, `~/Movies`, `~/Music`;
- browser and credential stores under `~/Library`: Chrome's and Firefox's profiles (`Application Support/Google/Chrome`, `Application Support/Firefox`), `Safari`, `Cookies`, `Keychains`, `Mail`, `Messages`.

The model's tools keep working with all of these denied: node, rg and git, and its chrome-devtools daemon, which runs outside the sandbox with its own profile in the run's directory and is reached through its socket.

What the deny list does:
- **`DIET_SANDBOX_SECRETS`** is the deny list: colon-separated absolute paths. The runner passes each to serve as `--sandbox-secret`, a per-run fact beside the regimen's own secrets, so the regimen's bytes stay the arm's. The receipt counts the paths and digests the sorted list; it never names one.
- **A live run or a dry run whose regimen runs commands refuses to start with it unset.** `none` waives it, for CI and machines that hold nothing.
- **The probe proves it,** from inside serve's own sandbox: a short session on the canned substrate, under the run's regimen with its substrate alone swapped and the same secrets. Its canned replies run:
  - `ls` on each denied path, which must fail;
  - `cat` of `DIET_SANDBOX_PROBE_FILE`, a file the model must not read, which must fail;
  - ripgrep (`rg --no-ignore --hidden --fixed-strings --files-with-matches --no-messages --max-filesize 64M`, `node_modules`, `Library` and `.Trash` skipped) for `DIET_SANDBOX_PROBE_PHRASE`, text only the denied files hold. **A file over 64 MB is not searched:** a leaked solution, task note or brief is small text, and the transcripts that grow large sit in a denied `~/.claude`. The probe line records the cap.
- **Where the search looks:** `$HOME`'s top level but the denied paths, then `/private/tmp` and `$TMPDIR`.
- **What the search proves:** rg exits 2 wherever a file is unreadable, as a denied one is, so its exit says nothing. What it lists does. It also looks for a canary, a string only a file the probe wrote holds, and must list that file and no other: a search that found not even the canary searched nothing. The phrase and the canary go hex-escaped through `printf`, so no file the probe writes holds either.
- **What the run records:** its line carries the verdict, counts and the rg version only. A probe that read anything voids the run as `sandbox`, nothing else starts, and the failed probe's log is kept beside the run.
- **Once per batch:** a batch probes once for each regimen and deny list, and its later runs say they reused that proof.

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
| `ask` | `text`, `scoping`?, `refused`?, `attach`?, `task`? | posted; with `refused`, it must be refused with that tag; each `attach` path, a PNG in the worktree, is uploaded through `POST /files` and attached by its digest; `task` names the task it asks on its `ask` line |
| `grade` | `task`, `check`, `wrong`, `attempts`?, `last`?, `right`?, `before`?, `timeout_s`? | grades the turn the `ask` right before it settled; see "Grading a task" below |
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

## Grading a task (#942)

A `grade` step comes right after the admitted `ask` of its task. That ask asks the task: its own `task`, or the step's. The step gives the task `attempts` tries (3 unless given), the first being the ask's.
- **After each attempt settles,** the runner runs `check` and logs its verdict as a `grade` line (log v18): the turn, `task`, `attempt` of `attempts`, `right`, `timed_out` when the check ran out of time, and the check's `check_sha256`. Serve logs the line and never sends it to the model.
  - **Right:** the runner sends `right` as an ask, when it is given, and moves on.
  - **Wrong, with attempts left:** it sends `wrong` as the next attempt's ask.
  - **Wrong on the last attempt:** it sends `last`, when it is given, and moves on.
  - Every feedback ask names the task.
  - The templates take `{expected}`, `{got}` (clipped at 1,500 characters, `(nothing)` when blank), `{task}`, `{attempt}` and `{attempts}`.
- **The check** is the operator's command.
  - **Who runs it:** the runner, not serve, so it runs outside the model's sandbox, as `bash -c` in the session's worktree, in its own process group.
  - **Its environment:** the runner's, plus:
    - the run's services' ports and values (above);
    - `DIET_GRADE_OUT`, a directory kept for its attempt, `DIR/grades/<task>/attempt-<n>/`, outside the worktree: for dumps and screenshots;
    - `DIET_GRADE_TASK`;
    - `DIET_GRADE_ATTEMPT`;
    - `DIET_GRADE_SCENARIO_DIR`, the scenario file's directory;
    - `DIET_GRADE_TURN`;
    - `DIET_GRADE_BEFORE`, when the step has a `before`.
  - **Its output:** one JSON line, `{"right": bool, "expected": str, "got": str}`, the last it prints.
  - **When it fails:**
    - A check past `timeout_s` (30 unless given) is killed, its process group with it. The attempt is wrong, with `got` "(the check timed out)".
    - A check that prints no such line fails the run, void as `grader`.
- **`DIET_GRADE_TURN`** is a file, outside the worktree and gone once the check has run. It is written from the session's log once the log holds every line, and holds the latest turn:
  - `task`, `attempt`, `turn`;
  - `answer`, its last trunk response's text;
  - `calls`, each `tool_call` with its `seq`, `name`, `outcome`, `exit`, and its `stdout` and `stderr` as `{text, bytes}`.

  A stream past what a call holds in memory (#734) is in the log as its head and tail alone. Its `bytes` then exceed its text's: it is not whole. Its whole in the recording is joined with its other stream, as the model saw them, so the file can't carry it apart.
- **The check never changes what the model sees.**
  - The runner lists the worktree's entries before and after each check, by path, mode, size and modification time in nanoseconds. A check that added, removed or changed one fails the run, void as `grader`, naming the paths.
  - **What it costs:** one `lstat` per entry, twice per attempt, and no bytes read, so a worktree holding a dependency tree stays cheap.
  - **What it can't do:** with no bytes from before, it can't tell a write that left the bytes as they were from one that changed them. A changed modification time alone, a `touch`, counts as a change, which the model could see.
- **`before`** (optional) runs before each attempt's ask, under the same guard. Its stdout reaches the check as the file `DIET_GRADE_BEFORE`: for a task that must change nothing, compare the state before and after.
- **[`checks/stdout-equals.py`](checks/stdout-equals.py)** is a reference check for a stdout task, `python3 -I stdout-equals.py EXPECTED`. The task is right when a `bash` call in the turn ran, exited 0, and printed the expected text as its whole stdout (CRLF read as LF, trailing whitespace stripped).
- **The record** has no row for a `grade` line. The projection names it in `unspellable`, as graded by the runner. A results check that needs grades reads the log.
- **`script`** derives a graded run's grade step back:
  - its task and its `attempts`;
  - for `wrong`, `last` and `right`, the texts the run sent, filled, not their templates;
  - no ask step of the feedback asks, which the step sends.

  The log keeps the check's digest, not the check, so the derived `check` fails, naming the digest, until it is written.
- **`table`** gains two rows per task:
  - attempts to right: the first right grade's `attempt`, none when none is;
  - never right: 1 when no grade is right, 0 when one is.

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

## A session that goes on from its refill (#948)

The regimen's `refill_session` says where a run goes after its declared meditation:
- **`same`** (the default): the session goes on, as it always has.
- **`fresh`:** the meditation ends the session. A second session starts from the refilled trunk exactly as sent, and from the working memory as it stood, and runs the scenario's remaining steps. The first request of each session after the refill has the same head digest.
- **`sealed`:** `fresh`, and the second session's commands cannot read the header's `seal`: those paths join its deny list, and its probe asks it to `cat` each `seal_probe` file, which must fail. A regimen that says `sealed` under a scenario with no `seal` is refused. A seal is proved even where the deny list is waived (`none`), so a sealed run, a dry run included, needs `DIET_SANDBOX_PROBE_FILE` and `DIET_SANDBOX_PROBE_PHRASE`; without them its second session voids as `sandbox`, and the repeat's line says why under `next`.
- **`blank`:** the second session starts from the head alone, carrying nothing: the run's floor. Its worktree is the seed.

How it runs:
- **The first session keeps what crosses.** At each meditation it writes the refilled trunk (`application/json`) and, when the working memory holds anything, its dump (`application/x-ndjson`) into the recording's `files/<sha256>`, and the meditation line names them as `trunk_sha256` and `object_sha256`, with the files a native refill restored as `restored_files` (log v24).
- **The second session is `serve --carry LOG`.** Serve reads the log's last meditation that kept a trunk, checks each kept file against its digest, and starts from them. Nothing is re-derived. The working memory crosses as its dump, so merge bookkeeping beyond it does not cross, and the dedupe threshold is the second session's regimen's.
- **Its start says where it came from:** `carried_from` (the log's digest, the meditation's `seq`, the trunk's and the object's digests), and, when the trunk restored a file under the seal, `carried_source`, each such file relative to its sealed root (log v24). A native refill restores only worktree files its file tools read, so only a seal inside the worktree can be named. Its record's start carries `carried_from` too (record v6).
- **Its outputs are in `next/`,** beside the first session's: `repeat-NN/next/session.log` and the rest. Services restart for it, so the seal's paths are the second session's own.
- **One run, two sessions:** the repeat's line is the first session's, with the second's under `next`; the second's void voids the run, and its log is the one the scenario's expectations are checked against. `table` reads both logs as one run, and `carried_source` gives the carried runs a row of their own.

- **The worktree crosses too (v26).** At each meditation the session snapshots its worktree into the recording: every regular file kept by digest, and a manifest of path and digest, itself kept by digest (`worktree_sha256` on the meditation line). Symlinks and files over 32 MiB are named in the manifest as skipped, never kept. A `fresh` or `sealed` session restores that worktree, each file checked against its digest, in place of the seed, and its `carried_from` names it (`worktree_sha256`, `worktree_from: meditation`).

## A batch from a recorded refill (#948)

```
diet-drive scenario FILE --out DIR --from-carry LOG [--carry-worktree DIR2] [--repeat N]
diet-drive arms FILE --arms ARMS.toml --repeat N --out DIR --from-carry LOG [--carry-worktree DIR2]
```

Every run starts from the same refill: LOG's last meditation that kept a trunk, carried as a `fresh` session is, and the drive takes up FILE after its declared meditation. So every arm or variant starts from an identical trunk, working memory and worktree, each digest-checked.
- **The script is the one LOG ran:** LOG's `run.script_sha256` must be FILE's digest, so LOG must be a repeat's or a batch's (a single run's log names no run). A different script is refused.
- **The worktree:** the snapshot LOG's meditation kept. A log written before v26 kept none, so `--carry-worktree DIR2` supplies one: it is snapshotted into the run's own recording, restored from there, and named `worktree_from: given`. Verifying that DIR2 is the worktree as it stood at the refill is the supplier's job, and the start line says so by its tag. A `--carry-worktree` beside a log that keeps its own is refused.
- **A delivery variant (v27):** `--carry-trunk FILE` sends FILE's messages, in the recording's trunk format, in place of the kept trunk: the same notes delivered another way. The start line names its sha256 as `carried_from.variant_sha256`, beside the `trunk_sha256` it varies. A variant on a run that is not carried is refused, never dropped.
- **The phase tool's description (v27):** `--phase-tool-description FILE` replaces the pinned description where the regimen declares phases, and the start line names its sha256 as `phase_tool_description_sha256`. Where no phase tool is declared it is refused.
- **A first-move probe:** `--only-task TASK` runs only that task's ask, the first after the meditation that names it, and then ends: no grade, and nothing after it. It needs `--from-carry`. A regimen's `[limits] max_steps` caps the probe at K calls, and `lite/grader/first_move.py` measures it (`lite/README.md`).
- **A killed run's raw outputs:** while the assembler runs, a run's own `services/`, `grades/` and `next/` wait in `.<repeat>.parts` beside its directory (#960). A run killed then leaves them there; that is where to look.

## Repeating a scenario (#910)

```
diet-drive scenario FILE --out DIR --repeat N [--stop-file PATH] [--void-cap K]
```

This runs the scenario N times. Each repeat ends before the next starts.
- **Each repeat is its own session.** It gets a fresh worktree, so `--worktree` is refused, and its own outputs in `DIR/repeat-NN/`, from `repeat-01`.
- **Each repeat says which it is.** Serve is told with `--run`: the scenario file's `script_sha256`, the `repeat` and the count, `of`. Its log's `session.start` carries them as `run` (log v17), and so does its record's start (record v3).
- **Each batch has an id** (#932): `batch`, 16 hex digits, drawn once per invocation and named in every run's `run` (log v25, record v7), on every `runs.jsonl` line and on the batch's line. A void attempt's re-run at its slot keeps it. A head nonce is drawn from the run, its `batch` included, so two batches of one manifest never send the same nonce, and a run's nonce re-derives from its `run` alone. Runs from before v25 have no `batch`, and two batches of one manifest shared their nonces.
- **One JSON line per repeat** as it ends, then the batch's line. A repeat's line holds:
  - `repeat`, `of` and `attempt`;
  - `void`: why it was void, or `null`;
  - `turns`, the `turn.settled` lines, and `settles`, counted by reason;
  - `tokens`, every response's prompt and completion tokens;
  - `captures`, the `patch` lines, and `refusals`, the `refused` lines;
  - `unmet`, the expectations the log did not meet, and `record`, its path.
- **Unmet expectations are data.** A repeat that misses one is still assembled, and the batch still passes. So is a failed turn, a `summary_failed` and the API cap.
- **What a run writes in its own directory stays beside its assembled run.** Its services' files (`services/`), its grader's (`grades/`) and a carried session's (`next/`) are set aside while the assembler builds the directory, then moved back in, whatever the assembler answered. The assembler refuses a directory that exists, and before this every run with services or grades was refused. These files are the run's raw outputs: unlike the assembled log and record, they are not scrubbed or hygiene-scanned.
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
- **Each run's `run`** names its `arm` (`a` or `b`), the batch's digest as `arms`, the script's digest and its repeat of its arm, on its log's `session.start` and its record's start. The digest is the sha256 of the two regimens' sha256s, `R1`'s then `R2`'s, as lowercase hex with nothing between. Before log v22 and record v5 it was spelled `pair`, which both still read.
- **Each run is a repeat as above.** The same mechanics apply: a fresh worktree, a void run re-run at its slot up to the cap, the stop file read before each run, and one line per run, which names its `arm`. The batch's line names the `pair`, the `lever` with its state under each arm, and how many runs each arm `ran`.

## Running k arms (#932)

```
diet-drive arms FILE --arms ARMS.toml --repeat N --out DIR [--endpoint URL --model NAME [--key-file FILE]] [--no-assemble] [--stop-file PATH] [--void-cap K]
```

This runs the scenario under each of a manifest's regimens, N times each. A pair is its k = 2 case: `pair` is `arms` with arms `a` (the baseline) and `b`.
- **The manifest** names its `baseline`, then one table per arm:
  ```toml
  baseline = "native"
  [arm.native]
  regimen = "native.toml"
  [arm.refute]
  regimen = "refute.toml"
  [arm.dedupe]
  regimen = "dedupe.toml"
  contrast = ["refutation_interview", "note_dedupe"]
  ```
  - An arm's name is a directory its runs go in: 1 to 32 lowercase letters, digits, `-` and `_`, starting with a letter or digit.
  - `regimen` is relative to the manifest. `contrast` is optional, as `--contrast` is for a pair, and the baseline declares none.
  - Two or more arms, and no other key.
- **Every other arm must differ from the baseline in exactly one lever, or in exactly its contrast,** checked as a pair's are. Otherwise the batch is refused before any run, naming the `arm`.
- **The order is a Williams design.** Block b runs the arms in row b of the design, cycling through its rows.
  - Over a whole cycle, every arm runs in every position equally often and follows every other arm once. So a linear drift on the box, and a carryover from the run before (the server's cache), balance across arms.
  - A cycle is k rows when k is even and 2k when it is odd. Two arms' rows are AB and BA, so a pair runs ABBA ABBA as before.
  - The batch's line says the design, its rows, the blocks run, and whether N is a whole number of cycles (`balanced`). When it is not, it says so: positions and carryover balance only at a multiple of the cycle.
- **Each run's `run`** names its `arm` by name, the batch's digest as `arms`, the script's digest and its repeat of its arm. The digest is the sha256 of every arm's regimen sha256, in arm-name order, as lowercase hex with nothing between.
- **Each run is a repeat as above,** in `DIR/<arm>/repeat-NN/`. The batch's line names the `baseline`, the digest, what each other arm differs in (`against`), the `order`, and how many runs each arm `ran`. It is also kept as `DIR/batch.json`.

## A batch's table (#914)

```
diet-drive table DIR [--seed S]
```

This reads a `pair` or `arms` batch and writes `DIR/table.md` and `DIR/table.json`. It is a table, not a claim.
- **The baseline** is what `DIR/batch.json` names. A pair batch written before it was kept has arms `a` and `b`, and its baseline is `a`.
- **What it reads:** the batch's `runs.jsonl`, skipping void attempts, and each run's log at `DIR/<arm>/repeat-NN/session.log`. Each log's `run` must be the run `runs.jsonl` names there. A run that went on in a second session (#948) also has `next/session.log`: counts and times are summed over both, each turn's time kept, and a task's grades are wherever it was graded. No model is asked.
- **Per arm:** each row's median, range and mean over the arm's runs. The JSON also holds every run's value.
- **Per other arm:** two deltas against the baseline, the arm's minus the baseline's, of the medians and of the means. For a pair, B − A.
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
  - **carried sealed source** (#948), only when a run of the batch went on in a second session: 1 for a run whose carry held a file under the seal (its second start's `carried_source`), 0 for one whose did not. Memory alone is told apart from memory carrying restored source.
  - **cached and new prefill, per lane** (#932): each `response`'s `timings` `cache_n` and `prompt_n` (llama.cpp, TabbyAPI), else its `usage` `cached_tokens` and the rest of `prompt_tokens` (a hosted API, cache writes counted new), summed by its `request`'s lane.
    - A run's lane value counts only when every response on the lane reported it. Otherwise it prints `not reported`, never 0, and the JSON's `notes` say `reported k of n`.
    - The rows are labelled carryover-free only when every run's head carried its own nonce (`head_nonce = "per_run"`). Otherwise they say the cached prefill may include the server's cache of the run before.
