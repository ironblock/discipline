#!/bin/bash
PPL=$HOME/src/llama-q8sparse/build-cuda/bin/llama-perplexity
M=$(ls $HOME/models/Qwen3.8-Flash-Next/UD-Q4_K_XL/*00001-of-*.gguf)
for c in code prose; do
  echo "##### udq4 $c $(date -u +%FT%TZ)"
  CUDA_VISIBLE_DEVICES= nice $PPL -m $M -t 16 -c 4096 -b 4096 -ub 512 --chunks 12 -f $HOME/setup/kld/$c.txt --kl-divergence-base $HOME/setup/kld/base-$c.kld --kl-divergence > $HOME/setup/kld/kld-udq4-$c.log 2>&1
  echo "rc=$? $(date -u +%FT%TZ) $(grep -E "Mean PPL\(Q\)|Mean    KLD|Same top p" $HOME/setup/kld/kld-udq4-$c.log | tr -s " " | tr "\n" "|")"
done
echo UDQ4-DONE
