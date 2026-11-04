#!/usr/bin/env bash
# The CSP's recorded check (#719): build the shell over csp-probe/ instead of
# the page, with tauri.conf.json's CSP, run it, and list which beacons reached
# a loopback recorder. Each beacon is one way a script could run.
#
# With the CSP, expected: bundle-script-ran, inline-script-ran (a script
# written in the bundle's HTML, which Tauri hashes into the CSP at build time)
# and eval-refused. Never: injected-script-ran, inline-handler-ran, eval-ran,
# the ways rendered model output could run one. `--no-csp` builds with
# `csp: null`, the control, where every beacon fires.
#
# It rebuilds target/release/exercise-shell; run `cargo tauri build` after.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
config="{\"build\":{\"frontendDist\":\"$here/csp-probe\"}}"
if [ "${1:-}" = "--no-csp" ]; then
  config="{\"build\":{\"frontendDist\":\"$here/csp-probe\"},\"app\":{\"security\":{\"csp\":null}}}"
fi
(cd "$here" && cargo tauri build --no-bundle --config "$config" >&2)
hits="$(mktemp)"
python3 -I "$here/csp-probe/recorder.py" > "$hits" &
recorder=$!
EXERCISE_DRIVE=http://127.0.0.1:7988/ "$here/target/release/exercise-shell" >&2 &
app=$!
sleep 8
kill "$app" "$recorder"
wait "$app" "$recorder" 2>/dev/null || true
sort "$hits"
rm -f "$hits"
