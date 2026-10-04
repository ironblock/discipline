# The test-equipment registry

Public prose in this repository names hardware by a **registry id**, never by a
hostname or a household phrase. This directory is where those ids resolve.
`registry.toml` is the machine-readable copy; this file says what the ids mean,
how to use them, and where a line runs that this file once crossed.

It is a **manifest of the hardware available for testing, in detail** — exact
parts, memory configuration and channels, interconnect, storage, cooling,
operating system, engine versions — not a set of class descriptions. That is the
ruling on #52, and the reason is diagnostic: "a 24 GiB Ampere-class card" cannot
tell a platform-specific finding from a systemic one, and the specifications are
what a reader needs to reproduce either.

**The excluded set is identity, and only identity**: serial numbers, MAC
addresses, hostnames, physical location. A product name is a specification. The
problem this registry fixes was referents with no referent, not the word "Mac
Pro".

## Machines and substrates are different things

A **machine** is a physical computer. It gets a possessive display name, in the
form `<github username>'s <name of thing>`, and an id.

A **substrate** is a served model on a machine: an engine, a set of weights, a
sampler card, and a serving configuration. It gets an id and references a
machine.

The relation is many-to-one, and it has to be. This program's extraction bakeoff
compares a 27B model on the accelerator against a 1.7B model on the same host's
CPU cores against an encoder that runs no server at all. The first two are **one
machine**: same hardware, different weights and different placement. A registry
keyed by machine could not record that experiment. A registry keyed by substrate
can.

A machine may also serve nothing. Nothing in this program serves a model on
`mac-pro-2019`; what it holds are the sense bakeoff's four encoder cells, which
run on its CPU cores with no server and are registered as substrates because a
results directory consumes their caches and its record names what produced
them. A substrate is what a result ran on, served or not.

And a piece of equipment may not be a machine. The table is `[equipment.*]`,
not `[machine.*]`, because `canned-loopback` — `diet-drive`'s own server,
replaying a committed set of acts byte for byte — is equipment with no hardware.
Its identity is exactly two artifacts, what serves and what it plays, and those
are its declared fields. It exists so that the field defined as a registry
digest never has to hold a sentinel string; #68 carried `"canned-loopback"`
there as a placeholder, and the ruling was that no magic string sits in an
identity field, anywhere.

## Instances

An id is stable. What it points at is not: an operating system gets updated, a
part gets replaced, a serving flag changes. So each substrate id holds a list of
**instances**, each pinned by a dated capture, and:

> **A changed deployment, a replaced part, or a changed serving configuration is
> a new instance. It is never an edit of the old one.**

Editing an instance in place silently re-dates every fire that referenced it.
That is not hypothetical here: both results directories on their way into this
repository were fired against earlier deployments than the one the registry's
current entry describes.

The stable id is what a reader shops for. **The instance is what parity matches.**

An instance is a statement about a moment, and `current = true` is a statement
about today: as of 2026-09-07 the accelerator host has a newer image staged and
unbooted, which will mint a new instance the next time it reboots.

### What an instance is not

The line, ruled on #52: **the registry records what the machine *is* — deployment
checksum, kernel, engine and weights digests, hardware. The regimen records how
it was *run*.**

An earlier version of this file crossed it. The archive holds two captures,
2026-08-13 and 2026-08-16, that agree on every field an instance carries and
differ in the server's parallel-slot count, and this file made them two
instances and called the pair "the measured proof" that an instance must key on
serving configuration. The slot count is how the machine was run. They are one
instance, keyed on the deployment, and the file says so where the second one
used to be.

The measurement was not wrong, only mis-filed: the archive does record the slot
count changing under a fixed deployment. That is a fact about a run, and it
belongs to the regimen that declares it.

## How a record references equipment

    substrate = { id = "accel24-beellama-qwen27b-q4kxl", instance = "2026-07-25" }

That shape is **ruled but not yet buildable**, and this directory ships ahead of
it deliberately, so that the ids exist before the schema that validates them.
Two consumers are needed, and both live in directories this seat does not own:

1. `scripts/check-results.py` types `regime.substrate` as `str`, so a table is
   refused today.
2. `diet check-record` must refuse a reference to an id or an instance this
   registry does not hold. A reference that resolves to nothing is worse than a
   nickname, because it reads as rigour.

Until then the two results directories on their way in do something weaker, and
they do two *different* weaker things, which is worth stating exactly:

Both name their instance in the report's Test section — the record's start row
declares the substrate by this registry's id, and the schema has no instance
field yet — and both carry a `known_defects` entry saying the instance is an
**inference**, because neither fire has a substrate capture of its own:

- `results/2026-07-29-confabulation-on-nulls` is instance `2026-07-25`, inferred
  from bracketing: two captures before the fire and three after it, on
  2026-08-03, all on the same deployment and kernel.
- `results/2026-08-10-extraction-acceptance-inverts` is instance `2026-08-13`,
  inferred from that capture's uptime, which places its boot before the fire and
  uninterrupted since, corroborated by the archive dating the deployment into
  service on 2026-08-07.

Both are the same shape of claim and both say so, which is the point: **a fire
with no capture of its own gets an inferred instance with its basis stated, not
a bare id and not a shrug.** Neither directory exists on this branch; both are
open elsewhere and merge separately.

## What is measured, and what is not

Every field in `registry.toml` is measured on the machine or read out of a
committed record, and each block says which and when. Four qualifications are
carried in the file rather than left to be discovered:

- **No capture pins the engine.** All four substrate captures carry an empty
  source-commit field and none has a binary-digest field at all. The engine
  digest that exists was produced by a separate act of measurement on
  2026-09-05, on the deployment the `2026-09-04` instance describes, and it
  attaches there and not to the three earlier instances — which per #52 must
  carry their own digest rather than inherit this one. The lesson is a
  recommendation, not a repair: **a substrate capture should record the serving
  binary's digest**, because nothing else in the capture identifies it.
- **The laptop's core counts are an inference** from the owner's description and
  the public SKU list, flagged `core_counts_inferred = true`. Its operating
  system, memory limits, engine and sampler card are *not* inferences — they are
  in a committed manifest recorded on that machine, which an earlier version of
  this entry failed to read and wrongly reported as absent.
- **`apple32-omlx-gemma12b` is unpinned for exactly one reason: no digests.**
  Its manifest records model ids, quantization and size on disk and states that
  file hashes were not computed. Everything else the shape asks for is there.
- **Two specifications the #52 amendment names are missing** from `linux-pc`:
  the power supply and the NVMe firmware revisions. Listed by name in the entry,
  so a reader diagnosing power derating or storage behaviour knows the manifest
  cannot help yet.
- **`os_deployment_digest` is the base-checksum**, the upstream base image,
  which does not change when packages are layered locally. The deployment's own
  checksum is the more specific identity and no capture records it. Read on the
  machine for the current instance and carried there; the earlier instances have
  what their captures took, and the field says so rather than implying more.

## The hardware fingerprint

`substrate.hardware_fingerprint` is required by the record schema. This registry
supplies it as **a digest over the entry's declared hardware fields** — not a
captured value, so any reader recomputes it from published data and it means
exactly *this machine, as specified*.

The declaration lives per **entry type**, in `[entry_type.*]`, so an Apple
entry's `chip` and an accelerator host's `board` are both covered by name. Two
exclusions are rules rather than habits, and both were found by computing the
thing rather than designing it:

- **`os` and anything naming a deployment are excluded.** They are the instance.
  A hardware fingerprint that changes on an operating-system update is not one.
- **Any field marked `_inferred = true` is excluded.** The laptop's core counts
  are the live case. An unverified claim baked into an identifier is one nobody
  can correct later without changing the identity of every record citing it.

**The coverage rule has a check, because a convention would not have caught the
thing that made it necessary.** The first version used one fixed field list for
every entry and silently covered *two of twelve* fields on the Apple machines —
producing a sixty-four-character digest that looked exactly like a working one.
So: every field a type declares must be present, the digest is over exactly
those, and a fingerprint covering fewer fields than declared **fails**.

    python3 substrates/check-fingerprints.py

Seen catching each of its four rules — a declared field removed, a digit flipped
in a digest, an inferred field added to a type's list, an instance field added
to one — plus an undeclared type, an empty declaration, and an unreadable
registry at exit 2.

**It runs in `verify.sh`'s `admission` check** (#202), before the admission derivation, with its own `--selftest` first.

**The engine has a fingerprint too (#202).** `engine_identity` used to be the sha256 of the server executable alone. A llama.cpp build's executable is a stub of about 18 KB, and its engine is the shared objects beside it. So every substrate whose `engine_identity` is one digest also carries exactly one of these:

- `engine_libraries` (basename to sha256), with `engine_fingerprint` over the exe and every library, and `engine_libraries_read` set to `process` or `disk`;
- `engine_libraries_unreadable`, giving the reason;
- `engine_single_digest_suffices`, giving the reason, stated as measured.

The recipe hashes every shared object in the executable's own directory. It is read on the host with one of:

    python3 substrates/check-fingerprints.py --read-engine <path>
    python3 substrates/check-fingerprints.py --read-engine-pid <pid>

A process read refuses a library replaced or rewritten since load, or mapped from outside the directory and the system's. The gate checks the registry, that its fingerprints recompute; whether a host still runs what it pins is the read, taken on the host.

It is seen red in three cases:

- a library digest changes while the exe digest holds;
- the recipe is narrowed back to the exe;
- its shared-object pattern loses versioned names.

The reads behind the current fields, and how each is tied to its instance, are in `measurements/2026-10-01-engine-libraries/`.

## The interconnect, as a worked example of being wrong twice

The accelerator's link is **PCIe 4.0 x16**. That is measured, by
`nvidia-smi --query-gpu=pcie.link.gen.max,…` returning `4, 1, 16, 16` on the
machine.

Two earlier statements about it were wrong, in opposite directions, and the
entry now carries only the reading:

1. A comment on #52 published "8 GT/s in 16 GT/s-capable slots" as a class fact.
   That was a sampled rate read as a ceiling: `gen.current` is 1 at idle and
   ramps under load.
2. The first version of *this file* then said the archived fingerprints had
   "caught the card mid-ramp". They had not caught the card at all. The capture
   instrument records six hard-coded PCI addresses, and **nothing in the archive
   identifies which — if any — is the accelerator**, so those `sta=8GT/s`
   readings cannot be attributed to this card either way.

The second error is the more instructive: it was an explanation invented to
reconcile two numbers, in the entry held up as the file's example of care.

## Cooling is a specification here, not a decoration

`linux-pc` is under a custom water loop, and the manifest describes it for one
reason: **a stock air-cooled card of the same model throttles under sustained
decode and this one does not.** Absolute tokens-per-second measured on that
machine are not portable to a stock card; only within-machine comparisons are.
Any result citing a substrate on that machine inherits that limit.

The same entry says plainly that the loop is **not** reproducible from a parts
list — two of its blocks are for other boards, one mounted on a custom
3D-printed adapter. Everything else in the entry is orderable. A manifest that
described the loop without saying so would be inviting a reader to reproduce
something they cannot.

## Pruned models, and a model that is not a candidate

**A pruned model's entry declares its calibration mix, and any claim run on one declares its domain, because a general-corpus number would hide both sides** (planning, #143, comment 5922544337).

- An entry marked `pruned = true` must carry `calibration_mix` as a non-empty string, and `pruned` is a boolean where present. `check-fingerprints.py` refuses either violation, and runs in the `admission` check.
- Nothing restricts `calibration_mix` on an entry that is not pruned: an imatrix quant has a calibration corpus too.

**Coder is not a candidate for any rung.** Planning's reasons, against Q8_0:
- code KLD is at parity;
- prose KLD is +84%;
- top-1 is 7.7 points lower.

An agent's reasoning traces are prose. Q2_0 remains the top rung. The 7.7 GB that Coder would free is the subject of a separate claim (placement against pruning at equal VRAM), not a reason to serve it. Coder has no entry here, because nothing was fired on it.

## Registering your own box

**A registered box is a declared fact, not an admitted rung.** An entry says what the box is. It does not say that results on it are comparable to anyone else's; that is #143's admission (the substrate ladder: which substrates are admitted as rungs whose results may be compared), which is separate work.

**The registry is compiled into the binary.** `diet/src/drive/registry.rs` reads `substrates/registry.toml` with `include_str!`. `diet-drive serve --regimen` therefore refuses an id it does not find, and it names the id: "`<id>` is not a substrate in the registry". To drive under a regimen on your own server:

1. add an equipment entry and a substrate entry to `substrates/registry.toml`;
2. rebuild: `cargo build -p discipline-diet --bin diet-drive`, from the repository root;
3. run `python3 substrates/check-fingerprints.py` until it reports no problems.

`serve` without `--regimen` needs none of this. It starts against any endpoint and announces no substrate.

### The equipment entry: `[equipment.<id>]`

- **`entry_type`**, one the registry declares for real hardware: `accelerator-host`, `x86-workstation` or `apple-silicon-laptop`. (`canned-server` is diet-drive's own loopback server, not a machine.) Each type's `hardware_fields` are listed under `[entry_type.<type>]`; if your machine needs a type none of these fits, that is a new `[entry_type.*]` with its own field list.
- **Every field its type declares**, as measured on the machine: the parts, memory and accelerator.
  - No hostname, serial number, MAC address or location appears anywhere. Those are identity (the ruling on #52, which set what this registry records and what it leaves out), and keeping them out is a rule reviewers hold you to; the hygiene gate catches only some shapes of them.
  - A value you could not measure stays out of the identity. Record it on the entry with `<field>_inferred = true` only if the field is not one the type declares: `check-fingerprints.py` refuses an inferred field the type declares (an unverified claim cannot be part of an identifier). The laptop's core counts are the live case: recorded, inferred, and not in its type's `hardware_fields`.
- **`hardware_fingerprint`**: the sha256 of exactly the declared fields, canonically serialised. `check-fingerprints.py` recomputes it and prints the digest it wants. The digest is also what a regimen's `substrate_hardware` must equal.

### The substrate entry: `[substrate.<id>]`

| field | what it holds | how it is measured |
| --- | --- | --- |
| `equipment` | the equipment id above | -- |
| `engine_name` | the engine, e.g. `llama.cpp` | -- |
| `engine_identity` | the sha256 of the running server binary | `sha256sum /proc/<pid>/exe` on Linux, with the process found by exact name (`pgrep -x llama-server`) |
| `engine_libraries`, `engine_libraries_read`, `engine_fingerprint` | the shared objects beside the binary, which are where a llama.cpp build's engine lives | `python3 substrates/check-fingerprints.py --read-engine-pid <pid>` (or `--read-engine <path>` from disk), copied as it prints them; a static binary instead says why in `engine_single_digest_suffices` |
| `engine_commit` **or** `engine_build_info` | what the start-time check compares with the server's `GET /props` `build_info` | if `build_info` names a commit (`bNNNN-<hash>`), `engine_commit` is the full 40-hex commit the hash resolves to in your checkout (`git rev-parse <hash>`). If it names none, as a prebuilt release reports `b0-unknown-dirty`, `engine_build_info` is that exact literal, and the check reports `engine_identity` as unreported (literal matched) (#157, whose second increment added the start-time engine check) |
| `weights_main` | the sha256 of the model file | `sha256sum`. One file is one string; a model split into shards is a list of their digests, on one line, in the order the server loads them (#92, #211: how a record spells a set of weight files) |
| `weights_main_file`, `weights_draft` | the file's name; a draft model's digest, if the server loads one | -- |
| `serving_flags`, `serving_context`, `serving_slots` | the serving line, without paths or keys | read off the running process's command line (`/proc/<pid>/cmdline`) |
| `sampler_card` | what the line fixes, or "none on the serving line; each request sets its own" | -- |
| `chat_template_sha256` | optional: the served template's digest | from `GET /props` `chat_template` |
| `vision`, `vision_is` | whether the line takes an image: `accepted`, `refused`, `answered-without-seeing` or `unreported`, and the cell that says so (#373) | the vision cell under the line's admission fingerprint directory (`substrates/admission/<substrate>/<fp>/vision/`), whose `recompute.sh` re-derives the word; anything but `unreported` needs that cell, and the admission check refuses a word without one |

**Instances.** One instance, with the date of its reads and `current = true`, says what deployment pinned the entry (see Instances above). A later change to the operating system, engine or line is a new instance, never an edit.

A regimen naming your substrate then binds the session to these declared facts:
- the substrate id;
- `substrate_hardware`, the equipment's fingerprint;
- the server's `build_info`, against `engine_commit` or `engine_build_info`.

`serve` refuses at start if any of these disagrees, and names the field.

## Still to be registered

Named in a committed record, not yet registry entries:

| named as | what is on the record | what is missing |
| --- | --- | --- |
| the extraction bakeoff's seat B, on the host's CPU cores | a 1.7B model at Q4_K_M | the model family, the engine, and digests |
| the extraction bakeoff's seat C, an encoder run offline | `urchade/gliner_small-v2.1` | a pinned revision and file digests; it serves nothing, so it may want a shape of its own |

Both are referenced in prose in `results/2026-08-10-extraction-acceptance-inverts`,
which is open on another branch. That directory's `known_defects` records that
the run's regime can name only one substrate and that the other two are carried
in prose — which is the gap these two rows would close.

## For the gate

There is no exemption request here, and an earlier version of this file made
one. It asked for `substrates/` to be excluded from the hygiene gate on the
grounds that a nickname pattern class would fire on the sanctioned display
names. The ruling on #52 already answers that: **a product name is a
specification, not a nickname**, and the excluded set is serial numbers, MAC
addresses, hostnames and physical location — none of which is in this directory.

If a future pattern does fire on a `display_name`, the fix is to narrow that
pattern, not to remove from the gate the one directory that holds the most
machine detail in the repository.
