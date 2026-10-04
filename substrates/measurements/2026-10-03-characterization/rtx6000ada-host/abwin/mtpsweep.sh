#!/bin/bash
# MTP draft-length sweep on the serving stack (Tabby venv: exllamav3 1.5.2), production model and cache config
# (262144 tokens, 8-bit KV, PLE in RAM). Arms: n = 3 8 1 6 2 4, dynamic (ceiling 8), 3 again (drift control).
# Bench: exl3spd.py (deep.py prompts) at 10k x1, 10k x4, 100k x1. Production (Tabby) stopped, restored on exit.
L=$HOME/setup/logs/mtpsweep-1003.log; : > $L
S=$HOME/setup; TV=$HOME/venvs/tabby/bin/python; M=$HOME/models/Qwen3.8-Flash-Next-exl3-2.05bpw
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
tabby_pids () { for p in $(pgrep -x python3.12) $(pgrep -x python); do tr "\0" " " < /proc/$p/cmdline | grep -q "venvs/tabby/bin/python main.py" && echo $p; done; }
restore () {
  log "restore: begin"
  for p in $(pgrep -f "exl3spd.py"); do kill $p; done; sleep 5
  [ -n "$(tabby_pids)" ] || (setsid nohup $S/prod-tabby.sh > /tmp/prod-tabby.log 2>&1 < /dev/null &)
  for i in $(seq 1 150); do curl -s localhost:<port>/health 2>/dev/null | grep -q "\"status\":\"healthy\"" && break; sleep 2; done
  log "RESTORED tabby pid=$(tabby_pids) health=$(curl -s localhost:<port>/health)"
}
trap restore EXIT
log "tabby before: pid=$(tabby_pids)"
for p in $(tabby_pids); do kill $p; done; for i in $(seq 1 30); do [ -z "$(tabby_pids)" ] && break; sleep 1; done; for p in $(tabby_pids); do kill -9 $p; done; sleep 3
log "DOWNTIME START vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader)"
cd $S
for arm in n3 n8 n1 n6 n2 n4 dyn8 n3b; do
  case $arm in dyn8) X="-mtp -ndt 8 -dds" ;; n3b) X="-mtp -ndt 3" ;; *) X="-mtp -ndt ${arm#n}" ;; esac
  log "arm $arm: $X"
  PHASES=10000:1,10000:4,100000:1 $TV exl3spd.py -m $M -ngr -cs 262144 -cq 8,8 $X --tag $arm >> $L 2>&1
  log "arm $arm rc=$?"
done
log "arms done"
