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

A machine may also have no substrate at all. `mac-pro-2019` has none — nothing
in this program serves a model on it — and it is registered as a host with an
empty shelf rather than given an invented one.

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

Both name their instance in the record's start row — the substrate's `hardware`
string — and both carry a `known_defects` entry saying the instance is an
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
