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
# `# scan: all` in a table means every file must be scannable text and every
# pattern runs over every file. Splitting text from binary is right for the
# genesis table, whose loose environment heuristics match inside ordinary
# binaries. It is wrong for a small curated surface: a UTF-16 page renders
# perfectly in a browser but encodes ASCII as two bytes, so it both classifies
# as binary AND defeats the patterns byte-wise. Neither scanning it nor
# skipping it is honest, so such a table rejects it instead.
scan_all=false
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

if grep -qE '^#[[:space:]]*scan:[[:space:]]*all[[:space:]]*$' -- "$patterns_file"; then
  scan_all=true
fi

# --- what to scan ------------------------------------------------------------
files=()
if [ -n "$tree" ]; then
  [ -d "$tree" ] || { echo "hygiene: $tree is not a directory" >&2; exit "$EXIT_BROKEN"; }
  while IFS= read -r -d '' path; do files+=("$path"); done \
    < <(find "$tree" ! -type d -not -path '*/.git/*' -print0)
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

# Check readability once, up front. Otherwise the first pattern's grep fails,
# its stderr is discarded, and the scan dies with a status and no filename.
for path in "${scanned[@]}"; do
  [ -r "$path" ] || {
    echo "hygiene: cannot read ${path}; a file the scan cannot open is not clean" >&2
    exit "$EXIT_BROKEN"
  }
done

# Split by whether grep would call the file binary. Every pattern runs over the
# text files; only patterns flagged `b` run over the binary ones. Scanning a
# binary with a loose heuristic produces noise, but never scanning it at all
# would hide a committed credential, so precise patterns still go there.
# A file of nothing but blank lines classifies as binary and is therefore only
# credential-scanned, which is all it could ever carry.
text_files=()
binary_files=()
for path in "${scanned[@]}"; do
  if grep -Iq . -- "$path" 2>/dev/null || [ ! -s "$path" ]; then
    text_files+=("$path")
  else
    binary_files+=("$path")
  fi
done

# Under `scan: all` a file that is not scannable text is a finding in itself.
# Reporting it as clean would be a lie, and scanning it byte-wise would find
# nothing in a UTF-16 document however hostile its contents.
if [ "$scan_all" = true ] && [ "${#binary_files[@]}" -gt 0 ]; then
  for path in "${binary_files[@]}"; do
    echo "hygiene: unscannable-encoding: ${path}: not scannable text;" \
         "this surface must be UTF-8" >&2
  done
  echo "hygiene: ${#binary_files[@]} unscannable file(s) across ${#scanned[@]} file(s)" >&2
  exit "$EXIT_DIRTY"
fi

# --- scan --------------------------------------------------------------------
# grep's output goes to a file rather than a command substitution: a match
# inside a binary carries NUL bytes, which `$(...)` discards with a warning on
# stderr. A file keeps grep's exit status ours to read and keeps the noise out.
matchfile="$(mktemp)" || { echo "hygiene: mktemp failed" >&2; exit "$EXIT_BROKEN"; }
trap 'rm -f -- "$matchfile"' EXIT

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
  case "${flags:-}" in *i*) opts+=(-i) ;; esac

  targets=("${text_files[@]}")
  case "${flags:-}" in
    *b*) targets+=(${binary_files+"${binary_files[@]}"}) ;;
  esac
  [ "${#targets[@]}" -gt 0 ] || continue

  start=0
  while [ "$start" -lt "${#targets[@]}" ]; do
    part=("${targets[@]:start:CHUNK}")
    rc=0
    grep "${opts[@]}" -e "$regex" -- "${part[@]}" > "$matchfile" 2>/dev/null || rc=$?

    case "$rc" in
      0)
        while IFS= read -r line; do
          [ -n "$line" ] || continue
          echo "hygiene: ${label}: ${line:0:MAX_REPORT}" >&2
          hits=$((hits + 1))
        done < <(tr -d '\0' < "$matchfile")
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

if [ "$scan_all" = true ]; then
  echo "hygiene: ${#scanned[@]} file(s) clean against ${patterns} pattern(s)" \
       "(every pattern over every file; this table sets 'scan: all')"
else
  echo "hygiene: ${#scanned[@]} file(s) clean against ${patterns} pattern(s)" \
       "(${#text_files[@]} text, ${#binary_files[@]} binary, the latter searched" \
       "only for credential shapes)"
fi
