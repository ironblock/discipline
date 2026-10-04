# `exercise`

The reference coding harness: it implements `diet`'s dogma perfectly, is
inspectable if you choose, and is otherwise a conventional, minimal harness.
Its charter and its definition of done are on #31. The loop it drives is
#117.

`exercise` is the vehicle that exercises `diet`. `diet` meets the UX and
workflow requirements this application expresses, not the other way round.

## What is here: R1, the surface

```
npx --yes pnpm@11.20.0 -C exercise install
npx --yes pnpm@11.20.0 -C exercise dev           # the harness on the canned transport, http://localhost:5173 (?speed=4 to hurry it)
npx --yes pnpm@11.20.0 -C exercise storybook     # the surface at every moment of the specimen, http://localhost:6006
npx --yes pnpm@11.20.0 -C exercise verify        # typecheck, lint, unit tests, every story as a browser test
npx --yes pnpm@11.20.0 -C exercise perf          # a performance trace: frames, long frames, layout, style, script, a profile (--ablate, --css)
npx --yes pnpm@11.20.0 -C exercise build:replay  # the replay page for Pages (#32), into _site/replay/
```

Every command in this file runs from the repository root; every file it
cites is this directory's, `exercise/`, unless it says the repository's. pnpm
is pinned in this directory's `package.json` (`packageManager`), and the commands run
that version through `npx --yes`: a pnpm at another version switches to the
pinned one by itself, and on an Intel Mac that switch fails (#194). The
repository's `exercise` check runs it the same way.

**The replay page** (#32, `replay.html`, `src/replay.tsx`) is the surface
built for GitHub Pages: a recorded session replayed from this origin, with
no drive, no endpoint and nothing fetched but the site's own files. It
publishes `first-drive`, `cancelled-capture` and `step-limit`, which were
recorded whole (`src/replay/published.ts`); `voxel-stress` is not published,
since its side calls were written after the session, and the page says so.
Each published recording is a file of its own beside the page,
`data/<name>.js`, and carries its **admission** beside it in the tree:
`src/drive/recorded/<name>.admission.json`, written by
`python3 exercise/scripts/admission.py admit <name>`. The admission names the table
it was admitted under -- a snapshot of the genesis table, written once into
the repository's `scripts/` as `hygiene-admitted-<id>-patterns.tsv` and its siblings -- with
the snapshot's digests, the recording's own digest, and its scrub. A recording
edited since, or a snapshot edited, fails the build; a change to the live
table does not, since each recording stays under its snapshot until it is
admitted again, deliberately. A snapshot no admission names any more is
removed by the next `admit`, which says so, and the build refuses a tree that
still holds one (#257). A published recording with no admission does not
build. The repository's `check_site` (`verify.sh`) scans the built site: the
shell under the Pages table, each recording under its admitted snapshot, then
each against its admission. `scripts/replay-smoke.mjs` opens the built
page under a path, as Pages serves it, and loads a recording through its real
path.

**Authored examples** (#272; the maintainer's ruling on #32) are published
apart from the recordings, never as sessions: `kitchen-sink` is listed under
"Authored examples (not sessions)" and replayed under the maintainer's
sentence, `EXAMPLE_LABEL` in `src/replay/published.ts`, pinned over the page
for the whole replay. An example is written in TypeScript and committed as
`src/drive/examples/<name>.json` by `node scripts/write-examples.mjs`, which
is what the page publishes; `published.test.ts` fails while the two differ.
It is admitted by the same `admit`, under the same snapshot, with an
`Authored:` line carrying the sentence where a recording carries its
`Scrubbed:` line. After editing an example's source: write it, then admit it
again.

**Driving `diet`.** `?drive` drives `diet`'s own session, served over HTTP
by `diet-drive serve` (#140; `src/drive/http.ts`). The page reaches it
same-origin through the dev server's proxy:

```
cargo build -p discipline-diet --bin diet-drive
target/debug/diet-drive serve --endpoint <llama-server>/v1/chat/completions \
    --model <name> --head <file> --port 7801 --allow-origin http://localhost:5173
DIET_DRIVE=http://127.0.0.1:7801 npx --yes pnpm@11.20.0 -C exercise dev   # then open /?drive
```

With no model at hand, `node exercise/scripts/model-stand-in.mjs` is one: every
request answered with `diet`'s own captured llama-server reply, byte for
byte (a random-weight model, so the words are noise), at
`http://127.0.0.1:7901/v1/chat/completions`, with
`exercise/scripts/model-stand-in.head.txt` as the head.

`npx --yes pnpm@11.20.0 -C exercise perf` (`scripts/perf.mjs`) drives a production build in headless
Chromium through three sessions -- the kitchen sink, `first-drive` and
`voxel-stress` -- each in three phases (replay, scroll, pointing), and says
where frames go. Headless Chromium rasterises and composites in software:
its script, style and layout numbers are the app's, its paint and
compositing numbers are not (glass costs frames there and none on a GPU).
Compare runs with each other, not with a machine.

CI runs the same thing as the repository's `exercise` check (`./verify.sh --only exercise`, owned in the repository's `.github/check-owners.tsv`, run by its `.github/workflows/pkg-exercise.yml`); its seeded fault is a type error.

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
| `src/drive/log.ts` | The session's log as the surface reads it: `diet/formats/log` v0, its types **generated** from the format (the repository's `diet/formats/log/log.ts`, #144) and imported. On top of them, what the surface draws that the log does not say yet is marked AHEAD, each tagged with the step of #117 that will add it. Log v3's tool call -- the `tool_call` line and the delta's `tool_call` piece -- is generated like the rest (#297), and drawn (#300). `log.test.ts` folds every one of `diet`'s valid logs. A node's id is the `seq` of the line it began at. |
| `src/drive/script.ts`, `place.ts` | The authored shape the specimen, the kitchen sink and the recorded sessions are written in -- labels (`q/2`, `i/1`) where the log has `seq` references -- and the step that places a script in the log. A script's `tool.begin`/`tool.end` are placed as log v3's call, served-shaped: the calls' fragments before their response, each `tool_call` line at the call's end (`place.ts` says what that cannot carry). Only the log is folded; stories find nodes by label through `idOf`. |
| `src/drive/projection.ts`, `projections.ts` | **Every placed session, read by `diet`'s own log reader** (#300, ruled 5974717646): the repository's `exercise` check writes each one's PROJECTION (`scripts/export-projections.mjs`) and runs `diet check-log` on it. The projection takes out what the format does not have yet -- side lanes, forks, patches, seams, and the AHEAD keys -- declared in one list that only shrinks: an entry the format's generated types carry fails the typecheck. |
| `src/drive/transport.ts` | The drive interface: subscribe to the session's log; send an ask, cancel, declare a seam. |
| `src/drive/http.ts` | The transport against `diet`'s served session: `/events` as server-sent events, `/commands` as JSON. A closed stream asks, once, what it was answered with (finding 17, #117): 410 rebuilds from a new session's first line, anything else is shown to the author with its reason. |
| `src/drive/specimen.ts` | **Authored, not recorded.** One session walking the definition of done, as a script. Deleted when a recorded session replaces it. |
| `src/drive/canned.ts` | A transport that plays the specimen with real timing, and `snapshot()`, the same session stopped at any moment. |
| `src/drive/recorded/` | **Recorded, not authored** -- adapter-shaped inputs: foreign logs through a PII-scrubbed migration, allowed as stand-ins by #31's fixture rule as amended (Planning, #25, 2026-09-28); the migration is an adapter, not a second parser of `diet`. Sessions the predecessor recorded against a real model, migrated once by `scripts/migrate-recorded.py` (each file's `migration` header says what the migration decided) and scrubbed of names and paths. `?session=first-drive` replays one (also `cancelled-capture`, `step-limit`); `Session/Recorded` stops each where it went wrong. `voxel-stress` is an OpenCode session (`scripts/migrate-opencode.py`): native tool calls, several a step, six tools -- its side calls did not run, and are stitched on by `scripts/stitch-sides.py`, their answers written by a model from the trunk's own text; a stand-in until a drive's recording replaces it. A kind the vocabulary lacks is carried under its own name, never dropped. Deltas and progress frames are synthesized from each response's timings, since neither was recorded. |
| `src/drive/served/` | **Served, as `diet-drive serve` wrote it** -- a log in the format itself, not migrated: `rehearsal-turns-1-4.log` is lines 1-1440 of the rehearsal drive's log (#177), four turns, the fourth stopped mid-answer with its prefill's `progress` lines and no response. Committed under the recorded-fixture rule after the genesis scan. `served.test.ts` folds it at every line; `Session/Live` serves it to `?drive` through a stand-in for `serve` (#288). Beside it, `stopped-in-prefill.ts` is **constructed, not served**: the lines a stop in prefill would add after seq 1185, since no turn in the rehearsal stopped there (ruled on #294); a real capture replaces it. Log v3's tool calls are the courier's own fixtures (`diet/formats/log/fixtures/valid/a-v3-tool-call-*`, #297): `served.test.ts` folds them, through diet's projection, and `Session/Live` serves a ran, a refused and a policy-failed call (#300). |
| `src/drive/kitchen-sink.ts` | **Authored, at a working drive's cadence.** Six asks over three phases and two refills, side calls where #31 wants them (an extraction while a tool runs, an interview in the person's gap, a ratify before each refill): the happy path, to hold a recording's receipt up against. Written as a script and placed on a clock by `compose.ts` (tokens from text, prefill and decode from assumed rates). `?session=kitchen-sink` replays it; `Session/Kitchen sink` stops it at moments. Published on the replay page as an example, from `src/drive/examples/kitchen-sink.json` (#272). |
| `src/ui/sets.ts` | The registries for the open sets -- lanes, tools, outcomes, patch ops, reasons, refusals: each known member and how it is drawn, and a neutral fallback, under its own name, for everything else. |
| `src/session/gap.ts`, `useIdleGap.ts` | The idle gap (Q4 on #117), measured on the page: from a turn settling to the person's next accepted command, split into notice, read, compose, away and blocked -- integer ms, summing exactly to the gap -- and carried as `idle_gap` on the command that ends it, which the drive logs as `idle.gap`. `diet` logs it only if that command is admitted (#146); the canned transport does the same. The receipt's sixth number is exact once every gap was measured. |
| `src/session/fold.ts` | The only place an event becomes something drawable. Every node is branded `Folded`, carries the log positions it came from, and the steps of #117 it waits on. |
| `src/ui/` | The parts. `Block` is the session event; messages, tool calls and lane bars refine it. |
| `src/stories/` | `Session/Moments`: the whole surface at eleven moments of the specimen. `Session/Recorded`: a real session where it went wrong. `Session/Kitchen sink`: the happy path. `Session/Failures`: every way a session fails, drawn. `Session/Live`: the canned transport, to drive by hand, and one ask driven by the test. `Parts`: one story per state each part distinguishes, and each open set's fallback. |

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
