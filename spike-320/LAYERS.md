# #320 report 3: what each selftest layer caught that the layer below did not, from the record

Compiled from the issue and PR threads and `git log` by a separate read-only research instance, 2026-10-04. Every row cites what was read. No Actions logs were read.

| layer | caught, that the layer below did not | evidence | date |
|---|---|---|---|
| census (`check-selftest-census.py`) | `--shard 275/1000` selected no fault while the seeded-fault layer printed "every gate was seen red" and exited 0. The commit names the census's refusal of an incomplete union as what made it safe. It was reproduced by hand; CI could not reach it. | commit 62e4654 | 2026-09-12 |
| census, runtime form (`check-shard-census.py`) | Review reproduced three edits below the listing that dropped members while coverage and `verify --only ci` stayed at exit 0: recompute dropping a directory per shard, the applier dropping its last injection, and a shard passing when not listing. Each was seeded red on the new runtime census. A designed catch, not a real run. | #268 5957902133; commit 9f2fc01 | 2026-10-02 |
| scope plan | Measuring it against the trunk census found 6 of the last 30 trunk pushes cancelled, so PRs were being scoped against an older census. This led to "never cancel a trunk run". | #112 5879484294 (Finding 2); commit 7ed2ef6 | 2026-09-27/28 |
| scope plan (its own defect) | The same measurement found the plan scoped nothing (578 of 578 re-proven), because mechanics counted as machinery. Measurement found it, not another layer. | #112 5879484294 (Finding 1); commit 7ed2ef6 | 2026-09-27 |
| budget / wall clock | `pkg-bsd` (443 s) and `repo` (357 s) were over the 300 s budget. That led to #262's declared split and to `recompute` getting its own package. | #260 5951858963; #262 5952237672, 5952272752; commit fde6e4b (#268) | 2026-10-02 |
| budget / wall clock | The scope plan's false positive, a comment-only change read as machinery, re-proved about 800 faults on every PR touching `verify.sh` for about a day. It was noticed by the clock, not by any gate. | #25 5966627608; #305 (runs 37099510136, 37097117869); commit f6376e3 (#308) | 2026-10-03 |
| budget (a cost, not a catch) | A hard-failing load-balancing plan priced every fault-adding PR at a 76-minute harvest. It was ruled back to a printed number. | #108 5808326664; #25 5808333599; commit 1bc5a00 | 2026-09-24 |

**Layers with no recorded catch.**
- **Drift issues: none.** The opener has never opened an issue. The repository has no `check:` label, which the opener creates before its first issue. The two not-red faults fixed by commit (93cc5bb, 0e4d758) came from pull-request runs 37021427242 and 37131651858, where the opener does not run.
- **Census:** no refusal on a real CI run in the record. Its catches are hand reproductions and seeded faults.
- **Scope plan:** no case where the plan itself caught a defect relevant to a fault. Both rows came from measuring it, and its other defects were found by review (#317) or by the clock (#305).

**Not verified.**
- Whether the opener ever ran on a push or nightly run that had not-red rows. That needs the Actions logs.
- The trunk run where the stream-cancel flake reportedly hit a trunk shard (#268 5964789105). If it had a not-red row and no issue was filed, that is a drift-layer miss. No run id was found.
- The review threads of #130, #153 and #155.
