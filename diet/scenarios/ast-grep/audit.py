"""The execution audit of the static grader (#971; the maintainer, 2026-10-27): python3 -I audit.py --execute PASS_DIR...

Offline, after the passes, out of the model's sight, and never the verdict. For each pass directory (`run_pass.py`'s
DIR, its log at `run/session.log` and its instance at `run/services/fixture/`), it takes the last well-formed answer
command, judges it statically (`rubric.py`, the verdict), runs it confined against the instance's pristine copy
(`command.py`, macOS only) and compares its matched lines with the truth. It prints one JSON line per pass, and
`disagree` when the two verdicts differ, which is where the rubric should be looked at.

ast-grep only, because its searches change nothing: it is not a general mechanism, and a tool with destructive
actions gets no execution audit. It runs only with `--execute`.
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import command, rubric  # noqa: E402
from tasks import TASKS  # noqa: E402

# The answer tool's name now, and the one the first live passes used (2026-10-27's first live run, re-graded here).
ANSWER_NAMES = {json.load(open(os.path.join(HERE, "answer-tool.json")))["name"], "answer"}


def last_answer(log):
    cmd = None
    for line in log:
        if line.get("kind") == "tool_call" and line.get("name") in ANSWER_NAMES and line.get("outcome") != "refused":
            try:
                args = json.loads(line.get("arguments") or "")
            except ValueError:
                continue
            if isinstance(args, dict) and isinstance(args.get("command"), str):
                cmd = args["command"]
    return cmd


def audit(pass_dir):
    log = [json.loads(l) for l in open(os.path.join(pass_dir, "run", "session.log")) if l.strip()]
    task = next(l["task"] for l in log if l.get("kind") == "ask" and l.get("task"))
    family = next(t["family"] for t in TASKS if t["id"] == task)
    fixture = os.path.join(pass_dir, "run", "services", "fixture")
    truth = json.load(open(os.path.join(fixture, "truth.json")))
    cmd = last_answer(log)
    row = {"pass": os.path.relpath(pass_dir), "task": task, "command": cmd}
    if cmd is None:
        return dict(row, static="no answer", executed=None, disagree=False)
    verdict, why = rubric.judge(cmd, family, truth["params"])
    try:
        found = command.matches(command.checked(cmd), os.path.join(fixture, "pristine"))
        executed = "right" if found == sorted(truth["answers"][family]) else "wrong"
    except command.Refused as e:
        executed = f"refused: {e}"
    return dict(row, static=verdict, why=why, executed=executed, disagree=(verdict == "right") != (executed == "right"))


if __name__ == "__main__":
    if sys.argv[1:2] != ["--execute"] or len(sys.argv) < 3:
        sys.exit(__doc__)
    if sys.platform != "darwin":
        sys.exit("audit: the confined execution is built for macOS only")
    rows = [audit(d) for d in sys.argv[2:]]
    for r in rows:
        print(json.dumps(r))
    print(json.dumps({"passes": len(rows), "disagree": sum(r["disagree"] for r in rows)}))
