# Scenarios

A scenario (#760) is a drive written down as steps and expectations. `diet-drive scenario` runs it. Every `*.jsonl` here runs in `./verify.sh`'s `scenarios` check on the canned scenario substrate.

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

## `reply`: what the canned substrate answers

`{"reply": {"when": TEXT, "text": TEXT, "calls": [{"id", "name", "arguments"}]}}`

- `when` (optional) and `calls` (optional) shape how the reply is used.
- A reply with `when` answers the first request whose last message holds that text.
- A reply without `when` answers, in file order, a request no routed reply matched.
- Each reply is used once.
- A request no reply is left for is refused `500` and fails the run, naming its last message.
- `calls` are the tool calls the reply makes; `arguments` is the JSON text the model would write.
- `delay_ms` (optional) holds the reply back that long, its events trickled after the delay (#782). The stub answers each request on its own thread, so a held reply orders a trunk's answer after a fork beside it.
- A re-fire on an endpoint ignores every reply.

## `step`: what the runner drives

| step | keys | |
|---|---|---|
| `ask` | `text`, `scoping`?, `refused`? | posted; with `refused`, it must be refused with that tag |
| `flag` | `pick`, `note`, `as`? | flags the entry `pick` names and binds it to the name `as` |
| `edit` | `pick`, `text`, `as`? | edits it; the operator's new entry is bound |
| `declare-meditation` | | |
| `refill-anyway` | | |
| `rule-phase` | `call`, `choice` | rules on the model's phase proposal that call `call` made: `meditation`, `continue` or `stay` |
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
