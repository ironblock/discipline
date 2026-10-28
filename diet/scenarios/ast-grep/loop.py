"""The notes loop (#971; Dispatch's note-writer experiment, 2026-10-27): python3 -I loop.py --out DIR [--writers FILE]
[--skills] [--attempts N] [--notes FILE] [--before CMD] [--max-seconds S] [-- DRIVE_ARGS...].

For each tutorial operation t1..t6 in order, a fresh instance with the current notes (`run_pass.py`). Its notes turn
is the grade's feedback, its ask chosen by the attempt's failure kind (check.py's writer map, `--writers`). A pass
that isn't clean is retried on the same operation, with the notes it left, up to N attempts (4); then the loop moves
on, carrying the notes. After t6, h1..h6 run once each with the final notes and no notes turns: the held-out check.

Per attempt it records, in DIR/loop.jsonl: the operation, attempt, pass dir, clean, failure kind, writer, the notes'
character lengths before and after, and pass.json's path. `--before CMD` runs before every pass (the host's idle
gate); `--max-seconds S` (3600) stops a pass's whole process group and records it `stopped`, never graded. DRAFT and
experimental: the writers' text is the experiment's, never the maintainer's wording.
"""
import json, os, signal, subprocess, sys, time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from tasks import HELD_OUT, TUTORIAL  # noqa: E402


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


def one(task, attempt, notes, args, held_out):
    out = os.path.join(args["out"], ("held-out" if held_out else "tutorial"), f"{task}-{attempt}")
    if args["before"]:
        subprocess.run(["bash", "-c", args["before"]], check=False)
    cmd = [sys.executable, "-I", os.path.join(HERE, "run_pass.py"), task, "--out", out, "--notes", notes]
    cmd += ["--skills"] if args["skills"] else []
    cmd += ["--no-notes"] if held_out else (["--writers", args["writers"]] if args["writers"] else [])
    cmd += ["--"] + args["drive"] if args["drive"] else []
    started = time.time()
    code, stopped = run(cmd, args["max_seconds"])
    p = os.path.join(out, "pass.json")
    v = json.load(open(p)) if os.path.exists(p) and not stopped else {}
    t = v.get("tasks", {}).get(task, {})
    after = os.path.join(out, "notes.md")
    row = {"task": task, "attempt": attempt, "held_out": held_out, "pass": out, "exit": code, "stopped": stopped,
           "seconds": round(time.time() - started), "clean": bool(v.get("clean")) and not stopped, "failure": t.get("failure"),
           "writer": t.get("writer"), "notes_before_chars": len(open(notes).read().strip()),
           "notes_after_chars": len(open(after).read().strip()) if os.path.exists(after) and not held_out else None,
           "pass_json": p if v else None}
    with open(os.path.join(args["out"], "loop.jsonl"), "a") as f:
        f.write(json.dumps(row) + "\n")
    print(json.dumps(row), flush=True)
    return row, (after if os.path.exists(after) and not held_out and not stopped else notes)


def parse(argv):
    drive = argv[argv.index("--") + 1:] if "--" in argv else []
    argv = argv[:argv.index("--")] if "--" in argv else argv
    get = lambda flag, default=None: argv[argv.index(flag) + 1] if flag in argv else default
    if not get("--out"):
        sys.exit(__doc__)
    return {"out": os.path.abspath(get("--out")), "writers": get("--writers") and os.path.abspath(get("--writers")),
            "skills": "--skills" in argv, "attempts": int(get("--attempts", 4)), "notes": get("--notes"),
            "before": get("--before"), "max_seconds": int(get("--max-seconds", 3600)), "drive": drive}


def main(argv):
    args = parse(argv)
    os.makedirs(args["out"], exist_ok=False)
    notes = os.path.join(args["out"], "notes-0.md")
    with open(notes, "w") as f:
        f.write(open(args["notes"]).read() if args["notes"] else "")
    for task in TUTORIAL:
        for attempt in range(1, args["attempts"] + 1):
            row, notes = one(task["id"], attempt, notes, args, False)
            if row["clean"]:
                break
    for task in HELD_OUT:
        one(task["id"], 1, notes, args, True)
    print(json.dumps({"done": True, "final_notes": notes}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
