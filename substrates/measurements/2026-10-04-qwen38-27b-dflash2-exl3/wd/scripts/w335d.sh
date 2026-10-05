#!/bin/bash
# #335 follow-up (d) on linux-pc: EXL3 SC_3.00bpw_H4 + DFlash2 EXL3 drafter on ExLlamaV3 1.5.4 with an 8,8 cache (like for like
# with the llama.cpp line's q8_0/q8_0), at the largest pool that loads. No load-time VRAM gate
# (headroom is recorded at the run's peak and judged against the floor's 1,208 MiB afterwards).
#   floor reading (production up) -> stop floor -> EXL3 3.50 KLD -> EXL3 arm A (DFlash2 EXL3 drafter) and arm B (MTP)
#   -> mainline arm C (candidate IQ3_S + DFlash2 Q4_K_M GGUF) and C' (smaller GGUF drafters) -> restore.
# ALWAYS restores the floor via provision-diet.sh in the trap. Signals only pids found by pgrep -x whose command line
# names the port, or the python children this script started. Never pkill -f.
set -u
W=$HOME/w335d; L=$W/window.log; O=$W/out; mkdir -p $O; : > $L
M=$HOME/Models; PY=$HOME/venvs/exl3-154/bin/python
CBIN=$HOME/src/llama.cpp/build-cuda/bin/llama-server
CW=$M/Qwen3.8-27B-GSQ-RCO/Qwen3.8-27B-GSQ-RCO-IQ3_S-mtp.gguf
DA=$M/Qwen3.8-27B-DFlash2-EXL3-4.00bpw-igor255
GZ=$M/Qwen3.8-27B-DFlash2-GGUF-zlab/Qwen3.8-27B-DFlash2-Q4_K_M.gguf
G3=$M/Qwen3.8-27B-DFlash2-GGUF-anbeeld/Qwen3.8-27B-DFlash2-Q3_K_M.gguf
G2=$M/Qwen3.8-27B-DFlash2-GGUF-anbeeld/Qwen3.8-27B-DFlash2-Q2_K.gguf
E=$HOME/w335/exl3deep.py
FLOOR_FREE=1208   # Q9: the floor's measured free VRAM at full prefill (MiB)
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
pids_on () { for p in $(pgrep -x llama-server); do tr '\0' ' ' < /proc/$p/cmdline | grep -q -- "--port $1 " && echo $p; done; }
stop_port () {
  for p in $(pids_on $1); do kill $p; done
  for i in $(seq 1 60); do [ -z "$(pids_on $1)" ] && return 0; sleep 1; done
  for p in $(pids_on $1); do log "port $1 pid $p ignored TERM; KILL"; kill -9 $p; done
  for i in $(seq 1 30); do [ -z "$(pids_on $1)" ] && return 0; sleep 1; done
  log "port $1 did not stop"; return 1; }
up () { curl -s -m 5 localhost:$1/health | grep -q ok; }
wait_up () { for i in $(seq 1 300); do up $1 && return 0; kill -0 $2 2>/dev/null || return 1; sleep 2; done; return 1; }
vram () { nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits; }
CHILD=""
restore () {
  log "restore: begin"
  [ -n "$CHILD" ] && kill -9 $CHILD 2>/dev/null
  stop_port <port>
  sleep 3
  bash $HOME/provision-diet.sh >> $O/provision.log 2>&1; log "provision-diet rc=$?"
  for i in $(seq 1 90); do
    [ "$(curl -s -m5 -o /dev/null -w %{http_code} -H 'Content-Type: application/json' -d '{"model":"x","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}' http://127.0.0.1:<port>/v1/chat/completions)" = 200 ] && break; sleep 10; done
  PP=$(pids_on <port>)
  log "restored: pid=$PP exe=$(sha256sum "$(readlink -f /proc/$PP/exe)" | cut -c1-16) vram=$(vram)"
  touch $W/restored; log END
}
rm -f $W/restored

# ---------- preflight, floor up ----------
PROD=$(pids_on <port>)
[ "$(sha256sum "$(readlink -f /proc/$PROD/exe)" | cut -c1-16)" = 980845d60ae7a820 ] || { log "ABORT: production exe is not the floor's engine"; touch $W/restored; exit 1; }
{ echo "prod_exe $(sha256sum "$(readlink -f /proc/$PROD/exe)" | cut -d' ' -f1)"
  echo "cand_commit $(git -C $HOME/src/llama.cpp rev-parse HEAD) dirty=$(git -C $HOME/src/llama.cpp status --porcelain | wc -l)"
  echo "cand_server $(sha256sum $CBIN | cut -d' ' -f1)"
  echo "cand_weights $(sha256sum $CW | cut -d' ' -f1)"
  echo "exllamav3 $($PY -c 'import exllamav3.version as v;print(v.__version__)') torch $($PY -c 'import torch;print(torch.__version__)')"
  for f in $E $DA/model.safetensors $DA/quantization_config.json $GZ $G3 $G2; do echo "file $(sha256sum $f | cut -d' ' -f1) $(stat -c %s $f) $(basename $(dirname $f))/$(basename $f)"; done
  for b in 3.00bpw 3.50bpw; do d=$M/Qwen3.8-27B-exl3-$b; echo "weights $b $(stat -c %s $d/*.safetensors | awk '{s+=$1} END {print s}') qc=$(sha256sum $d/quantization_config.json | cut -c1-16)"; done
  echo "corpus_files $(ls $HOME/git/llama.cpp >/dev/null && git -C $HOME/git/llama.cpp rev-parse HEAD) dirty=$(git -C $HOME/git/llama.cpp status --porcelain | wc -l)"
  nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader | sed 's/^/smi /'
} > $O/identity.txt 2>&1
log "preflight ok; floor pid=$PROD"


trap restore EXIT
stop_port <port> || exit 1; log "DOWNTIME START floor stopped vram=$(vram)"
cd $W
ex () {  # $1 tag, $2 model dir, $3 pool, $4 phases, rest: extra args
  local tag=$1 m=$2 cs=$3 ph=$4; shift 4
  timeout 2400 $PY $E -m $m -cs $cs -ambs 2 -chunk_size 2048 --tag $tag --phases "$ph" "$@" > $O/$tag.log 2>&1 &
  CHILD=$!; wait $CHILD; local rc=$?; CHILD=""
  log "$tag rc=$rc $(grep -E '"event": "(loaded|done)"' $O/$tag.log | tr '\n' ' ' | cut -c1-500)"
  return $rc; }
phases () { echo "2000:1,100000:1,$(($1-3000)):1,$(($1/2-2000)):2"; }
MD=$M/Qwen3.8-27B-exl3-SC_3.00bpw_H4
EC=""
for cs in 229376 196608 163840 131072; do
  ex P-c$cs $MD $cs "2000:1" -dm $DA -cq 8,8 && { EC=$cs; break; }
done
log "pool choice (cq 8,8): $EC"
[ -n "$EC" ] && ex S3-dflash2-cq88 $MD $EC "$(phases $EC)" -dm $DA -cq 8,8
log "phases done"
