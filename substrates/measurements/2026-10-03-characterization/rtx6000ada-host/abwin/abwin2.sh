#!/bin/bash
# Clean ABAB: production line (b7-e7051ef, fresh on a side port <side-port>, no key) vs TabbyAPI + EXL3 2.05bpw (262k, 8-bit KV, MTP 3)
# on a second side port. Each arm: the 4-way concurrency correctness check, then the production bench. Production restored on exit.
L=$HOME/setup/logs/abwin2-1003.log; : > $L
S=$HOME/setup; W=$S/abwin; TV=$HOME/venvs/tabby/bin/python
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
LL=$(sed -e "s/--port <port>/--port <side-port>/" -e "s/--api-key [^ ]*//" $S/prod-cmdline.txt)
tabby_kill () { for p in $(pgrep -f "venvs/tabby/bin/python main.py"); do kill $p; done; for i in $(seq 1 30); do pgrep -f "venvs/tabby/bin/python main.py" >/dev/null || return 0; sleep 1; done
                for p in $(pgrep -f "venvs/tabby/bin/python main.py"); do kill -9 $p; done; sleep 3; }
stopll () { for p in $(pgrep -x llama-server); do kill $p; done; for i in $(seq 1 30); do pgrep -x llama-server >/dev/null || return 0; sleep 1; done
            for p in $(pgrep -x llama-server); do kill $p; done; sleep 5; for p in $(pgrep -x llama-server); do kill -9 $p; done; sleep 2; }
restore () {
  log "restore: begin"; tabby_kill; stopll
  cd $HOME/src/llama-prod && nohup $(cat $S/prod-cmdline.txt) > /tmp/prod-server.log 2>&1 < /dev/null &
  for i in $(seq 1 90); do curl -s -H "Authorization: Bearer <api-key>" localhost:<port>/health | grep -q ok && break; sleep 2; done
  log "RESTORED $(curl -s -H "Authorization: Bearer <api-key>" localhost:<port>/props | python3 -c "import json,sys;print(json.load(sys.stdin).get(\"build_info\"))") pid=$(pgrep -x llama-server) exe=$(sha256sum /proc/$(pgrep -x llama-server)/exe | cut -c1-16)"
}
trap restore EXIT
log "slots before stop: $(curl -s -H "Authorization: Bearer <api-key>" localhost:<port>/slots | python3 -c "import json,sys;print([s[\"is_processing\"] for s in json.load(sys.stdin)])")"
stopll; log "DOWNTIME START vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader)"
for arm in llama1 tabby1 llama2 tabby2; do
  case $arm in
  llama*) (cd $HOME/src/llama-prod && exec $LL) > /tmp/ab2-$arm.log 2>&1 < /dev/null & PORT=<side-port>
          for i in $(seq 1 150); do curl -s localhost:<port>/health | grep -q ok && break; pgrep -x llama-server >/dev/null || break; sleep 2; done
          UP=$(curl -s localhost:<port>/health | grep -c ok) ;;
  tabby*) (cd $HOME/src/tabbyAPI && exec $TV main.py) > /tmp/ab2-$arm.log 2>&1 < /dev/null & PORT=<side-port-2>
          for i in $(seq 1 300); do curl -s localhost:<side-port-2>/health 2>/dev/null | grep -q "\"status\":\"healthy\"" && break; pgrep -f "venvs/tabby/bin/python main.py" >/dev/null || break; sleep 2; done
          UP=$(curl -s localhost:<side-port-2>/health 2>/dev/null | grep -c "\"status\":\"healthy\"") ;;
  esac
  if [ "$UP" != 1 ]; then log "$arm FAILED TO START: $(grep -iE "error|exception" /tmp/ab2-$arm.log | tail -4 | tr "\n" "|")"; continue; fi
  log "$arm up after ~$((i*2))s vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader)"
  python3 $W/contcheck.py $PORT x 2>&1 | sed "s/^/[$arm] /" >> $L
  PHASES=10000:1,10000:2,10000:4,100000:1,200000:1 python3 $W/deep2.py $PORT $arm >> $L 2>&1
  log "$arm bench done"
  case $arm in llama*) stopll ;; tabby*) tabby_kill ;; esac
  log "$arm stopped vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader)"
done
log "arms done"
