"""Writes the ast-grep tutorial's passes from `tasks.py` (#971): python3 -I make_scenario.py [--check].

One operation in isolation (Dispatch's reading of the maintainer's "each operation demonstrated once, in isolation",
unconfirmed): a scenario per task, `passes/<task>.jsonl`, and its twin with the official skills,
`passes/<task>-skills.jsonl`. Each is one session on a fresh instance (the `fixture` service):
1. the task, asked once and graded once on its answer, a command (`check.py`), with no feedback;
2. then a notes update through the `notes` tool (`notes-tool.json`), its ask a DRAFT stub Dispatch will replace.

`run_pass.py` carries the notes into the next pass's head. The canned replies answer with `ref.py`'s reference
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
ANSWER = json.load(open(os.path.join(HERE, "answer-tool.json")))["name"]
NOTES = json.load(open(os.path.join(HERE, "notes-tool.json")))["name"]
READY = {"command": 'test -s "$DIET_SERVICE_DIR/provides.json"'}
FIXTURE = {"name": "fixture", "start": "python3 -I ../services/fixture.py start", "stop": "python3 -I ../services/fixture.py stop", "ready": READY}
SKILLS = {"name": "skills", "start": "python3 -I ../services/skills.py start", "stop": "python3 -I ../services/skills.py stop", "ready": READY,
          "ready_timeout_s": 120}
# DRAFT, model-facing: the task's id leads its ask, so a canned reply can be routed to it.
ASK = "DRAFT. Task `{id}`. {ask}"
NOTES_ASK = "DRAFT. Notes update: update your notes on ast-grep through the notes tool, as concise as you can make them."
WRONG = "DRAFT. (Not sent: a pass asks its task once.)"


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
              "live_regimen": "../bare.toml", "head": "../head-skills.md" if skills else "../head.md",
              "answer_tool": ["../answer-tool.json", "../notes-tool.json"], "services": [FIXTURE, SKILLS] if skills else [FIXTURE],
              "sandbox_tools": ["ast-grep"],
              # The answer key is the fixture's own directory: denied to the model's commands (probe_key.py proves it).
              "sandbox_secrets": ["service.fixture.dir"]}
    answer = {"id": "canned-answer", "name": ANSWER, "arguments": json.dumps({"command": cmd})}
    notes = {"id": "canned-notes", "name": NOTES, "arguments": json.dumps({"notes": "DRAFT. Canned notes."})}
    out = [header,
           {"reply": {"when": f"Task `{task['id']}`", "text": "", "calls": [answer]}},
           {"reply": {"when": "Answer recorded", "text": "Canned: answered."}},
           {"reply": {"when": "Notes update:", "text": "", "calls": [notes]}},
           {"reply": {"when": "Notes saved", "text": "Canned: noted."}},
           {"step": "ask", "text": ASK.format(id=task["id"], ask=task["ask"]), "task": task["id"]},
           {"step": "grade", "task": task["id"], "check": CHECK, "attempts": 1, "wrong": WRONG},
           {"step": "ask", "text": NOTES_ASK},
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
