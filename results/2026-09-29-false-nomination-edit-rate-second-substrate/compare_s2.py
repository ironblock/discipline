#!/usr/bin/env python3
"""The #142 comparison applier: the imperative edit rate at the 0.6 rung on the
second substrate against the 27B's, paired by fork, under comparison-rule.toml
(pinned). Counting and the edit source are apply_bprime.py's (pinned), read over
both records, so a fork counts here exactly when it counts for each word.
Usage: compare_s2.py RULE_TOML APPLY_BPRIME RECORD_27B RECORD_S2 | --selftest RULE_TOML APPLY_BPRIME"""
import hashlib, importlib.util, json, pathlib, sys
from fractions import Fraction as F
from math import comb
RULE_SHA = "3c5c2974eaeb1f12ed63ebdf4494dccef9193da67760067a590ec696e3bc6d4c"
APPLIER_SHA = "7a819a891237ad689515e684b8a4f51ac9f7ef243961a4a853fd1b96b1e4c32b"
def cannot(m): print(f"compare_s2: {m}", file=sys.stderr); raise SystemExit(2)
def pinned(path, sha, what):
    p = pathlib.Path(path)
    if not p.is_file() or hashlib.sha256(p.read_bytes()).hexdigest() != sha: cannot(f"{what} is missing or not the pinned bytes")
    return p
def load(rule_path, applier_path):
    import tomllib
    rule = tomllib.loads(pinned(rule_path, RULE_SHA, "the comparison rule").read_text())["comparison"]
    spec = importlib.util.spec_from_file_location("ab", pinned(applier_path, APPLIER_SHA, "apply_bprime.py")); ab = importlib.util.module_from_spec(spec); spec.loader.exec_module(ab)
    return rule, ab
def decide(rule, big, small, sham_rates):
    """big, small: {fork: 0/1 imperative edit} over counted forks at the rung; sham_rates: this substrate's sham rate per computable rung."""
    m = {k: F(v) for k, v in rule["margins"].items() if k != "min_shared"}; min_shared = rule["margins"]["min_shared"]
    if any(r > m["sham"] for r in sham_rates.values()):
        return {"word": "control_failed"}
    shared = sorted(set(big) & set(small)); n = len(shared)
    if n < min_shared:
        return {"word": "unadjudicated", "shared": n}
    rb, rs = F(sum(big[f] for f in shared), n), F(sum(small[f] for f in shared), n); d = rb - rs
    disc = [(big[f], small[f]) for f in shared if big[f] != small[f]]; nd = len(disc); k = sum(1 for b, s in disc if s and not b)
    p = F(sum(comb(nd, i) for i in range(k + 1)), 2 ** nd) if nd else F(1); floor = F(1, 2 ** nd) if nd else F(1)
    if d >= m["dependent"] and floor <= m["p"] and p <= m["p"]: word = "substrate_dependent"
    elif abs(d) < m["independent"]: word = "substrate_independent"
    else: word = "inconclusive"
    fr = lambda x: f"{x.numerator}/{x.denominator}"
    return {"word": word, "shared": n, "rate_27B": fr(rb), "rate_S2": fr(rs), "difference": fr(d), "sign": "predicted" if d > 0 else ("unpredicted" if d < 0 else "none"),
            "sign_test": {"n_discordant": nd, "k_S2_only": k, "p": fr(p), "floor": fr(floor)}}
def imperative_edits(ab, rule_path_bprime, record, rung):
    r = ab.load_rule(rule_path_bprime); g, e, b, seed, _ = ab.read_record(record); c, _ = ab.count(g, e, r["rungs"])
    return {f: v["imperative"] for f, v in c[rung].items()}, {rr: F(sum(v["sham"] for v in c[rr].values()), len(c[rr])) for rr in r["rungs"] if len(c[rr]) >= ab.MIN_COUNTED}
def selftest(rule, ab):
    mk = lambda n, ones, off=0: {f"f{i:03d}": int(off <= i < off + ones) for i in range(n)}
    cases = [
        ("a large predicted drop reads substrate_dependent", decide(rule, mk(99, 53), mk(99, 20), {"0.6": F(0)})["word"], "substrate_dependent"),
        ("a drop of exactly 0.15 is dependent (tie included)", decide(rule, mk(100, 50), mk(100, 35), {"0.6": F(0)})["word"], "substrate_dependent"),
        ("a drop of 0.14 is inconclusive", decide(rule, mk(100, 50), mk(100, 36), {"0.6": F(0)})["word"], "inconclusive"),
        ("a difference of 0.04 is substrate_independent", decide(rule, mk(100, 50), mk(100, 46), {"0.6": F(0)})["word"], "substrate_independent"),
        ("a difference of exactly 0.05 is not independent", decide(rule, mk(100, 50), mk(100, 45), {"0.6": F(0)})["word"], "inconclusive"),
        ("a rise (unpredicted) is inconclusive with its sign", decide(rule, mk(100, 30), mk(100, 60), {"0.6": F(0)})["sign"], "unpredicted"),
        ("fewer than 10 shared forks is unadjudicated", decide(rule, mk(9, 9), mk(9, 0), {"0.6": F(0)})["word"], "unadjudicated"),
        ("a sham above 0.05 is control_failed", decide(rule, mk(99, 53), mk(99, 20), {"0.6": F(6, 100)})["word"], "control_failed"),
        ("a sham at exactly 0.05 is not a failure", decide(rule, mk(100, 50), mk(100, 46), {"0.6": F(5, 100)})["word"], "substrate_independent"),
        ("a floor above 0.01 blocks dependent (6 discordant)", decide(rule, mk(12, 6), mk(12, 0), {"0.6": F(0)})["word"], "inconclusive"),
        ("only shared forks count", decide(rule, {**mk(20, 10), "x": 1}, {**mk(20, 10), "y": 0}, {"0.6": F(0)})["shared"], 20),
    ]
    bad = 0
    for label, got, want in cases:
        bad += got != want; print(f"{'ok  ' if got == want else 'FAIL'}  {label}: {got}")
    print(f"compare_s2 selftest: {len(cases) - bad} of {len(cases)}"); return 1 if bad else 0
def main(a):
    if len(a) == 3 and a[0] == "--selftest":
        rule, ab = load(a[1], a[2]); return selftest(rule, ab)
    if len(a) != 5: cannot("usage: compare_s2.py RULE_TOML APPLY_BPRIME RECORD_27B RECORD_S2 BPRIME_RULE, or --selftest RULE_TOML APPLY_BPRIME")
    rule, ab = load(a[0], a[1]); rung = rule["rung"]
    big, _ = imperative_edits(ab, a[4], a[2], rung); small, sham = imperative_edits(ab, a[4], a[3], rung)
    print(json.dumps({"rule_sha256": RULE_SHA, **decide(rule, big, small, sham)}, indent=1)); return 0
if __name__ == "__main__": raise SystemExit(main(sys.argv[1:]))
