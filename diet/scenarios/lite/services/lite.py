"""The `lite` service (#942): `python3 -I lite.py start|stop`. The Babylon Lite checkout every run reads, outside the tree, checked at the pinned commit.

`start` refuses (exit 1) unless the checkout at $LITE_REPO, else ~/git/reference/Babylon/Babylon-Lite, is a git checkout at PINNED with no change to a
tracked file and has its vite installed; then it writes `provides.json` with `root` (what the prompts name) and `commit`, and the seal's parts (#948):
`docs`, the checkout's docs directory, and `sealed_source`, every other top-level entry of the checkout (`.git` among them, which holds every
source object), colon-separated absolute paths. In a sealed session ($DIET_SEALED names the parts) it also builds the sealed bundle into its own
directory (bundle.py) and provides `bundle`, the built directory model-vite serves in the source's place. It leaves no process, so `pids` is empty.
`stop` does nothing.
"""
import json
import os
import runpy
import subprocess
import sys

PINNED = "a9cfda1b0940e1234b6a29773dada678607c7412"


def start():
    here = os.environ["DIET_SERVICE_DIR"]
    root = os.environ.get("LITE_REPO") or os.path.join(os.path.expanduser("~"), "git", "reference", "Babylon", "Babylon-Lite")
    git = lambda *args: subprocess.run(["git", "-C", root, *args], capture_output=True, text=True)
    head = git("rev-parse", "HEAD")
    if head.returncode != 0:
        sys.exit("lite: %s is not a git checkout: %s" % (root, head.stderr.strip()))
    if head.stdout.strip() != PINNED:
        sys.exit("lite: %s is at %s, and the tasks were verified at %s" % (root, head.stdout.strip(), PINNED))
    changed = git("status", "--porcelain", "--untracked-files=no").stdout.strip()
    if changed:
        sys.exit("lite: %s has changes to tracked files: %s" % (root, changed.splitlines()[0]))
    if not os.access(os.path.join(root, "node_modules", ".bin", "vite"), os.X_OK):
        sys.exit("lite: %s has no node_modules/.bin/vite (install its dependencies)" % root)
    open(os.path.join(here, "pids"), "w").close()
    provides = {"root": root, "commit": PINNED, "docs": os.path.join(root, "docs"),
                "sealed_source": ":".join(os.path.join(root, entry) for entry in sorted(os.listdir(root)) if entry != "docs")}
    if os.environ.get("DIET_SEALED"):
        bundle = runpy.run_path(os.path.join(os.path.dirname(os.path.abspath(__file__)), "bundle.py"))
        out = os.path.join(here, "bundle")
        bundle["build"](root, out)
        provides["bundle"] = os.path.join(out, "dist")
    with open(os.path.join(here, "provides.json"), "w") as f:
        json.dump(provides, f)


if __name__ == "__main__":
    if sys.argv[1:] == ["start"]:
        start()
    elif sys.argv[1:] == ["stop"]:
        pass
    else:
        sys.exit("usage: lite.py start|stop")
