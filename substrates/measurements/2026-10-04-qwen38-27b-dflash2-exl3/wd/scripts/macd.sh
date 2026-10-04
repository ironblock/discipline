#!/bin/bash
set -u
W=$HOME/diet-inference-runs/w335d; L=$W/mac.log; mkdir -p $W/out; : > $L
BH=$(cat $HOME/diet-inference-runs/<run-dir>/.boxhost)
SSH="ssh -i $HOME/.ssh/id_rsa -o IdentitiesOnly=yes -o BatchMode=yes <user>@$BH"
DI=$HOME/git/me/diet-inference
BASELINE=$HOME/diet-inference-runs/<restore-capture>/substrate-2026-10-02-postupdate.json
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
[ "$(shasum -a 256 $BASELINE | cut -c1-16)" = f2a6ca42810abb9d ] || { log "ABORT baseline digest"; exit 1; }
( cd $DI && bash scripts/verify-box.sh > $W/out/verify-before.log 2>&1 ); rc=$?; log "verify-box before rc=$rc"; [ $rc = 0 ] || { log "ABORT preflight"; exit 1; }
$SSH 'mkdir -p ~/w335d' && scp -q -i $HOME/.ssh/id_rsa -o IdentitiesOnly=yes <scratch>/w335d.sh <user>@$BH:w335d/ || { log "ABORT scp"; exit 1; }
( $SSH 'cd ~/w335d && chmod +x w335d.sh && setsid nohup ./w335d.sh > nohup.out 2>&1 < /dev/null & echo started' < /dev/null > $W/out/launch.log 2>&1 & ); sleep 10; log "box window launched"
until $SSH 'test -e ~/w335d/restored'; do sleep 30; done; log "box restored"
( cd $DI && python3 instruments/fingerprint.py --check $BASELINE --hash-models > $W/out/fingerprint-after.log 2>&1 ); log "fingerprint rc=$?"
( cd $DI && bash scripts/verify-box.sh > $W/out/verify-after.log 2>&1 ); log "verify-box after rc=$?"
( cd $DI/instruments && python3 canary.py --log fixtures/canary-e0827.jsonl --fork e0827 --anchor common_chat_templates_apply --k 36 \
    --base-url http://$BH:<port> --baseline $HOME/diet-inference-runs/w143/window/pool10.json --misses $W/out/misses-after.jsonl > $W/out/canary-after.log 2>&1 ); log "canary rc=$? $(grep -E '^(rate|verdict)' $W/out/canary-after.log | tr '\n' ' ')"
scp -q -r -i $HOME/.ssh/id_rsa -o IdentitiesOnly=yes <user>@$BH:w335d/out $W/box-out; scp -q -i $HOME/.ssh/id_rsa -o IdentitiesOnly=yes <user>@$BH:w335d/window.log $W/box-out/
log MAC-END
