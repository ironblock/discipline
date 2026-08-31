#!/usr/bin/env bash
#
# Genesis hygiene: fail if the tree carries anything that belongs to a private
# environment rather than to this repository.
#
# The patterns live in a table and are the only thing to edit when a new
# forbidden shape shows up. This script owns the mechanics: which files are
# scanned, and the exit code.
#
#   hygiene.sh                   scan every file git tracks or would track
#                                (cached + untracked, minus .gitignore)
#   hygiene.sh --tree DIR        scan every file under DIR instead
#   hygiene.sh --patterns FILE   use a different pattern table
#
# Exits 0 if nothing matched, 1 if anything did, 2 if the scan itself failed.
# A scan that finds no files to read is an error, not a pass.

set -euo pipefail

readonly EXIT_DIRTY=1
readonly EXIT_BROKEN=2

# How many paths to hand grep at once. Without chunking the scan dies with
# E2BIG on a worktree carrying a normal untracked build tree.
readonly CHUNK=500

# Matched lines can come from a binary file, where a "line" may be megabytes.
readonly MAX_REPORT=200

here="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly DEFAULT_PATTERNS="${here}/hygiene-patterns.tsv"

tree=""
patterns_file="$DEFAULT_PATTERNS"
while [ "$#" -gt 0 ]; do
  case "$1" in
    --patterns)
      [ "$#" -ge 2 ] || { echo "hygiene: --patterns needs a file" >&2; exit "$EXIT_BROKEN"; }
      patterns_file="$2"
      shift 2
      ;;
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

[ -f "$patterns_file" ] || {
  echo "hygiene: no pattern file at $patterns_file" >&2
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

# A pattern table is the one place forbidden shapes are meant to be written
# down, so pattern tables are excluded from a scan. "Pattern table" means a
# file named *-patterns.tsv sitting DIRECTLY in scripts/ -- not any path
# anywhere in the tree that happens to end that way, which would let anyone
# hide a credential by choosing a filename.
scanned=()
for path in ${files+"${files[@]}"}; do
  case "$path" in
    scripts/*-patterns.tsv)
      if [ "${path#scripts/}" = "$(basename -- "$path")" ]; then continue; fi
      ;;
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

# `-a` rather than `-I`: a file grep would classify as binary must still be
# searched. A credential inside one is exactly as committed as a credential in
# a text file, and skipping it while counting the file as clean is a hole.
# `-H` because with a single file grep would otherwise name no file at all.
#
# The trailing `|| [ -n "$label" ]` reads a final line with no newline, which
# would otherwise drop the last pattern in the table silently.
while IFS=$'\t' read -r label flags regex || [ -n "${label:-}" ]; do
  case "$label" in ''|\#*) continue ;; esac
  [ -n "${regex:-}" ] || continue
  patterns=$((patterns + 1))

  opts=(-n -a -H -E)
  [ "${flags:-}" = "i" ] && opts+=(-i)

  start=0
  while [ "$start" -lt "${#scanned[@]}" ]; do
    part=("${scanned[@]:start:CHUNK}")
    matches=""
    rc=0
    matches="$(grep "${opts[@]}" -e "$regex" -- "${part[@]}")" || rc=$?

    case "$rc" in
      0)
        while IFS= read -r line; do
          [ -n "$line" ] || continue
          echo "hygiene: ${label}: ${line:0:MAX_REPORT}" >&2
          hits=$((hits + 1))
        done <<< "$matches"
        ;;
      1) : ;;  # no match, which is the point
      *)
        echo "hygiene: grep failed with status ${rc} on pattern '${label}'" >&2
        exit "$EXIT_BROKEN"
        ;;
    esac
    start=$((start + CHUNK))
  done
done < "$patterns_file"

if [ "$patterns" -eq 0 ]; then
  echo "hygiene: pattern file defines no patterns; that is not a pass" >&2
  exit "$EXIT_BROKEN"
fi

if [ "$hits" -gt 0 ]; then
  echo "hygiene: ${hits} forbidden match(es) across ${#scanned[@]} file(s)" >&2
  exit "$EXIT_DIRTY"
fi

echo "hygiene: ${#scanned[@]} file(s) clean against ${patterns} pattern(s)"
