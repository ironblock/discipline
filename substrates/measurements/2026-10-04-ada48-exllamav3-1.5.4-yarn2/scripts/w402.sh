#!/bin/bash
# YaRN x2 trial on rtx6000ada-host (maintainer-approved 2026-10-04; recorded under #401). The serving 2.05bpw weights through a
# second model directory of symlinks whose config.json sets the card's YaRN rope_parameters at factor 2 (original 262144,
# max_position_embeddings 524288). (1) KLD on our corpora with YaRN x2 (the static-YaRN short-text cost; native is 0.0860 /
# 0.2297); (2) three-needle retrieval through TabbyAPI on a side port: native at ~250k, YaRN at ~250k and ~480k; (3) decode at
# 10k and ~100k with YaRN. Production is restored on the live 1.5.4 config by the trap. Never pkill -f.
set -u
W=$HOME/w402; L=$W/window.log; O=$W/out; mkdir -p $O; : > $L
S=$HOME/setup; T=$HOME/src/tabbyAPI; V=$HOME/venvs/tabby154/bin/python
MN=$HOME/models/Qwen3.8-Flash-Next-exl3-2.05bpw; MY=$HOME/models/Qwen3.8-Flash-Next-exl3-2.05bpw-yarn2
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
vram () { nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits; }
tabby_pids () { for p in $(pgrep -x python) $(pgrep -x python3); do
    [ "$(readlink /proc/$p/cwd 2>/dev/null)" = "$T" ] && tr '\0' ' ' < /proc/$p/cmdline | grep -q "main.py" && echo $p; done; }
tabby_stop () { for p in $(tabby_pids); do kill $p; done
  for i in $(seq 1 45); do [ -z "$(tabby_pids)" ] && return 0; sleep 1; done
  for p in $(tabby_pids); do log "tabby pid $p ignored TERM; KILL"; kill -9 $p; done; sleep 3; }
KEY=$(awk '/^api_key:/{print $2}' $T/api_tokens.yml)
CHILD=""
restore () {
  log "restore: begin"; [ -n "$CHILD" ] && kill -9 $CHILD 2>/dev/null; tabby_stop
  (cd $T && exec nohup $V main.py) > /tmp/prod-tabby.log 2>&1 < /dev/null &
  for i in $(seq 1 200); do curl -s -m5 -H "Authorization: Bearer $KEY" localhost:<port>/health 2>/dev/null | grep -q healthy && break; sleep 3; done
  local P=$(tabby_pids)
  log "restored: pid=$P venv=$(tr '\0' ' ' < /proc/$P/cmdline | cut -d' ' -f1) cache_size=$(curl -s -m5 -H "Authorization: Bearer $KEY" localhost:<port>/v1/model | python3 -c 'import json,sys;print(json.load(sys.stdin)["parameters"]["cache_size"])' 2>&1) config=$(sha256sum $T/config.yml | cut -c1-16) vram=$(vram)"
  touch $W/restored; log END
}
rm -f $W/restored
# ---------- the YaRN model directory (symlinks + one edited config.json) ----------
mkdir -p $MY; for f in $MN/*; do [ "$(basename $f)" = config.json ] || ln -sfn $f $MY/$(basename $f); done
python3 - $MN/config.json $MY/config.json <<'PYEOF'
import json, sys
c = json.load(open(sys.argv[1])); t = c["text_config"]
t["rope_parameters"] = dict(t["rope_parameters"], rope_type="yarn", factor=2.0, original_max_position_embeddings=262144)
t["max_position_embeddings"] = 524288
json.dump(c, open(sys.argv[2], "w"), indent=2)
print(t["rope_parameters"])
PYEOF
log "yarn dir ready: config $(sha256sum $MY/config.json | cut -c1-16) rope=$(python3 -c "import json;print(json.load(open('$MY/config.json'))['text_config']['rope_parameters'])")"
[ "$(sha256sum $T/config.yml | cut -c1-16)" = 64d7ca5684298282 ] || { log "ABORT: live config is not the post-#401 config"; touch $W/restored; exit 1; }
side () {  # $1 model dir name, $2 max_seq_len -> side config path
  sed -e 's/^\(  *port:\).*/\1 <side-port>/' -e 's/^\(  *disable_auth:\).*/\1 true/' -e "s/^\(  *model_name:\).*/\1 $1/" -e "s/^\(  *max_seq_len:\).*/\1 $2/" $T/config.yml > $T/config-402-$1.yml; echo config-402-$1.yml; }
up () { for i in $(seq 1 200); do curl -s -m5 localhost:<side-port>/health 2>/dev/null | grep -q healthy && return 0; kill -0 $1 2>/dev/null || return 1; sleep 3; done; return 1; }
PROD=$(tabby_pids); [ -n "$PROD" ] || { log "ABORT: no production"; touch $W/restored; exit 1; }
log "preflight ok; prod pid=$PROD"
trap restore EXIT
tabby_stop; log "DOWNTIME START vram=$(vram)"
cd $S
# ---------- 1. KLD with YaRN x2 ----------
timeout 1800 $V $S/exl3kld.py -m $MY -ngr --tag exl3-205-v154-yarn2 --base kld/base-code.kld --base kld/base-prose.kld > $O/kld-yarn2.log 2>&1 &
CHILD=$!; wait $CHILD; log "kld yarn2 rc=$? $(grep -h '^RESULT' $O/kld-yarn2.log | tr '\n' '|' | cut -c1-500)"; CHILD=""
# ---------- 2. native at ~250k ----------
C=$(side Qwen3.8-Flash-Next-exl3-2.05bpw 262144)
(cd $T && exec $V main.py --config $C) > $O/tabby-native.log 2>&1 < /dev/null & CHILD=$!
if up $CHILD; then log "native up vram=$(vram)"
  python3 $W/needle.py <side-port> 250000 native-250k $O/needle-native-250k.json >> $L 2>&1
else log "native FAILED to start"; fi
tabby_stop; CHILD=""
# ---------- 3. YaRN x2 at ~250k and ~480k, then decode ----------
C=$(side Qwen3.8-Flash-Next-exl3-2.05bpw-yarn2 524288)
(cd $T && exec $V main.py --config $C) > $O/tabby-yarn2.log 2>&1 < /dev/null & CHILD=$!
if up $CHILD; then log "yarn2 up vram=$(vram) max_seq_len=$(curl -s localhost:<side-port>/v1/model | python3 -c 'import json,sys;print(json.load(sys.stdin)["parameters"]["max_seq_len"])' 2>&1)"
  python3 $W/needle.py <side-port> 250000 yarn2-250k $O/needle-yarn2-250k.json >> $L 2>&1
  python3 $W/needle.py <side-port> 480000 yarn2-480k $O/needle-yarn2-480k.json >> $L 2>&1
  PHASES=10000:1,100000:1 python3 $S/abwin/deep2.py <side-port> yarn2 > $O/deep-yarn2.log 2>&1; log "yarn2 deep rc=$?"
else log "yarn2 FAILED to start: $(grep -iE 'error|exception' $O/tabby-yarn2.log | tail -3 | tr '\n' '|')"; fi
tabby_stop; CHILD=""
log "phases done"
