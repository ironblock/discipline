#!/bin/bash
# The floor's checkpoint-restore cell (#143, I4b), box side, production up. The instrument as merged (d87ca9d).
# The reference: the dense 1.7B on the floor's own binary, CPU-only, localhost. Never pkill -f.
set -u
W=$HOME/i4b; O=$W/out; mkdir -p $O; L=$O/window.log; CR="python3 -B $W/checkpoint_restore.py"
BEE=$HOME/<engine-dir>; BW=$HOME/Models/Qwen3-1.7B-Q4_K_M.gguf
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
pids_on () { for p in $(pgrep -x llama-server); do tr '\0' ' ' < /proc/$p/cmdline | grep -q -- "--port $1 " && echo $p; done; }
stopref () { for p in $(pids_on 8186); do kill $p; done; for i in $(seq 1 20); do [ -z "$(pids_on 8186)" ] && break; sleep 1; done; }
trap stopref EXIT
FP=$(pids_on 8082); FM=$(tr '\0' '\n' < /proc/$FP/cmdline | grep -A1 -x -- '-m' | tail -1)
log "floor pid=$FP exe=$(sha256sum $(readlink -f /proc/$FP/exe) | cut -d' ' -f1) vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader) health=$(curl -s -m5 localhost:8082/health)"
$CR gguf "$FM" > $O/rung-header.json; $CR gguf $BW > $O/reference-header.json; log "headers read"
$CR measure --endpoint http://127.0.0.1:8082 --role rung --out $O/rung.json; log "rung measure rc=$?"
FN=$(python3 -c "import json;a=[x for x in json.load(open('$O/rung.json'))['attempts'] if x['warm']['cache_n']>0];print(a[0]['warm']['prompt_n'] if a else 0)")
log "rung warm prompt_n=$FN"
[ -z "$(pids_on 8186)" ] || { log "ABORT: 8186 busy"; exit 1; }
( cd $BEE && CUDA_VISIBLE_DEVICES= exec ./llama-server -m $BW --parallel 1 --threads 8 -c 8192 --host 127.0.0.1 --port 8186 > $O/reference-server.log 2>&1 < /dev/null ) &
for i in $(seq 1 60); do curl -s -m2 localhost:8186/health | grep -q ok && break; sleep 1; done
RP=$(pids_on 8186); log "reference pid=$RP exe=$(sha256sum $(readlink -f /proc/$RP/exe) | cut -d' ' -f1)"
# match the continuation: drop lines until the reference's warm prompt_n is within 10% of the rung's
DROP=$(( FN / 23 )); for try in 1 2 3 4; do
  $CR measure --endpoint http://127.0.0.1:8186 --role reference --prime-drops-lines $DROP --out $O/reference.json; rc=$?
  RN=$(python3 -c "import json;a=[x for x in json.load(open('$O/reference.json'))['attempts'] if x['warm']['cache_n']>0];print(a[0]['warm']['prompt_n'] if a else 0)")
  log "reference measure drop=$DROP rc=$rc warm prompt_n=$RN"
  python3 -c "import sys;sys.exit(0 if $FN and abs($RN-$FN) <= 0.10*$FN else 1)" && break
  DROP=$(python3 -c "print(max(1, round($DROP * $FN / max($RN - 6, 1))))")
done
# the grounding measurement, committed: the same reference at the unmatched length (no lines dropped)
$CR measure --endpoint http://127.0.0.1:8186 --role reference --prime-drops-lines 0 --out $O/reference-unmatched.json; log "reference unmatched rc=$?"
stopref; log "reference stopped"
log "floor after: pid=$(pids_on 8082) vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader) health=$(curl -s -m5 localhost:8082/health)"
log END
