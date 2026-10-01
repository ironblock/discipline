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
  - a manifest that pins too little (either seat's log, or `grade.py`), or whose level or resamples is out of range;
  - a pinned report that leaves out either seat's tallies, or states tallies the logs do not sum to.

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
  - **A band strictly straddling zero (`lo < 0 < hi`) has no reference sign.** Outside it either way is `inconclusive`, and the fire cannot refute; a pre-registration using such a band must say so. A band with an edge at zero keeps its point's sign, as #115's rule reads it.
  - For #115's band, which is positive, this is exactly #115's rule.
- **The seat-A floor (N1's degenerate case).** A rung whose seat A accepts almost nothing gives a stable effect equal to seat B's rate, and would pass trivially. Seat A's accepted-and-deduped count must reach the config's floor in the fire and in the band's own fire, or the word is `unadjudicated`. The floor is the pre-registration's to state; D7 proposes 10, against #115's archived 18. #115's config sets 0, which reproduces #115.

## A counted-fork floor

A config may carry `counted_fork_floor`, as the candidate's pre-registration does (planning, #143, comments 5894139110 and 5921525110). Below the floor of 25 counted extraction forks of 31, the fire is `unadjudicated`. Under paired pooling the count is the intersection's.

- **Without a floor** (#115's config, `null`): every archived extraction fork must be present and routed. This is #115's rule, and it keeps #115's byte parity.
- **With a floor:** an extraction fork that is missing (a timeout leaves no event) or whose routing fails is not counted, and the seat is `unadjudicated` only below the floor. The misses and their reasons are reported per seat in `counted_forks`, whatever the word. The effect is pooled over each seat's counted forks. The interview forks, the logs, the box record and the offered check stay as #115 had them.
- **Planning's rulings on the pre-registration's gaps (#143, comment 5921525110), as config:**
  - **`pool`.** `"per-seat"` pools each seat over its own counted forks, which is #115's form. `"paired"` pools both seats over the intersection of the forks both counted, which keeps the archived band's pairing. Under `"paired"` the counted-fork floor applies to the intersection alone. `counted_forks` reports each seat's counted total and the intersection's size (`paired`).
  - **`interview_miss`.** `"fail"`, #115's rule, makes a missing interview fork fail the seat. Under `"byte-match"` an interview miss is reported in `interview_misses` and is not a word by itself. The fire is void (`unadjudicated`) only if **a planned fork ran with a request other than the planned one**:
    - **The plan.** `planned_requests` names a file pinned by digest. It keys each of the 87 planned forks per seat by `lane|turn|step|ask`, with the sha256 of its planned request over lane, parent turn, messages and params. A key the fire ran more than once must match on every copy.
    - **Where it comes from.** The plan is the offline rehearsal's, passed through the export #115's re-fired logs passed: the private alias map, **then home prefixes collapsed to `~`**. The fire's logs must pass the same two steps before grading, or every request carrying a path reads as changed.
    - **What is not a change.** A planned fork that did not run is a miss. A fork the plan does not name, such as the replay's extra interviews, is ignored.
    - **What a miss can do.** A miss that changed a later prompt fails the match. A miss that left every later request byte-identical changed nothing the effect depends on.
    - **`apply.py --check-plan CONFIG FIRE_DIR`** checks a fire's logs against the plan. Over both of #115's fires, every planned fork ran with its planned request.
    - A mis-routed fork still makes the seat `unadjudicated`.
  - **Harness stops, timeouts and refusals, as they fall.**
    - A harness stop (a 5xx, a repeated 4xx, a connection failure) ends the replay with no `run.end`, so that seat is `unadjudicated`.
    - A timeout leaves a fork missing.
    - A content refusal is a counted fork.
    - The pinned harness's per-fork timeout is stated by value as `fork_timeout_s`: 2400 s, both arms' `turn_timeout_s` at `0e14292`. The applier does not apply it.
  - **`report_refire_interval`.** It reports the fire's own interval: `band.py`'s method (its level, resamples and seed) over the paired counted forks, written with `--interval-out PATH` as a number beside the word, never in `verdict.json` and never as a word. Its one sentence is that it straddles zero, which bears on the sign rule. The band-edge sentence is withdrawn. Over #115's own re-fire it reads [0.022893, 0.19021], which does not straddle zero.
  - #115's `verdict.json` stays byte-identical with `pool` and the interval switched on, and the selftest checks this.
- **`configs/extraction-acceptance-inverts-candidate.json`** is the candidate's config:
  - from #115: the pins, band digest, arms, interviews and disclosure;
  - for the candidate: its model id;
  - as ruled: a floor of 25 on the intersection, **a seat-A floor of 10**, `pool = "paired"`, the interval on, and `interview_miss = "byte-match"` with `configs/candidate-planned-requests.json` pinned.
  - The archived band [0.024877, 0.21789] does not straddle zero, so `refuted` is reachable.

## The parity fixtures

`configs/extraction-acceptance-inverts-115.json` is #115's manifest and config. With it:
- `band.py` over `results/2026-09-25-extraction-acceptance-parity/archived` reproduces that record's `band.json` **byte for byte**;
- `apply.py` over the record's archived row, band, re-fired seats and `box.json` reproduces its `verdict.json` **byte for byte**;
- #115's 34 fixtures pass through the generalised applier.

## Tests

- **`bash selftest.sh`** runs:
  - both parities and #115's fixtures;
  - 15 fixtures on the generalised rule: a negative band five ways, a straddling band three ways, a band touching zero from each side, a point of zero, and the seat-A floor in the fire, in the band's own fire, and exactly at it;
  - band.py's seven refusals (altered tallies; a thin manifest two ways; a level out of range; a report leaving out seat B; a report disagreeing, for each seat), and that it reads its seed from the manifest;
  - apply.py's refusals of a band that is not the pinned bytes and of a config with a negative floor, bad arms or a malformed digest, and that it reads its floor from the config.
- **`python3 mutants.py`** seeds 48 mutations into `band.py` and `apply.py`. The selftest must exit 1 on each, and **all 48 are killed**. Among them:
  - N1's two named faults: `decide()` restored to `effect <= 0`, and the floor check removed (in the fire, and in the band's fire);
  - planning's rulings: the per-seat floor under paired pooling; a changed planned request not voiding; an interview miss failing the seat; the plan's pin unchecked; a planned fork that did not run read as changed; the request digest blind to the messages; the band-edge withdrawal undone; the interval never straddling zero;
  - a straddling band allowed to refute; the reference sign inverted; the band's edge excluded; the config's floor ignored;
  - #115's checks each dropped: box, canary, instance, seat-B think block, arm, routing, forks, interviews, offered, disclosure, band digest;
  - band.py's pin check, seed, stated-tally check (each seat), required pins, range check, and its refusal of a report missing a seat;
  - a band touching zero read as straddling; the point-of-zero guard removed; the config's own checks dropped;
  - the counted-fork floor: its check removed, a missing fork not listed, a mis-routed fork failing the seat, the config's floor ignored, a floor above the fork count accepted, an interview failure folded into misses, duplicate or foreign keys allowed;
  - paired pooling ignored; the fire's interval never straddling, or drawn with another seed.

  #115's original 19 mutants were never committed (m2). These are re-derived against the generalised code, and committed.

## Use

A rung's parity fire, per D7 (a):

```
band.py first-fire.json FIRST_FIRE_ROW > band.json                             # the band from the first fire
apply.py second-fire.json FIRST_FIRE_ROW band.json SECOND_FIRE_DIR box.json   # the second fire against it
```

- **The first fire's point:** reported beside the word against #115's band, the ladder's first capability-axis datum. Nothing reads it.
- **What the pre-registration states** (a claim, so planning writes it and the maintainer ratifies): the floor, the band's miss rate estimated by simulation over the first fire's tallies, and whether the band straddles zero.
