#!/usr/bin/env bash
# The parity instrument's selftest (#143, I5). With #115's config the generalised band.py and apply.py must
# reproduce #115's band.json and verdict.json byte for byte, and pass #115's fixtures plus the generalised rule's;
# band.py must refuse a row whose bytes are not the manifest's, and a manifest that pins too little.
# Exit 0 when all hold, 1 when any fails. PARITY_ROOT (default: this repository) holds results/.
set -uo pipefail
here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
root="${PARITY_ROOT:-$(cd -- "$here/../../.." && pwd)}"
R="$root/results/2026-09-25-extraction-acceptance-parity"; C="$here/configs/extraction-acceptance-inverts-115.json"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
bad=0; say () { echo "$1"; }
python3 -B "$here/band.py" "$C" "$R/archived" > "$tmp/band.json" 2>/dev/null && cmp -s "$tmp/band.json" "$R/band.json" \
  && say "ok    band.py with #115's manifest reproduces #115's band.json byte for byte" || { say "FAIL  band.py does not reproduce #115's band.json"; bad=1; }
( cd "$R" && python3 -B "$here/apply.py" "$C" archived band.json . box.json ) > "$tmp/verdict.json" 2>/dev/null && cmp -s "$tmp/verdict.json" "$R/verdict.json" \
  && say "ok    apply.py with #115's config reproduces #115's verdict.json byte for byte" || { say "FAIL  apply.py does not reproduce #115's verdict.json"; bad=1; }
( cd "$R" && python3 -B "$here/apply.py" --selftest "$C" archived band.json ) > "$tmp/fx.out" 2>&1 \
  && say "ok    $(tail -1 "$tmp/fx.out")" || { say "FAIL  the applier's fixtures: $(tail -1 "$tmp/fx.out")"; bad=1; }
# refusals: a row whose tallies are not the manifest's bytes; a manifest that pins too little
cp -R "$R/archived" "$tmp/row"; printf '\n' >> "$tmp/row/seat-b/events.jsonl"
python3 -B "$here/band.py" "$C" "$tmp/row" > /dev/null 2>&1; rc=$?
[ $rc = 2 ] && say "ok    band.py refuses a row whose tallies are not the manifest's bytes (rc 2)" || { say "FAIL  band.py over altered tallies exited $rc, not 2"; bad=1; }
python3 -c "import json,sys;m=json.load(open(sys.argv[1]));del m['band']['artifacts']['grade.py'];json.dump(m,open(sys.argv[2],'w'))" "$C" "$tmp/thin.json"
python3 -B "$here/band.py" "$tmp/thin.json" "$R/archived" > /dev/null 2>&1; rc=$?
[ $rc = 2 ] && say "ok    band.py refuses a manifest that does not pin grade.py (rc 2)" || { say "FAIL  band.py with a thin manifest exited $rc, not 2"; bad=1; }
python3 -c "import json,sys;m=json.load(open(sys.argv[1]));m['band']['seed']=1;json.dump(m,open(sys.argv[2],'w'))" "$C" "$tmp/seed.json"
python3 -B "$here/band.py" "$tmp/seed.json" "$R/archived" > "$tmp/band-seed.json" 2>/dev/null
python3 -c "import json,sys;sys.exit(0 if json.load(open(sys.argv[1]))['band'] != json.load(open(sys.argv[2]))['band'] else 1)" "$tmp/band-seed.json" "$R/band.json" \
  && say "ok    band.py reads its seed from the manifest (another seed, another interval)" || { say "FAIL  band.py ignores the manifest's seed"; bad=1; }
# a report.json whose stated tallies disagree with the logs, re-pinned so only the tally check can refuse it
cp -R "$R/archived" "$tmp/row2"
python3 - "$tmp/row2/report.json" "$C" "$tmp/rep.json" <<'PY'
import hashlib, json, sys
p, c, out = sys.argv[1:]; r = json.load(open(p)); r["quality"]["tallies"]["A"]["facts_accepted_deduped"] += 1
open(p, "w").write(json.dumps(r)); m = json.load(open(c)); m["band"]["artifacts"]["report.json"] = hashlib.sha256(open(p, "rb").read()).hexdigest(); json.dump(m, open(out, "w"))
PY
python3 -B "$here/band.py" "$tmp/rep.json" "$tmp/row2" > /dev/null 2>&1; rc=$?
[ $rc = 2 ] && say "ok    band.py refuses tallies the row's report does not state (rc 2)" || { say "FAIL  band.py with a disagreeing report exited $rc, not 2"; bad=1; }
# more refusals: a report leaving out a seat's tallies; seat B's stated tally disagreeing; a manifest not pinning a
# seat's log; the manifest's level out of range
for case in drop-B b-off no-seat-a level; do
  rm -rf "$tmp/row3"; cp -R "$R/archived" "$tmp/row3"
  python3 - "$tmp/row3/report.json" "$C" "$tmp/m3.json" "$case" <<'PY'
import hashlib, json, sys
p, c, out, case = sys.argv[1:]; r = json.load(open(p)); m = json.load(open(c))
if case == "drop-B": del r["quality"]["tallies"]["B"]
if case == "b-off": r["quality"]["tallies"]["B"]["facts_offered"] += 1
open(p, "w").write(json.dumps(r)); m["band"]["artifacts"]["report.json"] = hashlib.sha256(open(p, "rb").read()).hexdigest()
if case == "no-seat-a": del m["band"]["artifacts"]["seat-a/events.jsonl"]
if case == "level": m["band"]["level"] = 1.5
json.dump(m, open(out, "w"))
PY
  python3 -B "$here/band.py" "$tmp/m3.json" "$tmp/row3" > /dev/null 2>&1; rc=$?
  [ $rc = 2 ] && say "ok    band.py refuses: $case (rc 2)" || { say "FAIL  band.py with $case exited $rc, not 2"; bad=1; }
done
# the config's own checks: a negative floor, arms not A and B, a malformed band digest
for case in floor arms digest; do
  python3 - "$C" "$tmp/c3.json" "$case" <<'PY'
import json, sys
c, out, case = sys.argv[1:]; m = json.load(open(c))
if case == "floor": m["apply"]["seat_a_floor"] = -1
if case == "arms": m["apply"]["arms"] = {"A": "x"}
if case == "digest": m["apply"]["band_sha256"] = "xyz"
json.dump(m, open(out, "w"))
PY
  ( cd "$R" && python3 -B "$here/apply.py" "$tmp/c3.json" archived band.json . box.json ) > /dev/null 2>&1; rc=$?
  [ $rc = 2 ] && say "ok    apply.py refuses a config with a bad $case (rc 2)" || { say "FAIL  apply.py with a bad $case exited $rc, not 2"; bad=1; }
done
# the applier: a band that is not the config's pinned bytes; the config's own floor
cp "$R/band.json" "$tmp/band-x.json"; printf ' ' >> "$tmp/band-x.json"
( cd "$R" && python3 -B "$here/apply.py" "$C" archived "$tmp/band-x.json" . box.json ) > /dev/null 2>&1; rc=$?
[ $rc = 2 ] && say "ok    apply.py refuses a band that is not the config's pinned bytes (rc 2)" || { say "FAIL  apply.py over an altered band exited $rc, not 2"; bad=1; }
python3 -c "import json,sys;m=json.load(open(sys.argv[1]));m['apply']['seat_a_floor']=1000;json.dump(m,open(sys.argv[2],'w'))" "$C" "$tmp/floor.json"
w=$(cd "$R" && python3 -B "$here/apply.py" "$tmp/floor.json" archived band.json . box.json 2>/dev/null | python3 -c "import json,sys;print(json.load(sys.stdin)['word'])")
[ "$w" = unadjudicated ] && say "ok    apply.py reads its seat-A floor from the config (1000: unadjudicated)" || { say "FAIL  apply.py with a floor of 1000 read $w"; bad=1; }
echo "parity selftest: $([ $bad = 0 ] && echo 'all pass' || echo 'failing')"; exit $bad
