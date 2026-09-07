# The test-equipment registry

Public prose in this repository names hardware by a **registry id**, never by a
nickname, a hostname, or a household phrase. This directory is where those ids
resolve. `registry.toml` is the machine-readable copy; this file says what the
ids mean and how to use them.

The point is not tidiness. A nickname identifies a person's equipment; a
registry entry describes a **class** of equipment a reader could go and buy, and
pins the exact one a number came off. "A 24 GiB Ampere-class card plus an
Apple-silicon laptop" is a shopping list. "The box" is not.

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
That is not hypothetical here: the two results directories in this repository
were both fired against an earlier deployment than the one the registry's current
entry describes, and reducing them to a bare id would have quietly moved a July
fire onto September software.

The stable id is what a reader shops for. **The instance is what parity matches.**

An instance is not the operating system. `2026-08-13` and `2026-08-16` are the
same deployment, the same kernel, the same engine and the same weights — and
different substrates, because the server's parallel-slot count went from 1 to 2
between them, which changes how requests share the cache. That pair is the
measured proof that an instance keyed on the operating system alone would be
wrong.

## How a record references equipment

    substrate = { id = "accel24-beellama-qwen27b-q4kxl", instance = "2026-07-25" }

That shape is **ruled but not yet buildable**, and this directory ships ahead of
it deliberately, so that the ids exist before the schema that validates them.
Two consumers are needed, and both live in directories this seat does not own:

1. `scripts/check-results.py` types `regime.substrate` as `str`, so a table is
   refused today. Until it accepts one, the two results directories carry the id
   as a string and pin the instance in prose beside it, which is a weaker record
   and is marked as such in their own front matter.
2. `diet check-record` must refuse a reference to an id or an instance this
   registry does not hold. A reference that resolves to nothing is worse than a
   nickname, because it reads as rigour.

## What is measured, and what is not

Every field in `registry.toml` is measured on the machine or read out of a
committed substrate capture, and each block says which and when. Three
qualifications are carried in the file rather than left to be discovered:

- **The engine is pinned by digest only from `2026-09-04` onward.** The earlier
  captures carry an empty `source_commit`, so the three older instances have an
  unpinned engine. They say so. The lesson is a recommendation, not a repair:
  a substrate capture should record the serving binary's digest, because
  nothing else in the capture identifies it.
- **The laptop entry is a placeholder.** Nobody with access has run a readout.
  Its core counts are an inference from the owner's description and the public
  SKU list, flagged `inferred = true` in the file, and should be confirmed
  before anything leans on them.
- **`apple32-omlx-gemma12b` is a name with a machine behind it and nothing
  else.** It exists because the floor-suite regimen arms reference that
  substrate and had nothing to reference before.

## The interconnect, as a worked example of why this file is careful

The accelerator's archived fingerprints record `sta=8GT/s` on the card's link.
Read as a class fact, that says PCIe 3.0 and sends a reader shopping for the
wrong slot. It is not a class fact. The card idles at gen 1 in pstate P8 and
ramps under load; `pcie.link.gen.max` is 4 and `pcie.link.width.max` is 16. Both
archived captures happened to sample it mid-ramp.

The registry states **PCIe 4.0 x16**, and states that a sampled rate is an
artefact of when the sample was taken, because the first version of this entry
published the interpretation instead of the measurement and was wrong by two
generations.

## Cooling is a class fact here, not a decoration

`linux-pc` is under a custom water loop, and the registry describes it for one
reason: **a stock air-cooled card of the same model throttles under sustained
decode and this one does not.** Absolute tokens-per-second measured on that
machine are therefore not portable to a stock card; only within-machine
comparisons are. Any result citing this substrate inherits that limit.

The same entry says plainly that the loop is **not** reproducible from a parts
list — two of its blocks are for other boards, one mounted on a custom 3D-printed
adapter. A registry that described the loop without saying so would be inviting a
reader to reproduce something they cannot.

## Still to be registered

Named in committed records, not yet registry entries, because nothing measured
is available to fill them:

| named as | what is missing |
| --- | --- |
| the extraction bakeoff's seat B, a 1.7B model on the host's CPU cores | model family, quantization, engine, digests |
| the extraction bakeoff's seat C, an encoder running offline | the encoder's revision and digests; it serves nothing, so it may want a shape of its own |

Both are referenced in prose in `results/2026-08-10-extraction-acceptance-inverts`
and neither has an id. That directory's own `known_defects` says so.

## For the gate

The hygiene gate is asked, on #52, to grow a pattern class for the nicknames
that have already leaked. **This directory must be exempt from it.** The
sanctioned display names contain the product names the class would ban — that is
what makes them readable — so the registry is the one place they are written
down, exactly as `scripts/hygiene-patterns.tsv` is the one place forbidden shapes
are written down.
