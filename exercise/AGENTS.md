# PURPOSE
- A coding harness first; inspection is optional. The trunk looks like a conversation. Machinery goes behind the curtain, and it must look like what it is for.
- The definition of done on #31 ranks the work. Anything that unblocks none of its steps waits.

# MECHANISMS
- One path to the screen: transport → log → `fold()` → a `Folded` node → a component. A component takes nodes, never literals. A story gets its nodes from a folded moment of the specimen (`src/stories/moments.ts`).
- Every field the surface needs and `diet` cannot emit goes in `src/drive/events.ts`, tagged in `NEEDS_OF`. Never render a value the log does not carry.
- The specimen is authored intent, not a record. Never present it as a measurement, and never grow it to make a component look good. Grow it only when a step of the definition of done needs a moment it lacks.
- Tokens are semantic. Bind a component to the kind of text or the meaning (`--font-answer`, `--fill-user`), never to a face or a hue.
- No second parser: anything that reads a `diet` format goes through `diet`.
