#!/usr/bin/env python3
"""The applier for the extraction-acceptance-inverts parity fire. It reads the
archived row (pinned), the band (pinned), the re-fired seats' logs and the box
record, and writes the word. The rule, as ruled on #89 (2026-09-24): supported
if the re-fired effect lands within the band; refuted if outside with the sign
reversed; inconclusive if outside with the sign preserved -- a refuted or
inconclusive word carrying the unmeasured-baseline disclosure.

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
  offered  each seat offers at least one fact.

The effect is computed exactly as band.py computes the archived one: per fork,
offered and accepted through the archived row's own grade.py, accepted capped
at offered, pooled per seat from the integer tallies, seat B minus seat A, in
IEEE double, compared unrounded against the band's printed endpoints.

Usage:
  apply.py ARCHIVED_DIR BAND_JSON REFIRE_DIR BOX_JSON   -> prints the verdict as JSON
  apply.py --selftest ARCHIVED_DIR BAND_JSON           -> runs the fixtures
ARCHIVED_DIR holds the archived row's grade.py, seat-a/events.jsonl and
seat-b/events.jsonl; REFIRE_DIR holds the re-fired seat-a/ and seat-b/ logs.
Exit 0 a verdict printed (unadjudicated included); 1 a selftest fixture read
other than expected; 2 an input is missing or not the pinned bytes.
"""
import collections, copy, hashlib, importlib.util, json, pathlib, re, sys, tempfile

sys.dont_write_bytecode = True

PINNED = {
    "grade.py": "01765abb034eee57db7d573fb5414d6e394322afdae99008b9ff344523812778",
    "seat-a/events.jsonl": "1b1fdd414fb63f9da75b2a15ccc7f13131c519c731e3f3b4e70394b75c55d749",
    "seat-b/events.jsonl": "030813139cf9bd0065f003816a55b8bacbb46ec6c22b40e11340a53793867e36",
}
BAND_SHA = "e32eae7aab07590e33c3f25012ff32f83de4fe4db13e61ba1d2f524cf34c6cf6"
ARMS = {"A": "extraction-seat-a-warm27b", "B": "extraction-seat-b-offboard17b"}
MODEL_ID = "Qwen3.6-27B"
INTERVIEWS = [
    [2, 2, "evidence"], [2, 7, "evidence"], [2, 8, "evidence"], [2, 8, "open_next"], [2, 10, "evidence"],
    [2, 12, "evidence"], [2, 14, "evidence"], [2, 15, "evidence"], [2, 17, "evidence"], [2, 18, "open_next"],
    [2, 23, "constraints"], [2, 23, "decisions"], [2, 23, "gotchas"], [2, 24, "constraints"], [2, 24, "decisions"],
    [2, 24, "evidence"], [2, 24, "gotchas"], [2, 28, "evidence"], [2, 28, "open_next"], [2, 29, "constraints"],
    [2, 29, "decisions"], [2, 29, "gotchas"], [3, 8, "open_next"], [4, 5, "constraints"], [4, 5, "decisions"],
    [4, 5, "evidence"], [4, 5, "gotchas"], [4, 7, "evidence"], [4, 7, "open_next"], [4, 8, "evidence"],
    [4, 9, "evidence"], [4, 10, "evidence"], [4, 11, "evidence"], [4, 12, "evidence"], [4, 13, "evidence"],
    [4, 14, "evidence"], [4, 15, "constraints"], [4, 15, "decisions"], [4, 15, "evidence"], [4, 15, "gotchas"],
    [4, 16, "evidence"], [4, 17, "evidence"], [4, 17, "open_next"], [4, 19, "constraints"], [4, 19, "decisions"],
    [4, 19, "gotchas"], [4, 22, "constraints"], [4, 22, "decisions"], [4, 22, "evidence"], [4, 22, "gotchas"],
    [4, 23, "evidence"], [4, 24, "evidence"], [5, 1, "open_next"], [5, 6, "constraints"], [5, 6, "decisions"],
    [5, 6, "gotchas"],
]
BOX_FIELDS = ("instance_before", "instance_after", "verify_before", "verify_after",
              "fingerprint_before", "fingerprint_after", "canary_before", "canary_after", "seatb_canary")
DISCLOSURE = (
    "The archived fire's baseline was not measured, and each unmeasured part is a candidate "
    "explanation of this word that the record cannot exclude: the substrate instance was inferred "
    "from a later capture's uptime and the archive, no capture having been taken at that fire; seat "
    "B's engine binary and weights were not recorded, and its launch configuration is known only "
    "from the research program's note of the fire; and the harness is identified by commit and "
    "rebuilt, its binary and its working tree at the fire not recorded."
)


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


def decide(effect, band):
    lo, hi = band
    if lo <= effect <= hi:
        return "supported"
    if effect <= 0:
        return "refuted"
    return "inconclusive"


def check_box(box):
    missing = [f for f in BOX_FIELDS if f not in box]
    if missing:
        return [f"box: box.json lacks {', '.join(missing)}"]
    r = []
    ib, ia = box["instance_before"], box["instance_after"]
    if not (isinstance(ib, str) and re.fullmatch(r"\d{4}-\d{2}-\d{2}", ib) and ib == ia):
        r.append("box: the instance is not one registry instance id before and after")
    for f in ("verify_before", "verify_after", "fingerprint_before", "fingerprint_after"):
        if box[f] != 0:
            r.append(f"box: {f} exited {box[f]!r}, not 0")
    for f in ("canary_before", "canary_after"):
        if not canary_ok(box[f]):
            r.append(f"box: {f} is {box[f]!r}, not PASS within one re-draw")
    if box["seatb_canary"] != {"think": False, "draft_n": False}:
        r.append(f"box: the seat-B pre-fire answer is {box['seatb_canary']!r}")
    return r


def check_seat(grade, name, seat_dir, keys):
    """Returns (reasons, tallies or None)."""
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
        for f in forks:
            t, ext = f["timings"], f["lane"] == "extraction"
            if not t:
                n["no timings"] += 1
            elif name == "B" and ext:
                if "draft_n" in t:
                    n["seat-B extraction carrying draft_n"] += 1
            elif "draft_n" not in t:
                n[f"seat-{name} {f['lane']} lacking draft_n"] += 1
        if name == "B":
            for e in ev:
                if e.get("event") == "fork.response" and e.get("lane") == "extraction" and (
                        e.get("reasoning") or "<think>" in (e.get("content") or "")):
                    n["seat-B extraction with reasoning or a think block"] += 1
        r += [f"routing: {k}: {v}" for k, v in sorted(n.items())]
        tallies, dup = {}, False
        for f in forks:
            if f["lane"] != "extraction":
                continue
            key = (f["turn"], f["step"])
            dup |= key in tallies
            offered = len(grade.raw_facts(f["content"]))
            tallies[key] = (offered, min(len(s["accepted"].get(f["id"], [])), offered))
        if dup or set(tallies) != keys:
            r.append(f"forks: seat {name}'s extraction forks are not the archived {len(keys)} keys, each once")
        have = collections.Counter((f["turn"], f["step"], f["ask"]) for f in forks if f["lane"] == "interview")
        short = collections.Counter(tuple(k) for k in INTERVIEWS) - have
        if short:
            r.append(f"forks: seat {name} lacks {sum(short.values())} of the {len(INTERVIEWS)} planned interview forks")
        if sum(v[0] for v in tallies.values()) == 0:
            r.append(f"offered: seat {name} offers no fact")
        return r, tallies
    except (json.JSONDecodeError, KeyError, TypeError, AttributeError, UnicodeDecodeError) as e:
        return [f"logs: seat {name}'s log is partial or malformed ({type(e).__name__})"], None


def adjudicate(grade, band, seat_dirs, box):
    reasons = check_box(box)
    t = {}
    for name in ("A", "B"):
        r, t[name] = check_seat(grade, name, seat_dirs[name], band["keys"])
        reasons += r
    out = {"unadjudicated_checks": reasons or "all passed", "band": band["band"]}
    if reasons:
        out.update(word="unadjudicated", effect=None, disclosure=None)
        return out
    oa, aa = sum(v[0] for v in t["A"].values()), sum(v[1] for v in t["A"].values())
    ob, ab = sum(v[0] for v in t["B"].values()), sum(v[1] for v in t["B"].values())
    effect = ab / ob - aa / oa
    word = decide(effect, band["band"])
    out.update(seat_a={"offered": oa, "accepted_deduped": aa}, seat_b={"offered": ob, "accepted_deduped": ab},
               effect=effect, word=word, disclosure=DISCLOSURE if word in ("refuted", "inconclusive") else None)
    return out


def selftest(archived, band_path):
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
    words = [("the band's lower endpoint", lo, "supported"), ("the band's upper endpoint", hi, "supported"),
             ("just above the band", hi + 1e-9, "inconclusive"), ("between zero and the band", lo / 2, "inconclusive"),
             ("exactly zero", 0.0, "refuted"), ("below zero", -0.01, "refuted")]
    for label, e, want in words:
        got = decide(e, band["band"])
        bad += got != want
        print(f"{'ok  ' if got == want else 'FAIL'}  an effect at {label}: {got} (expected {want})")
    total = len(fixtures) + len(words)
    print(f"apply selftest: {total - bad} of {total} fixtures read as expected "
          f"({len(fixtures)} through the logs and the box record, {len(words)} on the word alone)")
    return 1 if bad else 0


def main(argv):
    if len(argv) == 3 and argv[0] == "--selftest":
        return selftest(argv[1], argv[2])
    if len(argv) != 4:
        cannot("usage: apply.py ARCHIVED_DIR BAND_JSON REFIRE_DIR BOX_JSON, or --selftest ARCHIVED_DIR BAND_JSON")
    archived, band_path, refire, box_path = argv
    grade, band = pinned(archived, band_path)
    try:
        box = json.loads(pathlib.Path(box_path).read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError, UnicodeDecodeError):
        cannot("box.json cannot be read")
    if not isinstance(box, dict):
        cannot("box.json is not an object")
    out = adjudicate(grade, band, {n: pathlib.Path(refire) / f"seat-{n.lower()}" for n in ("A", "B")}, box)
    print(json.dumps(out, indent=1))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main(sys.argv[1:]))
    except SystemExit:
        raise
    except Exception as e:  # never a traceback, which would print the caller's paths
        cannot(f"stopped on {type(e).__name__}")
