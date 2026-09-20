#!/usr/bin/env bash
# Gate 0 for this directory: every number the report states re-derives from the
# artefacts committed beside it -- and here the product IS a derivation, so it
# is re-derived whole. `report.json` and `decision-rule.toml` are hashed
# against the digests the record declares, the rule is applied to the report
# by the applier below, and the result must be byte-for-byte the committed
# `verdict.json`. A verdict edited after the fact, a rule edited after the
# fact, and a report swapped for another run's are each caught.
#
# WHAT THIS DOES NOT DO: it does not re-run the bakeoff. The cells in
# `report.json` are taken at their digest; a metric computed wrongly by the
# binary that wrote them is caught in that directory's gate 0, not here.
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"

python3 - <<'PY'
import hashlib
import json
import pathlib
import sys

FENCE = "+++"


# 0 CLEAN, 1 FOUND SOMETHING, 2 COULD NOT RUN -- the repository's contract.
def cannot_run(message):
    print(f"recompute: {message}", file=sys.stderr)
    raise SystemExit(2)


# A Python without tomllib cannot read the rule, and that is a 2, not a
# traceback that reads as "the numbers do not re-derive".
try:
    import tomllib
except ImportError:
    cannot_run(f"this needs Python 3.11 or later for tomllib; this is {sys.version.split()[0]}")


def read(path):
    try:
        return pathlib.Path(path).read_text(encoding="utf-8")
    except OSError as err:
        cannot_run(f"{path} cannot be read: {err.strerror}")


def digest(path):
    try:
        return hashlib.sha256(pathlib.Path(path).read_bytes()).hexdigest()
    except OSError as err:
        sys.exit(f"{path} is consumed by the record and is not here: {err.strerror}")


text = read("README.md")
if not text.startswith(FENCE + "\n"):
    cannot_run("README.md does not open with +++ front-matter")
try:
    front = tomllib.loads(text.split(FENCE + "\n", 2)[1])
except (tomllib.TOMLDecodeError, IndexError) as err:
    cannot_run(f"README.md front-matter is not TOML: {err}")

rows = []
for number, line in enumerate(read("run.jsonl").splitlines(), start=1):
    if not line.strip():
        continue
    try:
        rows.append(json.loads(line))
    except json.JSONDecodeError as err:
        cannot_run(f"run.jsonl line {number} is not JSON: {err.msg}")
summary = next((row for row in rows if row.get("record") == "summary"), None)
if summary is None:
    cannot_run("run.jsonl has no summary row")
claims = [row for row in rows if row.get("record") == "claim"]
if len(claims) != 1:
    cannot_run(f"run.jsonl carries {len(claims)} claim rows, this directory adjudicates one")

consumed = claims[0].get("consumes", [])
if not consumed:
    sys.exit("the claim consumes nothing, so there is nothing here to re-derive from")
matched = 0
for artifact in consumed:
    found = digest(artifact["path"])
    if found != artifact["sha256"]:
        sys.exit(f"{artifact['path']} is declared {artifact['sha256']} and hashes to {found}")
    matched += 1
for field, counted in (("targets_checked", len(consumed)), ("targets_matched", matched)):
    if summary[field] != counted:
        sys.exit(f"the summary says {field} is {summary[field]}, the rows hold {counted}")
    if front.get(field) not in (None, counted):
        sys.exit(f"the front-matter says {field} is {front[field]}, the rows hold {counted}")

product = digest("verdict.json")
for where, stated in (("front-matter", front["product_sha256"]),
                      ("the summary row", summary["product_sha256"])):
    if stated != product:
        sys.exit(f"{where} states product_sha256 {stated}, the product hashes to {product}")

# THE APPLIER, verbatim. --------------------------------------------------
# THE RULE APPLIER, VERSION 2, for the entry-to-turn nomination run: the v1
# applier under the ceiling-scaled precision bars the maintainer worded on #17
# (5747481065) -- "supported requires pooled precision >= 0.80 x ceiling at
# k = 5, and the refuted clause's precision floor is 0.60 x ceiling likewise;
# the d' margins have no arithmetic ceiling and stand as ratified" -- and with
# the pre-gate sub-rule read on the statistic it names, the drive-resampled
# paired bootstrap of pooled precision between the gate arms that the verb
# emits since #99 under `gate_comparisons[].precision_at_k`. The admitted-only
# precision the verb now emits is reported beside every cell, never adjudicated.
# Sits beside the v1 adjudication; both rules and both verdicts are in the ledger.
#
# Below this line the v1 applier's text, edited where the v2 wording bites.
#
# THE RULE APPLIER for the entry-to-turn nomination run. One function from two
# committed files to one verdict, so that a reader holding `report.json` and
# `decision-rule.toml` can re-derive `verdict.json` without this directory's
# author in the room. Ratified on #17 as amended (5739179890): "for this run the
# rule is fixed", applied mechanically after the numbers exist, which is not
# choosing after seeing them because the rule was written before they did.
#
# The ratified text, as amended, settles what #84 had to disclose: the floor's
# separation is read on the MATCHED cell (same register, scoring, gate); the
# refuted clause is the maximum separation over all scored contender cells
# against its matched floor cell; a margin that cannot be computed satisfies
# neither bound; the budget ladder is named; the primary register is named
# (`intent`); the sub-rule's `supported` needs the gate-arm paired bootstrap,
# which report.json now carries under `gate_comparisons`, or it is
# `unadjudicated`; and the two conjuncts never fold into one word.
#
# Readings still taken here, each stated on the verdict under `readings`:
#   1. "over all scored contender cells" is read over the PRIMARY register's
#      cells, since the rule adjudicates that register; the authored-sense
#      register is scored and reported beside, never adjudicated (the reading
#      #84 took for its reversal register).
#   2. "paired-bootstrap p between the arms below the attainable floor": ruled
#      on #17 (5743190831): the sub-rule names ONE comparison per contender
#      pair -- the anchored arm against the ungated arm at k = 5 on the primary
#      register -- and no correction applies to it; the attainable floor is
#      that test's own floor, so the clause holds when the uncorrected p sits
#      at the floor (the resampling never crossed zero). Holm correction is for
#      the family of secondary gate-arm comparisons and is reported beside;
#      the Holm-corrected readings are carried under `alternatives`.
#   3. A cell is a (scoring, gate) pair per embedder, as the pre-registration
#      lists them, so a gate arm is its own cell (#84's reading, ruled).

PRIMARY_REGISTER = "intent"
LOOSE_ALPHA = 0.05

# The envelope: every parameter the run fixes, cited against its component's
# recommended operating range (ruled on #17, 5744340773). A parameter outside
# its envelope labels the verdict `characterization`; the facts are measured on
# the register and declared in the cache metas, not typed from memory.
ENVELOPE = [
    {"parameter": "budget ladder", "value": "[1, 3, 5, 10, 25, 50], k = 5 adjudicated", "range": "the collector's per-session nomination budget, ratified on #24/#69; the ladder is the design's own", "within": True},
    {"parameter": "anchored pre-gate", "value": "two distinct anchors must recur", "range": "the ratified pre-registration's; the shipped tier 0 fires on one, carried as a reading", "within": True},
    {"parameter": "bge-small-en-v1.5 query instruction", "value": "not used", "range": "the card recommends its query instruction for short-query retrieval; declared unused on #24's regime", "within": False},
    {"parameter": "embeddinggemma-300m prompts", "value": "documented document and retrieval-query prompts, byte for byte", "range": "the model card's", "within": True},
    {"parameter": "Qwen3-Embedding-0.6B instruction byte form", "value": "`Instruct: {task}\\nQuery: ` with a space after `Query:`", "range": "the card emits no space; the space is the ruling's byte form declared on #24", "within": False},
    {"parameter": "Qwen3-Embedding-0.6B document prefix", "value": "none", "range": "the card: documents need no instruction", "within": True},
    {"parameter": "pooling per embedder", "value": "the model's shipped modules (mean; CLS; the graph's head; last token)", "range": "each model's documented pooling", "within": True},
]


def envelope_from_metas(metas):
    """The two sentence-transformers cells' sequence limits and truncation counts, read from
    the cache metas committed and consumed beside this applier; nothing typed."""
    rows = []
    for cid in ("all-MiniLM-L6-v2", "bge-small-en-v1.5"):
        m = metas.get(cid)
        if m is None:
            rows.append({"parameter": f"{cid} sequence length", "value": "meta not consumed", "range": "unknown", "within": False}); continue
        n = len(m.get("truncated", [])); lim = m.get("max_seq_length")
        rows.append({"parameter": f"{cid} sequence length", "value": f"max_seq_length {lim}, silent truncation",
                     "range": f"{n} of {m.get('texts')} cached texts exceed {lim} tokens, listed by digest in the meta", "within": n == 0})
    g = metas.get("embeddinggemma-300m.document")
    n = len(g.get("truncated", [])) if g else None
    rows.append({"parameter": "embeddinggemma-300m context", "value": f"{g.get('max_tokens') if g else '?'} positions, {n} documents truncated to the context less the closing token" if g else "meta not consumed",
                 "range": "the model's documented context; the truncated documents are listed by digest in the meta", "within": (n == 0) if g else False})
    return rows


def ceiling(by_drive, k):
    """The precision a pooled per-drive budget can attain: every drive's top-k
    filled with positives first, pooled -- sum of min(k, positives) over sum of
    min(k, rows). None when no drive has a row. Exact, not rounded: the v2 bar
    is a fraction of it and rounding twice moved the fourth decimal (review of
    PR #100, M1); rounded only where it is written."""
    hits = sum(min(k, d.get("positive", 0)) for d in by_drive.values())
    of = sum(min(k, sum(d.values())) for d in by_drive.values())
    return None if of == 0 else hits / of


def metric_at(cell, name, budget):
    found = [r["value"] for r in cell["metrics"] if r["metric"] == name and r["budget"] == budget]
    if len(found) > 1:
        raise ValueError(f"{name} at budget {budget} is read twice on one cell")
    return found[0] if found else None


def scored_cells(report, register):
    return [c for c in report["cells"] if c["result"] == "scored" and c["register"] == register]


def margin(cell_d, floor_d):
    if cell_d is None or floor_d is None:
        return None
    return round(cell_d - floor_d, 4)


def by_source_at(cell, budget):
    return {
        row["source"]: {"hits": row["hits"], "of": row["of"]}
        for row in cell.get("by_source", [])
        if row["budget"] == budget
    }


def adjudicate(report, rule, metas=None):
    thresholds = rule["rule"]
    floor = rule["floor"]["embedder"]
    k = thresholds["budget"]
    cells = scored_cells(report, PRIMARY_REGISTER)
    contenders = [c for c in cells if c["embedder"] != floor]
    floor_cells = {(c["scoring"], c["gate"]): c for c in cells if c["embedder"] == floor}
    floor_d_prime = {key: metric_at(c, "d_prime", k) for key, c in floor_cells.items()}

    by_drive = (report.get("registers", {}).get(PRIMARY_REGISTER, {}) or {}).get("by_drive", {})
    budgets = sorted({r["budget"] for c in cells for r in c["metrics"]})
    exact = {b: ceiling(by_drive, b) for b in budgets}
    ceilings = {b: (round(c, 4) if c is not None else None) for b, c in exact.items()}
    fixed_bar = thresholds["supported_precision_at_5"]
    # v2: the bar scales with the rung's ceiling
    bar_exact = fixed_bar * exact[k] if exact.get(k) is not None else None
    bar = round(bar_exact, 4) if bar_exact is not None else None
    supported_attainable = True  # by construction under v2; kept on the verdict for the reader

    evaluated = []
    for cell in sorted(contenders, key=lambda c: (c["scoring"], c["gate"], c["embedder"])):
        precision = metric_at(cell, "precision_at_k", k)
        over_firing = metric_at(cell, "over_firing", k)
        d_prime = metric_at(cell, "d_prime", k)
        floor_d = floor_d_prime.get((cell["scoring"], cell["gate"]))
        gap = margin(d_prime, floor_d)
        clears = (
            precision is not None
            and over_firing is not None
            and gap is not None
            and bar_exact is not None and precision >= bar_exact - 1e-9
            and over_firing <= thresholds["supported_over_firing_at_5"]
            and gap >= thresholds["supported_d_prime_margin_over_floor"]
        )
        evaluated.append(
            {
                "embedder": cell["embedder"],
                "scoring": cell["scoring"],
                "gate": cell["gate"],
                "precision_at_k": precision,
                "over_firing": over_firing,
                "d_prime": d_prime,
                "floor_d_prime": floor_d,
                "floor_cell_control_failed": (cell["scoring"], cell["gate"]) not in floor_cells,
                "d_prime_margin_over_floor": gap,
                "clears_supported": clears,
                "precision_ceiling_at_k": ceilings.get(k),
                "supported_bar_attainable": supported_attainable,
                "by_source": by_source_at(cell, k),
                "admitted_only_at_k": next(({"hits": x["hits"], "of": x["of"]} for x in cell.get("admitted_only", []) if x["budget"] == k), None),
                "scaled_supported_bar": bar,
            }
        )
    supported_by = [f"{e['embedder']}/{e['scoring']}/{e['gate']}" for e in evaluated if e["clears_supported"]]

    precision_ceiling_bar = thresholds["refuted_precision_ceiling"]
    # v2: the refuted clause's precision floor scales with each rung's ceiling
    scaled_floor_exact = {b: (precision_ceiling_bar * exact[b] if exact.get(b) is not None else None) for b in budgets}
    scaled_floor = {b: (round(x, 4) if x is not None else None) for b, x in scaled_floor_exact.items()}
    reaches_ceiling = [
        {"cell": f"{c['embedder']}/{c['scoring']}/{c['gate']}", "budget": r["budget"], "precision_at_k": r["value"], "scaled_bar": scaled_floor.get(r["budget"])}
        for c in contenders
        for r in c["metrics"]
        if r["metric"] == "precision_at_k" and scaled_floor_exact.get(r["budget"]) is not None and r["value"] >= scaled_floor_exact[r["budget"]] - 1e-9
    ]
    # ATTAINABILITY (ruled on #17, 5744340773) under v2: every bar is a fraction
    # of its rung's ceiling, so every rung is adjudicable by construction and the
    # `unadjudicated` branch v1 needed has no case here (review of PR #100, M5).
    no_cell_reaches_ceiling = not reaches_ceiling

    defined = [e for e in evaluated if e["d_prime"] is not None]
    best = max(defined, key=lambda e: e["d_prime"]) if defined else None
    best_margin = best["d_prime_margin_over_floor"] if best else None
    small = thresholds["refuted_d_prime_margin_over_floor"]
    best_fails_floor = best_margin is not None and best_margin < small

    if supported_by:
        verdict = "supported"
    elif no_cell_reaches_ceiling or best_fails_floor:
        verdict = "refuted"
    else:
        verdict = "inconclusive"
    envelope = ENVELOPE + envelope_from_metas(metas or {})
    outside = [e["parameter"] for e in envelope if not e["within"]]

    # The pre-gate sub-rule, per contender (embedder, scoring) on the primary
    # register at the budget, with the gate-arm bootstrap the verb emits.
    pre_gate = thresholds["pre_gate"]
    arms = {}
    for cell in contenders:
        arms.setdefault((cell["embedder"], cell["scoring"]), {})[cell["gate"]] = metric_at(cell, "precision_at_k", k)
    tests = {}
    for row in report.get("gate_comparisons", []):
        if not row["cell"].startswith(PRIMARY_REGISTER + "/"):
            continue
        at_k = next((x for x in row.get("precision_at_k", []) if x["budget"] == k), None)
        tests[(row["cell"].split("/")[2], row["cell"].split("/")[1])] = at_k
    deltas = []
    for (embedder, scoring), pair in sorted(arms.items()):
        if "with_gate" in pair and "without_gate" in pair and pair["with_gate"] is not None and pair["without_gate"] is not None:
            test = tests.get((embedder, scoring))
            deltas.append(
                {
                    "embedder": embedder,
                    "scoring": scoring,
                    "with_gate": pair["with_gate"],
                    "without_gate": pair["without_gate"],
                    # the verb's own observed difference (exact, then rounded once)
                    # where the bootstrap row exists; the difference of the two
                    # rounded precisions otherwise (review of PR #100, M2)
                    "delta": test["difference"] if test and test.get("difference") is not None else round(pair["with_gate"] - pair["without_gate"], 4),
                    "p": test["p"] if test else None,
                    "p_holm": test["p_holm"] if test else None,
                    "attainable_p_floor": test["attainable_p_floor"] if test else None,
                }
            )
    improved = [d for d in deltas if d["delta"] >= pre_gate["margin"]]
    worsened = [d for d in deltas if d["delta"] <= -pre_gate["margin"]]
    # v2: the verb now emits the named statistic -- a paired bootstrap of pooled
    # precision at k between the arms, resampled by drive (#99). Ruled on #17:
    # the pre-registered comparison's own p, uncorrected, at the attainable floor.
    at_floor = [d for d in improved if d["p"] is not None and d["p"] <= d["attainable_p_floor"] + 1e-9]
    untested = [d for d in improved if d["p"] is None]
    if improved and untested and not at_floor:
        pre_gate_verdict, because = "unadjudicated", "a pair clears the margin and carries no precision bootstrap at the budget"
    elif at_floor:
        pre_gate_verdict, because = "supported", "a pair clears the margin with its precision bootstrap's uncorrected p at the attainable floor"
    elif improved:
        pre_gate_verdict, because = "inconclusive", "a pair clears the margin but its precision bootstrap's p is above the attainable floor"
    elif worsened:
        pre_gate_verdict, because = "refuted", "no pair improves by the margin and at least one is lower by it"
    else:
        pre_gate_verdict, because = "inconclusive", "every delta is inside the margin"
    def under(cleared):
        """The sub-verdict under another reading of the p clause, computed on its
        own ladder rather than falling back to the primary verdict."""
        if improved and untested and not cleared:
            return "unadjudicated"
        if cleared:
            return "supported"
        if improved:
            return "inconclusive"
        return "refuted" if worsened else "inconclusive"
    emitted_p_readings = {
        "holm_p_at_floor": under([d for d in improved if d["p_holm"] is not None and d["p_holm"] <= d["attainable_p_floor"] + 1e-9]),
        "holm_p_below_0_05": under([d for d in improved if d["p_holm"] is not None and d["p_holm"] < LOOSE_ALPHA]),
    }
    sense = [
        {
            "embedder": c["embedder"], "scoring": c["scoring"], "gate": c["gate"],
            "precision_at_k": metric_at(c, "precision_at_k", k), "over_firing": metric_at(c, "over_firing", k),
            "d_prime": metric_at(c, "d_prime", k), "by_source": by_source_at(c, k),
        }
        for c in sorted(scored_cells(report, "sense"), key=lambda c: (c["scoring"], c["gate"], c["embedder"]))
    ]

    return {
        "rule": {
            "budget": k,
            "floor": floor,
            "primary_register": PRIMARY_REGISTER,
            "version": "v2",
            "supported": f"for at least one contender cell at k = {k} on the primary register: precision >= 0.80 x the rung's attainable ceiling ({bar} here), hard-negative false-positive rate <= {thresholds['supported_over_firing_at_5']}, and d' over the matched floor cell by >= {thresholds['supported_d_prime_margin_over_floor']} -- all three at once",
            "refuted": f"no contender cell at any pre-registered budget reaches 0.60 x that budget's ceiling ({scaled_floor}), or the best-separated contender cell does not clear its matched floor cell by >= {thresholds['refuted_d_prime_margin_over_floor']} -- either alone",
            "inconclusive": thresholds["inconclusive"],
            "v1_wording": {"supported": thresholds["supported"], "refuted": thresholds["refuted"]},
            "v2_wording": rule.get("rule", {}).get("v2", {}),
        },
        "attainability": {
            "ceiling_by_budget": ceilings,
            "fixed_supported_bar": fixed_bar,
            "supported_bar": bar,
            "scaled_refuted_floor_by_budget": scaled_floor,
            "supported_bar_attainable_at_k": supported_attainable,
            "refuted_precision_ceiling_bar": precision_ceiling_bar,
            "adjudicable_rungs": budgets,
            "note": "every v2 bar is a fraction of its rung's ceiling, so every rung is adjudicable by construction; d' has no arithmetic ceiling and a margin undefined on either side satisfies neither bound",
        },
        "envelope": {
            "parameters": envelope,
            "outside": outside,
            "label": "characterization" if outside else "mechanism",
        },
        "readings": {
            "floor_separation": "the floor's d_prime on the matched (register, scoring, gate) cell (ratified)",
            "maximum_separation": "the contender cell with the highest defined d_prime on the primary register, against its matched floor cell (ratified; the primary-register scope is this applier's reading)",
            "undefined_margin": "satisfies neither bound and is carried as null (ratified)",
            "cell": "a (scoring, gate) pair per embedder, so a gate arm is its own cell (#84, ruled)",
            "pre_gate_p": "the drive-resampled paired bootstrap of pooled precision at k between the gate arms (emitted since #99), its own p uncorrected at the attainable floor as ruled; Holm readings carried under alternatives",
            "gated_rows_fill_the_budget": "a row the gate rejects still fills a drive's budget at the floor when fewer than k are admitted; the pooled figure is adjudicated as ratified and the admitted-only precision the verb now emits is reported beside every cell",
            "scaled_bars": "v2 wording (#17, 5747481065): the supported precision bar is 0.80 x the rung's attainable ceiling and the refuted clause's precision floor 0.60 x each rung's ceiling; the d' margins stand as ratified",
            "authored_sense_register": "scored and reported beside the verdict, not adjudicated",
            "by_source": "reported beside every cell at the budget; the pooled verdict stands",
        },
        "alternatives": emitted_p_readings,
        "cells": evaluated,
        "supported_by": supported_by,
        "refuted_by": {
            "no_cell_reaches_precision_ceiling": no_cell_reaches_ceiling,
            "cells_reaching_ceiling": len(reaches_ceiling),
            "best_cell": f"{best['embedder']}/{best['scoring']}/{best['gate']}" if best else None,
            "best_cell_d_prime": best["d_prime"] if best else None,
            "best_cell_margin_over_floor": best_margin,
            "best_cell_fails_floor_margin": best_fails_floor,
        },
        "verdict": verdict,
        "pre_gate": {
            "margin": pre_gate["margin"],
            "deltas": deltas,
            "improved": len(improved),
            "worsened": len(worsened),
            "verdict": pre_gate_verdict,
            "because": because,
        },
        "authored_sense_register": sense,
        "registers": report.get("registers"),
    }

# ------------------------------------------------------------------------

try:
    report = json.loads(read("report.json"))
except json.JSONDecodeError as err:
    cannot_run(f"report.json is not JSON: {err.msg}")
try:
    rule = tomllib.loads(read("decision-rule-v2.toml"))
except tomllib.TOMLDecodeError as err:
    cannot_run(f"decision-rule.toml is not TOML: {err}")

metas = {a['path'][len('caches/'):-len('.meta.json')]: json.loads(read(a['path'])) for a in consumed if a['path'].startswith('caches/')}
derived = json.dumps(adjudicate(report, rule, metas), indent=2, sort_keys=True) + "\n"
committed = read("verdict.json")
yielded = json.loads(derived)["verdict"]
if derived != committed:
    try:
        stated = json.loads(committed).get("verdict")
    except json.JSONDecodeError:
        stated = "unreadable"
    sys.exit(
        f"verdict.json does not re-derive: the rule over report.json yields "
        f"{yielded!r} and the committed file says {stated!r}, or the two differ "
        f"somewhere below the verdict"
    )
for where, stated in (("front-matter", front["result"]), ("the claim row", claims[0]["result"])):
    if stated != yielded:
        sys.exit(f"{where} says {stated!r}, the rule yields {yielded!r}")

print(f"{len(consumed)} artefact(s), the product and the verdict re-derive")
PY
