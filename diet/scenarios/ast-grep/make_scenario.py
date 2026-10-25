"""Writes the ast-grep tutorial's scenarios from `tasks.py` (#971): python3 -I make_scenario.py [--check].

Four scenarios, a pass each: the tutorial tasks and the held-out tasks, each bare (`head.md`) and with the official
skills (`head-skills.md`, the `skills` service). A pass is one session on a fresh instance (the `fixture` service),
each task asked once and graded once on its answer (`check.py`), with no feedback in the pass: a pass that misses a
task, or reads the docs, is what the tutorial rolls back (`pass.py` says which).

Each carries canned replies for a rehearsal on the canned substrate: a real ast-grep command, then an answer call.
They are the answers of the instance at CANNED_SEED, so a rehearsal with `DIET_AST_GREP_SEED=CANNED_SEED` grades
them, and the first task's is wrong on purpose (a pattern without the return type), so it grades both ways.
`--check` says when a written scenario is stale.
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gen  # noqa: E402
from tasks import HELD_OUT, TUTORIAL  # noqa: E402

CANNED_SEED = 7
CHECK = 'python3 -I "$DIET_GRADE_SCENARIO_DIR/check.py"'
TOOL = json.load(open(os.path.join(HERE, "answer-tool.json")))["name"]
FIXTURE = {"name": "fixture", "start": "python3 -I services/fixture.py start", "stop": "python3 -I services/fixture.py stop",
           "ready": {"command": 'test -s "$DIET_SERVICE_DIR/provides.json"'}}
SKILLS = {"name": "skills", "start": "python3 -I services/skills.py start", "stop": "python3 -I services/skills.py stop",
          "ready": {"command": 'test -s "$DIET_SERVICE_DIR/provides.json"'}, "ready_timeout_s": 120}
# DRAFT: model-facing. The task's id leads its ask, so the answer can name it.
ASK = "DRAFT. Task `{id}`: {ask}"
WRONG = "DRAFT. (Not sent: a pass asks each task once.)"


def canned_truth():
    for s in range(CANNED_SEED, CANNED_SEED + 1000):
        truth = gen.build(s)[1]
        if gen.usable(truth):
            return truth
    raise SystemExit("make_scenario: no usable canned instance")


def command(family, p):
    """A canned reply's command: what a model that knew the tool might run."""
    rule = lambda body: "ast-grep scan --inline-rules '" + "id: q\nlanguage: ts\nrule:\n" + body + "' src"
    return {
        "ret-decl": "ast-grep run -l ts -p 'function $F($$$A) { $$$B }' src",  # the miss: no `: $RET`
        "ret-arrow": f"ast-grep run -l ts -p 'const $N = ($$$A): {p['arrow_target']} => $B' src",
        "zero-calls": f"ast-grep run -l ts -p '{p['zero_fn']}($$$A)' src",
        "maybe-defined": f"ast-grep outline src --match '^{p['maybe']}$'",
        "public-fns": "ast-grep outline src/index.ts",
        "hidden-itfs": "ast-grep outline src/index.ts; ast-grep run -l ts -p 'export interface $I { $$$B }' src",
        "await-any": rule("  kind: function_declaration\n  has:\n    pattern: await $E\n    stopBy: end"),
        "log-in-try": rule(f"  kind: function_declaration\n  has:\n    stopBy: end\n    pattern: {p['log_fn']}($$$A)\n"
                           "    inside:\n      kind: try_statement\n      stopBy: end"),
        "emit-callers": rule("  kind: function_declaration\n  has:\n    pattern: $emit($$$A)\n    stopBy: end"),
        "track-count": "ast-grep run -l ts -p '$track($$$A)' src",
        "fn-line": f"ast-grep outline src --match '^{p['line_fn']}$'",
        "itf-line": f"ast-grep outline src --match '^{p['line_itf']}$'",
    }[family]


def replies(tasks, truth):
    out = []
    for i, task in enumerate(tasks):
        fam = task["family"]
        answer = truth["answers"][fam]["items"]
        if i == 0:  # wrong on purpose: what the return-type miss answers
            answer = ["(none)"]
        bash = {"id": f"canned-run-{task['id']}", "name": "bash", "arguments": json.dumps({"command": command(fam, truth["params"])})}
        call = {"id": f"canned-answer-{task['id']}", "name": TOOL, "arguments": json.dumps({"task": task["id"], "answer": answer})}
        out.append({"reply": {"when": f"Task `{task['id']}`", "text": "", "calls": [bash]}})
        # The task's next two requests, unrouted, in file order: the answer after the command, then the turn's end.
        out.append({"reply": {"text": "", "calls": [call]}})
        out.append({"reply": {"text": "Canned: answered."}})
    return out


def header(name, skills):
    return {"scenario": name, "regimen": "canned.toml", "live_regimen": "bare.toml",
            "head": "head-skills.md" if skills else "head.md", "answer_tool": "answer-tool.json",
            "services": [FIXTURE, SKILLS] if skills else [FIXTURE], "sandbox_tools": ["ast-grep"],
            # The answer key is the fixture's own directory: denied to the model's commands (probe_key.py proves it).
            "sandbox_secrets": ["service.fixture.dir"]}


def lines(name, tasks, skills):
    out = [header(name, skills)] + replies(tasks, canned_truth())
    for task in tasks:
        out.append({"step": "ask", "text": ASK.format(id=task["id"], ask=task["ask"]), "task": task["id"]})
        out.append({"step": "grade", "task": task["id"], "check": CHECK, "attempts": 1, "wrong": WRONG})
    out.append({"step": "end"})
    return "".join(json.dumps(line, sort_keys=True) + "\n" for line in out)


def written():
    return {
        "tutorial.jsonl": lines("ast-grep-tutorial", TUTORIAL, False),
        "tutorial-skills.jsonl": lines("ast-grep-tutorial-skills", TUTORIAL, True),
        "held-out.jsonl": lines("ast-grep-held-out", HELD_OUT, False),
        "held-out-skills.jsonl": lines("ast-grep-held-out-skills", HELD_OUT, True),
    }


def main(check):
    stale = []
    for name, text in written().items():
        path = os.path.join(HERE, name)
        if check:
            if not os.path.exists(path) or open(path).read() != text:
                stale.append(name)
        else:
            with open(path, "w") as f:
                f.write(text)
    if stale:
        sys.exit(f"make_scenario: {', '.join(stale)} not what tasks.py writes: run python3 -I make_scenario.py")


if __name__ == "__main__":
    if sys.argv[1:] not in ([], ["--check"]):
        sys.exit("usage: make_scenario.py [--check]")
    main(sys.argv[1:] == ["--check"])
