#!/usr/bin/env bash
#
# Genesis hygiene: fail if the tree carries anything that belongs to a private
# environment rather than to this repository.
#
# The patterns live in scripts/hygiene-patterns.tsv and are the only thing to
# edit when a new forbidden shape shows up. This script owns the mechanics:
# which files are scanned, and the exit code.
#
#   hygiene.sh              scan every file git tracks or would track
#                           (cached + untracked, minus what .gitignore excludes)
#   hygiene.sh --tree DIR   scan every file under DIR
#
# Exits 0 if nothing matched, 1 if anything did, 2 if the scan itself failed.
# A scan that finds no files to read is an error, not a pass.

set -euo pipefail

readonly EXIT_DIRTY=1
readonly EXIT_BROKEN=2

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly PATTERN_FILE="${here}/hygiene-patterns.tsv"

tree=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    --tree)
      [ "$#" -ge 2 ] || { echo "hygiene: --tree needs a directory" >&2; exit "$EXIT_BROKEN"; }
      tree="$2"
      shift 2
      ;;
    *)
      echo "hygiene: unknown argument '$1'" >&2
      exit "$EXIT_BROKEN"
      ;;
  esac
done

[ -f "$PATTERN_FILE" ] || {
  echo "hygiene: no pattern file at $PATTERN_FILE" >&2
  exit "$EXIT_BROKEN"
}

# --- what to scan ------------------------------------------------------------
files=()
if [ -n "$tree" ]; then
  [ -d "$tree" ] || { echo "hygiene: $tree is not a directory" >&2; exit "$EXIT_BROKEN"; }
  while IFS= read -r -d '' path; do files+=("$path"); done \
    < <(find "$tree" -type f -not -path '*/.git/*' -print0)
else
  while IFS= read -r -d '' path; do files+=("$path"); done \
    < <(git ls-files -z --cached --others --exclude-standard)
fi

# The pattern file is the one place forbidden shapes are meant to be written
# down, so it is the one file excluded from the scan.
scanned=()
for path in "${files[@]}"; do
  case "$path" in
    */hygiene-patterns.tsv|hygiene-patterns.tsv) continue ;;
  esac
  scanned+=("$path")
done

if [ "${#scanned[@]}" -eq 0 ]; then
  echo "hygiene: nothing to scan; a scan of no files is not a pass" >&2
  exit "$EXIT_BROKEN"
fi

# --- scan --------------------------------------------------------------------
patterns=0
hits=0

while IFS=$'\t' read -r label flags regex; do
  case "$label" in ''|\#*) continue ;; esac
  [ -n "${regex:-}" ] || continue
  patterns=$((patterns + 1))

  opts=(-n -I -E)
  [ "$flags" = "i" ] && opts+=(-i)

  matches=""
  rc=0
  matches="$(grep "${opts[@]}" -e "$regex" -- "${scanned[@]}")" || rc=$?

  case "$rc" in
    0)
      while IFS= read -r line; do
        [ -n "$line" ] || continue
        echo "hygiene: ${label}: ${line}" >&2
        hits=$((hits + 1))
      done <<< "$matches"
      ;;
    1) : ;;  # no match, which is the point
    *)
      echo "hygiene: grep failed with status ${rc} on pattern '${label}'" >&2
      exit "$EXIT_BROKEN"
      ;;
  esac
done < "$PATTERN_FILE"

if [ "$patterns" -eq 0 ]; then
  echo "hygiene: pattern file defines no patterns; that is not a pass" >&2
  exit "$EXIT_BROKEN"
fi

if [ "$hits" -gt 0 ]; then
  echo "hygiene: ${hits} forbidden match(es) across ${#scanned[@]} file(s)" >&2
  exit "$EXIT_DIRTY"
fi

echo "hygiene: ${#scanned[@]} file(s) clean against ${patterns} pattern(s)"
