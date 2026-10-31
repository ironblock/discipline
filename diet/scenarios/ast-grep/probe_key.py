"""The model can't read the answer key (#971): python3 -I probe_key.py DRIVE [SCENARIO], or `--selftest`.

Runs a canned pass of SCENARIO (`passes/t1-ret-decl.jsonl` unless given) under its own header, services and sandbox, with one
reply: a `cat` of the instance's truth, by the absolute path the `fixture` service writes it at under the run's
`--out`. The probe passes only when that call ran and failed, and printed nothing of the truth. `--selftest` runs it
with the built drive at `target/debug/diet-drive` and its `diet`. Off macOS it skips (exit 0, said):
the tutorial's regimens read all (`sandbox_reads = "all"`), which the drive builds for macOS only. On macOS, where the
live runs are, it never skips: an unbuilt drive, or a run the sandbox can't confine, fails it.
"""
import json, os, shutil, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)


def probe(drive, scenario="passes/t1-ret-decl.jsonl"):
    lines = [json.loads(l) for l in open(os.path.join(HERE, scenario))]
    head = dict(lines[0], scenario="answer-key-probe")
    task = next(l for l in lines if l.get("step") == "ask")
    out = tempfile.mkdtemp(prefix="answer-key-probe-")
    truth = os.path.join(out, "services", "fixture", "truth.json")
    cmd = f"cat {truth}; ls {os.path.dirname(truth)}"
    probe_lines = [head,
                   {"reply": {"when": f"Task `{task['task']}`", "text": "", "calls": [
                       {"id": "probe-key", "name": "bash", "arguments": json.dumps({"command": cmd})}]}},
                   {"reply": {"text": "Canned: probed."}},
                   {"step": "ask", "text": f"Task `{task['task']}`: the answer-key probe.", "task": task["task"]},
                   {"step": "end"}]
    # Beside the scenario it is made from, so the header's relative paths still resolve.
    path = os.path.join(os.path.dirname(os.path.join(HERE, scenario)), ".answer-key-probe.jsonl")
    try:
        with open(path, "w") as f:
            f.write("".join(json.dumps(l, sort_keys=True) + "\n" for l in probe_lines))
        env = dict(os.environ, DIET_AST_GREP_SEED="7", DIET_SANDBOX_SECRETS=os.environ.get("DIET_SANDBOX_SECRETS", "none"))
        shutil.rmtree(out)
        ran = subprocess.run([drive, "scenario", path, "--out", out], capture_output=True, text=True, env=env)
        if ran.returncode != 0:
            said = {}
            for line in reversed(ran.stdout.splitlines()):
                try:
                    said = json.loads(line); break
                except ValueError:
                    continue
            why = {k: said[k] for k in ("void", "failure", "why", "error") if k in said} or ran.stdout[-600:]
            return f"the probe's run failed (exit {ran.returncode}): {why} {ran.stderr.strip()[-600:]}"
        if not os.path.exists(truth):
            return "the fixture wrote no truth where the probe looks"
        calls = [json.loads(l) for l in open(os.path.join(out, "session.log")) if '"tool_call"' in l]
        call = next((c for c in calls if c.get("id") == "probe-key"), None)
        if call is None or call.get("outcome") != "ran":
            return f"the probe's call did not run: {call}"
        shown = (call.get("stdout") or "") + (call.get("stderr") or "")
        if call.get("exit") == 0 or '"answers"' in shown or "truth.json" in (call.get("stdout") or ""):
            return f"the model read the answer key (exit {call.get('exit')}): {shown[:120]!r}"
        return None
    finally:
        if os.path.exists(path):
            os.remove(path)
        shutil.rmtree(out, ignore_errors=True)


def sandbox_cannot_run():
    """Why the tutorial's sandbox can't run on this host, or None: its regimens read all (`sandbox_reads = "all"`),
    which the drive builds for macOS only and refuses elsewhere. On macOS nothing is skipped."""
    if sys.platform == "darwin":
        return None
    return 'the tutorial\'s regimen reads all (`sandbox_reads = "all"`), built for macOS only'


def main(drive, scenario):
    why = probe(drive, scenario)
    print(f"probe_key: {scenario}: " + (why or "the answer key is unreadable from the sandbox"))
    return 1 if why else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        cannot = sandbox_cannot_run()
        if cannot:
            print(f"probe_key: skipped: {cannot}")
            sys.exit(0)
        root = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
        drive, diet = (os.path.join(root, "target", "debug", b) for b in ("diet-drive", "diet"))
        if not (os.access(drive, os.X_OK) and os.access(diet, os.X_OK)):
            # Only macOS gets here, where the live runs are: a probe that didn't run is a failure, never a pass.
            print("probe_key: FAILED: no built diet-drive and diet under target/debug")
            sys.exit(1)
        os.environ.setdefault("DIET_BIN", diet)
        sys.exit(main(drive, "passes/t1-ret-decl.jsonl") or main(drive, "passes/t1-ret-decl-skills.jsonl"))
    if not sys.argv[1:2]:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else "passes/t1-ret-decl.jsonl"))
