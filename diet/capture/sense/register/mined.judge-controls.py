#!/usr/bin/env python3
"""Score a judge's verdict file against the withheld control answers.

A verdict file is a JSON array of {"id", "mistake", "reversal", ...} rows, one
per input row of a batch, controls included. Every control the batch carried
must be answered as keyed for its own set; the first miss is named and the
batch is void (exit 1). A batch that carried no control at all is not scored
(exit 2): a batch with nothing to fail on proves nothing.

    mined.judge-controls.py VERDICTS.json [CONTROLS.json]
"""
import json, pathlib, sys

def main(argv):
    if len(argv) < 2:
        print(__doc__); return 2
    verdicts = json.load(open(argv[1]))
    key_path = pathlib.Path(argv[2]) if len(argv) > 2 else pathlib.Path(__file__).with_name("mined.controls.json")
    key = {c["id"]: c for c in json.load(open(key_path))["controls"]}
    got = {v["id"]: v for v in verdicts}
    carried = [cid for cid in got if cid in key]
    if not carried:
        print("controls: the batch carried no control, so it proves nothing"); return 2
    for cid in carried:
        c = key[cid]; answer = got[cid].get(c["set"])
        if answer != c["answer"]:
            print(f"controls: VOID: {cid} ({c['set']}) keyed {c['answer']}, judged {answer}"); return 1
    print(f"controls: {len(carried)}/{len(carried)} answered as keyed"); return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv))
