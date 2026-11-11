#!/bin/bash
# t1-browser-test.sh -- `t1-browser.sh shot` against a fake `chrome-devtools` on PATH that prints what CLI 1.10.1
# prints (measured 2026-11-06, a running daemon, `take_screenshot <page> --output-format=json`):
#   inline: a small viewport, the client writes the PNG under its TMPDIR and names it in images[0].filePath:
#     {"message":"Took a screenshot of the current page's viewport.","images":[{"filePath":"<TMPDIR>/chrome-devtools-mcp-XXXXXX/<uuid>.png","mimeType":"image/png"}]}
#   saved: a screenshot over the inline limit (21 MB of noise), the daemon writes it in ITS OWN temp dir, whatever
#     the client's TMPDIR, and names it only in the message:
#     {"message":"Took a screenshot of the current page's viewport.\nSaved screenshot to <daemon temp>/chrome-devtools-mcp-XXXXXX/screenshot.png."}
# Each case: `shot` exits 0, the PNG is <run>/shots/<name>.png byte for byte, and nothing is left in the daemon's
# temp dir or under shots/. A message naming a file outside a chrome-devtools-mcp-* dir is refused and moves nothing.
# And `start` passes a ws:// --allowedUrlPattern beside each http:// one; `restart` reuses the run's session id.
# Exit 0 when every case holds; prints each case's verdict.
set -u
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d)
trap 'rm -rf "${WORK:?}"' EXIT
mkdir -p "$WORK/bin"
cat > "$WORK/bin/chrome-devtools" <<'EOF'
#!/bin/bash
# The fake CLI: `status` says the daemon runs; `take_screenshot` answers as FAKE_SHOT says.
case $1 in
  status)
    # FAKE_DOWN names a marker: the daemon is down until a `start` makes it.
    if [ -z "${FAKE_DOWN:-}" ] || [ -e "$FAKE_DOWN" ]; then
      echo "chrome-devtools-mcp daemon is running. pid=1 socket=/nonexistent startDate=now"
    else
      echo "chrome-devtools-mcp daemon is not running."
    fi ;;
  start) shift; printf '%s\n' "$@" > "$FAKE_START_ARGS"
    if [ -n "${FAKE_START_SID:-}" ]; then printf '%s\n' "$CD4A_INTERNAL_DAEMON_SESSION_ID" > "$FAKE_START_SID"; fi
    if [ -n "${FAKE_DOWN:-}" ]; then touch "$FAKE_DOWN"; fi ;;
  take_screenshot)
    case $FAKE_SHOT in
      inline)
        d=$(mktemp -d "$TMPDIR/chrome-devtools-mcp-XXXXXX"); f=$d/3de42f13-d16c-4511-a8a8-b1fba663a749.png
        printf 'PNG-inline' > "$f"
        printf '{"message":"Took a screenshot of the current page'"'"'s viewport.","images":[{"filePath":"%s","mimeType":"image/png"}]}\n' "$f" ;;
      saved)
        d=$(mktemp -d "$FAKE_DAEMON_TMP/chrome-devtools-mcp-XXXXXX"); f=$d/screenshot.png
        printf 'PNG-saved' > "$f"
        printf '{"message":"Took a screenshot of the current page'"'"'s viewport.\\nSaved screenshot to %s."}\n' "$f" ;;
      stray)
        f=$FAKE_DAEMON_TMP/not-ours.png
        printf 'PNG-stray' > "$f"
        printf '{"message":"Took a screenshot of the current page'"'"'s viewport.\\nSaved screenshot to %s."}\n' "$f" ;;
    esac ;;
  *) exit 0 ;;
esac
EOF
chmod +x "$WORK/bin/chrome-devtools"
# `start` backgrounds caffeinate; the fake one exits at once.
printf '#!/bin/bash\nexit 0\n' > "$WORK/bin/caffeinate"
chmod +x "$WORK/bin/caffeinate"
export PATH="$WORK/bin:$PATH"

fail=0
case_() { # name expect-exit expect-bytes
  local name=$1 want=$2 bytes=$3 run="$WORK/run-$1"
  mkdir -p "$run" "$WORK/daemon-tmp-$1"
  printf 'SID=fake\nXDG=\nSOCKET=/nonexistent\nWT=%s\nPROFILE=%s\n' "$WORK" "$run/chrome-profile" > "$run/browser.state"
  FAKE_DAEMON_TMP="$WORK/daemon-tmp-$1" FAKE_SHOT=$name bash "$HERE/t1-browser.sh" shot "$run" 1 "$name" > "$run/out" 2>&1
  local got=$? ok=1
  [ "$got" = "$want" ] || ok=0
  if [ -n "$bytes" ]; then
    [ "$(cat "$run/shots/$name.png" 2>/dev/null)" = "$bytes" ] || ok=0
  else
    [ ! -e "$run/shots/$name.png" ] || ok=0
  fi
  [ -z "$(ls -A "$run/shots" | grep -v "^$name.png$")" ] || ok=0
  [ -z "$(find "$WORK/daemon-tmp-$1" -name 'chrome-devtools-mcp-*')" ] || ok=0
  if [ "$ok" = 1 ]; then echo "ok   $name"; else echo "FAIL $name (exit $got, want $want): $(cat "$run/out")"; fail=1; fi
}
case_ inline 0 PNG-inline
case_ saved 0 PNG-saved
case_ stray 1 ""
# The stray file is the fake daemon's, not the script's: it must still be there, untouched.
if [ "$(cat "$WORK/daemon-tmp-stray/not-ours.png" 2>/dev/null)" = PNG-stray ]; then echo "ok   stray left alone"; else echo "FAIL stray moved"; fail=1; fi
# `start` lets the page reach a dev server's WebSocket (Vite's HMR) on the
# hosts it lets the page load from: an http:// pattern alone blocks ws://
# (measured 2026-11-10, CLI 1.10.1: a page's WebSocket to the server that
# served it errors with only the http:// patterns, and opens with the ws://
# ones beside them).
mkdir -p "$WORK/run-start" "$WORK/wt-start"
FAKE_START_ARGS="$WORK/start-args" T1_BROWSER_NO_DOWNLOAD_PIN=1 \
  bash "$HERE/t1-browser.sh" start "$WORK/run-start" "$WORK/wt-start" > "$WORK/run-start/out" 2>&1
got=$?
patterns=$(grep -A1 -x -e '--allowedUrlPattern' "$WORK/start-args" 2>/dev/null | grep -v -x -e '--allowedUrlPattern' -e '--')
want=$(printf '%s\n' 'http://localhost:*/*' 'http://127.0.0.1:*/*' 'ws://localhost:*/*' 'ws://127.0.0.1:*/*')
if [ "$got" = 0 ] && [ "$(sort <<<"$patterns")" = "$(sort <<<"$want")" ]; then
  echo "ok   start allows ws:// beside http://"
else
  echo "FAIL start (exit $got) allowed: $(tr "\n" " " <<<"$patterns"): $(cat "$WORK/run-start/out")"; fail=1
fi
# The model's own `chrome-devtools start` and `stop` are refused (N8): `start` writes the run's shim, which the model's
# PATH reaches first (browser.env), and it answers start and stop with a refusal and passes every other command on
# to the CLI, so the run's daemon stays up.
SHIM="$WORK/run-start/bin/chrome-devtools"
if [ -x "$SHIM" ] && grep -q '^export PATH="[^"]*/run-start/bin:\$PATH"$' "$WORK/run-start/browser.env" 2>/dev/null; then
  echo "ok   start writes the model's shim and puts it first on PATH"
else
  echo "FAIL start wrote no shim at $SHIM, or browser.env does not put it first: $(cat "$WORK/run-start/browser.env" 2>/dev/null)"; fail=1
fi
for verb in start stop; do
  rm -f "$WORK/shim-args"
  FAKE_START_ARGS="$WORK/shim-args" PATH="$WORK/run-start/bin:$PATH" chrome-devtools "$verb" --headless=false > "$WORK/shim-out" 2>&1
  got=$?
  if [ "$got" != 0 ] && [ ! -e "$WORK/shim-args" ] && grep -q 'refused' "$WORK/shim-out"; then
    echo "ok   the model's chrome-devtools $verb is refused, and the daemon is left alone"
  else
    echo "FAIL the model's chrome-devtools $verb (exit $got) reached the CLI or was not refused: $(cat "$WORK/shim-out")"; fail=1
  fi
done
PATH="$WORK/run-start/bin:$PATH" chrome-devtools list_pages > "$WORK/shim-out" 2>&1
got=$?
if [ "$got" = 0 ]; then echo "ok   the model's other chrome-devtools commands reach the CLI"; else echo "FAIL list_pages through the shim (exit $got): $(cat "$WORK/shim-out")"; fail=1; fi

# The operator's own restart, from the shell that put the shim first (browser.env), still reaches the CLI.
mkdir -p "$WORK/run-op"
printf 'SID=the-runs-session\nXDG=\nSOCKET=/nonexistent\nWT=%s\nPROFILE=%s\n' "$WORK/wt-start" "$WORK/run-op/chrome-profile" > "$WORK/run-op/browser.state"
FAKE_DOWN="$WORK/op-up" FAKE_START_ARGS="$WORK/op-args" PATH="$WORK/run-start/bin:$PATH" \
  bash "$HERE/t1-browser.sh" restart "$WORK/run-op" > "$WORK/run-op/out" 2>&1
got=$?
if [ "$got" = 0 ] && [ -e "$WORK/op-args" ]; then
  echo "ok   the operator's restart reaches the CLI past the model's shim"
else
  echo "FAIL the operator's restart behind the shim (exit $got): $(cat "$WORK/run-op/out")"; fail=1
fi

# `restart` brings a stopped daemon back under the run's own session id, so the socket the regimen names (browser.env
# line 1) is the one it listens on again; `start` would mint a new one.
mkdir -p "$WORK/run-restart"
printf 'SID=the-runs-session\nXDG=\nSOCKET=/nonexistent\nWT=%s\nPROFILE=%s\n' "$WORK/wt-start" "$WORK/run-restart/chrome-profile" > "$WORK/run-restart/browser.state"
FAKE_DOWN="$WORK/restart-up" FAKE_START_ARGS="$WORK/restart-args" FAKE_START_SID="$WORK/restart-sid" \
  bash "$HERE/t1-browser.sh" restart "$WORK/run-restart" > "$WORK/run-restart/out" 2>&1
got=$?
if [ "$got" = 0 ] && [ "$(cat "$WORK/restart-sid" 2>/dev/null)" = the-runs-session ] && grep -q '^SID=the-runs-session$' "$WORK/run-restart/browser.state"; then
  echo "ok   restart reuses the run's session id"
else
  echo "FAIL restart (exit $got) started under: $(cat "$WORK/restart-sid" 2>/dev/null): $(cat "$WORK/run-restart/out")"; fail=1
fi
# And refuses while the daemon still runs: a second daemon on the profile is no restart.
FAKE_START_ARGS="$WORK/restart-args2" bash "$HERE/t1-browser.sh" restart "$WORK/run-restart" > "$WORK/run-restart/out2" 2>&1
got=$?
if [ "$got" = 1 ] && [ ! -e "$WORK/restart-args2" ]; then
  echo "ok   restart refuses a daemon still running"
else
  echo "FAIL restart of a running daemon (exit $got): $(cat "$WORK/run-restart/out2")"; fail=1
fi
exit $fail
