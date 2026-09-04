#!/usr/bin/env python3
"""Extraction-seat bakeoff grading: physics and mechanical quality tables for the
three-way fire.

Inputs: the three seat replay dirs (events.jsonl + probes.jsonl) written by
`exercise replay-capture`, plus seat C's offline GLiNER rows.

What this computes (the MECHANICAL axes only):

  physics.probes     per seat, per turn-boundary probe: the re-prefill the
                     next human turn would pay (observed_prompt_n), surviving
                     warmth (observed_cache_n), and the drive's own recorded
                     values for the same request.
  physics.warmth     warm-call fidelity: observed vs recorded cache_n on
                     every re-sent request (the behavioral byte-identity
                     track).
  physics.forks      per seat: extraction/interview fork counts, prompt_n /
                     prompt_ms / predicted_ms totals, expectation misses
                     (fork cache_n < expected_cache_n — cold forks), and
                     per-turn capture-round wall-clock.
  quality.mechanical per seat: facts offered (raw FACT: lines) vs accepted
                     (object.patch, the Rust grounding gate's own verdicts —
                     the gate is read from the record, never re-implemented
                     here), spans for seat C, and normalized-text lane
                     overlap between seats per source.

What this deliberately does NOT do: the mechanism-fact judgment axis. That
needs a judge-panel protocol (fresh instances, pinned prompts), not an
improvised heuristic in a tally script; the bakeoff's design names it as a
separate pass.

The bakeoff's adversarial review (2026-08-12) caught a real, pre-existing defect
in the harness's fact parser (filed separately, independent of the bakeoff): the
parser accepts ANY non-empty, non-"NONE" answer line as a fact
candidate, tagged or not (`strip_tag(...).unwrap_or(l)`), and has no
continuation handling — a multi-line quoted `FACT:` answer explodes into one
candidate per line. Both patterns inflate `facts_accepted` (the Rust gate's
own recorded verdicts) with fragments that are not genuine extracted facts:
an untagged conversational answer can "accept" stray fence markers, and one
multi-line quote can out-vote its own fork's real FACT: count several times
over. This module now surfaces that artifact rather than silently reporting
through it: `facts_accepted_raw` is the mechanism-truth number (what the
Rust gate actually recorded — never altered here); `facts_accepted_deduped`
caps each fork's contribution at its own `FACT:`-tagged line count (the most
that fork could honestly have offered), and `degenerate_forks` lists every
fork where the cap actually bit, so the delta is auditable per-fork rather
than asserted in prose. The overlap corpus (`quality.overlap_*`) is built
from the DEDUPED view only — a degenerate fork's exploded fragments never
feed the lane-overlap comparison, and `contains()` ignores spans shorter
than `MIN_OVERLAP_CHARS` (GLiNER emits many single-token spans that would
otherwise trivially "contain" nearly anything).

Usage (this directory, as recompute.sh runs it):
  python3 grade.py --seat-a seat-a --seat-b seat-b --seat-c seat-c --gliner seat-c --out <dir>
"""
import argparse
import json
import os
import re
from collections import defaultdict


def read_jsonl(path):
    if not os.path.exists(path):
        return []
    with open(path, encoding="utf-8") as f:
        return [json.loads(l) for l in f if l.strip()]


def norm_ws(s):
    return re.sub(r"\s+", " ", s).strip().lower()


FACT_RE = re.compile(r"^\s*[-*•]?\s*FACT:\s*(.+)$", re.IGNORECASE)


def raw_facts(content):
    """FACT: lines offered by a fork answer (before the grounding gate)."""
    return [m.group(1).strip() for line in content.splitlines()
            if (m := FACT_RE.match(line))]


def load_seat(replay_dir):
    events = read_jsonl(os.path.join(replay_dir, "events.jsonl"))
    probes = read_jsonl(os.path.join(replay_dir, "probes.jsonl"))

    # Fork pairs: request carries turn; response carries timings/ask/step.
    forks = []
    last_req = None
    for e in events:
        if e["event"] == "fork.request":
            last_req = e
        elif e["event"] == "fork.response":
            req = last_req if last_req and e.get("parent_id") == last_req["id"] else None
            forks.append({
                "turn": (req or {}).get("parent_turn"),
                "step": e.get("step"),
                "lane": e["lane"],
                "ask": e.get("ask"),
                "content": e.get("content", ""),
                "timings": e.get("timings") or {},
                "expected_cache_n": e.get("expected_cache_n"),
                "start": e.get("start"), "end": e.get("end"),
                "id": e["id"],
            })

    # Accepted facts: object.patch rows whose parent is a fork.response in the
    # extraction lane. `added` ids map through the patch's own prose field(s).
    ext_ids = {f["id"] for f in forks if f["lane"] == "extraction"}
    accepted = defaultdict(list)  # fork resp id -> [fact prose]
    for e in events:
        if e["event"] != "object.patch" or e.get("parent_id") not in ext_ids:
            continue
        # `added` entries are the concatenated `"#{id} {text}"` display strings
        # (the fold-hashed record form); the
        # singular `text` caption exists only on single-fact patches, and the
        # extraction lane batches every grounded fact from one response into
        # one multi-add patch. Strip the id token to recover each fact's prose.
        for entry in e.get("added", []):
            prose = re.sub(r"^#\d+\s*", "", entry)
            accepted[e["parent_id"]].append(prose)
    return {"events": events, "probes": probes, "forks": forks,
            "accepted": accepted}


def physics(seats):
    out = {"probes": {}, "warmth": {}, "forks": {}}
    for name, s in seats.items():
        rows = [r for r in s["probes"] if r.get("post_capture")]
        out["probes"][name] = [{
            "turn": r["turn"],
            "next_turn_reprefill_tokens": r.get("observed_prompt_n"),
            "surviving_cache_n": r.get("observed_cache_n"),
            "drive_recorded_cache_n": r.get("recorded_cache_n"),
            "drive_recorded_prompt_n": r.get("recorded_prompt_n"),
            "prompt_ms": r.get("prompt_ms"),
        } for r in rows]

        warm = [r for r in s["probes"] if not r.get("post_capture")]
        diffs = [(r["observed_cache_n"] or 0) - (r["recorded_cache_n"] or 0)
                 for r in warm
                 if r.get("observed_cache_n") is not None
                 and r.get("recorded_cache_n") is not None]
        out["warmth"][name] = {
            "warm_calls": len(warm),
            "cache_n_delta_min": min(diffs) if diffs else None,
            "cache_n_delta_max": max(diffs) if diffs else None,
        }

        by_lane = defaultdict(lambda: {"n": 0, "prompt_n": 0, "prompt_ms": 0.0,
                                       "predicted_n": 0, "predicted_ms": 0.0,
                                       "cold": 0, "no_timings": 0})
        round_span = defaultdict(lambda: [None, None])  # turn -> [t0, t1]
        for f in s["forks"]:
            d = by_lane[f["lane"]]
            d["n"] += 1
            t = f["timings"]
            if not t:
                d["no_timings"] += 1
            d["prompt_n"] += t.get("prompt_n") or 0
            d["prompt_ms"] += t.get("prompt_ms") or 0.0
            d["predicted_n"] += t.get("predicted_n") or 0
            d["predicted_ms"] += t.get("predicted_ms") or 0.0
            exp, got = f.get("expected_cache_n"), t.get("cache_n")
            if exp is not None and got is not None and got < exp:
                d["cold"] += 1
            if f["turn"] is not None and f["start"] and f["end"]:
                sp = round_span[f["turn"]]
                sp[0] = f["start"] if sp[0] is None else min(sp[0], f["start"])
                sp[1] = f["end"] if sp[1] is None else max(sp[1], f["end"])
        out["forks"][name] = {
            "by_lane": {k: dict(v) for k, v in by_lane.items()},
            "capture_round_wall_s": {
                t: round(sp[1] - sp[0], 2) for t, sp in sorted(round_span.items())
            },
        }
    return out


# Below this many characters a span/fact is common-substring noise
# (GLiNER emits many single-token spans — "x", "id" — that would otherwise
# trivially "contain" almost any prose fragment). Matches the threshold the
# the bakeoff's adversarial review used to demonstrate the inflation.
MIN_OVERLAP_CHARS = 4


def quality(seats, gliner_rows):
    per_source = defaultdict(dict)  # (turn, step) -> seat -> [normalized facts]
    tallies = {}
    degenerate_forks = {}
    for name in ("A", "B"):
        s = seats.get(name)
        if not s:
            continue
        offered = accepted_raw = accepted_deduped = 0
        degenerate = []
        for f in s["forks"]:
            if f["lane"] != "extraction":
                continue
            raw = raw_facts(f["content"])
            acc = s["accepted"].get(f["id"], [])
            raw_n, acc_n = len(raw), len(acc)
            offered += raw_n
            accepted_raw += acc_n
            # The parser defect: parse_facts accepts untagged lines and explodes
            # multi-line FACT: quotes one-line-per-candidate, so a fork's
            # true contribution can never honestly exceed its own tagged
            # FACT: line count. Cap here; the raw (mechanism-truth) count
            # stays untouched above for anyone auditing what actually ran.
            capped_n = min(acc_n, raw_n)
            accepted_deduped += capped_n
            if acc_n > raw_n:
                degenerate.append({
                    "fork_id": f["id"], "turn": f["turn"], "step": f["step"],
                    "fact_lines": raw_n, "accepted_raw": acc_n,
                    "excess": acc_n - raw_n,
                })
            key = (f["turn"], f["step"])
            # Overlap corpus uses the DEDUPED view only: a degenerate fork's
            # exploded fragments must not feed the lane-overlap comparison
            # (they are parser noise, not seat B's genuine extracted facts).
            per_source[key][name] = [norm_ws(x) for x in acc[:capped_n]]
        tallies[name] = {
            "facts_offered": offered,
            "facts_accepted_raw": accepted_raw,
            "facts_accepted_deduped": accepted_deduped,
            "gate_accept_rate_raw": round(accepted_raw / offered, 3) if offered else None,
            "gate_accept_rate_deduped": round(accepted_deduped / offered, 3) if offered else None,
        }
        degenerate_forks[name] = degenerate
    spans_total = 0
    for row in gliner_rows:
        key = (row["turn"], row["step"])
        per_source[key]["C"] = [norm_ws(sp["text"]) for sp in row["spans"]
                                if len(sp["text"]) >= MIN_OVERLAP_CHARS]
        spans_total += row["n_spans"]
    tallies["C"] = {"spans": spans_total}

    def contains(a, b):
        if len(a) < MIN_OVERLAP_CHARS or len(b) < MIN_OVERLAP_CHARS:
            return False
        return a in b or b in a

    overlap = defaultdict(int)
    counts = defaultdict(int)
    for key, by_seat in per_source.items():
        for x, y in (("A", "B"), ("A", "C"), ("B", "C")):
            if x in by_seat and y in by_seat:
                for fx in by_seat[x]:
                    if len(fx) < MIN_OVERLAP_CHARS:
                        continue
                    counts[f"{x}∩{y}:{x}-items"] += 1
                    if any(contains(fx, fy) for fy in by_seat[y]):
                        overlap[f"{x}∩{y}"] += 1
    return {"tallies": tallies,
            "degenerate_forks": degenerate_forks,
            "min_overlap_chars": MIN_OVERLAP_CHARS,
            "overlap_containment": dict(overlap),
            "overlap_bases": dict(counts),
            "sources_with_any_output": len(per_source)}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--seat-a", required=True)
    ap.add_argument("--seat-b", required=True)
    ap.add_argument("--seat-c", required=True)
    ap.add_argument("--gliner", required=True,
                    help="dir holding seat-c-gliner.jsonl")
    ap.add_argument("--out", required=True)
    args = ap.parse_args()

    seats = {"A": load_seat(args.seat_a), "B": load_seat(args.seat_b),
             "C": load_seat(args.seat_c)}
    gliner_rows = read_jsonl(os.path.join(args.gliner, "seat-c-gliner.jsonl"))

    report = {
        "physics": physics(seats),
        "quality": quality(seats, gliner_rows),
        "judge_panel": "NOT RUN — mechanism-fact axis needs the "
                       "panel protocol (separate pass, per the design comment)",
    }
    os.makedirs(args.out, exist_ok=True)
    path = os.path.join(args.out, "report.json")
    with open(path, "w") as f:
        json.dump(report, f, indent=2)
    print(json.dumps(report["physics"]["probes"], indent=2))
    print(json.dumps(report["quality"]["tallies"], indent=2))
    print(f"full report: {path}")


if __name__ == "__main__":
    main()
