"""The `fixture` service (#971): `python3 -I fixture.py start|stop`. Each run's fresh instance of the tasks.

`start` writes a `gen.py` instance into the run's worktree, `$DIET_WORKTREE`, and its truth into its own directory,
outside the worktree. Its seed is `$DIET_AST_GREP_SEED` when set (a canned rehearsal pins one,
so its canned answers are the instance's), else drawn fresh: every pass a new instance. It provides `truth` (the
check reads it as `$DIET_SERVICE_FIXTURE_TRUTH`), `pristine`, a copy of the instance the check runs the answer against, `dir`, its directory, which the scenarios deny the model's sandbox
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
    # The check runs the model's command against this copy, never the worktree the model could change.
    pristine = os.path.join(here, "pristine")
    gen.write(seed, pristine, os.path.join(here, "truth-again.json"))
    open(os.path.join(here, "pids"), "w").close()
    with open(os.path.join(here, "provides.json"), "w") as f:
        json.dump({"truth": truth_path, "pristine": pristine, "dir": here, "seed": str(truth["seed"]), **{k: str(v).lower() if isinstance(v, bool) else v for k, v in truth["params"].items()}}, f)  # a service provides text


if __name__ == "__main__":
    if sys.argv[1:] == ["start"]:
        start()
    elif sys.argv[1:] != ["stop"]:
        sys.exit("usage: fixture.py start|stop")
