# Gate
- Skipped is not failed: No path filtering. An aggregator compares each job's result to the literal string `success`; `!failure()` passes on skipped. A filtered-out workflow leaves its required check pending forever.
- The registry is the pin: A gate discovered by globbing can silently find nothing. Required checks are enumerated explicitly at the root.
- Assert on structured failure classes, not message strings: Matching prose is grep-as-verdict one level up; it is why signatures go stale.
- A gate with a build step has a stale-binary axis: Stamp the binary that ran, not the tree's HEAD. Refuse a stale choice rather than pick one. *Specimen: a developer-machine gate graded with a release binary seven days behind the source, lacking a guard the source had carried all week. Four instruments defined a constant named after the environment variable they ignored.*
- Replace hardened instruments by parity, never by patch: The successor is seen red on every seeded fault beside the incumbent before the incumbent retires.
- Changing an instrument invalidates its numbers: Measure before and after; zero rows changed is the receipt.

## `faults.toml`
Extracted from the gate, gated in both directions (it can neither omit a proven fault nor claim an unproven one), every fault carrying a `failure_class`, relocated fixtures carrying `migrated_to` with their redness re-proven in the new home.

## `shards.tsv`
Which CI job runs which fault, and how many jobs there are. **Harvested, never edited.** An edit here is not a merge to resolve, it is a measurement to retake:

```
./verify.sh --selftest --derive-shards DIR
python3 scripts/derive-shards.py --index DIR/derive-shards.tsv --emit > tools/gate/shards.tsv
```

- A split by arithmetic divides the count, not the cost. Eight round-robin shards were right when measured and wrong two hundred faults later; the spread reached 58% and the run outlasted the shortest prompt-cache TTL a seat runs on. The budget that ceiling comes from is declared in `.github/gate-budget.tsv`.
- The shard count is an output, not a constant: the smallest N whose slowest shard fits the budget once every shard's fixed overhead is off. `verify.sh --shard K/N` refuses an N this file was not packed for rather than reinterpreting it.
- Adding a fault means re-harvesting. `derive-shards.py --check` runs under the `ci` check and refuses an assignment whose fault set is not the manifest's, in either direction — so the drift cannot recur silently, and the price of that is a full unsharded selftest per fault-adding change.
- Its own file, not a field in `faults.toml`: an entry there is author-facing and stable, this is machine-rebalanced on every manifest edit, and folding it in would make every `faults.toml` diff touch every entry.
