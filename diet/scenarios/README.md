# Scenarios

A scenario (#760) is a drive written down as steps and expectations. `diet-drive scenario` runs it. Every `*.jsonl` here runs in `./verify.sh`'s `scenarios` check on the canned scenario substrate.

`split-turn.jsonl` (#813) runs on the same windowed substrate: a turn whose own steps outgrow the window, split by the window meditation inside it.

`kitchen-sink.jsonl` (#798) is one session showing every lever that writes a line to the log, each with an `expect`; its log is the page's `kitchen-sink` example. It runs on `canned-scenario-windowed`, the scenario substrate with a declared 24000-token window, so its budget meditation fires at a turn's end and its window meditation inside a later turn. `meditation_at_working_set_bytes` is not shown: serve refuses it (`NotServed`), and only the scripted drive takes it. Its replies are routed by `when` and provisioned one per request, so a request nobody wrote a reply for fails the run rather than taking another's.

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

## `reply`: what the canned substrate answers

`{"reply": {"when": TEXT, "text": TEXT, "calls": [{"id", "name", "arguments"}]}}`

- `when` (optional) and `calls` (optional) shape how the reply is used.
- A reply with `when` answers the first request whose last message holds that text.
- A reply without `when` answers, in file order, a request no routed reply matched.
- Each reply is used once.
- A request no reply is left for is refused `500` and fails the run, naming its last message.
- `calls` are the tool calls the reply makes; `arguments` is the JSON text the model would write.
- A `$job` anywhere in a reply is the last background job id (`bg_` and eight hex digits) the request carries: what `task_stop` names, which no script can know ahead of the run.
- `prompt_tokens` (optional) is the prompt the reply's usage reports, in tokens; absent, the canned server's 600. A trunk that large is what a budget meditation measures.
- `delay_ms` (optional) holds the reply back that long, its events trickled after the delay (#782). The stub answers each request on its own thread, so a held reply orders a trunk's answer after a fork beside it.
- A re-fire on an endpoint ignores every reply.

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
| `wait` | `until`, `timeout`? | `GET /wait` for `idle` or a line's kind, after the last command |
| `end` | | |

- After each admitted ask, meditation or refill, the runner waits until `GET /state` is `idle`.
- `pick` names a live entry, read from `GET /memory`: a number is its place in id order, and text is the first entry whose content holds it.

## `expect`: what the log must show

`{"expect": KIND, "where": {KEY: TEXT, "KEY~": TEXT}, "at_least": N, "name": NAME, "after": NAME}`

- **`expect`:** a line of this kind.
- **`where`:** each field equals the text (`KEY`) or holds it (`KEY~`). A `$name` in the text is the entry bound to that name.
- **`at_least`:** how many lines must match (default 1).
- **`name`:** a name for this expectation's first match.
- **`after`:** a later expectation counts only the lines after the first match of the expectation that `after` names.
