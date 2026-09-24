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
```

- **The trunk is a conversation.** One block per message, role by fill,
  prose proportional, chain-of-thought italic, a low-contrast mono footer of
  what the harness measured on every block.
- **Behind the curtain**, each server slot other than the trunk's is a
  column. An interview sits level with the trunk message it came from, in
  the slot that served it, with the patches it landed; working memory is on
  the right. A seam is drawn across every column, as the one deliberate
  prefill event.
- **"What diet can't emit yet"** outlines everything on screen that is
  drawn from an event `diet` does not produce, naming the step of #117 it
  waits on. Today that is everything, which is the point.

## How it is put together

| path | what it is |
| --- | --- |
| `src/drive/events.ts` | The events the surface needs the drive to emit. **Provisional**: this is the surface's half of #117, each kind tagged with the step that would make `diet` emit it. When the loop lands, its generated types replace this file. |
| `src/drive/transport.ts` | The drive interface: subscribe to the session's log; send an ask, cancel, declare a seam. An HTTP + SSE transport implements it against #117's loop. |
| `src/drive/specimen.ts` | **Authored, not recorded.** One session walking the definition of done, in the provisional vocabulary. Deleted when a recorded session replaces it. |
| `src/drive/canned.ts` | A transport that plays the specimen with real timing, and `snapshot()`, the same session stopped at any moment. |
| `src/session/fold.ts` | The only place an event becomes something drawable. Every node is branded `Folded`, carries the log positions it came from, and the steps of #117 it waits on. |
| `src/ui/` | The parts. `Block` is the session event; messages, tool calls and lane bars refine it. |
| `src/stories/` | `Session/Moments`: the whole surface at eleven moments of the specimen. `Session/Live`: the canned transport, driven. `Parts`: one story per state each part distinguishes. |

Theme tokens (`src/theme/tokens.css`) are named for kinds of text and
meanings, never for faces or hues; several resolve to the same value on
purpose.
