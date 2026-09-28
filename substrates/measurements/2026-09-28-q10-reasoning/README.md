# Q10: reasoning on the drive endpoint's server, measured

These are the three measurements #117 routed to the data seat (Q10, as amended 2026-09-27). All three were taken on 2026-09-28 against the running production server of `ada48-llamacpp-qwen38flashnext-q20`, instance `2026-09-28`, in `substrates/registry.toml`. Every request was read-only: the server was never stopped or relaunched. The serving line records `--jinja` and no `--reasoning-format`.

## 1. The stream

`diet/client/fixtures/llama-server-e7051ef-reasoning-stream.http` (sha256 `b91695d8…`) is one streamed thinking turn.

- **How it was captured:** off a raw socket, like `llama-server-4df29be-stream.http`, with `stream: true` and `include_usage`. It is byte for byte what the server sent, with no edits: nothing in it names a path, a host or an account.
- **What it carries:**
  - 295 events carrying `delta.reasoning_content`, then 14 carrying `delta.content`;
  - a `choices: []` chunk with `usage` and `timings`, the timings including `draft_n` and `draft_n_accepted`;
  - `data: [DONE]`.
- **The same framing as the reference:** `text/event-stream` inside `Transfer-Encoding: chunked`.
- **The turn:** `turn1.json` holds its decoded reasoning (1,074 characters) and answer (37 characters).

## 2. Re-sending reasoning: `ab.json`

The same turn-2 history (turn 1, its answer, a follow-up) was sent three times, with `max_tokens` 1, reading `timings`:

| order | `reasoning_content` on the assistant turn | prompt tokens | cached (`cache_n`) | prefilled (`prompt_n`) |
| --- | --- | --- | --- | --- |
| 1 | re-sent | 420 | 400 | 20 |
| 2 | dropped | 125 | 85 | 40 |
| 3 | re-sent | 420 | 85 | 335 |

- **The template preserves reasoning.** Re-sent, it renders into the history (420 tokens), and the whole generated turn is reused from cache.
- **Dropping it breaks the prefix.** The history is 295 tokens shorter and diverges at the previous assistant turn, where only the 85 tokens before the thinking are reused.
- **A drop costs the warm prefix for what follows.** Re-sending again after a drop prefilled 335 tokens, because the dropped request had overwritten the slot's cache.

Order is a factor, and the three rows are one sequence on one slot, not independent draws.

## 3. The tokenized prefix diff: `prefix-diff.json`

The turn as generated is the turn-1 generation prompt, which on this template already opens `<think>\n`, followed by `reasoning_content`, `</think>\n\n` and the content. Tokenized by the server's `/tokenize`, all 400 of its tokens are an exact prefix of the re-rendered turn-2 prompt (420 tokens).

- **A second framing, recorded beside:** a `\n` before `</think>` diverges at token 383.
- **It agrees with the A/B:** the server's own cache matched 400 tokens there.

This is the check I5r's trunk test pins.

## The scripts

- **`capture.py`** took items 1 and 2, and a first item 3.
- **That first item 3 assumed the stream opened `<think>` itself.** The template's generation prompt already opens it, so the draft double-counted the tag and diverged at token 89.
- **`prefix-diff.py` re-ran item 3 with the template's actual framing.** Its output is the committed `prefix-diff.json`. The earlier result is not kept.
- **Both scripts read the server's address and key from the machine's launch file.** Neither prints nor writes the key.
