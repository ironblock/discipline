"""A dev server service (#942): `exec python3 -I vite.py model|grader`, a `run` service, so the runner holds its process group.

It becomes Lite's vite (exec, so the process the runner stops is vite itself), rooted at the run's worktree ($DIET_WORKTREE) with Lite's source
($DIET_SERVICE_LITE_ROOT) aliased in read-only, on $DIET_PORT_MODEL_VITE or $DIET_PORT_GRADER_VITE. The `grader` flavour sets LITE_TRACE=1: the
lifecycle-recording wrappers (vite/lite-trace-plugin.mjs), which the model's server never carries. Its cache is the service's own directory.
In a sealed session ($DIET_SEALED, #948) the `model` flavour serves the sealed bundle ($DIET_SERVICE_LITE_BUNDLE) in the source's place, and allows
the worktree and the bundle alone: no path of the checkout. The grader's server is unchanged.
"""
import os
import sys

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def main(flavour):
    root = os.environ["DIET_SERVICE_LITE_ROOT"]
    cache = os.path.join(os.environ["DIET_SERVICE_DIR"], "cache")
    os.makedirs(cache, exist_ok=True)
    env = {
        "PATH": os.environ.get("PATH", "/usr/local/bin:/usr/bin:/bin"),
        "HOME": os.path.expanduser("~"),
        "LITE_REPO": root,
        "LITE_SRC": os.path.join(root, "packages", "babylon-lite", "src"),
        "VITE_ROOT": os.path.realpath(os.environ["DIET_WORKTREE"]),
        "VITE_CACHE": cache,
        "VITE_PORT": os.environ["DIET_PORT_%s_VITE" % flavour.upper()],
    }
    if flavour == "grader":
        env["LITE_TRACE"] = "1"
    elif os.environ.get("DIET_SEALED"):
        env["LITE_BUNDLE"] = os.environ["DIET_SERVICE_LITE_BUNDLE"]
    vite = os.path.join(root, "node_modules", ".bin", "vite")
    os.chdir(HERE)
    os.execve(vite, [vite, "--config", os.path.join(HERE, "vite", "vite.config.mjs")], env)


if __name__ == "__main__":
    if sys.argv[1:] not in (["model"], ["grader"]):
        sys.exit("usage: vite.py model|grader")
    main(sys.argv[1])
