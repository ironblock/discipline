"""The Lite tasks' grade check (#942): `python3 -I check.py`, run by the scenario runner after each attempt, outside the model's sandbox.

It grades `$DIET_GRADE_TASK` in the grader's own browser (`$DIET_SERVICE_GRADER_BROWSER_SESSION`), loading the page from the grader's dev server
(`$DIET_PORT_GRADER_VITE`, which records Lite's lifecycle), and prints one line, `{"right", "expected", "got"}`: right when tiers a (what the page does)
and b (how) both pass; `got` is what the model is told, observations only (grader/grade_trap.py's `feedback`). Every check of both tiers, the dumps and
the screenshots go to `$DIET_GRADE_OUT`, which the model never sees.

`python3 -I check.py --selftest` checks that what the model is told never carries a cause.
"""
import json
import os
import runpy
import shutil
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def load():
    lib = runpy.run_path(os.path.join(HERE, "grader", "trap_lib.py"))
    grader = runpy.run_path(os.path.join(HERE, "grader", "grade_trap.py"))
    tasks = runpy.run_path(os.path.join(HERE, "grader", "tasks_trap.py"))["TASKS"]
    return lib, grader, tasks


def main():
    lib, grader, tasks = load()
    name = os.environ["DIET_GRADE_TASK"]
    out = os.environ["DIET_GRADE_OUT"]
    task = next((t for t in tasks if t["id"] == name), None)
    if task is None:
        sys.exit("no task %s" % name)
    cdt = lib["Cdt"](os.environ["DIET_SERVICE_GRADER_BROWSER_SESSION"])
    session = grader["Session"](cdt, int(os.environ["DIET_PORT_GRADER_VITE"]), os.environ["DIET_SERVICE_GRADER_BROWSER_WORKSPACE"],
                                lib["DUMP_JS"], lib["INIT_JS"], task)
    res = grader["grade"](task, session, lib["Png"])
    obs = res["obs"]
    kept = dict(grader["diagnosis"](res), dumps=obs.get("dumps"), load=obs.get("load"), clicks=obs.get("clicks"), evals=obs.get("evals"),
                notes=obs.get("notes"), scenario_error=obs.get("scenario_error"))
    with open(os.path.join(out, "diagnosis.json"), "w") as f:
        json.dump(kept, f, indent=1, default=str)
    for label, path in obs.get("shot_paths", {}).items():
        shutil.copyfile(path, os.path.join(out, "%s.png" % label))
    right = res["a"]["ok"] and res["b"]["ok"]
    print(json.dumps({"right": right, "expected": "the page the task describes, made as it says", "got": grader["feedback"](res)}))


def selftest():
    grader = runpy.run_path(os.path.join(HERE, "grader", "grade_trap.py"))
    feedback = grader["feedback"]
    tier = grader["tier"]
    seen = ("after the click: the red box is drawn on the right", False, "at least 800 red px in the right half", "0 red px")
    kind = ("`crate` standard material, diffuse colour", False, "standard (0.9, 0.1, 0.1)", "pbrGroupBuilder (0.9, 0.1, 0.1)")
    cause = ("... and rendered through the swap drain (the drain built it a renderable)", False, "drain with own renderable", "0 drain(s)")
    told = feedback({"a": tier([seen, kind]), "b": tier([cause])})
    assert told.startswith("Graded: wrong. "), told
    assert "the red box is drawn on the right: expected at least 800 red px" in told, told
    assert "saw PBR (0.9, 0.1, 0.1)" in told and "GroupBuilder" not in told, told
    assert "drain" not in told and "renderable" not in told, told
    assert grader["CONSTRAINT_UNMET"] not in told, "a page that is wrong is told what it does, not the constraint: " + told
    only_b = feedback({"a": tier([]), "b": tier([cause])})
    assert only_b == "Graded: wrong. " + grader["CONSTRAINT_UNMET"], only_b
    assert feedback({"a": tier([]), "b": tier([])}) == "Graded: right."
    _, _, tasks = load()
    assert len(tasks) == 20 and len({t["id"] for t in tasks}) == 20, [t["id"] for t in tasks]
    print("check selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    else:
        main()
