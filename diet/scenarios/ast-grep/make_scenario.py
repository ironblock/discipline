"""Writes the ast-grep tutorial's passes from `tasks.py` (#971): python3 -I make_scenario.py [--check].

One operation in isolation: a scenario per task, `passes/<task>.jsonl`, and its twin with the official skills,
`passes/<task>-skills.jsonl`. Each is one session on a fresh instance (the `fixture` service), in the native
condition (PREREG.md, 2026-10-29): `ast-grep` is the real binary and searches run; the fixture's own directory, which
holds the truth, is denied to the model's commands. The task is asked once and graded once on its answer, a command,
by execution (`check.py`), with no feedback.

The canned replies answer with `ref.py`'s reference
command for the instance at CANNED_SEED, but the first task's is the naive one (no return type), so a rehearsal
with `DIET_AST_GREP_SEED=CANNED_SEED` grades both ways. `--check` says when a written scenario is stale.
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import gen, ref  # noqa: E402
from tasks import TASKS  # noqa: E402

CANNED_SEED = 7
CHECK = 'python3 -I "$DIET_GRADE_SCENARIO_DIR/../check.py"'
ANSWER = json.load(open(os.path.join(HERE, "answer-tool-native.json")))["name"]
READY = {"command": 'test -s "$DIET_SERVICE_DIR/provides.json"'}
FIXTURE = {"name": "fixture", "start": "python3 -I ../services/fixture.py start", "stop": "python3 -I ../services/fixture.py stop", "ready": READY}
SKILLS = {"name": "skills", "start": "python3 -I ../services/skills.py start", "stop": "python3 -I ../services/skills.py stop", "ready": READY,
          "ready_timeout_s": 120}
# DRAFT, model-facing: the task's id leads its ask, so a canned reply can be routed to it.
ASK = "DRAFT. Task `{id}`. {ask}"
WRONG = "DRAFT. (Not sent: a pass asks its task once.)"
# A step waits as long as the 60-minute guard allows a pass, never the drive's 900 s default (two floor passes voided on it).
PATIENCE = 3600
# The canned miss answers: what each subcommand does on a miss (miss_truth.json, ast-grep 0.44.0).
MISS_FIELDS = {"zero-calls": {"exit_status_when_none": 1, "prints_nothing_when_none": True},
               "maybe-defined": {"exit_status_when_none": 0, "prints_nothing_when_none": False}}


def canned_truth():
    for s in range(CANNED_SEED, CANNED_SEED + 1000):
        truth = gen.build(s)[1]
        if gen.usable(truth):
            return truth
    raise SystemExit("make_scenario: no usable canned instance")


def lines(task, skills, first):
    p = canned_truth()["params"]
    cmd = (ref.naive if first else ref.reference)(task["family"], p)
    header = {"scenario": f"ast-grep-{task['id']}" + ("-skills" if skills else ""), "regimen": "../canned.toml",
              "live_regimen": "../bare.toml", "head": "../head-native-skills.md" if skills else "../head-native.md",
              "answer_tool": ["../" + task.get("tool", "answer-tool-native.json")], "services": [FIXTURE, SKILLS] if skills else [FIXTURE],
              "sandbox_tools": ["ast-grep"],
              # The answer key is the fixture's own directory: denied to the model's commands, read and exec alike
              # (probe_key.py proves it).
              "sandbox_secrets": ["service.fixture.dir"]}
    answer = {"id": "canned-answer", "name": ANSWER, "arguments": json.dumps(dict({"command": cmd}, **MISS_FIELDS.get(task["family"], {})))}
    out = [header,
           {"reply": {"when": f"Task `{task['id']}`", "text": "", "calls": [answer]}},
           {"reply": {"when": "Answer recorded", "text": "Canned: answered."}},
           {"step": "ask", "text": ASK.format(id=task["id"], ask=task["ask"]), "task": task["id"], "patience": PATIENCE},
           {"step": "grade", "task": task["id"], "check": CHECK, "attempts": 1, "wrong": WRONG, "patience": PATIENCE},
           {"step": "end"}]
    return "".join(json.dumps(line, sort_keys=True) + "\n" for line in out)


def written():
    out = {}
    for i, task in enumerate(TASKS):
        for skills in (False, True):
            out[os.path.join("passes", task["id"] + ("-skills" if skills else "") + ".jsonl")] = lines(task, skills, i == 0)
    return out


def main(check):
    stale = []
    os.makedirs(os.path.join(HERE, "passes"), exist_ok=True)
    for name, text in written().items():
        path = os.path.join(HERE, name)
        if check:
            if not os.path.exists(path) or open(path).read() != text:
                stale.append(name)
        else:
            with open(path, "w") as f:
                f.write(text)
    known = {os.path.join("passes", f) for f in os.listdir(os.path.join(HERE, "passes"))}
    stray = sorted(known - set(written()))
    if stale or (check and stray):
        sys.exit(f"make_scenario: {', '.join(stale + stray)} not what tasks.py writes: run python3 -I make_scenario.py")


if __name__ == "__main__":
    if sys.argv[1:] not in ([], ["--check"]):
        sys.exit("usage: make_scenario.py [--check]")
    main(sys.argv[1:] == ["--check"])
