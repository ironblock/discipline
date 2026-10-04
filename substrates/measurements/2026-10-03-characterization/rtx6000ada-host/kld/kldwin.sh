#!/bin/bash
# KLD window: stop production, score 4 trunk x PLE candidates against the CPU-generated Q8_0 reference, restore production.
L=$HOME/setup/kld/window.log; : > $L
PPL=$HOME/src/llama-q8sparse/build-cuda/bin/llama-perplexity
M=$HOME/models
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
restore () {
  log "restore: begin"
  cd $HOME/src/llama-prod2 && nohup $(cat $HOME/setup/prod-cmdline.txt) > /tmp/prod-server.log 2>&1 < /dev/null &
  for i in $(seq 1 90); do curl -s -H "Authorization: Bearer <api-key>" localhost:<port>/health | grep -q ok && break; sleep 2; done
  log "RESTORED $(curl -s -H 'Authorization: Bearer <api-key>' localhost:<port>/props | python3 -c 'import json,sys;print(json.load(sys.stdin).get("build_info"))') exe=$(readlink /proc/$(pgrep -x llama-server)/exe)"
}
trap restore EXIT
P=$(pgrep -x llama-server); kill $P; for i in $(seq 1 20); do pgrep -x llama-server >/dev/null || break; sleep 1; done
pgrep -x llama-server >/dev/null && { kill $P; sleep 10; }; pgrep -x llama-server >/dev/null && kill -9 $P
log "DOWNTIME START prod pid $P stopped"
declare -A C=(
  [q2_iq4nl]=$M/Qwen3.8-Flash-Next-GSQ-RCO/Q2_0/Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00001-of-00002.gguf
  [q2_bf16]=$M/kld/q2bf16/Qwen3.8-Flash-Next-GSQ-RCO-Q2_0-00001-of-00002.gguf
  [coder_iq4nl]=$M/Qwen3.8-Flash-Next-GSQ-RCO-Coder/IQ1_M/Qwen3.8-Flash-Next-GSQ-RCO-IQ1_M-00001-of-00002.gguf
  [coder_bf16]=$M/kld/coderbf16/Qwen3.8-Flash-Next-GSQ-RCO-IQ1_M-00001-of-00002.gguf )
for arm in q2_iq4nl coder_iq4nl q2_bf16 coder_bf16; do
  for c in code prose; do
    $PPL -m ${C[$arm]} -ngl 99 -t 16 -fa on -c 4096 -b 4096 -ub 512 --chunks 12 -f $HOME/setup/kld/$c.txt \
       --kl-divergence-base $HOME/setup/kld/base-$c.kld --kl-divergence > $HOME/setup/kld/kld-$arm-$c.log 2>&1
    log "$arm $c rc=$? $(grep -E 'Mean PPL\(Q\) |Mean    KLD|99.9%   KLD|Same top p' $HOME/setup/kld/kld-$arm-$c.log | tr -s ' ' | tr '\n' '|')"
  done
done
log "arms done"
