#!/bin/bash
# The floor's T2 depth window (#143), production up. The probe from main/ at 10d866e.
set -u
W=$HOME/diet-inference-runs/t2-floor; OUT=$W/out; P=$HOME/git/me/discipline/main/substrates/admission/depth-probe
BH=$(cat $HOME/diet-inference-runs/ada-142/.boxhost)
# the floor binds 0.0.0.0 on the LAN; the forward through sshd was refused (2026-09-29), so the probe calls it directly
curl -s -m5 http://$BH:8082/health | grep -q ok || { echo "floor not healthy"; exit 2; }
echo "$(date -u +%FT%TZ) start head=$(git -C $HOME/git/me/discipline/main rev-parse --short HEAD)"
python3 -B $P/depth_probe.py run --endpoint http://$BH:8082 --corpus $P/corpus/manifest.json \
  --repo tokio=$HOME/diet-inference-runs/corpora/tokio --tier supported --serving-context 160000 --samples 5 \
  --sampler '{"temperature": 0.6, "top_k": 20, "top_p": 1.0, "min_p": 0.0}' --out $OUT
rc=$?; echo "$(date -u +%FT%TZ) run rc=$rc"
python3 -B $P/depth_probe.py summarise $OUT/rows.jsonl > $OUT/summary.json; echo "summarise rc=$?"
python3 -B $P/depth_probe.py decide $OUT/summary.json $P/criterion.toml > $OUT/decide.json; echo "decide rc=$?"
cat $OUT/decide.json
