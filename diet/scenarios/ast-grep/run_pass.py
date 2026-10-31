"""One tutorial pass (#971): python3 -I run_pass.py TASK --out DIR [--skills] [--notes FILE] [--floor] [--head FILE]
[--seed N] [--verdict execute] [-- DRIVE_ARGS...]. The last three are the falsification's (PREREG.md, 2026-10-29): a
head of its own, an explicit seed, execution as the verdict.

Writes `passes/TASK[-skills].jsonl` into DIR as a self-contained scenario: its paths made absolute, its head the
condition's head with FILE's notes appended under NOTES_HEADING, and its live regimen `bare.toml`, or
`bare-floor.toml` under `--floor`. Then it runs `diet-drive scenario` on it into DIR/run, with DRIVE_ARGS passed
through (`--endpoint URL --model NAME --key-file F` for a live pass; none for a canned one). It writes DIR/pass.json
(`pass.py`'s verdict). The drive is `$DIET_DRIVE`, else the built `target/debug/diet-drive`. DRAFT and provisional.
"""
import importlib, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, HERE)
verdict_of = importlib.import_module("pass")  # `pass` is a keyword, so by name
from docs_reads import installed_prefix  # noqa: E402

# DRAFT, model-facing: what heads the notes in the preamble (the falsification's T arm). Kept verbatim while that run
# is under way; its wording is the maintainer's to change at the re-plan.
NOTES_HEADING = "DRAFT. Your notes on ast-grep, from earlier demonstrations:"


def scenario(task, skills, notes, floor, out, head_file=None):
    src = os.path.join(HERE, "passes", task + ("-skills" if skills else "") + ".jsonl")
    lines = [json.loads(l) for l in open(src)]
    head = lines[0]
    passes = os.path.join(HERE, "passes")
    absolute = lambda rel: os.path.normpath(os.path.join(passes, rel))
    text = open(head_file or absolute(head["head"])).read()
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


def selftest():
    """The written scenario stands alone: its head the given head, with notes under NOTES_HEADING only when given; its
    paths absolute and present; the floor's live regimen under --floor; the grade step this tree's check."""
    import tempfile
    bad = 0
    head = os.path.join(HERE, "head-native.md")
    for notes, floor in (("", True), ("Some notes.", True), ("", False)):
        with tempfile.TemporaryDirectory() as out:
            lines = [json.loads(l) for l in open(scenario("t1-ret-decl", False, notes, floor, out, head_file=head))]
            text, want = open(os.path.join(out, "head.md")).read(), open(head).read()
            if notes:
                want = want.rstrip("\n") + "\n\n" + NOTES_HEADING + "\n" + notes + "\n"
            if text != want:
                print(f"run_pass: notes {notes!r}: the head is {text!r}"); bad += 1
            h = lines[0]
            paths = [h["head"], h["regimen"], h["live_regimen"]] + h["answer_tool"]
            if [p for p in paths if not (os.path.isabs(p) and os.path.exists(p))]:
                print(f"run_pass: a path not absolute, or missing: {paths}"); bad += 1
            # The native condition: the fixture alone (no shim), its directory the one secret, no PATH of serve's own.
            if ([s["name"] for s in h["services"]] != ["fixture"] or h.get("sandbox_secrets") != ["service.fixture.dir"]
                    or "serve_env" in h or [os.path.basename(t) for t in h["answer_tool"]] != ["answer-tool-native.json"]):
                print(f"run_pass: not the native condition: {h}"); bad += 1
            if os.path.basename(h["live_regimen"]) != ("bare-floor.toml" if floor else "bare.toml"):
                print(f"run_pass: floor {floor}: live regimen {h['live_regimen']}"); bad += 1
            grade = next(l for l in lines if l.get("step") == "grade")
            if grade["check"] != f"python3 -I {os.path.join(HERE, 'check.py')}":
                print(f"run_pass: the grade step's check is {grade['check']}"); bad += 1
    print(f"run_pass: 3 scenarios written, {bad} wrong")
    return 1 if bad else 0


def main(argv):
    drive_args = argv[argv.index("--") + 1:] if "--" in argv else []
    argv = argv[:argv.index("--")] if "--" in argv else argv
    if not argv or "--out" not in argv:
        sys.exit(__doc__)
    task, out = argv[0], argv[argv.index("--out") + 1]
    notes = open(argv[argv.index("--notes") + 1]).read() if "--notes" in argv else ""
    skills, floor = "--skills" in argv, "--floor" in argv
    # The falsification's switches (PREREG.md, 2026-10-29): a head of its own, an explicit seed (the arms are paired),
    # and execution as the verdict.
    head_file = os.path.abspath(argv[argv.index("--head") + 1]) if "--head" in argv else None
    seed = argv[argv.index("--seed") + 1] if "--seed" in argv else None
    verdict_mode = argv[argv.index("--verdict") + 1] if "--verdict" in argv else None
    if verdict_mode not in (None, "execute"):
        sys.exit("run_pass: --verdict is execute")
    out = os.path.abspath(out)
    os.makedirs(out, exist_ok=False)
    path = scenario(task, skills, notes, floor, out, head_file)
    drive = os.environ.get("DIET_DRIVE") or os.path.join(ROOT, "target", "debug", "diet-drive")
    # From the tree: the drive runs its assembler by a relative path (scripts/assemble-run.py).
    env = dict(os.environ, **({"DIET_AST_GREP_SEED": seed} if seed else {}), **({"DIET_AST_GREP_VERDICT": verdict_mode} if verdict_mode else {}))
    ran = subprocess.run([drive, "scenario", path, "--out", os.path.abspath(os.path.join(out, "run"))] + drive_args, cwd=ROOT, env=env)
    log = os.path.join(out, "run", "session.log")
    if not os.path.exists(log):
        sys.exit(f"run_pass: the drive wrote no log (exit {ran.returncode})")
    lines = [json.loads(l) for l in open(log) if l.strip()]
    v = verdict_of.verdict(lines, skills, installed_prefix())
    v["drive_exit"] = ran.returncode
    with open(os.path.join(out, "pass.json"), "w") as f:
        json.dump(v, f, indent=1)
    print(json.dumps({"task": task, "clean": v["clean"], "why": v["why"]}))
    return 0 if ran.returncode == 0 else 1


if __name__ == "__main__":
    sys.exit(selftest() if sys.argv[1:] == ["--selftest"] else main(sys.argv[1:]))


