#!/usr/bin/env python3
"""Score a judge batch's verdicts against the withheld key. Exit 0 when every control in
the batch is keyed; exit 1 naming the first miss (the batch is void); exit 2 when the
batch carried no control or a verdict is malformed (a batch with nothing to fail on
proves nothing).   usage: score.py KEY.json batch-NN.json verdicts-NN.json"""
import json, sys
batch = json.load(open(sys.argv[2])); verdicts = json.load(open(sys.argv[3]))
key = json.load(open(sys.argv[1]))[str(batch['batch'])]
ids = [r['id'] for r in batch['pairs']]
if [v.get('id') for v in verdicts] != ids: print("score: verdict ids do not match the batch, in order"); sys.exit(2)
allowed = {'supersedes', 'mentions', 'unrelated'}
bad = [v['id'] for v in verdicts if v.get('verdict') not in allowed]
if bad: print(f"score: malformed verdict on {bad[0]}"); sys.exit(2)
controls = [v for v in verdicts if v['id'] in key]
if not controls: print("score: batch carried no control"); sys.exit(2)
for v in controls:
    if v['verdict'] != key[v['id']]:
        print(f"score: MISS {v['id']} keyed {key[v['id']]} judged {v['verdict']} -- batch void"); sys.exit(1)
print(f"score: {len(controls)}/{len(controls)} controls keyed; {len(verdicts)-len(controls)} rows labelled")
