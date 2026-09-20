#!/usr/bin/env bash
# Gate 0 for this directory: the product re-derives from the artefacts committed
# beside it. grades.jsonl, the judge's verdicts, plan.json and decision-rule.toml
# are hashed against the digests the record declares, report.json is rebuilt
# from them by the report module below (the one copy), the rule is applied, and
# the result must be byte-for-byte the committed report.json, whose verdict must
# be the word the front-matter and the claim row carry.
#
# WHAT THIS DOES NOT DO: it does not re-fire the forks. rows.jsonl is the record
# of what the model said; a grade taken wrongly from a row is caught by
# re-grading in the instrument, not here.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
python3 - <<'PY'
import hashlib, json, pathlib, sys
FENCE = "+++"
def cannot_run(m): print(f"recompute: {m}", file=sys.stderr); raise SystemExit(2)
try:
    import tomllib
except ImportError:
    cannot_run(f"this needs Python 3.11 or later for tomllib; this is {sys.version.split()[0]}")
def read(p):
    try: return pathlib.Path(p).read_text(encoding="utf-8")
    except OSError as e: cannot_run(f"{p} cannot be read: {e.strerror}")
def digest(p):
    try: return hashlib.sha256(pathlib.Path(p).read_bytes()).hexdigest()
    except OSError as e: sys.exit(f"{p} is consumed by the record and is not here: {e.strerror}")
text = read("README.md")
if not text.startswith(FENCE + "\n"): cannot_run("README.md does not open with +++ front-matter")
try: front = tomllib.loads(text.split(FENCE + "\n", 2)[1])
except (tomllib.TOMLDecodeError, IndexError) as e: cannot_run(f"README.md front-matter is not TOML: {e}")
rows = []
for n, line in enumerate(read("run.jsonl").splitlines(), start=1):
    if not line.strip(): continue
    try: rows.append(json.loads(line))
    except json.JSONDecodeError as e: cannot_run(f"run.jsonl line {n} is not JSON: {e.msg}")
summary = next((r for r in rows if r.get("record") == "summary"), None)
if summary is None: cannot_run("run.jsonl has no summary row")
claims = [r for r in rows if r.get("record") == "claim"]
if len(claims) != 1: cannot_run(f"run.jsonl carries {len(claims)} claim rows, this directory makes one")
consumed = claims[0].get("consumes", [])
if not consumed: sys.exit("the claim consumes nothing, so there is nothing here to re-derive from")
for a in consumed:
    found = digest(a["path"])
    if found != a["sha256"]: sys.exit(f"{a['path']} is declared {a['sha256']} and hashes to {found}")
for field, counted in (("targets_checked", len(consumed)), ("targets_matched", len(consumed))):
    if summary[field] != counted: sys.exit(f"the summary says {field} is {summary[field]}, the rows hold {counted}")
    if front.get(field) not in (None, counted): sys.exit(f"the front-matter says {field} is {front[field]}, the rows hold {counted}")
product = digest("report.json")
for where, stated in (("front-matter", front["product_sha256"]), ("the summary row", summary["product_sha256"])):
    if stated != product: sys.exit(f"{where} states product_sha256 {stated}, the product hashes to {product}")

# THE REPORT MODULE, verbatim. ------------------------------------------------
# THE REPORT AND THE RULE APPLIER for the false-nomination framing run (gym #89).
# One module, imported by the instrument's `report` subcommand and written
# verbatim into the results directory's recompute.sh, so that a reader holding
# grades.jsonl, the judge's verdicts, plan.json and decision-rule.toml can
# re-derive report.json and the verdict without the instrument's author in the
# room. Nothing here reads the model; everything here is arithmetic over
# committed rows.
#
# Readings, each stated on the verdict under `readings`:
#   1. "at every rung" in the refuted clause quantifies over the COMPUTABLE rungs
#      (ten or more false nominations graded); a rung below that satisfies
#      neither bound, as ratified; a run with no computable rung is
#      `unadjudicated`.
#   2. "the maximum difference over all rungs does not exceed the sham's by
#      >= 0.05": the difference is imperative minus advisory (advisory's
#      advantage), and "the sham's" is imperative minus sham on the same rung;
#      the clause holds when, at the rung where advisory's advantage is largest,
#      that advantage is not 0.05 or more above the sham's -- framing does no
#      better than an irrelevant perturbation. THIS READING WAS NOT POSTED
#      BEFORE THE RUN (fresh review of PR #101, B1): the design restated the
#      ratified text and named no reading of "the sham's"; the applier's
#      wording is from 2026-09-20T09:12Z, when the run was near its end. Two
#      other literal readings are computed beside it on the verdict under
#      `sham_clause_readings`: (B) the sham's own maximum advantage over all
#      rungs; (C) the maximum over rungs of advisory's excess over the sham on
#      the same rung. The verdict word is under (A); (C) would change it.
#      RULED on PR #101 (relayed, 2026-09-20): "the sham's" binds to the
#      floor -- the largest changed-turn rate on false nominations over rungs,
#      against the sham arm's changed-turn rate -- a RATE against a RATE, which
#      is neither (A) nor (B); (C) is rejected. That reading is (D), computed
#      here over the advisory framing and over both framings, because the
#      ruling's text does not say which arm "false nominations" spans and its
#      gloss ("the floor prediction fails") points at advisory; the arm word
#      is asked on the thread. `[ruling].sham_reading` in decision-rule.toml
#      names the reading the verdict word is under (A until the ruling is
#      ratified); the [rule] table stays byte-identical to the ratified text.
#   3. "p below the attainable floor": the paired sign test is the pre-registered
#      comparison, one per rung, ONE-SIDED toward advisory changing less,
#      uncorrected (ruled on #17 for a named comparison); a p can sit at the
#      floor and not below it, so the clause holds when p equals the floor --
#      every discordant fork went the same way. Also worded after the run.
#      RULED (decision 3): the named comparison (the rule's rung) is
#      uncorrected against the attainable floor; the other rungs' sign tests
#      are secondaries and carry a Holm-corrected p beside the raw one under
#      `secondary_sign_tests`, read by nothing in the verdict.
#   4. "advisory exceeds the sham by less than imperative does": (advisory - sham)
#      < (imperative - sham) on the 0.6 rung, i.e. advisory < imperative there.
#   5. Attainability (ruled on #17): a rate over n graded forks moves in steps of
#      1/n, so a margin is attainable on a rung iff 1/n <= margin; the supported
#      clause is asked only where its margin is attainable, else `unadjudicated`.
#   6. The ladder (ruled, decision 1): with zero realised true nominations on
#      every rung the rung claims are `unadjudicated` and the ladder is labelled
#      `degenerate` on the verdict; the population claim over the false
#      nominations is what the verdict adjudicates.
#   7. The shuffled-framing null (ruled, decision 5) was added after the run:
#      labelled post-hoc, counted toward nothing.
import collections
import math
import random

RUNGS = [1.0, 0.8, 0.6, 0.4, 0.6222, 0.0556]
SEED = 0x89
MIN_GRADED = 10


def sign_test(pairs):
    """One-sided paired sign test that advisory changes less than imperative.
    pairs: (advisory_changed, imperative_changed) per fork. Discordant forks
    where advisory changed and imperative did not are the bad direction; p is
    the binomial probability of at most that many under 1/2. Floor = 0.5**n."""
    disc = [(x, y) for x, y in pairs if x != y]
    n = len(disc)
    if n == 0:
        return {"n_discordant": 0, "k_advisory_only": 0, "k_imperative_only": 0, "p": 1.0, "floor": 1.0}
    k = sum(1 for x, y in disc if x and not y)
    p = sum(math.comb(n, i) for i in range(0, k + 1)) / 2 ** n
    return {"n_discordant": n, "k_advisory_only": k, "k_imperative_only": n - k, "p": p, "floor": 0.5 ** n}


def bootstrap_diff(pairs, resamples=9999, seed=SEED):
    """Paired bootstrap of the rate difference advisory minus imperative over
    forks, add-one corrected, one-sided toward advisory lower."""
    rng = random.Random(seed)
    n = len(pairs)
    if n == 0:
        return None
    obs = sum(x - y for x, y in pairs) / n
    crossed = 0
    for _ in range(resamples):
        d = [pairs[rng.randrange(n)] for _ in range(n)]
        if sum(x - y for x, y in d) / n >= 0:
            crossed += 1
    return {"observed": round(obs, 4), "p": (crossed + 1) / (resamples + 1), "floor": 1 / (resamples + 1), "resamples": resamples}


def shuffled_framing_null(graded, resamples=9999, seed=SEED):
    """The pre-registered shuffled-framing null: the arm labels (imperative,
    advisory, sham) are permuted within each fork, and the advisory-minus-
    imperative rate difference is recomputed; over many permutations the
    difference must sit at chance -- a null centred on zero. Reported per rung
    with the observed difference's two-sided p under that null."""
    rng = random.Random(seed)
    arms = ("imperative", "advisory", "sham")
    flags = [[int(bool(a[x]["changed"])) for x in arms] for a in graded.values()]
    n = len(flags)
    if n == 0:
        return None
    obs = sum(f[1] - f[0] for f in flags) / n
    diffs = []
    for _ in range(resamples):
        tot = 0
        for f in flags:
            g = f[:]
            rng.shuffle(g)
            tot += g[1] - g[0]
        diffs.append(tot / n)
    diffs.sort()
    mean = sum(diffs) / len(diffs)
    sd = (sum((d - mean) ** 2 for d in diffs) / len(diffs)) ** 0.5
    lo, hi = diffs[int(0.025 * len(diffs))], diffs[int(0.975 * len(diffs)) - 1]
    extreme = sum(1 for d in diffs if abs(d) >= abs(obs) - 1e-12)
    return {"observed": round(obs, 4), "resamples": resamples, "null_mean": round(mean, 4), "null_sd": round(sd, 4),
            "null_ci95": [round(lo, 4), round(hi, 4)], "at_chance": lo <= 0 <= hi,
            "p_two_sided": (extreme + 1) / (resamples + 1), "floor": 1 / (resamples + 1),
            "label": "post-hoc", "counted_toward_verdict": False}


def holm(pvals):
    """Holm's step-down correction over a dict of raw p-values; returns the
    adjusted p per key (monotone, capped at 1)."""
    items = sorted(pvals.items(), key=lambda kv: kv[1])
    m = len(items)
    out, running = {}, 0.0
    for i, (k, p) in enumerate(items):
        running = max(running, min(1.0, (m - i) * p))
        out[k] = running
    return out


def build_report(grades, verdicts, plan):
    by = collections.defaultdict(dict)
    for g in grades:
        by[(g["rung"], g["fork"])][g["arm"]] = g
    rungs = {}
    for rung in [str(r) for r in RUNGS]:
        forks = {fid: arms for (rg, fid), arms in by.items() if rg == rung}
        false_forks = {fid: arms for fid, arms in forks.items() if arms and not next(iter(arms.values()))["true"]}
        graded = {fid: arms for fid, arms in false_forks.items()
                  if all(arms.get(x, {}).get("changed") is not None for x in ("imperative", "advisory", "sham"))}
        n = len(graded)

        def rate(arm):
            return (sum(1 for arms in graded.values() if arms[arm]["changed"]) / n) if n else None
        pairs = [(int(arms["advisory"]["changed"]), int(arms["imperative"]["changed"])) for arms in graded.values()]
        ack = {}
        for arm in ("imperative", "advisory"):
            named = [fid for fid, arms in graded.items() if arms[arm].get("names")]
            vs = collections.Counter(verdicts.get(f"{rung}|{fid}|{arm}", "unjudged") for fid in named)
            # split by the judged surface (design correction 5747611166): the
            # drives that ran with thinking on expose reasoning + prose, the
            # others prose only
            by_surface = {}
            for surface in sorted({arms[arm].get("surface", "prose") for arms in graded.values()}):
                fids = [fid for fid, arms in graded.items() if arms[arm].get("surface", "prose") == surface]
                nm = [fid for fid in fids if graded[fid][arm].get("names")]
                sv = collections.Counter(verdicts.get(f"{rung}|{fid}|{arm}", "unjudged") for fid in nm)
                by_surface[surface] = {"graded": len(fids), "named": len(nm), "verdicts": dict(sorted(sv.items()))}
            ack[arm] = {"named": len(named), "of": n, "verdicts": dict(sorted(vs.items())), "by_surface": by_surface}
        realised_true = sum(1 for arms in forks.values() if arms and next(iter(arms.values()))["true"])
        rungs[rung] = {
            "nominal": float(rung), "forks": len(forks), "true_nominations": realised_true,
            "realised_true_share": (realised_true / len(forks)) if forks else None,
            "false_nominations": len(false_forks), "graded": n, "computable": n >= MIN_GRADED,
            "rate_step": (1 / n) if n else None,
            "changed_rate": {arm: rate(arm) for arm in ("imperative", "advisory", "sham")},
            "changed_rate_nocomment": {arm: (sum(1 for arms in graded.values() if arms[arm].get("changed_nocomment")) / n) if n else None
                                       for arm in ("imperative", "advisory", "sham")},
            "advisory_minus_imperative": (rate("advisory") - rate("imperative")) if n else None,
            "imperative_minus_sham": (rate("imperative") - rate("sham")) if n else None,
            "advisory_minus_sham": (rate("advisory") - rate("sham")) if n else None,
            "sign_test": sign_test(pairs) if n else None,
            "bootstrap": bootstrap_diff(pairs) if n else None,
            "shuffled_framing_null": shuffled_framing_null(graded) if n else None,
            "per_session_cost": {arm: (rate(arm) * (1 - float(rung))) if n else None for arm in ("imperative", "advisory")},
            "acknowledgement": ack,
        }
    true_forks = [(rg, fid, arms) for (rg, fid), arms in by.items() if arms and next(iter(arms.values()))["true"]]
    true_rates = {arm: (sum(1 for _, _, arms in true_forks if arms.get(arm, {}).get("changed")) / len(true_forks)) if true_forks else None
                  for arm in ("imperative", "advisory", "sham")}
    return {"rungs": rungs, "true_nominations": {"n": len(true_forks), "changed_rate": true_rates},
            "arms": ["control", "imperative", "advisory", "sham"], "seed": SEED, "forks_per_rung": plan.get("forks_per_rung"),
            "sample": len(plan.get("sample", []))}


def adjudicate(report, rule):
    th = rule["rule"]
    rungs = report["rungs"]
    computable = {r: x for r, x in rungs.items() if x["computable"]}
    key = str(th["rung"])
    at = rungs.get(key)
    margin, small = th["supported_margin"], th["refuted_margin"]
    attainable = bool(at and at["computable"] and at["rate_step"] <= margin)
    supported = False
    if at and at["computable"] and attainable:
        diff = at["advisory_minus_imperative"]
        st = at["sign_test"]
        supported = (diff <= -margin and st["p"] <= st["floor"] + 1e-12
                     and at["advisory_minus_sham"] < at["imperative_minus_sham"])
    every_not_lower = bool(computable) and all(x["advisory_minus_imperative"] > -small for x in computable.values())
    best = max(computable.values(), key=lambda x: -x["advisory_minus_imperative"]) if computable else None
    max_advantage = (-best["advisory_minus_imperative"]) if best else None
    sham_advantage_there = best["imperative_minus_sham"] if best else None
    no_better_than_sham = best is not None and (max_advantage - sham_advantage_there) < small
    # the two other literal readings of "the sham's" (fresh review of PR #101, B1)
    sham_max = max((x["imperative_minus_sham"] for x in computable.values()), default=None)
    # advisory's excess over the sham on a rung = (imp - adv) - (imp - sham) = sham - adv
    excess = {r: round(-x["advisory_minus_sham"], 4) for r, x in computable.items()}
    max_excess = max(excess.values(), default=None)
    reading_b = sham_max is not None and (max_advantage - sham_max) < small
    reading_c = max_excess is not None and max_excess < small
    # (D) the ruled reading: the largest changed-turn rate on false nominations
    # over rungs, against the sham arm's changed-turn rate on that rung; over
    # the advisory framing, and over both framings, the arm word pending
    def reading_d(arms_):
        top = max(((x["changed_rate"][a], r, a) for r, x in computable.items() for a in arms_), default=None)
        if top is None:
            return {"fires": False}
        rate_, r, a = top
        return {"largest_rate": rate_, "rung": rungs[r]["nominal"], "arm": a, "sham_rate_there": rungs[r]["changed_rate"]["sham"],
                "excess_over_sham": round(rate_ - rungs[r]["changed_rate"]["sham"], 4), "fires": (rate_ - rungs[r]["changed_rate"]["sham"]) < small}
    d_adv, d_both = reading_d(("advisory",)), reading_d(("advisory", "imperative"))
    reading_a = no_better_than_sham
    fires = {"A": reading_a, "B": reading_b, "C": reading_c, "D-advisory": d_adv["fires"], "D-both": d_both["fires"]}
    ruling = rule.get("ruling", {})  # the maintainer's rulings after the run, in their own table; [rule] stays byte-identical
    applied = ruling.get("sham_reading", "A")
    if applied not in fires:
        raise SystemExit(f"decision-rule.toml names sham_reading {applied!r}; the applier knows {sorted(fires)}")
    no_better_than_sham = fires[applied]
    # the pre-registered floor prediction: a false nomination changes the turn
    # more often than the sham under BOTH framings, or framing is moot
    floor_pred = {r: {"imperative_above_sham": x["imperative_minus_sham"] > 0, "advisory_above_sham": x["advisory_minus_sham"] > 0}
                  for r, x in computable.items()}
    floor_holds = bool(computable) and all(v["imperative_above_sham"] and v["advisory_above_sham"] for v in floor_pred.values())
    def word(sham_fires):
        if supported:
            return "supported"
        if computable and (every_not_lower or sham_fires):
            return "refuted"
        if not computable or (at and not attainable):
            return "unadjudicated"
        return "inconclusive"
    verdict = word(no_better_than_sham)
    # secondaries (ruled, decision 3): every rung's sign test but the named one,
    # Holm-corrected beside the raw p; read by nothing above
    secondaries = {r: x["sign_test"]["p"] for r, x in computable.items() if r != key and x["sign_test"]}
    adj = holm(secondaries)
    secondary_tests = {r: {"p_raw": p, "p_holm": adj[r], "floor": computable[r]["sign_test"]["floor"]} for r, p in secondaries.items()}
    # the ladder (ruled, decision 1)
    shares = {r: x["realised_true_share"] for r, x in rungs.items()}
    degenerate = all((v or 0) == 0 for v in shares.values())
    rule_block = {"rung": th["rung"], "supported": th["supported"], "refuted": th["refuted"], "inconclusive": th["inconclusive"],
                  "sham_reading": applied}
    for k in ("version", "v1_wording", "v2_wording", "status"):
        if k in ruling:
            rule_block[k] = ruling[k]
    return {
        "rule": rule_block,
        "readings": {
            "every_rung": "quantifies over computable rungs (ten or more false nominations graded); none computable is unadjudicated",
            "maximum_difference": {"A": "advisory's advantage (imperative minus advisory) at the rung where it is largest, against the sham's advantage (imperative minus sham) on that rung",
                                   "B": "the same advantage against the sham's largest advantage over rungs",
                                   "C": "the largest over rungs of advisory's excess over the sham on the same rung",
                                   "D-advisory": "the largest advisory changed-turn rate over rungs against the sham's rate on that rung",
                                   "D-both": "the largest changed-turn rate under either framing over rungs against the sham's rate on that rung"}[applied],
            "p_below_floor": "the paired sign test, one per rung, uncorrected; holds when p equals its attainable floor",
            "sham_clause": "(advisory - sham) < (imperative - sham) on the adjudicated rung",
            "attainability": "a margin is attainable on a rung iff 1/n_graded <= margin; the supported clause is asked only where attainable",
        },
        "attainability": {"rung": th["rung"], "graded": at["graded"] if at else None, "rate_step": at["rate_step"] if at else None,
                          "supported_margin": margin, "supported_attainable": attainable},
        "supported_at_rung": {"advisory_minus_imperative": at["advisory_minus_imperative"] if at else None,
                              "sign_test": at["sign_test"] if at else None,
                              "advisory_minus_sham": at["advisory_minus_sham"] if at else None,
                              "imperative_minus_sham": at["imperative_minus_sham"] if at else None,
                              "holds": supported},
        "refuted_by": {"computable_rungs": sorted(computable), "every_rung_not_lower_by_margin": every_not_lower,
                       "best_rung": (best["nominal"] if best else None), "max_advisory_advantage": max_advantage,
                       "sham_advantage_at_best_rung": sham_advantage_there, "no_better_than_sham": no_better_than_sham, "under_reading": applied},
        "sham_clause_readings": {
            "applied": applied,
            "A": "the sham's advantage on the rung where advisory's advantage is largest", "A_fires": reading_a,
            "B": "the sham's own maximum advantage over all rungs", "B_sham_max": sham_max, "B_fires": reading_b,
            "C": "the maximum over rungs of advisory's excess over the sham on the same rung", "C_excess_by_rung": excess, "C_max_excess": max_excess, "C_fires": reading_c,
            "D": "the largest changed-turn rate on false nominations over rungs, against the sham arm's changed-turn rate on that rung (ruled on PR #101; the arm the largest rate is taken over is asked on the thread)",
            "D_advisory": d_adv, "D_both": d_both,
            "verdict_under": {k: word(v) for k, v in fires.items()},
            "note": "none of A, B, C was posted before the run; the ruling (relayed on PR #101) binds the sham's to the floor as a rate, reading D, which is neither A nor B; the verdict word is under rule.sham_reading",
        },
        "secondary_sign_tests": {"named": key, "named_p_uncorrected": at["sign_test"]["p"] if at and at["sign_test"] else None,
                                 "correction": "Holm over the other computable rungs", "by_rung": secondary_tests, "counted_toward_verdict": False},
        "ladder": {"label": "degenerate" if degenerate else "realised", "realised_true_share_by_rung": shares,
                   "rung_claims": "unadjudicated" if degenerate else "adjudicated",
                   "text": "zero realised true nominations on every rung: the rung claims are unadjudicated and the verdict adjudicates the population claim over the false nominations" if degenerate else "true nominations realised"},
        "floor_prediction": {"text": "a false nomination changes the turn more often than the sham under both framings, or framing is moot",
                             "by_rung": floor_pred, "holds": floor_holds},
        "envelope": {"parameters": "each fork re-fires its drive's archived turn.request with that request's own sampler parameters; the request digest on every row names it",
                     "within": True, "label": "within-envelope"},
        "verdict": verdict,
        "acknowledgement": {r: x["acknowledgement"] for r, x in rungs.items()},
        "true_nominations": report["true_nominations"],
    }

# -----------------------------------------------------------------------------
try: rule = tomllib.loads(read("decision-rule.toml"))
except tomllib.TOMLDecodeError as e: cannot_run(f"decision-rule.toml is not TOML: {e}")
grades = [json.loads(l) for l in read("grades.jsonl").splitlines() if l.strip()]
plan = json.loads(read("plan.json"))
verdicts = {}
for a in consumed:
    if a["path"].startswith("judge/verdicts-"):  # a judge/void-*.json is digest-checked above and read by nothing
        for v in json.loads(read(a["path"])):
            if "|" in v["id"]: verdicts[v["id"]] = v["verdict"]
report = build_report(grades, verdicts, plan)
report["verdict"] = adjudicate(report, rule)
derived = json.dumps(report, indent=1, sort_keys=True) + "\n"
committed = read("report.json")
yielded = report["verdict"]["verdict"]
if derived != committed:
    try: stated = json.loads(committed).get("verdict", {}).get("verdict")
    except json.JSONDecodeError: stated = "unreadable"
    sys.exit(f"report.json does not re-derive: the rule over the grades yields {yielded!r} and the committed file says {stated!r}, or the two differ somewhere below the verdict")
for where, stated in (("front-matter", front["result"]), ("the claim row", claims[0]["result"])):
    if stated != yielded: sys.exit(f"{where} says {stated!r}, the rule yields {yielded!r}")
print(f"{len(consumed)} artefact(s), the product and the verdict re-derive")
PY
