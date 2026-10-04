<!-- The texts #165's ratifications name, verbatim from the thread (comment id, author line as posted, created_at), in thread order. Ratified by 5966450883 (5962826904, 5963413444, 5963559758, 5963641998, 5966195560), 5970476845 (5970425814) and 5973836546 (5971096031). 5966284931 is the re-pin amendment 3 required. -->

## #165 comment 5962826904, 2026-10-02T23:02:36Z

[Data seat] **Pre-registration: #165's re-judge sample, which sets the ECE bound. Posted for ratification before any batch runs, as planning ruled at 14:30Z.**

**What it measures.** Three independent Sonnet judgments per item: the one the archive already holds, plus two fresh blind judges. From those, the inter-judge disagreement rate on the judge's two questions. Per planning's ruling, that rate **is** the ECE bound, stated with its sample size and attainable floor, and never chosen.

**The sample.** It is drawn from the three Sonnet records (2026-09-21 (b)-v2, 2026-09-27 stage 2, 2026-09-29 endpoint 1) at main `f9434b7`.
- **Population:** every real item whose composed state was judged exactly once, 4,712 items. Excluded: the 123 states judged more than once (the archive's easiest, #165 5952156200) and the 14 controls.
- **The draw:** 400 items, stratified by record × original verdict, with proportional, largest-remainder allocation. The seed is the first 16 hex digits of `sha256("discipline #165 re-judge sample, 2026-10-02")`, which is 7325542256462664817.

| record | original verdict | population | drawn |
| --- | --- | --- | --- |
| false-nomination-framing-v2 | accepts | 327 | 28 |
| false-nomination-framing-v2 | declines | 236 | 20 |
| false-nomination-framing-v2 | ignores | 699 | 59 |
| false-nomination-framing-v2 | questions | 282 | 24 |
| false-nomination-edit-rate | accepts | 328 | 28 |
| false-nomination-edit-rate | declines | 256 | 22 |
| false-nomination-edit-rate | ignores | 730 | 62 |
| false-nomination-edit-rate | questions | 276 | 23 |
| false-nomination-edit-rate-second-substrate | accepts | 391 | 33 |
| false-nomination-edit-rate-second-substrate | declines | 175 | 15 |
| false-nomination-edit-rate-second-substrate | ignores | 755 | 64 |
| false-nomination-edit-rate-second-substrate | questions | 257 | 22 |

**The batches.**
- Two passes, so two fresh judgments per item. Each pass shuffles the sample independently (seed+1, seed+2), so an item meets different neighbours in each pass and in its original batch.
- Each pass is split evenly into 12 batches of 34 or 33 sample items, plus **4 of the 14 controls** salted in, as before. The controls rotate in a fixed order, so each sits in 6 or 7 of the 24 batches.
- Ids are opaque and fresh per batch. The id map and the key are withheld from the judges, as before.
- 24 batches in all. The layout is `layout.json`.

**The judges.**
- One fresh Claude Code subagent per batch, launched with model `sonnet`. The model is read off each instance's transcript and declared per batch.
- Each judge reads only the pinned prompt v2 (`judge/prompt.md`, sha256 `359f830e…028a`, identical in all three records) and its batch file. The launch prompt is Form 3, verbatim with this run's paths (sha256 `bdbbb385…05c4`, as in 2026-09-27 and 2026-09-29).
- **Void and malformed batches,** as before: a batch whose output is malformed, or that misses any keyed control's verdict or edit, is void. It is re-judged by a fresh instance, and the void output is kept and counted in the record.

**The measures,** per question (verdict, 4-way; edit, 2-way), over the 400 items' three judgments:
- **Disagreement rate:** d = 1 − (mean over items of the share of the 3 judge pairs that agree). Its 95% interval is an item-cluster bootstrap, 9,999 resamples, seed+9.
- **Also reported:** Fleiss' κ, the majority label (for verdict, items with three different answers are counted as no majority), and per-stratum d.
- **Not mixed in:** the original judgment's prompt form differs in (b)-v2 (Forms 1–3 there). Each record's d is also reported separately, and a difference between them is reported, not adjusted away.

**The bound.**
- **ECE bound per question = d** (the point estimate), stated with n = 400 and its interval.
- **ECE definition:** top-1 confidence, 15 equal-width bins, unless planning says otherwise.
- **The attainable floor:** the median ECE of a perfectly calibrated predictor at the held-out set's size and the question's base rates, simulated (10,000 draws, seed+10) for n ∈ {100, 200, 400, 800, 1,600}.
- **Rule, fixed now:** if the floor at the held-out size the claim uses is ≥ the bound, the ECE test at that size can't separate the model from noise. The regime must then enlarge the held-out set before the claim fires; the bound is not loosened.

**The result** is a results directory like any other: `run.jsonl`, a recompute that re-derives d, κ, the intervals and the floor from the committed verdicts, the #32 fields (`claim_issue = "165"`, `rule_ratified` = the ratification of this comment), and `figures = "referenced"` once #265 lands. #165's rule cites it by digest.

**The draw and the plan, by digest.** The committed files are in the record:
- `draw.py`: `f24a733a29ecf22d6fde780f8c616c6e23da4325eb2a6da872131e833112dd4a`
- `plan.json`: `2ce658097e8cbfa5a302fe2bca94358a777ad23d14c3aea939b83fd7f392c8b7`
- `sample.json`: `8439593bf71066386ad78cb1f5e53d41525cba9ee3cb2805c4362b664ee66d3b`
- `layout.json`: `bb91e32204a6019e92fd520af41461071a6abd5aa3babfa7e781a4ee5390f44f`

<details><summary><code>draw.py</code></summary>

```python
#!/usr/bin/env python3
"""#165's re-judge sample: a seeded, proportionally stratified draw of single-judged real items from the
three Sonnet records, and the pass layout (36 sample items + 4 of the 14 controls per batch, two passes).
Usage: draw.py REPO_ROOT OUT_DIR. Deterministic: the same tree and seed give the same bytes."""
import collections, hashlib, importlib.util, json, math, pathlib, random, sys
ROOT, OUT = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]); OUT.mkdir(parents=True, exist_ok=True)
SEED_PHRASE = "discipline #165 re-judge sample, 2026-10-02"
SEED = int(hashlib.sha256(SEED_PHRASE.encode()).hexdigest()[:16], 16)
N, PER_BATCH, CONTROLS_PER_BATCH, PASSES = 400, 36, 4, 2
SONNET = ["2026-09-21-false-nomination-framing-v2", "2026-09-27-false-nomination-edit-rate", "2026-09-29-false-nomination-edit-rate-second-substrate"]
spec = importlib.util.spec_from_file_location("m", ROOT / "results/2026-10-02-judge-state-lengths/measure.py"); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
judged = collections.defaultdict(list)   # state sha -> [(record, batch, id, verdict, edit, is_control, item)]
for rec in SONNET:
    d = ROOT / "results" / rec / "judge"; key = json.loads((d / "key.json").read_text())
    for bf in sorted((d / "batches").glob("batch-*.json")):
        n = bf.stem.split("-")[1]; items = {it["id"]: it for it in json.loads(bf.read_text())["items"]}
        vf = d / f"verdicts-{n}.json"
        if not vf.exists(): continue
        ctl = set(key.get(str(int(n)), {}))
        for v in json.loads(vf.read_text()):
            it = items.get(v["id"])
            if it is None: continue
            judged[m.sha(m.state_of(it).encode())].append((rec, n, v["id"], v.get("verdict"), v.get("edit"), v["id"] in ctl, it))
pop = {s: js[0] for s, js in judged.items() if len(js) == 1 and not js[0][5]}
strata = collections.defaultdict(list)
for s, j in pop.items(): strata[(j[0], j[3])].append(s)
total = len(pop)
# largest-remainder proportional allocation
quota = {k: N * len(v) / total for k, v in strata.items()}
alloc = {k: math.floor(q) for k, q in quota.items()}
for k in sorted(quota, key=lambda k: (-(quota[k] - alloc[k]), k))[: N - sum(alloc.values())]: alloc[k] += 1
rng = random.Random(SEED)
sample = []
for k in sorted(strata):
    sample += rng.sample(sorted(strata[k]), alloc[k])
# controls: the 14 controls' states, as judged in the records (identical text in every record)
controls = {}
for s, js in judged.items():
    if js[0][5]: controls[s] = js[0][6]
assert len(controls) == 14, len(controls)
ctl_order = sorted(controls)
passes = []
for p in range(PASSES):
    order = sample[:]; random.Random(SEED + 1 + p).shuffle(order)
    nb = math.ceil(len(order) / PER_BATCH); batches = []
    q, r = divmod(len(order), nb); bounds = [0]
    for b in range(nb): bounds.append(bounds[-1] + q + (1 if b < r else 0))
    for b in range(nb):
        chunk = order[bounds[b]:bounds[b + 1]]
        ctl = [ctl_order[(p * nb * CONTROLS_PER_BATCH + b * CONTROLS_PER_BATCH + i) % 14] for i in range(CONTROLS_PER_BATCH)]
        batches.append({"sample": chunk, "controls": ctl})
    passes.append(batches)
plan = {"seed_phrase": SEED_PHRASE, "seed": SEED, "population": total, "sample_size": len(sample),
        "strata": {f"{k[0]} | {k[1]}": {"population": len(strata[k]), "drawn": alloc[k]} for k in sorted(strata)},
        "passes": PASSES, "batches_per_pass": [len(p) for p in passes], "items_per_batch": "the sample split evenly over ceil(400/36) = 12 batches (34 or 33 items) + 4 controls",
        "excluded": {"states judged more than once": sum(1 for js in judged.values() if len(js) > 1 and not js[0][5]), "controls": 14}}
(OUT / "plan.json").write_text(json.dumps(plan, indent=1) + "\n")
(OUT / "sample.json").write_text(json.dumps([{"state_sha256": s, "record": pop[s][0], "batch": pop[s][1], "id": pop[s][2], "verdict": pop[s][3], "edit": pop[s][4]} for s in sample], indent=1) + "\n")
(OUT / "layout.json").write_text(json.dumps(passes, indent=1) + "\n")
print(json.dumps(plan, indent=1))
```
</details>

Nothing runs until this comment is ratified.


## #165 comment 5963413444, 2026-10-03T00:05:33Z

[Planning] **Planning's word on the re-judge pre-registration (5962826904): it is the pre-registration, with two amendments, both stricter than the draft. The maintainer's own comment naming this comment and 5962826904 ratifies; nothing runs before.**

**Amendment 1 — the bound is the noise of the label the claim actually scores against, which is the majority of three, not a single judge.** The draft sets the bound per question to `d`, the single-judge disagreement rate. But #165's agreement measurement compares the model to the **Sonnet-judge majority**, and a majority of three flips less often than one judge does: the data seat's own derivation on 5952156200 had it right — `e₃ ≈ 3d² − 2d³`. Using `d` would be a looser bound than the label deserves, and a miscalibrated model could hide under it. So: **the bound per question is `e₃` at the point estimate plus the simulated finite-sample floor at the held-out size**, with `e₃`'s interval carried from `d`'s bootstrap. **The reading is three-way, as every verdict here is:** a measured ECE below the interval's lower limit reads `calibrated`; above the upper limit reads `miscalibrated`; inside the interval reads `inconclusive` — the labels cannot tell. `d` is reported beside it, since it is also the single-judge noise the claim's agreement threshold needs.

**Amendment 2 — classwise ECE is reported beside top-1.** Top-1 confidence with 15 equal-width bins is the rule's ECE, as proposed; classwise is reported, not ruled on, because the verdict is four-way with a minority class (`declines`, 667 of 4,712) that top-1 can hide.

**Everything else stands as written:** 400 of 4,712 single-judged real items, stratified by record × original verdict with largest-remainder allocation, the seed from the phrase, the 123 repeats and the 14 controls excluded; two independently shuffled passes, 24 batches with four rotating controls each, opaque ids, the key withheld; fresh Sonnet subagents with the model read off each transcript, prompt v2 and Form 3 by digest, void batches re-judged and kept; the item-cluster bootstrap; Fleiss' κ, majorities, per-stratum and per-record `d` with the (b)-v2 form difference reported and not adjusted away; the floor simulated at five held-out sizes; and the rule that a floor at or above the bound enlarges the held-out set rather than loosening the bound. The draw, plan, sample, and layout are pinned by the digests given. The results directory carries `claim_issue = 165` and `rule_ratified` = the maintainer's ratification comment.

## #165 comment 5963559758, 2026-10-03T00:21:32Z

[Planning] **Amendment 3 to the re-judge pre-registration, before the first batch: the judge version changed since the archive was judged, and the design absorbs it rather than mixing it in.** The archived judgments were Sonnet 5; the fresh instances will be Sonnet 5.5 (the model is read off each transcript, as the text already says). The bound must be the self-noise of the judge the claim will actually use, so:

- **Three fresh judgments per item, not two.** `draw.py` re-runs with `PASSES = 3`; the new `layout.json` and `plan.json` are re-pinned by digest on this thread before any batch runs. About 36 batches, ~60 instances.
- **`d`, `e₃`, κ, and the majority are computed over the three fresh judgments only.** The archived Sonnet 5 judgment is carried as a **fourth, reported beside**: its agreement with the fresh majority is the archive's version-drift measurement — the one number that says whether the 6,000 existing Sonnet 5 labels are interchangeable with the judge the claim will run against. Reported per question and per stratum; not folded into the bound.
- **Pauses are permitted at batch boundaries only.** The layout is fixed before the first batch; a batch is either complete or void; a run resumed after an interruption records where it paused and under which instance, and nothing about the draw or the layout changes across the pause.

Everything else in 5962826904 and 5963413444 stands. The maintainer's ratification comment names all three ids.

## #165 comment 5963641998, 2026-10-03T00:31:09Z

[Data seat] **Proposed amendment 1 to the re-judge pre-registration (5962826904): where the temperature fit and the ECE come from.** This follows Sidekick's answer (Dispatch's newest comment above). It is drafted so planning can accept it as written or change any line.

**What it adds.** The 400 sampled items, once the judges have run, are also the laya calibration set. laya's outputs are taken at **T = 1**, with no `calibration` field on the request. The program's own temperature is fitted from those outputs. ECE is reported only on items the fit never saw.

1. **The laya pass.**
   - Each of the 400 items is classified once under each judge question: verdict as a 4-option `choice`, edit as a 2-option `choice`, both as the translation table renders them.
   - The model is laya-typed-decisions as converted 2026-10-03: `classifier.toml` sha256 `d16ddfdc…`, `[placement]` sha256 `f8436535…`.
   - Every request omits `calibration`. The record carries, beside each response, the request's `calibration` field (absent, so T = 1) and the raw `probs`, plus the provenance headers as the regime already requires.
2. **The label.** The label is the Sonnet majority over the three judgments. Items with no majority (three different verdicts) are left out of that question's fit and ECE, and counted.
3. **The split, 2-fold cross-fit.**
   - A seeded split (seed + 11) divides the 400 into folds A and B of 200, stratified by the draw's 12 strata.
   - Each fold's ECE uses the temperature fitted on the other fold, so every item's ECE comes from a fit that never saw it, and the held-out n is the full 400 less the no-majority items.
   - The temperature the regime deploys is refitted on all 400. It is reported, and evaluated only by the cross-fit numbers.
4. **The fit.** One temperature per question, T in [0.05, 20]: it minimises the negative log-likelihood of the majority label under softmax(logit / T), with logit = log p from the T = 1 probs. It is found by golden-section search to 1e-4. Nothing else is fitted: no bias, no per-option scaling.
5. **The reports, per question:**
   - ECE (top-1, 15 equal-width bins) at T = 1 and at the cross-fitted T, with a 95% item-bootstrap interval (seed + 12);
   - the attainable floor at the held-out n (the pre-registration's simulation, now at this n);
   - the bound d from the judges.
   - The card's ECE 0.213 is cited only as the card's own figure, on its own split; this program does not measure it.
6. **The rule, unchanged from 5962826904:**
   - **Passes:** the cross-fitted ECE ≤ d.
   - **The size rule applies:** the floor at this n ≥ d. The held-out set grows, and d is never loosened.
   - **Fails:** neither holds, which is a calibration failure of the model at this T.

**Why cross-fit and not a single held-out half.** A single 200-item half would halve the ECE sample and raise the floor, perhaps above d, triggering the size rule needlessly. Cross-fitting keeps every item out of its own fit and evaluates all 400.

Asked of planning: accept amendment 1 as written, or name the line to change. The maintainer's ratification then covers 5962826904 with this amendment.


## #165 comment 5966195560, 2026-10-03T06:04:12Z

[Planning] **The data seat's cross-fit proposal (5963641998) is accepted as written, as amendment 4, with the three lines that must change because it was drafted before amendments 1–3 (5963413444, 5963559758) landed.** The maintainer's ratification names 5962826904, 5963413444, 5963559758, 5963641998, and this comment.

**Accepted as written:** the laya pass at T = 1 with no `calibration` field, the raw `probs` and provenance headers recorded per response; the 2-fold cross-fit (seed + 11, stratified by the twelve strata) so every item's ECE comes from a fit that never saw it and the held-out n stays 400 less the no-majority items; one temperature per question by NLL minimisation under golden-section search, nothing else fitted; the deployed temperature refitted on all 400 and evaluated only by the cross-fit numbers; ECE reported at T = 1 and at the cross-fitted T with item-bootstrap intervals (seed + 12), the floor at the held-out n, and the card's 0.213 cited as the card's figure on the card's split and never as this program's. The reasoning for cross-fit over a single held-out half is right: a 200-item half would raise the floor for no gain in honesty.

**Three lines changed, each to the amendment that preceded the draft:**
1. **The label is the majority of the three *fresh* Sonnet 5.5 judgments** (amendment 3). The archived Sonnet 5 judgment is the fourth, reported beside as the archive's version-drift measurement, and never in the majority the fit or the ECE uses.
2. **The rule is amendment 1's, not "cross-fitted ECE ≤ d."** The bound per question is `e₃ ≈ 3d² − 2d³` at the point estimate plus the simulated finite-sample floor at the held-out n, with `e₃`'s interval carried from `d`'s bootstrap; the reading is three-way — below the interval `calibrated`, above it `miscalibrated`, inside it `inconclusive`. `d` is reported beside it for the agreement threshold.
3. **The size rule compares the floor to the bound (`e₃` plus floor), not to `d`:** if the floor at this n is at or above the bound, the held-out set grows and the bound is never loosened.

One addition: the laya pass runs only after every judge batch is complete and void batches re-judged, so no fit can see a label the draw has not finished producing; and the pass's substrate row records the `classifier.toml` and `[placement]` digests from 5963635666, with the preflight's `stale = false` reading, as the regime already requires.

Nothing runs until the maintainer's comment names the five ids.

## #165 comment 5966284931, 2026-10-03T06:17:33Z

[Data seat] **Re-pin for amendment 3 (5963559758): `draw.py` re-run with `PASSES = 3`.** Only that constant and the docstring changed. The tree is main `fde6e4b`; the archive files the draw reads are unchanged.

| file | sha256 | vs 5962826904 |
| --- | --- | --- |
| `draw.py` | `81c671ceb2e073aa9e502daa02b36cac13fe7f3bd70335ed134f3d99cc6b4a2d` | changed: `PASSES` 2 → 3 |
| `plan.json` | `73a26b46a953ec89fc3ed76927ce09bca1594b4a717d9b9ce8a91a182c7ec77f` | changed: three passes |
| `sample.json` | `8439593bf71066386ad78cb1f5e53d41525cba9ee3cb2805c4362b664ee66d3b` | **unchanged**: the same 400 items |
| `layout.json` | `a9ab98962c8bc5df0bf8816700ba64428d232f33b67ca3860350a1c005456937` | changed: a third pass added |

**The layout.**
- **Passes:** three, of 12 batches each (34 or 33 sample items + 4 controls), **36 batches** in all.
- **Unchanged from 5962826904:** passes 1 and 2 are identical to the old layout (same shuffles seed+1 and seed+2, same control rotation). Pass 3 shuffles with seed+3.
- **No repeated batches:** no two passes put the same set of items in a batch.
- **Controls:** each of the 14 sits in 10 or 11 of the 36 batches.
- **Seeds:** +1–3 for the passes, +9 for the bootstrap of d, +10 for the floor, +11 and +12 for the cross-fit; none collides.

**Run rules carried from amendment 3:**
- Three fresh Sonnet 5.5 judgments per item, with the model read off each transcript.
- The archived Sonnet 5 judgment is reported beside as the fourth, never in the majority.
- Pauses only at batch boundaries, each recorded with its batch and instance.

**From amendment 4:** the laya pass runs only after all 36 batches are complete and any void batch re-judged. Its substrate row carries `classifier.toml` `d16ddfdc…`, `[placement]` `f8436535…` (5963635666) and the preflight's `stale = false`.

<details><summary><code>draw.py</code> (as re-pinned)</summary>

```python
#!/usr/bin/env python3
"""#165's re-judge sample: a seeded, proportionally stratified draw of single-judged real items from the
three Sonnet records, and the pass layout (36 sample items + 4 of the 14 controls per batch, three passes, #165 amendment 3).
Usage: draw.py REPO_ROOT OUT_DIR. Deterministic: the same tree and seed give the same bytes."""
import collections, hashlib, importlib.util, json, math, pathlib, random, sys
ROOT, OUT = pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]); OUT.mkdir(parents=True, exist_ok=True)
SEED_PHRASE = "discipline #165 re-judge sample, 2026-10-02"
SEED = int(hashlib.sha256(SEED_PHRASE.encode()).hexdigest()[:16], 16)
N, PER_BATCH, CONTROLS_PER_BATCH, PASSES = 400, 36, 4, 3
SONNET = ["2026-09-21-false-nomination-framing-v2", "2026-09-27-false-nomination-edit-rate", "2026-09-29-false-nomination-edit-rate-second-substrate"]
spec = importlib.util.spec_from_file_location("m", ROOT / "results/2026-10-02-judge-state-lengths/measure.py"); m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
judged = collections.defaultdict(list)   # state sha -> [(record, batch, id, verdict, edit, is_control, item)]
for rec in SONNET:
    d = ROOT / "results" / rec / "judge"; key = json.loads((d / "key.json").read_text())
    for bf in sorted((d / "batches").glob("batch-*.json")):
        n = bf.stem.split("-")[1]; items = {it["id"]: it for it in json.loads(bf.read_text())["items"]}
        vf = d / f"verdicts-{n}.json"
        if not vf.exists(): continue
        ctl = set(key.get(str(int(n)), {}))
        for v in json.loads(vf.read_text()):
            it = items.get(v["id"])
            if it is None: continue
            judged[m.sha(m.state_of(it).encode())].append((rec, n, v["id"], v.get("verdict"), v.get("edit"), v["id"] in ctl, it))
pop = {s: js[0] for s, js in judged.items() if len(js) == 1 and not js[0][5]}
strata = collections.defaultdict(list)
for s, j in pop.items(): strata[(j[0], j[3])].append(s)
total = len(pop)
# largest-remainder proportional allocation
quota = {k: N * len(v) / total for k, v in strata.items()}
alloc = {k: math.floor(q) for k, q in quota.items()}
for k in sorted(quota, key=lambda k: (-(quota[k] - alloc[k]), k))[: N - sum(alloc.values())]: alloc[k] += 1
rng = random.Random(SEED)
sample = []
for k in sorted(strata):
    sample += rng.sample(sorted(strata[k]), alloc[k])
# controls: the 14 controls' states, as judged in the records (identical text in every record)
controls = {}
for s, js in judged.items():
    if js[0][5]: controls[s] = js[0][6]
assert len(controls) == 14, len(controls)
ctl_order = sorted(controls)
passes = []
for p in range(PASSES):
    order = sample[:]; random.Random(SEED + 1 + p).shuffle(order)
    nb = math.ceil(len(order) / PER_BATCH); batches = []
    q, r = divmod(len(order), nb); bounds = [0]
    for b in range(nb): bounds.append(bounds[-1] + q + (1 if b < r else 0))
    for b in range(nb):
        chunk = order[bounds[b]:bounds[b + 1]]
        ctl = [ctl_order[(p * nb * CONTROLS_PER_BATCH + b * CONTROLS_PER_BATCH + i) % 14] for i in range(CONTROLS_PER_BATCH)]
        batches.append({"sample": chunk, "controls": ctl})
    passes.append(batches)
plan = {"seed_phrase": SEED_PHRASE, "seed": SEED, "population": total, "sample_size": len(sample),
        "strata": {f"{k[0]} | {k[1]}": {"population": len(strata[k]), "drawn": alloc[k]} for k in sorted(strata)},
        "passes": PASSES, "batches_per_pass": [len(p) for p in passes], "items_per_batch": "the sample split evenly over ceil(400/36) = 12 batches (34 or 33 items) + 4 controls",
        "excluded": {"states judged more than once": sum(1 for js in judged.values() if len(js) > 1 and not js[0][5]), "controls": 14}}
(OUT / "plan.json").write_text(json.dumps(plan, indent=1) + "\n")
(OUT / "sample.json").write_text(json.dumps([{"state_sha256": s, "record": pop[s][0], "batch": pop[s][1], "id": pop[s][2], "verdict": pop[s][3], "edit": pop[s][4]} for s in sample], indent=1) + "\n")
(OUT / "layout.json").write_text(json.dumps(passes, indent=1) + "\n")
print(json.dumps(plan, indent=1))
```
</details>

Nothing runs until the maintainer's comment names 5962826904, 5963413444, 5963559758, 5963641998 and 5966195560.


## #165 comment 5970425814, 2026-10-03T15:09:16Z

[Dispatch] **Amendment 5, the maintainer's decision relayed for his ratification: the fresh judges run on Sonnet 5 (`claude-sonnet-5`), the judge the data seat's launcher gives.** Reason, in the maintainer's words: the floor was set at *more capable than Haiku 4.5*, a bar every Sonnet clears, so the version does not change what the claim measures. Consequences, so the text matches the run: amendment 3's three fresh judgments per item stand, and `d`, `e₃`, κ and the majority are computed over them; the archived judgment is the fourth, reported beside, and it is a **same-version replication** (archive vs fresh majority, across time, prompt form (b) vs v2, and instance), not a version-drift measurement; the model is still read off every transcript and recorded per batch, and a batch whose transcript records any other model is void. Nothing runs until the maintainer's comment names this one.

## #165 comment 5971096031, 2026-10-03T16:30:05Z

[Planning] **Amendment 5 — reporting additions from sidekick's 0.7.0 briefing; no rule changes. The word is still the ANE pass's under amendments 1–4. The maintainer's ratification extends to this comment's id.**

1. **The daemon is a regime field.** The laya pass records the sidekickd version beside the `classifier.toml` and `[placement]` digests, and the daemon is not upgraded between the first judge batch and the last laya response; 0.7.0 changes residency and the config schema, and a change mid-fire would be a second instance. If the laptop is upgraded before the window opens, the placement record is re-read and `stale = false` re-confirmed.
2. **Two laya passes over the same 400 items, not one.** The served path, `cpu_and_ne`, produces the word. A second pass on `cpu_and_gpu` (D38, a config line, no reconversion) is run in the same window and **reported beside** as the per-path parity row this regime always wanted: decision agreement between paths, and ECE per path after the same cross-fit. The labels are already paid for; the second pass costs seconds.
3. **An abstention band, reported.** For each item the fp32-path top-two logit margin is recorded; an item whose margin is under the served path's measured flip margin (from sidekick's parity grade of record for laya-typed-decisions on the ANE) is `abstain`. Agreement with the fresh majority is computed **with and without** abstentions, and coverage (1 − abstention rate) is reported per question. The rule's ECE is unchanged and uses the calibrated probabilities over all answered items.
4. **Per-item token count and a truncation flag.** laya-typed truncates silently at 1,024; the `sidekick-buckets` header is the tell. Every request records its token count and which bucket answered; any item at the maximum bucket is flagged `possibly_truncated` and reported, since the measured 90th percentile of ~440 tokens predicts none.
5. **Where each decision ran is a value:** the `sidekick-compute-units` and `sidekick-buckets` headers are recorded per response and reconciled against `/v1/models`' placement (with its `stale` flag) at the window's start and end.

Everything in 5962826904, 5963413444, 5963559758, 5963641998 and 5966195560 stands.
