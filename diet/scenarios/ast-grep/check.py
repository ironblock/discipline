"""The ast-grep tutorial's check (#971): python3 -I check.py, as a scenario's `grade` step runs it.

The answer is never run (the maintainer, 2026-10-27). The check takes the turn's last well-formed `submit_answer`
call and judges its command statically (`rubric.py`) against the forms known to be right, with the instance's names
(`$DIET_SERVICE_FIXTURE_TRUTH`'s params). On top of that:
- **Trial searches are par cost, never a fail** (the maintainer, 2026-10-29): the shim (`shim.py`) refuses every
  ast-grep search in the sandbox, so a search before the answer is a refused attempt, counted (`trials`) and never
  graded; the verdict is the submitted command alone. `$DIET_AST_GREP_TRIAL=fail` restores the 2026-10-27 rule, under
  which one fails the attempt.
- **The miss** (t2, h2): their answer tool (`answer-tool-miss.json`) also says what the command does when nothing
  matches, `exit_status_when_none` and `prints_nothing_when_none`. Right only when the command passes the rubric and
  both fields match `miss_truth.json` for the command's subcommand, measured per ast-grep version; a version not
  measured there is unrecognized.
Right only when all of it holds; `unrecognized` and a refusal are wrong. The check names the attempt's failure kind
(`right`, `rubric`, `miss fields`, `trial`, `unrecognized`, `no answer`) in `expected`. An answer in prose, or
through another tool, is no answer. Prints one line, `{"right", "expected", "got"}`; `got` is empty, as no feedback
is sent. `--selftest` grades known answers. The confined execution (`audit.py`) is an offline audit, never the verdict.
"""
import importlib, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import command, rubric  # noqa: E402

TOOL = json.load(open(os.path.join(HERE, "answer-tool.json")))["name"]
MISS = ("zero-calls", "maybe-defined", "zero-new", "itf-maybe")
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


def trial_searches(calls):
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


def kind_of(verdict, why):
    if verdict == "right":
        return "right"
    if verdict == "unrecognized":
        return "unrecognized"
    if why.startswith("(no answer"):
        return "no answer"
    if "trial search" in why:
        return "trial"
    if "on a miss" in why:
        return "miss fields"
    return "rubric"


def installed_version():
    try:
        return subprocess.run(["ast-grep", "--version"], capture_output=True, text=True, timeout=10).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return None


def grade(turn, task, truth, trial="par", version=None):
    expected = "a command the rubric knows as right"
    args, before = answered(turn)
    if args is None:
        return {"right": False, "expected": expected, "got": "(no answer through the answer tool)", "kind": "no answer", "before": before,
                "trials": trial_searches(before)}
    family = family_of(task)
    verdict, why = rubric.judge(args["command"], family, truth["params"])
    trials = trial_searches(before)
    if verdict == "right" and trials and trial == "fail":
        verdict, why = "wrong", f"{trials} trial search(es) before the answer: a demonstration runs no ast-grep search"
    if verdict == "right" and family in MISS:
        expected = "a right command, and what it does when nothing matches"
        measured = json.load(open(os.path.join(HERE, "miss_truth.json"))).get(version or installed_version() or "")
        sub = command.parse(args["command"])["sub"]
        if measured is None or sub not in measured:
            verdict, why = "unrecognized", f"no measured miss behaviour for {version or 'this ast-grep'}'s `{sub}`"
        else:
            off = [k for k in FIELDS if args.get(k) != measured[sub][k]]
            if off:
                verdict, why = "wrong", f"the command is right, but {', '.join(off)} is not what `{sub}` does on a miss"
    return {"right": verdict == "right", "expected": expected, "got": f"{verdict}: {why}", "kind": kind_of(verdict, why), "before": before,
            "trials": trials}


def grade_executed(turn, task, truth, pristine, version=None):
    """The falsification's verdict (PREREG.md, 2026-10-29): the answer RUN, confined, on the pass's pristine fixture,
    right only when its matches are exactly the truth's lines, and on a miss task when its miss fields match
    `miss_truth.json` too. The rubric's verdict is a descriptor (`static`), never the verdict. Macos only (command.py)."""
    args, before = answered(turn)
    if args is None:
        return {"right": False, "executed": "no answer", "static": "no answer", "why": "", "trials": trial_searches(before)}
    family = family_of(task)
    static, why = rubric.judge(args["command"], family, truth["params"])
    try:
        found = command.matches(command.checked(args["command"]), pristine)
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
    return {"right": right, "executed": executed, "static": static, "why": why, "trials": trial_searches(before)}


def main():
    turn = json.load(open(os.environ["DIET_GRADE_TURN"]))
    if os.environ.get("DIET_AST_GREP_VERDICT") == "execute":  # the falsification's (PREREG.md): execution, not the rubric
        truth_path = os.environ["DIET_SERVICE_FIXTURE_TRUTH"]
        g = grade_executed(turn, os.environ["DIET_GRADE_TASK"], json.load(open(truth_path)), os.path.join(os.path.dirname(truth_path), "pristine"))
        print(json.dumps({"right": g["right"], "expected": f"executed {g['executed']}; static {g['static']}: {g['why']}", "got": ""}))
        return
    truth = json.load(open(os.environ["DIET_SERVICE_FIXTURE_TRUTH"]))
    trial = os.environ.get("DIET_AST_GREP_TRIAL", "par")
    if trial not in ("fail", "par"):
        sys.exit("check: DIET_AST_GREP_TRIAL is fail or par")
    g = grade(turn, os.environ["DIET_GRADE_TASK"], truth, trial)
    print(json.dumps({"right": g["right"], "expected": f"{g['kind']}: {g['got']}", "got": ""}))


def selftest():
    truth = {"params": rubric.P}
    call = lambda cmd, outcome="ran", name=TOOL, **fields: {"name": name, "outcome": outcome, "arguments": json.dumps(dict(command=cmd, **fields))}
    bash = lambda cmd: {"name": "bash", "outcome": "ran", "arguments": json.dumps({"command": cmd})}
    turn = lambda *calls, answer="": {"calls": list(calls), "answer": answer}
    right = "ast-grep run -l ts -p 'function $F($$$A): Relay { $$$B }' src"
    zero = "ast-grep run -p 'probeMeadow($$$A)' src"
    maybe = "ast-grep outline src --match '^VertexSpec$'"
    v = "ast-grep 0.44.0"
    RULE_UNKNOWN = "ast-grep scan --inline-rules 'id: q\nlanguage: ts\nrule:\n  any:\n    - pattern: function $F($$$A): Relay { $$$B }' src"
    cases = [
        ("the right command", "t1-ret-decl", turn(call(right)), "fail", True),
        ("no call", "t1-ret-decl", turn(answer=right), "fail", False),
        ("through bash", "t1-ret-decl", turn(call(right, name="bash")), "fail", False),
        ("a refused call", "t1-ret-decl", turn(call(right, "refused")), "fail", False),
        ("the last call decides", "t1-ret-decl", turn(call(right), call("ast-grep run -p 'function $F($$$A) { $$$B }' src")), "fail", False),
        ("not ast-grep", "t1-ret-decl", turn(call("rg -n 'function' src")), "fail", False),
        ("help read first is no trial", "t1-ret-decl", turn(bash("ast-grep run --help"), bash("man ast-grep"), call(right)), "fail", True),
        ("a trial search first fails", "t1-ret-decl", turn(bash("ast-grep run -p 'function $F($$$A): Relay { $$$B }' src"), call(right)), "fail", False),
        ("a trial outline of its own file fails", "t1-ret-decl", turn(bash("ast-grep outline scratch.ts"), call(right)), "fail", False),
        ("a trial search under par is recorded, not failed", "t1-ret-decl", turn(bash("ast-grep scan --inline-rules 'x' src"), call(right)), "par", True),
        ("by default a trial search is par cost, never a fail", "t1-ret-decl", turn(bash("ast-grep run -p x src"), call(right)), "par", True),
        ("a search after the answer is no trial", "t1-ret-decl", turn(call(right), bash("ast-grep run -p x src")), "fail", True),
        ("the miss: run, exit 1, prints nothing", "t2-zero-calls", turn(call(zero, exit_status_when_none=1, prints_nothing_when_none=True)), "fail", True),
        ("the miss: run, wrong exit", "t2-zero-calls", turn(call(zero, exit_status_when_none=0, prints_nothing_when_none=True)), "fail", False),
        ("the miss: run, wrong print", "t2-zero-calls", turn(call(zero, exit_status_when_none=1, prints_nothing_when_none=False)), "fail", False),
        ("the miss: outline, exit 0, prints `nothing found`", "h2-maybe-defined", turn(call(maybe, exit_status_when_none=0, prints_nothing_when_none=False)), "fail", True),
        ("the miss: outline answered as run's", "h2-maybe-defined", turn(call(maybe, exit_status_when_none=1, prints_nothing_when_none=True)), "fail", False),
        ("the miss: no fields", "t2-zero-calls", turn(call(zero)), "fail", False),
        ("the miss: right fields, wrong command", "t2-zero-calls", turn(call("ast-grep run -p 'probeMeadow' src", exit_status_when_none=1, prints_nothing_when_none=True)), "fail", False),
    ]
    bad = 0
    for why, task, t, trial, want in cases:
        got = grade(t, task, truth, trial, version=v)
        if got["right"] != want:
            print(f"check: {why}: graded {got}, expected {want}"); bad += 1
    kinds = [("right", "t1-ret-decl", turn(call(right))), ("no answer", "t1-ret-decl", turn()),
             ("trial", "t1-ret-decl", turn(bash("ast-grep run -p 'x' src"), call(right))),
             ("rubric", "t1-ret-decl", turn(call("ast-grep run -p 'function $F($$$A) { $$$B }' src"))),
             ("unrecognized", "t1-ret-decl", turn(call(RULE_UNKNOWN))),
             ("miss fields", "t2-zero-calls", turn(call(zero, exit_status_when_none=0, prints_nothing_when_none=True)))]
    for want, task, t in kinds:
        got = grade(t, task, truth, "fail" if want == "trial" else "par", version=v)["kind"]  # trial only under the old rule
        if got != want:
            print(f"check: kind {want}: got {got}"); bad += 1
    unmeasured = grade(turn(call(zero, exit_status_when_none=1, prints_nothing_when_none=True)), "t2-zero-calls", truth, version="ast-grep 9.9.9")
    if not unmeasured["got"].startswith("unrecognized"):
        print(f"check: an unmeasured version graded {unmeasured}"); bad += 1
    print(f"check: {sum(c[4] for c in cases)} right and {sum(not c[4] for c in cases) + 1} not right, 6 kinds, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main())
