#!/bin/bash
S=$HOME/setup; L=$S/logs/rco27b.log; : > $L
M=$HOME/Models/Qwen3.8-27B-GSQ-RCO
MAIN=$HOME/src/llama.cpp/build-cuda/bin
IK=$HOME/src/ik_llama.cpp/build-cuda/bin
QS="IQ2_XS IQ2_S IQ3_XXS IQ3_S"
dbx(){ distrobox enter llmbuild -- bash -lc "$1" >> $L 2>&1; }

echo "##### $(date +%T) PHASE A: llama-bench (mainline)" >> $L
for q in $QS; do
  echo "--- bench $q" >> $L
  dbx "$MAIN/llama-bench -m $M/Qwen3.8-27B-GSQ-RCO-$q-mtp.gguf -ngl 99 -t 8 -p 2048 -n 128 -fa 1 2>&1 | tail -6"
done

echo "##### $(date +%T) PHASE B: server plain (mainline)" >> $L
for q in $QS; do dbx "bash $S/ab.sh main-$q $MAIN/llama-server $M/Qwen3.8-27B-GSQ-RCO-$q-mtp.gguf"; done

echo "##### $(date +%T) PHASE C: server + MTP n_max=2 (mainline, nextn inline)" >> $L
for q in $QS; do dbx "bash $S/ab.sh mainmtp-$q $MAIN/llama-server $M/Qwen3.8-27B-GSQ-RCO-$q-mtp.gguf --spec-type draft-mtp --spec-draft-n-max 2"; done

echo "##### $(date +%T) PHASE D: server plain (ik)" >> $L
for q in $QS; do dbx "bash $S/ab.sh ik-$q $IK/llama-server $M/Qwen3.8-27B-GSQ-RCO-$q-mtp.gguf"; done

echo "##### $(date +%T) DONE" >> $L
