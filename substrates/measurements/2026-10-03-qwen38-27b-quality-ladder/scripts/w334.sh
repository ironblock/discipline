#!/bin/bash
# #334 window on linux-pc: Qwen3.8 27B KLD ladder against a Q8_0 reference.
# Stops the floor (port <port>), builds the reference logits with mainline 4ceb171, scores the four GSQ-RCO quants on
# mainline and on the floor's BeeLlama release (read-only use of its llama-perplexity), then EXL3 4/5/6 bpw through
# exl3kld.py. ALWAYS restores the floor via provision-diet.sh in the trap. Signals only pids found by pgrep whose
# command line names the floor's port, or the pids this script started.
set -u
W=$HOME/w334; L=$W/window.log; O=$W/out; mkdir -p $O; : > $L
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
[ "$(sha256sum $REF | cut -d' ' -f1)" = $REF_SHA ] || { log "ABORT: reference sha mismatch"; touch $W/restored; exit 1; }
for c in code prose; do log "corpus $c $(sha256sum $W/$c.txt | cut -c1-16)"; done
{ echo "main_commit $(git -C $HOME/src/llama.cpp rev-parse HEAD) dirty=$(git -C $HOME/src/llama.cpp status --porcelain | wc -l)"
  echo "main_perplexity $(sha256sum $MAIN | cut -d' ' -f1)"
  echo "bee_perplexity $(sha256sum $BEE/llama-perplexity | cut -d' ' -f1)"
  echo "bee_server $(sha256sum $BEE/llama-server | cut -d' ' -f1)"
  echo "exllamav3 $($PY -c 'import exllamav3.version as v;print(v.__version__)') torch $($PY -c 'import torch;print(torch.__version__)')"
  echo "exl3kld $(sha256sum $W/exl3kld.py | cut -d' ' -f1)"
  echo "ref $(basename $REF) $REF_SHA $(stat -c %s $REF)"; } > $O/identity.txt 2>&1
log "floor pid=$PROD; slots: $(curl -s -m5 localhost:<port>/slots | python3 -c 'import json,sys;print([s.get("is_processing") for s in json.load(sys.stdin)])' 2>&1)"
trap restore EXIT
stop_port <port> || exit 1; log "DOWNTIME START floor stopped vram=$(vram) mem_avail=$(free -g | awk '/Mem:/{print $7}')G"

# --- 1. reference logits: Q8_0 on mainline, partial offload (try fewer GPU layers on failure) ---
for c in code prose; do
  ok=0
  for ngl in 52 46 40 32; do
    distrobox enter llmbuild -- bash -lc "exec $MAIN -m $REF -ngl $ngl $KF -f $W/$c.txt --kl-divergence-base $W/base-$c.kld" > $O/ref-$c.log 2>&1
    rc=$?; log "ref $c ngl=$ngl rc=$rc $(grep -E 'Final estimate' $O/ref-$c.log | tr -s ' ')"
    [ $rc = 0 ] && grep -q 'Final estimate' $O/ref-$c.log && { ok=1; break; }
    rm -f $W/base-$c.kld
  done
  [ $ok = 1 ] || { log "ABORT: reference $c failed"; exit 1; }
  log "base-$c.kld $(stat -c %s $W/base-$c.kld) $(sha256sum $W/base-$c.kld | cut -c1-16)"
done

# --- 2. GSQ-RCO on mainline 4ceb171 ---
for q in IQ2_XS IQ2_S IQ3_XXS IQ3_S; do for c in code prose; do
  distrobox enter llmbuild -- bash -lc "exec $MAIN -m $R/Qwen3.8-27B-GSQ-RCO-$q-mtp.gguf -ngl 99 $KF -f $W/$c.txt --kl-divergence-base $W/base-$c.kld --kl-divergence" > $O/kld-main-$q-$c.log 2>&1
  log "main $q $c rc=$? $(summ $O/kld-main-$q-$c.log)"
done; done

# --- 3. GSQ-RCO on the floor's BeeLlama release (its binary, run read-only from its own directory) ---
for q in IQ2_XS IQ2_S IQ3_XXS IQ3_S; do for c in code prose; do
  ( cd $BEE && exec ./llama-perplexity -m $R/Qwen3.8-27B-GSQ-RCO-$q-mtp.gguf -ngl 99 $KF -f $W/$c.txt --kl-divergence-base $W/base-$c.kld --kl-divergence ) > $O/kld-bee-$q-$c.log 2>&1
  rc=$?; log "bee $q $c rc=$rc $(summ $O/kld-bee-$q-$c.log)"
  [ $rc = 0 ] || { log "bee failed on $q; skipping the remaining bee rows"; break 2; }
done; done

# --- 4. EXL3 through exl3kld.py ---
cd $W
$PY exl3kld.py -m $M/Qwen3.8-27B-exl3-4.00bpw --tag eq-full --max-chunks 2 --base base-code.kld > $O/exl3-eq-full.log 2>&1
log "exl3 4.00 equivalence, full logits rc=$? $(grep -E '^RESULT|loaded' $O/exl3-eq-full.log | tr '\n' '|')"
$PY exl3kld.py -m $M/Qwen3.8-27B-exl3-4.00bpw --tag eq-last --max-chunks 2 --last-only --base base-code.kld > $O/exl3-eq-last.log 2>&1
log "exl3 4.00 equivalence, last-only rc=$? $(grep -E '^RESULT|loaded' $O/exl3-eq-last.log | tr '\n' '|')"
for b in 4.00 5.00 6.00; do
  $PY exl3kld.py -m $M/Qwen3.8-27B-exl3-${b}bpw --tag exl3-$b --last-only --base base-code.kld --base base-prose.kld > $O/exl3-$b.log 2>&1
  log "exl3 $b rc=$? $(grep -E '^RESULT|loaded|Error' $O/exl3-$b.log | tr '\n' '|' | cut -c1-600)"
done
log "arms done"
