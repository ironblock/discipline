#!/bin/bash
# #334 addendum window on linux-pc: size-matched EXL3 quants (2.50bpw, 3.00bpw, SC_3.00bpw_H4) scored with exl3kld.py
# against the #334 Q8_0 reference logits (reused; their sha256 prefixes are checked before the floor stops).
# ALWAYS restores the floor via provision-diet.sh in the trap. Signals only pids found by pgrep whose
# command line names the floor's port, or the pids this script started.
set -u
W=$HOME/w334b; B=$HOME/w334; L=$W/window.log; O=$W/out; mkdir -p $O; : > $L
M=$HOME/Models; R=$M/Qwen3.8-27B-GSQ-RCO
REF=$M/Qwen3.8-27B-Q8_0/Qwen3.8-27B-Q8_0.gguf; REF_SHA=a680f44a06920e5d689774823782006aa3acc8db95750323373b24139b67e348
MAIN=$HOME/src/llama.cpp/build-cuda/bin/llama-perplexity
BEE=$HOME/Downloads/beellama-preview-v0.3.2
PY=$HOME/venvs/exl3/bin/python
KF="-t 8 -fa on -c 4096 -b 4096 -ub 512 --chunks 12"
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
pids_on () { for p in $(pgrep -x llama-server); do tr '\0' ' ' < /proc/$p/cmdline | grep -q -- "--port $1 " && echo $p; done; }
stop_port () {
  for p in $(pids_on $1); do kill $p; done
  for i in $(seq 1 30); do [ -z "$(pids_on $1)" ] && return 0; sleep 1; done
  for p in $(pids_on $1); do log "port $1 pid $p ignored TERM; KILL"; kill -9 $p; done
  for i in $(seq 1 30); do [ -z "$(pids_on $1)" ] && return 0; sleep 1; done
  log "port $1 did not stop"; return 1; }
vram () { nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits; }
summ () { grep -E 'Mean PPL\(Q\) |Mean +KLD|Same top p|error|failed|out of memory' "$1" | tr -s ' ' | tr '\n' '|' | cut -c1-400; }
CHILD=""
restore () {
  log "restore: begin"
  [ -n "$CHILD" ] && kill -9 $CHILD 2>/dev/null
  for c in $(pgrep -f "$W/exl3kld.py"); do kill -9 $c 2>/dev/null; done
  for c in $(pgrep -x llama-perplexit); do tr '\0' ' ' < /proc/$c/cmdline | grep -q "$W/" && kill -9 $c 2>/dev/null; done
  sleep 3
  bash $HOME/provision-diet.sh >> $O/provision.log 2>&1; log "provision-diet rc=$?"
  for i in $(seq 1 90); do
    [ "$(curl -s -m5 -o /dev/null -w %{http_code} -H 'Content-Type: application/json' -d '{"model":"x","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}' http://127.0.0.1:<port>/v1/chat/completions)" = 200 ] && break; sleep 10; done
  PP=$(pids_on <port>)
  log "restored: pid=$PP exe=$(sha256sum "$(readlink -f /proc/$PP/exe)" | cut -c1-16) vram=$(vram)"
  touch $W/restored; log END
}
rm -f $W/restored

# --- preflight (floor still up) ---
PROD=$(pids_on <port>)
[ "$(sha256sum "$(readlink -f /proc/$PROD/exe)" | cut -c1-16)" = 980845d60ae7a820 ] || { log "ABORT: production exe is not the floor's engine"; touch $W/restored; exit 1; }
[ "$(sha256sum $B/base-code.kld | cut -c1-16)" = 471fe3d21ee07cc5 ] || { log "ABORT: base-code.kld digest"; touch $W/restored; exit 1; }
[ "$(sha256sum $B/base-prose.kld | cut -c1-16)" = 1cc5037cb35395a9 ] || { log "ABORT: base-prose.kld digest"; touch $W/restored; exit 1; }
log "base files verified: code 471fe3d21ee07cc5 prose 1cc5037cb35395a9"
{ echo "main_commit $(git -C $HOME/src/llama.cpp rev-parse HEAD) dirty=$(git -C $HOME/src/llama.cpp status --porcelain | wc -l)"
  echo "main_perplexity $(sha256sum $MAIN | cut -d' ' -f1)"
  echo "bee_perplexity $(sha256sum $BEE/llama-perplexity | cut -d' ' -f1)"
  echo "bee_server $(sha256sum $BEE/llama-server | cut -d' ' -f1)"
  echo "exllamav3 $($PY -c 'import exllamav3.version as v;print(v.__version__)') torch $($PY -c 'import torch;print(torch.__version__)')"
  echo "exl3kld $(sha256sum $W/exl3kld.py | cut -d' ' -f1)"
  for b in 2.50bpw 3.00bpw SC_3.00bpw_H4; do d=$M/Qwen3.8-27B-exl3-$b; echo "weights $b $(cat $d/.cache/huggingface/download/*.safetensors.metadata 2>/dev/null | sed -n 1p | head -1) $(stat -c %s $d/*.safetensors | awk '{s+=$1} END {print s}') $(sha256sum $d/quantization_config.json | cut -c1-16)"; done; } > $O/identity.txt 2>&1
log "floor pid=$PROD; slots: $(curl -s -m5 localhost:<port>/slots | python3 -c 'import json,sys;print([s.get("is_processing") for s in json.load(sys.stdin)])' 2>&1)"
trap restore EXIT
stop_port <port> || exit 1; log "DOWNTIME START floor stopped vram=$(vram) mem_avail=$(free -g | awk '/Mem:/{print $7}')G"


# --- EXL3, size-matched rungs ---
cd $W
for b in 2.50bpw 3.00bpw SC_3.00bpw_H4; do
  $PY exl3kld.py -m $M/Qwen3.8-27B-exl3-$b --tag exl3-$b --last-only --base $B/base-code.kld --base $B/base-prose.kld > $O/exl3-$b.log 2>&1
  log "exl3 $b rc=$? $(grep -E '^RESULT|loaded|Error' $O/exl3-$b.log | tr '\n' '|' | cut -c1-600)"
done
log "arms done"
