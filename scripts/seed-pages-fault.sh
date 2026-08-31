#!/usr/bin/env bash
#
# Materialise a published surface that scripts/pages-patterns.tsv must reject,
# one directory per forbidden class.
#
#   seed-pages-fault.sh DIR
#
# As with seed-hygiene-fault.sh, every forbidden string is assembled from
# fragments at run time, so this file does not trip the gates it tests.

set -euo pipefail

[ "$#" -eq 1 ] || { echo "usage: seed-pages-fault.sh DIR" >&2; exit 2; }
readonly DEST="$1"
mkdir -p "$DEST"

seed() {
  local label="$1" ext="$2"; shift 2
  mkdir -p "${DEST}/${label}"
  printf '%s\n' "$@" > "${DEST}/${label}/${label}.${ext}"
}

seed external-subresource html \
  "$(printf '<script %s="%s//cdn.example.com/x.js"></script>' 'src' 'https:')"

seed external-stylesheet html \
  "$(printf '<link rel="stylesheet" %s="%s//fonts.example.com/x.css">' 'href' 'https:')"

seed css-import css \
  "$(printf '%s%s url("theme.css");' '@' 'import')"

seed css-external-url css \
  "$(printf 'body { background: %s(%s//img.example.com/bg.png); }' 'url' 'https:')"

seed network-call js \
  "$(printf 'const r = await %s%s"/api/claims");' 'fetch' '(')"

seed beacon js \
  "$(printf '%s.%s("/collect", payload);' 'navigator' 'sendBeacon')"

seed dynamic-import js \
  "$(printf 'const m = await %s%s"%s//esm.example.com/x.js");' 'import' '(' 'https:')"

seed base-element html \
  "$(printf '%sbase href="%s//cdn.example.com/">' '<' 'https:')"

seed form-element html \
  "$(printf '%sform method="post" action="/collect"><input name="email"></form>' '<')"

seed api-key-shape html \
  "$(printf '<meta name="k" content="%s%s">' 'sk-ant-' 'api03-0123456789abcdefghijklmn')"

echo "seeded $(find "$DEST" -mindepth 1 -maxdepth 1 -type d | wc -l) fault directories in ${DEST}"
