# PURPOSE
- A coding harness first; inspection is optional. The trunk looks like a conversation. Machinery goes behind the curtain, and it must look like what it is for.
- The definition of done on #31 ranks the work. Anything that unblocks none of its steps waits.

# MECHANISMS
- One path to the screen: transport → log → `fold()` → a `Folded` node → a component. A component takes nodes, never literals. A story gets its nodes from a folded moment of the specimen (`src/stories/moments.ts`).
- The log is `diet`'s (`src/drive/log.ts`, v0). Every field the surface needs and `diet` cannot emit yet goes there marked AHEAD, tagged by a need in `NEEDS`. Never render a value the log does not carry.
- A need cites the open ticket that would close it (`NEEDS`' `ticket`), and that ticket's Done-when carries "remove the need from exercise's `NEEDS` (`exercise/src/drive/log.ts`)". No ticket fits: file one in the milestone that needs it, or in Later — loop correctness. `log.test.ts` turns red when a line `diet` writes carries a need, so a fixture update closes the loop.
- The specimen is authored intent, not a record. Never present it as a measurement, and never grow it to make a component look good. Grow it only when a step of the definition of done needs a moment it lacks.
- Tokens are semantic. Bind a component to the kind of text or the meaning (`--font-answer`, `--fill-user`), never to a face or a hue.
- No second parser: anything that reads a `diet` format goes through `diet`.
