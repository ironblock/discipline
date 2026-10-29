"""One gap-fill round's runs (#971; the maintainer's change of direction, Dispatch 2026-10-28): python3 -I tutor_round.py
--notes FILE --out DIR [--tasks t1,t2,...] [--held-out] [--skills] [--floor] [--before CMD] [--max-seconds S]
[-- DRIVE_ARGS...], or --selftest.

A stronger model (the tutor, run by Dispatch) edits a model's notes until its failures disappear; this is the
instrument it reads. Each task runs once on a fresh instance with the notes FILE and **no notes turn**
(`run_pass.py --no-notes`), under the same gates as the loop: `--before` (the host's idle gate), `--max-seconds` (3600,
the process group stopped and recorded `stopped`), `--floor` (the floor's regimen).

Writes DIR/rows.jsonl, one row per task: right, kind and the rubric's reason, the command, the trial searches and their
commands, the docs read, and the par fields (`pass.py`'s). The tutorial run (t1..t6 by default) also writes
DIR/failures.md, the tutor's input: per failed task its ask, command, kind and reason, trial commands, and the last
1,500 characters of reasoning before the answer. Private run data, never committed.

`--held-out` runs task set 2's held-out set (h7..h17), `--held-out-1` set 1's (h1..h6), or `--tasks` from either; each
writes **no failures.md**: the tutor never sees a held-out failure. A task list that mixes the splits is refused. The
tutorial run is t1..t22 by default. DRAFT and experimental.
"""
import importlib, json, os, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import check, loop  # noqa: E402
from tasks import HELD_OUT, HELD_OUT_1, TUTORIAL  # noqa: E402

tools_of = importlib.import_module("pass").tools_of  # `pass` is a keyword, so by name
REASONING_TAIL = 1500
PAR = ("seconds", "tool_calls", "completion_tokens_before_answer", "reasoning_chars_before_answer", "output_tokens", "capped")


def args_of(line):
    try:
        a = json.loads(line.get("arguments") or "")
    except ValueError:
        return {}
    return a if isinstance(a, dict) else {}


def details(lines, task, truth):
    """The task turn's grade (check.grade, as the check graded it), its ask, trial commands and reasoning tail."""
    ask = next(l for l in lines if l.get("kind") == "ask" and l.get("task") == task)
    turn = ask["turn"]
    calls = [l for l in lines if l.get("kind") == "tool_call" and l.get("turn") == turn]
    g = check.grade({"calls": calls}, task, truth)
    answer, _ = check.answered({"calls": calls})
    answer_seq = next((c.get("seq") for c in calls if c.get("name") == check.TOOL and c.get("outcome") != "refused"
                       and args_of(c).get("command") == (answer or {}).get("command")), None)
    before = [c for c in calls if answer_seq is None or (c.get("seq") or 0) < answer_seq]
    trials = [args_of(c).get("command", "") for c in before if c.get("name") == "bash" and "ast-grep" in tools_of(args_of(c).get("command", ""))]
    req_turn = {l.get("seq"): l.get("turn") for l in lines if l.get("kind") == "request"}
    reasoning = "".join(l.get("reasoning") or "" for l in lines if l.get("kind") == "response" and req_turn.get(l.get("to_request")) == turn
                        and (answer_seq is None or (l.get("seq") or 0) < answer_seq))
    verdict, _, why = g["got"].partition(": ")
    kind = g["kind"]
    # A generation cut at its output cap before any answer is a typed outcome, never wrong (diet/AGENTS.md): "capped",
    # listed for the tutor with its reasoning tail (Dispatch, 2026-10-28).
    cut = [l for l in lines if l.get("kind") == "response" and req_turn.get(l.get("to_request")) == turn and l.get("finish_reason") == "length"]
    if answer is None and cut:
        kind, verdict, why = "capped", "capped", f"the generation hit its output cap ({len(cut)} response(s) cut) before an answer"
    return {"ask": ask.get("text", ""), "right": g["right"], "kind": kind, "verdict": verdict, "reason": why,
            "command": (answer or {}).get("command"), "miss_fields": {k: (answer or {}).get(k) for k in check.FIELDS if answer and k in answer},
            "trial_commands": trials, "reasoning_tail": reasoning[-REASONING_TAIL:]}


def failure_md(row):
    lines = [f"## {row['task']}", "", f"**Ask:** {row['ask']}", "", f"**Command:** `{row['command']}`" if row["command"] else "**Command:** (no answer)"]
    if row.get("miss_fields"):
        lines.append(f"**Miss fields:** {json.dumps(row['miss_fields'])}")
    lines += [f"**Kind:** {row['kind']}" + (f". **Reason:** {row['reason']}" if row["reason"] else "")]
    if row["trial_commands"]:
        lines += ["", "**Trial searches before the answer:**"] + [f"- `{c}`" for c in row["trial_commands"]]
    if row["reasoning_tail"]:
        lines += ["", f"**Reasoning before the answer (last {REASONING_TAIL} characters):**", "", "```", row["reasoning_tail"], "```"]
    return "\n".join(lines) + "\n"


def one(task, args):
    out = os.path.join(args["out"], task)
    if args["before"]:
        loop.subprocess.run(["bash", "-c", args["before"]], check=False)
    started = time.time()
    code, stopped = loop.run(loop.pass_cmd(task, out, args["notes"], args, True), args["max_seconds"])
    row = {"task": task, "held_out": args["held_out"], "pass": out, "exit": code, "stopped": stopped, "wall_seconds": round(time.time() - started)}
    p, log = os.path.join(out, "pass.json"), os.path.join(out, "run", "session.log")
    truth = os.path.join(out, "run", "services", "fixture", "truth.json")
    if stopped or not (os.path.exists(p) and os.path.exists(log) and os.path.exists(truth)):
        return dict(row, right=False, kind="stopped" if stopped else "no pass")
    t = json.load(open(p))["tasks"].get(task, {})
    lines = [json.loads(l) for l in open(log) if l.strip()]
    row.update(details(lines, task, json.load(open(truth))))
    row.update({k: t.get(k) for k in PAR}, trial_searches=t.get("trial_searches"), docs_reads=t.get("docs_reads", []),
               clean=bool(json.load(open(p)).get("clean")))
    return row


def parse(argv):
    drive = argv[argv.index("--") + 1:] if "--" in argv else []
    argv = argv[:argv.index("--")] if "--" in argv else argv
    get = lambda flag, default=None: argv[argv.index(flag) + 1] if flag in argv else default
    if not get("--out") or not get("--notes"):
        sys.exit(__doc__)
    # `--held-out` is task set 2's held-out set; `--held-out-1`, set 1's (h1..h6, the gap-fill rounds' comparison).
    held_out = "--held-out" in argv or "--held-out-1" in argv
    split = HELD_OUT_1 if "--held-out-1" in argv else HELD_OUT if held_out else TUTORIAL
    ids = [t["id"] for t in split]
    tasks = get("--tasks").split(",") if get("--tasks") else ids
    stray = [t for t in tasks if t not in ids]
    if stray:
        sys.exit(f"tutor_round: not {'held-out' if held_out else 'tutorial'} tasks: {stray} (the splits never mix)")
    return {"out": os.path.abspath(get("--out")), "notes": os.path.abspath(get("--notes")), "tasks": tasks, "held_out": held_out,
            "skills": "--skills" in argv, "floor": "--floor" in argv, "writers": None, "before": get("--before"),
            "max_seconds": int(get("--max-seconds", 3600)), "drive": drive}


def main(argv):
    args = parse(argv)
    os.makedirs(args["out"], exist_ok=False)
    rows = []
    for task in args["tasks"]:
        row = one(task, args)
        rows.append(row)
        with open(os.path.join(args["out"], "rows.jsonl"), "a") as f:
            f.write(json.dumps(row) + "\n")
        print(json.dumps({k: row.get(k) for k in ("task", "right", "kind", "reason", "command")}), flush=True)
    if not args["held_out"]:  # the tutor's input: tutorial failures only, never a held-out one
        void = [r for r in rows if not r.get("ask")]  # stopped, or no pass: no data, never "no failures"
        with open(os.path.join(args["out"], "failures.md"), "w") as f:
            f.write("".join(failure_md(r) + "\n" for r in rows if not r["right"] and r.get("ask"))
                    + "".join(f"## {r['task']}\n\nNot run ({r['kind']}): no data.\n\n" for r in void)
                    or "No failures.\n")
    print(json.dumps({"done": True, "right": sum(r["right"] for r in rows), "of": len(rows)}))
    return 0


def selftest():
    bad = 0
    req = lambda seq, turn: {"kind": "request", "seq": seq, "turn": turn}
    resp = lambda seq, to, text: {"kind": "response", "seq": seq, "to_request": to, "reasoning": text}
    call = lambda seq, name, turn, **a: {"kind": "tool_call", "seq": seq, "name": name, "turn": turn, "outcome": "ran", "arguments": json.dumps(a)}
    wrong = "ast-grep run -p 'function $F($$$A) { $$$B }' src"
    lines = [{"kind": "ask", "seq": 1, "turn": 1, "task": "t1-ret-decl", "text": "the ask"}, req(2, 1), resp(3, 2, "x" * 2000 + "END"),
             call(4, "bash", 1, command="ast-grep run -p 'x' src"), call(5, "bash", 1, command="ast-grep run --help"),
             call(6, check.TOOL, 1, command=wrong), req(7, 1), resp(8, 7, "AFTER"),
             {"kind": "ask", "seq": 9, "turn": 2, "task": "t1-ret-decl", "text": "notes"}, call(10, check.TOOL, 2, command="later")]
    d = details(lines, "t1-ret-decl", {"params": check.rubric.P})
    want = {"ask": "the ask", "command": wrong, "kind": "rubric", "trial_commands": ["ast-grep run -p 'x' src"], "right": False}
    for k, w in want.items():
        if d[k] != w:
            print(f"tutor_round: {k}: {d[k]!r}, expected {w!r}"); bad += 1
    if not d["reasoning_tail"].endswith("END") or len(d["reasoning_tail"]) != REASONING_TAIL or "AFTER" in d["reasoning_tail"]:
        print(f"tutor_round: the reasoning tail is not the last {REASONING_TAIL} characters before the answer"); bad += 1
    # A turn cut at its output cap before any answer is typed "capped", never wrong (Dispatch, 2026-10-28).
    cut = [{"kind": "ask", "seq": 1, "turn": 1, "task": "t1-ret-decl", "text": "the ask"}, req(2, 1),
           dict(resp(3, 2, "unsure whether it reaches any depth"), finish_reason="length")]
    c = details(cut, "t1-ret-decl", {"params": check.rubric.P})
    if c["kind"] != "capped" or c["right"] or "unsure" not in c["reasoning_tail"] or "cap" not in c["reason"]:
        print(f"tutor_round: a capped turn: {c['kind']!r}, {c['reason']!r}"); bad += 1
    md = failure_md(dict(d, task="t1-ret-decl"))
    if "the ask" not in md or wrong not in md or "ast-grep run -p 'x' src" not in md:
        print("tutor_round: failures.md lacks the ask, command or trial"); bad += 1
    for argv, ok in [(["--out", "/x", "--notes", "/n"], True), (["--out", "/x", "--notes", "/n", "--tasks", "h1-ret-arrow"], False),
                     (["--out", "/x", "--notes", "/n", "--held-out"], True), (["--out", "/x", "--notes", "/n", "--held-out", "--tasks", "t1-ret-decl"], False),
                     (["--out", "/x", "--notes", "/n", "--held-out-1"], True)]:
        try:
            a = parse(argv)
            got = True
            if "--held-out-1" in argv and a["tasks"] != [t["id"] for t in HELD_OUT_1]:
                print("tutor_round: --held-out-1 does not default to set 1's held-out tasks"); bad += 1
            if "--held-out" in argv and a["tasks"] != [t["id"] for t in HELD_OUT]:
                print("tutor_round: --held-out does not default to set 2's held-out tasks"); bad += 1
            if "--no-notes" not in loop.pass_cmd("t1-ret-decl", "/x/p", "/n", a, True):
                print("tutor_round: a pass with a notes turn"); bad += 1
        except SystemExit:
            got = False
        if got != ok:
            print(f"tutor_round: {argv}: accepted {got}, expected {ok}"); bad += 1
    print(f"tutor_round: the task turn's details, failures.md and 5 task lists, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main(sys.argv[1:]))
