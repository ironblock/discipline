# Harness baseline: Pi, OpenCode 2, Qwen Code, diet/exercise

A measurement, not a roadmap. It records what each harness shows the model and does with its context, so that a comparison between them is a comparison of like with like (distribution matching: "The harness presents tools in the shape the model was trained on", the program document's cross-cutting concerns, #511). One pass, taken 2026-10-09; the OpenCode 2 column re-read at its `v2.0.32` tag on 2026-10-20 (#925).

## Sources

| column | source | read at |
|---|---|---|
| **Pi** | `earendil-works/pi` | `42a3497d03ad17e308a2299fa824727894f2c0ec` |
| **OpenCode 2** | `anomalyco/opencode`, tag `v2.0.32`, plus the "Migrate from V1" doc (a snapshot taken 2026-10-09, cited as `migrate:N`) | `843a6c514e606b436a7fe713422ace0a93ad3d61` |
| **Qwen Code** | `QwenLM/qwen-code` | `c0c697c8a3d7d0460d37c497b76d52968c9216f7` |
| **diet/exercise** | this repository | `ff279e1f594274833b72a5d2e07614bcf5b34ed3` |
| **Qwen3.8 thinking levels** (Qwen3.8 section only) | the maintainer's notes on Qwen3.8-27B effort control, cited as `ql:` | `834fd3b52eb056b3f80bba3f26d3f0234ebc2208` (2026-08-17) |

How to read the cells:
- **Prefixes.** `pi:`, `oc:`, `qc:` and `ours:` name the repository; the path after them is repo-relative.
- **Qwen Code paths.** A `qc:` path that does not start with `packages/` is under `packages/core/src/`.
- **OpenCode versions.** The tag is the V2 line: V1 (`packages/opencode`) is not in it. The runtime is `packages/core`; the wire protocols and the cache policy are `packages/ai`. `oc:` paths are given in full.

**Gap labels** (the diet/exercise cell carries one where it differs):
- **(1)** changes what the model sees, so it bears on fair comparison;
- **(2)** a lever we have, or should add;
- **(3)** does not bear on the thesis.

## Lineage

**Qwen Code is a fork of Google Gemini CLI.**
- **What its own sources say:**
  - The README says it was "originally based on Google Gemini CLI v0.8.2" and stopped syncing at Qwen Code v0.1 (`qc:README.md:214`).
  - The history begins with Gemini CLI's own initial commit (`add233c504`, 2025-04-15).
  - The first Qwen pre-release is `a9d6965bef` (2025-07-22), which added `QWEN.md`.
  - The last upstream sync is `eb95c131be` ("Sync upstream Gemini-CLI v0.8.2", 2025-10-23).
- **What it inherited:** the `@google/genai` content model, the tool-class framework, and the context-file hierarchy.
- **What it changed for Qwen:**
  - an OpenAI-compatible content generator with DashScope and Qwen OAuth;
  - per-family tool-call examples in the prompt;
  - recovery of XML tool calls written as text;
  - DashScope prompt caching.
- **What it has become:** at the read commit it has 10,601 commits. Its compaction, deferred tools and hooks follow Claude Code's shape, so at HEAD it is not the Gemini-CLI-shaped harness of mid-2025. See the closing note on Qwen training.

Pi and OpenCode are independent harnesses.

## Tools

| | Pi | OpenCode 2 | Qwen Code | diet/exercise |
|---|---|---|---|---|
| **default set the model sees** | `read`, `bash`, `edit`, `write` (`pi:packages/coding-agent/src/core/settings-manager.ts:83`); grep, find and ls exist but are opt-in (`pi:…/core/tools/index.ts:95-105`) | `edit` and `write`, or `patch` (by model, below), `glob`, `grep`, `question`, `read`, `shell`, `skill`, `subagent`, `webfetch`, `websearch`, plus the `opencode` session and worktree tools and the MCP resource tools (`oc:packages/core/src/plugin/internal.ts:231-248`); `todowrite` is removed (`packages/core/src/database/v1-migration.bun.ts:901`) | `read_file`, `grep_search`, `glob`, `edit`, `notebook_edit`, `write_file`, `run_shell_command`, memory tools, `zoom_image`, `skill`, and more; `todo_write` and `web_fetch` when enabled; `list_directory` is opt-in (`qc:config/config.ts:12786-12930`, `:12596-12605`, `:12800-12808`). `--bare`: `read_file`, `edit`, `notebook_edit`, `run_shell_command` (`:12728-12744`) | **`bash` only**, declared when the regimen runs commands (`ours:diet/src/bin/drive.rs:617-619`, `diet/src/drive/tool_loop.rs:146-163`). **(1)** |
| **edit shape** | `edit`: `edits[]` of exact `{oldText,newText}` (`pi:…/tools/edit.ts:20-40`); `write` overwrites the whole file (`write.ts:50-53`); no `apply_patch` | `edit` and `write`, or `patch`, never both (below) | `edit`: `file_path`, `old_string`, `new_string`, `replace_all` (`qc:tools/edit.ts:804-826`); `write_file`: `file_path`, `content` (`tools/write-file.ts:838-854`); no `apply_patch` (not found) | none; writes go through `bash` **(1)** |
| **per-model tool sets** | none (`pi:…/core/agent-session.ts:3650-3652`); Anthropic OAuth re-cases the tool names to Claude Code's (`pi:packages/ai/src/api/anthropic-messages.ts:96-124`) | a model ID containing `gpt-` but not `oss` or `gpt-4` gets `patch` only, and every other model, Qwen included, gets `edit` + `write` (`oc:packages/core/src/tool/plugin/patch.ts:296-306`) | none in the tool set (`qc:tools/*.ts`); only the prompt's tool-call examples vary per family (see Instructions) | none (`ours:diet/src/drive/tool_loop.rs:1376-1384`) |
| **search** | `grep`, `find`, `ls`, opt-in | `grep`, `glob`; `read` lists directories (`oc:packages/core/src/tool/plugin/read.ts:44,187-198`) | `grep_search` (ripgrep), `glob` | through `bash` **(1)** |
| **shell** | `bash`; description ≈308 characters (`pi:…/tools/bash.ts:255`) | `shell`, with a `background` argument (see background shells, below) (`oc:packages/core/src/tool/plugin/shell.ts:23,57-60`) | `run_shell_command`: `command`, `is_background`, `timeout`, `description`, `directory` (`qc:tools/shell.ts:5754-5781`) | `bash`: one property, `command`. No function-level description; the property text is 26 characters (`ours:diet/src/drive/tool_loop.rs:149`, `diet/src/client/shape.rs:517-522`) **(1)** |
| **web** | not built in | `webfetch`, `websearch`; `websearch` is dropped when no search integration is configured (`oc:packages/core/src/tool/plugin/websearch.ts:184-191`) | `web_fetch`, `web_search` (`qc:tools/web-fetch.ts:754-770`, `tools/web-search.ts:972-980`) | none (3) |
| **images** | `read` attaches images (`pi:…/tools/read.ts:96`) | `read` returns images and PDFs as file parts (`oc:packages/core/src/tool/plugin/read.ts:175-185`) | `read_file` vision bridge (`qc:tools/read-file.ts:32-44`) | the operator attaches a PNG to the ask; the model cannot request one (`ours:diet/src/drive/attach.rs:4-7`, `diet/src/client/wire.rs:301-306`) **(1)** |
| **todo, subagent, skill** | none built in; todo and subagents are example extensions (`pi:packages/coding-agent/README.md:19`) | `skill`, `subagent`, with `general` and `explore` agents (`oc:packages/core/src/tool/plugin/subagent.ts:17`, `packages/core/src/plugin/agent.ts:87-118`); no todo tool | `todo_write`, `agent`, `skill`, plan mode, `tool_search` (deferred tools, `qc:tools/tool-search.ts:11`) | none; forks are harness-side, not a tool (Session mechanics) (3) |
| **call format** | native function calling | native function calling | native OpenAI function calling (`qc:core/openaiContentGenerator/converter.ts:532`); **plus a text fallback** that parses `<function=NAME><parameter=K>` (qwen3-coder style) and `<invoke name=…>` when a turn has no native call (`qc:core/xml-tool-call-fallback.ts:10-13`, `core/llm-chat.ts:6447-6452`) | native, `{"type":"function",…}` (`ours:diet/src/client/wire.rs:235-248`), marked unverified against a live tool-using server (`diet/src/client/shape.rs:536`); no text fallback **(1)** |
| **tool results** | not checked | not checked | `role: 'tool'`; media moved to a following `user` message (`qc:core/openaiContentGenerator/converter.ts:806-819,940-955`) | `role: tool` (`ours:diet/src/drive/session.rs:2294`) |

## Session mechanics

| | Pi | OpenCode 2 | Qwen Code | diet/exercise |
|---|---|---|---|---|
| **compaction trigger** | auto when tokens > window − 16,384 (`pi:…/core/compaction/compaction.ts:268-269`, `settings-defaults.ts:8-12`); also on overflow; `/compact` (`pi:…/docs/compaction.md:37-41`) | auto before each turn when the estimate reaches window − max(10% of the window, 16,000), with no output term (`oc:packages/core/src/session/compaction.ts:97,211,242-251,911-918`); on overflow (`packages/core/src/session/runner/llm.ts:265-271`, `runner/step.ts:155-160`); manual, `/compact` (`compaction.ts:57`, `packages/core/src/session.ts:439`) | auto at min(0.85 × window, window − 33,000) (`qc:services/chatCompressionService.ts:109-124,210-249`); `/compress`, `/compressFast` | the meditation: operator-declared, or the regimen's `meditation_every_turns` / `meditation_at_context_fraction` (`ours:diet/src/meditation/policy.rs:28,33`); and automatically before every trunk request when the prompt would leave less than Qwen Code's reserve of the window, plus a fork's room with interview forks on, unless `meditation_window = "off"` (#617, #931) **(2)** |
| **what compaction keeps** | an LLM summary of the old part, plus the last ≈20,000 tokens **verbatim** (`pi:…/docs/compaction.md:45-49`) | a structured summary (objective, requirements, decisions, work state, next move, relevant files, context) **plus** the newest ≈15,000 tokens of conversation verbatim, the cut moved back to a user message (`oc:packages/core/src/session/compaction.ts:104-143,197,704-744`), replayed as one user message in `<conversation-checkpoint>` (`packages/core/src/session/runner/to-llm-message.ts:306-325`) | a `<state_snapshot>` summary, an ack, and restored files and images; "**No tail preservation**" (`qc:services/chatCompressionService.ts:1114-1118`, `core/prompts.ts:921-938`) | head + rendered working memory, **no turns**; the render is appended to the system message (`ours:diet/src/drive/session.rs:2638-2672`, `diet/src/meditation/render.rs:106-116`). Its content is typed working memory, not a model-written summary **(2)** |
| **tool-output handling** | cut to 2,000 characters in the summarizer's input only (`pi:…/compaction/utils.ts:94`) | output over 2,000 lines / 50 KB spilled to a file kept 7 days; the model is shown its head (`oc:packages/core/src/tool-output.ts:13-16,78-90`); old outputs are not pruned, only cut to 1,250 characters in the summarizer's input (`packages/core/src/session/compaction.ts:100`) | microcompaction replaces old results with "[Old tool result content cleared]" (`qc:services/microcompaction/microcompact.ts:14,33-45`) | kept verbatim on the trunk until a meditation **(2)** (the tool-output disposition lever) |
| **step limit** | none; the loop is `while (true)` (`pi:packages/agent/src/agent-loop.ts:179-183`) | optional per-agent `steps`; the last step keeps the tool definitions with `toolChoice: "none"` and appends a MAX_STEPS prompt (`oc:packages/core/src/session/runner/llm.ts:226-244`) | 100 turns per send (`qc:core/client.ts:213`) | `[limits] max_steps`; the last step settles `max_steps` with its calls kept (`ours:diet/src/drive/tool_loop.rs:1358-1369`, `session.rs:2265-2279`) (3) |
| **subagents** | example extension only | `general` and `explore`, through the `subagent` tool | `agent` tool; built-in `Explore` (`qc:subagents/builtin-agents.ts:71`) | none for the model; the harness fires at most one interview fork per settled turn, off the warm trunk, never appended (`ours:diet/src/drive/session.rs:2779-2785`) **(2)** |
| **background shells** | none | `shell` with `background: true` returns at once with the shell's id and its output file, and has no timeout by default (`oc:packages/core/src/tool/plugin/shell.ts:54-60,96-101,201,254-257`); the output streams to that file (`packages/core/src/shell.ts:323-335`); the model has no tool to stop it, the client does (`DELETE /api/shell/:id`, `packages/protocol/src/groups/shell.ts:72`); when it settles, a `<shell id=… state=…>` message is steered into the session and wakes it (`shell.ts:157-189`, `packages/core/src/shell/result.ts:45-62`, `packages/core/src/session/session.ts:284-311`) | `run_shell_command` with `is_background` (`qc:tools/shell.ts:5759-5763`); `task_stop` returns at once and the end is reported later (`tools/task-stop.ts:243-252`) | not at the read commit; since #614, Qwen Code's shape: `is_background` and `task_stop` (`ours:diet/src/drive/background.rs:1-11`) (3) |
| **resume** | `--continue`, `--resume`, `--fork`, `/tree` (`pi:…/src/cli/args.ts:308-313`) | `resume`, `interrupt`, steer/queue (`oc:packages/core/src/session.ts:403-404,460-462`) | `/resume`, `/rewind`, `/branch` | the event stream resumes by `Last-Event-ID`; the session does not (`ours:diet/src/drive/serve.rs:7-11`) (3) |
| **cancel** | Esc → `abort()` (`pi:…/core/agent-session.ts:2434-2442`) | `interrupt` | yes | `Cancel(turn)`, `End` (`ours:diet/src/drive/serve.rs:853-858`) (3) |
| **snapshots / undo** | conversation branching; file checkpoints only as an example extension | git snapshot per step in a git project, revert (`oc:packages/core/src/session/runner/step.ts:234-260`, `packages/core/src/snapshot.ts:95-97,114`) | `/restore` to a tool call, with file backups (`qc:services/fileHistoryService.ts`) | none (3) |

## Instructions

| | Pi | OpenCode 2 | Qwen Code | diet/exercise |
|---|---|---|---|---|
| **instruction files** | first of `AGENTS.override.md`, `AGENTS.md`, `CLAUDE.md` per directory; global `~/.pi/agent`, then cwd up to root (`pi:…/core/resource-loader.ts:184-185,241-268`) | `AGENTS.md` only: global, then, when cwd is in the project, cwd up to `$HOME` if cwd is under it, else up to the project root (`oc:packages/core/src/config/plugin/instruction.ts:36-58,76-86`); plus configured `instructions` paths and URLs (`:104-130`) | `QWEN.md`, `AGENTS.md`, global `~/.qwen`, then cwd up to the project root (`qc:utils/memory-constants.ts:7-8`, `memory/memoryDiscovery.ts:111-199`) | none discovered; the system prompt is `--head FILE`, verbatim (`ours:diet/src/bin/drive.rs:293-296,1141-1149`) **(1)** |
| **how injected** | into the system prompt as `<project_instructions path=…>` (`pi:…/core/system-prompt.ts:79-86`) | into the system context, `Instructions from: <path>` (`oc:packages/core/src/instruction-discovery.ts:123-125`) | appended to the system prompt in `--- Context from: … ---` markers (`qc:memory/memoryDiscovery.ts:405`, `core/prompts.ts:884-893`) | n/a |
| **base prompt** | a preamble plus one line and guidelines per tool (`pi:…/core/system-prompt.ts:157-161`); replaceable via `SYSTEM.md` | a default prompt, "You are an AI agent running in OpenCode, a coding agent harness…", with guidance per tool (`oc:packages/core/src/session/runner/prompt/system.txt:1`, `packages/core/src/session/system-prompt.ts:3-7`); per family, `gpt`, `kimi`, `trinity` and `muse` prompts replace it and `claude`'s is appended to it; Qwen gets the default (`packages/core/src/plugin/optimize.ts:16-62,85`) | one shared prompt, "You are Qwen Code, … developed by Alibaba Group" (`qc:core/prompts.ts:106`). **The tool-call example block varies per family:** `/qwen[^-]*-coder/` gets XML `<tool_call><function=…><parameter=…>`, `/qwen[^-]*-vl/` gets JSON in `<tool_call>`, everything else `[tool_call: …]` (`qc:core/prompts.ts:1369-1409,1128-1150,1260-1267,1010-1015`) | the operator's head file **(1)** |

## Model and provider setup

| | Pi | OpenCode 2 | Qwen Code | diet/exercise |
|---|---|---|---|---|
| **providers** | 42 built in, llama.cpp via an extension (`pi:packages/ai/src/providers/all.ts:138-179`) | the models.dev catalog; any provider package, or an AI SDK package, loads (`oc:packages/core/src/model-resolver.ts:196-260`) | OpenAI-compatible, DashScope, Qwen OAuth | llama.cpp and TabbyAPI dialects (`ours:diet/src/client/shape.rs:442-483`); no https, so no hosted endpoint (`diet/src/client/transport.rs:32-36`) (3) |
| **reasoning** | one level per session, `off`…`max` (`pi:…/src/cli/args.ts:302`); Qwen over llama.cpp: `chat_template_kwargs.enable_thinking` + `preserve_thinking` (`pi:packages/ai/src/api/openai-completions.ts:893-905`, `…/extensions/llama/provider.ts:149-156`) | variants `provider/model#variant` merge settings, headers and body (`migrate:163`, `oc:packages/core/src/model-resolver.ts:137-161`); Qwen's come from the catalog's reasoning support: on Alibaba, `low`/`medium`/`xhigh` with `enableThinking: true` (`packages/core/src/variant.ts:275-299`), on NVIDIA an `enable_thinking` toggle (`:181-186`); `enable_thinking` is sent only from a selected variant (`packages/ai/src/protocols/alibaba-chat.ts:73`) | per provider profile: `enable_thinking`, `chat_template_kwargs.enable_thinking`, or `reasoning_effort` (`qc:core/openaiContentGenerator/pipeline.ts:101-127`) | `chat_template_kwargs.enable_thinking` and `reasoning_effort` from the regimen, on every request, forks included; `budget_tokens` is recorded but not sent (`ours:diet/src/drive/regimen.rs:220-260`) **(2)** |
| **request body** | `max_tokens` clamped to the remaining context, temperature only if set, `prompt_cache_key` (`pi:packages/ai/src/api/openai-completions.ts:822-853`) | `request.body` passthrough (`oc:packages/core/src/model-resolver.ts:225-231`); `max_tokens` is the catalog's output limit fitted to the room the prompt leaves in the window (`packages/core/src/session/model-request.ts:88-98,259-262`) | temperature, top_p, max_tokens, top_k, penalties, `extra_body` (`qc:core/openaiContentGenerator/pipeline.ts:1568-1625`) | sampler pinned from the regimen's card, `max_tokens` 8192, `grammar`, `reasoning_content` echoed back (`ours:diet/src/client/wire.rs:151-167,314-316`) (3) |
| **prompt caching** | `cacheRetention` short/long; Anthropic `cache_control` with 1 h when long (`pi:packages/ai/src/api/anthropic-messages.ts:69-93`) | auto breakpoints on the tools, the first and last system parts and the final message, at most 4; 5 min or 1 h (`oc:packages/ai/src/cache-policy.ts:17-35`, `packages/ai/src/protocols/anthropic-messages.ts:520-531`); behind a gateway, Qwen gets no tool breakpoint (`cache-policy.ts:58-67`) | DashScope `cache_control` on the system message, last tool and latest history (`qc:provider/dashscope.ts:295,343-347`) | none sent; misses are measured and classified (`ours:diet/src/client/cache.rs:1-25`) **(2)** (the cache-lifetime lever, parked pending a hosted arm) |

## Extensibility and permissions

| | Pi | OpenCode 2 | Qwen Code | diet/exercise |
|---|---|---|---|---|
| **MCP, skills, commands, plugins, hooks** | all, as TypeScript extensions with `pi.on()` hooks; MCP exposed through `codemode` (`pi:packages/coding-agent/docs/mcp.md:189-200`, `docs/extensions.md:95-105`) | MCP tools advertised (`oc:packages/core/src/tool/mcp.ts:40-43`), skills, commands, agents, plugins, and session hooks (`packages/core/src/session/model-request.ts:305-309`, `packages/core/src/plugin/optimize.ts:88-90`) | MCP, extensions (Claude, Gemini and Qoder converters), commands, skills, subagents, hooks (`qc:hooks/types.ts:24-44`) | none (3) |
| **permissions** (out of scope, going away, #540) | none per call (`pi:packages/coding-agent/docs/security.md:3`) | allow/ask/deny rules, last match wins (`migrate:96-125`) | `plan`, `default`, `auto-edit`, `auto`, `yolo` (`qc:config/approval-mode.ts:7-13`) | denylist + operator prompt (`ours:diet/src/drive/shell_gate.rs:1-12`); #540 removes it (3) |

## How each harness talks to Qwen3.8

### The template r2 serves, against the Unsloth GGUF template

- **What r2 serves:** the model directory's template, sha256 `c3cf9e34…`.
  - It is the template recorded for r2 (`ours:substrates/admission/accel24-tabbyapi-exl3-qwen38-27b-3p00/6a96d5696231/fingerprint.json:3`, `raw/kw.json:2`).
  - It is also the one the llama.cpp candidate serves from its GGUF header (`ours:substrates/registry.toml:645-646`).
  - A copy captured in the #393 window hashes to the same digest; line numbers below are that file's, cited as `served:`.
- **What it is compared with:** Unsloth's GGUF template, sha256 `12827f24…` (`ql:templates/qwen3.8-27b-gguf-chat-template.jinja`). That digest is also the one recorded for the Flash-Next GGUF (`ours:results/2026-09-29-false-nomination-edit-rate-second-substrate/window/kwarg-start.json:2`).

| | served (`c3cf9e34`) | Unsloth GGUF (`12827f24`) |
|---|---|---|
| **effort default** | `reasoning_effort` defaults to `xhigh`; only `xhigh`, `medium` and `low` are accepted, and anything else raises (`served:47-50`). `xhigh` injects "…think carefully through the task…" and `low` injects "Keep your thinking brief…" into the system turn; `medium` injects nothing (`served:51-55`) | the same, except `high` is silently mapped to `xhigh` (`ql:templates/…gguf-chat-template.jinja:59-62`; `ql:README.md:63-64`) |
| **thinking switch** | `enable_thinking` undefined or true means think; false prefills an empty `<think>` block (`served:46,165-166`) | same |
| **tool-call format** | tools are declared in the system turn inside `<tools>` as JSON (`served:62-66`). Calls are **XML**, `<tool_call><function=NAME><parameter=K>…</parameter></function></tool_call>`, with a reminder block (`served:68,128-143`). Results come back as a `user` turn wrapping `<tool_response>` (`served:147-153`) | same format. It also refuses a call with no name, and arguments passed as a JSON string rather than an object (`ql:…gguf-chat-template.jinja:129-131,141-153`) |
| **system / developer** | one `system` message, which must be first (`served:106`); no `developer` role | merges every leading `system` **or `developer`** message into one (`ql:…gguf-chat-template.jinja:44-56,109`). Its closing comment: "Unsloth fixes - developer role, merged system messages, tool calling" (`:184`) |
| **preserve_thinking** | undefined or true keeps reasoning in **all** earlier assistant turns; false keeps it only after the last user query (`served:116`) | same (`ql:…gguf-chat-template.jinja:119`) |

### What each harness sends to a Qwen3.8 server by default

- **Pi.**
  - Its llama.cpp provider marks a model as reasoning when the served template contains `enable_thinking` (`pi:packages/coding-agent/src/extensions/llama/provider.ts:134`), which this template does. It then uses `thinkingFormat: "qwen-chat-template"` (`:156`).
  - That format sends `chat_template_kwargs: {enable_thinking: <a thinking level is set>, preserve_thinking: true}` and **never `reasoning_effort`** (`pi:packages/ai/src/api/openai-completions.ts:901-905`; `supportsReasoningEffort: false`, `provider.ts:152`). So the template's `xhigh` default applies whenever thinking is on.
  - Tools go out as native function calls; the template renders them as XML.
  - The maintainer's notes, written 2026-08-17 against an earlier Pi, say the built-in provider registered models as `reasoning: false` and forwarded no thinking parameters (pi #5917) (`ql:README.md:15-19`). The code at `42a3497d` above detects reasoning from the template instead.
- **OpenCode 2.**
  - Thinking kwargs come only from a selected variant: on Alibaba an effort with `enable_thinking`, on NVIDIA an `enable_thinking` toggle (`oc:packages/core/src/variant.ts:181-186,275-299`, `packages/ai/src/protocols/alibaba-chat.ts:73`). Which variants a local OpenAI-compatible server gets was not checked.
  - With no variant the body is the model's `request.body` passed through (`oc:packages/core/src/model-resolver.ts:225-231`), so the template's defaults (thinking on, `xhigh`, preserve) apply. Native function calls.
- **Qwen Code.**
  - Thinking is set per provider profile: `enable_thinking`, `chat_template_kwargs.enable_thinking`, or `reasoning_effort` (`qc:core/openaiContentGenerator/pipeline.ts:101-127`). Which profile a local OpenAI-compatible server gets was not checked.
  - It sends native function calls (`qc:core/openaiContentGenerator/converter.ts:532`). Its prompt's tool-call example is chosen by model name (`qc:core/prompts.ts:1393-1409`), and a model id such as `qwen3.8-27b` matches neither `/qwen[^-]*-coder/` nor `/qwen[^-]*-vl/`. So it would get the generic `[tool_call: …]` example (`:1010-1015`), not the XML shape the template itself declares. This is read from the regular expressions, not run.
- **diet (#531, merged in the read commit).** `template_kwargs` sends the regime's state on every request (`ours:diet/src/drive/regimen.rs:228-260`):
  - `enable_thinking` is false under `off` and true under `on`/`suppressed`, and **nothing under `undeclared`** (`:235`).
  - `reasoning_effort` is sent only when `[reasoning]` declares a level (`:249`).
  - It never sends `preserve_thinking` (no hit in `diet/src` outside the operating-points format).
  - So an undeclared regimen runs under the template's defaults: thinking on, `xhigh`, reasoning preserved in every turn.
  - Native function calls, rendered as XML by the template.

## Which tool set Qwen-family models were trained with

**Not stated** in any of the four sources.
- **Pi and OpenCode** say nothing about Qwen's training. They only route Qwen's thinking flags, and OpenCode gives Qwen `edit` + `write` and its default prompt.
- **Qwen Code** is the nearest evidence, but it is evidence of a harness, not of training:
  - It prompts qwen-coder models with an XML tool-call example, `<tool_call><function=…><parameter=…>` (`qc:core/prompts.ts:1128-1150`).
  - It recovers that shape when a model writes it as text (`qc:core/xml-tool-call-fallback.ts:10-13`, added in `912f73998a`, 2026-08-01).
  - Its edit tool takes `old_string`/`new_string`.
- **Our own serving stack** agrees on the format. TabbyAPI, serving the 3.8 27B, logs "tool format qwen3_coder" (`ours:substrates/measurements/2026-10-04-qwen38-27b-dflash2-exl3/wf/logs/tabby.log:20-21`). That records which parser the server picked, not the training.
- **At HEAD, Qwen Code is not that harness.** At the read commit it is far from the Gemini-CLI-shaped harness of mid-2025. If "trained in" matters, the comparison is against an early tag (for example `a9d6965bef`, or v0.1), read and cited, not inferred.

## The vote at the re-pin (#931)

The maintainer ruled on 2026-10-20 on the conventions the OpenCode 2 re-pin (#925) moved. Each was voted with Pi at `v1.3.0` (`71023aec`), OpenCode 2 at `v2.0.32` and Qwen Code at `v0.25.3` (`b28b0d94`), read only at these cells. The rule: OpenCode 2 and Pi agreeing is the default, and Qwen Code breaks a tie.

| convention | Pi `v1.3.0` | OpenCode 2 `v2.0.32` | Qwen Code `v0.25.3` | ours since #931 |
|---|---|---|---|---|
| **a call's timeout** | no default, no bound (`pi:packages/coding-agent/src/core/tools/bash.ts:22-43`) | no bound, 0 for none, default 120,000 (`oc:packages/core/src/tool/plugin/shell.ts:24,54-56,201`) | at most 600,000, default 120,000 (`qc:packages/core/src/tools/shell.ts:1164,5767,5821-5822`) | no bound (Pi and OpenCode 2 agree); 0 for none; default 120,000 (`ours:diet/src/drive/tool_loop.rs`) |
| **compaction trigger** | window − 16,384 (`pi:packages/coding-agent/src/core/settings-defaults.ts:9-13`) | window − max(10% of window, 16,000) (`oc:packages/core/src/session/compaction.ts:97,911-918`) | above min(0.85 × window, window − 33,000) (`qc:packages/core/src/services/chatCompressionService.ts:210-232`) | Qwen Code's, plus a fork's room with interview forks on (`ours:diet/src/meditation/policy.rs`, `diet/src/drive/session.rs` `reserve_of`) |
| **what a mid-turn compaction keeps** | about 20,000 tokens (`pi:packages/coding-agent/src/core/settings-defaults.ts:12`) | a flat 15,000 (`oc:packages/core/src/session/compaction.ts:197,704-744`) | a pending call and its result (`qc:packages/core/src/services/chatCompressionService.ts:570-574,627-648`) | Qwen Code's: the last step alone (`ours:diet/src/drive/session.rs` `split_turn`) |
| **shell output in memory** | a rolling tail (`pi:packages/coding-agent/src/core/tools/output-accumulator.ts:38,60,239-256`) | none; all of it to a file (`oc:packages/core/src/shell.ts:323-335`) | head and tail, the whole to a file (`qc:packages/core/src/tools/truncation.ts:113-114,196-223`) | head and tail (`ours:diet/src/isolation/mod.rs`), unchanged |
| **default `max_tokens`** | 16,384, clamped to the window (`pi:packages/coding-agent/src/core/provider-composer.ts:243`) | the catalog's limit, clamped (`oc:packages/core/src/session/model-request.ts:88-98,259-262`) | 64,000 for `qwen3.x`, clamped (`qc:packages/core/src/core/tokenLimits.ts:34,335-337,453-459`) | 64,000, clamped (`ours:diet/src/drive/regimen.rs`), unchanged |

## The compaction prompt (#936)

What each harness asks for when it compacts, and what it carries beside the summary. It's read at Pi `v1.3.0`, OpenCode 2 `v2.0.32` and Qwen Code `v0.25.3`. This is the vote `meditation_carry = "native_summary"` follows: they differ throughout, so Qwen Code decides.

| | Pi `v1.3.0` | OpenCode 2 `v2.0.32` | Qwen Code `v0.25.3` | ours under `native_summary` |
|---|---|---|---|---|
| **the summariser's prompt** | its own system prompt (`pi:packages/coding-agent/src/core/compaction/utils.ts:161-163`) and a 7-section markdown instruction (`compaction.ts:507-538`) | the session's own system and tools, with a 7-section template and rules appended (`oc:packages/core/src/session/compaction.ts:104-176`) | the compression prompt as the system message: an `<analysis>` scratchpad, then a 9-section `<state_snapshot>` (`qc:packages/core/src/core/prompts.ts:925-971`) | Qwen Code's, verbatim (`ours:diet/dogma/compaction/qwen-code-v0.25.3.txt`) |
| **how the history is shown** | serialised into one `<conversation>` text (`compaction.ts:682-693,723-733`) | as messages, the instruction a final user message (`compaction.ts:253-294`) | as messages, then "First, reason in your <analysis> block. Then, produce the <state_snapshot> XML." (`qc:packages/core/src/services/chatCompressionService.ts:100-101`) | Qwen Code's |
| **output cap** | 13,107 by default (`compaction.ts:712-715`) | at most 32,000, fitted to the window (`oc:packages/core/src/session/model-request.ts:87-98`) | at most 20,000, fitted to the window less 1,024 (`chatCompressionService.ts:58,69,91-98`) | Qwen Code's |
| **tools, reasoning** | none; the session's thinking level (`compaction.ts:595-610`) | the session's tools; reasoning not established | none, thinking off on its cold path (`chatCompressionService.ts:838-880`) | Qwen Code's |
| **beside the summary** | ~20,000 recent tokens, verbatim (`compaction.ts:129`) | the newest 15,000 tokens, stringified into the checkpoint (`compaction.ts:195-197`) | no tail; the model's acknowledgement; up to 5 files and 3 images restored; a plan-mode reminder and running subagents; a trailing call (`qc:packages/core/src/services/postCompactAttachments.ts:800-882`) | Qwen Code's, without the reminder and the subagents, which this harness has neither of |
| **the wrapper** | a user message, `<summary>` tags with a lead line (`pi:packages/coding-agent/src/core/messages.ts:11-17`) | a user message, `<conversation-checkpoint>` with `<summary>` (`oc:packages/core/src/session/runner/to-llm-message.ts:312-324`) | a user message with a "Resume the prior task…" trailer (`postCompactAttachments.ts:514-515,572`) | ours (#597): Pi and OpenCode 2 agree on a user message in `<summary>` tags |

## For the maintainer: approve or reject

### Label (1): changes what the model sees

1. **Edit tool.** We declare only `bash`. All three others give an exact-replacement edit (`old_string`/`new_string`, or Pi's `edits[]`) plus a whole-file write. A Qwen model in our harness writes files through shell quoting.
2. **Search and read tools.** Pi, OpenCode and Qwen Code each declare read, grep and glob tools; we fold them into `bash`.
3. **The `bash` tool's description.** Ours has none at function level and 26 characters on its property. Pi's `bash` description is about 308 characters and Qwen Code's shell takes five parameters.
4. **Instruction files.** None of AGENTS.md / QWEN.md is discovered. The others inject them into the system prompt, so a trajectory whose repository carries an AGENTS.md is seen differently by us.
5. **Tool-call example in the prompt.** Qwen Code shows qwen-coder models an XML `<tool_call>` example. Our head has none.
6. **Text tool-call fallback.** Qwen Code parses XML tool calls a model writes as text. We would read such a turn as a plain answer.
7. **The effort line in the system turn.** When no effort is declared, the served template injects the `xhigh` "think carefully" line into the system turn (`served:47-55`). Pi and diet (undeclared) both get it by default.
8. **Model-requested images.** In the others, the model can read an image file; in ours, only the operator attaches one.

### Label (2): levers we have or should add

9. **Compaction depth.** Pi keeps about 20,000 tokens verbatim, OpenCode keeps about 15,000 verbatim beside its summary, and Qwen Code keeps no tail. Ours keeps no turns. Three working references for the 0-1-n sweep.
10. **Meditation trigger.** The others fire at about 0.85 of the window or within 16-33K tokens of the edge. Ours is operator-declared, plus cadence and budget since #520.
11. **Tool-output disposition.** OpenCode spills to a file, Qwen Code clears old results (microcompaction), and Pi only trims them for its summarizer. Ours keeps everything until a meditation.
12. **Reasoning budget.** All four send an on/off switch, and Pi, Qwen Code and we also send effort. Pi sends a token budget for Qwen providers (`thinking_budget`, `pi:packages/ai/src/providers/compat-schema.ts:21`); ours records `budget_tokens` but does not send it.
13. **Prompt caching.** All three others place cache breakpoints for hosted APIs. Ours sends none; the lever is parked until a hosted arm exists.
14. **Subagents.** Qwen Code exposes an `agent` tool. Our interview fork is harness-fired and never shown to the trunk.
