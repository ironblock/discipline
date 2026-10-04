#!/bin/bash
# ExLlamaV3 Flash-Next window: KLD vs the 2026-09-30 Q8_0 reference (2.05 and 3.05 bpw), then the production
# bench (deep.py's prompts) on 2.05 bpw with and without MTP. Production stopped for the duration, restored on exit.
L=$HOME/setup/logs/exl3win-1002.log; : > $L
E=$HOME/venvs/exl3/bin/python; S=$HOME/setup
M2=$HOME/models/Qwen3.8-Flash-Next-exl3-2.05bpw; M3=$HOME/models/Qwen3.8-Flash-Next-exl3-3.05bpw
export PATH="$HOME/.local/bin:$PATH:/usr/local/cuda/bin"
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
restore () {
  log "restore: begin"
  for p in $(pgrep -f "exl3(kld|spd)\.py"); do kill $p; done; sleep 5
  for p in $(pgrep -x llama-server); do kill $p; done; sleep 5
  cd $HOME/src/llama-prod2 && nohup $(cat $HOME/setup/prod-cmdline.txt) > /tmp/prod-server.log 2>&1 < /dev/null &
  for i in $(seq 1 90); do curl -s -H "Authorization: Bearer <api-key>" localhost:<port>/health | grep -q ok && break; sleep 2; done
  log "RESTORED $(curl -s -H 'Authorization: Bearer <api-key>' localhost:<port>/props | python3 -c 'import json,sys;print(json.load(sys.stdin).get("build_info"))') pid=$(pgrep -x llama-server)"
}
trap restore EXIT
log "slots before stop: $(curl -s -H 'Authorization: Bearer <api-key>' localhost:<port>/slots | python3 -c 'import json,sys;print([s["is_processing"] for s in json.load(sys.stdin)])')"
for p in $(pgrep -x llama-server); do kill $p; done
for i in $(seq 1 30); do pgrep -x llama-server >/dev/null || break; sleep 1; done
for p in $(pgrep -x llama-server); do kill $p; done; sleep 3; for p in $(pgrep -x llama-server); do kill -9 $p; done; sleep 2
log "DOWNTIME START vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader)"

cd $S
log "kld 2.05"
$E exl3kld.py -m $M2 -ngr --tag exl3-205 --base kld/base-code.kld --base kld/base-prose.kld > logs/exl3kld-205.log 2>&1
log "kld 2.05 rc=$? $(grep -h '^RESULT\|loaded' logs/exl3kld-205.log | tr '\n' '|')"
log "kld 3.05 (-mcl 10)"
$E exl3kld.py -m $M3 -ngr -mcl 10 -mct 16 --tag exl3-305 --base kld/base-code.kld --base kld/base-prose.kld > logs/exl3kld-305.log 2>&1
log "kld 3.05 rc=$? $(grep -h '^RESULT\|loaded' logs/exl3kld-305.log | tr '\n' '|')"

for arm in mtp nospec; do
  X=""; [ $arm = mtp ] && X="-mtp -ndt 3"
  log "speed 2.05 $arm"
  PHASES=10000:1,10000:2,10000:4,100000:1 $E exl3spd.py -m $M2 -ngr -cs 131072 $X --tag exl3-205-$arm >> $L 2>&1
  log "speed 2.05 $arm rc=$?"
done
log "arms done"
