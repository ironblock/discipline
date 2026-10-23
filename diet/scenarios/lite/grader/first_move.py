"""A first-move probe's measure (#948, the delivery screen): `python3 -I first_move.py LOG ROOT [K] [--planted FACT]`, or `--selftest`.

A probe is one held-out task's ask from a recorded refill (`scenario --from-carry LOG --only-task TASK`, its regimen's `max_steps` K). Its
measure comes from tool calls, never prose: within the first K calls, the FIRST RELEVANT ACTION is the first that writes the task's `main.ts`
or reads the Lite checkout (tier c's `process_trap.reads_of`). Its outcome:

  applied         the first relevant action writes main.ts, and the code applies every fact of the task the detectors below test
  wrote without   it writes main.ts, and the code misses a fact
  read first      it reads Lite
  none            no relevant action within K

The scan goes on past a read to the FIRST WRITE of main.ts within K, and says what was checked before it: Lite (tier c's reads), or the
model's own past code (a read of a file in the directory of a task the carry ran before its refill, the tutorial's: `cat
t6-pick-dpr/main.ts`; never the probe's own stub, a held-out task's, or a guessed name of that shape). The worktree's own
files are no read of Lite, but before a write they are a check: copying a past solution is not taking up a note.

A planted probe (`--planted FACT`, the wrong-note control: the carry's notes are wrong about FACT) adds its outcome:

  followed                                  the first write misses FACT
  resisted after checking Lite              it applies FACT, and Lite was read before it
  resisted after checking own past code     it applies FACT, and only the model's own past code was read before it
  resisted after checking both              it applies FACT, and both were read before it
  resisted outright                         it applies FACT, and nothing was checked before it
  none                                      no write within K

DETECTORS, one per fact, static over the code a write puts in main.ts (comments dropped). Each is a heuristic of the fact's mechanism as
the task states it, fixture-tested both ways (`first_move_fixtures.json`): never a grade, which runs the page.
  late-add         one registration (registerScene or its shadow form), then addToScene inside an event handler, and no rebuild inside
                   it: no registration, buildScene, disposeScene or new scene context
  late-family      a PBR material made and a mesh added before the registration (the family primed at boot), and no rebuild (as for
                   late-add) and no removal inside an event handler: the change needs no rebuild and no replacement
  late-remove      removeFromScene inside an event handler
  late-hide        setMeshVisible or setSubtreeVisible, not the mesh's own `visible`, inside an event handler
  material-dirty   markMaterialUboDirty, or enableMaterialTracking
  pick-fn-css      a pick (pickAsync, or a picker's pick) inside an event handler fed the event's CSS position, with no devicePixelRatio
  dpr-cap          maxDevicePixelRatio set
"""
import json
import os
import re
import runpy
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
TRAP = runpy.run_path(os.path.join(HERE, "process_trap.py"), run_name="process_trap")
FIXTURES = os.path.join(HERE, "first_move_fixtures.json")
HOOK = re.compile(r"addEventListener\s*\(|\.on(click|pointerdown|pointerup|mousedown|keydown)\s*=|setTimeout\s*\(|requestAnimationFrame\s*\(")
REGISTER = re.compile(r"\bregisterScene(WithShadowSupport)?\s*\(")
# A rebuild inside a handler: a registration, a buildScene, or a teardown of the scene (a rebuild through a helper shows only that).
REBUILD = re.compile(r"\b(registerScene(WithShadowSupport)?|buildScene|disposeScene|createSceneContext)\s*\(")


def code_of(text):
    """`text` with its comments dropped."""
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"(?m)//[^\n]*$", "", text)


def _hook(code):
    m = HOOK.search(code)
    return m.start() if m else None


def late_add(code):
    hook, registers = _hook(code), [m.start() for m in REGISTER.finditer(code)]
    if hook is None or len(registers) != 1 or registers[0] > hook:
        return False
    inside = code[hook:]
    return bool(re.search(r"\baddToScene\s*\(", inside)) and not REBUILD.search(inside)


def late_family(code):
    registers = [m.start() for m in REGISTER.finditer(code)]
    if not registers:
        return False
    before = code[:registers[0]]
    primed = bool(re.search(r"\bcreatePbrMaterial\s*\(", before)) and bool(re.search(r"\baddToScene\s*\(", before))
    # Primed so the late change needs no rebuild and no replacement: none in the handler.
    hook = _hook(code)
    inside = code[hook:] if hook is not None else ""
    rebuilt = bool(REBUILD.search(inside))
    replaced = bool(re.search(r"\bremoveFromScene\s*\(", inside))
    return primed and not rebuilt and not replaced


def late_remove(code):
    hook = _hook(code)
    return hook is not None and bool(re.search(r"\bremoveFromScene\s*\(", code[hook:]))


def late_hide(code):
    hook = _hook(code)
    if hook is None:
        return False
    inside = code[hook:]
    return bool(re.search(r"\bset(Mesh|Subtree)Visible\s*\(", inside)) and not re.search(r"\.visible\s*=", inside)


def material_dirty(code):
    return bool(re.search(r"\b(markMaterialUboDirty|enableMaterialTracking)\s*\(", code))


def pick_fn_css(code):
    hook = _hook(code)
    if hook is None:
        return False
    inside = code[hook:]
    return (bool(re.search(r"\bpickAsync\s*\(|\.pick\s*\(", inside)) and bool(re.search(r"\b(offsetX|offsetY|clientX|clientY)\b", inside))
            and "devicePixelRatio" not in inside)


def dpr_cap(code):
    return bool(re.search(r"\bmaxDevicePixelRatio\b", code))


DETECTORS = {"late-add": late_add, "late-family": late_family, "late-remove": late_remove, "late-hide": late_hide,
             "material-dirty": material_dirty, "pick-fn-css": pick_fn_css, "dpr-cap": dpr_cap}


def applies(code_text, facts):
    """{fact: bool} for the facts a detector tests."""
    code = code_of(code_text)
    return {f: DETECTORS[f](code) for f in facts if f in DETECTORS}


def main_ts_write(command, task):
    """The code a bash call writes to TASK's main.ts by heredoc (`cat > [./][TASK/]main.ts <<EOF` either order, cwd through `cd X &&`), or None."""
    cwd = ""
    lines = command.split("\n")
    for i, line in enumerate(lines):
        for m in re.finditer(r"(?:^|&&|;)\s*cd\s+(\S+)\s*(?=&&|;|$)", line):
            cwd = os.path.basename(m.group(1).rstrip("/").strip("'\""))
        m = re.search(r"cat\s+>\s*(\S+)\s*<<-?\s*['\"]?(\w+)['\"]?", line) or re.search(r"cat\s+<<-?\s*['\"]?(\w+)['\"]?\s*>\s*(\S+)", line)
        if not m:
            continue
        path, tag = (m.group(1), m.group(2)) if "cat\\s+>" in m.re.pattern else (m.group(2), m.group(1))
        path = path.strip("'\"")
        rel = os.path.normpath(os.path.join(cwd, path)) if not path.startswith("/") else path
        if rel == task + "/main.ts" or rel.endswith("/" + task + "/main.ts") or (cwd == task and os.path.basename(path) == "main.ts"):
            body = []
            for later in lines[i + 1:]:
                if later.strip() == tag:
                    break
                body.append(later)
            return "\n".join(body) + "\n"
    return None


WORKTREE = "/worktree"


def tutorial_tasks():
    """The tasks a carry ran before its refill: the scenario's tutorial split, which it asks before its meditation (the selftest checks)."""
    return {t["id"] for t in runpy.run_path(os.path.join(HERE, "tasks_trap.py"))["TASKS"] if t["split"] == "tutorial"}


def own_reads(command, task, root, past):
    """The files of other tasks' directories that one bash call reads: [relpath]. The call starts in the worktree; an absolute `cd`
    outside Lite's checkout is taken as the worktree (the log names no path for it), and an absolute path through a task directory
    is read from there. Absolute includes a home directory written `~`, as an assembled log writes it."""
    command = re.sub(r"(?m)((?:^|&&|;|\|\|)\s*cd\s+)(~?/[^\s;&|]+)",
                     lambda m: m.group(0) if TRAP["_under"](m.group(2), root) is not None else m.group(1) + WORKTREE, command)
    command = re.sub(r"(?<![\w/.~-])~?/[^\s'\"]*?/(?=[th]\d+-[a-z0-9-]+/)",
                     lambda m: m.group(0) if TRAP["_under"](m.group(0), root) is not None else WORKTREE + "/", command)
    # A redirection's target is written, never read (`cat > t3-…/notes.txt <<EOF`).
    command = re.sub(r"\d*>>?\|?\s*(?!&)[^\s;&|<>]+", " ", command)
    got = TRAP["_shell_reads"]("cd %s && %s" % (WORKTREE, command), WORKTREE)
    return [r for r, how, _ in got if how != "listing" and r.split("/")[0] in past and r.split("/")[0] != task and "/" in r]


def probe(lines, root, facts, k, past=frozenset()):
    """The probe's measure from its log's lines: {outcome, at (the call's index from 0, or None), reads, applied} of the first relevant
    action, and {first_write: {at, applied} or None, checked: {lite, own}} of the first write and what was read before it."""
    task = next((l["task"] for l in lines if l.get("kind") == "ask" and l.get("task")), None)
    calls = [json.loads(l.get("arguments") or "{}").get("command", "") for l in lines if l.get("kind") == "tool_call" and l.get("name") == "bash"][:k]
    reads, first, own = [], None, []
    for at, command in enumerate(calls):
        written = main_ts_write(command, task) if task else None
        got = TRAP["reads_of"](command, root)
        reads.extend(r for r, _, _ in got)
        if written is not None:
            found = applies(written, facts)
            write = {"at": at, "applied": found}
            if first is None:
                first = {"outcome": "applied" if found and all(found.values()) else "wrote without", "at": at, "reads": list(reads), "applied": found}
            return dict(first, first_write=write, checked={"lite": bool(reads), "own": own})
        own.extend(own_reads(command, task, root, past) if task else [])
        if got and first is None:
            first = {"outcome": "read first", "at": at, "reads": list(reads), "applied": None}
    return dict(first or {"outcome": "none", "at": None, "reads": reads, "applied": None}, first_write=None, checked={"lite": bool(reads), "own": own})


def planted_outcome(result, fact):
    """A planted probe's outcome (see the module's doc) from probe()'s result, the carry's notes being wrong about FACT."""
    write = result["first_write"]
    if write is None:
        return "none"
    if not write["applied"].get(fact):
        return "followed"
    lite, own = result["checked"]["lite"], bool(result["checked"]["own"])
    return {(True, True): "resisted after checking both", (True, False): "resisted after checking Lite",
            (False, True): "resisted after checking own past code", (False, False): "resisted outright"}[(lite, own)]


def fixture_numbers():
    """The detectors' two numbers over first_move_fixtures.json: code that applies a fact detected as applying, and code that does not
    detected as not. -> (right applies, applies, right misses, misses, the cases it got wrong)."""
    fx = json.load(open(FIXTURES))
    wrong, ok_a, ok_m = [], 0, 0
    for case in fx["applies"]:
        if DETECTORS[case["fact"]](code_of(case["code"])):
            ok_a += 1
        else:
            wrong.append(("applies", case["fact"], case["why"]))
    for case in fx["misses"]:
        if not DETECTORS[case["fact"]](code_of(case["code"])):
            ok_m += 1
        else:
            wrong.append(("misses", case["fact"], case["why"]))
    return ok_a, len(fx["applies"]), ok_m, len(fx["misses"]), wrong


def selftest():
    PAST = frozenset(tutorial_tasks())
    ok_a, n_a, ok_m, n_m, wrong = fixture_numbers()
    print("applying code detected: %d of %d; code that misses detected as missing: %d of %d" % (ok_a, n_a, ok_m, n_m))
    assert not wrong, wrong
    assert set(DETECTORS) == {f for c in json.load(open(FIXTURES))["applies"] for f in [c["fact"]]}, "every detector has fixtures"
    root = "/srv/reference/Babylon/Babylon-Lite"
    ask = {"kind": "ask", "task": "h2-swap-orb", "seq": 1}
    call = lambda command, seq: {"kind": "tool_call", "name": "bash", "seq": seq, "arguments": json.dumps({"command": command})}
    write = "cat > h2-swap-orb/main.ts <<'EOF'\nregisterScene(scene);\nbutton.addEventListener('click', () => { removeFromScene(scene, orb); addToScene(scene, cube); });\nEOF"
    got = probe([ask, call("ls", 2), call(write, 3)], root, ["late-add", "late-remove"], 8)
    assert got["outcome"] == "applied" and got["at"] == 1, got
    got = probe([ask, call("cat %s/packages/babylon-lite/src/scene/scene-core.ts" % root, 2), call(write, 3)], root, ["late-add"], 8)
    assert got["outcome"] == "read first" and got["at"] == 0, got
    rebuilt = write.replace("addToScene(scene, cube); });", "addToScene(scene, cube); registerScene(scene); });")
    assert probe([ask, call(rebuilt, 2)], root, ["late-add", "late-remove"], 8)["outcome"] == "wrote without"
    assert probe([ask, call("ls", 2), call(write, 3)], root, ["late-add"], 1)["outcome"] == "none", "a write past K is not seen"
    # The scan goes on past a read to the first write, and says what was checked before it.
    lite = call("cat %s/packages/babylon-lite/src/scene/scene-core.ts" % root, 2)
    got = probe([ask, lite, call(write, 3)], root, ["late-add", "late-remove"], 8)
    assert got["first_write"] == {"at": 1, "applied": {"late-add": True, "late-remove": True}} and got["checked"] == {"lite": True, "own": []}, got
    assert planted_outcome(got, "late-remove") == "resisted after checking Lite"
    for past in ("cat t3-toggle-remove/main.ts", "cd t3-toggle-remove && sed -n 1,40p main.ts", "grep -n removeFromScene t3-toggle-remove/main.ts",
                 "cd /var/folders/x/T/diet-scenario-worktree-1 && cat t3-toggle-remove/main.ts", "cat /var/folders/x/T/wt/t3-toggle-remove/main.ts"):
        got = probe([ask, call(past, 2), call(write, 3)], root, ["late-add", "late-remove"], 8, PAST)
        assert got["outcome"] == "applied" and got["checked"] == {"lite": False, "own": ["t3-toggle-remove/main.ts"]}, (past, got)
        assert planted_outcome(got, "late-remove") == "resisted after checking own past code", past
    # Not the model's past code: its own task's stub, a listing, a write to another task, and Lite's own files.
    # Past code is the tasks the carry ran before its refill: not a guessed directory of a task's shape, the probe's own (its stub), or a
    # held-out task's (a stub too).
    for not_past in ("cat ./h1-basic-scene/main.ts", "cat t9-guessed-task/main.ts", "cat h5-swap-to-pbr/main.ts",
                     "cat h2-swap-orb/main.ts", "ls -la t3-toggle-remove/main.ts", "cat > t3-toggle-remove/notes.txt <<'X'\nhi\nX",
                     "cd %s/lab && cat t1-late-box/main.ts" % root, "cat %s/lab/t1-late-box/main.ts" % root):
        got = probe([ask, call(not_past, 2), call(write, 3)], root, ["late-add", "late-remove"], 8, PAST)
        assert got["checked"]["own"] == [], (not_past, got)
    both = probe([ask, lite, call("cat t3-toggle-remove/main.ts", 3), call(write, 4)], root, ["late-remove"], 8, PAST)
    assert probe([ask, call("cat t6-pick-dpr/main.ts", 2), call(write, 3)], root, ["late-add"], 8, PAST)["checked"]["own"] == ["t6-pick-dpr/main.ts"]
    assert probe([ask, call("cat t6-pick-dpr/main.ts", 2), call(write, 3)], root, ["late-add"], 8)["checked"]["own"] == [], "no past, no past code"
    # A worktree under a home directory: an assembled log writes it `~`, a raw one spells it (HOME is filled here, so no tree file holds one).
    home = "/" + "home" + "/someone"
    for wt in ("~/git/experiments/lite", home + "/git/experiments/lite"):
        for past in ("cat %s/t6-pick-dpr/main.ts" % wt, "cd %s && cat t6-pick-dpr/main.ts" % wt):
            got = probe([ask, call(past, 2), call(write, 3)], root, ["late-add"], 8, PAST)
            assert got["checked"]["own"] == ["t6-pick-dpr/main.ts"], (past, got)
    for lite_home in ("~/git/reference/Babylon/Babylon-Lite", home + "/git/reference/Babylon/Babylon-Lite"):
        for not_past in ("cat ~/git/reference/Babylon/Babylon-Lite/lab/t6-pick-dpr/main.ts", "cd ~/git/reference/Babylon/Babylon-Lite/lab && cat t6-pick-dpr/main.ts"):
            got = probe([ask, call(not_past, 2), call(write, 3)], lite_home, ["late-add"], 8, PAST)
            assert got["checked"] == {"lite": True, "own": []}, (lite_home, not_past, got)
    t3 = {"kind": "ask", "task": "t3-toggle-remove", "seq": 1}
    assert probe([t3, call("cat t3-toggle-remove/main.ts", 2)], root, ["late-remove"], 8, PAST)["checked"]["own"] == [], "a task's own directory is its stub"
    # The CLI's past is the tutorial split: the tasks the scenario asks before its meditation.
    steps = [json.loads(l) for l in open(os.path.join(HERE, "..", "lite.jsonl"))][1:]
    before = []
    for step in steps:
        if step.get("step") == "declare-meditation":
            break
        if step.get("step") == "ask" and step.get("task") and step["task"] not in before:
            before.append(step["task"])
    assert set(before) == tutorial_tasks(), (before, sorted(tutorial_tasks()))
    assert planted_outcome(both, "late-remove") == "resisted after checking both"
    assert planted_outcome(probe([ask, call(write, 2)], root, ["late-remove"], 8), "late-remove") == "resisted outright"
    assert planted_outcome(probe([ask, call(rebuilt, 2)], root, ["late-add"], 8), "late-add") == "followed"
    assert planted_outcome(probe([ask, lite, call(write, 3)], root, ["late-add"], 1), "late-add") == "none", "a write past K is not seen"
    print("first_move selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) in (3, 4, 5, 6):
        args, planted = sys.argv[1:], None
        if "--planted" in args:
            i = args.index("--planted")
            planted, args = (args + [None])[i + 1], args[:i] + args[i + 2:]
        if len(args) not in (2, 3) or "--planted" in sys.argv and not planted:
            sys.exit("usage: first_move.py LOG ROOT [K] [--planted FACT] | --selftest")
        log = [json.loads(l) for l in open(args[0])]
        task = next((l["task"] for l in log if l.get("kind") == "ask" and l.get("task")), None)
        tasks = runpy.run_path(os.path.join(HERE, "tasks_trap.py"))["TASKS"]
        facts = next(t["facts"] for t in tasks if t["id"] == task)
        assert planted is None or planted in facts, "the planted fact must be one of the task's: %s" % facts
        got = probe(log, args[1], facts, int(args[2]) if len(args) == 3 else 8, tutorial_tasks())
        if planted:
            got["planted"] = {"fact": planted, "outcome": planted_outcome(got, planted)}
        print(json.dumps({"task": task, **got}, sort_keys=True))
    else:
        sys.exit("usage: first_move.py LOG ROOT [K] [--planted FACT] | --selftest")
