#!/usr/bin/env python3
"""Split a seeded fault's selftest seconds into phases (#353), without changing the tree.

Replays what verify.sh's seeded_case does for one fault, in the same order,
and times each step: the sandbox copy, the two state fingerprints, the
injection, `verify.sh --only CHECK [--scope S]` under scripts/hermetic.sh, and
the verdict. A lane fault is applied by apply-lane-faults.py, as
inject_lane_fault does; a hand-written case's inject_* function runs from
verify.sh's own definitions (everything above its `# main` banner, sourced with
ROOT pinned to the tree), in the box, as seeded_case runs it.
Inside the check, three wrappers that live outside the box log what cargo did:
a `cargo` shim first on PATH (each invocation's wall clock and cargo's own
"Finished ... in Xs"), a rustc wrapper (each crate's rustc), and a linker
wrapper (each link). They reach the box through PATH and an untracked
.cargo/config.toml written after the fingerprint, so the fault's tree is the
one seeded_case grades.

    phases.py ROOT WORK OUT.jsonl lane:LANE:ID | case:ID ...

ROOT is a checkout; WORK a scratch directory (box, target, wrappers, logs).
Each fault appends one JSON line to OUT.jsonl; seconds are wall clock.
"""
import json, os, pathlib, re, shutil, subprocess, sys, time

root, work, out = map(pathlib.Path, sys.argv[1:4])
faults = sys.argv[4:]
sys.path.insert(0, str(root / "scripts"))
import gatelib  # noqa: E402
box, bin_, logs = work / "box", work / "bin", work / "logs"
# PHASES_TARGET: a target a Cargo cache restored into, as the selftest's VERIFY_SELFTEST_TARGET is on CI.
target = pathlib.Path(os.environ.get("PHASES_TARGET") or work / "target")
for d in (target, bin_, logs):
    d.mkdir(parents=True, exist_ok=True)
real_cargo = shutil.which("cargo")
real_cc = shutil.which("cc")
triple = re.search(r"host: (\S+)", subprocess.run(["rustc", "-vV"], capture_output=True, text=True).stdout).group(1)
gnu_cp = os.environ.get("GNU_CP", "cp")
events = logs / "events.tsv"

def wrapper(name, body):
    p = bin_ / name
    p.write_text("#!/usr/bin/env bash\n" + body)
    p.chmod(0o755)
    return p

stamp = 'python3 -c "import time;print(time.time())"'
wrapper("cargo", f"""s=$({stamp})
"{real_cargo}" "$@" 2> >(tee "{logs}/cargo.$$.err" >&2); rc=$?
wait
e=$({stamp})
f=$(grep -Eo 'Finished .* in [0-9.]+(s|m [0-9.]+s)' "{logs}/cargo.$$.err" | tail -1 | grep -Eo '[0-9.]+s$' | tr -d s)
printf 'cargo\\t%s\\t%s\\t%s\\t%s\\n' "$s" "$e" "${{f:-}}" "$*" >> "{events}"
exit $rc
""")
wrapper("rustc-wrap", f"""s=$({stamp})
"$@"; rc=$?
e=$({stamp})
c=""; prev=""; for a in "$@"; do [ "$prev" = "--crate-name" ] && c="$a"; prev="$a"; done
printf 'rustc\\t%s\\t%s\\t%s\\t\\n' "$s" "$e" "$c" >> "{events}"
exit $rc
""")
wrapper("cc-wrap", f"""s=$({stamp})
"{real_cc}" "$@"; rc=$?
e=$({stamp})
printf 'link\\t%s\\t%s\\t\\t\\n' "$s" "$e" >> "{events}"
exit $rc
""")
config = f'[build]\nrustc-wrapper = "{bin_}/rustc-wrap"\n[target.{triple}]\nlinker = "{bin_}/cc-wrap"\n'

def run(cmd, cwd, **kw):
    t = time.time()
    p = subprocess.run(cmd, cwd=cwd, shell=isinstance(cmd, str), capture_output=True, text=True, **kw)
    return time.time() - t, p

def sandbox():
    shutil.rmtree(box, ignore_errors=True)
    box.mkdir(parents=True)
    files = subprocess.run(["git", "-C", str(root), "ls-files", "-z", "--cached", "--others", "--exclude-standard"],
                           capture_output=True, check=True).stdout
    subprocess.run(["xargs", "-0", gnu_cp, "-L", "--parents", "-t", str(box)], cwd=root, input=files, check=True)
    subprocess.run(["git", "-C", str(box), "init", "--quiet"], check=True)
    subprocess.run(["git", "-C", str(box), "add", "--all"], check=True)

def state():
    subprocess.run(["git", "-C", str(box), "add", "--all"], check=True, capture_output=True)
    tree = subprocess.run(["git", "-C", str(box), "write-tree"], capture_output=True, text=True, check=True).stdout.strip()
    subprocess.run(["git", "-C", str(box), "show-ref"], capture_output=True)
    return tree

signatures = {}
for line in subprocess.run(["python3", "scripts/apply-lane-faults.py", "--list"], cwd=root,
                           capture_output=True, text=True, check=True).stdout.splitlines():
    lane, ident, sig = line.split("\t")[:3]
    signatures[ident] = sig.split("\x1f")
verify_text = (root / "verify.sh").read_text(encoding="utf-8")
cases = {f"{c.check}.{c.injection[len('inject_'):]}": c for c in gatelib.seeded_cases(verify_text)}
head = verify_text.split("\n# main\n", 1)[0]
head = re.sub(r'^ROOT=.*$', f'ROOT="{root}"', head, count=1, flags=re.M)
functions = work / "verify-functions.sh"
functions.write_text(head)

env = {"PATH": f"{bin_}:{os.environ['PATH']}", "HOME": os.environ["HOME"],
       "TMPDIR": os.environ.get("TMPDIR", "/tmp"), "CARGO_TARGET_DIR": str(target)}
for k in ("CARGO_HOME", "RUSTUP_HOME", "DIET_REQUIRE_SANDBOX", "LANG"):
    if k in os.environ:
        env[k] = os.environ[k]

with out.open("a") as fh:
    for spec in faults:
        if spec.startswith("lane:"):
            _, lane, ident = spec.split(":", 2)
            check, scope, sigs = "lanes", None, signatures[ident]
            inject = ["python3", "scripts/apply-lane-faults.py", "--apply-only", lane, ident]
        else:
            ident = spec.split(":", 1)[1]
            c = cases[ident]
            lane, check, scope, sigs = None, c.check, c.scope, [c.signature]
            inject = ["bash", "-c", f'source "{functions}" && {c.injection}']
        r = {"lane": lane, "id": ident, "check": check, "scope": scope}
        t = time.time(); sandbox(); r["copy"] = time.time() - t
        t = time.time(); before = state(); r["state_before"] = time.time() - t
        r["inject"], p = run(inject, box)
        r["inject_rc"] = p.returncode
        t = time.time(); after = state()
        subprocess.run(["git", "-C", str(box), "diff-tree", "-r", "--name-only", before, after], capture_output=True)
        r["state_after"] = time.time() - t
        (box / ".cargo").mkdir(exist_ok=True)
        (box / ".cargo" / "config.toml").write_text(config)
        (box / "diet" / "src" / "lib.rs").touch()
        events.write_text("")
        r["check"], p = run(["bash", str(root / "scripts" / "hermetic.sh"), "env", f"CARGO_TARGET_DIR={target}",
                             "bash", "./verify.sh", "--only", check] + (["--scope", scope] if scope else []), box, env=env)
        r["check_rc"] = p.returncode
        log = p.stdout + p.stderr
        t = time.time()
        r["verdict_ok"] = p.returncode != 0 and all(re.search(s, log) for s in sigs)
        r["verdict"] = time.time() - t
        ev = [l.split("\t") for l in events.read_text().splitlines()]
        r["cargo"] = [{"wall": float(e[2]) - float(e[1]), "finished": float(e[3]) if e[3] else None, "args": e[4]}
                      for e in ev if e[0] == "cargo"]
        r["rustc"] = [{"wall": float(e[2]) - float(e[1]), "crate": e[3]} for e in ev if e[0] == "rustc"]
        r["link"] = [float(e[2]) - float(e[1]) for e in ev if e[0] == "link"]
        fh.write(json.dumps(r) + "\n"); fh.flush()
        print(f"{ident}: copy {r['copy']:.1f} inject {r['inject']:.1f} check {r['check']:.1f} rc {r['check_rc']} "
              f"verdict_ok {r['verdict_ok']} cargo {len(r['cargo'])} rustc {len(r['rustc'])} links {len(r['link'])}", flush=True)
