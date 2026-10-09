#!/usr/bin/env bash
#
# The repository's checks. CI runs the default set, one package's share per
# job (.github/check-owners.tsv).
#
#   verify.sh                 run the default checks: CI's (DEFAULT_CHECKS, #506)
#   verify.sh --only CHECK    run one check (repeatable)
#   verify.sh --only test --scope SPEC   narrow the test check
#   verify.sh --only recompute [--scope DIR[,DIR]]   re-derive results directories (by hand)
#   verify.sh --only admission  re-derive every admission word (by hand)
#   verify.sh --only history --range A..B   scan an explicit range, to repro
#   verify.sh --list          name the checks, in order
#   verify.sh --site DIR      check a built site as Pages would serve it (#32): check_site
#
# Bind to real exit codes. No `cmd | tee`, no `cmd | grep`, nothing that puts
# a pipeline's status where a command's status belongs. Every check runs with
# its own streams and its status is captured directly.
#
# Exit 0 if every check passed, 1 if any failed, 2 if the script was misused.

set -euo pipefail

readonly EXIT_FAIL=1
readonly EXIT_MISUSE=2

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT

readonly CHECKS=(fmt clippy test results recompute admission regimen hygiene pages exercise history)
# What runs with no arguments, and what CI runs (.github/check-owners.tsv;
# keep the two together). `recompute` and `admission` re-derive committed
# data and run only by hand, with `--only`.
readonly DEFAULT_CHECKS=(fmt clippy test results regimen hygiene pages exercise history)

FAILED=()

# --------------------------------------------------------------------------
# checks
# --------------------------------------------------------------------------

check_fmt() { cargo fmt --all --check; }

check_clippy() { cargo clippy --workspace --all-targets -- -D warnings; }

# What `--scope` narrows the `test` check to: which test binaries are built,
# and which tests inside them run. Empty means the whole workspace, which is
# what a contributor gets and what CI's package job runs.
#
# For a quick run by hand. The saving is mostly in the BINARIES NOT BUILT --
# a change to the library does not need tests/cli.rs and tests/conformance.rs
# linked -- and only secondarily in the tests not run.
#
# A flag rather than an environment variable, deliberately. An environment
# variable is inherited by everything started under it, so a scope exported
# once in CI would narrow every `test` run beneath it, silently and for good;
# a gate that runs less than it says is the failure this repository exists to
# refuse. A flag is on the command line of the one process that carries it.
VERIFY_TEST_SCOPE=""
VERIFY_SCOPE=""
VERIFY_SCOPE_GIVEN=""

# SPEC is TARGET or TARGET/FILTER.
#   lib            the library's own tests
#   bins           the binaries' tests
#   test:NAME      the integration target tests/NAME.rs
#   all            the whole workspace, for a fault no one target catches
# FILTER is passed to the test harness, so it is a substring of a test's path.
#
# Sets SCOPE_ARGS (cargo's, selecting targets) and SCOPE_FILTER (the test
# harness's). Kept apart because they go on opposite sides of `--`, and the
# control below needs to put `--list` there too: one flat list would spell
# `-- FILTER -- --list`, which the harness reads as two filters.
#
# Returns 1 if the spec is not one of those, because a spec nobody recognises
# must not quietly become "everything" -- that is the shape of a filter which
# has silently stopped filtering.
SCOPE_ARGS=()
SCOPE_FILTER=""
scope_args() {
  local spec="$1" target
  SCOPE_FILTER=""
  case "$spec" in
    */*) target="${spec%%/*}"; SCOPE_FILTER="${spec#*/}" ;;
    *)   target="$spec" ;;
  esac
  case "$target" in
    lib)    SCOPE_ARGS=(--lib) ;;
    bins)   SCOPE_ARGS=(--bins) ;;
    all)    SCOPE_ARGS=() ;;
    test:*) SCOPE_ARGS=(--test "${target#test:}") ;;
    *) return 1 ;;
  esac
  return 0
}

# `--no-fail-fast` because cargo otherwise stops at the first test binary that
# fails, and the failures it then never prints are the ones a reader needs: a
# broken unit test in the library would hide every conformance failure behind
# it.
check_test() {
  # SCOPE_ARGS and SCOPE_FILTER are set once, by the argument parser, and are
  # empty when no scope was given. Deriving them again here would put a
  # SECOND reader on the spelling -- and it also put the refusal in the wrong
  # place: a check that returns EXIT_MISUSE still leaves the script exiting
  # EXIT_FAIL, because a failed check is a failed check. A misused flag is not
  # a failed gate, and this repository already seeds a fault for exactly that
  # confusion one layer down, in the diet CLI.
  local -a args=(--workspace --no-fail-fast ${SCOPE_ARGS+"${SCOPE_ARGS[@]}"})

  # THE SELECTED-COUNT CONTROL.
  #
  # `cargo test` with a filter that matches no test prints `running 0 tests`
  # and EXITS 0. So a scope with a typo in it, or one left behind after the
  # module it named was renamed, is a check that ran nothing and passed --
  # which is the whole failure this repository refuses, arriving through the
  # door that was opened to make the gate faster.
  #
  # So the scope is asked what it selects BEFORE anything runs. `--list` is
  # the harness's own answer to that question rather than a count scraped out
  # of a run's output, and asking first means a bad scope is refused without
  # running a single test.
  #
  # This runs unscoped too. An unscoped workspace run cannot select nothing
  # today, but "cannot today" is how a guard becomes decoration; the cost is
  # one cargo invocation against an already-built tree.
  local listing rc=0
  listing="$(cargo test "${args[@]}" -- --list ${SCOPE_FILTER:+"$SCOPE_FILTER"} 2>&1)" || rc=$?
  if [ "$rc" -ne 0 ]; then
    # Not the control's failure: the tree did not build, or the target does
    # not exist. Hand back what cargo said, and its status.
    printf '%s\n' "$listing"
    return "$rc"
  fi
  local selected=0
  selected="$(grep -c ': test$' <<< "$listing")" || selected=0
  if [ "$selected" -eq 0 ]; then
    printf 'verify: the test scope %s selected no tests, and a check of nothing is not a pass\n' \
      "${VERIFY_TEST_SCOPE:-<the whole workspace>}" >&2
    return "$EXIT_FAIL"
  fi
  printf 'test scope: %s (%d test(s) selected)\n' \
    "${VERIFY_TEST_SCOPE:-the whole workspace}" "$selected"

  cargo test "${args[@]}" ${SCOPE_FILTER:+-- "$SCOPE_FILTER"}
}

# Build the one binary the format checks below dispatch to. Factored out so
# that `results` and `regimen` cannot disagree about which build that is.
build_diet() {
  cargo build --quiet -p discipline-diet --bin diet || {
    local build_rc=$?
    echo "verify: the diet binary did not build (exit ${build_rc})" >&2
    return "$build_rc"
  }
}

# The report contract, with the record's format verdict dispatched to
# `diet check-record` through the resolver. The linter prints which binary
# answered; it does not read run.jsonl itself.
# The results linter over every directory, and the ledger page drawn from what
# it passed (#32 I2): check-results.py emits the ledger only if every
# directory passes, and exercise/scripts/render-ledger.py draws _site/ledger
# from it, refusing -- naming the directory -- a row with no word, no product
# digest, or no directory behind it. Each step's own exit status is the
# check's: the renderer's verdict reaches it (`RENDER_LEDGER` names the
# renderer, so a test can stand one in). The page links the
# commit it was rendered from; a tree with no commit links HEAD, the default
# branch (#326).
check_results() {
  build_diet || return
  local ledger rc=0
  ledger="$(mktemp)" || return 2
  python3 scripts/check-results.py --root results --ledger "$ledger" || rc=$?
  if [ "$rc" -eq 0 ]; then
    python3 "${RENDER_LEDGER:-exercise/scripts/render-ledger.py}" "$ledger" _site/ledger --results results \
      --commit "$(git rev-parse --verify --quiet HEAD || echo HEAD)" || rc=$?
  fi
  # And the page as published, under the Pages table (#32 I3): it is checked
  # here, where the gate can fail, and not first at deploy time.
  if [ "$rc" -eq 0 ]; then
    bash scripts/hygiene.sh --patterns scripts/pages-patterns.tsv --tree _site/ledger || rc=$?
  fi
  rm -f "$ledger"
  return "$rc"
}

# Every regimen.toml under results/ must parse as a `regimen` document. This
# is what keeps diet/formats load-bearing: the format is used on the
# repository's own data, not merely defined.
#
# Through `scripts/resolve-diet.py`, which asks where cargo would have built
# -- CARGO_TARGET_DIR, CARGO_BUILD_TARGET_DIR, then `[build] target-dir` from
# the nearest `.cargo/config.toml` -- so the binary under test is the one
# cargo produced for this tree and never a stale artifact left at
# `target/debug/diet`. Reading only CARGO_TARGET_DIR let this check grade
# every regimen document `ok` through a seventeen-byte shell script, and
# record its SHA-256 as provenance: accurate, and for the wrong artifact.
check_regimen() {
  local rc=0

  # The grammar declares regimen to be a subset of TOML, and that claim is
  # load-bearing: the same regimen.toml is read by this grammar and by
  # tomllib. Check the claim rather than trusting it.
  python3 scripts/check-toml-subset.py || rc=$?

  build_diet || return $?

  # Through the resolver, and the resolver says which binary and what its
  # digest is. Four instruments once banked numbers through a release binary
  # seven days behind its source because their resolver picked the newer of
  # two builds; this one refuses to pick.
  local resolved bin
  resolved="$(python3 scripts/resolve-diet.py)" || {
    echo "verify: the diet binary could not be resolved" >&2
    return 2
  }
  printf '  %s\n' "$resolved"
  bin="$(printf '%s' "$resolved" \
    | python3 -c 'import json,sys; print(json.load(sys.stdin)["path"])')"

  # And the pin is honoured on every run. A DIET_BIN
  # that can be silently ignored is not a pin, and that being a no-op is how
  # the documented way to fix a run to a build stopped working without anyone
  # noticing. Pinned to a COPY, deliberately: pinning the path the resolver
  # would have chosen anyway proves nothing about whether the pin was read.
  local pinned pin_rc=0
  pinned="$(mktemp)" && cp "$bin" "$pinned" && chmod +x "$pinned" || {
    echo "verify: could not stage a pinned copy of ${bin}" >&2
    return 2
  }
  DIET_BIN="$pinned" python3 scripts/resolve-diet.py --expect "$pinned" > /dev/null \
    || pin_rc=$?
  rm -f "$pinned"
  if [ "$pin_rc" -eq 1 ]; then
    echo "verify: DIET_BIN did not pin the binary the resolver used" >&2
    return 1
  elif [ "$pin_rc" -ne 0 ]; then
    echo "verify: the resolver refused the pinned copy (exit ${pin_rc})" >&2
    return "$pin_rc"
  fi

  local seen=0 file
  while IFS= read -r -d '' file; do
    seen=$((seen + 1))
    if "$bin" check-regimen "$file" > /dev/null; then
      printf '  ok  %s\n' "$file"
    else
      printf '  BAD %s\n' "$file"
      rc=1
    fi
  done < <(find results -mindepth 2 -maxdepth 2 -name regimen.toml -print0)

  if [ "$seen" -eq 0 ]; then
    echo "verify: found no regimen.toml under results/; a check of nothing is not a pass" >&2
    return 2
  fi
  printf '  %d regimen document(s) parsed\n' "$seen"
  return "$rc"
}

# Every results directory's recorded numbers re-derive from the artefacts
# committed beside it, or the directory declares itself historical and is
# counted as skipped. `results` checks that the report agrees with the record;
# both were written by the same run, so agreement between them is not
# derivation. Zero recomputable directories is exit 2, not a pass. Run by
# hand (`--only recompute`), not by default: it takes minutes.
# VERIFY_RECOMPUTE_SCOPE is `--scope` on this check (#318): the directories to
# re-derive, comma-separated.
VERIFY_RECOMPUTE_SCOPE=""
check_recompute() {
  python3 scripts/check-recompute.py --root results \
    ${VERIFY_RECOMPUTE_SCOPE:+--only "$VERIFY_RECOMPUTE_SCOPE"}
}

# A rung's admission word, derived from its admission directory rather than trusted as written (#183): the
# derivation's own fixtures first, then every admission.toml in the tree re-verified through its own
# admission-recompute.sh and its word compared with the word its results derive under planning's rule (#143).
#
# The registry's fingerprints first (#202): every equipment entry's hardware fingerprint covers its declared
# fields, and every substrate whose engine is one exe digest also pins the libraries beside it (or says why one
# digest suffices), with an engine_fingerprint that recomputes from them. This checks the REGISTRY: a library
# digest edited under a held exe reads as a changed fingerprint. Whether a host still runs what the registry
# pins is a read on the host (`check-fingerprints.py --read-engine-pid`), which no gate here can take.
check_admission() {
  python3 substrates/check-fingerprints.py --selftest &&
    python3 substrates/check-fingerprints.py &&
    python3 substrates/admission/derive_admission.py --selftest &&
    python3 substrates/admission/derive_admission.py --all &&
    check_vision_cells
}

# The vision cells (#373): each re-derives its word from its raw responses and refuses when the registry's `vision`
# for its substrate disagrees. Finding none is a failure, not a pass: a check that runs nothing proves nothing.
check_vision_cells() {
  local n=0 f
  for f in substrates/admission/*/*/vision/recompute.sh; do
    [ -f "$f" ] || continue
    n=$((n + 1))
    bash "$f" || { echo "admission: $f exited non-zero"; return 1; }
  done
  [ "$n" -gt 0 ] || { echo "admission: no vision cell found under substrates/admission/; a check that runs nothing is not a pass"; return 1; }
  # The registry side (#392's review, M2): every `vision` is one of the four words, and every word but `unreported`
  # has a cell whose cell.toml names that substrate. A cell checks the registry; this checks the registry has cells.
  python3 -B - <<'PYEOF' || return 1
import pathlib, sys, tomllib
subs = tomllib.loads(pathlib.Path("substrates/registry.toml").read_text())["substrate"]
celled = {tomllib.loads(p.read_text())["substrate"] for p in pathlib.Path("substrates/admission").glob("*/*/vision/cell.toml")}
bad = 0
for name, sub in sorted(subs.items()):
    if "vision" not in sub: continue
    w = sub["vision"]
    if w not in ("accepted", "refused", "answered-without-seeing", "unreported"):
        print(f"admission: {name}'s vision is {w!r}, not accepted, refused, answered-without-seeing or unreported"); bad += 1
    elif w != "unreported" and name not in celled:
        print(f"admission: {name}'s vision is {w!r} and no vision cell under substrates/admission/ names it"); bad += 1
sys.exit(1 if bad else 0)
PYEOF
  echo "admission: $n vision cell(s) re-derive and agree with the registry, and every registry vision word has its cell"
}

# ...and the scanner itself stays honest on the shell a stock Mac runs (#236):
# CI cannot run bash 3.2, so this reads hygiene.sh for what keeps it so.
check_hygiene() {
  python3 scripts/check-hygiene-portable.py &&
    bash scripts/hygiene.sh
}

# The site published to gh-pages is static, and this is what makes that a gate
# rather than a promise: no subresource from another origin, no network call,
# no form, no credential shapes.
check_pages() {
  bash scripts/hygiene.sh --patterns scripts/pages-patterns.tsv --tree pages
}

# Every session the surface places, read by diet's own log reader (ruled on
# #300, 5974717646): a placed log is served-shaped, and two readers that
# never meet drift -- the class that blanked the rehearsal's page (#288).
# What is read is each log's PROJECTION (exercise/src/drive/projection.ts):
# the kinds and keys the format does not have yet, declared there, taken
# out. Run inside check_exercise's subshell, from exercise/.
check_exercise_projections() {
  local dir file out rc=0
  (cd .. && cargo build --locked --quiet -p discipline-diet --bin diet) || return
  dir="$(mktemp -d)" || return 2
  if node scripts/export-projections.mjs "$dir" >/dev/null; then
    for file in "$dir"/*.log; do
      out="$(cd .. && cargo run --locked --quiet -p discipline-diet --bin diet -- check-log "$file")" || {
        printf 'exercise: %s: diet check-log refuses the placed projection: %s\n' "$(basename "$file" .log)" "$out" >&2
        rc=1
      }
    done
  else
    rc=$?
  fi
  rm -rf "${dir:?}"
  return "$rc"
}

# The web surface in exercise/: its typecheck, its lint, and every story as a
# browser test (Vitest runs each Storybook story in Chromium, the plain unit
# tests in Node). It needs Node, at the version exercise/package.json
# declares. The install is frozen to the
# lockfile; the browser is fetched only when the machine does not have it yet,
# and only after the cheap steps pass.
#
# pnpm is the one `packageManager` pins, run as the JavaScript package through
# npx -- never whichever pnpm the host installed (#194). A host's pnpm at any
# other version switches to the pinned one by itself, and pnpm 11's switch,
# whichever build is running, resolves the pinned release with its native
# build (@pnpm/exe) and refuses to run unless this platform's binary is among
# them. pnpm has published none for macOS on Intel after 11.0.4, so on such a
# host the check could not run at all. Through npx the pinned version runs as
# itself and switches to nothing. A pin that cannot be had fails here, first,
# naming the pin.
check_exercise() {
  (
    cd exercise || exit
    local pinned
    # A pin's `+sha512...` suffix, if it ever carries one, is dropped, not checked: npx takes a version only.
    pinned="$(node -p "require('./package.json').packageManager.split('+')[0]")" || exit
    pnpm() { npx --yes "$pinned" "$@"; }
    pnpm --version &&
      pnpm install --frozen-lockfile &&
      pnpm gate &&
      pnpm typecheck &&
      pnpm lint &&
      pnpm exec playwright install chromium &&
      pnpm test &&
      check_exercise_projections &&
      python3 scripts/test_render_ledger.py &&
      python3 scripts/test_admission.py &&
      pnpm build:replay &&
      (cd .. && check_site _site) &&
      node scripts/replay-smoke.mjs ../_site
  )
}

# The published site (#32), DIR as Pages would serve it: two tables over its
# two parts, then the recordings' admissions. The shell -- everything but the
# recordings -- under the Pages table, small and curated. Each recording under
# the table that admitted it (#32, ruling 1; the ruling on #210): the snapshot
# its admission names, scripts/hygiene-admitted-<id>-patterns.tsv and its
# siblings, not the live table, which may have moved on -- so the recordings
# are not scanned under the Pages table, nor rewritten to pass it, nor failed
# by a later table nobody admitted them under. Then each recording against its
# admission (exercise/scripts/admission.py): the recording that was admitted,
# under the snapshot as it was written. Called by check_exercise over the
# build, and by pages.yml over what it is about to publish. A hit names the
# site's own path.
check_site() {
  local site="${1:?check_site: name the site directory}"
  local data="${site}/replay/data" shell log groups patterns hashes rc=0
  local -a payloads
  shell="$(mktemp -d)" || return 2
  log="$(mktemp)" || { rm -rf "$shell"; return 2; }
  if ! { cp -R "${site}/." "${shell}/" && rm -rf "${shell}/replay/data"; }; then
    rm -rf "$shell" "$log"
    return 2
  fi
  bash scripts/hygiene.sh --patterns scripts/pages-patterns.tsv --tree "$shell" > "$log" 2>&1 || rc=$?
  sed "s|${shell}|${site}|g" "$log"
  rm -rf "$shell"
  [ "$rc" -eq 0 ] || { rm -f "$log"; return "$rc"; }
  # Each admitted table, over the recordings it governs, in a box of their own.
  groups="$(python3 exercise/scripts/admission.py tables "$data")" || { rc=$?; rm -f "$log"; return "$rc"; }
  while IFS=$'\t' read -r patterns hashes rest; do
    IFS=$'\t' read -r -a payloads <<< "$rest"
    shell="$(mktemp -d)" || { rm -f "$log"; return 2; }
    cp -- "${payloads[@]}" "$shell/" || { rm -rf "$shell" "$log"; return 2; }
    bash scripts/hygiene.sh --patterns "$patterns" --hashes "$hashes" --tree "$shell" > "$log" 2>&1 || rc=$?
    sed "s|${shell}|${data}|g" "$log"
    rm -rf "$shell"
    [ "$rc" -eq 0 ] || { rm -f "$log"; return "$rc"; }
  done <<< "$groups"
  rm -f "$log"
  python3 exercise/scripts/admission.py verify "$data"
}

# What `--range` hands the history check, and empty unless it was given.
#
# Empty means the script INFERS the range from the event, which is what CI
# gets and what a bare local run gets. The inferred range is `origin/<default>
# ..HEAD`, which is EMPTY on a checkout that matches the trunk -- so a red
# that CI found on a push cannot be replayed locally by the default. A gate
# whose verdict nobody can reproduce is a verdict nobody can check, so the
# range is askable, and only ever explicitly. Ruled 2026-09-14 on #81.
#
# A flag rather than an environment variable, for `--scope`'s reason: a range
# exported once would narrow every history run beneath it, silently.
VERIFY_HISTORY_RANGE=""

# Commit messages, and a pull request's title and body, against the same
# pattern table the file gate uses. A file carrying a forbidden shape can be
# fixed with a commit; a commit message carrying one is permanent.
#
# The MESSAGE, and not the author or committer line. Those are metadata the
# platform publishes on every commit page, and scanning them through a table
# built for content reddened this trunk once for no content at all -- #81.
check_history() {
  python3 scripts/check-history.py \
    ${VERIFY_HISTORY_RANGE:+--range "$VERIFY_HISTORY_RANGE"}
}

# --------------------------------------------------------------------------
# runner
# --------------------------------------------------------------------------

run_check() {
  local name="$1"
  printf '\n=== %s ===\n' "$name"
  local rc=0
  "check_${name}" || rc=$?
  if [ "$rc" -eq 0 ]; then
    printf -- '--- %s: PASS (exit 0)\n' "$name"
  else
    printf -- '--- %s: FAIL (exit %d)\n' "$name" "$rc"
    FAILED+=("${name} (exit ${rc})")
  fi
}

is_check() {
  local candidate="$1" known
  for known in "${CHECKS[@]}"; do
    [ "$known" = "$candidate" ] && return 0
  done
  return 1
}

usage() {
  sed -n '2,/^$/s/^# \{0,1\}//p' "${BASH_SOURCE[0]}"
}

# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------

selected=()
SITE_DIR=""

while [ "$#" -gt 0 ]; do
  case "$1" in
    --only)
      [ "$#" -ge 2 ] || { echo "verify: --only needs a check name" >&2; exit "$EXIT_MISUSE"; }
      is_check "$2" || {
        echo "verify: no such check '$2'; known checks: ${CHECKS[*]}" >&2
        exit "$EXIT_MISUSE"
      }
      selected+=("$2")
      shift 2
      ;;
    --scope)
      [ "$#" -ge 2 ] || { echo "verify: --scope needs a spec" >&2; exit "$EXIT_MISUSE"; }
      # Read now, graded below once the check it narrows is known.
      VERIFY_SCOPE="$2"
      VERIFY_SCOPE_GIVEN=1
      shift 2
      ;;
    --range)
      [ "$#" -ge 2 ] || { echo "verify: --range needs A..B" >&2; exit "$EXIT_MISUSE"; }
      case "$2" in
        *..*) ;;
        *) echo "verify: --range wants A..B, not '$2'" >&2; exit "$EXIT_MISUSE" ;;
      esac
      VERIFY_HISTORY_RANGE="$2"
      shift 2
      ;;
    --site)
      [ "$#" -ge 2 ] || { echo "verify: --site needs the site's directory" >&2; exit "$EXIT_MISUSE"; }
      # Resolved here, against the directory the caller is in: parsing
      # happens before the `cd "$ROOT"` below.
      case "$2" in
        /*) SITE_DIR="$2" ;;
        *)  SITE_DIR="$(pwd)/$2" ;;
      esac
      shift 2
      ;;
    --list) printf '%s\n' "${CHECKS[@]}"; exit 0 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "verify: unknown argument '$1'" >&2; usage >&2; exit "$EXIT_MISUSE" ;;
  esac
done

cd "$ROOT"

# `--site DIR`: the published site, checked as pages.yml checks it before it
# publishes (#32). Alone: a site check that also ran other checks would be a
# different question answered.
if [ -n "$SITE_DIR" ]; then
  if [ "${#selected[@]}" -ne 0 ] || [ -n "$VERIFY_SCOPE_GIVEN" ] || [ -n "$VERIFY_HISTORY_RANGE" ]; then
    echo "verify: --site checks a site and nothing else" >&2
    exit "$EXIT_MISUSE"
  fi
  [ -d "$SITE_DIR" ] || { echo "verify: --site ${SITE_DIR}: no such directory" >&2; exit "$EXIT_MISUSE"; }
  check_site "$SITE_DIR"
  exit $?
fi

# A scope narrows ONE check, so it may only be given when that check is the
# only one asked for. `verify.sh --scope lib` on its own would otherwise read
# as "run everything" while running a fraction of the tests.
# Graded on "was --scope given", never on its value: an empty `--scope ''`
# -- an unset variable in a caller -- is a spelling nobody defined, and must
# be a misuse rather than silently no scope at all.
if [ -n "$VERIFY_SCOPE_GIVEN" ]; then
  if [ "${#selected[@]}" -ne 1 ]; then
    echo "verify: --scope narrows one check, so it needs exactly --only test or --only recompute" >&2
    exit "$EXIT_MISUSE"
  fi
  case "${selected[0]}" in
    test)
      scope_args "$VERIFY_SCOPE" || {
        echo "verify: --scope '$VERIFY_SCOPE': want lib, bins, all or test:NAME, each" \
             "optionally followed by /FILTER" >&2
        exit "$EXIT_MISUSE"
      }
      VERIFY_TEST_SCOPE="$VERIFY_SCOPE"
      ;;
    recompute)
      # Directory names under results/, comma-separated (#318). The script
      # refuses a name that is not there; this refuses a spelling that is
      # not a list of names at all.
      case "$VERIFY_SCOPE" in
        ''|,*|*,|*,,*|*[!A-Za-z0-9._,-]*|.*|*,.*|-*|*,-*) ;;
        *) VERIFY_RECOMPUTE_SCOPE="$VERIFY_SCOPE" ;;
      esac
      [ -n "$VERIFY_RECOMPUTE_SCOPE" ] || {
        echo "verify: --scope '$VERIFY_SCOPE': the recompute check takes results directory names, comma-separated" >&2
        exit "$EXIT_MISUSE"
      }
      ;;
    *)
      echo "verify: --scope narrows one check, so it needs exactly --only test or --only recompute" >&2
      exit "$EXIT_MISUSE"
      ;;
  esac
fi

# A range narrows ONE check, the same way a scope does, and for a sharper
# reason: `history` INFERS its range when none is given, so a `--range` left
# loose would read as "everything was scanned" over a run in which the other
# checks ignored it.
if [ -n "$VERIFY_HISTORY_RANGE" ] &&
   { [ "${#selected[@]}" -ne 1 ] || [ "${selected[0]-}" != "history" ]; }; then
  echo "verify: --range narrows the history check, so it needs exactly --only history" >&2
  exit "$EXIT_MISUSE"
fi

if [ "${#selected[@]}" -eq 0 ]; then
  selected=("${DEFAULT_CHECKS[@]}")
fi

for name in "${selected[@]}"; do
  run_check "$name"
done

echo
if [ "${#FAILED[@]}" -gt 0 ]; then
  printf 'verify: %d of %d check(s) failed:\n' "${#FAILED[@]}" "${#selected[@]}"
  printf '  - %s\n' "${FAILED[@]}"
  exit "$EXIT_FAIL"
fi
printf 'verify: %d check(s) passed.\n' "${#selected[@]}"
