"""The ast-grep tutorial's check (#971): python3 -I check.py, as a scenario's `grade` step runs it.

Execution is the verdict (PREREG.md, 2026-10-29). The check takes the turn's last well-formed `submit_answer` call
and runs its command, confined (`command.py`, macOS only), on the pass's pristine copy of the instance
(`$DIET_SERVICE_FIXTURE_TRUTH`'s directory). It is right only when the matched lines are exactly the truth's, and on a
miss task (t2, h2) only when its answer fields, `exit_status_when_none` and `prints_nothing_when_none`, also match
`miss_truth.json` for the command's subcommand, measured per ast-grep version; a version not measured there is not
right. A command `command.py` refuses is not right. An answer in prose, or through another tool, is no answer.
`$DIET_AST_GREP_VERDICT`, when set, must be `execute`: there is no other verdict. Prints one line,
`{"right", "expected", "got"}`; `got` is empty, as no feedback is sent. `--selftest` grades known answers with the
execution stubbed, so it needs no ast-grep.
"""
import importlib, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import command  # noqa: E402

TOOL = json.load(open(os.path.join(HERE, "answer-tool-native.json")))["name"]
MISS = ("zero-calls", "maybe-defined")
FIELDS = ("exit_status_when_none", "prints_nothing_when_none")


def family_of(task):
    from tasks import TASKS
    return next(t["family"] for t in TASKS if t["id"] == task)


def answered(turn):
    """(the turn's last well-formed answer call's arguments, the calls before it), or (None, every call)."""
    got, before = None, []
    for i, call in enumerate(turn.get("calls", [])):
        if call.get("name") != TOOL or call.get("outcome") == "refused":
            continue
        try:
            args = json.loads(call.get("arguments") or "")
        except ValueError:
            continue
        if isinstance(args, dict) and isinstance(args.get("command"), str):
            got, before = args, turn["calls"][:i]
    return got, (before if got is not None else turn.get("calls", []))


def searches_before(calls):
    """The ast-grep searches the calls before the answer ran: help and docs reads aren't searches."""
    tools_of = importlib.import_module("pass").tools_of  # `pass` is a keyword, so by name
    n = 0
    for call in calls:
        if call.get("name") != "bash":
            continue
        try:
            cmd = json.loads(call.get("arguments") or "{}").get("command", "")
        except ValueError:
            continue
        n += tools_of(cmd).count("ast-grep")
    return n


def installed_version():
    try:
        return subprocess.run(["ast-grep", "--version"], capture_output=True, text=True, timeout=10).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return None


def grade_executed(turn, task, truth, pristine, version=None, matches=command.matches):
    """The answer RUN, confined, on PRISTINE: right only when its matches are exactly the truth's lines, and on a miss
    task when its miss fields match `miss_truth.json` too. MATCHES runs a checked argv on a root (the selftest's stub)."""
    args, before = answered(turn)
    if args is None:
        return {"right": False, "executed": "no answer", "searches": searches_before(before)}
    family = family_of(task)
    try:
        found = matches(command.checked(args["command"]), pristine)
        executed = "right" if found == sorted(truth["answers"][family]) else "wrong"
    except command.Refused as e:
        executed = f"refused: {e}"
    right = executed == "right"
    if right and family in MISS:
        measured = json.load(open(os.path.join(HERE, "miss_truth.json"))).get(version or installed_version() or "")
        sub = command.parse(args["command"])["sub"]
        right = measured is not None and sub in measured and all(args.get(k) == measured[sub][k] for k in FIELDS)
        if not right:
            executed = "right, miss fields wrong"
    return {"right": right, "executed": executed, "searches": searches_before(before)}


def verdict_refused(env):
    """Why ENV's `DIET_AST_GREP_VERDICT` is refused, or None: when set, it is `execute`, the only verdict."""
    mode = env.get("DIET_AST_GREP_VERDICT")
    if mode is not None and mode != "execute":
        return f"check: DIET_AST_GREP_VERDICT is {mode!r}, but execution is the only verdict: set it to execute, or unset"
    return None


def main():
    why = verdict_refused(os.environ)
    if why:
        sys.exit(why)
    turn = json.load(open(os.environ["DIET_GRADE_TURN"]))
    truth_path = os.environ["DIET_SERVICE_FIXTURE_TRUTH"]
    g = grade_executed(turn, os.environ["DIET_GRADE_TASK"], json.load(open(truth_path)), os.path.join(os.path.dirname(truth_path), "pristine"))
    print(json.dumps({"right": g["right"], "expected": f"executed {g['executed']}", "got": ""}))


def selftest():
    call = lambda cmd, outcome="ran", name=TOOL, **fields: {"name": name, "outcome": outcome, "arguments": json.dumps(dict(command=cmd, **fields))}
    bash = lambda cmd: {"name": "bash", "outcome": "ran", "arguments": json.dumps({"command": cmd})}
    turn = lambda *calls: {"calls": list(calls)}
    right, wrong, part = "ast-grep run -p 'right($$$A)' src", "ast-grep run -p 'wrong($$$A)' src", "ast-grep run -p 'part($$$A)' src"
    outline = "ast-grep outline src --match '^Maybe$'"
    truth = {"answers": {"ret-decl": ["src/a.ts:1", "src/b.ts:2"], "zero-calls": [], "maybe-defined": []}}
    # The execution stubbed: each command's matches, by its checked argv.
    found = {right: ["src/a.ts:1", "src/b.ts:2"], wrong: ["src/a.ts:1", "src/b.ts:2", "src/c.ts:3"], part: ["src/a.ts:1"],
             "ast-grep run -p 'probe($$$A)' src": [], outline: []}
    stub = lambda argv, root: found[next(c for c in found if command.checked(c) == argv)]
    v = "ast-grep 0.44.0"
    cases = [
        ("the right matches", "t1-ret-decl", turn(call(right)), True),
        ("one match too many", "t1-ret-decl", turn(call(wrong)), False),
        ("one match too few", "t1-ret-decl", turn(call(part)), False),
        ("no call", "t1-ret-decl", turn(), False),
        ("through bash", "t1-ret-decl", turn(call(right, name="bash")), False),
        ("a refused call", "t1-ret-decl", turn(call(right, "refused")), False),
        ("the last call decides", "t1-ret-decl", turn(call(right), call(wrong)), False),
        ("the last call decides, the other way", "t1-ret-decl", turn(call(wrong), call(right)), True),
        ("a command command.py refuses", "t1-ret-decl", turn(call("ast-grep run -p 'right($$$A)' -U src")), False),
        ("not ast-grep", "t1-ret-decl", turn(call("rg -n right src")), False),
        ("the miss: run, exit 1, prints nothing", "t2-zero-calls", turn(call("ast-grep run -p 'probe($$$A)' src", exit_status_when_none=1, prints_nothing_when_none=True)), True),
        ("the miss: run, wrong exit", "t2-zero-calls", turn(call("ast-grep run -p 'probe($$$A)' src", exit_status_when_none=0, prints_nothing_when_none=True)), False),
        ("the miss: run, wrong print", "t2-zero-calls", turn(call("ast-grep run -p 'probe($$$A)' src", exit_status_when_none=1, prints_nothing_when_none=False)), False),
        ("the miss: no fields", "t2-zero-calls", turn(call("ast-grep run -p 'probe($$$A)' src")), False),
        ("the miss: outline, exit 0, prints `nothing found`", "h2-maybe-defined", turn(call(outline, exit_status_when_none=0, prints_nothing_when_none=False)), True),
        ("the miss: outline answered as run's", "h2-maybe-defined", turn(call(outline, exit_status_when_none=1, prints_nothing_when_none=True)), False),
    ]
    bad = 0
    for why, task, t, want in cases:
        got = grade_executed(t, task, truth, "/pristine", version=v, matches=stub)
        if got["right"] != want:
            print(f"check: {why}: graded {got}, expected {want}"); bad += 1
    miss = turn(call("ast-grep run -p 'probe($$$A)' src", exit_status_when_none=1, prints_nothing_when_none=True))
    if grade_executed(miss, "t2-zero-calls", truth, "/pristine", version="ast-grep 9.9.9", matches=stub)["right"]:
        print("check: an unmeasured version's miss graded right"); bad += 1
    searched = turn(bash("ast-grep run --help"), bash("ast-grep run -p x src"), bash("sg outline src"), call(right), bash("ast-grep run -p y src"))
    if grade_executed(searched, "t1-ret-decl", truth, "/pristine", version=v, matches=stub)["searches"] != 2:
        print("check: the searches before the answer, help apart, are not 2"); bad += 1
    for env, refused in (({}, False), ({"DIET_AST_GREP_VERDICT": "execute"}, False), ({"DIET_AST_GREP_VERDICT": "static"}, True),
                         ({"DIET_AST_GREP_VERDICT": ""}, True)):
        if bool(verdict_refused(env)) != refused:
            print(f"check: verdict {env}: refused {not refused}, expected {refused}"); bad += 1
    print(f"check: {sum(c[3] for c in cases)} right and {sum(not c[3] for c in cases) + 1} not right, a search count, 4 verdict settings, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main())
