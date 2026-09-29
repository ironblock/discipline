#!/bin/bash
# The candidate's T2 depth window (#143), box side. Production stopped, the candidate launched on its
# cells-window line (the inference seat's recommended line), the Mac runs the probe at the marker, then
# production is ALWAYS restored through the trap via ~/provision-diet.sh. Never pkill -f: only pids found
# by pgrep -x whose own command line names the port are signalled.
set -u
W=$HOME/t2c; L=$W/window.log; O=$W/out; mkdir -p $O
CBIN=$HOME/src/llama.cpp/build-cuda/bin/llama-server
CW=$HOME/Models/Qwen3.8-27B-GSQ-RCO/Qwen3.8-27B-GSQ-RCO-IQ3_S-mtp.gguf
CLINE="-m $CW -ngl 99 -t 8 -c 229376 -np 2 --kv-unified -ctk q8_0 -ctv q8_0 -fa on -b 1024 -ub 512 --jinja --host 0.0.0.0 --port 8082 --spec-type draft-mtp --spec-draft-n-max 2"
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
pids_on () { for p in $(pgrep -x llama-server); do tr '\0' ' ' < /proc/$p/cmdline | grep -q -- "--port $1 " && echo $p; done; }
stop_port () { for p in $(pids_on $1); do kill $p; done; for i in $(seq 1 60); do [ -z "$(pids_on $1)" ] && return 0; sleep 1; done
               for p in $(pids_on $1); do log "port $1 pid $p ignored TERM; KILL"; kill -9 $p; done; sleep 2; [ -z "$(pids_on $1)" ]; }
up () { curl -s -m 5 localhost:$1/health | grep -q ok; }
ready () { [ "$(curl -s -m5 -o /dev/null -w %{http_code} -H 'Content-Type: application/json' -d '{"model":"x","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}' http://127.0.0.1:$1/v1/chat/completions)" = 200 ]; }
vram () { nvidia-smi --query-gpu=memory.used --format=csv,noheader; }
exe16 () { sha256sum "$(readlink -f /proc/$1/exe)" 2>/dev/null | cut -c1-16; }
restore () {
  log "restore: begin"
  stop_port 8082
  bash $HOME/provision-diet.sh >> $O/provision.log 2>&1; log "provision-diet rc=$?"
  for i in $(seq 1 90); do ready 8082 && break; sleep 10; done
  local pp=$(pids_on 8082)
  log "restored: pid=$pp exe=$(exe16 $pp) cmdline_same=$([ "$(tr '\0' ' ' < /proc/$pp/cmdline)" = "$FCMD" ] && echo yes || echo NO) vram=$(vram)"
  touch $W/restored; log END
}
rm -f $W/ready-depth $W/depth.done $W/restored
PROD=$(pids_on 8082); FCMD=$(tr '\0' ' ' < /proc/$PROD/cmdline)
{ echo "prod_pid $PROD"; echo "prod_exe $(sha256sum "$(readlink -f /proc/$PROD/exe)" | cut -d' ' -f1)"
  echo "cand_commit $(git -C $HOME/src/llama.cpp rev-parse HEAD) dirty=$(git -C $HOME/src/llama.cpp status --porcelain | wc -l)"
  echo "cand_file $(sha256sum "$(readlink -f $CBIN)" | cut -d' ' -f1) llama-server"
  echo "cand_weights $(sha256sum $CW | cut -d' ' -f1) $(stat -c %s $CW)"
  echo "kernel $(uname -r)"; } > $O/identity-before.txt 2>&1
log "identity read; prod pid=$PROD"
[ "$(exe16 $PROD)" = 980845d60ae7a820 ] || { log "ABORT: production exe is not the floor's engine"; exit 1; }
trap restore EXIT
stop_port 8082 || exit 1; log "production stopped vram=$(vram)"
( exec distrobox enter <build-container> -- bash -lc "exec $CBIN $CLINE" > $O/server.log 2>&1 < /dev/null ) &
DP=$!
for i in $(seq 1 300); do up 8082 && break; kill -0 $DP 2>/dev/null || break; sleep 2; done
up 8082 || { log "candidate FAILED to start"; grep -iE ' E |error|out of memory' $O/server.log | tail -4 >> $L; exit 1; }
SP=$(pids_on 8082); log "candidate up server=$SP exe=$(exe16 $SP) vram=$(vram)"
echo "cand_running_exe $(sha256sum "$(readlink -f /proc/$SP/exe)" | cut -d' ' -f1) pid=$SP" >> $O/identity-before.txt
touch $W/ready-depth; log "waiting for the Mac's depth probe"
for i in $(seq 1 720); do [ -e $W/depth.done ] && break; sleep 10; done
log "depth wait over (done=$([ -e $W/depth.done ] && echo yes || echo TIMEOUT))"
