"""The ast-grep tutorial's check (#971): python3 -I check.py, as a scenario's `grade` step runs it.

The answer is never run (the maintainer, 2026-10-27). The check takes the turn's last well-formed `submit_answer`
call and judges its command statically (`rubric.py`) against the forms known to be right, with the instance's names
(`$DIET_SERVICE_FIXTURE_TRUTH`'s params). On top of that:
- **No trial searches** (ruled 2026-10-27): help and docs may be read, but an ast-grep search before the answer,
  against the project or a file the model made, fails the attempt. `$DIET_AST_GREP_TRIAL=par` records it without
  failing; the default is `fail`.
- **The miss** (t2, h2): their answer tool (`answer-tool-miss.json`) also says what the command does when nothing
  matches, `exit_status_when_none` and `prints_nothing_when_none`. Right only when the command passes the rubric and
  both fields match `miss_truth.json` for the command's subcommand, measured per ast-grep version; a version not
  measured there is unrecognized.
Right only when all of it holds; `unrecognized` and a refusal are wrong.

**The notes turn rides on the grade** (#971's notes loop, Dispatch 2026-10-27): the check also names the attempt's
failure kind (`right`, `rubric`, `miss fields`, `trial`, `unrecognized`, `no answer`) as `expected`, and prints as
`got` the notes ask for it, which the scenario's grade step sends as its feedback (`right` and `last` are `{got}`).
The asks are a writer table, `$DIET_AST_GREP_WRITERS` (JSON: `tail`, and `W0`, `W0n`, `W1`, `W2`, `W3`, `W3n`, `W4`), experiment
text kept out of this tree; without it, every kind gets the DRAFT stub. The map: rubric → W1, miss fields (the command was
right, its miss fields weren't) → W4, trial (the command was right) → W3, unrecognized with trial searches → W3n
(the searches named, right or wrong never said), right with a docs read → W2, right and clean → W0, unrecognized or no answer → W0n, a
neutral ask that never says the command was right or wrong. An answer in prose,
or through another tool, is no answer. Prints one line, `{"right", "expected", "got"}`. `--selftest` grades known
answers. The confined execution (`audit.py`) is an offline audit, never the verdict.
"""
import importlib, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import command, rubric  # noqa: E402
from docs_reads import installed_prefix, reads_of  # noqa: E402

TOOL = json.load(open(os.path.join(HERE, "answer-tool.json")))["name"]
# The DRAFT notes ask every kind gets when no writer table is given.
STUB = "DRAFT. Notes update: update your notes on ast-grep through the notes tool, as concise as you can make them."
WRITER_OF = {"rubric": "W1", "miss fields": "W4", "trial": "W3", "unrecognized": "W0n", "no answer": "W0n"}
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


def docs_read(calls):
    """Whether any call before the answer read ast-grep's docs (docs_reads.py), the skills under ./.agent-skills
    counted when the condition put them there."""
    skills = os.path.abspath(".agent-skills") if os.path.isdir(".agent-skills") else None
    cwd, prefix = os.getcwd(), installed_prefix()
    for call in calls:
        if call.get("name") != "bash":
            continue
        try:
            cmd = json.loads(call.get("arguments") or "{}").get("command", "")
        except ValueError:
            continue
        if reads_of(cmd, skills, cwd=cwd, home=os.path.expanduser("~"), installed=prefix):
            return True
    return False


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


def notes_ask(kind, read, writers, trials=0):
    """The writer and the ask for KIND: W2 for a right attempt that read the docs; W3n for an unrecognized attempt that
    ran trial searches (a wrong one stays W1, whatever its searches)."""
    writer = "W3n" if kind == "unrecognized" and trials else WRITER_OF.get(kind) or ("W2" if read else "W0")
    if writers is None:
        return writer, STUB
    return writer, (writers[writer].rstrip() + " " + writers["tail"].strip()).strip()


def installed_version():
    try:
        return subprocess.run(["ast-grep", "--version"], capture_output=True, text=True, timeout=10).stdout.strip()
    except (OSError, subprocess.TimeoutExpired):
        return None


def grade(turn, task, truth, trial="fail", version=None):
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


def main():
    turn = json.load(open(os.environ["DIET_GRADE_TURN"]))
    truth = json.load(open(os.environ["DIET_SERVICE_FIXTURE_TRUTH"]))
    trial = os.environ.get("DIET_AST_GREP_TRIAL", "fail")
    if trial not in ("fail", "par"):
        sys.exit("check: DIET_AST_GREP_TRIAL is fail or par")
    g = grade(turn, os.environ["DIET_GRADE_TASK"], truth, trial)
    path = os.environ.get("DIET_AST_GREP_WRITERS")
    writers = json.load(open(path)) if path else None
    writer, ask = notes_ask(g["kind"], g["kind"] == "right" and docs_read(g["before"]), writers, g["trials"])
    # What the model is sent is only the notes ask; the verdict and its reason stay in `expected` and the log.
    print(json.dumps({"right": g["right"], "expected": f"{g['kind']} ({writer}): {g['got']}", "got": ask}))


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
        got = grade(t, task, truth, version=v)["kind"]
        if got != want:
            print(f"check: kind {want}: got {got}"); bad += 1
    table = {"tail": "TAIL.", "W0": "w0.", "W0n": "w0n.", "W1": "w1.", "W2": "w2.", "W3": "w3.", "W3n": "w3n.", "W4": "w4."}
    for kind, read, trials, writer in [("right", False, 0, "W0"), ("right", True, 0, "W2"), ("rubric", True, 0, "W1"), ("rubric", False, 5, "W1"),
                                       ("miss fields", False, 0, "W4"), ("trial", True, 3, "W3"), ("unrecognized", False, 0, "W0n"),
                                       ("unrecognized", True, 2, "W3n"), ("no answer", True, 0, "W0n")]:
        w, ask = notes_ask(kind, read, table, trials)
        if w != writer or ask != table[writer] + " TAIL." or notes_ask(kind, read, None, trials) != (writer, STUB):
            print(f"check: writer for {kind} (docs {read}): {w} {ask!r}"); bad += 1
    unmeasured = grade(turn(call(zero, exit_status_when_none=1, prints_nothing_when_none=True)), "t2-zero-calls", truth, version="ast-grep 9.9.9")
    if not unmeasured["got"].startswith("unrecognized"):
        print(f"check: an unmeasured version graded {unmeasured}"); bad += 1
    print(f"check: {sum(c[4] for c in cases)} right and {sum(not c[4] for c in cases) + 1} not right, 6 kinds and 9 writer choices, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main())
