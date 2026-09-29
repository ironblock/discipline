# The parity fire, generalised (#143, I5)

**What this is.** The instrument for admission's third result: a parity fire of `extraction-acceptance-inverts` on a rung.

Planning answered Q-P1 with the plan's D7 (a):
1. Fire the extraction instrument once on the rung: the rung in seat A, the pinned 1.7B instrument in seat B.
2. Derive a band from that fire's tallies.
3. Fire again, and require the second fire inside the band.

#115's `band.py` and `apply.py` could not do that. They carried #115's pins and constants in their source, and their word rule assumed a positive band (N1). This directory holds them generalised, as N10 and N1 ask.

## band.py

`band.py MANIFEST ROW_DIR` derives the band from a fire's tallies.

- **The method is #115's, unchanged:** per fork, offered and accepted through the row's own `grade.py`, accepted capped at offered. The tallies are pooled per seat, seat B minus seat A, with the forks paired by (turn, step). The band is a paired percentile bootstrap.
- **What the manifest carries:** its `"band"` object names the artefacts, each by sha256 (`grade.py` and both seats' `events.jsonl` required; `report.json` when the row has one, whose stated tallies must then agree), plus the level, the resamples and the seed.
- **Refusals (exit 2):**
  - a row whose bytes are not the manifest's;
  - a manifest that pins too little;
  - tallies the report does not state.

## apply.py

`apply.py CONFIG ARCHIVED_DIR BAND_JSON FIRE_DIR BOX_JSON` writes the word.

- **The config:** its `"apply"` object carries what #115's applier held in its source:
  - the reference row's artefacts by sha256, and the band's sha256;
  - the arms and the model id;
  - the planned interview forks;
  - the disclosure;
  - **the seat-A floor.**
- **The unadjudicated checks:** #115's box, logs, routing, forks and offered checks, unchanged.
- **The generalised word rule (N1):** the reference sign is the sign of the band's point estimate.
  - Within the band: `supported`.
  - Outside it, with the reference sign kept: `inconclusive`.
  - Outside it, with the opposite sign or zero: `refuted`.
  - **A band straddling zero has no reference sign.** Outside it either way is `inconclusive`, and the fire cannot refute; a pre-registration using such a band must say so.
  - For #115's band, which is positive, this is exactly #115's rule.
- **The seat-A floor (N1's degenerate case).** A rung whose seat A accepts almost nothing gives a stable effect equal to seat B's rate, and would pass trivially. Seat A's accepted-and-deduped count must reach the config's floor in the fire and in the band's own fire, or the word is `unadjudicated`. The floor is the pre-registration's to state; D7 proposes 10, against #115's archived 18. #115's config sets 0, which reproduces #115.

## The parity fixtures

`configs/extraction-acceptance-inverts-115.json` is #115's manifest and config. With it:
- `band.py` over `results/2026-09-25-extraction-acceptance-parity/archived` reproduces that record's `band.json` **byte for byte**;
- `apply.py` over the record's archived row, band, re-fired seats and `box.json` reproduces its `verdict.json` **byte for byte**;
- #115's 34 fixtures pass through the generalised applier.

## Tests

- **`bash selftest.sh`** runs:
  - both parities and #115's fixtures;
  - 11 fixtures on the generalised rule: a negative band five ways, a straddling band three ways, and the seat-A floor in the fire, in the band's own fire, and exactly at it;
  - band.py's three refusals, and that it reads its seed from the manifest;
  - apply.py's refusal of a band that is not the pinned bytes, and that it reads its floor from the config.
- **`python3 mutants.py`** seeds 22 mutations into `band.py` and `apply.py`. The selftest must exit 1 on each, and **all 22 are killed**. Among them:
  - N1's two named faults: `decide()` restored to `effect <= 0`, and the floor check removed (in the fire, and in the band's fire);
  - a straddling band allowed to refute; the reference sign inverted; the band's edge excluded; the config's floor ignored;
  - #115's checks each dropped: box, canary, instance, seat-B think block, arm, routing, forks, interviews, offered, disclosure, band digest;
  - band.py's pin check, seed, stated-tally check and required pins.

  #115's original 19 mutants were never committed (m2). These are re-derived against the generalised code, and committed.

## Use

A rung's parity fire, per D7 (a):

```
band.py first-fire.json FIRST_FIRE_ROW > band.json                             # the band from the first fire
apply.py second-fire.json FIRST_FIRE_ROW band.json SECOND_FIRE_DIR box.json   # the second fire against it
```

- **The first fire's point:** reported beside the word against #115's band, the ladder's first capability-axis datum. Nothing reads it.
- **What the pre-registration states** (a claim, so planning writes it and the maintainer ratifies): the floor, the band's miss rate estimated by simulation over the first fire's tallies, and whether the band straddles zero.
