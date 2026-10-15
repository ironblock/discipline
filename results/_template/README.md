+++
hypothesis = "State the claim being tested, in one sentence, so that it could be wrong."
result = "supported"
kind = "reproducible-by-config"
figures = "referenced"
product_sha256 = "b491f615ad737993319517a59c5d450c320ea7d20358e4787e41526baae05182"
controls_run = ["null-regimen"]
known_defects = []
ambient = "not applicable: the template runs no judge, labeller, reader or coder"
turns = 2
prefill_tokens_total = 2048

[regime]
arm = "baseline"
substrates = ["local"]
dogma_version = 0
+++

# Template

Copy this directory to `results/YYYY-MM-DD-<slug>/`, where `<slug>` is the
claim's slug in the claim ledger. `scripts/check-results.py` lints it; the
copy must pass before it lands.

Every number in the front-matter, outside `[regime]`, must appear in
`run.jsonl`'s summary record. `[regime]` must agree with `regimen.toml`.
That is the whole point: prose is verified against data, never merely written.

Every figure in the body is a reference, not digits (#63): a path inside
double braces, rooted at `product.` (the one file here whose sha256 is
`product_sha256`), `front.` (a date; `regime.arm`, `regime.substrates` or
`regime.dogma_version`; `product_sha256` or `pre_registration_sha256`, the
digests this linter checks; or `claim_issue`, rendered `#<n>`) or `summary.`,
or `count()`, `round(…, n)` or `pct(…, n)` of one. `count()` is the number of
members of the list or table the path names, never a line or byte count;
`round` and `pct` round half-even, so a column of rounded figures does not
drift. A `front.` path the front-matter lacks, or whose value is not one of
those, is refused, never rendered empty. The linter renders every reference,
wherever it stands -- code included -- from the data at check time; `--render
DIR` prints the result. Results and Conclusion carry no typed figure, not in
prose, code, a link or a comment; elsewhere a figure stands only inside
`[uncited: <reason>]`.

A copy of this template must also give the claim's provenance (#32), each
field at the top level or named in `absent = { field = "why it is absent" }`:
`claim_issue` (the issue's number as a string of digits, no leading zero),
`supersedes` (the lowercase-hex sha256 of the product this one replaces: the
`product_sha256` of exactly one directory beside this one, never its own, and
never closing a cycle of supersessions; and no two directories beside each
other declare one product), `rule_ratified = { comment, at, digest }` with an
optional `of` (the maintainer's ratifying comment id, digits with no leading
zero; its UTC time; and the sha256 of the file `of` names --
`decision-rule.toml` unless `of` names another file here, spelled plainly, and
never this README) and `window_start` (UTC), with `window_start_from` naming
where that time was read. `rule_ratified_note` carries a caveat the three
values cannot. Whether the rule is post-hoc is derived -- ratified after the
window opened -- and never written.

A copy dated after the claim rules landed is held to them as well
(`results/AGENTS.md`):
- `regimen.toml` states `substrate_reasoning` and `[sampler]`, and the README
  lists in `characterisation` any field outside the registry's recommended
  sets;
- every `controls_run` name has a `seen_red` row whose committed evidence
  shows it red before the window, or `controls_run_absent` says why there is
  none;
- a later fire of the same claim under the same rule declares `repeats`.

A copy dated after the ambient-context rule landed declares its instruments'
ambient context (`results/AGENTS.md`). Each judge, labeller, reader or coder
run through an agent harness gets one row:

```toml
[[ambient]]
instrument = "segment-reader"
harness = "a subagent of the coding agent"
instruction_files = [{ path = "AGENTS.md", sha256 = "<the file's sha256>" }]
system_line = "<the system line it ran under, as it saw it>"
tools = ["Read"]
memory = "none"
probe = { evidence = "ambient/segment-reader.probe.json", sha256 = "<its sha256>" }
```

The probe is a committed JSON file. It holds `instrument`, the four carried
fields as the instrument actually carried them, and `at`, a UTC time. It is
recorded by asking the instrument or by reading its harness's own record,
never from what was meant to load. With no such instrument, the copy says
`ambient = "not applicable: <why>"`, as this template does. A rule that
declares `[[labels]]` has labellers, so that answer is not open to it.

## Observation

What was seen that prompted the test. No interpretation.

## Hypothesis

The claim, stated so that a run could falsify it. Repeat the front-matter
`hypothesis` verbatim.

## Test

The regimen, the arms, the controls, and the command that produces
`run.jsonl`. Someone else must be able to re-run it from this section alone.

## Results

What the run recorded, every figure a reference to the field that holds it.

## Conclusion

Whether the hypothesis survived, and what is still unknown. Brief by design.
