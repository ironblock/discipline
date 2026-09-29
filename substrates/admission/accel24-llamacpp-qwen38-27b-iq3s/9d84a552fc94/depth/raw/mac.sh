#!/bin/bash
# The candidate's T2 depth window (#143), Mac side: start the box script, run the depth probe at its
# marker, signal done, then after the restore: fingerprint, verify-box, canary against the pool.
set -u
W=$HOME/diet-inference-runs/t2-cand; L=$W/mac.log; OUT=$W/out
P=$HOME/git/me/discipline/main/substrates/admission/depth-probe
POOL=$HOME/diet-inference-runs/w143/window/pool10.json
BH=$(cat $HOME/diet-inference-runs/ada-142/.boxhost)
K=(-i $HOME/.ssh/id_rsa -o IdentitiesOnly=yes -o BatchMode=yes -o ConnectTimeout=15)
I=$HOME/git/me/diet-inference/instruments
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
box () { ssh ${K[@]} -n corey@$BH "$@"; }
scp -q ${K[@]} $W/d143c.sh "corey@${BH}:t2c-d143c.sh" && box 'mkdir -p ~/t2c && mv ~/t2c-d143c.sh ~/t2c/d143c.sh'; log "script copied rc=$?"
# launch detached; the ssh that starts it is itself bounded, since a launch over ssh once held its channel open
( ssh ${K[@]} -n -f corey@$BH 'cd ~/t2c && setsid nohup bash d143c.sh > d143c.out 2>&1 < /dev/null &' ) & LP=$!
for i in $(seq 1 20); do kill -0 $LP 2>/dev/null || break; sleep 1; done; kill $LP 2>/dev/null
box 'pgrep -f "^bash d143c.sh" >/dev/null' && log "box script running" || { log "box script NOT running"; exit 1; }
until box 'test -e ~/t2c/ready-depth || test -e ~/t2c/restored'; do sleep 15; done
if box 'test -e ~/t2c/ready-depth && ! test -e ~/t2c/restored'; then
  log "depth start head=$(git -C $HOME/git/me/discipline/main rev-parse --short HEAD)"
  python3 -B $P/depth_probe.py run --endpoint http://$BH:8082 --corpus $P/corpus/manifest.json \
    --repo tokio=$HOME/diet-inference-runs/corpora/tokio --tier supported --serving-context 229376 --samples 5 \
    --sampler '{"temperature": 0.6, "top_k": 20, "top_p": 1.0, "min_p": 0.0}' --out $OUT > $W/probe.log 2>&1
  log "depth run rc=$?"
fi
box 'touch ~/t2c/depth.done'; log "depth.done sent"
until box 'test -e ~/t2c/restored'; do sleep 30; done; log "restored marker seen"
( cd $HOME/git/me/diet-inference && python3 instruments/fingerprint.py --check instruments/baselines/substrate-2026-09-20-postupdate.json --hash-models > $W/fingerprint-after.log 2>&1 ); log "fingerprint rc=$?"
( cd $HOME/git/me/diet-inference && bash scripts/verify-box.sh > $W/verify-after.log 2>&1 ); log "verify-box rc=$?"
( cd $I && python3 canary.py --log fixtures/canary-e0827.jsonl --fork e0827 --anchor common_chat_templates_apply --k 36 \
    --base-url http://$BH:8082 --misses $W/misses-after.jsonl --baseline $POOL > $W/canary-after.log 2>&1 ); log "canary after rc=$? $(grep -E '^rate' $W/canary-after.log) | $(grep -E '^verdict' $W/canary-after.log)"
python3 -B $P/depth_probe.py summarise $OUT/rows.jsonl > $OUT/summary.json; log "summarise rc=$?"
python3 -B $P/depth_probe.py decide $OUT/summary.json $P/criterion.toml > $OUT/decide.json; log "decide rc=$?"
log MAC-END
