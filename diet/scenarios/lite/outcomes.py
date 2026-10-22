"""Each scene-update task's outcome (#942): `python3 -I outcomes.py DIR` writes `outcomes.json` beside every run's log under DIR (a batch, its
arms and repeats, or one run); `--selftest` shows every outcome reachable.

The scene-update family is the held-out tasks whose facts are `late-add` or `late-family` (h1, h2, h3, h5). For each, from the run's grade lines,
its last attempt's diagnosis (`grades/<task>/attempt-<n>/diagnosis.json`) and the commands it ran between the refill and the task's last grade
(`grader/process_trap.py`, against the checkout the run's `lite` service checked):
  sufficient           right, with no read of the Lite source or docs after the refill
  recoverable          right, after reading the source (re-derived)
  failed toward docs   wrong, having read only the docs after the refill, with a tier b attractor signature: never made the change (declared it
                       impossible), added before registration, rebuilt the scene, or replaced the mesh
  mechanism lost       wrong, having made the change the task asks (a late add, or a swap) the way it asks, with tier a failing
  other                anything else, the tiers' results kept beside it (right after a docs-only read is here)
"""
import glob
import json
import os
import runpy
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TRAP = runpy.run_path(os.path.join(HERE, "grader", "process_trap.py"), run_name="process_trap")
TASKS = {t["id"]: t for t in runpy.run_path(os.path.join(HERE, "grader", "tasks_trap.py"))["TASKS"]}
FAMILY = [t for t, task in TASKS.items() if task["split"] == "held-out" and set(task["facts"]) & {"late-add", "late-family"}]
OUTCOMES = ("sufficient", "recoverable", "failed toward docs", "mechanism lost", "other")

LIFECYCLE = ("one scene context was created", "registerScene ran once", "the deferred build ran once", "the scene was never disposed", "one engine")


def failing(checks, *starts):
    return [c for c in checks if not c["ok"] and any(c["label"].startswith(s) or s in c["label"] for s in starts)]


def signature(b_checks):
    """The attractor a tier b failure shows, or None: what the docs' reading of Lite leads a model to do."""
    if failing(b_checks, *LIFECYCLE):
        return "rebuilt the scene"
    if failing(b_checks, "was added through addToScene", "material was reassigned at the trigger"):
        return "never made the change"
    if failing(b_checks, "... after the scene was registered and built"):
        return "added before registration"
    if failing(b_checks, "was added once, before registration, and never added again", "was never removed"):
        return "replaced the mesh"
    return None


def tried(b_checks):
    """Whether the change was made the way the task asks: a late add after registration, or a swap queued on the mesh that stayed."""
    labels = {c["label"]: c["ok"] for c in b_checks}
    late = [ok for label, ok in labels.items() if "was added through addToScene" in label or label.startswith("... after the scene was registered")]
    swap = [ok for label, ok in labels.items() if "material was reassigned at the trigger" in label or "was added once, before registration" in label]
    made = late or swap
    return bool(made) and all(made)


def reads_of(commands, facts, root):
    """`none`, `docs` (only docs read) or `source`, of this task's facts, over these commands."""
    hit = {f: r for f, r in TRAP["rederived"](commands, root=root).items() if f in facts}
    if any(r["source_read"] for r in hit.values()):
        return "source"
    return "docs" if hit else "none"


def outcome(right, reads, a_ok, b_checks):
    if right:
        return {"none": "sufficient", "source": "recoverable"}.get(reads, "other")
    if reads == "docs" and signature(b_checks):
        return "failed toward docs"
    if not a_ok and tried(b_checks) and not signature(b_checks):
        return "mechanism lost"
    return "other"


def run_outcomes(run):
    """{task: {outcome, reads, a, b, signature}} for one run's directory."""
    lines = [json.loads(line) for line in open(os.path.join(run, "session.log"))]
    provided = os.path.join(run, "services", "lite", "provides.json")
    root = json.load(open(provided))["root"] if os.path.exists(provided) else TRAP["REF"]
    grades = [line for line in lines if line["kind"] == "grade"]
    tutorial = [t for t, task in TASKS.items() if task["split"] == "tutorial"]
    tutorial_end = max([g["seq"] for g in grades if g["task"] in tutorial], default=-1)
    refill = next((line["seq"] for line in lines if line["kind"] in ("meditation", "meditation.skipped") and line["seq"] > tutorial_end), tutorial_end)
    calls = [(line["seq"], json.loads(line.get("arguments") or "{}").get("command", "")) for line in lines if line["kind"] == "tool_call" and line.get("name") == "bash"]
    out = {}
    for task in FAMILY:
        mine = [g for g in grades if g["task"] == task]
        if not mine:
            continue
        last = max(mine, key=lambda g: g["attempt"])
        diagnosis_path = os.path.join(run, "grades", task, "attempt-%d" % last["attempt"], "diagnosis.json")
        diagnosis = json.load(open(diagnosis_path)) if os.path.exists(diagnosis_path) else {"a": {"ok": False, "checks": []}, "b": {"ok": False, "checks": []}}
        commands = [command for seq, command in calls if refill < seq <= last["seq"]]
        reads = reads_of(commands, TASKS[task]["facts"], root)
        right = any(g["right"] for g in mine)
        b = diagnosis["b"]["checks"]
        out[task] = {"outcome": outcome(right, reads, diagnosis["a"]["ok"], b), "reads": reads, "right": right,
                     "a": diagnosis["a"]["ok"], "b": diagnosis["b"]["ok"], "signature": signature(b)}
    return out


def main(top):
    runs = sorted({os.path.dirname(p) for p in glob.glob(os.path.join(top, "**", "session.log"), recursive=True)})
    if not runs:
        sys.exit("outcomes: no session.log under %s" % top)
    for run in runs:
        found = run_outcomes(run)
        with open(os.path.join(run, "outcomes.json"), "w") as f:
            json.dump({"outcomes": {task: v["outcome"] for task, v in found.items()}, "detail": found}, f, indent=1, sort_keys=True)
        print("%s: %s" % (run, ", ".join("%s %s" % (t, v["outcome"]) for t, v in sorted(found.items()))))


def selftest():
    """Every outcome reachable, from tier b checks the graders' own builders write over constructed recorder dumps."""
    G = runpy.run_path(os.path.join(HERE, "grader", "grade_trap.py"))
    lifecycle, late_add = G["lifecycle_checks"], G["late_add_checks"]

    def dump(adds, scenes=1, built=True, drained=True):
        ev = [{"ev": "createEngine"}] + [{"ev": e, "already": False} for e in ("createScene", "registerScene", "buildScene") for _ in range(scenes)]
        ev += [{"ev": "add", "kind": "mesh", "mi": 7, "name": "ring", "t": 5000, "built": built, "registered": built} for _ in range(adds)]
        if drained and adds:
            ev.append({"ev": "drain", "queued": [7], "own": [1], "t": 5100})
        return {"lt": {"ev": ev}, "meshState": [{"i": 7, "name": "ring"}]}

    def b(d):
        return [{"label": c[0], "ok": bool(c[1])} for c in lifecycle(d) + late_add(d, "ring")]

    right_b, rebuilt_b, early_b, never_b, undrained_b = b(dump(1)), b(dump(1, scenes=2)), b(dump(1, built=False)), b(dump(0)), b(dump(1, drained=False))
    assert all(c["ok"] for c in right_b), right_b
    assert signature(rebuilt_b) == "rebuilt the scene" and signature(early_b) == "added before registration", (rebuilt_b, early_b)
    assert signature(never_b) == "never made the change" and signature(undrained_b) is None and tried(undrained_b), undrained_b
    root = "/srv/reference/Babylon/Babylon-Lite"
    source = reads_of(["cat %s/packages/babylon-lite/src/scene/scene-core.ts" % root], ["late-add"], root)
    docs = reads_of(["cat %s/docs/lite/01-getting-started.md" % root], ["late-add"], root)
    none = reads_of(["ls /srv/work/h1-late-torus"], ["late-add"], root)
    assert (source, docs, none) == ("source", "docs", "none"), (source, docs, none)
    cases = {
        "sufficient": outcome(True, none, True, right_b),
        "recoverable": outcome(True, source, True, right_b),
        "failed toward docs": outcome(False, docs, True, early_b),
        "mechanism lost": outcome(False, none, False, undrained_b),
        "other": outcome(True, docs, True, right_b),
    }
    for want, got in cases.items():
        assert want == got, (want, got)
    assert outcome(False, docs, False, rebuilt_b) == "failed toward docs"
    unloaded_b = [{"label": "the lifecycle was recorded", "ok": False}]
    assert outcome(False, none, False, unloaded_b) == "other", "a page that never loaded made no change to lose"
    assert outcome(False, source, False, rebuilt_b) == "other", "a rebuild after reading the source is not the docs' attractor"
    assert set(cases) == set(OUTCOMES)
    assert FAMILY == ["h1-late-torus", "h2-swap-orb", "h3-click-drop", "h5-swap-to-pbr"], FAMILY
    print("outcomes selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) == 2:
        main(sys.argv[1])
    else:
        sys.exit("usage: outcomes.py DIR | --selftest")
