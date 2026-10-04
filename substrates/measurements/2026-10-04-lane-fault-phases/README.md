# Where a seeded fault's selftest seconds go: lane and `test` faults, by phase (#353)

**The finding.** A lane fault's seconds go mostly to running test binaries: 74% on a CI runner, 65% on the Mac Pro. Most of that is the five lanes the fault is not in. Each lane fault is proven by the `lanes` check, which is `apply-lane-faults.py --verify`, and that runs every registered lane's `cargo test`, not only the faulted lane's. Those other five lanes are 67% of a lane fault's median on CI (19.0 s of 28.3 s) and 60% on the Mac Pro (25.6 s of 42.5 s).

The smallest change that roughly halves a lane fault is a per-lane scope: the `lanes` check, inside a lane fault's sandbox, runs the faulted lane's command alone. Estimated from the per-lane medians below, weighted by each lane's fault count, that removes about 13.9 s per lane fault on CI, about 5,660 s across the 407 lane faults. On #347's last PR run (verify 37164428741) those 407 faults took 9,536 s of shard-log seconds. This is an estimate, not a measured change.

A `test` fault is different: its tests run in 0.1 s on CI. Its time goes to the build (61%) and the sandbox copy (31%).

## Method

`phases.py` replays what `verify.sh`'s `seeded_case` does for one fault, in its order and with its commands, and times each step:

1. the sandbox copy: `git ls-files` → `cp -L --parents`, `git init`, `git add --all`;
2. the two `sandbox_state()` fingerprints and the `diff-tree` between them;
3. the injection: `apply-lane-faults.py --apply-only` for a lane fault, or the case's `inject_*` function sourced from `verify.sh` for a hand-written one;
4. `hermetic.sh env CARGO_TARGET_DIR=… bash ./verify.sh --only CHECK [--scope S]` in the box;
5. the verdict: every catcher's signature matched against the log.

Inside step 4, three wrappers log what cargo did without changing the tree the fault is graded on:

- a `cargo` shim, first on `PATH` (which `hermetic.sh` admits), records each invocation's wall clock and cargo's own `Finished … in Xs`;
- a `build.rustc-wrapper` times each rustc;
- a linker wrapper times each link.

The two wrappers reach the build through a `.cargo/config.toml` written into the box after the second fingerprint. `analyze.py` turns the rows into the tables below, and its docstring defines each phase. `lanes.py` gives each lane's own test run.

**The sample** is chosen by `sample.py`, by rule, not by hand:

- for each of the six lanes, its first and middle fault by sorted id (12 lane faults);
- twelve `test` cases, spaced evenly over their sorted ids;
- a warm-up first, which primes the target and is dropped from every table.

Every fault in both samples went red for its own reason: each row's `verdict_ok` is true.

## Receipts

| host | where | when (UTC) | tree | rows |
|---|---|---|---|---|
| a GitHub `ubuntu-latest` runner | workflow `measure-353` on the scratch branch `measure/353`, run 37168505268; one job, set up as `gate-selftest.yml`'s shards are, with bubblewrap, the user-namespace relaxation, `DIET_REQUIRE_SANDBOX=1`, and its Cargo cache restored with no save | 2026-10-04 01:40–01:48 | 65dac30, develop 8518de9 plus the harness | `ci-phases.jsonl` |
| the Mac Pro: Xeon W-3235, 24 threads | `phases.py` run directly, with GNU `cp` | 2026-10-04 01:55–02:08 | the same tree | `macpro-phases.jsonl` |

The scratch branch existed only to run that job. It was never a pull request, and it was deleted after the run. Its harness is byte-identical to the one here.

**The Mac Pro's numbers were taken under shared load**, recorded rather than retaken (ruled by Dispatch): load averages of 8.00 at the start and 9.72 at the end, with 11.20 as the five-minute average, on 24 threads, while another seat's verify ran. The Mac Pro has no bubblewrap, so the isolation lane's tests take their refusal path there, and its isolation row is not comparable with CI's. Its copy phase is 6.9 s against CI's 2.0 s, because macOS copies and stats the tree more slowly.

## CI runner

**lane faults, n = 12** (all verdicts read: True)

| phase | median s | p90 s | median share of the total |
|---|---:|---:|---:|
| copy | 2.0 | 2.7 | 7% |
| fingerprint | 0.1 | 0.1 | 0% |
| inject | 0.1 | 0.1 | 0% |
| build | 4.7 | 6.2 | 17% |
| link (within build) | 2.5 | 2.6 | 9% |
| test run | 20.8 | 21.3 | 74% |
| other | 0.2 | 0.2 | 1% |
| verdict | 0.0 | 0.0 | 0% |
| total | 28.3 | 30.8 |  |
| other lanes (removable) | 19.0 | 20.8 | 67% |
| own lane's test run | 0.8 | 4.8 | 3% |

per fault, the other lanes' share of its total: median 69%, min 24%, max 77%

**test faults, n = 12** (all verdicts read: True)

| phase | median s | p90 s | median share of the total |
|---|---:|---:|---:|
| copy | 2.0 | 2.3 | 31% |
| fingerprint | 0.1 | 0.1 | 1% |
| inject | 0.0 | 0.0 | 1% |
| build | 4.0 | 5.6 | 61% |
| link (within build) | 0.3 | 0.8 | 5% |
| test run | 0.1 | 0.3 | 2% |
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

## Mac Pro

**lane faults, n = 12** (all verdicts read: True)

| phase | median s | p90 s | median share of the total |
|---|---:|---:|---:|
| copy | 6.9 | 7.6 | 16% |
| fingerprint | 0.5 | 0.7 | 1% |
| inject | 0.1 | 0.1 | 0% |
| build | 7.5 | 10.4 | 18% |
| link (within build) | 3.7 | 4.0 | 9% |
| test run | 27.7 | 31.5 | 65% |
| other | 0.6 | 0.7 | 1% |
| verdict | 0.0 | 0.0 | 0% |
| total | 42.5 | 51.5 |  |
| other lanes (removable) | 25.6 | 30.7 | 60% |
| own lane's test run | 0.8 | 7.8 | 2% |

per fault, the other lanes' share of its total: median 60%, min 30%, max 67%

**test faults, n = 12** (all verdicts read: True)

| phase | median s | p90 s | median share of the total |
|---|---:|---:|---:|
| copy | 6.8 | 7.5 | 42% |
| fingerprint | 0.5 | 0.6 | 3% |
| inject | 0.1 | 0.1 | 1% |
| build | 6.4 | 10.5 | 39% |
| link (within build) | 0.4 | 0.9 | 3% |
| test run | 2.0 | 2.6 | 12% |
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

## How this sample relates to the trunk numbers

#353's 18.5 s median is a whole trunk run's median over all 374 lane faults then. #347's last PR run gave 23 s over 407 (shard-log seconds). This sample takes two faults per lane and does not weight them by lane, so its 28.3 s median is the sample's own and does not stand in for either. The per-lane estimate above is weighted by the lane counts `apply-lane-faults.py --list` gives on this tree.

## Re-running it

```
python3 sample.py ROOT > sample.txt
python3 phases.py ROOT WORK out.jsonl $(cat sample.txt)    # GNU_CP=… on macOS; PHASES_TARGET=… for a cached target
python3 analyze.py out.jsonl
python3 lanes.py out.jsonl ROOT
```
