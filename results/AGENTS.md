
 
 
 
 
 
 
 
**Never preserve a flaw to reproduce a result.** Preserve the raw output and the version tag; any version can re-run. Regrade over raws and record both numbers.
 
 
**Every p-value ships with its attainable floor.** A sample size bounds the smallest p it can produce; report it.

**A bar is checked against what can attain it, before a verdict** (ruled on #17; operative from 2026-10-13). A rule (`decision-rule*.toml`, `comparison-rule*.toml`) dated after that declares an `[[attainability]]` row for each supported and refuted clause, and a `[constructed]` table, and `scripts/check-results.py` holds them to six checks:
- a `bar` row (`clause`, `statistic`, `asks` = `at_least` | `at_most` | `below` | `above`, `bar`, `best`, and `trivial` with `constant`, `random` and `lexical`, each an exact decimal or `not applicable: <why>`): the bar is the one its clause's text states, under the comparison `asks` names (`>=`, `<=`, `<`, `>`); it is attainable by a perfect scorer on the same pooling, and no trivial scorer clears it;
- a `floor` or `control` row declares `fires_on_predicted_outcome = false`: a floor that fires when the hypothesis is right refutes it for being right;
- no clause asks a p below its attainable floor, which no data can give;
- a `margin` row reads an interval (`reads = "interval"`), never a point estimate;
- a cell-veto control (`veto = "cell"`) reports the share of rows that tripped it (`reports_tripped_share = true`);
- `[constructed]` states `share_of_positives` and `carries_verdict`, and a verdict planted or authored rows carry says so in its word.

A rule from before the date is held to the third check only, and its failures, with what the construction audit of 2026-10-13 found, are reported beside the audit's row and do not fail (`GRANDFATHERED` in the checker). Fixtures: `tests/fixtures/results-attainability/`.
 
**Under concurrency, every throughput figure declares its question** — per-stream or aggregate.

# CONTENT
**Unregistered artifacts accumulate authority by presence.** Everything in results has a manifest row or it doesn't exist. *Specimen: 1,019 committed data artifacts; 11 cited.*
**Artifacts store what recomputation consumes.** The answer, not its character count. Recompute-sufficiency is a property of the storage schema, not a hope.
**Evidence cited by the record lives in the repo.** External attachments are pointers, never primary storage. *Specimen: three of the program's most-cited findings had their only evidence attached to tickets in a system a reader can't open.*
**Every result carries its full regime.** Model, quantization, sampler settings, reasoning state, dogma version, substrate. A temperature-0, thinking-off result does not generalize to the configuration you actually code in.
**Defect disclosure rides the artifact.** `known_defects` lives in the manifest, machine-readable, not in a methodology doc nobody greps.

# INTERPRETING
**A historical rate is a floor for its era's instrument, not the mechanism's ceiling.**
**Prose is verified against data or generated from it — never merely written.** Numbers in a report are checked against the run's data by CI. Documentation of what a crate encodes is generated from the crate.
