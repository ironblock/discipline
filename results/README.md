# Results

This directory contains experiment results.

The contract is `results/_template/`, enforced by `scripts/check-results.py` (files, sections, front-matter keys, numbers-versus-data) and `diet check-record` (the record itself).

Each test should have a subdirectory named by the pattern: `YYYY-MM-DD-<ledger-slug>`.

Results are brief by design, meant to describe:
  observation → hypothesis → test → results → conclusion
