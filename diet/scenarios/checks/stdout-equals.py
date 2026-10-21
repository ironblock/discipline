"""A scenario `grade` check (#942) for a stdout task. Standard library only.

    python3 -I stdout-equals.py EXPECTED

Reads the turn the runner hands it (`DIET_GRADE_TURN`, see
`diet/scenarios/README.md`) and prints one verdict line,
`{"right", "expected", "got"}`. The turn is right when some `bash` call in
it ran, exited 0, and printed the expected text as its whole stdout: CRLF
read as LF, trailing whitespace stripped. A stream the log holds as its head
and tail alone (#734) is not whole, and is never equal.

`got` is the last such call's stdout; with none, the last `bash` call's
stdout, else its stderr; with no `bash` call, the turn's final answer.
"""
import json
import os
import sys


def norm(text):
    return text.replace("\r\n", "\n").rstrip()


def whole(stream):
    return len(stream["text"].encode("utf-8")) == stream["bytes"]


def verdict(turn, expected):
    bash = [call for call in turn["calls"] if call["name"] == "bash"]
    exited_0 = [call for call in bash if call["outcome"] == "ran" and call["exit"] == 0]
    for call in exited_0:
        if whole(call["stdout"]) and norm(call["stdout"]["text"]) == norm(expected):
            return True, call["stdout"]["text"]
    if exited_0:
        return False, exited_0[-1]["stdout"]["text"]
    if bash:
        last = bash[-1]
        return False, last["stdout"]["text"] or last["stderr"]["text"]
    return False, turn["answer"]


def main(argv):
    if len(argv) != 2:
        print("usage: stdout-equals.py EXPECTED", file=sys.stderr)
        return 2
    with open(os.environ["DIET_GRADE_TURN"], encoding="utf-8") as handle:
        turn = json.load(handle)
    right, got = verdict(turn, argv[1])
    print(json.dumps({"right": right, "expected": argv[1], "got": got}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
