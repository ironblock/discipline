"""Each scene-update task's outcome (#942): `python3 -I outcomes.py DIR` writes `outcomes.json` beside every run's log under DIR (a batch, its
arms and repeats, or one run); `--selftest` shows every outcome reachable.

The scene-update family is the held-out tasks whose facts are `late-add` or `late-family` (h1, h2, h3, h5). For each, from the run's grade lines,
its last attempt's diagnosis (`grades/<task>/attempt-<n>/diagnosis.json`) and the reads in the task's own window: the commands between its
first ask and its last grade, classified by `grader/process_trap.py` against the checkout the run's `lite` service checked. A task's window
holds only its own reads, so tasks that share a fact are no longer credited each other's (the interviews smoke's audit). A run that went on
in a later session (#948, `next/`) is read there, against the checkout its own `lite` service checked:
  sufficient           right, with no read of the Lite source or docs after the refill
  recoverable          right, after reading the source (re-derived)
  failed toward docs   wrong, having read only the docs after the refill, with a tier b attractor signature: never made the change (declared it
                       impossible), added before registration, rebuilt the scene, or replaced the mesh
  mechanism lost       wrong, having made the change the task asks (a late add, or a swap) the way it asks, with tier a failing; or wrong
                       with a page error naming a trap internal (`internal`, below), whatever was tried: the trap missed, or (in a sealed
                       session, whose bundle renames the internals) the seal working on an internal the notes named
  API lost             wrong, with a page error naming Lite's public API (`api`, below) and none naming an internal: a missing export,
                       or a public export, member or options field the page used wrongly (#948's second smoke, declared before its data)
  other                anything else, the tiers' results kept beside it (right after a docs-only read is here)

A wrong task's page errors (`errors_of`: its last attempt's `load` and `dumps` errors) are read for the names
they carry (`named`): a missing export (`does not provide an export named 'X'`), `X is not a function` / `is not a constructor` /
`is not defined`, `reading 'X'` / `setting 'X'` of undefined or null, and in Lite's own thrown messages (`must`, `unknown`, `invalid`,
`expected`, `received`) any public name. A name is `internal` when it starts with `_` (Lite's convention; the bundle's `_r1`... too) or
is one of the bundle's renamed internals; `api` when it is a missing export, or one of the checkout's exports or the fields of its
exported `*Options` interfaces and types (`public_names`, which leaves out names any JavaScript value has). A name that is neither, a page's own, counts for nothing. A right task is never
classified by its errors. The grader's own exception (`scenario_error`) is never a page error: it is listed beside the outcome as
`grader_error`, and the task falls through to the rules above. Known limit: a method the page invented on a Lite object (no import, no public name) is not API lost.
"""
import glob
import json
import re
import os
import runpy
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TRAP = runpy.run_path(os.path.join(HERE, "grader", "process_trap.py"), run_name="process_trap")
TASKS = {t["id"]: t for t in runpy.run_path(os.path.join(HERE, "grader", "tasks_trap.py"))["TASKS"]}
FAMILY = [t for t, task in TASKS.items() if task["split"] == "held-out" and set(task["facts"]) & {"late-add", "late-family"}]
OUTCOMES = ("sufficient", "recoverable", "failed toward docs", "mechanism lost", "API lost", "other")
RENAMED = runpy.run_path(os.path.join(HERE, "services", "bundle.py"))["RENAMED"]
SRC = os.path.join("packages", "babylon-lite", "src")
MISSING_EXPORT = re.compile(r"does not provide an export named '([\w$]+)'")
NAMES = [re.compile(r"([\w$]+) is not (?:a function|a constructor|defined)"), re.compile(r"\((?:reading|setting) '([\w$]+)'\)")]
# Names every JavaScript array, function, promise, map or string has: an error reading one says nothing about Lite.
JS_NAMES = {"length", "name", "size", "value", "values", "keys", "entries", "push", "pop", "map", "forEach", "filter", "reduce", "slice",
            "splice", "indexOf", "includes", "join", "then", "catch", "finally", "call", "apply", "bind", "get", "set", "has", "add",
            "delete", "clear", "prototype", "constructor", "toString", "valueOf"}
THROWN = re.compile(r"\b(?:must|unknown|invalid|expected|received)\b", re.I)

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


_PUBLIC = {}


def public_names(root):
    """The checkout's public API, by name: what its source exports, and the fields of its exported `*Options` interfaces and types; no
    `_` name, no name of two characters or fewer, and none a JavaScript value carries of its own (JS_NAMES): those a page's own code throws
    on too. Read once per checkout."""
    if root not in _PUBLIC:
        names = set()
        for path in glob.glob(os.path.join(root, SRC, "**", "*.ts"), recursive=True):
            text = open(path, encoding="utf-8", errors="replace").read()
            names.update(re.findall(r"^export\s+(?:declare\s+)?(?:async\s+)?(?:function\*?|const|let|class|interface|type|enum)\s+([\w$]+)", text, re.M))
            for block in re.findall(r"^export\s+(?:interface|type)\s+[\w$]*Options\b[^{=]*[{=]\s*\{?(.*?)^\}", text, re.M | re.S):
                names.update(re.findall(r"^\s*(?:readonly\s+)?([\w$]+)\??\s*:", block, re.M))
            for group in re.findall(r"^export\s*(?:type\s*)?\{([^}]*)\}", text, re.M):
                names.update(part.split(" as ")[-1].strip() for part in group.split(",") if part.strip())
        _PUBLIC[root] = {n for n in names if len(n) > 2 and not n.startswith("_") and n not in JS_NAMES}
    return _PUBLIC[root]


def internal(name):
    return name.startswith("_") or name in RENAMED


def errors_of(diagnosis):
    """A grade's page errors: its load's and its dumps'. Never its `scenario_error`, the grader's own exception (see `grader_error`)."""
    errs = list((diagnosis.get("load") or {}).get("errs") or [])
    dumps = diagnosis.get("dumps") or {}
    for d in dumps.values() if isinstance(dumps, dict) else dumps:
        if isinstance(d, dict):
            errs += d.get("errs") or []
    return errs


def grader_error(diagnosis):
    """The grader's own exception, or None: listed beside the outcome so a grader bug shows, never read as the page's error."""
    e = diagnosis.get("scenario_error")
    return None if e in (None, "None", "") else str(e)


def named(errors, public):
    """{api: [names], internal: [names]} the page errors carry (see the module's doc)."""
    out = {"api": [], "internal": []}
    for e in errors:
        found = [(n, True) for n in MISSING_EXPORT.findall(e)]
        found += [(n, False) for rx in NAMES for n in rx.findall(e)]
        if THROWN.search(e):
            found += [(n, False) for n in re.findall(r"[A-Za-z_$][\w$]*", e) if n in public]
        for n, missing in found:
            if internal(n):
                out["internal"].append(n)
            elif missing or n in public:
                out["api"].append(n)
    return {k: sorted(set(v)) for k, v in out.items()}


def outcome(right, reads, a_ok, b_checks, names=None):
    if right:
        return {"none": "sufficient", "source": "recoverable"}.get(reads, "other")
    names = names or {"api": [], "internal": []}
    if names["internal"]:
        return "mechanism lost"
    if names["api"]:
        return "API lost"
    if reads == "docs" and signature(b_checks):
        return "failed toward docs"
    if not a_ok and tried(b_checks) and not signature(b_checks):
        return "mechanism lost"
    return "other"


def run_outcomes(run):
    """{task: {outcome, reads, a, b, signature}} for one run's directory: its later session's, when it went on in one (#948)."""
    later = os.path.join(run, "next")
    if os.path.exists(os.path.join(later, "session.log")):
        return session_outcomes(later)
    return session_outcomes(run)


def session_outcomes(run):
    """The outcomes of one session's directory, each task's from its own window."""
    lines = [json.loads(line) for line in open(os.path.join(run, "session.log"))]
    provided = os.path.join(run, "services", "lite", "provides.json")
    root = json.load(open(provided))["root"] if os.path.exists(provided) else TRAP["REF"]
    grades = [line for line in lines if line["kind"] == "grade"]
    windows = TRAP["task_windows"](lines)
    calls = [(line["seq"], json.loads(line.get("arguments") or "{}").get("command", "")) for line in lines if line["kind"] == "tool_call" and line.get("name") == "bash"]
    out = {}
    for task in FAMILY:
        mine = [g for g in grades if g["task"] == task]
        if not mine:
            continue
        last = max(mine, key=lambda g: g["attempt"])
        diagnosis_path = os.path.join(run, "grades", task, "attempt-%d" % last["attempt"], "diagnosis.json")
        diagnosis = json.load(open(diagnosis_path)) if os.path.exists(diagnosis_path) else {"a": {"ok": False, "checks": []}, "b": {"ok": False, "checks": []}}
        lo, hi = windows.get(task, (last["seq"], last["seq"]))
        commands = [command for seq, command in calls if lo < seq <= hi]
        reads = reads_of(commands, TASKS[task]["facts"], root)
        right = any(g["right"] for g in mine)
        b = diagnosis["b"]["checks"]
        errors = errors_of(diagnosis)
        names = named(errors, public_names(root)) if errors and not right else {"api": [], "internal": []}
        out[task] = {"outcome": outcome(right, reads, diagnosis["a"]["ok"], b, names), "reads": reads, "right": right,
                     "a": diagnosis["a"]["ok"], "b": diagnosis["b"]["ok"], "signature": signature(b), "named": names,
                     "grader_error": grader_error(diagnosis)}
    return out


def main(top):
    logs = {os.path.dirname(p) for p in glob.glob(os.path.join(top, "**", "session.log"), recursive=True)}
    # A later session (#948) is its run's, not a run of its own.
    runs = sorted(d for d in logs if not (os.path.basename(d) == "next" and os.path.dirname(d) in logs))
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
    cases["API lost"] = api_lost(none, undrained_b)
    assert set(cases) == set(OUTCOMES)
    assert FAMILY == ["h1-late-torus", "h2-swap-orb", "h3-click-drop", "h5-swap-to-pbr"], FAMILY
    carried_run(root)
    shared_fact(root)
    print("outcomes selftest ok")


def api_lost(reads, tried_b):
    """#948's second smoke: API lost needs a page error naming Lite's public API, never one naming an internal; a right task is never
    classified by its errors. -> the outcome of a missing export, for the reachability table."""
    import tempfile
    with tempfile.TemporaryDirectory() as root:
        src = os.path.join(root, SRC, "mesh")
        os.makedirs(src)
        with open(os.path.join(src, "create-torus.ts"), "w") as f:
            f.write("export interface TorusOptions {\n    radius?: number;\n    readonly tessellation: number;\n    _cache?: number;\n    length: number;\n}\n"
                    "export interface TorusState {\n    spin: number;\n}\n"
                    "export function createTorus(name: string, o: TorusOptions) {}\nexport function enqueueMaterialSwap() {}\n"
                    "function helper() {}\nexport { helper as torusHelper };\n")
        public = public_names(root)
        assert {"TorusOptions", "radius", "tessellation", "createTorus", "torusHelper"} <= public and "_cache" not in public, public
        assert "helper" not in public and "spin" not in public and "length" not in public, public
        diagnosis = {"load": {"errs": ["uncaught: The requested module '/@fs/x/index.ts' does not provide an export named 'createTorusMesh'"]},
                     "dumps": {"final": {"errs": ["uncaught: createTorus: tessellation must be an integer"]}}, "scenario_error": "None"}
        assert len(errors_of(diagnosis)) == 2 and grader_error(diagnosis) is None, errors_of(diagnosis)
        # The grader's own exception names a public field and says "expected": never the page's error, and listed.
        grader_bug = {"load": {"errs": []}, "dumps": {}, "scenario_error": "TypeError: Cannot read properties of undefined (reading 'radius'); expected a dump"}
        assert errors_of(grader_bug) == [] and named(errors_of(grader_bug), public) == {"api": [], "internal": []}, grader_bug
        assert grader_error(grader_bug) == grader_bug["scenario_error"]
        assert outcome(False, reads, False, tried_b, named(errors_of(grader_bug), public)) != "API lost"
        cases = [
            # (page error, the outcome a wrong task gets, what it names)
            ("uncaught: The requested module '/@fs/x/index.ts' does not provide an export named 'createTorusMesh'", "API lost", "api"),
            ("uncaught: createTorus: tessellation must be an integer", "API lost", "api"),
            ("uncaught: Cannot read properties of undefined (reading 'radius')", "API lost", "api"),
            ("uncaught: Cannot read properties of undefined (reading '_rebuildSingle')", "mechanism lost", "internal"),
            # A sealed bundle renames the internals: importing one the notes named fails as a missing export of an internal's name.
            ("uncaught: The requested module '/@fs/b/index.js' does not provide an export named 'enqueueMaterialSwap'", "mechanism lost", "internal"),
            ("uncaught: scene._r3 is not a function", "mechanism lost", "internal"),
            # An internal reached through a property the bundle renamed: the error names only `push`, which is no public name.
            ("uncaught: Cannot read properties of undefined (reading 'push')", outcome(False, reads, False, tried_b), None),
            ("uncaught: myHelper is not defined", outcome(False, reads, False, tried_b), None),
            ("uncaught: Cannot read properties of undefined (reading 'length')", outcome(False, reads, False, tried_b), None),
        ]
        for error, want, kind in cases:
            names = named([error], public)
            got = outcome(False, reads, False, tried_b, names)
            assert got == want and (bool(names[kind]) if kind else not any(names.values())), (error, got, names)
            assert got != "API lost" or not names["internal"], (error, names)
            # A right task is never classified by its errors.
            assert outcome(True, reads, True, tried_b, names) == "sufficient", error
        both = named([cases[0][0], cases[3][0]], public)
        assert outcome(False, reads, False, tried_b, both) == "mechanism lost", "an internal named anywhere outranks the API"
        return outcome(False, reads, False, tried_b, named([cases[0][0]], public))


def shared_fact(root):
    """Tasks that share a fact are each credited only their own window's reads: h1's read of late-add's source makes h1 recoverable, and h2,
    which shares late-add and read nothing in its window, stays sufficient (before per-task windows, both were recoverable)."""
    import tempfile
    with tempfile.TemporaryDirectory() as top:
        run = os.path.join(top, "repeat-01")
        os.makedirs(os.path.join(run, "services", "lite"))
        with open(os.path.join(run, "services", "lite", "provides.json"), "w") as f:
            json.dump({"root": root}, f)
        cat = {"kind": "tool_call", "name": "bash", "arguments": json.dumps({"command": "cd %s/packages/babylon-lite/src && cat scene/scene-core.ts" % root})}
        lines = [{"kind": "session.start"}, {"kind": "meditation"},
                 {"kind": "ask", "task": "h1-late-torus"}, cat, {"kind": "grade", "task": "h1-late-torus", "attempt": 1, "right": True},
                 {"kind": "ask", "task": "h2-swap-orb"}, {"kind": "grade", "task": "h2-swap-orb", "attempt": 1, "right": True}]
        with open(os.path.join(run, "session.log"), "w") as f:
            f.writelines(json.dumps(dict(line, seq=i)) + "\n" for i, line in enumerate(lines))
        found = {t: v["outcome"] for t, v in run_outcomes(run).items()}
        assert found == {"h1-late-torus": "recoverable", "h2-swap-orb": "sufficient"}, found


def carried_run(root):
    """#948: a run that went on in a later session is read there, every command of it after the refill, and that session is not a run."""
    import tempfile
    with tempfile.TemporaryDirectory() as top:
        run = os.path.join(top, "repeat-01")
        later = os.path.join(run, "next")
        os.makedirs(os.path.join(later, "services", "lite"))
        def log(path, lines):
            with open(path, "w") as f:
                f.writelines(json.dumps(dict(line, seq=i)) + "\n" for i, line in enumerate(lines))
        log(os.path.join(run, "session.log"), [{"kind": "session.start"}, {"kind": "grade", "task": "t1-first-scene", "attempt": 1, "right": True},
                                               {"kind": "meditation"}])
        cat = {"kind": "tool_call", "name": "bash", "arguments": json.dumps({"command": "cat %s/packages/babylon-lite/src/scene/scene-core.ts" % root})}
        # A meditation of its own after the read: the read is still in the task's window.
        log(os.path.join(later, "session.log"), [{"kind": "session.start"}, {"kind": "ask", "task": "h1-late-torus"}, cat, {"kind": "meditation"},
                                                 {"kind": "grade", "task": "h1-late-torus", "attempt": 1, "right": True}])
        with open(os.path.join(later, "services", "lite", "provides.json"), "w") as f:
            json.dump({"root": root}, f)
        found = run_outcomes(run)
        assert {t: v["outcome"] for t, v in found.items()} == {"h1-late-torus": "recoverable"}, found
        main(top)
        assert os.path.exists(os.path.join(run, "outcomes.json")) and not os.path.exists(os.path.join(later, "outcomes.json"))


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) == 2:
        main(sys.argv[1])
    else:
        sys.exit("usage: outcomes.py DIR | --selftest")
