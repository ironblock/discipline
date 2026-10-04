# #320 spike: evidence for planning's redesign question, never merged

Branch `spike/320-checker-tests`, off develop 8518de9. Planning's question is at #25 5966627608. The deliverable is the comment on #25. This directory is that comment's evidence, and the branch stays in place for planning to read.

## Report 1: the `ci` faults as tests of their checker

The issue counted 56 `ci` faults. Develop 8518de9 has 61.

**What was built.**
- `scripts/check-ci-coverage.py` gains `SPIKE_320_SKIP_RUNS`, a spike-only switch. It skips rule 13's dry and real shard runs: 20 `verify.sh` and script subprocesses, the census of what each shard ran.
- `spike-320/ci_successors.py` proves each `ci` fault red against the checker. For each fault it copies the tracked tree, runs the fault's own `inject_*` function sourced from `verify.sh`, then runs the checker twice: with the switch, and in full. A fault counts as red only when it exits non-zero with its own signature.

**Measured** in a Linux container (python 3.12, bash 5, GNU `cp`, ruby), rows in `ci-successors.jsonl`:

| | value |
|---|---|
| `ci` faults | 61, every injection applied, exit 0 |
| red for its own reason, rule 13's runs skipped | 51 |
| red only in full | 10: the shard and census faults, listed in the rows |
| red in neither | 0 |
| checker seconds per fault, median: switch on / full | 0.37 s / 16.55 s |
| tree copy per fault, median | 2.94 s |
| the checker alone, clean tree, container: switch on / full | 0.32 s / 14.1 s |

**Not built, so not measured:**
- No test module: the successor is the checker run against an injected tree, not a fixture inside a `unittest`.
- No `successor` field in `faults.toml`.
- The 2.94 s copy is still paid per fault. A rule-level fixture would avoid it, and nothing here measures that.

## Report 2: `cargo-mutants` on the drive lane

drive is the lane with the most faults: 214 in `diet/drive/gate.toml`, 212 of them in Rust.

**The setup.**
- cargo-mutants 27.1.0, installed with `cargo install --locked`.
- Run over `diet/src/drive/*.rs` and `diet/src/bin/drive.rs`, with the lane's own test filter (`-- drive --skip every_seeded_fault_still_names_source_that_is_there`).
- `-j 3`, a 300 s timeout, on the Mac Pro (Xeon W-3235, 24 threads, shared with other seats' runs).
- The Rust tree at develop 8518de9: the spike changes no Rust.

| run | mutants | caught | missed (survived) | unviable | wall clock |
|---|---|---|---|---|---|
| the whole lane | 534 | 344 | 55 | 135 | 48 m (2,875 s) |
| `--in-diff` PR #315's diff (`gh pr diff 315`), applied to today's tree | 45 | 12 | 10 | 23 | 4 m 15 s (255 s) |

The 55 survivors are the class a hand-written list cannot show: a mutation the lane's tests do not catch. They are listed in `drive-mutants-missed.txt`. The caught and unviable lists, and the in-diff survivors, sit beside it. `--in-diff` ran against #315's diff after later commits had moved some lines, so it scopes by today's line numbers, not #315's.

**Do the hand faults have a generated mutant?** Each of the 212 Rust hand faults was matched by its anchor's line span against every generated mutant's span (`drive-hand-fault-coverage.json`):

| on the hand fault's lines | faults |
|---|---|
| a generated mutant that was caught | 146 |
| only mutants that survived or were unviable | 65 |
| no generated mutant | 1 (`drive.serve-bin-cap-cuts-reasoning`) |

"On its lines" means a mutant on the same lines, not the same mutation. Whether a caught mutant there stands in for the hand fault's own catcher is not measured.

## Report 3: the layers, from the record

See `LAYERS.md`. Each layer has rows with comment ids. The record shows no drift issue ever opened, and no census refusal on a real CI run.

No recommendation: planning decides.
