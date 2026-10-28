"""One tutorial pass (#971): python3 -I run_pass.py TASK --out DIR [--skills] [--notes FILE] [--floor] [--writers FILE]
[--no-notes] [-- DRIVE_ARGS...].

Writes `passes/TASK[-skills].jsonl` into DIR as a self-contained scenario: its paths made absolute, its head the
condition's head with the notes so far appended (FILE, from the last pass), and its live regimen `bare.toml`, or
`bare-floor.toml` under `--floor`. Then it runs `diet-drive scenario` on it into DIR/run, with DRIVE_ARGS passed
through (`--endpoint URL --model NAME --key-file F` for a live pass; none for a canned one). It writes DIR/pass.json
(`pass.py`'s verdict) and DIR/notes.md, the pass's notes update, or the notes it was given when it made none: the
next pass's `--notes`. The drive is `$DIET_DRIVE`, else the built `target/debug/diet-drive`. DRAFT and provisional.

`--writers FILE` is the notes asks' writer table (check.py, passed as `$DIET_AST_GREP_WRITERS`). `--no-notes` takes the
pass without its notes turn (no feedback, no notes tool): a held-out check of notes, not a lesson. pass.json gains,
per task, `failure` (check.py's kind, re-derived from the log's own calls and the pass's truth) and `writer`.
"""
import importlib, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, HERE)
verdict_of = importlib.import_module("pass")  # `pass` is a keyword, so by name
from docs_reads import installed_prefix  # noqa: E402

# DRAFT, model-facing: what heads the notes in the next pass's preamble.
NOTES_HEADING = "DRAFT. Your notes on ast-grep, from earlier demonstrations:"


def scenario(task, skills, notes, floor, out, no_notes=False):
    src = os.path.join(HERE, "passes", task + ("-skills" if skills else "") + ".jsonl")
    lines = [json.loads(l) for l in open(src)]
    head = lines[0]
    passes = os.path.join(HERE, "passes")
    absolute = lambda rel: os.path.normpath(os.path.join(passes, rel))
    text = open(absolute(head["head"])).read()
    if notes and notes.strip():
        text = text.rstrip("\n") + "\n\n" + NOTES_HEADING + "\n" + notes.strip() + "\n"
    with open(os.path.join(out, "head.md"), "w") as f:
        f.write(text)
    head["head"] = os.path.join(out, "head.md")
    head["regimen"] = absolute(head["regimen"])
    head["live_regimen"] = os.path.join(HERE, "bare-floor.toml" if floor else "bare.toml")
    head["answer_tool"] = [absolute(t) for t in head["answer_tool"] if not (no_notes and t.endswith("notes-tool.json"))]
    for service in head["services"]:
        for key in ("start", "stop"):
            service[key] = service[key].replace("../services/", os.path.join(HERE, "services") + "/")
    for line in lines[1:]:
        if line.get("step") == "grade":
            line["check"] = f"python3 -I {os.path.join(HERE, 'check.py')}"
            if no_notes:
                line.pop("right", None)
                line.pop("last", None)
    path = os.path.join(out, "scenario.jsonl")
    with open(path, "w") as f:
        f.write("".join(json.dumps(l, sort_keys=True) + "\n" for l in lines))
    return path


def kind_into(v, lines, task, truth_path, writers, no_notes):
    """The task's failure kind, re-derived with check.grade from the turn's own calls and the truth, and the writer
    whose ask followed the grade (by its text), into pass.json."""
    check = importlib.import_module("check")
    t = v["tasks"].get(task)
    if t is None or not os.path.exists(truth_path):
        return
    turn = next(l["turn"] for l in lines if l.get("kind") == "ask" and l.get("task") == task)
    calls = [{"name": l.get("name"), "outcome": l.get("outcome"), "arguments": l.get("arguments")}
             for l in lines if l.get("kind") == "tool_call" and l.get("turn") == turn]
    g = check.grade({"calls": calls}, task, json.load(open(truth_path)))
    t["failure"] = g["kind"]
    asks = [l.get("text", "") for l in lines if l.get("kind") == "ask"]
    table = json.load(open(writers)) if writers else None
    t["writer"] = None
    if not no_notes and len(asks) > 1:
        t["writer"] = writer_of(asks[1], table)


def _kind_read(writer):
    """A (kind, read, trials) that check.notes_ask maps to WRITER: to tell which ask the log holds."""
    return {"W0": ("right", False, 0), "W2": ("right", True, 0), "W1": ("rubric", False, 0), "W3": ("trial", False, 1),
            "W4": ("miss fields", False, 0), "W0n": ("unrecognized", False, 0), "W3n": ("unrecognized", False, 1)}[writer]


def writer_of(ask_text, table):
    """Which writer's ask ASK_TEXT holds, by check.notes_ask's own text for each: None when none does."""
    check = importlib.import_module("check")
    if table is None:  # with no table every kind gets the stub, so the writer can't be told
        return "stub" if check.STUB in ask_text else None
    for w in ("W0", "W0n", "W1", "W2", "W3", "W3n", "W4"):
        kind, read, trials = _kind_read(w)
        if check.notes_ask(kind, read, table, trials)[1] in ask_text:
            return w
    return None


def selftest():
    check = importlib.import_module("check")
    table = {"tail": "TAIL.", "W0": "w0.", "W0n": "w0n.", "W1": "w1.", "W2": "w2.", "W3": "w3.", "W3n": "w3n.", "W4": "w4."}
    bad = 0
    for w in ("W0", "W0n", "W1", "W2", "W3", "W3n", "W4"):
        kind, read, trials = _kind_read(w)
        got_w, ask = check.notes_ask(kind, read, table, trials)
        if got_w != w or writer_of(ask, table) != w:
            print(f"run_pass: writer {w}: notes_ask says {got_w}, matching says {writer_of(ask, table)}"); bad += 1
    if writer_of(check.STUB, None) != "stub" or writer_of("something else", table) is not None:
        print("run_pass: the stub, or an unknown ask, misread"); bad += 1
    print(f"run_pass: 7 writers matched from their asks, {bad} wrong")
    return 1 if bad else 0


def main(argv):
    drive_args = argv[argv.index("--") + 1:] if "--" in argv else []
    argv = argv[:argv.index("--")] if "--" in argv else argv
    if not argv or "--out" not in argv:
        sys.exit(__doc__)
    task, out = argv[0], argv[argv.index("--out") + 1]
    notes = open(argv[argv.index("--notes") + 1]).read() if "--notes" in argv else ""
    skills, floor, no_notes = "--skills" in argv, "--floor" in argv, "--no-notes" in argv
    writers = os.path.abspath(argv[argv.index("--writers") + 1]) if "--writers" in argv else None
    out = os.path.abspath(out)
    os.makedirs(out, exist_ok=False)
    path = scenario(task, skills, notes, floor, out, no_notes)
    drive = os.environ.get("DIET_DRIVE") or os.path.join(ROOT, "target", "debug", "diet-drive")
    # From the tree: the drive runs its assembler by a relative path (scripts/assemble-run.py).
    env = dict(os.environ, **({"DIET_AST_GREP_WRITERS": writers} if writers else {}))
    ran = subprocess.run([drive, "scenario", path, "--out", os.path.abspath(os.path.join(out, "run"))] + drive_args, cwd=ROOT, env=env)
    log = os.path.join(out, "run", "session.log")
    if not os.path.exists(log):
        sys.exit(f"run_pass: the drive wrote no log (exit {ran.returncode})")
    lines = [json.loads(l) for l in open(log) if l.strip()]
    v = verdict_of.verdict(lines, skills, installed_prefix())
    v["drive_exit"] = ran.returncode
    kind_into(v, lines, task, os.path.join(out, "run", "services", "fixture", "truth.json"), writers, no_notes)
    with open(os.path.join(out, "pass.json"), "w") as f:
        json.dump(v, f, indent=1)
    with open(os.path.join(out, "notes.md"), "w") as f:
        f.write((v["notes"] if v["notes"] is not None else notes).rstrip("\n") + "\n")
    print(json.dumps({"task": task, "clean": v["clean"], "why": v["why"], "notes_updated": v["notes"] is not None}))
    return 0 if ran.returncode == 0 else 1


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main(sys.argv[1:]))
