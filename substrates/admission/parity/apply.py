#!/usr/bin/env python3
"""The applier for a parity fire of extraction-acceptance-inverts, generalised (#143, I5; the plan's D7, N1
and N10). It reads a CONFIG (the pins and constants #115's applier carried in its source), the reference row
(pinned), the band (pinned), a fire's seat logs and its box record, and writes the word. With #115's config
(configs/extraction-acceptance-inverts-115.json) it reproduces #115's verdict.json byte for byte and runs
#115's fixtures.

The rule (#89 as generalised by D7 (a) and N1): the reference sign is the sign of the band's point estimate
(the first fire's, or the archived row's). Supported if the fire's effect lands within the band; outside it,
inconclusive if the effect keeps the reference sign, refuted if it has the opposite sign or is zero. If the
band straddles zero there is no reference sign: outside either way is inconclusive, and the fire cannot
refute. A band that only touches zero (an edge at 0) keeps its point's sign, as #115's rule reads it. A refuted or inconclusive word carries the config's disclosure. The degenerate case (N1): a declared
floor on seat A's accepted-and-deduped count, in the fire and in the band's own fire; below it the word is
unadjudicated.

The unadjudicated checks are asked first, before the effect is compared with
the band; any failing check makes the word unadjudicated, every failure listed:
  box      box.json holds every field below, the instance is a registry
           instance id and the same before the first fork and after the last,
           the box verification script and the fingerprint check exited 0 on
           both sides, the canary's verdicts on each side are PASS or DRIFT then
           PASS, and the seat-B server's pre-fire answer carried neither a think
           block nor draft_n;
  logs     each seat's log parses, opens with exactly one run.begin naming the
           declared arm and model, and ends with its one run.end; a log that
           does not parse is a partial log;
  routing  every fork response carries timings; every seat-B extraction
           response lacks draft_n, reasoning and a think block (the CPU-side
           server runs no speculative draft and thinking is off); every seat-A
           extraction response and every interview response carries draft_n
           (the production server runs one);
  forks    each seat's extraction forks are exactly the archived 31
           (turn, step) keys, each once; each seat's interview forks include
           every one of the 56 (turn, step, ask) keys the offline rehearsal
           plans -- a timed-out fork leaves no event, and the harness writes
           run.end ok = true, timeouts = 0 unconditionally, so presence is the
           only evidence a fork ran;
  offered  each seat offers at least one fact;
  floor    seat A's accepted-and-deduped count, in this fire and in the band's, is at least the config's floor.

The effect is computed exactly as band.py computes the archived one: per fork,
offered and accepted through the archived row's own grade.py, accepted capped
at offered, pooled per seat from the integer tallies, seat B minus seat A, in
IEEE double, compared unrounded against the band's printed endpoints.

Usage:
  apply.py CONFIG ARCHIVED_DIR BAND_JSON REFIRE_DIR BOX_JSON   -> prints the verdict as JSON
  apply.py --selftest CONFIG ARCHIVED_DIR BAND_JSON           -> runs the fixtures
CONFIG is JSON with an "apply" object: pinned (the reference row's artefacts by sha256), band_sha256, arms,
model_id, interviews, disclosure, seat_a_floor; optionally counted_fork_floor, pool ("per-seat" or "paired") and
report_refire_interval (the fire's own interval, written to --interval-out as a number beside the word, with a sentence
only if it straddles zero), and interview_miss ("fail", #115's, or "byte-match" with planned_requests pinned).
ARCHIVED_DIR holds the archived row's grade.py, seat-a/events.jsonl and
seat-b/events.jsonl; REFIRE_DIR holds the re-fired seat-a/ and seat-b/ logs.
Exit 0 a verdict printed (unadjudicated included); 1 a selftest fixture read
other than expected; 2 an input is missing or not the pinned bytes.
"""
import collections, copy, hashlib, importlib.util, json, pathlib, re, sys, tempfile

sys.dont_write_bytecode = True

# the constants #115's applier carried in its source, now read from the config by configure()
PINNED, BAND_SHA, ARMS, MODEL_ID, INTERVIEWS, DISCLOSURE, SEAT_A_FLOOR = {}, "", {}, "", [], "", 0
COUNTED_FLOOR = None  # None: every archived extraction fork must be present (#115); an int: at least that many counted
POOL = "per-seat"      # "per-seat": each seat over its own counted forks; "paired": both over the forks both seats counted
REFIRE_INTERVAL = False  # report the fire's own paired interval beside the word, by band.py's method, and whether it straddles zero
INTERVIEW_MISS = "fail"  # "fail": a missing interview fork fails the seat (#115); "byte-match": it is reported, and the fire is
                         # void only if a planned fork ran with another request (planning, #143 comment 5921525110)
ROUTING = "draft_n"      # "draft_n": #115's routing fingerprint; "identity": the candidate's responses are routed by the instance identity
                         # and /v1/model's id checked at every request (ruled #393 5986337117, ruling 3), the box record carrying the checks
_BOX = None
PLANNED = {}             # under "byte-match": seat -> {fork key: the planned request's digest} (the offline rehearsal's)
BOX_FIELDS = ("instance_before", "instance_after", "verify_before", "verify_after",
              "fingerprint_before", "fingerprint_after", "canary_before", "canary_after", "seatb_canary")


def configure(path):
    """Read CONFIG and set the applier's constants from it."""
    global ROUTING, PINNED, BAND_SHA, ARMS, MODEL_ID, INTERVIEWS, DISCLOSURE, SEAT_A_FLOOR, COUNTED_FLOOR, POOL, REFIRE_INTERVAL, INTERVIEW_MISS, PLANNED
    try:
        c = json.loads(pathlib.Path(path).read_text(encoding="utf-8"))["apply"]
        PINNED, BAND_SHA, ARMS, MODEL_ID = dict(c["pinned"]), str(c["band_sha256"]), dict(c["arms"]), str(c["model_id"])
        INTERVIEWS, DISCLOSURE, SEAT_A_FLOOR = [list(x) for x in c["interviews"]], str(c["disclosure"]), int(c["seat_a_floor"])
        COUNTED_FLOOR = None if c.get("counted_fork_floor") is None else int(c["counted_fork_floor"])
        POOL, REFIRE_INTERVAL = str(c.get("pool", "per-seat")), bool(c.get("report_refire_interval", False))
        INTERVIEW_MISS = str(c.get("interview_miss", "fail"))
        ROUTING = str(c.get("routing", "draft_n"))
        if ROUTING not in ("draft_n", "identity"):
            cannot("the config's routing is neither draft_n nor identity")
        if INTERVIEW_MISS == "byte-match":
            planned = c["planned_requests"]  # {"file": relative to the config, "sha256": its pinned digest}
            f = pathlib.Path(path).resolve().parent / planned["file"]
            if not f.is_file() or hashlib.sha256(f.read_bytes()).hexdigest() != planned["sha256"]:
                cannot("the planned requests are missing or not the pinned bytes")
            PLANNED = {n: dict(v) for n, v in json.loads(f.read_text(encoding="utf-8"))["seats"].items()}
            if set(PLANNED) != {"A", "B"} or not all(PLANNED.values()):
                cannot("the plan does not name both seats' forks")
    except (OSError, ValueError, KeyError, TypeError) as e:
        cannot(f"the config cannot be read: {type(e).__name__}")
    if set(ARMS) != {"A", "B"} or SEAT_A_FLOOR < 0 or not re.fullmatch(r"[0-9a-f]{64}", BAND_SHA) or (COUNTED_FLOOR is not None and COUNTED_FLOOR < 1) or POOL not in ("per-seat", "paired") or INTERVIEW_MISS not in ("fail", "byte-match"):
        cannot("the config's arms, floor or band digest is malformed")


def request_digest(e):
    """One fork request as the planned set keys it: lane, parent turn, the messages and the params, canonically."""
    return hashlib.sha256(json.dumps([e.get("lane"), str(e.get("parent_turn")), e.get("messages"), e.get("params")],
                                     sort_keys=True, separators=(",", ":")).encode("utf-8")).hexdigest()


def fork_requests(ev):
    """Each fork's request digest under its key, "lane|turn|step|ask": the request carries the turn, its response
    the step and the ask. A key the replay asked more than once keeps every digest."""
    byid, out = {e.get("id"): e for e in ev}, collections.defaultdict(list)
    for e in ev:
        if e.get("event") == "fork.response":
            q = byid.get(e.get("parent_id"))
            if q is not None and q.get("event") == "fork.request":
                out[f"{e.get('lane')}|{q.get('parent_turn')}|{e.get('step')}|{e.get('ask') or ''}"].append(request_digest(q))
    return out


def changed_requests(name, ev):
    """The planned forks this seat ran whose request is not the planned one. A planned fork that did not run is a
    miss, counted or reported elsewhere; a fork the plan does not name (the replay's extra interviews) is not
    the plan's business. So a miss voids the fire exactly when it changed a later planned request."""
    got = fork_requests(ev)
    return sorted(k for k, want in PLANNED.get(name, {}).items() if any(d != want for d in got.get(k, [])))  # every copy


def cannot(m):
    print(f"apply: {m}", file=sys.stderr)
    raise SystemExit(2)


def pinned(archived, band_path):
    for rel, want in PINNED.items():
        p = pathlib.Path(archived) / rel
        if not p.is_file() or hashlib.sha256(p.read_bytes()).hexdigest() != want:
            cannot(f"the archived {rel} is missing or not the pinned bytes")
    p = pathlib.Path(band_path)
    if not p.is_file() or hashlib.sha256(p.read_bytes()).hexdigest() != BAND_SHA:
        cannot("band.json is missing or not the pinned bytes")
    spec = importlib.util.spec_from_file_location("grade", pathlib.Path(archived) / "grade.py")
    grade = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(grade)
    band = json.loads(p.read_text(encoding="utf-8"))
    keys = set()
    for seat in ("seat-a", "seat-b"):
        keys |= {(f["turn"], f["step"]) for f in grade.load_seat(str(pathlib.Path(archived) / seat))["forks"]
                 if f["lane"] == "extraction"}
    if len(keys) != band["forks"]:
        cannot("the archived extraction keys do not number the band's forks")
    band["keys"] = keys
    return grade, band


def canary_ok(side):
    return side in (["PASS"], ["DRIFT", "PASS"])


def decide(effect, band, point):
    """Within the band, supported. Outside it: with the reference sign (the point's) kept, inconclusive; with
    the opposite sign or zero, refuted. A band strictly straddling zero (lo < 0 < hi) has no reference sign:
    outside it is inconclusive. A band with an edge at zero keeps its point's sign."""
    lo, hi = band
    if lo <= effect <= hi:
        return "supported"
    if lo < 0 < hi or point == 0:  # strictly across zero; a band touching zero keeps its point's sign
        return "inconclusive"
    ref = 1 if point > 0 else -1
    return "inconclusive" if effect * ref > 0 else "refuted"


def check_box(box):
    missing = [f for f in BOX_FIELDS if f not in box]
    if missing:
        return [f"box: box.json lacks {', '.join(missing)}"]
    r = []
    ib, ia = box["instance_before"], box["instance_after"]
    if not (isinstance(ib, str) and re.fullmatch(r"(?:[a-z][a-z0-9]*-)?\d{4}-\d{2}-\d{2}", ib) and ib == ia):
        r.append("box: the instance is not one registry instance id before and after")
    for f in ("verify_before", "verify_after", "fingerprint_before", "fingerprint_after"):
        if box[f] != 0:
            r.append(f"box: {f} exited {box[f]!r}, not 0")
    for f in ("canary_before", "canary_after"):
        if not canary_ok(box[f]):
            r.append(f"box: {f} is {box[f]!r}, not PASS within one re-draw")
    if box["seatb_canary"] != {"think": False, "draft_n": False}:
        r.append(f"box: the seat-B pre-fire answer is {box['seatb_canary']!r}")
    if ROUTING == "identity":
        rt = box.get("routing")
        if not (isinstance(rt, dict) and rt.get("model_id") == MODEL_ID and rt.get("instance") == ib
                and isinstance(rt.get("checks"), dict) and set(rt["checks"]) == {"A", "B"}
                and all(isinstance(v, list) for v in rt["checks"].values())):
            r.append("routing: the box record lacks the identity routing check (the instance and the model id the checks were made against, and a list of checks per seat); a fire whose routing check cannot run is void")
    return r


def check_seat(grade, name, seat_dir, keys):
    """Returns (reasons, tallies or None, misses). Under a counted-fork floor (a pre-registration's), an extraction
    fork that is missing, or whose routing fails, is not counted, and its reason is listed in misses; the seat is
    unadjudicated only below the floor. Without one, every archived fork must be present and routed (#115)."""
    misses = {}
    try:
        s = grade.load_seat(str(seat_dir))
        ev, forks = s["events"], s["forks"]
        r = []
        begins = [e for e in ev if e.get("event") == "run.begin"]
        ends = [e for e in ev if e.get("event") == "run.end"]
        if not ev or ev[0].get("event") != "run.begin" or len(begins) != 1:
            r.append(f"logs: seat {name}'s log does not open with its one run.begin")
        elif begins[0].get("arm") != ARMS[name] or begins[0].get("model_id") != MODEL_ID:
            r.append(f"logs: seat {name}'s run.begin names another arm or model")
        if not ev or ev[-1].get("event") != "run.end" or len(ends) != 1:
            r.append(f"logs: seat {name}'s log does not end with its one run.end")
        n = collections.Counter()
        def bad(f, why):
            if COUNTED_FLOOR is not None and f["lane"] == "extraction":
                k = (f["turn"], f["step"]); misses[k] = f"{misses[k]}; {why}" if k in misses else why  # not counted
            else:
                n[why] += 1
        for f in forks:
            t, ext = f["timings"], f["lane"] == "extraction"
            if not t:
                bad(f, "no timings")
            elif name == "B" and ext:
                if "draft_n" in t:
                    bad(f, "seat-B extraction carrying draft_n")
            elif ROUTING == "draft_n" and "draft_n" not in t:
                bad(f, f"seat-{name} {f['lane']} lacking draft_n")
        if name == "B":
            byid = {f["id"]: f for f in forks}
            for e in ev:
                if e.get("event") == "fork.response" and e.get("lane") == "extraction" and (
                        e.get("reasoning") or "<think>" in (e.get("content") or "")):
                    f = byid.get(e.get("id")) or {"lane": "extraction", "turn": e.get("turn"), "step": e.get("step")}
                    bad(f, "seat-B extraction with reasoning or a think block")
        if ROUTING == "identity" and isinstance(_BOX, dict) and isinstance(_BOX.get("routing"), dict):
            # the candidate endpoint answered every seat-A fork and every seat-B interview fork; each such request was
            # preceded by a /v1/model read whose id is recorded in the box's checks for that seat: one per request, all the pinned id
            chk = _BOX["routing"].get("checks", {}).get(name)
            if isinstance(chk, list):
                want = sum(1 for f in forks if name == "A" or f["lane"] != "extraction")
                if len(chk) < want:
                    n[f"{want} candidate-routed forks, {len(chk)} model-id checks"] += 1
                if any(c != MODEL_ID for c in chk):
                    n["a model-id check named another id than the pinned one"] += 1
        r += [f"routing: {k}: {v}" for k, v in sorted(n.items())]
        tallies, dup = {}, False
        for f in forks:
            if f["lane"] != "extraction":
                continue
            key = (f["turn"], f["step"])
            dup |= key in tallies
            offered = len(grade.raw_facts(f["content"]))
            tallies[key] = (offered, min(len(s["accepted"].get(f["id"], [])), offered))
        if COUNTED_FLOOR is None:
            if dup or set(tallies) != keys:
                r.append(f"forks: seat {name}'s extraction forks are not the archived {len(keys)} keys, each once")
        else:
            if dup or not set(tallies) <= keys:
                r.append(f"forks: seat {name}'s extraction forks are not archived keys, each at most once")
            for k in keys - set(tallies):
                misses[k] = "missing (a timeout leaves no event)"
            tallies = {k: v for k, v in tallies.items() if k not in misses}
            if POOL != "paired" and len(tallies) < COUNTED_FLOOR:  # paired: the floor is the intersection's (planning)
                r.append(f"floor: seat {name} counted {len(tallies)} of its {len(keys)} extraction forks, below the floor of {COUNTED_FLOOR}")
        have = collections.Counter((f["turn"], f["step"], f["ask"]) for f in forks if f["lane"] == "interview")
        short = collections.Counter(tuple(k) for k in INTERVIEWS) - have
        if short and INTERVIEW_MISS == "fail":
            r.append(f"forks: seat {name} lacks {sum(short.values())} of the {len(INTERVIEWS)} planned interview forks")
        elif short:  # reported; it voids the fire only through a planned fork below that ran with another request
            for k in sorted(short.elements()):
                misses[("interview",) + k] = "missing (a timeout leaves no event)"
        if INTERVIEW_MISS == "byte-match":
            changed = changed_requests(name, ev)
            if changed:
                r.append(f"requests: seat {name}'s planned request changed at {len(changed)} fork(s), first {changed[0]} (a miss changed a later request, or the replay diverged)")
        if sum(v[0] for v in tallies.values()) == 0:
            r.append(f"offered: seat {name} offers no fact")
        return r, tallies, misses
    except (json.JSONDecodeError, KeyError, TypeError, AttributeError, UnicodeDecodeError) as e:
        return [f"logs: seat {name}'s log is partial or malformed ({type(e).__name__})"], None, misses


def adjudicate(grade, band, seat_dirs, box):
    global _BOX
    _BOX = box
    reasons = check_box(box)
    if band["seat_a"]["accepted_deduped"] < SEAT_A_FLOOR:
        reasons.append(f"floor: the band's own fire has seat A at {band['seat_a']['accepted_deduped']} accepted, below the floor of {SEAT_A_FLOOR}")
    if COUNTED_FLOOR is not None and COUNTED_FLOOR > len(band["keys"]):
        reasons.append(f"floor: a counted-fork floor of {COUNTED_FLOOR} exceeds the band's {len(band['keys'])} forks")
    t, missed = {}, {}
    for name in ("A", "B"):
        r, t[name], missed[name] = check_seat(grade, name, seat_dirs[name], band["keys"])
        reasons += r
    out = {"unadjudicated_checks": reasons or "all passed", "band": band["band"]}
    ext = {n: {k: v for k, v in missed[n].items() if k[0] != "interview"} for n in ("A", "B")}
    if COUNTED_FLOOR is not None:  # reported per seat regardless of the word (the pre-registration's)
        out["counted_forks"] = {n: {"counted": len(t[n] or {}), "of": len(band["keys"]), "floor": COUNTED_FLOOR,
                                    "misses": [{"turn": k[0], "step": k[1], "reason": v} for k, v in sorted(ext[n].items())]} for n in ("A", "B")}
    if INTERVIEW_MISS == "byte-match":  # reported, never a word by itself (planning, #143 comment 5921525110)
        out["interview_misses"] = {n: [{"turn": k[1], "step": k[2], "ask": k[3], "reason": v}
                                       for k, v in sorted(missed[n].items()) if k[0] == "interview"] for n in ("A", "B")}
    if POOL == "paired" and COUNTED_FLOOR is not None and t["A"] is not None and t["B"] is not None:
        out["counted_forks"]["paired"] = len(set(t["A"]) & set(t["B"]))
    if reasons:
        out.update(word="unadjudicated", effect=None, disclosure=None)
        return out
    if POOL == "paired":  # the band's pairing kept: both seats over the forks both counted
        both = set(t["A"]) & set(t["B"])
        t = {n: {k: v for k, v in t[n].items() if k in both} for n in ("A", "B")}
        if COUNTED_FLOOR is not None:
            out["counted_forks"]["paired"] = len(both)
            if len(both) < COUNTED_FLOOR:
                out.update(unadjudicated_checks=[f"floor: the seats counted {len(both)} forks in common, below the floor of {COUNTED_FLOOR}"], word="unadjudicated", effect=None, disclosure=None)
                return out
    oa, aa = sum(v[0] for v in t["A"].values()), sum(v[1] for v in t["A"].values())
    ob, ab = sum(v[0] for v in t["B"].values()), sum(v[1] for v in t["B"].values())
    if aa < SEAT_A_FLOOR:
        out.update(unadjudicated_checks=[f"floor: seat A accepted {aa}, below the floor of {SEAT_A_FLOOR}"], word="unadjudicated", effect=None, disclosure=None)
        return out
    effect = ab / ob - aa / oa
    word = decide(effect, band["band"], band["point"])
    out.update(seat_a={"offered": oa, "accepted_deduped": aa}, seat_b={"offered": ob, "accepted_deduped": ab},
               effect=effect, word=word, disclosure=DISCLOSURE if word in ("refuted", "inconclusive") else None)
    if REFIRE_INTERVAL:
        out["refire_interval"] = refire_interval(t, band)
    return out


def refire_interval(t, band):
    """The fire's own paired interval, by band.py's method (percentile bootstrap over the forks both seats counted,
    the band's level, resamples and seed), reported as a number beside the word and never as a word. The one
    sentence it can carry is that it straddles zero, which bears on the sign rule (planning, #143 comment
    5921525110: the band-edge sentence is withdrawn)."""
    import random
    keys = sorted(set(t["A"]) & set(t["B"])); n = len(keys)
    def eff(ks):
        oa = sum(t["A"][k][0] for k in ks); aa = sum(t["A"][k][1] for k in ks); ob = sum(t["B"][k][0] for k in ks); ab = sum(t["B"][k][1] for k in ks)
        return None if oa == 0 or ob == 0 else ab / ob - aa / oa
    rng = random.Random(band["seed"]); draws = []
    for _ in range(band["resamples"]):
        d = eff([keys[int(rng.random() * n)] for _ in range(n)])
        if d is not None: draws.append(d)
    draws.sort(); lvl = band["level"]
    lo, hi = round(draws[int((1 - lvl) / 2 * len(draws))], 6), round(draws[int((1 + lvl) / 2 * len(draws)) - 1], 6)  # the band's own precision
    zero = lo < 0 < hi
    return {"forks": n, "level": lvl, "interval": [lo, hi], "straddles_zero": zero,
            "sentence": f"The fire's own {lvl:.0%} interval [{lo:.6f}, {hi:.6f}] straddles zero." if zero else None}


def selftest(archived, band_path):
    """Every case prints one ok/FAIL line; the summary counts those lines, so it cannot drift from the cases."""
    import contextlib, io
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        bad = _selftest(archived, band_path)
    text = buf.getvalue(); sys.stdout.write(text)
    lines = [l for l in text.splitlines() if l.startswith(("ok  ", "FAIL"))]
    word = sum(1 for l in lines if l[6:].startswith(("an effect at", "a negative band", "a straddling band", "a band touching", "a point of zero")))
    failed = sum(1 for l in lines if l.startswith("FAIL"))
    print(f"apply selftest: {len(lines) - failed} of {len(lines)} fixtures read as expected "
          f"({len(lines) - word} through the logs and the box record, {word} on the word alone)")
    return 1 if bad or failed else 0


def _selftest(archived, band_path):
    grade, band = pinned(archived, band_path)
    base = {n: [json.loads(l) for l in (pathlib.Path(archived) / f"seat-{n.lower()}" / "events.jsonl")
                .read_text(encoding="utf-8").splitlines() if l.strip()] for n in ("A", "B")}
    ok = {"instance_before": "2026-09-20", "instance_after": "2026-09-20", "verify_before": 0, "verify_after": 0,
          "fingerprint_before": 0, "fingerprint_after": 0, "canary_before": ["PASS"], "canary_after": ["PASS"],
          "seatb_canary": {"think": False, "draft_n": False}}
    lo, hi = band["band"]

    def mut(fn):
        s = copy.deepcopy(base)
        fn(s)
        return s

    def resp(s, name, lane, i=0):
        return [e for e in s[name] if e["event"] == "fork.response" and e["lane"] == lane][i]

    def drop_fork(s, name, lane):
        r = resp(s, name, lane)
        s[name] = [e for e in s[name] if e is not r and e.get("id") != r["parent_id"] and e.get("parent_id") != r["id"]]

    def empty_patches(s, name, keep=0):
        for e in s[name]:
            if e["event"] == "object.patch":
                e["added"], keep = e.get("added", [])[:keep], max(0, keep - len(e.get("added", [])))

    def ask(label, seats, box, want, check=None):
        with tempfile.TemporaryDirectory() as tmp:
            dirs = {}
            for n, ev in seats.items():
                d = pathlib.Path(tmp) / f"seat-{n.lower()}"
                d.mkdir()
                body = ev if isinstance(ev, str) else "".join(json.dumps(e) + "\n" for e in ev)
                (d / "events.jsonl").write_text(body, encoding="utf-8")
                dirs[n] = d
            try:
                out = adjudicate(grade, band, dirs, box)
            except Exception as e:  # a fixture that stops the applier is a failed fixture, not a pass
                print(f"FAIL  {label}: the applier stopped on {type(e).__name__} (expected {want})")
                return True
        good = out["word"] == want and (check is None or check(out))
        print(f"{'ok  ' if good else 'FAIL'}  {label}: {out['word']} (expected {want})")
        if out["word"] == "unadjudicated":
            for reason in out["unadjudicated_checks"]:
                print(f"        {reason}")
        return not good

    torn = "".join(json.dumps(e) + "\n" for e in base["B"])[:-40]
    fixtures = [
        ("the archived logs as a re-fire, a clean box: the effect and counts are band.py's", base, ok, "supported",
         lambda o: round(o["effect"], 6) == band["point"] and o["seat_a"] == band["seat_a"] and o["seat_b"] == band["seat_b"]
         and o["disclosure"] is None),
        ("seat B accepting nothing: the disclosure attached", mut(lambda s: empty_patches(s, "B")), ok, "refuted",
         lambda o: o["disclosure"] == DISCLOSURE and o["effect"] < 0),
        ("seat A accepting nothing, seat B 30: the disclosure attached", mut(lambda s: (empty_patches(s, "A"), empty_patches(s, "B", 30))),
         ok, "inconclusive", lambda o: o["disclosure"] == DISCLOSURE and 0 < o["effect"] < lo),
        ("the instance changed across the fire", base, dict(ok, instance_after="2026-09-21"), "unadjudicated", None),
        ("the instance unregistered on both sides", base, dict(ok, instance_before="unregistered", instance_after="unregistered"), "unadjudicated", None),
        ("the instance missing", base, dict(ok, instance_before=None, instance_after=None), "unadjudicated", None),
        ("box.json lacking a field", base, {k: v for k, v in ok.items() if k != "fingerprint_after"}, "unadjudicated", None),
        ("the verification script failing after", base, dict(ok, verify_after=1), "unadjudicated", None),
        ("the fingerprint check drifting before", base, dict(ok, fingerprint_before=3), "unadjudicated", None),
        ("two canary DRIFTs before", base, dict(ok, canary_before=["DRIFT", "DRIFT"]), "unadjudicated", None),
        ("a canary draw with no verdict after", base, dict(ok, canary_after=["NONE"]), "unadjudicated", None),
        ("one canary DRIFT re-drawn to PASS after", base, dict(ok, canary_after=["DRIFT", "PASS"]), "supported", None),
        ("the seat-B canary answering with a think block", base, dict(ok, seatb_canary={"think": True, "draft_n": False}), "unadjudicated", None),
        ("seat B's run.end missing", mut(lambda s: s.__setitem__("B", [e for e in s["B"] if e["event"] != "run.end"])), ok, "unadjudicated", None),
        ("seat B's run.end not last", mut(lambda s: s["B"].insert(1, s["B"].pop())), ok, "unadjudicated", None),
        ("seat A fired twice into one log", mut(lambda s: s["A"].insert(0, dict(s["A"][0]))), ok, "unadjudicated", None),
        ("seat A's run.begin naming seat B's arm", mut(lambda s: s["A"][0].__setitem__("arm", ARMS["B"])), ok, "unadjudicated", None),
        ("seat B's log torn mid-line", dict(A=base["A"], B=torn), ok, "unadjudicated", None),
        ("a seat-B extraction answered by the 27B", mut(lambda s: resp(s, "B", "extraction")["timings"].__setitem__("draft_n", 1)), ok, "unadjudicated", None),
        ("a seat-B extraction with no timings", mut(lambda s: resp(s, "B", "extraction").__setitem__("timings", None)), ok, "unadjudicated", None),
        ("a seat-B extraction answering with a think block", mut(lambda s: resp(s, "B", "extraction").__setitem__("content", "<think>x</think>")), ok, "unadjudicated", None),
        ("a seat-A extraction answered off the production server", mut(lambda s: resp(s, "A", "extraction")["timings"].pop("draft_n")), ok, "unadjudicated", None),
        ("a seat-B interview answered off the production server", mut(lambda s: resp(s, "B", "interview")["timings"].pop("draft_n")), ok, "unadjudicated", None),
        ("a seat-B extraction fork missing", mut(lambda s: drop_fork(s, "B", "extraction")), ok, "unadjudicated", None),
        ("a seat-B interview fork missing (a silent timeout)", mut(lambda s: drop_fork(s, "B", "interview")), ok, "unadjudicated", None),
        ("a seat-A extraction fork duplicated", mut(lambda s: s["A"].extend([e for e in s["A"] if e.get("lane") == "extraction" and e["event"].startswith("fork.")][:2]) or s["A"].append(s["A"].pop(s["A"].index(next(e for e in s["A"] if e["event"] == "run.end"))))), ok, "unadjudicated", None),
        ("seat A offering nothing", mut(lambda s: [e.__setitem__("content", "") for e in s["A"] if e["event"] == "fork.response" and e["lane"] == "extraction"]), ok, "unadjudicated", None),
        ("seat B offering nothing", mut(lambda s: [e.__setitem__("content", "") for e in s["B"] if e["event"] == "fork.response" and e["lane"] == "extraction"]), ok, "unadjudicated", None),
    ]
    bad = sum(ask(*f) for f in fixtures)
    # ruling 3 (#393 5986337117): the identity routing replaces the draft_n fingerprint for the candidate's responses
    global ROUTING
    keep_r = ROUTING
    try:
        ROUTING = "identity"
        def nreq(s, n):
            return sum(1 for e in s[n] if e["event"] == "fork.response" and (n == "A" or e["lane"] != "extraction"))
        def rbox(s, a=None, b=None, **kw):
            rt = {"model_id": MODEL_ID, "instance": "2026-09-20",
                  "checks": {"A": [MODEL_ID] * nreq(s, "A") if a is None else a, "B": [MODEL_ID] * nreq(s, "B") if b is None else b}}
            rt.update(kw); return dict(ok, routing=rt)
        nodn = lambda s: [e["timings"].pop("draft_n", None) for n in ("A", "B") for e in s[n] if e["event"] == "fork.response" and e.get("timings") and not (n == "B" and e["lane"] == "extraction")]
        idf = []
        s0 = mut(nodn)
        idf.append(("identity routing: no draft_n anywhere on the candidate, a model-id check per request: adjudicated", s0, rbox(s0), "supported", None))
        idf.append(("identity routing: the box record without the routing check is void", s0, ok, "unadjudicated", lambda o: any(r.startswith("routing: the box record lacks") for r in o["unadjudicated_checks"])))
        idf.append(("identity routing: a check naming another model id", s0, rbox(s0, a=[MODEL_ID] * (nreq(s0, "A") - 1) + ["other"]), "unadjudicated", lambda o: any("another id" in r for r in o["unadjudicated_checks"])))
        idf.append(("identity routing: one request without its check", s0, rbox(s0, a=[MODEL_ID] * (nreq(s0, "A") - 1)), "unadjudicated", lambda o: any("model-id checks" in r for r in o["unadjudicated_checks"])))
        idf.append(("identity routing: checks made against another instance", s0, rbox(s0, instance="2026-09-21"), "unadjudicated", lambda o: any(r.startswith("routing: the box record lacks") for r in o["unadjudicated_checks"])))
        idf.append(("identity routing: checks pinned to another model id", s0, rbox(s0, model_id="other"), "unadjudicated", lambda o: any(r.startswith("routing: the box record lacks") for r in o["unadjudicated_checks"])))
        s1 = mut(lambda s: (nodn(s), resp(s, "B", "extraction")["timings"].__setitem__("draft_n", 1)))
        idf.append(("identity routing: a seat-B extraction still must not carry draft_n", s1, rbox(s1), "unadjudicated", None))
        bad += sum(ask(*f) for f in idf)
    finally:
        ROUTING = keep_r
    words = [("the band's lower endpoint", lo, "supported"), ("the band's upper endpoint", hi, "supported"),
             ("just above the band", hi + 1e-9, "inconclusive"), ("between zero and the band", lo / 2, "inconclusive"),
             ("exactly zero", 0.0, "refuted"), ("below zero", -0.01, "refuted")]
    for label, e, want in words:
        got = decide(e, band["band"], band["point"])
        bad += got != want
        print(f"{'ok  ' if got == want else 'FAIL'}  an effect at {label}: {got} (expected {want})")
    # the generalised rule (D7 (a), N1): the reference sign from the band's point; a straddling band cannot refute
    neg, strad = ([-0.2, -0.05], -0.12), ([-0.03, 0.06], 0.01)
    general = [("a negative band: within it", -0.1, neg, "supported"), ("a negative band: beyond it, sign kept", -0.25, neg, "inconclusive"),
               ("a negative band: short of it, sign kept", -0.01, neg, "inconclusive"), ("a negative band: the opposite sign", 0.05, neg, "refuted"),
               ("a negative band: zero", 0.0, neg, "refuted"), ("a straddling band: within it", 0.0, strad, "supported"),
               ("a straddling band: below it", -0.05, strad, "inconclusive"), ("a straddling band: above it", 0.2, strad, "inconclusive"),
               ("a band touching zero from above: a sign reversal refutes", -0.02, ([0.0, 0.1], 0.05), "refuted"),
               ("a band touching zero from above: zero is within it", 0.0, ([0.0, 0.1], 0.05), "supported"),
               ("a band touching zero from below: a sign reversal refutes", 0.02, ([-0.1, 0.0], -0.05), "refuted"),
               ("a point of zero outside a band not containing zero: no reference sign", 0.5, ([0.1, 0.2], 0.0), "inconclusive")]
    for label, e, (bd, pt), want in general:
        got = decide(e, bd, pt)
        bad += got != want
        print(f"{'ok  ' if got == want else 'FAIL'}  {label}: {got} (expected {want})")
    # the seat-A floor (N1): in the fire, and in the band's own fire
    global SEAT_A_FLOOR
    keep = SEAT_A_FLOOR
    try:
        SEAT_A_FLOOR = band["seat_a"]["accepted_deduped"] + 1  # the band's fire itself below the floor
        floors = [("the band's own fire below the seat-A floor", base, ok, "unadjudicated", lambda o: any("band's own fire" in r for r in o["unadjudicated_checks"]))]
        bad += sum(ask(*f) for f in floors)
        SEAT_A_FLOOR = 10  # the band's fire (18) clears it; the fire's seat A accepting nothing does not
        floors = [("seat A accepting nothing in the fire, the floor 10", mut(lambda s: empty_patches(s, "A")), ok, "unadjudicated",
                   lambda o: o["unadjudicated_checks"] == ["floor: seat A accepted 0, below the floor of 10"])]
        bad += sum(ask(*f) for f in floors)
        SEAT_A_FLOOR = band["seat_a"]["accepted_deduped"]  # exactly at the floor, in both fires: adjudicated
        floors = [("seat A exactly at the floor, in both fires: adjudicated", base, ok, "supported", None)]
        bad += sum(ask(*f) for f in floors)
    finally:
        SEAT_A_FLOOR = keep
    # a pre-registration's counted-fork floor (the candidate's, 25 of 31): misses listed per seat, the word
    # unadjudicated only below the floor
    global COUNTED_FLOOR
    keep_c = COUNTED_FLOOR
    def drop_ext(s, name, k):
        for _ in range(k): drop_fork(s, name, "extraction")
    global POOL, REFIRE_INTERVAL, INTERVIEW_MISS, PLANNED
    keep_p = (POOL, REFIRE_INTERVAL, INTERVIEW_MISS, PLANNED)
    try:
        COUNTED_FLOOR = 25; POOL = "per-seat"; REFIRE_INTERVAL = False; INTERVIEW_MISS = "fail"
        cfl = [("a floor of 25, every fork present: adjudicated, no misses", base, ok, "supported",
                lambda o: o["counted_forks"]["A"]["counted"] == 31 and o["counted_forks"]["B"]["misses"] == []),
               ("a floor of 25, seat B missing 7 forks (24 counted): unadjudicated", mut(lambda s: drop_ext(s, "B", 7)), ok, "unadjudicated",
                lambda o: any(r == "floor: seat B counted 24 of its 31 extraction forks, below the floor of 25" for r in o["unadjudicated_checks"]) and o["counted_forks"]["B"]["counted"] == 24),
               ("a floor of 25, seat B missing 6 forks (exactly 25): adjudicated", mut(lambda s: drop_ext(s, "B", 6)), ok, None,
                lambda o: o["word"] != "unadjudicated" and o["counted_forks"]["B"]["counted"] == 25 and len(o["counted_forks"]["B"]["misses"]) == 6),
               ("a floor of 25, seat B missing 5 forks: adjudicated, the 5 listed", mut(lambda s: drop_ext(s, "B", 5)), ok, None,
                lambda o: o["counted_forks"]["B"]["counted"] == 26 and len(o["counted_forks"]["B"]["misses"]) == 5 and o["word"] != "unadjudicated"),
               ("a floor of 25, a seat-B extraction answered by the 27B: not counted, not unadjudicated", mut(lambda s: resp(s, "B", "extraction")["timings"].__setitem__("draft_n", 1)), ok, None,
                lambda o: o["word"] != "unadjudicated" and o["counted_forks"]["B"]["counted"] == 30 and o["counted_forks"]["B"]["misses"][0]["reason"] == "seat-B extraction carrying draft_n")]
        for label, seats, box, want, chk in cfl:
            if want is None:
                with tempfile.TemporaryDirectory() as tmp:
                    dirs = {}
                    for n2, ev in seats.items():
                        d = pathlib.Path(tmp) / f"seat-{n2.lower()}"; d.mkdir(); (d / "events.jsonl").write_text("".join(json.dumps(e) + "\n" for e in ev), encoding="utf-8"); dirs[n2] = d
                    o = adjudicate(grade, band, dirs, box)
                good = chk(o); bad += not good
                print(f"{'ok  ' if good else 'FAIL'}  {label}: {o['word']}")
            else:
                bad += ask(label, seats, box, want, chk)
        # every route to a miss under a floor, and the routes that must still fail the seat
        def adj(seats):
            with tempfile.TemporaryDirectory() as tmp:
                dirs = {}
                for n2, ev in seats.items():
                    d = pathlib.Path(tmp) / f"seat-{n2.lower()}"; d.mkdir(); (d / "events.jsonl").write_text("".join(json.dumps(e) + "\n" for e in ev), encoding="utf-8"); dirs[n2] = d
                return adjudicate(grade, band, dirs, ok)
        routes = [
            ("a seat-A extraction lacking draft_n: a miss", lambda s: resp(s, "A", "extraction")["timings"].pop("draft_n"), "A", "seat-A extraction lacking draft_n", True),
            ("a seat-B extraction with no timings: a miss", lambda s: resp(s, "B", "extraction").__setitem__("timings", None), "B", "no timings", True),
            ("a seat-B extraction with a think block: a miss", lambda s: resp(s, "B", "extraction").__setitem__("content", "<think>x</think>" + resp(s, "B", "extraction")["content"]), "B", "seat-B extraction with reasoning or a think block", True),
            ("a seat-A interview lacking draft_n: still fails the seat", lambda s: resp(s, "A", "interview")["timings"].pop("draft_n"), "A", None, False),
        ]
        for label, fn, seat, why, adjudicated in routes:
            o = adj(mut(fn))
            good = (o["word"] != "unadjudicated") == adjudicated and (why is None or any(m["reason"] == why for m in o["counted_forks"][seat]["misses"]))
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  under a floor, {label}: {o['word']}")
        def dup_ext(s, name):
            r = resp(s, name, "extraction"); q = next(e for e in s[name] if e.get("id") == r["parent_id"])
            s[name].insert(len(s[name]) - 1, dict(q, id=q["id"] + "x")); s[name].insert(len(s[name]) - 1, dict(r, id=r["id"] + "x", parent_id=q["id"] + "x"))
        def foreign_ext(s, name):
            r = resp(s, name, "extraction"); q = next(e for e in s[name] if e.get("id") == r["parent_id"]); q["step"] = r["step"] = 999
        for label, fn in (("a duplicated extraction key under a floor fails the seat", lambda s: dup_ext(s, "A")), ("a foreign extraction key under a floor fails the seat", lambda s: foreign_ext(s, "A"))):
            o = adj(mut(fn)); good = o["word"] == "unadjudicated" and any("each at most once" in r for r in o["unadjudicated_checks"])
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  {label}: {o['word']}")
        # pooling: paired keeps the band's pairing; per-seat does not
        try:
            m6 = mut(lambda s: drop_ext(s, "B", 6))
            POOL = "per-seat"; o1 = adj(m6); POOL = "paired"; o2 = adj(m6)
            good = o2["counted_forks"]["paired"] == 25 and o2["seat_a"]["offered"] < o1["seat_a"]["offered"]
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  paired pooling drops seat A's forks that seat B did not count ({o1['seat_a']['offered']} -> {o2['seat_a']['offered']} offered)")
            def drop_last(s, name, k):
                for _ in range(k):
                    r = [e for e in s[name] if e["event"] == "fork.response" and e["lane"] == "extraction"][-1]
                    s[name] = [e for e in s[name] if e is not r and e.get("id") != r["parent_id"] and e.get("parent_id") != r["id"]]
            m7 = mut(lambda s: (drop_ext(s, "B", 3), drop_last(s, "A", 4)))
            o3 = adj(m7); good = o3["word"] == "unadjudicated" and o3["counted_forks"].get("paired") == 24
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  paired pooling: 27 and 28 counted but 24 in common is below the floor: {o3['word']}")
            REFIRE_INTERVAL = True; POOL = "paired"; o4 = adj(base); ri = o4.get("refire_interval", {})
            want = {"forks": 31, "interval": band["band"], "straddles_zero": False, "sentence": None}
            good = {k: ri.get(k) for k in want} == want
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  the fire's own interval over the archived logs is the band itself (band.py's method): {ri.get('interval')}" + ("" if good else f" {ri}"))
            t0 = {n: check_seat(grade, n, pathlib.Path(archived) / f"seat-{n.lower()}", band["keys"])[1] for n in ("A", "B")}
            # seat B's accepted counts cut to a third: the fire's effect falls toward zero, and its interval reaches across it
            t1 = {"A": t0["A"], "B": {k: (o, a // 3) for k, (o, a) in t0["B"].items()}}
            r1 = refire_interval(t1, band)
            good = r1["straddles_zero"] is True and r1["sentence"] is not None and "straddles zero" in r1["sentence"] and r1["interval"][0] < 0 < r1["interval"][1]
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  an interval reaching across zero carries the one sentence: {r1['interval']}")
            good = "edge" not in json.dumps(ri)
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  the band-edge sentence is withdrawn: an interval inside the band carries no sentence")
            # planning's rulings (#143 comment 5921525110): the floor on the intersection only, under paired pooling
            POOL = "paired"; REFIRE_INTERVAL = False
            o6 = adj(mut(lambda s: drop_ext(s, "B", 7)))
            good = o6["word"] == "unadjudicated" and o6["unadjudicated_checks"] == ["floor: the seats counted 24 forks in common, below the floor of 25"] \
                and o6["counted_forks"]["A"]["counted"] == 31 and o6["counted_forks"]["B"]["counted"] == 24
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  paired: the floor is the intersection's alone, each seat's total reported: {o6['unadjudicated_checks']}")
            # a content refusal is a counted fork
            def refuse(s):
                resp(s, "B", "extraction")["content"] = "I can't help with that request."
            o7 = adj(mut(refuse))
            good = o7["word"] != "unadjudicated" and o7["counted_forks"]["B"]["counted"] == 31 and o7["counted_forks"]["B"]["misses"] == []
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  a content refusal is a counted fork: {o7['word']}, seat B counted {o7['counted_forks']['B']['counted']}")
            # interview misses under byte-match: reported unless a planned fork ran with another request
            INTERVIEW_MISS = "byte-match"
            PLANNED = {n: {k: v[0] for k, v in fork_requests(base[n]).items()} for n in ("A", "B")}
            o8 = adj(mut(lambda s: drop_fork(s, "B", "interview")))
            good = o8["word"] == "supported" and len(o8["interview_misses"]["B"]) == 1 and o8["interview_misses"]["A"] == []
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  byte-match: an interview miss leaving every request planned is reported, not voiding: {o8['word']}")
            def alter_later(s):
                q = [e for e in s["B"] if e["event"] == "fork.request" and e["lane"] == "extraction"][-1]
                q["messages"] = q["messages"][:-1] + [dict(q["messages"][-1], content=q["messages"][-1]["content"] + " ")]
            o9 = adj(mut(lambda s: (drop_fork(s, "B", "interview"), alter_later(s))))
            good = o9["word"] == "unadjudicated" and any(r.startswith("requests: seat B's planned request changed at 1 fork(s)") for r in o9["unadjudicated_checks"])
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  byte-match: a miss that changed a later request voids the fire: {o9['word']}")
            o10 = adj(mut(lambda s: resp(s, "A", "interview")["timings"].pop("draft_n")))
            good = o10["word"] == "unadjudicated" and any(r.startswith("routing:") for r in o10["unadjudicated_checks"])
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  byte-match: a mis-routed interview stays unadjudicated: {o10['word']}")
            def extra_interview(s):  # the replay asking one more interview than planned, under a new key
                r0 = resp(s, "A", "interview"); q0 = next(e for e in s["A"] if e.get("id") == r0["parent_id"])
                q1 = dict(q0, id=q0["id"] + "x"); r1 = dict(r0, id=r0["id"] + "x", parent_id=q1["id"], step=9999)
                s["A"].insert(len(s["A"]) - 1, q1); s["A"].insert(len(s["A"]) - 1, r1)
            o12 = adj(mut(extra_interview))
            good = o12["word"] == "supported"
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  byte-match: an unplanned extra interview is not the plan's business: {o12['word']}")
            def suppress_later(s):  # a later planned interview that never ran: a miss, reported, not a changed request
                drop_fork(s, "A", "interview")
            o13 = adj(mut(suppress_later))
            good = o13["word"] == "supported" and len(o13["interview_misses"]["A"]) == 1
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  byte-match: a planned fork that did not run is a miss, not a changed request: {o13['word']}")
            def dup_changed(s):  # one planned interview run twice, the second copy with another request
                r0 = resp(s, "A", "interview"); q0 = next(e for e in s["A"] if e.get("id") == r0["parent_id"])
                q1 = json.loads(json.dumps(q0)); q1["id"] += "x"; q1["messages"][-1]["content"] += " "
                r1 = dict(r0, id=r0["id"] + "x", parent_id=q1["id"])
                s["A"].insert(len(s["A"]) - 1, q1); s["A"].insert(len(s["A"]) - 1, r1)
            o14 = adj(mut(dup_changed))
            good = o14["word"] == "unadjudicated" and any(r.startswith("requests: seat A's planned request changed") for r in o14["unadjudicated_checks"])
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  byte-match: a planned fork run twice must match on every copy: {o14['word']}")
            o11 = adj(base)
            good = o11["word"] == "supported" and o11["interview_misses"] == {"A": [], "B": []}
            bad += not good; print(f"{'ok  ' if good else 'FAIL'}  byte-match: the archived logs as a re-fire, every request planned: {o11['word']}")
        finally:
            POOL, REFIRE_INTERVAL, INTERVIEW_MISS, PLANNED = keep_p
        COUNTED_FLOOR = 32; o5 = adj(base); good = o5["word"] == "unadjudicated" and any("exceeds the band's 31" in r for r in o5["unadjudicated_checks"])
        bad += not good; print(f"{'ok  ' if good else 'FAIL'}  a floor above the fork count can never be met: {o5['word']}")
    finally:
        COUNTED_FLOOR = keep_c
    return 1 if bad else 0


def main(argv):
    if len(argv) == 3 and argv[0] == "--check-plan":  # a fire's logs against the config's committed plan
        configure(argv[1])
        if INTERVIEW_MISS != "byte-match":
            cannot("the config carries no plan")
        bad = 0
        for n in ("A", "B"):
            ev = [json.loads(l) for l in (pathlib.Path(argv[2]) / f"seat-{n.lower()}" / "events.jsonl").read_text(encoding="utf-8").splitlines() if l.strip()]
            got, ch = fork_requests(ev), changed_requests(n, ev)
            ran = sum(1 for k in PLANNED[n] if got.get(k))
            print(f"seat {n}: {ran} of {len(PLANNED[n])} planned forks ran, {len(ch)} with a changed request{': ' + ', '.join(ch) if ch else ''}")
            bad += bool(ch)
        return 1 if bad else 0
    if len(argv) == 4 and argv[0] == "--selftest":
        configure(argv[1])
        return selftest(argv[2], argv[3])
    interval_out = None
    if len(argv) == 7 and argv[5] == "--interval-out":
        interval_out, argv = argv[6], argv[:5]
    if len(argv) != 5:
        cannot("usage: apply.py CONFIG ARCHIVED_DIR BAND_JSON REFIRE_DIR BOX_JSON [--interval-out PATH], or --selftest CONFIG ARCHIVED_DIR BAND_JSON")
    configure(argv[0])
    if REFIRE_INTERVAL and interval_out is None:
        cannot("the config reports the fire's interval: pass --interval-out PATH")
    archived, band_path, refire, box_path = argv[1:]
    grade, band = pinned(archived, band_path)
    try:
        box = json.loads(pathlib.Path(box_path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, UnicodeDecodeError):
        cannot("box.json cannot be read")
    if not isinstance(box, dict):
        cannot("box.json is not an object")
    out = adjudicate(grade, band, {n: pathlib.Path(refire) / f"seat-{n.lower()}" for n in ("A", "B")}, box)
    interval = out.pop("refire_interval", None)  # beside the word, never in the verdict: the verdict's bytes are the rule's alone
    print(json.dumps(out, indent=1))
    if interval_out is not None:
        pathlib.Path(interval_out).write_text(json.dumps(interval, indent=1) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except SystemExit:
        raise
    except Exception as e:  # never a traceback, which would print the caller's paths
        cannot(f"stopped on {type(e).__name__}")
