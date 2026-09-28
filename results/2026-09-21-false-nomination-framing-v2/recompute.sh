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
# THE REPORT AND THE RULE APPLIER for the false-nomination framing run, second
# draw ((b)-v2, gym #89, authorized by decision 1 on PR #101). One module,
# imported by the instrument's `report --v2` subcommand and written verbatim
# into the results directory's recompute.sh, so that a reader holding
# grades.jsonl, the judge's verdicts, plan.json and decision-rule.toml can
# re-derive report.json and the verdict without the instrument's author in the
# room. Nothing here reads the model; everything here is arithmetic over
# committed rows.
#
# Every reading this applier takes is in decision-rule.toml [readings], posted
# on #89 before the first fork; this module applies them and invents none:
#   turn_change  changed iff the comment-stripped (tool, target) differs from the
#                control draw's (a command against none is a difference), OR the
#                blind judge's `edit` field on the row is "yes". The (tool,
#                target) grade alone is reported beside, adjudicating nothing.
#   changed_rate over fork-rungs graded on both framings; a fork-rung with a
#                null edit on either framing (unjudged after the void protocol)
#                or an errored row is excluded from that rung pairwise, counted.
#   sham_clause  reading D as ratified: per framing, the largest changed-turn
#                rate over computable rungs against the sham's rate on that same
#                rung; fires when either framing's (rate - sham) < margin.
#   p_at_floor   the one-sided paired sign test; supported asks p <= supported_p
#                and is asked only where the attainable floor 0.5**n_discordant
#                is <= supported_p; the floor is printed beside every p.
#   every_rung   quantifies over computable rungs (>= MIN_GRADED graded on both
#                framings); none computable is unadjudicated.
#   attainability a margin is attainable on a rung iff 1/n_graded <= margin; the
#                supported clause is asked only where attainable.
#   ties         "lower by >= m" is (imperative - advisory) >= m; "does not
#                exceed by >= m" is (x - y) < m.
#   ladder       nominal by construction (at most two forks can draw a true
#                nomination); rung claims are not adjudicated; the rungs are
#                replicate assignments and "every rung" reads over them.
#   precedence   when a refuted clause and the supported clause both hold, the
#                word is refuted: the floor is asked first.
#   controls     the shuffled-framing null must put its mean within
#                NULL_TOLERANCE of zero on every computable rung, or the result
#                is inconclusive whatever the rule says.
import collections
import math
import random

RUNGS = [1.0, 0.8, 0.6, 0.4, 0.6222, 0.0556]
SEED = 0x189
MIN_GRADED = 10
NULL_TOLERANCE = 0.02
ARMS3 = ("imperative", "advisory", "sham")


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


def shuffled_framing_null(flags, resamples=9999, seed=SEED):
    """The pre-registered shuffled-framing null: the arm labels (imperative,
    advisory, sham) are permuted within each fork and the advisory-minus-
    imperative rate difference recomputed; the null must be centred on zero.
    flags: per fork, [imperative, advisory, sham] changed flags."""
    rng = random.Random(seed)
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
            "null_ci95": [round(lo, 4), round(hi, 4)], "at_chance": abs(mean) <= NULL_TOLERANCE, "tolerance": NULL_TOLERANCE,
            "p_two_sided": (extreme + 1) / (resamples + 1), "floor": 1 / (resamples + 1),
            "label": "pre-registered", "counted_toward_verdict": "as a control: a failed null makes the result inconclusive"}


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


def composite(g, edit):
    """The v2 turn-change grade of one arm row: None when ungradable."""
    tt = g.get("changed_nocomment")
    if tt is None or g.get("error"):
        return None
    if edit is None:
        return None
    return bool(tt) or edit == "yes"


def build_report(grades, verdicts, plan):
    """grades: the committed grades.jsonl rows; verdicts: {(rung, fork, arm):
    {"verdict":..., "edit":...}} resolved through the id map; plan: plan.json."""
    by = collections.defaultdict(dict)
    for g in grades:
        by[(g["rung"], g["fork"])][g["arm"]] = g
    rungs = {}
    for rung in [str(r) for r in RUNGS]:
        forks = {fid: arms for (rg, fid), arms in by.items() if rg == rung}
        false_forks = {fid: arms for fid, arms in forks.items() if arms and not next(iter(arms.values()))["true"]}
        comp = {}
        excluded = collections.Counter()
        for fid, arms in false_forks.items():
            c = {}
            for arm in ARMS3:
                g = arms.get(arm)
                v = verdicts.get((rung, fid, arm)) or {}
                c[arm] = composite(g, v.get("edit")) if g else None
            if any(c[a] is None for a in ("imperative", "advisory")):
                excluded["framing row ungradable (errored or edit unjudged)"] += 1
                continue
            if c["sham"] is None:
                excluded["sham row ungradable"] += 1
                continue
            comp[fid] = c
        graded = {fid: false_forks[fid] for fid in comp}
        n = len(comp)

        def rate(arm, field=None):
            if not n:
                return None
            if field is None:
                return sum(1 for c in comp.values() if c[arm]) / n
            return sum(1 for fid in comp if graded[fid][arm].get(field)) / n
        pairs = [(int(c["advisory"]), int(c["imperative"])) for c in comp.values()]
        flags = [[int(c[a]) for a in ARMS3] for c in comp.values()]
        edit_rate = {arm: (sum(1 for fid in comp if (verdicts.get((rung, fid, arm)) or {}).get("edit") == "yes") / n) if n else None for arm in ARMS3}
        ack = {}
        for arm in ARMS3:
            named = [fid for fid in comp if graded[fid][arm].get("names")]
            vs = collections.Counter((verdicts.get((rung, fid, arm)) or {}).get("verdict", "unjudged") for fid in named)
            by_surface = {}
            for surface in sorted({graded[fid][arm].get("surface", "prose") for fid in comp}):
                fids = [fid for fid in comp if graded[fid][arm].get("surface", "prose") == surface]
                nm = [fid for fid in fids if graded[fid][arm].get("names")]
                sv = collections.Counter((verdicts.get((rung, fid, arm)) or {}).get("verdict", "unjudged") for fid in nm)
                by_surface[surface] = {"graded": len(fids), "named": len(nm), "verdicts": dict(sorted(sv.items()))}
            ack[arm] = {"named": len(named), "of": n, "verdicts": dict(sorted(vs.items())), "by_surface": by_surface}
        realised_true = sum(1 for arms in forks.values() if arms and next(iter(arms.values()))["true"])
        rungs[rung] = {
            "nominal": float(rung), "forks": len(forks), "true_nominations": realised_true,
            "realised_true_share": (realised_true / len(forks)) if forks else None,
            "false_nominations": len(false_forks), "graded": n, "excluded": dict(excluded), "computable": n >= MIN_GRADED,
            "rate_step": (1 / n) if n else None,
            "changed_rate": {arm: rate(arm) for arm in ARMS3},
            "changed_rate_tool_target": {arm: rate(arm, "changed_nocomment") for arm in ARMS3},
            "changed_rate_tool_target_raw_extractor": {arm: rate(arm, "changed") for arm in ARMS3},
            "edit_rate": edit_rate,
            "advisory_minus_imperative": (rate("advisory") - rate("imperative")) if n else None,
            "imperative_minus_sham": (rate("imperative") - rate("sham")) if n else None,
            "advisory_minus_sham": (rate("advisory") - rate("sham")) if n else None,
            "sign_test": sign_test(pairs) if n else None,
            "bootstrap": bootstrap_diff(pairs) if n else None,
            "shuffled_framing_null": shuffled_framing_null(flags) if n else None,
            "per_session_cost": {arm: (rate(arm) * (1 - float(rung))) if n else None for arm in ("imperative", "advisory")},
            "acknowledgement": ack,
        }
    true_rows = [(rg, fid, arms) for (rg, fid), arms in by.items() if arms and next(iter(arms.values()))["true"]]
    true_comp = {arm: [composite(arms[arm], (verdicts.get((rg, fid, arm)) or {}).get("edit")) for rg, fid, arms in true_rows if arms.get(arm)] for arm in ARMS3}
    true_rates = {arm: {"n": sum(1 for c in cs if c is not None), "changed_rate": (sum(1 for c in cs if c) / sum(1 for c in cs if c is not None)) if any(c is not None for c in cs) else None}
                  for arm, cs in true_comp.items()}
    return {"rungs": rungs, "true_nominations": {"fork_rungs": len(true_rows), "by_arm": true_rates, "role": "a control figure, read by nothing"},
            "arms": ["control", "imperative", "advisory", "sham"], "seed": plan.get("seed"), "forks_per_rung": plan.get("forks_per_rung"),
            "sample": len(plan.get("sample", [])), "forced_forks": plan.get("forced", [])}


def adjudicate(report, rule):
    th = rule["rule"]
    if th.get("version") != "v2":
        raise SystemExit(f"this applier is for [rule].version = 'v2'; decision-rule.toml says {th.get('version')!r}")
    readings = rule.get("readings", {})
    rungs = report["rungs"]
    computable = {r: x for r, x in rungs.items() if x["computable"]}
    key = str(th["rung"])
    at = rungs.get(key)
    margin, small, p_thr = th["supported_margin"], th["refuted_margin"], th["supported_p"]
    attainable = bool(at and at["computable"] and at["rate_step"] <= margin)
    p_askable = bool(at and at["sign_test"] and at["sign_test"]["floor"] <= p_thr)
    supported = False
    if at and at["computable"] and attainable and p_askable:
        diff = at["advisory_minus_imperative"]
        st = at["sign_test"]
        supported = (-diff >= margin and st["p"] <= p_thr and at["advisory_minus_sham"] < at["imperative_minus_sham"])
    every_not_lower = bool(computable) and all(-x["advisory_minus_imperative"] < small for x in computable.values())

    def reading_d(arm):
        top = max(((x["changed_rate"][arm], r) for r, x in computable.items()), default=None)
        if top is None:
            return {"arm": arm, "fires": False}
        rate_, r = top
        sham = rungs[r]["changed_rate"]["sham"]
        return {"arm": arm, "largest_rate": rate_, "rung": rungs[r]["nominal"], "sham_rate_there": sham,
                "excess_over_sham": round(rate_ - sham, 4), "fires": (rate_ - sham) < small}
    d = {arm: reading_d(arm) for arm in ("advisory", "imperative")}
    sham_fires = d["advisory"]["fires"] or d["imperative"]["fires"]
    floor_pred = {r: {"imperative_above_sham": x["imperative_minus_sham"] > 0, "advisory_above_sham": x["advisory_minus_sham"] > 0}
                  for r, x in computable.items()}
    floor_holds = bool(computable) and all(v["imperative_above_sham"] and v["advisory_above_sham"] for v in floor_pred.values())
    null_by_rung = {r: x["shuffled_framing_null"]["at_chance"] for r, x in computable.items() if x["shuffled_framing_null"]}
    null_ok = bool(computable) and all(null_by_rung.values())
    # precedence (a posted reading): the floor is asked first, so a refuted
    # clause that holds beats a supported clause that also holds -- a framing
    # no better than an irrelevant passage is not supported by any margin
    if computable and (every_not_lower or sham_fires):
        word = "refuted"
    elif supported:
        word = "supported"
    elif not computable or (at and not (attainable and p_askable)):
        word = "unadjudicated"
    else:
        word = "inconclusive"
    verdict = word if (null_ok or word == "unadjudicated") else "inconclusive"
    secondaries = {r: x["sign_test"]["p"] for r, x in computable.items() if r != key and x["sign_test"]}
    adj = holm(secondaries)
    secondary_tests = {r: {"p_raw": p, "p_holm": adj[r], "floor": computable[r]["sign_test"]["floor"]} for r, p in secondaries.items()}
    shares = {r: x["realised_true_share"] for r, x in rungs.items()}
    return {
        "rule": {k: th[k] for k in ("version", "rung", "turn_change", "supported", "supported_margin", "supported_p", "refuted", "refuted_margin", "inconclusive", "uncomputable_rung", "controls") if k in th},
        "readings": dict(readings),
        "attainability": {"rung": th["rung"], "graded": at["graded"] if at else None, "rate_step": at["rate_step"] if at else None,
                          "supported_margin": margin, "supported_attainable": attainable,
                          "supported_p": p_thr, "p_floor": at["sign_test"]["floor"] if at and at["sign_test"] else None, "p_askable": p_askable},
        "supported_at_rung": {"advisory_minus_imperative": at["advisory_minus_imperative"] if at else None,
                              "sign_test": at["sign_test"] if at else None,
                              "advisory_minus_sham": at["advisory_minus_sham"] if at else None,
                              "imperative_minus_sham": at["imperative_minus_sham"] if at else None,
                              "holds": supported},
        "refuted_by": {"computable_rungs": sorted(computable), "every_rung_not_lower_by_margin": every_not_lower,
                       "sham_clause": d, "sham_clause_fires": sham_fires, "reading": "D per framing, as ratified on PR #101"},
        "floor_prediction": {"text": "a false nomination changes the turn more often than the sham under both framings, or framing is moot",
                             "by_rung": floor_pred, "holds": floor_holds},
        "controls": {"shuffled_framing_null_at_chance_by_rung": null_by_rung, "shuffled_framing_null_ok": null_ok,
                     "effect": None if null_ok else "the null control failed: the result is inconclusive whatever the rule says"},
        "secondary_sign_tests": {"named": key, "named_p_uncorrected": at["sign_test"]["p"] if at and at["sign_test"] else None,
                                 "correction": "Holm over the other computable rungs", "by_rung": secondary_tests, "counted_toward_verdict": False},
        "ladder": {"label": "nominal", "realised_true_share_by_rung": shares, "rung_claims": "unadjudicated",
                   "text": "at most two forks can draw a true nomination on any rung, so the rungs are replicate assignments of a false nomination and no rung claim is adjudicable; stated before the run"},
        "envelope": {"parameters": "each fork re-fires its drive's archived turn.request with that request's own sampler parameters; the request digest on every row names it",
                     "within": True, "label": "within-envelope"},
        "word_before_controls": word,
        "verdict": verdict,
        "acknowledgement": {r: x["acknowledgement"] for r, x in rungs.items()},
        "edit_rate": {r: x["edit_rate"] for r, x in rungs.items()},
        "true_nominations": report["true_nominations"],
    }

# -----------------------------------------------------------------------------
try: rule = tomllib.loads(read("decision-rule.toml"))
except tomllib.TOMLDecodeError as e: cannot_run(f"decision-rule.toml is not TOML: {e}")
grades = [json.loads(l) for l in read("grades.jsonl").splitlines() if l.strip()]
plan = json.loads(read("plan.json"))
ids = json.loads(read("judge/ids.json"))
verdicts = {}
for a in consumed:
    if a["path"].startswith("judge/verdicts-"):  # a judge/void-*.json or malformed-*.json is digest-checked above and read by nothing
        for v in json.loads(read(a["path"])):
            m = ids.get(v["id"])
            if m and "rung" in m: verdicts[(m["rung"], m["fork"], m["arm"])] = {"verdict": v["verdict"], "edit": v["edit"]}
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
