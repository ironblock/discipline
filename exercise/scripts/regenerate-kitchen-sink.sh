#!/usr/bin/env bash
# The kitchen sink's regeneration (the maintainer's ruling, 2026-11-11: the sink lags): one command that catches the
# published example up with develop's head, as one small commit, ready to push as a PR merged ahead of the queue.
#
#   exercise/scripts/regenerate-kitchen-sink.sh [--not-develop]
#
# Run it from a clean checkout of a branch made for it at develop's head, e.g.
#   git worktree add -b chore/kitchen-sink-<sha8> chore/kitchen-sink-<sha8> origin/develop
# It refuses a dirty tree, and a HEAD that is not origin/develop's (after a fetch) unless --not-develop is given --
# a PR that changes the scenario file regenerates on its own branch that way. It builds diet-drive and diet from that
# clean tree, so the example's build_commit is a real code commit; writes the example; admits it; checks it is
# current; and commits it. It pushes nothing: it prints the push and the PR to open. Dispatch runs it after a merge
# that left develop's sink STALE (verify's `exercise` check says so), or routes it to a seat.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
cd "$root"

anywhere=false
for arg in "$@"; do
  case "$arg" in
    --not-develop) anywhere=true ;;
    *) echo "usage: exercise/scripts/regenerate-kitchen-sink.sh [--not-develop]" >&2; exit 2 ;;
  esac
done

if [ -n "$(git status --porcelain)" ]; then
  echo "regenerate-kitchen-sink: the tree is not clean; the example's build_commit must be a clean build of a commit" >&2
  exit 1
fi
head="$(git rev-parse HEAD)"
if [ "$anywhere" = false ]; then
  git fetch --quiet origin develop
  develop="$(git rev-parse origin/develop)"
  if [ "$head" != "$develop" ]; then
    echo "regenerate-kitchen-sink: HEAD is ${head:0:8}, not develop's head ${develop:0:8}; branch from origin/develop, or pass --not-develop" >&2
    exit 1
  fi
fi

# One diet build, named: the scenario's assembly refuses to pick between a debug and a release build.
export DIET_BIN="$root/target/debug/diet"
node exercise/scripts/kitchen-sink.mjs write
python3 exercise/scripts/admission.py admit kitchen-sink
node exercise/scripts/kitchen-sink.mjs check --current

if [ -z "$(git status --porcelain)" ]; then
  echo "regenerate-kitchen-sink: the example is already current at ${head:0:8}; nothing to commit"
  exit 0
fi
git add exercise/src/drive/examples
git commit --quiet -m "exercise: the kitchen sink regenerated at ${head:0:8}"
branch="$(git branch --show-current)"
echo "regenerate-kitchen-sink: committed $(git rev-parse --short HEAD); next:"
echo "  git push -u origin ${branch}"
echo "  gh pr create --base develop --head ${branch} --title \"exercise: the kitchen sink regenerated at ${head:0:8}\" --body \"The lagging sink's regeneration (exercise/scripts/regenerate-kitchen-sink.sh).\""
