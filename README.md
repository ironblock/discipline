# discipline
An inverted economic model for agentic coding: Ephemeral interview forks inform a rolling summary of your session, separating signal from noise and preventing your LLM from paying "full freight" on the 300kb HTML file it read to answer a simple question in turn two.

**Start here.** The library, and `diet-drive`, the binary a harness talks to, live in `diet/`; the harness's surface lives in `exercise/`. The first thing to run is [`exercise/README.md`](exercise/README.md)'s stand-in drive: a session driven end to end with no model at hand, against a captured model reply served on loopback.
