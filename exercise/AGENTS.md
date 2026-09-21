# MECHANISMS
- Instrumentation-first: every fork, seam, and capture visible with its provenance
- No second parser: anything that reads a format uses `diet` or passes the same conformance corpus
- A row renders only what it can cite: row props are `Bound<T>` values, constructible only in `src/record/bound.ts`. A literal handed to a row is a type error, and so is a field the SPA computed for itself.
- A field the record lacks is drawn dotted and filed, never faked: one `Pending` in a row, one line in `src/record/pending.types.ts`, one fixture in `fixtures/pending/`. The three share the issue string.
- Rows are event groups formed only through relations `validate()` checks. Row order is not a relation.

# LEDGERS
- Green while the gap exists, red the moment it closes. A typecheck or `pnpm check:fixtures` going red on a ledger line is an instruction, not a defect: delete the line, move the fixture, write the Sequences story.
- Every `@ts-expect-error` cites an issue or an `unfiled/` slug. Lint refuses one that does neither.
- A gate is seen red before it is trusted: `scripts/verify-red.mjs` seeds one fault per gate and requires each to fail for its own diagnostic. Add a gate, add its fault.
