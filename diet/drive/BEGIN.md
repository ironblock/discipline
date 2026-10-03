# How to begin a drive

One start path, from a clean checkout to a recorded session (#287). It is two steps: start `serve`, then start the surface. Nothing runs anywhere as a service. Whoever drives runs both on their own machine, against a model server they name.

## What you bring

- **The endpoint:** the model server's `…/v1/chat/completions` URL. It goes on the command line, never in a file.
- **The head:** the trunk's system message, a file you name with `--head`. [`heads/floor.md`](heads/floor.md) is a worked example: the head the 2026-10-03 rehearsal ran with. Edit it, or bring your own. The log carries the head the session ran with, and each request's `head_sha256`, so a recording cites whatever ran.
- **The regimen:** [`floor.toml`](floor.toml) binds the session to the maintainer's registered floor, substrate `accel24-beellama-qwen27b-q4kxl`.
  - `serve` refuses to start unless the server's `GET /props` reports that substrate's registered engine, and the record names what it ran on.
  - **The drive needs it:** `--record` refuses to start without `--regimen`, because a record's `start` names the regime it ran under. Only a rehearsal, with `--log` alone, can leave it out; then `serve` runs against any server and claims no substrate.

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

If a file already holds something, naming it empties it. The first stdout line says so (`log_truncated`, `record_truncated`), and a failure after the emptying names what it emptied.

**The output cap** is `--max-output-tokens`, **8192 by default**. That leaves room for a reasoning model's thinking: on the floor, three turns that hit a 512 cap, re-sent at a cap of 4096, finished at 3,561, 408 and 1,873 completion tokens, one sample each ([#290, measured](https://github.com/ironblock/discipline/issues/290#issuecomment-5969377550)). Pass the flag for another value. A turn that hits the cap is logged `capped` and settles `failed`, and it is not answered.

**The first line on stdout** is JSON:

- `listening` and `opened`;
- `log` and `log_truncated`;
- `record` and `record_truncated`;
- with `--regimen`, also `substrate`, `registry_sha256`, `engine_build` and `engine_identity`.

## 2. Start the surface

From the repository root, once per checkout, install the surface's dependencies, then start it. Both run `pnpm` pinned through `npx`; `exercise/README.md` says why (#194):

```
npx --yes pnpm@11.20.0 -C exercise install
DIET_DRIVE=http://127.0.0.1:7801 npx --yes pnpm@11.20.0 -C exercise dev
```

Open `http://localhost:5173/?drive`. The surface's dev server proxies `/events` and `/commands` to `serve` (`exercise/vite.config.ts`).

## Ending

Send `end`: `{"kind":"end"}` posted to `serve`'s `/commands`. `serve` then:

1. logs `ended`, which is the log's last line;
2. writes the record;
3. lets open streams finish;
4. exits `0`.

You do not need to interrupt it. A command sent after `end` is refused (`409`) and writes nothing.

## No model at hand

`node exercise/scripts/model-stand-in.mjs` replays a captured llama-server reply on `127.0.0.1:7901`. Point `--endpoint` at `http://127.0.0.1:7901/v1/chat/completions`, use `exercise/scripts/model-stand-in.head.txt` as the head, and leave out `--regimen` and `--record` together: the stand-in is not the floor, and `--record` needs `--regimen`. With `--log` alone, that run is a rehearsal.
