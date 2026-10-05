#!/bin/bash
# rtx6000ada-host production -> YaRN x2 (maintainer-approved 2026-10-04; #401). The served name `qwen3.8-flash-next` is a symlink in
# the models directory; it is repointed from the native 2.05bpw directory to the YaRN x2 overlay (the same files by symlink, one
# edited config.json), and max_seq_len goes 262144 -> 524288. cache_size stays 606208. Any failure rolls back to the native
# symlink and the saved config. Never pkill -f.
set -u
W=$HOME/w402; L=$W/yarnprod.log; O=$W/out; mkdir -p $O; : > $L
T=$HOME/src/tabbyAPI; V=$HOME/venvs/tabby154/bin/python; MD=$HOME/models
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
vram () { nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits; }
tabby_pids () { for p in $(pgrep -x python) $(pgrep -x python3); do
    [ "$(readlink /proc/$p/cwd 2>/dev/null)" = "$T" ] && tr '\0' ' ' < /proc/$p/cmdline | grep -q "main.py" && echo $p; done; }
tabby_stop () { for p in $(tabby_pids); do kill $p; done
  for i in $(seq 1 45); do [ -z "$(tabby_pids)" ] && return 0; sleep 1; done
  for p in $(tabby_pids); do kill -9 $p; done; sleep 3; }
KEY=$(awk '/^api_key:/{print $2}' $T/api_tokens.yml)
healthy () { curl -s -m5 -H "Authorization: Bearer $KEY" localhost:<port>/health 2>/dev/null | grep -q healthy; }
start () { (cd $T && exec nohup $V main.py) > /tmp/prod-tabby.log 2>&1 < /dev/null &
  for i in $(seq 1 200); do healthy && return 0; sleep 3; done; return 1; }
OLD=$(readlink $MD/qwen3.8-flash-next)
[ "$OLD" = Qwen3.8-Flash-Next-exl3-2.05bpw ] || { log "ABORT: served symlink is $OLD"; exit 1; }
[ "$(sha256sum $T/config.yml | cut -c1-16)" = 64d7ca5684298282 ] || { log "ABORT: live config is not the post-#401 config"; exit 1; }
cp $T/config.yml $T/config.yml.pre-yarn
DONE=0
rollback () { [ $DONE = 1 ] && return; log "ROLLBACK"; tabby_stop; ln -sfn $OLD $MD/qwen3.8-flash-next; cp $T/config.yml.pre-yarn $T/config.yml; start; log "rolled back: health=$(healthy && echo ok || echo FAIL) pid=$(tabby_pids)"; }
trap rollback EXIT
tabby_stop; log "DOWNTIME START vram=$(vram)"
ln -sfn Qwen3.8-Flash-Next-exl3-2.05bpw-yarn2 $MD/qwen3.8-flash-next
sed -i 's/^\(  *max_seq_len:\).*/\1 524288/' $T/config.yml
log "symlink -> $(readlink $MD/qwen3.8-flash-next); config $(sha256sum $T/config.yml | cut -c1-16); diff: $(diff $T/config.yml.pre-yarn $T/config.yml | tr '\n' ' ')"
start || { log "start FAILED: $(grep -iE 'error|exception' /tmp/prod-tabby.log | tail -2 | tr '\n' '|')"; exit 1; }
P=$(tabby_pids)
M=$(curl -s -m10 -H "Authorization: Bearer $KEY" localhost:<port>/v1/model)
log "up: pid=$P vram=$(vram) model=$(echo "$M" | python3 -c 'import json,sys;d=json.load(sys.stdin);p=d["parameters"];print(d["id"],p["max_seq_len"],p["cache_size"],p["cache_mode"])')"
echo "$M" | python3 -c 'import json,sys;sys.exit(0 if json.load(sys.stdin)["parameters"]["max_seq_len"]==524288 else 1)' || { log "max_seq_len not applied"; exit 1; }
python3 $HOME/setup/abwin/contcheck.py <port> $KEY > $O/cont-prod-yarn.log 2>&1; log "contcheck rc=$?"
DONE=1; log "DOWNTIME END: production on YaRN x2"
