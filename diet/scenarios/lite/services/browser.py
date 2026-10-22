"""A browser daemon service (#942): `python3 -I browser.py start|stop|status [--vision] [--workspace DIR] --allow PORT_KEY...`, a `start`/`stop` service.

`start` starts one headless chrome-devtools daemon with its own profile under $DIET_SERVICE_DIR, its workspace DIR (else $DIET_SERVICE_DIR/workspace),
and only the run's dev servers allowed (each PORT_KEY names $DIET_PORT_<KEY>). `--vision` gives it --experimentalVision, so `click_at` sends real
input. It writes `provides.json` (`session`, the id the CLI selects it by; `socket`, the Unix socket it listens on; `workspace`) and `pids`: every
process whose command line names its own profile, which is the run's alone. `stop` stops the daemon by its session id and then ends what still names
its profile; the runner then ends any PID in `pids` still alive. `status` exits 0 while the daemon answers.
"""
import json
import os
import runpy
import subprocess
import sys

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
LIB = runpy.run_path(os.path.join(HERE, "grader", "trap_lib.py"))


def own_pids(profile):
    """The processes whose command line names this service's own profile path: no other run's can."""
    found = subprocess.run(["pgrep", "-f", "--", profile], capture_output=True, text=True).stdout.split()
    return [pid for pid in found if pid.isdigit() and int(pid) != os.getpid()]


def start(args):
    here = os.environ["DIET_SERVICE_DIR"]
    vision = "--vision" in args
    workspace = args[args.index("--workspace") + 1] if "--workspace" in args else os.path.join(here, "workspace")
    ports = [int(os.environ["DIET_PORT_%s" % args[i + 1].upper()]) for i, a in enumerate(args) if a == "--allow"]
    if not ports:
        sys.exit("browser: start names no --allow PORT_KEY")
    os.makedirs(workspace, exist_ok=True)
    daemon = LIB["Daemon"](here, "chrome", workspace, ports[0], vision=vision, extra_ports=ports[1:])
    try:
        daemon.start()
    finally:
        with open(os.path.join(here, "pids"), "w") as f:
            f.write(" ".join(own_pids(daemon.profile)))
    if not daemon.socket:
        sys.exit("browser: the daemon named no socket")
    with open(os.path.join(here, "provides.json"), "w") as f:
        json.dump({"session": daemon.sid, "socket": daemon.socket, "workspace": daemon.workspace, "profile": daemon.profile}, f)


def provided():
    with open(os.path.join(os.environ["DIET_SERVICE_DIR"], "provides.json")) as f:
        return json.load(f)


def stop():
    here = provided()
    LIB["Cdt"](here["session"]).run("stop", timeout=60)
    for pid in own_pids(here["profile"]):
        subprocess.run(["/bin/kill", "-TERM", pid], capture_output=True)


def status():
    sys.exit(0 if LIB["Cdt"](provided()["session"]).alive() else 1)


if __name__ == "__main__":
    verb, rest = (sys.argv[1], sys.argv[2:]) if len(sys.argv) > 1 else ("", [])
    if verb == "start":
        start(rest)
    elif verb == "stop" and not rest:
        stop()
    elif verb == "status" and not rest:
        status()
    else:
        sys.exit("usage: browser.py start|stop|status [--vision] [--workspace DIR] --allow PORT_KEY...")
