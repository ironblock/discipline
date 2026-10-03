#!/bin/bash
L=$HOME/setup/logs/exl3-run.log; : > $L
PY=$HOME/venvs/exl3/bin/python
B=$HOME/setup/exl3bench.py
M=$HOME/Models
run(){ tag=$1; shift
  echo "##### $(date +%T) $tag" >> $L
  timeout 900 $PY $B --tag "$tag" "$@" >> $L 2>&1
  echo "  rc=$? vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader)" >> $L
  sleep 5
}
run exl3-4.00      -m $M/Qwen3.8-27B-exl3-4.00bpw -cs 32768
run exl3-4.00-dfl  -m $M/Qwen3.8-27B-exl3-4.00bpw -cs 32768 -dm $M/Qwen3.8-27B-DFlash2-EXL3-4.00bpw
run exl3-5.00      -m $M/Qwen3.8-27B-exl3-5.00bpw -cs 32768
run exl3-5.00-dfl  -m $M/Qwen3.8-27B-exl3-5.00bpw -cs 32768 -dm $M/Qwen3.8-27B-DFlash2-EXL3-4.00bpw
run exl3-6.00-c8k  -m $M/Qwen3.8-27B-exl3-6.00bpw -cs 8192
echo "##### $(date +%T) DONE" >> $L
