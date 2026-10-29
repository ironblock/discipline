"""The ast-grep shim (#971, the maintainer 2026-10-29): what `ast-grep` and `sg` are on the model's PATH.

`python3 -I shim.py CAPTURES NAME ARGS...`, as the shim service's wrappers run it. It answers the help forms -- `--help`,
`<sub> --help`, `help <sub>`, `--version`, `completions <shell>` -- from CAPTURES, the real binary's outputs taken at
session start outside the sandbox (`services/shim.py`), byte for byte with their exit status. Everything else, every
search, is refused with the DRAFT line below and exit 1, and searches nothing: the demonstration's answer is never run
(`check.py`). The real binary is denied to the sandbox, reads and exec alike, so this is the only ast-grep there.
"""
import json, sys

# DRAFT, model-facing (the maintainer's ruling, 2026-10-29).
REFUSAL = "DRAFT. This is a demonstration: ast-grep searches don't run here. Give your answer through submit_answer."
SUBCOMMANDS = ("run", "scan", "test", "new", "lsp", "outline", "completions", "help")
NEW_ITEMS = ("project", "rule", "test", "util", "help")
SHELLS = ("bash", "elvish", "fish", "powershell", "zsh")


def forms():
    """Every argv (after the name) the shim answers: the help and version forms, and nothing that searches."""
    out = [[], ["--help"], ["-h"], ["--version"], ["-V"], ["help"]]
    for sub in SUBCOMMANDS:
        out += [[sub, "--help"], [sub, "-h"], ["help", sub]]
    for item in NEW_ITEMS:
        out += [["new", item, "--help"], ["new", item, "-h"]]
    out += [["completions", shell] for shell in SHELLS]
    return out


def key(name, args):
    return "\x1f".join([name] + list(args))


def answer(captures, name, args):
    """(stdout, stderr, exit status) for NAME ARGS: the capture, or the refusal."""
    got = captures.get(key(name, args))
    if got is None:
        return "", REFUSAL + "\n", 1
    return got["stdout"], got["stderr"], got["code"]


def selftest():
    caps = {key("ast-grep", ["--help"]): {"stdout": "Usage\n", "stderr": "", "code": 0},
            key("ast-grep", ["run", "--help"]): {"stdout": "run usage\n", "stderr": "", "code": 0}}
    bad = 0
    cases = [(["--help"], ("Usage\n", "", 0)), (["run", "--help"], ("run usage\n", "", 0)),
             (["run", "-p", "x", "src"], ("", REFUSAL + "\n", 1)), (["-p", "x"], ("", REFUSAL + "\n", 1)),
             (["outline", "src"], ("", REFUSAL + "\n", 1)), (["run", "--help", "-p", "x"], ("", REFUSAL + "\n", 1))]
    for args, want in cases:
        if answer(caps, "ast-grep", args) != want:
            print(f"shim: {args}: {answer(caps, 'ast-grep', args)}"); bad += 1
    if any(f and f[0] in ("run", "scan", "outline") and "--help" not in f and "-h" not in f for f in forms()):
        print("shim: a search form among the answered ones"); bad += 1
    print(f"shim: {len(cases)} argvs and {len(forms())} help forms, {bad} wrong")
    return 1 if bad else 0


if __name__ == "__main__":
    if sys.argv[1:] == ["--selftest"]:
        sys.exit(selftest())
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    out, err, code = answer(json.load(open(sys.argv[1])), sys.argv[2], sys.argv[3:])
    sys.stdout.write(out)
    sys.stderr.write(err)
    sys.exit(code)
