# `exercise`

The reference coding harness: it implements `diet`'s dogma perfectly, is
inspectable if you choose, and is otherwise a conventional, minimal harness.
Its charter and its definition of done are on #31. The loop it drives is
#117.

`exercise` is the vehicle that exercises `diet`. `diet` meets the UX and
workflow requirements this application expresses, not the other way round.

## What is here: R1, the surface

```
pnpm install
pnpm dev              # the harness on the canned transport, http://localhost:5173 (?speed=4 to hurry it)
pnpm storybook        # the surface at every moment of the specimen, http://localhost:6006
pnpm verify           # typecheck, lint, unit tests, every story as a browser test
pnpm perf             # a performance trace: frames, long frames, layout, style, script, a profile (--ablate, --css)
```

**Driving `diet`.** `?drive` drives `diet`'s own session, served over HTTP
by `diet-drive serve` (#140; `src/drive/http.ts`). The page reaches it
same-origin through the dev server's proxy:

```
cargo build --bin diet-drive                                   # from diet/
target/debug/diet-drive serve --endpoint <llama-server>/v1/chat/completions \
    --model <name> --head <file> --port 7801 --allow-origin http://localhost:5173
DIET_DRIVE=http://127.0.0.1:7801 pnpm dev                      # then open /?drive
```

With no model at hand, `node scripts/model-stand-in.mjs` is one: every
request answered with `diet`'s own captured llama-server reply, byte for
byte (a random-weight model, so the words are noise), at
`http://127.0.0.1:7901/v1/chat/completions`, with
`scripts/model-stand-in.head.txt` as the head.

`pnpm perf` (`scripts/perf.mjs`) drives a production build in headless
Chromium through three sessions -- replay, scroll, pointing -- and says
where frames go. Headless Chromium rasterises and composites in software:
its script, style and layout numbers are the app's, its paint and
compositing numbers are not (glass costs frames there and none on a GPU).
Compare runs with each other, not with a machine.

CI runs the same thing as the repository's `exercise` check (`./verify.sh --only exercise`, owned in `.github/check-owners.tsv`, run by `pkg-exercise.yml`); its seeded fault is a type error.

- **The trunk is a conversation.** One block per message, role by fill,
  prose proportional, chain-of-thought italic. Each block's header says what
  went in -- its role, and what it read -- and its footer what came out, in
  one shape for both: `+100 tok in 5.0 s (20 t/s pp)`, counting up while it
  runs (and, while it reads, its top edge fills with the new part). A tool
  call is one block after the message that wrote it, by the same rule: its
  header what writing it took, its body like a REPL -- the command, and what
  it printed under it, first lines until opened from under them -- and its
  footer the tool's stats instead of tokens: `18 lines · 475 B in 30 ms`,
  and its exit.
- **Behind the curtain**, each server slot other than the trunk's is a
  column. An interview sits level with the trunk message it came from, in
  the slot that served it, with the patches it landed, cabled to it. A seam
  is drawn across every column, as the one deliberate prefill event.
  Condensed, each side call is a bar that keeps its place and grows as it
  writes. Narrow, the curtain draws no more than the row has room for:
  bars when whole side calls do not fit, closed when bars do not either;
  then a side call opens under the message it came from.
- **Working memory** is always on the right: a column when the row has
  room, a drawer on the right edge when it does not. Each side call on
  screen has a line into every entry it wrote.
- **Lines** -- the trunk's cables to its side calls, and those into working
  memory -- are traces by default, routed as a wiring harness
  (`src/ui/harness.ts`): each net on its own track in a gutter, forking to
  where it goes, hopping what it crosses. They can be sweeps (curves)
  instead; see settings.
- **Settings** (the header's `settings`, `src/ui/prefs.ts`) are what a
  person may prefer: theme, mode (dark, light, the system's), connectors
  (traces or sweeps, and a trace's crossings and corners), motion (the
  system's, on, off), lines into memory, the minimap. Remembered in the
  browser; any may be set for one visit from the address by name
  (`?theme=paper&mode=light&connectors=sweep`). Each is a toolbar switch in
  Storybook.
- **"What diet can't emit yet"** outlines everything on screen that is
  drawn from an event `diet` does not produce, naming the step of #117 it
  waits on. Today that is everything, which is the point.

## How it is put together

| path | what it is |
| --- | --- |
| `src/drive/log.ts` | The session's log as the surface reads it: `diet/formats/log` v0 (#137), **mirrored by hand** until track one's generated bindings replace it, and held to `diet`'s own valid fixtures by `log.test.ts`. Beside v0, what the surface draws that the log does not say yet is marked AHEAD, each tagged with the step of #117 that will add it. A node's id is the `seq` of the line it began at. |
| `src/drive/script.ts`, `place.ts` | The authored shape the specimen, the kitchen sink and the recorded sessions are written in -- labels (`q/2`, `i/1`) where the log has `seq` references -- and the step that places a script in the log. Only the log is folded; stories find nodes by label through `idOf`. |
| `src/drive/transport.ts` | The drive interface: subscribe to the session's log; send an ask, cancel, declare a seam. |
| `src/drive/http.ts` | The transport against `diet`'s served session: `/events` as server-sent events, `/commands` as JSON. A closed stream asks, once, what it was answered with (finding 17, #117): 410 rebuilds from a new session's first line, anything else is shown to the author with its reason. |
| `src/drive/specimen.ts` | **Authored, not recorded.** One session walking the definition of done, as a script. Deleted when a recorded session replaces it. |
| `src/drive/canned.ts` | A transport that plays the specimen with real timing, and `snapshot()`, the same session stopped at any moment. |
| `src/drive/recorded/` | **Recorded, not authored** -- adapter-shaped inputs: foreign logs through a PII-scrubbed migration, allowed as stand-ins by #31's fixture rule as amended (Planning, #25, 2026-09-28); the migration is an adapter, not a second parser of `diet`. Sessions the predecessor recorded against a real model, migrated once by `scripts/migrate-recorded.py` (each file's `migration` header says what the migration decided) and scrubbed of names and paths. `?session=first-drive` replays one (also `cancelled-capture`, `step-limit`); `Session/Recorded` stops each where it went wrong. `voxel-stress` is an OpenCode session (`scripts/migrate-opencode.py`): native tool calls, several a step, six tools -- its side calls did not run, and are stitched on by `scripts/stitch-sides.py`, their answers written by a model from the trunk's own text; a stand-in until a drive's recording replaces it. A kind the vocabulary lacks is carried under its own name, never dropped. Deltas and progress frames are synthesized from each response's timings, since neither was recorded. |
| `src/drive/kitchen-sink.ts` | **Authored, at a working drive's cadence.** Six asks over three phases and two refills, side calls where #31 wants them (an extraction while a tool runs, an interview in the person's gap, a ratify before each refill): the happy path, to hold a recording's receipt up against. Written as a script and placed on a clock by `compose.ts` (tokens from text, prefill and decode from assumed rates). `?session=kitchen-sink` replays it; `Session/Kitchen sink` stops it at moments. |
| `src/ui/sets.ts` | The registries for the open sets -- lanes, tools, outcomes, patch ops, reasons, refusals: each known member and how it is drawn, and a neutral fallback, under its own name, for everything else. |
| `src/session/gap.ts`, `useIdleGap.ts` | The idle gap (Q4 on #117), measured on the page: from a turn settling to the person's next accepted command, split into notice, read, compose, away and blocked -- integer ms, summing exactly to the gap -- and carried as `idle_gap` on the command that ends it, which the drive logs as `idle.gap`. The canned transport logs it as `diet` will; the HTTP transport holds it until `diet` takes it. The receipt's sixth number is exact once every gap was measured. |
| `src/session/fold.ts` | The only place an event becomes something drawable. Every node is branded `Folded`, carries the log positions it came from, and the steps of #117 it waits on. |
| `src/ui/` | The parts. `Block` is the session event; messages, tool calls and lane bars refine it. |
| `src/stories/` | `Session/Moments`: the whole surface at eleven moments of the specimen. `Session/Recorded`: a real session where it went wrong. `Session/Kitchen sink`: the happy path. `Session/Failures`: every way a session fails, drawn. `Session/Live`: the canned transport, driven. `Parts`: one story per state each part distinguishes, and each open set's fallback. |

Theme tokens (`src/theme/tokens.css`) are named for kinds of text and
meanings, never for faces or hues; several resolve to the same value on
purpose. Surfaces bind to tokens that say how a thing sits -- relief (in
the prefix, being written, evicted at a seam, beside it), glass, light,
texture, the live field -- and every default is neutral.

A look is a **theme** and a **mode**. `tokens.css` is the dark palette and
every default, and `themes/light.css` the light palette. A theme's file in
`src/theme/themes/` sets only what it treats differently, under
`[data-theme]`, and what it treats differently by mode under
`[data-theme][data-mode]`, so each token has one home per look and nothing
depends on import order. The palette makes every colour an LED with the
meaning it has on a server: blue is you, white the model, green work, amber
attention, red a fault; violet, magenta and cyan are diet, asking, deciding
and reading (the lamps are in `tokens.css`). `bloom` is frosted slabs over
pools of their own colour, glass beside the prefix, light pooled where the
work is, what runs glowing while it runs, each header and footer a shaded band.
`paper` is the same meanings flat and printed. `emboss` raises the prefix
out of one material, square-edged, lit from the top left.
The mapping behind all of them: material = in the prefix, glass = beside it,
light = happening now.
