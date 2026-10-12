# The starter phase library (#787)

Five phases a session can run in: `orient`, `plan`, `build`, `test`, and
`quick`, a quick task with no transitions. Each is a phase document
(`diet/formats/phase/`): its name, its ground rules (what the model is given
while the session is in it) and the moves it lists, `suggested` (offered as
the recommended next step) or `allowed` (listed as available). The model
proposes a move, and the operator rules on it; a move a phase does not list
is refused.

A regimen names its library with `phase_library` -- `"starter"` for these,
compiled in; `"user"` for `$XDG_CONFIG_HOME/discipline/phases/`
(`~/.config/discipline/phases/` when that is unset); or a directory -- and the
phase it starts in with `starting_phase`. To make your own, copy these files
there and edit them. A document's file is named for its phase.

A regimen that runs in phases declares self-capture on, explicitly: it is how
the model proposes a move, and whether `update_record` is used unprompted is
part of what a phased run shows. Nothing turns it on by default.

```toml
phase_library = "starter"
starting_phase = "orient"
self_capture = "on"
```

The ground rules are model-facing, approved by the maintainer as a starting
point (2026-10-11).
