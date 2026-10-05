# How to begin a drive

One start path, from a clean checkout to a recorded session (#287, the ticket that asked for one). It is two steps: start `serve`, then start the surface. Nothing runs anywhere as a service. Whoever drives runs both on their own machine, against a model server they name.

**With no model server, start at [No model at hand](#no-model-at-hand)**: the same two steps against a stand-in, then the session driven from the page or from `curl`.

## What you need

Read from the tree, so they hold for whoever runs this:

- **Rust** at the version `rust-toolchain.toml` pins (`channel`). With [rustup](https://rustup.rs) installed, the first `cargo build` fetches it.
- **Node** at the version `exercise/package.json`'s `engines` asks for, with `npx`. It runs the stand-in model and the surface, and fetches the pinned `pnpm` itself.
- **A browser**, for the surface. Without one, `curl` drives the session instead (**No model at hand**, below).
- **A model server**, `llama-server` or anything answering `…/v1/chat/completions` the same way. Without one, use the stand-in (**No model at hand**), which needs no GPU. Setup fetches the Rust toolchain, crates, npm packages and, for `verify` only, Playwright's Chromium. A drive reaches only loopback and the endpoint you name; the stand-in drive, loopback alone.

Only the surface's own tests (`npx --yes pnpm@11.20.0 -C exercise verify`) also need Playwright's Chromium; `exercise/README.md` says how to install it.

## What you bring

- **The endpoint:** the model server's `…/v1/chat/completions` URL. It goes on the command line, never in a file.
- **The head:** the trunk's system message, a file you name with `--head`. [`heads/floor.md`](heads/floor.md) is a worked example: the head the 2026-10-03 rehearsal ran with. Edit it, or bring your own. The log carries the head the session ran with, and each request's `head_sha256`, so a recording cites whatever ran.
- **The regimen:** [`floor.toml`](floor.toml) binds the session to the maintainer's registered floor, substrate `accel24-beellama-qwen27b-q4kxl`.
  - `serve` refuses to start unless the server's `GET /props` reports that substrate's registered engine, and the record names what it ran on.
  - Every request, the trunk's and the interview fork's, carries the regimen's `[sampler]` settings, the values the record's `sampler_card` names, in the digits written. A `[sampler]` key outside `temperature`, `top_p`, `top_k`, `min_p`, `repeat_penalty` and `seed` refuses the start. Without `--regimen`, no sampler setting is sent and the server's defaults apply.
  - **The drive needs it:** `--record` refuses to start without `--regimen`, because a record's `start` names the regime it ran under. Only a rehearsal, with `--log` alone, can leave it out; then `serve` runs against any server and claims no substrate.
  - **On your own server,** `floor.toml` refuses to start unless your server's `GET /props` reports the floor's registered `build_info` literal; the machine itself is not checked at start. Leave `--regimen` out for a rehearsal, or register your own machine and model and name them in a regimen of your own: [`substrates/README.md`](../../substrates/README.md), section **Registering your own box**. The registry is compiled into `diet-drive`, so registering means rebuilding.

## 1. Start `serve`

From the repository root:

```
cargo build -p discipline-diet --bin diet-drive
target/debug/diet-drive serve --endpoint "$DIET_ENDPOINT" --model <served model name> \
    --head diet/drive/heads/floor.md --regimen diet/drive/floor.toml \
    --port 7801 --allow-origin http://localhost:5173 \
    --log session.log --record session.record.jsonl
```

**`--log` and `--record` are part of the drive, not options to it.** The drive exists to produce a recording, and a session run without them is a rehearsal. The two outputs:

- **`--log`** is the session's log, written as each line is appended. It holds byte for byte what `GET /events` streams.
- **`--record`** is the session's record, written once when the session ends. It is projected from the log, with `session.record.jsonl.unspellable.json` beside it naming what the record cannot spell. Both digests are reported on stdout.

If a file already holds something, naming it empties it. The first stdout line says so (`log_truncated`, `record_truncated`), and a failure after the emptying names what it emptied. A start refused for its usage, its regimen or the engine check empties nothing: those refusals come first.

**The output cap** is `--max-output-tokens`, **8192 by default**. That leaves room for a reasoning model's thinking: on the floor, three turns that hit a 512 cap, re-sent at a cap of 4096, finished at 3,561, 408 and 1,873 completion tokens, one sample each ([#290, measured](https://github.com/ironblock/discipline/issues/290#issuecomment-5969377550): the ticket where the cap was measured). Pass the flag for another value. A turn that hits the cap is logged `capped` and settles `failed`, and it is not answered.

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
DIET_DRIVE=http://127.0.0.1:7801 npx --yes pnpm@11.20.0 -C exercise dev
```

Open `http://localhost:5173/?drive`. The surface's dev server proxies `/events` and `/commands` to `serve` (`exercise/vite.config.ts`).

## Ending

Send `end`: `{"kind":"end"}` posted to `serve`'s `/commands`. `serve` then:

1. logs the settlement to `ended`, which is the log's last line;
2. writes the record;
3. lets open streams finish;
4. exits `0`.

You do not need to interrupt it. Once it has exited, a later command finds nothing listening. In the moment before (while the record is written and the streams drain) one is answered `409` and writes nothing to the log.

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
