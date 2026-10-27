"""The ast-grep tutorial's check (#971): python3 -I check.py, as a scenario's `grade` step runs it.

The answer is a command, and it is never run (the maintainer, 2026-10-27). The check takes the turn's last
well-formed `submit_answer` call (`answer-tool.json`) and judges it statically (`rubric.py`) against the forms known
to be right for the task's operation, with the instance's names (`$DIET_SERVICE_FIXTURE_TRUTH`'s params). It is
right only when the rubric says right; `unrecognized` and a refusal are wrong, and the reason says which. An answer
in prose, or through another tool, is no answer. Prints one line, `{"right", "expected", "got"}`. `--selftest`
grades known answers. The confined execution (`audit.py`) is an offline audit of this verdict, never the verdict.
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import rubric  # noqa: E402

TOOL = json.load(open(os.path.join(HERE, "answer-tool.json")))["name"]


def family_of(task):
    from tasks import TASKS
    return next(t["family"] for t in TASKS if t["id"] == task)


def answered(turn):
    """The command of the turn's last well-formed answer call, or None."""
    got = None
    for call in turn.get("calls", []):
        if call.get("name") != TOOL or call.get("outcome") == "refused":
            continue
        try:
            args = json.loads(call.get("arguments") or "")
        except ValueError:
            continue
        if isinstance(args, dict) and isinstance(args.get("command"), str):
            got = args["command"]
    return got


def grade(turn, task, truth):
    cmd = answered(turn)
    expected = "a command the rubric knows as right"
    if cmd is None:
        return {"right": False, "expected": expected, "got": "(no answer through the answer tool)"}
    verdict, why = rubric.judge(cmd, family_of(task), truth["params"])
    return {"right": verdict == "right", "expected": expected, "got": f"{verdict}: {why}"}


def main():
    turn = json.load(open(os.environ["DIET_GRADE_TURN"]))
    truth = json.load(open(os.environ["DIET_SERVICE_FIXTURE_TRUTH"]))
    print(json.dumps(grade(turn, os.environ["DIET_GRADE_TASK"], truth)))


def selftest():
    truth = {"params": rubric.P}
    call = lambda cmd, outcome="ran", name=TOOL: {"name": name, "outcome": outcome, "arguments": json.dumps({"command": cmd})}
    turn = lambda *calls, answer="": {"calls": list(calls), "answer": answer}
    right = "ast-grep run -l ts -p 'function $F($$$A): Relay { $$$B }' src"
    cases = [
        ("the right command", turn(call(right)), True),
        ("no call", turn(answer=right), False),
        ("through bash", turn(call(right, name="bash")), False),
        ("a refused call", turn(call(right, "refused")), False),
        ("the last call decides", turn(call(right), call("ast-grep run -p 'function $F($$$A) { $$$B }' src")), False),
        ("a corrected last call", turn(call("ast-grep run -p 'x' src"), call(right)), True),
        ("not ast-grep", turn(call("rg -n 'function' src")), False),
        ("a shell operator", turn(call(right + " | head")), False),
    ]
    bad = 0
    for why, t, want in cases:
        got = grade(t, "t1-ret-decl", truth)
        if got["right"] != want:
            print(f"check: {why}: graded {got}, expected {want}"); bad += 1
    print(f"check: {sum(c[2] for c in cases)} right and {sum(not c[2] for c in cases)} wrong answers, {bad} misgraded")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main())
