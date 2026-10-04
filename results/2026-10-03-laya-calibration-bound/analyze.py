#!/usr/bin/env python3
"""#165's re-judge and laya analysis, under pre-registration 5962826904 and amendments 1-6. Stdlib only.

Reads, relative to DIR (the results directory):
  rejudge/sample.json                  the 400 drawn items
  rejudge/judge/ids.json               batch id -> state sha, control flag, pass, batch
  rejudge/judge/verdicts-NN.json       the 36 accepted batches (29 = 29a, ruled 5973836546)
  laya/requests.jsonl                  800 requests
  laya/fp32.jsonl                      laya's fp32 PyTorch forward
  laya/passA/responses-cpu_and_ne.jsonl, laya/passB/responses-cpu_and_gpu.jsonl
Writes OUT (results.json). Usage: analyze.py DIR OUT.

Seeds are the draw's SEED plus the offsets the pre-registration fixes: +9 the bootstrap of d, +10 the floor,
+11 the cross-fit split, +12 the ECE bootstrap. Each independent stream is random.Random seeded with the string
"<seed+offset>:<what>", so the streams can run in parallel and the output is the same bytes on any machine."""
import collections, hashlib, json, math, multiprocessing, pathlib, random, sys

SEED = int(hashlib.sha256(b"discipline #165 re-judge sample, 2026-10-02").hexdigest()[:16], 16)
BOOT, FLOOR_DRAWS, FLOOR_NS, BINS = 9999, 10000, [100, 200, 400, 800, 1600], 15
FLIP_MARGIN_MEASURED = 0.0242  # the grade of record (report 7cb25496...): largest fp32 margin among ANE decision changes
BAND_FLOOR = 0.05              # 5973863758: the band is widened to at least 0.05 logits
T_RANGE, T_TOL = (0.05, 20.0), 1e-4
OPTS = {"verdict": ["accepts", "questions", "declines", "ignores"], "edit": ["yes", "no"]}
PASSES = {"A": "cpu_and_ne", "B": "cpu_and_gpu"}


def rng(offset, what): return random.Random(f"{SEED + offset}:{what}")
def r4(x): return round(x, 4)


def pct(xs, q):
    """The pth percentile, linear between closest ranks (numpy's default)."""
    s = sorted(xs); h = (len(s) - 1) * q; lo = math.floor(h)
    return s[lo] + (s[min(lo + 1, len(s) - 1)] - s[lo]) * (h - lo)


def softmax_T(logits, T):
    z = [l / T for l in logits]; m = max(z); e = [math.exp(v - m) for v in z]; t = sum(e)
    return [v / t for v in e]


def nll(L, y, T): return -sum(math.log(max(softmax_T(l, T)[c], 1e-300)) for l, c in zip(L, y)) / len(y)


def fit_T(L, y):
    """Golden-section search for the NLL-minimising temperature in T_RANGE, to T_TOL (amendment 4)."""
    g = (math.sqrt(5) - 1) / 2; a, b = T_RANGE
    c, d = b - g * (b - a), a + g * (b - a); fc, fd = nll(L, y, c), nll(L, y, d)
    while b - a > T_TOL:
        if fc < fd: b, d, fd = d, c, fc; c = b - g * (b - a); fc = nll(L, y, c)
        else: a, c, fc = c, d, fd; d = a + g * (b - a); fd = nll(L, y, d)
    return (a + b) / 2


def ece_top1(P, y):
    """Top-1 confidence, BINS equal-width bins on [0, 1], |accuracy - confidence| weighted by bin share."""
    n_b, acc_b, conf_b = [0] * BINS, [0.0] * BINS, [0.0] * BINS
    for p, c in zip(P, y):
        conf = max(p); k = min(int(conf * BINS), BINS - 1)
        n_b[k] += 1; conf_b[k] += conf; acc_b[k] += p.index(conf) == c
    return sum(abs(acc_b[k] - conf_b[k]) for k in range(BINS) if n_b[k]) / len(y)


def ece_classwise(P, y):
    out = []
    for c in range(len(P[0])):
        n_b, t_b, p_b = [0] * BINS, [0.0] * BINS, [0.0] * BINS
        for p, t in zip(P, y):
            k = min(int(p[c] * BINS), BINS - 1); n_b[k] += 1; p_b[k] += p[c]; t_b[k] += t == c
        out.append(sum(abs(t_b[k] - p_b[k]) for k in range(BINS) if n_b[k]) / len(y))
    return sum(out) / len(out)


def boot_d(a):
    r = rng(9, "d"); n = len(a)
    return [1 - sum(a[r.randrange(n)] for _ in range(n)) / n for _ in range(BOOT)]


def boot_ece(args):
    what, P, y = args; r = rng(12, what); n = len(y); out = []
    for _ in range(BOOT):
        idx = [r.randrange(n) for _ in range(n)]
        out.append(ece_top1([P[i] for i in idx], [y[i] for i in idx]))
    return out


def floor_task(args):
    """Median ECE of a perfectly calibrated predictor at n. F1: n of the served pass's cross-fitted probability
    vectors drawn with replacement, each label drawn from its own vector. F2: every item predicted at the
    question's fresh-majority base rates, the label drawn from them (one bin, so ECE = |freq(top) - p_top|)."""
    f, n, Pcf, base = args; r = rng(10, f"{f}:{n}")
    top = max(range(len(base)), key=lambda k: base[k]); cum = list(_accumulate(base))
    f1, f2 = [], []
    for _ in range(FLOOR_DRAWS):
        P = [Pcf[r.randrange(len(Pcf))] for _ in range(n)]
        y = [_draw(p, r.random()) for p in P]
        f1.append(ece_top1(P, y))
        hits = sum(_index(cum, r.random()) == top for _ in range(n))
        f2.append(abs(hits / n - base[top]))
    return f, n, r4(pct(f1, 0.5)), r4(pct(f2, 0.5))


def _accumulate(p):
    s = 0.0
    for v in p: s += v; yield s


def _index(cum, u):
    for k, c in enumerate(cum):
        if u < c: return k
    return len(cum) - 1


def _draw(p, u): return _index(list(_accumulate(p)), u)


def e3(d): return 3 * d * d - 2 * d ** 3


def pair_agree(ls): return sum(ls[i] == ls[j] for i, j in ((0, 1), (0, 2), (1, 2))) / 3


def majority(ls):
    lab, k = collections.Counter(ls).most_common(1)[0]
    return lab if k >= 2 else None


def fleiss(rows, cats):
    N, n = len(rows), 3
    P_i = [(sum(r.count(c) ** 2 for c in cats) - n) / (n * (n - 1)) for r in rows]
    p_j = [sum(r.count(c) for r in rows) / (N * n) for c in cats]
    Pe = sum(p * p for p in p_j)
    return (sum(P_i) / N - Pe) / (1 - Pe)


def main(D, OUT, pool):
    sha = lambda p: hashlib.sha256((D / p).read_bytes()).hexdigest()
    J = D / "rejudge/judge"
    sample = json.loads((D / "rejudge/sample.json").read_text())
    ids = json.loads((J / "ids.json").read_text())
    fresh = collections.defaultdict(lambda: collections.defaultdict(list))
    for b in range(1, 37):
        for v in json.loads((J / f"verdicts-{b:02d}.json").read_text()):
            meta = ids[v["id"]]; assert meta["batch"] == f"{b:02d}"
            if not meta["control"]:
                for f in OPTS: fresh[meta["state_sha256"]][f].append((meta["pass"], v.get(f)))
    items = [s["state_sha256"] for s in sample]
    assert set(items) == set(fresh) and len(items) == 400
    stratum = {s["state_sha256"]: (s["record"], s["verdict"]) for s in sample}
    archive = {s["state_sha256"]: {"verdict": s["verdict"], "edit": s["edit"]} for s in sample}
    labels = {}
    for s in items:
        labels[s] = {}
        for f in OPTS:
            js = sorted(fresh[s][f]); assert [p for p, _ in js] == [1, 2, 3], (s, f)
            labels[s][f] = [l for _, l in js]; assert all(l in OPTS[f] for l in labels[s][f])

    # ---------- judges ----------
    judges, maj = {}, {}
    agree = {f: [pair_agree(labels[s][f]) for s in items] for f in OPTS}
    boots = dict(zip(OPTS, pool.map(boot_d, [agree[f] for f in OPTS])))
    for f in OPTS:
        a = agree[f]; d = 1 - sum(a) / len(a); lo, hi = pct(boots[f], 0.025), pct(boots[f], 0.975)
        maj[f] = {s: majority(labels[s][f]) for s in items}
        have = [s for s in items if maj[f][s] is not None]
        groups = {"stratum": collections.defaultdict(list), "record": collections.defaultdict(list)}
        for s, x in zip(items, a):
            groups["stratum"][" | ".join(stratum[s])].append(x); groups["record"][stratum[s][0]].append(x)
        rep = collections.defaultdict(lambda: [0, 0])
        for s in have:
            k = " | ".join(stratum[s]); rep[k][0] += archive[s][f] == maj[f][s]; rep[k][1] += 1
        ag = sum(archive[s][f] == maj[f][s] for s in have)
        judges[f] = {
            "n_items": len(items), "d": r4(d), "d_ci95": [r4(lo), r4(hi)], "e3": r4(e3(d)), "e3_ci95": [r4(e3(lo)), r4(e3(hi))],
            "fleiss_kappa": r4(fleiss([labels[s][f] for s in items], OPTS[f])), "no_majority": len(items) - len(have),
            "majority_counts": dict(sorted(collections.Counter(maj[f][s] for s in have).items())),
            **{f"per_{k}_d": {g: {"n": len(v), "d": r4(1 - sum(v) / len(v))} for g, v in sorted(groups[k].items())} for k in groups},
            "archive_vs_fresh_majority": {"n": len(have), "agree": ag, "rate": r4(ag / len(have)),
                                          "per_stratum": {k: {"n": v[1], "agree": v[0]} for k, v in sorted(rep.items())}}}

    # ---------- laya ----------
    reqs = {(r["item"], r["field"]): r for r in map(json.loads, open(D / "laya/requests.jsonl"))}
    for (s, f), r in reqs.items(): assert [l.split(":")[0] for l in r["request"]["candidate_labels"]] == OPTS[f]
    fp32 = {(r["item"], r["field"]): r for r in map(json.loads, open(D / "laya/fp32.jsonl"))}
    rows = {}
    for name, units in PASSES.items():
        rows[name] = {(r["item"], r["field"]): r for r in map(json.loads, open(D / f"laya/pass{name}/responses-{units}.jsonl"))}
        assert set(rows[name]) == set(reqs) == set(fp32)
    probs = lambda n, s, f: rows[n][(s, f)]["response"]["data"][0]["probs"]
    argmax = lambda v: max(range(len(v)), key=lambda k: v[k])

    # the cross-fit split: each stratum, sorted, shuffled by one random.Random(seed+11) in stratum order,
    # concatenated, then dealt A, B, A, B ... so the folds are 200/200 and every stratum splits to within one
    r11 = random.Random(SEED + 11); flat = []
    for k in sorted(set(stratum.values())):
        grp = sorted(s for s in items if stratum[s] == k); r11.shuffle(grp); flat += grp
    fold = {s: "AB"[i % 2] for i, s in enumerate(flat)}

    laya, jobs, Pcf_keep = {}, [], {}
    for f in OPTS:
        have = [s for s in items if maj[f][s] is not None]
        y = [OPTS[f].index(maj[f][s]) for s in have]
        margin = [fp32[(s, f)]["torch"]["margin"] for s in have]
        band = max(FLIP_MARGIN_MEASURED, BAND_FLOOR); abstain = [m < band for m in margin]
        per = {}
        for name in PASSES:
            L = [[math.log(p) for p in probs(name, s, f)] for s in have]
            fv = [fold[s] for s in have]; Ts = {}; Pcf = [None] * len(have)
            for tr, te in (("B", "A"), ("A", "B")):
                Ts[tr] = fit_T([l for l, x in zip(L, fv) if x == tr], [c for c, x in zip(y, fv) if x == tr])
                for i, x in enumerate(fv):
                    if x == te: Pcf[i] = softmax_T(L[i], Ts[tr])
            P1 = [softmax_T(l, 1.0) for l in L]
            pred = [argmax(l) for l in L]; ok = [p == c for p, c in zip(pred, y)]
            ans = [o for o, ab in zip(ok, abstain) if not ab]
            per[name] = {
                "compute_units": PASSES[name], "T_fit_on_fold": {k: r4(v) for k, v in Ts.items()}, "T_deployed_all": r4(fit_T(L, y)),
                "T_at_range_bound": {k: abs(v - T_RANGE[1]) < 10 * T_TOL or abs(v - T_RANGE[0]) < 10 * T_TOL for k, v in Ts.items()},
                "ece_top1_T1": r4(ece_top1(P1, y)), "ece_top1_crossfit": r4(ece_top1(Pcf, y)),
                "ece_top1_crossfit_by_fold": {k: r4(ece_top1([p for p, x in zip(Pcf, fv) if x == k], [c for c, x in zip(y, fv) if x == k])) for k in "AB"},
                "ece_classwise_T1": r4(ece_classwise(P1, y)), "ece_classwise_crossfit": r4(ece_classwise(Pcf, y)),
                "agreement_with_fresh_majority": {"n": len(y), "agree": sum(ok), "rate": r4(sum(ok) / len(y)),
                                                  "answered_n": len(ans), "answered_agree": sum(ans), "answered_rate": r4(sum(ans) / len(ans)),
                                                  "coverage": r4(len(ans) / len(y))},
                "predicted_counts": {OPTS[f][k]: v for k, v in sorted(collections.Counter(pred).items())},
                "confusion_majority_by_predicted": {OPTS[f][t]: [sum(1 for c, p in zip(y, pred) if c == t and p == q) for q in range(len(OPTS[f]))] for t in range(len(OPTS[f]))},
            }
            jobs += [(f"{f}:{name}:T1", P1, y), (f"{f}:{name}:crossfit", Pcf, y)]
            Pcf_keep[(f, name)] = Pcf
        pa = {n: [argmax(probs(n, s, f)) for s in items] for n in PASSES}
        f32 = [argmax(fp32[(s, f)]["torch"]["logits"]) for s in items]
        m400 = [fp32[(s, f)]["torch"]["margin"] for s in items]
        same = lambda u, v: sum(a == b for a, b in zip(u, v))
        laya[f] = {"n_with_majority": len(have), "majority_class_rate": r4(max(collections.Counter(y).values()) / len(y)),
                   "abstention_band_logits": band, "abstain_n": sum(abstain), "passes": per,
                   "parity_400": {"A_vs_B_agree": same(pa["A"], pa["B"]), "A_vs_fp32_agree": same(pa["A"], f32), "B_vs_fp32_agree": same(pa["B"], f32),
                                  "fp32_margins_of_changed": {n: sorted(r4(m400[i]) for i in range(400) if pa[n][i] != f32[i]) for n in PASSES},
                                  "fp32_margin_lt_0p05": sum(m < 0.05 for m in m400)}}
        base = [c / len(y) for c in [y.count(k) for k in range(len(OPTS[f]))]]
        laya[f]["_base"] = base

    ece_boot = dict(zip([j[0] for j in jobs], pool.map(boot_ece, jobs)))
    for f in OPTS:
        for name in PASSES:
            for kind in ("T1", "crossfit"):
                b = ece_boot[f"{f}:{name}:{kind}"]
                laya[f]["passes"][name][f"ece_top1_{kind}_ci95"] = [r4(pct(b, 0.025)), r4(pct(b, 0.975))]

    # ---------- the attainable floor ----------
    tasks = [(f, n, Pcf_keep[(f, "A")], laya[f]["_base"]) for f in OPTS for n in sorted(set(FLOOR_NS + [laya[f]["n_with_majority"]]))]
    floor = {f: {"held_out_n": laya[f]["n_with_majority"], "base_rates": {o: r4(b) for o, b in zip(OPTS[f], laya[f]["_base"])}, "median_ece": {}} for f in OPTS}
    for f, n, a, b in pool.map(floor_task, tasks):
        floor[f]["median_ece"][str(n)] = {"F1_consistency": a, "F2_base_rate": b}

    # ---------- the rule (amendment 1 as amended by 5966195560) ----------
    rule = {}
    for f in OPTS:
        nh = str(floor[f]["held_out_n"]); m = laya[f]["passes"]["A"]["ece_top1_crossfit"]
        e, (elo, ehi) = judges[f]["e3"], judges[f]["e3_ci95"]; rule[f] = {}
        for c in ("F1_consistency", "F2_base_rate"):
            fl = floor[f]["median_ece"][nh][c]; lo, hi = r4(elo + fl), r4(ehi + fl)
            rule[f][c] = {"floor": fl, "bound": r4(e + fl), "bound_interval": [lo, hi], "measured_crossfit_ece_passA": m,
                          "reading": "calibrated" if m < lo else "miscalibrated" if m > hi else "inconclusive",
                          "size_rule_fires": fl >= r4(e + fl)}

    # ---------- tokens, buckets, provenance ----------
    tok = {}
    for name in PASSES:
        rs = list(rows[name].values()); h = lambda k: dict(sorted(collections.Counter(r["headers"][k] for r in rs).items()))
        tok[name] = {"buckets": h("sidekick-buckets"), "compute_units": h("sidekick-compute-units"), "version": h("sidekick-version"),
                     "model": h("sidekick-model"), "possibly_truncated_bucket_1024": sum(r["headers"]["sidekick-buckets"] == "1024" for r in rs),
                     "prompt_tokens_at_1024": sum(r["response"]["usage"]["prompt_tokens"] >= 1024 for r in rs),
                     "tokens_match_fp32": sum(r["response"]["usage"]["prompt_tokens"] == fp32[(r["item"], r["field"])]["n_tokens"] for r in rs)}
    tok["fp32_at_max_len"] = sum(r["at_max_len"] for r in fp32.values())

    for f in OPTS: laya[f].pop("_base")
    res = {"seed": SEED, "judges": judges, "laya": laya, "floor": floor, "rule": rule, "tokens": tok,
           "folds": dict(sorted(collections.Counter(fold.values()).items())),
           "inputs": {p: sha(p) for p in ["rejudge/sample.json", "rejudge/judge/ids.json", "laya/requests.jsonl", "laya/fp32.jsonl",
                                           "laya/passA/responses-cpu_and_ne.jsonl", "laya/passB/responses-cpu_and_gpu.jsonl"]
                      + [f"rejudge/judge/verdicts-{b:02d}.json" for b in range(1, 37)]}}
    OUT.write_text(json.dumps(res, indent=1, sort_keys=True) + "\n")


if __name__ == "__main__":
    with multiprocessing.Pool() as pool:
        main(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), pool)
