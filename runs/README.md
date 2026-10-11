# Runs

Driven sessions kept as evidence that the program's pieces and levers work: a session someone drove through the exercise surface, with what it showed and what broke. A run is not a claim about the program. Claims, with a hypothesis held against a rule, stay in `results/`.

Each run is one directory, `runs/YYYY-MM-DD-<name>/`, where the name says what was driven and by whom (`-agent-driven` when an agent played the operator). It holds:

| file | what it is |
| --- | --- |
| `README.md` | who drove it; the build, regimen and floor; what it showed (which rows or levers); what broke; what was scrubbed; each file below pinned by sha256 |
| `session.log` | serve's log of the session (`--log`) |
| `record.jsonl` | the record serve wrote at the session's end (`--record`) |
| `receipt.json` | the receipt written beside it |
| `product.txt` | the working memory at the end; the record's summary row carries its sha256 |
| `unspellable.json` | the record's sidecar: what the log holds that the record cannot spell |
| `regimen.toml` | the regimen the run applied, its own copy, with tilde paths |

A run whose record was never written has no `record.jsonl`, and its README says why.

**Before a run lands:**
- `diet check-log` and `diet check-record` accept its log and record, and its README says they were run. A log or record that serve wrote and diet refuses lands as the run's finding: its README quotes the refusal and names the issue that fixes it.
- The tree-wide hygiene check (`./verify.sh --only hygiene`) covers `runs/`. A log is scrubbed until it passes, and the README names each replacement. Digests the log and record carry inside them were taken over the unscrubbed bytes.

A run another person or agent drove says so in its first lines. Runs the maintainer drives and runs an agent drives sit side by side here, and the differences between them are data.
