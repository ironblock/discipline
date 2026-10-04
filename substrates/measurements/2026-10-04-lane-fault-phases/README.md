# Where a seeded fault's selftest seconds go: lane and `test` faults, by phase (#353)

**The finding.** A lane fault is proven by the `lanes` check, which is `apply-lane-faults.py --verify`. That runs every registered lane's `cargo test`, not only the faulted lane's, and most of a lane fault's seconds go to those cargo invocations after their build: 74% of the median on a CI runner, 65% on the Mac Pro.

**The smallest change that roughly halves a lane fault** is a per-lane scope: inside a lane fault's sandbox, the `lanes` check runs only the lanes its catchers live in. Weighted by each lane's fault count, that removes about 13.9 s of a 28.5 s mean lane fault on CI (49%), and about 5,660 s across the 407 lane faults (`lanes.py`). On the Mac Pro it removes 20.2 s of 44.8 s (45%). This is estimated from the per-lane medians below, not measured as a change. It is filed as #360.

The sample's unweighted median puts the other five lanes higher, at 67% (19.0 s of 28.3 s). That is two faults per lane, so it over-weights the lanes whose tests are cheap relative to drive's, and drive holds 214 of the 407 faults.

A `test` fault is different. Its tests run in 0.1 s on CI, and its time goes to the build (61%) and the sandbox copy (31%).

## Method

`phases.py` replays what `verify.sh`'s `seeded_case` does for one fault, in its order and with its commands, and times each step:

1. **The sandbox copy:** `git ls-files` → `cp -L --parents`, `git init`, `git add --all`.
2. **The two `sandbox_state()` fingerprints** and the `diff-tree` between them.
3. **The injection:** `apply-lane-faults.py --apply-only` for a lane fault, or for a hand-written case its `inject_*` function, sourced from `verify.sh`.
4. **The check:** `hermetic.sh env CARGO_TARGET_DIR=… bash ./verify.sh --only CHECK [--scope S]` in the box.
5. **The verdict:** every catcher's signature matched against the log.

Inside step 4, three wrappers log what cargo did without changing the tree the fault is graded on:
- a `cargo` shim first on `PATH`, which `hermetic.sh` admits, records each invocation's wall clock and cargo's own `Finished … in Xs`;
- a `build.rustc-wrapper` times each rustc;
- a linker wrapper times each link.

The wrappers reach the build through a `.cargo/config.toml` written into the box after the second fingerprint. `analyze.py` turns the rows into the tables below, and its docstring defines each phase. "After build" is each cargo invocation's time after its build. That is mostly the test binaries, but it also includes the library's doctests, cargo's start-up and the shim's own stamps, all of which go away with the invocation. `lanes.py` gives each lane's own after-build time, and the weighted saving.

**The sample** is chosen by `sample.py`, by rule, not by hand:
- for each of the six lanes, its first and middle fault by sorted id: 12 lane faults;
- twelve `test` cases, spaced evenly over their sorted ids;
- a warm-up first, which primes the target and is dropped from every table.

Every row in both files went red for its own reason (`verdict_ok`).

**Where the replay is not exact.** None of these touches a sampled row's verdict, and the harness is kept as it ran:
- `phases.py`'s copy skips `sandbox()`'s per-file `[ -f ]` loop over the tracked paths, so its copy phase is a few tens of milliseconds short.
- It hands `hermetic.sh` a smaller environment than `hermetic.sh` admits: no `TERM`, `LC_ALL`, `LC_CTYPE`, `RUSTUP_TOOLCHAIN` or proxy variables.
- Its verdict matches each signature over the whole log with Python's `re.search`, where `log_carries_every` uses `grep -E` line by line. The two differ only on `^`/`$`. Three of the hand-written signatures carry an anchor, and none of them is in the sample.

## Receipts

| host | where | when (UTC) | tree | rows |
|---|---|---|---|---|
| a GitHub `ubuntu-latest` runner | workflow `measure-353` on the scratch branch `measure/353`, run 37168505268. One job, set up as `gate-selftest.yml`'s shards are: bubblewrap, the user-namespace relaxation, `DIET_REQUIRE_SANDBOX=1`, and its Cargo cache restored with no save | 2026-10-04 01:40–01:48 | 65dac30: develop 8518de9 plus the harness | `ci-phases.jsonl` |
| the Mac Pro (Xeon W-3235, 24 threads) | `phases.py` run directly, with GNU `cp` | 2026-10-04 01:55–02:08 | the same tree | `macpro-phases.jsonl` |

The scratch branch existed only to run that job. It was never a pull request and was deleted after the run. Its `phases.py` and `sample.py` are byte-identical to these.

**The Mac Pro's rows were taken under shared load**, recorded rather than retaken, as Dispatch ruled:
- the load averages were 8.00 at the start, and 9.72 at the end with 11.20 over five minutes, on 24 threads, while another seat's verify ran;
- it has no bubblewrap, so the isolation lane's tests take their refusal path, and its isolation row is not comparable with CI's;
- its copy phase (6.9 s against CI's 2.0 s) and its `test` faults' after-build time (2.0 s against 0.1 s) are higher too. The cause, the host or the load, was not isolated.

## CI runner

**lane faults, n = 12** (all verdicts read: True)

| phase | median s | p90 s | median share of the total |
|---|---:|---:|---:|
| copy | 2.0 | 2.7 | 7% |
| fingerprint | 0.1 | 0.1 | 0% |
| inject | 0.1 | 0.1 | 0% |
| build | 4.7 | 6.2 | 17% |
| link (within build) | 2.5 | 2.6 | 9% |
| after build (test binaries) | 20.8 | 21.3 | 74% |
| other | 0.2 | 0.2 | 1% |
| verdict | 0.0 | 0.0 | 0% |
| total | 28.3 | 30.8 |  |
| other lanes (removable) | 19.0 | 20.8 | 67% |
| own lane's after build | 0.8 | 4.8 | 3% |

per fault, the other lanes' share of its total: median 69%, min 24%, max 77%

**test faults, n = 12** (all verdicts read: True)

| phase | median s | p90 s | median share of the total |
|---|---:|---:|---:|
| copy | 2.0 | 2.3 | 31% |
| fingerprint | 0.1 | 0.1 | 1% |
| inject | 0.0 | 0.0 | 1% |
| build | 4.0 | 5.6 | 61% |
| link (within build) | 0.3 | 0.8 | 5% |
| after build (test binaries) | 0.1 | 0.3 | 2% |
| other | 0.1 | 0.1 | 1% |
| verdict | 0.0 | 0.0 | 0% |
| total | 6.6 | 8.7 |  |

| lane | faults | its test run, median s (n) |
|---|---:|---:|
| drive | 214 | 11.9 (12) |
| adapters | 37 | 4.8 (12) |
| client | 98 | 2.2 (12) |
| isolation | 21 | 1.1 (12) |
| digest | 8 | 0.6 (12) |
| seam | 29 | 0.6 (12) |

all six lanes' test runs, sum of medians: 21.3 s; a per-lane scope would remove ~5663 s over the 407 lane faults, 13.9 s per fault (estimated from these medians)
lane-weighted mean lane fault: 28.5 s; the per-lane scope's saving is 49% of it

## Mac Pro

**lane faults, n = 12** (all verdicts read: True)

| phase | median s | p90 s | median share of the total |
|---|---:|---:|---:|
| copy | 6.9 | 7.6 | 16% |
| fingerprint | 0.5 | 0.7 | 1% |
| inject | 0.1 | 0.1 | 0% |
| build | 7.5 | 10.4 | 18% |
| link (within build) | 3.7 | 4.0 | 9% |
| after build (test binaries) | 27.7 | 31.5 | 65% |
| other | 0.6 | 0.7 | 1% |
| verdict | 0.0 | 0.0 | 0% |
| total | 42.5 | 51.5 |  |
| other lanes (removable) | 25.6 | 30.7 | 60% |
| own lane's after build | 0.8 | 7.8 | 2% |

per fault, the other lanes' share of its total: median 60%, min 30%, max 67%

**test faults, n = 12** (all verdicts read: True)

| phase | median s | p90 s | median share of the total |
|---|---:|---:|---:|
| copy | 6.8 | 7.5 | 42% |
| fingerprint | 0.5 | 0.6 | 3% |
| inject | 0.1 | 0.1 | 1% |
| build | 6.4 | 10.5 | 39% |
| link (within build) | 0.4 | 0.9 | 3% |
| after build (test binaries) | 2.0 | 2.6 | 12% |
| other | 0.2 | 0.2 | 1% |
| verdict | 0.0 | 0.0 | 0% |
| total | 16.3 | 20.7 |  |

| lane | faults | its test run, median s (n) |
|---|---:|---:|
| drive | 214 | 13.2 (12) |
| adapters | 37 | 7.8 (12) |
| client | 98 | 2.7 (12) |
| isolation | 21 | 1.9 (12) |
| digest | 8 | 1.6 (12) |
| seam | 29 | 1.5 (12) |

all six lanes' test runs, sum of medians: 28.7 s; a per-lane scope would remove ~8208 s over the 407 lane faults, 20.2 s per fault (estimated from these medians)
lane-weighted mean lane fault: 44.8 s; the per-lane scope's saving is 45% of it

## How this sample relates to the trunk numbers

#353's 18.5 s is a whole trunk run's median over the 374 lane faults it had then. #347's last PR run (verify 37164428741) gave a 23 s median and a 23.4 s mean over 407 lane faults, 9,536 s of shard-log seconds in all.

Lane by lane, the sampled faults' totals on CI match that run's shard-log means. This shows the wrappers do not distort the split:

| lane | sampled faults here, total s | #347's shard-log mean over the lane's faults, s |
|---|---|---:|
| adapters | 28.6, 28.4 | 28.8 (37) |
| client | 27.5, 27.0 | 26.9 (98) |
| isolation | 28.3, 28.2 | 28.0 (21) |
| seam | 28.6, 30.8 | 28.1 (29) |
| drive | 19.0, 39.5 | 19.8 (214) |
| digest | 26.9, 16.3 | 22.9 (8) |

The gap between this sample's lane-weighted mean (28.5 s) and #347's 23.4 s comes from one drive fault, `drive.serve-bin-ignores-the-log-flag` at 39.5 s. It does not come from the wrappers.

## Re-running it

```
python3 sample.py ROOT > sample.txt
python3 phases.py ROOT WORK out.jsonl $(cat sample.txt)    # GNU_CP=… on macOS; PHASES_TARGET=… for a cached target
python3 analyze.py out.jsonl
python3 lanes.py out.jsonl ROOT
```
