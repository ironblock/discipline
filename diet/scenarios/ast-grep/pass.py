"""A tutorial pass's verdict and its par instruments (#971): python3 -I pass.py RUN_DIR [--skills] [--docs-read par|fail],
or --selftest.

Reads the run's `session.log`. Per task:
- `right`, from its `grade` line;
- the par instruments, recorded only (par values come from data later): `seconds` from the ask to the first
  well-formed answer call, `tool_calls` before it, and `completion_tokens_before_answer` and
  `reasoning_chars_before_answer` over the responses before it;
- `output_tokens`, every response of the task's turn, and `capped`, the responses cut at their cap
  (`finish_reason` `length`);
- `trial_searches`: ast-grep searches against the project before the answer, apart from help and docs reads,
  recorded only (whether trial is allowed now that the answer never runs is the maintainer's open question);
- `requests`, each response's `prompt_tokens` and `cached_tokens`: cache state moves the seconds;
- `docs_reads` (`docs_reads.py`), the official skills' root taken as `.agent-skills` under the call's working
  directory under `--skills`, and the tool's install prefix on this machine; `commands` and `tool_used`
  (`ast-grep`, `other` or `none`: descriptive only).
Then the pass's `notes`, the last well-formed `notes` call's text, which `run_pass.py` carries into the next head.

A pass is clean when every task is right and none was capped. A docs read costs against par by default, reported and
never failing (`--docs-read par`, Dispatch's reading, unconfirmed); `--docs-read fail` makes one fail the pass.
DRAFT and provisional, as the tutorial is.
"""
import json, os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from docs_reads import TOOLS, installed_prefix, reads_of, simple_commands, strip_wrappers  # noqa: E402

ANSWER = json.load(open(os.path.join(HERE, "answer-tool.json")))["name"]
# The first live passes (2026-10-27) answered through `answer`, before it was `submit_answer`: read too.
ANSWERS = {ANSWER, "answer"}
NOTES = json.load(open(os.path.join(HERE, "notes-tool.json")))["name"]


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
                helped = any(w in {"-h", "--help", "-V", "--version"} for w in words[1:])
                searched = not helped and (sub in {"run", "outline", "scan"} or sub.startswith("-"))
                name = "ast-grep" if searched else "ast-grep " + (sub or "(bare)")
            names.append(name)
    return names


def used(names):
    return "ast-grep" if "ast-grep" in names else "other" if names else "none"


def args_of(line):
    try:
        args = json.loads(line.get("arguments") or "")
    except ValueError:
        return None
    return args if isinstance(args, dict) else None


def verdict(lines, skills=False, installed=None, docs_read="par"):
    tasks, turn_task, notes = {}, {}, None
    for l in lines:
        if l.get("kind") == "ask" and l.get("task"):
            turn_task[l["turn"]] = l["task"]
            tasks.setdefault(l["task"], {"right": False, "graded": False, "asked_t": l.get("t", 0), "answered_t": None,
                                         "tool_calls": 0, "completion_tokens_before_answer": 0, "reasoning_chars_before_answer": 0,
                                         "output_tokens": 0, "capped": 0, "docs_reads": [], "commands": [], "trial_searches": 0,
                                         "requests": []})
    # A response names its request, not its turn: the request does.
    req_turn = {l.get("seq"): l.get("turn") for l in lines if l.get("kind") == "request"}
    for l in lines:
        kind = l.get("kind")
        turn = req_turn.get(l.get("to_request"), l.get("turn")) if kind == "response" else l.get("turn")
        task = tasks.get(turn_task.get(turn))
        if kind == "grade" and l.get("task") in tasks:
            tasks[l["task"]].update(right=bool(l["right"]), graded=True)
        if kind == "tool_call" and l.get("name") == NOTES and l.get("outcome") != "refused":
            a = args_of(l)
            if a and isinstance(a.get("notes"), str):
                notes = a["notes"]
        if task is None:
            continue
        before = task["answered_t"] is None
        if kind == "response":
            usage = l.get("usage") or {}
            used_tokens = usage.get("completion_tokens", 0)
            # Cache state moves the seconds, so each request's prompt and the part of it served from cache.
            task["requests"].append({"prompt_tokens": usage.get("prompt_tokens", 0), "cached_tokens": usage.get("cached_tokens", 0)})
            task["output_tokens"] += used_tokens
            task["capped"] += l.get("finish_reason") == "length"
            if before:
                task["completion_tokens_before_answer"] += used_tokens
                task["reasoning_chars_before_answer"] += len(l.get("reasoning") or "")
        if kind == "tool_call":
            if l.get("name") in ANSWERS and l.get("outcome") != "refused" and before:
                task["answered_t"] = l.get("t", 0)
                continue
            if before:
                task["tool_calls"] += 1
            if l.get("name") == "bash":
                a = args_of(l) or {}
                cmd, cwd = a.get("command", ""), l.get("cwd") or "/"
                task["docs_reads"] += reads_of(cmd, os.path.join(cwd, ".agent-skills") if skills else None, cwd=cwd,
                                               home=os.path.expanduser("~"), installed=installed)
                ran = tools_of(cmd)
                task["commands"] += [n for n in ran if n not in task["commands"]]
                if before:  # a search against the project before the answer: trial, apart from help and docs reads
                    task["trial_searches"] += ran.count("ast-grep")
    why = []
    for t, v in tasks.items():
        v["seconds"] = round((v.pop("answered_t") - v["asked_t"]) / 1000, 1) if v.get("answered_t") is not None else None
        v.pop("asked_t")
        v["tool_used"] = used(v["commands"])
        if not v["graded"]:
            why.append(f"{t}: not graded")
        elif not v["right"]:
            why.append(f"{t}: wrong")
        if v["capped"]:
            why.append(f"{t}: capped ({v['capped']})")
        if v["docs_reads"] and docs_read == "fail":
            why.append(f"{t}: read the docs ({len(v['docs_reads'])})")
    return {"tasks": tasks, "notes": notes, "docs_read": docs_read, "clean": bool(tasks) and not why, "why": why}


def selftest():
    ask = lambda task, t=0: {"kind": "ask", "turn": 1, "task": task, "t": t}
    request = {"kind": "request", "seq": 1, "turn": 1}
    grade = lambda task, right: {"kind": "grade", "task": task, "right": right}
    bash = lambda cmd, t=0: {"kind": "tool_call", "name": "bash", "turn": 1, "t": t, "cwd": "/w", "arguments": json.dumps({"command": cmd})}
    answer = lambda cmd, t=0, outcome="ran": {"kind": "tool_call", "name": ANSWER, "turn": 1, "t": t, "outcome": outcome, "arguments": json.dumps({"command": cmd})}
    resp = lambda n, reasoning="", finish="tool_calls": {"kind": "response", "to_request": 1, "usage": {"completion_tokens": n}, "reasoning": reasoning, "finish_reason": finish}
    note = lambda text: {"kind": "tool_call", "name": NOTES, "turn": 2, "outcome": "ran", "arguments": json.dumps({"notes": text})}
    bad = 0
    full = [request, ask("a", 1000), resp(10, "think"), bash("ast-grep run --help", 2000), bash("ast-grep run -p 'x' src", 3000), resp(20, "more"), answer("ast-grep run -p x src", 5500),
            resp(5), grade("a", True), note("use : $RET")]
    v = verdict(full)
    a = v["tasks"]["a"]
    want = {"seconds": 4.5, "tool_calls": 2, "trial_searches": 1, "completion_tokens_before_answer": 30, "reasoning_chars_before_answer": 9, "output_tokens": 35,
            "capped": 0, "tool_used": "ast-grep"}
    for k, w in want.items():
        if a[k] != w:
            print(f"pass: instrument {k}: {a[k]}, expected {w}"); bad += 1
    if [r["cached_tokens"] for r in a["requests"]] != [0, 0, 0]:
        print(f"pass: requests {a['requests']}"); bad += 1
    if v["notes"] != "use : $RET":
        print(f"pass: notes {v['notes']!r}"); bad += 1
    cases = [
        ("right, with a docs read, costs against par", full, "par", True),
        ("the same under --docs-read fail", full, "fail", False),
        ("wrong", [ask("a"), answer("x"), grade("a", False)], "par", False),
        ("never graded", [ask("a"), answer("x")], "par", False),
        ("capped", [request, ask("a"), resp(100, finish="length"), answer("x"), grade("a", True)], "par", False),
        ("a refused answer starts no clock", [ask("a"), answer("x", 100, "refused"), grade("a", False)], "par", False),
        ("no tasks", [], "par", False),
    ]
    for why, lines, mode, clean in cases:
        if verdict(lines, docs_read=mode)["clean"] != clean:
            print(f"pass: {why}: clean {not clean}, expected {clean}"); bad += 1
    if "capped" not in " ".join(verdict(cases[4][1])["why"]):
        print("pass: a capped task's why does not say capped"); bad += 1
    for why, cmds, want_used in [("ast-grep run", ["ast-grep run -p 'x' src"], "ast-grep"), ("sg outline after cd", ["cd src && sg outline ."], "ast-grep"),
                                 ("rg and a node script only", ["rg -n foo src", "node -e 'require(1)'"], "other"),
                                 ("help alone is no use of the tool", ["ast-grep --help"], "other"),
                                 ("a subcommand's help is no use either", ["ast-grep run --help"], "other"), ("no command", [], "none")]:
        got = verdict([ask("a")] + [bash(c) for c in cmds] + [grade("a", True)])["tasks"]["a"]["tool_used"]
        if got != want_used:
            print(f"pass: tool_used, {why}: {got}, expected {want_used}"); bad += 1
    print(f"pass: {len(want)} instruments, {len(cases)} verdicts and 6 tool_used cases, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        sys.exit(selftest())
    if not sys.argv[1:2]:
        sys.exit(__doc__)
    mode = sys.argv[sys.argv.index("--docs-read") + 1] if "--docs-read" in sys.argv else "par"
    if mode not in ("par", "fail"):
        sys.exit("pass: --docs-read is par or fail")
    log = [json.loads(l) for l in open(os.path.join(sys.argv[1], "session.log")) if l.strip()]
    print(json.dumps(verdict(log, "--skills" in sys.argv[2:], installed_prefix(), mode), indent=1))
