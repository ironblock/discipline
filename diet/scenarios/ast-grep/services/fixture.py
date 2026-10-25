"""The `fixture` service (#971): `python3 -I fixture.py start|stop`. Each run's fresh instance of the tasks.

`start` writes a `gen.py` instance into the run's worktree, `$DIET_WORKTREE`, and its truth into its own directory,
outside the worktree. Its seed is `$DIET_AST_GREP_SEED` when set (a canned rehearsal pins one,
so its canned answers are the instance's), else drawn fresh: every pass a new instance. It provides `truth` (the
check reads it as `$DIET_SERVICE_FIXTURE_TRUTH`), `dir`, its directory, which the scenarios deny the model's sandbox
(`sandbox_secrets`), `seed`, and each param the asks fill (`{service.fixture.target}`).
It leaves no process, so `pids` is empty. `stop` does nothing.
"""
import json, os, secrets, sys

sys.path.insert(0, os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
import gen  # noqa: E402


def start():
    here, worktree = os.environ["DIET_SERVICE_DIR"], os.environ["DIET_WORKTREE"]
    seed = int(os.environ.get("DIET_AST_GREP_SEED") or secrets.randbelow(2 ** 31))
    truth_path = os.path.join(here, "truth.json")
    truth = gen.write(seed, worktree, truth_path)
    open(os.path.join(here, "pids"), "w").close()
    with open(os.path.join(here, "provides.json"), "w") as f:
        json.dump({"truth": truth_path, "dir": here, "seed": str(truth["seed"]), **truth["params"]}, f)


if __name__ == "__main__":
    if sys.argv[1:] == ["start"]:
        start()
    elif sys.argv[1:] != ["stop"]:
        sys.exit("usage: fixture.py start|stop")
