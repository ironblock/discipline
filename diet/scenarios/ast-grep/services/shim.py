"""The `shim` service (#971, the maintainer 2026-10-29): `python3 -I shim.py start|stop`.

`start`, outside the sandbox, runs the real `ast-grep` and `sg` (found on the operator's PATH) for every help form the
shim answers (`../shim.py`'s `forms`), and writes their outputs to `captures.json` in its own directory, which the model
may read. It writes `bin/ast-grep` and `bin/sg`, wrappers that run the shim on those captures. It provides:
- `bin`, the wrappers' directory;
- `path`, `bin` ahead of the operator's PATH, which the scenarios give serve as `PATH` (`serve_env`);
- `real`, the real binaries' resolved paths, colon-separated, which the scenarios deny the model's sandbox
  (`sandbox_secrets`: no read, no exec), so the shim is the only ast-grep there.
It leaves no process, so `pids` is empty. `stop` does nothing.
"""
import json, os, shutil, subprocess, sys

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, HERE)
import shim  # noqa: E402


def start():
    here = os.environ["DIET_SERVICE_DIR"]
    real = {name: shutil.which(name) for name in ("ast-grep", "sg")}
    if not real["ast-grep"]:
        sys.exit("shim: no ast-grep on PATH to capture")
    captures = {}
    for name, path in real.items():
        if not path:
            continue
        for args in shim.forms():
            ran = subprocess.run([path] + args, capture_output=True, text=True, timeout=30, stdin=subprocess.DEVNULL)
            captures[shim.key(name, args)] = {"stdout": ran.stdout, "stderr": ran.stderr, "code": ran.returncode}
    with open(os.path.join(here, "captures.json"), "w") as f:
        json.dump(captures, f)
    bindir = os.path.join(here, "bin")
    os.makedirs(bindir, exist_ok=True)
    python = shutil.which("python3")
    for name in real:
        wrapper = os.path.join(bindir, name)
        with open(wrapper, "w") as f:
            f.write(f'#!/bin/sh\nexec "{python}" -I "{os.path.join(HERE, "shim.py")}" "{os.path.join(here, "captures.json")}" {name} "$@"\n')
        os.chmod(wrapper, 0o755)
    resolved = sorted({os.path.realpath(path) for path in real.values() if path})
    open(os.path.join(here, "pids"), "w").close()
    with open(os.path.join(here, "provides.json"), "w") as f:
        json.dump({"bin": bindir, "path": bindir + ":" + os.environ.get("PATH", ""), "real": ":".join(resolved)}, f)


if __name__ == "__main__":
    if sys.argv[1:] == ["start"]:
        start()
    elif sys.argv[1:] != ["stop"]:
        sys.exit("usage: shim.py start|stop")
