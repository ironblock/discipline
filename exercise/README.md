# `exercise`

`exercise` is a React SPA which serves as a reference implementation of "how an agent harness would perfectly implement a `diet` `regimen`".

It's not meant to replace Claude Code or OpenCode or Codex or Pi - it's meant to provide a similar "core" loop with total flexibility, absolute instrumentation, and inspection capabilities that are either not implemented or not implement*able* within one of the popular harnesses.

## What is here now: the catalog

The SPA's shell, routing and drive mode are not built. What is here is the
Storybook catalog that is the SPA's spec (#31): the record's atoms as
components, one row per event kind with a story per state the record
distinguishes, every consequential sequence rendered from a record `diet`
accepts, and a typed, CI-tracked list of every place the viewer needs
something the record does not carry.

```
pnpm install
pnpm storybook        # the catalog, at http://localhost:6006
pnpm verify           # everything CI runs, in order
```

`pnpm verify` needs the workspace's `diet` and `diet-drive` binaries and
builds them if they are missing (`cargo build -p discipline-diet`), and
Playwright's Chromium (`pnpm exec playwright install chromium`).

### Organized by the record, not by atomic design

| section | what a story is |
| --- | --- |
| `Fields/` | an atom with plain local props: token count, byte size, id chip, link, lane and substrate badges, outcome glyph, digest, exit status, the dotted `Pending` placeholder. Designable now, pending atoms included, because nothing binds them yet. |
| `Rows/` | an event group, bound from a fixture through the loader. One story per state the record's doc comments distinguish: `tool_call.output` absent (not kept) is a different story from present-and-empty (printed nothing); an empty `response.text` is a typed outcome; a request with no response is a lane still running. |
| `Sequences/` | a whole record `diet check-record` accepts, rendered through `src/record/load.ts` -- the loader the app will use. Never a hand-assembled component tree. |
| `Pending/` | a record change the viewer needs and `diet` refuses today, with `diet`'s verdict on it. This section is the burn-down. |

### The loader, and the one thing it trusts

`src/record/load.ts` takes what `diet check-record` returned -- `project()`'s
value, identical bytes from the CLI and from `diet::wasm::check_record`
(#104) -- and decodes `canonical`, the record rendered back by `diet` itself,
line by line. It reads no `.jsonl`. The `JSON.parse` there is the SPA's one
trust boundary and it decodes a verdict already given; it decides nothing
about the format.

`scripts/gen-fixtures.mjs` writes every accepted fixture as a module
`export const events = [...] as const satisfies readonly Event[]`, which is
how the hand-written provisional types in `src/record/types.ts` are checked
against what `diet` actually emits. CI regenerates and diffs them.

### Two inverted ledgers

Both are green while a gap exists and red the moment it closes, so "this
fails until the record has the field" can live under a merge protocol that
requires CI green.

**Types** -- `src/record/pending.types.ts`, one line per missing atom:

```ts
// @ts-expect-error #92.4 fork.outcome is not on the fork event
export type _92_4 = Has<Fork, 'outcome'>;
```

Hinged on the field's *presence*, never on a literal value, so a field that
lands with a different vocabulary than the one the viewer drew still flips
the line. Every directive cites an issue or an `unfiled/<slug>`; lint refuses
one that does neither.

**Fixtures** -- `fixtures/pending/<name>.jsonl` with `<name>.expect.json`: the
proposed record change as JSONL, and the exit code, refusal class and message
`diet` gives it today, pinned. `pnpm check:fixtures` reports a fixture `diet`
now accepts as the gap closed, and a changed message as the refusal moved.
`diet` prints no refusal class, so the class in the sidecar is this package's
reading of the message, and the message itself is pinned verbatim.

When a line goes red: delete it, move its fixture from `fixtures/pending/` to
`fixtures/records/`, run `pnpm gen`, and give it a Sequences story.

### Seen red before trusted

`scripts/verify-red.mjs` copies the sources, seeds one fault per gate, and
requires each to fail for its own diagnostic -- a removed ledger directive, a
field landed in the provisional type, a literal handed to a row, a `Bound`
forged outside its module, a misspelt provisional field, a pending fixture
accepted, a pinned refusal moved. A fault whose needle matches nothing is a
failure of the script, not a pass.

### Fixtures

- `../diet/formats/record/fixtures/valid/` -- the record corpus, read in
  place. One source.
- `fixtures/records/canned-drive.jsonl` -- the canned drive's own record
  (`diet-drive diet/drive/dev-loop.toml`), the only record in reach with a
  fork, its capture and a seam that no hand wrote. Deterministic;
  `scripts/drive-fixture.mjs --check` re-drives and diffs it.
- `fixtures/records/{lane-running,retry-still-running,control-lane-before-turn-one}.jsonl`
  -- hand-written, accepted by `diet`, for states the corpus lacks.

### Type tokens are semantic

`src/theme/tokens.css` has one token per kind of text -- `--font-system`,
`--font-record`, `--font-tool`, `--font-ask`, `--font-answer` -- even where
two resolve to the same face today. A component binds to the kind of text,
never to a face.
