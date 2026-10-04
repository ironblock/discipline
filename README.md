<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="logo/logo-dark.svg">
    <img alt="Discipline" src="logo/logo-light.svg" width="720">
  </picture>
</p>

# discipline
An inverted economic model for agentic coding: Ephemeral interview forks inform a rolling summary of your session, separating signal from noise and preventing your LLM from paying "full freight" on the 300kb HTML file it read to answer a simple question in turn two.

**Start here.** `diet/` holds the library and its two binaries, `diet` (the command-line reader of every format: `diet check-log`, `diet check-record`, ...) and `diet-drive` (what a harness talks to); `exercise/`, the reference harness, holds the surface a person drives a session from. To drive one, read [`diet/drive/BEGIN.md`](diet/drive/BEGIN.md), the one start document, and run its **No model at hand** section first: a session driven end to end against a captured model reply served on loopback, from a browser or from `curl` alone, with no model server and no GPU.

**The gate** is `./verify.sh` at the repository root: `./verify.sh --help` says what it checks and `./verify.sh --list` names each check. What a pull request runs before it is pushed, and how branches and reviews work, is in [`CONTRIBUTING.md`](CONTRIBUTING.md).
