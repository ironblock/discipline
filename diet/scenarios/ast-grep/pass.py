"""A tutorial pass's verdict (#971): python3 -I pass.py RUN_DIR [--skills], or --selftest.

Reads the run's `session.log`: each task's `grade` line, and each `bash` call's docs reads (`docs_reads.py`), the
official skills' root taken as `.agent-skills` under the call's working directory when `--skills` says the
condition gave them. A pass is clean when every task graded right and no call read the docs: the tutorial ends on a
clean pass, and rolls back from any other. Prints one JSON object: per task, `right`, the turn's `docs_reads`, its `commands` and `tool_used`
(`ast-grep`, `other` or `none`: descriptive only, since whether a clean pass must use ast-grep is not ruled);
then `reads_outside_tasks`, `clean` and `why`.
"""
import json, os, sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from docs_reads import TOOLS, reads_of, simple_commands, strip_wrappers  # noqa: E402


def tools_of(cmd):
    """The commands one call ran, by name: `ast-grep` for a search by the tool under either name, and `ast-grep
    <word>` for any other use of it (help, version, new)."""
    names = []
    for words in simple_commands(cmd):
        words = strip_wrappers(words)
        if words and os.path.basename(words[0]) != "cd":
            name = os.path.basename(words[0])
            if name in TOOLS:  # a search: run, outline or scan, or an option-led call, which runs `run`
                sub = words[1] if len(words) > 1 else ""
                searched = sub in {"run", "outline", "scan"} or sub.startswith("-") and sub not in {"-h", "--help", "-V", "--version"}
                name = "ast-grep" if searched else "ast-grep " + (sub or "(bare)")
            names.append(name)
    return names


def used(names):
    """Descriptive only (whether a clean pass must use ast-grep is not ruled): `ast-grep`, `other` or `none`."""
    return "ast-grep" if "ast-grep" in names else "other" if names else "none"


def verdict(lines, skills):
    tasks, turn_task, outside = {}, {}, []
    for l in lines:
        if l.get("kind") == "ask" and l.get("task"):
            turn_task[l["turn"]] = l["task"]
            tasks.setdefault(l["task"], {"right": False, "graded": False, "docs_reads": [], "commands": []})
    for l in lines:
        if l.get("kind") == "grade" and l.get("task") in tasks:
            tasks[l["task"]].update(right=bool(l["right"]), graded=True)
        if l.get("kind") == "tool_call" and l.get("name") == "bash":
            try:
                cmd = json.loads(l.get("arguments") or "{}").get("command", "")
            except ValueError:
                continue
            cwd = l.get("cwd") or "/"
            found = reads_of(cmd, os.path.join(cwd, ".agent-skills") if skills else None, cwd=cwd, home=os.path.expanduser("~"))
            task = turn_task.get(l.get("turn"))
            (tasks[task]["docs_reads"] if task else outside).extend(found)
            if task:
                tasks[task]["commands"] += [n for n in tools_of(cmd) if n not in tasks[task]["commands"]]
    for v in tasks.values():
        v["tool_used"] = used(v["commands"])
    why = [f"{t}: {'not graded' if not v['graded'] else 'wrong'}" for t, v in tasks.items() if not v["right"]]
    why += [f"{t}: read the docs ({len(v['docs_reads'])})" for t, v in tasks.items() if v["docs_reads"]]
    why += [f"read the docs outside a task ({len(outside)})"] if outside else []
    return {"tasks": tasks, "reads_outside_tasks": outside, "clean": bool(tasks) and not why, "why": why}


def selftest():
    ask = lambda turn, task: {"kind": "ask", "turn": turn, "task": task}
    grade = lambda task, right: {"kind": "grade", "task": task, "right": right}
    bash = lambda turn, cmd: {"kind": "tool_call", "name": "bash", "turn": turn, "cwd": "/w", "arguments": json.dumps({"command": cmd})}
    cases = [
        ("every task right, no docs", [ask(1, "a"), bash(1, "ast-grep run -p 'x' src"), grade("a", True), ask(2, "b"), grade("b", True)], False, True),
        ("one task wrong", [ask(1, "a"), grade("a", True), ask(2, "b"), grade("b", False)], False, False),
        ("a task never graded", [ask(1, "a"), grade("a", True), ask(2, "b")], False, False),
        ("right, but read --help", [ask(1, "a"), bash(1, "ast-grep run --help"), grade("a", True)], False, False),
        ("right, but read a skill file", [ask(1, "a"), bash(1, "cat .agent-skills/outline/SKILL.md"), grade("a", True)], True, False),
        ("a skill path with no skills condition is no read", [ask(1, "a"), bash(1, "cat .agent-skills/outline/SKILL.md"), grade("a", True)], False, True),
        ("no tasks at all", [], False, False),
    ]
    bad = 0
    for why, lines, skills, want in cases:
        got = verdict(lines, skills)["clean"]
        if got != want:
            print(f"pass: {why}: clean {got}, expected {want}"); bad += 1
    for why, cmds, want in [("ast-grep run", ["ast-grep run -p 'x' src"], "ast-grep"), ("sg outline after cd", ["cd src && sg outline ."], "ast-grep"),
                            ("rg and a node script only", ["rg -n foo src", "node -e 'require(1)'"], "other"),
                            ("grep then ast-grep", ["grep -rn x src", "ast-grep scan -r r.yml"], "ast-grep"), ("no command", [], "none"),
                            ("help alone is no use of the tool", ["ast-grep --help", "rg foo"], "other")]:
        lines = [ask(1, "a")] + [bash(1, c) for c in cmds] + [grade("a", True)]
        got = verdict(lines, False)["tasks"]["a"]["tool_used"]
        if got != want:
            print(f"pass: tool_used, {why}: {got}, expected {want}"); bad += 1
    print(f"pass: {sum(c[3] for c in cases)} clean and {sum(not c[3] for c in cases)} unclean passes, 6 tool_used cases, {bad} misjudged")
    return 1 if bad else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        sys.exit(selftest())
    if not sys.argv[1:2]:
        sys.exit(__doc__)
    log = [json.loads(l) for l in open(os.path.join(sys.argv[1], "session.log")) if l.strip()]
    print(json.dumps(verdict(log, "--skills" in sys.argv[2:]), indent=1))
