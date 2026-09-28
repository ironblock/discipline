#!/usr/bin/env python3
"""The applier for (b'), the edit-rate claim (gym #114). It applies the ratified
decision-rule.toml (pinned by digest) to a record of the false-nomination
instrument's shape: grades.jsonl, judge/ids.json, judge/key.json (pinned),
judge/batches/batch-NN.json, judge/verdicts-NN.json, plan.json (for the seed).
The same applier, by digest, reads (b)-v2's committed rows (stage 1, post-hoc)
and the fresh draw (stage 2, the claim).

Every rate is an exact rational and every comparison is made on exact
rationals; the sign test's p and floor are exact; no float enters a word.

Usage:
  apply_bprime.py RULE_TOML RECORD_DIR   -> prints the verdict as JSON
  apply_bprime.py --selftest RULE_TOML   -> runs the fixtures (made-up counts only)
Exit 0 printed; 1 a selftest fixture read other than expected; 2 an input is
missing, malformed, or not the pinned bytes.
"""
import collections, hashlib, json, pathlib, random, sys, tempfile, tomllib
from fractions import Fraction as F
from math import comb

RULE_SHA = "cee9e51342592d9ec4eca966706e2a6207294e211d67fbf08ffcf0e9bc1658d2"
KEY_SHA = "0eb704daddba6e938b2ac2b386ecb2f82ab4a79cc56b3775cb9dc454fcc9cf5f"
# The ruled key's answers, per control. (b)-v2's judge/key.json (the digest above)
# maps each batch's opaque control ids to these answers through judge/ids.json;
# the answer is the same for a control in every batch it sits in, and ctl05 is
# keyed on the edit field only (the ruling of 2026-09-21). A fresh draw places
# its controls in different batches under different ids, so its key.json cannot
# carry the same bytes; it must carry these answers, which is what is checked.
RULED = {
    "ctl01": {"edit": "yes", "verdict": "accepts"}, "ctl02": {"edit": "yes", "verdict": "accepts"},
    "ctl03": {"edit": "yes", "verdict": "accepts"}, "ctl04": {"edit": "yes", "verdict": "accepts"},
    "ctl05": {"edit": "yes", "verdict": None}, "ctl06": {"edit": "yes", "verdict": "accepts"},
    "ctl07": {"edit": "no", "verdict": "accepts"}, "ctl08": {"edit": "no", "verdict": "questions"},
    "ctl09": {"edit": "no", "verdict": "questions"}, "ctl10": {"edit": "no", "verdict": "declines"},
    "ctl11": {"edit": "no", "verdict": "declines"}, "ctl12": {"edit": "no", "verdict": "declines"},
    "ctl13": {"edit": "no", "verdict": "ignores"}, "ctl14": {"edit": "no", "verdict": "ignores"},
}
ARMS3 = ("imperative", "advisory", "sham")
MIN_COUNTED = 10
PERMS = 9999
ECHO = {  # fixed wording of the two nomination lines, entry excluded; matched case-insensitively
    "imperative": ("is superseded by this step", "update it now", "before anything else", "state the correction in your reply"),
    "advisory": ("may be affected by this step", "if it no longer holds, say so", "otherwise carry on"),
}


def cannot(m):
    print(f"apply_bprime: {m}", file=sys.stderr)
    raise SystemExit(2)


def load_rule(path):
    p = pathlib.Path(path)
    if not p.is_file() or hashlib.sha256(p.read_bytes()).hexdigest() != RULE_SHA:
        cannot("decision-rule.toml is missing or not the ratified bytes")
    rule = tomllib.loads(p.read_text(encoding="utf-8"))
    r = rule["rule"]
    if r["version"] != "b-prime-1" or r["floor_arms"] != ["imperative"]:
        cannot("the rule is not the ratified b-prime-1 text")
    return {"rung": str(r["rung"]), "margin": F(r["supported_margin"]), "p": F(r["supported_p"]),
            "floor": F(r["floor_margin"]), "refuted": F(r["refuted_margin"]),
            "rungs": [str(x) for x in rule["claim"]["rungs"]]}


def sign_test(pairs):
    """pairs: (advisory_edit, imperative_edit) per counted fork-rung, 0/1. One-sided in the
    predicted direction (advisory edits less). Exact rationals."""
    disc = [(a, i) for a, i in pairs if a != i]
    n = len(disc)
    k = sum(1 for a, i in disc if a and not i)
    if n == 0:
        return {"n_discordant": 0, "k_advisory_only": 0, "p": F(1), "floor": F(1)}
    return {"n_discordant": n, "k_advisory_only": k, "p": F(sum(comb(n, j) for j in range(k + 1)), 2 ** n), "floor": F(1, 2 ** n)}


def null_mean(triples, seed):
    """9,999 within-fork-rung shuffles of the three labels among the three edit grades;
    the mean over permutations of the (advisory - imperative) edit rate, exact."""
    rng = random.Random(seed + 4)
    n = len(triples)
    total = 0
    for _ in range(PERMS):
        for t in triples:
            g = list(t)
            rng.shuffle(g)
            total += g[1] - g[0]  # positions: imperative, advisory, sham
    return F(total, PERMS * n)


def rung_stats(rows_by_fork, seed):
    """rows_by_fork: {fork: {arm: 0/1 edit}} for counted fork-rungs of one rung."""
    n = len(rows_by_fork)
    if n == 0:
        return {"counted": 0, "computable": False}
    e = {arm: sum(v[arm] for v in rows_by_fork.values()) for arm in ARMS3}
    rate = {arm: F(e[arm], n) for arm in ARMS3}
    trip = [(v["imperative"], v["advisory"], v["sham"]) for v in rows_by_fork.values()]
    return {"counted": n, "computable": n >= MIN_COUNTED, "edits": e, "rate": rate,
            "imp_minus_adv": rate["imperative"] - rate["advisory"], "imp_minus_sham": rate["imperative"] - rate["sham"],
            "sign_test": sign_test([(v["advisory"], v["imperative"]) for v in rows_by_fork.values()]),
            "null_mean": null_mean(trip, seed)}


def decide(stats, rule):
    """stats: {rung: rung_stats}. Returns the word and every clause's value."""
    comp = {r: s for r, s in stats.items() if s["computable"]}
    at = comp.get(rule["rung"])
    supported = bool(at and at["imp_minus_adv"] >= rule["margin"] and at["sign_test"]["p"] <= rule["p"]
                     and at["imp_minus_sham"] >= rule["floor"])
    r1 = bool(comp) and all(s["imp_minus_adv"] < rule["refuted"] for s in comp.values())
    r2 = bool(comp) and max(s["imp_minus_sham"] for s in comp.values()) < rule["refuted"]
    if supported:
        word = "supported"
    elif r1 or r2:
        word = "refuted"
    elif not comp or at is None:
        word = "unadjudicated"
    else:
        word = "inconclusive"
    null_ok = bool(comp) and all(abs(s["null_mean"]) <= F(1, 50) for s in comp.values())
    verdict = word if (word == "unadjudicated" or null_ok) else "inconclusive"
    return {"word_before_null_gate": word, "verdict": verdict, "supported": supported, "R1": r1, "R2": r2,
            "null_gate_ok": null_ok, "computable_rungs": sorted(comp), "rung_asked": rule["rung"] if at else None}


def count(grades, edits, rungs):
    """grades: grades.jsonl rows; edits: {(rung, fork, arm): 'yes'|'no'|None}. Returns
    ({rung: {fork: {arm: 0/1}}}, {rung: Counter of exclusion reasons})."""
    by = collections.defaultdict(dict)
    for g in grades:
        by[(g["rung"], g["fork"])][g["arm"]] = g
    counted = {r: {} for r in rungs}
    excluded = {r: collections.Counter() for r in rungs}
    for (rung, fork), arms in sorted(by.items()):
        if rung not in counted:
            continue
        if any(a.get("true") for a in arms.values()):
            excluded[rung]["true nomination (a control figure)"] += 1
            continue
        missing = [a for a in ARMS3 if a not in arms]
        if missing:
            excluded[rung]["an arm row missing"] += 1
            continue
        if any(arms[a].get("error") is not None for a in ARMS3):
            excluded[rung]["an arm row errored"] += 1
            continue
        e = {a: edits.get((rung, fork, a)) for a in ARMS3}
        if any(v not in ("yes", "no") for v in e.values()):
            excluded[rung]["an arm row unjudged on edit"] += 1
            continue
        counted[rung][fork] = {a: int(e[a] == "yes") for a in ARMS3}
    return counted, excluded


def read_record(d):
    d = pathlib.Path(d)
    try:
        grades = [json.loads(l) for l in (d / "grades.jsonl").read_text(encoding="utf-8").splitlines() if l.strip()]
        ids = json.loads((d / "judge" / "ids.json").read_text(encoding="utf-8"))
        kp = d / "judge" / "key.json"
        key = json.loads(kp.read_text(encoding="utf-8"))
        key_sha = hashlib.sha256(kp.read_bytes()).hexdigest()
        seed = json.loads((d / "plan.json").read_text(encoding="utf-8"))["seed"]
    except (OSError, json.JSONDecodeError, KeyError) as e:
        cannot(f"the record cannot be read ({type(e).__name__})")
    for nn, controls in key.items():  # every keyed answer is the ruled answer for its control
        for cid, ans in controls.items():
            ctl = (ids.get(cid) or {}).get("control")
            if ctl not in RULED or ans != RULED[ctl]:
                cannot("judge/key.json keys a control other than as the ruled key does")
    edits, batches = {}, {}
    for nn, controls in sorted(key.items(), key=lambda kv: int(kv[0])):
        tag = f"{int(nn):02d}"
        try:
            items = json.loads((d / "judge" / "batches" / f"batch-{tag}.json").read_text(encoding="utf-8"))["items"]
            got = json.loads((d / "judge" / f"verdicts-{tag}.json").read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError, KeyError):
            batches[tag] = "no accepted instance"
            continue
        if not isinstance(got, list) or [x.get("id") for x in got] != [x["id"] for x in items]:
            batches[tag] = "malformed"
            continue
        by_id = {x["id"]: x for x in got}
        miss = [cid for cid, k in controls.items() for f in ("verdict", "edit")
                if k.get(f) is not None and by_id[cid].get(f) != k[f]]
        if miss:
            batches[tag] = "void (a keyed control missed)"
            continue
        batches[tag] = "accepted"
        for x in got:
            m = ids.get(x["id"])
            if m and "arm" in m:
                edits[(m["rung"], m["fork"], m["arm"])] = x.get("edit")
    return grades, edits, batches, seed, key_sha


def echo_count(d):
    """Items whose shown prose or reasoning carries a nomination line's fixed wording, by arm."""
    d = pathlib.Path(d)
    ids = json.loads((d / "judge" / "ids.json").read_text(encoding="utf-8"))
    out = collections.Counter()
    for p in sorted((d / "judge" / "batches").glob("batch-*.json")):
        for it in json.loads(p.read_text(encoding="utf-8"))["items"]:
            m = ids.get(it["id"])
            if not m or m.get("arm") not in ("imperative", "advisory"):
                continue
            text = ((it.get("prose") or "") + "\n" + (it.get("reasoning") or "")).lower()
            if any(ph in text for phrases in ECHO.values() for ph in phrases):
                out[m["arm"]] += 1
    return dict(out)


def holm(ps):
    order = sorted(ps.items(), key=lambda kv: kv[1])
    m, out, run = len(order), {}, F(0)
    for i, (r, p) in enumerate(order):
        run = max(run, min(F(1), (m - i) * p))
        out[r] = run
    return out


def fr(x):
    return None if x is None else f"{x.numerator}/{x.denominator}"


def apply(rule, record):
    grades, edits, batches, seed, key_sha = read_record(record)
    counted, excluded = count(grades, edits, rule["rungs"])
    stats = {r: rung_stats(counted[r], seed) for r in rule["rungs"]}
    word = decide(stats, rule)
    others = {r: s["sign_test"]["p"] for r, s in stats.items() if s["computable"] and r != rule["rung"]}
    out = {"rule_sha256": RULE_SHA, "key_sha256": key_sha, "key_is_bv2": key_sha == KEY_SHA, "seed": seed, "batches": dict(collections.Counter(batches.values())),
           "rungs": {r: {"counted": s["counted"], "computable": s["computable"], "excluded": dict(excluded[r]),
                         **({"edits": s["edits"], "rate": {a: fr(v) for a, v in s["rate"].items()},
                             "imp_minus_adv": fr(s["imp_minus_adv"]), "imp_minus_sham": fr(s["imp_minus_sham"]),
                             "sign_test": {k: (fr(v) if isinstance(v, F) else v) for k, v in s["sign_test"].items()},
                             "null_mean": fr(s["null_mean"])} if s["counted"] else {})}
                     for r, s in stats.items()},
           "holm_other_rungs": {r: fr(p) for r, p in holm(others).items()},
           "echo_items": echo_count(record), **word}
    return out


def selftest(rule):
    """Made-up counts only; no record is read."""
    def fork_rows(n, imp, adv, sham, disc_adv_only=0):
        """n counted fork-rungs with the given edit counts; disc_adv_only forks where advisory edits and imperative does not."""
        rows = {}
        for i in range(n):
            rows[f"f{i:03d}"] = {"imperative": 0, "advisory": 0, "sham": 0}
        ks = list(rows)
        for i in range(disc_adv_only):
            rows[ks[i]]["advisory"] = 1
        for i in range(imp):
            rows[ks[disc_adv_only + i]]["imperative"] = 1
        for i in range(adv - disc_adv_only):
            rows[ks[disc_adv_only + i]]["advisory"] = 1  # concordant with an imperative edit
        for i in range(sham):
            rows[ks[-1 - i]]["sham"] = 1
        return rows

    def stats_of(spec, seed=924):
        return {r: rung_stats(spec.get(r, {}), seed) for r in rule["rungs"]}

    def at(rows, others=None):
        spec = {r: (others if others is not None else rows) for r in rule["rungs"]}
        spec[rule["rung"]] = rows
        return spec

    base = fork_rows(100, 48, 3, 0)
    cases = [
        ("a large framing difference, sham at 0", at(base), "supported"),
        ("imperative - advisory exactly 0.15 (a tie, included)", at(fork_rows(100, 20, 5, 0)), "supported"),
        ("imperative - advisory 0.14", at(fork_rows(100, 19, 5, 0)), "inconclusive"),
        ("imperative - sham exactly 0.05 at 0.6 (a tie, included)", at(fork_rows(100, 40, 0, 35)), "supported"),
        ("imperative - sham 0.04 at 0.6, 0.06 elsewhere", at(fork_rows(100, 40, 0, 36), fork_rows(100, 40, 0, 34)), "inconclusive"),
        ("imperative clears the sham by less than 0.05 on every rung", at(fork_rows(100, 40, 0, 36)), "refuted"),
        ("imperative - advisory exactly 0.05 on every rung (R1 does not fire)", at(fork_rows(100, 5, 0, 0)), "inconclusive"),
        ("imperative - advisory 0.04 on every rung (R1)", at(fork_rows(100, 4, 0, 0)), "refuted"),
        ("advisory at a sham of 0 does not refute", at(fork_rows(100, 14, 0, 0)), "inconclusive"),
        ("a high sham where imperative peaks, a clear excess elsewhere, supported failing", at(fork_rows(100, 14, 0, 0), fork_rows(100, 50, 0, 48)), "inconclusive"),
        ("p above 0.01 at 0.6 fails supported, not unadjudicated", at(fork_rows(12, 6, 0, 0), fork_rows(100, 30, 0, 0)), "inconclusive"),
        ("0.6 uncomputable, no refuted clause", at({}, fork_rows(100, 30, 0, 0)), "unadjudicated"),
        ("0.6 uncomputable, R1 elsewhere", at({}, fork_rows(100, 2, 0, 0)), "refuted"),
        ("no rung computable", at(fork_rows(9, 9, 0, 0), fork_rows(9, 9, 0, 0)), "unadjudicated"),
        ("exactly ten counted is computable", at(fork_rows(10, 10, 0, 0), fork_rows(9, 9, 0, 0)), "supported"),
    ]
    bad = 0
    for label, spec, want in cases:
        got = decide(stats_of({r: v for r, v in spec.items() if v}), rule)["verdict"]
        bad += got != want
        print(f"{'ok  ' if got == want else 'FAIL'}  {label}: {got} (expected {want})")
    # the null gate: a mean outside 0.02 turns any word but unadjudicated into inconclusive
    s = stats_of(at(base))
    for r in s:
        s[r]["null_mean"] = F(21, 1000)
    got = decide(s, rule)["verdict"]
    bad += got != "inconclusive"
    print(f"{'ok  ' if got == 'inconclusive' else 'FAIL'}  a failed null gate: {got} (expected inconclusive)")
    s = stats_of(at(base))
    for r in s:
        s[r]["null_mean"] = F(1, 50)
    got = decide(s, rule)["verdict"]
    bad += got != "supported"
    print(f"{'ok  ' if got == 'supported' else 'FAIL'}  a null mean exactly 0.02 passes: {got} (expected supported)")
    # the sign test, exact
    st = sign_test([(0, 1)] * 7)
    good = st["p"] == F(1, 128) and st["floor"] == F(1, 128)
    bad += not good
    print(f"{'ok  ' if good else 'FAIL'}  seven discordant, none against: p = floor = 1/128")
    # each exclusion reason, through count()
    g = [{"rung": "0.6", "fork": f, "arm": a, "true": f == "t", "error": ("x" if (f == "e" and a == "sham") else None)}
         for f in ("ok", "t", "e", "m", "u") for a in ARMS3 if not (f == "m" and a == "advisory")]
    ed = {("0.6", f, a): ("yes" if a == "imperative" else "no") for f in ("ok", "t", "e", "m") for a in ARMS3}
    ed[("0.6", "u", "imperative")], ed[("0.6", "u", "advisory")] = "yes", None
    c, x = count(g, ed, ["0.6"])
    good = list(c["0.6"]) == ["ok"] and x["0.6"] == collections.Counter({
        "true nomination (a control figure)": 1, "an arm row errored": 1, "an arm row missing": 1, "an arm row unjudged on edit": 1})
    bad += not good
    print(f"{'ok  ' if good else 'FAIL'}  each exclusion reason counted once, only the clean fork-rung counted")
    # the judge path, through read_record(), on a made-up record of 12 fork-rungs in one batch
    def record(tmp, mutate=None):
        d = pathlib.Path(tmp)
        (d / "judge" / "batches").mkdir(parents=True)
        grades, ids, items, verdicts = [], {}, [], []
        for f in range(12):
            for a in ARMS3:
                rid = f"r{f:02d}{a[:3]}"
                grades.append({"rung": "0.6", "fork": f"f{f}", "arm": a, "true": False, "error": None})
                ids[rid] = {"arm": a, "fork": f"f{f}", "rung": "0.6"}
                items.append({"id": rid})
                verdicts.append({"id": rid, "verdict": "ignores", "edit": "yes" if a == "imperative" else "no"})
        key = {"1": {}}
        for c in ("ctl01", "ctl05", "ctl08", "ctl13"):
            cid = f"k{c}"
            ids[cid] = {"batch": 1, "control": c}
            key["1"][cid] = dict(RULED[c])
            items.append({"id": cid})
            verdicts.append({"id": cid, **{k: (v if v is not None else "declines") for k, v in RULED[c].items()}})
        if mutate:
            mutate(key, verdicts)
        (d / "grades.jsonl").write_text("".join(json.dumps(g) + "\n" for g in grades))
        (d / "judge" / "ids.json").write_text(json.dumps(ids))
        (d / "judge" / "key.json").write_text(json.dumps(key))
        (d / "judge" / "batches" / "batch-01.json").write_text(json.dumps({"batch": 1, "items": items}))
        (d / "judge" / "verdicts-01.json").write_text(json.dumps(verdicts))
        (d / "plan.json").write_text(json.dumps({"seed": 924}))
        return d

    def judged(label, mutate, want_counted, want_batch):
        with tempfile.TemporaryDirectory() as tmp:
            try:
                g, ed, bt, seed, _ = read_record(record(tmp, mutate))
            except SystemExit as e:
                got = ("refused", e.code)
            else:
                c, _ = count(g, ed, ["0.6"])
                got = (len(c["0.6"]), bt.get("01"))
        want = (want_counted, want_batch)
        print(f"{'ok  ' if got == want else 'FAIL'}  {label}: {got} (expected {want})")
        return got != want

    bad += judged("a batch whose controls all read as keyed is accepted", None, 12, "accepted")
    bad += judged("a batch missing a keyed control is void, its rows unjudged",
                  lambda k, v: v[-4].__setitem__("edit", "no"), 0, "void (a keyed control missed)")
    bad += judged("ctl05, keyed on edit only, is not missed on its verdict",
                  lambda k, v: next(x for x in v if x["id"] == "kctl05").__setitem__("verdict", "accepts"), 12, "accepted")
    bad += judged("a batch whose ids are out of order is malformed, its rows unjudged",
                  lambda k, v: v.insert(0, v.pop()), 0, "malformed")
    bad += judged("a key that answers a control otherwise than the ruled key is refused",
                  lambda k, v: k["1"]["kctl08"].__setitem__("edit", "yes"), "refused", 2)
    total = len(cases) + 10
    print(f"apply_bprime selftest: {total - bad} of {total} fixtures read as expected (made-up counts only)")
    return 1 if bad else 0


def main(argv):
    if len(argv) == 2 and argv[0] == "--selftest":
        return selftest(load_rule(argv[1]))
    if len(argv) != 2:
        cannot("usage: apply_bprime.py RULE_TOML RECORD_DIR, or --selftest RULE_TOML")
    print(json.dumps(apply(load_rule(argv[0]), argv[1]), indent=1))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except SystemExit:
        raise
    except Exception as e:  # never a traceback, which would print the caller's paths
        cannot(f"stopped on {type(e).__name__}")
