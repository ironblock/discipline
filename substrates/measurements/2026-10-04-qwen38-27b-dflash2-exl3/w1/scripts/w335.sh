#!/bin/bash
# #335 window on linux-pc: Qwen3.8 27B, DFlash2 against MTP at two slots and >=160k.
#   floor reading (production up) -> stop floor -> EXL3 3.50 KLD -> EXL3 arm A (DFlash2 EXL3 drafter) and arm B (MTP)
#   -> mainline arm C (candidate IQ3_S + DFlash2 Q4_K_M GGUF) and C' (smaller GGUF drafters) -> restore.
# ALWAYS restores the floor via provision-diet.sh in the trap. Signals only pids found by pgrep -x whose command line
# names the port, or the python children this script started. Never pkill -f.
set -u
W=$HOME/w335; L=$W/window.log; O=$W/out; mkdir -p $O; : > $L
M=$HOME/Models; PY=$HOME/venvs/exl3/bin/python
CBIN=$HOME/src/llama.cpp/build-cuda/bin/llama-server
CW=$M/Qwen3.8-27B-GSQ-RCO/Qwen3.8-27B-GSQ-RCO-IQ3_S-mtp.gguf
DA=$M/Qwen3.8-27B-DFlash2-EXL3-4.00bpw-igor255
GZ=$M/Qwen3.8-27B-DFlash2-GGUF-zlab/Qwen3.8-27B-DFlash2-Q4_K_M.gguf
G3=$M/Qwen3.8-27B-DFlash2-GGUF-anbeeld/Qwen3.8-27B-DFlash2-Q3_K_M.gguf
G2=$M/Qwen3.8-27B-DFlash2-GGUF-anbeeld/Qwen3.8-27B-DFlash2-Q2_K.gguf
DEEP=$W/deepb.py; PROBE=$W/probe143.py
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
[ "$(sha256sum $HOME/w334/base-code.kld | cut -c1-16)" = 471fe3d21ee07cc5 ] || { log "ABORT: base-code.kld digest"; touch $W/restored; exit 1; }
[ "$(sha256sum $HOME/w334/base-prose.kld | cut -c1-16)" = 1cc5037cb35395a9 ] || { log "ABORT: base-prose.kld digest"; touch $W/restored; exit 1; }
{ echo "prod_exe $(sha256sum "$(readlink -f /proc/$PROD/exe)" | cut -d' ' -f1)"
  echo "cand_commit $(git -C $HOME/src/llama.cpp rev-parse HEAD) dirty=$(git -C $HOME/src/llama.cpp status --porcelain | wc -l)"
  echo "cand_server $(sha256sum $CBIN | cut -d' ' -f1)"
  echo "cand_weights $(sha256sum $CW | cut -d' ' -f1)"
  echo "exllamav3 $($PY -c 'import exllamav3.version as v;print(v.__version__)') torch $($PY -c 'import torch;print(torch.__version__)')"
  for f in $W/exl3deep.py $W/deepb.py $W/probe143.py $W/exl3kld.py $DA/model.safetensors $DA/quantization_config.json $GZ $G3 $G2; do echo "file $(sha256sum $f | cut -d' ' -f1) $(stat -c %s $f) $(basename $(dirname $f))/$(basename $f)"; done
  for b in 3.00bpw 3.50bpw; do d=$M/Qwen3.8-27B-exl3-$b; echo "weights $b $(stat -c %s $d/*.safetensors | awk '{s+=$1} END {print s}') qc=$(sha256sum $d/quantization_config.json | cut -c1-16)"; done
  echo "corpus_files $(ls $HOME/git/llama.cpp >/dev/null && git -C $HOME/git/llama.cpp rev-parse HEAD) dirty=$(git -C $HOME/git/llama.cpp status --porcelain | wc -l)"
  nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv,noheader | sed 's/^/smi /'
} > $O/identity.txt 2>&1
log "preflight ok; floor pid=$PROD"

# ---------- 1. the floor at depth, production up (pool 160,000, two slots, original DFlash) ----------
log "floor reading: begin"
python3 $DEEP deep <port> floor $O/deep-floor.json "2000:1,100000:1,157000:1,78000:2" > $O/deep-floor.log 2>&1; log "floor reading rc=$? vram=$(vram)"

# ---------- 2. stop the floor ----------
trap restore EXIT
stop_port <port> || exit 1; log "DOWNTIME START floor stopped vram=$(vram)"
cd $W

# ---------- 3. EXL3 3.50 bpw KLD against the #334 Q8_0 reference ----------
timeout 900 $PY $W/exl3kld.py -m $M/Qwen3.8-27B-exl3-3.50bpw --tag exl3-3.50bpw --last-only --base $HOME/w334/base-code.kld --base $HOME/w334/base-prose.kld > $O/kld-3.50bpw.log 2>&1 &
CHILD=$!; wait $CHILD; log "kld 3.50bpw rc=$? $(grep -E 'RESULT' $O/kld-3.50bpw.log | tr '\n' '|' | cut -c1-400)"; CHILD=""

# ---------- 4. EXL3 arms ----------
ex () {  # $1 tag, $2 model dir, $3 pool, $4 phases, rest: extra args. Returns exl3deep's rc (3 = too little free VRAM at load)
  local tag=$1 m=$2 cs=$3 ph=$4; shift 4
  timeout 2400 $PY $W/exl3deep.py -m $m -cs $cs -cq 8,8 -ambs 2 --tag $tag --phases "$ph" --min-free-mib $((FLOOR_FREE + 400)) "$@" > $O/$tag.log 2>&1 &
  CHILD=$!; wait $CHILD; local rc=$?; CHILD=""
  log "$tag rc=$rc $(grep -E '"event": "(loaded|done|too_little_free)"' $O/$tag.log | tr '\n' ' ' | cut -c1-500)"
  return $rc; }
phases () { echo "2000:1,100000:1,$(($1-3000)):1,$(($1/2-2000)):2"; }
EM=""; EC=""
for m in 3.50bpw 3.00bpw; do
  for cs in 229376 196608 163840; do
    ex A-$m-c$cs $M/Qwen3.8-27B-exl3-$m $cs "2000:1" -dm $DA; rc=$?
    [ $rc = 0 ] && { EM=$m; EC=$cs; break 2; }
  done
done
log "EXL3 choice: model=$EM pool=$EC"
if [ -n "$EM" ]; then
  MD=$M/Qwen3.8-27B-exl3-$EM
  ex A-dflash2 $MD $EC "$(phases $EC)" -dm $DA
  ex B-mtp2 $MD $EC "$(phases $EC)" -mtp -ndt 2
  ex B-mtp3 $MD $EC "2000:1,100000:1" -mtp -ndt 3
fi

# ---------- 5. mainline arms: the candidate's line with DFlash2 GGUF drafters ----------
COMMON="-m $CW -ngl 99 -t 8 -fa on -b 1024 -ub 512 -np 2 --kv-unified --jinja --host 0.0.0.0 --port <port> -ctk q8_0 -ctv q8_0"
cand () {  # $1 tag, rest: flags
  local tag=$1; shift
  [ -z "$(pids_on <port>)" ] || { log "$tag ABORT: <port> already served"; return 1; }
  ( exec distrobox enter llmbuild -- bash -lc "exec $CBIN $COMMON $*" > $O/server-$tag.log 2>&1 < /dev/null ) &
  local CP=$!
  if wait_up <port> $CP; then sleep 3; local SP=$(pids_on <port>); log "$tag up server=$SP exe=$(sha256sum "$(readlink -f /proc/$SP/exe)" 2>/dev/null | cut -c1-16) launch_vram_mib=$(vram) args: $*"; return 0; fi
  log "$tag FAILED to start"; grep -iE ' E |error|out of memory' $O/server-$tag.log | tail -4 >> $L; stop_port <port>; return 1; }
fillok () {  # $1 tag: two-slot fill, pass if free at peak >= the floor's
  python3 $PROBE fill <port> 2 $O/fill-$1.json >> $O/fill.log 2>&1
  python3 - "$O/fill-$1.json" $FLOOR_FREE "$1" >> $L <<'PYEOF'
import json, sys
d = json.load(open(sys.argv[1])); ok = d["vram_free_at_peak_mib"] >= int(sys.argv[2])
print(sys.argv[3], "fill peak", d["vram_peak_mib"], "free", d["vram_free_at_peak_mib"], "pass_vs_floor", ok, [r.get("prompt_n") for r in d["requests"]])
sys.exit(0 if ok else 1)
PYEOF
}
DF="--spec-type draft-dflash -ngld 99"
CC=""
for cs in 229376 196608 163840; do
  cand C-fill-c$cs -c $cs $DF -md $GZ || continue
  if fillok C-c$cs; then CC=$cs; stop_port <port>; break; fi
  stop_port <port>
done
log "C choice: pool=$CC"
if [ -n "$CC" ]; then
  cand C-dflash2-q4km -c $CC $DF -md $GZ && { python3 $DEEP deep <port> C-q4km $O/deep-C-q4km.json "$(phases $CC)" > $O/deep-C-q4km.log 2>&1; log "deep C-q4km rc=$?"; stop_port <port>; }
  cand Cp-q3km -c $CC $DF -md $G3 && { python3 $DEEP deep <port> Cp-q3km $O/deep-Cp-q3km.json "100000:1" > $O/deep-Cp-q3km.log 2>&1; log "deep Cp-q3km rc=$?"; stop_port <port>; }
  cand Cp-q2k -c $CC $DF -md $G2 && { python3 $DEEP deep <port> Cp-q2k $O/deep-Cp-q2k.json "100000:1" > $O/deep-Cp-q2k.log 2>&1; log "deep Cp-q2k rc=$?"; stop_port <port>; }
fi
log "phases done"
