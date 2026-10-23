"""A first-move probe's measure (#948, the delivery screen): `python3 -I first_move.py LOG ROOT [K]`, or `--selftest`.

A probe is one held-out task's ask from a recorded refill (`scenario --from-carry LOG --only-task TASK`, its regimen's `max_steps` K). Its
measure comes from tool calls, never prose: within the first K calls, the FIRST RELEVANT ACTION is the first that writes the task's `main.ts`
or reads the Lite checkout (tier c's `process_trap.reads_of`). Its outcome:

  applied         the first relevant action writes main.ts, and the code applies every fact of the task the detectors below test
  wrote without   it writes main.ts, and the code misses a fact
  read first      it reads Lite
  none            no relevant action within K

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


def probe(lines, root, facts, k):
    """The probe's measure from its log's lines: {outcome, at (the call's index from 0, or None), reads, applied}."""
    task = next((l["task"] for l in lines if l.get("kind") == "ask" and l.get("task")), None)
    calls = [json.loads(l.get("arguments") or "{}").get("command", "") for l in lines if l.get("kind") == "tool_call" and l.get("name") == "bash"][:k]
    reads = []
    for at, command in enumerate(calls):
        written = main_ts_write(command, task) if task else None
        got = TRAP["reads_of"](command, root)
        reads.extend(r for r, _, _ in got)
        if written is not None:
            found = applies(written, facts)
            return {"outcome": "applied" if found and all(found.values()) else "wrote without", "at": at, "reads": reads, "applied": found}
        if got:
            return {"outcome": "read first", "at": at, "reads": reads, "applied": None}
    return {"outcome": "none", "at": None, "reads": reads, "applied": None}


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
    print("first_move selftest ok")


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        selftest()
    elif len(sys.argv) in (3, 4):
        log = [json.loads(l) for l in open(sys.argv[1])]
        task = next((l["task"] for l in log if l.get("kind") == "ask" and l.get("task")), None)
        tasks = runpy.run_path(os.path.join(HERE, "tasks_trap.py"))["TASKS"]
        facts = next(t["facts"] for t in tasks if t["id"] == task)
        print(json.dumps({"task": task, **probe(log, sys.argv[2], facts, int(sys.argv[3]) if len(sys.argv) == 4 else 8)}, sort_keys=True))
    else:
        sys.exit("usage: first_move.py LOG ROOT [K] | --selftest")
