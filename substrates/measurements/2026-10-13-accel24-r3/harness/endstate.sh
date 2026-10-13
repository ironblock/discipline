#!/bin/bash
# endstate.sh r3|r2 -- put CONFIG's bytes at config-floor.yml (the stock launcher's file), stop whatever serves, launch via
# provision-exl3-27b.sh detached (its subshell holds ssh pipes otherwise), wait READY, print the four confirmations.
set -u
WHICH=$1; B=<user>@<floor host>
ssh $B "set -e; cd ~/src/tabbyAPI
  [ -f config-floor.r2-1e17c49d.yml ] || cp -p config-floor.yml config-floor.r2-1e17c49d.yml
  if [ $WHICH = r3 ]; then cp ~/w-r3/cfg-r3.yml config-floor.yml; else cp config-floor.r2-1e17c49d.yml config-floor.yml; fi
  sha256sum config-floor.yml | cut -c1-16
  ~/w-r3/arm.sh stop x
  setsid nohup ~/provision-exl3-27b.sh > ~/w-r3/provision-$WHICH.log 2>&1 < /dev/null &
  for i in \$(seq 60); do grep -q READY ~/w-r3/provision-$WHICH.log && break; sleep 5; done; tail -1 ~/w-r3/provision-$WHICH.log" < /dev/null
ssh $B 'X=http://127.0.0.1:<port>; echo "up: health $(curl -s -m5 -o /dev/null -w %{http_code} $X/health)"
  curl -s -m5 $X/v1/model | python3 -c "import json,sys;m=json.load(sys.stdin);p=m.get(\"parameters\") or {};print(\"model:\",m[\"id\"],p.get(\"max_seq_len\"),p.get(\"cache_size\"),p.get(\"cache_mode\"),p.get(\"max_batch_size\"),\"vision\",p.get(\"use_vision\"))"
  grep -E "Warmup finished|vision_offload|Sampler overrides" ~/tabby-floor.log | tail -3 | cut -c1-110
  curl -s -m60 -H "Content-Type: application/json" -d "{\"model\":\"x\",\"max_tokens\":16,\"messages\":[{\"role\":\"user\",\"content\":\"Reply with the single word: ready\"}],\"chat_template_kwargs\":{\"enable_thinking\":false}}" $X/v1/chat/completions | python3 -c "import json,sys;r=json.load(sys.stdin);print(\"warmed:\",repr(r[\"choices\"][0][\"message\"][\"content\"]))"
  for p in $(pgrep -x python); do [ "$(readlink -f /proc/$p/cwd)" = "$(readlink -f ~/src/tabbyAPI)" ] && echo "settings: pid $p $(tr "\0" " " </proc/$p/cmdline) config $(sha256sum ~/src/tabbyAPI/config-floor.yml | cut -c1-16)"; done
  grep -nE "recurrent_checkpoint_interval_pp|sysmem_" ~/src/tabbyAPI/config-floor.yml' < /dev/null
