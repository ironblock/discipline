# `diet`

`diet` is a Rust project which represents the dogma and the core mechanical aspects of the overall program. 

It's primarily a library, but exists primarily to apply a `regimen` - a fixed combination of variables expected to describe the character, performance, and timing of a session (live or replayed).

**A regimen** is a TOML file naming the arm, the dogma version, the substrate (a served model on a registered machine, `substrates/README.md`) and the sampler a session runs under, and what else it is held to (`diet check-regimen` reads one). Three ship as specimens:

- [`drive/floor.toml`](drive/floor.toml) binds a session to the maintainer's registered floor substrate. It starts only against that substrate's registered engine.
- [`drive/dev-loop.toml`](drive/dev-loop.toml) names the canned substrate: a scripted server inside this crate, with no weights. It is served only by that canned server, which the batch drive starts for itself.
- [`drive/replay.toml`](drive/replay.toml) is a rehearsal: commands, approvals and a record, with no model. Its substrate, `canned-replay-tools`, is served by `diet-drive replay`, which answers each ask with a captured llama.cpp `bash` call and then that session's answer, so the shell gate and its approval prompt run.

**Tool output is capped as it arrives** (#554): what the model is shown of a command's output stops at 2000 lines or 50 KiB, the convention of the harnesses it is compared with. Past the cap it sees the head and the tail with a notice, and the whole output is kept in the recording's `files/` by digest, for it to read in slices. A regimen's `[tool_output]` sets `max_lines` and `max_bytes`, or `cap = false` to keep every output whole. `session.start` and the record's start name the state the session ran under.

**A seam can keep a recent tail** (#552): a regimen's `seam_tail_tokens = N` keeps the old trunk's most recent whole turns, within N estimated tokens (characters ÷ 4), after the refill, as they sat on the trunk. A tail starts only at a user message, and the turn that would cross N is not kept. 0, the default, is the total refill. The seam line and the record's seam row name the depth and what was kept.

**A seam fires before the window fills** (#617): before every trunk request, the turn's own steps included, a served session that keeps working memory and knows its window refills the trunk when the prompt as sized (#588) would leave less than max(20,000, the output cap) tokens of the window: OpenCode 2's reserve, where Pi and OpenCode both compact automatically and Qwen Code's larger reserve breaks the tie on the number. Under a turn, the turn's ask and steps so far ride after the refill. With nothing in working memory, or no turn the refill would drop, the request goes as it is, as all three harnesses send it. A request that overflows anyway (#616) is seamed once more and sent again, once per turn, as each of the three compacts and retries once after an overflow. A regimen turns it off with `seam_window = "off"`. The seam line and the record's seam row carry reason `window`, the prompt that fired it and the window.

**A seam can carry the tool outputs it compacts away** (#553): a regimen's `seam_tool_outputs` says what the refill carries of the outputs in the turns before the kept tail, in a section after the rendered working memory, one entry per call in call order. `evict`, the default, carries nothing. `keep` carries each output as the trunk had it, after the cap. `reference` carries a line naming the tool, its arguments, and the output's size and sha256, with the whole saved by digest in the recording's `files/`. `salient` carries the verbatim excerpts the read fork quoted from each output. The kept tail's outputs are untouched. The seam line names the state, logs the section as sent with how many outputs and bytes it carried, and the start row names the state beside the cap.

**The tool surface** (#557): a regimen's `tool_surface = "standard"` offers `read`, `write`, `edit`, `grep` and `glob` beside `bash`, shaped as the compared harnesses shape them. `bash` alone is the default. Every tool runs through the session's confinement, as `bash` does, and no approval gate decides them; each call is logged under its tool's name.

**The interview cadence** (#564): a regimen's `interview_cadence` says when the capture gap's forks fire, and on what. `gap`, the default, fires at most one: the judgment ask on an operator-marked turn, else the class ask on the turn's last read. `per_class` forks on every call the router routes to a class ask. `per_call` forks on every call that ran, with the generic ask for a call the router would not interrupt. `turn_boundary` asks the judgment question at every turn's end. Every state queues its forks into the gap in call order, one after another. `interview_threshold_bytes = N`, unset by default, forks a read only when its output is at least N bytes. Each fork line names its `trigger`, and the start row names the cadence.

**Two binaries**, built from the repository root with `cargo build -p discipline-diet`:

- `diet` is the command-line reader of every format (`diet check-log`, `diet check-record`, ...). Anything outside this library that reads a format goes through it.
- `diet-drive` drives a session. `diet-drive <regimen> <worktree> <output.jsonl>` runs a pinned three-turn script as a batch. `diet-drive serve` serves one interactive session over HTTP, which is what a harness talks to. `diet-drive --help` and `diet-drive serve --help` print each form's usage.

**To drive a session, start with [`drive/BEGIN.md`](drive/BEGIN.md).**
