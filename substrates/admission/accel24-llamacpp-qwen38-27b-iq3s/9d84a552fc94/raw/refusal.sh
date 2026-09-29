#!/bin/bash
# The kwarg-delivery control's refused-level row (#143, planning 2026-09-29), production up.
# Candidate: the same binary and weights, launched CPU-only (GPU hidden, weights memory-mapped with no
# repack and no warmup, so no weight is touched and the floor's card and memory are undisturbed), on
# localhost; the refusal happens in template rendering, before generation. Floor: its production line.
# Never pkill -f: the candidate is stopped by pgrep -x plus its port.
set -u
W=$HOME/w143/refusal; mkdir -p $W; L=$W/log
CBIN=$HOME/src/llama.cpp/build-cuda/bin/llama-server
CW=$HOME/Models/Qwen3.8-27B-GSQ-RCO/Qwen3.8-27B-GSQ-RCO-IQ3_S-mtp.gguf
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
pid_on () { for p in $(pgrep -x llama-server); do tr '\0' ' ' < /proc/$p/cmdline | grep -q -- "--port $1 " && echo $p; done; }
stop () { for p in $(pid_on 8093); do kill $p; done; }
trap stop EXIT
log "floor before: pid=$(pid_on 8082) vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader) mem_avail_mib=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)"
( exec distrobox enter llmbuild -- bash -lc "CUDA_VISIBLE_DEVICES= exec $CBIN -m $CW -ngl 0 -t 2 -c 4096 -np 1 --jinja --no-repack --no-warmup --host 127.0.0.1 --port 8093" > $W/server.log 2>&1 < /dev/null ) &
for i in $(seq 1 120); do curl -s -m 3 localhost:8093/health | grep -q ok && break; sleep 2; done
log "candidate cpu-only up: pid=$(pid_on 8093) exe=$(sha256sum $(readlink -f /proc/$(pid_on 8093)/exe) | cut -c1-16) mem_avail_mib=$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo)"
python3 - "$W" <<'EOF'
import json, sys, urllib.request, urllib.error, hashlib
W = sys.argv[1]; out = {}
def req(port, path, body):
    r = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    try:
        with urllib.request.urlopen(r, timeout=120) as resp: return resp.status, resp.read().decode()
    except urllib.error.HTTPError as e: return e.code, e.read().decode()
q = [{"role": "user", "content": "What is 12 times 12? Answer with the number."}]
for port, rung in ((8093, "candidate"), (8082, "floor")):
    out[rung] = {}
    for level in ("high", "none", "max", "low"):
        if rung == "candidate" and level == "low":
            st, body = req(port, "/apply-template", {"messages": q, "add_generation_prompt": True, "chat_template_kwargs": {"reasoning_effort": level}})
            out[rung][level] = {"endpoint": "/apply-template", "status": st, "rendered_sha256": hashlib.sha256(body.encode()).hexdigest()}
            continue
        st, body = req(port, "/v1/chat/completions", {"messages": q, "max_tokens": 1, "chat_template_kwargs": {"reasoning_effort": level}})
        out[rung][level] = {"endpoint": "/v1/chat/completions", "status": st, "body_sha256": hashlib.sha256(body.encode()).hexdigest(),
                            "error_type": (json.loads(body).get("error", {}) or {}).get("type") if st >= 400 else None, "body_head": body[:240]}
json.dump(out, open(f"{W}/refusal.json", "w"), indent=1)
for r, v in out.items(): print(r, {k: x["status"] for k, x in v.items()})
EOF
log "probe rc=$?"
stop; for i in $(seq 1 30); do [ -z "$(pid_on 8093)" ] && break; sleep 1; done
log "floor after: pid=$(pid_on 8082) vram=$(nvidia-smi --query-gpu=memory.used --format=csv,noheader) health=$(curl -s -m5 localhost:8082/health)"
