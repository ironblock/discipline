# I0: a streamed tool call from a real llama-server, and turn 2 after the round-trip (#29)

These captures are I0 of #29's plan (comment 5943518924): a real llama-server streams a `bash` call, the command runs, and turn 2 re-sends it in both shapes Q9 leaves open. They were taken 2026-10-02 on the Mac Pro against substrate `macpro-llamacpp-qwen38flashnext-q20-cpu` in `substrates/registry.toml`, which is registered by the same change.

**The substrate.**
- **Engine:** ironblock's llama.cpp fork at `e486f80`, built CPU-only on this host.
- **Model:** the Qwen3.8-Flash-Next GSQ-RCO Q2_0 shards and the Q4_K_M MTP draft (`weights_*` in the entry).
- **Flags:** the drive endpoint's chat-relevant flags: `--jinja`, no `--reasoning-format`, `-np 4 --kv-unified`, q8_0 KV, the MTP draft at `--spec-draft-n-max 3`.
- **How it differs from the drive endpoint** (`ada48-llamacpp-qwen38flashnext-q20`): the hardware (CPU, not the 48 GiB card), the context (16,384, not 262,144), and the build counter. This server's `system_fingerprint` reads `b11102-e486f802d`; the endpoint reports `b8-e486f80`, and the commit parts agree.
- **Why not the endpoint itself:** the endpoint is a public server. This seat's standing work there is read-only measurement, and these captures are not that.

**How the captures were taken.** `capture.py` sends each request over a raw socket and keeps the server's bytes exactly as received, beside the request body as sent.

- **The request bodies:**
  - Turn 1 and turn 2's `user` shape are rendered by the client's own `wire::streaming_body`, through `i0_bodies.rs`. Copy that file to `diet/examples/` to rebuild it. `turn1.request.json` re-renders byte for byte from it and the nonce in `notes.json`.
  - The `openai` shape can't be rendered by the client: `Message` has no `tool_calls` and `Role` has no `tool`. So `capture.py` splices it into the `user` body's text, replacing the last two messages and leaving every byte before them, the head included, as the client wrote it.
- **The request's content:**
  - **The tool:** one tool, `bash {command}`, in the client's rendering. The function carries `name` and `parameters`, no description.
  - **Thinking:** off, through `chat_template_kwargs.enable_thinking = false`.
  - **Sampling:** temperature 0.6, top_p 0.95, seed 7, max_tokens 512.
  - **The prompt:** a system line, then a user ask opening with a per-run nonce.
- **Turn 1** asks for `ls | wc -l`. The model called `bash` with exactly that. The command was read before it ran, then run in a scratch worktree of this repository, and its output, `      18\n` (BSD `wc`'s padding), is `tool-output.txt`.
- **Turn 2 sends both shapes, `user` first and then `openai`:**
  - **`user`:** the assistant's streamed content (empty), then a user message: ``The bash tool ran `ls | wc -l` and printed:\n      18\n``. The client's `Message` can't carry a call, so this shape **drops the call itself**: the assistant turn is `content: ""` and the call survives only as the paraphrase in the user's text. The plan's T6 and S1 expect the assistant's call to be carried, so this is the shape as the client can render it today, not a ruling on Q9. The wording is this capture's choice.
  - **`openai`:** an assistant message with `content: null` and `tool_calls` (the call's id, name and arguments as streamed), then a `tool` message carrying the output.

| capture | reply | request body | reply sha256 |
| --- | --- | --- | --- |
| turn 1, the call | `turn1.http` | `turn1.request.json` | `d72b1c06…` |
| turn 2, `user` shape | `turn2-user.http` | `turn2-user.request.json` | `2bd5ceaf…` |
| turn 2, `openai` shape | `turn2-openai.http` | `turn2-openai.request.json` | `5186d6e7…` |

`notes.json` has every request and reply digest and size, the nonce, and each reply as assembled. The replies go into `diet/client/fixtures/` by courier on #29, which is track three's directory.

## What the server did

**Turn 1: how a streamed call is fragmented.** The reply has 16 JSON `data:` events and then `[DONE]`, plus three bare `:` SSE comment lines, which the reader already skips (`diet/src/client/stream.rs`).

| events | what they carry |
| --- | --- |
| 5 | role chunks, `delta: {"role":"assistant","content":null}`: one plain first chunk and four carrying `prompt_progress`, the frames `return_progress` adds |
| 1 | `tool_calls[0]` with `index` 0, `id`, `type: "function"`, `function.name: "bash"`, and the first argument fragment `{` |
| 8 | `tool_calls[0]` with `index` and an `arguments` fragment only: `"command":"`, `ls`, ` \|`, ` wc`, ` -`, `l`, `"`, `}` |
| 1 | `delta: {}` with **`finish_reason: "tool_calls"`** |
| 1 | `choices: []` with `usage` (prompt 334, completion 30) and `timings` |

- No `content` delta was streamed.
- The arguments assemble to `{"command":"ls | wc -l"}`, which is valid JSON.
- The finish word comes on its own chunk, after the last fragment.

**Turn 2: the warmth receipt.** Each reply's final chunk carries `timings`.

| shape | prompt tokens | `cache_n` | `prompt_n` | answer |
| --- | --- | --- | --- | --- |
| `user` | 367 | 330 | 37 | "There are **18** files in the current directory." |
| `openai` | 383 | 330 | 53 | the same sentence |

- **Both shapes kept the same 330-token prefix warm**, four short of turn 1's 334-token prompt.
  - Why 330 is not measured. Two readings fit:
    - the last four tokens are the rendered generation prompt, which turn 2 replaces;
    - this is a hybrid recurrent model, so warm reuse stops where a state checkpoint landed (the endpoint entry's hazard). In every reply the last progress frame is at the prompt's full total (334, 367, 383) and the one before it at total less four (330, 363, 379). Turn 1 started with nothing cached and still shows that boundary at 330, which fits a checkpoint as well.
  - On the second reading, `cache_n` can't tell the two shapes apart past 330 on this model.
- **So the receipt does not choose between the shapes here.** It shows neither invalidated the shared head. It doesn't show they would cost the same on a model whose cache reuse is not checkpoint-bound.

## Not checked

- **The e486f80 concurrency hazard does not reach these captures.** On 2026-10-03 the inference seat found that e486f80 lets concurrent requests see each other's context under `-np > 1` with `--kv-unified` (`substrates/measurements/2026-10-03-ada48-concurrency/`). Each capture here was one request at a time against an otherwise idle server, so no ubatch held more than one sequence.

- One sample of each, at temperature 0.6. The fragmentation's dependence on the sampler, on longer arguments, and on parallel calls is not measured.
- Thinking on was not captured, so a call after streamed `reasoning_content` is not represented.
- The `openai` shape was sent second, into a cache that already held the `user` shape's turn 2. Its `cache_n` equals the `user` shape's, so it reused only the shared prefix.
- Whether the drive endpoint fragments the same way on its card is inferred from the shared commit, not measured there.
- The fixture names in the courier carry the commit and not the substrate, so a reader could take them for the endpoint's captures. Naming is track three's, and the courier's table names the source.
- The reader in `check-fingerprints.py` matched only `.so` names, so on macOS it hashed the stub executable without its seven `.dylib` libraries. This change teaches it `.dylib`, with a selftest case seen red on the old rule. `engine-read.json` is the read after the fix.
