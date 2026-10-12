#!/bin/bash
# arm.sh NAME CONFIG -- stop the floor's TabbyAPI (by PID: cwd + main.py), start CONFIG under the metrics hook, wait ready, sample at 1 Hz.
set -u
NAME=$1; CFG=$(readlink -f "$2"); W=$HOME/w-r3; T=$(readlink -f $HOME/src/tabbyAPI); V=$HOME/venvs/tabby154/bin/python
stop_tabby() {
  for p in $(pgrep -x python); do
    [ "$(readlink -f /proc/$p/cwd 2>/dev/null)" = "$T" ] && grep -qa -e main.py -e tabby_metrics.py -e tabby_crashdiag.py /proc/$p/cmdline || continue
    echo "stop pid $p $(date -u +%T)"; kill $p
    for i in $(seq 60); do kill -0 $p 2>/dev/null || break; sleep 1; done
    kill -0 $p 2>/dev/null && { echo "pid $p did not exit"; exit 1; }
  done
  [ -f $W/sampler.pid ] && kill $(cat $W/sampler.pid) 2>/dev/null; rm -f $W/sampler.pid
}
ready() { [ "$(curl -s -m5 -o /dev/null -w %{http_code} -H 'Content-Type: application/json' -d '{"model":"x","max_tokens":1,"messages":[{"role":"user","content":"hi"}]}' http://127.0.0.1:<port>/v1/chat/completions)" = 200 ]; }
stop_tabby
[ "$NAME" = stop ] && exit 0
echo "$NAME config $(sha256sum $CFG | cut -c1-16) start $(date -u +%T)"
( cd $T && R3_METRICS=$W/metrics-$NAME.jsonl exec setsid nohup $V $W/tabby_metrics.py --config $CFG ) > $W/tabby-$NAME.log 2>&1 < /dev/null &
for i in $(seq 75); do ready && break; sleep 4; done
ready || { echo "NOT READY"; tail -20 $W/tabby-$NAME.log; exit 1; }
PID=$(for p in $(pgrep -x python); do [ "$(readlink -f /proc/$p/cwd)" = "$T" ] && grep -qa tabby_metrics.py /proc/$p/cmdline && echo $p; done | head -1)
echo "$NAME ready $(date -u +%T) pid $PID"
( while :; do echo "$(date +%s),$(nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits),$(awk '/MemAvailable/{print int($2/1024)}' /proc/meminfo),$(awk '/VmRSS/{print int($2/1024)}' /proc/$PID/status 2>/dev/null)"; sleep 1; done > $W/samp-$NAME.csv 2>/dev/null < /dev/null ) > /dev/null 2>&1 &
echo $! > $W/sampler.pid
