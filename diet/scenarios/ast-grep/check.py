"""The ast-grep tutorial's check (#971): python3 -I check.py, as a scenario's `grade` step runs it.

Grades the task's answer, never an exit status: the last well-formed call to the answer tool (`answer-tool.json`)
in the turn naming the task, against the instance's truth from `gen.py` (`$DIET_SERVICE_FIXTURE_TRUTH`). An answer
in prose, with no call, is no answer. Prints one line, `{"right", "expected", "got"}`. `--selftest` grades known
turns.
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
TOOL = json.load(open(os.path.join(HERE, "answer-tool.json")))["name"]


def family_of(task):
    sys.path.insert(0, HERE)
    from tasks import TASKS
    return next(t["family"] for t in TASKS if t["id"] == task)


def norm(item):
    item = item.strip().strip("`\"'").strip()
    return item[:-2] if item.endswith("()") else item


def answered(turn, task):
    """The answer's items from the turn's last well-formed answer call for TASK, or None."""
    got = None
    for call in turn.get("calls", []):
        if call.get("name") != TOOL or call.get("outcome") == "refused":
            continue
        try:
            args = json.loads(call.get("arguments") or "")
        except ValueError:
            continue
        if isinstance(args, dict) and args.get("task") == task and isinstance(args.get("answer"), list):
            got = [str(x) for x in args["answer"]]
    return got


def grade(turn, task, truth):
    want = truth["answers"][family_of(task)]
    kind, items = want["kind"], want["items"]
    expected = ", ".join(items)
    got = answered(turn, task)
    if got is None:
        return {"right": False, "expected": expected, "got": "(no answer through the answer tool)"}
    shown = ", ".join(got)
    g = [norm(x) for x in got if norm(x)]
    if kind == "set":
        right = sorted(set(g)) == sorted(items) and len(g) == len(set(g))
    elif kind == "count":
        right = len(g) == 1 and g[0].isdigit() and int(g[0]) == int(items[0])
    elif kind == "word":
        right = len(g) == 1 and g[0].lower().rstrip(".") == items[0]
    else:  # seq: a path, then a 1-based line
        right = (len(g) == 2 and g[0].removeprefix("./") == items[0] and g[1].isdigit()
                 and int(g[1]) == int(items[1]))
    return {"right": right, "expected": expected, "got": shown}


def main():
    turn = json.load(open(os.environ["DIET_GRADE_TURN"]))
    truth = json.load(open(os.environ["DIET_SERVICE_FIXTURE_TRUTH"]))
    print(json.dumps(grade(turn, os.environ["DIET_GRADE_TASK"], truth)))


def selftest():
    truth = {"answers": {"ret-decl": {"kind": "set", "items": ["aFn", "bFn"]},
                         "zero-calls": {"kind": "count", "items": ["0"]},
                         "maybe-defined": {"kind": "word", "items": ["no"]},
                         "fn-line": {"kind": "seq", "items": ["src/a/b.ts", "7"]}}}
    call = lambda task, answer, outcome="ok", name=TOOL: {"name": name, "outcome": outcome,
                                                         "arguments": json.dumps({"task": task, "answer": answer})}
    turn = lambda *calls, answer="": {"calls": list(calls), "answer": answer}
    cases = [
        ("the right set, in any order", "t1-ret-decl", turn(call("t1-ret-decl", ["bFn", "aFn"])), True),
        ("names in backticks, with parentheses", "t1-ret-decl", turn(call("t1-ret-decl", ["`aFn`", "bFn()"])), True),
        ("one name missing", "t1-ret-decl", turn(call("t1-ret-decl", ["aFn"])), False),
        ("one name extra", "t1-ret-decl", turn(call("t1-ret-decl", ["aFn", "bFn", "cFn"])), False),
        ("a name twice", "t1-ret-decl", turn(call("t1-ret-decl", ["aFn", "bFn", "aFn"])), False),
        ("the right answer in prose, with no call", "t1-ret-decl", turn(answer="aFn, bFn"), False),
        ("an answer for another task", "t1-ret-decl", turn(call("t2-zero-calls", ["aFn", "bFn"])), False),
        ("a refused call is no answer", "t1-ret-decl", turn(call("t1-ret-decl", ["aFn", "bFn"], "refused")), False),
        ("the last call decides", "t1-ret-decl", turn(call("t1-ret-decl", ["aFn", "bFn"]), call("t1-ret-decl", ["aFn"])), False),
        ("a corrected last call", "t1-ret-decl", turn(call("t1-ret-decl", ["aFn"]), call("t1-ret-decl", ["aFn", "bFn"])), True),
        ("a bash call is no answer", "t1-ret-decl", turn(call("t1-ret-decl", ["aFn", "bFn"], name="bash")), False),
        ("a zero count", "t2-zero-calls", turn(call("t2-zero-calls", ["0"])), True),
        ("a count in words", "t2-zero-calls", turn(call("t2-zero-calls", ["zero"])), False),
        ("a nonzero count from a text search", "t2-zero-calls", turn(call("t2-zero-calls", ["2"])), False),
        ("no, any case", "h2-maybe-defined", turn(call("h2-maybe-defined", ["No."])), True),
        ("yes when it is not", "h2-maybe-defined", turn(call("h2-maybe-defined", ["yes"])), False),
        ("a path and a 1-based line", "t6-fn-line", turn(call("t6-fn-line", ["./src/a/b.ts", "7"])), True),
        ("json's 0-based line", "t6-fn-line", turn(call("t6-fn-line", ["src/a/b.ts", "6"])), False),
        ("the line alone", "t6-fn-line", turn(call("t6-fn-line", ["7"])), False),
    ]
    bad = 0
    for why, task, t, want in cases:
        got = grade(t, task, truth)["right"]
        if got != want:
            print(f"check: {why}: graded {got}, expected {want}")
            bad += 1
    right = sum(1 for c in cases if c[3])
    print(f"check: {right} right and {len(cases) - right} wrong answers, {bad} misgraded")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main())
