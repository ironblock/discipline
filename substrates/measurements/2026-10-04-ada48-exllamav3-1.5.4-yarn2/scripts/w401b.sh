#!/bin/bash
# #401 switch on rtx6000ada-host, approved by the maintainer 2026-10-04: size the 1.5.4 KV pool to the 300 MiB bar
# (size401.py), then serve production on ExLlamaV3 1.5.4 with that cache_size. If anything fails before the switch
# completes, the trap restores production on 1.5.2 with the original config. Signals only TabbyAPI main.py pids whose
# cwd is the TabbyAPI tree. Never pkill -f.
set -u
W=$HOME/w401; L=$W/window-b.log; O=$W/out; : > $L; : > $W/size.log
T=$HOME/src/tabbyAPI; V152=$HOME/venvs/tabby/bin/python; V154=$HOME/venvs/tabby154/bin/python
log () { echo "$(date -u +%FT%TZ) $*" >> $L; }
vram () { nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits; }
tabby_pids () { for p in $(pgrep -x python) $(pgrep -x python3); do
    [ "$(readlink /proc/$p/cwd 2>/dev/null)" = "$T" ] && tr '\0' ' ' < /proc/$p/cmdline | grep -q "main.py" && echo $p; done; }
tabby_stop () { for p in $(tabby_pids); do kill $p; done
  for i in $(seq 1 45); do [ -z "$(tabby_pids)" ] && return 0; sleep 1; done
  for p in $(tabby_pids); do log "tabby pid $p ignored TERM; KILL"; kill -9 $p; done; sleep 3; }
KEY=$(awk '/^api_key:/{print $2}' $T/api_tokens.yml)
healthy () { curl -s -m5 -H "Authorization: Bearer $KEY" localhost:<port>/health 2>/dev/null | grep -q healthy; }
start_prod () {  # $1 python
  (cd $T && exec nohup $1 main.py) > /tmp/prod-tabby.log 2>&1 < /dev/null &
  for i in $(seq 1 200); do healthy && return 0; sleep 3; done; return 1; }
SWITCHED=0
restore () {
  [ $SWITCHED = 1 ] && { touch $W/restored-b; log END; return; }
  log "restore to 1.5.2: begin"; tabby_stop
  cp $T/config.yml.pre401b $T/config.yml
  start_prod $V152; local P=$(tabby_pids)
  log "restored 1.5.2: pid=$P health=$(healthy && echo ok || echo FAIL) cache_size=$(awk '/cache_size:/{print $2}' $T/config.yml) vram=$(vram)"
  touch $W/restored-b; log END
}
rm -f $W/restored-b
cp $T/config.yml $T/config.yml.pre401b
PROD=$(tabby_pids); [ -n "$PROD" ] || { log "ABORT: no production process"; touch $W/restored-b; exit 1; }
trap restore EXIT
tabby_stop; log "DOWNTIME START (sizing + switch) vram=$(vram)"
$V154 $W/size401.py >> $O/size401.out 2>&1; log "size401 rc=$? $(tail -1 $W/size.log)"
CS=$(awk '/CHOSEN/{print $NF}' $W/size.log | tail -1)
case "$CS" in ''|none) log "no pool met the bar; keeping 1.5.2"; exit 1 ;; esac
sed -i "s/^\(  *cache_size:\).*/\1 $CS/" $T/config.yml
log "config: cache_size=$CS (max_seq_len unchanged: $(awk '/max_seq_len:/{print $2}' $T/config.yml)); live config sha $(sha256sum $T/config.yml | cut -c1-16)"
start_prod $V154 || { log "1.5.4 production failed to start"; exit 1; }
P=$(tabby_pids)
python3 $HOME/setup/abwin/contcheck.py <port> $KEY > $O/cont-prod154.log 2>&1; log "prod contcheck rc=$?"
log "SWITCHED: pid=$P venv=$(tr '\0' ' ' < /proc/$P/cmdline | cut -d' ' -f1) exllamav3=$($V154 -c 'import exllamav3.version as e;print(e.__version__)' 2>/dev/null | tail -1) vram=$(vram)"
SWITCHED=1
cp $HOME/setup/prod-tabby.sh $HOME/setup/prod-tabby.sh.pre401b
printf '#!/bin/bash\n# Start rtx6000ada-host production (TabbyAPI on ExLlamaV3 1.5.4 since 2026-10-04, #401). Rollback: venvs/tabby (1.5.2) with config.yml.pre401b.\ncd $HOME/src/tabbyAPI && exec $HOME/venvs/tabby154/bin/python main.py\n' > $HOME/setup/prod-tabby.sh
log "prod-tabby.sh now starts the 1.5.4 venv"
