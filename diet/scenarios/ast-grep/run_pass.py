"""One tutorial pass (#971): python3 -I run_pass.py TASK --out DIR [--skills] [--notes FILE] [--floor] [-- DRIVE_ARGS...].

Writes `passes/TASK[-skills].jsonl` into DIR as a self-contained scenario: its paths made absolute, its head the
condition's head with the notes so far appended (FILE, from the last pass), and its live regimen `bare.toml`, or
`bare-floor.toml` under `--floor`. Then it runs `diet-drive scenario` on it into DIR/run, with DRIVE_ARGS passed
through (`--endpoint URL --model NAME --key-file F` for a live pass; none for a canned one). It writes DIR/pass.json
(`pass.py`'s verdict) and DIR/notes.md, the pass's notes update, or the notes it was given when it made none: the
next pass's `--notes`. The drive is `$DIET_DRIVE`, else the built `target/debug/diet-drive`. DRAFT and provisional.
"""
import importlib, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, HERE)
verdict_of = importlib.import_module("pass")  # `pass` is a keyword, so by name
from docs_reads import installed_prefix  # noqa: E402

# DRAFT, model-facing: what heads the notes in the next pass's preamble.
NOTES_HEADING = "DRAFT. Your notes on ast-grep, from earlier demonstrations:"


def scenario(task, skills, notes, floor, out):
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
    head["answer_tool"] = [absolute(t) for t in head["answer_tool"]]
    for service in head["services"]:
        for key in ("start", "stop"):
            service[key] = service[key].replace("../services/", os.path.join(HERE, "services") + "/")
    for line in lines[1:]:
        if line.get("step") == "grade":
            line["check"] = f"python3 -I {os.path.join(HERE, 'check.py')}"
    path = os.path.join(out, "scenario.jsonl")
    with open(path, "w") as f:
        f.write("".join(json.dumps(l, sort_keys=True) + "\n" for l in lines))
    return path


def main(argv):
    drive_args = argv[argv.index("--") + 1:] if "--" in argv else []
    argv = argv[:argv.index("--")] if "--" in argv else argv
    if not argv or "--out" not in argv:
        sys.exit(__doc__)
    task, out = argv[0], argv[argv.index("--out") + 1]
    notes = open(argv[argv.index("--notes") + 1]).read() if "--notes" in argv else ""
    skills, floor = "--skills" in argv, "--floor" in argv
    out = os.path.abspath(out)
    os.makedirs(out, exist_ok=False)
    path = scenario(task, skills, notes, floor, out)
    drive = os.environ.get("DIET_DRIVE") or os.path.join(ROOT, "target", "debug", "diet-drive")
    # From the tree: the drive runs its assembler by a relative path (scripts/assemble-run.py).
    ran = subprocess.run([drive, "scenario", path, "--out", os.path.abspath(os.path.join(out, "run"))] + drive_args, cwd=ROOT)
    log = os.path.join(out, "run", "session.log")
    if not os.path.exists(log):
        sys.exit(f"run_pass: the drive wrote no log (exit {ran.returncode})")
    lines = [json.loads(l) for l in open(log) if l.strip()]
    v = verdict_of.verdict(lines, skills, installed_prefix())
    v["drive_exit"] = ran.returncode
    with open(os.path.join(out, "pass.json"), "w") as f:
        json.dump(v, f, indent=1)
    with open(os.path.join(out, "notes.md"), "w") as f:
        f.write((v["notes"] if v["notes"] is not None else notes).rstrip("\n") + "\n")
    print(json.dumps({"task": task, "clean": v["clean"], "why": v["why"], "notes_updated": v["notes"] is not None}))
    return 0 if ran.returncode == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
