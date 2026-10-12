#!/bin/bash
# r3 window: ABCCBA (forks capped 32768 in the first half, 8192 in the second), then restore r2 via the stock launcher.
set -u
B=<user>@<floor host>; W=$HOME/diet-inference-runs/w-r3; export R3_URL=http://<floor host>:<port>
ARMS=("A-r2:cfg-r2.yml:32768" "B-r2kv:cfg-r2kv.yml:32768" "C-r3:cfg-r3.yml:32768" "C-r3:cfg-r3.yml:8192" "B-r2kv:cfg-r2kv.yml:8192" "A-r2:cfg-r2.yml:8192")
echo "WINDOW START $(date -u +%FT%TZ)"
n=0
for a in "${ARMS[@]}"; do
  n=$((n+1)); IFS=: read -r name cfg fmax <<< "$a"; tag="$n-$name-f$fmax"
  echo "=== arm $tag $(date -u +%T)"
  ssh $B "~/w-r3/arm.sh $tag ~/w-r3/$cfg" || { echo "ARM START FAILED $tag"; break; }
  python3 $W/drive.py "$tag" "$fmax" $W/results.jsonl || echo "DRIVER FAILED $tag"
done
echo "=== restore r2 $(date -u +%T)"
ssh $B '~/w-r3/arm.sh stop x; ~/provision-exl3-27b.sh'
echo "WINDOW END $(date -u +%FT%TZ)"
