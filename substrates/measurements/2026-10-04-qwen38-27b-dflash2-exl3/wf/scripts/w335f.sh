#!/bin/bash
# #335 follow-up (f) on linux-pc: EXL3 plain 3.00bpw + DFlash2 EXL3 drafter on ExLlamaV3 1.5.4, cache 8,8 -- (1) the in-process depth bench
# at the largest pool that loads; (2) the same line served by TabbyAPI be74bf0a with the BF16 vision tower (vision: true,
# vision_offload: true), #373's three image requests against it, and VRAM sampled every 0.2 s through them. Measurement only.
# (headroom is recorded at the run's peak and judged against the floor's 1,208 MiB afterwards).
#   floor reading (production up) -> stop floor -> EXL3 3.50 KLD -> EXL3 arm A (DFlash2 EXL3 drafter) and arm B (MTP)
#   -> mainline arm C (candidate IQ3_S + DFlash2 Q4_K_M GGUF) and C' (smaller GGUF drafters) -> restore.
# ALWAYS restores the floor via provision-diet.sh in the trap. Signals only pids found by pgrep -x whose command line
# names the port, or the python children this script started. Never pkill -f.
set -u
W=$HOME/w335f; L=$W/window.log; O=$W/out; mkdir -p $O; : > $L
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
MD=$M/Qwen3.8-27B-exl3-3.00bpw
EC=""
for cs in 196608 188416 180224 172032 163840; do
  ex P-c$cs $MD $cs "2000:1" -dm $DA -cq 8,8 && { EC=$cs; break; }
done
log "pool choice (cq 8,8): $EC"
[ -n "$EC" ] && ex P3-dflash2-cq88 $MD $EC "$(phases $EC)" -dm $DA -cq 8,8
# ---------- TabbyAPI + vision ----------
TB=$HOME/src/tabbyAPI; TV=$HOME/venvs/tabby154/bin/python
cat > $TB/config-w335f.yml <<YML
network:
  host: 127.0.0.1
  port: 8083
  disable_auth: true
logging:
  log_prompt: false
  log_generation_params: false
  log_requests: false
model:
  model_dir: $M
  model_name: Qwen3.8-27B-exl3-3.00bpw
  backend: exllamav3
  max_seq_len: ${EC:-163840}
  cache_size: ${EC:-163840}
  cache_mode: "8,8"
  max_batch_size: 2
  chunk_size: 2048
  reasoning: true
  vision: true
  vision_offload: true
draft_model:
  draft_mode: model
  draft_model_dir: $M
  draft_model_name: Qwen3.8-27B-DFlash2-EXL3-4.00bpw-igor255
YML
cp $TB/config-w335f.yml $O/tabby-config.yml
( cd $TB && exec $TV main.py --config config-w335f.yml ) > $O/tabby.log 2>&1 < /dev/null &
TP=$!; CHILD=$TP
for i in $(seq 1 200); do curl -s -m5 localhost:8083/health 2>/dev/null | grep -q healthy && break; kill -0 $TP 2>/dev/null || break; sleep 3; done
if curl -s -m5 localhost:8083/health 2>/dev/null | grep -q healthy; then
  log "tabby up after ~$((i*3))s vram=$(vram) model=$(curl -s localhost:8083/v1/model | python3 -c 'import json,sys;d=json.load(sys.stdin);p=d.get("parameters",{});print(d.get("id"),p.get("cache_size"),p.get("cache_mode"),p.get("vision"),p.get("draft"))' 2>&1 | cut -c1-200)"
  ( while kill -0 $TP 2>/dev/null; do echo "$(date -u +%T.%N | cut -c1-12) $(vram)"; sleep 0.2; done ) > $O/vram-trace.txt 2>&1 &
  VS=$!
  mkdir -p $O/vision && cp $HOME/w335/vision/{image.png,make_image.py,token.sha256,derive.py} $O/vision/
  echo "MARK vision-start $(date -u +%T.%N | cut -c1-12)" >> $O/vram-trace.txt
  python3 $HOME/w335/vision/run_cell.py http://127.0.0.1:8083 $O/vision > $O/vision/run.log 2>&1; log "vision cell rc=$? $(tr '\n' ' ' < $O/vision/run.log)"
  echo "MARK vision-end $(date -u +%T.%N | cut -c1-12)" >> $O/vram-trace.txt
  python3 $O/vision/derive.py $O/vision > $O/vision/derive.json 2>&1; log "vision derive: $(cat $O/vision/derive.json)"
  python3 - $O > $O/tabby-text.json 2>&1 <<'PYEOF'
import json, sys, time, urllib.request, glob, os
home = os.path.expanduser("~"); out = sys.argv[1]
files = sorted(set(f for pat in ["src/*.cpp", "common/*.cpp", "ggml/src/**/*.c"] for f in glob.glob(home + "/git/llama.cpp/" + pat, recursive=True)))
corpus = "".join(open(f, errors="replace").read() for f in files)
Q = "\n\nReview the code above. Identify the three most likely bugs, explain each, and propose a fix with code."
res = []
for chars in (6000, 300000):
    body = {"messages": [{"role": "user", "content": corpus[:chars] + Q}], "max_tokens": 256, "temperature": 0, "top_k": 1}
    for rep in range(2):
        t0 = time.time()
        r = json.load(urllib.request.urlopen(urllib.request.Request("http://127.0.0.1:8083/v1/chat/completions", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"}), timeout=1800))
        res.append({"chars": chars, "rep": rep, "wall_s": round(time.time() - t0, 1), "usage": r.get("usage"), "timings": r.get("timings")})
print(json.dumps(res, indent=1))
PYEOF
  log "tabby text rc=$? $(python3 -c "import json;d=json.load(open('$O/tabby-text.json'));print([(x['chars'],x['rep'],(x.get('timings') or {}).get('predicted_per_second')) for x in d])" 2>&1 | cut -c1-300)"
  kill $VS 2>/dev/null
  python3 - $O/vram-trace.txt >> $L <<'PYEOF'
import sys
rows = [l.split() for l in open(sys.argv[1]) if l[0].isdigit()]
v = [int(r[1]) for r in rows if len(r) > 1 and r[1].isdigit()]
print("tabby vram trace: samples", len(v), "min", min(v), "peak", max(v), "free_at_peak", 24576 - max(v))
PYEOF
else
  log "tabby FAILED to start: $(grep -iE 'error|exception' $O/tabby.log | tail -4 | tr '\n' '|')"
fi
kill $TP 2>/dev/null; wait $TP 2>/dev/null; CHILD=""; sleep 3
log "phases done"
