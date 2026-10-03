#!/bin/bash
PPL=$HOME/src/llama-q8sparse/build-cuda/bin/llama-perplexity
REF=$HOME/models/Qwen3.8-Flash-Next/Q8_0/Qwen3.8-Flash-Next-Q8_0-00001-of-00006.gguf
for c in code prose; do
  echo "##### ref $c $(date -u +%T)"
  CUDA_VISIBLE_DEVICES= $PPL -m $REF -t 16 -c 4096 -b 4096 -ub 512 --chunks 12 -f $HOME/setup/kld/$c.txt --kl-divergence-base $HOME/setup/kld/base-$c.kld 2>&1 | grep -E "Final estimate|\[1\]|seconds per pass|error|failed|model type|file type"
  echo "rc=${PIPESTATUS[0]} $(date -u +%T)"
done
echo REF-DONE
