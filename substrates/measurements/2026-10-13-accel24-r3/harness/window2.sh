#!/bin/bash
# Follow-up window: 1) pool-edge diagnostic on r2 under the page-count hook, 2) ABCCBA with long trunk outputs,
# canary on r3 after arm 4. Leaves the arm-6 server up; the end state is set by hand after the numbers are read.
set -u
B=<user>@<floor host>; W=$HOME/diet-inference-runs/w-r3; export R3_URL=http://<floor host>:<port>
I=$HOME/git/me/diet-inference/instruments; P=$HOME/git/me/discipline/develop/substrates/admission/depth-probe
echo "WINDOW2 START $(date -u +%FT%TZ)"
echo "=== diag $(date -u +%T)"
ssh $B "~/w-r3/arm-diag.sh d-r2 ~/w-r3/cfg-r2.yml" < /dev/null || echo "DIAG START FAILED"
python3 $W/diag.py $W/diag.jsonl || echo "DIAG DRIVER FAILED"
ssh $B "grep -h crash_job ~/w-r3/metrics-d-r2.jsonl" < /dev/null > $W/diag-pages.jsonl; echo "diag pages: $(wc -l < $W/diag-pages.jsonl) records"
export R3_TRUNK_MODE=long R3_TRUNK_TOKENS=118000 R3_STEPS=6 R3_STEP_GAP=0 R3_EXTRA_COLD=0
ARMS=("A-r2:cfg-r2.yml:32768" "B-r2kv:cfg-r2kv.yml:32768" "C-r3:cfg-r3.yml:32768" "C-r3:cfg-r3.yml:8192" "B-r2kv:cfg-r2kv.yml:8192" "A-r2:cfg-r2.yml:8192")
n=0
for a in "${ARMS[@]}"; do
  n=$((n+1)); IFS=: read -r name cfg fmax <<< "$a"; tag="L$n-$name-f$fmax"
  echo "=== arm $tag $(date -u +%T)"
  ssh $B "~/w-r3/arm.sh $tag ~/w-r3/$cfg" < /dev/null || { echo "ARM START FAILED $tag"; break; }
  python3 $W/drive.py "$tag" "$fmax" $W/results2.jsonl || echo "DRIVER FAILED $tag"
  if [ $n = 4 ]; then
    echo "=== canary r3 $(date -u +%T)"
    python3 -B $P/tabby_canary.py --canary-dir $I --log $I/fixtures/canary-e0827.jsonl --fork e0827 --anchor common_chat_templates_apply --k 36 \
      --base-url $R3_URL --baseline $HOME/diet-inference-runs/w393/r2win/canary-baseline.json --substrate-label accel24-tabbyapi-exl3-qwen38-27b-3p00 \
      --misses $W/canary-r3-misses.jsonl > $W/canary-r3.log 2>&1; echo "canary rc=$? $(grep -E '^(verdict|rate)' $W/canary-r3.log | tr '\n' ' ')"
  fi
done
echo "WINDOW2 ARMS END $(date -u +%FT%TZ)"
