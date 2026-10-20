# How to begin a drive

One start path, from a clean checkout to a recorded session (#287, the ticket that asked for one). It is two steps: start `serve`, then start the surface. Nothing runs anywhere as a service.

**With no model server, start at [No model at hand](#no-model-at-hand)**: the same two steps against a stand-in, then the session driven from the page or from `curl`.

## Three roles

A drive has three parts. They may be one machine or three:

- **The model server:** an endpoint, anywhere `serve` can reach. It answers `…/v1/chat/completions`.
- **The harness host:** `serve` and the surface, both on the machine that holds the code the model works on. The model's commands and edits run there, under that operating system's sandbox (`--worktree`, **The model's commands**, below), so the harness host is wherever that code is.
- **The browser:** anywhere. On the harness host, open the surface directly. Anywhere else, reach it through an ssh tunnel to the harness host, not by opening `serve` or the surface to the network:

  ```
  ssh -L 5173:localhost:5173 <harness host>
  ```

  Then open `http://localhost:5173/?drive` on the browser's machine. `serve` listens on loopback by default. Off loopback it refuses to start without `--auth-file`, and it refuses a wildcard address. The tunnel needs neither.

Every command below runs on the harness host, from the repository root, unless it says otherwise.

## What you need

On the harness host, read from the tree, so they hold for whoever runs this:

- **Rust** at the version `rust-toolchain.toml` pins (`channel`). With [rustup](https://rustup.rs) installed, the first `cargo build` fetches it.
- **Node** at the version `exercise/package.json`'s `engines` asks for, with `npx`. It runs the stand-in model and the surface, and fetches the pinned `pnpm` itself.
- **The wasm32 target and the pinned `wasm-bindgen` CLI**, for the surface's shell gate (step 2's `gate`): `rustup target add wasm32-unknown-unknown`, and `cargo install wasm-bindgen-cli --version <the pin> --locked`, the pin being `wasm-bindgen`'s exact version in `diet/wasm/Cargo.toml`. Without the gate built, the page does not load.
- **A browser** for the surface, on any machine (**Three roles**). Without one, `curl` drives the session instead (**No model at hand**, below).
- **A model server**, the floor or anything answering `…/v1/chat/completions` the same way. Without one, use the stand-in (**No model at hand**), which needs no GPU. Setup fetches the Rust toolchain, crates, npm packages and, for `verify` only, Playwright's Chromium. A drive reaches only loopback and the endpoint you name; the stand-in drive, loopback alone.

Only the surface's own tests (`npx --yes pnpm@11.20.0 -C exercise verify`) also need Playwright's Chromium; `exercise/README.md` says how to install it.

## What you bring

- **The endpoint:** the model server's URL. A base URL (no path, or `/v1`) is completed to `/v1/chat/completions`. It goes on the command line, never in a file.
- **The key**, when the model server asks for one: a file holding it, named with `--key-file`. It is sent as a bearer credential, never taken as an argument, and never written to the log or the record.
- **The worktree:** an empty directory, named absolutely with `--worktree`, where the model's commands run and write. It is the code the model works on, so it lives on the harness host (**Three roles**), outside this checkout.
- **The auth file:** a file holding `user:password`, named with `--auth-file`. A regimen that runs commands makes `serve` ask every request for it, and the surface presents it for the page. Keep it outside the checkout, where no commit can take it and the model's sandbox never reads it (it joins the session's secrets).
- **The head:** the trunk's system message, a file you name with `--head`. [`heads/floor.md`](heads/floor.md) is a worked example: the head the 2026-10-03 rehearsal ran with. Edit it, or bring your own. The log carries the head the session ran with, and each request's `head_sha256`, so a recording cites whatever ran.
- **The regimen:** [`floor.toml`](floor.toml) binds the session to the maintainer's floor, the 3.8 line's config r2 (#497): substrate `accel24-tabbyapi-exl3-qwen38-27b-3p00`, Qwen3.8 27B as EXL3 on TabbyAPI. Its reasoning is `xhigh` and its sampler the registry's card for the line. It turns the product on, so a first drive shows a tool call, a fork and a meditation (#721):
    - **the model's commands:** `bash`, run in your worktree under macOS's sandbox (Seatbelt), with approvals off as T1 runs them. The regimen lets commands read anywhere (`sandbox_reads = "all"`), which is built for Seatbelt alone, so on a Linux harness host serve refuses it;
    - **interview forks:** one per gap, on a read or a scoping-marked ask, answering through the record tools in ask set `t2`;
    - **self-capture:** the model records what it learns as it goes.

    Every other lever is at its default.
  - **What `serve` confirms at start** is what the substrate's registry entry declares, and what the server reports of it (#509):
    - TabbyAPI reports no build, so the floor's engine is declared by its entry, not asked.
    - `serve` reads `GET /v1/model` and refuses to start if the server reports a model, context, cache, batch, chunk, vision setting or chat-template digest other than the entry declares.
    - A llama-server substrate is asked `GET /props` instead, and must report its entry's `build_info`.
    - The start line names what was confirmed and what stands declared.
  - Every request, the trunk's and the interview fork's, carries the regimen's `[sampler]` settings, the values the record's `sampler_card` names, in the digits written. A `[sampler]` key outside `temperature`, `top_p`, `top_k`, `min_p`, `repeat_penalty` and `seed` refuses the start. Without `--regimen`, no sampler setting is sent and the server's defaults apply.
  - **On a hosted API** (`server_kind = "api"`, #555), `--model` must be the entry's `weights_model_id`, the model the provider bills, and an entry naming none refuses the start. `serve` then checks the regimen against what the entry declares its API would refuse with a 400, read from the provider's docs: `max_output_tokens`, `efforts`, and each `api_<setting>` (the one value accepted, or `none`). A regimen that would meet one refuses the start, naming the declaration; nothing is sent. What the entry leaves undeclared is unchecked, and the log's `session.start` names it in `api_unchecked`. `[sampler] pins = "none"`, the table's only key, sends no sampler setting, so the provider's defaults sample and the record says so; only an `api` entry takes it. [`sonnet-5-5.toml`](sonnet-5-5.toml) is a regimen on Claude Sonnet 5.5 that passes the start this way.
  - **A hosted API's regimen declares its request limits** (#555), each required and none with a default, since each spends money: `max_api_requests`, `api_retries` and `api_retry_after_max_seconds`. A missing one refuses the start, named. Every request the session sends counts against `max_api_requests`, whatever its lane, the meditation's pre-warm included. The request that would pass the cap is never sent: the log says `request.failed` `api_cap`, its turn settles `api_cap`, and every later ask is refused `api-cap`. The log's `session.start` names the cap.
  - **Retries** (#555): a `429` or `529`, and nothing else, is retried at most `api_retries` times. It waits the reply's `retry-after` when that is at most `api_retry_after_max_seconds`. A longer wait is not retried, and the request fails. With no header, the wait is 2 s, doubling each retry, bounded by the same cap. Each retry is logged `request.retried` before it goes, and takes from `max_api_requests` first, so a retry never passes the cap. `session.start` names the policy. What the harnesses we compare against do by default, read at the commits pinned in `client/anthropic.rs` (OpenCode 2's, `055d95bb`, is the one this was chosen on):

    | | Pi | OpenCode 2 | Qwen Code |
    |---|---|---|---|
    | retries | 3, at the agent (`coding-agent/src/core/settings-defaults.ts`); 0 in the SDK | 5, at the session (`opencode/src/session/retry.ts`); 0 in the SDK | 7 attempts (`core/src/utils/retry.ts`) |
    | backoff | 2 s, doubling, to 60 s | 2 s, doubling, ±25 %, to 30 s without a header | 1.5 s, doubling, to 30 s |
    | `retry-after` | the agent's retry ignores it; the provider's, off by default, honours it to 60 s and fails past it | honoured, uncapped | honoured on 429 and 503 |

    Pi and OpenCode 2 agree that retries are on by default and start at 2 s, doubling: that is the backoff here. They disagree on the count and on `retry-after`. Qwen Code honours `retry-after`, which settles that tie. Nobody settles the count, so the regimen declares it, with no default.
  - **A dry run before a live one** (#555): `diet-drive scenario FILE --dry-run [--regimen R] --out DIR` rehearses the script on `canned-scenario-hosted` under the regimen the live run would take, `--regimen` or the scenario's `live_regimen`. Only its `substrate` and `substrate_hardware` are swapped, so its limits, sampler, effort and levers run as written. The script's own replies answer, nothing leaves loopback, and no key but the rehearsal's is read. The report's `plan` names:
    - the rung, its `weights_model_id`, and the regimen's limits;
    - the rehearsal's requests by lane, and within `interview` and `audit` by fork kind, with its retries and attempts;
    - its requests' `sized_tokens` summed, and their `max_tokens` summed, an upper bound on their output;
    - whether the rehearsal met the cap, and the rung's unchecked keys;
    - `bound`: `max_api_requests` × (the rung's window + its output cap), labelled a bound, not an estimate.

    A regimen on a substrate that is not an `api` entry is refused. Expectations are checked as in any run, so a script the live regimen would run differently fails here first. Prices are not in the tree; the counts are what a cap is set from.
  - **The drive needs it:** `--record` refuses to start without `--regimen`, because a record's `start` names the regime it ran under. Only a rehearsal, with `--log` alone, can leave it out; then `serve` runs against any server and claims no substrate.
  - **On your own server,** `floor.toml` refuses to start unless your server reports what the floor's entry declares. Leave `--regimen` out for a rehearsal, or register your own machine and model and name them in a regimen of your own: [`substrates/README.md`](../../substrates/README.md), section **Registering your own box**. The registry is compiled into `diet-drive`, so registering means rebuilding.

## 1. Start `serve`

Name the endpoint for this shell, the model server's URL (**What you bring**). Make the worktree, empty, and the auth file, both outside the checkout; here, under your home directory:

```
export DIET_ENDPOINT=<the model server's URL>
mkdir -p ~/drives/first && test -z "$(ls -A ~/drives/first)"
mkdir -p ~/.config/discipline && (umask 077 && printf 'driver:%s\n' "$(openssl rand -hex 16)" > ~/.config/discipline/drive.auth)
```

From the repository root:

```
cargo build -p discipline-diet --bin diet-drive
target/debug/diet-drive serve --endpoint "$DIET_ENDPOINT" --key-file <a file holding the key> \
    --model Qwen3.8-27B-exl3-3.00bpw-img1024 \
    --head diet/drive/heads/floor.md --regimen diet/drive/floor.toml \
    --worktree ~/drives/first --auth-file ~/.config/discipline/drive.auth \
    --port 7801 --allow-origin http://localhost:5173 \
    --log session.log --record session.record.jsonl
```

**`--log` and `--record` are part of the drive, not options to it.** The drive exists to produce a recording, and a session run without them is a rehearsal. The paths above write at the repository root, untracked, and an attached image is copied beside the log under `files/`; name paths outside the checkout to keep them out of it. The two outputs:

- **`--log`** is the session's log, written as each line is appended. It holds byte for byte what `GET /events` streams.
- **`--record`** is the session's record, written once when the session ends. It is projected from the log, with `session.record.jsonl.unspellable.json` beside it naming what the record cannot spell. Both digests are reported on stdout. The record is read back before it is written. A row the reader proves missing (a `prefix.changed` it owes) is added as `unattributed`, and the repair is named in the sidecar's `repairs`. Any other refusal leaves the record empty and keeps the projected rows at `session.record.jsonl.refused.jsonl`, with the refusal in the sidecar and on the end line, and serve exits as for a record it could not write (#645).
- **A record lost after its log was written** is projected again offline (#879): `diet-drive record --log FILE --regimen FILE --out FILE [--recording DIR]` writes the record serve would have written after `ended`, its sidecar, and, for a regimen that runs commands, its receipt. It refuses a log that does not end with `ended`, and one written under another registry than this build's. What only the running session held is left out and named under `unfilled`, each field with why, on stdout, in the sidecar and in the receipt: the summary row, whose product is the working memory at the end; the receipt's `allow`, whose pre-seeds' digests read the worktree at start and whose workspace store is not logged; `reference_modified`, whose baseline was taken at start; and, under a sandbox, `tmpdir`. Approvals are counted from the log, each decision once.

If a file already holds something, naming it empties it. The first stdout line says so (`log_truncated`, `record_truncated`), and a failure after the emptying names what it emptied. A start refused for its usage, its regimen or the engine check empties nothing: those refusals come first.

`--model` is the served model's name, the floor's being the entry's `served_model`. Leave `--key-file` out for a server that asks for no key.

**The output cap** is `--max-output-tokens`, else the regimen's `max_output_tokens`, else **64000** (the harness vote, #569). That leaves room for a reasoning model's thinking at `xhigh`. Pass the flag for another value. A turn that hits the cap is logged `capped` and settles `failed`, and it is not answered.

**A refused start** prints one JSON line on stdout instead, `{"error": <why>, "ok": false}`, and exits `1` for an input it cannot use (the endpoint, the head, a key or auth file, the regimen, the engine check), `2` for a usage it refuses (`--record` without `--regimen`, a `[sampler]` key it cannot send, a listen address it will not take) or an address it cannot listen on, or a server that could not start, or `3` when a file it was told to write cannot be opened. Flags it cannot parse print the usage on stderr and exit `2`.

**The first line on stdout** of a start that succeeds is JSON:

- `listening` and `opened`;
- with `--log`, `log` and `log_truncated`;
- with `--record`, `record` and `record_truncated`;
- with `--regimen`, also `substrate`, `registry_sha256`, `engine_build` and `engine_identity`.

## The model's commands

A regimen that declares `allowed_commands` runs the model's `bash` calls (#298); the list is the pre-seeded session set, and `[]` pre-seeds nothing.

- **`--worktree DIR`** is required with it, absolute: where the commands run. Each runs under the regimen's confinement, opened before anything binds. `isolation = "vm"` is refused, and `isolation = "none"` needs `[limits] max_steps`.
- **A command no approval covers waits on you**, with no timeout. `GET /events` shows it as an `event: waiting` (`request`, `id`, `command`, `cwd`, `reason`, `segments`), and `event: answered` (`request`, `id`) once decided; answer with `POST /approve` and `{"call": ID, "scope": "once"|"session"|"workspace"|"decline"}` (204, or 409 with `nothing-waiting` or `stale`). The surface (`?drive`) draws and answers it. A `cancel` or an `end` while it waits settles the call `cancelled`.
- **Workspace approvals** are kept under `$XDG_STATE_HOME`, else `~/.local/state`, in `discipline/approvals/`, never in the worktree.
- **The receipt** is written when the session ends, beside the record (or the log) as `FILE.receipt.json`: the final allow set, the denylist's digest, the decisions by scope, `approval_policy`, `lifecycle_scripts`, `env_passthrough`, and `reference_modified` for each writable git checkout.

## 2. Start the surface

From the repository root, once per checkout, install the surface's dependencies, then start it. Both run `pnpm` pinned through `npx`, because a `pnpm` at another version switches itself to the pinned one, and on an Intel Mac that switch fails (#194):

```
npx --yes pnpm@11.20.0 -C exercise install
npx --yes pnpm@11.20.0 -C exercise gate
DIET_DRIVE=http://127.0.0.1:7801 DIET_DRIVE_AUTH_FILE=~/.config/discipline/drive.auth npx --yes pnpm@11.20.0 -C exercise dev
```

`gate` builds `diet`'s shell gate to wasm (`exercise/src/gate/wasm/`), which every page imports; it needs the wasm32 target and the pinned `wasm-bindgen` CLI (**What you need**). Run it once per checkout, and again after `diet/wasm` changes. Without it the page shows Vite's error that `./wasm/diet_wasm.js` cannot be resolved.

Open `http://localhost:5173/?drive` in the browser. From another machine, open it through the tunnel (**Three roles**). The surface's dev server proxies `/events`, `/commands`, `/approve` and `/files` to `serve` (`exercise/vite.config.ts`), so the browser reaches `serve` only through the surface, and both stay on loopback.

**The auth file goes to the surface too.** `floor.toml` runs the model's commands, so `serve` requires `--auth-file` (`user:password`) on every request. The page cannot answer that credential itself, so `DIET_DRIVE_AUTH_FILE` names the same file to the surface, and its proxy presents it. Without it the page says the drive asks for credentials (401).

**A first drive** shows the product end to end:

- **A tool call:** ask the model to list the files in its directory. Its `bash` call runs in the worktree under the sandbox, and the answer comes back.
- **An interview fork:** mark an ask as scoping in the page, or have the model read a file. When the turn settles, one fork asks it what it decided, and the patch it records lands in working memory.
- **A meditation:** declare one from the page, or post `{"kind":"declare-meditation"}` to `/commands`. Working memory replaces the conversation, and the next ask runs on the refill.

## One process: `serve` serves the page

Step 2's dev server is for working on the surface. To drive with `serve` alone, and no Node at run time, build the surface's bundle once and let `serve` serve it on its own origin (#716):

```
npx --yes pnpm@11.20.0 -C exercise install
npx --yes pnpm@11.20.0 -C exercise gate
npx --yes pnpm@11.20.0 -C exercise build:drive
```

Then start `serve` as in step 1, naming the bundle with `--ui exercise/dist/app`. Its first line names the page as `page`, `http://127.0.0.1:7801/?drive`. A release archive or a native app ships that directory as `ui/` beside the binary, where `serve` finds it with no flag. Open it in the browser, or through the tunnel from another machine (**Three roles**). Forward `serve`'s port in place of 5173: `ssh -L 7801:localhost:7801 <harness host>`.

The page and the session share one origin, so there is no proxy and no `--allow-origin`. Under `--auth-file`, the page asks for the credential as every route does. The browser prompts once, with the auth file's user and password, and sends them with the page's own calls.

Rebuild the bundle after the surface changes. Building it needs Node; serving it does not.

## Ending

Send `end`: `{"kind":"end"}` posted to `serve`'s `/commands`. `serve` then:

1. logs the settlement to `ended`, which is the log's last line;
2. writes the record;
3. lets open streams finish;
4. exits `0`.

You do not need to interrupt it. Once it has exited, a later command finds nothing listening. In the moment before (while the record is written and the streams drain) one is answered `409` and writes nothing to the log.

## A profile, and the first-run check

The endpoint, the model, the head and the regimen can live in a per-user config instead of on every start's command line. The file is `$XDG_CONFIG_HOME/discipline/drive.toml`, or `~/.config/discipline/drive.toml` when `XDG_CONFIG_HOME` is unset. It is your own file, outside any checkout; nothing about it goes in the tree. It is written in the regimen's TOML subset, with two tables of tables:

```toml
[endpoints.floor]
url = "http://HOST:PORT/v1/chat/completions"
key_file = "~/.config/discipline/floor.key"

[profiles.floor]
endpoint = "floor"
model = "the-model"
head = "~/heads/floor.txt"
regimen = "~/regimens/floor.toml"
auth_file = "~/.config/discipline/drive.auth"
port = 7801
```

- **Keys:** a profile's keys are `serve`'s flags without their dashes, with `_` for `-`.
- **`endpoint`:** names an `[endpoints]` table, which supplies its `url`, and its `key_file` unless the profile names one. It can instead be a URL itself.
- **Credentials:** a key or credential is named by its file's path. No key holds one written in, so a profile cannot inline a secret.
- **Paths:** a leading `~` means the home directory.

Start from it:

```
diet-drive serve --profile floor
```

A flag given on the command line beats the profile's, and `--config FILE` names another config file.

**The first-run check.** `diet-drive check --profile floor` (or with `serve`'s flags) starts nothing and binds nothing. It reports four results on one screen, each `ok`, `failed` or `skipped`, with why:

1. **endpoint:** whether the endpoint answers `GET /v1/models`.
2. **model:** whether it lists the model.
3. **engine:** whether the regimen's engine check passes, as `serve` runs it before it binds.
4. **sandbox:** whether the sandbox opens on this host: Seatbelt on macOS, `bwrap` on Linux.

It exits `0` when nothing failed, `1` otherwise.

## An agent drives it over MCP

`diet-drive mcp` is an MCP server on standard input and output. Its tools drive one running `serve`, so a Claude app or Claude Code session drives the session in place of the page. Start `serve` as in step 1, then give the MCP server its `listening` and, when `serve` has one, the same auth file:

```
diet-drive mcp --url http://127.0.0.1:7801 --auth-file ~/.config/discipline/drive.auth
```

Attach it to Claude Code with the binary's absolute path:

```
claude mcp add diet-drive -- /abs/path/to/target/debug/diet-drive mcp --url http://127.0.0.1:7801 --auth-file ~/.config/discipline/drive.auth
```

In the Claude app, add the same command under `mcpServers` in its MCP configuration, as `command` and `args`.

**From nothing, in one call.** Started with no `--url`, the server's `start` tool takes a profile's name. It launches `diet-drive serve --profile NAME`, attaches to the address `serve` prints, and uses the profile's auth file. `stop` ends the session and the `serve` it started; nothing is left running, and closing the MCP server stops it too. Once `serve` has logged `ended` it is writing its record, which on a long session takes minutes, and `stop` waits for it to exit however long that takes (#879); a `serve` that has neither exited nor logged `ended` within 30 seconds is killed. So the Claude Code line can be:

```
claude mcp add diet-drive -- /abs/path/to/target/debug/diet-drive mcp
```


| tool | what it calls |
| --- | --- |
| `start`, `stop` | a profile launched as `serve`, with `phase` its `start-phase`; the session and that process ended |
| `status`, `wait` | `GET /state`, with the current `phase` and a pending proposal and its evidence read from the log; `GET /wait` |
| `ask` | the `ask` command: `text`, `scoping`, `files`; then waits for the turn (below) unless `wait` is false |
| `attach` | a local PNG uploaded to `POST /files`; returns its `sha256` for an ask's `files` |
| `approve` | `POST /approve`: `call`, `scope` |
| `cancel`, `end` | the matching commands; `cancel` takes the current turn when none is named |
| `memory` | `GET /memory`, or `?all` |
| `edit`, `flag` | `edit-entry`, `flag-entry` |
| `declare_meditation`, `refill_anyway` | the matching commands |
| `propose_phase`, `move_phase`, `rule_phase` | the operator's proposal (`propose-phase`), the operator's move alone (`move-phase`, refused `not-unilateral` under `operator_unilateral = false`), and a ruling on the model's (`ratify-phase`: `meditation`, `keep` or `stay`, the pending proposal when no `call` is named) |
| `open_tangent`, `close_tangent` | the matching commands |
| `events` | `GET /events`: the log lines after a seq, of the kinds named |

**`ask` waits for its turn** by default. It returns when the session is next idle (the turn and its forks and audits settled), holds a meditation, waits on an approval, or has ended, or when `timeout` passes (30 minutes unless given). It sends `notifications/progress` while it waits, when the call carries a progress token. It returns why it returned (`idle`, `held`, `approval`, `ended` or `timed_out`), the last state, and a digest of what the turn did: the final answer, each tool call and its outcome, each fork and audit with its patches, meditations, refusals, and how the turn settled. Claude Code moves a call that runs past two minutes to a background task and delivers its result as a notification, so a long turn tells the driving session when it is done. `wait: false` returns at once with the ask's seq and turn.

Each tool returns `serve`'s status and body as they came, a refusal's `refused` tag included. A tool that cannot reach `serve`, or is given the wrong arguments, says so as a tool error. The loop an agent runs is the one under `status` and `wait`: read the state, send a command, then wait with `after` set to the last state's `last_seq`.

## No model at hand

Run this first: a whole session, driven end to end, with no model server, no GPU and no regimen. Each long-running command gets its own terminal, every one from the repository root: the stand-in, `serve`, and (for the `curl` path) one watching the events and one sending commands.

**The stand-in model.** `exercise/scripts/model-stand-in.mjs` answers every request on `127.0.0.1:7901` with `diet`'s own captured llama-server reply, byte for byte. The reply came from a random-weight model, so its words are noise; what a drive checks is that every part of the pipeline runs.

```
node exercise/scripts/model-stand-in.mjs
```

It runs until you stop it: Ctrl-C in its terminal, once the session has ended.

**`serve` against it.** Use the stand-in's own head, and leave out `--regimen` and `--record` together: the stand-in is no registered substrate, and `--record` needs `--regimen`. With `--log` alone, the run is a rehearsal.

```
cargo build -p discipline-diet --bin diet-drive
target/debug/diet-drive serve --endpoint http://127.0.0.1:7901/v1/chat/completions \
    --model stand-in --head exercise/scripts/model-stand-in.head.txt \
    --port 7801 --allow-origin http://localhost:5173 --log session.log
```

The first stdout line is JSON naming `listening`; it names no `substrate`. `--log session.log` writes the log at the repository root, where `git status` shows it untracked; name a path outside the checkout to keep it out of the way.

**Drive it from the page:** start the surface as in step 2 and open `http://localhost:5173/?drive`. Ask anything; the noise answer streams into the trunk, and the receipt folds when the turn settles.

**Or drive it with `curl` alone,** with no browser. In one terminal, watch the session's log as server-sent events:

```
curl -sN http://127.0.0.1:7801/events
```

In another, send the commands, one JSON object each:

```
curl -s -X POST -H 'Content-Type: application/json' -d '{"kind":"ask","text":"hello"}' http://127.0.0.1:7801/commands
curl -s -X POST -H 'Content-Type: application/json' -d '{"kind":"end"}' http://127.0.0.1:7801/commands
```

An ask is answered `200` with the turn it opened, `{"seq":1,"turn":1}`. `end` is answered `200` with `{}`.

**What a settled session looks like.** Each event is one `data:` line, after its `id:` line, holding one line of the log as JSON with a `kind`. In order:

1. `session.start`, the head and the model it serves (`seq` 0);
2. `ask`, your text, then `settlement` from `awaiting` to `turn`;
3. `request`, the trunk's call to the model, with its `head_sha256`;
4. `delta`, once per streamed piece of the answer;
5. `response`, the whole answer. The stand-in's captured reply stopped at its length limit, so it carries `"capped":true` and `"finish_reason":"length"`;
6. `turn.settled`. Against the stand-in its `reason` is `failed`, because a capped turn is never taken as an answer (**The output cap**, above). The pipeline ran; a real model's uncapped answer settles `final`;
7. `settlement` from `turn` back to `awaiting`;
8. after `end`, `settlement` from `awaiting` to `ended`, the log's last line.

After `end`, the stream closes, `serve` exits `0`, and `session.log` holds the same lines the stream carried. Before `end`, a command `serve` refuses is answered `409` with `{"refused": <why>}`, and the log gains a `refused` line naming it. An ask sent while a turn is in flight is one: `{"refused":"in-flight"}`.

**An agent driving it reads the state, not the log.** Three routes beside `/events`, under the same `Host`, `Origin` and auth rules, log nothing:

- **`GET /state`** answers what the session is doing now, as one JSON object:
  - `session`: `running` or `ended`, and `settlement`;
  - `turn`: `number`, and `state` — `running`, `awaiting-approval`, or `settled` with its `reason`;
  - `in_flight`: the `forks` and `audits` running, by their lines' `seq`, the forks `queued` behind them, and whether a pre-warm is `warming`;
  - `meditation`: `state` — `none`, `capturing`, or `held` with its `unaddressed`;
  - `approval`: the prompt waiting on you, in `event: waiting`'s shape;
  - `refusal`: the last refused command, its `seq`, `command` and `because`;
  - `last_seq`, and `idle`: no turn running, nothing in flight or queued, and no prompt waiting.

  `turn`, `approval` and `refusal` are absent when there is none.
- **`GET /wait?until=idle|<kind>&after=<seq>&timeout=<s>`** is the same object, once the session is idle, or once a line of that `kind` (`meditation.held`, `turn.settled`, any line's) is logged after `after` — the whole log when `after` is absent — with that line as `event`. At `timeout` (30 s when absent, at most 300 s) it answers the state as it is with `"timed_out": true`, so loop on it rather than holding one request open. Any other query is `400`.

- **`GET /memory`** is working memory as `serve` holds it: `entries`, the live ones, each in the object's own dump shape (`id`, `content`, `state`, `provenances` with each one's `lane`, `turn` and `fork`, and `supersedes`, `voided_by` or `aliases` when it has them), and `last_seq`. An entry with an edit or flag of yours no meditation has addressed yet carries `operator`: each change's `kind`, `edit` or `flag`, and a flag's `note`. With `?all`, resolved, parked, retired and voided entries come too. Read memory here rather than folding the log again.

```
curl -s -X POST -H 'Content-Type: application/json' -d '{"kind":"ask","text":"hello"}' http://127.0.0.1:7801/commands
curl -s 'http://127.0.0.1:7801/wait?until=idle&timeout=120'
```

`idle` waits for the gap's forks and a meditation's audit, not only for `turn.settled`. A held meditation is idle: it waits on you, and `meditation.state` says so.

## Rehearsing a regimen with no model

To rehearse a session that runs commands, with approvals and a record, use the rehearsal regimen [`replay.toml`](replay.toml) against `diet-drive replay`. It plays two captures of one real llama.cpp session, byte for byte. Every ask is answered with a `bash` call of `ls | wc -l`, and once the call's output comes back, with that session's answer, which settles `final`.

- `ls` is in the regimen's allowed set and `wc` is not, so the first call waits on you: the surface's approval prompt, or `POST /approve`.
- Approve it for the session or the workspace, and later calls run without asking. Decline it, and the model is told so and answers anyway.
- The record carries `turn`, `tool_call` and `response` rows with the captures' counts.

The words are the captures', whatever you ask. `diet-drive replay --answers` serves one captured answer for every request instead, with no call.

```
target/debug/diet-drive replay
target/debug/diet-drive serve --endpoint http://127.0.0.1:7902/v1/chat/completions \
    --model replay --head diet/drive/heads/floor.md --regimen diet/drive/replay.toml \
    --worktree <a scratch checkout> --auth-file <a file holding user:password> \
    --port 7801 --allow-origin http://localhost:5173 --log session.log --record session.record.jsonl
```

`serve`'s engine check passes only against the replay: its `GET /props` reports the `build_info` the registry's `canned-replay` entry declares.

## Scenarios: a drive written down

A scenario is a drive written as steps and expectations, one JSON line each (#760). Its format note is [`diet/scenarios/README.md`](../scenarios/README.md), and each one lives under [`diet/scenarios/`](../scenarios/). From the repository root:

```
target/debug/diet-drive scenario diet/scenarios/levers-run-c.jsonl --out runs/YYYY-MM-DD-<name>
```

This runs it on the canned scenario substrate:
- `serve` is started as a child process, answered by the scenario's own replies;
- it is driven through `/commands`, `/state`, `/wait` and `/memory`;
- its log is checked against the expectations;
- when they pass, the outputs go to `scripts/assemble-run.py` (`--no-assemble` keeps the log and the record under `--out` instead).

One JSON line on stdout says how it went:
- `ok`;
- `substrate`;
- `replies_sha256`, on a canned run;
- `unmet`: the expectations not met;
- `unmatched`: the requests no reply answered;
- `failure`: the step that went wrong.

It exits `0` only when all of it passed. `./verify.sh`'s `scenarios` check runs every scenario this way, and fails when there are none.

**Re-firing a run on a model:** add `--endpoint URL --model NAME` (and `--key-file FILE`). It runs under the scenario's `live_regimen` unless `--regimen` names another. The replies are ignored. The assembled README says who drove it: the scenario, re-fired on the regimen's substrate. The endpoint is never written.

