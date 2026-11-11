#!/bin/bash
# t1-browser.sh -- the operator's browser for one T1 run: one headed
# chrome-devtools-mcp daemon (CLI 1.10.1) per run, outside the model's
# sandbox, which the model's `chrome-devtools` CLI reaches over one Unix socket.
#
#   t1-browser.sh start  <run-dir> <worktree>   start caffeinate and the daemon; print the two lines to paste
#   t1-browser.sh shot   <run-dir> <pageId> <name>   the operator's own screenshot -> <run-dir>/shots/<name>.png
#   t1-browser.sh status <run-dir>              exit 0 if the daemon is alive, 1 if not (the model can stop it)
#   t1-browser.sh stop   <run-dir>              stop the daemon, kill caffeinate by pidfile, end leftovers
#   t1-browser.sh restart <run-dir>             start a stopped daemon again under the run's own session id
#
# State lives in <run-dir>: browser.state (sourced by the subcommands),
# browser.env (the lines to paste and export), caffeinate.pid, daemon.log, chrome-profile/, shots/, and bin/, the
# model's chrome-devtools, which refuses the CLI's own start and stop.
#
# Before the daemon starts, chrome-profile/Default/Preferences pins downloads
# to the worktree (download.default_directory, savefile.default_directory,
# prompt_for_download=false). T1_BROWSER_NO_DOWNLOAD_PIN=1 skips the pin; it
# exists only to measure what the pin changes.
set -u
export CHROME_DEVTOOLS_MCP_NO_UPDATE_CHECKS=1 CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS=1
# This script is the operator's: the model's shim (below) lets its start and stop through. A model's command never
# carries the mark -- serve passes it only the default environment and the regimen's env_passthrough names.
export T1_BROWSER_OPERATOR=1

die() { echo "t1-browser: $*" >&2; exit 1; }
usage() { sed -n '2,11p' "$0" | sed 's/^# \{0,1\}//' >&2; exit 2; }
real() { python3 -I -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$1"; }
cdt() { chrome-devtools "$@" 2> >(grep -v -e ExperimentalWarning -e 'trace-warnings' >&2); }
alive() { cdt status 2>/dev/null | grep -q 'daemon is running'; }

load() {
  [ -n "${1:-}" ] || usage
  RUN=$(real "$1")
  [ -f "$RUN/browser.state" ] || die "no session in $RUN (run 'start' first)"
  # shellcheck disable=SC1091
  . "$RUN/browser.state"
  export CD4A_INTERNAL_DAEMON_SESSION_ID=$SID
  if [ -n "$XDG" ]; then export XDG_RUNTIME_DIR=$XDG; else unset XDG_RUNTIME_DIR; fi
}

pin_downloads() { # profile-dir worktree
  mkdir -p "$1/Default"
  python3 -I - "$1/Default/Preferences" "$2" <<'EOF'
import json, os, sys
path, wt = sys.argv[1], sys.argv[2]
prefs = {}
if os.path.exists(path):
    with open(path) as f:
        prefs = json.load(f)
prefs.setdefault("download", {}).update(
    {"default_directory": wt, "prompt_for_download": False, "directory_upgrade": True})
prefs.setdefault("savefile", {})["default_directory"] = wt
with open(path, "w") as f:
    json.dump(prefs, f)
EOF
}

# Start the daemon for $RUN under session $SID, from $WT, on $PROFILE, and set $SOCKET to the socket it listens on.
launch() {
  export CD4A_INTERNAL_DAEMON_SESSION_ID=$SID
  # The daemon resolves a relative --filePath against ITS OWN cwd, which it
  # inherits from this `start` (measured 2026-10-12: a relative path resolved into the
# operator's cwd, and --workspace refused it with "Access denied" and exit 0).
  # Start it from the worktree root, so the model's `--filePath shots/x.png`
  # means <worktree>/shots/x.png. A model in a subdirectory still needs an
  # absolute path. A dev server's WebSocket (Vite's HMR) needs its own ws://
  # pattern: an http:// one does not cover it (measured 2026-11-10).
  (cd "$WT" && cdt start --headless=false \
    --userDataDir "$PROFILE" --logFile "$RUN/daemon.log" \
    --workspace "$WT" \
    --allowedUrlPattern 'http://localhost:*/*' --allowedUrlPattern 'http://127.0.0.1:*/*' \
    --allowedUrlPattern 'ws://localhost:*/*' --allowedUrlPattern 'ws://127.0.0.1:*/*' \
    --categoryExtensions=false --no-performance-crux >/dev/null)
  rc=$?
  [ $rc -eq 0 ] || die "chrome-devtools start exited $rc (see $RUN/daemon.log)"
  # Launch Chrome now, so a failure shows here and not at the model's first call.
  cdt list_pages >/dev/null || die "the daemon started but Chrome did not (see $RUN/daemon.log)"
  RAW=$(cdt status | sed -n 's/.* socket=\([^ ]*\) .*/\1/p')
  [ -n "$RAW" ] || die "could not read the socket from 'chrome-devtools status'"
  SOCKET=$(real "$RAW")
}

# The model's chrome-devtools for $RUN (N8): first on the PATH serve's shell exports (browser.env), it refuses the
# CLI's own `start` and `stop` -- either takes the run's daemon down, and only `restart` with the run's session id
# brings it back -- and hands every other command to the CLI by its absolute path. The refusal's words are PROPOSED,
# for the maintainer.
shim() {
  local real
  real=$(PATH=$(printf '%s' "$PATH" | tr ':' '\n' | grep -vx "$RUN/bin" | paste -sd: -) command -v chrome-devtools) \
    || die "no chrome-devtools on PATH for the model's shim"
  mkdir -p "$RUN/bin"
  cat > "$RUN/bin/chrome-devtools" <<SHIM
#!/bin/bash
# The run's chrome-devtools for the model (t1-browser.sh): start and stop refused, the rest passed on.
case "\${1:-}" in
  start|stop)
    [ "\${T1_BROWSER_OPERATOR:-}" = 1 ] && exec "$real" "\$@"
    echo "chrome-devtools \$1 is refused here: this run's browser belongs to the operator, and its daemon is already running. Use the other chrome-devtools commands." >&2
    exit 1 ;;
esac
exec "$real" "\$@"
SHIM
  chmod +x "$RUN/bin/chrome-devtools"
}

start() {
  [ $# -eq 2 ] || usage
  mkdir -p "$1" || die "cannot create $1"
  [ -d "$2" ] || die "worktree $2 does not exist"
  RUN=$(real "$1"); WT=$(real "$2")
  if [ -f "$RUN/browser.state" ]; then
    ( load "$RUN"; alive ) && die "a daemon is already running for $RUN; 'stop' it first"
  fi
  SID=$(uuidgen | tr 'A-F' 'a-f')
  PROFILE=$RUN/chrome-profile
  mkdir -p "$RUN/shots"
  if [ "${T1_BROWSER_NO_DOWNLOAD_PIN:-}" = 1 ]; then
    echo "t1-browser: download pin SKIPPED (T1_BROWSER_NO_DOWNLOAD_PIN=1)" >&2
  else
    pin_downloads "$PROFILE" "$WT"
  fi

  caffeinate -d -i >/dev/null 2>&1 &
  echo $! > "$RUN/caffeinate.pid"

  launch
  shim

  {
    echo "SID=$SID"
    echo "XDG=${XDG_RUNTIME_DIR:-}"
    echo "SOCKET=$SOCKET"
    echo "WT=$WT"
    echo "PROFILE=$PROFILE"
  } > "$RUN/browser.state"
  {
    echo "sandbox_unix_sockets = [\"$SOCKET\"]"
    echo "export CD4A_INTERNAL_DAEMON_SESSION_ID=$SID"
    echo "export PATH=\"$RUN/bin:\$PATH\""
  } > "$RUN/browser.env"
  echo "daemon up: session $SID, profile $PROFILE, workspace $WT" >&2
  echo "paste line 1 into the run's regimen, run lines 2 and 3 in the shell that starts serve:" >&2
  cat "$RUN/browser.env"
}

shot() {
  [ $# -eq 3 ] || usage
  load "$1"; PAGE=$2; NAME=$3
  case $NAME in */*|'') die "name must be a plain file name" ;; esac
  # A tool call with no daemon would silently START a new headless one.
  alive || die "daemon not running (did the model stop it?); not taking a screenshot"
  mkdir -p "$RUN/shots"
  TMP=$(mktemp -d "$RUN/shots/.tmp.XXXXXX") || die "mktemp failed"
  # --filePath outside the workspace is refused, so take the image inline:
  # the CLI client writes it under TMPDIR.
  J=$(TMPDIR=$TMP cdt take_screenshot "$PAGE" --output-format=json)
  F=$(echo "$J" | jq -r '.images[0].filePath // empty' 2>/dev/null)
  SAVED=
  if [ -z "$F" ]; then
    # Over CLI 1.10.1's inline limit (measured 2026-11-06: a 21 MB viewport), the
    # daemon saves the PNG in its own temp dir, whatever this TMPDIR, and names it
    # only in the message: "...\nSaved screenshot to <dir>/screenshot.png.". Take
    # it only from a chrome-devtools-mcp-* dir, which the daemon made for it.
    F=$(echo "$J" | jq -r '.message // empty' 2>/dev/null | sed -n 's/^Saved screenshot to \(.*\)\.$/\1/p')
    case "$(basename "$(dirname "${F:-/}")")" in chrome-devtools-mcp-*) SAVED=$(dirname "$F") ;; *) F= ;; esac
  fi
  if [ -z "$F" ] || [ ! -f "$F" ]; then
    rm -rf "${TMP:?}"
    die "no screenshot: $J"
  fi
  mv "$F" "$RUN/shots/$NAME.png"
  # The daemon's dir held this one file: remove it, or each shot leaves one behind.
  [ -n "$SAVED" ] && rmdir "$SAVED" 2>/dev/null
  rm -rf "${TMP:?}"
  echo "$RUN/shots/$NAME.png"
}

status() {
  [ $# -eq 1 ] || usage
  load "$1"
  CAF=$(cat "$RUN/caffeinate.pid" 2>/dev/null)
  if [ -n "$CAF" ] && [ "$(ps -o comm= -p "$CAF" 2>/dev/null)" = caffeinate ]; then caf="caffeinate $CAF alive"; else caf="caffeinate not running"; fi
  if alive; then
    echo "running: $(cdt status | sed -n 2p) ($caf)"
    exit 0
  fi
  echo "NOT running: session $SID, socket $SOCKET ($caf)"
  exit 1
}

stop() {
  [ $# -eq 1 ] || usage
  load "$1"
  if alive; then
    cdt stop; echo "chrome-devtools stop exit=$?"
  else
    echo "daemon was not running"
  fi
  CAF=$(cat "$RUN/caffeinate.pid" 2>/dev/null)
  if [ -n "$CAF" ] && [ "$(ps -o comm= -p "$CAF" 2>/dev/null)" = caffeinate ]; then
    kill "$CAF"; echo "killed caffeinate $CAF"
  else
    echo "caffeinate ${CAF:-?} not running"
  fi
  sleep 2
  LEFT=$(pgrep -f -- "--user-data-dir=$PROFILE" | tr '\n' ' ')
  if [ -n "$LEFT" ]; then
    echo "ending leftover processes on $PROFILE: $LEFT"
    # shellcheck disable=SC2086
    kill $LEFT
  fi
  echo "left on the profile: $(pgrep -f -- "--user-data-dir=$PROFILE" | wc -l | tr -d ' ')"
  # The daemon removes its pid file and socket but leaves their directory.
  if [ -n "$XDG" ]; then PIDDIR=$XDG/chrome-devtools-mcp-$SID; else PIDDIR=/private/tmp/chrome-devtools-mcp-$SID-$(id -u); fi
  [ -d "$PIDDIR" ] && rmdir "$PIDDIR" 2>/dev/null && echo "removed empty $PIDDIR"
  return 0
}

# Bring a stopped daemon back for $RUN under the session id browser.state holds, so the socket the regimen names
# (browser.env line 1) and the id serve's shell exported are the ones it listens on again: `start` mints a new id,
# and a model's own `chrome-devtools start` or `stop` (measured 2026-11-11) leaves the run's daemon down.
restart() {
  [ $# -eq 1 ] || usage
  load "$1"
  alive && die "the daemon for $RUN is running; 'stop' it first, or leave it"
  WAS=$SOCKET
  CAF=$(cat "$RUN/caffeinate.pid" 2>/dev/null)
  if [ -z "$CAF" ] || [ "$(ps -o comm= -p "$CAF" 2>/dev/null)" != caffeinate ]; then
    caffeinate -d -i >/dev/null 2>&1 &
    echo $! > "$RUN/caffeinate.pid"
  fi
  launch
  [ "$SOCKET" = "$WAS" ] || die "the daemon is back on $SOCKET, not $WAS: the regimen's sandbox_unix_sockets names the old one"
  echo "daemon back: session $SID, socket $SOCKET, profile $PROFILE, workspace $WT" >&2
}

cmd=${1:-}; [ $# -gt 0 ] && shift
case $cmd in
  start|shot|status|stop|restart) "$cmd" "$@" ;;
  *) usage ;;
esac
