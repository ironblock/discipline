"""The ast-grep tutorial's check (#971): python3 -I check.py, as a scenario's `grade` step runs it.

The answer is a command. The check takes the turn's last well-formed `answer` call (`answer-tool.json`), refuses
it as wrong when it may not run (`command.py`: not ast-grep, a shell operator, a forbidden flag, a path out of the
project), and otherwise runs it out of the model's sight, confined, against a pristine copy of the instance
(`$DIET_SERVICE_FIXTURE_PRISTINE`). It is right when the lines it matches are exactly the truth's
(`$DIET_SERVICE_FIXTURE_TRUTH`, from `gen.py`'s construction): equivalent spellings pass, and an answer reached any
other way fails. It never grades an exit status, and an answer in prose is no answer. It runs commands on macOS
only, where its confinement is built. Prints one line, `{"right", "expected", "got"}`; `got` names counts, never
what the command printed. `--selftest` grades known answers.
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import command  # noqa: E402

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


def grade(turn, task, truth, pristine):
    want = sorted(truth["answers"][family_of(task)])
    expected = f"{len(want)} matching line(s)"
    cmd = answered(turn)
    if cmd is None:
        return {"right": False, "expected": expected, "got": "(no answer through the answer tool)"}
    if sys.platform != "darwin":
        return {"right": False, "expected": expected, "got": "(not run: the check confines commands on macOS only)"}
    try:
        found = command.matches(command.checked(cmd), pristine)
    except command.Refused as why:
        return {"right": False, "expected": expected, "got": f"(refused: {why})"}
    extra, missing = len(set(found) - set(want)), len(set(want) - set(found))
    return {"right": found == want, "expected": expected,
            "got": f"{len(found)} matching line(s), {missing} missing, {extra} extra"}


def main():
    turn = json.load(open(os.environ["DIET_GRADE_TURN"]))
    truth = json.load(open(os.environ["DIET_SERVICE_FIXTURE_TRUTH"]))
    print(json.dumps(grade(turn, os.environ["DIET_GRADE_TASK"], truth, os.environ["DIET_SERVICE_FIXTURE_PRISTINE"])))


def selftest():
    import tempfile, gen
    call = lambda cmd, outcome="ok", name=TOOL: {"name": name, "outcome": outcome, "arguments": json.dumps({"command": cmd})}
    turn = lambda *calls, answer="": {"calls": list(calls), "answer": answer}
    bad, n = 0, 0
    with tempfile.TemporaryDirectory() as d:
        pristine = os.path.join(d, "pristine")
        truth = gen.write(11, pristine, os.path.join(d, "truth.json"))
        p = truth["params"]
        cases = [
            ("no call", "t1-ret-decl", turn(answer="ast-grep run ..."), False),
            ("a bash call is no answer", "t1-ret-decl", turn(call("ast-grep run -p x src", name="bash")), False),
            ("a refused call is no answer", "t1-ret-decl", turn(call(f"ast-grep run -l ts -p 'function $F($$$A): {p['target']} {{ $$$B }}' src", "refused")), False),
            ("not ast-grep", "t1-ret-decl", turn(call("rg -n 'function' src")), False),
            ("a shell operator", "t1-ret-decl", turn(call(f"ast-grep run -p 'function $F($$$A): {p['target']} {{ $$$B }}' src | head")), False),
        ]
        if sys.platform == "darwin" and command.shutil.which("ast-grep"):
            cases += [
                ("the return type", "t1-ret-decl", turn(call(f"ast-grep run -l ts -p 'function $F($$$A): {p['target']} {{ $$$B }}' src")), True),
                ("the same, by the sg name, with its own --json", "t1-ret-decl", turn(call(f"sg run -l ts -p 'function $F($$$A): {p['target']} {{ $$$B }}' src --json")), True),
                ("no return type: every untyped function instead", "t1-ret-decl", turn(call("ast-grep run -l ts -p 'function $F($$$A) { $$$B }' src")), False),
                ("the last call decides", "t1-ret-decl", turn(call("ast-grep run -l ts -p 'function $F($$$A) { $$$B }' src"),
                                                               call(f"ast-grep run -l ts -p 'function $F($$$A): {p['target']} {{ $$$B }}' src")), True),
                ("a direct-call pattern", "t2-zero-calls", turn(call(f"ast-grep run -l ts -p '{p['zero_fn']}($$$A)' src")), True),
                ("the barrel's outline", "t3-public-surface", turn(call("ast-grep outline src/index.ts")), True),
                ("the directory's exports are not the surface", "t3-public-surface", turn(call("ast-grep outline src --items exports")), False),
                ("an anchored outline", "t6-fn-outline", turn(call(f"ast-grep outline src --match '^{p['line_fn']}$'")), True),
                ("an unanchored outline matches more", "t6-fn-outline", turn(call(f"ast-grep outline src --match '{p['line_fn'][:4]}'")), False),
                ("a double-quoted metavariable is the shell's", "t5-emit-calls", turn(call('ast-grep run -p "$emit($$$A)" src')), False),
                ("single quotes keep it", "t5-emit-calls", turn(call("ast-grep run -p '$emit($$$A)' src")), True),
            ]
        for why, task, t, want in cases:
            n += 1
            got = grade(t, task, truth, pristine)
            if got["right"] != want:
                print(f"check: {why}: graded {got}, expected {want}"); bad += 1
    print(f"check: {n} answers graded, {bad} misgraded" + ("" if n > 5 else " (the commands run on macOS only: not run here)"))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main())
