#!/bin/bash
export PATH="$HOME/.local/bin:$PATH"
L=$HOME/setup/logs/exl3-dfl.log; : > $L
PY=$HOME/venvs/exl3/bin/python
B=$HOME/setup/exl3bench.py
M=$HOME/Models
D=$M/Qwen3.8-27B-DFlash2-hf
echo "### $(date +%T) fetch HF drafter" >> $L
[ -d "$D" ] || hf download z-lab/Qwen3.8-27B-DFlash2 --local-dir $D >> $L 2>&1
echo "  rc=$? $(du -sh $D 2>/dev/null | cut -f1)" >> $L
run(){ tag=$1; shift
  echo "##### $(date +%T) $tag" >> $L
  timeout 900 $PY $B --tag "$tag" "$@" >> $L 2>&1
  echo "  rc=$? vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader)" >> $L
  sleep 5
}
run exl3-4.00-dfl2 -m $M/Qwen3.8-27B-exl3-4.00bpw -cs 32768 -dm $D
run exl3-5.00-dfl2 -m $M/Qwen3.8-27B-exl3-5.00bpw -cs 32768 -dm $D
echo "##### $(date +%T) DONE" >> $L
