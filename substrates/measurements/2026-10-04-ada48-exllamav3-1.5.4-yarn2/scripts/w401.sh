#!/bin/bash
# #401 window on rtx6000ada-host: ExLlamaV3 1.5.2 (serving venv) against 1.5.4 (staged copy, only exllamav3 replaced).
#   KLD of the serving 2.05bpw weights on 1.5.4 -> ABAB of TabbyAPI on a side port (no auth): the four-way concurrency
#   check, then 10k x1, 10k x4, 100k x1 -> production restored on 1.5.2 by the trap. Switching production to 1.5.4 is a
#   separate step after the results are read. Signals only pids whose cmdline and cwd are TabbyAPI's main.py or this
#   script's children. Never pkill -f.
set -u
W=$HOME/w401; L=$W/window.log; O=$W/out; mkdir -p $O; : > $L
S=$HOME/setup; T=$HOME/src/tabbyAPI
V152=$HOME/venvs/tabby/bin/python; V154=$HOME/venvs/tabby154/bin/python
M=$HOME/models/Qwen3.8-Flash-Next-exl3-2.05bpw
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
vram () { nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits; }
tabby_pids () { for p in $(pgrep -x python) $(pgrep -x python3); do
    [ "$(readlink /proc/$p/cwd 2>/dev/null)" = "$T" ] && tr '\0' ' ' < /proc/$p/cmdline | grep -q "main.py" && echo $p; done; }
tabby_stop () { for p in $(tabby_pids); do kill $p; done
  for i in $(seq 1 45); do [ -z "$(tabby_pids)" ] && return 0; sleep 1; done
  for p in $(tabby_pids); do log "tabby pid $p ignored TERM; KILL"; kill -9 $p; done; sleep 3; }
KEY=$(awk '/^api_key:/{print $2}' $T/api_tokens.yml)
CHILD=""
restore () {
  log "restore: begin"
  [ -n "$CHILD" ] && kill -9 $CHILD 2>/dev/null
  tabby_stop
  (cd $T && exec nohup $V152 main.py) > /tmp/prod-tabby.log 2>&1 < /dev/null &
  for i in $(seq 1 200); do curl -s -m5 -H "Authorization: Bearer $KEY" localhost:<port>/health 2>/dev/null | grep -q healthy && break; sleep 3; done
  local P=$(tabby_pids)
  log "restored: pid=$P venv=$(tr '\0' ' ' < /proc/$P/cmdline | cut -d' ' -f1) health=$(curl -s -m5 -H "Authorization: Bearer $KEY" localhost:<port>/health) vram=$(vram)"
  touch $W/restored; log END
}
rm -f $W/restored

# ---------- preflight, production up ----------
PROD=$(tabby_pids)
[ -n "$PROD" ] || { log "ABORT: no production TabbyAPI process"; touch $W/restored; exit 1; }
{ echo "prod_pid $PROD started $(ps -o lstart= -p $PROD)"
  echo "prod_cmd $(tr '\0' ' ' < /proc/$PROD/cmdline)"
  echo "tabby_commit $(git -C $T rev-parse HEAD) dirty=$(git -C $T status --porcelain | grep -v -E 'config|api_tokens' | wc -l)"
  for v in $V152 $V154; do echo "venv $v: $(CUDA_VISIBLE_DEVICES= $v -c 'import torch, exllamav3.version as e; print("torch", torch.__version__, "exllamav3", e.__version__)' 2>/dev/null | tail -1)"
    echo "  ext $(sha256sum $(dirname $(dirname $v))/lib/python3.12/site-packages/exllamav3_ext*.so 2>/dev/null | cut -d' ' -f1)"; done
  echo "wheel154 $(sha256sum $W/exllamav3-1.5.4+cu132.torch2.11.0-cp312-cp312-linux_x86_64.whl | cut -d' ' -f1)"
  echo "config_live $(sha256sum $T/config.yml | cut -d' ' -f1)"
  echo "exl3kld $(sha256sum $S/exl3kld.py | cut -d' ' -f1)"
  echo "contcheck $(sha256sum $S/abwin/contcheck.py | cut -d' ' -f1) deep2 $(sha256sum $S/abwin/deep2.py | cut -d' ' -f1)"
  nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader | sed 's/^/smi /'
} > $O/identity.txt 2>&1
sed -e 's/^\(  *port:\).*/\1 <side-port>/' -e 's/^\(  *disable_auth:\).*/\1 true/' $T/config.yml > $T/config-401.yml
log "preflight ok; prod pid=$PROD; side config $(sha256sum $T/config-401.yml | cut -c1-16) (port <side-port>, no auth; otherwise the live config)"
diff <(grep -v -E 'port:|disable_auth:' $T/config.yml) <(grep -v -E 'port:|disable_auth:' $T/config-401.yml) > /dev/null || { log "ABORT: side config differs beyond network"; touch $W/restored; exit 1; }

trap restore EXIT
tabby_stop; log "DOWNTIME START vram=$(vram)"
cd $S

# ---------- 1. KLD on 1.5.4 (same scorer and flags as #336's 1.5.2 row) ----------
timeout 1800 $V154 $S/exl3kld.py -m $M -ngr --tag exl3-205-v154 --base kld/base-code.kld --base kld/base-prose.kld > $O/kld-205-v154.log 2>&1 &
CHILD=$!; wait $CHILD; log "kld v154 rc=$? $(grep -h '^RESULT' $O/kld-205-v154.log | tr '\n' '|' | cut -c1-500)"; CHILD=""

# ---------- 2. ABAB ----------
for arm in v152a v154a v152b v154b; do
  case $arm in v152*) V=$V152 ;; v154*) V=$V154 ;; esac
  (cd $T && exec $V main.py --config config-401.yml) > $O/tabby-$arm.log 2>&1 < /dev/null &
  for i in $(seq 1 200); do curl -s -m5 localhost:<side-port>/health 2>/dev/null | grep -q healthy && break; [ -n "$(tabby_pids)" ] || break; sleep 3; done
  if ! curl -s -m5 localhost:<side-port>/health 2>/dev/null | grep -q healthy; then log "$arm FAILED TO START: $(grep -iE 'error|exception' $O/tabby-$arm.log | tail -3 | tr '\n' '|')"; tabby_stop; continue; fi
  log "$arm up after ~$((i*3))s vram=$(vram) exllamav3=$($V -c 'import exllamav3.version as e;print(e.__version__)' 2>/dev/null | tail -1)"
  python3 $S/abwin/contcheck.py <side-port> x > $O/cont-$arm.log 2>&1; log "$arm contcheck rc=$? $(tail -2 $O/cont-$arm.log | tr '\n' '|' | cut -c1-300)"
  PHASES=10000:1,10000:4,100000:1 python3 $S/abwin/deep2.py <side-port> $arm > $O/deep-$arm.log 2>&1; log "$arm deep rc=$?"
  tabby_stop; log "$arm stopped vram=$(vram)"
done
log "arms done"
