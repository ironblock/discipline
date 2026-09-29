#!/bin/bash
# #142 window, part 1: on/off/off output invariance. Stops production, runs a spec-OFF
# and a spec-ON test server on 8098 (production's own line otherwise), and ALWAYS
# restores production through the trap. Never pkill -f; only pids found by pgrep -x
# whose own command line is checked are signalled. The key is never echoed.
set -u
W=$HOME/setup/w142; L=$W/inv${RUN:-}.log; mkdir -p $W/inv
CMD=$(cat $HOME/setup/prod-cmdline.txt)
KEY=$(grep -o -- '--api-key [^ ]*' $HOME/setup/prod-cmdline.txt | cut -d' ' -f2)
SPEC='--spec-type draft-mtp --spec-draft-model [^ ]+ --spec-draft-ngl 99 --spec-draft-n-max 3 --spec-draft-p-min 0.0'
ON=$(echo "$CMD" | sed -E 's/--port 8080/--port 8098/')
OFF=$(echo "$ON" | sed -E "s/ $SPEC//")
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
pid_on_port () { for p in $(pgrep -x llama-server); do tr '\0' ' ' < /proc/$p/cmdline | grep -q -- "--port $1 " && echo $p; done; }
health () { curl -s -m 5 -H "Authorization: Bearer $KEY" localhost:$1/health | grep -q ok; }
wait_health () { for i in $(seq 1 300); do health $1 && return 0; kill -0 $2 2>/dev/null || return 1; sleep 2; done; return 1; }
PROD=$(pid_on_port 8080)
PRODCWD=$(readlink -f /proc/$PROD/cwd)
PRODEXE=$(sha256sum "$(readlink -f /proc/$PROD/exe)" | cut -d' ' -f1)
restore () {
  for p in $(pid_on_port 8098); do kill $p; done; sleep 3
  if [ -z "$(pid_on_port 8080)" ]; then
    ( cd "$PRODCWD" && exec nohup $CMD > /tmp/prod-server.log 2>&1 < /dev/null ) &
    NP=$!
    if wait_health 8080 $NP; then log "RESTORED pid=$NP"; else log "RESTORE FAILED"; fi
  fi
  P=$(pid_on_port 8080)
  log "after: pid=$P exe=$(sha256sum "$(readlink -f /proc/$P/exe)" | cut -c1-16) cmdline_same=$([ "$(tr '\0' ' ' < /proc/$P/cmdline | sed 's/ $//')" = "$(echo $CMD)" ] && echo yes || echo NO) vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader)"
  log END
}
trap restore EXIT
log "START prod pid=$PROD exe=${PRODEXE:0:16} cwd=$PRODCWD vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader) load=$(cut -d' ' -f1-3 /proc/loadavg)"
[ "${PRODEXE:0:16}" = 41e6591d710fd4b5 ] || { log "ABORT: production exe is not the declared engine"; exit 1; }
[ -f /tmp/prod-server.log ] && mv /tmp/prod-server.log /tmp/prod-server-pre-w142.log
kill $PROD; for i in $(seq 1 60); do kill -0 $PROD 2>/dev/null || break; sleep 1; done
log "production stopped"
for arm in ${ARMS:-off on}; do
  [ -z "$(pid_on_port 8098)" ] || { log "$arm ABORT: 8098 already served"; continue; }
  if [ $arm = off ]; then LINE=$OFF; else LINE=$ON; fi
  ( cd "$PRODCWD" && exec $LINE > $W/inv/server2-$arm.log 2>&1 < /dev/null ) &
  SP=$!
  if wait_health 8098 $SP; then
    log "$arm server up pid=$SP vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader) spec_flags=$(tr '\0' ' ' < /proc/$SP/cmdline | grep -c -- '--spec-type')"
    [ "$(tr '\0' ' ' < /proc/$SP/cmdline | grep -c -- '--port 8098')" = 1 ] && python3 $W/inv.py 8098 $arm 5 $W/inv2 >> $L 2>&1; log "$arm inv.py rc=$?"
  else
    log "$arm server FAILED to start"; grep -iE ' E |error|out of memory' $W/inv/server2-$arm.log | tail -4 >> $L
  fi
  for p in $(pid_on_port 8098); do kill $p; done; for i in $(seq 1 60); do [ -z "$(pid_on_port 8098)" ] && break; sleep 1; done
done
