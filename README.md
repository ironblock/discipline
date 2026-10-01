# discipline
An inverted economic model for agentic coding: Ephemeral interview forks inform a rolling summary of your session, separating signal from noise and preventing your LLM from paying "full freight" on the 300kb HTML file it read to answer a simple question in turn two.

**Start here.** `diet/` holds the library and its binaries, `diet` and `diet-drive` (the one a harness talks to); `exercise/`, the reference harness, holds its surface. The first thing to run is the stand-in drive in [`exercise/README.md`](exercise/README.md), under **Driving `diet`**: a session driven end to end with no model at hand, against a captured model reply served on loopback.
