#!/usr/bin/env bash
#
# The gate. Everything that must be true of this repository is checked here,
# and CI runs exactly this script.
#
#   verify.sh                 run every check
#   verify.sh --only CHECK    run one check (repeatable)
#   verify.sh --only test --scope SPEC   narrow the test check (selftest only)
#   verify.sh --only history --range A..B   scan an explicit range, to repro
#   verify.sh --list          name the checks, in order
#   verify.sh --selftest      prove the gate goes red on seeded faults (bash 4+)
#   verify.sh --selftest --shard K/N    run this job's share of the faults
#   verify.sh --selftest --census PATH  write what this run ran, for the sum
#   verify.sh --selftest --derive-scopes DIR   re-harvest the test cases' scopes
#   verify.sh --selftest --derive-shards DIR   re-harvest each fault's cost
#
# THE SCOPES ARE A HARVEST, NOT A LIST. Every `test` case declares which tests
# it needs, and there are 181 of them; a flag that makes the gate run LESS is a
# hazard, and 181 hand-maintained declarations are 181 chances at a scope that
# was right when it was written and wrong after a rename. So they are derived
# rather than kept, and the derivation is re-runnable:
#
#   ./verify.sh --selftest --derive-scopes /tmp/harvest
#   python3 scripts/derive-scopes.py --index /tmp/harvest/derive-scopes.tsv
#   python3 scripts/derive-scopes.py --index /tmp/harvest/... --emit
#
# The run is UNSCOPED on purpose -- every test case runs the whole workspace,
# which is the cost the scopes exist to avoid -- so that what each seeded fault
# breaks is read from a run the declarations did not already narrow. It takes
# as long as the scopes save, which is the point of not doing it in CI.
#
# What the check asks of a declaration is that it SELECT SOMETHING THE FAULT
# BREAKS -- not everything. One failing test fails a run, so a scope naming one
# of thirty-three failures is as red as one naming all of them; it is more
# fragile, and the check says so, but a narrow blast radius is the entire
# reason `--scope` exists and calling it a defect would be calling the feature
# a defect. Selecting NOTHING is the defect: those tests all pass, the run
# exits 0, and a seeded fault reports green. Ruled 2026-09-11 as the condition
# on keeping `--scope`.
#
# The reader itself is not trusted on its own word. `verify.sh --only derive`
# runs its fixture suite in a second, because a tool whose only input is a
# forty-minute harvest is a tool nobody runs, and the first defect in it was
# exactly that kind.
#
# THE SHARD SPLIT IS A HARVEST TOO, for the same reason and with the same
# shape. `--shard K/N` used to divide by arithmetic -- every Nth fault -- which
# splits the COUNT evenly and the COST not at all, and a 58% spread between the
# slowest and fastest shard pushed CI's wall clock past the shortest prompt-
# cache TTL any seat runs on. So the split is measured instead:
#
#   ./verify.sh --selftest --derive-shards /tmp/cost
#   python3 scripts/derive-shards.py --index /tmp/cost/derive-shards.tsv --emit \
#     > tools/gate/shards.tsv
#   python3 scripts/derive-shards.py --check
#
# That run is SCOPED and UNSHARDED: scoped because a shard's cost is the cost
# of the run CI actually performs, and unsharded because a harvest of one
# shard's faults cannot balance the others. `--derive-shards` and
# `--derive-scopes` therefore refuse each other -- one needs the declarations
# on and the other needs them off, and a single run cannot be both.
#
# Three rules this script exists to keep:
#
#   * Bind to real exit codes. No `cmd | tee`, no `cmd | grep`, nothing that
#     puts a pipeline's status where a command's status belongs. Every check
#     runs with its own streams and its status is captured directly.
#   * A gate that has never been seen red is not a gate. `--selftest` copies
#     the tree, injects one deliberate fault per check, and runs *this same
#     script* against it.
#   * Red is not enough: a case must go red *for its own fault*. Each seeded
#     case declares the signature its log must carry. The exit code is still
#     the verdict; the signature only decides whether that verdict is about
#     the fault we seeded. Without it a stale build artifact can make an
#     untested gate look proven.
#
# Exit 0 if every check passed, 1 if any failed, 2 if the script was misused.

set -euo pipefail

readonly EXIT_FAIL=1
readonly EXIT_MISUSE=2

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
readonly ROOT

readonly CHECKS=(fmt clippy test library results recompute regimen lanes metadata hygiene pages exercise ci history injections resolver derive parity)

# The forbidden classes the genesis brief names by hand. Pinning them here
# means a pattern row cannot be deleted along with its seeded class and leave
# the selftest still reporting success.
# Every class, not only the ones the brief names: otherwise a pattern and its
# seeded class can be deleted together and the selftest still reports success.
readonly REQUIRED_HYGIENE_CLASSES=(
  private-ipv4 internal-hostname personal-home-path session-scratchpad-path
  ssh-user-at-host windows-user-path
  internal-ticket-id aws-access-key-id github-token slack-token
  private-key-block anthropic-api-key openai-api-key assigned-secret
)
readonly REQUIRED_PAGES_CLASSES=(
  external-subresource external-stylesheet css-import css-external-url
  base-element network-call beacon dynamic-import form-element api-key-shape
)

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
# It exists for the selftest. A seeded case mutates one file and asks whether
# one gate fires; building and running the whole workspace to answer that was
# most of what remained of the selftest's wall clock once the sandbox stopped
# recompiling from scratch. The saving is mostly in the BINARIES NOT BUILT --
# a fault in the library does not need tests/cli.rs and tests/conformance.rs
# linked -- and only secondarily in the tests not run.
#
# A flag rather than an environment variable, deliberately. An environment
# variable is inherited by everything started under it, so a scope exported
# once in CI would narrow every `test` run beneath it, silently and for good;
# a gate that runs less than it says is the failure this repository exists to
# refuse. A flag is on the command line of the one process that carries it,
# and scripts/check-ci-coverage.py refuses the spelling in any workflow the
# gate depends on.
VERIFY_TEST_SCOPE=""

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
# fails, and the failures it then never prints are the ones a seeded case has
# to recognise: a broken unit test in the library would hide every conformance
# failure behind it, and the log would say the gate fired for the wrong reason.
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

# Two rules about the library saying what it says. Field kinds, verdicts and
# outcome classes are enums with exhaustive matches -- the compiler enforces
# that once the predicate IS an enum, and cannot stop somebody writing a new
# `match text { "DECISION" => ... }`. And every source file is reached by a
# `mod` declaration: an orphan compiles nowhere, so its tests do not run and
# `cargo test -- <its module>` matches nothing and exits 0.
check_library() { python3 scripts/check-library.py; }

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
check_results() { build_diet && python3 scripts/check-results.py --root results; }

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

  # And the pin is honoured on every run, not only in the selftest. A DIET_BIN
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

# Gate 0: every results directory's recorded numbers re-derive from the
# artefacts committed beside it, or the directory declares itself historical
# and is counted as skipped. `results` checks that the report agrees with the
# record; both were written by the same run, so agreement between them is not
# derivation. Zero recomputable directories is exit 2, not a pass.
# --root is the check's own parameter and `results` is its default; it is
# spelled out because a seeded case below depends on this being the root the
# check reads.
check_recompute() { python3 scripts/check-recompute.py --root results; }

# Lane-declared seeded faults, applied for real. Nine lanes write their own
# `diet/<lane>/gate.toml` beside the code that emits it -- ruling 1 on #59 --
# and until this check existed nothing ran a single one of them: 123 faults
# declared, proven once by hand, applied by nothing CI runs. `--verify`
# confirms the root registry (`tools/gate/lanes.toml`) matches what is on
# disk in both directions and that every registered lane's own command is
# currently green.
#
# GREEN HERE on an ordinary run is not the interesting case -- the crate is
# already built by `check_test` moments earlier, so a lane's own scoped
# `cargo test` is an incremental rerun of tests already compiled, not a
# second build. The interesting case is a SEEDED one: this same check, run
# inside a sandbox where one lane fault has been applied, is what proves that
# fault still breaks the thing it says it breaks. Those cases are generated
# from `apply-lane-faults.py --list`, below, rather than declared by hand.
check_lanes() { python3 scripts/apply-lane-faults.py --verify; }

check_metadata() { python3 scripts/check-repo-metadata.py; }

check_hygiene() { bash scripts/hygiene.sh; }

# The site published to gh-pages is static, and this is what makes that a gate
# rather than a promise: no subresource from another origin, no network call,
# no form, no credential shapes.
check_pages() {
  bash scripts/hygiene.sh --patterns scripts/pages-patterns.tsv --tree pages
}

# The web surface in exercise/: its typecheck, its lint, and every story as a
# browser test (Vitest runs each Storybook story in Chromium, the plain unit
# tests in Node). It needs Node and pnpm, at the versions exercise/package.json
# declares. The install is frozen to the lockfile; the browser is fetched only
# when the machine does not have it yet, and only after the cheap steps pass.
check_exercise() {
  (
    cd exercise &&
      pnpm install --frozen-lockfile &&
      pnpm typecheck &&
      pnpm lint &&
      pnpm exec playwright install chromium &&
      pnpm test
  )
}

# The wiring between these checks and the CI that runs them. CI can go green
# while running almost nothing -- a check owned by no workflow, a job the gate
# does not depend on, a path filter that turns a skip into a pass.
#
# AND THE SHARD ASSIGNMENT, which is the same kind of wiring: it is what
# decides how many selftest jobs CI spawns and which faults each one runs.
# `derive-shards.py --check` refuses a plan that is not a partition of its own
# shards, or carries an outlier; a PR that adds a fault without re-deriving is
# REPORTED here and not refused, because `in_shard` runs that fault in the
# shard a hash of its id picks (ruled on #108, 2026-09-24).
#
# Here rather than in a check of its own: a new check needs an owner row, a
# workflow that runs it and a seeded fault of its own, and this is not a new
# concern -- it is the same question `check-ci-coverage.py` already asks, about
# the same workflows, one file further along.
check_ci() {
  python3 scripts/check-ci-coverage.py &&
    python3 scripts/derive-shards.py --check
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

# Every injection in this file changes the tree it is run against. A verdict
# is worth what the fault behind it cost, so an injection is proven to change
# something before the RED it produces counts as anything. This runs on every
# invocation and not only in --selftest: an inert injection is introduced by
# an edit, and the edit is what should fail.
check_injections() { python3 scripts/check-injections.py; }

# The merge resolver, exercised on fixtures before it is trusted to resolve a
# merge. `merge-gate.py` rebuilds the gate files from both sides by name, and
# for its first three hundred lines nothing ran it: five defects lived in it
# at once, each in a behaviour no command had ever executed.
check_resolver() { python3 scripts/check-merge-gate.py; }

# The scope derivation, exercised on fixtures before it is trusted to grade a
# declaration. `derive-scopes.py` reads a forty-minute unscoped harvest, which
# is exactly the shape of tool that is never run against a case it has not
# already seen: its first defect -- a compile-failure pattern anchored without
# re.MULTILINE, so every build-breaking fault came back as "the derivation
# broke" -- survived being written, reviewed and read, and was found only by a
# harvest. The fixtures cost a second and the harvest does not.
#
# BOTH harvests' readers, for the same reason. `derive-shards.py` reads a run
# of the same length and packs the whole fault list into the CI matrix off what
# it finds; a defect in it is a shard that overruns, or an assignment that
# covers the manifest on paper and not in fact. No count here: this line's
# neighbour carried one and it was wrong the day it was written.
#
# Chained with `&&` rather than run in a loop: `&&` puts a command's own status
# where the check's status belongs, and the second reader is only interesting
# if the first is sound.
check_derive() {
  python3 scripts/derive-scopes.py --selftest &&
    python3 scripts/derive-shards.py --selftest
}

# The fault-migration manifest defines what parity means for the replacement
# gate. A manifest that has drifted from this script defines the wrong parity.
check_parity() { python3 scripts/check-fault-manifest.py; }

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

# The declared wall-clock budget, beside check-owners.tsv, which is where this
# repository keeps what CI is held to. Named here; read at the bottom.
readonly GATE_BUDGET=".github/gate-budget.tsv"

# One line, on every run, saying what this run cost against what CI is allowed
# to cost. THE RUN IS NEVER FAILED ON IT: a slow gate is not a wrong gate, and
# a budget that can fail a build is a budget somebody raises rather than meets.
# What it is for is that the drift is visible in the log of the run that caused
# it, rather than in a wall-clock nobody was watching -- which is how #87's
# 58% shard spread went five months unnoticed.
#
# `SECONDS` is this process, so in CI this line measures exactly the step that
# ran it: for a selftest shard, that shard; for a package job, that job's
# checks. What it does NOT measure is the runner's own overhead around it --
# checkout, apt, the toolchain -- which is declared in the same file and
# subtracted by derive-shards.py when it picks the shard count.
report_wall_clock() {
  local elapsed="$SECONDS" budget="" key value
  if [ -f "${ROOT}/${GATE_BUDGET}" ]; then
    while IFS=$'\t' read -r key value || [ -n "${key:-}" ]; do
      case "$key" in
        wall_clock_seconds) budget="$value" ;;
      esac
    done < "${ROOT}/${GATE_BUDGET}"
  fi
  case "${budget:-x}" in *[!0-9]*) budget="" ;; esac
  if [ -z "$budget" ]; then
    # Reported, not raised. scripts/derive-shards.py --check is what refuses a
    # budget file that declares nothing, and it runs under the `ci` check; a
    # second refusal here would fail the run for a defect a gate already owns.
    printf 'verify: wall-clock %ds against no budget -- %s declares no wall_clock_seconds\n' \
      "$elapsed" "$GATE_BUDGET"
  elif [ "$elapsed" -le "$budget" ]; then
    printf 'verify: wall-clock %ds of the %ds budget (%s), %ds to spare\n' \
      "$elapsed" "$budget" "$GATE_BUDGET" "$(( budget - elapsed ))"
  else
    # ONE printf, ONE single-quoted format, not split across lines. A `\`
    # before a newline inside single quotes is a literal backslash and a
    # literal newline -- single quotes take everything literally -- so the
    # continuation that works in the double-quoted messages elsewhere in this
    # file would print the backslash and break the line here. Caught by
    # reading it back, which is the only thing that catches a format string.
    printf 'verify: wall-clock %ds of the %ds budget (%s), OVER BY %ds -- a seat waiting this long returns to a cold prefix\n' \
      "$elapsed" "$budget" "$GATE_BUDGET" "$(( elapsed - budget ))"
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
# selftest
# --------------------------------------------------------------------------

SELFTEST_SCRATCH=()
SELFTEST_BROKEN=()
SEEDED_CHECKS=()
SELFTEST_CASES=0
SELFTEST_LOGS=""

# Where `--derive-scopes` writes its index, and empty when that mode is off.
#
# The mode runs every `test` case with NO scope, so that what the seeded fault
# breaks is read from a run that was not already narrowed by the answer. A
# derivation that ran under the declaration it is checking would confirm
# whatever the declaration said.
SELFTEST_DERIVE=""

# --- the shard ------------------------------------------------------------
#
# `--selftest --shard K/N` runs the faults tools/gate/shards.tsv assigns to
# shard K, so N jobs between them run each fault exactly once.
#
# IT USED TO BE ARITHMETIC: every Nth fault, starting at the Kth. That divides
# the COUNT evenly and the COST not at all. A `test` fault rebuilds the crate
# and costs seconds; a pattern class costs milliseconds; and which of the two a
# round-robin hands a shard is decided by where in this file its declaration
# happens to sit. Measured on the two runs #87 was filed against, shard 2 took
# 7m24s and shard 4 took 4m40s -- a 58% spread -- and the whole run overran the
# 5-minute budget .github/gate-budget.tsv now declares.
#
# So the assignment is HARVESTED, the same way the `--scope` declarations are:
# `--derive-shards` measures what each fault actually costs, and
# `scripts/derive-shards.py` packs those costs into shards. N is a DERIVED
# OUTPUT of that packing and is carried in the file, so a caller passing an N
# the file does not agree with is refused rather than reinterpreted -- two
# opinions about how many shards there are is exactly how a fault ends up in
# no shard at all.
#
# This is still NOT fault selection. Nothing here decides that a fault need not
# run; it decides which JOB runs it, and scripts/check-selftest-census.py
# proves after the fact that the shards between them ran every one. Selecting
# faults by what changed is the thing this repository refuses, and the
# difference is that a shard's absence is a failure rather than a silence: a
# missing census is a missing shard, and the aggregate refuses.
#
# 0 means unsharded -- one job runs the lot, which is what a contributor gets,
# and what a harvest needs. The assignment is not consulted at all then: it
# cannot change which faults run, only which shard runs them, and requiring it
# of an unsharded run would make adding a fault impossible (no harvest without
# a run, no run without an assignment, no assignment without a harvest).
SELFTEST_SHARD=0
SELFTEST_SHARDS=1
SELFTEST_UNITS=0
SELFTEST_RAN=()
SELFTEST_UNPLANNED=0
SELFTEST_CENSUS=""

# The checked-in assignment, relative to ROOT. Named once; derive-shards.py
# holds the same path and is the only thing that writes it.
readonly SHARD_PLAN="tools/gate/shards.tsv"

# Read `${ROOT}/${SHARD_PLAN}` into SHARD_OF, and refuse an N that disagrees.
#
# SHARD_OF IS `selftest`'s LOCAL, not a global. `declare -A` at file scope is a
# usage error on the bash 3.2 a stock Mac ships, and the ORDINARY gate still
# runs there -- only `--selftest` requires bash 4. Bash scopes locals
# dynamically, so a `local -A` in selftest() is in scope for everything it
# calls, including this and in_shard, and the declaration then sits behind the
# version check that guards it.
load_shard_plan() {
  local path="${ROOT}/${SHARD_PLAN}" key a b c declared="" rows=0
  if [ ! -f "$path" ]; then
    echo "selftest: --shard needs ${SHARD_PLAN}, which is not there. It is a \
harvest: ./verify.sh --selftest --derive-shards DIR, then \
scripts/derive-shards.py --index DIR/derive-shards.tsv --emit" >&2
    exit "$EXIT_MISUSE"
  fi
  while IFS=$'\t' read -r key a b c || [ -n "${key:-}" ]; do
    case "$key" in ''|\#*) continue ;; esac
    case "$key" in
      fault)
        # `read_plan` in scripts/derive-shards.py refuses the same file's
        # second row for one id rather than taking the last of the two
        # silently; this docstring-claimed "parsed the same way" was false
        # until this matched it -- two readers of one file disagreeing on a
        # duplicate is a plan that means one thing to `--check` and another to
        # the run it is checking.
        if [ -n "${SHARD_OF[$a]+_}" ]; then
          echo "selftest: ${SHARD_PLAN} assigns '${a}' twice" >&2
          exit "$EXIT_MISUSE"
        fi
        SHARD_OF["$a"]="$b"
        rows=$(( rows + 1 ))
        ;;
      shards) declared="$a" ;;
      # Harvest provenance, read by derive-shards.py and not by this. Named
      # rather than skipped by default: a key this does not know is a file
      # written by something that is not derive-shards.py, and guessing at
      # one is how a plan gets read as half a plan. `substrate` is the row
      # `emit()` has written since the substrate ruling on #87 -- missing from
      # this list, the first runner harvest's own plan would have been refused
      # by every shard as a file of unknown keys.
      overhead_ms|harvest_ms|substrate) ;;
      *)
        echo "selftest: ${SHARD_PLAN}: unknown key '${key}'" >&2
        exit "$EXIT_MISUSE"
        ;;
    esac
  done < "$path"
  if [ -z "$declared" ]; then
    echo "selftest: ${SHARD_PLAN} declares no shard count, so nothing in it \
says how many shards it was packed for" >&2
    exit "$EXIT_MISUSE"
  fi
  case "$declared" in *[!0-9]*)
    echo "selftest: ${SHARD_PLAN} declares '${declared}' as its shard count, \
which is not a number -- \"-ne\" on it would report an error to stderr and the \
\`if\` around it would read that as false, so a corrupt count would be treated \
as agreeing with --shard rather than refused" >&2
    exit "$EXIT_MISUSE"
    ;;
  esac
  if [ "$declared" -ne "$SELFTEST_SHARDS" ]; then
    # The affected range runs from whichever of the two is smaller: a caller
    # passing a bigger N than the plan was packed for leaves shards
    # declared+1..N with nothing assigned to them, while a smaller N leaves
    # N+1..declared assigned to shard numbers no job in this run ever is.
    # Printing "${declared} .. ${SELFTEST_SHARDS}" unconditionally read
    # backwards -- "16 .. 8" -- in the second, more common case.
    local lo hi
    if [ "$declared" -lt "$SELFTEST_SHARDS" ]; then
      lo=$(( declared + 1 )); hi="$SELFTEST_SHARDS"
    else
      lo=$(( SELFTEST_SHARDS + 1 )); hi="$declared"
    fi
    echo "selftest: --shard ${SELFTEST_SHARD}/${SELFTEST_SHARDS} disagrees with \
${SHARD_PLAN}, which was packed for ${declared} shard(s). N is derived from the \
harvest and is not the caller's to choose: reinterpreting this one would leave \
the faults assigned to shards ${lo} .. ${hi} run by nobody" >&2
    exit "$EXIT_MISUSE"
  fi
  [ "$rows" -gt 0 ] || {
    echo "selftest: ${SHARD_PLAN} assigns no fault to any shard" >&2
    exit "$EXIT_MISUSE"
  }
}

# Whether the next counted fault belongs to this shard, counting it either
# way. Every counted fault calls this exactly once, in declaration order, so
# the ordinals are the same in every shard and the union of the shards is the
# whole list -- which is the claim the census script checks rather than trusts.
#
# The argument is the fault's MANIFEST ID -- the same string
# `check-fault-manifest.py` derives for it -- because that is what the
# assignment is keyed by and what survives a fault being added above this one.
# The ordinal cannot be the key: inserting one fault renumbers every fault
# after it, so an assignment keyed by ordinal would silently be about a
# different list the moment anything moved.
#
# unseedable: the sharded selftest's refusals cannot be seeded without re-entering --selftest; proved by hand instead
#
# A seeded case runs `verify.sh --only CHECK`; nothing it can run reaches
# `--selftest --shard`, and an assertion that invoked one would re-enter the
# function this sits in, whose inner run would do the same. Proved by hand in
# all four directions instead -- a plan that is not there, a plan packed for a
# different N, a fault with no row, and the sound plan that must still run --
# and the transcript is in the commit that added them. The third of those is
# no longer a refusal (ruled on #108, 2026-09-24): see the hash below.
in_shard() {
  local ident="${1-}"
  SELFTEST_UNITS=$(( SELFTEST_UNITS + 1 ))
  if [ -z "$ident" ]; then
    echo "selftest: fault ${SELFTEST_UNITS} was counted without an id, so \
nothing can say which shard runs it" >&2
    exit "$EXIT_MISUSE"
  fi
  if [ "$SELFTEST_SHARD" -eq 0 ]; then
    SELFTEST_RAN+=("$SELFTEST_UNITS")
    return 0
  fi
  local assigned="${SHARD_OF[$ident]-}"
  if [ -z "$assigned" ]; then
    # UNPLANNED, NOT REFUSED (ruled on #108, 2026-09-24). The invariant is
    # that every fault runs exactly once, and the census proves that on every
    # run; a plan row was only ever load balance. So a fault the plan does not
    # name -- one added since the last harvest -- goes to a shard derived from
    # its own id. `cksum` is POSIX's CRC, the same on every machine and so in
    # every shard, which is what keeps the union of the shards the whole list.
    # Counted, and printed once beside the census, rather than refused.
    local crc
    crc="$(printf '%s' "$ident" | cksum)"
    assigned=$(( ${crc%% *} % SELFTEST_SHARDS + 1 ))
    SELFTEST_UNPLANNED=$(( SELFTEST_UNPLANNED + 1 ))
  fi
  case "$assigned" in *[!0-9]*)
    echo "selftest: ${SHARD_PLAN} assigns '${assigned}' to '${ident}', which \
is not a shard number -- \"-eq\" on it would report an error to stderr and the \
\`if\` around it would read that as false, so a corrupt row would be silently \
treated as belonging to no shard rather than refused" >&2
    exit "$EXIT_MISUSE"
    ;;
  esac
  if [ "$assigned" -eq "$SELFTEST_SHARD" ]; then
    SELFTEST_RAN+=("$SELFTEST_UNITS")
    return 0
  fi
  return 1
}

# --- the cost harvest -------------------------------------------------------
#
# Where `--derive-shards` writes what each fault cost, and empty when that mode
# is off. The per-case line the selftest already prints carries whole seconds,
# which is the resolution a reader needs and not the resolution a packer does:
# most faults in the list finish well inside one second, and a split that
# treated every one of them as free would put every one of them in one shard.
SELFTEST_COST_INDEX=""
# The directory `--derive-shards` was given, before it is turned into an index
# path. Kept apart so the mode's own guards can refuse before anything is made.
SELFTEST_COST_DIR=""

# Milliseconds, into NOW_MS. Set into a global rather than echoed, because
# `x="$(now_ms)"` runs it in a subshell -- the same reason scratch() gives --
# and a fork per measurement is a measurement that includes the fork.
#
# `EPOCHREALTIME` is bash 5; the selftest requires 4. The fallback is SECONDS,
# whose epoch is this process rather than 1970 -- which is harmless, because
# nothing here reads anything but a difference of two readings from one run.
NOW_MS=0
now_ms() {
  case "${EPOCHREALTIME:-}" in
    *[.,]*)
      local whole="${EPOCHREALTIME%%[.,]*}" frac="${EPOCHREALTIME#*[.,]}000"
      NOW_MS=$(( 10#${whole}${frac:0:3} ))
      ;;
    *) NOW_MS=$(( SECONDS * 1000 )) ;;
  esac
}

# What one fault cost, appended to the harvest index. A no-op outside
# `--derive-shards`, so the ordinary selftest pays one function call per fault
# and writes nothing.
shard_cost() {
  [ -n "$SELFTEST_COST_INDEX" ] || return 0
  local ident="$1" since="$2" label="$3"
  now_ms
  printf 'fault\t%s\t%d\t%s\n' "$ident" "$(( NOW_MS - since ))" "$label" \
    >> "$SELFTEST_COST_INDEX"
}

selftest_cleanup() {
  local path
  for path in ${SELFTEST_SCRATCH+"${SELFTEST_SCRATCH[@]}"}; do
    rm -rf -- "$path" "${path}.log"
  done
  return 0
}

# Sets SCRATCH to a fresh directory and records it for cleanup.
#
# Deliberately NOT `path="$(scratch)"`: command substitution runs the function
# in a subshell, so the append to SELFTEST_SCRATCH would be discarded and the
# EXIT trap would have nothing to remove.
SCRATCH=""
scratch() {
  SCRATCH="$(mktemp -d)" || {
    echo "selftest: mktemp failed" >&2
    exit "$EXIT_MISUSE"
  }
  [ -n "$SCRATCH" ] && [ -d "$SCRATCH" ] || {
    echo "selftest: mktemp produced no directory" >&2
    exit "$EXIT_MISUSE"
  }
  SELFTEST_SCRATCH+=("$SCRATCH")
}

# Copy everything git tracks or would track into DEST, and make it a git
# repository so that checks which ask git what exists still work.
#
# Deliberately NOT `cp -p`. Preserving mtimes makes a sandbox's sources look
# older than artifacts a previous sandbox left in the shared cargo target
# directory, so cargo declares them fresh and runs the wrong binary. That is
# how a seeded case ends up red for another case's fault.
sandbox() {
  local dest="$1" path
  # Cleared, not merely written into. The path is reused across cases (see
  # SELFTEST_BOX), so anything a fault created -- a file, a directory, a
  # committed ref -- would otherwise be part of the next case's tree, and a
  # case that passes because its predecessor left something behind proves
  # nothing about the gate.
  rm -rf -- "$dest"
  mkdir -p "$dest"

  # The list first, so the check below is a shell builtin per file rather than
  # a process. This loop used to run `mkdir` and `cp` per path -- two forks
  # times a thousand files times two hundred cases. Measured on this tree,
  # back to back: 7.4s and 7.7s for the fork-per-file loop, 0.52s and 0.50s
  # for one batched `cp`. It cost as much as the compile it existed to feed.
  local -a files=()
  while IFS= read -r -d '' path; do
    # -f after dereference: a broken symlink, or one pointing at a directory,
    # would make `cp` fail mid-copy and leave a half-built sandbox.
    if [ ! -f "${ROOT}/${path}" ]; then
      echo "selftest: ${path} is not a regular file (missing, or a symlink to one)" >&2
      return 1
    fi
    files+=("$path")
  done < <(git -C "$ROOT" ls-files -z --cached --others --exclude-standard)
  if [ "${#files[@]}" -eq 0 ]; then
    echo "selftest: git names no files to copy; a sandbox of nothing proves nothing" >&2
    return 1
  fi

  # `--parents` rebuilds each path's directories under dest, and -L matches
  # the dereference the check above tests for. Still deliberately NOT `-p`:
  # preserving mtimes would make a box's sources look older than artifacts
  # left in the shared cargo target by the case before it, so cargo would
  # declare them fresh and run the wrong binary.
  #
  # The pipeline's status is `cp`'s: `set -o pipefail` is in force and xargs
  # exits non-zero if any `cp` it spawns does. Nothing here reads a status
  # through a filter -- the rule this script opens with is about `| grep` and
  # `| tee` standing in for a command's own exit code.
  ( cd "$ROOT" && printf '%s\0' "${files[@]}" |
      xargs -0 cp -L --parents -t "$dest" ) || {
    echo "selftest: the sandbox tree could not be copied" >&2
    return 1
  }

  git -C "$dest" init --quiet
  git -C "$dest" add --all
}

# A fingerprint of everything a seeded fault could plausibly change: the
# working tree, and the refs, because two of the injections below seed history
# rather than files. Compared either side of an injection, it answers the one
# question a seeded case cannot answer for itself -- did the injection inject
# anything at all?
# Any git failure here is reported rather than swallowed. It used to end with
# `|| true`, so a sandbox git could not read -- a full disk, a half-built copy
# -- fingerprinted identically either side of the injection, and every case
# printed THE INJECTION CHANGED NOTHING. That is a confident answer to a
# question nobody asked.
readonly STATE_UNREADABLE="sandbox-state-unreadable"
sandbox_state() {
  local box="$1"
  git -C "$box" add --all > /dev/null 2>&1 || { echo "$STATE_UNREADABLE"; return; }
  git -C "$box" write-tree 2> /dev/null || { echo "$STATE_UNREADABLE"; return; }
  git -C "$box" show-ref 2> /dev/null || true
}

# ONE sandbox path, reused by every case, created once by selftest().
#
# Deliberately NOT a fresh `mktemp -d` per case, which is what this was.
# Cargo's incremental cache is keyed by the path the crate was compiled from,
# so a fault compiled at /tmp/tmp.AAA and the next one at /tmp/tmp.BBB shared
# a target directory in which neither could reuse the other's incremental
# state: every Rust-class case paid a full recompile of the crate, and the
# cases alternated paths, so no case ever benefited from the one before it.
#
# Measured on this tree, same shared target, same freshly copied source:
#
#   fresh path each case   14.8s  14.8s
#   one path reused        15.5s   4.4s   4.7s      (the first primes it)
#
# Most of the faults below are Rust-class -- every `test` case, and the fmt,
# clippy and library cases besides -- so that difference was most of the
# selftest's wall clock and all of the reason it read fifty-two minutes in CI.
# No count is written here on purpose: this line carried one, it was wrong the
# day it was written, and nothing reads a comment closely enough to notice.
#
# Isolation is unchanged, because what is reused is the PATH and not the
# CONTENT: sandbox() removes the tree and copies it again from ROOT for every
# case. The fingerprint taken either side of the injection is what proves it,
# and it is taken after the copy.
SELFTEST_BOX=""

# Run `verify.sh --only CHECK` inside a sandbox carrying one seeded fault.
#
# EXPECT is an extended regex the sandbox's log must carry. The exit code is
# the verdict -- the signature decides whether that verdict is about the fault
# we seeded rather than something incidental.
#
# A signature must appear ONLY when the seeded fault fires. A test NAME is not
# a signature: `cargo test` prints it on success too, so matching it would
# certify a dead gate. Match the failure text instead.
#
# The sixth word is the fault's MANIFEST ID, and only the lane-fault generator
# passes one. Every case spelled out in this file derives its id the way
# `check-fault-manifest.py` derives it -- `${check}.${injection}` without the
# prefix -- so the two cannot drift. The generated lane cases all share one
# check and one injection, so that derivation would name every one of them
# `lanes.lane_fault`; the id they are actually registered under comes from the
# lane manifest instead, and the generator passes it.
seeded_case() {
  local label="$1" check="$2" inject="$3" expect="$4" scope="${5-}" ident="${6-}"
  local box="$SELFTEST_BOX"
  local started="$SECONDS"
  now_ms; local started_ms="$NOW_MS"
  [ -n "$ident" ] || ident="${check}.${inject#inject_}"
  # Recorded before the shard is consulted. This list answers "has every check
  # been seen red", which is a question about what the gate DECLARES, and the
  # answer must not depend on which shard is asking.
  SEEDED_CHECKS+=("$check")
  in_shard "$ident" || return 0
  SELFTEST_CASES=$(( SELFTEST_CASES + 1 ))

  # A `test` case says which tests it needs; anything else says nothing,
  # because `--scope` narrows that one check and verify.sh refuses it
  # elsewhere. Both halves are reported here rather than left to become a
  # confusing exit 2 from inside the box, and scripts/check-fault-manifest.py
  # refuses the same two states before a run ever starts.
  local -a scoped=()
  # UNSCOPED ON PURPOSE in derive mode: the point is to see everything this
  # fault breaks, including whatever the declaration currently excludes. A
  # derivation run under the declaration it is checking would confirm whatever
  # that declaration said.
  if [ "$check" = "test" ] && [ -n "$SELFTEST_DERIVE" ]; then
    :
  elif [ "$check" = "test" ]; then
    if [ -z "$scope" ]; then
      printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- NO TEST SCOPE DECLARED\n' \
        "$(( SECONDS - started ))" "$check" "$label"
      SELFTEST_BROKEN+=("${label}: a test case must name the tests it needs")
      shard_cost "$ident" "$started_ms" "$label"
      return
    fi
    scoped=(--scope "$scope")
  elif [ -n "$scope" ]; then
    printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- A SCOPE ON A CHECK THAT TAKES NONE\n' \
      "$(( SECONDS - started ))" "$check" "$label"
    SELFTEST_BROKEN+=("${label}: only the test check takes a scope")
    shard_cost "$ident" "$started_ms" "$label"
    return
  fi
  # One log per case, kept for the run, because the box itself is overwritten
  # by the case after this one and a failure is read after the fact.
  local log="${SELFTEST_LOGS}/$(printf '%03d' "$SELFTEST_CASES").log"
  # The index the derivation reads: which log, which case, and what that case
  # currently declares. Written here rather than beside the scope decision
  # above, where `log` does not exist yet -- `set -u` caught that on the first
  # run, which is the reason this file has `set -u`.
  if [ "$check" = "test" ] && [ -n "$SELFTEST_DERIVE" ]; then
    printf '%s\t%s\t%s\n' "$log" "$label" "$scope" >> "${SELFTEST_LOGS}/derive-scopes.tsv"
  fi

  # `cd ""` succeeds and stays put, so an empty box would run the injection in
  # the real working tree. Refuse rather than seed faults into the repository.
  if [ -z "$box" ] || [ ! -d "$box" ]; then
    echo "selftest: no sandbox for '${label}'; refusing to inject into ${PWD}" >&2
    exit "$EXIT_MISUSE"
  fi

  # `selftest` is invoked in a `||` list, which suspends errexit for everything
  # it calls, so a failed `sandbox` used to return 1 into a caller that carried
  # on regardless -- into a directory that was never even `git init`-ed.
  if ! sandbox "$box"; then
    printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- THE SANDBOX COULD NOT BE BUILT\n' \
      "$(( SECONDS - started ))" "$check" "$label"
    SELFTEST_BROKEN+=("${label}: the sandbox could not be built")
    shard_cost "$ident" "$started_ms" "$label"
    return
  fi

  # An injection is a `sed` or a `printf` against a file it names. Rename the
  # file, or reshape the line the pattern matches, and the injection silently
  # becomes a no-op -- the case then runs a clean tree, goes green, and is
  # reported as a gate that did not fire. That is the right verdict for the
  # wrong reason, and it costs a debugging session every time. Fingerprint the
  # sandbox instead, and say which of the two actually happened.
  #
  # AND THE INJECTION'S OWN EXIT STATUS IS EVIDENCE. It used to be discarded,
  # and then an injection that did half its work -- `cp -r` a directory, then
  # raise KeyError on a field the schema had renamed -- was graded on the tree
  # it left behind. The tree HAD changed, so the fingerprint was satisfied;
  # the check then passed on a copy of a valid directory and the case read
  # GREEN, THE GATE DID NOT FIRE. It is not the gate that did not fire. Found
  # by running it: item 3 renamed `substrate` to `substrates` and this case
  # accused the gate of a fault that was in the injection.
  local state_before state_after injected=0
  state_before="$(sandbox_state "$box")"
  ( cd "$box" && "$inject" ) || injected=$?
  state_after="$(sandbox_state "$box")"
  case "${state_before}${state_after}" in
    *"${STATE_UNREADABLE}"*)
      printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- THE SANDBOX COULD NOT BE READ\n' \
        "$(( SECONDS - started ))" "$check" "$label"
      SELFTEST_BROKEN+=("${label}: the sandbox's state could not be read")
      shard_cost "$ident" "$started_ms" "$label"
      return
      ;;
  esac
  # Every sandbox shares SELFTEST_TARGET, and a test binary bakes its
  # CARGO_MANIFEST_DIR in at compile time. When each case had a path of its
  # own, a binary cargo judged fresh and reused read the FIXTURES OF THE BOX
  # IT WAS BUILT IN -- a stale-artifact false receipt one directory over. One
  # reused path retires that hazard outright: the baked-in directory is always
  # this case's box, so a reused binary reads this case's fixtures.
  #
  # The line stays for the half that survives. sandbox() copies without -p, so
  # every source in the box is newer than any artifact built from the case
  # before it and cargo rebuilds regardless; this makes that invariant local
  # to the one file whose staleness would be silent, rather than resting on
  # the copy's mtimes alone. `git write-tree` hashes content, so it does not
  # disturb the comparison above.
  #
  # IN THE BOX. The first version of this line was relative, and only the
  # injection above runs inside the box -- so it touched the repository's own
  # lib.rs on every case, forced no rebuild where it meant to, and left the
  # repository's binary older than its source for anything that resolved it
  # afterwards. The results-fixture loop below found that out.
  touch "${box}/diet/src/lib.rs" 2> /dev/null || true

  # Before the fingerprint, because "the injection exited 1" says more than
  # "the injection changed nothing" and a half-applied injection can satisfy
  # the fingerprint while proving nothing.
  if [ "$injected" -ne 0 ]; then
    printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- THE INJECTION EXITED %d\n' \
      "$(( SECONDS - started ))" "$check" "$label" "$injected"
    SELFTEST_BROKEN+=("${label}: ${inject} exited ${injected}, so whatever it left is not the fault")
    shard_cost "$ident" "$started_ms" "$label"
    return
  fi

  if [ "$state_before" = "$state_after" ]; then
    printf 'BROKEN %4ds verify.sh --only %-8s          %s  <-- THE INJECTION CHANGED NOTHING\n' \
      "$(( SECONDS - started ))" "$check" "$label"
    SELFTEST_BROKEN+=("${label}: ${inject} changed nothing, so the case proves nothing")
    shard_cost "$ident" "$started_ms" "$label"
    return
  fi

  # Hermetic: only scripts/hermetic.sh's allowlist reaches the sandbox. A
  # blocklist would silently admit every variable nobody thought of, and a
  # sandbox that can see the ambient CI identity makes a check which reads it
  # behave differently there than a contributor would ever see.
  local rc=0
  ( cd "$box" && bash "${ROOT}/scripts/hermetic.sh" \
      env CARGO_TARGET_DIR="${SELFTEST_TARGET}" \
      bash ./verify.sh --only "$check" ${scoped+"${scoped[@]}"} ) \
    > "$log" 2>&1 || rc=$?

  if [ "$rc" -eq 0 ]; then
    printf 'GREEN  %4ds verify.sh --only %-8s exit %-3d  %s  <-- THE GATE DID NOT FIRE\n' \
      "$(( SECONDS - started ))" "$check" "$rc" "$label"
    SELFTEST_BROKEN+=("${label}: the gate did not fire")
    sed -n '1,40p' "$log" >&2
  elif ! grep -qE -- "$expect" "$log"; then
    printf 'WRONG  %4ds verify.sh --only %-8s exit %-3d  %s  <-- RED, BUT NOT FOR ITS OWN FAULT\n' \
      "$(( SECONDS - started ))" "$check" "$rc" "$label"
    printf '      the log carries no match for: %s\n' "$expect"
    SELFTEST_BROKEN+=("${label}: red for the wrong reason")
    sed -n '1,40p' "$log" >&2
  else
    printf 'RED    %4ds verify.sh --only %-8s exit %-3d  %s\n' \
      "$(( SECONDS - started ))" "$check" "$rc" "$label"
  fi
  # ONE OF SEVEN. Every path out of this function records what the fault cost,
  # including the ones that give up early, because a harvest missing a fault is
  # a harvest that cannot be packed. An eighth path added without one is not
  # silent: derive-shards.py refuses an index that does not name every fault
  # the manifest declares, and says which are missing.
  shard_cost "$ident" "$started_ms" "$label"
}

# Apply one `sed` expression to each file, in place, portably.
#
# `sed -i` IS NOT PORTABLE AND THIS GATE MUST NOT DEPEND ON WHICH SED IS
# INSTALLED. GNU sed takes the suffix as an optional argument attached to the
# flag; BSD sed takes it as the NEXT argument, so `sed -i 's/a/b/' f` there
# means suffix `s/a/b/` with `f` as the script -- and the errors that come
# back are about the script, not about the flag, which is why they read as
# nonsense. Twenty-eight injections were inert on a Mac and passing in CI for
# that reason (#50), and an inert injection is a case that proves nothing
# while reporting the same green.
#
# The portable form is no `-i` at all: read the file, write a temporary, and
# move it over only if sed succeeded. A failed edit that has already truncated
# the file leaves a sandbox in a state neither side asked for.
edit_in_place() {
  local expression="$1"; shift
  local file temporary
  for file in "$@"; do
    temporary="${file}.edit-in-place"
    if sed "$expression" "$file" > "$temporary"; then
      mv -- "$temporary" "$file"
    else
      rm -f -- "$temporary"
      echo "edit_in_place: sed refused ${expression} on ${file}" >&2
      return 1
    fi
  done
}

# One generic injection for every lane-declared fault, rather than one bash
# function per fault -- 123 of them today. `LANE_FAULT_LANE`/`LANE_FAULT_ID`
# are set by the loop that calls `seeded_case` for each row
# `apply-lane-faults.py --list` prints, immediately before the call; a plain
# (non-exported) variable set in this shell is visible to the subshell
# `seeded_case` runs the injection in, the same way every other injection's
# closure already reaches variables set above it.
#
# RELATIVE PATH, DELIBERATELY. `seeded_case` runs this after `cd "$box"`, and
# `apply-lane-faults.py` resolves its own root from where IT is loaded from
# -- so a relative path here means the script that mutates is the box's own
# copy, mutating the box's own files. The absolute `$ROOT` this script
# otherwise uses throughout would mutate the real checkout instead.
inject_lane_fault() {
  local lane="${LANE_FAULT_LANE:-}" id="${LANE_FAULT_ID:-}"
  if [ -z "$lane" ] || [ -z "$id" ]; then
    # check-injections.py calls every inject_* function alone, with none of
    # the per-case context the loop above supplies -- so, exactly like every
    # other injection here, this one must prove BY ITSELF that it changes
    # something. Default to the first row `--list` prints, sorted, so the
    # choice is reproducible rather than whichever fault happened to run last.
    read -r lane id _ < <(python3 scripts/apply-lane-faults.py --list | sort | head -1)
  fi
  python3 scripts/apply-lane-faults.py --apply-only "$lane" "$id"
}

inject_fmt() {
  printf '\n#[allow(dead_code)]\nfn seeded_fmt_fault(){let x=1;let _=x;}\n' >> diet/src/lib.rs
}

inject_clippy() {
  printf '\n/// Seeded fault: `clippy::ptr_arg` rejects `&Vec<T>` in an argument.\n#[must_use]\npub fn seeded_clippy_fault(values: &Vec<String>) -> usize {\n    values.len()\n}\n' >> diet/src/lib.rs
}

inject_test() {
  printf '\n#[cfg(test)]\nmod seeded_fault {\n    #[test]\n    fn seeded_failure() {\n        assert_eq!(1, 2, "seeded fault");\n    }\n}\n' >> diet/src/lib.rs
}

inject_formats_empty() {
  python3 - <<'EOF'
import pathlib
import re

path = pathlib.Path("diet/src/formats/mod.rs")
source = path.read_text(encoding="utf-8")
emptied = re.sub(
    r"pub const FORMATS: &\[Format\] = &\[.*?\];",
    "pub const FORMATS: &[Format] = &[];",
    source,
    count=1,
    flags=re.DOTALL,
)
path.write_text(emptied, encoding="utf-8")
EOF
}

# The historical shape of this bug: a detector that finds a decline's opening
# words and stops looking. Dropping the end anchor turns the grammar from "the
# answer IS a decline" into "the answer BEGINS with one", which is what a
# `^none\b` matcher does and why English declines and decline-shaped content
# were both mis-read for a year.
inject_decline_unanchored() {
  edit_in_place \
    's/^document = { SOI ~ ws\* ~ decline ~ ws\* ~ EOI }$/document = { SOI ~ ws* ~ decline ~ ANY* }/' \
    diet/formats/decline/grammar.pest
}

inject_conformance() {
  printf 'budget = 1.5\n' > diet/formats/regimen/fixtures/valid/seeded-nonconforming.toml
  printf '{ "budget": { "integer": 1 } }\n' > diet/formats/regimen/fixtures/valid/seeded-nonconforming.expected.json
}

# The 71-of-630 defect, restaged: the continuation lines are parsed but never
# reach the value. Every historical fix for this made the parser more tolerant
# and shipped its own regression, which is why the corpus rather than the
# tolerance is the gate.
inject_interview_drops_continuations() {
  edit_in_place 's|^    let joined = value.join("\\n");$|    let joined = value.first().cloned().unwrap_or_default();|' \
    diet/src/formats/interview.rs
}

# Truncation graded as an ordinary parse. An emission that hit its token cap
# produced exactly as much valid answer as it had room for; calling it complete
# banks a partial answer as a whole one.
inject_interview_truncation_blind() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/interview.rs")
source = path.read_text(encoding="utf-8")
old = "    if let Some(signal) = signal {\n        return Completion::Truncated(signal);\n    }"
new = "    if false {\n        return Completion::Truncated(signal.unwrap_or(\n            TruncationSignal::UnterminatedFence,\n        ));\n    }"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The sixth defect, restaged: content after the wrapper's closing fence
# discarded with no signal. It is what a corpus of hand-authored fixtures
# cannot catch -- a fixture pins what the parser produced, never what it
# dropped -- so the accounting property is what has to go red here.
inject_interview_discards_trailing() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/interview.rs")
source = path.read_text(encoding="utf-8")
old = "    if close_at < lines.len() {\n        fields.extend(group(&lines[close_at + 1..]));\n    }"
new = "    if false && close_at < lines.len() {\n        fields.extend(group(&lines[close_at + 1..]));\n    }"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# An event kind wired into the schema but never fixtured. The compiler catches
# a variant that is not wired at all; what it cannot catch is a variant that is
# wired everywhere and exercised nowhere, which is the shape a silent
# serialization bug actually arrives in.
inject_record_unfixtured_kind() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
edits = [
    ('        /// The session\'s totals.\n        Summary => "summary",',
     '        /// The session\'s totals.\n        Summary => "summary",\n'
     '        /// Seeded: wired everywhere, fixtured nowhere.\n        Spurious => "spurious",'),
    ("        Kind::Summary => Event::Summary {",
     "        Kind::Spurious => Event::Turn { index: 1, prefill_tokens: Count::default() },\n"
     "        Kind::Summary => Event::Summary {"),
]
for old, new in edits:
    assert old in source, old
    source = source.replace(old, new, 1)
path.write_text(source, encoding="utf-8")
EOF
}

# Provenance made optional. A substrate that defaults when absent is a regime
# that reads complete and is not, which is how partial regimes were compared as
# though they were comparable.
# Links checked after the row's own id was recorded, which is what let a
# request retry itself and a claim supersede itself.
inject_record_self_link_allowed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = "        seen.admit(event)?;\n        seen.claim_id(event)?;"
new = "        seen.claim_id(event)?;\n        seen.admit(event)?;"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The depth limit removed. Recursive descent then runs out of stack and aborts
# the process, and an abort is not a verdict.
inject_record_depth_unbounded() {
  edit_in_place 's|^    if depth > MAX_DEPTH {$|    if false {|' diet/src/formats/record/mod.rs
}

# The kind's own fields made advisory. `turns` is drained and thrown away on a
# recompute summary, so the row is accepted and the number nobody can compute
# is simply not there afterwards -- which is exactly the "tolerate the stray
# field" change somebody makes when a producer emits one, and exactly what
# #47's acceptance row exists to refuse.
inject_record_summary_kind_fields_advisory() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = """        SummaryKind::Recompute => Summary::Recompute {
            targets_checked: take_u32(members, of, "targets_checked")?,"""
new = """        SummaryKind::Recompute => Summary::Recompute {
            targets_checked: {
                members.remove("turns");
                take_u32(members, of, "targets_checked")?
            },"""
if source.count(old) != 1:
    raise SystemExit(f"the recompute arm appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# The one total a recompute summary can contradict on its own, left unchecked.
# Nothing counts a recompute's targets, so this is the only rule that can tell
# a summary that cannot be true of itself from one that merely surprises you.
inject_record_summary_impossible_unchecked() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = """                if targets_matched > targets_checked {"""
new = """                if false && targets_matched > targets_checked {"""
if source.count(old) != 1:
    raise SystemExit(f"the impossible-total check appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# A reference that resolves to whatever it names. `declares` answers yes for
# every id, so a row served by a substrate the run never declared reads as a
# row served by one it did -- and the regime the result is attributed to is a
# regime nothing in the file describes. The reference is the whole mechanism:
# without the check it is a string somebody typed.
# An injection written in a form only GNU sed accepts. It applies here and is
# inert on a Mac, so the tree's verdict depends on the machine -- twenty-eight
# of them did, and CI was green the whole time (#50).
#
# The flag is ASSEMBLED rather than written: the lint this seeds scans
# verify.sh line by line, so a literal in this body would make the clean tree
# fail the check it exists to prove fires on a dirty one.
inject_injection_needs_gnu_sed() {
  python3 - <<'EOF'
import pathlib
import re

path = pathlib.Path("verify.sh")
source = path.read_text(encoding="utf-8")
# The injection is found by NAME and its helper call swapped for the GNU form.
# Neither the anchor nor the flag is written literally: the lint this seeds
# scans verify.sh line by line, so a literal in this body would make the clean
# tree fail the check it exists to prove fires on a dirty one -- and an anchor
# written literally would appear twice, here and there, which is how the first
# version of this refused to run.
body = re.search(
    r"^inject_record_depth_unbounded\(\) \{\n.*?^\}\n", source, re.M | re.S
)
if body is None:
    raise SystemExit("inject_record_depth_unbounded is not there to make unportable")
was = body.group(0)
if "edit_in_place" not in was:
    raise SystemExit("that injection no longer uses the portable helper")
now = was.replace("edit_in_place", "sed" + " -" + "i", 1)
path.write_text(source.replace(was, now, 1), encoding="utf-8")
EOF
}

# The runner's digest comparison made advisory. The cache is still read, the
# scores are still computed, and they are the scores of whatever bytes happen
# to be on disk rather than of the bytes the record consumed -- which is a
# recompute that recomputes something else.
inject_bakeoff_digest_unchecked() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/bakeoff.rs")
source = path.read_text(encoding="utf-8")
old = "    if found != artifact.sha256 {"
new = "    if false && found != artifact.sha256 {"
if source.count(old) != 1:
    raise SystemExit(f"the digest comparison appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# The precision fixture pinned back to a remembered size instead of built from
# the pre-registered ladder. The widest budget the bakeoff reports at is then a
# budget the instrument has never been seen fail at, so `Reported::take`
# refuses it -- correctly, and a bakeoff that reports nothing is not the defect
# being seeded. The defect is the one the 2026-09-10 ruling named: a fixture's
# shape capping the instrument's parameter space, which is how the budget came
# to be eight in the first place. Eight was never chosen; it was the widest the
# fixture allowed.
inject_bakeoff_budget_unfixtured() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "Metric::PrecisionAtK => (widest_budget(), Label::Negative, vec![Label::Positive; 2]),"
new = "Metric::PrecisionAtK => (8, Label::Negative, vec![Label::Positive; 2]),"
if source.count(old) != 1:
    raise SystemExit(f"the precision fixture's width appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# The product digest made the drive's business again -- read per kind, checked
# per kind -- which is where it lived until 2026-09-11 and is the defect that
# ruling closed. A recompute summary is then accepted carrying no digest of
# the product it produced, and every results directory whose record is a
# recompute becomes unlintable: `check-results.py` requires `product_sha256`
# in the front-matter and requires it to equal the summary's, and there is
# nothing there to equal. Nothing in the tree noticed for a day, because no
# fixture crossed the schema and the directory linter.
inject_record_recompute_digest_optional() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")

read_now = """        Kind::Summary => Event::Summary {
            summary: summary(&mut members, of)?,
"""
read_then = """        Kind::Summary => {
            let summary = summary(&mut members, of)?;
            let product_sha256 = match summary.kind() {
                SummaryKind::Drive => take_string(&mut members, of, "product_sha256")?,
                SummaryKind::Recompute => String::new(),
            };
            Event::Summary {
                summary,
"""
check_now = """        if !digest_ok(product_sha256) {
            return Err(StructureError::BadDigest(product_sha256.to_owned()).into());
        }
"""
check_then = """        if matches!(summary, Summary::Drive { .. }) && !digest_ok(product_sha256) {
            return Err(StructureError::BadDigest(product_sha256.to_owned()).into());
        }
"""
for old in (read_now, check_now):
    if source.count(old) != 1:
        raise SystemExit(f"the anchor appears {source.count(old)} times")
source = source.replace(read_now, read_then, 1).replace(check_now, check_then, 1)

# The arm's tail, now one brace deeper. Anchored to the arm's own close
# rather than to the match's -- `Kind::Unknown` (#76) now sits between
# `Kind::Summary` and the match's closing `};`, so the two braces no longer
# abut it.
tail_now = """            product_sha256: take_string(&mut members, of, "product_sha256")?,
        },
"""
tail_then = """                product_sha256,
            }
        }
"""
if source.count(tail_now) != 1:
    raise SystemExit(f"the arm's tail appears {source.count(tail_now)} times")
path.write_text(source.replace(tail_now, tail_then, 1), encoding="utf-8")
EOF
}
# The evidence left where it was instead of copied in. The assembled directory
# then names inputs that are not beside it, and a results directory is a claim
# with its evidence ATTACHED -- evidence that lives somewhere else is a link,
# and a link is what a reader cannot check. `check-results.py` says so:
# "claim `c1` consumes X, which is not a file here". Seeded because the verb
# is the only writer of these directories now, so a verb that writes a
# directory the gates reject is a verb producing a shape nobody can land.
inject_bakeoff_evidence_not_attached() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/bakeoff.rs")
source = path.read_text(encoding="utf-8")
# ANCHORED ON THE LOOP HEADER ALONE, not on the body beneath it. The first cut
# of this injection carried two lines, and `cargo fmt` rewrapped the second one
# the moment the function around it changed -- so the anchor matched nothing and
# the injection went INERT, which `check-injections.py` caught on the next run.
# A one-line anchor is a smaller thing for a formatter to move.
old = "    for artifact in &artifacts {"
new = "    for artifact in artifacts.iter().take(0) {"
if source.count(old) != 1:
    raise SystemExit(f"the copy loop appears {source.count(old)} times")
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
inject_record_substrate_reference_unchecked() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = "        self.substrates.iter().any(|s| s.id == id)"
new = "        let _ = id;\n        true"
if source.count(old) != 1:
    raise SystemExit(f"`declares` body appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# The reference made optional, resolved to the only declared substrate. Every
# record in the corpus still parses -- they all declare one -- which is what
# makes it the tempting change and what makes it worth a seeded fault: the
# same three lines then mean something different in a two-substrate run, and
# nothing in the row says so.
inject_record_substrate_defaults_when_alone() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = '            substrate: take_string(&mut members, of, "substrate")?,\n            retry_of:'
new = ('            substrate: take_optional_string(&mut members, of, "substrate")?\n'
       '                .unwrap_or_else(|| "local".to_owned()),\n'
       '            retry_of:')
if source.count(old) != 1:
    raise SystemExit(f"the request arm appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# #94 design point 1: an effort level with no budget beside it. The level is
# an INSTRUCTION the chat template renders as system-message text -- it bounds
# nothing -- so a regimen naming one and no cap has declared a reasoning state
# no claim can be fixed in. With the pair check off, the corpus fixture is
# accepted and `diet check-regimen` exits 0 on it.
inject_regimen_reasoning_pair_unchecked() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/regimen.rs")
source = path.read_text(encoding="utf-8")
old = "    reasoning_of(&parsed)?;\n"
new = "    let _ = reasoning_of(&parsed);\n"
if source.count(old) != 1:
    raise SystemExit(f"the reasoning pair check appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# #94 design point 2: two substrate entries that differ in nothing but their
# ids. A result attributed to one and a result attributed to the other came
# from the same substrate under two names, and the chat template's digest is
# what makes two files of one model into two substrates. With the check off,
# the record is accepted and the comparison reads as a comparison.
inject_record_substrates_indistinguishable() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = "        if let Some(twin) = declared.iter().find(|s| indistinguishable(s, &one)) {"
new = "        if let Some(twin) = declared.iter().find(|s| false && indistinguishable(s, &one)) {"
if source.count(old) != 1:
    raise SystemExit(f"the indistinguishable check appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# #79: the precedence over prefix reasons, reordered. `PrefixReason`'s
# DECLARATION ORDER is the precedence -- `Ord` is derived from it -- so
# swapping two variants compiles, passes every other test, and silently
# changes what every future record attributes a cache miss to, while leaving
# the rows already written saying something they no longer mean.
inject_record_prefix_precedence_reordered() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
first = '        Model => "model",'
second = '        Tools => "tools",'
for line in (first, second):
    if source.count(line) != 1:
        raise SystemExit(f"{line!r} appears {source.count(line)} times")
source = source.replace(first, "        PRECEDENCE_SWAP", 1)
source = source.replace(second, first, 1)
source = source.replace("        PRECEDENCE_SWAP", second, 1)
path.write_text(source, encoding="utf-8")
EOF
}

# #79: the check that a `prefix.changed` is a change at all. Disabled, a row
# over two requests that hash the same is accepted -- and a cache census reads
# a mutation the file itself denies.
inject_record_prefix_change_not_a_change() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = "            Predecessor::Digest(before) if before == digest => {"
new = "            Predecessor::Digest(before) if false && before == digest => {"
if source.count(old) != 1:
    raise SystemExit(f"the not-a-change check appears {source.count(old)} times")
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# #79: the reason checked against its own diff. Disabled, a row may name any
# class over any evidence -- which is worse than naming none, because the
# reader stops looking.
inject_record_prefix_reason_unchecked() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = "        let supports = reason_of(diff);\n        if reason != supports {"
new = "        let supports = reason_of(diff);\n        if false && reason != supports {"
if source.count(old) != 1:
    raise SystemExit(f"the reason check appears {source.count(old)} times")
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# #79: the head fingerprint checked as a digest. Disabled, prose sits where an
# identity belongs -- the sentinel class the hardware fingerprint already
# caught once -- and every comparison against it reads as a change.
inject_record_head_fingerprint_not_digested() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = "    if !digest_ok(&text) {\n        return Err(StructureError::BadDigest(text).into());\n    }\n    Ok(Some(text))"
new = "    if false && !digest_ok(&text) {\n        return Err(StructureError::BadDigest(text).into());\n    }\n    Ok(Some(text))"
if source.count(old) != 1:
    raise SystemExit(f"the optional-digest check appears {source.count(old)} times")
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# #79: a live record's request rows required to carry a head fingerprint.
# Disabled, a live record silently loses the measurement the whole issue
# exists to take -- the same state `Reasoning::Undeclared` and `Kind::Unknown`
# are each refused for, one field over.
inject_record_head_unhashed_in_live_record() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = "            } if live => {\n                return Err(StructureError::PrefixChange("
new = "            } if false && live => {\n                return Err(StructureError::PrefixChange("
if source.count(old) != 1:
    raise SystemExit(f"the live-record head check appears {source.count(old)} times")
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

inject_record_weights_named_not_digested() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = '            let text = take_string(&mut members, of, "sha256")?;\n            if !digest_ok(&text) {'
new = '            let text = take_string(&mut members, of, "sha256")?;\n            if false && !digest_ok(&text) {'
if source.count(old) != 1:
    raise SystemExit(f"the weights check appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

inject_record_substrate_optional() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = """    let Some(value) = members.remove("substrates") else {
        return Err(SchemaError::MissingField {
            of,
            field: "substrates",
        }
        .into());
    };"""
new = """    let value = members.remove("substrates").unwrap_or_else(|| {
        Value::Array(vec![Value::Object(BTreeMap::from([
            ("id".to_owned(), Value::String("unknown".to_owned())),
            (
                "engine".to_owned(),
                Value::Object(BTreeMap::from([
                    ("name".to_owned(), Value::String("unknown".to_owned())),
                    (
                        "version_or_digest".to_owned(),
                        Value::String("unknown".to_owned()),
                    ),
                ])),
            ),
            (
                "weights".to_owned(),
                Value::Object(BTreeMap::from([
                    ("kind".to_owned(), Value::String("digest".to_owned())),
                    ("sha256".to_owned(), Value::String("0".repeat(64))),
                ])),
            ),
            (
                // DIGEST-SHAPED, like the weights above. The field is checked
                // as a digest since the fingerprint tightening, so a prose
                // placeholder here would be refused by THAT check and the
                // fixture this fault is meant to let through would stay
                // rejected -- for the wrong reason, with this case reading
                // green. The fault is "substrates made optional", and nothing
                // else about the fabricated default may be refusable.
                "hardware_fingerprint".to_owned(),
                Value::String("0".repeat(64)),
            ),
            (
                "sampler_card".to_owned(),
                Value::Object(BTreeMap::from([(
                    "seed".to_owned(),
                    Value::Integer(0),
                )])),
            ),
            ("reasoning".to_owned(), Value::String("off".to_owned())),
        ]))])
    });"""
if source.count(old) != 1:
    raise SystemExit(f"the substrates guard appears {source.count(old)} times")
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The per-lane floor made inert. A pass that mostly fabricated has shown it
# was not structuring, and its individual survivors are not trustworthy; a gate
# that keeps them anyway banks the survivors of a fabrication.
# Presence loosened from a contiguous run of whole tokens to "every word
# appears somewhere". That is a recombination matcher, and it scores sentences
# the source never said as present in it -- a judgment call in the one place
# that must not have one.
inject_grounded_loose_matching() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/grounded.rs")
source = path.read_text(encoding="utf-8")
old = """    haystack
        .iter()
        .any(|line| line.windows(needle.len()).any(|window| window == needle))"""
new = """    haystack
        .iter()
        .any(|line| needle.iter().all(|word| line.contains(word)))"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A floor of zero, which every lane meets. The per-lane rule switched off by a
# value that looks like a setting.
inject_grounded_zero_floor() {
  edit_in_place 's|^        if grounded == 0 {$|        if false {|' diet/src/capture/grounded.rs
}

inject_grounded_floor_inert() {
  edit_in_place 's|^    let outcome = if score.meets(floor) {$|    let outcome = if true {|' \
    diet/src/capture/grounded.rs
}

# The judgment exemption removed. Grounding a plan is a category error, and a
# gate that does it rejects legitimate content -- the failure that made the
# scoping a ruling rather than an implementation detail.
inject_grounded_gates_judgment() {
  edit_in_place 's|^        !matches!(self, Self::Judgment)$|        let _ = self; true|' \
    diet/src/capture/grounded.rs
}

# A measurement handed back without its instrument having been seen fail. The
# 1.000 that meant nothing was a real score, computed by real code, on a probe
# where fabrication was structurally impossible.
inject_grounded_undemonstrated() {
  edit_in_place 's|^        if demonstrated_failure.outcome != LaneOutcome::Rejected {$|        if false {|' \
    diet/src/capture/grounded.rs
}

# A stringly predicate reintroduced by hand. Field kinds were matched as
# strings in more than one place before, and the places drifted.
inject_stringly_predicate() {
  printf '\n#[must_use]\npub fn seeded_stringly(text: &str) -> u8 {\n    match text {\n        "DECISION" => 1,\n        _ => 0,\n    }\n}\n' \
    >> diet/src/lib.rs
}

# A source file no `mod` declaration reaches. It sits on disk, compiles
# nowhere, and `cargo test -- object` selects nothing and exits 0 -- which is
# how five hundred lines and eleven tests went unrun with the gate green.
inject_orphaned_module() {
  edit_in_place '/^pub mod object;$/d' diet/src/lib.rs
}

# The same module lost the ordinary way: commented OUT rather than deleted.
# Rule two read raw text, so a block comment hid the declaration and the file
# went uncompiled with every check green -- the deletion above, restaged in
# the shape somebody actually produces.
inject_commented_out_module() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/lib.rs")
source = path.read_text(encoding="utf-8")
old = "pub mod object;"
new = "/* undone for a probe:\npub mod object;\n*/"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The or-pattern shape `cargo fmt` produces unprompted once tag names are
# realistic. It is the same stringly predicate as the one above, wrapped
# across lines -- and the rule's first version was anchored to the start of a
# line, so its coverage depended on how long the identifiers were.
inject_stringly_or_pattern() {
  printf '\n#[must_use]\npub fn seeded_wrapped(text: &str) -> u8 {\n    match text {\n        "a_decision_tag_that_is_quite_long_indeed"\n        | "an_evidence_tag_that_is_also_long_here" => 1,\n        _ => 0,\n    }\n}\n' \
    >> diet/src/lib.rs
}

# A `claim:` bound to a test that is not there -- whether never named
# correctly or (as here) simply invented. Rule three exists because six
# comments this true this week rotted into false ones with nothing to catch
# it; this is the seeded case that proves a dangling one goes red.
inject_dangling_claim() {
  printf '\n// claim: a seeded fact that binds to nothing :: this_test_does_not_exist_anywhere\n' \
    >> diet/src/lib.rs
}

# The acceptance case where THE BUILD IS THE GATE: a field-kind variant added
# without wiring it. Every exhaustive match over FieldKind stops compiling,
# which is the whole reason the predicate is an enum.
inject_field_kind_variant() {
  # Not `sed`: a `\n` in the replacement is a GNU extension, and POSIX wants a
  # literal backslash-newline. The python form every other multi-line
  # injection uses says what it does instead of encoding it.
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/interview.rs")
source = path.read_text(encoding="utf-8")
old = "    Stuck,\n"
new = "    Stuck,\n    /// Seeded: a variant nothing covers.\n    Seeded,\n"
if source.count(old) != 1:
    raise SystemExit(f"`Stuck,` appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# A supersede that deletes what it replaced. Claim atomicity at the object
# level: a correction is a linked row, and the row it corrects has to still be
# there to be read.
inject_object_supersede_deletes() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = """        if let Some(old) = self.entries.get_mut(voids) {
            old.state = EntryState::Voided { by: added.clone() };
        }"""
new = """        self.entries.remove(voids);"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# Dedup removed, so two forks saying one thing become two facts and each
# carries half the provenance.
# A correction whose content the object already holds. Without the guard the
# supersede links whatever dedup handed back -- an entry to itself, or over a
# link another correction already wrote -- and reports Ok either way.
# The CLI's exit codes, its stdout, and which format sits behind which verb.
# Three mutations of this shape survived cargo test, verify.sh AND --selftest
# before diet/tests/cli.rs existed: nothing ran the program.
# A subshell read as a group. The scoping the grammar exists to preserve --
# `cd a; (cd b; ls); pwd` ending in `a` -- is gone, and every consumer that
# asked which state a bracketed list ran against is told the wrong one.
inject_shell_subshell_as_group() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/shell.rs")
source = path.read_text(encoding="utf-8")
old = "        Rule::subshell => nested_list(&inner).map(Command::Subshell),\n"
new = "        Rule::subshell => nested_list(&inner).map(Command::Group),\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# `|&` read as a plain pipe. The shell sends stderr down that pipe; a reader
# that drops the duplication reports stderr as having gone nowhere.
inject_shell_stderr_pipe_flat() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/shell.rs")
source = path.read_text(encoding="utf-8")
old = "            Rule::pipe if inner.as_str().len() == 2 => match commands.last_mut() {"
new = "            Rule::pipe if inner.as_str().len() == 99 => match commands.last_mut() {"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# Every expansion reported literal. `cd $DIR` then names a directory called
# `$DIR`, and the lane that trusts the flag tracks a path that never existed.
inject_shell_expansion_literal() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/shell.rs")
source = path.read_text(encoding="utf-8")
old = "    Ok(inner.as_rule() == Rule::dollar_alone)\n"
new = "    let _ = inner;\n    Ok(true)\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The producer read as the last command WRITTEN rather than the one whose
# output the line carries. `cargo test | tail -15` is then a `tail` run, and
# every routing decision downstream of it is about the wrong tool.
inject_shell_producer_is_last_written() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/shell.rs")
source = path.read_text(encoding="utf-8")
old = "        match tail.pipeline.first()? {\n"
new = "        match tail.pipeline.last()? {\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# One row dropped from the operator table. `>|` then has no spelling and no
# reading, so a line that truncates a file parses as something else or not at
# all -- and the table was, until this fault existed, guarded only by a test
# that iterated it.
inject_shell_operator_table_row_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/shell.rs")
source = path.read_text(encoding="utf-8")
old = "        (Self::Clobber, \">|\"),\n"
new = ""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# `<<-` recorded as the operator and read as a plain heredoc. The body then
# carries the leading tabs the shell strips before the command ever sees them,
# so the recorded input is not the input that ran.
inject_shell_heredoc_strip_keeps_tabs() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/shell.rs")
source = path.read_text(encoding="utf-8")
old = "        out.push_str(line.trim_start_matches('\\t'));\n"
new = "        out.push_str(line);\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The command word counted among its own operands. The mechanical lane derives
# the files a turn touched from the operands, so `rm -rf build` reports a file
# called `rm`.
inject_shell_command_word_is_an_operand() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/shell.rs")
source = path.read_text(encoding="utf-8")
old = "        self.words.get(1..).unwrap_or(&[])\n"
new = "        &self.words\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# An empty payload read as an absent one. The silence collapse the ablation
# measures is an answer of no characters; a record that turns it into a
# missing field reports the worst case as missing data.
inject_record_empty_payload_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = """) -> Result<Option<String>, ParseError> {
    match members.remove(field) {
        Some(Value::String(text)) => Ok(Some(text)),"""
new = """) -> Result<Option<String>, ParseError> {
    match members.remove(field) {
        Some(Value::String(text)) if text.is_empty() => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A tangent that drops an entry by removing it. Evict to the archive, never
# delete: a drop is a ruling about the trunk, and the entry it ruled on has to
# still be there for anyone to see what was explored.
inject_tangent_drop_removes() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = """                Disposition::Drop => Patch::Retire {
                    target: id.clone(),
                    provenance: self.provenance(at_turn, CLOSING_LANE, None, index),
                },"""
new = """                Disposition::Drop => {
                    object.entries.remove(id);
                    continue;
                }"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The sign let back onto a zero. `-0.0` is then a second spelling of `0.0`,
# and a banked sampler temperature reads back as a number nobody wrote.
#
# The lesion is in `number.pest` because that is where the rule went on
# 2026-09-12; before that it was in the record's own grammar. The move
# CHANGED WHAT THIS FAULT PROVES, and the change is the point: widening the
# shared terminal widens `regimen::float` and `record::decimal` together, so
# the parity test between them stays green -- both readers agree, and both
# are wrong. What catches it is the record's refusal fixture, one level down
# from the agreement. A shared terminal removes the divergence class and
# leaves the widening class exactly where it was; this case is what says so.
inject_record_negative_zero_decimal() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/number.pest")
source = path.read_text(encoding="utf-8")
old = """fraction = @{ ("-" ~ negative_fraction) | (int_part ~ "." ~ ASCII_DIGIT+) }"""
new = """fraction = @{ "-"? ~ int_part ~ "." ~ ASCII_DIGIT+ }"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A parked entry that goes on speaking for the object. Park is the disposition
# for a fact that was true inside the tangent and is not the trunk's; a park
# that renders is a keep with a different name, and the trunk silently
# inherits what the branch was exploring.
inject_tangent_park_renders() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = """                Disposition::Park => Patch::Park {
                    target: id.clone(),
                    provenance: self.provenance(at_turn, CLOSING_LANE, None, index),
                },"""
new = """                Disposition::Park => continue,"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# Scope decided by recency instead of provenance. The trunk goes on writing
# while a tangent runs, so every fact it records after the fork turn is swept
# into the tangent and retired by a closure that never created it.
inject_tangent_scope_by_recency() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = "            .filter(|entry| entry.state.is_live() && born_under(entry) == Some(self.id.as_str()))"
new = """            .filter(|entry| {
                entry.state.is_live()
                    && entry
                        .provenances
                        .first()
                        .is_some_and(|provenance| provenance.turn >= self.at_turn)
            })"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A closure that is not total. An entry the tangent created and nobody ruled
# on stays live, so the trunk inherits it and nothing in the record says who
# decided that.
inject_tangent_undisposed_ignored() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = """        if let Some(id) = scope.iter().find(|id| !dispositions.contains_key(id)) {
            return Err(TangentError::Undisposed { id: id.clone() });
        }
"""
new = ""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The prefix reported intact rather than compared. Rollback is free only for a
# tangent that left the trunk alone, and a claim asserted instead of measured
# is a claim about the design rather than about the run.
inject_tangent_prefix_asserted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = "            prefix_intact: trunk_prefix(object, &self.id) == self.prefix_at_open,"
new = "            prefix_intact: true,"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A tangent opened under an id the record already carries. The earlier
# tangent's entries fall into the new one's scope, and its closure rules on
# facts it never created.
inject_tangent_id_reused() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = """        if object.entries().any(|entry| born_under(entry) == Some(id)) {
            return Err(TangentError::IdInUse { id: id.to_owned() });
        }
"""
new = ""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A tangent that dates every fact it finds at the turn it forked. The turn is
# content, not bookkeeping: a tangent spans turns, and stamping the fork turn
# onto each entry says the whole branch arrived the moment it started looking.
inject_tangent_provenance_ignores_turn() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = """    pub fn provenance(&self, turn: u32, lane: &str, fork: Option<&str>, index: u32) -> Provenance {
        Provenance {
            turn,"""
new = """    pub fn provenance(&self, turn: u32, lane: &str, fork: Option<&str>, index: u32) -> Provenance {
        let _ = turn;
        Provenance {
            turn: self.at_turn,"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A closure that files its rulings at the fork turn. Closure happens later
# than the fork, and a ruling dated before the fact it ruled on is a verdict
# the record says was reached before there was anything to reach it about.
inject_tangent_ruling_dated_at_fork() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = "provenance: self.provenance(at_turn, CLOSING_LANE, None, index),"
new = "provenance: self.provenance(self.at_turn, CLOSING_LANE, None, index),"
assert source.count(old) == 2
source = source.replace(old, new)
head = "        let mut patches = Vec::new();"
assert head in source
path.write_text(source.replace(head, "        let _ = at_turn;\n" + head, 1), encoding="utf-8")
EOF
}

# A closure that coins a lane of its own. The lane is written into the dump
# and orders the turn, so a name the record does not already have is a second
# name for what the tangent in the provenance already says.
inject_tangent_closing_lane_coined() {
  edit_in_place 's|^const CLOSING_LANE: &str = "main";$|const CLOSING_LANE: \&str = "tangent-closure";|' \
    diet/src/object/tangent.rs
}

# A disposition dropped from the list a closure rules with. The enumeration
# test iterates that list, so emptying it does not fail the test -- it just
# leaves the test covering less and still reporting ok.
inject_tangent_disposition_missing_from_all() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = "pub const ALL: &'static [Self] = &[Self::Keep, Self::Drop, Self::Park];"
new = "pub const ALL: &'static [Self] = &[Self::Keep, Self::Drop];"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A scope that offers an entry the record already ruled on. A tangent that
# corrected its own fact would have to rule on the corrected one again, and
# the object refuses to write through it -- so the closure cannot complete.
inject_tangent_scope_offers_a_ruled_entry() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = "            .filter(|entry| entry.state.is_live() && born_under(entry) == Some(self.id.as_str()))"
new = "            .filter(|entry| born_under(entry) == Some(self.id.as_str()))"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A prefix that counts the trunk's dead rows. The trunk goes on ruling on its
# own facts while a tangent runs, and counting rows that had already stopped
# speaking reports the trunk as moved by a change to something it put away.
inject_tangent_prefix_counts_dead_rows() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = "        if !entry.state.is_live() || born_under(entry) == Some(tangent) {"
new = "        if born_under(entry) == Some(tangent) {"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A prefix called intact whenever it only grew. That answers a different
# question than the one the flag asks: the trunk writing while the tangent
# ran moves the prefix, and a caller told otherwise never looks.
inject_tangent_prefix_only_grew() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object/tangent.rs")
source = path.read_text(encoding="utf-8")
old = "            prefix_intact: trunk_prefix(object, &self.id) == self.prefix_at_open,"
new = "            prefix_intact: trunk_prefix(object, &self.id).starts_with(&self.prefix_at_open),"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A parked entry that renders as a retired one. The dump is the durable half
# of the object, and two states rendering the same word read as one to
# everyone who arrives later.
inject_object_park_renders_as_retired() {
  edit_in_place 's|^            Self::Parked => "parked",$|            Self::Parked => "retired",|' \
    diet/src/object.rs
}

# A park with no tangent behind it. Parked says the entry was the tangent's
# rather than the trunk's, so a park nobody forked takes an entry out of the
# live set and records nothing about whose fact it was.
inject_object_park_needs_no_tangent() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = """                if provenance.tangent.is_none() {
                    return Err(ObjectError::ParkOutsideTangent(target.clone()));
                }
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
inject_cli_usage_exit() {
  edit_in_place 's|^const EXIT_USAGE: u8 = 2;$|const EXIT_USAGE: u8 = 0;|' diet/src/bin/diet.rs
}

inject_cli_wrong_format() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/bin/diet.rs")
source = path.read_text(encoding="utf-8")
old = '''    ("parse-interview", Operation::Format("interview")),
    ("check-record", Operation::Format("record")),'''
new = '''    ("parse-interview", Operation::Format("record")),
    ("check-record", Operation::Format("interview")),'''
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

inject_cli_silent() {
  edit_in_place 's|^    println!("{rendered}");$||' diet/src/bin/diet.rs
}

inject_object_self_void() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = """        if let Some(already) = self.by_content.get(&key).cloned() {
            return Err(if already == *voids {
                ObjectError::SelfSupersede(voids.clone())
            } else {
                ObjectError::SupersedeRestates {
                    id: id.clone(),
                    held: already,
                }
            });
        }
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}

# Resolve or Retire written straight through a voided entry. The state is one
# slot and `Voided` keeps the supersede link in it, so the correction is gone
# with nothing to show it happened -- and the double-void guard reads that
# same slot, so it stops firing too.
inject_object_state_overwrite() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = """        if let EntryState::Voided { by } = &entry.state {
            return Err(ObjectError::TargetNotLive {
                id: target.clone(),
                state: EntryState::Voided { by: by.clone() },
            });
        }
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}

# Provenance recording that always claims it wrote. Every no-op patch then
# reports a touched entry, and `touched()` names rows no diff of the dumps
# can support -- which is the acceptance this module is built against.
inject_object_false_attribution() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = """            entry.provenances.push(provenance.clone());
            return true;
        }
        false
    }"""
new = """            entry.provenances.push(provenance.clone());
        }
        true
    }"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# Planning's rulings on #13, each as a guard that has been seen red.

# The regime moves under a patch. There is no Patch variant that can express
# this, and the test that holds it is exhaustive over the variants -- so the
# only way to seed it is to make apply() itself do what no patch can.
inject_object_regime_mutable() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = "        self.version += 1;\n        Ok(applied)"
new = "        self.version += 1;\n        self.regime.dogma_version += 1;\n        Ok(applied)"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# Dedup rebinds rather than aliases: the id the lane chose is forgotten, and
# its next patch naming that id is told the object never heard of it.
inject_object_no_alias() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = "            let aliased = self.record_alias(id, &held);"
new = "            let aliased = false;"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A turn applied in arrival order. Which fork's fact becomes the canonical
# entry then depends on which fork was faster.
inject_object_unsorted_turn() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = """        ordered.sort_by_key(|patch| {
            let p = patch.provenance();
            (p.lane.clone(), p.fork.clone(), p.index)
        });
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}

# The belt removed: a supersede whose new id and whose target resolve to one
# entry through an alias is no longer named as a self-supersede.
inject_object_alias_self_void() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
old = """        if self.canonical(id).as_ref() == Some(voids) {
            return Err(ObjectError::SelfSupersede(voids.clone()));
        }
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}

inject_object_no_dedup() {
  edit_in_place 's|^        if let Some(held) = self.by_content.get(&key).cloned() {$|        if let Some(held) = None::<EntryId> {|' \
    diet/src/object.rs
}

inject_results() {
  cp -r tests/fixtures/results-bad/2026-01-09-unbacked-number results/
}

# A run.jsonl whose start row has lost its substrate. The report is fine; the
# record is not a session record, and only diet says so -- the linter must
# relay that verdict and reach none of its own.
# Take the declared substrates out of a record's `start` row.
#
# ONE READER, because there were two and they went stale one at a time. The
# seeded case and the mechanics assertion below both need this exact edit --
# the case makes it inside a sandbox, the assertion inside a scratch relay --
# and each carried its own copy of the four lines. Item 3 renamed the field,
# the copies were fixed one run apart, and the second cost a whole selftest to
# find. The edit is spelled here now and both call it.
strip_substrates() {
  python3 - "$1" <<'EOF'
import json, pathlib, sys

path = pathlib.Path(sys.argv[1])
lines = path.read_text(encoding="utf-8").split("\n")
row = json.loads(lines[0])
assert row["record"] == "start", "the first row of a record is its start"
del row["regime"]["substrates"]
lines[0] = json.dumps(row, separators=(",", ":"))
path.write_text("\n".join(lines), encoding="utf-8")
EOF
}

inject_results_no_substrate() {
  cp -r results/_template results/2026-01-30-no-substrate
  strip_substrates results/2026-01-30-no-substrate/run.jsonl
}

# A run directory inside a run directory. Every walker is one level deep, so
# the inner one is linted by nothing while carrying a claim record and a
# product digest -- which is how two of them came to be committed here.
inject_results_nested_directory() {
  # Staged through a temporary directory: `cp -r x x/y` copies a directory
  # into itself, which warns and leaves a partial tree. An injection whose
  # own mechanism half-fails proves nothing about the gate.
  local stage
  stage="$(mktemp -d)"
  cp -r results/_template "${stage}/_template"
  mv "${stage}/_template" results/_template/_template
  rmdir "${stage}"
}

# A DIET_BIN that the resolver quietly ignores. The documented way to pin a
# run to a specific build being a silent no-op is how four instruments banked
# numbers through a release binary seven days behind its source.
inject_diet_bin_ignored() {
  edit_in_place 's|^    pinned = os.environ.get("DIET_BIN")$|    pinned = None|' \
    scripts/resolve-diet.py
}

# A results directory carrying a regimen.toml the format refuses.
#
# This wrote `arm = 1.5` until the grammar grew floats, at which point the
# injection kept changing the tree and stopped changing the VERDICT -- the
# gate went green and the seeded case reported "did not fire". An injection
# is proven to change the tree by `check-injections`, which cannot see that
# the change no longer means anything; only the selftest can. So the text
# here is one both readers refuse permanently rather than one the current
# subset happens to exclude: an unterminated string is not TOML and never
# will be.
inject_regimen() {
  printf 'arm = "unterminated\n' > results/_template/regimen.toml
}

inject_toml_subset() {
  # A document the regimen grammar accepts but TOML does not would make the
  # two gates disagree about the same bytes.
  printf 'arm = "a"\rdogma_version = 0\r' \
    > diet/formats/regimen/fixtures/valid/seeded-not-toml.toml
  printf '{ "arm": { "string": "a" } }\n' \
    > diet/formats/regimen/fixtures/valid/seeded-not-toml.expected.json
}

inject_metadata() {
  edit_in_place 's/"name": "claim"/"name": "claim-renamed"/' .github/labels.json
}

# A type error in the surface. `tsc` prints `error TS2322` for exactly this --
# a string where a number was declared -- and prints nothing like it on a
# clean tree, so the signature is the fault's own.
inject_exercise_type_error() {
  printf '\nexport const seededFault: number = %s;\n' "'not a number'" >> exercise/src/ui/format.ts
}

inject_hygiene() {
  bash scripts/seed-hygiene-fault.sh seeded-faults > /dev/null
  git add --all
}

inject_pages() {
  printf '<script src="https://cdn.example.com/x.js"></script>\n' >> pages/index.html
}

# A sandbox is a fresh `git init` with no commits and no remote, so both of
# these build the history they need.
seed_commit() {
  git -c user.email=seed@example.invalid -c user.name=seed commit --quiet "$@"
}

inject_history() {
  git add --all
  seed_commit --message 'a base commit'
  git update-ref refs/remotes/origin/main HEAD
  printf 'x\n' >> pages/index.html
  git add --all
  seed_commit --message "carries $(printf '%s%s' 'DIE' '-9001') forward"
}

# Content that enters a diff and leaves again in the next commit. The tree is
# clean at the end and every commit message is clean throughout, so the file
# gate and the message gate both pass -- and `git log -p` still hands the
# token to anyone. Measured on merged `main` before this was gated: 32
# occurrences in patch text, zero in the tree, both checks green.
inject_history_added_then_removed() {
  git add --all
  seed_commit --message 'a base commit'
  git update-ref refs/remotes/origin/main HEAD
  # The copy lives under .git/, which git never tracks: a scratch file beside
  # the source would be committed by the `git add --all` below and then
  # deleted, putting a second file in the very patches this case reads.
  cp pages/index.html .git/index.html.before
  printf 'host = %s%s\n' '192.168' '.4.9' >> pages/index.html
  git add --all
  seed_commit --message 'add a line'
  cp .git/index.html.before pages/index.html
  git add --all
  seed_commit --message 'and take it out again'
}

# An injection that changes nothing. This is the whole failure the
# `injections` gate exists for: a body whose anchor no longer matches the
# file, or one a line-based merge spliced into silence, still reports its
# seeded case RED -- because the gate it runs was already failing, or because
# it was going to fail anyway -- and proves nothing about the guard it names.
# A float rendered as its digits in quotes. `0.6` becomes `"0.6"` and a
# consumer can no longer tell a temperature from a label that reads like one
# -- the exact lie the ruling that added floats to the regimen refused.
inject_regimen_float_as_a_string() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/regimen.rs")
source = path.read_text(encoding="utf-8")
old = '        Value::Float(number) => ("float", Json::Decimal(number.clone())),\n'
new = '        Value::Float(number) => ("float", Json::String(number.as_str().to_owned())),\n'
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A comment after a table header read as part of the table. `comment` is a
# non-silent rule, so the header yields it among its children, and taking
# every child as a segment opens a table NAMED for the comment: the
# projection disagrees with tomllib about the same bytes, a third segment
# walks off the end of `scope_of` into `unreachable!`, and `[b]` twice stops
# colliding when the second carries a comment -- which accepts a document
# TOML refuses, the one direction this subset may never take.
inject_regimen_header_comment_is_a_table() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/regimen.rs")
source = path.read_text(encoding="utf-8")
old = "                    .filter(|part| part.as_rule() == Rule::key)\n"
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}

# A table header that opens nothing. Every key lands at the top level, so
# `[sampler] seed` and a document-level `seed` become one binding and the arm
# that named both is recorded as an arm that named one.
inject_regimen_table_scope_flattened() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/regimen.rs")
source = path.read_text(encoding="utf-8")
old = "                    Some(segments) => scope_of(&mut entries, segments),\n"
new = "                    Some(_) => &mut entries,\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# Two tables of one name, unchecked. One of the two would have to disappear,
# and a regimen that quietly drops a binding is not the regime that ran.
inject_regimen_table_collision_unchecked() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/regimen.rs")
source = path.read_text(encoding="utf-8")
old = "        Some(Value::Table(_)) if rest.is_empty() => {\n            return Err(ParseError::DuplicateTable { name });\n        }\n"
new = "        Some(Value::Table(_)) if rest.is_empty() => {}\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# The regimen's float rule widened past the record's decimal. `-0.0` is then
# a regimen float and not a record decimal, so the same digits are a value or
# an error depending on which side of the format you ask.
#
# Since 2026-09-12 the two names alias one shared rule, so there is no longer
# a second copy to widen: the ONLY way back to a divergence is to stop
# aliasing and write a body out again under the old name. That is what this
# lesion does.
#
# ONE GATE SEES IT, NOT TWO. The shared-terminal guard in tests/conformance.rs
# compares bodies, and this body is not a copy of the shared one -- it is a
# WIDER one, so the comparison finds no match and the guard stays green.
# Measured, after writing the opposite here first.
#
# The two gates divide the class cleanly, and each is blind where the other
# sees:
#
#   * A body copied EXACTLY under another name leaves the two formats still
#     agreeing, so the parity test is green; the conformance guard catches it.
#     Seeded as `a shared body written out under another name`.
#   * A body written out WIDER, as here, has already diverged, so the
#     conformance guard has nothing to match; the parity test catches it.
#
# Neither of them alone is the gate. That is why the widening half is scoped
# to the regimen tests below and the copying half is scoped to conformance:
# each case runs the gate that can actually see it.
inject_regimen_float_rule_widened() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/regimen/grammar.pest")
source = path.read_text(encoding="utf-8")
old = 'float     = @{ fraction }\n'
new = 'float     = @{ "-"? ~ int_part ~ "." ~ ASCII_DIGIT+ }\n'
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A summary that states a turn count the rows do not carry. The report and
# the record agree -- both say three -- so the results linter passes them, and
# only a re-derivation from the rows themselves can see that the run held two.
# This is the fault that separates gate 0 from the linter: a flipped digit in
# the front-matter alone would have turned `results` red too, and a seeded
# case another gate also catches proves nothing about this one.
inject_recompute_summary_not_derived() {
  python3 - <<'EOF'
import pathlib

report = pathlib.Path("results/_template/README.md")
source = report.read_text(encoding="utf-8")
assert "turns = 2\n" in source
report.write_text(source.replace("turns = 2\n", "turns = 3\n", 1), encoding="utf-8")

record = pathlib.Path("results/_template/run.jsonl")
source = record.read_text(encoding="utf-8")
old = '{"record":"summary","kind":"drive","turns":2,'
new = '{"record":"summary","kind":"drive","turns":3,'
assert old in source
record.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# A directory that declares no kind. It is then neither recomputed nor counted
# as knowingly skipped, and the census that says so is the only thing standing
# between "nothing to check here" and "nothing was checked".
# A recompute.sh that cannot fail. It exits 0 whatever the report says, so
# gate 0 counts it as "1 recomputed" while nothing was re-derived -- the
# vacuous class inside the check whose entire meaning is re-derivation.
inject_recompute_cannot_fail() {
  cat > results/_template/recompute.sh <<'SH'
#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
echo "recompute: 3 recorded value(s) re-derived from the artefacts"
exit 0
SH
}

# THE PROBE DISENGAGED BY A TRAILING COMMENT, which is how the vacuity check
# came to have a vacuity of its own. It located the integer to perturb with
# `^(\w+) = (\d+)$` -- a TOML reader written in a hurry -- so
# `dogma_version = 0  # ...` did not match, the probe returned None, and the
# caller read None as "passed" and counted the directory as RECOMPUTED. A
# script reading nothing and comparing nothing, counted as one recomputed
# result, by the gate whose docstring says a check of nothing is not a pass.
#
# The field is chosen by `tomllib` now -- the same reader `front_matter` uses,
# which is the only one this repository is supposed to have -- and the edit is
# RE-PARSED before the probe is trusted. This fault puts a comment on every
# front-matter integer AND makes the script vacuous: under the old reader the
# tree passes with the directory counted, under the new one the script is
# named. Ruled 2026-09-12.
inject_recompute_probe_blinded_by_a_comment() {
  cat > results/_template/recompute.sh <<'SH'
#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
echo "recompute: 3 recorded value(s) re-derived from the artefacts"
exit 0
SH
  python3 - <<'PY'
import pathlib, re

report = pathlib.Path("results/_template/README.md")
text = report.read_text(encoding="utf-8")
_, fence, rest = text.partition("+++\n")
front, closing, tail = rest.partition("+++\n")
front, count = re.subn(
    r"^([A-Za-z0-9_-]+ = \d+)(?=\s*$)",
    r"\1  # the comment that used to blind the probe",
    front,
    flags=re.M,
)
assert count, "no front-matter integer to comment; this fault would prove nothing"
report.write_text(fence + front + closing + tail, encoding="utf-8")
PY
}

# A recompute.sh that makes the comparison true instead of finding it true.
# Re-derivation reads; a script that rewrites the artefact and then restates
# the report's digest has proved nothing and destroyed the evidence.
inject_recompute_tampers() {
  cat > results/_template/recompute.sh <<'SH'
#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")"
printf 'rewritten to match whatever the report claims\n' > product.txt
python3 -c "
import hashlib, pathlib, re
d = hashlib.sha256(pathlib.Path('product.txt').read_bytes()).hexdigest()
p = pathlib.Path('README.md')
p.write_text(re.sub(r'product_sha256 = \"[0-9a-f]{64}\"', 'product_sha256 = \"' + d + '\"', p.read_text(), count=1))
"
exit 0
SH
}

# A claim naming evidence outside its own directory. A results directory is
# self-contained: a path that climbs out names something this repository does
# not carry, and its digest would be of whatever sat there on the machine.
inject_results_consumes_outside() {
  python3 - <<'EOF'
import json, pathlib
path = pathlib.Path("results/_template/run.jsonl")
rows = [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]
for row in rows:
    if row.get("record") == "claim":
        row["consumes"][0]["path"] = "../_template/product.txt"
path.write_text("\n".join(json.dumps(r, separators=(",", ":")) for r in rows) + "\n",
                encoding="utf-8")
EOF
}

# A claim consuming the record that carries it. The digest would be of a file
# the digest is part of, so it can never be stated correctly -- a record
# cannot hash itself.
inject_results_consumes_the_record() {
  python3 - <<'EOF'
import json, pathlib
path = pathlib.Path("results/_template/run.jsonl")
rows = [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]
for row in rows:
    if row.get("record") == "claim":
        row["consumes"][0]["path"] = "run.jsonl"
path.write_text("\n".join(json.dumps(r, separators=(",", ":")) for r in rows) + "\n",
                encoding="utf-8")
EOF
}

# A claim naming evidence that is not there at all. Until the digests were
# checked this was invisible: a claim could cite a file nobody committed.
inject_results_consumes_a_missing_file() {
  python3 - <<'EOF'
import json, pathlib
path = pathlib.Path("results/_template/run.jsonl")
rows = [json.loads(l) for l in path.read_text(encoding="utf-8").splitlines() if l.strip()]
for row in rows:
    if row.get("record") == "claim":
        row["consumes"][0]["path"] = "never-committed.txt"
path.write_text("\n".join(json.dumps(r, separators=(",", ":")) for r in rows) + "\n",
                encoding="utf-8")
EOF
}

# A directory that declares itself reproducible and ships nothing to reproduce
# it with. Gate 0 must not read the declaration as the deed.
inject_recompute_script_missing() {
  rm -f results/_template/recompute.sh
}

# Every directory declared historical, so gate 0 has nothing to run. A census
# reading "0 recomputed, N historical, 0 undeclared" is a gate over nothing,
# and the tripwire for it is the reason exit 2 exists.
inject_recompute_template_opts_out() {
  edit_in_place 's/^kind = "reproducible-by-config"$/kind = "historical-observation"/' \
    results/_template/README.md
}

inject_recompute_kind_undeclared() {
  python3 - <<'EOF'
import pathlib
import shutil

# A RESULTS directory, not the template. The census counts the template
# separately and it never satisfies the check, so emptying the template's kind
# proves something about the template rather than about an undeclared result --
# which is what this case's label has always said it was for.
seeded = pathlib.Path("results/2026-01-30-seeded-undeclared")
shutil.copytree(pathlib.Path("results/_template"), seeded)
readme = seeded / "README.md"
source = readme.read_text(encoding="utf-8")
row = 'kind = "reproducible-by-config"\n'
assert row in source
readme.write_text(source.replace(row, "", 1), encoding="utf-8")
EOF
}

# A consumed digest that no longer matches its file. The claim then cites
# evidence it never read, which reads exactly like evidence it did.
inject_results_consumed_digest_stale() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("results/_template/product.txt")
path.write_text(path.read_text(encoding="utf-8") + "one more line\n", encoding="utf-8")
EOF
}

# A seeded case naming an injection that does not exist. The pre-flight
# enumerates DEFINITIONS, so a definition deleted outright leaves nothing to
# run and nothing to report inert -- the case goes on claiming coverage over a
# guard nothing exercises. Found on the dev-loop lane, where a merge dropped
# one function whose anchor another injection shared, and only the selftest
# noticed, seven hours later.
inject_case_without_an_injection() {
  python3 - <<'EOF'
import pathlib
import re

path = pathlib.Path("verify.sh")
source = path.read_text(encoding="utf-8")
m = re.search(r"^inject_inert_injection\(\) \{\n.*?^\}\n", source, re.M | re.S)
assert m
path.write_text(source[: m.start()] + source[m.end() :], encoding="utf-8")
EOF
}

# The second level of table flattened: `[serving.flags]` bindings land beside
# `[serving]`'s own. The four cache-ram sweep arms then project identically to
# their siblings except by name, which is a regimen that cannot tell two
# regimes apart -- the exact hazard the level was grown for.
inject_regimen_nested_table_flattened() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/regimen.rs")
source = path.read_text(encoding="utf-8")
old = "    let mut scope = entries;\n    for segment in segments {\n"
new = "    let mut scope = entries;\n    for segment in segments.iter().take(1) {\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

# An array read by a second reader instead of the one a binding uses. A
# decimal inside brackets is then whatever that reader makes of the digits,
# and `0.6` means one thing at the top level and another inside an array.
inject_regimen_array_second_reader() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/regimen.rs")
source = path.read_text(encoding="utf-8")
old = "                .map(|item| value_of(key, &item))\n"
new = "                .map(|item| Ok(Value::String(item.as_str().to_owned())))\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

inject_inert_injection() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("verify.sh")
source = path.read_text(encoding="utf-8")
old = "inject_history_no_base() {\n"
new = "inject_that_changes_nothing() {\n  :\n}\n\ninject_history_no_base() {\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}

inject_history_no_base() {
  git add --all
  seed_commit --message 'the only commit'
  # No origin ref at all: the base is undeterminable, which must fail rather
  # than quietly scan nothing.
}

inject_parity() {
  # Prove a fault the manifest does not account for: parity would then be
  # declared over less than the gate actually covers.
  rm -rf tests/fixtures/results-bad/2026-01-14-bad-sha
}

# The empty-shard guard's own tag, quietly dropped. #77 item 3: a guard
# declared unseedable is not exempt from being seen red -- if its tag goes
# missing, the reasoning above it is still there, now unbound, exactly like
# a `claim:` whose test was deleted.
inject_unseedable_tag_dropped() {
  edit_in_place '/^  # unseedable: a re-entrant --selftest call/d' verify.sh
}

# An injection that writes a literal of an AMBIGUOUS type while editing a file
# that knows none of its declarations. Twenty-three names in this crate are
# declared in more than one place -- measured on 2026-09-11, after a disclosure
# claimed five and had never been re-asked -- and the scan places a literal by
# the file the injection edits, then that file's imports, then a tree-wide
# answer only if there is exactly one. None of the three applies here.
#
# RULED 2026-09-11: that turns the injections lane RED rather than being
# skipped, because a scan that guesses places a literal against the wrong
# declaration, and one that skips reports a pass over something it never read.
# This is the case that proves the branch fires; before it, the branch had
# never been seen red, which is the condition this repository refuses.
#
# `Ground` is the literal because it is declared THREE times -- twice under
# diet/src and once in diet/tests/drive_cli.rs -- so it also proves the scan
# now reads the tests root. `diet/src/lib.rs` names Ground nowhere, which is
# what makes the edited file no help in placing it.
inject_injections_literal_unplaceable() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("verify.sh")
source = path.read_text(encoding="utf-8")
anchor = "inject_injections_struct_grew() {\n"
assert source.count(anchor) == 1, "the anchor is not where this fault expects it"
seeded = (
    "inject_seeded_unplaceable_ground() {\n"
    "  edit_in_place 's/^/ /' diet/src/lib.rs\n"
    # ASSEMBLED FROM PIECES, and it has to be. The scan looks for the two
    # tokens written together, and this fault's own body is scanned like any
    # other: written whole, it made the CLEAN tree fail the check this fault
    # exists to prove fires on a dirty one. Caught by running it -- the fault
    # fired, and named itself alongside the injection it plants. The same
    # trap `inject_injection_needs_gnu_sed` documents, one lint along.
    "  # " + "Ground" + " { tree: PathBuf::new() }\n"
    "}\n\n"
)
path.write_text(source.replace(anchor, seeded + anchor, 1), encoding="utf-8")
EOF
}

# A merge that adds a field to a struct. Every injection that writes a WHOLE
# literal of that struct is now invalid text -- and the tree still builds,
# because an injection's replacement text is a string the compiler never sees.
# This went red on CI once, as "red for the wrong reason", forty minutes in.
inject_injections_struct_grew() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/object.rs")
source = path.read_text(encoding="utf-8")
opened = "pub struct Provenance {\n"
assert source.count(opened) == 1
path.write_text(
    source.replace(
        opened,
        opened + "    /// Seeded fault: a field a merge added.\n"
        "    pub cohort: Option<String>,\n",
        1,
    ),
    encoding="utf-8",
)
EOF
}

# A tag the dogma writes and the vocabulary does not carry. The interview
# parser reads it as prose, silently, on every answer that carries it -- the
# continuation bug in a new dress, and the reason the table is checked against
# the templates rather than trusted beside them.
#
# An INLINE row, deliberately, and the two obvious choices were tried first: a
# tag added to a template also trips the dogma's digest pin, and a `line` row
# removed from the table also trips the grammar/table agreement test. Both
# would go red for two reasons at once, and a signature that fires for two
# faults grades neither.
inject_interview_tag_undeclared() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/interview/tags.tsv")
source = path.read_text(encoding="utf-8")
row = "IMPLICATION\tinline\t-\n"
assert row in source, "the row this injection removes is not in the table"
path.write_text(source.replace(row, "", 1), encoding="utf-8")
EOF
}

# The operating points sorted. A table sorts, which is why this format projects
# to an array -- and sorted, `qwen3` precedes `qwen3.6`, so the id `qwen3.6`
# matches the entry whose `thinking_kwarg` is false, while the entry that
# should have won says true and carries the receipt that the soft switch is
# dead on that model. The wrong control, silently, and reachable.
inject_operating_points_sorted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/operating_points.rs")
source = path.read_text(encoding="utf-8")
anchor = "    Ok(entries)\n"
assert source.count(anchor) == 1, "the projection's return moved"
path.write_text(
    source.replace(anchor, "    entries.sort_by(|a, b| a.key.cmp(&b.key));\n" + anchor, 1),
    encoding="utf-8",
)
EOF
}

# The shadowing refusal switched off, so the dogma's own retired spelling is
# accepted again.
#
# `qwen3` is a substring of `qwen3.6`. Unmarked, whichever is written first
# wins, and an operating point -- a transcribed measurement -- is decided by a
# line number. Enabled 2026-09-13 with the respell, and the fixture it refuses
# is the dogma as it was actually written until that day, not a document
# invented to fail.
#
# Skips the loop rather than deleting it: a deleted loop is an unused-variable
# warning away from being a compile error, and a case that fails to build
# grades BROKEN rather than RED.
inject_shadowing_admitted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/operating_points.rs")
source = path.read_text(encoding="utf-8")
anchor = "    for wider in &entries {\n        if wider.fallback {\n"
assert source.count(anchor) == 1, "the shadowing loop moved"
path.write_text(
    source.replace(anchor, "    for wider in &entries {\n        if true {\n", 1),
    encoding="utf-8",
)
EOF
}

# A grammar that grew its own integer terminal while the others go on sharing
# one.
#
# The obvious injection is wrong and running it is how that was found:
# re-inlining `integer` into the regimen grammar breaks the RECORD parser --
# eighteen compile errors, and the guard never runs, so the case would prove
# the compiler works. Leaving the shared copy in place as well is a duplicate
# rule, which pest rejects: also a build error. The drift that actually
# COMPILES is a third grammar growing a copy of its own, unused, which pest is
# perfectly happy with and nothing but this guard would say a word about.
inject_number_terminal_regrown() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/decline/grammar.pest")
path.write_text(
    path.read_text(encoding="utf-8")
    + '\ninteger = @{ ("-" ~ ASCII_NONZERO_DIGIT ~ ASCII_DIGIT*) | "0" }\n',
    encoding="utf-8",
)
EOF
}

# The same drift wearing a name nobody is looking for. The guard above this
# one asks "is a shared rule defined outside the shared file", which reads
# NAMES -- and a copy called something else walks straight past it. That is
# why the guard grew a body comparison, and this is the case that says the
# comparison works: `nonzero`'s body, written out again as `counter`.
#
# `nonzero` rather than `fraction` because the body has to COMPILE where it
# lands: `fraction` is built out of `int_part` and `negative_fraction`, which
# the decline grammar has never heard of, and a build error would prove the
# compiler works rather than the guard. `nonzero`'s body stands alone.
#
# The name is deliberately innocuous. A drift that announced itself would not
# need a gate.
inject_number_terminal_body_regrown() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/decline/grammar.pest")
path.write_text(
    path.read_text(encoding="utf-8")
    + "\ncounter = _{ ASCII_NONZERO_DIGIT ~ ASCII_DIGIT* }\n",
    encoding="utf-8",
)
EOF
}

# Declared unreproducible and recomputable at once. The way a red result would
# escape gate 0 is by ACQUIRING the tag rather than by losing the script, so
# the two together are a contradiction and not a skip.
inject_recompute_historical_with_a_script() {
  python3 - <<'EOF'
import pathlib
import shutil

template = pathlib.Path("results/_template")
seeded = pathlib.Path("results/2026-01-30-seeded-historical")
shutil.copytree(template, seeded)
readme = seeded / "README.md"
text = readme.read_text(encoding="utf-8")
text = text.replace(
    'kind = "reproducible-by-config"',
    'kind = "historical-observation"\nhistorical_reason = "the capture lane dropped substrate ids"',
    1,
)
readme.write_text(text, encoding="utf-8")
EOF
}

# A tag with no reason behind it, which is the opt-out every red result reaches
# for. Which capture-side flaw, or why the inputs cannot exist.
inject_recompute_historical_without_a_reason() {
  python3 - <<'EOF'
import pathlib
import shutil

template = pathlib.Path("results/_template")
seeded = pathlib.Path("results/2026-01-30-seeded-unreasoned")
shutil.copytree(template, seeded)
(seeded / "recompute.sh").unlink()
readme = seeded / "README.md"
readme.write_text(
    readme.read_text(encoding="utf-8").replace(
        'kind = "reproducible-by-config"', 'kind = "historical-observation"', 1
    ),
    encoding="utf-8",
)
EOF
}

# The reason present and EMPTY. `historical-observation` is the opt-out from
# gate 0, so what it costs is a sentence saying which part of the world made
# the run unreproducible. Two quotes is not that sentence -- it is the tag
# acquired for free, which is what every red result reaches for.
#
# Its own case because the sibling above deletes the KEY, and the two are
# different code paths: one is `reason is None`, this one is a string that is
# there and says nothing. A fresh instance found that removing `.strip()`
# from the check left the absent-key case still red and this one green, so
# the sibling was never testing this line.
inject_recompute_historical_reason_blank() {
  python3 - <<'EOF'
import pathlib
import shutil

template = pathlib.Path("results/_template")
seeded = pathlib.Path("results/2026-01-31-seeded-blank-reason")
shutil.copytree(template, seeded)
(seeded / "recompute.sh").unlink()
readme = seeded / "README.md"
readme.write_text(
    readme.read_text(encoding="utf-8").replace(
        'kind = "reproducible-by-config"',
        'kind = "historical-observation"\nhistorical_reason = "   "',
        1,
    ),
    encoding="utf-8",
)
EOF
}

# Results present and none of them recomputed, with the template excluded from
# the count. A check of nothing is not a pass, applied to results -- and the
# directory this leaves behind is entirely LEGAL, which is the point: the
# census goes red on the shape of the tree, not on a defect in the directory.
# The case below states its expectation as an exact census -- results present,
# none recomputed, gate 0's exit 2 -- and an exact census is only exact over a
# known set of directories. What `results/` holds is the science, which the
# gate does not own. The first real reproducible-by-config directory turns the
# case green: the seeded historical directory is still there, but so is a real
# one that recomputes, and once one is on `main` this fault could never fire
# again. So the case runs over a scratch results root holding only the
# template, whatever the repository contains. The box is already a scratch
# copy of the tree: the injector first reduces the box's results root to the
# template, then seeds its fault, and check_recompute reads that root through
# the check's own --root. Files at the results root (AGENTS.md, README.md)
# stay; the check walks directories. The reduction is spelled out in the
# injector rather than shared, because check-injections runs every injector on
# its own and would report a helper-calling one inert. Ruled on #39
# (2026-09-11), for the two cases that then read the census; the other has
# since been rewritten to seed its own directory and no longer needs it.
inject_recompute_only_the_template_recomputes() {
  find results -mindepth 1 -maxdepth 1 -type d ! -name _template -exec rm -r -- {} +
  python3 - <<'EOF'
import pathlib
import shutil

template = pathlib.Path("results/_template")
seeded = pathlib.Path("results/2026-01-30-seeded-only-historical")
shutil.copytree(template, seeded)
(seeded / "recompute.sh").unlink()
readme = seeded / "README.md"
readme.write_text(
    readme.read_text(encoding="utf-8").replace(
        'kind = "reproducible-by-config"',
        'kind = "historical-observation"\nhistorical_reason = "the substrate no longer exists"',
        1,
    ),
    encoding="utf-8",
)
EOF
}

inject_ci() {
  # Take a check's owner away: it then runs in no workflow, while CI is green.
  edit_in_place '/^hygiene\t/d' .github/check-owners.tsv
}

# A branch filter on the PULL-REQUEST trigger. On `push` the same filter is
# what stops one sha being gated twice, once per event; here it leaves every
# pull request against another branch with no run at all and its required
# checks pending forever. Two spellings a line apart, opposite verdicts --
# which is why the rule is mechanical and not a comment.
inject_ci_pr_branch_filter() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path(".github/workflows/verify.yml")
source = path.read_text(encoding="utf-8")
path.write_text(
    source.replace("  pull_request:\n", "  pull_request:\n    branches: [main]\n", 1),
    encoding="utf-8",
)
EOF
}

# A signature loosened until the line naming its own scope satisfies it.
#
# check_test announces `test scope: lib/capture::router::tests (18 test(s)
# selected)` before it runs anything, and the selftest greps the whole log for
# the case's signature. A signature that is only the module path is therefore
# in the log whether the gate fired or not: the case can still read RED or
# GREEN, because the exit code is the verdict, but it can no longer read
# WRONG -- and WRONG is the verdict a signature exists to make reachable.
#
# Both files are staled together, because a signature that moved in only one
# of them is a different fault with a different refusal, and a seeded case
# that fires for the neighbouring reason proves the neighbour.
inject_parity_scope_signature() {
  python3 - <<'EOF'
import pathlib
import tomllib

# The signature to stale is LOOKED UP, not spelled. Spelling it here would put
# a second copy of it in verify.sh, and the replacement below -- which insists
# on finding exactly one -- would refuse on the copy this injection had just
# added. An injection that cannot run is an injection that proves nothing.
TARGET = "test.router_ask_class_untuned"
NEW = "capture::router::tests"

manifest = pathlib.Path("tools/gate/faults.toml")
doc = tomllib.loads(manifest.read_text(encoding="utf-8"))
entry = next((f for f in doc.get("fault", []) if f.get("id") == TARGET), None)
if entry is None or not entry.get("legacy_signature"):
    raise SystemExit(f"{TARGET}: no such fault, or it carries no signature to stale")
old = entry["legacy_signature"]

# ONE SIGNATURE, TWO SPELLINGS. `tomllib` hands back the DECODED regex, which
# is what verify.sh carries inside shell quotes -- but the manifest's own text
# spells every backslash twice, because a TOML basic string escapes them. That
# was invisible while this fault's signature was prose with nothing to escape:
# the decoded value and the file's text were the same characters, and one
# lookup served both files. The moment #46 moved it to `<test path> \.\.\.
# FAILED` the decoded form appeared zero times in the manifest, and this
# injection -- which refuses rather than guesses -- said so. Re-encode instead
# of searching for a form the file does not contain.
for path, spelling in (
    (pathlib.Path("verify.sh"), old),
    (manifest, old.replace("\\", "\\\\").replace('"', '\\"')),
):
    source = path.read_text(encoding="utf-8")
    if source.count(spelling) != 1:
        raise SystemExit(
            f"{path}: the signature to stale appears {source.count(spelling)} times"
        )
    path.write_text(source.replace(spelling, NEW), encoding="utf-8")
EOF
}

# The push trigger deleted. Every pull request still runs the whole gate and
# still goes green, so nothing looks different -- and `pull_request` grades
# `refs/pull/N/merge`, a preview commit computed at run time. Under a squash
# or a rebase merge the sha that lands on the trunk is one no pull-request run
# ever saw. Delete this trigger and the tree that actually ships is graded by
# nothing, which is the failure this whole file exists to make impossible.
inject_ci_push_ungated() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path(".github/workflows/verify.yml")
source = path.read_text(encoding="utf-8")
old = "  push:\n    branches: [main]\n"
if source.count(old) != 1:
    raise SystemExit("verify.yml: no single `push:` trigger to remove")
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}

# Both corroborating namers stripped of their branch filter. Nothing is
# broken today: verify.yml still says `branches: [main]`, the gate still runs
# on the trunk, and every check passes. What is gone is the second opinion. The
# rule that grades the NAME has only corroboration to grade it with, so with
# one namer left it quietly stops grading anything while still reporting a
# pass -- which is the shape of both defects this whole file was extended for.
inject_ci_trunk_uncorroborated() {
  edit_in_place '/^    branches: \[main\]$/d' \
    .github/workflows/pages.yml .github/workflows/repo-metadata.yml
}

# One character of the trunk's name, in the gating workflow only. `mian` is a
# branch nobody pushes to, so the push trigger fires for nothing and the gate
# watches a branch that does not exist -- and every pull request is still
# green, because the pull-request trigger is untouched. Nothing in this file
# knows what the trunk is called; the other workflows that name it do, and
# they are what catches this.
inject_ci_trunk_typo() {
  edit_in_place 's|^    branches: \[main\]$|    branches: [mian]|' .github/workflows/verify.yml
}

# A gating workflow that narrows the test check to part of the suite. The job
# is green, the run is faster, and most of the tests did not happen.
inject_ci_scoped_test() {
  edit_in_place 's|^\( *\)\./verify\.sh "\${args\[@\]}"$|\1./verify.sh "${args[@]}" --scope lib|' \
    .github/workflows/pkg-diet.yml
}
# The same defect through the flag added for #81's reproduction. `--range`
# makes the history check scan a slice the caller names instead of the one the
# event names, which is why it exists for a person at a terminal -- and why a
# gating workflow may not spell it. A gate that scans a range somebody chose
# reports on history nobody pushed.
inject_ci_ranged_history() {
  edit_in_place 's|^\( *\)\./verify\.sh "\${args\[@\]}"$|\1./verify.sh "${args[@]}" --range HEAD~1..HEAD|' \
    .github/workflows/pkg-repo.yml
}
# A subshell run against the shell's own state. `cd a; (cd b; ls); pwd` then
# ends in `b`, and every relative path after it resolves against a directory
# the session was never in.
inject_mechanical_subshell_leaks() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = """            Command::Subshell(inner) => {
                let mut copy = state.clone();
                self.run_list(inner, &mut copy, call);
                false
            }"""
new = """            Command::Subshell(inner) => {
                self.run_list(inner, state, call);
                false
            }"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A `cd` the shell reported failing, applied anyway. The tracked working
# directory then names a directory that is not there, which is the mistake the
# whole lane exists to stop being made by a model.
inject_mechanical_failed_cd_applied() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = "        let refused = call.refusal(builtin, written.as_deref(), simple);\n"
new = """        let refused: Option<String> = {
            let _ = call.refusal(builtin, written.as_deref(), simple);
            None
        };
"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# `popd` with nothing on the stack passed over in silence. The cwd stays
# right and the record stops saying the session tried to leave.
inject_mechanical_popd_empty_ignored() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = """        self.record_failure(call, simple, EMPTY_STACK.to_owned());
        true
    }"""
new = """        false
    }"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The lint's table of mechanical nouns emptied. Every ask template then passes,
# and the working directory can go back to being an interview question.
inject_mechanical_lint_table_emptied() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = "pub const MECHANICAL_NOUNS: &[&str] = &[\n"
new = "pub const MECHANICAL_NOUNS: &[&str] = &[];\nconst RETIRED_NOUNS: &[&str] = &[\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Mechanical entries routed through the groundedness gate. A derivation is not
# a quotation, so "the working directory is /work/diet" is absent from the row
# that says `cd diet` and the gate drops the lane's whole output as invention.
inject_mechanical_entry_grounded() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = """        let patches = self.patches(turn);
        object.apply_turn(&patches)
"""
new = """        let patches = self.patches(turn);
        let texts: Vec<String> = patches
            .iter()
            .map(|patch| match patch {
                Patch::Add { content, .. } => content.clone(),
                other => format!("{other:?}"),
            })
            .collect();
        let source = self
            .commands
            .iter()
            .map(|run| run.line.as_str())
            .collect::<Vec<_>>()
            .join("\\n");
        let floor = crate::capture::grounded::Floor::pre_registered(1, 2, "the seeded fault")
            .expect("a floor");
        let report = crate::capture::grounded::check(
            &texts,
            crate::formats::interview::FieldKind::ApiSurface,
            crate::capture::grounded::ContractInput {
                source: &source,
                session_prefix: "",
            },
            &floor,
        );
        let kept = report.kept();
        let patches: Vec<Patch> = patches
            .into_iter()
            .zip(texts.iter())
            .filter(|(_, text)| kept.contains(&text.as_str()))
            .map(|(patch, _)| patch)
            .collect();
        object.apply_turn(&patches)
"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The declared default replaced by silence. A pattern nobody wrote a row for
# is exactly the call nobody has looked at yet, and silence is how it stays
# that way.
inject_router_unknown_silent() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = "            Self::Unknown => Routing::Fork(AskKind::Generic),\n"
new = "            Self::Unknown => Routing::Silent,\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A judgment ask released in the middle of a turn: the question that needs
# the model to have concluded something, asked before it has.
inject_router_judgment_mid_turn() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = """            class: classified.class,
            routing,
            ask: match routing {"""
new = """            class: classified.class,
            routing: if routing == Routing::Defer {
                Routing::Fork(AskKind::Judgment)
            } else {
                routing
            },
            ask: match routing {"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A row of the table lost. A test run then routes as unknown, and the corpus
# of real calls is what notices.
inject_router_table_row_lost() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/classes.tsv")
source = path.read_text(encoding="utf-8")
old = "test-run\tshell\tword=cargo sub=test|bench\n"
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# An unknown call routed but not recorded. Misrouting is then a suspicion
# again rather than a number.
inject_router_unclassified_silent() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = """            self.unclassified.push(Unclassified {
                id: id.to_owned(),
                turn,
                tool: tool.to_owned(),
                word: classified.word,
            });
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# The reduction claimed rather than computed from the counts beside it.
inject_router_reduction_claimed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = "            let saved = naive.saturating_sub(self.forks());\n"
new = "            let saved = naive;\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A word's command substitutions found by scanning the text the quoting has
# already been taken out of, instead of by the grammar that read the quoting.
# `echo '$(cat notes.txt)'` then records a file read that never happened,
# which is the one thing this lane exists not to do.
inject_mechanical_quoted_substitution() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/shell.rs")
source = path.read_text(encoding="utf-8")
old = """    Ok(Word {
        text,
        literal,
        substitutions,
    })"""
new = """    let mut substitutions = Vec::new();
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'$'
            && bytes[index + 1] == b'('
            && let Some(end) = text[index + 2..].find(')')
        {
            substitutions.push(text[index + 2..index + 2 + end].to_owned());
            index = index + 2 + end + 1;
            continue;
        }
        index += 1;
    }
    Ok(Word {
        text,
        literal,
        substitutions,
    })"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# An ask wired to another class's question. Both templates still carry the
# imperative and both still render; what stops is the ask being about the
# thing that was just done.
inject_router_ask_class_untuned() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = """            Self::ApiSurface => include_str!("asks/api_surface.txt"),
            Self::Outcome => include_str!("asks/outcome.txt"),"""
new = """            Self::ApiSurface => include_str!("asks/outcome.txt"),
            Self::Outcome => include_str!("asks/api_surface.txt"),"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A template that lost the fork-local imperative. The ask still asks; what it
# stops doing is telling the fork which turn it is answering from.
inject_router_ask_imperative_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/asks/finding.txt")
source = path.read_text(encoding="utf-8")
old = "{imperative}\n"
assert source.startswith(old)
path.write_text(source[len(old) :], encoding="utf-8")
EOF
}
# A census whose totals are right and whose per-class counts are not: the
# drive is told a directory listing was forked.
inject_router_census_class_miscounted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = "            Routing::Silent => tally.silent += 1,\n"
new = "            Routing::Silent => tally.forked += 1,\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# `diet route` answers with a census it did not compute: every count zero,
# ok true, exit 0. The replay still runs, so nothing downstream complains.
inject_router_route_census_hollow() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/bin/diet.rs")
source = path.read_text(encoding="utf-8")
old = """    let replayed = diet::capture::router::replay(&record).map_err(|err| err.to_string())?;
    replayed.census.value().map_err(|err| err.to_string())"""
new = """    diet::capture::router::replay(&record).map_err(|err| err.to_string())?;
    diet::capture::router::Census::default()
        .value()
        .map_err(|err| err.to_string())"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A row moved below one that already claims every call it names. The row is
# still in the table, still parses, and can never decide anything.
inject_router_table_row_shadowed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/classes.tsv")
source = path.read_text(encoding="utf-8")
row = "directory-listing\tshell\tword=git sub=ls-files\n"
below = "version-control\tshell\tword=git|hg|svn|jj\n"
assert row in source and below in source
source = source.replace(row, "", 1)
path.write_text(source.replace(below, below + row, 1), encoding="utf-8")
EOF
}
# The corpus stops covering a class. The calls that remain still route as
# labelled, so only the coverage of the corpus itself says anything.
inject_router_corpus_class_uncovered() {
  python3 - <<'EOF'
import pathlib

corpus = pathlib.Path("diet/capture/router/corpus")
for name in ("tool-families.jsonl", "tool-families.expected.json"):
    path = corpus / name
    assert path.exists(), path
    path.unlink()
EOF
}
# A table row that does not parse, skipped instead of refused. Every call in
# the drive is then unknown, which is what a router with a perfect table
# reports for a drive full of novel tools.
inject_router_table_row_skipped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = """        let [class, family, rule] = fields.as_slice() else {
            return Err(TableError::Shape { line });
        };"""
new = """        let [class, family, rule] = fields.as_slice() else {
            continue;
        };"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The stated intent taken from whatever lane spoke last. An interview's own
# answer is then quoted back to the drive as something the drive said.
inject_router_intent_lane_ignored() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = "                    .is_some_and(|lane| lane == CANONICAL_LANE)\n"
new = "                    .is_some_and(|lane| lane != CANONICAL_LANE)\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The first stated intent instead of the last: the ask quotes back something
# the model has already finished doing.
inject_router_intent_first_not_last() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = """    sentences
        .iter()
        .rev()
        .map(|sentence| sentence.trim())"""
new = """    sentences
        .iter()
        .map(|sentence| sentence.trim())"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A phrase the model states its intent with, dropped from the table. The
# stated-intent hole then goes unfilled for every turn that used it.
inject_router_intent_marker_lost() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = '    "let me ",\n'
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# An unclassified call recorded without the tool and against the wrong turn.
# The count is still right, and nothing it names can be looked up.
inject_router_unclassified_unattributed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = """                id: id.to_owned(),
                turn,
                tool: tool.to_owned(),
                word: classified.word,"""
new = """                id: id.to_owned(),
                turn: turn + 1,
                tool: String::new(),
                word: classified.word,"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The declared default dropped out of the vocabulary. Every loop that walks
# `Class::ALL` then walks past it rather than over it.
inject_router_class_vocabulary_shortened() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/router/mod.rs")
source = path.read_text(encoding="utf-8")
old = """        Self::VersionControl,
        Self::Unknown,
    ];"""
new = """        Self::VersionControl,
    ];"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A cosine that divides by one norm and the square of the other. Every
# similarity becomes a function of how long the sentence is, so the seeded
# control row -- the sense text verbatim -- no longer sits at one, and every
# ranking in the bakeoff is a ranking by length.
inject_sense_cosine_unnormalised() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "    let norm_a = a.iter().map(|x| x * x).sum::<f64>().sqrt();\n"
new = "    let norm_a = a.iter().map(|x| x * x).sum::<f64>();\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Contrastive scoring that never subtracts the authored negative sense. It
# becomes raw cosine wearing another tag, and the one repair the bakeoff has
# for an abstract description sitting near everything is reported as measured
# and is not there.
inject_sense_contrastive_ignores_negative() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "                Some(toward - away)\n"
new = "                let _ = away;\n                Some(toward)\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A shuffled-label null that never shuffles. It reports the real separation as
# what chance looks like, so every cell measured against it is measured
# against itself and no metric can be caught finding structure in noise.
inject_sense_null_labels_unshuffled() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "            let j = rng.below(i + 1);\n            labels.swap(i, j);\n"
new = "            let _ = rng.below(i + 1);\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A bootstrap p-value that travels without the floor its resample count
# implies. A p of 0.001 from 999 resamples is the smallest number the
# procedure can produce, and printed alone it reads as a finding.
inject_sense_p_without_floor() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "            floor: attainable_p_floor(resamples),\n"
new = "            floor: 0.0,\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A metric whose demonstrated-failure fixture is deleted. The metric still
# computes and still prints, and nothing has ever seen it report failure --
# which is the shape a perfect grounding score of 1.000 had on a probe where
# fabrication was structurally impossible.
inject_sense_metric_fixture_removed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = """    (
        Metric::Auc,
        &[
            ("failing/positive/0.1", Label::Positive, 0.1),
            ("failing/positive/0.2", Label::Positive, 0.2),
            ("failing/negative/0.8", Label::Negative, 0.8),
            ("failing/negative/0.9", Label::Negative, 0.9),
        ],
    ),
"""
assert source.count(old) == 1
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# The one-object reader taking the first line of a file and dropping the rest.
# Every data file this reader serves -- sense sets, registers, vector caches --
# is read one line at a time, so a reader that silently accepts two returns a
# row nobody wrote and loses one somebody did.
inject_record_data_line_two_lines() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/json.rs")
source = path.read_text(encoding="utf-8")
old = """    if event_line.as_span().end() != text.len() {
        return Err(LineError::NotOneLine);
    }
"""
assert source.count(old) == 1
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# Controls that never look at the register. The two seeded control rows are
# still scored and still compared with each other, so the run reports its
# controls as being at their extremes -- while an embedder that cannot tell
# the authored sense from a transcript sentence ties them and is not caught.
inject_sense_controls_ignore_register() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = """    let control_ids = [top.id(set.set()), bottom.id(set.set())];
    for row in scored.iter().filter(|row| !control_ids.contains(&row.id)) {
        if row.score >= top_score {
            return Err(ControlFailure::NotAtTop {
                control: top,
                score: top_score,
                row: row.id.clone(),
                other: row.score,
            });
        }
        if row.label.is_positive() && row.score < bottom_score {
            return Err(ControlFailure::NotAtBottom {
                control: bottom,
                score: bottom_score,
                row: row.id.clone(),
                other: row.score,
            });
        }
    }
"""
assert source.count(old) == 1
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# A paired bootstrap whose every resample is the observed sample. Nothing ever
# crosses zero, so every p the bakeoff prints is the attainable floor -- the
# smallest number the procedure can produce, reported for every comparison as
# though it were a finding.
inject_sense_bootstrap_never_resamples() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "    let total: f64 = (0..n).map(|_| differences[rng.below(n)]).sum();\n"
new = "    let _ = rng;\n    let total: f64 = (0..n).map(|i| differences[i]).sum();\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The reading at which a metric counts as having failed, moved off the worst
# the metric can say. An area under the curve of 0.9 is nearly perfect
# separation, and a fixture that reaches it would then certify every number
# the bakeoff goes on to report.
inject_sense_failure_reading_moved() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "            Self::Auc => 0.5,\n"
new = "            Self::Auc => 0.9,\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The lane renamed. Every derived entry then claims to have come from `main`,
# the canonical lane, whose authority a mechanical derivation does not carry.
inject_mechanical_lane_renamed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = 'pub const LANE: &str = "mechanical";'
new = 'pub const LANE: &str = "main";'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A pre-registration whose primary endpoint says nothing. The plan is the one
# artefact that has to be fixed before the data arrives; emptied, it can be
# written once the numbers are in and read as though it never had been.
inject_sense_pre_registration_emptied() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = """    primary: "precision at a fixed nomination budget, the top k of the register, per embedder, \\
              scoring and gate",
"""
new = '    primary: "",\n'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# An option word to a builtin read as the directory it names. `cd -P /work/x`
# then states the working directory as `/work/-P`: an absolute path, marked
# resolved, that every later relative path resolves against.
inject_mechanical_option_is_a_directory() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = """        let optioned = simple.operands().iter().any(is_option);
        let argument = simple.operands().iter().find(|word| !is_option(word));"""
new = """        let optioned = false;
        let argument = simple.operands().first();"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The lexical pre-gate asked about the row's id instead of its text. The
# shipped ids are slugs of their texts and mostly agree, so the with-gate arm
# of every cell is computed from identifiers with a row silently dropped to
# the scoring's floor -- and the gate is one of the two factors the bakeoff
# exists to measure.
inject_sense_gate_reads_the_id() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "            let admitted = cell.gate.admits(set.set(), &row.text);\n"
new = "            let admitted = cell.gate.admits(set.set(), &row.id);\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A flag's value read as a file operand. `touch -t 202401010000 f.txt` then
# says the turn wrote a file named after the timestamp.
inject_mechanical_flag_value_is_a_file() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = """    FileCommand::plain("touch", Operands::Write, &["-d", "-r", "-t"]),
    FileCommand::plain("mkdir", Operands::Write, &["-m"]),"""
new = """    FileCommand::plain("touch", Operands::Write, &[]),
    FileCommand::plain("mkdir", Operands::Write, &[]),"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A standardised separation divided by one class's spread rather than by both.
# It is one of the two pre-registered separation endpoints, and a cell could
# report a separation computed from the positives alone.
inject_sense_d_prime_unpooled() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "    let pooled = f64::midpoint(positive_moments.variance, negative_moments.variance).sqrt();\n"
new = "    let pooled = positive_moments.variance.sqrt();\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The verb an entry uses to say what happened to a file, swapped. The lane
# then writes, under capture authority, that a file it made was deleted.
inject_mechanical_entry_verb_swapped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = """            Self::Read => "read",
            Self::Written => "wrote",
            Self::Deleted => "deleted","""
new = """            Self::Read => "read",
            Self::Written => "deleted",
            Self::Deleted => "wrote","""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The null's acceptance band widened until nothing is outside it. A
# shuffled-label null separating the two classes by a whole standard deviation
# is then reported as chance, and every cell measured against that null is
# measured against itself.
inject_sense_null_band_widened() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = "pub const NULL_D_PRIME_BAND: f64 = 0.25;\n"
new = "pub const NULL_D_PRIME_BAND: f64 = 5.0;\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A pipeline's members never run. Nothing a pipeline reads is recorded, and a
# pipeline is the ordinary shape of an agent's shell call.
inject_mechanical_pipeline_skipped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = """        if link.pipeline.len() > 1 {
            for command in &link.pipeline {
                let mut copy = state.clone();
                self.run_command(command, &mut copy, call);
            }
            return false;
        }"""
new = """        if link.pipeline.len() > 1 {
            return false;
        }"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A reported metric whose record carries a constant instead of the number the
# metric produced. The record is the one door from a computed number to a
# result, and the assembly can be guarded while the content is not.
inject_sense_reported_value_constant() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = '            ("value".to_owned(), decimal(self.value, 4)),\n'
new = '            ("value".to_owned(), decimal(0.0, 4)),\n'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A path written and then read by one call kept as one fact. The write is the
# one discarded, so the lane tells a later reader the turn wrote nothing.
inject_mechanical_write_lost_to_a_read() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/mechanical.rs")
source = path.read_text(encoding="utf-8")
old = """                && matches!(
                    &fact.derived,
                    Derived::Touch { path: held, touch: held_touch }
                        if *held == path && held_touch.kind == kind
                )"""
new = """                && matches!(&fact.derived, Derived::Touch { path: held, .. } if *held == path)"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A register whose rows disagree with the name on the file. The directory
# then lists an authored register that is in fact corpus, and a metric taken
# over it reads as a statement about the world.
inject_sense_register_source_mislabelled() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/capture/sense/register/authored-mistake.jsonl")
source = path.read_text(encoding="utf-8")
old = '"source":"authored"'
assert old in source
path.write_text(source.replace(old, '"source":"mined"', 1), encoding="utf-8")
EOF
}
# A file in the register directory that names nothing. A walk that skipped it
# would skip a register whose name was mistyped and call the directory clean.
inject_sense_register_unnamed_file() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/capture/sense/register/notaregister.jsonl")
assert not path.exists()
path.write_text('{"id":"a/b","text":"x","label":"positive","source":"authored"}\n', encoding="utf-8")
EOF
}
# The join between a mined row and its provenance made optional. A mined
# register can then ship a row nobody can trace, which is evidence in name
# and an assertion in fact.
inject_sense_provenance_join_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/sense.rs")
source = path.read_text(encoding="utf-8")
old = """    if let Some(row) = register
        .iter()
        .find(|row| !traced.contains(row.id.as_str()))
    {
        return Err(JoinError::Untraced(row.id.clone()));
    }
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# The compile-failure pattern anchored `^` with no re.MULTILINE. It then
# matches only a log that BEGINS with the error, and every real log begins
# with `Compiling`, so a fault that breaks the BUILD reads as "no failing test
# and no build failure" -- exit 2, the derivation refusing to run over a case
# that is perfectly readable. This is the defect the file actually had.
inject_derive_build_failure_unread() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("scripts/derive-scopes.py")
source = path.read_text(encoding="utf-8")
old = ', re.MULTILINE\n)'
new = '\n)'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The selection forgetting which target ran a failure. Every declared scope
# then "selects" every failure in the log, whichever harness produced it, and
# the one thing this check exists to refuse -- a case scoped past its own
# failure, which runs tests that all pass and reports GREEN over a seeded
# fault -- is accepted by the grader written to catch it.
inject_derive_accepts_a_fast_green() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("scripts/derive-scopes.py")
source = path.read_text(encoding="utf-8")
old = '        if target not in ("all", ran_in):\n            continue\n        hit |='
new = '        hit |='
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The shard packer balancing the COUNT of faults instead of their cost. That
# is round-robin wearing a bin-packer's clothes, and it is exactly the split
# #87 was filed against: every shard gets the same number of faults, and the
# shard that drew the Rust-class ones runs for half as long again as the one
# that drew the pattern classes.
inject_derive_shards_packs_by_count() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("scripts/derive-shards.py")
source = path.read_text(encoding="utf-8")
old = "        load[lightest] += cost[ident]"
new = "        load[lightest] += 1"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The balance check grading a shard against the MEAN of the shards rather than
# their median. The mean is dragged up by the outlier it is being used to find,
# so the check goes quiet exactly as the imbalance gets bad enough to matter --
# a guard that fires on a small problem and not on a large one.
inject_derive_shards_outlier_against_the_mean() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("scripts/derive-shards.py")
source = path.read_text(encoding="utf-8")
old = "    middle = statistics.median(summed)"
new = "    middle = sum(summed) / len(summed)"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Half the checked-in assignment's faults moved onto shard 1, in the real
# file `--check` reads on every run of the `ci` check -- not a synthetic
# index handed to the grading function directly, which is what the sibling
# outlier fixture in derive-shards.py's own selftest already covers. Every
# shard keeps at least one fault, so this is distinguishable from the
# empty-shard refusal; shard 1 alone carries roughly half the harvest's
# total cost, which is far past 3x any other shard's share for any shard
# count `--emit` would ever produce.
inject_shard_assignment_outlier() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("tools/gate/shards.tsv")
lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
fault_lines = [i for i, line in enumerate(lines) if line.startswith("fault\t")]
assert len(fault_lines) >= 2, "not enough faults to make an outlier"
half = fault_lines[: len(fault_lines) // 2]
for i in half:
    parts = lines[i].rstrip("\n").split("\t")
    parts[2] = "1"
    lines[i] = "\t".join(parts) + "\n"
path.write_text("".join(lines), encoding="utf-8")
EOF
}
# The declared wall-clock budget deleted. The number is what the shard count is
# derived from and what every run's report is printed against; without it the
# gate would carry on, silently, with no ceiling at all -- which is the state
# #87 found the repository in.
inject_gate_budget_undeclared() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path(".github/gate-budget.tsv")
lines = path.read_text(encoding="utf-8").splitlines(keepends=True)
kept = [line for line in lines if not line.startswith("wall_clock_seconds\t")]
if len(kept) != len(lines) - 1:
    raise SystemExit("wall_clock_seconds is not declared exactly once")
path.write_text("".join(kept), encoding="utf-8")
EOF
}
# The resolver keying a block on the first injection name anywhere inside it,
# rationale comment included. A comment reading "the deliberate pair of
# inject_alpha" then keys the block that introduces inject_beta as
# inject_alpha, the union sees a name it already holds, and the genuinely new
# body is skipped with no warning and exit 0. Silence is the whole hazard.
inject_keyed_by_a_comment() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("scripts/merge-gate.py")
source = path.read_text(encoding="utf-8")
old = '        m := re.search(r"^(inject_[a-z0-9_]+)\\(\\) \\{", block, re.M)\n    ) and m.group(1)\n'
new = '        m := re.search(r"inject_[a-z0-9_]+", block)\n    ) and m.group(0)\n'
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The answer's tail swallowed instead of anchored. `DONEISH` is then a
# `DONE`, `PARTIALLY` a `PARTIAL`, and every word after the verdict is
# discarded -- so the reconciler applies a judgment the fork did not give.
inject_verdict_prefix_accepted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/verdict/grammar.pest")
source = path.read_text(encoding="utf-8")
old = "document = { SOI ~ ws* ~ verdict ~ reason? ~ ws* ~ EOI }"
new = "document = { SOI ~ ws* ~ verdict ~ reason? ~ ANY* ~ EOI }"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The boundary check in `upper_verdict` removed. A reason that names
# `DONE_MARKER` is then a second answer, and one answer the fork did give is
# refused as two.
inject_verdict_identifier_read_as_a_verdict() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/verdict/grammar.pest")
source = path.read_text(encoding="utf-8")
old = 'upper_verdict = _{ ("SUPERSEDED" | "NOT_THIS" | "NOT THIS" | "PARTIAL" | "DONE") ~ !word_char }'
new = 'upper_verdict = _{ "SUPERSEDED" | "NOT_THIS" | "NOT THIS" | "PARTIAL" | "DONE" }'
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A second verdict inside a reason accepted. The reconciler branches on one
# word; `DONE - or PARTIAL` hands it two.
inject_verdict_second_verdict_accepted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/formats/verdict/grammar.pest")
source = path.read_text(encoding="utf-8")
old = "reason_token = _{ !upper_verdict ~ (!sp ~ !nl ~ ANY)+ }"
new = "reason_token = _{ (!sp ~ !nl ~ ANY)+ }"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# An anchor matched inside a longer word. `plan_a` then recurs in every
# `plan_ab`, and the entry is nominated by every mention of the longer name.
inject_collector_substring_match() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = "    !continues(before) && !continues(after)\n"
new = "    let _ = (before, after, continues);\n    true\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# An English word made an anchor. `plan` is in every third sentence of a
# drive, and an entry anchored on it is nominated by everything.
inject_collector_english_anchor() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = "        .any(|(a, b)| a.is_lowercase() && b.is_uppercase())\n}\n"
new = "        .any(|(a, b)| a.is_lowercase() && b.is_uppercase())\n        || has_alpha\n}\n"
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# An entry nominated by the turn that made it. Its own prose carries its own
# anchors, so every new entry would go straight to a confirm fork.
inject_collector_self_nomination() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = """        if entry.provenances.iter().any(|p| p.turn >= new.turn) {
            continue;
        }
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# A supersession that adds without voiding. Both facts are then live, the
# object holds a contradiction, and the entry the archive was supposed to
# make recoverable is instead one of two answers with nothing to choose
# between them.
inject_reconcile_supersede_without_voiding() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/reconcile.rs")
source = path.read_text(encoding="utf-8")
old = """        Verdict::Superseded => Outcome::Superseded(Patch::Supersede {
            id: EntryId::new(&format!(
                "{}/supersedes/{}",
                replacement.event, nomination.entry
            ))?,
            content: replacement.content.to_owned(),
            voids: nomination.entry.clone(),
            provenance: replacement.provenance,
        }),"""
new = """        Verdict::Superseded => Outcome::Superseded(Patch::Add {
            id: EntryId::new(&format!(
                "{}/supersedes/{}",
                replacement.event, nomination.entry
            ))?,
            content: replacement.content.to_owned(),
            provenance: replacement.provenance,
        }),"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A verdict that settles nothing made to settle something. `PARTIAL` says
# the prose bears on the entry without replacing it; a reconciler that
# resolves on it closes an entry the fork deliberately left open.
inject_reconcile_partial_applies_a_patch() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/reconcile.rs")
source = path.read_text(encoding="utf-8")
old = "        Verdict::Partial => Outcome::Partial,\n"
new = """        Verdict::Partial => Outcome::Resolved(Patch::Resolve {
            target: nomination.entry.clone(),
            provenance: replacement.provenance,
        }),
"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The fixture policy accepted by the door that ships. Its threshold was
# chosen to make the fixtures readable, which is exactly the property a
# shipped threshold must not have, so a tier running on it spends confirm
# forks at a rate nobody measured. The neighbouring case takes the other
# half of the same door: a `calibrated_on` that names no run at all.
inject_collector_uncalibrated_policy() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = "            Calibration::Fixture => Err(PolicyError::Fixture),\n"
new = "            Calibration::Fixture => Ok(policy),\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The budget removed. One turn then nominates the whole object the first
# time a threshold is set a little too low.
inject_collector_budget_ignored() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = "    scored.truncate(policy.budget as usize);\n"
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# Tier 0 stopped after the first entry it found. A turn that reverses two
# facts loses one of them, silently: no budget to point at and no report
# saying anything was left out.
inject_collector_one_nomination_per_turn() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = '        }\n    }\n    nominations\n}'
new = '        }\n    }\n    nominations.truncate(1);\n    nominations\n}'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Tier 0 read every entry rather than the live ones. An entry a verdict
# already voided is nominated again on every later turn that names its
# anchor, spending a confirm fork on an answer that is already on file.
inject_collector_voided_entry_renominated() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = '    for entry in object.live() {'
new = '    for entry in object.entries() {'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Every hit reported offset zero. The offset is half of what a literal
# nomination hands the confirm fork -- this anchor recurred, and here.
inject_collector_hit_offset_lost() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = '            hits.push(Hit { offset: at });'
new = '            hits.push(Hit { offset: 0 });'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The scan resumed one byte past the start of the match instead of past
# the match, so an anchor that overlaps itself is counted once per
# shifted position.
inject_collector_overlapping_scan() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = '        from = at + needle.len();'
new = '        from = at + 1;'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Only backticks set a span off. A double-quoted term is one of the three
# anchor classes the tier claims, and it went untested for a whole branch.
inject_collector_quoted_anchor_delimiter() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = '    for delimiter in [\'`\', \'"\'] {'
new = "    for delimiter in ['`'] {"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The minimum anchor length dropped to one byte, so two-character
# coincidences become anchors and the tier fires on version numbers.
inject_collector_short_shape_anchored() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = 'const MIN_ANCHOR_LEN: usize = 3;'
new = 'const MIN_ANCHOR_LEN: usize = 1;'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# `a::b` stopped being an identifier shape. A path through the module
# tree is the one anchor class a Rust drive produces most.
inject_collector_module_path_shape() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = '    // a::b\n    if token.contains("::") && !token.starts_with(\':\') && !token.ends_with(\':\') {\n        return true;\n    }\n'
new = ''
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The file-extension window widened, so a dotted word whose tail is an
# English word is read as a file name and a record carries the wrong kind.
inject_collector_extension_window() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = '            && (1..=4).contains(&ext.len())'
new = '            && (1..=12).contains(&ext.len())'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The anchor kinds swapped their spellings. A record then says `quoted`
# for an identifier, and nothing reads it back.
inject_collector_anchor_kind_permuted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = '            Self::Identifier => "identifier",\n            Self::Path => "path",\n            Self::Quoted => "quoted",'
new = '            Self::Identifier => "quoted",\n            Self::Path => "identifier",\n            Self::Quoted => "path",'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The source vocabulary lost its members, so a test that walked `ALL`
# would walk nothing and pass.
inject_collector_source_vocabulary_emptied() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = "    pub const ALL: &'static [Self] = &[Self::Prose, Self::ToolOutput];"
new = "    pub const ALL: &'static [Self] = &[];"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A nomination named the other tier. The two cost different things to be
# wrong about, and the name is how a reader tells them apart later.
inject_collector_tier_name_swapped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/mod.rs")
source = path.read_text(encoding="utf-8")
old = '            Evidence::Literal { .. } => "literal",\n            Evidence::Sense { .. } => "sense",'
new = '            Evidence::Literal { .. } => "sense",\n            Evidence::Sense { .. } => "literal",'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The two registers swapped their spellings, so a nomination measured
# against the turn's stated intent says it came from the reversal senses.
inject_collector_register_permuted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '            Self::Reversal => "reversal",\n            Self::Intent => "intent",'
new = '            Self::Reversal => "intent",\n            Self::Intent => "reversal",'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The lexical pre-gate stopped being applied. It is one of the four
# settings a calibration run fixes, and the policy this tree ships selects
# it, so a tier that ignores it is running a cell nobody measured.
inject_collector_gate_ignored() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '    if !policy.gate.admits(SenseSet::Reversal, new.prose) {\n        return Ok(Vec::new());\n    }\n'
new = ''
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The turn-level threshold stopped being compared. Silence on a turn that
# reverses nothing is this tier's main product, and it was masked by the
# per-entry cut for a whole branch.
inject_collector_turn_threshold_ignored() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '    if turn_score < policy.cut() {\n        return Ok(Vec::new());\n    }\n'
new = '    let _ = turn_score;\n'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The per-entry threshold stopped being compared, so a turn that reverses
# anything nominates every live entry up to the budget.
inject_collector_entry_threshold_ignored() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '        if score >= policy.cut() {\n            scored.push((score, entry));\n        }\n'
new = '        scored.push((score, entry));\n'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The turn was scored against the authored `mistake` senses instead of
# `reversal`. Both are real event classes; only one retires a fact.
inject_collector_wrong_sense_set() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = 'EmbeddedSet::embed(senses, SenseSet::Reversal, embedder)'
new = 'EmbeddedSet::embed(senses, SenseSet::Mistake, embedder)'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The second register measured the turn's prose twice instead of its
# stated intent, so whichever entry shares the reversal's words is
# nominated rather than the entry the turn is about.
inject_collector_intent_register_reads_the_prose() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '    let intent = embedder.embed(new.intent);'
new = '    let intent = embedder.embed(new.prose);'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Tier 1 nominated an entry born in the turn that is reading it. Tier 0
# keeps the same rule and has its own case; this is the tier-1 copy, which
# was pinned by a test that named an id the object never held.
inject_collector_sense_self_nomination() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '        if entry.provenances.iter().any(|p| p.turn >= new.turn) {\n            continue;\n        }\n'
new = ''
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Tier 1 read every entry rather than the live ones -- the same fault as
# tier 0's, on the tier that has a budget to spend.
inject_collector_sense_voided_entry_renominated() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '    for entry in object.live() {'
new = '    for entry in object.entries() {'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The tier's report lost the scoring and the gate. A nomination count and
# a threshold cannot be matched to the cell that produced them without the
# other two factors beside them.
inject_collector_report_drops_the_policy() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '        (\n            "scoring".to_owned(),\n            Value::String(policy.scoring.tag().to_owned()),\n        ),\n        (\n            "gate".to_owned(),\n            Value::String(policy.gate.tag().to_owned()),\n        ),\n'
new = ''
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The calibration door stopped asking whether `calibrated_on` names a run
# and only refused the fixture by name, so any other word is a
# calibration -- the fixture's own tag with a space on the end included.
inject_collector_policy_names_no_run() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = '            Calibration::Run(directory) if !names_a_run(directory) => {\n                Err(PolicyError::Uncalibrated)\n            }\n'
new = ''
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The superseding entry carried a constant instead of the fact that
# replaced the old one. The link is right, the states are right, and the
# object accumulates entries whose text is a placeholder.
inject_reconcile_supersede_writes_a_constant() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/reconcile.rs")
source = path.read_text(encoding="utf-8")
old = '            content: replacement.content.to_owned(),'
new = '            content: "superseded".to_owned(),'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The accessor a caller applies answered `None` for every verdict. The
# four outcomes stay distinct, every assertion about them stays true, and
# the module applies nothing.
inject_reconcile_patch_never_handed_back() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/reconcile.rs")
source = path.read_text(encoding="utf-8")
old = '            Self::Superseded(patch) | Self::Resolved(patch) => Some(patch),\n            Self::Partial | Self::NotThis => None,'
new = '            Self::Superseded(_) | Self::Resolved(_) | Self::Partial | Self::NotThis => None,'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# PARTIAL collapsed into NOT_THIS. NOT_THIS is the false nomination the
# precision gate is calibrated against, so conflating the two inflates the
# number the gate reads.
inject_reconcile_partial_read_as_a_false_nomination() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/reconcile.rs")
source = path.read_text(encoding="utf-8")
old = '        Verdict::Partial => Outcome::Partial,\n        Verdict::NotThis => Outcome::NotThis,\n'
new = '        Verdict::Partial | Verdict::NotThis => Outcome::NotThis,\n'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The punctuation prose hangs on a token stayed part of the token, so an
# anchor at the end of a sentence is not found at all.
inject_collector_token_untrimmed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/literal.rs")
source = path.read_text(encoding="utf-8")
old = "fn trim_token(token: &str) -> &str {\n"
new = """fn trim_token(token: &str) -> &str {
    if !token.is_empty() {
        return token;
    }
"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The ranking reversed, so the budget is spent on the entries least likely
# to be the one. A precision-first tier with an anti-sorted budget is worse
# than a tier with no budget at all.
inject_collector_budget_takes_the_worst() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = """        right
            .partial_cmp(left)
"""
new = """        left
            .partial_cmp(right)
"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Every nomination carried a constant instead of the score that was measured.
# The number is the only thing that can be compared to the run that set the
# threshold, which is the whole argument for the policy having one.
inject_collector_score_not_measured() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/sense.rs")
source = path.read_text(encoding="utf-8")
old = """        .filter_map(|(score, entry)| {"""
new = """        .filter_map(|(_score, entry)| {"""
assert source.count(old) == 1
source = source.replace(old, new, 1)
old = "                    score: written(score)?,"
new = "                    score: written(0.0)?,"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The superseding entry lost the provenance of the fork that judged, so
# nothing can attribute a wrong supersession to the turn that made it.
inject_reconcile_supersede_loses_its_fork() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/reconcile.rs")
source = path.read_text(encoding="utf-8")
old = """            voids: nomination.entry.clone(),
            provenance: replacement.provenance,"""
new = """            voids: nomination.entry.clone(),
            provenance: Provenance {
                turn: 0,
                ..replacement.provenance
            },"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A fork that said the nomination was wrong superseded the entry anyway. A
# mention is not a reversal, and the verdict is the only thing that tells
# them apart.
inject_reconcile_mention_superseded() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/collector/reconcile.rs")
source = path.read_text(encoding="utf-8")
old = "        Verdict::NotThis => Outcome::NotThis,\n"
new = """        Verdict::NotThis => Outcome::Superseded(Patch::Supersede {
            id: EntryId::new(&format!(
                "{}/supersedes/{}",
                replacement.event, nomination.entry
            ))?,
            content: replacement.content.to_owned(),
            voids: nomination.entry.clone(),
            provenance: replacement.provenance,
        }),
"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Self-capture exempt from the groundedness gate. The modality is the whole
# argument -- a tool call rides the harness's own parsing instead of the
# content channel -- and none of that says anything about whether the content
# is true. The 454-entry fabrication came through a lane that was also
# confident, also well-formed, and also sure of itself.
inject_tools_ungrounded() {
  edit_in_place 's|^    let kept = !report.kept().is_empty();$|    let kept = true;|' \
    diet/src/capture/tools.rs
}
# The reminder that never comes round. Every model eventually stops recording;
# a cadence that cannot fire turns the forget rate this lane exists to survive
# into a silence nobody counts.
inject_tools_reminder_silent() {
  edit_in_place 's|^        if self.since < self.cadence.interval() {$|        if true {|' \
    diet/src/capture/tools.rs
}
# A harness tool call accepted as a capture. Another system's tool output then
# becomes a fact about this session, written with capture authority, and the
# provenance says the model recorded it.
inject_tools_foreign_call() {
  edit_in_place 's|^        return Err(ToolError::NotACaptureTool(tool.clone()));$|        return Ok(Effect::default());|' \
    diet/src/capture/tools.rs
}
# A phase-transition proposal that writes. The tool was the most successful
# capture-adjacent mechanism the program ever ran, and it was that because it
# ASKED; a version that writes makes the model's wish for a phase change
# indistinguishable from a phase change.
inject_tools_proposal_writes() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/tools.rs")
source = path.read_text(encoding="utf-8")
old = """fn advisory(proposal: Proposal) -> Effect {
    Effect {
        proposal: Some(proposal),
        ..Effect::default()
    }
}"""
new = """fn advisory(proposal: Proposal) -> Effect {
    let patches = vec![Patch::Add {
        id: EntryId::new(&proposal.from_call).expect("a call id is not blank"),
        content: proposal.reason.clone(),
        provenance: Provenance {
            turn: proposal.at_turn,
            lane: LANE.to_owned(),
            fork: None,
            tangent: None,
            index: 0,
        },
    }];
    Effect {
        patches,
        proposal: Some(proposal),
        ..Effect::default()
    }
}"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A capture grounded in the harness's echo of itself. A harness that answers
# `update_record` with `recorded: <content>` is the ordinary shape, and reading
# that line as evidence lets a fabrication certify itself by being repeated
# back to the model that made it up.
inject_tools_self_echo() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/tools.rs")
source = path.read_text(encoding="utf-8")
old = """                Event::ToolCall {
                    at_turn,
                    tool,
                    output: Some(output),
                    ..
                } if CaptureTool::from_tag(tool).is_none() => seen.push(*at_turn, turn, output),"""
new = """                Event::ToolCall {
                    at_turn,
                    output: Some(output),
                    ..
                } => seen.push(*at_turn, turn, output),"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A capture grounded in a turn the model had not reached. The whole record is
# in front of the gate at replay time, so this one arm is what stops a turn-4
# entry being certified against turn-9's output -- evidence that did not exist
# when the model wrote.
inject_tools_future_output() {
  edit_in_place 's|^            Ordering::Greater => return,$|            Ordering::Greater => \&mut self.source,|' \
    diet/src/capture/tools.rs
}
# A superseding entry whose id is minted from a counter. The entry that
# replaces a live one is then unreachable from the row that carried it, and its
# provenance is a claim rather than a walk back to the record.
inject_tools_supersede_minted() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/tools.rs")
source = path.read_text(encoding="utf-8")
old = """            Some(voids) => Patch::Supersede {
                id,"""
new = """            Some(voids) => Patch::Supersede {
                id: EntryId::new(&format!("supersede/{}", id.as_str().len()))
                    .map_err(ToolError::BadEntry)?,"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A verdict that resolves somebody else's entry. The one patch a verdict alone
# is allowed to justify, pointed at an entry the model never named.
inject_tools_resolve_elsewhere() {
  edit_in_place 's|^            target: entry.clone(),$|            target: EntryId::new("somebody/else").map_err(ToolError::BadEntry)?,|' \
    diet/src/capture/tools.rs
}
# The asks, reworded to nothing. What this lane says out loud is its whole
# product, and a test that only asks whether two strings differ is happy with
# "a" and "b".
inject_tools_ask_words() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/tools.rs")
source = path.read_text(encoding="utf-8")
old = """            Self::Reminder => "Anything you meant to record?",
            Self::Sweep => "What did this turn establish that a later turn would need?","""
new = """            Self::Reminder => "a",
            Self::Sweep => "b","""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# An ask that drops its own question whenever it carries a deferral. The
# reminder then reads as the router's note alone, with nothing asked.
inject_tools_ask_question_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/tools.rs")
source = path.read_text(encoding="utf-8")
old = """            Some(about) => format!(
                "{} It went by without an answer: {about}.",
                self.kind.opening()
            ),"""
new = """            Some(about) => about.clone(),"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The recovery sweep asking as the cadence. `AskKind::Sweep` is then produced
# nowhere, and the enumeration over `ALL` certifies the enum rather than the
# caller that was supposed to use it.
inject_tools_sweep_kind() {
  edit_in_place 's|^                kind: AskKind::Sweep,$|                kind: AskKind::Reminder,|' \
    diet/src/capture/tools.rs
}
# The model's own spelling of a closed choice, kept. Case is then decided
# wherever somebody thought of it next: `EVIDENCE` mints a second id for one
# entry, or is refused outright as a field nothing knows.
inject_tools_choice_uncanonical() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/tools.rs")
source = path.read_text(encoding="utf-8")
old = """            choice.clone()"""
new = """            let _ = choice;
            text.clone()"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A phase proposal with no reason. The tool is a request for a ruling, and the
# why is the whole of what it carries into one.
inject_tools_proposal_reasonless() {
  edit_in_place 's|^            reason: text_argument(&args, "reason"),$|            reason: String::new(),|' \
    diet/src/capture/tools.rs
}
# A reminder that drops what the router put off. The deferral then reaches only
# the post-drive sweep, and the cadence half of the join with the router is
# dead while every test stays green.
inject_tools_reminder_deferral_dropped() {
  edit_in_place 's|^            about: self.deferred.get(&turn).cloned(),$|            about: None,|' \
    diet/src/capture/tools.rs
}
# Every tool description reduced to one character. These bytes are what a
# foreign harness registers verbatim, and they are the advisory framing the
# whole modality argument rests on.
inject_tools_description_thin() {
  python3 - <<'EOF'
import json
import pathlib

path = pathlib.Path("diet/src/capture/tools/contract.jsonl")
rows = [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
assert rows
for row in rows:
    row["description"] = "x"
path.write_text(
    "\n".join(json.dumps(row, separators=(",", ":")) for row in rows) + "\n",
    encoding="utf-8",
)
EOF
}
# The check that every offered tool is described, removed. A tool this lane
# offers and the file omits is then registered with nothing said about it.
inject_tools_contract_undescribed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/tools.rs")
source = path.read_text(encoding="utf-8")
old = """    for tool in CaptureTool::ALL {
        if !specs.iter().any(|spec| spec.tool == *tool) {
            return Err(ContractError::Undescribed(tool.tag()));
        }
    }
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# The check against one tool described twice, removed. Which of the two rows a
# harness registers is then decided by the order of the file.
inject_tools_contract_duplicate() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/tools.rs")
source = path.read_text(encoding="utf-8")
old = """        if specs.iter().any(|spec| spec.tool == tool) {
            return Err(ContractError::DuplicateTool(at));
        }
"""
assert old in source
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# A word in the contract's closed list that the lane cannot honour. The model
# is invited to say `abandoned`, the harness constrains its argument to it, and
# `apply` then refuses the answer it asked for.
inject_tools_verdict_list_open() {
  edit_in_place 's|"of":\["done","not_this","partial","superseded"\]|"of":["abandoned","done","not_this","partial","superseded"]|' \
    diet/src/capture/tools/contract.jsonl
}
# The depth limit on the record reader's other door. `objects` is a second
# entry into the same recursive descent, and the lane contract and every lane
# corpus expectation arrive through it.
inject_record_objects_undepthed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = """    let depth = nesting_depth(input);
    if depth > MAX_DEPTH {
        return Err(ParseError::TooDeep {
            depth,
            limit: MAX_DEPTH,
        });
    }
    let mut parsed = RecordParser::parse(Rule::document, input)"""
new = """    let mut parsed = RecordParser::parse(Rule::document, input)"""
assert old in source
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A turn that recorded in the end, swept anyway. The sweep then asks about a
# fact the model did record, which teaches that recording changes nothing.
inject_tools_silent_kept() {
  edit_in_place 's|^            self.silent.remove(&turn);$||' diet/src/capture/tools.rs
}
# The one corpus case that drives a tool other than `update_record`, cut back
# to `update_record` alone. The corpus then covers the tool whose calls its
# replay helper finds easiest to read, which is how two of the three tools this
# lane offers sat outside it in the first place.
inject_tools_corpus_one_tool() {
  python3 - <<'EOF'
import json
import pathlib

case = pathlib.Path("diet/capture/tools/corpus/a-verdict-and-a-proposal.jsonl")
lines = [line for line in case.read_text(encoding="utf-8").splitlines() if line.strip()]
kept = [line for line in lines if '"tool":"resolve_entry"' not in line
        and '"tool":"propose_phase_transition"' not in line]
assert len(kept) < len(lines)
case.write_text("\n".join(kept) + "\n", encoding="utf-8")

expected = pathlib.Path("diet/capture/tools/corpus/a-verdict-and-a-proposal.expected.json")
row = json.loads(expected.read_text(encoding="utf-8"))
row["captures"] = [c for c in row["captures"] if c["call"] == "c1"]
expected.write_text(json.dumps(row, separators=(",", ":")) + "\n", encoding="utf-8")
EOF
}
# The control arm dropped from the power set. An ablation whose control is
# missing can rank its clauses against each other and cannot say that any of
# them beats an ask with no imperative in it -- which is the one thing the
# original result it is checking itself against established.
inject_ablation_no_control() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = "    (0..count).map(Arm).collect()\n"
new = "    (1..count).map(Arm).collect()\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Silence counted as engagement. The collapse case is an answer of nothing at
# all, and a grader that reads it as engagement reports the worst outcome a
# wording can buy as the best one.
inject_ablation_silence_engages() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = "    matches!(grade(answer), Grade::Engaged)\n"
new = "    matches!(grade(answer), Grade::Engaged | Grade::Silent)\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A p-value reported without its attainable floor. A sample size bounds the
# smallest p it can produce; a p quoted at that bound with the bound stripped
# off reads as a strength the run never had.
inject_ablation_p_floor_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = """            "{}/{} against {}/{}: p {}, attainable floor {} over {} resamples",
            self.successes_a,
            self.pairs,
            self.successes_b,
            self.pairs,
            self.p_value().fixed(P_DIGITS),
            self.attainable_p_floor().fixed(P_DIGITS),
            self.resamples,
"""
new = """            "{}/{} against {}/{}: p {} over {} resamples",
            self.successes_a,
            self.pairs,
            self.successes_b,
            self.pairs,
            self.p_value().fixed(P_DIGITS),
            self.resamples,
"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A resample that draws once instead of once for every fork. The report still
# carries a p and still carries its floor, and the p is now a statement about a
# sample of one. This is the failure a bootstrap actually has: not summing
# nothing, which shows up at once, but resampling wrongly.
inject_ablation_resample_single_draw() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = "        for _ in 0..pairs {\n"
new = "        for _ in 0..1 {\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The generator frozen: every draw returns the same index, so a resample is one
# fork counted over and over. Deterministic, reproducible, and not a sample.
inject_ablation_frozen_generator() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = """        self.0 = state;
        state
    }
"""
new = """        let _ = state;
        self.0
    }
"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Each arm credited with the other arm's outcomes. The p is untouched, so
# nothing downstream of the counts can notice: the reported line states a real
# p about a comparison that was never made.
inject_ablation_rates_swapped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = """        successes_a: count(a),
        successes_b: count(b),
"""
new = """        successes_a: count(b),
        successes_b: count(a),
"""
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The silence endpoint dropped from the pre-registration. What is left is the
# single endpoint of the experiment this one exists to improve on, and a
# wording that buys engagement with occasional silence reads as a win.
inject_ablation_endpoint_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = "    pub const ALL: &'static [Self] = &[Self::Engagement, Self::Silence];"
new = "    pub const ALL: &'static [Self] = &[Self::Engagement];"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# Every arm of the plan printed with the control's imperative. The plan is the
# one artefact this instrument produces before a run, and it would name eight
# arms that all say the same thing.
inject_ablation_plan_one_imperative() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = "                .map(|arm| (arm, self.render(arm)))\n"
new = "                .map(|arm| (arm, self.render(Arm::CONTROL)))\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The control reported under a clause's name. Two arms then answer to one name
# in the results table, and one of the two is the arm the design rests on.
inject_ablation_arm_names_collide() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = 'pub const CONTROL_TAG: &str = "none";'
new = 'pub const CONTROL_TAG: &str = "scope";'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The clause separator removed, so an arm's clauses run together. The sentence
# handed to every fork of a run is the one variable this experiment
# manipulates, and it would go out malformed.
inject_ablation_clauses_run_together() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = """            if !out.is_empty() {
                out.push(' ');
            }
"""
assert source.count(old) == 1
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# A row taken out of the placeholder table. A form handed back with TODO where
# an answer goes then counts as engagement, which is the endpoint counting a
# blank as a win.
inject_ablation_placeholder_word_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = 'const PLACEHOLDER_WORDS: &[&str] = &["tbd", "todo", "...", "\\u{2026}"];'
new = 'const PLACEHOLDER_WORDS: &[&str] = &["tbd", "...", "\\u{2026}"];'
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# A blank clause text admitted. The arm carrying that clause is then the
# control wearing another name, and the ablation compares two arms that are
# one arm.
inject_ablation_blank_clause_allowed() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = """            if text.trim().is_empty() {
                return Err(ClauseError::BlankText(clause));
            }
"""
assert source.count(old) == 1
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# A case removed from the grading corpus, its expectation left behind. A
# corpus walked in one direction only reports a corpus somebody deleted a case
# out of as a corpus that passed.
inject_ablation_corpus_case_dropped() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path(
    "diet/capture/ablation/corpus/the-form-handed-back-with-its-blanks.answer.txt"
)
assert path.is_file()
path.unlink()
EOF
}
# An untagged decline read as content. The same words then grade two ways
# depending on whether a tag was written over them, and the arm most likely to
# draw a reply with no tag on it is the brevity clause under test.
inject_ablation_untagged_decline_engages() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/capture/ablation.rs")
source = path.read_text(encoding="utf-8")
old = "        Outcome::Unparseable => decline::classify(field.raw.trim()).is_decline(),\n"
new = "        Outcome::Unparseable => false,\n"
assert source.count(old) == 1
path.write_text(source.replace(old, new, 1), encoding="utf-8")
EOF
}
# The second reader of the record grammar left unbounded. One reader then
# returns a verdict on deeply nested text and the other hands it the stack,
# and which of the two a caller reaches depends on which module it imported.
inject_json_objects_unbounded() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/json.rs")
source = path.read_text(encoding="utf-8")
old = """    if let Some(depth) = too_deep(text) {
        return Err(LineError::TooDeep {
            depth,
            limit: MAX_DEPTH,
        });
    }
"""
assert source.count(old) == 1
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# A lane free to change substrate mid-run. Then a `rejected` row, which
# carries a lane and no substrate, has two answers to inherit from -- and the
# one that answers is whichever the lookup happens to find first.
inject_record_lane_may_change_substrate() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = '            if let Some(was) = lanes.insert(lane.as_str(), id.as_str())\n                && was != id\n'
new = '            if let Some(was) = lanes.insert(lane.as_str(), id.as_str())\n                && false\n                && was != id\n'
if source.count(old) != 1:
    raise SystemExit(f"the lane check appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}
inject_record_substrate_reference_inferred_from_a_count() {
  python3 - <<'EOF'
import pathlib

# THE REFERENCE, RESOLVED BY COUNTING. Enforcing it only when the run declares
# more than one substrate is the plausible edit -- with one declared there is
# nothing to choose between, so the check looks like a formality. It is not:
# the field exists so that a row's substrate is never inferred from how many
# there are, and this is the shape that inference takes.
path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = '            if let Some(known) = regime.as_ref()\n                && !known.declares(id)\n'
new = (
    '            if let Some(known) = regime.as_ref()\n'
    '                && known.substrate_ids().len() > 1\n'
    '                && !known.declares(id)\n'
)
if source.count(old) != 1:
    raise SystemExit(f"the declares check appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}
inject_record_a_fork_names_no_substrate() {
  python3 - <<'EOF'
import pathlib

# THE FORK ARM, DROPPED. A fork is how a run reaches a second substrate, so it
# is the row most able to name one nothing declared -- and both substrate
# rules read request and fork rows through the same match. Removing the fork
# arm leaves every request-shaped fixture passing.
path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
old = '            Event::Fork {\n                lane, substrate, ..\n            } => Some(("fork", lane, substrate)),\n'
if source.count(old) != 1:
    raise SystemExit(f"the fork arm appears {source.count(old)} times")
path.write_text(source.replace(old, "", 1), encoding="utf-8")
EOF
}
# Weights identified by whatever string is there. A name is prose: two runs
# can spell the same weights differently and a third can spell different
# weights the same, and then a regime comparison compares strings.
# A canned substrate that may decline to identify itself. The acts are the
# whole identity a server with no weights has, so a `canned` kind that does
# not have to carry them is the null the typed identity was adopted to remove,
# wearing the new kind's name.
inject_record_canned_acts_optional() {
  python3 - <<'EOF'
import pathlib

path = pathlib.Path("diet/src/formats/record/mod.rs")
source = path.read_text(encoding="utf-8")
# BOTH halves, or the injection is inert. Making only the read optional leaves
# the empty string failing `digest_ok`, so the fixture is still refused -- with
# a different message and the same verdict, which the selftest grades GREEN
# because the gate never stopped firing. It did, on the first attempt.
old = """            let text = take_string(&mut members, of, "acts_sha256")?;
            if !digest_ok(&text) {
                return Err(StructureError::BadDigest(text).into());
            }
            Weights::Canned { acts_sha256: text }"""
new = """            Weights::Canned {
                acts_sha256: take_string(&mut members, of, "acts_sha256").unwrap_or_default(),
            }"""
if source.count(old) != 1:
    raise SystemExit(f"the canned acts block appears {source.count(old)} times")
path.write_text(source.replace(old, new), encoding="utf-8")
EOF
}

# Every pattern in a table, shown catching its own class. A pattern that has
# never caught anything is a guess.
#
# Iterates the TABLE rather than the seed directory, so a pattern with no
# seeded class is reported as unproven instead of quietly skipped. The trailing
# `|| [ -n "$label" ]` catches a table whose final line has no newline.
prove_patterns() {
  local kind="$1" table="$2" seeder="$3"; shift 3
  local required=("$@")
  local seed dir label flags regex out rc
  local -a defined=()

  echo
  echo "--- ${kind} patterns, each proven against its own class ---"
  scratch; seed="$SCRATCH"
  bash "${ROOT}/${seeder}" "$seed" > /dev/null

  local escaped=false
  if find "$seed" -name '*.jsonl' -print -quit | grep -q .; then escaped=true; fi

  while IFS=$'\t' read -r label flags regex || [ -n "${label:-}" ]; do
    case "$label" in ''|\#*) continue ;; esac
    [ -n "${regex:-}" ] || continue
    # Before the shard: whether the brief's required classes are in the table
    # is a question about the table, not about this job's share of it.
    defined+=("$label")
    # `${kind}.${label}` is the id check-fault-manifest.py registers this class
    # under, read there out of REQUIRED_HYGIENE_CLASSES/REQUIRED_PAGES_CLASSES
    # and here out of the table those pin.
    in_shard "${kind}.${label}" || continue
    now_ms; local started_ms="$NOW_MS"

    dir="${seed}/${label}"
    if [ ! -d "$dir" ]; then
      printf 'UNSEEDED %s pattern %s  <-- NO CLASS TO PROVE IT AGAINST\n' "$kind" "$label"
      SELFTEST_BROKEN+=("${kind} pattern ${label} has no seeded class")
      shard_cost "${kind}.${label}" "$started_ms" "${kind} pattern ${label}"
      continue
    fi

    # A corpus that carries escaped twins requires one for EVERY class, and
    # requires each class to fire on it. A class that guards prose and not
    # logs is a guard over the half of the surface where the artefacts are
    # not. Derived from the corpus rather than passed in: a table whose seeder
    # stops writing twins loses the requirement, and a table that never had
    # them (the published surface is HTML and CSS, never a JSON string) is not
    # asked for one.
    #
    # A CORRECTION. This comment used to add "and the twin reaches the pattern
    # only through the decoded view, so this is also what keeps that view
    # load-bearing". IT DID NOT. `json.dumps` puts the escaped newline at the
    # END of the line, so nothing is welded to the forbidden string's left and
    # the raw bytes match it anyway -- measured, with the mirror removed, 13
    # of 14 classes still fired on their twin. The twin proved the pattern and
    # said nothing about the view.
    #
    # The WELDED file is what does that job, and it is required too: the
    # fragment alone, immediately after an escaped newline, which is the shape
    # the decoder exists for. With the mirror removed it silences the four
    # classes whose match may begin with an alphanumeric; the other ten are
    # anchored on a character that is never a token character (`/home/`,
    # `-----BEGIN `, `C:\`, `sk-ant-`) and cannot be welded shut at all. See
    # seed-hygiene-fault.sh for the measurement and the list.
    if [ "$escaped" = true ]; then
      local missing=""
      [ -f "${dir}/${label}.jsonl" ] || missing="escaped twin"
      [ -f "${dir}/${label}.welded.jsonl" ] || missing="${missing:+${missing} and }welded twin"
      if [ -n "$missing" ]; then
        printf 'UNSEEDED %s pattern %s  <-- NO %s TO PROVE IT AGAINST\n' \
          "$kind" "$label" "$(printf '%s' "$missing" | tr '[:lower:]' '[:upper:]')"
        SELFTEST_BROKEN+=("${kind} pattern ${label} has no ${missing}")
        shard_cost "${kind}.${label}" "$started_ms" "${kind} pattern ${label}"
        continue
      fi
    fi

    rc=0
    out="$(bash "${ROOT}/scripts/hygiene.sh" --patterns "${ROOT}/${table}" --tree "$dir" 2>&1)" || rc=$?
    local plain=false twin=false welded=false
    # The PROSE form is "a hit in neither twin", not "a hit in `${label}.txt`":
    # the pages corpus writes `.html`, `.css` and `.js`, and naming one
    # extension made every one of those classes report prose=false. Caught by
    # this suite on the first run after the welded form was added, which is
    # the corpus doing its job -- but the fix is to exclude both twins by
    # name, since with three files "not the plain twin" is no longer enough.
    printf '%s\n' "$out" | grep "hygiene: ${label}:" \
      | grep -v "${label}\.welded\.jsonl" \
      | grep -qv "${label}\.jsonl" && plain=true
    if [ "$escaped" = true ]; then
      # `${label}.jsonl` matches only the plain twin: the welded file is
      # `${label}.welded.jsonl`, which does not contain that string.
      printf '%s\n' "$out" | grep -q "hygiene: ${label}:.*${label}\.jsonl" && twin=true
      printf '%s\n' "$out" | grep -q "hygiene: ${label}:.*${label}\.welded\.jsonl" && welded=true
    else
      twin=true
      welded=true
    fi

    if [ "$rc" -eq 1 ] && [ "$plain" = true ] && [ "$twin" = true ] \
       && [ "$welded" = true ]; then
      printf 'RED   hygiene.sh exit %-3d  %s\n' "$rc" "$label"
    elif [ "$rc" -eq 1 ] \
         && { [ "$plain" = true ] || [ "$twin" = true ] || [ "$welded" = true ]; }; then
      printf 'GREEN hygiene.sh exit %-3d  %s  <-- FIRED ON %s%s%s, NOT ALL THREE\n' \
        "$rc" "$label" \
        "$([ "$plain" = true ] && printf 'prose ' || true)" \
        "$([ "$twin" = true ] && printf 'twin ' || true)" \
        "$([ "$welded" = true ] && printf 'welded' || true)"
      SELFTEST_BROKEN+=("${kind} pattern ${label}: not every form")
    else
      printf 'GREEN hygiene.sh exit %-3d  %s  <-- PATTERN DID NOT FIRE\n' "$rc" "$label"
      SELFTEST_BROKEN+=("${kind} pattern ${label}")
    fi
    shard_cost "${kind}.${label}" "$started_ms" "${kind} pattern ${label}"
  done < "${ROOT}/${table}"

  local want found
  for want in ${required+"${required[@]}"}; do
    found=false
    for label in ${defined+"${defined[@]}"}; do
      [ "$label" = "$want" ] && { found=true; break; }
    done
    if [ "$found" = false ]; then
      printf 'MISSING %s pattern %s  <-- A CLASS THE BRIEF NAMES IS NOT IN THE TABLE\n' "$kind" "$want"
      SELFTEST_BROKEN+=("${kind} table has no ${want} pattern")
    fi
  done
}

# Assert that a command exits exactly WANT. The scanners have behaviour no
# seeded gate-fault reaches -- how they classify files, which files they
# exempt, how they read their own tables -- and every one of those was
# reverted-and-still-green before this existed.
expect_exit() {
  local label="$1" want="$2"; shift 2
  local rc=0
  "$@" > /dev/null 2>&1 || rc=$?
  if [ "$rc" -eq "$want" ]; then
    printf 'OK    exit %-3d  %s\n' "$rc" "$label"
  else
    printf 'BAD   exit %-3d (wanted %d)  %s\n' "$rc" "$want" "$label"
    SELFTEST_BROKEN+=("mechanics: ${label}")
  fi
}

prove_mechanics() {
  echo
  echo "--- scanner mechanics ---"
  local box; scratch; box="$SCRATCH"

  # A credential inside a file grep calls binary must still be found.
  mkdir -p "${box}/bin-secret"
  printf 'x\000GH=%s%s\000\n' 'ghp_' '0123456789abcdefghijklmnopqrstuvwxyz' \
    > "${box}/bin-secret/blob.bin"
  expect_exit "a credential inside a binary is caught" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/bin-secret"

  # ...but an ordinary binary must not trip the loose heuristics. Over-strict
  # is a failure too: a gate that cries wolf on every binary gets switched off.
  mkdir -p "${box}/bin-clean"
  cp "$(command -v git)" "${box}/bin-clean/git.bin"
  head -c 65536 /dev/urandom > "${box}/bin-clean/random.bin"
  expect_exit "an ordinary binary does not false-positive" 0 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/bin-clean"

  # The pattern-table exemption is scoped to scripts/. Any other file that
  # happens to be named that way is still scanned.
  mkdir -p "${box}/fake-table/docs"
  printf 'see %s%s\n' 'DIE' '-9001' > "${box}/fake-table/docs/notes-patterns.tsv"
  expect_exit "a *-patterns.tsv outside scripts/ is still scanned" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/fake-table"

  # A table whose final line has no newline must not lose its last pattern.
  mkdir -p "${box}/last-line"
  printf 'see %s%s\n' 'DIE' '-1' > "${box}/last-line/hit.txt"
  printf '# only pattern, no trailing newline\nlast-pattern\t-\tDIE-[0-9]+' \
    > "${box}/unterminated-patterns.tsv"
  expect_exit "the last pattern in an unterminated table still fires" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --patterns "${box}/unterminated-patterns.tsv" \
      --tree "${box}/last-line"

  # --- content as a reader would see it -------------------------------------
  #
  # An address that exists ONLY inside a JSON string, immediately after an
  # escaped newline. `private-ipv4` requires a non-alphanumeric to its left;
  # in the bytes on disk the character to its left is the `n` of `\n`, so the
  # raw scan reads the escaping and calls the content clean. This is how a
  # private path reached a replay log unflagged.
  mkdir -p "${box}/escaped"
  printf '{"stdout":"up\\n%s%s ok\\n"}\n' '192.168' '.4.9' \
    > "${box}/escaped/run.jsonl"
  expect_exit "an address only a decoded view can see is caught" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/escaped"
  # The control. Without it the assertion above passes on a file whose bytes
  # match anyway, and proves nothing about the decoding.
  expect_exit "the same address, unread, is not a hit" 1 \
    grep -qE "(^|[^0-9A-Za-z.-])192\\.168\\.[0-9]{1,3}\\.[0-9]{1,3}" \
      "${box}/escaped/run.jsonl"

  # THE WELDED CASE, which is the one the boundary misses rather than the one
  # the escaping hides. `\n` puts a literal `n` immediately left of the token,
  # so `(^|[^A-Za-z0-9])` does not match there -- the raw bytes are searched
  # and found clean while the content is not. This is the defect the data seat
  # reported: a pattern that guards prose does not guard logs.
  mkdir -p "${box}/welded"
  printf '{"stdout":"ran\\n%s%s regressed\\n"}\n' 'die' '45' \
    > "${box}/welded/run.jsonl"
  expect_exit "an escape-welded token is caught through the decoding" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/welded"
  # ...and the control that makes the assertion above mean something: the same
  # bytes, matched directly, are NOT a hit. If this ever exits 0 the case
  # above has stopped testing the decoding and started testing the pattern.
  expect_exit "the same bytes, unread, are not a hit" 1 \
    grep -qiE "(^|[^A-Za-z0-9])DIE-?[0-9]+" "${box}/welded/run.jsonl"

  # THE SAME WELD, in a file carrying one byte that is not UTF-8. The decoder
  # skipped the WHOLE FILE on UnicodeDecodeError, so one cp1252 quote anywhere
  # in a log meant no decoded view for ANY of it -- the weld above went unseen
  # and the gate printed `clean`. Six such files are already tracked here, one
  # of them a JSONL record fixture, so the combination is not exotic.
  #
  # `errors="replace"` keeps the view. U+FFFD is not alphanumeric, so it
  # separates like any other non-token byte: it can split a token that spanned
  # the bad byte, and it cannot invent one that was not there.
  mkdir -p "${box}/welded-undecodable"
  printf '{"stdout":"ran\\n%s%s regressed\\n"}\n' 'die' '45' \
    > "${box}/welded-undecodable/run.jsonl"
  printf '{"note":"caf\xe9"}\n' >> "${box}/welded-undecodable/run.jsonl"
  expect_exit "a weld survives one byte that is not UTF-8" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/welded-undecodable"
  # Two controls, because this assertion has two ways to pass for the wrong
  # reason. The file must really be undecodable...
  expect_exit "and that file really is undecodable as UTF-8" 1 \
    python3 -c 'import sys; open(sys.argv[1], encoding="utf-8").read()' \
      "${box}/welded-undecodable/run.jsonl"
  # ...and its raw bytes must really be clean, so the hit came from the view.
  expect_exit "and its raw bytes, unread, are not a hit" 1 \
    grep -qiE "(^|[^A-Za-z0-9])DIE-?[0-9]+" "${box}/welded-undecodable/run.jsonl"

  # A pattern beginning with `-`. grep would read it as an OPTION and report
  # every row after it absent -- a whole class silently unguarded, printing
  # the same green. The scanner passes each regex with `-e`.
  mkdir -p "${box}/dash-lead"
  printf 'value = %s%s\n' '-forbid' 'den-shape' > "${box}/dash-lead/hit.txt"
  printf 'dash-leading\t-\t-forbidden-shape\n' > "${box}/dash-patterns.tsv"
  expect_exit "a pattern beginning with a dash is a pattern, not an option" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --patterns "${box}/dash-patterns.tsv" \
      --tree "${box}/dash-lead"

  # --- literals that have no shape ------------------------------------------
  #
  # A digest row cannot be seeded the way a pattern row is: assembling the
  # literal from fragments would still BE the literal, in a public repository,
  # which is what the row exists to avoid. A DECOY token and a scratch table
  # prove the mechanism end to end without the real value ever being written.
  local decoy table
  decoy="$(printf '%s%s%s' 'zz' 'subject' 'zz')"
  table="${box}/decoy-hashes.txt"
  printf '# Salt: discipline-hygiene-v1\n' > "$table"
  python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
    --emit "$decoy" decoy-username >> "$table"

  mkdir -p "${box}/decoy-hit"
  printf 'drwx 3 %s staff\n' "$decoy" > "${box}/decoy-hit/ls.txt"
  expect_exit "a digest row catches its literal" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" --tree "${box}/decoy-hit"

  # The same token welded to an escape. A regex can be given a looser
  # boundary to reach through one; a digest cannot be given a looser hash, so
  # this half needs the decoding more than the other half does.
  mkdir -p "${box}/decoy-welded"
  printf '{"stdout":"total 4\\n%s ok\\n"}\n' "$decoy" \
    > "${box}/decoy-welded/run.jsonl"
  expect_exit "a digest row catches an escape-welded literal" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" --tree "${box}/decoy-welded"

  # A SUBSTRING IS NOT THE TOKEN. Without this the row would fire on every
  # word that contains the literal, and a gate that cries wolf gets switched
  # off -- the same failure mode as an over-loose pattern, arrived at by
  # hashing instead of by matching.
  mkdir -p "${box}/decoy-miss"
  printf '%sx and x%s and someone-else\n' "$decoy" "$decoy" \
    > "${box}/decoy-miss/near.txt"
  expect_exit "a substring of the literal is not the literal" 0 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" --tree "${box}/decoy-miss"

  # THE REPORT NAMES THE LABEL AND NEVER THE TOKEN. A scanner that printed
  # what it found would publish the value on every CI log that caught one.
  expect_exit "a hit names its label, not what it matched" 0 \
    bash -c "out=\$(python3 '${ROOT}/scripts/check-hashes.py' --table '$table' \
      --tree '${box}/decoy-hit' 2>&1; true) \
      && grep -q 'decoy-username' <<<\"\$out\" \
      && ! grep -q '$decoy' <<<\"\$out\""

  # THE WIRING, not just the scanner. Every assertion above runs
  # `check-hashes.py` directly, so all of them would still pass with the call
  # removed from `hygiene.sh` -- and then `--only hygiene` would be a pattern
  # scan wearing a digest scan's green.
  expect_exit "the hygiene gate runs the digest half" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --hashes "$table" --tree "${box}/decoy-hit"

  # A table with no salt cannot be computed against, and a table with no rows
  # checks nothing. Neither is a pass.
  printf 'c3cfd25a52f47a385452ff4098ace911eb0c4b44e7eebfc39c29fb38dcee408a  x\n' \
    > "${box}/no-salt.txt"
  expect_exit "a digest table naming no salt is refused" 2 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "${box}/no-salt.txt" \
      --tree "${box}/decoy-hit"
  printf '# Salt: discipline-hygiene-v1\n' > "${box}/no-rows.txt"
  expect_exit "a digest table with no rows is not a pass" 2 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "${box}/no-rows.txt" \
      --tree "${box}/decoy-hit"

  # ONE BYTE THAT IS NOT UTF-8 USED TO DROP THE WHOLE FILE. The scan caught a
  # `UnicodeDecodeError`, skipped the file, and justified it by saying the
  # pattern half covered it -- which is the one thing that cannot be true
  # here, because patterns reach strings that have a shape and this half is
  # for the strings that do not. The literal below sits in plain ASCII; only
  # the curly quote beside it is undecodable.
  mkdir -p "${box}/not-utf8"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='cp1252').write('owned by ' + sys.argv[2] + ', it\u2019s theirs\n')
" "${box}/not-utf8/notes.txt" "$decoy"
  expect_exit "a literal in a file with one undecodable byte is still found" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/not-utf8"

  # And the control, so the assertion above is about the ENCODING and not
  # about the literal being present: the same undecodable byte, no literal.
  mkdir -p "${box}/not-utf8-clean"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='cp1252').write('nothing here, it\u2019s fine\n')
" "${box}/not-utf8-clean/notes.txt"
  expect_exit "an undecodable byte alone is not a hit" 0 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/not-utf8-clean"

  # A CHARACTER WITH NO VISUAL WIDTH IS NOT A SEPARATOR. A zero-width space
  # inside the literal is invisible in review and invisible in `git diff`, and
  # a tokeniser that split on it handed back two tokens that hash to nothing
  # and reported the file clean. Ruled 2026-09-11 as IN SCOPE: this gate
  # guards against mistakes and against a careless commit later tidied, and an
  # invisible byte a tidy-up left behind is exactly that -- the one evasion
  # that survives a human reading the diff.
  mkdir -p "${box}/zero-width"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write(
    'drwx 3 ' + sys.argv[2][:5] + '​' + sys.argv[2][5:] + ' staff\n')
" "${box}/zero-width/ls.txt" "$decoy"
  expect_exit "a literal split by a zero-width space is still found" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/zero-width"

  # The control, so the assertion above is about the CHARACTER and not about
  # the literal being present: the same zero-width space, no literal. Without
  # it, "dropped the character" and "found the literal" are confounded and a
  # scanner that fired on everything would satisfy the pair.
  mkdir -p "${box}/zero-width-clean"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write('nothing​ here at all\n')
" "${box}/zero-width-clean/notes.txt"
  expect_exit "a zero-width space alone is not a hit" 0 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/zero-width-clean"

  # The other half of the rule, and it is a DIFFERENT category: a combining
  # accent is `Mn` where the zero-width space is `Cf`. Both are dropped, and
  # asserting only one leaves the other a branch nothing reaches -- which is
  # how a guard ends up unable to fire.
  mkdir -p "${box}/combining"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write(
    'drwx 3 ' + sys.argv[2][:5] + '́' + sys.argv[2][5:] + ' staff\n')
" "${box}/combining/ls.txt" "$decoy"
  expect_exit "a literal split by a combining mark is still found" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/combining"

  # THE SIBLING ONE STEP SIDEWAYS, and the reason the strip is a PROPERTY now
  # rather than a list of categories. `U+3164 HANGUL FILLER` is category `Lo`
  # and `isalnum()` is TRUE for it, so it does not split the token -- it welds
  # INTO it, and no set of categories could ever have reached it while the
  # docstring promised "a character of no visual width is still caught"
  # without qualification. Ruled 2026-09-12: strip Unicode
  # `Default_Ignorable_Code_Point`, which names exactly the code points that
  # render as nothing, the four alphanumeric fillers among them.
  mkdir -p "${box}/filler"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write(
    'drwx 3 ' + sys.argv[2][:5] + '\u3164' + sys.argv[2][5:] + ' staff\n')
" "${box}/filler/ls.txt" "$decoy"
  expect_exit "a literal welded by a hangul filler is still found" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/filler"

  # Its control, for the same reason the other two have one.
  mkdir -p "${box}/filler-clean"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write('nothing\u3164 here at all\n')
" "${box}/filler-clean/notes.txt"
  expect_exit "a hangul filler alone is not a hit" 0 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/filler-clean"

  # THE DECLARED LIMIT, asserted rather than only written down. A visible
  # lookalike -- Cyrillic `e` where a Latin one belongs -- is NOT caught, and
  # that is the line the ruling drew: invisible to a reader is in scope, a
  # confusable is not. An undeclared non-catch is the vacuous class; this one
  # is declared, and pinned, so a later change that quietly starts folding
  # confusables has to come here and say so.
  mkdir -p "${box}/confusable"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write(
    'drwx 3 ' + sys.argv[2].replace('e', '\u0435', 1) + ' staff\n')
" "${box}/confusable/ls.txt" "$decoy"
  expect_exit "a visible lookalike is the declared limit, not a hit" 0 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/confusable"

  # ...and the control that stops the row above from passing because the decoy
  # has no `o` in it to replace. Same file, same shape, the Latin letter left
  # alone: that one must fire.
  mkdir -p "${box}/confusable-control"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write(
    'drwx 3 ' + sys.argv[2] + ' staff\n')
" "${box}/confusable-control/ls.txt" "$decoy"
  expect_exit "and the same line with the Latin letter is a hit" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/confusable-control"

  # NO TABLE IS NO SCAN. The strip reads `default-ignorable.tsv` beside the
  # decoder, and without it the tokeniser does not know which characters are
  # invisible -- which makes it exactly as blind as the defect it exists to
  # catch. Exit 2, like a missing salt and a missing decoder, because `clean`
  # from a blind scan is the vacuous class.
  mkdir -p "${box}/tableless"
  cp "${ROOT}/scripts/check-hashes.py" "${ROOT}/scripts/decoding.py" "${box}/tableless/"
  expect_exit "a decoder with no invisible-character table is broken, not clean" 2 \
    python3 "${box}/tableless/check-hashes.py" --table "$table" \
      --tree "${box}/decoy-hit"
  # The control: the same copied pair, WITH the table, finds the same hit the
  # real scanner does. Without it the row above passes on any copy that fails
  # for any reason at all.
  cp "${ROOT}/scripts/default-ignorable.tsv" "${box}/tableless/"
  expect_exit "and the same copy, with the table beside it, finds the literal" 1 \
    python3 "${box}/tableless/check-hashes.py" --table "$table" \
      --tree "${box}/decoy-hit"

  # ESCAPED TWICE IS STILL ESCAPED. Parsing JSON spends one level, so content
  # a tool logged from another tool's JSON output arrives with `\` and `n`
  # still welded to the front of a token one layer down. The decoder runs to a
  # fixed point and spends the escapes at each layer; before it did, this file
  # was reported clean.
  mkdir -p "${box}/twice-wrapped"
  python3 -c "
import json, sys
inner = json.dumps({'out': 'ls -l\\n' + sys.argv[2] + ' staff 42'})
open(sys.argv[1], 'w').write(json.dumps({'log': inner}) + '\n')
" "${box}/twice-wrapped/nested.jsonl" "$decoy"
  expect_exit "a literal wrapped in JSON twice is still found" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/twice-wrapped"

  # THREE GUARDS THE FIXTURE ABOVE DOES NOT REACH, each one measured by
  # neutering the code it names and watching the whole suite stay green.
  # Its input is nested JSON, so "parsed another layer" and "spent the
  # escapes" are confounded in the one case that touches both -- and the two
  # halves of the decoder that only one of those exercises had no fixture at
  # all. A fixture that proves nothing the first one didn't is not a fixture;
  # a guard nothing reaches is worse than no guard.

  # 1. THE ESCAPE-SPENDING HALF, ALONE. A JSON string whose PARSED value still
  # holds a literal backslash-n -- two backslashes in the file -- welded to
  # the token. There is no second JSON layer here, so parsing cannot recover
  # it and only `_unescaped` can. Measured with `_unescaped` made the identity
  # function: the view loses its second half and this file reads clean.
  mkdir -p "${box}/escape-only"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write(
    '{\"stdout\":\"ran' + chr(92) + chr(92) + 'n' + sys.argv[2] + ' regressed\"}' + chr(10))
" "${box}/escape-only/run.jsonl" "$decoy"
  expect_exit "a token welded one layer under the parse is still found" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/escape-only"
  # Its control, and it is about TOKENS rather than about the substring: the
  # decoy is right there in the bytes -- what is missing is a separator to its
  # left, because the character before it is the `n` of an escaped newline.
  # A boundary-aware search of the raw file finds nothing, which is exactly
  # the silence the decoded view exists to break.
  expect_exit "and the raw bytes hold no such token" 1 \
    grep -qE "(^|[^A-Za-z0-9])${decoy}([^A-Za-z0-9]|$)" \
      "${box}/escape-only/run.jsonl"

  # 2. MORE THAN ONE PASS. `MAX_PASSES` was reachable by nothing: the decoy is
  # recovered at one pass in every nested case, because spending the escapes
  # collapses arbitrary depth in a single go. A UNICODE escape is the case it
  # cannot: `_unescaped` spends `\n`, `\t` and `\r` and not `\u0009`, so
  # only a SECOND JSON PARSE separates the token. Measured: one pass reads
  # this clean, two find it.
  mkdir -p "${box}/two-passes"
  python3 -c "
import json, sys
inner = '{\"stdout\":\"ran' + chr(92) + 'u0009' + sys.argv[2] + ' x\"}'
open(sys.argv[1], 'w', encoding='utf-8').write(json.dumps({'log': inner}) + chr(10))
" "${box}/two-passes/nested.jsonl" "$decoy"
  expect_exit "a token only a second parse can separate is still found" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/two-passes"

  # 3. SPLIT ON EVERY NON-ALPHANUMERIC. The tokeniser's central rule, and the
  # reason this function exists rather than a regex at each call site: the
  # obvious class keeps `owner.example.net` whole, so a digest of the bare
  # name never matches it. The decoy carries no dot, so every fixture above
  # passes with the rule reverted. This one does not.
  mkdir -p "${box}/dotted"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write(
    'proxy: ' + sys.argv[2] + '.example.net:8080' + chr(10))
" "${box}/dotted/hosts.txt" "$decoy"
  expect_exit "a literal joined by dots to more text is still found" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/dotted"
  # Its control, and the one that says the row above is about the SPLITTING:
  # the same file with the decoy standing alone must fire too, so a failure
  # here is never "the decoy stopped being in the table".
  mkdir -p "${box}/dotted-control"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write('proxy: ' + sys.argv[2] + chr(10))
" "${box}/dotted-control/hosts.txt" "$decoy"
  expect_exit "and the same decoy standing alone is a hit" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/dotted-control"

  # 4. SOMETHING IN FRONT OF THE JSON. Not the token -- the WHOLE VIEW. The
  # decoder required the first character of a line to open a JSON value, so
  # one byte of preamble meant no decoded view for the file at all, and the
  # welded token went unseen while the gate printed clean. Both forms are
  # accidents rather than evasions, which is this gate's stated threat model:
  # PowerShell's `Out-File` writes a BOM by default, and every timestamped
  # log line ever written has a prefix.
  mkdir -p "${box}/bom" "${box}/log-prefix"
  python3 -c "
import sys
d = sys.argv[3]
open(sys.argv[1], 'w', encoding='utf-8').write(
    '\ufeff{\"stdout\":\"ran' + chr(92) + 'n' + d + ' x\"}' + chr(10))
open(sys.argv[2], 'w', encoding='utf-8').write(
    '2026-01-01T00:00:00Z INFO {\"stdout\":\"ran' + chr(92) + 'n' + d + ' x\"} (ok)' + chr(10))
" "${box}/bom/run.jsonl" "${box}/log-prefix/run.jsonl" "$decoy"
  expect_exit "a byte-order mark in front of the JSON does not blind the view" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" --tree "${box}/bom"
  expect_exit "a log prefix in front of the JSON does not blind the view" 1 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/log-prefix"
  # The control both rows need: neither file holds the decoy as a token in its
  # raw bytes, so each hit came from a view the preamble used to prevent.
  expect_exit "and neither file's raw bytes hold such a token" 1 \
    grep -qE "(^|[^A-Za-z0-9])${decoy}([^A-Za-z0-9]|$)" \
      "${box}/bom/run.jsonl" "${box}/log-prefix/run.jsonl"
  # ...and the other control, which stops the pair from passing on a decoder
  # that returns a view for ANY line: prose with a stray brace is not JSON and
  # must still decode to nothing.
  mkdir -p "${box}/brace-prose"
  printf 'a sentence with a { brace and no JSON in it at all\n' \
    > "${box}/brace-prose/notes.txt"
  expect_exit "a stray brace in prose is not a document" 0 \
    python3 "${ROOT}/scripts/check-hashes.py" --table "$table" \
      --tree "${box}/brace-prose"

  # THE DECODER MISSING IS NOT A FINDING. `sys.exit(message)` exits one, which
  # is this scanner's code for "ran, and found something" -- so an operator
  # error would have been reported as a forbidden literal.
  mkdir -p "${box}/lonely"
  cp "${ROOT}/scripts/check-hashes.py" "${box}/lonely/check-hashes.py"
  expect_exit "a scanner with no decoder beside it is broken, not dirty" 2 \
    python3 "${box}/lonely/check-hashes.py" --table "$table" \
      --tree "${box}/decoy-hit"
  cp "${ROOT}/scripts/hygiene-decode.py" "${box}/lonely/hygiene-decode.py"
  expect_exit "and the decoder's own driver says so the same way" 2 \
    bash -c "printf '' | python3 '${box}/lonely/hygiene-decode.py' --into '${box}/lonely/mirror'"

  # A dot-prefixed directory under a results root is linted, not skipped.
  #
  # The valid directory beside it is what makes this assertion able to fail.
  # With only the hidden one, BOTH behaviours exit 1 -- linted, it fails the
  # name rule; skipped, the root holds no run directories -- so the assertion
  # was satisfied either way and pinned nothing.
  mkdir -p "${box}/root/2026-01-01-ok" "${box}/root/.hidden-run"
  cp "${ROOT}"/results/_template/* "${box}/root/2026-01-01-ok/"
  cp "${ROOT}"/results/_template/* "${box}/root/.hidden-run/"
  # Built first: the linter dispatches the record verdict to diet through the
  # resolver, and a resolver with nothing to resolve refuses (2), which is
  # not the 1 this assertion is about.
  expect_exit "a dot-prefixed results directory is linted" 1 \
    bash -c "cd '${ROOT}' && cargo build --quiet -p discipline-diet --bin diet \
      && python3 scripts/check-results.py --root '${box}/root'"

  # The same leak through a file handle. A contributor with commit signing
  # enabled must not get a different verdict from the same tree.
  local gitfake; scratch; gitfake="$SCRATCH"
  printf '[commit]\n\tgpgsign = true\n[user]\n\tsigningkey = 0xDEADBEEF\n' \
    > "${gitfake}/.gitconfig"
  mkdir -p "${gitfake}/repo"
  git -C "${gitfake}/repo" init --quiet
  printf 'x\n' > "${gitfake}/repo/a.txt"
  git -C "${gitfake}/repo" add --all
  expect_exit "a signing global gitconfig cannot reach a sandbox" 0 \
    env HOME="$gitfake" bash "${ROOT}/scripts/hermetic.sh" \
      git -C "${gitfake}/repo" -c user.email=seed@example.invalid -c user.name=seed \
        commit --quiet --message 'probe'

  # Hermeticity itself. Nothing that identifies a repository or a CI system
  # may cross into a sandbox, whatever the ambient environment holds.
  expect_exit "no ambient CI identity reaches a sandbox" 0 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=push GITHUB_SHA=deadbeef \
        GITHUB_EVENT_PATH=/nonexistent RUNNER_OS=Linux CI=true \
      bash "${ROOT}/scripts/hermetic.sh" bash -c \
        'set -u; for v in ${!GITHUB_@} ${!RUNNER_@} ${CI+CI}; do exit 1; done; exit 0'

  # Checks that read the environment, exercised under a FAKED one. `history`
  # is the only such check today; it went red in CI and green locally before
  # the sandbox was made hermetic, and nothing would have caught that here.
  #
  # On a repository of this assertion's OWN making, never the ambient
  # checkout. The first version of these read HEAD~1 and origin/main from
  # whatever tree it happened to be run in, and a runner clones at depth 1 --
  # so they passed on a laptop and failed on CI, which is precisely the class
  # of divergence they exist to catch.
  local fake; scratch; fake="$SCRATCH"
  mkdir -p "${fake}/repo"
  cp -R "${ROOT}/scripts" "${fake}/repo/scripts"
  (
    cd "${fake}/repo"
    git init --quiet
    printf 'a\n' > a.txt && git add --all && seed_commit --message 'base'
    git update-ref refs/remotes/origin/main HEAD
    printf 'b\n' > b.txt && git add --all && seed_commit --message 'second'
  )
  local fake_base fake_head
  fake_base="$(git -C "${fake}/repo" rev-parse HEAD~1)"
  fake_head="$(git -C "${fake}/repo" rev-parse HEAD)"
  local zero; zero="$(printf '0%.0s' $(seq 40))"

  printf '{"before":"%s","after":"%s"}' "$zero" "$fake_head" \
    > "${fake}/push-new-branch.json"
  printf '{"pull_request":{"base":{"sha":"%s"},"head":{"sha":"%s"},"title":"t","body":"carries %s%s forward"}}' \
    "$fake_base" "$fake_head" 'DIE' '-9001' > "${fake}/pr-dirty.json"
  printf '{"pull_request":{"base":{"sha":"%s"},"head":{"sha":"%s"},"title":"carries %s%s forward","body":"clean"}}' \
    "$fake_base" "$fake_head" 'DIE' '-9003' > "${fake}/pr-dirty-title.json"
  printf '{"before":"%s","after":"%s"}' "$fake_head" "$fake_head" \
    > "${fake}/push-empty.json"

  expect_exit "history: a faked pull request with a dirty body" 1 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=pull_request \
        GITHUB_EVENT_PATH="${fake}/pr-dirty.json" \
      python3 "${fake}/repo/scripts/check-history.py"
  # A fresh-instance review of #83 found the title half of this pair had no
  # fixture at all: every history fixture in this file used a clean,
  # constant `"title":"t"`, so deleting the two lines in check-history.py
  # that read `pr["title"]` stayed green. The body half was already covered.
  expect_exit "history: a faked pull request with a dirty title" 1 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=pull_request \
        GITHUB_EVENT_PATH="${fake}/pr-dirty-title.json" \
      python3 "${fake}/repo/scripts/check-history.py"
  expect_exit "history: a faked push whose range is empty" 2 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=push \
        GITHUB_EVENT_PATH="${fake}/push-empty.json" \
      python3 "${fake}/repo/scripts/check-history.py"
  expect_exit "history: a faked new-branch push resolves a base" 0 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=push \
        GITHUB_EVENT_PATH="${fake}/push-new-branch.json" \
      python3 "${fake}/repo/scripts/check-history.py"

  # A SECRET IN A PATH GIT WILL NOT DIFF. Without `--text`, `git show` prints
  # `Binary files a/x and b/x differ` for a path its NUL heuristic calls
  # binary AND for a plain-text path marked `-diff` in .gitattributes -- so
  # the content never reaches the scan. Added-then-removed is the hole this
  # check exists to close, and it stayed open for exactly the content the
  # pattern table singles out with its `b` flag: "a secret in a .pack or an
  # image is exactly as committed as one in a text file".
  #
  # The `-diff` form is used here rather than the NUL form because it is the
  # worse one: an attribute committed to the tree under scan decided what the
  # history scan was allowed to see.
  local fake_undiffable
  (
    cd "${fake}/repo"
    printf 'hidden.txt -diff\n' > .gitattributes
    printf 'ticket %s%s, in a path git will not diff\n' 'DIE' '-4242' \
      > hidden.txt
    git add --all && seed_commit --message 'a path marked -diff'
  )
  fake_undiffable="$(git -C "${fake}/repo" rev-parse HEAD)"
  printf '{"pull_request":{"base":{"sha":"%s"},"head":{"sha":"%s"},"title":"t","body":"clean"}}' \
    "$fake_head" "$fake_undiffable" > "${fake}/pr-undiffable.json"
  expect_exit "history: a secret git renders as binary is still found" 1 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=pull_request \
        GITHUB_EVENT_PATH="${fake}/pr-undiffable.json" \
      python3 "${fake}/repo/scripts/check-history.py"
  # The control. git really does hide it: if this stops finding the `Binary
  # files` line, the assertion above has stopped testing `--text` and is
  # passing on an ordinary text diff.
  expect_exit "and git really does hide that path without --text" 0 \
    bash -c "git -C '${fake}/repo' show --format= --patch '${fake_undiffable}' \
      | grep -q '^Binary files'"

  # A COMMITTED BYTE THAT IS NOT UTF-8. `text=True` decodes the patch as
  # strict UTF-8 and RAISES on the first byte that is not -- and `git show
  # --patch` pulls raw file content into that decode. The traceback exits 1,
  # which is this repository's code for "the scan ran and found something".
  # A crash is not a finding: it reddens the lane over history nothing can
  # edit, and no content change clears it.
  local fake_undecodable
  (
    cd "${fake}/repo"
    printf 'caf\xe9, in cp1252\n' > bytes.txt
    git add --all && seed_commit --message 'a byte that is not UTF-8'
  )
  fake_undecodable="$(git -C "${fake}/repo" rev-parse HEAD)"
  printf '{"pull_request":{"base":{"sha":"%s"},"head":{"sha":"%s"},"title":"t","body":"clean"}}' \
    "$fake_undiffable" "$fake_undecodable" > "${fake}/pr-undecodable.json"
  expect_exit "history: a committed non-UTF-8 byte is survived, not reported" 0 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=pull_request \
        GITHUB_EVENT_PATH="${fake}/pr-undecodable.json" \
      python3 "${fake}/repo/scripts/check-history.py"
  # The control, and this one is load-bearing: without it the assertion above
  # is satisfied by any clean range, and says nothing about the decoding.
  cat > "${fake}/strict.py" <<'STRICT'
import subprocess, sys
try:
    subprocess.run(
        ["git", "-C", sys.argv[1], "show", "--format=", "--patch", "--text",
         sys.argv[2]],
        capture_output=True, text=True,
    )
except UnicodeDecodeError:
    sys.exit(0)
sys.exit(1)
STRICT
  expect_exit "and a strict decode of that same patch really does raise" 0 \
    python3 "${fake}/strict.py" "${fake}/repo" "$fake_undecodable"

  # AN AUTHOR LINE IS METADATA, NOT CONTENT -- ruled 2026-09-14 on #81, and
  # the PAIR is what says so. The same literal is planted twice: once in a
  # commit body, where it is a finding, and once in the author line of a
  # commit whose body and patch are both clean, where it is not.
  #
  # Neither half asserts anything alone. Delete the pattern from the table
  # and the second still passes; go back to scanning `%an <%ae>` and the
  # first still passes. Only together do they pin WHERE the table applies.
  #
  # A CORRECTION. This planted the repository owner's own handle, on the
  # THEN-current premise that it was a hashed literal in
  # `scripts/hygiene-hashes.txt` -- which is what made it double as the
  # thing an author line legitimately carries. Ruled separately on #58
  # (2026-09-15, landed on `main` in 0c20052): the owner's handle firing on
  # the owner's own prose was the wrong rule regardless of which field it
  # scanned, and the entry was removed from the denylist outright. That
  # left this fixture proving nothing -- the literal it plants was no
  # longer listed, so planting it in a body was never going to be a
  # finding, pattern table or not.
  #
  # A TICKET ID never depended on that entry and does not depend on
  # whatever the denylist holds tomorrow: `internal-ticket-id` is a SHAPE
  # in `hygiene-patterns.tsv`, not a hash of one string, so this pair is
  # robust to the hashed list shrinking to nothing, which it very nearly
  # has (one entry remains, and its plaintext belongs to nobody this
  # fixture can cite).
  #
  # Assembled from pieces, because this file is itself read by the tree gate
  # and a literal written whole here would be a finding about verify.sh --
  # the trap `inject_injection_needs_gnu_sed` documents, one lint along.
  local token; token="$(printf '%s%s' 'DIE' '-9002')"
  local fake_body fake_author
  (
    cd "${fake}/repo"
    printf 'd\n' > d.txt && git add --all
    seed_commit --message "a message carrying ${token}, which is content"
  )
  fake_body="$(git -C "${fake}/repo" rev-parse HEAD)"
  (
    cd "${fake}/repo"
    printf 'e\n' > e.txt && git add --all
    git -c "user.email=${token}@example.invalid" -c "user.name=${token}" \
      commit --quiet --message 'a clean message, and an author line that is not'
  )
  fake_author="$(git -C "${fake}/repo" rev-parse HEAD)"
  printf '{"pull_request":{"base":{"sha":"%s"},"head":{"sha":"%s"},"title":"t","body":"clean"}}' \
    "$fake_undecodable" "$fake_body" > "${fake}/pr-in-body.json"
  printf '{"pull_request":{"base":{"sha":"%s"},"head":{"sha":"%s"},"title":"t","body":"clean"}}' \
    "$fake_body" "$fake_author" > "${fake}/pr-in-author.json"
  expect_exit "history: a listed literal in a commit body is a finding" 1 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=pull_request \
        GITHUB_EVENT_PATH="${fake}/pr-in-body.json" \
      python3 "${fake}/repo/scripts/check-history.py"
  expect_exit "history: the same literal in an author line is not content" 0 \
    env GITHUB_ACTIONS=true GITHUB_EVENT_NAME=pull_request \
        GITHUB_EVENT_PATH="${fake}/pr-in-author.json" \
      python3 "${fake}/repo/scripts/check-history.py"

  # `--range` REACHES THE CHECK, AND BEATS THE INFERRED ANSWER. A flag that
  # is accepted and dropped would leave the reproduction finding open while
  # looking closed, so the two rows are chosen to disagree with the default:
  # this repository's inferred range is `origin/<default>..HEAD`, which here
  # spans every commit above and is DIRTY, so a dropped `--range` gives 1 on
  # a slice whose verdict is 0.
  #
  # Copied in after the commits, so that no `git add --all` above sweeps it
  # into the history under scan.
  cp "${ROOT}/verify.sh" "${fake}/repo/verify.sh"
  expect_exit "history: an explicit range is honoured, not the inferred one" 0 \
    bash "${fake}/repo/verify.sh" --only history --range "${fake_body}..${fake_author}"
  expect_exit "history: and an explicit range that is dirty is still a finding" 1 \
    bash "${fake}/repo/verify.sh" --only history --range "${fake_undecodable}..${fake_body}"
  # A range narrows one check, so a loose one is a misuse and not a verdict --
  # `--scope`'s rule, for a check that INFERS what it scans when unasked.
  expect_exit "a range without --only history is a misuse" 2 \
    bash "${ROOT}/verify.sh" --range "${fake_body}..${fake_author}"

  # The CI aggregator's comparison. A skipped job is not a failed job, and
  # GitHub's own `!failure()` idiom passes on skipped, so the one thing this
  # must get right is that only the literal 'success' passes.
  expect_exit "the gate accepts a run where every job succeeded" 0 \
    env NEEDS='{"a":{"result":"success"}}' \
      python3 "${ROOT}/scripts/check-job-results.py"
  expect_exit "the gate rejects a SKIPPED job" 1 \
    env NEEDS='{"a":{"result":"success"},"b":{"result":"skipped"}}' \
      python3 "${ROOT}/scripts/check-job-results.py"
  expect_exit "the gate rejects depending on no jobs at all" 1 \
    env NEEDS='{}' python3 "${ROOT}/scripts/check-job-results.py"

  # A surface whose table sets `scan: all` must reject a file it cannot scan.
  # UTF-16 renders fine in a browser but encodes ASCII as two bytes, so it
  # defeats every pattern byte-wise; reporting it clean would be a lie.
  mkdir -p "${box}/utf16"
  python3 -c 'import pathlib,sys; pathlib.Path(sys.argv[1]).write_bytes(
    "<script src=\"https://cdn.example.com/x.js\"></script>\n".encode("utf-16"))' \
    "${box}/utf16/page.html"
  expect_exit "an unscannable page is rejected, not called clean" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --patterns "${ROOT}/scripts/pages-patterns.tsv" \
      --tree "${box}/utf16"

  # A DECLARED EXCEPTION SUBTRACTS A FORM, NOT A PATTERN -- ruled 2026-09-12
  # on #72. `ssh-user-at-host` keeps its exact shape; a public forge's own
  # SSH remote is carved out by name in `hygiene-exceptions.tsv`, and a
  # private host beside it, or instead of it, still fires.
  mkdir -p "${box}/forge-alone" "${box}/private-host" "${box}/forge-and-private"
  printf 'deploy_key = %s\n' "$(printf '%s' 'git@github.com:owner/repo.git')" \
    > "${box}/forge-alone/remote.txt"
  printf 'scp %s%s:/var/log/run.log .\n' 'someone@' 'box.example.net' \
    > "${box}/private-host/remote.txt"
  printf 'clone %s and warn %s%s\n' "$(printf '%s' 'git@github.com:owner/repo.git')" \
    'someone@' 'evil.example.net:' > "${box}/forge-and-private/remote.txt"
  expect_exit "ssh-user-at-host: a forge remote form is a declared exception" 0 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/forge-alone"
  expect_exit "ssh-user-at-host: a private host is not exempted by any declared form" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/private-host"
  expect_exit "ssh-user-at-host: a private host beside an exempt one still fires" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/forge-and-private"

  # THE PATTERN HALF READS THE SAME NORMALISED VIEW AS THE HASH HALF -- ruled
  # 2026-09-12 on #72: "the gate scans content as it would be read... [so]
  # normalise once ... then run both halves over the result." A fresh-instance
  # review of #83 found this specific claim uncovered: `hygiene-decode.py`'s
  # raw-content mirror (`raw_normalized = decoding.normalized(text)`) had no
  # fixture at all, so deleting the whole branch stayed green -- measured, and
  # fixed here rather than only noted.
  #
  # A zero-width space between a private host's first label and its dot
  # breaks the colon form's `(\.[A-Za-z0-9-]+)+:` requirement in the RAW
  # bytes, and carries no ssh/scp/rsync keyword, so neither alternative in
  # the pattern matches unread. `Default_Ignorable_Code_Point` is stripped
  # before either half sees this file's decoded view, the shape re-forms,
  # and the pattern half catches it there -- the digest half's zero-width
  # case, over again for the half that matches by shape instead of by hash.
  mkdir -p "${box}/private-host-zwsp"
  python3 -c "
import sys
open(sys.argv[1], 'w', encoding='utf-8').write(
    'remote = someone@bo' + '​' + 'x.example.net:/var/log/run.log .\n')
" "${box}/private-host-zwsp/remote.txt"
  expect_exit "ssh-user-at-host: a host split by a zero-width space is still found" 1 \
    bash "${ROOT}/scripts/hygiene.sh" --tree "${box}/private-host-zwsp"
  # The control: the SAME bytes, matched directly against the pattern's own
  # shape, are not a hit. Without it the assertion above could pass because
  # the pattern is loose, not because the mirror caught anything.
  expect_exit "and the same zero-width host, unread, is not a hit" 1 \
    grep -qiE '(^|[^A-Za-z0-9._-])([A-Za-z0-9._-]+@[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)+:|(ssh|scp|rsync)[[:space:]]+[A-Za-z0-9._-]+@[A-Za-z0-9-]+)' \
      "${box}/private-host-zwsp/remote.txt"

  # A DECLARED EXCEPTION CONTAINING A SLASH IS A REFUSAL, NOT A FINDING. A
  # fresh-instance review of #83 found `sed -E "s/${exception}//${sed_flags}"`
  # breaks its own delimiter on an unescaped `/`, and under this file's
  # `set -e`, the unchecked failure killed the whole scan with EXIT_DIRTY (1,
  # "found something") before the line that tripped it was ever reported --
  # so a malformed row in a maintainer's own table read as a commit to
  # rewrite. A synthetic pattern and a slash-bearing exception, never the
  # real tables.
  mkdir -p "${box}/broken-exception"
  printf 'custom-label\t-\tprivate@[a-z.]+:\n' > "${box}/broken-exception-patterns.tsv"
  printf 'custom-label\tprivate@|https://forge\\.example\\.com/\n' \
    > "${box}/broken-exception-exceptions.tsv"
  printf 'remote = %s%s:/srv\n' 'private@host' '.example' \
    > "${box}/broken-exception/remote.txt"
  expect_exit "a declared exception with an unescaped slash is refused, not misreported" 2 \
    bash "${ROOT}/scripts/hygiene.sh" --patterns "${box}/broken-exception-patterns.tsv" \
      --tree "${box}/broken-exception"

  # BOTH SPELLINGS OF ONE LITERAL, ONE DIGEST -- ruled 2026-09-12 on #72.
  # `--emit` computes the row from the NFC form; a file carrying the NFD
  # spelling of the SAME name must be caught by that same row, because
  # `decoding.normalized` composes before it tokenises. A synthetic table,
  # never the real `hygiene-hashes.txt`: this proves the mechanism, not a
  # claim about what this repository's own digests happen to guard.
  local nfc_name nfd_name nfc_row salt_line table
  nfc_name="$(python3 -c 'import unicodedata; print(unicodedata.normalize("NFC", "café"))')"
  nfd_name="$(python3 -c 'import unicodedata; print(unicodedata.normalize("NFD", "café"))')"
  [ "$nfc_name" != "$nfd_name" ] || {
    echo "verify: the NFC/NFD probe strings collapsed to one spelling" >&2
    exit "$EXIT_FAIL"
  }
  nfc_row="$(python3 "${ROOT}/scripts/check-hashes.py" --emit "$nfc_name" probe-name)"
  table="${box}/nfc-nfd-hashes.txt"
  {
    echo "# Salt: discipline-hygiene-v1"
    echo "$nfc_row"
  } > "$table"
  mkdir -p "${box}/nfc-form" "${box}/nfd-form"
  printf 'owner: %s\n' "$nfc_name" > "${box}/nfc-form/owner.txt"
  printf 'owner: %s\n' "$nfd_name" > "${box}/nfd-form/owner.txt"
  expect_exit "check-hashes: the NFC spelling matches its own row" 1 \
    bash -c "printf '%s\0' '${box}/nfc-form/owner.txt' \
      | python3 '${ROOT}/scripts/check-hashes.py' --table '$table'"
  expect_exit "check-hashes: the NFD spelling of the same name matches too" 1 \
    bash -c "printf '%s\0' '${box}/nfd-form/owner.txt' \
      | python3 '${ROOT}/scripts/check-hashes.py' --table '$table'"
  expect_exit "check-hashes: --emit refuses a decomposed literal" 2 \
    python3 "${ROOT}/scripts/check-hashes.py" --emit "$nfd_name" probe-name

  # The results-fixture loop grades on the class the manifest declares, so the
  # emission is load-bearing: a `failure_class` nothing prints is a field the
  # grader cannot read, and the loop silently falls back to "exited 1" -- which
  # is the state this branch was in while twenty-four declared defects went
  # unexercised. Pinned in both directions.
  expect_exit "every results fixture emits the class its fault declares" 0 \
    bash -c "cd '${ROOT}' && python3 scripts/check-fault-manifest.py --fixture-classes \
      | while IFS=\$'\\t' read -r name want; do \
          out=\$(python3 scripts/check-results.py \"tests/fixtures/results-bad/\${name}\" 2>&1) \
            && exit 1; \
          grep -qF \"[\${want}]\" <<<\"\${out}\" || { echo \"\${name}: no \${want}\"; exit 1; }; \
        done"
  expect_exit "a fixture red for another fixture's reason is not a pass" 1 \
    bash -c "cd '${ROOT}' \
      && out=\$(python3 scripts/check-results.py tests/fixtures/results-bad/2026-01-27-UPPERCASE-slug 2>&1); \
      grep -qF '[results.sections-wrong]' <<<\"\${out}\""
}

selftest() {
  # DECLARED REQUIREMENT, checked before anything runs. The results-fixture
  # loop below keys its expectations in an associative array (`local -A`),
  # which is bash 4. The bash a stock Mac ships is 3.2, where `local -A` is a
  # usage error and the next expansion aborts the run under `set -u` -- after
  # every seeded case has run and before the tally -- and the status bash 3.2
  # reports for that abort is not the abort's: with the EXIT trap installed it
  # is the trap's (0 in a full run here; `return 3` reads 3 in reduction). So a
  # stock Mac got a green selftest that proved nothing, the vacuous pass at the
  # gate's own front door. Refuse first, name the version, exit 2. The ordinary
  # gate has no such requirement and runs on 3.2. Ruled on #39 (2026-09-11).
  if [ "${BASH_VERSINFO[0]:-0}" -lt 4 ]; then
    echo "selftest: needs bash 4 or later (associative arrays); this is bash ${BASH_VERSION}" >&2
    exit "$EXIT_MISUSE"
  fi
  trap selftest_cleanup EXIT
  # The assignment, in scope for in_shard and load_shard_plan below. `local`
  # rather than a file-scope `declare -A`, which the bash 3.2 the ordinary gate
  # still runs on refuses outright; see load_shard_plan.
  local -A SHARD_OF=()
  now_ms; local selftest_started_ms="$NOW_MS"
  [ "$SELFTEST_SHARD" -eq 0 ] || load_shard_plan
  scratch; SELFTEST_TARGET="${SCRATCH}/target"
  scratch; SELFTEST_LOGS="$SCRATCH"
  # DERIVE MODE KEEPS ITS LOGS. `selftest_cleanup` removes every scratch it
  # registered, so an index written there would name files that no longer
  # exist by the time anybody read it. The operator names a directory instead
  # and it is not registered for cleanup.
  if [ -n "$SELFTEST_DERIVE" ]; then
    mkdir -p "$SELFTEST_DERIVE" || {
      echo "selftest: --derive-scopes ${SELFTEST_DERIVE} could not be made" >&2
      exit "$EXIT_MISUSE"
    }
    SELFTEST_LOGS="$SELFTEST_DERIVE"
    # Truncated, never appended to. An index carrying rows from two runs is a
    # derivation over a tree that never existed.
    : > "${SELFTEST_LOGS}/derive-scopes.tsv"
  fi
  # The cost harvest keeps nothing but its index, so unlike --derive-scopes it
  # does not move SELFTEST_LOGS: the logs are the scope derivation's evidence
  # and mean nothing to a packer.
  if [ -n "$SELFTEST_COST_INDEX" ]; then
    : > "$SELFTEST_COST_INDEX" || {
      echo "selftest: the cost index ${SELFTEST_COST_INDEX} could not be written" >&2
      exit "$EXIT_MISUSE"
    }
  fi
  # The one sandbox path every case is built into and torn down from. Made
  # here rather than per case; see SELFTEST_BOX for the measurement that says
  # why.
  scratch; SELFTEST_BOX="${SCRATCH}/box"
  # Made here, not by the first case, so that seeded_case's refusal to inject
  # into a box that is not a directory keeps its meaning: an unset or removed
  # box is a misuse, not a first run.
  mkdir -p "$SELFTEST_BOX"

  # sandbox(), seed_commit and the fake-repository builder all run git in THIS
  # process, outside scripts/hermetic.sh, so they need the same protection from
  # the contributor's global git config. Scoped to the selftest: the real
  # `history` check reads the real repository and should see its real config.
  export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null

  echo "Seeded-fault selftest. Every line below must read RED: the fault is"
  echo "deliberate and the gate is what is under test. A case that goes red"
  echo "without its own signature in the log reads WRONG, not RED."
  echo

  seeded_case "misformatted source"                   fmt      inject_fmt \
    'Diff in .*diet/src/lib\.rs'
  seeded_case "clippy lint violation"                 clippy   inject_clippy \
    'ptr_arg'
  seeded_case "failing unit test"                     test     inject_test \
    'seeded_fault::seeded_failure \.\.\. FAILED' 'lib/seeded_fault'
  seeded_case "non-conforming format fixture"         test     inject_conformance \
    'regimen/fixtures/valid/seeded-nonconforming\.toml' 'test:conformance/formats::regimen'
  seeded_case "FORMATS emptied, harness covers none"  test     inject_formats_empty \
    'FORMATS is empty' 'test:conformance'
  seeded_case "decline grammar loses its end anchor"  test     inject_decline_unanchored \
    'decline/fixtures/invalid/coordinated-with-and\.txt' 'test:conformance/formats::decline'
  seeded_case "interview drops continuation lines"    test     inject_interview_drops_continuations \
    'interview/fixtures/valid/multi-line-continuation\.txt' 'test:conformance/formats::interview'
  seeded_case "interview blind to truncation"         test     inject_interview_truncation_blind \
    'interview/fixtures/valid/truncated-unterminated-fence\.txt' 'test:conformance/formats::interview'
  seeded_case "interview discards trailing content"   test     inject_interview_discards_trailing \
    'formats::interview::tests::every_byte_of_an_answer_is_accounted_for \.\.\. FAILED' 'lib/formats::interview::tests'
  seeded_case "an event kind with no fixture"         test     inject_record_unfixtured_kind \
    'formats::record::tests::every_event_kind_appears_in_the_committed_corpus \.\.\. FAILED' 'lib/formats::record::tests'
  seeded_case "record substrate made optional"        test     inject_record_substrate_optional \
    'record/fixtures/invalid/regime-missing-substrate\.jsonl' 'test:conformance/formats::record'
  seeded_case "a summary kind's fields made advisory" test     inject_record_summary_kind_fields_advisory \
    'record/fixtures/invalid/recompute-summary-carries-turns\.jsonl' 'test:conformance/formats::record'
  seeded_case "a recompute summary with no product digest" test inject_record_recompute_digest_optional \
    'record/fixtures/invalid/recompute-summary-without-its-product-digest\.jsonl' 'test:conformance/formats::record'
  seeded_case "a summary that cannot be true of itself" test   inject_record_summary_impossible_unchecked \
    'record/fixtures/invalid/recompute-matched-exceeds-checked\.jsonl' 'test:conformance/formats::record'
  seeded_case "a substrate reference that resolves to anything" test inject_record_substrate_reference_unchecked \
    'record/fixtures/invalid/lane-names-an-undeclared-substrate\.jsonl' 'test:conformance/formats::record'
  seeded_case "a substrate reference defaulted when alone" test inject_record_substrate_defaults_when_alone \
    'record/fixtures/invalid/request-with-no-substrate\.jsonl' 'test:conformance/formats::record'
  seeded_case "weights identified by name"            test     inject_record_weights_named_not_digested \
    'record/fixtures/invalid/weights-named-not-digested\.jsonl' 'test:conformance/formats::record'
  seeded_case "an effort level with no budget"        test     inject_regimen_reasoning_pair_unchecked \
    'regimen/fixtures/invalid/reasoning-effort-without-budget\.toml' 'test:conformance/formats::regimen'
  seeded_case "two ids over one substrate"            test     inject_record_substrates_indistinguishable \
    'record/fixtures/invalid/substrates-indistinguishable\.jsonl' 'test:conformance/formats::record'
  seeded_case "a head change that is not a change"    test     inject_record_prefix_change_not_a_change \
    'record/fixtures/invalid/prefix-change-that-is-not-a-change\.jsonl' 'test:conformance/formats::record'
  seeded_case "the miss classes reordered"           test     inject_record_prefix_precedence_reordered \
    'formats::record::tests::the_precedence_over_prefix_reasons_is_the_order_they_are_declared_in \.\.\. FAILED' 'lib/formats::record'
  seeded_case "a head change attributed to anything"  test     inject_record_prefix_reason_unchecked \
    'record/fixtures/invalid/prefix-reason-disagrees-with-its-diff\.jsonl' 'test:conformance/formats::record'
  seeded_case "a head fingerprint that is prose"      test     inject_record_head_fingerprint_not_digested \
    'record/fixtures/invalid/head-fingerprint-not-a-digest\.jsonl' 'test:conformance/formats::record'
  seeded_case "a live request with no head hashed"    test     inject_record_head_unhashed_in_live_record \
    'record/fixtures/invalid/request-unhashed-in-a-live-record\.jsonl' 'test:conformance/formats::record'
  seeded_case "an injection only GNU sed accepts"     injections inject_injection_needs_gnu_sed \
    'sed forms only GNU accepts'
  seeded_case "the runner's digest made advisory"     test     inject_bakeoff_digest_unchecked \
    'capture::bakeoff::tests::a_cache_the_record_did_not_consume_is_refused \.\.\. FAILED' 'lib/capture::bakeoff'
  seeded_case "the assembled directory's evidence is elsewhere" test inject_bakeoff_evidence_not_attached \
    'capture::bakeoff::tests::the_assembled_directory_is_one_the_gates_accept \.\.\. FAILED' 'lib/capture::bakeoff'
  seeded_case "a budget no fixture demonstrates"      test     inject_bakeoff_budget_unfixtured \
    'capture::sense::tests::every_pre_registered_budget_is_one_its_fixture_demonstrates \.\.\. FAILED' 'lib/capture::sense'
  seeded_case "a row that links to itself"            test     inject_record_self_link_allowed \
    'record/fixtures/invalid/retry-of-itself\.jsonl' 'test:conformance/formats::record'
  seeded_case "record nesting left unbounded"         test     inject_record_depth_unbounded \
    'record/fixtures/invalid/deep-nesting\.jsonl' 'test:conformance/formats::record'
  seeded_case "grounding floor made inert"            test     inject_grounded_floor_inert \
    'capture::grounded::tests::a_lane_below_the_floor_is_rejected_whole_and_recorded \.\.\. FAILED' 'lib/capture::grounded::tests'
  seeded_case "grounding gates a judgment field"      test     inject_grounded_gates_judgment \
    'capture::grounded::tests::the_gate_does_not_touch_a_judgment_class_field \.\.\. FAILED' 'lib/capture'
  seeded_case "a score with no demonstrated failure"  test     inject_grounded_undemonstrated \
    'capture::grounded::tests::a_measurement_requires_its_instrument_to_have_been_seen_fail \.\.\. FAILED' 'lib/capture::grounded::tests'
  seeded_case "grounding loosened to recombination"   test     inject_grounded_loose_matching \
    'capture::grounded::tests::presence_is_not_recombination \.\.\. FAILED' 'lib/capture::grounded::tests'
  seeded_case "a floor of zero"                       test     inject_grounded_zero_floor \
    'capture::grounded::tests::a_floor_must_be_pre_registered_and_nonzero \.\.\. FAILED' 'lib/capture::grounded::tests'
  seeded_case "supersede deletes what it replaced"    test     inject_object_supersede_deletes \
    'object::tests::a_supersede_voids_and_links_and_never_deletes \.\.\. FAILED' 'lib'
  seeded_case "the reconciler stops deduping"         test     inject_object_no_dedup \
    'object::tests::two_forks_saying_the_same_thing_dedup_with_both_provenances \.\.\. FAILED' 'lib/object'
  seeded_case "a correction that restates what it voids" test   inject_object_self_void \
    'object::tests::a_supersede_that_restates_the_entry_it_voids_is_refused \.\.\. FAILED' 'lib/object::tests'
  seeded_case "a state change over a supersede link"  test     inject_object_state_overwrite \
    'object::tests::a_voided_entry_cannot_be_resolved_retired_or_voided_again \.\.\. FAILED' 'lib/object::tests'
  seeded_case "a no-op patch that claims an entry"    test     inject_object_false_attribution \
    'object::tests::a_patch_that_changes_nothing_claims_nothing \.\.\. FAILED' 'lib/object::tests'
  seeded_case "a usage error that exits zero"         test     inject_cli_usage_exit \
    'a_usage_error_exits_two_and_prints_no_result \.\.\. FAILED' 'test:cli'
  seeded_case "a verb wired to the wrong format"      test     inject_cli_wrong_format \
    'every_verb_reads_its_own_formats_valid_fixtures \.\.\. FAILED' 'test:cli'
  seeded_case "a CLI that prints no result"           test     inject_cli_silent \
    'every_verb_prints_its_result_on_stdout \.\.\. FAILED' 'test:cli'
  seeded_case "the regime moves under a patch"        test     inject_object_regime_mutable \
    'object::tests::no_patch_variant_can_move_the_regime \.\.\. FAILED' 'lib/object::tests'
  seeded_case "dedup rebinds instead of aliasing"     test     inject_object_no_alias \
    'object::tests::an_alias_resolves_to_its_entry_and_keeps_both_provenances \.\.\. FAILED' 'lib/object::tests'
  seeded_case "a turn applied in arrival order"       test     inject_object_unsorted_turn \
    'object::tests::a_turns_patches_apply_the_same_in_any_order \.\.\. FAILED' 'lib/object::tests'
  seeded_case "a self-supersede through an alias"     test     inject_object_alias_self_void \
    'object::tests::a_supersede_of_an_entry_by_its_own_alias_is_refused \.\.\. FAILED' 'lib/object::tests'
  seeded_case "a field kind nothing covers"           test     inject_field_kind_variant \
    'error\[E0004\]: non-exhaustive patterns: `FieldKind::Seeded` not covered' 'lib'
  seeded_case "a subshell read as a group"            test     inject_shell_subshell_as_group \
    'formats::shell::tests::a_subshell_is_a_command_of_its_own_and_not_a_word \.\.\. FAILED' 'lib'
  seeded_case "a stderr pipe read as a plain pipe"    test     inject_shell_stderr_pipe_flat \
    'formats::shell::tests::a_stderr_pipe_is_the_duplication_it_abbreviates \.\.\. FAILED' 'lib/formats::shell::tests'
  seeded_case "an expanding word reported literal"    test     inject_shell_expansion_literal \
    'formats::shell::tests::a_brace_expansion_is_not_the_word_it_looks_like \.\.\. FAILED' 'lib'
  seeded_case "an empty payload read as absent"       test     inject_record_empty_payload_dropped \
    'formats::record::tests::archive_rows_keep_their_payloads_through_a_round_trip \.\.\. FAILED' 'lib/formats::record::tests'
  seeded_case "a tangent drop that deletes"           test     inject_tangent_drop_removes \
    'object::tangent::tests::a_dropped_entry_is_evicted_to_the_archive_and_never_deleted \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a parked entry that still renders"     test     inject_tangent_park_renders \
    'object::tangent::tests::a_parked_entry_is_retained_and_stops_speaking_for_the_object \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a tangent scoped by recency"           test     inject_tangent_scope_by_recency \
    'object::tangent::tests::an_entry_the_trunk_created_after_the_fork_is_not_in_the_tangents_scope \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a tangent closed leaving an entry unruled" test  inject_tangent_undisposed_ignored \
    'object::tangent::tests::a_tangent_born_entry_left_undisposed_refuses_the_closure \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a prefix claimed intact, not compared" test     inject_tangent_prefix_asserted \
    'object::tangent::tests::a_tangent_that_touched_the_trunk_says_the_prefix_moved \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a tangent id opened twice"             test     inject_tangent_id_reused \
    'object::tangent::tests::a_tangent_id_the_record_already_carries_is_refused \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a tangent that dates a fact at the fork" test   inject_tangent_provenance_ignores_turn \
    'object::tangent::tests::a_tangent_stamps_the_provenance_of_every_patch_made_under_it \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a closure ruling dated at the fork"    test     inject_tangent_ruling_dated_at_fork \
    'object::tangent::tests::a_closure_files_its_ruling_at_the_turn_it_closed \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a closure under a coined lane"         test     inject_tangent_closing_lane_coined \
    'object::tangent::tests::a_closure_files_its_ruling_in_the_canonical_lane \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a disposition missing from the list"   test     inject_tangent_disposition_missing_from_all \
    'object::tangent::tests::the_dispositions_are_the_three_a_closure_rules_with \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a tangent that offers a settled fact again" test inject_tangent_scope_offers_a_ruled_entry \
    'object::tangent::tests::a_tangent_does_not_offer_a_fact_it_has_already_ruled_on \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a prefix that counts dead trunk rows"  test     inject_tangent_prefix_counts_dead_rows \
    'object::tangent::tests::a_trunk_row_already_dead_at_the_fork_is_not_part_of_the_prefix \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a prefix called intact whenever it only grew" test inject_tangent_prefix_only_grew \
    'object::tangent::tests::a_trunk_that_went_on_writing_moved_the_prefix_the_fork_recorded \.\.\. FAILED' 'lib/object::tangent::tests'

  seeded_case "a parked entry that reads as retired"  test     inject_object_park_renders_as_retired \
    'object::tests::every_entry_state_renders_the_name_the_dump_promises \.\.\. FAILED' 'lib/object::tests'

  seeded_case "a park with no tangent behind it"      test     inject_object_park_needs_no_tangent \
    'object::tests::parking_an_entry_no_tangent_created_is_refused \.\.\. FAILED' 'lib/object::tests'

  seeded_case "a negative zero decimal accepted"      test     inject_record_negative_zero_decimal \
    'formats::record::json::tests::a_spelling_the_grammar_refuses_cannot_be_constructed \.\.\. FAILED' 'lib'
  seeded_case "the producer read as the last command" test     inject_shell_producer_is_last_written \
    'formats::shell::tests::the_producer_is_the_head_of_the_last_pipeline_and_not_its_tail \.\.\. FAILED' 'lib/formats::shell::tests'
  seeded_case "an operator dropped from the table"    test     inject_shell_operator_table_row_dropped \
    'formats::shell::tests::every_operator_is_named_here_and_the_table_says_the_same \.\.\. FAILED' 'lib/formats::shell::tests'
  seeded_case "a stripping heredoc that keeps tabs"   test     inject_shell_heredoc_strip_keeps_tabs \
    'formats::shell::tests::a_stripping_heredoc_strips_its_tabs_and_a_plain_one_keeps_them \.\.\. FAILED' 'lib/formats::shell::tests'
  seeded_case "the command word read as an operand"   test     inject_shell_command_word_is_an_operand \
    'formats::shell::tests::the_command_word_is_the_first_word_and_the_operands_are_the_rest \.\.\. FAILED' 'lib'
  seeded_case "a regimen float rendered as a string"  test     inject_regimen_float_as_a_string \
    'formats::regimen::tests::a_float_projects_as_a_number_and_not_as_its_digits_in_quotes \.\.\. FAILED' 'lib/formats::regimen::tests'
  seeded_case "a table header that opens nothing"     test     inject_regimen_table_scope_flattened \
    'formats::regimen::tests::a_table_scopes_the_keys_that_follow_it \.\.\. FAILED' 'lib/formats::regimen::tests'
  seeded_case "a header comment read as a table"      test     inject_regimen_header_comment_is_a_table \
    'formats::regimen::tests::a_comment_after_a_table_header_is_not_part_of_the_table \.\.\. FAILED' 'lib/formats::regimen::tests'
  seeded_case "two tables of one name accepted"       test     inject_regimen_table_collision_unchecked \
    'formats::regimen::tests::a_table_may_not_arrive_twice_or_take_a_key_s_name \.\.\. FAILED' 'lib/formats::regimen::tests'
  seeded_case "the float rule widened past the record" test    inject_regimen_float_rule_widened \
    'formats::regimen::tests::the_float_rule_and_the_records_decimal_rule_agree \.\.\. FAILED' 'lib/formats::regimen::tests'
  seeded_case "a summary the rows do not carry"       recompute inject_recompute_summary_not_derived \
    'the report does not re-derive'
  seeded_case "a results directory declaring no kind" recompute inject_recompute_kind_undeclared \
    'front-matter .kind. is None'
  seeded_case "a recompute that cannot fail"          recompute inject_recompute_cannot_fail \
    'does not compare the report to the artefacts'
  seeded_case "a probe blinded by a trailing comment"  recompute inject_recompute_probe_blinded_by_a_comment \
    'does not compare the report to the artefacts'
  seeded_case "a recompute that edits what it checks"  recompute inject_recompute_tampers \
    'tampering, not recomputation'
  seeded_case "a claim consuming evidence outside"     results   inject_results_consumes_outside \
    'outside the run directory'
  seeded_case "a claim consuming its own record"       results   inject_results_consumes_the_record \
    'a record cannot state its own hash'
  seeded_case "a claim consuming a file not there"     results   inject_results_consumes_a_missing_file \
    'which is not a file here'
  seeded_case "reproducible, with nothing to run"      recompute inject_recompute_script_missing \
    'carries no recompute.sh'
  seeded_case "the template carrying the opt-out"      recompute inject_recompute_template_opts_out \
    'the template declares .historical-observation.'
  seeded_case "a consumed digest gone stale"          results  inject_results_consumed_digest_stale \
    'but the committed file hashes to'
  seeded_case "an injection that changes nothing"     injections inject_inert_injection \
    'inject_that_changes_nothing'
  seeded_case "a nested table flattened"              test     inject_regimen_nested_table_flattened \
    'formats::regimen::tests::a_table_may_hold_one_table_and_no_more \.\.\. FAILED' 'lib/formats::regimen::tests'
  seeded_case "an array read by a second reader"      test     inject_regimen_array_second_reader \
    'formats::regimen::tests::an_array_holds_scalars_read_by_the_same_reader \.\.\. FAILED' 'lib/formats::regimen::tests'
  seeded_case "a merged field an injection cannot see" injections inject_injections_struct_grew \
    'builds a Provenance without cohort'
  seeded_case "a literal the scan cannot place"       injections inject_injections_literal_unplaceable \
    'builds a Ground: declared in 3 places'
  seeded_case "a case naming no injection"            injections inject_case_without_an_injection \
    'named by a seeded case, defined nowhere'
  seeded_case "a stringly predicate in the library"   library  inject_stringly_predicate \
    'a match arm on a string literal'
  seeded_case "a module nothing compiles"             library  inject_orphaned_module \
    'no .mod. declaration reaches it'
  seeded_case "a module commented out, not deleted"   library  inject_commented_out_module \
    'object.rs: no .mod. declaration reaches it'
  seeded_case "a stringly predicate cargo fmt wrapped" library inject_stringly_or_pattern \
    'a_decision_tag_that_is_quite_long_indeed'
  seeded_case "a claim bound to a test that is not there" library inject_dangling_claim \
    'claim names a test that does not exist'
  seeded_case "a record missing its substrates"       results  inject_results_no_substrate \
    'says diet check-record: a .start. row is missing its required .substrates.'
  seeded_case "results claim contradicts run.jsonl"   results  inject_results \
    'front-matter `turns` states 3 but the summary record binds'
  seeded_case "regimen.toml that is not a regimen"    regimen  inject_regimen \
    'BAD results/_template/regimen\.toml'
  seeded_case "a valid regimen that is not TOML"      regimen  inject_toml_subset \
    'accepted as a regimen but rejected by tomllib'
  seeded_case "a pin the resolver ignores"            regimen  inject_diet_bin_ignored \
    'a pin that can be silently ignored is not a pin'
  seeded_case "template label nothing defines"        metadata inject_metadata \
    'assigns label'
  seeded_case "forbidden content in the tree"         hygiene  inject_hygiene \
    'hygiene: internal-ticket-id:'
  seeded_case "external subresource on the site"      pages    inject_pages \
    'hygiene: external-subresource:'
  seeded_case "a type error in the web surface"       exercise inject_exercise_type_error \
    'error TS2322'
  seeded_case "a dogma tag in no vocabulary"          test     inject_interview_tag_undeclared \
    'formats::interview::tests::no_dogma_tag_is_missing_from_the_table \.\.\. FAILED' 'lib/formats::interview'
  seeded_case "operating points sorted, not in file order" test  inject_operating_points_sorted \
    "formats::operating_points::tests::the_entries_come_back_in_the_order_the_file_wrote_them \.\.\. FAILED" 'lib/formats::operating_points'
  seeded_case "an unmarked entry that shadows another is admitted" test  inject_shadowing_admitted \
    "formats::operating_points::tests::the_retired_spelling_is_refused_for_shadowing_and_not_something_else \.\.\. FAILED" 'lib/formats::operating_points'
  seeded_case "an integer terminal grown a second time" test     inject_number_terminal_regrown \
    'it belongs in number\.pest and nowhere else' 'test:conformance/the_integer_terminal'
  seeded_case "a shared body written out under another name" test inject_number_terminal_body_regrown \
    'has a shared terminal.s body written out again' 'test:conformance/the_integer_terminal'
  seeded_case "historical, and carrying a recompute"   recompute inject_recompute_historical_with_a_script \
    'declares .historical-observation. and carries a recompute\.sh'
  seeded_case "historical with no reason stated"       recompute inject_recompute_historical_without_a_reason \
    'states no .historical_reason.'
  seeded_case "historical with a reason that says nothing" recompute inject_recompute_historical_reason_blank \
    'states no .historical_reason.'
  seeded_case "only the template recomputes"           recompute inject_recompute_only_the_template_recomputes \
    'results are present and none recomputed'
  seeded_case "a check no workflow runs"              ci       inject_ci \
    'has no owner in check-owners\.tsv'
  seeded_case "pull requests filtered by branch"      ci       inject_ci_pr_branch_filter \
    'carries .branches: \[main\]. and is reached'
  seeded_case "CI narrowing the test check"           ci       inject_ci_scoped_test \
    'passes .--scope. to verify\.sh'
  seeded_case "CI narrowing the history check"        ci       inject_ci_ranged_history \
    'passes .--range. to verify\.sh'
  seeded_case "the trunk gated by no push run"        ci       inject_ci_push_ungated \
    'and has no .push:. trigger'
  seeded_case "one workflow renaming the trunk"       ci       inject_ci_trunk_typo \
    'disagree about which branch is the trunk'
  seeded_case "the trunk's name left uncorroborated"  ci       inject_ci_trunk_uncorroborated \
    'is the only workflow naming the trunk'
  seeded_case "parity drifts from what is proven"     parity   inject_parity \
    'which verify\.sh does not prove'
  seeded_case "a signature its own scope line matches" parity  inject_parity_scope_signature \
    'matches the line naming its own scope'
  seeded_case "an unseedable guard's tag dropped"      parity   inject_unseedable_tag_dropped \
    'the guard this declares is untagged, undeclared, or the two have drifted apart'
  seeded_case "a forbidden id in a commit message"    history  inject_history \
    'hygiene: internal-ticket-id:'
  seeded_case "content added and then removed"        history  inject_history_added_then_removed \
    'hygiene: private-ipv4: .*patch-'
  seeded_case "history with an undeterminable base"   history  inject_history_no_base \
    'an undeterminable base is a failure, not an empty scan'
  seeded_case "an ask wired to another class's question" test inject_router_ask_class_untuned \
    'capture::router::tests::each_ask_asks_the_question_its_class_calls_for \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a template without the imperative"     test     inject_router_ask_imperative_dropped \
    'capture::router::tests::every_ask_kind_has_a_template_that_carries_the_imperative \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a census that files a silent call as a fork" test inject_router_census_class_miscounted \
    'capture::router::tests::the_census_files_a_silent_call_as_silent_and_not_as_a_fork \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a route verb answering with a hollow census" test inject_router_route_census_hollow \
    'the_route_verb_answers_with_a_census_and_not_with_prose \.\.\. FAILED' 'test:cli'
  seeded_case "a table row that can never fire"       test     inject_router_table_row_shadowed \
    'capture::router::tests::no_row_of_the_table_can_ever_fire_from_below_an_earlier_one \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a corpus that stops covering a class"  test     inject_router_corpus_class_uncovered \
    'capture::router::tests::the_corpus_covers_every_class_and_every_tool_name_the_router_knows \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a table row skipped, not refused"      test     inject_router_table_row_skipped \
    'capture::router::tests::a_table_row_that_is_not_a_rule_is_refused_and_not_skipped \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "an intent taken from any lane"         test     inject_router_intent_lane_ignored \
    'capture::router::tests::an_ask_quotes_back_what_the_canonical_lane_said_it_would_do \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "the first stated intent, not the last" test     inject_router_intent_first_not_last \
    'capture::router::tests::stated_intent_takes_the_last_sentence_that_states_one \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "an intent marker lost from the table"  test     inject_router_intent_marker_lost \
    'capture::router::tests::every_marker_the_model_states_an_intent_with_is_heard \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "an unclassified call nobody can look up" test   inject_router_unclassified_unattributed \
    'capture::router::tests::an_unclassified_event_names_the_call_its_turn_its_tool_and_its_word \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "the declared default out of the vocabulary" test inject_router_class_vocabulary_shortened \
    'capture::router::tests::every_vocabulary_is_named_in_full_where_the_tests_walk_it \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a quoted substitution descended into"  test     inject_mechanical_quoted_substitution \
    'capture::mechanical::tests::a_quoted_substitution_is_three_characters_and_not_a_command \.\.\. FAILED' 'lib'
  seeded_case "the declared default replaced by silence" test   inject_router_unknown_silent \
    'capture::router::tests::an_unknown_tool_call_routes_to_the_declared_default \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a judgment ask released mid-turn"      test     inject_router_judgment_mid_turn \
    'capture::router::tests::a_judgment_ask_waits_for_the_turn_boundary \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a row of the routing table lost"       test     inject_router_table_row_lost \
    'capture::router::tests::the_producer_of_the_last_pipeline_decides_the_class \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "an unknown call routed but not recorded" test   inject_router_unclassified_silent \
    'capture::router::tests::an_unknown_tool_call_is_a_typed_event \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a reduction claimed, not computed"     test     inject_router_reduction_claimed \
    'capture::router::tests::the_census_reports_the_reduction_as_a_decimal_of_its_own_counts \.\.\. FAILED' 'lib/capture::router::tests'
  seeded_case "a subshell that shares the parent state" test   inject_mechanical_subshell_leaks \
    'capture::mechanical::tests::a_subshells_cd_does_not_leak_into_the_parent \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "a failed cd applied anyway"            test     inject_mechanical_failed_cd_applied \
    'capture::mechanical::tests::a_failed_cd_leaves_the_cwd_where_it_was_and_is_recorded \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "popd on an empty stack ignored"        test     inject_mechanical_popd_empty_ignored \
    'capture::mechanical::tests::popd_on_an_empty_stack_is_a_recorded_failure \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "the mechanical-noun table emptied"     test     inject_mechanical_lint_table_emptied \
    'capture::mechanical::tests::the_lint_catches_every_shape_of_mechanical_question \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "a mechanical entry sent through the gate" test  inject_mechanical_entry_grounded \
    'capture::mechanical::tests::mechanical_entries_are_applied_without_a_lane_report \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "a cosine that forgot its second norm"  test     inject_sense_cosine_unnormalised \
    'capture::sense::tests::cosine_of_a_vector_with_itself_is_one \.\.\. FAILED' 'lib/capture'
  seeded_case "contrastive scoring that ignores the negative sense" test inject_sense_contrastive_ignores_negative \
    'capture::sense::tests::contrastive_scoring_subtracts_the_negative_sense \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a null whose labels are never shuffled" test   inject_sense_null_labels_unshuffled \
    'capture::sense::tests::a_shuffled_label_null_sits_at_chance \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a bootstrap p with no attainable floor" test   inject_sense_p_without_floor \
    'capture::sense::tests::a_p_value_never_travels_without_its_attainable_floor \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a metric whose failure fixture is gone" test   inject_sense_metric_fixture_removed \
    'capture::sense::tests::every_metric_is_reported_only_after_failing_its_own_fixture \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a register mislabelled at its source" test     inject_sense_register_source_mislabelled \
    'capture/sense/register/authored-mistake\.jsonl' 'lib/capture::sense::tests'
  seeded_case "a file in the register naming nothing"  test     inject_sense_register_unnamed_file \
    'capture/sense/register/notaregister\.jsonl' 'lib/capture::sense::tests'
  seeded_case "a mined row nobody can trace"          test     inject_sense_provenance_join_dropped \
    'capture::sense::tests::a_mined_row_without_provenance_is_refused_and_so_is_provenance_without_a_row \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "two data lines read as one"            test     inject_record_data_line_two_lines \
    'formats::record::json::tests::a_data_line_is_one_object_of_the_record_grammar \.\.\. FAILED' 'lib/formats::record::json::tests'
  seeded_case "controls that never look at the register" test inject_sense_controls_ignore_register \
    'capture::sense::tests::a_register_row_that_reaches_a_control_is_named_as_the_row_that_displaced_it \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a bootstrap that never resamples"      test     inject_sense_bootstrap_never_resamples \
    'capture::sense::tests::a_paired_bootstrap_resamples_rather_than_repeating_the_observed_difference \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a failure reading off the worst reading" test   inject_sense_failure_reading_moved \
    'capture::sense::tests::every_metric_names_the_reading_at_which_it_has_failed \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a pre-registration with nothing in it" test     inject_sense_pre_registration_emptied \
    'capture::sense::tests::the_pre_registration_names_its_endpoints_and_its_blockers \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a lexical gate asked about the id"     test     inject_sense_gate_reads_the_id \
    'capture::sense::tests::a_gated_out_row_sits_at_the_scorings_floor \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a separation over one class spread"    test     inject_sense_d_prime_unpooled \
    'capture::sense::tests::d_prime_is_the_standardised_separation_of_the_two_means \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a null band widened past a finding"    test     inject_sense_null_band_widened \
    'capture::sense::tests::the_null_bands_are_wider_than_measurement_and_narrower_than_a_finding \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "a reported metric that reports a constant" test inject_sense_reported_value_constant \
    'capture::sense::tests::the_record_of_a_metric_carries_the_numbers_it_produced \.\.\. FAILED' 'lib/capture::sense::tests'
  seeded_case "the mechanical lane renamed"           test     inject_mechanical_lane_renamed \
    'capture::mechanical::tests::the_lane_is_named_mechanical_and_every_entry_says_so \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "an option word read as a directory"    test     inject_mechanical_option_is_a_directory \
    'capture::mechanical::tests::an_option_word_is_not_a_directory \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "a flag value read as a file"           test     inject_mechanical_flag_value_is_a_file \
    'capture::mechanical::tests::a_flag_value_is_not_a_file_the_turn_touched \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "an entry with the wrong verb"          test     inject_mechanical_entry_verb_swapped \
    'capture::mechanical::tests::an_entry_names_what_happened_to_the_file_it_is_about \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "a pipeline whose members never run"    test     inject_mechanical_pipeline_skipped \
    'capture::mechanical::tests::a_pipeline_and_a_background_chain_still_run_the_commands_in_them \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "a write lost to a read of the same path" test   inject_mechanical_write_lost_to_a_read \
    'capture::mechanical::tests::a_file_written_and_read_by_one_call_is_two_facts_and_a_file_named_twice_is_one \.\.\. FAILED' 'lib/capture::mechanical::tests'
  seeded_case "the resolver keying on a comment"       resolver inject_keyed_by_a_comment \
    'keyed as .*inject_alpha'
  seeded_case "a build failure read as a broken derivation" derive inject_derive_build_failure_unread \
    'the wrecked target read as'
  seeded_case "a scope past its own failure accepted"  derive   inject_derive_accepts_a_fast_green \
    'a scope naming another target selected something'
  seeded_case "a shard split balanced by count"        derive   inject_derive_shards_packs_by_count \
    'not two equal halves'
  seeded_case "an outlier shard graded against the mean" derive inject_derive_shards_outlier_against_the_mean \
    'produced 0 finding\(s\)'
  seeded_case "a shard carrying three times the median" ci      inject_shard_assignment_outlier \
    'x the median shard'
  seeded_case "the wall-clock budget left undeclared"  ci       inject_gate_budget_undeclared \
    'declares no wall_clock_seconds'
  seeded_case "a run directory inside a run directory" results inject_results_nested_directory \
    'a run directory inside a run directory'
  seeded_case "a verdict read by prefix"               test     inject_verdict_prefix_accepted \
    'verdict/fixtures/invalid/verdict-as-a-prefix\.txt' 'test:conformance/formats::verdict'
  seeded_case "an identifier in a reason read as a verdict" test inject_verdict_identifier_read_as_a_verdict \
    'verdict/fixtures/valid/reason-naming-an-identifier\.txt' 'test:conformance/formats::verdict'
  seeded_case "a second verdict in a reason accepted"  test     inject_verdict_second_verdict_accepted \
    'verdict/fixtures/invalid/two-verdicts\.txt' 'test:conformance/formats::verdict'
  seeded_case "an anchor matched inside a longer word"  test     inject_collector_substring_match \
    'capture::collector::literal::tests::a_match_is_at_a_word_boundary \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "an English word made an anchor"         test     inject_collector_english_anchor \
    'capture::collector::literal::tests::an_english_sentence_has_no_anchors \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "an entry nominated by its own turn"     test     inject_collector_self_nomination \
    'capture::collector::literal::tests::an_entry_is_not_nominated_by_the_turn_that_made_it \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "a supersession that adds without voiding" test   inject_reconcile_supersede_without_voiding \
    'capture::collector::reconcile::tests::a_superseded_verdict_voids_the_old_entry_and_links_it_rather_than_deleting \.\.\. FAILED' 'lib/capture::collector::reconcile::tests'
  seeded_case "a verdict that settles nothing settling"  test   inject_reconcile_partial_applies_a_patch \
    'capture::collector::reconcile::tests::a_partial_verdict_applies_nothing \.\.\. FAILED' 'lib/capture::collector::reconcile::tests'
  seeded_case "an uncalibrated nomination policy accepted" test inject_collector_uncalibrated_policy \
    'capture::collector::sense::tests::the_shipped_fixture_policy_is_refused_by_the_door_that_ships \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a nomination budget ignored"           test     inject_collector_budget_ignored \
    'capture::collector::sense::tests::the_budget_is_the_most_a_turn_may_spend \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a turn that nominates only its first entry" test inject_collector_one_nomination_per_turn \
    'capture::collector::literal::tests::every_entry_whose_anchor_recurs_is_nominated \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "a voided entry nominated by tier 0" test inject_collector_voided_entry_renominated \
    'capture::collector::literal::tests::a_voided_entry_is_not_nominated_again \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "a hit that says nothing about where" test inject_collector_hit_offset_lost \
    'capture::collector::literal::tests::a_hit_says_where_the_anchor_recurred \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "an overlapping anchor scan" test inject_collector_overlapping_scan \
    'capture::collector::literal::tests::an_anchor_that_overlaps_itself_is_counted_once \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "a double-quoted span that is not an anchor" test inject_collector_quoted_anchor_delimiter \
    'capture::collector::literal::tests::a_double_quoted_span_is_an_anchor \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "a two-byte shape read as an anchor" test inject_collector_short_shape_anchored \
    'capture::collector::literal::tests::a_shape_below_three_bytes_is_a_coincidence_not_an_anchor \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "a module path that is not an identifier" test inject_collector_module_path_shape \
    'capture::collector::literal::tests::a_path_through_the_module_tree_is_an_anchor \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "a sentence word read as a file extension" test inject_collector_extension_window \
    'capture::collector::literal::tests::a_dotted_word_whose_tail_is_a_word_is_a_name_not_a_file \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "an anchor with the sentence still on it" test inject_collector_token_untrimmed \
    'capture::collector::literal::tests::the_punctuation_prose_hangs_on_a_token_is_not_part_of_it \.\.\. FAILED' 'lib/capture::collector::literal::tests'
  seeded_case "anchor kinds that swapped their names" test inject_collector_anchor_kind_permuted \
    'capture::collector::tests::the_anchor_kind_vocabulary_is_the_words_a_record_carries \.\.\. FAILED' 'lib/capture::collector::tests'
  seeded_case "a source vocabulary with no members" test inject_collector_source_vocabulary_emptied \
    'capture::collector::tests::the_source_vocabulary_is_the_words_a_record_carries \.\.\. FAILED' 'lib/capture::collector::tests'
  seeded_case "a nomination that names the other tier" test inject_collector_tier_name_swapped \
    'capture::collector::tests::a_nomination_names_the_tier_that_made_it \.\.\. FAILED' 'lib/capture::collector::tests'
  seeded_case "registers that swapped their names" test inject_collector_register_permuted \
    'capture::collector::tests::the_register_vocabulary_is_the_words_a_record_carries \.\.\. FAILED' 'lib/capture::collector::tests'
  seeded_case "a lexical pre-gate the tier ignores" test inject_collector_gate_ignored \
    'capture::collector::sense::tests::the_lexical_gate_decides_whether_a_turn_is_scored_at_all \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a turn-level threshold never compared" test inject_collector_turn_threshold_ignored \
    'capture::collector::sense::tests::a_turn_that_is_not_a_reversal_nominates_nothing \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a per-entry threshold never compared" test inject_collector_entry_threshold_ignored \
    'capture::collector::sense::tests::an_entry_the_turn_is_not_about_is_not_nominated \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a turn scored against the wrong sense set" test inject_collector_wrong_sense_set \
    'capture::collector::sense::tests::the_turn_is_scored_against_the_authored_reversal_senses \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "an intent register that reads the prose" test inject_collector_intent_register_reads_the_prose \
    'capture::collector::sense::tests::the_second_register_measures_the_stated_intent_and_not_the_prose \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a budget spent on the worst candidates" test inject_collector_budget_takes_the_worst \
    'capture::collector::sense::tests::the_ranking_a_budget_spends_on_is_best_first \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a nomination score nothing measured" test inject_collector_score_not_measured \
    'capture::collector::sense::tests::a_nomination_carries_the_score_that_was_measured \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "an entry nominated by its own turn at tier 1" test inject_collector_sense_self_nomination \
    'capture::collector::sense::tests::an_entry_is_not_nominated_by_the_turn_that_made_it \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a voided entry nominated by tier 1" test inject_collector_sense_voided_entry_renominated \
    'capture::collector::sense::tests::a_voided_entry_is_not_nominated_again \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a report that drops half its policy" test inject_collector_report_drops_the_policy \
    'capture::collector::sense::tests::the_report_carries_the_policy_that_produced_it \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a calibration that names no run" test inject_collector_policy_names_no_run \
    'capture::collector::sense::tests::a_policy_that_names_no_run_is_refused_and_one_that_names_a_run_is_not \.\.\. FAILED' 'lib/capture::collector::sense::tests'
  seeded_case "a supersession that writes a constant" test inject_reconcile_supersede_writes_a_constant \
    'capture::collector::reconcile::tests::a_superseding_entry_says_what_superseded_the_old_one \.\.\. FAILED' 'lib/capture::collector::reconcile::tests'
  seeded_case "a supersession with no fork behind it" test inject_reconcile_supersede_loses_its_fork \
    'capture::collector::reconcile::tests::a_superseding_entry_says_which_fork_produced_it \.\.\. FAILED' 'lib/capture::collector::reconcile::tests'
  seeded_case "a patch the reconciler never hands back" test inject_reconcile_patch_never_handed_back \
    'capture::collector::reconcile::tests::a_done_verdict_resolves_the_entry_and_hands_its_patch_back \.\.\. FAILED' 'lib/capture::collector::reconcile::tests'
  seeded_case "a PARTIAL counted as a false nomination" test inject_reconcile_partial_read_as_a_false_nomination \
    'capture::collector::reconcile::tests::a_partial_is_not_a_false_nomination \.\.\. FAILED' 'lib/capture::collector::reconcile::tests'
  seeded_case "a mention applied as a supersession" test inject_reconcile_mention_superseded \
    'capture::collector::reconcile::tests::a_not_this_verdict_applies_nothing \.\.\. FAILED' 'lib/capture::collector::reconcile::tests'
  seeded_case "self-capture exempt from grounding"    test     inject_tools_ungrounded \
    'capture::tools::tests::a_self_captured_entry_absent_from_what_the_model_saw_is_dropped \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a reminder cadence that never fires"   test     inject_tools_reminder_silent \
    'capture::tools::tests::ten_silent_turns_are_reminded_on_the_cadence_and_the_sweep_covers_them \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a harness tool call read as a capture" test     inject_tools_foreign_call \
    'capture::tools::tests::a_harness_tool_call_is_not_a_patch_source \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a phase proposal that writes a fact"   test     inject_tools_proposal_writes \
    'capture::tools::tests::a_phase_transition_proposal_is_advisory_and_writes_nothing \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a capture grounded in its own echo"   test     inject_tools_self_echo \
    'capture::tools::tests::a_capture_is_not_grounded_in_a_harness_repeating_it_back \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a capture grounded in a later turn"   test     inject_tools_future_output \
    'capture::tools::tests::a_capture_is_not_grounded_in_a_turn_the_model_had_not_reached \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a superseding entry with a minted id" test     inject_tools_supersede_minted \
    'capture::tools::tests::naming_what_it_replaces_makes_the_capture_a_supersede \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a verdict that resolves elsewhere"    test     inject_tools_resolve_elsewhere \
    'capture::tools::tests::only_a_settled_entry_is_resolved_by_a_verdict_alone \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "the asks reworded to nothing"         test     inject_tools_ask_words \
    'capture::tools::tests::every_ask_kind_is_named_and_asks_in_its_own_words \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "an ask that drops its own question"   test     inject_tools_ask_question_dropped \
    'capture::tools::tests::the_sweep_carries_what_the_router_put_off \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "the sweep asking as the cadence"      test     inject_tools_sweep_kind \
    'capture::tools::tests::the_sweep_asks_as_a_sweep_and_not_as_the_cadence \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a closed choice left in its own case" test     inject_tools_choice_uncanonical \
    'capture::tools::tests::an_argument_in_the_case_the_harness_used_is_settled_by_the_contract \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a phase proposal with no reason"      test     inject_tools_proposal_reasonless \
    'capture::tools::tests::a_phase_transition_proposal_carries_the_ruling_it_asks_for \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a reminder that drops the deferral"   test     inject_tools_reminder_deferral_dropped \
    'capture::tools::tests::a_fired_reminder_carries_its_kind_and_what_the_router_put_off \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "tools described in one character"     test     inject_tools_description_thin \
    'capture::tools::tests::every_tool_is_offered_in_words_rather_than_ordered \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "an offered tool the contract omits"   test     inject_tools_contract_undescribed \
    'capture::tools::tests::a_contract_that_omits_an_offered_tool_is_refused \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "one tool described twice"             test     inject_tools_contract_duplicate \
    'capture::tools::tests::a_contract_that_describes_one_tool_twice_is_refused \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a verdict the lane cannot honour"     test     inject_tools_verdict_list_open \
    'capture::tools::tests::every_verdict_the_contract_admits_is_one_this_lane_can_honour \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "a turn swept after it recorded"       test     inject_tools_silent_kept \
    'capture::tools::tests::a_turn_that_records_after_it_looked_silent_is_not_swept \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "the object reader with no limit"      test     inject_record_objects_undepthed \
    'formats::record::tests::a_deeply_nested_object_document_is_a_verdict_and_not_a_crash \.\.\. FAILED' 'lib/formats::record::tests'
  seeded_case "a corpus that drives one tool"       test     inject_tools_corpus_one_tool \
    'capture::tools::tests::the_corpus_drives_every_tool_this_lane_offers \.\.\. FAILED' 'lib/capture::tools::tests'
  seeded_case "the ablation's control arm dropped"     test     inject_ablation_no_control \
    'capture::ablation::tests::the_arms_include_the_control_with_no_imperative_at_all \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "silence counted as engagement"          test     inject_ablation_silence_engages \
    'capture::ablation::tests::silence_and_engagement_are_never_both_true \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "a p reported without its floor"         test     inject_ablation_p_floor_dropped \
    'capture::ablation::tests::every_reported_p_carries_its_attainable_floor \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "a resample that draws once"            test     inject_ablation_resample_single_draw \
    'capture::ablation::tests::a_resample_draws_one_outcome_for_every_fork_there_is \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "a generator that never advances"       test     inject_ablation_frozen_generator \
    'capture::ablation::tests::the_generator_advances_so_a_resample_is_not_one_fork_counted_over \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "each arm credited with the other's"    test     inject_ablation_rates_swapped \
    'capture::ablation::tests::each_arms_rate_counts_that_arms_own_outcomes \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "the silence endpoint unregistered"     test     inject_ablation_endpoint_dropped \
    'capture::ablation::tests::both_endpoints_are_pre_registered_in_the_direction_each_is_read_in \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "one imperative for every arm"          test     inject_ablation_plan_one_imperative \
    'capture::ablation::tests::the_plan_pairs_every_arm_with_its_own_imperative_and_says_none_has_run \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "two arms under one name"               test     inject_ablation_arm_names_collide \
    'capture::ablation::tests::every_arm_is_reported_under_a_name_no_other_arm_answers_to \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "an arm's clauses run together"         test     inject_ablation_clauses_run_together \
    'capture::ablation::tests::the_full_arm_renders_the_sentence_this_ablation_takes_apart \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "a placeholder nobody counts"           test     inject_ablation_placeholder_word_dropped \
    'capture/ablation/corpus/the-placeholder-words-nobody-replaced\.answer\.txt' 'lib/capture::ablation::tests'
  seeded_case "a blank clause text admitted"          test     inject_ablation_blank_clause_allowed \
    'capture::ablation::tests::a_clause_table_that_breaks_the_schema_says_which_rule_it_broke \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "a grading case quietly dropped"        test     inject_ablation_corpus_case_dropped \
    'capture::ablation::tests::the_corpus_pairs_every_case_with_its_expectation \.\.\. FAILED' 'lib/capture::ablation::tests'
  seeded_case "an untagged decline read as content"   test     inject_ablation_untagged_decline_engages \
    'capture/ablation/corpus/an-untagged-decline\.answer\.txt' 'lib/capture::ablation::tests'
  seeded_case "the second reader left unbounded"      test     inject_json_objects_unbounded \
    'formats::record::json::tests::a_line_nested_past_the_limit_is_a_verdict_and_not_a_crash \.\.\. FAILED' 'lib/formats::record::json::tests'
  seeded_case "a lane free to change substrate"       test     inject_record_lane_may_change_substrate \
    'record/fixtures/invalid/lane-changes-substrate\.jsonl' 'test:conformance/formats::record'
  seeded_case "a substrate reference inferred from a count" test inject_record_substrate_reference_inferred_from_a_count \
    'record/fixtures/invalid/one-substrate-and-a-request-elsewhere\.jsonl' 'test:conformance/formats::record'
  seeded_case "a fork that names no substrate"         test     inject_record_a_fork_names_no_substrate \
    'record/fixtures/invalid/fork-names-an-undeclared-substrate\.jsonl' 'test:conformance/formats::record'
  seeded_case "a canned substrate that need not say which acts" test inject_record_canned_acts_optional \
    'record/fixtures/invalid/canned-with-no-acts-digest\.jsonl' 'test:conformance/formats::record'

  echo
  echo "--- results fixtures, checked directly ---"
  # The linter dispatches the record verdict to diet through the resolver, so
  # it needs exactly one fresh build where the resolver will look -- the same
  # thing check_results does before it runs the linter for real.
  ( cd "${ROOT}" && build_diet ) || SELFTEST_BROKEN+=("results fixtures: diet did not build")
  # Graded on the failure class the manifest DECLARES for each fixture, not on
  # "exited 1". Every one of these fixtures carries exactly one defect and the
  # manifest names the class it must produce; grading on the exit code alone
  # cannot tell a fixture that failed for its own reason from one that failed
  # for a reason nobody checked. That is not a hypothetical -- adding `kind` to
  # REQUIRED_KEYS made all thirty go red on a missing key while twenty-four of
  # their declared defects went unexercised, and this loop reported thirty REDs
  # throughout. Red for the wrong reason is the WRONG verdict in a green shirt.
  local dir rc name want out
  local -A WANT=()
  while IFS=$'\t' read -r name want; do
    [ -n "$name" ] && WANT["$name"]="$want"
  done < <(cd "${ROOT}" && python3 scripts/check-fault-manifest.py --fixture-classes)
  local started_ms
  for dir in "${ROOT}"/tests/fixtures/results-bad/*/; do
    # Named before the shard is consulted: `results.<directory>` is the id
    # check-fault-manifest.py registers this fixture under, and the lookup
    # needs it.
    name="$(basename "$dir")"
    in_shard "results.${name}" || continue
    now_ms; started_ms="$NOW_MS"
    rc=0
    out="$(python3 "${ROOT}/scripts/check-results.py" "$dir" 2>&1)" || rc=$?
    want="${WANT[$name]-}"
    if [ -z "$want" ]; then
      printf 'GREEN %-52s <-- NO FAULT DECLARED FOR THIS FIXTURE\n' "$name"
      SELFTEST_BROKEN+=("results fixture $name: declared by no fault")
    elif [ "$rc" -ne 1 ]; then
      printf 'GREEN exit %-3d %-46s <-- FIXTURE DID NOT FAIL\n' "$rc" "$name"
      SELFTEST_BROKEN+=("results fixture $name")
    elif ! grep -qF "[$want]" <<<"$out"; then
      printf 'GREEN exit %-3d %-46s <-- RED FOR THE WRONG REASON, wanted %s\n' \
        "$rc" "$name" "$want"
      SELFTEST_BROKEN+=("results fixture $name: red, but not $want")
    else
      printf 'RED   exit %-3d %-46s %s\n' "$rc" "$name" "$want"
    fi
    shard_cost "results.${name}" "$started_ms" "results fixture ${name}"
  done

  # LANE-DECLARED FAULTS, ONE `seeded_case` PER FAULT, GENERATED RATHER THAN
  # WRITTEN OUT. `apply-lane-faults.py --list` is the one reader of the lane
  # manifests and the root registry; a hand-maintained list of 123 rows here
  # would be exactly the thing this repository refuses everywhere else --
  # a list nobody can re-derive is a list that was right when it was
  # written. `LANE_FAULT_LANE`/`LANE_FAULT_ID` are read by `inject_lane_fault`
  # in the subshell `seeded_case` runs it in.
  #
  # The signature is the fault's OWN FIRST `catches` NAME, read out of the
  # manifest -- never typed here -- so a case cannot go WRONG by drifting
  # from prose nobody re-checks against a run. `check` is always `lanes`:
  # every lane fault is proven through the one check that runs every
  # registered lane's own command, whichever lane the fault belongs to.
  # The signature is used verbatim as an ERE against the fault's own log --
  # not escaped, because every one is a Rust path (`mod::mod::tests::name`),
  # and a colon or underscore is not a metacharacter. The seeded fixtures
  # below prove the wiring; the shape of the data is what makes that safe.
  # `sc_inject`, NOT the literal `inject_lane_fault`, at the call site below.
  # `gatelib.seeded_cases()` -- the shared static reader `check-fault-
  # manifest.py` and `check-injections.py` both trust -- finds a case by
  # `shlex.split`-ing each line and checking whether its THIRD WORD starts
  # with `inject_`, with no bash evaluation at all. A literal `inject_
  # lane_fault` in this loop's own source line would satisfy that test on
  # the GENERATOR's one line of code, adding a phantom `lanes.lane_fault`
  # seeded-gate case nothing runs and the manifest cannot describe -- caught
  # by running `check-fault-manifest.py` against this loop's first draft, not
  # foreseen. A variable reference is identical at RUN TIME, since bash
  # expands it before `seeded_case` ever sees the value, and invisible to a
  # reader that never evaluates anything.
  #
  # `sc_call`, NOT the literal `seeded_case`, for the same reason one layer
  # up. `scripts/check-merge-gate.py` holds a second, cruder census of its
  # own -- "the shared reader agrees with verify.sh's own count of cases" --
  # that counts every line STARTING WITH THE WORD `seeded_case` and compares
  # that count against how many `gatelib.seeded_cases()` actually parses, to
  # catch a reader that silently drops a real case. This loop's one call
  # site is not a real case to either reader (its arguments are template
  # variables, not the case gatelib rejects above), but its source line
  # still spelled the literal word `seeded_case`, so the crude counter
  # counted it while the careful one, correctly, did not -- 250 spelled
  # against 249 parsed, caught by `check-merge-gate.py`'s own fixture, not
  # foreseen either. Indirecting the call word too removes it from both
  # counts equally, which is the only count this generator should ever be
  # in: zero.
  sc_call="seeded_case"
  sc_inject="inject_lane_fault"
  while IFS=$'\t' read -r lane fault_id signature failure_class || [ -n "${lane:-}" ]; do
    [ -n "$lane" ] || continue
    LANE_FAULT_LANE="$lane" LANE_FAULT_ID="$fault_id"
    # `fault_id` is already lane-prefixed (every gate.toml declares it that
    # way), so `${lane}.` here duplicated it -- "lane: isolation.isolation.…"
    # -- a fresh-instance review of #83 found this, cosmetic but repeated
    # across all 123 generated cases.
    # The sixth word is the MANIFEST ID, and this is the one caller that must
    # pass one: every generated case shares `lanes` and `inject_lane_fault`,
    # so the id seeded_case would otherwise derive names every one of them
    # `lanes.lane_fault` and the shard assignment would carry a single row for
    # the whole lane corpus. The fifth word is the scope, which a `lanes` case
    # never has; it is spelled empty rather than omitted because bash positions
    # arguments and does not name them.
    "$sc_call" "lane: ${fault_id}" lanes "$sc_inject" "$signature" "" "$fault_id"
  done < <(python3 "${ROOT}/scripts/apply-lane-faults.py" --list)

  prove_mechanics

  # --- the linter dispatches; it does not judge the format ---
  #
  # A record diet refuses must surface as diet's verdict and nothing else:
  # no number check, no digest check, no summary-row count from this side.
  # Any of those appearing would mean the Python side read the record and
  # reached a verdict of its own, which is the second reader this exists to
  # remove.
  local relay; scratch; relay="$SCRATCH"
  cp -r "${ROOT}/results/_template" "${relay}/2026-01-30-no-substrate"
  strip_substrates "${relay}/2026-01-30-no-substrate/run.jsonl"
  expect_exit "a record diet refuses gets no verdict from the linter" 0 \
    bash -c "cd '${ROOT}' && cargo build --quiet -p discipline-diet --bin diet \
      && out=\$(python3 scripts/check-results.py '${relay}/2026-01-30-no-substrate' 2>&1; true) \
      && grep -q 'says diet check-record: a .start. row is missing its required .substrates.' <<<\"\$out\" \
      && ! grep -qE 'front-matter|summary row|product_sha256' <<<\"\$out\" \
      && grep -q 'record verdicts from .* sha256=' <<<\"\$out\""

  # --- #50: the three defects a Mac seat found, each asserted here ---
  #
  # Every one of them was a verdict that depended on the machine or on the
  # operation, and every one was invisible on the machine that had the
  # forgiving behaviour. That is the class these assertions exist for.

  # A force-push orphans the sha the push payload names as `before`, so it is
  # absent from CI's fresh checkout. That is the EXPECTED shape of a
  # legitimate operation -- this repository's own rules mandate rebases, so
  # the check was guaranteed to redden the lane that gates the merge on a
  # branch whose content is fine.
  # IN A REPOSITORY OF THEIR OWN, not in this one. These ran against `$ROOT`
  # and passed on every full clone; `actions/checkout` takes a SHALLOW one, so
  # in CI `HEAD~1` did not resolve and `origin/main` did not exist, and both
  # assertions failed for reasons that had nothing to do with what they test.
  # An assertion whose subject is "the checkout you happen to have" is the
  # same defect as the one it was written for -- a verdict that depends on the
  # machine -- so it builds the shape it needs instead.
  local pushes; scratch; pushes="$SCRATCH"
  (
    cd "$pushes" && git init -q .
    git -c user.email=gate@example.invalid -c user.name=gate \
      commit -q --allow-empty -m "a trunk commit"
    git update-ref refs/remotes/origin/main HEAD
    git -c user.email=gate@example.invalid -c user.name=gate \
      commit -q --allow-empty -m "one"
    git -c user.email=gate@example.invalid -c user.name=gate \
      commit -q --allow-empty -m "two"
  ) > /dev/null 2>&1
  cp -r "${ROOT}/scripts" "${pushes}/scripts"
  printf '{"before":"0123456789012345678901234567890123456789","after":"%s"}' \
    "$(git -C "$pushes" rev-parse HEAD)" > "${pushes}/force.json"
  printf '{"before":"%s","after":"%s"}' \
    "$(git -C "$pushes" rev-parse HEAD~1)" "$(git -C "$pushes" rev-parse HEAD)" \
    > "${pushes}/ordinary.json"
  expect_exit "a force-push is scanned, not refused" 0 \
    bash -c "cd '${pushes}' && GITHUB_ACTIONS=true GITHUB_EVENT_NAME=push \
      GITHUB_EVENT_PATH='${pushes}/force.json' python3 scripts/check-history.py \
      | grep -q 'push, force-push: merge-base..after'"
  expect_exit "an ordinary push still scans before..after" 0 \
    bash -c "cd '${pushes}' && GITHUB_ACTIONS=true GITHUB_EVENT_NAME=push \
      GITHUB_EVENT_PATH='${pushes}/ordinary.json' python3 scripts/check-history.py \
      | grep -q 'push before..after'"

  # A base that is genuinely undeterminable is still a failure. The fallback
  # above must not have turned "I cannot tell what to scan" into "scan the
  # trunk", which would be an empty scan wearing a verdict.
  local lonely; scratch; lonely="$SCRATCH"
  ( cd "$lonely" && git init -q . \
    && git -c user.email=gate@example.invalid -c user.name=gate \
         commit -q --allow-empty -m "only commit" ) > /dev/null 2>&1
  cp -r "${ROOT}/scripts" "${lonely}/scripts"
  printf '{"before":"0123456789012345678901234567890123456789","after":"HEAD"}' \
    > "${lonely}/event.json"
  expect_exit "no trunk to fall back to is still undeterminable" 2 \
    bash -c "cd '${lonely}' && GITHUB_ACTIONS=true GITHUB_EVENT_NAME=push \
      GITHUB_EVENT_PATH='${lonely}/event.json' \
      GITHUB_SHA=\$(git rev-parse HEAD) python3 scripts/check-history.py"

  # `resolve-diet` decides staleness by asking the source what it EMBEDS. A
  # grammar and a dogma template are compiled in; a conformance fixture and a
  # register corpus are data the binary reads at test time. Before this, the
  # whole of `diet/` counted, so editing a fixture made `--only regimen` exit
  # 2 until the binary was relinked -- and `cargo build` did not clear it,
  # because cargo correctly rebuilds nothing.
  #
  # EACH PUTS THE TREE BACK. These two touch a real file and then ask about
  # the real binary, so an assertion that left the binary stale would hand
  # every assertion after it a tree it did not make -- which is what happened:
  # `a pin spelled with ./ is the same pin` read exit 2 from a grammar this
  # block had touched three lines earlier.
  expect_exit "a grammar makes the binary stale" 2 \
    bash -c "cd '${ROOT}' && touch diet/src/lib.rs && cargo build --quiet --bin diet \
      && touch diet/formats/record/grammar.pest \
      ; python3 scripts/resolve-diet.py > /dev/null 2>&1; rc=\$? \
      ; touch diet/src/lib.rs && cargo build --quiet --bin diet; exit \$rc"
  expect_exit "a conformance fixture does not" 0 \
    bash -c "cd '${ROOT}' && touch diet/src/lib.rs && cargo build --quiet --bin diet \
      && touch diet/formats/record/fixtures/valid/minimal.expected.json \
      ; python3 scripts/resolve-diet.py > /dev/null 2>&1; rc=\$? \
      ; touch diet/src/lib.rs && cargo build --quiet --bin diet; exit \$rc"

  # A BINARY WHOSE DEP-INFO CANNOT SAY WHAT IT EMBEDS IS NOT THEREBY FRESH.
  #
  # The narrowing above reads `target/debug/diet.d` for the embedded set.
  # When there is none to read, `embedded()` used to return an empty list
  # under a comment calling that "a smaller list, not a wrong one". Both
  # halves of its reasoning were true and the conclusion was not: the answer
  # this feeds is a NEGATIVE one -- nothing is newer than the binary -- and a
  # list with the grammars missing produces that answer for a binary whose
  # grammars have changed.
  #
  # Every binary pinned through `DIET_BIN` carries no `.d`, so the embedded
  # half of rule three was switched off for exactly the case the rule was
  # written about: a build handed in from somewhere else. It now falls back
  # to the whole of `diet/`.
  #
  # Synthetic roots, not this one. The question is what the resolver does
  # with a tree it cannot narrow, and building that state here would mean
  # deleting dep-info from a real build the rest of this run depends on.
  local depless; scratch; depless="$SCRATCH"
  python3 - "$depless" <<'PYEOF'
import os
import pathlib
import sys

root = pathlib.Path(sys.argv[1])
# Fixed stamps rather than offsets from now, so the three cases differ by
# years and no clock skew can reorder them.
OLD, BUILT, NEW = 1577836800, 1609459200, 1767225600  # 2020, 2021, 2026
CASES = ("a-grammar-it-embeds", "nothing-newer", "dep-info-narrows")
TREE = (
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "diet/Cargo.toml",
    "diet/src/lib.rs",
    "diet/formats/record/grammar.pest",
    "diet/formats/record/fixtures/one.json",
)


def stamp(path, when):
    os.utime(path, (when, when))


for case in CASES:
    here = root / case
    for name in TREE:
        path = here / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("x\n", encoding="utf-8")
        stamp(path, OLD)
    binary = here / "diet-bin"
    binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
    binary.chmod(0o755)
    stamp(binary, BUILT)

# THE CASE: a grammar the binary embeds, changed after it was built. There is
# no dep-info to say it is embedded, and it is under `diet/`.
stamp(root / CASES[0] / "diet/formats/record/grammar.pest", NEW)

# THE CONTROL is `nothing-newer`, left exactly as built above. It is what
# keeps the case about a STALE binary rather than about pinning one: a
# fallback that refused every dep-info-less binary would satisfy the first
# assertion and break every pinned build.

# AND THE NARROWING, still applying wherever cargo did write a list. This
# `.d` names `diet/src/lib.rs` and nothing else, and the file made newer is
# a test fixture the binary does not embed. Widening when there is no list
# must not become widening when there is one.
narrows = root / CASES[2]
(narrows / "diet-bin.d").write_text(
    f"{narrows / 'diet-bin'}: diet/src/lib.rs\n", encoding="utf-8"
)
stamp(narrows / "diet-bin.d", BUILT)
stamp(narrows / "diet/formats/record/fixtures/one.json", NEW)
PYEOF
  expect_exit "a grammar changed under a binary with no dep-info is stale" 2 \
    bash -c "cd '${depless}/a-grammar-it-embeds' \
      && DIET_BIN='${depless}/a-grammar-it-embeds/diet-bin' \
      python3 '${ROOT}/scripts/resolve-diet.py'"
  expect_exit "and a binary with no dep-info over an untouched tree resolves" 0 \
    bash -c "cd '${depless}/nothing-newer' \
      && DIET_BIN='${depless}/nothing-newer/diet-bin' \
      python3 '${ROOT}/scripts/resolve-diet.py'"
  expect_exit "a dep-info that does narrow still narrows" 0 \
    bash -c "cd '${depless}/dep-info-narrows' \
      && DIET_BIN='${depless}/dep-info-narrows/diet-bin' \
      python3 '${ROOT}/scripts/resolve-diet.py'"

  # --- the resolver's own suite cannot report a pass it did not measure ---
  #
  # 0 from `check-merge-gate.py` is the only thing standing between a lane
  # merge and a resolver that guesses, and 0 is indistinguishable from "not
  # asked" to CI, to verify.sh, and to a person reading a log. Both of the
  # ways it can fail to ask are exercised here, because the suite that guards
  # the resolver was itself guarded by nothing.
  local suite; scratch; suite="$SCRATCH"
  cat > "${suite}/empty.py" <<'PYEOF'
import importlib.util, pathlib, sys
spec = importlib.util.spec_from_file_location("cm", sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.FIXTURES.clear()
sys.exit(module.main())
PYEOF
  expect_exit "a resolver suite that collected nothing is not a pass" 2 \
    python3 "${suite}/empty.py" "${ROOT}/scripts/check-merge-gate.py"
  # ...and the control, which is what makes the assertion above about the
  # EMPTY list rather than about the driver: the same driver, without the
  # clear, runs the real suite and passes.
  sed 's/^module.FIXTURES.clear()$//' "${suite}/empty.py" > "${suite}/full.py"
  expect_exit "and the same driver, with the fixtures left in place, passes" 0 \
    python3 "${suite}/full.py" "${ROOT}/scripts/check-merge-gate.py"
  # Every fixture drives a real repository, so no git is no verdict.
  mkdir -p "${suite}/nogit"
  ln -sf "$(command -v python3)" "${suite}/nogit/python3"
  expect_exit "a resolver suite with no git has no verdict to give" 2 \
    env PATH="${suite}/nogit" python3 "${ROOT}/scripts/check-merge-gate.py"

  # --- binary provenance at the boundary ---
  #
  # The resolver's whole job is refusing to guess, so each refusal is asserted
  # against the exit code it must produce -- and the pin is asserted to be the
  # binary actually used, because a pin that can be silently ignored is not a
  # pin, and that is precisely how four instruments banked numbers through a
  # build seven days behind their source.
  local pin; scratch; pin="$SCRATCH"
  printf '#!/bin/sh\nexit 0\n' > "${pin}/diet"
  chmod +x "${pin}/diet"

  expect_exit "a pinned DIET_BIN is the binary that is used" 0 \
    env DIET_BIN="${pin}/diet" python3 "${ROOT}/scripts/resolve-diet.py" --expect "${pin}/diet"
  expect_exit "a DIET_BIN that is ignored is a failed test, not a refusal" 1 \
    env DIET_BIN="${pin}/diet" python3 "${ROOT}/scripts/resolve-diet.py" \
      --expect "${pin}/some-other-build"
  expect_exit "a DIET_BIN naming nothing is not a fallback" 2 \
    bash -c "cd '${pin}' && CARGO_TARGET_DIR= DIET_BIN='${pin}/never-built' \
      python3 '${ROOT}/scripts/resolve-diet.py'"

  local builds; scratch; builds="$SCRATCH"
  mkdir -p "${builds}/diet/src" "${builds}/target/debug" "${builds}/target/release"
  printf 'fn main() {}\n' > "${builds}/diet/src/main.rs"
  printf '#!/bin/sh\nexit 0\n' > "${builds}/target/debug/diet"
  printf '#!/bin/sh\nexit 0\n' > "${builds}/target/release/diet"
  chmod +x "${builds}/target/debug/diet" "${builds}/target/release/diet"
  expect_exit "two builds are a refusal, not a choice" 2 \
    bash -c "cd '${builds}' && CARGO_TARGET_DIR= python3 '${ROOT}/scripts/resolve-diet.py'"

  rm -f "${builds}/target/release/diet"
  expect_exit "one build resolves" 0 \
    bash -c "cd '${builds}' && CARGO_TARGET_DIR= python3 '${ROOT}/scripts/resolve-diet.py'"

  # ...and the same one build, once the source is newer than it.
  touch "${builds}/diet/src/main.rs"
  expect_exit "a build older than its source does not reflect it" 2 \
    bash -c "cd '${builds}' && CARGO_TARGET_DIR= python3 '${ROOT}/scripts/resolve-diet.py'"

  expect_exit "no build at all is a refusal, not an empty scan" 2 \
    bash -c "cd '${pin}' && CARGO_TARGET_DIR= DIET_BIN= python3 '${ROOT}/scripts/resolve-diet.py'"

  # Where cargo actually builds. Reading only CARGO_TARGET_DIR meant a build
  # directed elsewhere left `target/debug/diet` free for anything to occupy,
  # and the gate ran that instead -- provenance and all.
  mkdir -p "${builds}/target/debug" "${builds}/elsewhere/debug"
  printf '#!/bin/sh\nexit 9\n' > "${builds}/target/debug/diet"
  printf '#!/bin/sh\nexit 0\n' > "${builds}/elsewhere/debug/diet"
  chmod +x "${builds}/target/debug/diet" "${builds}/elsewhere/debug/diet"
  touch "${builds}/target/debug/diet" "${builds}/elsewhere/debug/diet"
  expect_exit "CARGO_BUILD_TARGET_DIR is where cargo built" 0 \
    bash -c "cd '${builds}' && CARGO_TARGET_DIR= \
      CARGO_BUILD_TARGET_DIR='${builds}/elsewhere' \
      python3 '${ROOT}/scripts/resolve-diet.py' --expect '${builds}/elsewhere/debug/diet'"
  mkdir -p "${builds}/.cargo"
  printf '[build]\ntarget-dir = "elsewhere"\n' > "${builds}/.cargo/config.toml"
  expect_exit "a config target-dir is where cargo built" 0 \
    bash -c "cd '${builds}' && CARGO_TARGET_DIR= \
      python3 '${ROOT}/scripts/resolve-diet.py' --expect '${builds}/elsewhere/debug/diet'"
  rm -rf "${builds}/.cargo"

  # Rule three is stated without conditions. Off a tree that holds the
  # sources, the scan found nothing and the rule quietly did not apply: a
  # stale binary resolved with `"checked_against": null` and exit 0.
  local nowhere; scratch; nowhere="$SCRATCH"
  expect_exit "a staleness check with no sources to check is a refusal" 2 \
    bash -c "cd '${nowhere}' && CARGO_TARGET_DIR= DIET_BIN='${pin}/diet' \
      python3 '${ROOT}/scripts/resolve-diet.py'"

  # A pin honoured is not a pin ignored, whatever spelling it arrived in.
  #
  # WITH A SOURCE TO CHECK IT AGAINST. This used to run in a directory whose
  # only content was a file named `diet`, and the staleness check took THAT as
  # the crate -- `diet/` was a source root, `is_file()` matched, and the binary
  # was compared against itself. The assertion passed for a reason that had
  # nothing to do with pins, and it passed right beside a sibling asserting
  # that a directory with no sources is a REFUSAL. It surfaced when `diet/`
  # stopped being a source root (#50); it was resting on the coincidence the
  # whole time.
  local spelled; scratch; spelled="$SCRATCH"
  mkdir -p "${spelled}/diet/src" "${spelled}/build"
  : > "${spelled}/diet/src/lib.rs"
  : > "${spelled}/Cargo.toml"
  printf '#!/bin/sh\nexit 0\n' > "${spelled}/build/diet"
  chmod +x "${spelled}/build/diet"
  expect_exit "a pin spelled with ./ is the same pin" 0 \
    bash -c "cd '${spelled}' && CARGO_TARGET_DIR= DIET_BIN=./build/diet \
      python3 '${ROOT}/scripts/resolve-diet.py' --expect ./build/diet"

  expect_exit "a file that cannot be run is not a build" 2 \
    bash -c "cd '${builds}' && CARGO_TARGET_DIR= DIET_BIN='${builds}/diet/src/main.rs' \
      python3 '${ROOT}/scripts/resolve-diet.py'"

  # --- the shard census ---
  #
  # Sharding the selftest divides the labour; the one way it could become
  # fault SELECTION is a shard that ran less than it was assigned and exited 0
  # like every other one. No shard can detect that about itself, so the claim
  # is checked afterwards from the ordinals each shard writes down. These are
  # the states that must be refused, and the first is the state that must not.
  local census; scratch; census="$SCRATCH"
  python3 - "$census" \
    "$(python3 "${ROOT}/scripts/check-fault-manifest.py" --count-selftest-red)" <<'EOF'
import pathlib
import sys

root, total = pathlib.Path(sys.argv[1]), int(sys.argv[2])
SHARDS = 4


def share(shard, n=None, of=SHARDS):
    n = total if n is None else n
    return [i for i in range(1, n + 1) if (i - 1) % of + 1 == shard]


def write(case, shard, ordinals, said_total=None, said_shards=None):
    # Each shard's artifact arrives in a directory of its own, as
    # download-artifact leaves them.
    where = root / case / f"shard-{shard}"
    where.mkdir(parents=True, exist_ok=True)
    (where / "census.tsv").write_text(
        f"shard\t{shard}\n"
        f"shards\t{SHARDS if said_shards is None else said_shards}\n"
        f"total\t{total if said_total is None else said_total}\n"
        + "".join(f"ordinal\t{n}\n" for n in ordinals),
        encoding="utf-8",
    )


# The state where download-artifact found nothing: no shard reported at all.
(root / "none").mkdir(parents=True, exist_ok=True)

for k in range(1, SHARDS + 1):
    write("whole", k, share(k))
    write("skipped", k, share(k)[1:] if k == 2 else share(k))
    if k != 3:
        write("missing", k, share(k))
    write("wrong-total", k, share(k, n=total - 1), said_total=total - 1)
    write("twice", k, share(k) + ([share(2)[0]] if k == 1 else []))
    # HOW MANY SHARDS THERE ARE is the shards' own claim, so they must agree
    # on it. One shard believing it is one of five while the rest believe four
    # means they partitioned against different divisors -- the ordinals can
    # still add up, and the run still proved less than it says.
    write("disagree-count", k, share(k), said_shards=(SHARDS + 1 if k == 2 else SHARDS))
    # A COMPLETE PARTITION PLUS A FAULT THAT DOES NOT EXIST. Every ordinal in
    # 1..total is accounted for, so every count is right; one shard also
    # reports running something outside the list. A census that can report an
    # ordinal nobody assigned is a census whose numbers are not about the
    # manifest.
    write("stray-ordinal", k, share(k) + ([total + 9999] if k == 1 else []))
    write("impossible-shard", k, share(k))

# A CENSUS CLAIMING AN ORDINAL NO SHARD COULD HAVE. The four real shards
# partition the list correctly; a fifth file says it is shard nine of four.
# Written after the loop because it is an extra file rather than a variant of
# one, and `shard-9` is not in `range(1, SHARDS + 1)`.
(root / "impossible-shard" / "shard-9").mkdir(parents=True, exist_ok=True)
(root / "impossible-shard" / "shard-9" / "census.tsv").write_text(
    f"shard\t9\nshards\t{SHARDS}\ntotal\t{total}\n", encoding="utf-8"
)
EOF
  expect_exit "shards that between them ran every fault are a whole" 0 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/whole"
  expect_exit "a shard that skipped a fault it was assigned" 1 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/skipped"
  expect_exit "a shard that filed no census at all" 1 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/missing"
  expect_exit "a census that adds up to the wrong manifest" 1 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/wrong-total"
  expect_exit "one fault claimed by two shards" 1 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/twice"
  expect_exit "no shard reporting at all is not a pass" 1 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/none"
  expect_exit "shards that disagree about how many there are" 1 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/disagree-count"
  expect_exit "an ordinal outside the manifest's range" 1 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/stray-ordinal"
  expect_exit "a census claiming to be a shard that cannot exist" 1 \
    python3 "${ROOT}/scripts/check-selftest-census.py" "${census}/impossible-shard"

  # THE LANE-GATE APPLIER'S OWN THREE RULES, ruled on #76: a registry entry
  # naming no manifest, a manifest on disk with no registry entry, and (its
  # own guard against a truly EMPTY registry, since a registry of nothing
  # pins nothing and would call any tree clean). Over a SYNTHETIC root, never
  # this repository's own -- the point is the mechanism, and the real
  # `tools/gate/lanes.toml` is exercised for real by `check_lanes` on every
  # ordinary run.
  local lanes_root; scratch; lanes_root="$SCRATCH"
  mkdir -p "${lanes_root}/present/tools/gate" "${lanes_root}/present/diet/alpha"
  cat > "${lanes_root}/present/tools/gate/lanes.toml" <<'EOF'
[lanes]
alpha = "diet/alpha/gate.toml"
EOF
  cat > "${lanes_root}/present/diet/alpha/gate.toml" <<'EOF'
[package]
name = "synthetic-alpha"
check = "test"
command = "true"
faults = 0
EOF
  expect_exit "a lane registry naming one present manifest is clean" 0 \
    python3 "${ROOT}/scripts/apply-lane-faults.py" --root "${lanes_root}/present" --verify

  mkdir -p "${lanes_root}/absent/tools/gate"
  cat > "${lanes_root}/absent/tools/gate/lanes.toml" <<'EOF'
[lanes]
alpha = "diet/nowhere/gate.toml"
EOF
  expect_exit "a registry entry naming no manifest is a finding" 1 \
    python3 "${ROOT}/scripts/apply-lane-faults.py" --root "${lanes_root}/absent" --verify

  mkdir -p "${lanes_root}/orphan/tools/gate" "${lanes_root}/orphan/diet/alpha" \
    "${lanes_root}/orphan/diet/beta"
  cp "${lanes_root}/present/tools/gate/lanes.toml" "${lanes_root}/orphan/tools/gate/lanes.toml"
  cp "${lanes_root}/present/diet/alpha/gate.toml" "${lanes_root}/orphan/diet/alpha/gate.toml"
  cp "${lanes_root}/present/diet/alpha/gate.toml" "${lanes_root}/orphan/diet/beta/gate.toml"
  expect_exit "a manifest on disk with no registry entry is a finding" 1 \
    python3 "${ROOT}/scripts/apply-lane-faults.py" --root "${lanes_root}/orphan" --verify

  mkdir -p "${lanes_root}/empty/tools/gate"
  printf '[lanes]\n' > "${lanes_root}/empty/tools/gate/lanes.toml"
  expect_exit "a registry of nothing pins nothing, and cannot run" 2 \
    python3 "${ROOT}/scripts/apply-lane-faults.py" --root "${lanes_root}/empty" --verify

  mkdir -p "${lanes_root}/broken/tools/gate" "${lanes_root}/broken/diet/alpha"
  cp "${lanes_root}/present/tools/gate/lanes.toml" "${lanes_root}/broken/tools/gate/lanes.toml"
  cat > "${lanes_root}/broken/diet/alpha/gate.toml" <<'EOF'
[package]
name = "synthetic-alpha"
check = "test"
command = "false"
faults = 0
EOF
  expect_exit "a registered lane whose own command fails is a finding" 1 \
    python3 "${ROOT}/scripts/apply-lane-faults.py" --root "${lanes_root}/broken" --verify

  # `shell=True` means "the shell could not find the program" is an ordinary
  # nonzero exit FROM THE SHELL (127), not a Python-level failure to launch
  # anything at all -- so this is still the DIRTY path, exit 1, and the case
  # says so rather than assuming a spelling mistake in a manifest is somehow
  # a different class of failure from a test that fails.
  mkdir -p "${lanes_root}/wrecked/tools/gate" "${lanes_root}/wrecked/diet/alpha"
  cp "${lanes_root}/present/tools/gate/lanes.toml" "${lanes_root}/wrecked/tools/gate/lanes.toml"
  cat > "${lanes_root}/wrecked/diet/alpha/gate.toml" <<'EOF'
[package]
name = "synthetic-alpha"
check = "test"
command = "exit-not-a-real-binary-xyz"
faults = 0
EOF
  expect_exit "a command the shell cannot find is still a finding, not broken" 1 \
    python3 "${ROOT}/scripts/apply-lane-faults.py" --root "${lanes_root}/wrecked" --verify

  # --- nothing is read from a half-merged file ---
  #
  # This gate has two inputs and both of them conflict routinely: `faults.toml`
  # conflicts on essentially every merge in a stack, which is what
  # `merge-gate.py --union` exists for, and `verify.sh` conflicts whenever two
  # branches both seed a case. What a checker reads out of a half-merged file
  # is the union of both sides, or neither side, depending on where the markers
  # fell -- and either way it looks like an answer. So it refuses, exit 2, "I
  # was asked something I cannot answer".
  #
  # Neither refusal had ever been seen fire. The manifest side did not exist:
  # a conflicted `faults.toml` reached `tomllib`, came back "not TOML", exit 1,
  # a finding about the manifest when the truth was that the caller is
  # mid-merge.
  #
  # Run against COPIES of this repository rather than against it. The check
  # locates its inputs from its own path, so the only way to hand it a
  # conflicted file is to hand it a different root; and the scripts are copied
  # rather than symlinked because that path is resolved before it is used.
  local halfmerged side; scratch; halfmerged="$SCRATCH"
  for side in verify manifest; do
    mkdir -p "${halfmerged}/${side}/scripts" "${halfmerged}/${side}/tools/gate"
    # `apply-lane-faults.py` and its registry, alongside the two scripts
    # already copied: `observed()` now shells out to `--list` to count lane
    # faults into the red total, so a copy missing either one made `--count-
    # red` exit 2 -- BROKEN, not the tolerated mid-merge answer -- over a
    # dependency this fixture predates. The registry's own five lanes are not
    # copied, so `--list` reports each as a missing manifest and answers zero
    # lane faults; that undercounts the total this synthetic copy prints, but
    # the case below asks only whether the count is ANSWERED, not what it is.
    cp "${ROOT}/scripts/check-fault-manifest.py" "${ROOT}/scripts/gatelib.py" \
      "${ROOT}/scripts/apply-lane-faults.py" "${halfmerged}/${side}/scripts/"
    cp "${ROOT}/verify.sh" "${halfmerged}/${side}/verify.sh"
    cp "${ROOT}/tools/gate/faults.toml" "${halfmerged}/${side}/tools/gate/faults.toml"
    cp "${ROOT}/tools/gate/lanes.toml" "${halfmerged}/${side}/tools/gate/lanes.toml"
  done
  # One marker apiece, of the kind git writes, on the file whose turn it is.
  # `=======` alone is deliberately not enough to trip this -- it is a
  # plausible separator in ordinary prose -- so each case carries an arrow.
  printf '<%s HEAD\n' '<<<<<<' | cat - "${ROOT}/verify.sh" \
    > "${halfmerged}/verify/verify.sh.half"
  mv "${halfmerged}/verify/verify.sh.half" "${halfmerged}/verify/verify.sh"
  printf '>%s theirs\n' '>>>>>>' >> "${halfmerged}/manifest/tools/gate/faults.toml"

  # The manifest side needs its `verify.sh` to parse cleanly, because the
  # refusal it is about fires after `observed()` has read one.
  expect_exit "a half-merged verify.sh is refused rather than counted" 2 \
    python3 "${halfmerged}/verify/scripts/check-fault-manifest.py"
  expect_exit "a half-merged manifest is a refusal and not a finding" 2 \
    python3 "${halfmerged}/manifest/scripts/check-fault-manifest.py"
  # And the same manifest asked for a COUNT still answers, because that is the
  # one caller holding a conflicted manifest on purpose: `merge-gate.py` is
  # mid-merge and about to rewrite it with the number it is asking for. A
  # refusal hoisted up to the top of the script would break exactly it.
  expect_exit "a count is still answered over a manifest being merged" 0 \
    python3 "${halfmerged}/manifest/scripts/check-fault-manifest.py" --count-red

  # --- the scope's own control ---
  #
  # `cargo test` with a filter matching no test prints `running 0 tests` and
  # EXITS 0. Every scoped case in the list above therefore rests on this: a
  # scope that has gone stale -- a module renamed under it, a typo -- must be
  # a failure and not a fast green. Through SELFTEST_TARGET so it costs an
  # incremental build rather than a cold one.
  expect_exit "a test scope that selects nothing is not a pass" 1 \
    env CARGO_TARGET_DIR="${SELFTEST_TARGET}" \
    bash "${ROOT}/verify.sh" --only test --scope lib/a_test_this_repository_does_not_have
  expect_exit "a scope spelling nobody defined is a misuse, not everything" 2 \
    env CARGO_TARGET_DIR="${SELFTEST_TARGET}" \
    bash "${ROOT}/verify.sh" --only test --scope 'whatever'
  expect_exit "a scope without exactly --only test is a misuse" 2 \
    bash "${ROOT}/verify.sh" --scope lib
  expect_exit "a shard outside 1..N is a misuse" 2 \
    bash "${ROOT}/verify.sh" --selftest --shard 9/8

  prove_patterns "hygiene" scripts/hygiene-patterns.tsv scripts/seed-hygiene-fault.sh \
    "${REQUIRED_HYGIENE_CLASSES[@]}"
  prove_patterns "pages" scripts/pages-patterns.tsv scripts/seed-pages-fault.sh \
    "${REQUIRED_PAGES_CLASSES[@]}"

  # A check with no seeded fault has never been seen red, which is the one
  # thing this mode exists to rule out.
  echo
  echo "--- every check has a seeded fault ---"
  local check missing=()
  for check in "${CHECKS[@]}"; do
    case " ${SEEDED_CHECKS[*]} " in
      *" ${check} "*) printf 'seeded  %s\n' "$check" ;;
      *) printf 'UNSEEDED %s  <-- NEVER SEEN RED\n' "$check"; missing+=("$check") ;;
    esac
  done
  [ "${#missing[@]}" -eq 0 ] || SELFTEST_BROKEN+=("checks with no seeded fault: ${missing[*]}")

  # --- the cost harvest's own total ---
  #
  # The whole run, so that the packer can subtract the faults from it and be
  # left with what a shard pays WHATEVER faults it draws: the sandbox target
  # directory, prove_mechanics -- which runs unsharded, in every shard -- and
  # the fixture loops that are not faults. That residue is the per-shard
  # overhead, and it is measured here rather than guessed at, because guessing
  # it wrong is what decides the shard count wrong.
  if [ -n "$SELFTEST_COST_INDEX" ]; then
    now_ms
    printf 'run\twhole\t%d\tthe unsharded selftest, end to end\n' \
      "$(( NOW_MS - selftest_started_ms ))" >> "$SELFTEST_COST_INDEX"

    # THE PROVENANCE IS PART OF THE HARVEST, not something asserted about it
    # afterward: a plan is a claim about the runner's own timing, and the claim
    # travels with the numbers or not at all. `RUNNER_ENVIRONMENT` is GitHub
    # Actions' own variable for this and is unset outside Actions, so a local
    # harvest honestly records `local` rather than guessing at a substrate
    # nobody declared.
    printf 'substrate\t%s\t%s\n' "${RUNNER_ENVIRONMENT:-local}" "$(uname -srm 2>/dev/null || echo unknown)" >> "$SELFTEST_COST_INDEX"
  fi

  # --- the census ---
  #
  # What this run RAN, by ordinal, so that N shards can be added up afterwards
  # and the sum compared against the manifest. A shard that passes having run
  # a subset of what it was assigned is the one way sharding could quietly
  # become fault selection, and it is the reason this is emitted as data
  # rather than asserted here: a shard cannot certify itself.
  echo
  printf 'selftest-census: shard %d of %d ran %d of %d fault(s)\n' \
    "$(( SELFTEST_SHARD == 0 ? 1 : SELFTEST_SHARD ))" \
    "$SELFTEST_SHARDS" "${#SELFTEST_RAN[@]}" "$SELFTEST_UNITS"
  if [ "$SELFTEST_UNPLANNED" -gt 0 ]; then
    printf 'selftest: %d fault(s) unplanned in %s; balance approximate -- each is assigned a shard by a hash of its id\n' \
      "$SELFTEST_UNPLANNED" "$SHARD_PLAN"
  fi
  if [ -n "$SELFTEST_CENSUS" ]; then
    {
      printf 'shard\t%d\n' "$(( SELFTEST_SHARD == 0 ? 1 : SELFTEST_SHARD ))"
      printf 'shards\t%d\n' "$SELFTEST_SHARDS"
      printf 'total\t%d\n' "$SELFTEST_UNITS"
      # MEASURED, so the aggregate can report the slowest shard against the
      # budget from what the runner took rather than from what a plan
      # predicted (ruled on #108, 2026-09-24). Reported, never graded: the
      # budget is a printed number, not a gate (#112).
      printf 'elapsed\t%d\n' "$SECONDS"
      printf 'ordinal\t%s\n' ${SELFTEST_RAN+"${SELFTEST_RAN[@]}"}
    } > "$SELFTEST_CENSUS" || {
      echo "selftest: the census could not be written to ${SELFTEST_CENSUS}" >&2
      SELFTEST_BROKEN+=("the census could not be written")
    }
  fi

  echo
  if [ "${#SELFTEST_BROKEN[@]}" -gt 0 ]; then
    printf 'selftest: %d gate(s) failed to fire, or fired for the wrong reason:\n' \
      "${#SELFTEST_BROKEN[@]}"
    printf '  - %s\n' "${SELFTEST_BROKEN[@]}"
    return "$EXIT_FAIL"
  fi
  # A CHECK OF NOTHING IS NOT A PASS, and this line is the one place the
  # selftest says otherwise. `--shard 275/1000` selects no case at all: the
  # `1 <= K <= N` bound admits it, every loop below runs zero times, nothing
  # lands in SELFTEST_BROKEN, and the run prints "every gate was seen red on
  # its own seeded fault" and exits 0. Every word of that sentence is false
  # about a run that saw no gate.
  #
  # Found by a fresh instance, reproduced live. Not reachable through CI --
  # `grade_shape` refuses an empty shard in the checked-in plan and LPT packing
  # cannot produce one, so the matrix CI derives never contains a K this empty,
  # and the census refuses an incomplete union whatever any single shard
  # claims -- so this is a foot-gun for a person running --shard by hand, and a
  # comment that overclaimed what the bound guards against. Both are the same
  # defect: the thing that made it safe was somewhere else, and nothing said
  # so here.
  #
  # unseedable: a re-entrant --selftest call would recurse into the function containing it; proved by hand instead
  #
  # This repository's own law is that a gate nothing has seen red is a gate
  # that does not exist, so the tag above is not decoration: #77's lint
  # refuses a guard with neither a manifest fault nor this line. Both are run
  # from inside `selftest`: a seeded case runs one `verify.sh --only <check>`, and
  # `prove_mechanics` runs unconditionally in every shard. An assertion that
  # invoked `verify.sh --selftest` to watch this line refuse would re-enter
  # the function containing it, and the inner run would do the same. Covering
  # it needs a re-entry flag, which is a change to how the selftest is
  # invoked and not a fixture.
  #
  # It was proved by hand in both directions, and the commit that added it
  # records the transcript: `--shard 275/1000` exits 2 naming the empty
  # shard, `--shard 1/8` runs 35 of 274 and still declares the pass. That is
  # weaker than a fixture and it is what there is.
  if [ "${#SELFTEST_RAN[@]}" -eq 0 ]; then
    echo "selftest: this run selected no seeded case, so it proves nothing" >&2
    if [ "$SELFTEST_SHARD" -ne 0 ]; then
      echo "selftest: shard ${SELFTEST_SHARD} of ${SELFTEST_SHARDS} is empty; \
there are ${SELFTEST_UNITS} case(s) to divide" >&2
    fi
    return "$EXIT_MISUSE"
  fi
  echo "selftest: every gate was seen red on its own seeded fault."
}

# --------------------------------------------------------------------------
# main
# --------------------------------------------------------------------------

selected=()
mode="all"
shard_arg=""

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
    --selftest) mode="selftest"; shift ;;
    --derive-scopes)
      [ "$#" -ge 2 ] || { echo "verify: --derive-scopes needs a directory" >&2; exit "$EXIT_MISUSE"; }
      SELFTEST_DERIVE="$2"
      shift 2
      ;;
    --derive-shards)
      [ "$#" -ge 2 ] || { echo "verify: --derive-shards needs a directory" >&2; exit "$EXIT_MISUSE"; }
      # Resolved against the caller's directory for the same reason --census is:
      # parsing happens before the `cd "$ROOT"` below.
      case "$2" in
        /*) SELFTEST_COST_DIR="$2" ;;
        *)  SELFTEST_COST_DIR="$(pwd)/$2" ;;
      esac
      shift 2
      ;;
    --scope)
      [ "$#" -ge 2 ] || { echo "verify: --scope needs a spec" >&2; exit "$EXIT_MISUSE"; }
      scope_args "$2" || {
        echo "verify: --scope '$2': want lib, bins, all or test:NAME, each" \
             "optionally followed by /FILTER" >&2
        exit "$EXIT_MISUSE"
      }
      VERIFY_TEST_SCOPE="$2"
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
    --shard)
      [ "$#" -ge 2 ] || { echo "verify: --shard needs K/N" >&2; exit "$EXIT_MISUSE"; }
      shard_arg="$2"
      case "$shard_arg" in
        *[!0-9/]*|*/*/*|/*|*/) shard_arg="" ;;
        */*) ;;
        *) shard_arg="" ;;
      esac
      [ -n "$shard_arg" ] || {
        echo "verify: --shard wants K/N in decimal, not '$2'" >&2
        exit "$EXIT_MISUSE"
      }
      SELFTEST_SHARD="${shard_arg%%/*}"
      SELFTEST_SHARDS="${shard_arg##*/}"
      # 1 <= K <= N, and N >= 1. A K of 0 would run nothing while reporting a
      # shard, and a K above N would run nothing while the sum still looked
      # like N shards -- both are a job that passes having done nothing, which
      # is the failure this whole issue is about not introducing.
      if [ "$SELFTEST_SHARDS" -lt 1 ] || [ "$SELFTEST_SHARD" -lt 1 ] ||
         [ "$SELFTEST_SHARD" -gt "$SELFTEST_SHARDS" ]; then
        echo "verify: --shard ${2}: needs 1 <= K <= N and N >= 1" >&2
        exit "$EXIT_MISUSE"
      fi
      shift 2
      ;;
    --census)
      [ "$#" -ge 2 ] || { echo "verify: --census needs a path" >&2; exit "$EXIT_MISUSE"; }
      # Resolved here, against the directory the caller is in. Parsing
      # happens before the `cd "$ROOT"` below, so a relative path left alone
      # would land somewhere the caller never named.
      case "$2" in
        /*) SELFTEST_CENSUS="$2" ;;
        *)  SELFTEST_CENSUS="$(pwd)/$2" ;;
      esac
      shift 2
      ;;
    --list) printf '%s\n' "${CHECKS[@]}"; exit 0 ;;
    -h|--help) usage; exit 0 ;;
    *) echo "verify: unknown argument '$1'" >&2; usage >&2; exit "$EXIT_MISUSE" ;;
  esac
done

cd "$ROOT"

# `--shard` and `--census` describe a selftest run. Silently ignoring them on
# an ordinary run would let a workflow think it had sharded a gate that in
# fact ran whole, or ran nothing.
if [ "$mode" != "selftest" ] &&
   { [ "$SELFTEST_SHARD" -ne 0 ] || [ -n "$SELFTEST_CENSUS" ]; }; then
  echo "verify: --shard and --census are for --selftest" >&2
  exit "$EXIT_MISUSE"
fi

# A scope narrows ONE check, so it may only be given when that check is the
# only one asked for. `verify.sh --scope lib` on its own would otherwise read
# as "run everything" while running a fraction of the tests, and read that way
# in a workflow, where nobody would see it.
if [ -n "$VERIFY_TEST_SCOPE" ] &&
   { [ "$mode" = "selftest" ] || [ "${#selected[@]}" -ne 1 ] ||
     [ "${selected[0]-}" != "test" ]; }; then
  echo "verify: --scope narrows the test check, so it needs exactly --only test" >&2
  exit "$EXIT_MISUSE"
fi

# A range narrows ONE check, the same way a scope does, and for a sharper
# reason: `history` INFERS its range when none is given, so a `--range` left
# loose would read as "everything was scanned" over a run in which fifteen
# checks ignored it and the sixteenth scanned a slice somebody else chose.
if [ -n "$VERIFY_HISTORY_RANGE" ] &&
   { [ "$mode" = "selftest" ] || [ "${#selected[@]}" -ne 1 ] ||
     [ "${selected[0]-}" != "history" ]; }; then
  echo "verify: --range narrows the history check, so it needs exactly --only history" >&2
  exit "$EXIT_MISUSE"
fi

# `--derive-scopes` re-runs the selftest's test cases unscoped and is nothing
# on its own: without --selftest there is no run to derive from, and
# SELFTEST_LOGS is unset, so the index would be written to the filesystem root.
if [ -n "$SELFTEST_DERIVE" ] && [ "$mode" != "selftest" ]; then
  echo "verify: --derive-scopes derives from a selftest run, so it needs --selftest" >&2
  exit "$EXIT_MISUSE"
fi

# `--derive-shards` measures what each fault costs, and three things would make
# that measurement about something other than the run CI performs.
if [ -n "$SELFTEST_COST_DIR" ]; then
  if [ "$mode" != "selftest" ]; then
    echo "verify: --derive-shards derives from a selftest run, so it needs --selftest" >&2
    exit "$EXIT_MISUSE"
  fi
  # A sharded harvest measures one shard's faults and nothing else, so the
  # packer would be balancing a list it has costs for a fraction of -- and it
  # would be balancing them against the split they came from.
  if [ "$SELFTEST_SHARD" -ne 0 ]; then
    echo "verify: --derive-shards harvests the whole fault list, so it does not \
take --shard; the split is what it exists to derive" >&2
    exit "$EXIT_MISUSE"
  fi
  # `--derive-scopes` runs every `test` case UNSCOPED on purpose. That is the
  # opposite of what a cost harvest needs: CI runs those cases scoped, and a
  # cost measured with the declaration switched off is several times the cost
  # the shard will actually pay.
  if [ -n "$SELFTEST_DERIVE" ]; then
    echo "verify: --derive-shards and --derive-scopes want opposite runs -- one \
needs every scope on, the other needs them all off -- so one run cannot be both" >&2
    exit "$EXIT_MISUSE"
  fi
  mkdir -p "$SELFTEST_COST_DIR" || {
    echo "verify: --derive-shards ${SELFTEST_COST_DIR} could not be made" >&2
    exit "$EXIT_MISUSE"
  }
  SELFTEST_COST_INDEX="${SELFTEST_COST_DIR}/derive-shards.tsv"
fi

if [ "$mode" = "selftest" ]; then
  rc=0
  selftest || rc=$?
  if [ -n "$SELFTEST_DERIVE" ]; then
    printf '\nderive-scopes: the index is %s\n' "${SELFTEST_LOGS}/derive-scopes.tsv"
    printf 'derive-scopes: read it with\n'
    printf '  python3 scripts/derive-scopes.py --index %s\n' \
      "${SELFTEST_LOGS}/derive-scopes.tsv"
  fi
  if [ -n "$SELFTEST_COST_INDEX" ]; then
    printf '\nderive-shards: the index is %s\n' "$SELFTEST_COST_INDEX"
    printf 'derive-shards: read it with\n'
    printf '  python3 scripts/derive-shards.py --index %s\n' "$SELFTEST_COST_INDEX"
    printf '  python3 scripts/derive-shards.py --index %s --emit > %s\n' \
      "$SELFTEST_COST_INDEX" "$SHARD_PLAN"
  fi
  report_wall_clock
  exit "$rc"
fi

if [ "${#selected[@]}" -eq 0 ]; then
  selected=("${CHECKS[@]}")
fi

for name in "${selected[@]}"; do
  run_check "$name"
done

echo
if [ "${#FAILED[@]}" -gt 0 ]; then
  printf 'verify: %d of %d check(s) failed:\n' "${#FAILED[@]}" "${#selected[@]}"
  printf '  - %s\n' "${FAILED[@]}"
  report_wall_clock
  exit "$EXIT_FAIL"
fi
printf 'verify: %d check(s) passed.\n' "${#selected[@]}"
report_wall_clock
