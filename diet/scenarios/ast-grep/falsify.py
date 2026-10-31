"""The falsification's runner (#971; PREREG.md and its Amendment 1, 2026-10-29): python3 -I falsify.py --out DIR
--notes FILE [--before CMD] [--max-seconds S] [-- DRIVE_ARGS...], or --selftest.

Do tailored notes beat native ast-grep use on cost, at equal correctness? 12 set-1 tasks (t1..t6, h1..h6) x 10 seeds
(101..104, 106..111: Amendment 1) x 3 arms, each pass a fresh instance under the floor's regimen, in the native
condition (the passes' own, `make_scenario.py`: ast-grep runs, the fixture's truth a secret), no notes turn, and
execution as the verdict (`--verdict execute`, check.py's `grade_executed`). The arms differ only in the preamble:
- N: `head-native.md`;
- S: `head-native-skills.md`, with the official skills on disk (the `-skills` pass);
- T: `head-native.md` with FILE (the floor's gap-filled notes) under the notes heading.
The order: per (task, seed), in task-then-seed order, the three arms shuffled by one `random.Random(29)`. Before each
pass, `--before` (the idle gate); a pass is stopped at `--max-seconds` (3600). A void or stopped pass is re-run once
on the same seed; failing again, it is recorded void and counts as not right.

DIR/rows.jsonl, one row per pass (the measures, Amendment 1): `executed_right`, `executed`, and the cost of
what the pass spent: `tool_calls`, `seconds`, `completion_tokens`, `output_tokens` -- before the answer when there is
one, else to the end of its task turn -- plus `searches` (ast-grep searches run) and `docs_reads`. Re-running on the
same DIR resumes: a (task, seed, arm) with a row is not run again. DIR/manifest.json records the build commit, the
PREREG files' sha256, the notes' sha256 and the plan.
"""
import hashlib, importlib, json, os, random, signal, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, HERE)
import check  # noqa: E402

tools_of = importlib.import_module("pass").tools_of  # `pass` is a keyword, so by name
TASKS = ["t1-ret-decl", "t2-zero-calls", "t3-public-surface", "t4-await-any", "t5-emit-calls", "t6-fn-outline",
         "h1-ret-arrow", "h2-maybe-defined", "h3-public-types", "h4-log-in-try", "h5-track-calls", "h6-itf-outline"]
SEEDS = [101, 102, 103, 104, 106, 107, 108, 109, 110, 111]  # Amendment 1: 105 resolves to 106
ARMS = ("N", "S", "T")
ORDER_SEED = 29


def plan():
    """[(task, seed, arm)]: per (task, seed) in order, its arms shuffled by one Random(29)."""
    rng, out = random.Random(ORDER_SEED), []
    for task in TASKS:
        for seed in SEEDS:
            arms = list(ARMS)
            rng.shuffle(arms)
            out += [(task, seed, arm) for arm in arms]
    return out


def run(cmd, max_seconds):
    """CMD in its own session, stopped whole past MAX_SECONDS: (exit, stopped)."""
    p = subprocess.Popen(cmd, start_new_session=True)
    try:
        return p.wait(timeout=max_seconds), False
    except subprocess.TimeoutExpired:
        os.killpg(p.pid, signal.SIGTERM)
        try:
            p.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL)
            p.wait()
        return None, True


def args_of(line):
    try:
        a = json.loads(line.get("arguments") or "")
    except ValueError:
        return {}
    return a if isinstance(a, dict) else {}


def measure(lines, task):
    """A pass's cost (Amendment 1): before the answer when it gave one, else everything its task turn spent."""
    ask = next(l for l in lines if l.get("kind") == "ask" and l.get("task") == task)
    turn, t0 = ask["turn"], ask.get("t", 0)
    req_turn = {l.get("seq"): l.get("turn") for l in lines if l.get("kind") == "request"}
    in_turn = [l for l in lines if (req_turn.get(l.get("to_request")) if l.get("kind") == "response" else l.get("turn")) == turn]
    calls = [l for l in in_turn if l.get("kind") == "tool_call"]
    answer, _ = check.answered({"calls": calls})
    answer_call = None
    if answer is not None:
        answer_call = next(c for c in reversed(calls) if c.get("name") == check.TOOL and c.get("outcome") != "refused"
                           and args_of(c).get("command") == answer["command"])
    end = answer_call.get("seq") if answer_call else None
    before = [l for l in in_turn if end is None or (l.get("seq") or 0) < end]
    responses = [l for l in before if l.get("kind") == "response"]
    t_end = answer_call.get("t", t0) if answer_call else max([l.get("t", t0) for l in in_turn] or [t0])
    bash = [args_of(c).get("command", "") for c in before if c.get("kind") == "tool_call" and c.get("name") == "bash"]
    return {"answered": answer is not None,
            "tool_calls": sum(1 for l in before if l.get("kind") == "tool_call" and l is not answer_call),
            "seconds": round((t_end - t0) / 1000, 1),
            "completion_tokens": sum((l.get("usage") or {}).get("completion_tokens", 0) for l in responses),
            "output_tokens": sum((l.get("usage") or {}).get("completion_tokens", 0) for l in in_turn if l.get("kind") == "response"),
            "capped": sum(l.get("finish_reason") == "length" for l in in_turn if l.get("kind") == "response"),
            "searches": sum(tools_of(c).count("ast-grep") for c in bash)}


def pass_cmd(task, seed, arm, out, args):
    cmd = [sys.executable, "-I", os.path.join(HERE, "run_pass.py"), task, "--out", out, "--floor",
           "--seed", str(seed), "--verdict", "execute",
           "--head", os.path.join(HERE, "head-native-skills.md" if arm == "S" else "head-native.md")]
    cmd += ["--skills"] if arm == "S" else []
    cmd += ["--notes", args["notes"]] if arm == "T" else []
    return cmd + (["--"] + args["drive"] if args["drive"] else [])


def void_row(row, lines, task, wall):
    """A void or stopped pass, not right, with what it spent (Amendment 1): measured from what its log holds, its
    seconds at least its wall time when stopped; with no log of its task at all, its wall time alone, marked
    `cost_incomplete`."""
    row = dict(row, void=True, executed_right=False)
    if lines and any(l.get("kind") == "ask" and l.get("task") == task for l in lines):
        m = measure(lines, task)
        if row["stopped"]:
            m["seconds"] = max(m["seconds"], wall)
        return dict(row, **m)
    return dict(row, tool_calls=0, seconds=wall, completion_tokens=0, output_tokens=0, searches=0, docs_reads=0, cost_incomplete=True)


def row_of(task, seed, arm, out, attempt, stopped, wall=0):
    """One pass's row from its log and pass.json, the verdict re-derived offline as the check gave it."""
    row = {"task": task, "seed": seed, "arm": arm, "attempt": attempt, "pass": out, "stopped": stopped}
    log, truth = os.path.join(out, "run", "session.log"), os.path.join(out, "run", "services", "fixture", "truth.json")
    lines = []
    if os.path.exists(log):
        for l in open(log):
            try:
                lines.append(json.loads(l))
            except ValueError:  # a stopped pass's last line may be cut
                pass
    if stopped or not os.path.exists(truth) or not any(l.get("kind") == "ask" and l.get("task") == task for l in lines):
        return void_row(row, lines, task, wall)
    ask_turn = next(l["turn"] for l in lines if l.get("kind") == "ask" and l.get("task") == task)
    calls = [l for l in lines if l.get("kind") == "tool_call" and l.get("turn") == ask_turn]
    g = check.grade_executed({"calls": calls}, task, json.load(open(truth)), os.path.join(os.path.dirname(truth), "pristine"))
    graded = next((bool(l.get("right")) for l in lines if l.get("kind") == "grade" and l.get("task") == task), None)
    p = os.path.join(out, "pass.json")
    docs = len(json.load(open(p))["tasks"].get(task, {}).get("docs_reads", [])) if os.path.exists(p) else None
    answer, _ = check.answered({"calls": calls})
    return dict(row, void=False, executed_right=g["right"], executed=g["executed"],
                graded_right=graded, command=(answer or {}).get("command"), docs_reads=docs, **measure(lines, task))


def parse(argv):
    drive = argv[argv.index("--") + 1:] if "--" in argv else []
    argv = argv[:argv.index("--")] if "--" in argv else argv
    get = lambda flag, default=None: argv[argv.index(flag) + 1] if flag in argv else default
    if not get("--out") or not get("--notes"):
        sys.exit(__doc__)
    return {"out": os.path.abspath(get("--out")), "notes": os.path.abspath(get("--notes")), "before": get("--before"),
            "max_seconds": int(get("--max-seconds", 3600)), "drive": drive}


def sha(path):
    return hashlib.sha256(open(path, "rb").read()).hexdigest() if path and os.path.exists(path) else None


def main(argv):
    args = parse(argv)
    os.makedirs(args["out"], exist_ok=True)
    rows_path = os.path.join(args["out"], "rows.jsonl")
    done = {(r["task"], r["seed"], r["arm"]) for r in map(json.loads, open(rows_path))} if os.path.exists(rows_path) else set()
    manifest = os.path.join(args["out"], "manifest.json")
    if not os.path.exists(manifest):
        prereg = os.environ.get("DIET_FALSIFY_PREREG_DIR", "")
        commit = subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
        dirty = subprocess.run(["git", "-C", ROOT, "status", "--porcelain"], capture_output=True, text=True).stdout.strip() != ""
        json.dump({"build_commit": commit, "build_dirty": dirty, "notes_sha256": sha(args["notes"]),
                   "prereg_sha256": sha(os.path.join(prereg, "PREREG.md")), "amendment_1_sha256": sha(os.path.join(prereg, "PREREG-AMENDMENT-1.md")),
                   "seeds": SEEDS, "order_seed": ORDER_SEED, "plan": plan(), "drive_args": args["drive"]},
                  open(manifest, "w"), indent=1)
    for task, seed, arm in plan():
        if (task, seed, arm) in done:
            continue
        for attempt in (1, 2):  # a void or stopped pass is re-run once on the same seed
            out = os.path.join(args["out"], "passes", f"{task}-{seed}-{arm}-{attempt}")
            if args["before"]:
                subprocess.run(["bash", "-c", args["before"]], check=False)
            started = time.time()
            _, stopped = run(pass_cmd(task, seed, arm, out, args), args["max_seconds"])
            wall = round(time.time() - started)
            row = row_of(task, seed, arm, out, attempt, stopped, wall)
            row["wall_seconds"] = wall
            if not row["void"] or attempt == 2:
                break
        with open(rows_path, "a") as f:
            f.write(json.dumps(row) + "\n")
        print(json.dumps({k: row.get(k) for k in ("task", "seed", "arm", "executed_right", "tool_calls", "seconds", "void")}), flush=True)
    return 0


def selftest():
    bad = 0
    p = plan()
    if len(p) != 360 or len(set(p)) != 360 or p != plan():
        print(f"falsify: the plan is {len(p)} passes, {len(set(p))} distinct, reproducible {p == plan()}"); bad += 1
    groups = [p[i:i + 3] for i in range(0, 360, 3)]
    if any(len({(t, s) for t, s, _ in g}) != 1 or sorted(a for _, _, a in g) != list(ARMS) for g in groups):
        print("falsify: a group is not one (task, seed) with its three arms"); bad += 1
    if len({tuple(a for _, _, a in g) for g in groups}) < 3:
        print("falsify: the arms' order is not shuffled"); bad += 1
    # The cost of a pass with an answer stops at it; without one, it runs to the end of the turn (Amendment 1).
    req = lambda seq, turn: {"kind": "request", "seq": seq, "turn": turn}
    resp = lambda seq, to, n, t, fin="tool_calls": {"kind": "response", "seq": seq, "to_request": to, "usage": {"completion_tokens": n}, "t": t, "finish_reason": fin}
    call = lambda seq, name, t, **a: {"kind": "tool_call", "seq": seq, "name": name, "turn": 1, "t": t, "outcome": "ran", "arguments": json.dumps(a)}
    ask = {"kind": "ask", "seq": 1, "turn": 1, "task": "t1-ret-decl", "t": 1000}
    answered = [ask, req(2, 1), resp(3, 2, 100, 2000), call(4, "bash", 3000, command="ast-grep run -p x src"),
                call(5, "bash", 4000, command="ast-grep run --help"), req(6, 1), resp(7, 6, 50, 5000),
                call(8, check.TOOL, 6000, command="ast-grep run -p y src"), req(9, 1), resp(10, 9, 20, 9000)]
    m = measure(answered, "t1-ret-decl")
    want = {"answered": True, "tool_calls": 2, "seconds": 5.0, "completion_tokens": 150, "output_tokens": 170, "searches": 1}
    for k, w in want.items():
        if m[k] != w:
            print(f"falsify: answered {k}: {m[k]}, expected {w}"); bad += 1
    gave_up = [l for l in answered if l.get("name") != check.TOOL] + [req(11, 1), resp(12, 11, 30, 12000, "length")]
    m = measure(gave_up, "t1-ret-decl")
    want = {"answered": False, "tool_calls": 2, "seconds": 11.0, "completion_tokens": 200, "output_tokens": 200, "capped": 1}
    for k, w in want.items():
        if m[k] != w:
            print(f"falsify: no answer {k}: {m[k]}, expected {w}"); bad += 1
    a = parse(["--out", "/x", "--notes", "/n.md"])
    for arm, have, lack in (("N", ["--floor", "--verdict", "execute", "head-native.md"], ["--skills", "--notes", "head-native-skills.md"]),
                            ("S", ["--skills", "head-native-skills.md"], ["--notes"]), ("T", ["--notes", "/n.md", "head-native.md"], ["--skills"])):
        cmd = pass_cmd("t1-ret-decl", 101, arm, "/x/p", a)
        joined = " ".join(cmd)
        if [w for w in have if w not in joined] or [w for w in lack if w in cmd]:
            print(f"falsify: arm {arm}'s command: {cmd}"); bad += 1
    print(f"falsify: a 360-pass plan, its order, the two cost rules and three arms' commands, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main(sys.argv[1:]))
