#!/usr/bin/env bash
#
# Reconcile this repository's labels and milestones with the definitions in
# .github/labels.json and .github/milestones.json.
#
#   sync-repo-metadata.sh [--dry-run]
#
# Additive and updating, never destructive: entries that exist are updated to
# match the files, entries that do not are created, and entries the files do
# not mention are reported and left alone. Deleting a label deletes its use on
# every issue that carries it, which is not a thing a workflow should do on a
# push.
#
# Needs `gh` (preinstalled on GitHub-hosted runners) with GH_TOKEN set, and
# GITHUB_REPOSITORY in owner/repo form.
#
# Exits 0 if every entry is in place, 1 if any call failed, 2 on misuse.

set -euo pipefail

readonly EXIT_FAIL=1
readonly EXIT_MISUSE=2

dry_run=false
[ "${1:-}" = "--dry-run" ] && { dry_run=true; shift; }
[ "$#" -eq 0 ] || { echo "sync: unexpected argument '$1'" >&2; exit "$EXIT_MISUSE"; }

for tool in gh jq; do
  command -v "$tool" > /dev/null || {
    echo "sync: ${tool} is not on PATH" >&2
    exit "$EXIT_MISUSE"
  }
done
: "${GITHUB_REPOSITORY:?sync: GITHUB_REPOSITORY must be set to owner/repo}"
readonly REPO="$GITHUB_REPOSITORY"

failures=0

note() { printf '%s\n' "$*"; }

# --- labels ------------------------------------------------------------------
note "== labels =="
while IFS=$'\t' read -r name color description; do
  [ -n "$name" ] || continue
  if gh api "repos/${REPO}/labels/${name}" > /dev/null 2>&1; then
    action="update"; method="PATCH"; endpoint="repos/${REPO}/labels/${name}"
  else
    action="create"; method="POST"; endpoint="repos/${REPO}/labels"
  fi

  if [ "$dry_run" = true ]; then
    note "  would ${action} ${name} (#${color})"
    continue
  fi

  if gh api -X "$method" "$endpoint" \
      -f "name=${name}" -f "color=${color}" -f "description=${description}" \
      > /dev/null; then
    note "  ${action}d ${name} (#${color})"
  else
    note "  FAILED to ${action} ${name}"
    failures=$((failures + 1))
  fi
done < <(jq -r '.labels[] | [.name, .color, .description] | @tsv' .github/labels.json)

# --- milestones --------------------------------------------------------------
note "== milestones =="
existing="$(gh api --paginate "repos/${REPO}/milestones?state=all")"
while IFS=$'\t' read -r title description; do
  [ -n "$title" ] || continue
  number="$(printf '%s' "$existing" \
    | jq -r --arg t "$title" 'map(select(.title == $t)) | .[0].number // empty')"

  if [ -n "$number" ]; then
    action="update"; method="PATCH"; endpoint="repos/${REPO}/milestones/${number}"
  else
    action="create"; method="POST"; endpoint="repos/${REPO}/milestones"
  fi

  if [ "$dry_run" = true ]; then
    note "  would ${action} ${title}"
    continue
  fi

  if gh api -X "$method" "$endpoint" \
      -f "title=${title}" -f "description=${description}" -f "state=open" \
      > /dev/null; then
    note "  ${action}d ${title}"
  else
    note "  FAILED to ${action} ${title}"
    failures=$((failures + 1))
  fi
done < <(jq -r '.milestones[] | [.title, .description] | @tsv' .github/milestones.json)

# --- what the files do not mention -------------------------------------------
note "== present but undefined (left alone) =="
comm -23 \
  <(gh api --paginate "repos/${REPO}/labels" --jq '.[].name' | sort) \
  <(jq -r '.labels[].name' .github/labels.json | sort) \
  | sed 's/^/  label /' || true
comm -23 \
  <(printf '%s' "$existing" | jq -r '.[].title' | sort) \
  <(jq -r '.milestones[].title' .github/milestones.json | sort) \
  | sed 's/^/  milestone /' || true

if [ "$failures" -gt 0 ]; then
  echo "sync: ${failures} call(s) failed" >&2
  exit "$EXIT_FAIL"
fi
note "sync: labels and milestones match the definitions"
