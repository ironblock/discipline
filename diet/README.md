# `diet`

`diet` is a Rust project which represents the dogma and the core mechanical aspects of the overall program. 

It's primarily a library, but exists primarily to apply a `regimen` - a fixed combination of variables expected to describe the character, performance, and timing of a session (live or replayed).

**A regimen** is a TOML file naming the arm, the dogma version, the substrate (a served model on a registered machine, `substrates/README.md`) and the sampler a session runs under, and what else it is held to (`diet check-regimen` reads one). Three ship as specimens:

- [`drive/floor.toml`](drive/floor.toml) binds a session to the maintainer's registered floor substrate. It starts only against that substrate's registered engine.
- [`drive/dev-loop.toml`](drive/dev-loop.toml) names the canned substrate: a scripted server inside this crate, with no weights. It is served only by that canned server, which the batch drive starts for itself.
- [`drive/replay.toml`](drive/replay.toml) is a rehearsal: commands, approvals and a record, with no model. Its substrate, `canned-replay-tools`, is served by `diet-drive replay`, which answers each ask with a captured llama.cpp `bash` call and then that session's answer, so the shell gate and its approval prompt run.

**Two binaries**, built from the repository root with `cargo build -p discipline-diet`:

- `diet` is the command-line reader of every format (`diet check-log`, `diet check-record`, ...). Anything outside this library that reads a format goes through it.
- `diet-drive` drives a session. `diet-drive <regimen> <worktree> <output.jsonl>` runs a pinned three-turn script as a batch. `diet-drive serve` serves one interactive session over HTTP, which is what a harness talks to. `diet-drive --help` and `diet-drive serve --help` print each form's usage.

**To drive a session, start with [`drive/BEGIN.md`](drive/BEGIN.md).**
