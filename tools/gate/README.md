# Gate
- Skipped is not failed: No path filtering. An aggregator compares each job's result to the literal string `success`; `!failure()` passes on skipped. A filtered-out workflow leaves its required check pending forever.
- The registry is the pin: A gate discovered by globbing can silently find nothing. Required checks are enumerated explicitly at the root.
- Assert on structured failure classes, not message strings: Matching prose is grep-as-verdict one level up; it is why signatures go stale.
- A gate with a build step has a stale-binary axis: Stamp the binary that ran, not the tree's HEAD. Refuse a stale choice rather than pick one. *Specimen: a developer-machine gate graded with a release binary seven days behind the source, lacking a guard the source had carried all week. Four instruments defined a constant named after the environment variable they ignored.*
- Replace hardened instruments by parity, never by patch: The successor is seen red on every seeded fault beside the incumbent before the incumbent retires.
- Changing an instrument invalidates its numbers: Measure before and after; zero rows changed is the receipt.

## `faults.toml`
Extracted from the gate, gated in both directions (it can neither omit a proven fault nor claim an unproven one), every fault carrying a `failure_class`, relocated fixtures carrying `migrated_to` with their redness re-proven in the new home.
