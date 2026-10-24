"""Write the Lite scenarios from the task definitions (#942): `python3 -I make_scenario.py` writes `lite.jsonl` and `lite-empty.jsonl` beside it;
`--check` exits 1, naming the file, when either is not what it would write. The prompts' one source is grader/tasks_trap.py.

- `lite.jsonl`: the eight tutorial tasks, a declared meditation (the refill every arm crosses; golden acknowledges it as skipped), then the twelve
  held-out tasks; each ask is graded by check.py, three attempts, the feedback observations only.
- `lite-empty.jsonl`: `lite.jsonl` whose canned interviews capture nothing, so a rehearsal crosses an empty refill (`meditation_empty = "refill"`).

Both start the same services: the pinned Lite checkout, the model's and the grader's dev servers, and the grader's and the model's browser daemons.
The model's daemon is reached through its own socket (`sandbox_unix_sockets`) and its session id (`serve_env`). Their canned replies answer every
request with text and no call, but for what the interviews need: a rehearsal of the plumbing, every attempt graded wrong, not a run of the
tasks. The header's `seal` is the Lite checkout by part (#948): `source`, every top-level entry but `docs` (`.git` among them), and `docs`; a
sealed arm's next session cannot read the parts its regimen's `sealed` names. Its `seal_probe` is a file of each part, which the probe asks that
session to read (a sealed part's must fail, an unsealed part's must read), and its `seal_root` the checkout, whose `git show` of a sealed file must
fail. The `seal-probe` service, last, probes a sealed session's served code and browser reach before serve starts, and passes elsewhere. In `lite.jsonl` the first task's canned answer reads its stub, so an interview fork is warranted, and that fork records what it learned: the
interviews arm's working memory is not empty at the refill. Refutation forks, cut as each ask arrives and quoting it, decline first.
"""
import json
import os
import runpy
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TASKS = runpy.run_path(os.path.join(HERE, "grader", "tasks_trap.py"))["TASKS"]

# Every request a rehearsal makes is answered from this pool, in order: trunk turns, interview forks, the meditation's.
POOL = 600
# The refutation forks a rehearsal may cut: one per ask, and per failed command.
REFUTATIONS = 120
# A refutation's answer: no belief shown wrong. Routed on the refutation question's own words.
DECLINE = {"reply": {"when": "show that something you believed was wrong", "text": "",
                     "calls": [{"id": "canned-refuted", "name": "refute_beliefs", "arguments": "{\"refuted\":[]}"}]}}

ATTEMPTS = 3
CHECK = 'python3 -I "$DIET_GRADE_SCENARIO_DIR/check.py"'
BROWSER = "python3 -I services/browser.py"
LITE = "python3 -I services/lite.py"


def header(name):
    seed = {}
    for task in TASKS:
        for leaf in ("index.html", "main.ts"):
            seed["%s/%s" % (task["id"], leaf)] = "fixtures/%s/%s" % (task["id"], leaf)
    return {
        "scenario": name,
        "regimen": "canned.toml",
        "live_regimen": "native.toml",
        "head": "head.md",
        "seed": seed,
        "ports": ["model_vite", "grader_vite"],
        "services": [
            {"name": "lite", "start": LITE + " start", "stop": LITE + " stop", "ready": {"command": 'test -s "$DIET_SERVICE_DIR/provides.json"'}},
            {"name": "model-vite", "run": "exec python3 -I services/vite.py model", "ready": {"tcp": "model_vite"}},
            {"name": "grader-vite", "run": "exec python3 -I services/vite.py grader", "ready": {"tcp": "grader_vite"}},
            {"name": "grader-browser", "start": BROWSER + " start --vision --allow grader_vite --allow model_vite", "stop": BROWSER + " stop",
             "ready": {"command": BROWSER + " status"}, "ready_timeout_s": 180},
            {"name": "model-browser", "start": BROWSER + ' start --vision --workspace "$DIET_WORKTREE" --allow model_vite', "stop": BROWSER + " stop",
             "ready": {"command": BROWSER + " status"}, "ready_timeout_s": 180},
            {"name": "seal-probe", "start": "python3 -I services/seal_probe.py start", "stop": "python3 -I services/seal_probe.py stop",
             "ready": {"command": 'test -s "$DIET_SERVICE_DIR/provides.json"'}, "ready_timeout_s": 300},
        ],
        "sandbox_unix_sockets": ["service.model-browser.socket"],
        "seal": {"source": ["service.lite.sealed_source"], "docs": ["service.lite.docs"]},
        "seal_probe": {"source": ["{service.lite.root}/packages/babylon-lite/src/scene/scene-core.ts"],
                       "docs": ["{service.lite.root}/docs/lite/01-getting-started.md"]},
        "seal_root": "{service.lite.root}",
        "serve_env": {
            "CD4A_INTERNAL_DAEMON_SESSION_ID": "{service.model-browser.session}",
            "CHROME_DEVTOOLS_MCP_NO_UPDATE_CHECKS": "1",
            "CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS": "1",
        },
    }


def graded(task):
    return [
        {"step": "ask", "text": task["prompt"](False), "task": task["id"]},
        {"step": "grade", "task": task["id"], "check": CHECK, "attempts": ATTEMPTS, "timeout_s": 180,
         "wrong": "{got} That was attempt {attempt} of {attempts} at {task}; try again in ./{task}.",
         "last": "{got} That was the last attempt at {task}; moving on.",
         "right": "Graded: right. Moving on."},
    ]


def capturing(first):
    """The first task's answer reads its stub, and the interview the read warrants records what it learned."""
    read = {"id": "canned-read", "name": "bash", "arguments": json.dumps({"command": "cat %s/main.ts" % first})}
    return [
        {"reply": {"when": "In ./%s," % first, "text": "Reading the stub.", "calls": [read]}},
        # The default ask set's read asks (v3): a source read's, and a document's; whichever the read warrants answers.
        {"reply": {"when": "You just read source", "text": "API_SURFACE: NONE\nLEARNED: the stub of %s holds only a comment\nSTUCK: NONE" % first}},
        {"reply": {"when": "You just read a document", "text": "LEARNED: the stub of %s holds only a comment\nEVIDENCE: \"// The Babylon Lite scene for this task goes here.\"\nSTUCK: NONE" % first}},
    ]


def lines(name, tasks, meditate_after=None, capture=False):
    out = [header(name)]
    out += [DECLINE] * REFUTATIONS
    if capture:
        out += capturing(tasks[0]["id"])
    out += [{"reply": {"text": "Canned: no attempt."}} for _ in range(POOL)]
    for task in tasks:
        out += graded(task)
        if task["id"] == meditate_after:
            out.append({"step": "declare-meditation"})
    out.append({"step": "end"})
    return "".join(json.dumps(line, sort_keys=True) + "\n" for line in out)


def written():
    tutorial = [t for t in TASKS if t["split"] == "tutorial"]
    held = [t for t in TASKS if t["split"] == "held-out"]
    return {
        "lite.jsonl": lines("lite", tutorial + held, meditate_after=tutorial[-1]["id"], capture=True),
        "lite-empty.jsonl": lines("lite", tutorial + held, meditate_after=tutorial[-1]["id"]),
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
        sys.exit("make_scenario: %s not what grader/tasks_trap.py writes: run python3 -I make_scenario.py" % ", ".join(stale))


if __name__ == "__main__":
    if sys.argv[1:] not in ([], ["--check"]):
        sys.exit("usage: make_scenario.py [--check]")
    main(sys.argv[1:] == ["--check"])
